//! Animated PNG (the APNG extension to PNG 1.2, as registered in PNG Third Edition):
//! the container every video in the simulation is stored in. A plain PNG reader shows
//! the first frame, a browser plays it, and nothing about it is private to this crate.
//!
//! Decoding walks the chunks itself and hands each frame's compressed data to the `png`
//! crate as a standalone image, then composes frames with the dispose and blend
//! operations the file names, so any conforming APNG plays as its author made it.
//! Encoding writes every frame full size with `APNG_BLEND_OP_SOURCE`, so each frame of
//! an exported movie is independently decodable.
use cw_raster::{BlendMode, Canvas};

pub const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
/// A malformed file must not be able to ask for unbounded work.
pub const MAX_FRAMES: usize = 20_000;

/// One chunk: its four-letter type and its data.
pub struct Chunk<'a> {
    pub kind: [u8; 4],
    pub data: &'a [u8],
}

/// Split a PNG stream into chunks, checking every CRC.
pub fn chunks(bytes: &[u8]) -> Result<Vec<Chunk<'_>>, String> {
    if bytes.len() < 8 || bytes[..8] != SIGNATURE {
        return Err("not a PNG file".into());
    }
    let mut at = 8;
    let mut out = Vec::new();
    while at + 12 <= bytes.len() {
        let len =
            u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as usize;
        let end = at
            .checked_add(12)
            .and_then(|v| v.checked_add(len))
            .filter(|end| *end <= bytes.len())
            .ok_or("truncated PNG chunk")?;
        let kind = [bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]];
        let data = &bytes[at + 8..at + 8 + len];
        let crc = u32::from_be_bytes([
            bytes[end - 4],
            bytes[end - 3],
            bytes[end - 2],
            bytes[end - 1],
        ]);
        if crc32(&kind, data) != crc {
            return Err(format!(
                "corrupt PNG chunk {}",
                String::from_utf8_lossy(&kind)
            ));
        }
        out.push(Chunk { kind, data });
        at = end;
        if &kind == b"IEND" {
            return Ok(out);
        }
    }
    Err("PNG file has no end".into())
}

fn crc32(kind: &[u8], data: &[u8]) -> u32 {
    let mut h = crc32fast::Hasher::new();
    h.update(kind);
    h.update(data);
    h.finalize()
}

fn write_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    out.extend_from_slice(&crc32(kind, data).to_be_bytes());
}

fn be32(d: &[u8], at: usize) -> Result<u32, String> {
    d.get(at..at + 4)
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| "truncated PNG field".into())
}
fn be16(d: &[u8], at: usize) -> Result<u16, String> {
    d.get(at..at + 2)
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
        .ok_or_else(|| "truncated PNG field".into())
}

/// A decoded moving picture: its canvas size and each frame with how long it shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decoded {
    pub width: u32,
    pub height: u32,
    /// Full-canvas frames and how long each shows, as a fraction of a second
    /// `(numerator, denominator)`; a still's is `(0, 1)`.
    pub frames: Vec<(Canvas, (u32, u32))>,
    /// Whether the file carried an animation control chunk.
    pub animated: bool,
}

struct FrameControl {
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    delay: (u32, u32),
    dispose: u8,
    blend: u8,
}
impl FrameControl {
    fn parse(d: &[u8]) -> Result<Self, String> {
        let num = u32::from(be16(d, 20)?);
        let den = match be16(d, 22)? {
            0 => 100,
            v => u32::from(v),
        };
        // Players treat a delay under 10 ms as "as fast as you can"; a movie needs a
        // duration, so it gets the 10 ms floor browsers apply.
        let delay = if num * 100 < den {
            (1, 100)
        } else {
            (num, den)
        };
        Ok(Self {
            width: be32(d, 4)?,
            height: be32(d, 8)?,
            x: be32(d, 12)?,
            y: be32(d, 16)?,
            delay,
            dispose: *d.get(24).ok_or("truncated fcTL")?,
            blend: *d.get(25).ok_or("truncated fcTL")?,
        })
    }
}

