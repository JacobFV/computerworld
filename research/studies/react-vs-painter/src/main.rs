//! Renders three agent-written apps two ways and scores their layout faults with the
//! same checks: the React/TSX original (tests/framework-parity/app-*, run on the
//! engine's realm and painted by cw-web) and a Painter version (src/painter/), then
//! prints a JSON report and writes the pictures. See research/notes/react-vs-painter.md.
#![allow(non_snake_case)]
mod painter;

use cw_scene::{metrics, Primitive, Rect, Scene};
use cw_web::script::{MemoryHost, Modifiers, Realm, UiEvent};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// The size the apps were designed at, and a smaller window to see how each copes.
const DESIGN: (u32, u32) = (1280, 800);
const SMALL: (u32, u32) = (1024, 768);
const BASE: &str = "https://example.test/";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..").canonicalize().unwrap()
}

// ---------------------------------------------------------------- the React side

fn host() -> MemoryHost {
    let dir = repo().join("crates/web/engine/tests/vendor");
    let mut h = MemoryHost::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        if let Ok(body) = std::fs::read_to_string(&p) {
            let ty = if name.ends_with(".css") { "text/css" } else { "text/javascript" };
            h = h.with_response(&format!("{BASE}vendor/{name}"), ty, &body);
        }
    }
    h
}

fn settle(r: &mut Realm, ms: u32) {
    for _ in 0..16 {
        if !r.run_until_idle(ms) {
            break;
        }
    }
}

/// The app in `state` as the framework-parity harness drives it, painted by cw-web.
fn react_scene(app: &str, state: &str, (vw, vh): (u32, u32)) -> Scene {
    let dir = repo().join("crates/web/engine/tests/framework-parity");
    let html = std::fs::read_to_string(dir.join(format!("app-{app}.html")))
        .unwrap()
        .replacen("<head>", "<head><style>* { scrollbar-width: none }</style>", 1);
    let steps: serde_json::Map<String, Value> = serde_json::from_str(
        &std::fs::read_to_string(dir.join(format!("app-{app}.steps.json"))).unwrap(),
    )
    .unwrap();
    let mut h = host();
    h.viewport = cw_web::Viewport { width: vw, height: vh, scale: 1, zoom: 100 };
    let mut r = Realm::new(&html, &format!("{BASE}app-{app}.html"), Box::new(h));
    r.run_document();
    settle(&mut r, 50);
    let modifiers = Modifiers::default();
    for step in steps[state].as_array().unwrap() {
        match step["action"].as_str().unwrap() {
            "click" => {
                let sel = step["selector"].as_str().unwrap();
                let v = r
                    .eval(&format!(
                        "(() => {{ const q = document.querySelector({sel:?}).getBoundingClientRect(); return [q.left + q.width / 2, q.top + q.height / 2].join(); }})()"
                    ))
                    .unwrap();
                let mut it = v.split(',').map(|n| n.parse::<f64>().unwrap() as i32);
                let (x, y) = (it.next().unwrap(), it.next().unwrap());
                r.dispatch(UiEvent::PointerMove { x, y, modifiers });
                r.dispatch(UiEvent::Click { x, y, button: 0, modifiers, detail: 1 });
            }
            "type" => {
                r.dispatch(UiEvent::TypeText { text: step["text"].as_str().unwrap().into() });
            }
            "press" => {
                r.dispatch(UiEvent::Key {
                    key: step["key"].as_str().unwrap().into(),
                    code: String::new(),
                    modifiers,
                    repeat: false,
                });
            }
            _ => {}
        }
        settle(&mut r, 20);
    }
    let tree = r.fragment_tree().clone();
    let styles = r.styles().clone();
    let doc = r.document().clone();
    let images = cw_web::paint::ImageMap::from_document(&doc, &styles);
    cw_web::paint::paint(
        &doc,
        &styles,
        &tree,
        cw_web::Viewport { width: vw, height: vh, scale: 1, zoom: 100 },
        &cw_web::paint::PaintContext::new(&images),
    )
}

// --------------------------------------------------------------- the Painter side

fn new_painter((w, h): (u32, u32)) -> cw_applications::desktop_scene::Painter {
    let mut p = cw_applications::desktop_scene::Painter::new(w, h);
    p.scene.typeface = cw_scene::Typeface::Inter;
    p
}

