//! KiCad 8 project files: `.kicad_pro` (JSON), `.kicad_sch` and `.kicad_pcb`
//! (S-expressions, file versions 20231120 and 20240108). What is written reads back as
//! the same design; KiCad-written files load as long as their symbols and footprints are
//! ones this installation's libraries have.
use crate::footprints::{PadKind, PadShape};
use crate::geom::{mm, parse_mm, Pt, Rect, Xf};
use crate::pcb::{
    Board, DrawShape, Drawing, Footprint, FpLine, Layer, Pad, Rules, Track, Via, Zone,
};
use crate::schematic::{
    item_id, item_uuid, uuid, Field, LabelKind, Marker, Schematic, SymbolInst, Text, TitleBlock,
    Wire,
};
use crate::sexpr::{self, Sexp};
use crate::symbols::{self, Fill, Graphic, LibSymbol};

pub const SCH_VERSION: &str = "20231120";
pub const PCB_VERSION: &str = "20240108";

fn a(s: &str) -> Sexp {
    Sexp::atom(s)
}
fn q(s: &str) -> Sexp {
    Sexp::string(s)
}
fn n(head: &str, args: Vec<Sexp>) -> Sexp {
    Sexp::node(head, args)
}
fn yes(b: bool) -> Sexp {
    a(if b { "yes" } else { "no" })
}
/// Schematic mils to millimetres.
fn sm(v: i64) -> Sexp {
    a(&mm(v * 254, 10_000))
}
/// Board nanometres to millimetres.
fn bm(v: i64) -> Sexp {
    a(&mm(v, 1_000_000))
}
fn parse_sm(s: &str) -> Result<i64, String> {
    let tenths = parse_mm(s, 10_000).ok_or_else(|| format!("bad coordinate {s}"))?;
    // Round to the nearest mil; KiCad schematics are on a 50 mil grid.
    Ok((tenths + if tenths >= 0 { 127 } else { -127 }) / 254)
}
fn parse_bm(s: &str) -> Result<i64, String> {
    parse_mm(s, 1_000_000).ok_or_else(|| format!("bad coordinate {s}"))
}
fn font(size: &str) -> Sexp {
    n(
        "effects",
        vec![n("font", vec![n("size", vec![a(size), a(size)])])],
    )
}
fn font_hidden(size: &str, hide: bool) -> Sexp {
    let mut items = vec![n("font", vec![n("size", vec![a(size), a(size)])])];
    if hide {
        items.push(n("hide", vec![a("yes")]));
    }
    n("effects", items)
}
fn stroke(width: &str) -> Sexp {
    n(
        "stroke",
        vec![n("width", vec![a(width)]), n("type", vec![a("default")])],
    )
}
fn fill(f: Fill) -> Sexp {
    n(
        "fill",
        vec![n(
            "type",
            vec![a(match f {
                Fill::None => "none",
                Fill::Outline => "outline",
                Fill::Background => "background",
            })],
        )],
    )
}
fn lib_xy(p: (i64, i64)) -> Vec<Sexp> {
    vec![sm(p.0), sm(p.1)]
}

fn lib_symbol(lib: &LibSymbol) -> Sexp {
    let name = lib.name();
    let mut items = vec![q(lib.lib_id)];
    if lib.power {
        items.push(n("power", vec![]));
    }
    if lib.pin_numbers_hidden {
        items.push(n("pin_numbers", vec![n("hide", vec![a("yes")])]));
    }
    let mut names = vec![n("offset", vec![a("0")])];
    if lib.pin_names_hidden {
        names.push(n("hide", vec![a("yes")]));
    }
    items.push(n("pin_names", names));
    items.push(n("exclude_from_sim", vec![yes(lib.exclude_from_sim)]));
    items.push(n("in_bom", vec![yes(lib.in_bom)]));
    items.push(n("on_board", vec![yes(lib.on_board)]));
    let prop = |key: &str, value: &str, at: (i64, i64), hide: bool| {
        n(
            "property",
            vec![
                q(key),
                q(value),
                n("at", vec![sm(at.0), sm(at.1), a("0")]),
                font_hidden("1.27", hide),
            ],
        )
    };
    items.push(prop("Reference", lib.reference, lib.ref_at, lib.power));
    items.push(prop("Value", lib.value, lib.value_at, false));
    items.push(prop("Footprint", lib.footprint, (0, 0), true));
    items.push(prop("Datasheet", lib.datasheet, (0, 0), true));
    items.push(prop("Description", lib.description, (0, 0), true));
    items.push(prop("ki_keywords", lib.keywords, (0, 0), true));
    let mut body = vec![q(&format!("{name}_0_1"))];
    for g in &lib.graphics {
        body.push(match g {
            Graphic::Rect { a: p, b, fill: f } => n(
                "rectangle",
                vec![
                    n("start", lib_xy(*p)),
                    n("end", lib_xy(*b)),
                    stroke("0.254"),
                    fill(*f),
                ],
            ),
            Graphic::Poly {
                pts,
                fill: f,
                width,
            } => n(
                "polyline",
                vec![
                    n("pts", pts.iter().map(|p| n("xy", lib_xy(*p))).collect()),
                    stroke(&mm(width * 254, 10_000)),
                    fill(*f),
                ],
            ),
            Graphic::Circle { c, r, fill: f } => n(
                "circle",
                vec![
                    n("center", lib_xy(*c)),
                    n("radius", vec![sm(*r)]),
                    stroke("0.254"),
                    fill(*f),
                ],
            ),
            Graphic::Arc { start, mid, end } => n(
                "arc",
                vec![
                    n("start", lib_xy(*start)),
                    n("mid", lib_xy(*mid)),
                    n("end", lib_xy(*end)),
                    stroke("0"),
                    fill(Fill::None),
                ],
            ),
            Graphic::Text { at, text } => n(
                "text",
                vec![
                    q(text),
                    n("at", vec![sm(at.0), sm(at.1), a("0")]),
                    font("1.27"),
                ],
            ),
        });
    }
    items.push(n("symbol", body));
    let mut pins = vec![q(&format!("{name}_1_1"))];
    for p in &lib.pins {
        let mut pin = vec![
            a(p.kind.keyword()),
            a("line"),
            n("at", vec![sm(p.at.0), sm(p.at.1), a(&p.angle.to_string())]),
            n("length", vec![sm(p.length)]),
        ];
        if p.hidden {
            pin.push(n("hide", vec![a("yes")]));
        }
        pin.push(n("name", vec![q(p.name), font("1.27")]));
        pin.push(n("number", vec![q(p.number), font("1.27")]));
        pins.push(n("pin", pin));
    }
    items.push(n("symbol", pins));
    n("symbol", items)
}

