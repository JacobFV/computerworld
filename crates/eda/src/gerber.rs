//! Fabrication outputs: Gerber RS-274X (with X2 file attributes) for each board layer
//! and an Excellon drill file, plus parsers that read them back and check the syntax a
//! board house's CAM tool would reject.
use crate::footprints::{PadKind, PadShape};
use crate::geom::{mm, Pt};
use crate::pcb::{Board, DrawShape, Layer};
use std::collections::BTreeMap;

const VERSION: &str = "8.0.4";

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Aperture {
    Circle(i64),
    Rect(i64, i64),
    Obround(i64, i64),
}
impl Aperture {
    fn definition(&self, code: usize) -> String {
        let f = |v: i64| format!("{:.6}", v as f64 / 1e6);
        match self {
            Self::Circle(d) => format!("%ADD{code}C,{}*%", f(*d)),
            Self::Rect(w, h) => format!("%ADD{code}R,{}X{}*%", f(*w), f(*h)),
            Self::Obround(w, h) => format!("%ADD{code}O,{}X{}*%", f(*w), f(*h)),
        }
    }
}

enum Op {
    Flash(Aperture, Pt),
    Draw(Aperture, Pt, Pt),
    Region(Vec<Pt>),
}

/// Layer file names KiCad's plotter uses with "Use Protel filename extensions" off.
pub fn file_name(project: &str, layer: Layer) -> String {
    format!("{project}-{}.gbr", layer.name().replace('.', "_"))
}
fn file_function(layer: Layer) -> (&'static str, &'static str) {
    match layer {
        Layer::FCu => ("Copper,L1,Top", "Positive"),
        Layer::BCu => ("Copper,L2,Bot", "Positive"),
        Layer::FMask => ("Soldermask,Top", "Negative"),
        Layer::BMask => ("Soldermask,Bot", "Negative"),
        Layer::FPaste => ("Paste,Top", "Positive"),
        Layer::BPaste => ("Paste,Bot", "Positive"),
        Layer::FSilkS => ("Legend,Top", "Positive"),
        Layer::BSilkS => ("Legend,Bot", "Positive"),
        Layer::EdgeCuts => ("Profile,NP", "Positive"),
        Layer::FCrtYd | Layer::BCrtYd => ("Other,User", "Positive"),
        Layer::FFab | Layer::BFab => ("Other,User", "Positive"),
    }
}

fn pad_aperture(shape: PadShape, (w, h): (i64, i64)) -> Aperture {
    match shape {
        PadShape::Circle => Aperture::Circle(w),
        PadShape::Oval if w == h => Aperture::Circle(w),
        PadShape::Oval => Aperture::Obround(w, h),
        PadShape::Rect | PadShape::RoundRect => Aperture::Rect(w, h),
    }
}