fn painter_scene(app: &str, state: &str, size: (u32, u32)) -> Scene {
    let mut p = new_painter(size);
    match app {
        "analytics" => {
            let mut d = painter::analytics::Dashboard::default();
            match state {
                "range-menu" => d.click("range"),
                "quarter" => {
                    d.click("range");
                    d.click("range:90d");
                }
                "reports" => d.click("tab:1"),
                _ => {}
            }
            d.render(&mut p);
        }
        "kanban" => {
            let mut b = painter::kanban::Board::default();
            match state {
                "modal" => b.click("new-task"),
                "invalid" => {
                    b.click("new-task");
                    b.draft = "Hi".into();
                    b.click("create");
                }
                "created" => {
                    b.click("new-task");
                    b.draft = "Write the launch announcement".into();
                    b.click("priority:2");
                    b.click("create");
                }
                "moved" => {
                    b.click("move:3");
                    b.click("move:4");
                }
                _ => {}
            }
            b.render(&mut p);
        }
        "settings" => {
            let mut s = painter::settings::Profile::default();
            match state {
                "errors" => {
                    s.click("field:website");
                    s.type_text("portfolio.site");
                    s.click("field:username");
                    s.type_text("!");
                    s.click("save");
                }
                "saved" => {
                    s.click("field:website");
                    s.type_text("https://maya.design");
                    s.click("save");
                }
                "notifications" => {
                    s.click("tab:2");
                    s.click("toggle:2");
                    s.click("toggle:0");
                }
                _ => {}
            }
            s.render(&mut p);
        }
        _ => unreachable!(),
    }
    p.scene
}

// ------------------------------------------------------------------- the checks

struct Text {
    r: Rect,
    natural: u32,
    size: u16,
    text: String,
    z: usize,
}

fn intersect(a: Rect, b: Rect) -> Option<Rect> {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = (a.x + a.width as i32).min(b.x + b.width as i32);
    let y1 = (a.y + a.height as i32).min(b.y + b.height as i32);
    (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32))
}

fn visible(n: &cw_scene::Node, (w, h): (u32, u32)) -> Option<Rect> {
    let mut r = n.bounds;
    if let Some(c) = n.clip {
        r = intersect(r, c)?;
    }
    if let Some(rc) = n.rounded_clip {
        r = intersect(r, rc.rect)?;
    }
    intersect(r, Rect::new(0, 0, w, h))
}

