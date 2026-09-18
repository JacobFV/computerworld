//! Canvas drawing shared by the schematic editor, the symbol chooser's preview and the
//! board editor: world coordinates to pixels, strokes, arcs and schematic symbols in
//! KiCad's default colour theme.
use super::View;
use crate::desktop_scene::{shared::Align, Painter};
use cw_eda::geom::{Pt, Xf};
use cw_eda::schematic::Field;
use cw_eda::symbols::{Fill, Graphic, LibSymbol};
use cw_scene::Color;

pub const SCH_BG: Color = Color::rgb(245, 244, 239);
pub const SCH_GRID: Color = Color::rgb(181, 181, 181);
pub const WIRE: Color = Color::rgb(0, 150, 0);
pub const BODY: Color = Color::rgb(132, 0, 0);
pub const BODY_FILL: Color = Color::rgb(255, 255, 194);
pub const PIN_NUM: Color = Color::rgb(169, 0, 0);
pub const PIN_NAME: Color = Color::rgb(0, 100, 100);
pub const FIELD: Color = Color::rgb(0, 100, 100);
pub const LABEL: Color = Color::rgb(15, 15, 15);
pub const NO_CONNECT: Color = Color::rgb(0, 0, 132);
pub const SHEET: Color = Color::rgb(132, 0, 0);
pub const SELECT: Color = Color(0, 120, 215, 60);
pub const SELECT_EDGE: Color = Color::rgb(0, 120, 215);
/// KiCad's default theme colours for buses, sheets and hierarchical labels.
pub const BUS: Color = Color::rgb(0, 0, 132);
pub const SHEET_EDGE: Color = Color::rgb(132, 0, 132);
pub const SHEET_FILL: Color = Color::rgb(255, 255, 221);
pub const HIER: Color = Color::rgb(114, 86, 0);

/// The flag of a hierarchical label or sheet pin at `at` on side `side` (0 left, 1
/// right, 2 top, 3 bottom of what it belongs to): an arrow in for an input, out for an
/// output, a diamond for bidirectional, a box for passive. `inside` points the flag
/// into the sheet (a sheet pin) rather than away from the wire (a label).
pub fn port_shape(
    p: &mut Painter,
    cv: &Cv,
    at: Pt,
    side: u8,
    shape: cw_eda::symbols::PinType,
    c: Color,
    inside: bool,
) {
    use cw_eda::symbols::PinType;
    // Unit vector along which the flag extends, and across it.
    let (ax, ay) = match side {
        0 => (1, 0),
        1 => (-1, 0),
        2 => (0, 1),
        _ => (0, -1),
    };
    let (ax, ay) = if inside { (ax, ay) } else { (-ax, -ay) };
    let (cx, cy) = (-ay, ax);
    let q = |along: i64, across: i64| {
        Pt::new(
            at.x + ax * along + cx * across,
            at.y + ay * along + cy * across,
        )
    };
    let s = 40;
    let points_in = match shape {
        PinType::Input => !inside,
        PinType::Output => inside,
        _ => false,
    };
    let pts: Vec<Pt> = match shape {
        PinType::Input | PinType::Output => {
            if points_in {
                vec![
                    q(0, 0),
                    q(s, -s),
                    q(2 * s, -s),
                    q(2 * s, s),
                    q(s, s),
                    q(0, 0),
                ]
            } else {
                vec![q(0, -s), q(s, -s), q(2 * s, 0), q(s, s), q(0, s), q(0, -s)]
            }
        }
        PinType::Bidirectional | PinType::TriState => {
            vec![q(0, 0), q(s, -s), q(2 * s, 0), q(s, s), q(0, 0)]
        }
        _ => vec![q(0, -s), q(2 * s, -s), q(2 * s, s), q(0, s), q(0, -s)],
    };
    cv.stroke(p, &pts, c, 6, 1);
}