fn sch_xy(p: Pt) -> Vec<Sexp> {
    vec![sm(p.x), sm(p.y)]
}

pub fn write_schematic(sch: &Schematic, project: &str) -> String {
    let id = |i: u64| q(&item_uuid(&sch.uuid, i));
    let mut doc = vec![
        n("version", vec![a(SCH_VERSION)]),
        n("generator", vec![q("eeschema")]),
        n("generator_version", vec![q("8.0")]),
        n("uuid", vec![q(&sch.uuid)]),
        n("paper", vec![q("A4")]),
    ];
    let tb = &sch.title_block;
    doc.push(n(
        "title_block",
        vec![
            n("title", vec![q(&tb.title)]),
            n("date", vec![q(&tb.date)]),
            n("rev", vec![q(&tb.rev)]),
            n("company", vec![q(&tb.company)]),
        ],
    ));
    let mut used: Vec<&str> = sch.symbols.iter().map(|s| s.lib_id.as_str()).collect();
    used.sort();
    used.dedup();
    let libs: Vec<Sexp> = used
        .iter()
        .filter_map(|lib_id| symbols::find(lib_id).map(lib_symbol))
        .collect();
    doc.push(n("lib_symbols", libs));
    for j in &sch.junctions {
        doc.push(n(
            "junction",
            vec![
                n("at", sch_xy(j.pos)),
                n("diameter", vec![a("0")]),
                n("color", vec![a("0"), a("0"), a("0"), a("0")]),
                n("uuid", vec![id(j.id)]),
            ],
        ));
    }
    for nc in &sch.no_connects {
        doc.push(n(
            "no_connect",
            vec![n("at", sch_xy(nc.pos)), n("uuid", vec![id(nc.id)])],
        ));
    }
    for w in &sch.wires {
        doc.push(n(
            "wire",
            vec![
                n("pts", vec![n("xy", sch_xy(w.a)), n("xy", sch_xy(w.b))]),
                stroke("0"),
                n("uuid", vec![id(w.id)]),
            ],
        ));
    }
    for t in &sch.texts {
        doc.push(n(
            "text",
            vec![
                q(&t.text),
                n("exclude_from_sim", vec![yes(false)]),
                n("at", vec![sm(t.pos.x), sm(t.pos.y), a("0")]),
                n(
                    "effects",
                    vec![
                        n("font", vec![n("size", vec![a("1.27"), a("1.27")])]),
                        n("justify", vec![a("left"), a("bottom")]),
                    ],
                ),
                n("uuid", vec![id(t.id)]),
            ],
        ));
    }
    for l in &sch.labels {
        let angle = (l.orient as u16 % 4) * 90;
        let justify = if l.orient == 2 || l.orient == 3 {
            "right"
        } else {
            "left"
        };
        let mut items = vec![q(&l.text)];
        if l.kind == LabelKind::Global {
            items.push(n("shape", vec![a("input")]));
        }
        items.push(n(
            "at",
            vec![sm(l.pos.x), sm(l.pos.y), a(&angle.to_string())],
        ));
        items.push(n("fields_autoplaced", vec![yes(true)]));
        items.push(n(
            "effects",
            vec![
                n("font", vec![n("size", vec![a("1.27"), a("1.27")])]),
                n("justify", vec![a(justify), a("bottom")]),
            ],
        ));
        items.push(n("uuid", vec![id(l.id)]));
        doc.push(n(
            match l.kind {
                LabelKind::Local => "label",
                LabelKind::Global => "global_label",
            },
            items,
        ));
    }
    for s in &sch.symbols {
        let (angle, mirror) = s.xf.decompose();
        let mut items = vec![
            n("lib_id", vec![q(&s.lib_id)]),
            n("at", vec![sm(s.pos.x), sm(s.pos.y), a(&angle.to_string())]),
        ];
        if let Some(m) = mirror {
            items.push(n("mirror", vec![a(&m.to_string())]));
        }
        items.push(n("unit", vec![a("1")]));
        items.push(n("exclude_from_sim", vec![yes(s.exclude_from_sim)]));
        items.push(n("in_bom", vec![yes(s.in_bom)]));
        items.push(n("on_board", vec![yes(s.on_board)]));
        items.push(n("dnp", vec![yes(s.dnp)]));
        items.push(n("uuid", vec![id(s.id)]));
        for f in &s.fields {
            let at = s.pos.add(f.offset);
            items.push(n(
                "property",
                vec![
                    q(&f.name),
                    q(&f.value),
                    n("at", vec![sm(at.x), sm(at.y), a("0")]),
                    font_hidden("1.27", !f.visible),
                ],
            ));
        }
        if let Some(lib) = s.lib() {
            for p in &lib.pins {
                items.push(n(
                    "pin",
                    vec![
                        q(p.number),
                        n(
                            "uuid",
                            vec![q(&uuid(&format!("{}{}", sch.uuid, p.number), s.id))],
                        ),
                    ],
                ));
            }
        }
        items.push(n(
            "instances",
            vec![n(
                "project",
                vec![
                    q(project),
                    n(
                        "path",
                        vec![
                            q(&format!("/{}", sch.uuid)),
                            n("reference", vec![q(s.reference())]),
                            n("unit", vec![a("1")]),
                        ],
                    ),
                ],
            )],
        ));
        doc.push(n("symbol", items));
    }
    doc.push(n(
        "sheet_instances",
        vec![n("path", vec![q("/"), n("page", vec![q("1")])])],
    ));
    n("kicad_sch", doc).to_text()
}