/// Decode a standalone PNG image (any colour type and depth) to straight RGBA.
pub fn decode_png(bytes: &[u8]) -> Result<Canvas, String> {
    let mut decoder = png::Decoder::new(bytes);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let size = reader.output_buffer_size();
    if size > (cw_raster::MAX_PIXELS as usize) * 4 {
        return Err("image is too large to decode".into());
    }
    let mut buffer = vec![0; size];
    let info = reader.next_frame(&mut buffer).map_err(|e| e.to_string())?;
    buffer.truncate(info.buffer_size());
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => buffer,
        png::ColorType::Rgb => buffer
            .chunks(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::Grayscale => buffer.iter().flat_map(|g| [*g, *g, *g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => buffer
            .chunks(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Indexed => return Err("palette image was not expanded".into()),
    };
    Canvas::from_rgba(info.width, info.height, rgba)
}

/// Decode a PNG or APNG. A still PNG is one frame with no duration of its own (zero).
pub fn decode(bytes: &[u8]) -> Result<Decoded, String> {
    let list = chunks(bytes)?;
    let ihdr = list
        .iter()
        .find(|c| &c.kind == b"IHDR")
        .ok_or("PNG has no header")?
        .data;
    let (width, height) = (be32(ihdr, 0)?, be32(ihdr, 4)?);
    cw_raster::check_size(width, height)?;
    let animated = list.iter().any(|c| &c.kind == b"acTL");
    if !animated {
        return Ok(Decoded {
            width,
            height,
            frames: vec![(decode_png(bytes)?, (0, 1))],
            animated: false,
        });
    }
    // Chunks a frame needs to be decoded on its own.
    let shared: Vec<&Chunk> = list
        .iter()
        .filter(|c| matches!(&c.kind, b"PLTE" | b"tRNS"))
        .collect();
    let mut frames: Vec<(FrameControl, Vec<u8>)> = Vec::new();
    let mut open = false;
    for c in &list {
        match &c.kind {
            b"fcTL" => {
                if frames.len() >= MAX_FRAMES {
                    return Err("animation has too many frames".into());
                }
                frames.push((FrameControl::parse(c.data)?, Vec::new()));
                open = true;
            }
            // The default image is the first frame only when a frame control precedes it.
            b"IDAT" if open => frames.last_mut().unwrap().1.extend_from_slice(c.data),
            b"fdAT" if open => frames
                .last_mut()
                .unwrap()
                .1
                .extend_from_slice(c.data.get(4..).ok_or("truncated fdAT")?),
            _ => {}
        }
    }
    let mut canvas = Canvas::new(width, height);
    let mut out = Vec::with_capacity(frames.len());
    for (control, data) in &frames {
        if control.width == 0
            || control.height == 0
            || u64::from(control.x) + u64::from(control.width) > u64::from(width)
            || u64::from(control.y) + u64::from(control.height) > u64::from(height)
        {
            return Err("animation frame lies outside the image".into());
        }
        let mut header = ihdr.to_vec();
        header[0..4].copy_from_slice(&control.width.to_be_bytes());
        header[4..8].copy_from_slice(&control.height.to_be_bytes());
        let mut png = SIGNATURE.to_vec();
        write_chunk(&mut png, b"IHDR", &header);
        for c in &shared {
            write_chunk(&mut png, &c.kind, c.data);
        }
        write_chunk(&mut png, b"IDAT", data);
        write_chunk(&mut png, b"IEND", &[]);
        let image = decode_png(&png)?;
        let saved = (control.dispose == 2).then(|| canvas.clone());
        let (x, y) = (control.x as i32, control.y as i32);
        if control.blend == 0 {
            canvas.put(x, y, &image);
        } else {
            canvas.draw(x, y, &image, BlendMode::Normal, 255);
        }
        out.push((canvas.clone(), control.delay));
        match control.dispose {
            1 => canvas.put(x, y, &Canvas::new(control.width, control.height)),
            2 => canvas = saved.unwrap(),
            _ => {}
        }
    }
    if out.is_empty() {
        return Err("animation has no frames".into());
    }
    Ok(Decoded {
        width,
        height,
        frames: out,
        animated: true,
    })
}

/// Compress one full frame into the zlib stream of an `IDAT`/`fdAT` payload.
pub fn frame_data(canvas: &Canvas) -> Result<Vec<u8>, String> {
    let png = encode_png(canvas)?;
    let mut data = Vec::new();
    for c in chunks(&png)? {
        if &c.kind == b"IDAT" {
            data.extend_from_slice(c.data);
        }
    }
    Ok(data)
}

/// Encode a canvas as an ordinary RGBA PNG.
pub fn encode_png(canvas: &Canvas) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, canvas.width(), canvas.height());
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Default);
        encoder.set_adaptive_filter(png::AdaptiveFilterType::Adaptive);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer
            .write_image_data(canvas.pixels())
            .map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// Assemble an APNG from frames already compressed by [`frame_data`]. Every frame is
/// the full canvas, shown for `delay_num / delay_den` seconds; `plays` 0 loops forever.
pub fn assemble(
    width: u32,
    height: u32,
    delay_num: u16,
    delay_den: u16,
    frames: &[&[u8]],
    plays: u32,
) -> Vec<u8> {
    let mut out = SIGNATURE.to_vec();
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA, deflate, adaptive, no interlace
    write_chunk(&mut out, b"IHDR", &ihdr);
    let mut actl = Vec::with_capacity(8);
    actl.extend_from_slice(&(frames.len() as u32).to_be_bytes());
    actl.extend_from_slice(&plays.to_be_bytes());
    write_chunk(&mut out, b"acTL", &actl);
    let mut sequence = 0u32;
    for (i, data) in frames.iter().enumerate() {
        let mut fctl = Vec::with_capacity(26);
        fctl.extend_from_slice(&sequence.to_be_bytes());
        sequence += 1;
        fctl.extend_from_slice(&width.to_be_bytes());
        fctl.extend_from_slice(&height.to_be_bytes());
        fctl.extend_from_slice(&0u32.to_be_bytes());
        fctl.extend_from_slice(&0u32.to_be_bytes());
        fctl.extend_from_slice(&delay_num.to_be_bytes());
        fctl.extend_from_slice(&delay_den.to_be_bytes());
        fctl.push(0); // APNG_DISPOSE_OP_NONE
        fctl.push(0); // APNG_BLEND_OP_SOURCE
        write_chunk(&mut out, b"fcTL", &fctl);
        if i == 0 {
            write_chunk(&mut out, b"IDAT", data);
        } else {
            let mut fdat = Vec::with_capacity(data.len() + 4);
            fdat.extend_from_slice(&sequence.to_be_bytes());
            sequence += 1;
            fdat.extend_from_slice(data);
            write_chunk(&mut out, b"fdAT", &fdat);
        }
    }
    write_chunk(&mut out, b"IEND", &[]);
    out
}

/// Encode whole frames at a constant frame rate.
pub fn encode(frames: &[Canvas], fps: u16) -> Result<Vec<u8>, String> {
    let first = frames.first().ok_or("a movie needs at least one frame")?;
    let data = frames
        .iter()
        .map(|f| {
            if f.width() != first.width() || f.height() != first.height() {
                return Err("every frame of a movie has the same size".to_owned());
            }
            frame_data(f)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let refs: Vec<&[u8]> = data.iter().map(Vec::as_slice).collect();
    Ok(assemble(first.width(), first.height(), 1, fps, &refs, 0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cw_raster::blend;
    fn frame(i: u8) -> Canvas {
        let mut c = Canvas::filled(24, 16, [i * 40, 100, 255 - i * 40, 255]);
        c.set(i as i32, 3, [255, 255, 255, 128]);
        c
    }
    #[test]
    fn an_encoded_movie_decodes_to_the_same_frames_and_timing() {
        let frames: Vec<Canvas> = (0..5).map(frame).collect();
        let bytes = encode(&frames, 12).unwrap();
        // Well-formed: signature, header, animation control, one fcTL per frame.
        let list = chunks(&bytes).unwrap();
        assert_eq!(&list[1].kind, b"acTL");
        assert_eq!(list.iter().filter(|c| &c.kind == b"fcTL").count(), 5);
        let decoded = decode(&bytes).unwrap();
        assert!(decoded.animated);
        assert_eq!((decoded.width, decoded.height), (24, 16));
        assert_eq!(decoded.frames.len(), 5);
        for (i, (canvas, delay)) in decoded.frames.iter().enumerate() {
            assert_eq!(canvas, &frames[i]);
            assert_eq!(*delay, (1, 12));
        }
        // Encoding is a pure function of the frames.
        assert_eq!(encode(&frames, 12).unwrap(), bytes);
        // A still PNG is one frame, and a plain PNG reader sees the first frame.
        assert_eq!(decode_png(&bytes).unwrap(), frames[0]);
        let still = decode(&encode_png(&frames[2]).unwrap()).unwrap();
        assert!(!still.animated);
        assert_eq!(still.frames, vec![(frames[2].clone(), (0, 1))]);
    }
    #[test]
    fn partial_frames_compose_with_their_blend_and_dispose_operations() {
        // Hand-build: a 4x4 red base, then a 2x2 half-transparent blue OVER at (1,1)
        // disposed to background, then an empty-looking 1x1 frame.
        let base = Canvas::filled(4, 4, [255, 0, 0, 255]);
        let patch = Canvas::filled(2, 2, [0, 0, 255, 128]);
        let dot = Canvas::filled(1, 1, [0, 255, 0, 255]);
        let mut out = SIGNATURE.to_vec();
        let mut ihdr = vec![];
        ihdr.extend_from_slice(&4u32.to_be_bytes());
        ihdr.extend_from_slice(&4u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        write_chunk(&mut out, b"IHDR", &ihdr);
        write_chunk(&mut out, b"acTL", &[0, 0, 0, 3, 0, 0, 0, 0]);
        let mut seq = 0u32;
        for (i, (img, x, y, dispose, blend)) in [
            (&base, 0u32, 0u32, 0u8, 0u8),
            (&patch, 1, 1, 1, 1),
            (&dot, 3, 3, 0, 0),
        ]
        .into_iter()
        .enumerate()
        {
            let mut f = vec![];
            f.extend_from_slice(&seq.to_be_bytes());
            seq += 1;
            for v in [img.width(), img.height(), x, y] {
                f.extend_from_slice(&v.to_be_bytes());
            }
            f.extend_from_slice(&[0, 1, 0, 10, dispose, blend]);
            write_chunk(&mut out, b"fcTL", &f);
            let data = frame_data(img).unwrap();
            if i == 0 {
                write_chunk(&mut out, b"IDAT", &data);
            } else {
                let mut d = seq.to_be_bytes().to_vec();
                seq += 1;
                d.extend_from_slice(&data);
                write_chunk(&mut out, b"fdAT", &d);
            }
        }
        write_chunk(&mut out, b"IEND", &[]);
        let decoded = decode(&out).unwrap();
        assert_eq!(decoded.frames.len(), 3);
        assert_eq!(decoded.frames[0].1, (1, 10));
        let over = decoded.frames[1].0.get(1, 1);
        assert_eq!(
            over,
            blend::composite([255, 0, 0, 255], [0, 0, 255, 128], 255, BlendMode::Normal)
        );
        // Frame 2 sees the patch area cleared to transparent by the dispose op.
        assert_eq!(decoded.frames[2].0.get(1, 1), [0, 0, 0, 0]);
        assert_eq!(decoded.frames[2].0.get(0, 0), [255, 0, 0, 255]);
        assert_eq!(decoded.frames[2].0.get(3, 3), [0, 255, 0, 255]);
        // Corruption is detected rather than decoded into garbage.
        let mut bad = out.clone();
        let n = bad.len();
        bad[n - 20] ^= 0xff;
        assert!(decode(&bad).is_err());
    }
}
