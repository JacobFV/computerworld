//! Rendering the timeline to files: an APNG of every frame at the chosen size and rate,
//! and a WAV of the audio mixdown. The work is done a few frames at a time, so a long
//! export advances with the simulation instead of stalling one step, and its progress
//! is a count of frames really encoded.
use crate::blob::Blob;
use crate::compose::Compositor;
use crate::project::{Library, Project};
use crate::{apng, audio, wav};
use cw_raster::transform;
use serde::{Deserialize, Serialize};

/// Pixels encoded per simulation step: four frames at 320x180.
pub const STEP_PIXELS: u64 = 4 * 320 * 180;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Export {
    /// Where the movie and its sound are written.
    pub video_path: String,
    pub audio_path: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// Frames the movie will have, and how many are encoded so far.
    pub total: u32,
    pub done: u32,
    /// Each encoded frame's compressed image data.
    #[serde(default)]
    pub frames: Vec<Blob>,
}

impl Export {
    pub fn new(
        p: &Project,
        video_path: &str,
        audio_path: &str,
        width: u32,
        height: u32,
        fps: u32,
    ) -> Result<Self, String> {
        if p.duration() == 0 {
            return Err("the timeline is empty".into());
        }
        cw_raster::check_size(width, height)?;
        if !(1..=60).contains(&fps) {
            return Err("frame rate is 1 to 60".into());
        }
        let den = i64::from(p.fps.max(1));
        let total = (p.duration() * i64::from(fps) + den - 1) / den;
        Ok(Self {
            video_path: video_path.into(),
            audio_path: audio_path.into(),
            width,
            height,
            fps,
            total: total.max(1) as u32,
            done: 0,
            frames: vec![],
        })
    }
    /// Timeline frame shown at output frame `i`.
    pub fn source_frame(&self, p: &Project, i: u32) -> i64 {
        i64::from(i) * i64::from(p.fps) / i64::from(self.fps)
    }
    pub fn finished(&self) -> bool {
        self.done >= self.total
    }
    pub fn percent(&self) -> u32 {
        self.done * 100 / self.total.max(1)
    }
    /// Encode the next frames, up to one step's worth of pixels.
    pub fn step(&mut self, comp: &Compositor<'_>) -> Result<(), String> {
        let per = (STEP_PIXELS / (u64::from(self.width) * u64::from(self.height)).max(1)).max(1);
        for _ in 0..per {
            if self.finished() {
                break;
            }
            let at = self.source_frame(comp.project, self.done);
            let mut frame = comp.frame(at);
            if frame.width() != self.width || frame.height() != self.height {
                frame = transform::resize(
                    &frame,
                    self.width,
                    self.height,
                    transform::Resample::Bilinear,
                );
            }
            self.frames.push(Blob::new(apng::frame_data(&frame)?));
            self.done += 1;
        }
        Ok(())
    }
    /// The finished movie and its sound.
    pub fn files(&self, p: &Project, library: &Library) -> Result<(Vec<u8>, Vec<u8>), String> {
        if !self.finished() {
            return Err("the export has not finished".into());
        }
        let refs: Vec<&[u8]> = self.frames.iter().map(Blob::bytes).collect();
        let fps = u16::try_from(self.fps).map_err(|_| "frame rate out of range")?;
        let movie = apng::assemble(self.width, self.height, 1, fps, &refs, 0);
        let sound = wav::encode(&wav::Pcm {
            rate: p.sample_rate,
            samples: audio::mixdown(p, library),
        });
        Ok((movie, sound))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compose::Masks;
    use crate::media::{Media, MediaKind};
    use crate::project::{Source, TrackKind, TransitionKind};
    use cw_raster::Canvas;

    #[test]
    fn an_export_reimports_frame_for_frame_and_sample_for_sample() {
        let mut p = Project::new("E", 32, 18, 12, 8000);
        let mut lib = Library::new();
        let frames: Vec<Canvas> = (0..12)
            .map(|i| {
                let mut c = Canvas::filled(32, 18, [20, 40, 60, 255]);
                c.set(i, i / 2, [250, 250, 0, 255]);
                c
            })
            .collect();
        let m = p.add_media("a.apng", MediaKind::Video);
        lib.insert(
            m,
            Media::import("a.apng", &apng::encode(&frames, 12).unwrap(), 8000).unwrap(),
        );
        let s = p.add_media("s.wav", MediaKind::Audio);
        lib.insert(
            s,
            Media::audio(
                "s.wav",
                (0..8000).map(|i| (i % 200) as i16 * 50).collect(),
                8000,
            ),
        );
        let v = p.tracks_of(TrackKind::Video)[0].id;
        let a = p.tracks_of(TrackKind::Audio)[0].id;
        let c1 = p.insert(&lib, Source::Media { media: m }, v, 0).unwrap();
        p.insert(
            &lib,
            Source::Color {
                color: "ff0000".into(),
            },
            v,
            12,
        )
        .unwrap();
        p.add_transition(c1, TransitionKind::CrossDissolve, 6)
            .unwrap();
        p.insert(&lib, Source::Media { media: s }, a, 0).unwrap();
        let masks = Masks::new();
        let comp = Compositor {
            project: &p,
            library: &lib,
            masks: &masks,
        };
        let mut job = Export::new(&p, "out.apng", "out.wav", 32, 18, 12).unwrap();
        assert_eq!(job.total, 60);
        let mut steps = 0;
        while !job.finished() {
            job.step(&comp).unwrap();
            steps += 1;
        }
        assert_eq!(steps, 1, "a tiny movie is one step's work");
        // A full-size one takes four frames a step.
        let mut big = Export::new(&p, "b.apng", "b.wav", 320, 180, 12).unwrap();
        big.step(&comp).unwrap();
        assert_eq!((big.done, big.percent()), (4, 6));
        let (movie, sound) = job.files(&p, &lib).unwrap();
        let back = Media::import("out.apng", &movie, 8000).unwrap();
        assert_eq!(back.frames.len(), 60);
        assert_eq!(back.duration_us, 5_000_000);
        for f in [0, 5, 9, 12, 14, 40] {
            assert_eq!(
                back.frame(f * 1_000_000 / 12).unwrap(),
                comp.frame(f),
                "frame {f}"
            );
        }
        let heard = wav::decode(&sound).unwrap();
        assert_eq!(heard.samples, audio::mixdown(&p, &lib));
        assert_eq!(heard.samples.len(), 5 * 8000);
        // Re-imported and laid on a fresh timeline, it plays back identically.
        let mut q = Project::new("R", 32, 18, 12, 8000);
        let mut lib2 = Library::new();
        let id = q.add_media("out.apng", MediaKind::Video);
        lib2.insert(id, back);
        let v2 = q.tracks_of(TrackKind::Video)[0].id;
        q.insert(&lib2, Source::Media { media: id }, v2, 0).unwrap();
        let replay = Compositor {
            project: &q,
            library: &lib2,
            masks: &masks,
        };
        for f in 0..60 {
            assert_eq!(replay.frame(f), comp.frame(f));
        }
        // Half size, half rate.
        let small = Export::new(&p, "s.apng", "s.wav", 16, 9, 6).unwrap();
        assert_eq!(small.total, 30);
        assert_eq!(small.source_frame(&p, 3), 6);
        assert!(Export::new(&Project::new("x", 8, 8, 12, 8000), "a", "b", 8, 8, 12).is_err());
    }
}
