//! Windows bitmaps. Paint saves "24-bit Bitmap": a BITMAPINFOHEADER, bottom-up rows of
//! blue-green-red padded to four bytes, transparency flattened over white. Reading takes
//! what Windows programs write: 1, 4, 8 bit palettes, 24 bit, 32 bit (BI_RGB or
//! BI_BITFIELDS), bottom-up or top-down.
use crate::{check_size, Canvas};

fn u16le(b: &[u8], at: usize) -> Result<u16, String> {
    b.get(at..at + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
        .ok_or_else(|| "the bitmap ends early".into())
}
fn u32le(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| "the bitmap ends early".into())
}

/// A 24-bit bottom-up bitmap of `canvas`, alpha composited over white.
pub fn encode(canvas: &Canvas) -> Vec<u8> {
    let (w, h) = (canvas.width(), canvas.height());
    let stride = (w as usize * 3).div_ceil(4) * 4;
    let image = stride * h as usize;
    let mut out = Vec::with_capacity(54 + image);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&((54 + image) as u32).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(h as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(image as u32).to_le_bytes());
    // 96 dpi, in pixels per metre.
    out.extend_from_slice(&3780u32.to_le_bytes());
    out.extend_from_slice(&3780u32.to_le_bytes());
    out.extend_from_slice(&[0; 8]);
    for y in (0..h as i32).rev() {
        let start = out.len();
        for x in 0..w as i32 {
            let p = canvas.get(x, y);
            let a = u32::from(p[3]);
            let over = |c: u8| crate::fmath::div255(u32::from(c) * a + 255 * (255 - a)) as u8;
            out.extend_from_slice(&[over(p[2]), over(p[1]), over(p[0])]);
        }
        out.resize(start + stride, 0);
    }
    out
}

