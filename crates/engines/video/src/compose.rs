//! Turning the edit into pictures: the frame at any timeline position, composited from
//! every visible track exactly as an export writes it. The program monitor shows this
//! function's output and nothing else, so what is previewed is what is exported.
//!
//! Per clip: the source frame is cropped, colour-corrected with the raster engine's
//! adjustments, fitted to the canvas, then scaled, rotated and positioned (bilinear, in
//! 16.16 fixed point), and its opacity and fades applied. Tracks composite bottom-up
//! with source-over. A transition mixes its two clips' layers before they composite.
use crate::blob::Blob;
use crate::project::{
    Clip, Library, Project, Source, Title, TitlePosition, TrackKind, TransitionKind,
};
use cw_raster::{adjust, fmath, transform, Adjustment, Canvas, Rgba};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Glyph coverage for one line of title text, rasterised by the renderer in the
/// platform's font. Titles are drawn from these so a title is exactly the renderer's text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextMask {
    pub width: u32,
    pub height: u32,
    pub alpha: Blob,
}
pub type Masks = BTreeMap<String, TextMask>;

pub struct Compositor<'a> {
    pub project: &'a Project,
    pub library: &'a Library,
    pub masks: &'a Masks,
}

fn fade(local: i64, length: i64, fade_in: u32, fade_out: u32) -> i64 {
    let mut f = 1000;
    if fade_in > 0 && local < i64::from(fade_in) {
        f = f.min(local.max(0) * 1000 / i64::from(fade_in));
    }
    let left = length - 1 - local;
    if fade_out > 0 && left < i64::from(fade_out) {
        f = f.min(left.max(0) * 1000 / i64::from(fade_out));
    }
    f
}

