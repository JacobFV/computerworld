//! The pieces every video editor interface is built from — the program monitor, the
//! transport, the multi-track timeline, the media bin, inspector sliders and the file
//! sheets — styled by each product's [`Skin`]. Every control dispatches a command the
//! [`Editor`] really handles, or is painted disabled with its reason.
use super::{Drag, Editor, Field, Product, Prop, Sheet, EXPORT_RATES, EXPORT_SIZES, PREFIX};
use crate::desktop_scene::{shared::Align, Painter};
use crate::AppEnv;
use cw_scene::{Color, Primitive, Rect};
use cw_video::{MediaKind, Source, TrackKind};

/// A product's palette.
#[derive(Clone, Copy, Debug)]
pub struct Skin {
    pub bg: Color,
    pub panel: Color,
    pub bar: Color,
    pub raised: Color,
    pub text: Color,
    pub muted: Color,
    pub accent: Color,
    pub on_accent: Color,
    pub line: Color,
    pub video: Color,
    pub audio: Color,
    pub title: Color,
    pub color_clip: Color,
    pub selected: Color,
    pub playhead: Color,
    pub radius: u32,
}

pub fn skin(product: Product, mobile: bool) -> Skin {
    match product {
        // Clipchamp: near-black panels, a violet accent on Export and selection.
        Product::Clipchamp => Skin {
            bg: Color::rgb(24, 24, 27),
            panel: Color::rgb(33, 33, 37),
            bar: Color::rgb(28, 28, 31),
            raised: Color::rgb(48, 48, 54),
            text: Color::rgb(240, 240, 244),
            muted: Color::rgb(160, 160, 170),
            accent: Color::rgb(124, 92, 255),
            on_accent: Color::WHITE,
            line: Color(255, 255, 255, 22),
            video: Color::rgb(58, 62, 80),
            audio: Color::rgb(33, 94, 86),
            title: Color::rgb(102, 72, 168),
            color_clip: Color::rgb(70, 70, 76),
            selected: Color::rgb(124, 92, 255),
            playhead: Color::WHITE,
            radius: 6,
        },
        // iMovie: graphite chrome, blue video, green audio, purple titles, yellow select.
        Product::Imovie if !mobile => Skin {
            bg: Color::rgb(30, 30, 30),
            panel: Color::rgb(40, 40, 42),
            bar: Color::rgb(52, 52, 54),
            raised: Color::rgb(70, 70, 72),
            text: Color::rgb(236, 236, 236),
            muted: Color::rgb(150, 150, 152),
            accent: Color::rgb(10, 132, 255),
            on_accent: Color::WHITE,
            line: Color(255, 255, 255, 24),
            video: Color::rgb(44, 88, 150),
            audio: Color::rgb(46, 132, 70),
            title: Color::rgb(116, 72, 170),
            color_clip: Color::rgb(90, 90, 96),
            selected: Color::rgb(255, 204, 0),
            playhead: Color::rgb(255, 204, 0),
            radius: 4,
        },
        Product::Imovie => Skin {
            bg: Color::BLACK,
            panel: Color::rgb(28, 28, 30),
            bar: Color::rgb(18, 18, 20),
            raised: Color::rgb(44, 44, 46),
            text: Color::WHITE,
            muted: Color::rgb(142, 142, 147),
            accent: Color::rgb(255, 204, 0),
            on_accent: Color::BLACK,
            line: Color(255, 255, 255, 30),
            video: Color::rgb(50, 90, 150),
            audio: Color::rgb(48, 136, 72),
            title: Color::rgb(116, 72, 170),
            color_clip: Color::rgb(90, 90, 96),
            selected: Color::rgb(255, 204, 0),
            playhead: Color::WHITE,
            radius: 8,
        },
        // Kdenlive under Breeze Dark.
        Product::Kdenlive => Skin {
            bg: Color::rgb(35, 38, 41),
            panel: Color::rgb(42, 46, 50),
            bar: Color::rgb(49, 54, 59),
            raised: Color::rgb(64, 70, 76),
            text: Color::rgb(239, 240, 241),
            muted: Color::rgb(160, 164, 168),
            accent: Color::rgb(61, 174, 233),
            on_accent: Color::WHITE,
            line: Color(0, 0, 0, 90),
            video: Color::rgb(68, 102, 142),
            audio: Color::rgb(56, 120, 88),
            title: Color::rgb(142, 96, 50),
            color_clip: Color::rgb(96, 96, 104),
            selected: Color::rgb(255, 170, 0),
            playhead: Color::rgb(255, 64, 64),
            radius: 3,
        },
        // A Material 3 dark editor.
        Product::VideoEditor => Skin {
            bg: Color::rgb(16, 16, 18),
            panel: Color::rgb(30, 30, 34),
            bar: Color::rgb(24, 24, 28),
            raised: Color::rgb(46, 46, 52),
            text: Color::rgb(236, 236, 240),
            muted: Color::rgb(150, 150, 158),
            accent: Color::rgb(0, 214, 190),
            on_accent: Color::BLACK,
            line: Color(255, 255, 255, 26),
            video: Color::rgb(52, 60, 76),
            audio: Color::rgb(28, 98, 118),
            title: Color::rgb(140, 70, 110),
            color_clip: Color::rgb(80, 80, 88),
            selected: Color::WHITE,
            playhead: Color::WHITE,
            radius: 10,
        },
    }
}

pub fn target(command: &str) -> String {
    format!("{PREFIX}:{command}")
}

/// A flat button with a symbol and an optional label; `on` fills it as engaged.
#[allow(clippy::too_many_arguments)]
pub fn tool(
    p: &mut Painter,
    s: &Skin,
    r: Rect,
    symbol: &str,
    label: &str,
    command: &str,
    on: bool,
    show_label: bool,
) {
    p.button(
        r,
        if on { s.raised } else { Color::TRANSPARENT },
        s.radius,
        &target(command),
        label,
    );
    let size = 16.min(r.height.saturating_sub(4));
    if show_label && !label.is_empty() {
        let w = p.measure(label, 12, false);
        let total = size + 6 + w;
        let x = r.x + (r.width as i32 - total as i32) / 2;
        p.symbol(
            symbol,
            x,
            r.y + (r.height as i32 - size as i32) / 2,
            size,
            if on { s.accent } else { s.text },
        );
        p.left(
            x + size as i32 + 6,
            r.y + (r.height as i32 - 16) / 2,
            w + 2,
            label,
            12,
            s.text,
        );
    } else {
        p.symbol(
            symbol,
            r.x + (r.width as i32 - size as i32) / 2,
            r.y + (r.height as i32 - size as i32) / 2,
            size,
            if on { s.accent } else { s.text },
        );
    }
}

