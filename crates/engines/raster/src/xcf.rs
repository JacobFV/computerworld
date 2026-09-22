//! GIMP's native XCF format (<https://developer.gimp.org/core/standards/xcf/>), for 8-bit
//! RGB and grayscale images: layers with their names, offsets, opacity, visibility,
//! blend modes and layer masks, in 64x64 tiles that are stored raw, RLE or zlib
//! compressed.
//!
//! Writing produces what GIMP 2.10 writes for such an image: XCF version 11 (64-bit
//! offsets), 8-bit gamma precision, the non-legacy layer modes, RGBA layers and the
//! chosen tile compression. Reading takes every version from the original `file` up,
//! with 32- or 64-bit offsets, legacy or current mode numbers.
use crate::blend::BlendMode;
use crate::document::{Document, Layer};
use crate::{check_size, zlib, Canvas};
use serde::{Deserialize, Serialize};

const TILE: u32 = 64;

const PROP_END: u32 = 0;
const PROP_ACTIVE_LAYER: u32 = 2;
const PROP_OPACITY: u32 = 6;
const PROP_MODE: u32 = 7;
const PROP_VISIBLE: u32 = 8;
const PROP_APPLY_MASK: u32 = 11;
const PROP_OFFSETS: u32 = 15;
const PROP_COMPRESSION: u32 = 17;
const PROP_RESOLUTION: u32 = 19;
const PROP_FLOAT_OPACITY: u32 = 33;

/// How tile data is stored.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Compression {
    None,
    /// GIMP's default: per-channel run-length coding.
    #[default]
    Rle,
    /// "Save using better but slower compression".
    Zlib,
}
impl Compression {
    fn byte(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Rle => 1,
            Self::Zlib => 2,
        }
    }
}

/// A read XCF: the document, and what in the file could not be kept exactly.
#[derive(Clone, Debug)]
pub struct Opened {
    pub document: Document,
    /// Readable notes, e.g. a layer mode this engine does not composite.
    pub notes: Vec<String>,
}

/// The current (GIMP 2.10) mode number of a blend mode.
fn mode_id(mode: BlendMode) -> u32 {
    match mode {
        BlendMode::Normal => 28,
        BlendMode::Multiply => 30,
        BlendMode::Screen => 31,
        BlendMode::Overlay => 23,
        BlendMode::Add => 33,
        BlendMode::Darken => 35,
        BlendMode::Lighten => 36,
    }
}
/// A mode number, current or legacy, as a blend mode.
fn mode_from(id: u32) -> Option<BlendMode> {
    Some(match id {
        0 | 28 => BlendMode::Normal,
        3 | 30 => BlendMode::Multiply,
        4 | 31 => BlendMode::Screen,
        5 | 23 => BlendMode::Overlay,
        7 | 33 => BlendMode::Add,
        9 | 35 => BlendMode::Darken,
        10 | 36 => BlendMode::Lighten,
        _ => return None,
    })
}

// ----- writing -------------------------------------------------------------------

struct Out {
    bytes: Vec<u8>,
}
impl Out {
    fn u32(&mut self, v: u32) {
        self.bytes.extend_from_slice(&v.to_be_bytes());
    }
    fn ptr(&mut self, v: u64) {
        self.bytes.extend_from_slice(&v.to_be_bytes());
    }
    fn at(&self) -> u64 {
        self.bytes.len() as u64
    }
    fn set_ptr(&mut self, slot: usize, v: u64) {
        self.bytes[slot..slot + 8].copy_from_slice(&v.to_be_bytes());
    }
    fn string(&mut self, s: &str) {
        self.u32(s.len() as u32 + 1);
        self.bytes.extend_from_slice(s.as_bytes());
        self.bytes.push(0);
    }
    fn prop(&mut self, id: u32, payload: &[u8]) {
        self.u32(id);
        self.u32(payload.len() as u32);
        self.bytes.extend_from_slice(payload);
    }
}