fn at_of(node: &Sexp) -> Option<(&str, &str, &str)> {
    let at = node.child("at")?;
    Some((at.arg(0)?, at.arg(1)?, at.arg(2).unwrap_or("0")))
}
fn hidden(effects: Option<&Sexp>) -> bool {
    effects.is_some_and(|e| {
        e.child("hide").is_some_and(|h| h.arg(0) != Some("no"))
            || e.items().iter().any(|i| i.text() == Some("hide"))
    })
}

pub fn read_schematic(text: &str) -> Result<(Schematic, Vec<String>), String> {
    let doc = sexpr::parse(text)?;
    if doc.head() != Some("kicad_sch") {
        return Err("not a KiCad schematic (no (kicad_sch …) root)".into());
    }
    let mut warnings = vec![];
    let seed_uuid = doc.value_of("uuid").unwrap_or("").to_owned();
    let mut sch = Schematic::new("");
    sch.uuid = if seed_uuid.is_empty() {
        uuid("", 0)
    } else {
        seed_uuid
    };
    let seed = sch.uuid.clone();
    let mut max_id = 0u64;
    // Items this writer did not number get id 0 here and a fresh one afterwards.
    let mut take = |node: &Sexp| -> u64 {
        match node.value_of("uuid").and_then(|u| item_id(&seed, u)) {
            Some(i) => {
                max_id = max_id.max(i);
                i
            }
            None => 0,
        }
    };
    if let Some(tb) = doc.child("title_block") {
        sch.title_block = TitleBlock {
            title: tb.value_of("title").unwrap_or("").into(),
            date: tb.value_of("date").unwrap_or("").into(),
            rev: tb.value_of("rev").unwrap_or("").into(),
            company: tb.value_of("company").unwrap_or("").into(),
        };
    }
    for item in doc.items().iter().skip(1) {
        match item.head() {
            Some("junction") => {
                let (x, y, _) = at_of(item).ok_or("junction without position")?;
                let id = take(item);
                sch.junctions.push(Marker {
                    id,
                    pos: Pt::new(parse_sm(x)?, parse_sm(y)?),
                });
            }
            Some("no_connect") => {
                let (x, y, _) = at_of(item).ok_or("no_connect without position")?;
                let id = take(item);
                sch.no_connects.push(Marker {
                    id,
                    pos: Pt::new(parse_sm(x)?, parse_sm(y)?),
                });
            }
            Some("wire") => {
                let pts: Vec<&Sexp> = item
                    .child("pts")
                    .map(|p| p.children("xy").collect())
                    .unwrap_or_default();
                if pts.len() != 2 {
                    return Err("wire without two points".into());
                }
                let p = |s: &Sexp| -> Result<Pt, String> {
                    Ok(Pt::new(
                        parse_sm(s.arg(0).unwrap_or(""))?,
                        parse_sm(s.arg(1).unwrap_or(""))?,
                    ))
                };
                let id = take(item);
                sch.wires.push(Wire {
                    id,
                    a: p(pts[0])?,
                    b: p(pts[1])?,
                });
            }
            Some(kind @ ("label" | "global_label")) => {
                let (x, y, angle) = at_of(item).ok_or("label without position")?;
                let id = take(item);
                sch.labels.push(crate::schematic::Label {
                    id,
                    pos: Pt::new(parse_sm(x)?, parse_sm(y)?),
                    text: item.arg(0).unwrap_or("").into(),
                    kind: if kind == "label" {
                        LabelKind::Local
                    } else {
                        LabelKind::Global
                    },
                    orient: ((angle
                        .split('.')
                        .next()
                        .unwrap_or("0")
                        .parse::<i64>()
                        .unwrap_or(0)
                        .rem_euclid(360))
                        / 90) as u8,
                });
            }
            Some("text") => {
                let (x, y, _) = at_of(item).ok_or("text without position")?;
                let id = take(item);
                sch.texts.push(Text {
                    id,
                    pos: Pt::new(parse_sm(x)?, parse_sm(y)?),
                    text: item.arg(0).unwrap_or("").into(),
                });
            }
            Some("symbol") => {
                let lib_id = item
                    .value_of("lib_id")
                    .ok_or("symbol without lib_id")?
                    .to_owned();
                if symbols::find(&lib_id).is_none() {
                    warnings.push(format!(
                        "Symbol {lib_id} is not in the installed libraries and was skipped"
                    ));
                    continue;
                }
                let (x, y, angle) = at_of(item).ok_or("symbol without position")?;
                let pos = Pt::new(parse_sm(x)?, parse_sm(y)?);
                let angle: i64 = angle.split('.').next().unwrap_or("0").parse().unwrap_or(0);
                let mirror = item.value_of("mirror").and_then(|m| m.chars().next());
                let flag = |name: &str, default: bool| {
                    item.value_of(name).map(|v| v == "yes").unwrap_or(default)
                };
                let id = take(item);
                let mut fields = vec![];
                for p in item.children("property") {
                    let name = p.arg(0).unwrap_or("").to_owned();
                    let value = p.arg(1).unwrap_or("").to_owned();
                    let at = match at_of(p) {
                        Some((fx, fy, _)) => Pt::new(parse_sm(fx)?, parse_sm(fy)?),
                        None => pos,
                    };
                    fields.push(Field {
                        name,
                        value,
                        offset: at.sub(pos),
                        visible: !hidden(p.child("effects")),
                    });
                }
                sch.symbols.push(SymbolInst {
                    id,
                    lib_id,
                    pos,
                    xf: Xf::compose(angle, mirror),
                    fields,
                    exclude_from_sim: flag("exclude_from_sim", false),
                    in_bom: flag("in_bom", true),
                    on_board: flag("on_board", true),
                    dnp: flag("dnp", false),
                });
            }
            _ => {}
        }
    }
    let mut next = max_id + 1;
    let mut renumber = |id: &mut u64| {
        if *id == 0 {
            *id = next;
            next += 1;
        }
    };
    sch.symbols.iter_mut().for_each(|x| renumber(&mut x.id));
    sch.wires.iter_mut().for_each(|x| renumber(&mut x.id));
    sch.junctions.iter_mut().for_each(|x| renumber(&mut x.id));
    sch.labels.iter_mut().for_each(|x| renumber(&mut x.id));
    sch.no_connects.iter_mut().for_each(|x| renumber(&mut x.id));
    sch.texts.iter_mut().for_each(|x| renumber(&mut x.id));
    sch.next_id = next;
    Ok((sch, warnings))
}