/// A tool the product paints but cannot honour now: announced disabled with its reason.
pub fn tool_off(p: &mut Painter, s: &Skin, r: Rect, symbol: &str, why: &str) {
    p.box_(r, Color::TRANSPARENT, s.radius);
    p.disabled(why);
    let size = 16.min(r.height.saturating_sub(4));
    p.symbol(
        symbol,
        r.x + (r.width as i32 - size as i32) / 2,
        r.y + (r.height as i32 - size as i32) / 2,
        size,
        s.muted,
    );
}

/// A text button, filled with the accent when `primary`.
pub fn text_button(p: &mut Painter, s: &Skin, r: Rect, label: &str, command: &str, primary: bool) {
    p.button(
        r,
        if primary { s.accent } else { s.raised },
        s.radius,
        &target(command),
        label,
    );
    p.label(
        r.x,
        r.y + (r.height as i32 - 17) / 2,
        r.width,
        label,
        12,
        if primary { s.on_accent } else { s.text },
        primary,
        Align::Center,
    );
}
pub fn text_button_off(p: &mut Painter, s: &Skin, r: Rect, label: &str, why: &str) {
    p.border(r, Color::TRANSPARENT, s.radius, s.line);
    p.disabled(why);
    p.label(
        r.x,
        r.y + (r.height as i32 - 17) / 2,
        r.width,
        label,
        12,
        s.muted,
        false,
        Align::Center,
    );
}
/// A chip in a row of choices.
pub fn chip(p: &mut Painter, s: &Skin, r: Rect, label: &str, command: &str, on: bool) {
    p.button(
        r,
        if on { s.accent } else { s.raised },
        s.radius,
        &target(command),
        label,
    );
    p.label(
        r.x,
        r.y + (r.height as i32 - 16) / 2,
        r.width,
        label,
        11,
        if on { s.on_accent } else { s.text },
        on,
        Align::Center,
    );
}

pub fn rgba(c: [u8; 4]) -> Color {
    Color(c[0], c[1], c[2], c[3])
}

/// The program monitor: the composited frame at the playhead, fitted to `r` at 16:9.
pub fn monitor(p: &mut Painter, ed: &Editor, env: &AppEnv<'_>, r: Rect, s: &Skin) {
    p.box_(r, Color::BLACK, 0);
    let (pw, ph) = (ed.project.width, ed.project.height);
    let scale = (r.width * 1000 / pw).min(r.height * 1000 / ph).max(1);
    let (w, h) = ((pw * scale / 1000).max(1), (ph * scale / 1000).max(1));
    let frame_rect = Rect::new(
        r.x + (r.width as i32 - w as i32) / 2,
        r.y + (r.height as i32 - h as i32) / 2,
        w,
        h,
    );
    let frame = ed.frame(env.clock_us);
    p.node(
        frame_rect,
        Primitive::Image {
            width: frame.width(),
            height: frame.height(),
            rgba: frame.into_pixels(),
        },
        None,
    );
    // Say plainly when a clip under the playhead has no media to show.
    let now = ed.now(env.clock_us);
    let missing = ed.project.clips.iter().any(|c| {
        c.contains(now)
            && c.media()
                .is_some_and(|m| !ed.library.contains_key(&m) && ed.offline.contains_key(&m))
    });
    let untitled = ed.text_problem.is_some()
        && ed
            .project
            .clips
            .iter()
            .any(|c| c.contains(now) && matches!(c.source, Source::Title(_)));
    let note = if missing {
        Some("Missing media")
    } else if untitled {
        Some("Titles cannot be drawn in this build")
    } else {
        None
    };
    if let Some(note) = note {
        p.box_(
            Rect::new(frame_rect.x, frame_rect.y + h as i32 - 26, w, 26),
            Color(180, 30, 30, 200),
            0,
        );
        p.center(
            frame_rect.x,
            frame_rect.y + h as i32 - 22,
            w,
            note,
            12,
            Color::WHITE,
        );
    }
    let _ = s;
}

/// Timecode shown under a monitor, in the product's format.
pub fn clock_text(ed: &Editor, env: &AppEnv<'_>) -> String {
    let p = &ed.project;
    let now = ed.now(env.clock_us);
    match ed.product {
        Product::Clipchamp => {
            let fps = i64::from(p.fps);
            let fmt = |f: i64| {
                format!(
                    "{:02}:{:02}.{:02}",
                    f / fps / 60,
                    f / fps % 60,
                    f % fps * 100 / fps
                )
            };
            format!("{} / {}", fmt(now), fmt(p.duration()))
        }
        Product::Kdenlive => p.timecode(now),
        _ => format!("{} / {}", short(p, now), short(p, p.duration())),
    }
}
fn short(p: &cw_video::Project, f: i64) -> String {
    let fps = i64::from(p.fps.max(1));
    format!("{}:{:02}", f / fps / 60, f / fps % 60)
}

/// Transport buttons laid out centred in `r`.
pub fn transport(p: &mut Painter, ed: &Editor, env: &AppEnv<'_>, r: Rect, s: &Skin) {
    let playing = ed.play.is_some_and(|pl| pl.rate > 0);
    let at_start = ed.now(env.clock_us) == 0;
    let at_end = ed.now(env.clock_us) >= ed.project.duration();
    let empty = ed.project.duration() == 0;
    let mut buttons: Vec<(&str, &str, &str, Option<&str>)> = vec![];
    let skip = ed.product == Product::Clipchamp;
    buttons.push((
        "skip-previous",
        "Go to start",
        "start",
        at_start.then_some("Already at the start"),
    ));
    if skip {
        buttons.push((
            "backward",
            "Skip back 5 seconds",
            "skip:-1",
            at_start.then_some("Already at the start"),
        ));
    } else {
        buttons.push((
            "chevron-left",
            "Previous frame",
            "step:-1",
            at_start.then_some("Already at the start"),
        ));
    }
    buttons.push((
        if playing { "pause" } else { "play" },
        if playing { "Pause" } else { "Play" },
        "play",
        empty.then_some("The timeline is empty"),
    ));
    if skip {
        buttons.push((
            "forward",
            "Skip forward 5 seconds",
            "skip:1",
            at_end.then_some("Already at the end"),
        ));
    } else {
        buttons.push((
            "chevron-right",
            "Next frame",
            "step:1",
            at_end.then_some("Already at the end"),
        ));
    }
    buttons.push((
        "skip-next",
        "Go to end",
        "end",
        at_end.then_some("Already at the end"),
    ));
    let size = r.height.min(32);
    let total = buttons.len() as i32 * (size as i32 + 6);
    let mut x = r.x + (r.width as i32 - total) / 2;
    for (symbol, label, command, off) in buttons {
        let b = Rect::new(x, r.y + (r.height as i32 - size as i32) / 2, size, size);
        match off {
            Some(why) => tool_off(p, s, b, symbol, why),
            None => tool(
                p,
                s,
                b,
                symbol,
                label,
                command,
                command == "play" && playing,
                false,
            ),
        }
        x += size as i32 + 6;
    }
}

