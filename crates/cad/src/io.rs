//! Exchange formats: the native document (FreeCAD-like structure, as JSON), STL in both
//! encodings, Wavefront OBJ, and DXF R12 for sketches. Every writer is a pure function of
//! its input and every reader refuses what it cannot represent rather than guessing.
use crate::document::Document;
use crate::math::{self, v2, v3, V2, V3};
use crate::mesh::{FixedMap, Mesh, Surface};
use crate::sketch::{Geom, Sketch};
use serde::{Deserialize, Serialize};

/// Largest mesh an import accepts, so a document (and every snapshot holding it) stays
/// a reasonable size.
pub const IMPORT_TRIANGLE_LIMIT: usize = 200_000;

// ---------------------------------------------------------------------------------
// Native document.

pub const NATIVE_EXTENSION: &str = "FCStd.json";
const SCHEMA: u32 = 1;

#[derive(Serialize, Deserialize)]
struct Native {
    #[serde(rename = "SchemaVersion")]
    schema: u32,
    #[serde(rename = "ProgramName")]
    program: String,
    #[serde(rename = "ProgramVersion")]
    version: String,
    #[serde(rename = "Document")]
    document: Document,
}

/// The document as the native JSON file: FCStd's `Document.xml` structure (objects with
/// `Name`, `Label`, `TypeId` and their properties), written as JSON.
pub fn save_native(doc: &Document) -> String {
    serde_json::to_string_pretty(&Native {
        schema: SCHEMA,
        program: "FreeCAD".into(),
        version: "1.0.2".into(),
        document: doc.clone(),
    })
    .expect("documents serialise")
}
pub fn load_native(text: &str) -> Result<Document, String> {
    let n: Native =
        serde_json::from_str(text).map_err(|e| format!("Not a FreeCAD document: {e}"))?;
    if n.schema != SCHEMA {
        return Err(format!("Unsupported document version {}", n.schema));
    }
    Ok(n.document)
}

// ---------------------------------------------------------------------------------
// STL.

fn tri_normal(m: &Mesh, i: usize) -> V3 {
    m.tri_normal(i)
}

/// ASCII STL. Numbers are printed exactly (shortest round-trip form).
pub fn stl_ascii(m: &Mesh, name: &str) -> String {
    let name = name.replace(['\n', '\r'], " ");
    let mut s = format!("solid {name}\n");
    for i in 0..m.tris.len() {
        let n = tri_normal(m, i);
        s.push_str(&format!(
            "  facet normal {:e} {:e} {:e}\n    outer loop\n",
            n.x, n.y, n.z
        ));
        for p in m.tri(i) {
            s.push_str(&format!("      vertex {:e} {:e} {:e}\n", p.x, p.y, p.z));
        }
        s.push_str("    endloop\n  endfacet\n");
    }
    s.push_str(&format!("endsolid {name}\n"));
    s
}