/// GIMP's tile RLE: each channel's bytes as runs (`n`, value: `n + 1` copies; or 127,
/// hi, lo, value) and literals (`256 - n`, bytes; or 128, hi, lo, bytes).
pub fn rle_encode(data: &[u8], out: &mut Vec<u8>) {
    let mut i = 0;
    while i < data.len() {
        let mut run = 1;
        while i + run < data.len() && data[i + run] == data[i] && run < 32768 {
            run += 1;
        }
        if run >= 3 {
            if run <= 127 {
                out.extend_from_slice(&[(run - 1) as u8, data[i]]);
            } else {
                out.extend_from_slice(&[127, (run >> 8) as u8, run as u8, data[i]]);
            }
            i += run;
            continue;
        }
        // A literal runs until three equal bytes start a run.
        let start = i;
        while i < data.len()
            && i - start < 32768
            && !(i + 2 < data.len() && data[i] == data[i + 1] && data[i] == data[i + 2])
        {
            i += 1;
        }
        let len = i - start;
        if len <= 127 {
            out.push((256 - len) as u8);
        } else {
            out.extend_from_slice(&[128, (len >> 8) as u8, len as u8]);
        }
        out.extend_from_slice(&data[start..i]);
    }
}

fn tile_bytes(canvas: &Canvas, tx: u32, ty: u32, compression: Compression) -> Vec<u8> {
    let (x0, y0) = (tx * TILE, ty * TILE);
    let w = TILE.min(canvas.width() - x0);
    let h = TILE.min(canvas.height() - y0);
    let mut raw = Vec::with_capacity((w * h * 4) as usize);
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            raw.extend_from_slice(&canvas.get(x as i32, y as i32));
        }
    }
    match compression {
        Compression::None => raw,
        Compression::Zlib => zlib::compress(&raw),
        Compression::Rle => {
            let mut out = Vec::new();
            for c in 0..4 {
                let channel: Vec<u8> = raw.iter().skip(c).step_by(4).copied().collect();
                rle_encode(&channel, &mut out);
            }
            out
        }
    }
}

/// The document as an XCF file.
pub fn write(doc: &Document, compression: Compression) -> Vec<u8> {
    let mut o = Out {
        bytes: b"gimp xcf v011\0".to_vec(),
    };
    o.u32(doc.width());
    o.u32(doc.height());
    o.u32(0); // RGB
    o.u32(150); // 8-bit gamma integer
    o.prop(PROP_COMPRESSION, &[compression.byte()]);
    let mut res = 72.0f32.to_bits().to_be_bytes().to_vec();
    res.extend_from_slice(&72.0f32.to_bits().to_be_bytes());
    o.prop(PROP_RESOLUTION, &res);
    o.prop(PROP_END, &[]);
    // Layer pointers, top of the stack first, then the (empty) channel list.
    let layers = doc.layers();
    let slots: Vec<usize> = (0..layers.len())
        .map(|_| {
            let at = o.bytes.len();
            o.ptr(0);
            at
        })
        .collect();
    o.ptr(0);
    o.ptr(0);
    for (slot, (index, layer)) in slots.iter().zip(layers.iter().enumerate().rev()) {
        let here = o.at();
        o.set_ptr(*slot, here);
        let canvas = &layer.canvas;
        o.u32(canvas.width());
        o.u32(canvas.height());
        o.u32(1); // RGB with alpha
        o.string(&layer.name);
        if index == doc.active() {
            o.prop(PROP_ACTIVE_LAYER, &[]);
        }
        o.prop(PROP_VISIBLE, &u32::from(layer.visible).to_be_bytes());
        let opacity = u32::from(crate::fmath::percent255(layer.opacity));
        o.prop(PROP_OPACITY, &opacity.to_be_bytes());
        let float = (f32::from(layer.opacity.min(100)) / 100.0).to_bits();
        o.prop(PROP_FLOAT_OPACITY, &float.to_be_bytes());
        let mut offsets = 0i32.to_be_bytes().to_vec();
        offsets.extend_from_slice(&0i32.to_be_bytes());
        o.prop(PROP_OFFSETS, &offsets);
        o.prop(PROP_MODE, &mode_id(layer.blend).to_be_bytes());
        o.prop(PROP_END, &[]);
        let hierarchy_slot = o.bytes.len();
        o.ptr(0);
        o.ptr(0); // no layer mask
        let here = o.at();
        o.set_ptr(hierarchy_slot, here);
        o.u32(canvas.width());
        o.u32(canvas.height());
        o.u32(4);
        let level_slot = o.bytes.len();
        o.ptr(0);
        o.ptr(0);
        let here = o.at();
        o.set_ptr(level_slot, here);
        o.u32(canvas.width());
        o.u32(canvas.height());
        let (tw, th) = (
            canvas.width().div_ceil(TILE),
            canvas.height().div_ceil(TILE),
        );
        let tile_slots: Vec<usize> = (0..tw * th)
            .map(|_| {
                let at = o.bytes.len();
                o.ptr(0);
                at
            })
            .collect();
        o.ptr(0);
        for (k, slot) in tile_slots.iter().enumerate() {
            let here = o.at();
            o.set_ptr(*slot, here);
            let data = tile_bytes(canvas, k as u32 % tw, k as u32 / tw, compression);
            o.bytes.extend_from_slice(&data);
        }
    }
    o.bytes
}

