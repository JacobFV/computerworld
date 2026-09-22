//! Footprint library: KiCad's standard footprint names with their real pad geometry,
//! courtyards, silkscreen and fabrication outlines. Units are nanometres, origin at
//! pad 1 as KiCad's through-hole footprints place it.
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

pub const MM: i64 = 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PadKind {
    ThroughHole,
    Smd,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PadShape {
    Circle,
    Rect,
    Oval,
    RoundRect,
}
impl PadShape {
    pub fn keyword(self) -> &'static str {
        match self {
            Self::Circle => "circle",
            Self::Rect => "rect",
            Self::Oval => "oval",
            Self::RoundRect => "roundrect",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "circle" => Self::Circle,
            "rect" => Self::Rect,
            "oval" => Self::Oval,
            "roundrect" => Self::RoundRect,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibPad {
    pub number: String,
    pub kind: PadKind,
    pub shape: PadShape,
    pub at: (i64, i64),
    pub size: (i64, i64),
    pub drill: i64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibFootprint {
    pub id: String,
    pub description: String,
    pub pads: Vec<LibPad>,
    /// Silkscreen strokes (front side), in footprint coordinates.
    pub silk: Vec<((i64, i64), (i64, i64))>,
    /// Courtyard rectangle.
    pub courtyard: ((i64, i64), (i64, i64)),
    /// Fabrication-layer body outline.
    pub fab: ((i64, i64), (i64, i64)),
    /// Height of the component body above the board, for the 3D viewer.
    #[serde(default)]
    pub height: i64,
}
impl LibFootprint {
    pub fn library(&self) -> &str {
        self.id.split(':').next().unwrap_or("")
    }
    pub fn name(&self) -> &str {
        self.id.split(':').nth(1).unwrap_or(&self.id)
    }
    /// An empty footprint for the Footprint Editor's New Footprint.
    pub fn blank(id: &str) -> LibFootprint {
        LibFootprint {
            id: id.into(),
            description: String::new(),
            pads: vec![],
            silk: vec![],
            courtyard: ((-MM, -MM), (MM, MM)),
            fab: ((-MM / 2, -MM / 2), (MM / 2, MM / 2)),
            height: MM,
        }
    }
    /// Recompute the courtyard as KiCad's checker expects it: the pads and body with a
    /// 0.25 mm margin, on a 0.01 mm grid.
    pub fn fit_courtyard(&mut self) {
        let mut lo = (
            self.fab.0 .0.min(self.fab.1 .0),
            self.fab.0 .1.min(self.fab.1 .1),
        );
        let mut hi = (
            self.fab.0 .0.max(self.fab.1 .0),
            self.fab.0 .1.max(self.fab.1 .1),
        );
        for p in &self.pads {
            lo = (
                lo.0.min(p.at.0 - p.size.0 / 2),
                lo.1.min(p.at.1 - p.size.1 / 2),
            );
            hi = (
                hi.0.max(p.at.0 + p.size.0 / 2),
                hi.1.max(p.at.1 + p.size.1 / 2),
            );
        }
        let m = 250_000;
        let down = |v: i64| (v - m).div_euclid(10_000) * 10_000;
        let up = |v: i64| -(-(v + m)).div_euclid(10_000) * 10_000;
        self.courtyard = ((down(lo.0), down(lo.1)), (up(hi.0), up(hi.1)));
    }
}

/// A body height for a footprint whose library does not give one: low for surface
/// mount parts, a typical radial height for through-hole ones.
pub fn default_height(pads: &[LibPad]) -> i64 {
    if pads.iter().any(|p| p.kind == PadKind::ThroughHole) {
        3 * MM
    } else {
        MM
    }
}

fn mm(v: f64) -> i64 {
    // Library literals are written in millimetres with at most four decimals; convert
    // through an integer count of 0.1 µm so nothing depends on float formatting.
    let tenths = if v >= 0.0 {
        (v * 10_000.0 + 0.5) as i64
    } else {
        -((-v * 10_000.0 + 0.5) as i64)
    };
    tenths * 100
}
fn p(x: f64, y: f64) -> (i64, i64) {
    (mm(x), mm(y))
}
fn tht(number: &str, shape: PadShape, at: (f64, f64), size: (f64, f64), drill: f64) -> LibPad {
    LibPad {
        number: number.into(),
        kind: PadKind::ThroughHole,
        shape,
        at: p(at.0, at.1),
        size: p(size.0, size.1),
        drill: mm(drill),
    }
}
fn smd(number: &str, at: (f64, f64), size: (f64, f64)) -> LibPad {
    LibPad {
        number: number.into(),
        kind: PadKind::Smd,
        shape: PadShape::RoundRect,
        at: p(at.0, at.1),
        size: p(size.0, size.1),
        drill: 0,
    }
}
fn outline(a: (f64, f64), b: (f64, f64)) -> Vec<((i64, i64), (i64, i64))> {
    let (x0, y0, x1, y1) = (a.0, a.1, b.0, b.1);
    vec![
        (p(x0, y0), p(x1, y0)),
        (p(x1, y0), p(x1, y1)),
        (p(x1, y1), p(x0, y1)),
        (p(x0, y1), p(x0, y0)),
    ]
}

fn axial(
    id: &'static str,
    description: &'static str,
    pitch: f64,
    body: (f64, f64),
    first: PadShape,
    second: PadShape,
) -> LibFootprint {
    let (len, dia) = body;
    let x0 = (pitch - len) / 2.0;
    let mut silk = outline(
        (x0 - 0.12, -dia / 2.0 - 0.12),
        (x0 + len + 0.12, dia / 2.0 + 0.12),
    );
    silk.push((p(1.04, 0.0), p(x0 - 0.12, 0.0)));
    silk.push((p(pitch - 1.04, 0.0), p(x0 + len + 0.12, 0.0)));
    LibFootprint {
        id: id.into(),
        description: description.into(),
        pads: vec![
            tht("1", first, (0.0, 0.0), (1.6, 1.6), 0.8),
            tht("2", second, (pitch, 0.0), (1.6, 1.6), 0.8),
        ],
        silk,
        courtyard: (
            p(-1.05, -dia / 2.0 - 0.37),
            p(pitch + 1.05, dia / 2.0 + 0.37),
        ),
        fab: (p(x0, -dia / 2.0), p(x0 + len, dia / 2.0)),
        height: 0,
    }
}
fn chip(
    id: &'static str,
    description: &'static str,
    pad_x: f64,
    pad: (f64, f64),
    court: (f64, f64),
) -> LibFootprint {
    LibFootprint {
        id: id.into(),
        description: description.into(),
        pads: vec![smd("1", (-pad_x, 0.0), pad), smd("2", (pad_x, 0.0), pad)],
        silk: vec![
            (p(-0.227, -0.735), p(0.227, -0.735)),
            (p(-0.227, 0.735), p(0.227, 0.735)),
        ],
        courtyard: (p(-court.0, -court.1), p(court.0, court.1)),
        fab: (p(-1.0, -0.625), p(1.0, 0.625)),
        height: 0,
    }
}

fn build() -> Vec<LibFootprint> {
    use PadShape::*;
    let mut lib = vec![
        axial(
            "Resistor_THT:R_Axial_DIN0207_L6.3mm_D2.5mm_P10.16mm_Horizontal",
            "Resistor, Axial_DIN0207 series, Axial, Horizontal, pin pitch=10.16mm, 0.25W",
            10.16,
            (6.3, 2.5),
            Circle,
            Oval,
        ),
        chip(
            "Resistor_SMD:R_0805_2012Metric",
            "Resistor SMD 0805 (2012 Metric), square (rectangular) end terminal",
            0.9125,
            (1.025, 1.4),
            (1.68, 0.95),
        ),
        chip(
            "Capacitor_SMD:C_0805_2012Metric",
            "Capacitor SMD 0805 (2012 Metric), square (rectangular) end terminal",
            0.95,
            (1.0, 1.45),
            (1.7, 0.98),
        ),
        axial(
            "Inductor_THT:L_Axial_L5.3mm_D2.2mm_P10.16mm_Horizontal_Vishay_IM-1",
            "Inductor, Axial series, Axial, Horizontal, pin pitch=10.16mm, length*diameter=5.3*2.2mm^2",
            10.16,
            (5.3, 2.2),
            Circle,
            Oval,
        ),
        axial(
            "Diode_THT:D_DO-35_SOD27_P7.62mm_Horizontal",
            "Diode, DO-35_SOD27 series, Axial, Horizontal, pin pitch=7.62mm",
            7.62,
            (4.0, 2.0),
            Rect,
            Oval,
        ),
    ];
    lib.push(LibFootprint {
        id: "Capacitor_THT:C_Disc_D5.0mm_W2.5mm_P5.00mm".into(),
        description: "C, Disc series, Radial, pin pitch=5.00mm, diameter*width=5*2.5mm^2".into(),
        pads: vec![
            tht("1", Circle, (0.0, 0.0), (1.6, 1.6), 0.8),
            tht("2", Circle, (5.0, 0.0), (1.6, 1.6), 0.8),
        ],
        silk: outline((-0.12, -1.37), (5.12, 1.37)),
        courtyard: (p(-1.05, -1.5), p(6.05, 1.5)),
        fab: (p(0.0, -1.25), p(5.0, 1.25)),
        height: 0,
    });
    lib.push(LibFootprint {
        id: "Capacitor_THT:CP_Radial_D5.0mm_P2.00mm".into(),
        description:
            "CP, Radial series, Radial, pin pitch=2.00mm, diameter=5mm, Electrolytic Capacitor"
                .into(),
        pads: vec![
            tht("1", Rect, (0.0, 0.0), (1.6, 1.6), 0.8),
            tht("2", Circle, (2.0, 0.0), (1.6, 1.6), 0.8),
        ],
        silk: {
            let mut s = outline((-1.62, -2.62), (3.62, 2.62));
            s.push((p(-1.6, -2.2), p(-1.1, -2.2)));
            s.push((p(-1.35, -2.45), p(-1.35, -1.95)));
            s
        },
        courtyard: (p(-1.75, -2.75), p(3.75, 2.75)),
        fab: (p(-1.5, -2.5), p(3.5, 2.5)),
        height: 0,
    });
    lib.push(LibFootprint {
        id: "LED_THT:LED_D5.0mm".into(),
        description: "LED, diameter 5.0mm, 2 pins".into(),
        pads: vec![
            tht("1", Rect, (0.0, 0.0), (1.8, 1.8), 0.9),
            tht("2", Circle, (2.54, 0.0), (1.8, 1.8), 0.9),
        ],
        silk: outline((-1.29, -2.62), (3.83, 2.62)),
        courtyard: (p(-1.95, -3.25), p(4.5, 3.25)),
        fab: (p(-1.23, -2.5), p(3.77, 2.5)),
        height: 0,
    });
    lib.push(LibFootprint {
        id: "Package_TO_SOT_THT:TO-92_Inline".into(),
        description: "TO-92 leads in-line, narrow, oval pads, drill 0.75mm".into(),
        pads: vec![
            tht("1", Rect, (0.0, 0.0), (1.05, 1.5), 0.75),
            tht("2", Oval, (1.27, 0.0), (1.05, 1.5), 0.75),
            tht("3", Oval, (2.54, 0.0), (1.05, 1.5), 0.75),
        ],
        silk: vec![(p(-0.53, 1.85), p(3.07, 1.85))],
        courtyard: (p(-1.46, -2.73), p(4.0, 2.01)),
        fab: (p(-0.5, -2.0), p(3.04, 1.75)),
        height: 0,
    });
    lib.push(LibFootprint {
        id: "Package_TO_SOT_SMD:SOT-23".into(),
        description:
            "SOT, 3 Pin (https://www.jedec.org/document_search?search_api_views_fulltext=to-236)"
                .into(),
        pads: vec![
            smd("1", (-0.9375, -0.95), (1.325, 0.6)),
            smd("2", (-0.9375, 0.95), (1.325, 0.6)),
            smd("3", (0.9375, 0.0), (1.325, 0.6)),
        ],
        silk: vec![
            (p(0.0, -1.56), p(0.65, -1.56)),
            (p(0.0, 1.56), p(0.65, 1.56)),
        ],
        courtyard: (p(-1.92, -1.7), p(1.92, 1.7)),
        fab: (p(-0.65, -1.45), p(0.65, 1.45)),
        height: 0,
    });
    lib.push(LibFootprint {
        id: "Package_TO_SOT_SMD:SOT-23-5".into(),
        description: "SOT, 5 Pin (https://www.jedec.org/sites/default/files/docs/Mo-178c.PDF)"
            .into(),
        pads: vec![
            smd("1", (-1.1375, -0.95), (1.325, 0.6)),
            smd("2", (-1.1375, 0.0), (1.325, 0.6)),
            smd("3", (-1.1375, 0.95), (1.325, 0.6)),
            smd("4", (1.1375, 0.95), (1.325, 0.6)),
            smd("5", (1.1375, -0.95), (1.325, 0.6)),
        ],
        silk: vec![(p(0.0, -1.56), p(0.8, -1.56)), (p(0.0, 1.56), p(0.8, 1.56))],
        courtyard: (p(-2.05, -1.7), p(2.05, 1.7)),
        fab: (p(-0.8, -1.45), p(0.8, 1.45)),
        height: 0,
    });
    lib.push(LibFootprint {
        id: "Package_DIP:DIP-8_W7.62mm".into(),
        description: "8-lead though-hole mounted DIP package, row spacing 7.62 mm (300 mils)"
            .into(),
        pads: (0..8)
            .map(|i| {
                let numbers = ["1", "2", "3", "4", "5", "6", "7", "8"];
                let (x, y) = if i < 4 {
                    (0.0, 2.54 * i as f64)
                } else {
                    (7.62, 2.54 * (7 - i) as f64)
                };
                tht(
                    numbers[i],
                    if i == 0 { Rect } else { Oval },
                    (x, y),
                    (1.6, 1.6),
                    0.8,
                )
            })
            .collect(),
        silk: {
            let mut s = vec![(p(2.81, -1.33), p(1.16, -1.33))];
            s.push((p(1.16, -1.33), p(1.16, 8.95)));
            s.push((p(1.16, 8.95), p(6.46, 8.95)));
            s.push((p(6.46, 8.95), p(6.46, -1.33)));
            s.push((p(6.46, -1.33), p(4.81, -1.33)));
            s
        },
        courtyard: (p(-1.1, -1.55), p(8.7, 9.15)),
        fab: (p(0.635, -1.27), p(6.985, 8.89)),
        height: 0,
    });
    for n in [2usize, 3] {
        let numbers = ["1", "2", "3"];
        lib.push(LibFootprint {
            id: if n == 2 {
                "Connector_PinHeader_2.54mm:PinHeader_1x02_P2.54mm_Vertical"
            } else {
                "Connector_PinHeader_2.54mm:PinHeader_1x03_P2.54mm_Vertical"
            }
            .into(),
            description: if n == 2 {
                "Through hole straight pin header, 1x02, 2.54mm pitch, single row"
            } else {
                "Through hole straight pin header, 1x03, 2.54mm pitch, single row"
            }
            .into(),
            pads: (0..n)
                .map(|i| {
                    tht(
                        numbers[i],
                        if i == 0 { Rect } else { Oval },
                        (0.0, 2.54 * i as f64),
                        (1.7, 1.7),
                        1.0,
                    )
                })
                .collect(),
            silk: outline((-1.33, 1.27), (1.33, 2.54 * (n as f64 - 1.0) + 1.33)),
            courtyard: (p(-1.8, -1.8), p(1.8, 2.54 * (n as f64 - 1.0) + 1.8)),
            fab: (p(-1.27, -1.27), p(1.27, 2.54 * (n as f64 - 1.0) + 1.27)),
            height: 0,
        });
    }
    lib.push(LibFootprint {
        id: "Button_Switch_THT:SW_PUSH_6mm".into(),
        description: "tactile push button, 6x6mm e.g. PHAP33xx series, height=4.3mm".into(),
        pads: vec![
            tht("1", Circle, (0.0, 0.0), (2.0, 2.0), 1.1),
            tht("2", Circle, (6.5, 0.0), (2.0, 2.0), 1.1),
            tht("1", Circle, (0.0, 4.5), (2.0, 2.0), 1.1),
            tht("2", Circle, (6.5, 4.5), (2.0, 2.0), 1.1),
        ],
        silk: outline((0.25, -1.0), (6.25, 5.5)),
        courtyard: (p(-1.25, -1.5), p(7.75, 6.0)),
        fab: (p(0.25, -0.75), p(6.25, 5.25)),
        height: 0,
    });
    // Body heights from the parts' datasheets (seated height above the board).
    for f in &mut lib {
        f.height = match f.name() {
            n if n.starts_with("R_Axial") => 2_500_000,
            n if n.starts_with("L_Axial") => 2_200_000,
            n if n.starts_with("D_DO-35") => 2_000_000,
            n if n.starts_with("R_0805") || n.starts_with("C_0805") => 600_000,
            n if n.starts_with("C_Disc") => 5_000_000,
            n if n.starts_with("CP_Radial") => 11_000_000,
            n if n.starts_with("LED_D5") => 8_600_000,
            n if n.starts_with("TO-92") => 4_800_000,
            n if n.starts_with("SOT-23") => 1_100_000,
            n if n.starts_with("DIP-8") => 3_900_000,
            n if n.starts_with("PinHeader") => 2_500_000,
            n if n.starts_with("SW_PUSH") => 4_300_000,
            _ => default_height(&f.pads),
        };
    }
    lib
}

pub fn library() -> &'static [LibFootprint] {
    static LIB: OnceLock<Vec<LibFootprint>> = OnceLock::new();
    LIB.get_or_init(build)
}
pub fn find(id: &str) -> Option<&'static LibFootprint> {
    library().iter().find(|f| f.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_footprint_used_by_a_symbol_exists_and_holds_its_pins() {
        for s in crate::symbols::library() {
            for fp in &s.footprints {
                let f = find(fp).unwrap_or_else(|| panic!("{} names missing {fp}", s.lib_id));
                for pin in &s.pins {
                    if pin.kind == crate::symbols::PinType::NoConnect {
                        continue;
                    }
                    assert!(
                        f.pads.iter().any(|p| p.number == pin.number),
                        "{fp} has no pad {} for {}",
                        pin.number,
                        s.lib_id
                    );
                }
            }
        }
        for f in library() {
            for pad in &f.pads {
                if pad.kind == PadKind::ThroughHole {
                    assert!(pad.drill > 0 && pad.drill < pad.size.0.min(pad.size.1));
                }
            }
        }
        assert_eq!(mm(10.16), 10_160_000);
        assert_eq!(mm(-0.9125), -912_500);
    }
}
