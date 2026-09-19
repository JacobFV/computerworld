//! The schematic symbol library: KiCad's standard library names, pin numbering and
//! electrical types, drawn in library coordinates (mils, Y up) the way KiCad draws them.
//! Each symbol also says how it simulates and which footprint it usually takes.
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// Electrical type of a pin, which is what the electrical rules check reasons about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PinType {
    Input,
    Output,
    Bidirectional,
    TriState,
    Passive,
    Free,
    Unspecified,
    PowerIn,
    PowerOut,
    OpenCollector,
    OpenEmitter,
    NoConnect,
}
impl PinType {
    /// The keyword KiCad files use.
    pub fn keyword(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Output => "output",
            Self::Bidirectional => "bidirectional",
            Self::TriState => "tri_state",
            Self::Passive => "passive",
            Self::Free => "free",
            Self::Unspecified => "unspecified",
            Self::PowerIn => "power_in",
            Self::PowerOut => "power_out",
            Self::OpenCollector => "open_collector",
            Self::OpenEmitter => "open_emitter",
            Self::NoConnect => "no_connect",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Input => "Input",
            Self::Output => "Output",
            Self::Bidirectional => "Bidirectional",
            Self::TriState => "Tri-state",
            Self::Passive => "Passive",
            Self::Free => "Free",
            Self::Unspecified => "Unspecified",
            Self::PowerIn => "Power input",
            Self::PowerOut => "Power output",
            Self::OpenCollector => "Open collector",
            Self::OpenEmitter => "Open emitter",
            Self::NoConnect => "Unconnected",
        }
    }
    /// Pins that can set a net's level.
    pub fn drives(self) -> bool {
        matches!(
            self,
            Self::Output
                | Self::Bidirectional
                | Self::TriState
                | Self::Passive
                | Self::PowerOut
                | Self::OpenCollector
                | Self::OpenEmitter
                | Self::Unspecified
                | Self::Free
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fill {
    None,
    /// Filled with the outline colour.
    Outline,
    /// Filled with the body background colour.
    Background,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum Graphic {
    Rect {
        a: (i64, i64),
        b: (i64, i64),
        fill: Fill,
    },
    Poly {
        pts: Vec<(i64, i64)>,
        fill: Fill,
        width: i64,
    },
    Circle {
        c: (i64, i64),
        r: i64,
        fill: Fill,
    },
    /// Arc through three points.
    Arc {
        start: (i64, i64),
        mid: (i64, i64),
        end: (i64, i64),
    },
    /// Text drawn as part of the body (a "+" or an "E").
    Text {
        at: (i64, i64),
        text: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibPin {
    pub number: String,
    pub name: String,
    /// Connection point, library coordinates.
    pub at: (i64, i64),
    /// Direction from the connection point towards the body, degrees CCW (Y up).
    pub angle: u16,
    pub length: i64,
    pub kind: PinType,
    pub hidden: bool,
    /// The unit the pin belongs to, 1-based; 0 is common to every unit.
    #[serde(default)]
    pub unit: u32,
}
impl LibPin {
    /// The end of the pin that touches the body.
    pub fn inner(&self) -> (i64, i64) {
        let (x, y) = self.at;
        match self.angle {
            0 => (x + self.length, y),
            90 => (x, y + self.length),
            180 => (x - self.length, y),
            _ => (x, y - self.length),
        }
    }
}

/// The logic function of a single-gate symbol.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogicGate {
    And,
    Nand,
    Or,
    Nor,
    Xor,
    Not,
}
impl LogicGate {
    /// The XSPICE code model that computes it.
    pub fn code_model(self) -> &'static str {
        match self {
            Self::And => "d_and",
            Self::Nand => "d_nand",
            Self::Or => "d_or",
            Self::Nor => "d_nor",
            Self::Xor => "d_xor",
            Self::Not => "d_inverter",
        }
    }
}

/// How a symbol becomes SPICE.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spice {
    /// No model: simulation needs the symbol excluded.
    None,
    /// Power symbol: its pin names the net (`GND` is node 0).
    Power,
    Resistor,
    Capacitor,
    Inductor,
    Diode,
    Npn,
    Pnp,
    Nmos,
    Pmos,
    OpAmp,
    VoltageSource,
    CurrentSource,
    /// A single logic gate: inputs A (and B), output Y, powered from VCC/GND, as XSPICE
    /// digital models behind ADC bridges and a push-pull output stage.
    Gate(LogicGate),
    /// A positive-edge D flip-flop (pins D, CLK, Q, VCC, GND).
    DFlipFlop,
    /// The 555 timer as a comparator/latch/discharge macro-model.
    Timer555,
    /// A microcontroller driven by a pin script in `Sim.Params` (a behavioral model of
    /// its pins, not of its firmware).
    Mcu,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibSymbol {
    pub lib_id: String,
    pub description: String,
    pub keywords: String,
    pub reference: String,
    pub value: String,
    pub footprint: String,
    pub datasheet: String,
    pub pins: Vec<LibPin>,
    /// Body graphics common to every unit.
    pub graphics: Vec<Graphic>,
    /// Graphics that belong to one unit (1-based).
    #[serde(default)]
    pub unit_graphics: Vec<(u32, Graphic)>,
    /// Number of units (gates of a quad NAND are four); at least 1.
    #[serde(default = "one")]
    pub units: u32,
    pub power: bool,
    pub in_bom: bool,
    pub on_board: bool,
    pub spice: Spice,
    /// Default model parameters (`Sim.Params`), e.g. a diode's saturation current.
    pub sim_params: String,
    pub pin_names_hidden: bool,
    pub pin_numbers_hidden: bool,
    /// Placed instances start excluded from simulation (connectors, switches): parts
    /// that only matter on the board.
    pub exclude_from_sim: bool,
    /// Where Reference and Value sit, library coordinates.
    pub ref_at: (i64, i64),
    pub value_at: (i64, i64),
    /// Footprints that fit this symbol, first is the default.
    pub footprints: Vec<String>,
    /// User fields beyond the four KiCad always has.
    #[serde(default)]
    pub fields: Vec<(String, String)>,
}
fn one() -> u32 {
    1
}
impl LibSymbol {
    /// An empty symbol for the Symbol Editor's New Symbol.
    pub fn blank(lib_id: &str, reference: &str) -> LibSymbol {
        let name = lib_id.split(':').nth(1).unwrap_or(lib_id).to_owned();
        LibSymbol {
            lib_id: lib_id.into(),
            description: String::new(),
            keywords: String::new(),
            reference: reference.into(),
            value: name,
            footprint: String::new(),
            datasheet: "~".into(),
            pins: vec![],
            graphics: vec![],
            unit_graphics: vec![],
            units: 1,
            power: false,
            in_bom: true,
            on_board: true,
            spice: Spice::None,
            sim_params: String::new(),
            pin_names_hidden: false,
            pin_numbers_hidden: false,
            exclude_from_sim: false,
            ref_at: (0, 150),
            value_at: (0, -150),
            footprints: vec![],
            fields: vec![],
        }
    }
    pub fn library(&self) -> &str {
        self.lib_id.split(':').next().unwrap_or("")
    }
    pub fn name(&self) -> &str {
        self.lib_id.split(':').nth(1).unwrap_or(&self.lib_id)
    }
    pub fn pin(&self, number: &str) -> Option<&LibPin> {
        self.pins.iter().find(|p| p.number == number)
    }
    /// Pins drawn and connected by unit `unit` (1-based): its own and the common ones.
    pub fn unit_pins(&self, unit: u32) -> impl Iterator<Item = &LibPin> {
        let unit = unit.max(1);
        self.pins
            .iter()
            .filter(move |p| p.unit == 0 || p.unit == unit || self.units <= 1)
    }
    /// Graphics drawn by unit `unit`.
    pub fn unit_graphics(&self, unit: u32) -> impl Iterator<Item = &Graphic> {
        let unit = unit.max(1);
        self.graphics.iter().chain(
            self.unit_graphics
                .iter()
                .filter(move |(u, _)| *u == unit || *u == 0)
                .map(|(_, g)| g),
        )
    }
    /// The letter KiCad appends to a reference for unit `unit` of a multi-unit part.
    pub fn unit_letter(unit: u32) -> String {
        let mut n = unit.max(1);
        let mut s = String::new();
        while n > 0 {
            let r = ((n - 1) % 26) as u8;
            s.insert(0, (b'A' + r) as char);
            n = (n - 1) / 26;
        }
        s
    }
    /// Library-space bounding box of the body and pins.
    pub fn bounds(&self) -> ((i64, i64), (i64, i64)) {
        let mut lo = (i64::MAX, i64::MAX);
        let mut hi = (i64::MIN, i64::MIN);
        let mut add = |(x, y): (i64, i64)| {
            lo = (lo.0.min(x), lo.1.min(y));
            hi = (hi.0.max(x), hi.1.max(y));
        };
        for g in self
            .graphics
            .iter()
            .chain(self.unit_graphics.iter().map(|(_, g)| g))
        {
            match g {
                Graphic::Rect { a, b, .. } => {
                    add(*a);
                    add(*b);
                }
                Graphic::Poly { pts, .. } => pts.iter().for_each(|p| add(*p)),
                Graphic::Circle { c, r, .. } => {
                    add((c.0 - r, c.1 - r));
                    add((c.0 + r, c.1 + r));
                }
                Graphic::Arc { start, mid, end } => {
                    add(*start);
                    add(*mid);
                    add(*end);
                }
                Graphic::Text { at, .. } => add(*at),
            }
        }
        for p in &self.pins {
            if !p.hidden {
                add(p.at);
                add(p.inner());
            } else {
                add(p.at);
            }
        }
        if lo.0 > hi.0 {
            return ((0, 0), (0, 0));
        }
        (lo, hi)
    }
}

pub fn pin(
    number: &str,
    name: &str,
    at: (i64, i64),
    angle: u16,
    length: i64,
    kind: PinType,
) -> LibPin {
    LibPin {
        number: number.into(),
        name: name.into(),
        at,
        angle,
        length,
        kind,
        hidden: false,
        unit: 0,
    }
}
fn hidden(mut p: LibPin) -> LibPin {
    p.hidden = true;
    p
}
fn poly(pts: &[(i64, i64)]) -> Graphic {
    Graphic::Poly {
        pts: pts.to_vec(),
        fill: Fill::None,
        width: 10,
    }
}
fn thick(pts: &[(i64, i64)], width: i64) -> Graphic {
    Graphic::Poly {
        pts: pts.to_vec(),
        fill: Fill::None,
        width,
    }
}
fn filled(pts: &[(i64, i64)]) -> Graphic {
    Graphic::Poly {
        pts: pts.to_vec(),
        fill: Fill::Outline,
        width: 10,
    }
}
fn rect(a: (i64, i64), b: (i64, i64), fill: Fill) -> Graphic {
    Graphic::Rect { a, b, fill }
}
fn circle(c: (i64, i64), r: i64, fill: Fill) -> Graphic {
    Graphic::Circle { c, r, fill }
}
fn arc(start: (i64, i64), mid: (i64, i64), end: (i64, i64)) -> Graphic {
    Graphic::Arc { start, mid, end }
}

#[allow(clippy::too_many_arguments)]
fn sym(
    lib_id: &'static str,
    description: &'static str,
    keywords: &'static str,
    reference: &'static str,
    value: &'static str,
    footprints: &'static [&'static str],
    pins: Vec<LibPin>,
    graphics: Vec<Graphic>,
    spice: Spice,
) -> LibSymbol {
    LibSymbol {
        lib_id: lib_id.into(),
        description: description.into(),
        keywords: keywords.into(),
        reference: reference.into(),
        value: value.into(),
        footprint: footprints.first().copied().unwrap_or("").into(),
        datasheet: "~".into(),
        pins,
        graphics,
        unit_graphics: vec![],
        units: 1,
        power: false,
        in_bom: true,
        on_board: true,
        spice,
        sim_params: String::new(),
        pin_names_hidden: true,
        pin_numbers_hidden: true,
        exclude_from_sim: false,
        ref_at: (100, 50),
        value_at: (100, -50),
        footprints: footprints.iter().map(|f| (*f).to_owned()).collect(),
        fields: vec![],
    }
}

const P: PinType = PinType::Passive;
const TWO_PIN_V: fn(i64) -> Vec<LibPin> = |len| {
    vec![
        pin("1", "~", (0, 150), 270, len, P),
        pin("2", "~", (0, -150), 90, len, P),
    ]
};

fn power_symbol(name: &'static str, description: &'static str, ground: bool) -> LibSymbol {
    let lib_id: &'static str = match name {
        "GND" => "power:GND",
        "+5V" => "power:+5V",
        "+3V3" => "power:+3V3",
        "+12V" => "power:+12V",
        "VCC" => "power:VCC",
        "-5V" => "power:-5V",
        _ => "power:PWR_FLAG",
    };
    let graphics = if ground {
        vec![poly(&[
            (0, 0),
            (0, -50),
            (50, -50),
            (0, -100),
            (-50, -50),
            (0, -50),
        ])]
    } else if name.starts_with('-') {
        vec![
            poly(&[(0, 0), (0, -50)]),
            poly(&[(-30, -70), (0, -100), (30, -70), (-30, -70)]),
        ]
    } else {
        vec![
            poly(&[(0, 0), (0, 70)]),
            poly(&[(-30, 50), (0, 100), (30, 50)]),
        ]
    };
    LibSymbol {
        lib_id: lib_id.into(),
        description: description.into(),
        keywords: "global power".into(),
        reference: "#PWR".into(),
        value: name.into(),
        footprint: String::new(),
        datasheet: String::new(),
        pins: vec![hidden(pin(
            "1",
            name,
            (0, 0),
            if ground { 90 } else { 270 },
            0,
            PinType::PowerIn,
        ))],
        graphics,
        unit_graphics: vec![],
        units: 1,
        power: true,
        in_bom: false,
        on_board: false,
        spice: Spice::Power,
        sim_params: String::new(),
        pin_names_hidden: true,
        pin_numbers_hidden: true,
        exclude_from_sim: false,
        ref_at: (0, if ground { -250 } else { -150 }),
        value_at: (0, if ground { -150 } else { 150 }),
        footprints: vec![],
        fields: vec![],
    }
}

fn source(
    lib_id: &'static str,
    description: &'static str,
    value: &'static str,
    current: bool,
    glyph: Vec<Graphic>,
) -> LibSymbol {
    let mut graphics = vec![circle((0, 0), 100, Fill::Background)];
    graphics.extend(glyph);
    let mut s = sym(
        lib_id,
        description,
        "simulation source",
        if current { "I" } else { "V" },
        value,
        &[],
        vec![
            pin("1", "+", (0, 200), 270, 100, P),
            pin("2", "-", (0, -200), 90, 100, P),
        ],
        graphics,
        if current {
            Spice::CurrentSource
        } else {
            Spice::VoltageSource
        },
    );
    s.in_bom = false;
    s.on_board = false;
    s.ref_at = (150, 50);
    s.value_at = (150, -50);
    s
}

fn gate(lib_id: &'static str, description: &'static str, kind: &str) -> LibSymbol {
    let inverted = matches!(kind, "nand" | "not");
    let out_x = if inverted { 200 } else { 150 };
    let mut graphics = match kind {
        "not" => vec![Graphic::Poly {
            pts: vec![(-100, 100), (150, 0), (-100, -100), (-100, 100)],
            fill: Fill::Background,
            width: 10,
        }],
        "or" => vec![
            arc((-150, 150), (-100, 0), (-150, -150)),
            arc((-150, 150), (70, 110), (150, 0)),
            arc((-150, -150), (70, -110), (150, 0)),
        ],
        "xor" => vec![
            arc((-190, 150), (-140, 0), (-190, -150)),
            arc((-150, 150), (-100, 0), (-150, -150)),
            arc((-150, 150), (70, 110), (150, 0)),
            arc((-150, -150), (70, -110), (150, 0)),
        ],
        _ => vec![
            poly(&[(0, 150), (-150, 150), (-150, -150), (0, -150)]),
            arc((0, 150), (150, 0), (0, -150)),
        ],
    };
    if inverted {
        graphics.push(circle((175, 0), 25, Fill::None));
    }
    let mut pins = if kind == "not" {
        vec![
            pin("2", "A", (-300, 0), 0, 200, PinType::Input),
            pin("4", "Y", (out_x + 150, 0), 180, 150, PinType::Output),
            hidden(pin("1", "NC", (-100, 200), 270, 0, PinType::NoConnect)),
        ]
    } else {
        vec![
            pin(
                "1",
                "A",
                (-300, 100),
                0,
                if kind == "or" || kind == "xor" {
                    190
                } else {
                    150
                },
                PinType::Input,
            ),
            pin(
                "2",
                "B",
                (-300, -100),
                0,
                if kind == "or" || kind == "xor" {
                    190
                } else {
                    150
                },
                PinType::Input,
            ),
            pin("4", "Y", (out_x + 150, 0), 180, 150, PinType::Output),
        ]
    };
    pins.push(pin(
        "5",
        "VCC",
        (0, 300),
        270,
        if kind == "not" { 250 } else { 150 },
        PinType::PowerIn,
    ));
    pins.push(pin(
        "3",
        "GND",
        (0, -300),
        90,
        if kind == "not" { 250 } else { 150 },
        PinType::PowerIn,
    ));
    let mut s = sym(
        lib_id,
        description,
        "logic gate single",
        "U",
        lib_id.split(':').nth(1).unwrap_or(""),
        &["Package_TO_SOT_SMD:SOT-23-5"],
        pins,
        graphics,
        Spice::Gate(match kind {
            "nand" => LogicGate::Nand,
            "and" => LogicGate::And,
            "or" => LogicGate::Or,
            "xor" => LogicGate::Xor,
            _ => LogicGate::Not,
        }),
    );
    s.pin_names_hidden = true;
    s.pin_numbers_hidden = false;
    s.ref_at = (200, 250);
    s.value_at = (200, -250);
    s
}

/// A single positive-edge D flip-flop, 74LVC1G79 (SOT-23-5): D, CLK, GND, Q, VCC.
fn dff(lib_id: &'static str, description: &'static str) -> LibSymbol {
    let mut s = sym(
        lib_id,
        description,
        "Single D Flip-Flop positive edge trigger",
        "U",
        lib_id.split(':').nth(1).unwrap_or(""),
        &["Package_TO_SOT_SMD:SOT-23-5"],
        vec![
            pin("1", "D", (-400, 100), 0, 150, PinType::Input),
            pin("2", "CLK", (-400, -100), 0, 150, PinType::Input),
            pin("3", "GND", (0, -400), 90, 150, PinType::PowerIn),
            pin("4", "Q", (400, 100), 180, 150, PinType::Output),
            pin("5", "VCC", (0, 400), 270, 150, PinType::PowerIn),
        ],
        vec![
            rect((-250, 250), (250, -250), Fill::Background),
            // The clock input's edge marker.
            poly(&[(-250, -60), (-200, -100), (-250, -140)]),
        ],
        Spice::DFlipFlop,
    );
    s.pin_names_hidden = false;
    s.pin_numbers_hidden = false;
    s.ref_at = (-250, 300);
    s.value_at = (100, 300);
    s
}

#[allow(clippy::vec_init_then_push)]
fn build() -> Vec<LibSymbol> {
    let mut lib = Vec::new();
    // ---- Device --------------------------------------------------------------
    lib.push(sym(
        "Device:R",
        "Resistor",
        "R res resistor",
        "R",
        "10k",
        &[
            "Resistor_THT:R_Axial_DIN0207_L6.3mm_D2.5mm_P10.16mm_Horizontal",
            "Resistor_SMD:R_0805_2012Metric",
        ],
        TWO_PIN_V(50),
        vec![rect((-40, -100), (40, 100), Fill::None)],
        Spice::Resistor,
    ));
    lib.push(sym(
        "Device:C",
        "Unpolarized capacitor",
        "cap capacitor",
        "C",
        "100n",
        &[
            "Capacitor_THT:C_Disc_D5.0mm_W2.5mm_P5.00mm",
            "Capacitor_SMD:C_0805_2012Metric",
        ],
        TWO_PIN_V(120),
        vec![
            thick(&[(-80, 30), (80, 30)], 20),
            thick(&[(-80, -30), (80, -30)], 20),
        ],
        Spice::Capacitor,
    ));
    lib.push(sym(
        "Device:C_Polarized",
        "Polarized capacitor",
        "cap capacitor electrolytic",
        "C",
        "10u",
        &["Capacitor_THT:CP_Radial_D5.0mm_P2.00mm"],
        TWO_PIN_V(110),
        vec![
            rect((-80, 40), (80, 20), Fill::None),
            rect((-80, -20), (80, -40), Fill::Outline),
            poly(&[(-70, 90), (-30, 90)]),
            poly(&[(-50, 110), (-50, 70)]),
        ],
        Spice::Capacitor,
    ));
    lib.push(sym(
        "Device:L",
        "Inductor",
        "inductor choke coil",
        "L",
        "10m",
        &["Inductor_THT:L_Axial_L5.3mm_D2.2mm_P10.16mm_Horizontal_Vishay_IM-1"],
        TWO_PIN_V(50),
        vec![
            arc((0, 100), (25, 75), (0, 50)),
            arc((0, 50), (25, 25), (0, 0)),
            arc((0, 0), (25, -25), (0, -50)),
            arc((0, -50), (25, -75), (0, -100)),
        ],
        Spice::Inductor,
    ));
    let diode_body = || {
        vec![
            poly(&[(-50, 50), (-50, -50)]),
            poly(&[(50, 0), (-50, 0)]),
            poly(&[(50, 50), (50, -50), (-50, 0), (50, 50)]),
        ]
    };
    let diode_pins = || {
        vec![
            pin("1", "K", (-150, 0), 0, 100, P),
            pin("2", "A", (150, 0), 180, 100, P),
        ]
    };
    let mut d = sym(
        "Device:D",
        "Diode",
        "diode rectifier",
        "D",
        "1N4148",
        &["Diode_THT:D_DO-35_SOD27_P7.62mm_Horizontal"],
        diode_pins(),
        diode_body(),
        Spice::Diode,
    );
    d.sim_params = "is=4.352n n=1.906 rs=0.6458 bv=110 ibv=0.0001 cjo=0.7p tt=3.48n".into();
    d.ref_at = (0, 150);
    d.value_at = (0, -150);
    lib.push(d);
    let mut led_graphics = diode_body();
    led_graphics.push(poly(&[(-20, -70), (-70, -120)]));
    led_graphics.push(filled(&[(-70, -120), (-60, -85), (-35, -110), (-70, -120)]));
    led_graphics.push(poly(&[(30, -70), (-20, -120)]));
    led_graphics.push(filled(&[(-20, -120), (-10, -85), (15, -110), (-20, -120)]));
    let mut led = sym(
        "Device:LED",
        "Light emitting diode",
        "LED diode",
        "D",
        "LED",
        &["LED_THT:LED_D5.0mm"],
        diode_pins(),
        led_graphics,
        Spice::Diode,
    );
    led.sim_params = "is=1e-20 n=1.5 rs=2".into();
    led.ref_at = (0, 150);
    led.value_at = (0, -200);
    lib.push(led);
    let mut zener = sym(
        "Device:D_Zener",
        "Zener diode",
        "diode zener",
        "D",
        "5V1",
        &["Diode_THT:D_DO-35_SOD27_P7.62mm_Horizontal"],
        diode_pins(),
        vec![
            poly(&[(-30, 50), (-50, 50), (-50, -50), (-70, -50)]),
            poly(&[(50, 0), (-50, 0)]),
            poly(&[(50, 50), (50, -50), (-50, 0), (50, 50)]),
        ],
        Spice::Diode,
    );
    zener.sim_params = "is=1e-14 n=1 bv=5.1 ibv=5m".into();
    zener.ref_at = (0, 150);
    zener.value_at = (0, -150);
    lib.push(zener);
    let bjt = |npn: bool| {
        let mut g = vec![
            circle((50, 0), 110, Fill::None),
            thick(&[(-10, 70), (-10, -70)], 20),
            poly(&[(-50, 0), (-10, 0)]),
            poly(&[(-10, 30), (100, 100)]),
            poly(&[(-10, -30), (100, -100)]),
        ];
        if npn {
            g.push(filled(&[(80, -90), (55, -55), (35, -85), (80, -90)]));
        } else {
            g.push(filled(&[(10, -40), (30, -75), (50, -45), (10, -40)]));
        }
        g
    };
    let bjt_pins = || {
        vec![
            pin("1", "B", (-200, 0), 0, 150, P),
            pin("2", "C", (100, 200), 270, 100, P),
            pin("3", "E", (100, -200), 90, 100, P),
        ]
    };
    let mut q = sym(
        "Device:Q_NPN_BCE",
        "NPN transistor, base/collector/emitter",
        "transistor NPN",
        "Q",
        "Q_NPN_BCE",
        &["Package_TO_SOT_THT:TO-92_Inline"],
        bjt_pins(),
        bjt(true),
        Spice::Npn,
    );
    q.sim_params = "is=1e-14 bf=200 br=2 vaf=100".into();
    q.ref_at = (250, 50);
    q.value_at = (250, -50);
    lib.push(q.clone());
    let mut qp = sym(
        "Device:Q_PNP_BCE",
        "PNP transistor, base/collector/emitter",
        "transistor PNP",
        "Q",
        "Q_PNP_BCE",
        &["Package_TO_SOT_THT:TO-92_Inline"],
        bjt_pins(),
        bjt(false),
        Spice::Pnp,
    );
    qp.sim_params = "is=1e-14 bf=150 br=2 vaf=80".into();
    qp.ref_at = q.ref_at;
    qp.value_at = q.value_at;
    lib.push(qp);
    let mos = |n: bool| {
        let mut g = vec![
            circle((50, 0), 110, Fill::None),
            poly(&[(-40, 70), (-40, -70)]),
            poly(&[(-50, 0), (-40, 0)]),
            thick(&[(0, 90), (0, 50)], 15),
            thick(&[(0, 20), (0, -20)], 15),
            thick(&[(0, -50), (0, -90)], 15),
            poly(&[(0, 70), (100, 70), (100, 100)]),
            poly(&[(0, -70), (100, -70), (100, -100)]),
            poly(&[(0, 0), (100, 0), (100, -70)]),
        ];
        if n {
            g.push(filled(&[(10, 0), (50, 20), (50, -20), (10, 0)]));
        } else {
            g.push(filled(&[(90, 0), (50, 20), (50, -20), (90, 0)]));
        }
        g
    };
    let mos_pins = || {
        vec![
            pin("1", "G", (-200, 0), 0, 150, P),
            pin("2", "D", (100, 200), 270, 100, P),
            pin("3", "S", (100, -200), 90, 100, P),
        ]
    };
    let mut m = sym(
        "Device:Q_NMOS_GDS",
        "N-MOSFET transistor, gate/drain/source",
        "transistor NMOS N-MOS N-MOSFET",
        "Q",
        "Q_NMOS_GDS",
        &["Package_TO_SOT_SMD:SOT-23"],
        mos_pins(),
        mos(true),
        Spice::Nmos,
    );
    m.sim_params = "vto=2 kp=0.5 lambda=0.01".into();
    m.ref_at = (250, 50);
    m.value_at = (250, -50);
    lib.push(m.clone());
    let mut pm = sym(
        "Device:Q_PMOS_GDS",
        "P-MOSFET transistor, gate/drain/source",
        "transistor PMOS P-MOS P-MOSFET",
        "Q",
        "Q_PMOS_GDS",
        &["Package_TO_SOT_SMD:SOT-23"],
        mos_pins(),
        mos(false),
        Spice::Pmos,
    );
    pm.sim_params = "vto=-2 kp=0.5 lambda=0.01".into();
    pm.ref_at = m.ref_at;
    pm.value_at = m.value_at;
    lib.push(pm);
    // ---- Simulation sources and the ideal op-amp --------------------------------
    let mut opamp = sym(
        "Simulation_SPICE:OPAMP",
        "Operational amplifier, single, simulation model",
        "operational amplifier opamp",
        "U",
        "${SIM.PARAMS}",
        &["Package_DIP:DIP-8_W7.62mm"],
        vec![
            pin("1", "+", (-300, 100), 0, 100, PinType::Input),
            pin("2", "-", (-300, -100), 0, 100, PinType::Input),
            pin("3", "V+", (-100, 300), 270, 150, PinType::PowerIn),
            pin("4", "V-", (-100, -300), 90, 150, PinType::PowerIn),
            pin("5", "~", (300, 0), 180, 100, PinType::Output),
        ],
        vec![
            Graphic::Poly {
                pts: vec![(-200, 200), (200, 0), (-200, -200), (-200, 200)],
                fill: Fill::Background,
                width: 10,
            },
            Graphic::Text {
                at: (-160, 100),
                text: "+".into(),
            },
            Graphic::Text {
                at: (-160, -100),
                text: "-".into(),
            },
        ],
        Spice::OpAmp,
    );
    opamp.value = "OPAMP".into();
    opamp.sim_params = "gain=100k".into();
    opamp.ref_at = (100, 250);
    opamp.value_at = (100, -250);
    lib.push(opamp);
    lib.push(source(
        "Simulation_SPICE:VDC",
        "Voltage source, DC",
        "5",
        false,
        vec![
            poly(&[(0, 70), (0, 30)]),
            poly(&[(-20, 50), (20, 50)]),
            poly(&[(-20, -50), (20, -50)]),
        ],
    ));
    lib.push(source(
        "Simulation_SPICE:VSIN",
        "Voltage source, sinusoidal",
        "dc 0 ac 1 sin(0 1 1k)",
        false,
        vec![
            arc((-60, 0), (-30, 40), (0, 0)),
            arc((0, 0), (30, -40), (60, 0)),
        ],
    ));
    lib.push(source(
        "Simulation_SPICE:VPULSE",
        "Voltage source, pulse",
        "pulse(0 5 1u 1u 1u 1m 2m)",
        false,
        vec![poly(&[
            (-60, -30),
            (-30, -30),
            (-30, 30),
            (30, 30),
            (30, -30),
            (60, -30),
        ])],
    ));
    lib.push(source(
        "Simulation_SPICE:IDC",
        "Current source, DC",
        "1m",
        true,
        vec![
            poly(&[(0, -60), (0, 60)]),
            filled(&[(0, 60), (-20, 20), (20, 20), (0, 60)]),
        ],
    ));
    // ---- Power --------------------------------------------------------------------
    lib.push(power_symbol(
        "GND",
        "Power symbol creates a global label with name \"GND\" , ground",
        true,
    ));
    lib.push(power_symbol(
        "+5V",
        "Power symbol creates a global label with name \"+5V\"",
        false,
    ));
    lib.push(power_symbol(
        "+3V3",
        "Power symbol creates a global label with name \"+3V3\"",
        false,
    ));
    lib.push(power_symbol(
        "+12V",
        "Power symbol creates a global label with name \"+12V\"",
        false,
    ));
    lib.push(power_symbol(
        "VCC",
        "Power symbol creates a global label with name \"VCC\"",
        false,
    ));
    lib.push(power_symbol(
        "-5V",
        "Power symbol creates a global label with name \"-5V\"",
        false,
    ));
    let mut flag = LibSymbol {
        lib_id: "power:PWR_FLAG".into(),
        description: "Special symbol for telling ERC where power comes from".into(),
        keywords: "flag power".into(),
        reference: "#FLG".into(),
        value: "PWR_FLAG".into(),
        footprint: String::new(),
        datasheet: "~".into(),
        pins: vec![hidden(pin("1", "~", (0, 0), 90, 0, PinType::PowerOut))],
        graphics: vec![poly(&[
            (0, 0),
            (0, 50),
            (-40, 75),
            (0, 100),
            (40, 75),
            (0, 50),
        ])],
        unit_graphics: vec![],
        units: 1,
        power: true,
        in_bom: false,
        on_board: false,
        spice: Spice::None,
        sim_params: String::new(),
        pin_names_hidden: true,
        pin_numbers_hidden: true,
        exclude_from_sim: false,
        ref_at: (0, 190),
        value_at: (0, 150),
        footprints: vec![],
        fields: vec![],
    };
    flag.value_at = (0, 170);
    lib.push(flag);
    // ---- Connectors and switches ---------------------------------------------------
    let conn = |n: usize| {
        let lib_id: &'static str = if n == 2 {
            "Connector:Conn_01x02"
        } else {
            "Connector:Conn_01x03"
        };
        let numbers = ["1", "2", "3"];
        let names = ["Pin_1", "Pin_2", "Pin_3"];
        let pins = (0..n)
            .map(|i| pin(numbers[i], names[i], (-200, -(i as i64) * 100), 0, 150, P))
            .collect();
        let bottom = -((n as i64 - 1) * 100) - 50;
        let mut g = vec![rect((-50, 50), (50, bottom), Fill::Background)];
        for i in 0..n as i64 {
            g.push(rect(
                (-50, -i * 100 + 10),
                (0, -i * 100 - 10),
                Fill::Outline,
            ));
        }
        let mut s = sym(
            lib_id,
            if n == 2 {
                "Generic connector, single row, 01x02"
            } else {
                "Generic connector, single row, 01x03"
            },
            "connector",
            "J",
            if n == 2 { "Conn_01x02" } else { "Conn_01x03" },
            if n == 2 {
                &["Connector_PinHeader_2.54mm:PinHeader_1x02_P2.54mm_Vertical"]
            } else {
                &["Connector_PinHeader_2.54mm:PinHeader_1x03_P2.54mm_Vertical"]
            },
            pins,
            g,
            Spice::None,
        );
        s.pin_names_hidden = true;
        s.pin_numbers_hidden = false;
        s.exclude_from_sim = true;
        s.ref_at = (0, 150);
        s.value_at = (0, bottom - 100);
        s
    };
    lib.push(conn(2));
    lib.push(conn(3));
    let mut sw = sym(
        "Switch:SW_Push",
        "Push button switch, generic, two pins",
        "switch normally-open pushbutton push-button",
        "SW",
        "SW_Push",
        &["Button_Switch_THT:SW_PUSH_6mm"],
        vec![
            pin("1", "1", (-200, 0), 0, 100, P),
            pin("2", "2", (200, 0), 180, 100, P),
        ],
        vec![
            circle((-80, 0), 20, Fill::None),
            circle((80, 0), 20, Fill::None),
            poly(&[(-100, 50), (100, 50)]),
            poly(&[(0, 50), (0, 120)]),
        ],
        Spice::None,
    );
    sw.exclude_from_sim = true;
    sw.ref_at = (0, 250);
    sw.value_at = (0, -100);
    lib.push(sw);
    // ---- Logic ----------------------------------------------------------------------
    lib.push(gate(
        "74xGxx:74LVC1G00",
        "Single NAND Gate, Low-Voltage CMOS",
        "nand",
    ));
    lib.push(gate(
        "74xGxx:74LVC1G08",
        "Single AND Gate, Low-Voltage CMOS",
        "and",
    ));
    lib.push(gate(
        "74xGxx:74LVC1G32",
        "Single OR Gate, Low-Voltage CMOS",
        "or",
    ));
    lib.push(gate(
        "74xGxx:74LVC1G04",
        "Single NOT Gate, Low-Voltage CMOS",
        "not",
    ));
    lib.push(gate(
        "74xGxx:74LVC1G86",
        "Single XOR Gate, Low-Voltage CMOS",
        "xor",
    ));
    lib.push(dff(
        "74xGxx:74LVC1G79",
        "Single D Flip-Flop, Positive Edge Trigger, Low-Voltage CMOS",
    ));
    // ---- ICs --------------------------------------------------------------------------
    let mut timer = sym(
        "Timer:NE555P",
        "Precision Timers, 555 compatible, PDIP-8",
        "single timer 555",
        "U",
        "NE555P",
        &["Package_DIP:DIP-8_W7.62mm"],
        vec![
            pin("1", "GND", (0, -400), 90, 100, PinType::PowerIn),
            pin("2", "TR", (-400, 200), 0, 100, PinType::Input),
            pin("3", "Q", (400, 200), 180, 100, PinType::Output),
            pin("4", "R", (-400, -200), 0, 100, PinType::Input),
            pin("5", "CV", (-400, 0), 0, 100, PinType::Input),
            pin("6", "THR", (400, -200), 180, 100, PinType::Input),
            pin("7", "DIS", (400, 0), 180, 100, PinType::OpenCollector),
            pin("8", "VCC", (0, 400), 270, 100, PinType::PowerIn),
        ],
        vec![rect((-300, 300), (300, -300), Fill::Background)],
        Spice::Timer555,
    );
    timer.pin_names_hidden = false;
    timer.pin_numbers_hidden = false;
    timer.ref_at = (-300, 350);
    timer.value_at = (100, 350);
    timer.datasheet = "http://www.ti.com/lit/ds/symlink/ne555.pdf".into();
    lib.push(timer);
    let mut mcu = sym(
        "MCU_Microchip_ATtiny:ATtiny85-20P",
        "20MHz, 8kB Flash, 512B SRAM, 512B EEPROM, debugWIRE, DIP-8",
        "AVR 8bit Microcontroller tinyAVR",
        "U",
        "ATtiny85-20P",
        &["Package_DIP:DIP-8_W7.62mm"],
        vec![
            pin("1", "PB5", (600, -200), 180, 100, PinType::Bidirectional),
            pin("2", "PB3", (600, 0), 180, 100, PinType::Bidirectional),
            pin("3", "PB4", (600, -100), 180, 100, PinType::Bidirectional),
            pin("4", "GND", (0, -600), 90, 100, PinType::PowerIn),
            pin("5", "PB0", (600, 300), 180, 100, PinType::Bidirectional),
            pin("6", "PB1", (600, 200), 180, 100, PinType::Bidirectional),
            pin("7", "PB2", (600, 100), 180, 100, PinType::Bidirectional),
            pin("8", "VCC", (0, 600), 270, 100, PinType::PowerIn),
        ],
        vec![rect((-500, 500), (500, -500), Fill::Background)],
        Spice::Mcu,
    );
    mcu.pin_names_hidden = false;
    mcu.pin_numbers_hidden = false;
    mcu.ref_at = (-500, 550);
    mcu.value_at = (100, 550);
    mcu.datasheet = "http://ww1.microchip.com/downloads/en/DeviceDoc/atmel-2586-avr-8-bit-microcontroller-attiny25-attiny45-attiny85_datasheet.pdf".into();
    // No firmware runs in simulation: the pins follow a script (see netlist::mcu_model).
    mcu.sim_params = "PB0=square(1k)".into();
    lib.push(mcu);
    lib
}