// ----- reading -------------------------------------------------------------------

struct In<'a> {
    b: &'a [u8],
    at: usize,
    wide: bool,
}
impl In<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let s = self
            .b
            .get(self.at..self.at + n)
            .ok_or("the XCF file ends early")?;
        self.at += n;
        Ok(s)
    }
    fn u32(&mut self) -> Result<u32, String> {
        let s = self.take(4)?;
        Ok(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn ptr(&mut self) -> Result<usize, String> {
        if self.wide {
            let s = self.take(8)?;
            let v = u64::from_be_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]);
            usize::try_from(v).map_err(|_| "an XCF offset is out of range".into())
        } else {
            Ok(self.u32()? as usize)
        }
    }
    fn string(&mut self) -> Result<String, String> {
        let n = self.u32()? as usize;
        if n == 0 {
            return Ok(String::new());
        }
        let s = self.take(n)?;
        Ok(String::from_utf8_lossy(&s[..n - 1]).into_owned())
    }
    /// Property list: `(id, payload)` pairs up to PROP_END.
    fn props(&mut self) -> Result<Vec<(u32, Vec<u8>)>, String> {
        let mut out = vec![];
        loop {
            let id = self.u32()?;
            let len = self.u32()? as usize;
            if id == PROP_END {
                return Ok(out);
            }
            out.push((id, self.take(len)?.to_vec()));
            if out.len() > 10_000 {
                return Err("the XCF file has too many properties".into());
            }
        }
    }
    fn seek(&mut self, at: usize) -> Result<(), String> {
        if at == 0 || at > self.b.len() {
            return Err("an XCF offset points outside the file".into());
        }
        self.at = at;
        Ok(())
    }
}