/// A canvas: where it sits in the window and how the world maps onto it.
#[derive(Clone, Copy)]
pub struct Cv {
    pub ox: i32,
    pub oy: i32,
    pub w: u32,
    pub h: u32,
    pub view: View,
}
impl Cv {
    pub fn pt(&self, p: Pt) -> (i32, i32) {
        let (x, y) = self.view.px(p);
        (self.ox + x, self.oy + y)
    }
    pub fn len(&self, v: i64) -> i32 {
        self.view.len(v)
    }
    pub fn stroke(&self, p: &mut Painter, pts: &[Pt], c: Color, width_world: i64, min_px: u16) {
        let w = (self.len(width_world).max(min_px as i32)).clamp(1, 60) as u16;
        p.line(pts.iter().map(|q| self.pt(*q)).collect(), c, w);
    }
    pub fn fill(&self, p: &mut Painter, pts: &[Pt], c: Color) {
        p.path(pts.iter().map(|q| self.pt(*q)).collect(), c);
    }
    /// Text of world height `size`, skipped when it would be too small to read.
    pub fn text(&self, p: &mut Painter, at: Pt, text: &str, size: i64, c: Color, align: Align) {
        let px = self.len(size).clamp(0, 200);
        if px < 6 || text.is_empty() {
            return;
        }
        let px = px as u16;
        let (x, y) = self.pt(at);
        let width = p.measure(text, px, false) + 4;
        let x = match align {
            Align::Left => x,
            Align::Center => x - width as i32 / 2,
            Align::Right => x - width as i32,
        };
        p.label(
            x,
            y - px as i32 / 2 - 2,
            width,
            text,
            px,
            c,
            false,
            Align::Left,
        );
    }
}

/// Points along the circular arc through three points, for drawing.
pub fn arc_points(a: (f64, f64), m: (f64, f64), b: (f64, f64)) -> Vec<(f64, f64)> {
    let d = 2.0 * (a.0 * (m.1 - b.1) + m.0 * (b.1 - a.1) + b.0 * (a.1 - m.1));
    if d == 0.0 {
        return vec![a, b];
    }
    let sq = |p: (f64, f64)| p.0 * p.0 + p.1 * p.1;
    let cx = (sq(a) * (m.1 - b.1) + sq(m) * (b.1 - a.1) + sq(b) * (a.1 - m.1)) / d;
    let cy = (sq(a) * (b.0 - m.0) + sq(m) * (a.0 - b.0) + sq(b) * (m.0 - a.0)) / d;
    let r = ((a.0 - cx) * (a.0 - cx) + (a.1 - cy) * (a.1 - cy)).sqrt();
    let ang = |p: (f64, f64)| cw_eda::num::atan2(p.1 - cy, p.0 - cx);
    let (sa, ma, ea) = (ang(a), ang(m), ang(b));
    let tau = 2.0 * cw_eda::num::PI;
    let norm = |x: f64| {
        let mut v = x;
        while v < 0.0 {
            v += tau;
        }
        while v >= tau {
            v -= tau;
        }
        v
    };
    // Sweep from a to b the way that passes through m.
    let ccw_to_m = norm(ma - sa);
    let ccw_to_b = norm(ea - sa);
    let sweep = if ccw_to_m <= ccw_to_b {
        ccw_to_b
    } else {
        ccw_to_b - tau
    };
    let steps = 12;
    (0..=steps)
        .map(|i| {
            let t = sa + sweep * i as f64 / steps as f64;
            (cx + r * cw_eda::num::cos(t), cy + r * cw_eda::num::sin(t))
        })
        .collect()
}

/// Draw a symbol (unit 1): body, pins, pin numbers and names, and its visible fields.
pub fn symbol(
    p: &mut Painter,
    cv: &Cv,
    lib: &LibSymbol,
    pos: Pt,
    xf: Xf,
    fields: Option<&[Field]>,
    ghost: bool,
) {
    symbol_unit(p, cv, lib, 1, pos, xf, fields, ghost);
}

