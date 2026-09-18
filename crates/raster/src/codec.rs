//! Compact, deterministic text encoding for pixel buffers. A snapshot is JSON, and a
//! JSON array of bytes costs four characters a byte; a picture held that way makes every
//! snapshot enormous. Buffers are run-length coded in whole units (a 4-byte pixel, or a
//! 1-byte mask sample) and then base64'd, so flat artwork collapses to almost nothing
//! and a photograph costs about a third more than its raw size.
//!
//! Token stream: a LEB128 header `h`; even `h` is a run of `h/2 + 1` copies of the next
//! unit, odd `h` is `h/2 + 1` literal units that follow.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn put_varint(out: &mut Vec<u8>, mut v: usize) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn get_varint(input: &[u8], at: &mut usize) -> Result<usize, String> {
    let mut v = 0usize;
    let mut shift = 0;
    loop {
        let byte = *input.get(*at).ok_or("truncated run header")?;
        *at += 1;
        if shift > 56 {
            return Err("run header too long".into());
        }
        v |= usize::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(v);
        }
        shift += 7;
    }
}

/// Run-length code `data` in units of `unit` bytes.
pub fn pack(data: &[u8], unit: usize) -> Vec<u8> {
    let units: Vec<&[u8]> = data.chunks(unit).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < units.len() {
        let mut run = 1;
        while i + run < units.len() && units[i + run] == units[i] {
            run += 1;
        }
        if run >= 2 {
            put_varint(&mut out, (run - 1) * 2);
            out.extend_from_slice(units[i]);
            i += run;
            continue;
        }
        // Literal: extend until the next pair of equal units starts a run.
        let start = i;
        while i < units.len() && !(i + 1 < units.len() && units[i + 1] == units[i]) {
            i += 1;
        }
        put_varint(&mut out, (i - start - 1) * 2 + 1);
        for u in &units[start..i] {
            out.extend_from_slice(u);
        }
    }
    out
}

/// Inverse of `pack`, refusing anything that does not expand to exactly `expected` bytes.
pub fn unpack(packed: &[u8], unit: usize, expected: usize) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(expected);
    let mut at = 0;
    while at < packed.len() {
        let h = get_varint(packed, &mut at)?;
        let count = h / 2 + 1;
        if out.len() + count * unit > expected {
            return Err("pixel data is longer than the image".into());
        }
        if h % 2 == 0 {
            let u = packed.get(at..at + unit).ok_or("truncated run")?;
            at += unit;
            for _ in 0..count {
                out.extend_from_slice(u);
            }
        } else {
            let lit = packed
                .get(at..at + count * unit)
                .ok_or("truncated literal")?;
            at += count * unit;
            out.extend_from_slice(lit);
        }
    }
    if out.len() != expected {
        return Err("pixel data is shorter than the image".into());
    }
    Ok(out)
}

pub fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn unbase64(text: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut acc = 0u32;
    let mut bits = 0;
    for c in text.bytes() {
        if c == b'=' {
            break;
        }
        let v = ALPHABET
            .iter()
            .position(|a| *a == c)
            .ok_or("invalid base64 character")? as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

/// `rle:` + base64 of the packed buffer.
pub fn encode(data: &[u8], unit: usize) -> String {
    format!("rle:{}", base64(&pack(data, unit)))
}

pub fn decode(text: &str, unit: usize, expected: usize) -> Result<Vec<u8>, String> {
    let body = text
        .strip_prefix("rle:")
        .ok_or("pixel data has no encoding tag")?;
    unpack(&unbase64(body)?, unit, expected)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packing_round_trips_runs_and_literals() {
        let mut data = vec![];
        data.extend([1, 2, 3, 4].repeat(300));
        data.extend([9, 8, 7, 6, 5, 4, 3, 2]);
        data.extend([0, 0, 0, 0].repeat(2));
        data.extend([1, 1, 1, 1]);
        let packed = pack(&data, 4);
        assert!(packed.len() < 40, "{}", packed.len());
        assert_eq!(unpack(&packed, 4, data.len()).unwrap(), data);
        let text = encode(&data, 4);
        assert_eq!(decode(&text, 4, data.len()).unwrap(), data);
        assert!(decode(&text, 4, data.len() + 4).is_err());
        assert!(decode("nope", 4, 4).is_err());
        for n in 0..7 {
            let bytes: Vec<u8> = (0..n).map(|i| i * 37).collect();
            assert_eq!(unbase64(&base64(&bytes)).unwrap(), bytes);
        }
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(encode(&[], 4), "rle:");
        assert_eq!(decode("rle:", 4, 0).unwrap(), Vec::<u8>::new());
    }
}
