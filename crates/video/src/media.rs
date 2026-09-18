//! Imported media: what an editor holds of a file after reading it. Frames are kept
//! losslessly (QOI) so any frame can be pulled at random, audio is kept as mono 16-bit
//! PCM at the project's sample rate, and small thumbnails and a waveform are computed
//! once at import so a timeline draws without decoding anything.
use crate::blob::Blob;
use crate::{apng, qoi, wav};
use cw_raster::{transform, Canvas};
use serde::{Deserialize, Serialize};

/// Height of a stored thumbnail, and how many a moving clip keeps.
pub const THUMB_HEIGHT: u32 = 36;
pub const THUMBS: usize = 8;
/// Longest edge a picture is kept at. Larger stills are reduced on import, the way an
/// editor makes an optimized copy; the file on disk is untouched.
pub const MAX_EDGE: u32 = 1280;
/// Waveform resolution: one peak per hundredth of a second.
pub const PEAKS_PER_SECOND: u32 = 100;
/// Longest audio an import keeps, in seconds.
pub const MAX_AUDIO_SECONDS: u32 = 600;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    /// Moving pictures (APNG).
    Video,
    /// One picture (PNG or JPEG) that lasts as long as its clip.
    Still,
    /// Sound (WAV).
    Audio,
}
impl MediaKind {
    pub fn visual(self) -> bool {
        self != Self::Audio
    }
}