// ---------------------------------------------------------------------------
// Board
// ---------------------------------------------------------------------------

fn bxy(p: Pt) -> Vec<Sexp> {
    vec![bm(p.x), bm(p.y)]
}

pub fn write_board(board: &Board) -> String {
    let id = |i: u64| q(&item_uuid(&board.uuid, i));
    let mut doc = vec![
        n("version", vec![a(PCB_VERSION)]),
        n("generator", vec![q("pcbnew")]),
        n("generator_version", vec![q("8.0")]),
        n(
            "general",
            vec![
                n("thickness", vec![a("1.6")]),
                n("legacy_teardrops", vec![yes(false)]),
            ],
        ),
        n("paper", vec![q("A4")]),
    ];
    let mut layers = vec![];
    let mut ordered = Layer::ALL.to_vec();
    ordered.sort_by_key(|l| l.ordinal());
    for l in ordered {
        let mut entry = vec![
            a(&l.ordinal().to_string()),
            q(l.name()),
            a(if l.is_copper() { "signal" } else { "user" }),
        ];
        if l.user_name() != l.name() {
            entry.push(q(l.user_name()));
        }
        layers.push(Sexp::List(entry));
    }
    doc.push(n("layers", layers));
    doc.push(n(
        "setup",
        vec![
            n("pad_to_mask_clearance", vec![a("0")]),
            n("allow_soldermask_bridges_in_footprints", vec![yes(false)]),
            n(
                "pcbplotparams",
                vec![
                    n("layerselection", vec![a("0x00010fc_ffffffff")]),
                    n(
                        "plot_on_all_layers_selection",
                        vec![a("0x0000000_00000000")],
                    ),
                    n("outputformat", vec![a("1")]),
                    n("outputdirectory", vec![q("gerbers/")]),
                ],
            ),
        ],
    ));
    for (i, name) in board.nets.iter().enumerate() {
        doc.push(n("net", vec![a(&i.to_string()), q(name)]));
    }
    for f in &board.footprints {
        let side = if f.back { "B.Cu" } else { "F.Cu" };
        let angle = (f.rot as u16 % 4) * 90;
        let mut items = vec![
            q(&f.fp_id),
            n("layer", vec![q(side)]),
            n("uuid", vec![id(f.id)]),
            n("at", vec![bm(f.pos.x), bm(f.pos.y), a(&angle.to_string())]),
        ];
        if f.locked {
            items.push(n("locked", vec![yes(true)]));
        }
        let silk = f.layer(Layer::FSilkS).name();
        let fab = f.layer(Layer::FFab).name();
        items.push(n(
            "property",
            vec![
                q("Reference"),
                q(&f.reference),
                n("at", vec![a("0"), a("-2.5"), a(&angle.to_string())]),
                n("layer", vec![q(silk)]),
                n(
                    "effects",
                    vec![n(
                        "font",
                        vec![
                            n("size", vec![a("1"), a("1")]),
                            n("thickness", vec![a("0.15")]),
                        ],
                    )],
                ),
            ],
        ));
        items.push(n(
            "property",
            vec![
                q("Value"),
                q(&f.value),
                n("at", vec![a("0"), a("2.5"), a(&angle.to_string())]),
                n("layer", vec![q(fab)]),
                n(
                    "effects",
                    vec![n(
                        "font",
                        vec![
                            n("size", vec![a("1"), a("1")]),
                            n("thickness", vec![a("0.15")]),
                        ],
                    )],
                ),
            ],
        ));
        if let Some(s) = f.symbol {
            items.push(n("path", vec![q(&format!("/{}", uuid("symbol", s)))]));
        }
        items.push(n(
            "attr",
            vec![a(if f.pads.iter().any(|p| p.kind == PadKind::Smd) {
                "smd"
            } else {
                "through_hole"
            })],
        ));
        for l in &f.lines {
            items.push(n(
                "fp_line",
                vec![
                    n("start", bxy(l.a)),
                    n("end", bxy(l.b)),
                    n(
                        "stroke",
                        vec![n("width", vec![bm(l.width)]), n("type", vec![a("solid")])],
                    ),
                    n("layer", vec![q(f.layer(l.layer).name())]),
                ],
            ));
        }
        for p in &f.pads {
            let layers: Vec<Sexp> = match p.kind {
                PadKind::ThroughHole => vec![q("*.Cu"), q("*.Mask")],
                PadKind::Smd => {
                    let (c, pa, m) = if f.back {
                        ("B.Cu", "B.Paste", "B.Mask")
                    } else {
                        ("F.Cu", "F.Paste", "F.Mask")
                    };
                    vec![q(c), q(pa), q(m)]
                }
            };
            let mut pad = vec![
                q(&p.number),
                a(match p.kind {
                    PadKind::ThroughHole => "thru_hole",
                    PadKind::Smd => "smd",
                }),
                a(p.shape.keyword()),
                n("at", vec![bm(p.at.x), bm(p.at.y), a(&angle.to_string())]),
                n("size", vec![bm(p.size.0), bm(p.size.1)]),
            ];
            if p.kind == PadKind::ThroughHole {
                pad.push(n("drill", vec![bm(p.drill)]));
            }
            pad.push(n("layers", layers));
            if p.shape == PadShape::RoundRect {
                pad.push(n("roundrect_rratio", vec![a("0.25")]));
            }
            if p.net != 0 {
                pad.push(n(
                    "net",
                    vec![a(&p.net.to_string()), q(board.net_name(p.net))],
                ));
            }
            pad.push(n("pintype", vec![q("passive")]));
            items.push(n("pad", pad));
        }
        doc.push(n("footprint", items));
    }
    for d in &board.drawings {
        let (head, s, e) = match d.shape {
            DrawShape::Line { a: p, b } => ("gr_line", p, b),
            DrawShape::Rect { a: p, b } => ("gr_rect", p, b),
        };
        let mut items = vec![n("start", bxy(s)), n("end", bxy(e))];
        items.push(n(
            "stroke",
            vec![n("width", vec![bm(d.width)]), n("type", vec![a("default")])],
        ));
        if head == "gr_rect" {
            items.push(n("fill", vec![a("none")]));
        }
        items.push(n("layer", vec![q(d.layer.name())]));
        items.push(n("uuid", vec![id(d.id)]));
        doc.push(n(head, items));
    }
    for t in &board.tracks {
        doc.push(n(
            "segment",
            vec![
                n("start", bxy(t.a)),
                n("end", bxy(t.b)),
                n("width", vec![bm(t.width)]),
                n("layer", vec![q(t.layer.name())]),
                n("net", vec![a(&t.net.to_string())]),
                n("uuid", vec![id(t.id)]),
            ],
        ));
    }
    for v in &board.vias {
        doc.push(n(
            "via",
            vec![
                n("at", bxy(v.pos)),
                n("size", vec![bm(v.diameter)]),
                n("drill", vec![bm(v.drill)]),
                n("layers", vec![q("F.Cu"), q("B.Cu")]),
                n("net", vec![a(&v.net.to_string())]),
                n("uuid", vec![id(v.id)]),
            ],
        ));
    }
    for z in &board.zones {
        let mut items = vec![
            n("net", vec![a(&z.net.to_string())]),
            n("net_name", vec![q(board.net_name(z.net))]),
            n("layer", vec![q(z.layer.name())]),
            n("uuid", vec![id(z.id)]),
            n("hatch", vec![a("edge"), a("0.5")]),
            n("connect_pads", vec![n("clearance", vec![bm(z.clearance)])]),
            n("min_thickness", vec![bm(z.min_width)]),
            n("filled_areas_thickness", vec![yes(false)]),
            n(
                "fill",
                vec![
                    yes(z.filled),
                    n("thermal_gap", vec![bm(z.thermal_gap)]),
                    n("thermal_bridge_width", vec![bm(z.spoke_width)]),
                ],
            ),
            n(
                "polygon",
                vec![n(
                    "pts",
                    z.outline.iter().map(|p| n("xy", bxy(*p))).collect(),
                )],
            ),
        ];
        for r in &z.fill {
            items.push(n(
                "filled_polygon",
                vec![
                    n("layer", vec![q(z.layer.name())]),
                    n(
                        "pts",
                        [
                            r.min,
                            Pt::new(r.max.x, r.min.y),
                            r.max,
                            Pt::new(r.min.x, r.max.y),
                        ]
                        .iter()
                        .map(|p| n("xy", bxy(*p)))
                        .collect(),
                    ),
                ],
            ));
        }
        doc.push(n("zone", items));
    }
    n("kicad_pcb", doc).to_text()
}