/// Where the lanes of a timeline are, for surfaces elsewhere that drop onto them.
#[derive(Clone, Copy, Debug)]
pub struct Lanes {
    /// Left edge of frame `scroll`, and top of the first lane.
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub lane: u32,
    pub scroll: i64,
}

/// Timeline geometry for a rectangle: the lanes start after the headers and ruler.
pub fn lanes(ed: &Editor, env: &AppEnv<'_>, r: Rect, header: u32, ruler: u32, lane: u32) -> Lanes {
    let width = r.width.saturating_sub(header);
    let visible = ed.frames(i64::from(width)).max(1);
    let now = ed.now(env.clock_us);
    let mut scroll = ed.scroll;
    if now < scroll || now >= scroll + visible {
        scroll = (now - visible / 5).max(0);
    }
    Lanes {
        x: r.x + header as i32,
        y: r.y + ruler as i32,
        width,
        lane,
        scroll,
    }
}

/// The multi-track timeline in `r`: ruler, optional track headers, clips with their
/// thumbnails, waveforms, fades, keyframes and transitions, the playhead, and whatever
/// is being dragged.
pub fn timeline(
    p: &mut Painter,
    ed: &Editor,
    env: &AppEnv<'_>,
    r: Rect,
    s: &Skin,
    g: Lanes,
    header: u32,
) {
    let features = ed.features(env.theme.mobile());
    p.box_(r, s.bg, 0);
    let ruler_h = (g.y - r.y) as u32;
    // Ruler: a drag surface that scrubs.
    let ruler = Rect::new(g.x, r.y, g.width, ruler_h);
    p.box_(ruler, s.bar, 0);
    let fps = i64::from(ed.project.fps);
    let step_s = [1i64, 2, 5, 10, 30, 60]
        .into_iter()
        .find(|sec| ed.pixels(sec * fps) >= 60)
        .unwrap_or(120);
    let first = g.scroll / (step_s * fps) * step_s * fps;
    let mut f = first;
    while ed.pixels(f - g.scroll) < i64::from(g.width) {
        let x = g.x + ed.pixels(f - g.scroll) as i32;
        if f >= g.scroll {
            p.vline(x, r.y + ruler_h as i32 - 6, 6, s.muted);
            let label = match ed.product {
                Product::Kdenlive => ed.project.timecode(f),
                _ => format!("{}:{:02}", f / fps / 60, f / fps % 60),
            };
            p.left(x + 3, r.y + 2, 90, &label, 10, s.muted);
        }
        f += step_s * fps;
    }
    p.node(
        ruler,
        Primitive::Region,
        Some((&target(&format!("ruler:{}", g.scroll)), "Timeline ruler")),
    );
    let lanes = ed.lanes();
    let empty = ed.project.clips.is_empty();
    for (i, id) in lanes.iter().enumerate() {
        let Some(track) = ed.project.track(*id) else {
            continue;
        };
        let y = g.y + (i as u32 * g.lane) as i32;
        if y > r.y + r.height as i32 {
            break;
        }
        let row = Rect::new(g.x, y, g.width, g.lane);
        p.box_(row, if i % 2 == 0 { s.panel } else { s.bg }, 0);
        p.hline(r.x, y + g.lane as i32 - 1, r.width, s.line);
        if header > 0 {
            let h = Rect::new(r.x, y, header, g.lane);
            p.box_(h, s.bar, 0);
            p.left(
                r.x + 8,
                y + (g.lane as i32 - 16) / 2,
                30,
                &track.name,
                12,
                if track.locked {
                    Color::rgb(230, 80, 80)
                } else {
                    s.text
                },
            );
            let mut bx = r.x + header as i32 - 4;
            let size = 22.min(g.lane.saturating_sub(4));
            let by = y + (g.lane as i32 - size as i32) / 2;
            let controls: [(bool, &str, &str, bool); 3] = [
                (features.track_lock, "lock", "track-lock", track.locked),
                (
                    features.track_hide && track.kind == TrackKind::Video,
                    if track.hidden { "eye-off" } else { "eye" },
                    "track-hide",
                    track.hidden,
                ),
                (
                    features.track_mute && track.kind == TrackKind::Audio,
                    if track.muted { "volume-mute" } else { "volume" },
                    "track-mute",
                    track.muted,
                ),
            ];
            for (shown, symbol, command, on) in controls {
                if !shown {
                    continue;
                }
                bx -= size as i32 + 2;
                let label = match command {
                    "track-lock" => {
                        if on {
                            "Unlock track"
                        } else {
                            "Lock track"
                        }
                    }
                    "track-hide" => {
                        if on {
                            "Show track"
                        } else {
                            "Hide track"
                        }
                    }
                    _ => {
                        if on {
                            "Unmute track"
                        } else {
                            "Mute track"
                        }
                    }
                };
                tool(
                    p,
                    s,
                    Rect::new(bx, by, size, size),
                    symbol,
                    label,
                    &format!("{command}:{}", track.id),
                    on,
                    false,
                );
            }
        }
        for c in ed.project.on_track(*id) {
            clip(
                p,
                ed,
                s,
                g,
                i as u32,
                c,
                track.kind,
                track.hidden || track.muted,
            );
        }
    }
    if empty {
        p.center(
            g.x,
            g.y + (g.lane as i32 - 16) / 2,
            g.width,
            match ed.product {
                Product::Clipchamp => "Drag and drop media here",
                Product::Kdenlive => "Drop clips from the Project Bin",
                _ => "Drag media here to start your movie",
            },
            12,
            s.muted,
        );
    }
    // Transitions sit on the cut they cross.
    for t in &ed.project.transitions {
        let Some((from, to)) = ed.project.transition_span(t) else {
            continue;
        };
        let Some(a) = ed.project.clip(t.left) else {
            continue;
        };
        let Some(i) = lanes.iter().position(|l| *l == a.track) else {
            continue;
        };
        let x0 = g.x + ed.pixels(from - g.scroll) as i32;
        let x1 = g.x + ed.pixels(to - g.scroll) as i32;
        if x1 < g.x || x0 > g.x + g.width as i32 {
            continue;
        }
        let y = g.y + (i as u32 * g.lane) as i32;
        let w = (x1 - x0).max(10) as u32;
        let on = ed.selected_transition == Some(t.id);
        let box_r = Rect::new(x0, y + g.lane as i32 / 4, w, g.lane / 2);
        p.button(
            box_r,
            if on { s.selected } else { Color(0, 0, 0, 150) },
            3,
            &target(&format!("transition:{}", t.id)),
            ed.product.transition_name(t.kind),
        );
        p.line(
            vec![(x0, y + g.lane as i32 * 3 / 4), (x1, y + g.lane as i32 / 4)],
            Color::WHITE,
            1,
        );
    }
    // The drag in progress.
    match &ed.drag {
        Some(Drag::Clip {
            id,
            press_x,
            press_y,
            x,
            y,
            ..
        }) => {
            if let Some(c) = ed.project.clip(*id) {
                let i = lanes.iter().position(|l| *l == c.track).unwrap_or(0) as i32;
                let lane_y = g.y + i * g.lane as i32;
                let lane_shift = (*press_y + (y - press_y)).div_euclid(g.lane as i32);
                let gx = g.x + ed.pixels(c.start - g.scroll) as i32 + (x - press_x);
                let gy = lane_y + lane_shift * g.lane as i32;
                let w = ed.pixels(c.length).max(4) as u32;
                p.border(
                    Rect::new(gx, gy, w, g.lane - 2),
                    Color(255, 255, 255, 50),
                    s.radius,
                    s.selected,
                );
            }
        }
        Some(Drag::Media {
            id, ox, oy, x, y, ..
        }) => {
            let lx = x - ox;
            let ly = y - oy;
            if lx >= 0 {
                let lane = ly.div_euclid(g.lane as i32);
                let length = ed
                    .library
                    .get(id)
                    .and_then(|m| m.length_us())
                    .map(|us| ed.project.centiframes(us) / 100)
                    .unwrap_or(4 * fps);
                let gx = g.x + lx;
                let gy = g.y + lane * g.lane as i32;
                p.border(
                    Rect::new(gx, gy, ed.pixels(length).max(4) as u32, g.lane - 2),
                    Color(255, 255, 255, 50),
                    s.radius,
                    s.selected,
                );
                if lane < 0 || lane >= lanes.len() as i32 {
                    p.left(gx + 4, gy + 4, 120, "New track", 11, s.text);
                }
            }
        }
        _ => {}
    }
    // The playhead.
    let now = ed.now(env.clock_us);
    let px = g.x + ed.pixels(now - g.scroll) as i32;
    if px >= g.x && px <= g.x + g.width as i32 {
        p.box_(Rect::new(px - 1, r.y, 2, r.height), s.playhead, 0);
        p.path(
            vec![(px - 6, r.y), (px + 6, r.y), (px, r.y + 8)],
            s.playhead,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn clip(
    p: &mut Painter,
    ed: &Editor,
    s: &Skin,
    g: Lanes,
    lane_index: u32,
    c: &cw_video::Clip,
    kind: TrackKind,
    dimmed: bool,
) {
    let x0 = g.x + ed.pixels(c.start - g.scroll) as i32;
    let x1 = g.x + ed.pixels(c.end() - g.scroll) as i32;
    let right = g.x + g.width as i32;
    if x1 < g.x || x0 > right {
        return;
    }
    let (vx0, vx1) = (x0.max(g.x), x1.min(right));
    let y = g.y + (lane_index * g.lane) as i32 + 1;
    let h = g.lane - 3;
    let r = Rect::new(vx0, y, (vx1 - vx0).max(2) as u32, h);
    let selected = ed.selected == Some(c.id);
    let fill = match (&c.source, kind) {
        (_, TrackKind::Audio) => s.audio,
        (Source::Title(_), _) => s.title,
        (Source::Color { color }, _) => cw_raster::parse_hex(color)
            .map(rgba)
            .unwrap_or(s.color_clip),
        _ => s.video,
    };
    let offline = c.media().is_some_and(|m| !ed.library.contains_key(&m));
    // The clip body is its own drag surface.
    p.node(
        r,
        Primitive::RoundedBox {
            fill: if offline {
                Color::rgb(120, 40, 40)
            } else {
                fill
            },
            border: Some(if selected {
                s.selected
            } else {
                Color(0, 0, 0, 90)
            }),
            border_width: if selected { 2 } else { 1 },
            radius: s.radius.min(6),
        },
        Some((&target(&format!("clip:{}:{}", c.id, g.lane)), &c.name)),
    );
    // Thumbnails along a picture clip, or the waveform along a sound.
    if let Some(media) = c.media().and_then(|m| ed.library.get(&m)) {
        if media.kind == MediaKind::Audio {
            let mid = y + h as i32 / 2;
            let peaks = media.peaks.bytes();
            let mut x = vx0 + 1;
            while x < vx1 - 1 {
                let frame = c.start + ed.frames(i64::from(x - x0));
                let local = frame - c.start;
                let src = c.source_at(local.clamp(0, c.length - 1));
                let us = ed.project.micros(src);
                let idx = (us * i64::from(cw_video::media::PEAKS_PER_SECOND) / 1_000_000) as usize;
                let peak = u32::from(*peaks.get(idx).unwrap_or(&0));
                let gain = c.volume.at(local).clamp(0, 200) as u32;
                let amp = ((peak * gain / 100) * (h / 2 - 2) / 255).min(h / 2 - 2);
                if amp > 0 {
                    p.box_(
                        Rect::new(x, mid - amp as i32, 1, amp * 2),
                        Color(255, 255, 255, 150),
                        0,
                    );
                }
                x += 2;
            }
        } else if let Some(first) = media.thumbs.first() {
            let tw = (first.width() * (h - 4) / first.height().max(1)).max(8);
            let mut x = x0 + 2;
            while x < x1 - 2 {
                if x + tw as i32 > vx0 && x < vx1 {
                    let local = ed.frames(i64::from(x - x0));
                    let src = c.source_at(local.clamp(0, c.length - 1));
                    if let Some(t) = media.thumb(ed.project.micros(src)) {
                        let cw = (tw as i32).min(vx1 - 2 - x).max(1) as u32;
                        if x >= vx0 && cw == tw {
                            p.node(
                                Rect::new(x, y + 2, tw, h - 4),
                                Primitive::Image {
                                    width: t.width(),
                                    height: t.height(),
                                    rgba: t.pixels().to_vec(),
                                },
                                None,
                            );
                        }
                    }
                }
                x += tw as i32 + 1;
            }
        }
    }
    if dimmed {
        p.box_(r, Color(0, 0, 0, 120), s.radius.min(6));
    }
    let mut label = c.name.clone();
    if c.speed != 100 {
        label = format!("{label}  {}%", c.speed);
    }
    if c.reverse {
        label = format!("◀ {label}");
    }
    if offline {
        label = format!("{label} (missing)");
    }
    p.box_(
        Rect::new(
            r.x + 2,
            y + 2,
            r.width
                .saturating_sub(4)
                .min(p.measure(&label, 10, true) + 8),
            15,
        ),
        Color(0, 0, 0, 110),
        3,
    );
    p.label(
        r.x + 5,
        y + 2,
        r.width.saturating_sub(8),
        &label,
        10,
        Color::WHITE,
        true,
        Align::Left,
    );
    // Fade ramps.
    for (frames, at_start) in [(c.fade_in, true), (c.fade_out, false)] {
        if frames == 0 {
            continue;
        }
        let w = ed.pixels(i64::from(frames)) as i32;
        let pts = if at_start {
            vec![(x0, y + h as i32), (x0 + w, y)]
        } else {
            vec![(x1 - w, y), (x1, y + h as i32)]
        };
        p.line(pts, Color(255, 255, 255, 200), 1);
    }
    // Keyframe diamonds on the properties that have them.
    for param in [&c.opacity, &c.x, &c.y, &c.scale, &c.rotation, &c.volume] {
        for k in &param.keys {
            let kx = x0 + ed.pixels(k.frame) as i32;
            if kx < vx0 || kx > vx1 {
                continue;
            }
            let ky = y + h as i32 - 7;
            p.path(
                vec![(kx, ky - 4), (kx + 4, ky), (kx, ky + 4), (kx - 4, ky)],
                Color::rgb(255, 214, 0),
            );
        }
    }
    // Trim handles at both edges of the selected clip, above its body.
    if selected {
        let hw = 6;
        if x0 >= g.x {
            p.node(
                Rect::new(x0, y, hw, h),
                Primitive::RoundedBox {
                    fill: s.selected,
                    border: None,
                    border_width: 0,
                    radius: 2,
                },
                Some((&target(&format!("trim-in:{}", c.id)), "Trim start")),
            );
        }
        if x1 <= right {
            p.node(
                Rect::new(x1 - hw as i32, y, hw, h),
                Primitive::RoundedBox {
                    fill: s.selected,
                    border: None,
                    border_width: 0,
                    radius: 2,
                },
                Some((&target(&format!("trim-out:{}", c.id)), "Trim end")),
            );
        }
    }
}

/// Media bin tiles, laid out in `r`, each a drag surface onto the timeline.
#[allow(clippy::too_many_arguments)]
pub fn bin(
    p: &mut Painter,
    ed: &Editor,
    env: &AppEnv<'_>,
    r: Rect,
    s: &Skin,
    g: Lanes,
    tile_w: u32,
    list: bool,
) {
    let _ = env;
    if ed.project.media.is_empty() {
        p.center(
            r.x,
            r.y + 20,
            r.width,
            match ed.product {
                Product::Kdenlive => "Add clips to the Project Bin",
                Product::Clipchamp => "Import media to start",
                _ => "Import media to get started",
            },
            12,
            s.muted,
        );
        return;
    }
    let row_h = if list { 44 } else { tile_w * 9 / 16 + 22 };
    let cols = if list {
        1
    } else {
        ((r.width + 8) / (tile_w + 8)).max(1)
    };
    for (i, m) in ed.project.media.iter().enumerate() {
        let (col, row) = (i as u32 % cols, i as u32 / cols);
        let x = r.x + (col * (if list { r.width } else { tile_w + 8 })) as i32;
        let y = r.y + (row * (row_h + 6)) as i32;
        if y + row_h as i32 > r.y + r.height as i32 {
            break;
        }
        let tile = if list {
            Rect::new(x, y, r.width, row_h)
        } else {
            Rect::new(x, y, tile_w, row_h)
        };
        let selected = ed.selected_media == Some(m.id);
        let lane = g.lane;
        let surface = format!(
            "media:{}:{}:{}:{}:{}",
            m.id,
            g.x - tile.x,
            g.y - tile.y,
            lane,
            g.scroll
        );
        let name = m.path.rsplit('/').next().unwrap_or(&m.path);
        p.node(
            tile,
            Primitive::RoundedBox {
                fill: if selected { s.raised } else { s.panel },
                border: Some(if selected { s.accent } else { s.line }),
                border_width: if selected { 2 } else { 1 },
                radius: s.radius,
            },
            Some((&target(&surface), name)),
        );
        let thumb_r = if list {
            Rect::new(tile.x + 4, tile.y + 4, 64, 36)
        } else {
            Rect::new(tile.x + 3, tile.y + 3, tile_w - 6, tile_w * 9 / 16 - 4)
        };
        p.box_(thumb_r, Color::BLACK, 3);
        match ed.library.get(&m.id) {
            Some(media) if media.kind == MediaKind::Audio => {
                let peaks = media.peaks.bytes();
                let mid = thumb_r.y + thumb_r.height as i32 / 2;
                for k in 0..thumb_r.width as i32 / 2 {
                    let idx = k as usize * peaks.len() / (thumb_r.width as usize / 2).max(1);
                    let amp =
                        u32::from(*peaks.get(idx).unwrap_or(&0)) * (thumb_r.height / 2 - 1) / 255;
                    p.box_(
                        Rect::new(thumb_r.x + k * 2, mid - amp as i32, 1, amp * 2 + 1),
                        s.audio,
                        0,
                    );
                }
            }
            Some(media) => {
                if let Some(t) = media.thumbs.first() {
                    let w = (t.width() * thumb_r.height / t.height().max(1)).min(thumb_r.width);
                    p.node(
                        Rect::new(
                            thumb_r.x + (thumb_r.width - w) as i32 / 2,
                            thumb_r.y,
                            w,
                            thumb_r.height,
                        ),
                        Primitive::Image {
                            width: t.width(),
                            height: t.height(),
                            rgba: t.pixels().to_vec(),
                        },
                        None,
                    );
                }
            }
            None => {
                let why = ed.offline.get(&m.id).map_or("Loading…", |_| "Missing");
                p.center(
                    thumb_r.x,
                    thumb_r.y + thumb_r.height as i32 / 2 - 8,
                    thumb_r.width,
                    why,
                    10,
                    Color::rgb(255, 120, 120),
                );
            }
        }
        let duration = ed
            .library
            .get(&m.id)
            .and_then(|md| md.length_us())
            .map(|us| format!("{}.{:01}s", us / 1_000_000, us / 100_000 % 10));
        if list {
            p.left(
                tile.x + 76,
                tile.y + 6,
                tile.width.saturating_sub(110),
                name,
                12,
                s.text,
            );
            p.left(
                tile.x + 76,
                tile.y + 24,
                120,
                &duration.unwrap_or_else(|| "Still".into()),
                10,
                s.muted,
            );
        } else {
            p.left(
                tile.x + 4,
                tile.y + row_h as i32 - 18,
                tile_w - 8,
                name,
                10,
                s.text,
            );
            if let Some(d) = duration {
                p.label(
                    thumb_r.x,
                    thumb_r.y + thumb_r.height as i32 - 15,
                    thumb_r.width - 4,
                    &d,
                    9,
                    Color::WHITE,
                    true,
                    Align::Right,
                );
            }
        }
        // The add button every bin tile carries: onto the end of the timeline.
        let add = Rect::new(tile.x + tile.width as i32 - 24, tile.y + 4, 20, 20);
        if ed.library.contains_key(&m.id) {
            p.z += 1;
            p.button(
                add,
                s.accent,
                10,
                &target(&format!("append:{}", m.id)),
                &format!("Add {name} to the timeline"),
            );
            p.symbol("plus", add.x + 3, add.y + 3, 14, s.on_accent);
            p.z -= 1;
        }
    }
}

/// A labelled slider row for `prop`: drag or click the track, or nudge with − and +.
#[allow(clippy::too_many_arguments)]
pub fn slider(
    p: &mut Painter,
    ed: &Editor,
    env: &AppEnv<'_>,
    s: &Skin,
    x: i32,
    y: i32,
    w: u32,
    label: &str,
    prop: Prop,
    unit: &str,
) -> i32 {
    let value = ed.value(prop, env.clock_us);
    let (lo, hi) = ed.range(prop);
    p.left(x, y, w.saturating_sub(70), label, 12, s.text);
    let shown = match (value, prop) {
        (None, _) => "—".to_owned(),
        (Some(v), Prop::Opacity) => format!("{}%", v / 10),
        (Some(v), Prop::Scale) => format!("{}%", v / 10),
        (Some(v), Prop::Rotation) => format!("{}°", v / 100),
        (Some(v), Prop::Speed) => format!("{}.{:02}x", v / 100, v % 100),
        (Some(v), Prop::FadeIn | Prop::FadeOut | Prop::Transition) => {
            let fps = ed.project.fps as i32;
            format!("{}.{:01}s", v / fps, v % fps * 10 / fps)
        }
        (Some(v), Prop::CropLeft | Prop::CropTop | Prop::CropRight | Prop::CropBottom) => {
            format!("{}%", v / 10)
        }
        (Some(v), _) => format!("{v}{unit}"),
    };
    p.label(
        x + w as i32 - 70,
        y,
        70,
        &shown,
        12,
        s.muted,
        false,
        Align::Right,
    );
    let keyframes = ed.features(env.theme.mobile()).keyframes && prop.animatable();
    let track_w = w.saturating_sub(if keyframes { 70 } else { 50 });
    let tr = Rect::new(x, y + 22, track_w, 18);
    let Some(v) = value else {
        p.box_(Rect::new(tr.x, tr.y + 7, tr.width, 4), s.raised, 2);
        p.disabled("Select a clip first");
        return y + 46;
    };
    let t = ((v - lo) as i64 * i64::from(track_w) / i64::from((hi - lo).max(1))) as i32;
    p.box_(Rect::new(tr.x, tr.y + 7, tr.width, 4), s.raised, 2);
    p.box_(Rect::new(tr.x, tr.y + 7, t.max(0) as u32, 4), s.accent, 2);
    p.circle(tr.x + t, tr.y + 9, 7, Color::WHITE);
    p.node(
        tr,
        Primitive::Region,
        Some((&target(&format!("slider:{}:{}", prop.id(), track_w)), label)),
    );
    let step = match prop {
        Prop::Opacity | Prop::Scale => 50,
        Prop::Rotation => 500,
        Prop::Speed => 25,
        Prop::X | Prop::Y => 4,
        Prop::CropLeft | Prop::CropTop | Prop::CropRight | Prop::CropBottom => 25,
        _ => 5,
    };
    let bx = tr.x + track_w as i32 + 6;
    tool(
        p,
        s,
        Rect::new(bx, tr.y - 1, 20, 20),
        "minus",
        &format!("Decrease {label}"),
        &format!("nudge:{}:-{step}", prop.id()),
        false,
        false,
    );
    tool(
        p,
        s,
        Rect::new(bx + 22, tr.y - 1, 20, 20),
        "plus",
        &format!("Increase {label}"),
        &format!("nudge:{}:{step}", prop.id()),
        false,
        false,
    );
    if keyframes {
        let keyed = ed.keyed(prop, env.clock_us);
        let kr = Rect::new(bx + 46, tr.y - 1, 20, 20);
        p.button(
            kr,
            if keyed { s.raised } else { Color::TRANSPARENT },
            3,
            &target(&format!("key:{}", prop.id())),
            if keyed {
                "Remove keyframe"
            } else {
                "Add keyframe"
            },
        );
        let (cx, cy) = (kr.x + 10, kr.y + 10);
        p.path(
            vec![(cx, cy - 6), (cx + 6, cy), (cx, cy + 6), (cx - 6, cy)],
            if keyed {
                Color::rgb(255, 214, 0)
            } else {
                s.muted
            },
        );
    }
    y + 48
}

/// A modal file or settings sheet, centred over the editor.
pub fn sheet(p: &mut Painter, ed: &Editor, env: &AppEnv<'_>, s: &Skin) {
    let Some(sheet) = &ed.sheet else {
        return;
    };
    let (w, h) = (env.width, env.height);
    p.z += 10;
    p.region(Rect::new(0, 0, w, h), &target("noop"), "Sheet backdrop");
    p.box_(Rect::new(0, 0, w, h), Color(0, 0, 0, 120), 0);
    let sw = w.saturating_sub(40).min(460);
    let sh = h.saturating_sub(60).min(420);
    let r = Rect::new(
        (w as i32 - sw as i32) / 2,
        (h as i32 - sh as i32) / 2,
        sw,
        sh,
    );
    p.border(r, s.panel, s.radius + 4, s.line);
    let mobile = env.theme.mobile();
    match sheet {
        Sheet::Browse {
            folder,
            entries,
            loading,
            error,
            project,
        } => {
            let title = match (ed.product, project) {
                (_, true) => "Open Project",
                (Product::Clipchamp, _) => "Import media",
                (Product::Kdenlive, _) => "Add Clip or Folder",
                _ => "Import Media",
            };
            p.strong(r.x + 16, r.y + 14, sw - 120, title, 15, s.text);
            text_button(
                p,
                s,
                Rect::new(r.x + sw as i32 - 86, r.y + 10, 72, 26),
                "Done",
                "close-sheet",
                true,
            );
            p.left(r.x + 16, r.y + 40, sw - 90, folder, 11, s.muted);
            text_button(
                p,
                s,
                Rect::new(r.x + sw as i32 - 86, r.y + 40, 72, 22),
                "Up",
                "browse:..",
                false,
            );
            let mut y = r.y + 70;
            if *loading {
                p.center(r.x, y + 20, sw, "Loading…", 12, s.muted);
            } else if let Some(e) = error {
                p.center(r.x, y + 20, sw, e, 12, Color::rgb(255, 120, 120));
            } else if entries.is_empty() {
                p.center(
                    r.x,
                    y + 20,
                    sw,
                    if *project {
                        "No projects in this folder"
                    } else {
                        "No movies, pictures or sounds in this folder"
                    },
                    12,
                    s.muted,
                );
            }
            for e in entries {
                if y + 30 > r.y + sh as i32 - 8 {
                    break;
                }
                let row = Rect::new(r.x + 10, y, sw - 20, 28);
                let folder_row = e.ends_with('/');
                let imported = !folder_row
                    && ed.project.media.iter().any(|m| {
                        m.path.rsplit('/').next() == Some(e.as_str())
                            && m.path
                                .ends_with(&format!("{}/{e}", folder.trim_end_matches('/')))
                    });
                let command = if folder_row {
                    format!("browse:{e}")
                } else if mobile && !project {
                    format!("pick-add:{e}")
                } else {
                    format!("pick:{e}")
                };
                p.button(
                    row,
                    if imported {
                        s.raised
                    } else {
                        Color::TRANSPARENT
                    },
                    s.radius,
                    &target(&command),
                    e,
                );
                let symbol = if folder_row {
                    "folder"
                } else {
                    match cw_video::media::kind_of(e) {
                        Some(MediaKind::Audio) => "music",
                        Some(MediaKind::Video) => "film",
                        Some(MediaKind::Still) => "image",
                        None => "document",
                    }
                };
                p.symbol(symbol, row.x + 6, row.y + 6, 16, s.accent);
                p.left(
                    row.x + 30,
                    row.y + 6,
                    row.width - 120,
                    e.trim_end_matches('/'),
                    12,
                    s.text,
                );
                if imported {
                    p.label(
                        row.x,
                        row.y + 7,
                        row.width - 8,
                        "Imported",
                        10,
                        s.muted,
                        false,
                        Align::Right,
                    );
                }
                y += 30;
            }
        }
        Sheet::Save { name } => {
            p.strong(r.x + 16, r.y + 14, sw - 32, "Save Project", 15, s.text);
            field(
                p,
                s,
                Rect::new(r.x + 16, r.y + 48, sw - 32, 30),
                name,
                ed.field == Field::Name,
                "name",
            );
            p.left(
                r.x + 16,
                r.y + 86,
                sw - 32,
                &format!("in {}", ed.folder),
                11,
                s.muted,
            );
            text_button(
                p,
                s,
                Rect::new(r.x + sw as i32 - 176, r.y + 116, 76, 28),
                "Cancel",
                "close-sheet",
                false,
            );
            text_button(
                p,
                s,
                Rect::new(r.x + sw as i32 - 92, r.y + 116, 76, 28),
                "Save",
                "save-confirm",
                true,
            );
        }
        Sheet::Export {
            name,
            width,
            height,
            fps,
        } => {
            let title = match ed.product {
                Product::Kdenlive => "Render",
                Product::Imovie => "Export File",
                _ => "Export",
            };
            p.strong(r.x + 16, r.y + 14, sw - 32, title, 15, s.text);
            field(
                p,
                s,
                Rect::new(r.x + 16, r.y + 46, sw - 32, 30),
                name,
                ed.field == Field::Name,
                "name",
            );
            p.left(r.x + 16, r.y + 86, sw - 32, "Resolution", 12, s.muted);
            let mut x = r.x + 16;
            for (w2, h2) in EXPORT_SIZES {
                let label = format!("{h2}p ({w2}×{h2})");
                let cw = p.measure(&label, 11, false) + 18;
                chip(
                    p,
                    s,
                    Rect::new(x, r.y + 106, cw, 26),
                    &label,
                    &format!("export-size:{w2}x{h2}"),
                    (*width, *height) == (w2, h2),
                );
                x += cw as i32 + 6;
            }
            p.left(r.x + 16, r.y + 142, sw - 32, "Frame rate", 12, s.muted);
            let mut x = r.x + 16;
            for f in EXPORT_RATES {
                let label = format!("{f} fps");
                chip(
                    p,
                    s,
                    Rect::new(x, r.y + 162, 70, 26),
                    &label,
                    &format!("export-fps:{f}"),
                    *fps == f,
                );
                x += 76;
            }
            p.left(
                r.x + 16,
                r.y + 198,
                sw - 32,
                &format!("Writes {name}.apng and {name}.wav to {}", ed.folder),
                11,
                s.muted,
            );
            text_button(
                p,
                s,
                Rect::new(r.x + sw as i32 - 176, r.y + 230, 76, 28),
                "Cancel",
                "close-sheet",
                false,
            );
            text_button(
                p,
                s,
                Rect::new(r.x + sw as i32 - 92, r.y + 230, 76, 28),
                if ed.product == Product::Kdenlive {
                    "Render"
                } else {
                    "Export"
                },
                "export-start",
                true,
            );
        }
    }
    p.z -= 10;
}

/// A one-line text field; `focused` shows the caret.
pub fn field(p: &mut Painter, s: &Skin, r: Rect, text: &str, focused: bool, command: &str) {
    p.node(
        r,
        Primitive::RoundedBox {
            fill: s.bg,
            border: Some(if focused { s.accent } else { s.line }),
            border_width: if focused { 2 } else { 1 },
            radius: s.radius,
        },
        Some((&target(command), "Name")),
    );
    let w = p.left(
        r.x + 8,
        r.y + (r.height as i32 - 17) / 2,
        r.width - 16,
        text,
        13,
        s.text,
    );
    if focused {
        p.box_(
            Rect::new(r.x + 9 + w as i32, r.y + 6, 1, r.height - 12),
            s.accent,
            0,
        );
    }
}

/// Export progress, when an export is running.
pub fn progress(p: &mut Painter, ed: &Editor, s: &Skin, r: Rect) {
    let Some(job) = &ed.export else {
        return;
    };
    p.border(r, s.panel, s.radius, s.line);
    p.left(
        r.x + 10,
        r.y + 6,
        r.width.saturating_sub(100),
        &format!("Exporting… {}%", job.percent()),
        12,
        s.text,
    );
    let bar = Rect::new(r.x + 10, r.y + 26, r.width.saturating_sub(100), 6);
    p.box_(bar, s.raised, 3);
    p.box_(
        Rect::new(bar.x, bar.y, bar.width * job.percent() / 100, 6),
        s.accent,
        3,
    );
    text_button(
        p,
        s,
        Rect::new(r.x + r.width as i32 - 82, r.y + 8, 72, 24),
        "Cancel",
        "export-cancel",
        false,
    );
}

/// Title styling controls for a selected title clip.
pub fn title_controls(
    p: &mut Painter,
    ed: &Editor,
    env: &AppEnv<'_>,
    s: &Skin,
    x: i32,
    mut y: i32,
    w: u32,
) -> i32 {
    let Some(c) = ed.selected_clip() else {
        return y;
    };
    let Source::Title(t) = &c.source else {
        return y;
    };
    if w >= 440 {
        return title_controls_wide(p, ed, s, t, x, y, w);
    }
    p.left(x, y, w, "Text", 12, s.muted);
    y += 18;
    field(
        p,
        s,
        Rect::new(x, y, w, 28),
        &t.text,
        ed.field == Field::Title,
        "title-text",
    );
    y += 36;
    let mut bx = x;
    for (label, cmd, on) in [
        ("B", "title-bold", t.bold),
        ("A−", "title-size:-2", false),
        ("A+", "title-size:2", false),
    ] {
        chip(p, s, Rect::new(bx, y, 36, 24), label, cmd, on);
        bx += 40;
    }
    y += 32;
    let mut bx = x;
    for hex in ["ffffff", "000000", "ffd60a", "ff453a", "0a84ff", "30d158"] {
        let r = Rect::new(bx, y, 22, 22);
        p.button(
            r,
            rgba(cw_raster::parse_hex(hex).unwrap_or(cw_raster::WHITE)),
            11,
            &target(&format!("title-color:{hex}")),
            &format!("Text colour #{hex}"),
        );
        if t.color == hex {
            p.border(
                Rect::new(bx - 2, y - 2, 26, 26),
                Color::TRANSPARENT,
                13,
                s.accent,
            );
        }
        bx += 28;
    }
    y += 30;
    let mut bx = x;
    for (label, pos) in [
        ("Top", "top"),
        ("Center", "center"),
        ("Lower third", "lower"),
        ("Bottom", "bottom"),
    ] {
        let cw = p.measure(label, 11, false) + 14;
        chip(
            p,
            s,
            Rect::new(bx, y, cw, 24),
            label,
            &format!("title-pos:{pos}"),
            t.position.id() == pos,
        );
        bx += cw as i32 + 4;
    }
    y += 32;
    let mut bx = x;
    for (label, bg) in [
        ("No box", "none"),
        ("Dark box", "000000b0"),
        ("Light box", "ffffffc0"),
    ] {
        let cw = p.measure(label, 11, false) + 14;
        let on = t.background.as_deref().unwrap_or("none") == bg;
        chip(
            p,
            s,
            Rect::new(bx, y, cw, 24),
            label,
            &format!("title-bg:{bg}"),
            on,
        );
        bx += cw as i32 + 4;
    }
    let _ = env;
    y + 32
}

/// The same title controls laid out across a wide strip, three rows deep.
fn title_controls_wide(
    p: &mut Painter,
    ed: &Editor,
    s: &Skin,
    t: &cw_video::Title,
    x: i32,
    mut y: i32,
    w: u32,
) -> i32 {
    field(
        p,
        s,
        Rect::new(x, y, w - 132, 28),
        &t.text,
        ed.field == Field::Title,
        "title-text",
    );
    let mut bx = x + w as i32 - 124;
    for (label, cmd, on) in [
        ("B", "title-bold", t.bold),
        ("A−", "title-size:-2", false),
        ("A+", "title-size:2", false),
    ] {
        chip(p, s, Rect::new(bx, y + 2, 36, 24), label, cmd, on);
        bx += 42;
    }
    y += 36;
    let mut bx = x;
    for hex in ["ffffff", "000000", "ffd60a", "ff453a", "0a84ff", "30d158"] {
        let r = Rect::new(bx, y + 1, 22, 22);
        p.button(
            r,
            rgba(cw_raster::parse_hex(hex).unwrap_or(cw_raster::WHITE)),
            11,
            &target(&format!("title-color:{hex}")),
            &format!("Text colour #{hex}"),
        );
        if t.color == hex {
            p.border(
                Rect::new(bx - 2, y - 1, 26, 26),
                Color::TRANSPARENT,
                13,
                s.accent,
            );
        }
        bx += 28;
    }
    bx += 12;
    for (label, pos) in [
        ("Top", "top"),
        ("Center", "center"),
        ("Lower third", "lower"),
        ("Bottom", "bottom"),
    ] {
        let cw = p.measure(label, 11, false) + 14;
        chip(
            p,
            s,
            Rect::new(bx, y, cw, 24),
            label,
            &format!("title-pos:{pos}"),
            t.position.id() == pos,
        );
        bx += cw as i32 + 4;
    }
    y += 32;
    let mut bx = x;
    for (label, bg) in [
        ("No box", "none"),
        ("Dark box", "000000b0"),
        ("Light box", "ffffffc0"),
    ] {
        let cw = p.measure(label, 11, false) + 14;
        let on = t.background.as_deref().unwrap_or("none") == bg;
        chip(
            p,
            s,
            Rect::new(bx, y, cw, 24),
            label,
            &format!("title-bg:{bg}"),
            on,
        );
        bx += cw as i32 + 4;
    }
    y + 32
}
