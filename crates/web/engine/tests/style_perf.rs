//! Where a post-interaction flush spends its time: each step of every
//! framework-parity fixture (`tests/framework-parity/<name>.steps.json`) is driven on
//! the Realm as `framework_parity.rs` drives it, with the style/layout/paint phase
//! timers of `cw_web::style::profile` on, then the page is painted once as a host
//! paints a frame. Printed per fixture and step (median over runs): the step's wall
//! time, the script share (wall minus the style and layout phases), and each phase.
//! A from-scratch style + layout + paint of the settled page is printed for scale.
//!
//!     cargo test --release -p cw-web --features pipeline --test style_perf -- --ignored --nocapture
//!
//! `STYLE_PERF_RUNS` sets the runs (default 7), `STYLE_PERF_FIXTURE` limits the run to
//! fixtures whose name contains it.

#[cfg(feature = "pipeline")]
mod perf {
    use cw_web::script::{MemoryHost, Modifiers, Realm, UiEvent};
    use cw_web::style::profile::{self, Phase, Times, PHASES};
    use serde_json::Value;
    use std::path::PathBuf;
    use std::sync::OnceLock;
    use std::time::Instant;

    const BASE: &str = "https://example.test/";

    fn crate_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }
    fn fixture_dir() -> PathBuf {
        crate_dir().join("tests/framework-parity")
    }

    fn clock() -> u64 {
        static START: OnceLock<Instant> = OnceLock::new();
        START.get_or_init(Instant::now).elapsed().as_nanos() as u64
    }

    fn host() -> MemoryHost {
        let mut h = MemoryHost::new();
        for dir in [crate_dir().join("tests/vendor"), fixture_dir()] {
            let mut entries: Vec<_> = std::fs::read_dir(&dir)
                .unwrap()
                .map(|e| e.unwrap().path())
                .filter(|p| p.is_file())
                .collect();
            entries.sort();
            let vendor = dir.ends_with("vendor");
            for p in entries {
                let name = p.file_name().unwrap().to_string_lossy().into_owned();
                let (ty, prefix) = if name.ends_with(".css") {
                    ("text/css", "vendor/")
                } else if name.ends_with(".js") {
                    ("text/javascript", if vendor { "vendor/" } else { "" })
                } else {
                    continue;
                };
                let Ok(body) = std::fs::read_to_string(&p) else {
                    continue;
                };
                h = h.with_response(&format!("{BASE}{prefix}{name}"), ty, &body);
            }
        }
        h
    }

    fn centre(r: &mut Realm, selector: &str) -> (i32, i32) {
        let src = format!(
            "(() => {{ const q = document.querySelector({selector:?}).getBoundingClientRect(); \
             return [q.left + q.width / 2, q.top + q.height / 2].join(); }})()"
        );
        let v = r.eval(&src).unwrap_or_else(|e| panic!("{selector}: {e}"));
        let mut it = v.split(',').map(|n| n.parse::<f64>().unwrap() as i32);
        (it.next().unwrap(), it.next().unwrap())
    }

    fn settle(r: &mut Realm, advance_ms: u32) {
        for _ in 0..16 {
            if !r.run_until_idle(advance_ms) {
                break;
            }
        }
    }

    fn viewport() -> cw_web::Viewport {
        cw_web::Viewport {
            width: 1280,
            height: 800,
            scale: 1,
            zoom: 100,
        }
    }

    fn paint(r: &mut Realm) {
        let tree = r.fragment_tree().clone();
        let styles = r.styles().clone();
        let doc = r.document();
        let images = cw_web::paint::ImageMap::from_document(&doc, &styles);
        std::hint::black_box(cw_web::paint::paint(
            &doc,
            &styles,
            &tree,
            viewport(),
            &cw_web::paint::PaintContext::new(&images),
        ));
    }

    /// One step, then a frame's paint; returns the wall time of the step (ns) and
    /// the phase times of step and paint together.
    fn step(r: &mut Realm, step: &Value) -> (u64, Times) {
        let modifiers = Modifiers::default();
        // The target's position is looked up before the timer starts.
        let at = match step["action"].as_str().unwrap() {
            "click" => Some(centre(r, step["selector"].as_str().unwrap())),
            _ => None,
        };
        r.fragment_tree();
        profile::take();
        let t = clock();
        match step["action"].as_str().unwrap() {
            "click" => {
                let (x, y) = at.unwrap();
                r.dispatch(UiEvent::PointerMove { x, y, modifiers });
                r.dispatch(UiEvent::Click {
                    x,
                    y,
                    button: 0,
                    modifiers,
                    detail: 1,
                });
            }
            "type" => {
                r.dispatch(UiEvent::TypeText {
                    text: step["text"].as_str().unwrap().into(),
                });
            }
            "press" => {
                r.dispatch(UiEvent::Key {
                    key: step["key"].as_str().unwrap().into(),
                    code: String::new(),
                    modifiers,
                    repeat: false,
                });
            }
            other => panic!("unknown action {other:?}"),
        }
        settle(r, 20);
        // A frame: the host lays out (if script did not) and paints.
        r.fragment_tree();
        paint(r);
        let wall = clock() - t;
        (wall, profile::take())
    }

    fn med(mut v: Vec<u64>) -> f64 {
        v.sort();
        v[v.len() / 2] as f64 / 1e6
    }

    #[derive(Default)]
    struct Row {
        label: String,
        wall: Vec<u64>,
        phases: Vec<Vec<u64>>,
    }

    fn print_header() {
        let mut h = format!("{:<44} {:>8} {:>8}", "step", "wall", "script");
        for p in PHASES {
            h.push_str(&format!(" {:>9}", short(p)));
        }
        eprintln!("{h}");
    }

    fn short(p: Phase) -> &'static str {
        match p {
            Phase::Sheets => "sheets",
            Phase::EngineBuild => "eng.build",
            Phase::Invalidation => "invalid.",
            Phase::Match => "match",
            Phase::Compute => "compute",
            Phase::StyleOther => "style.oth",
            Phase::BoxTree => "boxtree",
            Phase::Layout => "layout",
            Phase::Paint => "paint",
            Phase::HitTest => "hittest",
        }
    }

    fn print_row(row: &Row) {
        let wall = med(row.wall.clone());
        let mut phase_meds: Vec<f64> = Vec::new();
        for i in 0..PHASES.len() {
            phase_meds.push(med(row.phases.iter().map(|p| p[i]).collect()));
        }
        // Script: wall minus the pipeline's top-level phases (Match and Compute run
        // inside the flush, as do Invalidation and EngineBuild; none nest in another
        // top-level phase).
        let pipeline: f64 = phase_meds.iter().sum();
        let mut s = format!(
            "{:<44} {:>8.3} {:>8.3}",
            row.label,
            wall,
            (wall - pipeline).max(0.0)
        );
        for m in phase_meds {
            s.push_str(&format!(" {:>9.3}", m));
        }
        eprintln!("{s}");
    }

    fn fixtures() -> Vec<String> {
        let filter = std::env::var("STYLE_PERF_FIXTURE").unwrap_or_default();
        let mut names: Vec<String> = std::fs::read_dir(fixture_dir())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let n = e.file_name().to_string_lossy().into_owned();
                n.strip_suffix(".html").map(str::to_owned)
            })
            .filter(|n| n.contains(&filter))
            .collect();
        names.sort();
        names
    }

    fn states(name: &str) -> Vec<(String, Vec<Value>)> {
        let text =
            std::fs::read_to_string(fixture_dir().join(format!("{name}.steps.json"))).unwrap();
        let v: serde_json::Map<String, Value> = serde_json::from_str(&text).unwrap();
        v.into_iter()
            .map(|(k, l)| (k, l.as_array().unwrap().clone()))
            .collect()
    }

    fn boot(name: &str, html: &str) -> Realm {
        let mut r = Realm::new(html, &format!("{BASE}{name}.html"), Box::new(host()));
        r.set_overlay_scrollbars(true);
        r.run_document();
        settle(&mut r, 50);
        r.fragment_tree();
        r
    }

    /// A from-scratch cascade, layout and paint of the realm's settled document.
    fn full_pass(r: &mut Realm) -> (u64, Times) {
        let (doc, sheets, media) = {
            let inner = r.layout();
            let sheets: Vec<_> = inner
                .sheets
                .iter()
                .filter(|s| !s.disabled)
                .map(|s| s.sheet.clone())
                .collect();
            (inner.doc.clone(), sheets, inner.media())
        };
        let ctx = cw_web::css::MatchContext::new();
        profile::take();
        let t = clock();
        let styles =
            cw_web::style::cascade(&doc, &sheets, &media, &ctx, cw_web::Strictness::Lenient)
                .unwrap();
        let tree = cw_web::layout::layout(&doc, &styles, viewport());
        let images = cw_web::paint::ImageMap::from_document(&doc, &styles);
        std::hint::black_box(cw_web::paint::paint(
            &doc,
            &styles,
            &tree,
            viewport(),
            &cw_web::paint::PaintContext::new(&images),
        ));
        (clock() - t, profile::take())
    }

    #[test]
    #[ignore]
    fn phases() {
        let runs: usize = std::env::var("STYLE_PERF_RUNS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(7);
        profile::set_clock(Some(clock));
        for name in fixtures() {
            let html = std::fs::read_to_string(fixture_dir().join(format!("{name}.html"))).unwrap();
            let mut rows: Vec<Row> = Vec::new();
            let mut full = Row {
                label: "(full style+layout+paint)".into(),
                ..Default::default()
            };
            for _ in 0..runs {
                let mut i = 0;
                for (state, list) in states(&name) {
                    if list.is_empty() {
                        continue;
                    }
                    let mut r = boot(&name, &html);
                    for (k, s) in list.iter().enumerate() {
                        let (wall, times) = step(&mut r, s);
                        if rows.len() <= i {
                            let what = s["selector"]
                                .as_str()
                                .or(s["text"].as_str())
                                .or(s["key"].as_str())
                                .unwrap_or("");
                            let what: String = what.chars().take(22).collect();
                            rows.push(Row {
                                label: format!(
                                    "{state}#{k} {} {what}",
                                    s["action"].as_str().unwrap()
                                ),
                                ..Default::default()
                            });
                        }
                        rows[i].wall.push(wall);
                        rows[i].phases.push(times.nanos.to_vec());
                        i += 1;
                    }
                    if full.wall.len() < rows[0].wall.len() {
                        let (w, t) = full_pass(&mut r);
                        full.wall.push(w);
                        full.phases.push(t.nanos.to_vec());
                    }
                }
            }
            let elements = {
                let mut r = boot(&name, &html);
                let doc = r.document();
                let n = doc
                    .descendants(cw_web::dom::Document::ROOT)
                    .filter(|n| doc.is_element(*n))
                    .count();
                drop(doc);
                let _ = &mut r;
                n
            };
            eprintln!("\n== {name} ({elements} elements, median of {runs}, ms)");
            print_header();
            for row in &rows {
                print_row(row);
            }
            print_row(&full);
        }
        profile::set_clock(None);
    }
}