/// The layout-fault checklist (research/notes/react-vs-painter.md), counted from the
/// scene alone so both renderers are judged the same way.
fn faults(scene: &Scene, source: &str) -> Value {
    let (W, H) = (scene.width, scene.height);
    let mut order: Vec<&cw_scene::Node> = scene.nodes.iter().collect();
    order.sort_by_key(|n| (n.z, n.id));
    let mut texts = Vec::new();
    let mut boxes = Vec::new();
    let mut icons = Vec::new();
    let mut opaque: Vec<(Rect, usize)> = Vec::new();
    for (z, n) in order.iter().enumerate() {
        let Some(v) = visible(n, (W, H)) else { continue };
        match &n.primitive {
            Primitive::UiText { text, size, typeface, .. } | Primitive::UiTextBold { text, size, typeface, .. } => {
                if text.trim().is_empty() {
                    continue;
                }
                let face = typeface.unwrap_or(scene.typeface);
                let style = n.primitive.text_style().unwrap_or_default();
                // A node taller than two lines is a wrapped paragraph: its extent is
                // its widest wrapped line.
                let natural = if n.bounds.height >= *size as u32 * 2 {
                    metrics::wrap(face, style.bold, text, *size, n.bounds.width)
                        .iter()
                        .map(|l| metrics::text_width(face, style, l, *size))
                        .max()
                        .unwrap_or(0)
                } else {
                    metrics::text_width(face, style, text, *size)
                };
                texts.push(Text { r: v, natural, size: *size, text: text.clone(), z });
            }
            Primitive::Box { fill, border, .. } | Primitive::RoundedBox { fill, border, .. } => {
                if fill.3 > 0 || border.is_some() {
                    boxes.push((n.bounds, z));
                }
                if fill.3 >= 200 {
                    opaque.push((v, z));
                }
            }
            // A box with unequal corner radii paints as a closed path.
            Primitive::Path { fill: Some(fill), closed: true, .. } if fill.3 > 0 => {
                boxes.push((n.bounds, z));
                if fill.3 >= 200 {
                    opaque.push((v, z));
                }
            }
            Primitive::Symbol { .. } | Primitive::Image { .. } if v.width <= 48 && v.height <= 48 => {
                icons.push(v)
            }
            _ => {}
        }
    }
    let mut out = serde_json::Map::new();
    // F1 labels the renderer shortened: an ellipsis in text that does not occur in
    // the app's own source (a placeholder written with "…" is not a fault).
    let truncated: Vec<String> = texts
        .iter()
        .filter(|t| t.text.contains('…') && !source.contains(t.text.trim()))
        .map(|t| t.text.clone())
        .collect();
    // F2 text drawn over text: the glyphs' extents (the natural advance by the font
    // size, from the node's top-left) intersect by more than 2 px each way. Pieces
    // of one line (same top and size: runs split by markup or letter-spacing) are
    // one text, not a collision.
    let ink = |t: &Text| Rect::new(t.r.x, t.r.y, t.natural.min(t.r.width), t.size as u32);
    let mut overlaps = Vec::new();
    for (i, a) in texts.iter().enumerate() {
        for b in &texts[i + 1..] {
            if a.size == b.size && (a.r.y - b.r.y).abs() <= 2 {
                continue;
            }
            if let Some(x) = intersect(ink(a), ink(b)) {
                // Layered on purpose: an opaque surface painted between the two
                // (a dialog, a toast) hides the lower one there.
                let (lo, hi) = (a.z.min(b.z), a.z.max(b.z));
                let covered = opaque.iter().any(|(r, z)| {
                    *z > lo && *z < hi && intersect(*r, x) == Some(x)
                });
                if x.width > 2 && x.height > 2 && !covered {
                    overlaps.push(format!("{:?} / {:?}", a.text, b.text));
                }
            }
        }
    }
    // F3 text running past the box it starts in (the innermost filled or bordered
    // box painted before it that contains its first pixel).
    let container = |t: &Text| {
        boxes
            .iter()
            .filter(|(b, z)| *z < t.z && b.width < W && b.width >= 8 && b.height >= 8
                && b.x <= t.r.x && b.y <= t.r.y
                && t.r.x < b.x + b.width as i32 && t.r.y < b.y + b.height as i32)
            // The box painted last before the text is the one it sits on.
            .max_by_key(|(_, z)| *z)
            .map(|(b, _)| *b)
    };
    let mut overflow = Vec::new();
    for t in &texts {
        let inner = container(t).map(|b| (b, 0));
        if let Some((b, _)) = inner {
            let right = t.r.x + t.natural as i32;
            let bottom = t.r.y + t.size as i32;
            if right > b.x + b.width as i32 + 1 || bottom > b.y + b.height as i32 + 1 {
                overflow.push(format!(
                    "{:?} at {},{} ({}x{}) in {},{} {}x{}",
                    t.text, t.r.x, t.r.y, t.natural, t.size, b.x, b.y, b.width, b.height
                ));
            }
        }
    }
    // F4 near-miss alignment: same-size texts sharing a line whose tops differ by
    // 1-3 px, and icons beside text whose centres differ by 2-4 px.
    let mut misaligned = Vec::new();
    for (i, a) in texts.iter().enumerate() {
        for b in &texts[i + 1..] {
            let dy = (a.r.y - b.r.y).abs();
            let apart = a.r.x + a.r.width as i32 <= b.r.x || b.r.x + b.r.width as i32 <= a.r.x;
            let gap = (b.r.x - (a.r.x + a.r.width as i32)).max(a.r.x - (b.r.x + b.r.width as i32));
            let near = gap < 120 && container(a) == container(b);
            if a.size == b.size && apart && near && (1..=3).contains(&dy) {
                misaligned.push(format!("{:?} / {:?} ({dy} px)", a.text, b.text));
            }
        }
    }
    for ic in &icons {
        let icy = ic.y * 2 + ic.height as i32;
        if let Some(t) = texts.iter().find(|t| {
            let gap = t.r.x - (ic.x + ic.width as i32);
            (0..=16).contains(&gap) && intersect(Rect::new(t.r.x, ic.y, t.r.width, ic.height), t.r).is_some()
        }) {
            let tcy = t.r.y * 2 + t.size as i32 * 13 / 10;
            let d = (icy - tcy).abs() / 2;
            if (2..=4).contains(&d) {
                misaligned.push(format!("icon / {:?} ({d} px)", t.text));
            }
        }
    }
    // F5 inconsistent spacing in runs of three or more same-size boxes in a row or
    // column.
    let mut spacing = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for (b, _) in &boxes {
        let key = (b.width, b.height);
        if b.width < 12 || b.height < 12 || !seen.insert((key, b.y)) {
            continue;
        }
        for horizontal in [true, false] {
            let mut run: Vec<Rect> = boxes
                .iter()
                .map(|(r, _)| *r)
                .filter(|r| (r.width as i32 - b.width as i32).abs() <= 1 && (r.height as i32 - b.height as i32).abs() <= 1)
                .filter(|r| if horizontal { r.y == b.y } else { r.x == b.x })
                .collect();
            run.sort_by_key(|r| if horizontal { r.x } else { r.y });
            run.dedup_by_key(|r| if horizontal { r.x } else { r.y });
            if run.len() < 3 {
                continue;
            }
            let gaps: Vec<i32> = run
                .windows(2)
                .map(|w| if horizontal { w[1].x - w[0].x - w[0].width as i32 } else { w[1].y - w[0].y - w[0].height as i32 })
                .collect();
            let (lo, hi) = (*gaps.iter().min().unwrap(), *gaps.iter().max().unwrap());
            // A list or row of items sits within a few dozen pixels of each other;
            // boxes far apart (a card in each column) are not a run.
            if hi - lo > 1 && lo >= 0 && hi <= 64 {
                spacing.push(format!("{}x{} {} gaps {:?}", b.width, b.height, if horizontal { "row" } else { "column" }, gaps));
            }
        }
    }
    spacing.sort();
    spacing.dedup();
    // F6 content cut off by the right edge.
    // Content past the window's right edge that nothing lets the user reach: a node
    // clipped by a narrower scroll container scrolls into view, so it is not lost.
    let offscreen = scene
        .nodes
        .iter()
        .filter(|n| n.bounds.x + n.bounds.width as i32 > W as i32 && n.bounds.width < W)
        .filter(|n| n.clip.is_none_or(|c| c.x + c.width as i32 >= W as i32))
        .count();
    out.insert("truncated".into(), json!(truncated));
    out.insert("text_overlaps".into(), json!(overlaps));
    out.insert("text_overflowing_its_box".into(), json!(overflow));
    out.insert("near_miss_alignment".into(), json!(misaligned));
    out.insert("inconsistent_spacing".into(), json!(spacing));
    out.insert("cut_off_right".into(), json!(offscreen));
    Value::Object(out)
}