/// Every symbol this installation ships, in library order.
pub fn library() -> &'static [LibSymbol] {
    static LIB: OnceLock<Vec<LibSymbol>> = OnceLock::new();
    LIB.get_or_init(build)
}
pub fn find(lib_id: &str) -> Option<&'static LibSymbol> {
    library().iter().find(|s| s.lib_id == lib_id)
}
/// Library names in the order the chooser lists them.
pub fn libraries() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for s in library() {
        let lib = s.library();
        if !out.contains(&lib) {
            out.push(lib);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_symbol_is_well_formed() {
        let mut ids = std::collections::BTreeSet::new();
        for s in library() {
            assert!(ids.insert(&s.lib_id), "{} twice", s.lib_id);
            assert!(!s.pins.is_empty(), "{} has no pins", s.lib_id);
            let mut numbers = std::collections::BTreeSet::new();
            for p in &s.pins {
                assert!(
                    numbers.insert(&p.number),
                    "{} pin {} twice",
                    s.lib_id,
                    p.number
                );
                // Connection points sit on the 50 mil grid so wires can reach them.
                assert_eq!(p.at.0 % 50, 0, "{} pin {}", s.lib_id, p.number);
                assert_eq!(p.at.1 % 50, 0, "{} pin {}", s.lib_id, p.number);
            }
            if s.on_board {
                assert!(!s.footprint.is_empty(), "{} has no footprint", s.lib_id);
            }
        }
        assert!(libraries().contains(&"power"));
        assert!(find("Device:R").is_some());
    }
}
