//! The Quite OK Image format (qoiformat.org, specification 1.0), used for the frames an
//! editor keeps of its imported media. It is lossless, so a frame the monitor shows is
//! the frame the file held, and it decodes in one linear pass, so a monitor can pull any
//! frame of a clip at random without replaying the ones before it.
use cw_raster::Canvas;

const OP_INDEX: u8 = 0x00;
const OP_DIFF: u8 = 0x40;
const OP_LUMA: u8 = 0x80;
const OP_RUN: u8 = 0xc0;
const OP_RGB: u8 = 0xfe;
const OP_RGBA: u8 = 0xff;
const END: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 1];

fn hash(p: [u8; 4]) -> usize {
    (p[0] as usize * 3 + p[1] as usize * 5 + p[2] as usize * 7 + p[3] as usize * 11) % 64
}

/// Encode straight RGBA as a four-channel sRGB QOI image.
pub fn encode(canvas: &Canvas) -> Vec<u8> {
    let (w, h) = (canvas.width(), canvas.height());
    let mut out = Vec::with_capacity(22 + canvas.pixels().len() / 4);
    out.extend_from_slice(b"qoif");
    out.extend_from_slice(&w.to_be_bytes());
    out.extend_from_slice(&h.to_be_bytes());
    out.push(4);
    out.push(0);
    let mut index = [[0u8; 4]; 64];
    let mut prev = [0u8, 0, 0, 255];
    let mut run = 0u8;
    let pixels = canvas.pixels();
    let count = pixels.len() / 4;
    for i in 0..count {
        let p = [
            pixels[i * 4],
            pixels[i * 4 + 1],
            pixels[i * 4 + 2],
            pixels[i * 4 + 3],
        ];
        if p == prev {
            run += 1;
            if run == 62 || i == count - 1 {
                out.push(OP_RUN | (run - 1));
                run = 0;
            }
            continue;
        }
        if run > 0 {
            out.push(OP_RUN | (run - 1));
            run = 0;
        }
        let h = hash(p);
        if index[h] == p {
            out.push(OP_INDEX | h as u8);
        } else {
            index[h] = p;
            if p[3] == prev[3] {
                let dr = p[0].wrapping_sub(prev[0]) as i8;
                let dg = p[1].wrapping_sub(prev[1]) as i8;
                let db = p[2].wrapping_sub(prev[2]) as i8;
                let (dr_dg, db_dg) = (dr.wrapping_sub(dg), db.wrapping_sub(dg));
                if (-2..=1).contains(&dr) && (-2..=1).contains(&dg) && (-2..=1).contains(&db) {
                    out.push(
                        OP_DIFF | ((dr + 2) as u8) << 4 | ((dg + 2) as u8) << 2 | (db + 2) as u8,
                    );
                } else if (-32..=31).contains(&dg)
                    && (-8..=7).contains(&dr_dg)
                    && (-8..=7).contains(&db_dg)
                {
                    out.push(OP_LUMA | (dg + 32) as u8);
                    out.push(((dr_dg + 8) as u8) << 4 | (db_dg + 8) as u8);
                } else {
                    out.extend_from_slice(&[OP_RGB, p[0], p[1], p[2]]);
                }
            } else {
                out.extend_from_slice(&[OP_RGBA, p[0], p[1], p[2], p[3]]);
            }
        }
        prev = p;
    }
    out.extend_from_slice(&END);
    out
}

/// Decode a QOI image to straight RGBA. Three-channel images are widened to opaque RGBA.
pub fn decode(data: &[u8]) -> Result<Canvas, String> {
    if data.len() < 14 + END.len() || &data[..4] != b"qoif" {
        return Err("not a QOI image".into());
    }
    let w = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    let h = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    cw_raster::check_size(w, h)?;
    let count = w as usize * h as usize;
    let mut pixels = Vec::with_capacity(count * 4);
    let mut index = [[0u8; 4]; 64];
    let mut p = [0u8, 0, 0, 255];
    let mut at = 14;
    let end = data.len() - END.len();
    let mut run = 0usize;
    while pixels.len() < count * 4 {
        if run > 0 {
            run -= 1;
        } else if at < end {
            let b = data[at];
            at += 1;
            if b == OP_RGB {
                p[..3].copy_from_slice(data.get(at..at + 3).ok_or("truncated QOI")?);
                at += 3;
            } else if b == OP_RGBA {
                p.copy_from_slice(data.get(at..at + 4).ok_or("truncated QOI")?);
                at += 4;
            } else {
                match b & 0xc0 {
                    OP_INDEX => p = index[b as usize],
                    OP_DIFF => {
                        p[0] = p[0].wrapping_add(((b >> 4) & 3).wrapping_sub(2));
                        p[1] = p[1].wrapping_add(((b >> 2) & 3).wrapping_sub(2));
                        p[2] = p[2].wrapping_add((b & 3).wrapping_sub(2));
                    }
                    OP_LUMA => {
                        let b2 = *data.get(at).ok_or("truncated QOI")?;
                        at += 1;
                        let dg = (b & 0x3f).wrapping_sub(32);
                        p[0] = p[0].wrapping_add(dg.wrapping_add((b2 >> 4).wrapping_sub(8)));
                        p[1] = p[1].wrapping_add(dg);
                        p[2] = p[2].wrapping_add(dg.wrapping_add((b2 & 0x0f).wrapping_sub(8)));
                    }
                    _ => run = (b & 0x3f) as usize,
                }
            }
            index[hash(p)] = p;
        } else {
            return Err("truncated QOI".into());
        }
        pixels.extend_from_slice(&p);
    }
    Canvas::from_rgba(w, h, pixels)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_operation_round_trips_exactly() {
        let mut c = Canvas::new(70, 3);
        for x in 0..70 {
            // Runs, small diffs, luma steps, big jumps and alpha changes.
            c.set(x, 0, [10, 10, 10, 255]);
            c.set(x, 1, [(x * 3) as u8, (x * 5) as u8, (x * 40) as u8, 255]);
            c.set(x, 2, [x as u8, 200, 7, (x * 3) as u8]);
        }
        let coded = encode(&c);
        assert_eq!(&coded[..4], b"qoif");
        assert_eq!(decode(&coded).unwrap(), c);
        assert!(decode(&coded[..coded.len() - 12]).is_err());
        assert!(decode(b"nope").is_err());
        // A flat frame is almost nothing.
        let flat = Canvas::filled(320, 180, [0, 0, 0, 255]);
        assert!(encode(&flat).len() < 1000);
    }
}