fn operations(board: &Board, layer: Layer) -> Vec<Op> {
    let mut ops = Vec::new();
    let copper = layer.is_copper();
    let mask = matches!(layer, Layer::FMask | Layer::BMask);
    let paste = matches!(layer, Layer::FPaste | Layer::BPaste);
    for f in &board.footprints {
        for pad in &f.pads {
            let side_copper = if f.back { Layer::BCu } else { Layer::FCu };
            let on = if copper {
                pad.kind == PadKind::ThroughHole || side_copper == layer
            } else if mask {
                pad.kind == PadKind::ThroughHole || f.layer(Layer::FMask) == layer
            } else if paste {
                pad.kind == PadKind::Smd && f.layer(Layer::FPaste) == layer
            } else {
                false
            };
            if on {
                ops.push(Op::Flash(
                    pad_aperture(pad.shape, f.pad_size(pad)),
                    f.pad_pos(pad),
                ));
            }
        }
        for l in &f.lines {
            if f.layer(l.layer) == layer {
                ops.push(Op::Draw(
                    Aperture::Circle(l.width),
                    f.to_board(l.a),
                    f.to_board(l.b),
                ));
            }
        }
        // Reference designators on the silkscreen, as stroke text.
        if f.layer(Layer::FSilkS) == layer {
            let b = f.bounds();
            let h = 1_000_000;
            let w = crate::font::width(&f.reference, h);
            let origin = if f.back {
                (b.center().x + w / 2, b.min.y - h - 400_000)
            } else {
                (b.center().x - w / 2, b.min.y - h - 400_000)
            };
            for stroke in crate::font::strokes(&f.reference, origin, h, f.back) {
                for pair in stroke.windows(2) {
                    ops.push(Op::Draw(
                        Aperture::Circle(150_000),
                        Pt::new(pair[0].0, pair[0].1),
                        Pt::new(pair[1].0, pair[1].1),
                    ));
                }
            }
        }
    }
    if copper {
        for t in board.tracks.iter().filter(|t| t.layer == layer) {
            ops.push(Op::Draw(Aperture::Circle(t.width), t.a, t.b));
        }
        for v in &board.vias {
            ops.push(Op::Flash(Aperture::Circle(v.diameter), v.pos));
        }
        for z in board.zones.iter().filter(|z| z.layer == layer) {
            for r in &z.fill {
                ops.push(Op::Region(vec![
                    r.min,
                    Pt::new(r.max.x, r.min.y),
                    r.max,
                    Pt::new(r.min.x, r.max.y),
                    r.min,
                ]));
            }
        }
    }
    for d in board.drawings.iter().filter(|d| d.layer == layer) {
        match d.shape {
            DrawShape::Line { a, b } => ops.push(Op::Draw(Aperture::Circle(d.width), a, b)),
            DrawShape::Rect { a, b } => {
                let c = [a, Pt::new(b.x, a.y), b, Pt::new(a.x, b.y)];
                for i in 0..4 {
                    ops.push(Op::Draw(Aperture::Circle(d.width), c[i], c[(i + 1) % 4]));
                }
            }
        }
    }
    ops
}

fn xy(p: Pt) -> String {
    // Gerber's Y axis points up; the board's points down.
    format!("X{}Y{}", p.x, -p.y)
}

/// One layer as an RS-274X file (format 4.6, millimetres).
pub fn layer(board: &Board, project: &str, layer: Layer) -> String {
    let ops = operations(board, layer);
    let mut apertures: BTreeMap<Aperture, usize> = BTreeMap::new();
    for op in &ops {
        let a = match op {
            Op::Flash(a, _) | Op::Draw(a, _, _) => a.clone(),
            Op::Region(_) => continue,
        };
        let next = 10 + apertures.len();
        apertures.entry(a).or_insert(next);
    }
    // Aperture numbers in first-use order read better; renumber by insertion.
    let (function, polarity) = file_function(layer);
    let mut out = vec![
        format!("%TF.GenerationSoftware,KiCad,Pcbnew,{VERSION}*%"),
        format!(
            "%TF.ProjectId,{project},{},rev?*%",
            board.uuid.replace('-', "")
        ),
        "%TF.SameCoordinates,Original*%".into(),
        format!("%TF.FileFunction,{function}*%"),
        format!("%TF.FilePolarity,{polarity}*%"),
        "%FSLAX46Y46*%".into(),
        "G04 Gerber Fmt 4.6, Leading zero omitted, Abs format (unit mm)*".into(),
        format!("G04 Created by KiCad (PCBNEW {VERSION})*"),
        "%MOMM*%".into(),
        "%LPD*%".into(),
        "G01*".into(),
        "G04 APERTURE LIST*".into(),
    ];
    let mut defs: Vec<(&Aperture, &usize)> = apertures.iter().collect();
    defs.sort_by_key(|(_, code)| **code);
    for (a, code) in defs {
        out.push(a.definition(*code));
    }
    out.push("G04 APERTURE END LIST*".into());
    let mut current: Option<usize> = None;
    let mut select = |a: &Aperture, out: &mut Vec<String>| {
        let code = apertures[a];
        if current != Some(code) {
            out.push(format!("D{code}*"));
            current = Some(code);
        }
    };
    for op in &ops {
        match op {
            Op::Flash(a, p) => {
                select(a, &mut out);
                out.push(format!("{}D03*", xy(*p)));
            }
            Op::Draw(a, p, q) => {
                select(a, &mut out);
                out.push(format!("{}D02*", xy(*p)));
                out.push(format!("{}D01*", xy(*q)));
            }
            Op::Region(pts) => {
                out.push("G36*".into());
                out.push(format!("{}D02*", xy(pts[0])));
                for p in &pts[1..] {
                    out.push(format!("{}D01*", xy(*p)));
                }
                out.push("G37*".into());
            }
        }
    }
    out.push("M02*".into());
    out.join("\n") + "\n"
}

