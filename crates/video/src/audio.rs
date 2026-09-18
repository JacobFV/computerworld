//! Sound: every unmuted audio track mixed to one mono 16-bit stream, sample for sample.
//! A clip's samples are read at its speed (faster raises the pitch, as a tape would),
//! backwards when reversed, scaled by its volume keyframes and its fades, and summed
//! with every other track's; the sum is clipped to the 16-bit range.
use crate::project::{Library, Project, TrackKind};

/// `count` mixed samples starting at timeline sample `from`.
pub fn mix(p: &Project, library: &Library, from: i64, count: usize) -> Vec<i16> {
    let mut acc = vec![0i64; count];
    let to = from + count as i64;
    let fps = i64::from(p.fps.max(1));
    let rate = i64::from(p.sample_rate.max(1));
    for track in p.tracks_of(TrackKind::Audio) {
        if track.muted {
            continue;
        }
        for clip in p.on_track(track.id) {
            let Some(media) = clip.media().and_then(|m| library.get(&m)) else {
                continue;
            };
            let (cs, ce) = (p.frame_sample(clip.start), p.frame_sample(clip.end()));
            let (lo, hi) = (from.max(cs), to.min(ce));
            if lo >= hi {
                continue;
            }
            let speed = i64::from(clip.speed);
            let first = p.sample_of(clip.offset);
            let last = p.sample_of(clip.offset + clip.span()) - 1;
            let total = ce - cs;
            let fade_in = p.frame_sample(i64::from(clip.fade_in));
            let fade_out = p.frame_sample(i64::from(clip.fade_out));
            for s in lo..hi {
                let ls = s - cs;
                let step = ls * speed / 100;
                let index = if clip.reverse {
                    last - step
                } else {
                    first + step
                };
                let v = i64::from(media.sample(index));
                if v == 0 {
                    continue;
                }
                let volume = i64::from(clip.volume.at(ls * fps / rate).clamp(0, 400));
                let mut f = 1000;
                if fade_in > 0 && ls < fade_in {
                    f = f.min(ls * 1000 / fade_in);
                }
                let left = total - 1 - ls;
                if fade_out > 0 && left < fade_out {
                    f = f.min(left * 1000 / fade_out);
                }
                acc[(s - from) as usize] += v * volume * f / 100_000;
            }
        }
    }
    acc.into_iter()
        .map(|v| v.clamp(i64::from(i16::MIN), i64::from(i16::MAX)) as i16)
        .collect()
}

/// The whole programme, from the first frame to the end of the last clip.
pub fn mixdown(p: &Project, library: &Library) -> Vec<i16> {
    let end = p.frame_sample(p.duration());
    mix(p, library, 0, end.max(0) as usize)
}

/// Peak level 0..=255 of the mix during timeline frame `frame`: what a level meter shows.
pub fn level(p: &Project, library: &Library, frame: i64) -> u8 {
    let (a, b) = (p.frame_sample(frame), p.frame_sample(frame + 1));
    mix(p, library, a, (b - a).max(0) as usize)
        .iter()
        .map(|s| i32::from(*s).abs())
        .max()
        .map_or(0, |m| (m >> 7).min(255) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::{Media, MediaKind};
    use crate::project::{Edge, Source};

    fn setup() -> (Project, Library, u32, u32) {
        // 10 fps and 100 samples a second: ten samples a frame.
        let mut p = Project::new("A", 16, 8, 10, 100);
        let mut lib = Library::new();
        let ramp: Vec<i16> = (0..100).map(|i| i * 10).collect();
        let tone = vec![1000i16; 100];
        let a = p.add_media("ramp.wav", MediaKind::Audio);
        let b = p.add_media("tone.wav", MediaKind::Audio);
        lib.insert(a, Media::audio("ramp.wav", ramp, 100));
        lib.insert(b, Media::audio("tone.wav", tone, 100));
        (p, lib, a, b)
    }

    #[test]
    fn tracks_sum_sample_for_sample_and_mute_removes_one() {
        let (mut p, lib, a, b) = setup();
        let t1 = p.tracks_of(TrackKind::Audio)[0].id;
        let t2 = p.add_track(TrackKind::Audio).unwrap();
        p.insert(&lib, Source::Media { media: a }, t1, 0).unwrap();
        p.insert(&lib, Source::Media { media: b }, t2, 2).unwrap();
        let out = mixdown(&p, &lib);
        assert_eq!(out.len(), 120);
        assert_eq!(out[5], 50);
        assert_eq!(out[25], 250 + 1000);
        assert_eq!(out[110], 1000);
        p.track_mut(t2).unwrap().muted = true;
        assert_eq!(mix(&p, &lib, 25, 1), vec![250]);
        assert_eq!(level(&p, &lib, 0), 0);
    }

    #[test]
    fn speed_reverse_volume_and_fades_shape_the_samples() {
        let (mut p, lib, a, _) = setup();
        let t1 = p.tracks_of(TrackKind::Audio)[0].id;
        let c = p.insert(&lib, Source::Media { media: a }, t1, 0).unwrap();
        p.set_speed(c, 200, &lib).unwrap();
        let out = mixdown(&p, &lib);
        assert_eq!(out.len(), 50);
        assert_eq!(&out[..3], &[0, 20, 40]);
        p.set_reverse(c, true).unwrap();
        let out = mixdown(&p, &lib);
        assert_eq!(&out[..3], &[990, 970, 950]);
        assert_eq!(out[49], 10);
        p.set_reverse(c, false).unwrap();
        p.set_speed(c, 100, &lib).unwrap();
        p.clip_mut(c).unwrap().volume = crate::project::Param::fixed(50);
        assert_eq!(mix(&p, &lib, 40, 1), vec![200]);
        p.clip_mut(c).unwrap().volume = crate::project::Param::fixed(100);
        p.clip_mut(c).unwrap().fade_in = 2; // twenty samples
        assert_eq!(mix(&p, &lib, 10, 1), vec![50]);
        p.clip_mut(c).unwrap().fade_out = 1; // last ten samples
        assert_eq!(mix(&p, &lib, 99, 1), vec![0]);
        assert_eq!(mix(&p, &lib, 95, 1), vec![380]);
        // A trimmed clip starts later in its source.
        p.clip_mut(c).unwrap().fade_in = 0;
        p.trim(c, Edge::In, 3, &lib).unwrap();
        assert_eq!(mix(&p, &lib, 30, 1), vec![300]);
        assert_eq!(mix(&p, &lib, 29, 1), vec![0]);
    }

    #[test]
    fn the_sum_clips_instead_of_wrapping() {
        let (mut p, _, _, _) = setup();
        let mut lib = Library::new();
        let loud = p.add_media("loud.wav", MediaKind::Audio);
        lib.insert(loud, Media::audio("loud.wav", vec![30_000; 100], 100));
        let t1 = p.tracks_of(TrackKind::Audio)[0].id;
        let t2 = p.add_track(TrackKind::Audio).unwrap();
        p.insert(&lib, Source::Media { media: loud }, t1, 0)
            .unwrap();
        p.insert(&lib, Source::Media { media: loud }, t2, 0)
            .unwrap();
        assert_eq!(mix(&p, &lib, 0, 1), vec![32_767]);
    }
}