fn pt2(node: Option<&Sexp>) -> Result<Pt, String> {
    let node = node.ok_or("missing coordinate")?;
    Ok(Pt::new(
        parse_bm(node.arg(0).unwrap_or(""))?,
        parse_bm(node.arg(1).unwrap_or(""))?,
    ))
}

pub fn read_board(text: &str) -> Result<(Board, Vec<String>), String> {
    let doc = sexpr::parse(text)?;
    if doc.head() != Some("kicad_pcb") {
        return Err("not a KiCad board (no (kicad_pcb …) root)".into());
    }
    let mut warnings = vec![];
    let mut board = Board::new("");
    board.nets.clear();
    let mut max_id = 0u64;
    // Board files have no uuid of their own; ours number every item under one prefix,
    // so the first item's uuid identifies the board.
    if let Some(first) = doc
        .items()
        .iter()
        .find_map(|i| i.value_of("uuid"))
        .filter(|u| u.len() == 36)
    {
        // `Board::new` keeps all-ones in the item group of its own uuid.
        board.uuid = format!("{}ffffffffffff", &first[..24]);
    }
    let seed = board.uuid.clone();
    let mut take = |node: &Sexp| -> u64 {
        match node.value_of("uuid").and_then(|u| item_id(&seed, u)) {
            Some(i) => {
                max_id = max_id.max(i);
                i
            }
            None => 0,
        }
    };
    let mut nets: Vec<(usize, String)> = doc
        .children("net")
        .map(|n| {
            (
                n.arg(0).and_then(|v| v.parse().ok()).unwrap_or(0),
                n.arg(1).unwrap_or("").to_owned(),
            )
        })
        .collect();
    nets.sort();
    for (i, name) in nets {
        while board.nets.len() < i {
            board.nets.push(format!("unnamed-{}", board.nets.len()));
        }
        if board.nets.len() == i {
            board.nets.push(name);
        }
    }
    if board.nets.is_empty() {
        board.nets.push(String::new());
    }
    let net = |node: &Sexp| -> usize {
        node.child("net")
            .and_then(|n| n.arg(0))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    };
    for item in doc.items().iter().skip(1) {
        match item.head() {
            Some("footprint") => {
                let fp_id = item.arg(0).unwrap_or("").to_owned();
                let (x, y, angle) = at_of(item).ok_or("footprint without position")?;
                let pos = Pt::new(parse_bm(x)?, parse_bm(y)?);
                let angle: i64 = angle.split('.').next().unwrap_or("0").parse().unwrap_or(0);
                let back = item.value_of("layer") == Some("B.Cu");
                let id = take(item);
                let mut reference = String::new();
                let mut value = String::new();
                for p in item.children("property") {
                    match p.arg(0) {
                        Some("Reference") => reference = p.arg(1).unwrap_or("").into(),
                        Some("Value") => value = p.arg(1).unwrap_or("").into(),
                        _ => {}
                    }
                }
                // KiCad 7 and older kept these as fp_text.
                for t in item.children("fp_text") {
                    match t.arg(0) {
                        Some("reference") => reference = t.arg(1).unwrap_or("").into(),
                        Some("value") => value = t.arg(1).unwrap_or("").into(),
                        _ => {}
                    }
                }
                let mut lines = vec![];
                for l in item.children("fp_line") {
                    let layer = l.value_of("layer").and_then(Layer::parse);
                    let Some(layer) = layer else { continue };
                    let width = l
                        .child("stroke")
                        .and_then(|s| s.value_of("width"))
                        .or_else(|| l.value_of("width"))
                        .map(parse_bm)
                        .transpose()?
                        .unwrap_or(120_000);
                    lines.push(FpLine {
                        layer: if back { layer.flipped() } else { layer },
                        a: pt2(l.child("start"))?,
                        b: pt2(l.child("end"))?,
                        width,
                    });
                }
                let mut pads = vec![];
                for p in item.children("pad") {
                    let kind = match p.arg(1) {
                        Some("thru_hole") => PadKind::ThroughHole,
                        Some("smd") => PadKind::Smd,
                        other => {
                            warnings.push(format!(
                                "{reference}: pad type {other:?} is not supported; skipped"
                            ));
                            continue;
                        }
                    };
                    let shape = PadShape::parse(p.arg(2).unwrap_or("")).unwrap_or(PadShape::Rect);
                    let size = p.child("size").ok_or("pad without size")?;
                    pads.push(Pad {
                        number: p.arg(0).unwrap_or("").into(),
                        kind,
                        shape,
                        at: pt2(p.child("at"))?,
                        size: (
                            parse_bm(size.arg(0).unwrap_or(""))?,
                            parse_bm(size.arg(1).unwrap_or(""))?,
                        ),
                        drill: p.value_of("drill").map(parse_bm).transpose()?.unwrap_or(0),
                        net: net(p),
                    });
                }
                let symbol = item
                    .value_of("path")
                    .and_then(|p| p.rsplit('/').next())
                    .filter(|u| u.len() == 36)
                    .and_then(|u| u64::from_str_radix(&u[24..], 16).ok());
                board.footprints.push(Footprint {
                    id,
                    fp_id,
                    reference,
                    value,
                    pos,
                    rot: ((angle.rem_euclid(360)) / 90) as u8,
                    back,
                    pads,
                    lines,
                    symbol,
                    locked: item.child("locked").is_some()
                        || item.items().iter().any(|i| i.text() == Some("locked")),
                });
            }
            Some(head @ ("gr_line" | "gr_rect")) => {
                let Some(layer) = item.value_of("layer").and_then(Layer::parse) else {
                    continue;
                };
                let s = pt2(item.child("start"))?;
                let e = pt2(item.child("end"))?;
                let width = item
                    .child("stroke")
                    .and_then(|s| s.value_of("width"))
                    .or_else(|| item.value_of("width"))
                    .map(parse_bm)
                    .transpose()?
                    .unwrap_or(100_000);
                let id = take(item);
                board.drawings.push(Drawing {
                    id,
                    layer,
                    shape: if head == "gr_line" {
                        DrawShape::Line { a: s, b: e }
                    } else {
                        DrawShape::Rect { a: s, b: e }
                    },
                    width,
                });
            }
            Some("segment") => {
                let id = take(item);
                board.tracks.push(Track {
                    id,
                    a: pt2(item.child("start"))?,
                    b: pt2(item.child("end"))?,
                    width: parse_bm(item.value_of("width").unwrap_or("0.25"))?,
                    layer: item
                        .value_of("layer")
                        .and_then(Layer::parse)
                        .unwrap_or(Layer::FCu),
                    net: net(item),
                });
            }
            Some("via") => {
                let id = take(item);
                board.vias.push(Via {
                    id,
                    pos: pt2(item.child("at"))?,
                    diameter: parse_bm(item.value_of("size").unwrap_or("0.6"))?,
                    drill: parse_bm(item.value_of("drill").unwrap_or("0.3"))?,
                    net: net(item),
                });
            }
            Some("zone") => {
                let id = take(item);
                let outline = item
                    .child("polygon")
                    .and_then(|p| p.child("pts"))
                    .map(|pts| {
                        pts.children("xy")
                            .map(|x| pt2(Some(x)))
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                let mut fill = vec![];
                for fp in item.children("filled_polygon") {
                    let pts: Vec<Pt> = fp
                        .child("pts")
                        .map(|pts| {
                            pts.children("xy")
                                .map(|x| pt2(Some(x)))
                                .collect::<Result<Vec<_>, _>>()
                        })
                        .transpose()?
                        .unwrap_or_default();
                    if pts.len() == 4 && pts[0].y == pts[1].y && pts[1].x == pts[2].x {
                        fill.push(Rect::new(pts[0], pts[2]));
                    } else if !pts.is_empty() {
                        warnings.push(
                            "a zone fill that is not rectangular was dropped; refill zones".into(),
                        );
                    }
                }
                let fill_node = item.child("fill");
                board.zones.push(Zone {
                    id,
                    net: net(item),
                    layer: item
                        .value_of("layer")
                        .and_then(Layer::parse)
                        .unwrap_or(Layer::FCu),
                    outline,
                    clearance: item
                        .child("connect_pads")
                        .and_then(|c| c.value_of("clearance"))
                        .map(parse_bm)
                        .transpose()?
                        .unwrap_or(500_000),
                    min_width: item
                        .value_of("min_thickness")
                        .map(parse_bm)
                        .transpose()?
                        .unwrap_or(250_000),
                    thermal_gap: fill_node
                        .and_then(|f| f.value_of("thermal_gap"))
                        .map(parse_bm)
                        .transpose()?
                        .unwrap_or(500_000),
                    spoke_width: fill_node
                        .and_then(|f| f.value_of("thermal_bridge_width"))
                        .map(parse_bm)
                        .transpose()?
                        .unwrap_or(500_000),
                    filled: fill_node.and_then(|f| f.arg(0)) == Some("yes") && !fill.is_empty(),
                    fill,
                });
            }
            _ => {}
        }
    }
    let mut next = max_id + 1;
    let mut renumber = |id: &mut u64| {
        if *id == 0 {
            *id = next;
            next += 1;
        }
    };
    board
        .footprints
        .iter_mut()
        .for_each(|x| renumber(&mut x.id));
    board.drawings.iter_mut().for_each(|x| renumber(&mut x.id));
    board.tracks.iter_mut().for_each(|x| renumber(&mut x.id));
    board.vias.iter_mut().for_each(|x| renumber(&mut x.id));
    board.zones.iter_mut().for_each(|x| renumber(&mut x.id));
    board.next_id = next;
    Ok((board, warnings))
}

// ---------------------------------------------------------------------------
// Project
// ---------------------------------------------------------------------------

fn json_mm(v: i64) -> serde_json::Value {
    serde_json::from_str(&mm(v, 1_000_000)).unwrap_or(serde_json::Value::Null)
}
fn from_json_mm(v: Option<&serde_json::Value>, default: i64) -> i64 {
    v.and_then(|v| parse_mm(&v.to_string(), 1_000_000))
        .unwrap_or(default)
}

/// `.kicad_pro`: KiCad keeps project settings, including the board's design rules and
/// net classes, as JSON.
pub fn write_project(name: &str, rules: &Rules) -> String {
    let v = serde_json::json!({
        "board": {
            "design_settings": {
                "defaults": {"board_outline_line_width": 0.05, "copper_line_width": 0.2},
                "rules": {
                    "min_clearance": json_mm(rules.clearance),
                    "min_copper_edge_clearance": json_mm(rules.copper_edge_clearance),
                    "min_hole_to_hole": json_mm(rules.hole_to_hole),
                    "min_through_hole_diameter": json_mm(rules.min_through_hole),
                    "min_track_width": json_mm(rules.min_track_width),
                    "min_via_annular_width": json_mm(rules.min_annular_ring),
                    "min_via_diameter": json_mm(rules.min_via_diameter)
                },
                "track_widths": [],
                "via_dimensions": []
            }
        },
        "boards": [],
        "libraries": {"pinned_footprint_libs": [], "pinned_symbol_libs": []},
        "meta": {"filename": format!("{name}.kicad_pro"), "version": 1},
        "net_settings": {
            "classes": [{
                "bus_width": 12,
                "clearance": json_mm(rules.clearance),
                "diff_pair_gap": 0.25,
                "diff_pair_width": 0.2,
                "line_style": 0,
                "microvia_diameter": 0.3,
                "microvia_drill": 0.1,
                "name": "Default",
                "pcb_color": "rgba(0, 0, 0, 0.000)",
                "schematic_color": "rgba(0, 0, 0, 0.000)",
                "track_width": json_mm(rules.track_width),
                "via_diameter": json_mm(rules.via_diameter),
                "via_drill": json_mm(rules.via_drill),
                "wire_width": 6
            }],
            "meta": {"version": 3}
        },
        "pcbnew": {"last_paths": {"gencad": "", "idf": "", "netlist": "", "plot": "gerbers/", "specctra_dsn": "", "step": "", "svg": "", "vrml": ""}},
        "schematic": {"legacy_lib_dir": "", "legacy_lib_list": []},
        "sheets": [["", "Root"]],
        "text_variables": {}
    });
    serde_json::to_string_pretty(&v).unwrap_or_default() + "\n"
}
pub fn read_project(text: &str) -> Result<Rules, String> {
    let v: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("project file is not JSON: {e}"))?;
    if v.get("meta").is_none() {
        return Err("not a KiCad project file (no meta section)".into());
    }
    let d = Rules::default();
    let rules = v.pointer("/board/design_settings/rules");
    let class = v.pointer("/net_settings/classes/0");
    let r = |k: &str, def: i64| from_json_mm(rules.and_then(|r| r.get(k)), def);
    let c = |k: &str, def: i64| from_json_mm(class.and_then(|r| r.get(k)), def);
    Ok(Rules {
        clearance: c("clearance", d.clearance),
        track_width: c("track_width", d.track_width),
        via_diameter: c("via_diameter", d.via_diameter),
        via_drill: c("via_drill", d.via_drill),
        min_track_width: r("min_track_width", d.min_track_width),
        min_annular_ring: r("min_via_annular_width", d.min_annular_ring),
        min_via_diameter: r("min_via_diameter", d.min_via_diameter),
        min_through_hole: r("min_through_hole_diameter", d.min_through_hole),
        hole_to_hole: r("min_hole_to_hole", d.hole_to_hole),
        copper_edge_clearance: r("min_copper_edge_clearance", d.copper_edge_clearance),
    })
}