fn be32(p: &[u8]) -> u32 {
    p.get(..4)
        .map_or(0, |s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

/// Decode one channel stream of RLE into `count` bytes.
fn rle_decode(b: &[u8], at: &mut usize, count: usize) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(count);
    let byte = |at: &mut usize| -> Result<u8, String> {
        let v = *b.get(*at).ok_or("an RLE tile ends early")?;
        *at += 1;
        Ok(v)
    };
    while out.len() < count {
        let n = byte(at)?;
        match n {
            0..=126 => {
                let v = byte(at)?;
                out.extend(std::iter::repeat_n(v, n as usize + 1));
            }
            127 => {
                let len = usize::from(byte(at)?) << 8 | usize::from(byte(at)?);
                let v = byte(at)?;
                out.extend(std::iter::repeat_n(v, len));
            }
            128 => {
                let len = usize::from(byte(at)?) << 8 | usize::from(byte(at)?);
                let s = b.get(*at..*at + len).ok_or("an RLE tile ends early")?;
                out.extend_from_slice(s);
                *at += len;
            }
            _ => {
                let len = 256 - n as usize;
                let s = b.get(*at..*at + len).ok_or("an RLE tile ends early")?;
                out.extend_from_slice(s);
                *at += len;
            }
        }
    }
    if out.len() != count {
        return Err("an RLE run overflows its tile".into());
    }
    Ok(out)
}

/// A hierarchy's pixels as `bpp`-byte samples, row-major.
fn read_hierarchy(
    r: &mut In<'_>,
    at: usize,
    compression: u8,
    expect_bpp: Option<u32>,
) -> Result<(u32, u32, u32, Vec<u8>), String> {
    r.seek(at)?;
    let (w, h, bpp) = (r.u32()?, r.u32()?, r.u32()?);
    check_size(w, h)?;
    if let Some(e) = expect_bpp {
        if bpp != e {
            return Err(format!(
                "only 8-bit XCF images can be opened (this layer has {bpp} bytes per pixel)"
            ));
        }
    }
    if !(1..=4).contains(&bpp) {
        return Err("only 8-bit XCF images can be opened".into());
    }
    let level = r.ptr()?;
    r.seek(level)?;
    let (lw, lh) = (r.u32()?, r.u32()?);
    if (lw, lh) != (w, h) {
        return Err("an XCF level does not match its layer".into());
    }
    let (tw, th) = (w.div_ceil(TILE), h.div_ceil(TILE));
    let mut tiles = Vec::with_capacity((tw * th) as usize);
    for _ in 0..tw * th {
        tiles.push(r.ptr()?);
    }
    let bpp_u = bpp as usize;
    let mut pixels = vec![0u8; w as usize * h as usize * bpp_u];
    for (k, start) in tiles.iter().enumerate() {
        let (tx, ty) = (k as u32 % tw, k as u32 / tw);
        let (x0, y0) = (tx * TILE, ty * TILE);
        let (cw, ch) = (TILE.min(w - x0), TILE.min(h - y0));
        let n = (cw * ch) as usize;
        if *start == 0 || *start >= r.b.len() {
            return Err("an XCF tile offset points outside the file".into());
        }
        let raw: Vec<u8> = match compression {
            0 => {
                r.b.get(*start..*start + n * bpp_u)
                    .ok_or("an XCF tile ends early")?
                    .to_vec()
            }
            1 => {
                let mut at = *start;
                let channels: Vec<Vec<u8>> = (0..bpp_u)
                    .map(|_| rle_decode(r.b, &mut at, n))
                    .collect::<Result<_, _>>()?;
                (0..n * bpp_u)
                    .map(|i| channels[i % bpp_u][i / bpp_u])
                    .collect()
            }
            2 => {
                let end = tiles
                    .get(k + 1)
                    .copied()
                    .filter(|e| *e > *start)
                    .unwrap_or(r.b.len());
                let data = zlib::decompress(&r.b[*start..end.min(r.b.len())], n * bpp_u)?;
                if data.len() != n * bpp_u {
                    return Err("a zlib tile is the wrong size".into());
                }
                data
            }
            other => return Err(format!("XCF compression {other} is not supported")),
        };
        for row in 0..ch {
            let src = (row * cw) as usize * bpp_u;
            let dst = (((y0 + row) * w + x0) as usize) * bpp_u;
            pixels[dst..dst + cw as usize * bpp_u]
                .copy_from_slice(&raw[src..src + cw as usize * bpp_u]);
        }
    }
    Ok((w, h, bpp, pixels))
}

/// Read an XCF file.
pub fn read(b: &[u8]) -> Result<Opened, String> {
    if b.get(..9) != Some(b"gimp xcf ") || b.get(13) != Some(&0) {
        return Err("not a GIMP XCF file".into());
    }
    let version = match &b[9..13] {
        b"file" => 0,
        [b'v', d @ ..] => std::str::from_utf8(d)
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .ok_or("unknown XCF version")?,
        _ => return Err("unknown XCF version".into()),
    };
    let mut r = In {
        b,
        at: 14,
        wide: version >= 11,
    };
    let (width, height, base) = (r.u32()?, r.u32()?, r.u32()?);
    check_size(width, height)?;
    if version >= 4 {
        let precision = r.u32()?;
        let eight_bit = if version >= 7 {
            matches!(precision, 100 | 150)
        } else {
            // XCF 4-6 numbered precisions from 0 (8-bit integer).
            precision == 0
        };
        if !eight_bit {
            return Err("only 8-bit XCF images can be opened".into());
        }
    }
    let (gray, color_bpp) = match base {
        0 => (false, 3),
        1 => (true, 1),
        _ => return Err("indexed XCF images cannot be opened here".into()),
    };
    let mut compression = 0u8;
    for (id, payload) in r.props()? {
        if id == PROP_COMPRESSION {
            compression = *payload.first().unwrap_or(&0);
        }
    }
    let mut pointers = vec![];
    loop {
        let p = r.ptr()?;
        if p == 0 {
            break;
        }
        pointers.push(p);
        if pointers.len() > crate::document::LAYER_LIMIT {
            return Err(format!(
                "the image has more than {} layers",
                crate::document::LAYER_LIMIT
            ));
        }
    }
    if pointers.is_empty() {
        return Err("the XCF file has no layers".into());
    }
    let mut notes = vec![];
    let mut layers = vec![];
    let mut active = None;
    for (i, at) in pointers.iter().enumerate() {
        r.seek(*at)?;
        let (_lw, _lh, kind) = (r.u32()?, r.u32()?, r.u32()?);
        let name = r.string()?;
        let (mut visible, mut opacity, mut blend) = (true, 100u8, BlendMode::Normal);
        let (mut ox, mut oy, mut apply_mask) = (0i32, 0i32, true);
        let mut float_opacity = None;
        for (id, p) in r.props()? {
            match id {
                PROP_ACTIVE_LAYER => active = Some(i),
                PROP_VISIBLE => visible = be32(&p) != 0,
                PROP_OPACITY => {
                    opacity = ((be32(&p).min(255) * 100 + 127) / 255) as u8;
                }
                PROP_FLOAT_OPACITY => {
                    let v = f32::from_bits(be32(&p));
                    float_opacity = Some(crate::fmath::round(f64::from(v.clamp(0.0, 1.0)) * 100.0) as u8);
                }
                PROP_OFFSETS => {
                    ox = be32(&p) as i32;
                    oy = be32(&p[4.min(p.len())..]) as i32;
                }
                PROP_APPLY_MASK => apply_mask = be32(&p) != 0,
                PROP_MODE => match mode_from(be32(&p)) {
                    Some(m) => blend = m,
                    None => notes.push(format!(
                        "Layer “{name}” uses a mode this editor does not have; it is shown as Normal"
                    )),
                },
                _ => {}
            }
        }
        if let Some(v) = float_opacity {
            opacity = v;
        }
        let hierarchy = r.ptr()?;
        let mask = r.ptr()?;
        let alpha = match kind {
            0 | 2 => false,
            1 | 3 => true,
            _ => return Err("indexed XCF layers cannot be opened here".into()),
        };
        if (kind >= 2) != gray {
            return Err("an XCF layer does not match the image's colour mode".into());
        }
        let bpp = color_bpp + u32::from(alpha);
        let (w, h, _, px) = read_hierarchy(&mut r, hierarchy, compression, Some(bpp))?;
        let mut layer = Canvas::new(w, h);
        for (k, s) in px.chunks(bpp as usize).enumerate() {
            let p = match (gray, alpha) {
                (false, true) => [s[0], s[1], s[2], s[3]],
                (false, false) => [s[0], s[1], s[2], 255],
                (true, true) => [s[0], s[0], s[0], s[1]],
                (true, false) => [s[0], s[0], s[0], 255],
            };
            layer.set((k as u32 % w) as i32, (k as u32 / w) as i32, p);
        }
        if mask != 0 && apply_mask {
            // A layer mask is a channel: size, name, properties, then its hierarchy.
            r.seek(mask)?;
            let (_, _) = (r.u32()?, r.u32()?);
            let _ = r.string()?;
            let _ = r.props()?;
            let at = r.ptr()?;
            let (mw, mh, _, m) = read_hierarchy(&mut r, at, compression, Some(1))?;
            if (mw, mh) == (w, h) {
                for (k, v) in m.iter().enumerate() {
                    let (x, y) = ((k as u32 % w) as i32, (k as u32 / w) as i32);
                    let mut p = layer.get(x, y);
                    p[3] = crate::fmath::mul255(p[3], *v);
                    layer.set(x, y, p);
                }
            }
        }
        // Layers become image-sized, their pixels placed at their offsets.
        let mut canvas = Canvas::new(width, height);
        canvas.put(ox, oy, &layer);
        let mut l = Layer::new(if name.is_empty() { "Layer" } else { &name }, canvas);
        l.visible = visible;
        l.opacity = opacity.min(100);
        l.blend = blend;
        layers.push(l);
    }
    // XCF lists the top of the stack first.
    layers.reverse();
    let n = layers.len();
    let active = n - 1 - active.unwrap_or(0);
    Ok(Opened {
        document: Document::from_layers(width, height, layers, active)?,
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fmath::percent255;

    fn sample() -> Document {
        let mut bottom = Canvas::new(150, 70);
        for y in 0..70 {
            for x in 0..150 {
                bottom.set(x, y, [(x * 3 % 256) as u8, (y * 5) as u8, 90, 255]);
            }
        }
        let mut top = Canvas::new(150, 70);
        for y in 10..30 {
            for x in 60..130 {
                top.set(x, y, [200, 30, 40, (x * 2 % 256) as u8]);
            }
        }
        let mut a = Layer::new("Background", bottom);
        a.opacity = 100;
        let mut b = Layer::new("Ink — top", top);
        b.opacity = 37;
        b.blend = BlendMode::Multiply;
        let mut c = Layer::new("Hidden", Canvas::filled(150, 70, [1, 2, 3, 4]));
        c.visible = false;
        c.blend = BlendMode::Overlay;
        Document::from_layers(150, 70, vec![a, b, c], 1).unwrap()
    }

    #[test]
    fn documents_round_trip_through_every_compression() {
        let doc = sample();
        for compression in [Compression::None, Compression::Rle, Compression::Zlib] {
            let bytes = write(&doc, compression);
            assert_eq!(&bytes[..14], b"gimp xcf v011\0");
            assert_eq!(&bytes[14..18], &150u32.to_be_bytes());
            let back = read(&bytes).unwrap();
            assert!(back.notes.is_empty());
            let d = back.document;
            assert_eq!((d.width(), d.height()), (150, 70));
            assert_eq!(d.active(), 1);
            assert_eq!(d.layers(), doc.layers(), "{compression:?}");
            assert_eq!(write(&d, compression), bytes, "stable bytes");
        }
        let rle = write(&doc, Compression::Rle).len();
        let raw = write(&doc, Compression::None).len();
        assert!(rle < raw, "{rle} < {raw}");
        // Every percent survives the byte opacity and the float one.
        for p in 0..=100u8 {
            let v = u32::from(percent255(p));
            assert_eq!(((v * 100 + 127) / 255) as u8, p);
        }
    }

    #[test]
    fn run_length_coding_matches_gimps_opcodes() {
        let mut out = vec![];
        rle_encode(&[7, 7, 7, 7, 1, 2, 3, 9, 9, 9], &mut out);
        assert_eq!(out, vec![3, 7, 253, 1, 2, 3, 2, 9]);
        let mut at = 0;
        assert_eq!(
            rle_decode(&out, &mut at, 10).unwrap(),
            vec![7, 7, 7, 7, 1, 2, 3, 9, 9, 9]
        );
        let long: Vec<u8> = (0..300).map(|i| (i % 2) as u8).collect();
        let mut out = vec![];
        rle_encode(&long, &mut out);
        assert_eq!(&out[..3], &[128, 1, 44], "a long literal");
        let mut out2 = vec![];
        rle_encode(&[5; 1000], &mut out2);
        assert_eq!(out2, vec![127, 3, 232, 5]);
        let mut at = 0;
        assert_eq!(rle_decode(&out, &mut at, 300).unwrap(), long);
    }

    /// Builds a file by hand, byte by byte from the specification (XCF v003: 32-bit
    /// offsets, no precision field, legacy mode numbers), independent of the writer.
    fn reference_file() -> Vec<u8> {
        let mut f = b"gimp xcf v003\0".to_vec();
        let u = |f: &mut Vec<u8>, v: u32| f.extend_from_slice(&v.to_be_bytes());
        u(&mut f, 4); // width
        u(&mut f, 3); // height
        u(&mut f, 0); // RGB
        u(&mut f, 17); // PROP_COMPRESSION
        u(&mut f, 1);
        f.push(1); // RLE
        u(&mut f, 0);
        u(&mut f, 0); // PROP_END
        let layer_ptrs = f.len();
        u(&mut f, 0); // top layer, patched below
        u(&mut f, 0); // bottom layer
        u(&mut f, 0); // end of layers
        u(&mut f, 0); // end of channels
                      // Top layer: 2x1 RGBA at offset (1, 1), 50% "multiply" (legacy 3), active.
        let top = f.len() as u32;
        u(&mut f, 2);
        u(&mut f, 1);
        u(&mut f, 1);
        u(&mut f, 4);
        f.extend_from_slice(b"Top\0");
        for (id, payload) in [
            (2u32, vec![]),
            (6, 128u32.to_be_bytes().to_vec()),
            (7, 3u32.to_be_bytes().to_vec()),
            (8, 1u32.to_be_bytes().to_vec()),
            (15, [1i32.to_be_bytes(), 1i32.to_be_bytes()].concat()),
        ] {
            u(&mut f, id);
            u(&mut f, payload.len() as u32);
            f.extend_from_slice(&payload);
        }
        u(&mut f, 0);
        u(&mut f, 0);
        let h = f.len() as u32 + 8;
        u(&mut f, h); // hierarchy
        u(&mut f, 0); // no mask
        u(&mut f, 2);
        u(&mut f, 1);
        u(&mut f, 4); // bpp
        let level = f.len() as u32 + 8;
        u(&mut f, level);
        u(&mut f, 0);
        u(&mut f, 2);
        u(&mut f, 1);
        let tile = f.len() as u32 + 8;
        u(&mut f, tile);
        u(&mut f, 0);
        // R: literal 255,0; G: run of two 10; B: run of two 20; A: literal 255,128.
        f.extend_from_slice(&[254, 255, 0, 1, 10, 1, 20, 254, 255, 128]);
        // Bottom layer: 4x3 RGB without alpha, hidden, grey 60.
        let bottom = f.len() as u32;
        u(&mut f, 4);
        u(&mut f, 3);
        u(&mut f, 0);
        u(&mut f, 6);
        f.extend_from_slice(b"Paper\0");
        u(&mut f, 8);
        u(&mut f, 4);
        u(&mut f, 0); // hidden
        u(&mut f, 0);
        u(&mut f, 0);
        let h = f.len() as u32 + 8;
        u(&mut f, h);
        u(&mut f, 0);
        u(&mut f, 4);
        u(&mut f, 3);
        u(&mut f, 3);
        let level = f.len() as u32 + 8;
        u(&mut f, level);
        u(&mut f, 0);
        u(&mut f, 4);
        u(&mut f, 3);
        let tile = f.len() as u32 + 8;
        u(&mut f, tile);
        u(&mut f, 0);
        // Twelve pixels: a long run of 60 in each channel.
        for _ in 0..3 {
            f.extend_from_slice(&[127, 0, 12, 60]);
        }
        f[layer_ptrs..layer_ptrs + 4].copy_from_slice(&top.to_be_bytes());
        f[layer_ptrs + 4..layer_ptrs + 8].copy_from_slice(&bottom.to_be_bytes());
        f
    }

    #[test]
    fn a_hand_built_version_3_file_reads_as_specified() {
        let opened = read(&reference_file()).unwrap();
        let d = opened.document;
        assert_eq!((d.width(), d.height()), (4, 3));
        assert_eq!(d.layers().len(), 2);
        let (paper, top) = (&d.layers()[0], &d.layers()[1]);
        assert_eq!(paper.name, "Paper");
        assert!(!paper.visible);
        assert_eq!(paper.canvas.get(3, 2), [60, 60, 60, 255]);
        assert_eq!(top.name, "Top");
        assert_eq!(top.blend, BlendMode::Multiply);
        assert_eq!(top.opacity, 50);
        assert_eq!(d.active(), 1);
        // Placed at its offsets on an image-sized canvas.
        assert_eq!(top.canvas.get(0, 0), [0, 0, 0, 0]);
        assert_eq!(top.canvas.get(1, 1), [255, 10, 20, 255]);
        assert_eq!(top.canvas.get(2, 1), [0, 10, 20, 128]);
        assert_eq!(top.canvas.get(3, 1), [0, 0, 0, 0]);
        // Damage is refused, not guessed at.
        let f = reference_file();
        assert!(read(&f[..f.len() - 3]).is_err());
        assert!(read(b"gimp xcf v099").is_err());
        let mut indexed = f.clone();
        indexed[25] = 2;
        assert!(read(&indexed).is_err());
    }

    #[test]
    fn grayscale_masks_and_foreign_modes_are_read() {
        // Write a v011 grey image by hand: one 2x2 grey+alpha layer with a mask and
        // "soft light" (45), which has no equivalent here.
        let mut f = b"gimp xcf v011\0".to_vec();
        let u = |f: &mut Vec<u8>, v: u32| f.extend_from_slice(&v.to_be_bytes());
        let p = |f: &mut Vec<u8>, v: u64| f.extend_from_slice(&v.to_be_bytes());
        // A pointer to what starts `n` bytes after this pointer's own position.
        let rel = |f: &mut Vec<u8>, n: u64| {
            let v = f.len() as u64 + n;
            f.extend_from_slice(&v.to_be_bytes());
        };
        u(&mut f, 2);
        u(&mut f, 2);
        u(&mut f, 1); // grayscale
        u(&mut f, 150);
        u(&mut f, 17);
        u(&mut f, 1);
        f.push(0); // uncompressed
        u(&mut f, 0);
        u(&mut f, 0);
        let lp = f.len();
        p(&mut f, 0);
        p(&mut f, 0);
        p(&mut f, 0);
        let layer = f.len() as u64;
        u(&mut f, 2);
        u(&mut f, 2);
        u(&mut f, 3); // grey with alpha
        u(&mut f, 2);
        f.extend_from_slice(b"G\0");
        u(&mut f, 7);
        u(&mut f, 4);
        u(&mut f, 45);
        u(&mut f, 0);
        u(&mut f, 0);
        let hp = f.len();
        p(&mut f, 0);
        p(&mut f, 0);
        let h = f.len() as u64;
        f[hp..hp + 8].copy_from_slice(&h.to_be_bytes());
        u(&mut f, 2);
        u(&mut f, 2);
        u(&mut f, 2);
        rel(&mut f, 16);
        p(&mut f, 0);
        u(&mut f, 2);
        u(&mut f, 2);
        rel(&mut f, 16);
        p(&mut f, 0);
        f.extend_from_slice(&[10, 255, 20, 255, 30, 255, 40, 0]);
        // The mask channel.
        let mask = f.len() as u64;
        f[hp + 8..hp + 16].copy_from_slice(&mask.to_be_bytes());
        u(&mut f, 2);
        u(&mut f, 2);
        f.extend_from_slice(&2u32.to_be_bytes());
        f.extend_from_slice(b"M\0");
        u(&mut f, 0);
        u(&mut f, 0);
        rel(&mut f, 8);
        u(&mut f, 2);
        u(&mut f, 2);
        u(&mut f, 1);
        rel(&mut f, 16);
        p(&mut f, 0);
        u(&mut f, 2);
        u(&mut f, 2);
        rel(&mut f, 16);
        p(&mut f, 0);
        f.extend_from_slice(&[255, 0, 128, 255]);
        f[lp..lp + 8].copy_from_slice(&layer.to_be_bytes());
        let opened = read(&f).unwrap();
        assert_eq!(opened.notes.len(), 1, "{:?}", opened.notes);
        let c = &opened.document.layers()[0].canvas;
        assert_eq!(c.get(0, 0), [10, 10, 10, 255]);
        assert_eq!(c.get(1, 0), [20, 20, 20, 0], "masked out");
        assert_eq!(c.get(0, 1), [30, 30, 30, 128]);
        assert_eq!(c.get(1, 1), [40, 40, 40, 0]);
    }
}