/// Binary STL: an 80-byte header, a triangle count and 50 bytes per triangle.
pub fn stl_binary(m: &Mesh, header: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(84 + 50 * m.tris.len());
    let mut h = [b' '; 80];
    for (i, b) in header.bytes().take(80).enumerate() {
        h[i] = b;
    }
    out.extend_from_slice(&h);
    out.extend_from_slice(&(m.tris.len() as u32).to_le_bytes());
    for i in 0..m.tris.len() {
        let n = tri_normal(m, i);
        for v in [n.x, n.y, n.z] {
            out.extend_from_slice(&(v as f32).to_le_bytes());
        }
        for p in m.tri(i) {
            for v in [p.x, p.y, p.z] {
                out.extend_from_slice(&(v as f32).to_le_bytes());
            }
        }
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    out
}

/// Build an indexed mesh from a triangle soup, sharing exactly equal vertices.
fn from_soup(tris: Vec<[V3; 3]>) -> Result<Mesh, String> {
    if tris.is_empty() {
        return Err("The file contains no triangles".into());
    }
    if tris.len() > IMPORT_TRIANGLE_LIMIT {
        return Err(format!(
            "The mesh has more than {IMPORT_TRIANGLE_LIMIT} triangles"
        ));
    }
    let mut index: FixedMap<(u64, u64, u64), u32> = FixedMap::default();
    let mut m = Mesh {
        surfaces: vec![Surface::Facets],
        ..Mesh::default()
    };
    for t in tris {
        if t.iter().any(|p| !p.is_finite()) {
            return Err("The file contains a coordinate that is not a number".into());
        }
        let mut idx = [0u32; 3];
        for (k, p) in t.iter().enumerate() {
            let key = (p.x.to_bits(), p.y.to_bits(), p.z.to_bits());
            let next = m.verts.len() as u32;
            idx[k] = *index.entry(key).or_insert_with(|| {
                m.verts.push(*p);
                next
            });
        }
        if idx[0] == idx[1] || idx[1] == idx[2] || idx[0] == idx[2] {
            continue;
        }
        m.tris.push(idx);
        m.tri_surface.push(0);
    }
    Ok(m)
}

pub fn read_stl(bytes: &[u8]) -> Result<Mesh, String> {
    let binary_len = |n: usize| 84 + 50 * n;
    let count = (bytes.len() >= 84)
        .then(|| u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize);
    // Some binary files start their header with "solid"; the length decides.
    let is_binary = count.is_some_and(|n| binary_len(n) == bytes.len());
    if is_binary {
        let n = count.unwrap();
        let mut tris = Vec::with_capacity(n);
        for i in 0..n {
            let at = 84 + 50 * i + 12;
            let f = |k: usize| {
                let o = at + 4 * k;
                f64::from(f32::from_le_bytes([
                    bytes[o],
                    bytes[o + 1],
                    bytes[o + 2],
                    bytes[o + 3],
                ]))
            };
            tris.push([
                v3(f(0), f(1), f(2)),
                v3(f(3), f(4), f(5)),
                v3(f(6), f(7), f(8)),
            ]);
        }
        return from_soup(tris);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "Not an STL file")?;
    if !text.trim_start().starts_with("solid") {
        return Err("Not an STL file".into());
    }
    let mut tris = Vec::new();
    let mut cur: Vec<V3> = Vec::new();
    for line in text.lines() {
        let mut w = line.split_whitespace();
        match w.next() {
            Some("vertex") => {
                let nums: Result<Vec<f64>, _> = w.map(str::parse::<f64>).collect();
                let nums = nums.map_err(|_| "Malformed vertex in STL")?;
                if nums.len() != 3 {
                    return Err("Malformed vertex in STL".into());
                }
                cur.push(v3(nums[0], nums[1], nums[2]));
            }
            Some("endloop") => {
                if cur.len() != 3 {
                    return Err("An STL facet does not have three vertices".into());
                }
                tris.push([cur[0], cur[1], cur[2]]);
                cur.clear();
            }
            _ => {}
        }
    }
    from_soup(tris)
}

// ---------------------------------------------------------------------------------
// OBJ.

pub fn obj(m: &Mesh, name: &str) -> String {
    let mut s = String::from("# FreeCAD v1.0.2\n");
    s.push_str(&format!("o {}\n", name.replace(['\n', '\r'], " ")));
    for p in &m.verts {
        s.push_str(&format!("v {} {} {}\n", p.x, p.y, p.z));
    }
    for t in &m.tris {
        s.push_str(&format!("f {} {} {}\n", t[0] + 1, t[1] + 1, t[2] + 1));
    }
    s
}

pub fn read_obj(text: &str) -> Result<Mesh, String> {
    let mut verts: Vec<V3> = Vec::new();
    let mut tris: Vec<[V3; 3]> = Vec::new();
    for (no, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("");
        let mut w = line.split_whitespace();
        match w.next() {
            Some("v") => {
                let nums: Result<Vec<f64>, _> = w.take(3).map(str::parse::<f64>).collect();
                let nums = nums.map_err(|_| format!("line {}: malformed vertex", no + 1))?;
                if nums.len() != 3 {
                    return Err(format!("line {}: malformed vertex", no + 1));
                }
                verts.push(v3(nums[0], nums[1], nums[2]));
            }
            Some("f") => {
                let mut idx = Vec::new();
                for tok in w {
                    let first = tok.split('/').next().unwrap_or("");
                    let i: i64 = first
                        .parse()
                        .map_err(|_| format!("line {}: malformed face", no + 1))?;
                    let resolved = if i < 0 { verts.len() as i64 + i } else { i - 1 };
                    if resolved < 0 || resolved as usize >= verts.len() {
                        return Err(format!("line {}: face refers to a missing vertex", no + 1));
                    }
                    idx.push(verts[resolved as usize]);
                }
                if idx.len() < 3 {
                    return Err(format!("line {}: a face needs three vertices", no + 1));
                }
                for k in 1..idx.len() - 1 {
                    tris.push([idx[0], idx[k], idx[k + 1]]);
                }
            }
            _ => {}
        }
    }
    from_soup(tris)
}

// ---------------------------------------------------------------------------------
// DXF (R12, ASCII) for sketches.

fn group(out: &mut String, code: u32, value: impl std::fmt::Display) {
    out.push_str(&format!("{code}\n{value}\n"));
}

/// A sketch's (non-construction) geometry as DXF R12 entities on layer `0`, in the
/// sketch's own coordinates.
pub fn dxf(s: &Sketch) -> String {
    let mut out = String::new();
    for (c, v) in [
        (0, "SECTION"),
        (2, "HEADER"),
        (9, "$ACADVER"),
        (1, "AC1009"),
        (9, "$INSUNITS"),
        (70, "4"),
        (0, "ENDSEC"),
    ] {
        group(&mut out, c, v);
    }
    group(&mut out, 0, "SECTION");
    group(&mut out, 2, "ENTITIES");
    for g in s.geos.iter().filter(|g| !g.construction) {
        match g.geom {
            Geom::Point { p } => {
                group(&mut out, 0, "POINT");
                group(&mut out, 8, "0");
                group(&mut out, 10, p.x);
                group(&mut out, 20, p.y);
                group(&mut out, 30, 0.0);
            }
            Geom::Line { a, b } => {
                group(&mut out, 0, "LINE");
                group(&mut out, 8, "0");
                group(&mut out, 10, a.x);
                group(&mut out, 20, a.y);
                group(&mut out, 30, 0.0);
                group(&mut out, 11, b.x);
                group(&mut out, 21, b.y);
                group(&mut out, 31, 0.0);
            }
            Geom::Circle { c, r } => {
                group(&mut out, 0, "CIRCLE");
                group(&mut out, 8, "0");
                group(&mut out, 10, c.x);
                group(&mut out, 20, c.y);
                group(&mut out, 30, 0.0);
                group(&mut out, 40, r);
            }
            Geom::Arc { c, r, start, end } => {
                group(&mut out, 0, "ARC");
                group(&mut out, 8, "0");
                group(&mut out, 10, c.x);
                group(&mut out, 20, c.y);
                group(&mut out, 30, 0.0);
                group(&mut out, 40, r);
                group(&mut out, 50, math::degrees(start));
                group(&mut out, 51, math::degrees(end));
            }
        }
    }
    group(&mut out, 0, "ENDSEC");
    group(&mut out, 0, "EOF");
    out
}

/// Read DXF entities (POINT, LINE, CIRCLE, ARC, LWPOLYLINE with bulges) into sketch
/// geometry. Anything else in the file is skipped and counted.
pub fn read_dxf(text: &str) -> Result<(Sketch, usize), String> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    if !lines.len().is_multiple_of(2) && lines.last().is_some_and(|l| !l.is_empty()) {
        return Err("Not a DXF file: group codes and values do not pair up".into());
    }
    let mut pairs: Vec<(i32, &str)> = Vec::new();
    for w in lines.chunks(2) {
        if w.len() < 2 {
            break;
        }
        let code: i32 = w[0]
            .parse()
            .map_err(|_| format!("Not a DXF file: bad group code {:?}", w[0]))?;
        pairs.push((code, w[1]));
    }
    let start = pairs
        .iter()
        .position(|(c, v)| *c == 2 && *v == "ENTITIES")
        .ok_or("The DXF file has no ENTITIES section")?;
    let mut s = Sketch::default();
    let mut skipped = 0;
    let mut i = start + 1;
    let num = |v: &str| {
        v.parse::<f64>()
            .map_err(|_| format!("Bad number {v:?} in DXF"))
    };
    while i < pairs.len() {
        let (code, kind) = pairs[i];
        if code != 0 {
            i += 1;
            continue;
        }
        if kind == "ENDSEC" || kind == "EOF" {
            break;
        }
        let mut j = i + 1;
        while j < pairs.len() && pairs[j].0 != 0 {
            j += 1;
        }
        let body = &pairs[i + 1..j];
        let get = |c: i32| body.iter().find(|p| p.0 == c).map(|p| p.1);
        let f = |c: i32| -> Result<f64, String> { get(c).map(num).unwrap_or(Ok(0.0)) };
        match kind {
            "POINT" => {
                s.add_geo(
                    Geom::Point {
                        p: v2(f(10)?, f(20)?),
                    },
                    false,
                );
            }
            "LINE" => {
                let (a, b) = (v2(f(10)?, f(20)?), v2(f(11)?, f(21)?));
                if a.dist(b) > 1e-12 {
                    s.add_geo(Geom::Line { a, b }, false);
                }
            }
            "CIRCLE" => {
                let r = f(40)?;
                if r <= 0.0 {
                    return Err("A DXF circle has no radius".into());
                }
                s.add_geo(
                    Geom::Circle {
                        c: v2(f(10)?, f(20)?),
                        r,
                    },
                    false,
                );
            }
            "ARC" => {
                let r = f(40)?;
                let a0 = math::radians(f(50)?);
                let mut a1 = math::radians(f(51)?);
                while a1 <= a0 {
                    a1 += math::TAU;
                }
                let g = Geom::Arc {
                    c: v2(f(10)?, f(20)?),
                    r,
                    start: a0,
                    end: a1,
                };
                s.add_geo(g.with_params(&g.params()), false);
            }
            "LWPOLYLINE" => {
                let closed = get(70).and_then(|v| v.parse::<i32>().ok()).unwrap_or(0) & 1 == 1;
                let mut pts: Vec<(V2, f64)> = Vec::new();
                for (k, (c, v)) in body.iter().enumerate() {
                    if *c == 10 {
                        let x = num(v)?;
                        let y = body[k + 1..]
                            .iter()
                            .find(|p| p.0 == 20)
                            .map(|p| num(p.1))
                            .unwrap_or(Ok(0.0))?;
                        let bulge = body[k + 1..]
                            .iter()
                            .take_while(|p| p.0 != 10)
                            .find(|p| p.0 == 42)
                            .map(|p| num(p.1))
                            .unwrap_or(Ok(0.0))?;
                        pts.push((v2(x, y), bulge));
                    }
                }
                let n = pts.len();
                let segs = if closed { n } else { n.saturating_sub(1) };
                for k in 0..segs {
                    let (a, bulge) = pts[k];
                    let b = pts[(k + 1) % n].0;
                    if a.dist(b) < 1e-12 {
                        continue;
                    }
                    if bulge.abs() < 1e-12 {
                        s.add_geo(Geom::Line { a, b }, false);
                    } else {
                        // Bulge = tan(sweep / 4), positive counter-clockwise.
                        let sweep = 4.0 * math::atan(bulge);
                        let chord = a.dist(b);
                        let r = chord / (2.0 * math::sin(sweep.abs() / 2.0));
                        let mid = a.lerp(b, 0.5);
                        let h = r * math::cos(sweep.abs() / 2.0);
                        let dir = (b - a).norm().perp();
                        let c = if (sweep > 0.0) == (sweep.abs() < math::PI) {
                            mid + dir * h
                        } else {
                            mid - dir * h
                        };
                        let (sa, sb) = ((a - c).angle(), (b - c).angle());
                        let (start, mut end) = if sweep > 0.0 { (sa, sb) } else { (sb, sa) };
                        while end <= start {
                            end += math::TAU;
                        }
                        let g = Geom::Arc {
                            c,
                            r: r.abs(),
                            start,
                            end,
                        };
                        s.add_geo(g.with_params(&g.params()), false);
                    }
                }
            }
            _ => skipped += 1,
        }
        i = j;
    }
    Ok((s, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sketch::tools;
    #[test]
    fn stl_round_trips_in_both_encodings() {
        let m = Mesh::cuboid(v3(0.0, 0.0, 0.0), v3(1.5, 2.25, 3.125));
        let back = read_stl(stl_ascii(&m, "Part").as_bytes()).unwrap();
        assert_eq!(back.tris.len(), 12);
        assert!(back.is_watertight());
        assert_eq!(back.volume(), m.volume());
        let bin = stl_binary(&m, "binary STL");
        assert_eq!(bin.len(), 84 + 50 * 12);
        let back = read_stl(&bin).unwrap();
        assert!(back.is_watertight());
        assert!((back.volume() - m.volume()).abs() < 1e-5);
        assert!(read_stl(b"not a mesh").is_err());
    }
    #[test]
    fn obj_round_trips_and_reads_polygons() {
        let m = Mesh::cuboid(v3(-1.0, -1.0, -1.0), v3(1.0, 1.0, 1.0));
        let back = read_obj(&obj(&m, "Cube")).unwrap();
        assert_eq!(back.tris.len(), m.tris.len());
        assert_eq!(back.verts.len(), m.verts.len());
        for i in 0..m.tris.len() {
            assert_eq!(
                back.tri(i),
                m.tri(i),
                "triangle {i} kept its corners and winding"
            );
        }
        assert!(back.is_watertight());
        let quad = "v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nf 1/1/1 2/2/1 3/3/1 -1/4/1\n";
        assert_eq!(read_obj(quad).unwrap().tris.len(), 2);
        assert!(read_obj("f 1 2 3\n").is_err());
    }
    #[test]
    fn dxf_round_trips_sketch_geometry() {
        let mut s = Sketch::default();
        tools::rectangle(&mut s, v2(0.0, 0.0), v2(20.0, 10.0), false).unwrap();
        tools::circle(&mut s, v2(5.0, 5.0), 2.0, false).unwrap();
        tools::arc_center(&mut s, v2(15.0, 5.0), v2(17.0, 5.0), v2(15.0, 7.0), false).unwrap();
        tools::line(&mut s, v2(0.0, 0.0), v2(20.0, 10.0), true, false).unwrap();
        let text = dxf(&s);
        let (back, skipped) = read_dxf(&text).unwrap();
        assert_eq!(skipped, 0);
        // The construction line is not exported.
        assert_eq!(back.geos.len(), 6);
        for (a, b) in s.geos.iter().zip(&back.geos) {
            for (x, y) in a.geom.params().iter().zip(b.geom.params()) {
                assert!((x - y).abs() < 1e-12, "{:?} vs {:?}", a.geom, b.geom);
            }
        }
    }
    #[test]
    fn a_bulged_polyline_becomes_lines_and_an_arc() {
        let text = "0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n8\n0\n70\n1\n10\n0\n20\n0\n10\n10\n20\n0\n42\n1\n10\n10\n20\n10\n10\n0\n20\n10\n0\nENDSEC\n0\nEOF\n";
        let (s, _) = read_dxf(text).unwrap();
        assert_eq!(s.geos.len(), 4);
        let Geom::Arc { c, r, .. } = s.geos[1].geom else {
            panic!("{:?}", s.geos[1])
        };
        assert!((c - v2(10.0, 5.0)).len() < 1e-9 && (r - 5.0).abs() < 1e-9);
    }
    #[test]
    fn native_documents_round_trip() {
        let (doc, _) = crate::document::with_body("Bracket");
        let text = save_native(&doc);
        assert!(text.contains("\"ProgramName\": \"FreeCAD\""));
        assert_eq!(load_native(&text).unwrap(), doc);
        assert!(load_native("{}").is_err());
    }
}