/// Straight RGBA mix of two layers, `t` (0..=255) of the way to `b`, in premultiplied
/// colour so a transparent pixel contributes no colour.
fn mix(a: &Canvas, b: &Canvas, t: u32) -> Canvas {
    let mut out = a.clone();
    let (pa, pb) = (a.pixels(), b.pixels());
    let po = out.pixels_mut();
    let (s, u) = (i64::from(255 - t), i64::from(t));
    for i in (0..pa.len()).step_by(4) {
        let (aa, ab) = (i64::from(pa[i + 3]), i64::from(pb[i + 3]));
        let alpha = aa * s + ab * u; // 0..=255*255
        if alpha == 0 {
            po[i..i + 4].copy_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        for c in 0..3 {
            let num = i64::from(pa[i + c]) * aa * s + i64::from(pb[i + c]) * ab * u;
            po[i + c] = ((num + alpha / 2) / alpha).clamp(0, 255) as u8;
        }
        po[i + 3] = ((alpha + 127) / 255) as u8;
    }
    out
}

/// Multiply every alpha by `permille`/1000.
fn fade_alpha(c: &mut Canvas, permille: i64) {
    if permille >= 1000 {
        return;
    }
    let p = permille.max(0);
    for px in c.pixels_mut().chunks_mut(4) {
        px[3] = ((i64::from(px[3]) * p + 500) / 1000) as u8;
    }
}

fn colorize(hex: &str) -> Rgba {
    cw_raster::parse_hex(hex).unwrap_or(cw_raster::WHITE)
}

impl Compositor<'_> {
    /// The picture at timeline `frame`.
    pub fn frame(&self, frame: i64) -> Canvas {
        let p = self.project;
        let mut out = Canvas::filled(p.width, p.height, colorize(&p.background));
        for track in p.tracks_of(TrackKind::Video) {
            if track.hidden {
                continue;
            }
            if let Some(layer) = self.track_layer(track.id, frame) {
                out.draw(0, 0, &layer, cw_raster::BlendMode::Normal, 255);
            }
        }
        out
    }

    /// One track's contribution at `frame`, transitions included.
    pub fn track_layer(&self, track: u32, frame: i64) -> Option<Canvas> {
        let p = self.project;
        for t in &p.transitions {
            let (Some(a), Some(b)) = (p.clip(t.left), p.clip(t.right)) else {
                continue;
            };
            if a.track != track {
                continue;
            }
            let Some((from, to)) = p.transition_span(t) else {
                continue;
            };
            if frame < from || frame >= to {
                continue;
            }
            let d = i64::from(t.frames);
            // Progress through the transition, 0..=255, sampled at the frame's middle.
            let k = ((2 * (frame - from) + 1) * 255 + d) / (2 * d);
            let k = k.clamp(0, 255) as u32;
            let la = self.clip_layer(a, frame);
            let lb = self.clip_layer(b, frame);
            return Some(match t.kind {
                TransitionKind::CrossDissolve => mix(&la, &lb, k),
                TransitionKind::DipToBlack | TransitionKind::DipToWhite => {
                    let v = if t.kind == TransitionKind::DipToBlack {
                        0
                    } else {
                        255
                    };
                    let solid = Canvas::filled(p.width, p.height, [v, v, v, 255]);
                    if k < 128 {
                        mix(&la, &solid, (k * 2).min(255))
                    } else {
                        mix(&solid, &lb, ((k - 128) * 2 + 1).min(255))
                    }
                }
                TransitionKind::Wipe => {
                    // The incoming clip is revealed from the left edge.
                    let edge = (i64::from(p.width) * i64::from(k) / 255) as i32;
                    let mut out = la;
                    if let Some(part) =
                        lb.region(cw_raster::IRect::new(0, 0, edge as u32, p.height))
                    {
                        out.put(0, 0, &part);
                    }
                    out
                }
                TransitionKind::Slide => {
                    // The incoming clip slides in from the right over the outgoing one.
                    let x = (i64::from(p.width) * i64::from(255 - k) / 255) as i32;
                    let mut out = la;
                    out.draw(x, 0, &lb, cw_raster::BlendMode::Normal, 255);
                    out
                }
            });
        }
        let clip = p
            .clips
            .iter()
            .find(|c| c.track == track && c.contains(frame))?;
        Some(self.clip_layer(clip, frame))
    }

    /// The source picture a clip shows at clip-local `local`, before any effect.
    fn source(&self, clip: &Clip, local: i64) -> Option<Canvas> {
        let p = self.project;
        match &clip.source {
            Source::Media { media } => {
                let m = self.library.get(media)?;
                let mut at = clip.source_at(local);
                if let Some(us) = m.length_us() {
                    // Past either end of the media the nearest frame holds.
                    at = at.clamp(0, (p.centiframes(us) - 1).max(0));
                }
                m.frame(p.micros(at.max(0)))
            }
            Source::Color { color } => Some(Canvas::filled(p.width, p.height, colorize(color))),
            Source::Title(title) => Some(self.title(title)),
        }
    }

    /// A title on a transparent canvas the size of the frame.
    pub fn title(&self, title: &Title) -> Canvas {
        let p = self.project;
        let (w, h) = (p.width as i32, p.height as i32);
        let mut out = Canvas::new(p.width, p.height);
        let Some(mask) = self.masks.get(&title.raster_key()) else {
            return out;
        };
        let (mw, mh) = (mask.width as i32, mask.height as i32);
        let x = (w - mw) / 2;
        let y = match title.position {
            TitlePosition::Top => h / 10,
            TitlePosition::Center => (h - mh) / 2,
            TitlePosition::Lower => h * 2 / 3,
            TitlePosition::Bottom => h - mh - h / 12,
        };
        if let Some(bg) = &title.background {
            let pad = i32::from(title.size) / 3 + 2;
            let b = Canvas::filled((mw + 2 * pad) as u32, (mh + pad) as u32, colorize(bg));
            out.draw(x - pad, y - pad / 2, &b, cw_raster::BlendMode::Normal, 255);
        }
        let color = colorize(&title.color);
        let mut glyphs = Canvas::new(mask.width, mask.height);
        for (i, a) in mask.alpha.bytes().iter().enumerate() {
            if *a > 0 {
                let alpha = fmath::mul255(*a, color[3]);
                glyphs.set(
                    (i % mask.width as usize) as i32,
                    (i / mask.width as usize) as i32,
                    [color[0], color[1], color[2], alpha],
                );
            }
        }
        out.draw(x, y, &glyphs, cw_raster::BlendMode::Normal, 255);
        out
    }

    /// One clip's layer at timeline `frame`: a transparent canvas the size of the frame.
    pub fn clip_layer(&self, clip: &Clip, frame: i64) -> Canvas {
        let p = self.project;
        let local = frame - clip.start;
        let blank = || Canvas::new(p.width, p.height);
        let Some(mut src) = self.source(clip, local) else {
            return blank();
        };
        if !clip.crop.is_empty() {
            let (sw, sh) = (i64::from(src.width()), i64::from(src.height()));
            let c = clip.crop;
            let (l, t) = (sw * i64::from(c.left) / 1000, sh * i64::from(c.top) / 1000);
            let (r, b) = (
                sw - sw * i64::from(c.right) / 1000,
                sh - sh * i64::from(c.bottom) / 1000,
            );
            for y in 0..sh {
                for x in 0..sw {
                    if x < l || x >= r || y < t || y >= b {
                        src.set(x as i32, y as i32, cw_raster::TRANSPARENT);
                    }
                }
            }
        }
        let g = clip.grade;
        if !g.is_neutral() {
            if g.brightness != 0 || g.contrast != 0 {
                adjust::apply(
                    &mut src,
                    &Adjustment::BrightnessContrast {
                        brightness: g.brightness,
                        contrast: g.contrast,
                    },
                    None,
                );
            }
            if g.saturation != 0 {
                adjust::apply(
                    &mut src,
                    &Adjustment::HueSaturation {
                        hue: 0,
                        saturation: g.saturation,
                        lightness: 0,
                    },
                    None,
                );
            }
            if g.temperature != 0 {
                adjust::apply(
                    &mut src,
                    &Adjustment::Temperature {
                        temperature: g.temperature,
                        tint: 0,
                    },
                    None,
                );
            }
        }
        let mut layer = self.place(&src, clip, local);
        let opacity = i64::from(clip.opacity.at(local).clamp(0, 1000))
            * fade(local, clip.length, clip.fade_in, clip.fade_out)
            / 1000;
        fade_alpha(&mut layer, opacity);
        layer
    }

    /// Fit `src` to the frame, then scale, rotate and move it by the clip's transform.
    fn place(&self, src: &Canvas, clip: &Clip, local: i64) -> Canvas {
        let p = self.project;
        let (w, h) = (i64::from(p.width), i64::from(p.height));
        let (sw, sh) = (i64::from(src.width()), i64::from(src.height()));
        // Fit factor num/den: the largest scale showing the whole picture.
        let (num, den) = if w * sh <= h * sw { (w, sw) } else { (h, sh) };
        let scale = i64::from(clip.scale.at(local).clamp(10, 10_000));
        let rotation = clip.rotation.at(local);
        let (dx, dy) = (i64::from(clip.x.at(local)), i64::from(clip.y.at(local)));
        const FIX: i64 = 1 << 16;
        if rotation.rem_euclid(36_000) == 0 && num * scale == den * 1000 {
            // Unscaled and upright: an exact copy, pixel for pixel.
            let mut out = Canvas::new(p.width, p.height);
            out.put(((w - sw) / 2 + dx) as i32, ((h - sh) / 2 + dy) as i32, src);
            return out;
        }
        // Output-to-source: divide by the scale, rotate back.
        let inv = |v: f64| fmath::round(v * FIX as f64) as i64;
        let rad = f64::from(rotation) / 100.0 * fmath::PI / 180.0;
        let (c, s) = (fmath::cos(rad), fmath::sin(rad));
        let k = (den * 1000) as f64 / (num * scale) as f64;
        let m = [inv(c * k), inv(s * k), inv(-s * k), inv(c * k)];
        let anchor = (w * FIX / 2 + dx * FIX, h * FIX / 2 + dy * FIX);
        transform::affine(src, p.width, p.height, m, anchor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::Media;
    use crate::media::MediaKind;
    use crate::project::{Ease, TransitionKind};

    const RED: Rgba = [200, 0, 0, 255];
    const BLUE: Rgba = [0, 0, 200, 255];

    fn setup() -> (Project, Library, u32, u32) {
        let mut p = Project::new("T", 16, 8, 10, 8000);
        let mut lib = Library::new();
        for (name, color) in [("red.apng", RED), ("blue.apng", BLUE)] {
            let frames: Vec<Canvas> = (0..20).map(|_| Canvas::filled(16, 8, color)).collect();
            let m = Media::import(name, &crate::apng::encode(&frames, 10).unwrap(), 8000).unwrap();
            let id = p.add_media(name, MediaKind::Video);
            lib.insert(id, m);
        }
        let (r, b) = (p.media[0].id, p.media[1].id);
        (p, lib, r, b)
    }

    #[test]
    fn a_full_frame_clip_is_shown_pixel_for_pixel() {
        let (mut p, lib, r, _) = setup();
        let v = p.tracks[0].id;
        let frames: Vec<Canvas> = (0..10)
            .map(|i| {
                let mut c = Canvas::filled(16, 8, [0, 0, 0, 255]);
                c.set(i, 2, [255, 255, 255, 255]);
                c
            })
            .collect();
        let mut lib = lib;
        let id = p.add_media("dots.apng", MediaKind::Video);
        lib.insert(
            id,
            Media::import(
                "dots.apng",
                &crate::apng::encode(&frames, 10).unwrap(),
                8000,
            )
            .unwrap(),
        );
        let c = p.insert(&lib, Source::Media { media: id }, v, 5).unwrap();
        let masks = Masks::new();
        let comp = Compositor {
            project: &p,
            library: &lib,
            masks: &masks,
        };
        // Before the clip: background.
        assert_eq!(comp.frame(0), Canvas::filled(16, 8, [0, 0, 0, 255]));
        for i in 0..10 {
            assert_eq!(comp.frame(5 + i), frames[i as usize], "frame {i}");
        }
        // Reversed at half speed: each source frame twice, last first.
        p.set_reverse(c, true).unwrap();
        p.set_speed(c, 50, &lib).unwrap();
        let comp = Compositor {
            project: &p,
            library: &lib,
            masks: &masks,
        };
        assert_eq!(p.clip(c).unwrap().length, 20);
        assert_eq!(comp.frame(5), frames[9]);
        assert_eq!(comp.frame(6), frames[9]);
        assert_eq!(comp.frame(7), frames[8]);
        assert_eq!(comp.frame(24), frames[0]);
        let _ = r;
    }

    #[test]
    fn transitions_mix_exactly() {
        let (mut p, lib, r, b) = setup();
        let v = p.tracks[0].id;
        let a = p.insert(&lib, Source::Media { media: r }, v, 0).unwrap();
        p.trim(a, crate::project::Edge::Out, 10, &lib).unwrap();
        let bb = p.insert(&lib, Source::Media { media: b }, v, 10).unwrap();
        assert_eq!(p.clip(bb).unwrap().start, 10);
        let masks = Masks::new();
        let px = |p: &Project, f: i64| {
            Compositor {
                project: p,
                library: &lib,
                masks: &masks,
            }
            .frame(f)
            .get(3, 3)
        };
        p.add_transition(a, TransitionKind::CrossDissolve, 4)
            .unwrap();
        // Span [8, 12): progress 32, 96, 159, 223 of 255.
        assert_eq!(px(&p, 7), RED);
        assert_eq!(px(&p, 8), [175, 0, 25, 255]);
        assert_eq!(px(&p, 9), [125, 0, 75, 255]);
        assert_eq!(px(&p, 10), [75, 0, 125, 255]);
        assert_eq!(px(&p, 11), [25, 0, 175, 255]);
        assert_eq!(px(&p, 12), BLUE);
        p.add_transition(a, TransitionKind::DipToBlack, 4).unwrap();
        assert_eq!(px(&p, 8), [150, 0, 0, 255]);
        assert_eq!(px(&p, 9), [49, 0, 0, 255]);
        assert_eq!(px(&p, 10), [0, 0, 49, 255]);
        assert_eq!(px(&p, 11), [0, 0, 150, 255]);
        p.add_transition(a, TransitionKind::DipToWhite, 4).unwrap();
        assert_eq!(px(&p, 9), [241, 192, 192, 255]);
        p.add_transition(a, TransitionKind::Wipe, 4).unwrap();
        let wipe = Compositor {
            project: &p,
            library: &lib,
            masks: &masks,
        }
        .frame(9);
        // 96/255 of 16 columns = 6 columns of blue from the left.
        assert_eq!(wipe.get(5, 0), BLUE);
        assert_eq!(wipe.get(6, 0), RED);
        p.add_transition(a, TransitionKind::Slide, 4).unwrap();
        let slide = Compositor {
            project: &p,
            library: &lib,
            masks: &masks,
        }
        .frame(9);
        // Blue has slid in to x = 16 * 159 / 255 = 9.
        assert_eq!(slide.get(8, 0), RED);
        assert_eq!(slide.get(9, 0), BLUE);
    }

    #[test]
    fn opacity_keyframes_fades_and_hidden_tracks() {
        let (mut p, lib, r, b) = setup();
        let v1 = p.tracks[0].id;
        let v2 = p.add_track(TrackKind::Video).unwrap();
        p.insert(&lib, Source::Media { media: b }, v1, 0).unwrap();
        let top = p.insert(&lib, Source::Media { media: r }, v2, 0).unwrap();
        {
            let c = p.clip_mut(top).unwrap();
            c.opacity.set_key(0, 0, Ease::Linear).unwrap();
            c.opacity.set_key(10, 1000, Ease::Linear).unwrap();
            c.fade_out = 4;
        }
        let masks = Masks::new();
        let comp = |p: &Project, f: i64| {
            Compositor {
                project: p,
                library: &lib,
                masks: &masks,
            }
            .frame(f)
            .get(0, 0)
        };
        assert_eq!(comp(&p, 0), BLUE);
        // Half-way: 50% red over blue.
        assert_eq!(
            comp(&p, 5),
            cw_raster::blend::composite(BLUE, [200, 0, 0, 128], 255, cw_raster::BlendMode::Normal)
        );
        assert_eq!(comp(&p, 12), RED);
        // Fade out over the last four frames of twenty: 3/4 at frame 16.
        assert_eq!(comp(&p, 19), BLUE);
        p.track_mut(v2).unwrap().hidden = true;
        assert_eq!(comp(&p, 12), BLUE);
    }

    #[test]
    fn picture_in_picture_scales_and_positions_the_clip() {
        let (mut p, lib, r, b) = setup();
        let v1 = p.tracks[0].id;
        let v2 = p.add_track(TrackKind::Video).unwrap();
        p.insert(&lib, Source::Media { media: b }, v1, 0).unwrap();
        let pip = p.insert(&lib, Source::Media { media: r }, v2, 0).unwrap();
        {
            let c = p.clip_mut(pip).unwrap();
            c.scale = crate::project::Param::fixed(500);
            c.x = crate::project::Param::fixed(4);
            c.y = crate::project::Param::fixed(-2);
        }
        let masks = Masks::new();
        let f = Compositor {
            project: &p,
            library: &lib,
            masks: &masks,
        }
        .frame(0);
        // Half size (8x4) centred at (8+4, 4-2): covers x 8..16, y 0..4.
        assert_eq!(f.get(12, 1), RED);
        assert_eq!(f.get(4, 1), BLUE);
        assert_eq!(f.get(12, 6), BLUE);
        // Rotated a quarter turn it stays inside the frame and deterministic.
        p.clip_mut(pip).unwrap().rotation = crate::project::Param::fixed(9000);
        let g = Compositor {
            project: &p,
            library: &lib,
            masks: &masks,
        }
        .frame(0);
        assert_eq!(g.get(12, 2), RED);
        assert_eq!(g.get(4, 2), BLUE);
    }

    #[test]
    fn crop_and_grade_act_on_the_source() {
        let (mut p, lib, r, _) = setup();
        let v1 = p.tracks[0].id;
        let c = p.insert(&lib, Source::Media { media: r }, v1, 0).unwrap();
        p.clip_mut(c).unwrap().crop.left = 500;
        let masks = Masks::new();
        let f = Compositor {
            project: &p,
            library: &lib,
            masks: &masks,
        }
        .frame(0);
        assert_eq!(
            f.get(2, 2),
            [0, 0, 0, 255],
            "cropped away shows the background"
        );
        assert_eq!(f.get(12, 2), RED);
        p.clip_mut(c).unwrap().grade.brightness = 50;
        let g = Compositor {
            project: &p,
            library: &lib,
            masks: &masks,
        }
        .frame(0);
        assert!(g.get(12, 2)[1] > 0, "brighter moves toward white");
    }

    #[test]
    fn titles_and_colour_clips_draw_from_the_supplied_glyphs() {
        let (mut p, lib, _, _) = setup();
        let v1 = p.tracks[0].id;
        let v2 = p.add_track(TrackKind::Video).unwrap();
        p.insert(
            &lib,
            Source::Color {
                color: "00ff00".into(),
            },
            v1,
            0,
        )
        .unwrap();
        let title = Title {
            text: "Hi".into(),
            size: 6,
            bold: false,
            color: "ffffff".into(),
            background: None,
            position: TitlePosition::Center,
        };
        p.insert(&lib, Source::Title(title.clone()), v2, 0).unwrap();
        let mut masks = Masks::new();
        masks.insert(
            title.raster_key(),
            TextMask {
                width: 2,
                height: 2,
                alpha: Blob::new(vec![255, 0, 0, 255]),
            },
        );
        let f = Compositor {
            project: &p,
            library: &lib,
            masks: &masks,
        }
        .frame(0);
        assert_eq!(f.get(7, 3), [255, 255, 255, 255]);
        assert_eq!(f.get(8, 3), [0, 255, 0, 255]);
        assert_eq!(f.get(8, 4), [255, 255, 255, 255]);
        assert_eq!(f.get(0, 0), [0, 255, 0, 255]);
    }
}