/// The layers Fabrication Outputs ▸ Gerbers plots by default.
pub const DEFAULT_LAYERS: [Layer; 8] = [
    Layer::FCu,
    Layer::BCu,
    Layer::FPaste,
    Layer::FSilkS,
    Layer::BSilkS,
    Layer::FMask,
    Layer::BMask,
    Layer::EdgeCuts,
];

/// Excellon drill file for every plated hole (pads and vias), metric, decimal format.
pub fn drill(board: &Board, project: &str) -> String {
    let mut tools: BTreeMap<i64, Vec<Pt>> = BTreeMap::new();
    for f in &board.footprints {
        for p in &f.pads {
            if p.kind == PadKind::ThroughHole {
                tools.entry(p.drill).or_default().push(f.pad_pos(p));
            }
        }
    }
    for v in &board.vias {
        tools.entry(v.drill).or_default().push(v.pos);
    }
    let mut out = vec![
        "M48".to_owned(),
        format!("; DRILL file {{KiCad {VERSION}}} project {project}"),
        "; FORMAT={-:-/ absolute / metric / decimal}".into(),
        format!("; #@! TF.GenerationSoftware,Kicad,Pcbnew,{VERSION}"),
        "; #@! TF.FileFunction,Plated,1,2,PTH".into(),
        "FMAT,2".into(),
        "METRIC".into(),
    ];
    for (i, (d, _)) in tools.iter().enumerate() {
        out.push(format!("T{}C{:.3}", i + 1, *d as f64 / 1e6));
    }
    out.push("%".into());
    out.push("G90".into());
    out.push("G05".into());
    for (i, (_, holes)) in tools.iter().enumerate() {
        out.push(format!("T{}", i + 1));
        let mut holes = holes.clone();
        holes.sort();
        for h in holes {
            out.push(format!("X{}Y{}", mm(h.x, 1_000_000), mm(-h.y, 1_000_000)));
        }
    }
    out.push("M30".into());
    out.join("\n") + "\n"
}

/// What a Gerber file contains, as read back by `parse`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GerberStats {
    pub apertures: usize,
    pub flashes: usize,
    pub draws: usize,
    pub regions: usize,
    pub file_function: String,
    /// Bounding box of every coordinate, in nanometres, Gerber orientation.
    pub min: (i64, i64),
    pub max: (i64, i64),
}

