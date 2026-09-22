//! RIFF WAVE audio with linear PCM samples, the format every audio track in the
//! simulation is stored and mixed down to.
//!
//! Reading accepts integer PCM of 8, 16, 24 or 32 bits in any number of channels
//! (`WAVE_FORMAT_PCM`, or `WAVE_FORMAT_EXTENSIBLE` carrying the PCM sub-format) and
//! reduces it to mono 16-bit, averaging the channels. Writing produces canonical 16-bit
//! PCM with a 44-byte header.

/// Decoded audio: mono signed 16-bit samples at `rate` per second.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pcm {
    pub rate: u32,
    pub samples: Vec<i16>,
}

/// A file must not be able to ask for an unbounded buffer: ten minutes at 48 kHz.
pub const MAX_SAMPLES: usize = 48_000 * 600;

fn le16(d: &[u8], at: usize) -> Result<u16, String> {
    d.get(at..at + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or_else(|| "truncated WAV header".into())
}
fn le32(d: &[u8], at: usize) -> Result<u32, String> {
    d.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| "truncated WAV header".into())
}

pub fn decode(bytes: &[u8]) -> Result<Pcm, String> {
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a WAV file".into());
    }
    let mut at = 12;
    let mut format: Option<(u16, u16, u32, u16)> = None;
    let mut data: Option<&[u8]> = None;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let len = le32(bytes, at + 4)? as usize;
        let body = &bytes[at + 8..(at + 8).saturating_add(len).min(bytes.len())];
        match id {
            b"fmt " => {
                let mut tag = le16(body, 0)?;
                if tag == 0xfffe {
                    // WAVE_FORMAT_EXTENSIBLE: the sub-format GUID starts with the tag.
                    tag = le16(body, 24)?;
                }
                format = Some((tag, le16(body, 2)?, le32(body, 4)?, le16(body, 14)?));
            }
            b"data" => data = Some(body),
            _ => {}
        }
        // Chunks are padded to an even length.
        at = at.saturating_add(8).saturating_add(len + (len & 1));
    }
    let (tag, channels, rate, bits) = format.ok_or("WAV file has no format chunk")?;
    let data = data.ok_or("WAV file has no data chunk")?;
    if tag != 1 {
        return Err("only linear PCM WAV audio is supported".into());
    }
    if channels == 0 || !(1..=384_000).contains(&rate) {
        return Err("WAV format is invalid".into());
    }
    if !matches!(bits, 8 | 16 | 24 | 32) {
        return Err(format!("{bits}-bit WAV audio is not supported"));
    }
    let width = usize::from(bits / 8);
    let frame = width * usize::from(channels);
    let frames = data.len() / frame;
    if frames > MAX_SAMPLES {
        return Err("WAV file is too long".into());
    }
    let sample = |b: &[u8]| -> i32 {
        match width {
            1 => (i32::from(b[0]) - 128) << 8,
            2 => i32::from(i16::from_le_bytes([b[0], b[1]])),
            3 => i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 16,
            _ => i32::from_le_bytes([b[0], b[1], b[2], b[3]]) >> 16,
        }
    };
    let samples = data
        .chunks_exact(frame)
        .map(|f| {
            let sum: i32 = f.chunks_exact(width).map(sample).sum();
            (sum / i32::from(channels)) as i16
        })
        .collect();
    Ok(Pcm { rate, samples })
}

/// Canonical 16-bit PCM WAV with `channels` interleaved channels.
pub fn encode_channels(rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + samples.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2 * u32::from(channels)).to_le_bytes());
    out.extend_from_slice(&(2 * channels).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// Mono 16-bit PCM WAV.
pub fn encode(pcm: &Pcm) -> Vec<u8> {
    encode_channels(pcm.rate, 1, &pcm.samples)
}

/// Resample by linear interpolation in exact integer arithmetic.
pub fn resample(samples: &[i16], from: u32, to: u32) -> Vec<i16> {
    if from == to || samples.is_empty() {
        return samples.to_vec();
    }
    let (from, to) = (u64::from(from), u64::from(to));
    let count = (samples.len() as u64 * to).div_ceil(from) as usize;
    (0..count)
        .map(|i| {
            let pos = i as u64 * from;
            let idx = (pos / to) as usize;
            let frac = (pos % to) as i64;
            let a = i64::from(samples[idx.min(samples.len() - 1)]);
            let b = i64::from(samples[(idx + 1).min(samples.len() - 1)]);
            let num = a * (to as i64 - frac) + b * frac;
            // Round half away from zero, the same way on every target.
            let den = to as i64;
            let q = if num >= 0 {
                (num + den / 2) / den
            } else {
                -((-num + den / 2) / den)
            };
            q.clamp(-32768, 32767) as i16
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_written_file_reads_back_sample_for_sample() {
        let pcm = Pcm {
            rate: 22_050,
            samples: vec![0, 1, -1, 32767, -32768, 1234, -4321],
        };
        let bytes = encode(&pcm);
        assert_eq!(bytes.len(), 44 + 14);
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(decode(&bytes).unwrap(), pcm);
    }
    #[test]
    fn other_layouts_reduce_to_mono_sixteen_bit() {
        // Stereo 16-bit: channels are averaged.
        let stereo = encode_channels(8000, 2, &[100, 300, -50, -150]);
        assert_eq!(decode(&stereo).unwrap().samples, vec![200, -100]);
        // 8-bit unsigned: 128 is silence.
        let mut eight = encode_channels(8000, 1, &[]);
        eight[34] = 8; // bits per sample
        eight[32] = 1; // block align
        eight.extend_from_slice(&[128, 255, 0]);
        let n = eight.len() as u32 - 44;
        eight[40..44].copy_from_slice(&n.to_le_bytes());
        assert_eq!(
            decode(&eight).unwrap().samples,
            vec![0, 127 << 8, -128 << 8]
        );
        assert!(decode(b"RIFF\0\0\0\0WAVE").is_err());
        assert!(decode(b"not audio at all").is_err());
    }
    #[test]
    fn resampling_is_exact_on_integer_ratios() {
        assert_eq!(
            resample(&[0, 100, 200], 1, 2),
            vec![0, 50, 100, 150, 200, 200]
        );
        assert_eq!(resample(&[0, 100, 200, 300], 2, 1), vec![0, 200]);
        assert_eq!(resample(&[5, 6], 44_100, 44_100), vec![5, 6]);
    }
}