/// Which extensions an import sheet offers, and what they hold.
pub fn kind_of(name: &str) -> Option<MediaKind> {
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit_once('.')?.1;
    match ext {
        "apng" => Some(MediaKind::Video),
        "png" | "jpg" | "jpeg" => Some(MediaKind::Still),
        "wav" | "wave" => Some(MediaKind::Audio),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Media {
    pub path: String,
    pub kind: MediaKind,
    pub width: u32,
    pub height: u32,
    /// QOI-coded frames, and when each starts (microseconds from the first).
    #[serde(default)]
    pub frames: Vec<Blob>,
    #[serde(default)]
    pub starts_us: Vec<u64>,
    /// Zero for a still, which has no length of its own.
    pub duration_us: u64,
    /// Little-endian mono 16-bit PCM at `rate`.
    #[serde(default)]
    pub pcm: Blob,
    #[serde(default)]
    pub rate: u32,
    /// Peak level 0..=255 per hundredth of a second.
    #[serde(default)]
    pub peaks: Blob,
    #[serde(default)]
    pub thumbs: Vec<Canvas>,
}

impl Media {
    /// Read `bytes` from `path`. Audio is resampled to `rate`.
    pub fn import(path: &str, bytes: &[u8], rate: u32) -> Result<Self, String> {
        if bytes.starts_with(&apng::SIGNATURE) {
            let decoded = apng::decode(bytes)?;
            return Ok(Self::pictures(path, decoded));
        }
        if bytes.starts_with(&[0xff, 0xd8]) {
            let canvas = decode_jpeg(bytes)?;
            return Ok(Self::pictures(
                path,
                apng::Decoded {
                    width: canvas.width(),
                    height: canvas.height(),
                    frames: vec![(canvas, (0, 1))],
                    animated: false,
                },
            ));
        }
        if bytes.starts_with(b"RIFF") {
            let pcm = wav::decode(bytes)?;
            let samples = wav::resample(&pcm.samples, pcm.rate, rate);
            if samples.len() > (rate * MAX_AUDIO_SECONDS) as usize {
                return Err("audio is longer than ten minutes".into());
            }
            return Ok(Self::audio(path, samples, rate));
        }
        Err("not a movie, picture or sound this editor can read".into())
    }

    fn pictures(path: &str, decoded: apng::Decoded) -> Self {
        let animated = decoded.animated && decoded.frames.len() > 1;
        let (mut w, mut h) = (decoded.width, decoded.height);
        let reduce = w.max(h) > MAX_EDGE;
        if reduce {
            let edge = w.max(h);
            w = (w * MAX_EDGE / edge).max(1);
            h = (h * MAX_EDGE / edge).max(1);
        }
        let mut frames = Vec::with_capacity(decoded.frames.len());
        let mut starts = Vec::with_capacity(decoded.frames.len());
        // Start times accumulate as an exact fraction, so a movie at 12 frames a second
        // lasts exactly as long as its frames say, however many there are.
        let (mut num, mut den) = (0u128, 1u128);
        let mut kept: Vec<Canvas> = Vec::new();
        for (canvas, delay) in decoded.frames {
            let canvas = if reduce {
                transform::resize(&canvas, w, h, transform::Resample::Bilinear)
            } else {
                canvas
            };
            starts.push((num * 1_000_000 / den) as u64);
            let (n, d) = (u128::from(delay.0), u128::from(delay.1.max(1)));
            num = num * d + n * den;
            den *= d;
            let g = gcd(num, den);
            (num, den) = (num / g, den / g);
            frames.push(Blob::new(qoi::encode(&canvas)));
            kept.push(canvas);
            if !animated {
                break;
            }
        }
        let thumbs = pick(kept.len())
            .into_iter()
            .map(|i| thumbnail(&kept[i]))
            .collect();
        Self {
            path: path.to_owned(),
            kind: if animated {
                MediaKind::Video
            } else {
                MediaKind::Still
            },
            width: w,
            height: h,
            frames,
            starts_us: starts,
            duration_us: if animated {
                (num * 1_000_000 / den) as u64
            } else {
                0
            },
            pcm: Blob::default(),
            rate: 0,
            peaks: Blob::default(),
            thumbs,
        }
    }

    pub fn audio(path: &str, samples: Vec<i16>, rate: u32) -> Self {
        // Bucket i covers samples [i·rate/100, (i+1)·rate/100): exactly a hundredth of a
        // second each, whatever the rate.
        let per = u64::from(PEAKS_PER_SECOND);
        let rate64 = u64::from(rate.max(1));
        let buckets = (samples.len() as u64 * per).div_ceil(rate64);
        let peaks = (0..buckets)
            .map(|i| {
                let a = (i * rate64 / per) as usize;
                let b = (((i + 1) * rate64 / per) as usize)
                    .min(samples.len())
                    .max(a + 1);
                let m = samples[a.min(samples.len() - 1)..b]
                    .iter()
                    .map(|s| i32::from(*s).abs())
                    .max()
                    .unwrap_or(0);
                (m >> 7).min(255) as u8
            })
            .collect();
        let duration_us = samples.len() as u64 * 1_000_000 / u64::from(rate.max(1));
        let mut pcm = Vec::with_capacity(samples.len() * 2);
        for s in &samples {
            pcm.extend_from_slice(&s.to_le_bytes());
        }
        Self {
            path: path.to_owned(),
            kind: MediaKind::Audio,
            width: 0,
            height: 0,
            frames: vec![],
            starts_us: vec![],
            duration_us,
            pcm: Blob::new(pcm),
            rate,
            peaks: Blob::new(peaks),
            thumbs: vec![],
        }
    }

    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }
    /// Length in microseconds, `None` for a still that lasts as long as it is asked to.
    pub fn length_us(&self) -> Option<u64> {
        (self.kind != MediaKind::Still).then_some(self.duration_us)
    }
    /// Index of the frame on screen at `us` (clamped into the media).
    pub fn frame_index(&self, us: i64) -> usize {
        if self.starts_us.len() <= 1 || us <= 0 {
            return 0;
        }
        let us = us as u64;
        match self.starts_us.binary_search(&us) {
            Ok(i) => i,
            Err(i) => i - 1,
        }
    }
    /// The frame on screen at `us`, decoded.
    pub fn frame(&self, us: i64) -> Option<Canvas> {
        let blob = self.frames.get(self.frame_index(us))?;
        qoi::decode(blob.bytes()).ok()
    }
    /// The stored thumbnail nearest `us`.
    pub fn thumb(&self, us: i64) -> Option<&Canvas> {
        if self.thumbs.is_empty() {
            return None;
        }
        let picks = pick(self.frames.len());
        let index = self.frame_index(us);
        let nearest = picks
            .iter()
            .enumerate()
            .min_by_key(|(_, f)| (**f as i64 - index as i64).abs())
            .map(|(i, _)| i)
            .unwrap_or(0);
        self.thumbs.get(nearest)
    }
    pub fn samples(&self) -> usize {
        self.pcm.len() / 2
    }
    #[inline]
    pub fn sample(&self, index: i64) -> i16 {
        if index < 0 {
            return 0;
        }
        let at = index as usize * 2;
        match self.pcm.bytes().get(at..at + 2) {
            Some(b) => i16::from_le_bytes([b[0], b[1]]),
            None => 0,
        }
    }
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a.max(1)
}

/// Which frames get a thumbnail: evenly spread, first and last included.
fn pick(count: usize) -> Vec<usize> {
    if count <= THUMBS {
        return (0..count).collect();
    }
    (0..THUMBS)
        .map(|i| i * (count - 1) / (THUMBS - 1))
        .collect()
}

fn thumbnail(c: &Canvas) -> Canvas {
    let h = THUMB_HEIGHT;
    let w = (c.width() * h / c.height().max(1)).clamp(1, 4 * h);
    transform::resize(c, w, h, transform::Resample::Bilinear)
}

/// Baseline and progressive JPEG to straight RGBA.
pub fn decode_jpeg(bytes: &[u8]) -> Result<Canvas, String> {
    let mut decoder = jpeg_decoder::Decoder::new(bytes);
    decoder.read_info().map_err(|e| e.to_string())?;
    let info = decoder.info().ok_or("JPEG has no header")?;
    let (w, h) = (u32::from(info.width), u32::from(info.height));
    cw_raster::check_size(w, h)?;
    let pixels = decoder.decode().map_err(|e| e.to_string())?;
    let rgba: Vec<u8> = match info.pixel_format {
        jpeg_decoder::PixelFormat::RGB24 => pixels
            .chunks(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        jpeg_decoder::PixelFormat::L8 => pixels.iter().flat_map(|g| [*g, *g, *g, 255]).collect(),
        jpeg_decoder::PixelFormat::L16 => pixels
            .chunks(2)
            .flat_map(|p| [p[0], p[0], p[0], 255])
            .collect(),
        jpeg_decoder::PixelFormat::CMYK32 => pixels
            .chunks(4)
            .flat_map(|p| {
                let k = u32::from(p[3]);
                let ch = |c: u8| ((u32::from(c) * k + 127) / 255) as u8;
                [ch(p[0]), ch(p[1]), ch(p[2]), 255]
            })
            .collect(),
    };
    Canvas::from_rgba(w, h, rgba)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_movie_imports_with_random_access_frames_and_thumbnails() {
        let frames: Vec<Canvas> = (0..10)
            .map(|i| Canvas::filled(32, 18, [i * 20, 0, 0, 255]))
            .collect();
        let bytes = apng::encode(&frames, 10).unwrap();
        let media = Media::import("/m/a.apng", &bytes, 22_050).unwrap();
        assert_eq!(media.kind, MediaKind::Video);
        assert_eq!(media.name(), "a.apng");
        assert_eq!(media.duration_us, 1_000_000);
        assert_eq!(media.frame_index(0), 0);
        assert_eq!(media.frame_index(99_999), 0);
        assert_eq!(media.frame_index(100_000), 1);
        assert_eq!(media.frame_index(5_000_000), 9);
        assert_eq!(media.frame(350_000).unwrap(), frames[3]);
        assert_eq!(media.thumbs.len(), THUMBS);
        assert_eq!(media.thumbs[0].height(), THUMB_HEIGHT);
        let json = serde_json::to_string(&media).unwrap();
        assert_eq!(serde_json::from_str::<Media>(&json).unwrap(), media);
    }
    #[test]
    fn stills_and_sounds_import_and_other_files_are_refused() {
        let still = apng::encode_png(&Canvas::filled(4, 2, [1, 2, 3, 255])).unwrap();
        let media = Media::import("p.png", &still, 22_050).unwrap();
        assert_eq!(media.kind, MediaKind::Still);
        assert_eq!(media.length_us(), None);
        let sound = wav::encode(&wav::Pcm {
            rate: 11_025,
            samples: vec![1000; 11_025],
        });
        let media = Media::import("s.wav", &sound, 22_050).unwrap();
        assert_eq!(media.kind, MediaKind::Audio);
        assert_eq!(media.samples(), 22_050);
        assert_eq!(media.duration_us, 1_000_000);
        assert_eq!(media.sample(5), 1000);
        assert_eq!(media.peaks.len(), 100);
        assert_eq!(media.peaks.bytes()[0], 7);
        assert!(Media::import("x.txt", b"hello", 22_050).is_err());
        assert_eq!(kind_of("Clip.APNG"), Some(MediaKind::Video));
        assert_eq!(kind_of("notes.txt"), None);
    }
}