/// Read a Gerber file strictly: format and units declared before any coordinate,
/// apertures defined before they are selected, D01/D02/D03 only with coordinates,
/// regions closed and balanced, and M02 last.
pub fn parse(text: &str) -> Result<GerberStats, String> {
    let mut stats = GerberStats {
        min: (i64::MAX, i64::MAX),
        max: (i64::MIN, i64::MIN),
        ..Default::default()
    };
    let mut format = false;
    let mut units = false;
    let mut defined = std::collections::BTreeSet::new();
    let mut aperture: Option<u32> = None;
    let mut in_region = false;
    let mut contour: Vec<(i64, i64)> = Vec::new();
    let mut ended = false;
    let (mut x, mut y) = (0i64, 0i64);
    let mut rest = text;
    while !rest.trim().is_empty() {
        if ended {
            return Err("content after M02".into());
        }
        let trimmed = rest.trim_start();
        if let Some(body) = trimmed.strip_prefix('%') {
            let end = body.find('%').ok_or("unterminated extended command")?;
            let cmd = &body[..end];
            rest = &body[end + 1..];
            if !cmd.ends_with('*') {
                return Err(format!("extended command without '*': {cmd}"));
            }
            let cmd = cmd.trim_end_matches('*');
            if cmd.starts_with("FSLA") {
                if cmd != "FSLAX46Y46" {
                    return Err(format!("unsupported format {cmd}"));
                }
                format = true;
            } else if cmd == "MOMM" || cmd == "MOIN" {
                units = true;
            } else if let Some(def) = cmd.strip_prefix("ADD") {
                let digits: String = def.chars().take_while(|c| c.is_ascii_digit()).collect();
                let code: u32 = digits.parse().map_err(|_| format!("bad aperture {cmd}"))?;
                if code < 10 {
                    return Err(format!("aperture number {code} is reserved"));
                }
                let tmpl = &def[digits.len()..];
                let (kind, params) = tmpl
                    .split_once(',')
                    .ok_or(format!("aperture {code} has no parameters"))?;
                if !matches!(kind, "C" | "R" | "O" | "P") {
                    return Err(format!("unknown aperture template {kind}"));
                }
                for p in params.split('X') {
                    let v: f64 = p
                        .parse()
                        .map_err(|_| format!("bad aperture parameter {p}"))?;
                    if v < 0.0 {
                        return Err("negative aperture size".into());
                    }
                }
                defined.insert(code);
                stats.apertures += 1;
            } else if let Some(attr) = cmd.strip_prefix("TF.FileFunction,") {
                stats.file_function = attr.to_owned();
            } else if !(["TF", "TA", "TO", "TD"].iter().any(|p| cmd.starts_with(p))
                || cmd == "LPD"
                || cmd == "LPC")
            {
                // Attributes and polarity are accepted; anything else is not RS-274X
                // this reader knows, so the file is refused rather than half-read.
                return Err(format!("unsupported extended command {cmd}"));
            }
            continue;
        }
        let end = trimmed.find('*').ok_or("word without terminating '*'")?;
        let word = trimmed[..end].trim();
        rest = &trimmed[end + 1..];
        if word.starts_with("G04") {
            continue;
        }
        match word {
            "G01" | "G75" => continue,
            "G36" => {
                if in_region {
                    return Err("G36 inside a region".into());
                }
                in_region = true;
                contour.clear();
                continue;
            }
            "G37" => {
                if !in_region {
                    return Err("G37 without G36".into());
                }
                if contour.len() < 4 || contour.first() != contour.last() {
                    return Err("region contour is not closed".into());
                }
                in_region = false;
                stats.regions += 1;
                continue;
            }
            "M02" => {
                if in_region {
                    return Err("file ends inside a region".into());
                }
                ended = true;
                continue;
            }
            _ => {}
        }
        if let Some(code) = word
            .strip_prefix('D')
            .filter(|c| c.chars().all(|ch| ch.is_ascii_digit()))
        {
            let code: u32 = code.parse().map_err(|_| format!("bad word {word}"))?;
            if !defined.contains(&code) {
                return Err(format!("aperture D{code} selected before it is defined"));
            }
            aperture = Some(code);
            continue;
        }
        if !(word.starts_with('X') || word.starts_with('Y')) {
            return Err(format!("unrecognised word {word}"));
        }
        if !format || !units {
            return Err("coordinates before %FS% and %MO%".into());
        }
        let op = &word[word.len().saturating_sub(3)..];
        let coords = &word[..word.len().saturating_sub(3)];
        let mut nx = x;
        let mut ny = y;
        let mut s = coords;
        while !s.is_empty() {
            let axis = s.as_bytes()[0];
            let tail = &s[1..];
            let len = tail.find(['X', 'Y']).unwrap_or(tail.len());
            let v: i64 = tail[..len]
                .parse()
                .map_err(|_| format!("bad coordinate in {word}"))?;
            match axis {
                b'X' => nx = v,
                b'Y' => ny = v,
                _ => return Err(format!("bad coordinate in {word}")),
            }
            s = &tail[len..];
        }
        match op {
            "D01" => {
                if in_region {
                    if contour.is_empty() {
                        contour.push((x, y));
                    }
                    contour.push((nx, ny));
                } else {
                    if aperture.is_none() {
                        return Err("draw with no aperture selected".into());
                    }
                    stats.draws += 1;
                }
            }
            "D02" => {
                if in_region {
                    if contour.len() > 1 && contour.first() != contour.last() {
                        return Err("region contour is not closed".into());
                    }
                    contour = vec![(nx, ny)];
                }
            }
            "D03" => {
                if in_region {
                    return Err("flash inside a region".into());
                }
                if aperture.is_none() {
                    return Err("flash with no aperture selected".into());
                }
                stats.flashes += 1;
            }
            _ => return Err(format!("coordinate word without operation: {word}")),
        }
        x = nx;
        y = ny;
        stats.min = (stats.min.0.min(x), stats.min.1.min(y));
        stats.max = (stats.max.0.max(x), stats.max.1.max(y));
    }
    if !ended {
        return Err("missing M02".into());
    }
    Ok(stats)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrillStats {
    pub metric: bool,
    /// Tool number → diameter in millimetres.
    pub tools: BTreeMap<u32, String>,
    pub holes: usize,
}
/// Read an Excellon file back: header, tool table, then hits under a selected tool.
pub fn parse_drill(text: &str) -> Result<DrillStats, String> {
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    if lines.next() != Some("M48") {
        return Err("drill file must start with M48".into());
    }
    let mut stats = DrillStats::default();
    let mut header = true;
    let mut tool: Option<u32> = None;
    let mut ended = false;
    for line in lines {
        if ended {
            return Err("content after M30".into());
        }
        if line.starts_with(';') {
            continue;
        }
        if header {
            match line {
                "METRIC" | "METRIC,TZ" | "METRIC,LZ" => stats.metric = true,
                "INCH" | "INCH,TZ" | "INCH,LZ" => stats.metric = false,
                "FMAT,2" => {}
                "%" | "M95" => header = false,
                t if t.starts_with('T') => {
                    let (num, dia) = t[1..].split_once('C').ok_or(format!("bad tool {t}"))?;
                    let n: u32 = num.parse().map_err(|_| format!("bad tool {t}"))?;
                    let d: f64 = dia.parse().map_err(|_| format!("bad diameter {t}"))?;
                    if d <= 0.0 {
                        return Err(format!("tool T{n} has no diameter"));
                    }
                    stats.tools.insert(n, dia.to_owned());
                }
                other => return Err(format!("unexpected header line {other}")),
            }
            continue;
        }
        match line {
            "G90" | "G05" => {}
            "M30" => ended = true,
            t if t.starts_with('T') => {
                let n: u32 = t[1..].parse().map_err(|_| format!("bad tool select {t}"))?;
                if !stats.tools.contains_key(&n) {
                    return Err(format!("tool T{n} used but not defined"));
                }
                tool = Some(n);
            }
            t if t.starts_with('X') || t.starts_with('Y') => {
                if tool.is_none() {
                    return Err("hole before any tool is selected".into());
                }
                let body = t.replace('Y', " ");
                for part in body.trim_start_matches('X').split(' ') {
                    part.parse::<f64>()
                        .map_err(|_| format!("bad coordinate {t}"))?;
                }
                stats.holes += 1;
            }
            other => return Err(format!("unexpected line {other}")),
        }
    }
    if header {
        return Err("header never ends".into());
    }
    if !ended {
        return Err("missing M30".into());
    }
    Ok(stats)
}