/// Decode a bitmap file to a canvas.
pub fn decode(b: &[u8]) -> Result<Canvas, String> {
    if b.get(..2) != Some(b"BM") {
        return Err("not a bitmap".into());
    }
    let data_at = u32le(b, 10)? as usize;
    let header = u32le(b, 14)? as usize;
    if header < 40 {
        return Err("OS/2 bitmaps are not supported".into());
    }
    let width = u32le(b, 18)? as i32;
    let raw_height = u32le(b, 22)? as i32;
    let bpp = u16le(b, 28)?;
    let compression = u32le(b, 30)?;
    let top_down = raw_height < 0;
    let height = raw_height.unsigned_abs();
    if width <= 0 {
        return Err("the bitmap has no width".into());
    }
    let width = width as u32;
    check_size(width, height)?;
    let masks = match (compression, bpp) {
        (0, _) => None,
        (3, 32) | (3, 16) => {
            // Masks follow a 40-byte header, or sit inside a V4/V5 one.
            let at = 14 + 40;
            Some([u32le(b, at)?, u32le(b, at + 4)?, u32le(b, at + 8)?, {
                if header >= 56 {
                    u32le(b, at + 12)?
                } else {
                    0
                }
            }])
        }
        _ => return Err("compressed bitmaps are not supported".into()),
    };
    let palette: Vec<[u8; 4]> = if bpp <= 8 {
        let count = match u32le(b, 46)? {
            0 => 1usize << bpp,
            n => (n as usize).min(256),
        };
        let at = 14 + header;
        (0..count)
            .map(|i| {
                b.get(at + i * 4..at + i * 4 + 4)
                    .map(|q| [q[2], q[1], q[0], 255])
                    .ok_or_else(|| "the bitmap palette ends early".to_string())
            })
            .collect::<Result<_, _>>()?
    } else {
        vec![]
    };
    let stride = (width as usize * bpp as usize).div_ceil(32) * 4;
    let mut c = Canvas::new(width, height);
    let channel = |v: u32, mask: u32| -> u8 {
        if mask == 0 {
            return 255;
        }
        let shift = mask.trailing_zeros();
        let bits = (mask >> shift).count_ones();
        let max = (1u64 << bits) - 1;
        ((u64::from((v & mask) >> shift) * 255 + max / 2) / max) as u8
    };
    for row in 0..height {
        let y = if top_down { row } else { height - 1 - row } as i32;
        let line = b
            .get(data_at + row as usize * stride..data_at + (row as usize + 1) * stride)
            .ok_or("the bitmap's pixels end early")?;
        for x in 0..width as usize {
            let p = match bpp {
                1 | 4 | 8 => {
                    let bit = x * bpp as usize;
                    let byte = line[bit / 8];
                    let shift = 8 - bpp as usize - bit % 8;
                    let index = (byte >> shift) as usize & ((1 << bpp) - 1);
                    *palette.get(index).ok_or("a pixel names a missing colour")?
                }
                24 => [line[x * 3 + 2], line[x * 3 + 1], line[x * 3], 255],
                32 => {
                    let v = u32::from_le_bytes([
                        line[x * 4],
                        line[x * 4 + 1],
                        line[x * 4 + 2],
                        line[x * 4 + 3],
                    ]);
                    match masks {
                        Some(m) => [
                            channel(v, m[0]),
                            channel(v, m[1]),
                            channel(v, m[2]),
                            channel(v, m[3]),
                        ],
                        // BI_RGB's fourth byte is unused padding.
                        None => [line[x * 4 + 2], line[x * 4 + 1], line[x * 4], 255],
                    }
                }
                16 => {
                    let v = u32::from(u16::from_le_bytes([line[x * 2], line[x * 2 + 1]]));
                    let m = masks.unwrap_or([0x7c00, 0x03e0, 0x001f, 0]);
                    [channel(v, m[0]), channel(v, m[1]), channel(v, m[2]), 255]
                }
                other => return Err(format!("{other}-bit bitmaps are not supported")),
            };
            c.set(x as i32, y, p);
        }
    }
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bitmaps_round_trip_and_flatten_alpha() {
        let mut c = Canvas::filled(5, 3, [10, 20, 30, 255]);
        c.set(4, 0, [250, 1, 2, 255]);
        c.set(0, 2, [0, 0, 0, 0]);
        let bytes = encode(&c);
        // 5 px * 3 bytes = 15, padded to 16 per row.
        assert_eq!(bytes.len(), 54 + 16 * 3);
        assert_eq!(&bytes[..2], b"BM");
        assert_eq!(u32le(&bytes, 2).unwrap() as usize, bytes.len());
        // Bottom-up: the first row written is the image's last.
        assert_eq!(&bytes[54..57], &[255, 255, 255]);
        let back = decode(&bytes).unwrap();
        assert_eq!(back.get(4, 0), [250, 1, 2, 255]);
        assert_eq!(back.get(1, 1), [10, 20, 30, 255]);
        assert_eq!(back.get(0, 2), [255, 255, 255, 255], "flattened over white");
        assert!(decode(b"PNG").is_err());
        assert!(decode(&bytes[..60]).is_err());
    }
    #[test]
    fn palettes_and_top_down_32_bit_files_decode() {
        // A 2x2 top-down 32-bit bitmap with alpha masks (BI_BITFIELDS, V4-sized header).
        let mut b = b"BM".to_vec();
        let header = 108u32;
        let data_at = 14 + header;
        b.extend_from_slice(&(data_at + 16).to_le_bytes());
        b.extend_from_slice(&[0; 4]);
        b.extend_from_slice(&data_at.to_le_bytes());
        b.extend_from_slice(&header.to_le_bytes());
        b.extend_from_slice(&2i32.to_le_bytes());
        b.extend_from_slice(&(-2i32).to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&32u16.to_le_bytes());
        b.extend_from_slice(&3u32.to_le_bytes());
        b.extend_from_slice(&[0; 20]);
        for m in [0x00ff_0000u32, 0x0000_ff00, 0x0000_00ff, 0xff00_0000] {
            b.extend_from_slice(&m.to_le_bytes());
        }
        b.resize(data_at as usize, 0);
        for px in [
            [1u8, 2, 3, 255],
            [4, 5, 6, 128],
            [7, 8, 9, 0],
            [0, 0, 0, 255],
        ] {
            b.extend_from_slice(&px);
        }
        let c = decode(&b).unwrap();
        assert_eq!(c.get(0, 0), [3, 2, 1, 255], "top-down, BGRA");
        assert_eq!(c.get(1, 0), [6, 5, 4, 128]);
        assert_eq!(c.get(0, 1), [9, 8, 7, 0]);
        // A 1-bit bitmap with a two-colour palette, bottom-up.
        let mut b = b"BM".to_vec();
        b.extend_from_slice(&[0; 8]);
        b.extend_from_slice(&62u32.to_le_bytes());
        b.extend_from_slice(&40u32.to_le_bytes());
        b.extend_from_slice(&3i32.to_le_bytes());
        b.extend_from_slice(&2i32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&[0; 24]);
        b.extend_from_slice(&[0, 0, 0, 0, 255, 255, 255, 0]);
        b.extend_from_slice(&[0b1010_0000, 0, 0, 0, 0b0100_0000, 0, 0, 0]);
        let c = decode(&b).unwrap();
        assert_eq!(c.get(0, 1), [255, 255, 255, 255]);
        assert_eq!(c.get(1, 1), [0, 0, 0, 255]);
        assert_eq!(c.get(1, 0), [255, 255, 255, 255]);
        assert_eq!(c.get(0, 0), [0, 0, 0, 255]);
    }
}