fn counts(v: &Value) -> Value {
    let mut m = serde_json::Map::new();
    for (k, x) in v.as_object().unwrap() {
        m.insert(k.clone(), json!(x.as_array().map(|a| a.len()).unwrap_or_else(|| x.as_u64().unwrap_or(0) as usize)));
    }
    Value::Object(m)
}

// -------------------------------------------------------------------- effort

/// Non-blank lines that are not only a comment.
fn loc(path: &Path) -> usize {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("//") && !l.starts_with("/*") && !l.starts_with('*'))
        .count()
}

fn write_png(path: &Path, scene: &Scene) {
    let frame = cw_render::Renderer::new().render(scene);
    let file = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), frame.width, frame.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&frame.rgba).unwrap();
}

fn main() {
    let out = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(|| repo().join("target/research/react-vs-painter"));
    std::fs::create_dir_all(&out).unwrap();
    let src = repo().join("crates/web/engine/tests/framework-parity/app-src");
    let here = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/painter");
    let mut report = serde_json::Map::new();
    let states: [(&str, &[&str]); 3] = [
        ("analytics", &["initial", "range-menu", "quarter", "reports"]),
        ("kanban", &["initial", "modal", "invalid", "created", "moved"]),
        ("settings", &["initial", "errors", "saved", "notifications"]),
    ];
    for (app, list) in states {
      for state in list.iter().copied() {
      for (size, tag) in [(DESIGN, ""), (SMALL, "@1024")] {
        let react = react_scene(app, state, size);
        let native = painter_scene(app, state, size);
        write_png(&out.join(format!("{app}.{state}{tag}.react-engine.png")), &react);
        write_png(&out.join(format!("{app}.{state}{tag}.painter.png")), &native);
        let source = std::fs::read_to_string(src.join(app).join("App.tsx")).unwrap()
            + &std::fs::read_to_string(here.join(format!("{app}.rs"))).unwrap();
        let (fr, fp) = (faults(&react, &source), faults(&native, &source));
        let tsx = loc(&src.join(app).join("App.tsx")) + loc(&src.join(app).join("data.ts"));
        report.insert(
            format!("{app}.{state}{tag}"),
            json!({
                "state": state,
                "react": { "counts": counts(&fr), "detail": fr, "loc_tsx": tsx },
                "painter": { "counts": counts(&fp), "detail": fp, "loc_rust": loc(&here.join(format!("{app}.rs"))) },
            }),
        );
      }
      }
    }
    report.insert(
        "shared".into(),
        json!({ "icons_tsx_loc": loc(&src.join("shared/icons.tsx")), "tailwind_and_build_loc": loc(&src.join("build.sh")) + loc(&src.join("tailwind.config.js")) + loc(&src.join("input.css")) }),
    );
    println!("{}", serde_json::to_string_pretty(&Value::Object(report)).unwrap());
}