/// Draw one unit of a symbol; a multi-unit part's reference shows its unit letter.
#[allow(clippy::too_many_arguments)]
pub fn symbol_unit(
    p: &mut Painter,
    cv: &Cv,
    lib: &LibSymbol,
    unit: u32,
    pos: Pt,
    xf: Xf,
    fields: Option<&[Field]>,
    ghost: bool,
) {
    let to = |(x, y): (i64, i64)| pos.add(xf.apply(Pt::new(x, -y)));
    let (body, fill, pin, num, name) = if ghost {
        let g = Color(132, 0, 0, 140);
        (g, Color(255, 255, 194, 120), g, g, g)
    } else {
        (BODY, BODY_FILL, BODY, PIN_NUM, PIN_NAME)
    };
    for g in lib.unit_graphics(unit) {
        match g {
            Graphic::Rect { a, b, fill: f } => {
                let pts = [to(*a), to((b.0, a.1)), to(*b), to((a.0, b.1)), to(*a)];
                match f {
                    Fill::Background => cv.fill(p, &pts[..4], fill),
                    Fill::Outline => cv.fill(p, &pts[..4], body),
                    Fill::None => {}
                }
                cv.stroke(p, &pts, body, 6, 1);
            }
            Graphic::Poly {
                pts,
                fill: f,
                width,
            } => {
                let q: Vec<Pt> = pts.iter().map(|x| to(*x)).collect();
                match f {
                    Fill::Background => cv.fill(p, &q, fill),
                    Fill::Outline => cv.fill(p, &q, body),
                    Fill::None => {}
                }
                cv.stroke(p, &q, body, *width, 1);
            }
            Graphic::Circle { c, r, fill: f } => {
                let (x, y) = cv.pt(to(*c));
                let rr = cv.len(*r).max(1) as u32;
                match f {
                    Fill::Background => p.circle(x, y, rr, fill),
                    Fill::Outline => p.circle(x, y, rr, body),
                    Fill::None => {}
                }
                p.ring(x, y, rr, cv.len(6).clamp(1, 4) as u32, body);
            }
            Graphic::Arc { start, mid, end } => {
                let f = |q: (i64, i64)| {
                    let (x, y) = cv.pt(to(q));
                    (x as f64, y as f64)
                };
                let pts: Vec<(i32, i32)> = arc_points(f(*start), f(*mid), f(*end))
                    .into_iter()
                    .map(|(x, y)| (x as i32, y as i32))
                    .collect();
                p.line(pts, body, cv.len(6).clamp(1, 4) as u16);
            }
            Graphic::Text { at, text } => cv.text(p, to(*at), text, 50, body, Align::Center),
        }
    }
    for pn in lib.unit_pins(unit) {
        if pn.hidden {
            continue;
        }
        let a = to(pn.at);
        let b = to(pn.inner());
        cv.stroke(p, &[a, b], pin, 6, 1);
        if !lib.pin_numbers_hidden {
            let mid = Pt::new((a.x + b.x) / 2, (a.y + b.y) / 2);
            let off = if a.y == b.y {
                Pt::new(0, -30)
            } else {
                Pt::new(-30, 0)
            };
            cv.text(p, mid.add(off), &pn.number, 40, num, Align::Center);
        }
        if !lib.pin_names_hidden && pn.name != "~" {
            // The name sits just inside the body, reading away from the pin.
            let d = b.sub(a);
            let dir = Pt::new(d.x.signum(), d.y.signum());
            let at = b.add(Pt::new(dir.x * 30, dir.y * 30));
            let align = if dir.x > 0 {
                Align::Left
            } else if dir.x < 0 {
                Align::Right
            } else {
                Align::Center
            };
            let at = if dir.y != 0 {
                b.add(Pt::new(0, dir.y * 60))
            } else {
                at
            };
            cv.text(p, at, &pn.name, 40, name, align);
        }
    }
    if let Some(fields) = fields {
        for f in fields.iter().filter(|f| f.visible) {
            if f.name != "Reference" && f.name != "Value" {
                continue;
            }
            let align = if lib.power {
                Align::Center
            } else {
                Align::Left
            };
            // A multi-unit part shows which unit this is: U1A, U1B.
            let text = if f.name == "Reference" && lib.units > 1 {
                format!("{}{}", f.value, LibSymbol::unit_letter(unit))
            } else {
                f.value.clone()
            };
            cv.text(
                p,
                pos.add(f.offset),
                &text,
                50,
                if ghost { body } else { FIELD },
                align,
            );
        }
    }
}
