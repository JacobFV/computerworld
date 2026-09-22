//! Tailwind CSS 3.4 on the engine: a real generated stylesheet (`tests/vendor/
//! tailwind-3.4.17.css`, ~3,200 rules, built by `crates/web/tools/fetch-vendor.sh`
//! from `crates/web/tools/tailwind/`) over a 650-element page
//! (`tests/script/tailwind.html`).
//!
//! Three things are checked: the cascade over that sheet stays fast; every element's
//! box and computed values match what Chromium computed for the same page
//! (`tests/script/tailwind.chromium.json`, from `scripts/web-parity/dump.mjs`),
//! compared with `tests/support`'s rules; and the values that dump cannot reach
//! without interaction — `group-hover:` and `focus:` — match
//! `tailwind-hover.chromium.json`, driven through a `Realm`.
//!
//! The pipeline calls compile only with `--features pipeline` (see `support/mod.rs`):
//!
//!     cargo test -p cw-web --features pipeline --test tailwind

mod support;

#[cfg(feature = "pipeline")]
mod cases {
    use super::support::*;
    use cw_web::script::{MemoryHost, Modifiers, Realm, UiEvent};
    use std::collections::BTreeMap;
    use std::time::Instant;

    fn script_dir() -> std::path::PathBuf {
        crate_dir().join("tests/script")
    }

    fn stylesheet() -> String {
        let p = crate_dir().join("tests/vendor/tailwind-3.4.17.css");
        std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
    }

    fn fixture() -> String {
        let p = script_dir().join("tailwind.html");
        std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
    }

    /// The fixture with its `<link>` replaced by the stylesheet's text, so the
    /// pipeline runner (which fetches nothing) sees the same author sheet Chromium
    /// loaded. `<head>` is not part of either dump.
    fn inlined() -> String {
        let html = fixture();
        let link = "<link rel=\"stylesheet\" href=\"/vendor/tailwind-3.4.17.css\">";
        assert!(
            html.contains(link),
            "the fixture no longer links the stylesheet"
        );
        html.replace(link, &format!("<style>{}</style>", stylesheet()))
    }

    /// Every element of a Chromium dump by id, with its computed values.
    fn by_id(dump: &Dump) -> BTreeMap<String, BTreeMap<String, String>> {
        dump.nodes
            .iter()
            .filter_map(|n| match n {
                DumpNode::Element { id, computed, .. } if !id.is_empty() => {
                    Some((id.clone(), computed.clone()))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_stylesheet_is_a_real_tailwind_build() {
        let css = stylesheet();
        let rules = css.matches('{').count()
            - css.matches("@media").count()
            - css.matches("@keyframes").count();
        assert!(
            rules > 3000,
            "only {rules} rules: this is not a full Tailwind build"
        );
        // Preflight, the utilities, a responsive block and the custom-property chains.
        for marker in [
            "box-sizing: border-box",
            ".space-x-4 > :not([hidden]) ~ :not([hidden])",
            ".divide-y-2 > :not([hidden]) ~ :not([hidden])",
            "@media (min-width: 768px)",
            "--tw-ring-offset-shadow",
            ".group:hover .group-hover\\:",
            ".focus\\:",
        ] {
            assert!(css.contains(marker), "the stylesheet has no {marker:?}");
        }
    }

    /// The cascade over 3,200 rules and 650 elements, with no selector index, would be
    /// 2 million match attempts; it must stay well under a frame.
    #[test]
    fn the_cascade_stays_fast() {
        let html = inlined();
        // Warm the parse out of the measurement: only the cascade is timed.
        let doc = cw_web::html::parse(&html);
        let sheet = cw_web::css::parse_stylesheet(
            &stylesheet(),
            cw_web::css::Origin::Author,
            cw_web::Strictness::Lenient,
        )
        .expect("tailwind parses");
        let elements = doc
            .descendants(cw_web::dom::Document::ROOT)
            .filter(|n| doc.is_element(*n))
            .count();
        assert!(elements >= 500, "the fixture has only {elements} elements");
        let media = cw_web::css::Media {
            fonts: cw_web::css::FontEnvironment::LinuxBaseline,
            ..cw_web::css::Media::with_size(WIDTH as i32, HEIGHT as i32)
        };
        let ctx = cw_web::css::MatchContext::new();
        // One untimed run to fault in the sheet's selector index, then five timed.
        let _ = cw_web::style::cascade(
            &doc,
            std::slice::from_ref(&sheet),
            &media,
            &ctx,
            cw_web::Strictness::Lenient,
        )
        .expect("cascade");
        let mut best = f64::MAX;
        for _ in 0..5 {
            let t = Instant::now();
            let styles = cw_web::style::cascade(
                &doc,
                std::slice::from_ref(&sheet),
                &media,
                &ctx,
                cw_web::Strictness::Lenient,
            )
            .expect("cascade");
            best = best.min(t.elapsed().as_secs_f64() * 1000.0);
            std::hint::black_box(&styles);
        }
        eprintln!(
            "tailwind cascade: {elements} elements x {} rules, best of 5: {best:.1} ms",
            sheet.rules.len()
        );
        let limit = if cfg!(debug_assertions) { 1500.0 } else { 50.0 };
        assert!(
            best < limit,
            "the cascade took {best:.1} ms for {elements} elements (limit {limit} ms)"
        );
    }

    /// Preflight, `space-x-*`, `divide-*`, `md:`, arbitrary values and the geometry
    /// they produce, against Chromium's dump of the same page.
    #[test]
    fn boxes_and_computed_values_match_chromium() {
        let vp = viewport();
        let expected = read_dump(&script_dir().join("tailwind.chromium.json"));
        assert_eq!(expected.engine, "chromium");
        assert_eq!(
            (expected.viewport.width, expected.viewport.height),
            (WIDTH, HEIGHT)
        );
        let rendered = run(&inlined(), vp);
        let got = engine_dump("tailwind.html", &rendered, vp);
        let report = compare(&expected, &got);
        assert_eq!(
            report.missing,
            0,
            "nodes Chromium has that the engine does not: {:?}",
            report.worst(5)
        );

        // Every computed property of every element, exactly as Chromium computes it.
        // `width` and `height` are left out: for these boxes they are *used* values
        // that follow the shaped text, and the two engines shape Tailwind's
        // `ui-sans-serif, system-ui, …` stack with different faces (the page's
        // geometry differs by up to 12 px on wrapped card text for that reason alone).
        // `font-family` is informational for the same reason.
        let chromium = by_id(&expected);
        let engine: BTreeMap<String, BTreeMap<String, String>> = got
            .nodes
            .iter()
            .filter_map(|n| match n {
                DumpNode::Element { id, computed, .. } if !id.is_empty() => {
                    Some((id.clone(), computed.clone()))
                }
                _ => None,
            })
            .collect();
        let skip = ["font-family", "width", "height"];
        let mut mismatches: Vec<String> = Vec::new();
        let mut checked = 0usize;
        for (element, want) in &chromium {
            let Some(mine) = engine.get(element) else {
                panic!("the engine rendered no #{element}")
            };
            for p in PROPERTIES {
                if skip.contains(p) {
                    continue;
                }
                let e = normalise(p, want.get(*p).map(String::as_str).unwrap_or(""));
                let g = normalise(p, mine.get(*p).map(String::as_str).unwrap_or(""));
                checked += 1;
                let close = LENGTH_PROPERTIES.contains(p)
                    && matches!((parse_px(&e), parse_px(&g)), (Some(a), Some(b)) if (a - b).abs() <= 1.0);
                if e != g && !close {
                    mismatches.push(format!("#{element} {p}: Chromium {e:?}, engine {g:?}"));
                }
            }
        }
        eprintln!(
            "tailwind: {checked} computed values over {} elements, {} mismatched",
            chromium.len(),
            mismatches.len()
        );
        assert!(
            mismatches.is_empty(),
            "{} of {checked} computed values differ:\n{}",
            mismatches.len(),
            mismatches
                .iter()
                .take(20)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        );

        // The boxes the arbitrary values produce, where nothing depends on the text:
        // `w-[137px]` and the three tracks of `grid-cols-[repeat(3,minmax(0,1fr))]`.
        let rect = |element: &str| -> DumpRect {
            got.nodes
                .iter()
                .find_map(|n| match n {
                    DumpNode::Element { id, rect, .. } if id == element => Some(*rect),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("no #{element}"))
        };
        let want_rect = |element: &str| -> DumpRect {
            expected
                .nodes
                .iter()
                .find_map(|n| match n {
                    DumpNode::Element { id, rect, .. } if id == element => Some(*rect),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("no #{element} in Chromium's dump"))
        };
        for element in [
            "tw-arbitrary-width",
            "tw-arbitrary-grid-a",
            "tw-arbitrary-grid-b",
            "tw-arbitrary-grid-c",
        ] {
            let (w, g) = (want_rect(element), rect(element));
            assert!(
                (w.width - g.width).abs() <= 1.0 && (w.x - g.x).abs() <= 1.0,
                "#{element}: Chromium x {} w {}, engine x {} w {}",
                w.x,
                w.width,
                g.x,
                g.width
            );
        }

        // The properties beyond the shared set, asserted by id off the same dump:
        // preflight's resets, the spacing utilities, the responsive variant, the
        // arbitrary values and the `--tw-*` chains.
        let mut r = realm();
        for (id, props) in [
            (
                "tw-h1",
                &["margin-top", "margin-bottom", "font-size", "font-weight"][..],
            ),
            (
                "tw-ul",
                &["list-style-type", "padding-left", "margin-top"][..],
            ),
            (
                "tw-btn-preflight",
                &[
                    "border-top-width",
                    "background-color",
                    "font-family",
                    "box-sizing",
                ][..],
            ),
            // (`height` is preflight's `auto` over the SVG data URL's intrinsic size,
            // which this engine does not decode; the cascaded `display` is the point.)
            ("tw-img", &["display", "border-top-width"][..]),
            ("tw-blockquote", &["margin-top", "margin-left"][..]),
            ("tw-space-x-item-2", &["margin-left"][..]),
            ("tw-space-y-item-2", &["margin-top"][..]),
            (
                "tw-divide-x-item-2",
                &["border-left-width", "border-left-color"][..],
            ),
            (
                "tw-divide-y-item-2",
                &["border-top-width", "border-top-color"][..],
            ),
            ("tw-md-responsive", &["display"][..]),
            ("tw-arbitrary-width", &["width"][..]),
            ("tw-arbitrary-color", &["color"][..]),
            ("tw-arbitrary-margin", &["margin-top"][..]),
            ("tw-arbitrary-bg", &["background-color"][..]),
            (
                "tw-arbitrary-grid",
                &["display", "gap", "column-gap", "row-gap"][..],
            ),
            ("tw-shadow", &["box-shadow", "--tw-shadow"][..]),
            (
                "tw-ring",
                &["box-shadow", "--tw-ring-color", "--tw-ring-offset-width"][..],
            ),
            (
                "tw-bg-opacity",
                &["background-color", "--tw-bg-opacity"][..],
            ),
            ("tw-text-opacity", &["color", "--tw-text-opacity"][..]),
            (
                "tw-border-opacity",
                &["border-top-color", "--tw-border-opacity"][..],
            ),
            (
                "tw-transform",
                &["transform", "--tw-translate-x", "--tw-scale-x"][..],
            ),
            ("tw-arbitrary-grid-b", &["background-color"][..]),
            ("tw-backdrop-blur", &["--tw-backdrop-blur"][..]),
        ] {
            let want = chromium
                .get(id)
                .unwrap_or_else(|| panic!("#{id} is not in Chromium's dump"));
            for p in props {
                let expected = normalise(
                    p,
                    want.get(*p)
                        .unwrap_or_else(|| panic!("#{id}: Chromium dumped no {p}")),
                );
                let got = normalise(p, &computed(&mut r, id, p));
                assert_eq!(got, expected, "#{id} {p}");
            }
        }
    }

    /// A realm over the fixture, with the stylesheet served from `tests/vendor/`, so
    /// `:hover` and `:focus` can be driven and `getComputedStyle` asked for anything.
    fn realm() -> Realm {
        let host = MemoryHost::new().with_response(
            "https://example.test/vendor/tailwind-3.4.17.css",
            "text/css",
            &stylesheet(),
        );
        let mut r = Realm::new(
            &fixture(),
            "https://example.test/tailwind.html",
            Box::new(host),
        );
        r.run_document();
        r.run_until_idle(20);
        assert!(r.logs().is_empty(), "the fixture logged: {:?}", r.logs());
        r
    }

    fn computed(r: &mut Realm, id: &str, prop: &str) -> String {
        r.eval(&format!(
            "getComputedStyle(document.getElementById('{id}')).getPropertyValue('{prop}')"
        ))
        .unwrap_or_else(|e| panic!("#{id} {prop}: {e}"))
    }

    /// `group-hover:` and `focus:` variants: the states Chromium was put into for
    /// `tailwind-hover.chromium.json` (`tailwind-state.json`), reproduced here.
    #[test]
    fn group_hover_and_focus_variants_match_chromium() {
        let hovered = by_id(&read_dump(
            &script_dir().join("tailwind-hover.chromium.json"),
        ));
        let plain = by_id(&read_dump(&script_dir().join("tailwind.chromium.json")));
        let mut r = realm();
        // Not hovered, not focused: the base values.
        for (id, p) in [
            ("tw-group-hover-target", "color"),
            ("tw-group-hover-target", "text-decoration-line"),
            ("tw-focus-btn", "box-shadow"),
        ] {
            assert_eq!(
                normalise(p, &computed(&mut r, id, p)),
                normalise(p, &plain[id][p]),
                "#{id} {p} before the interaction"
            );
        }
        // Hover the group: only the descendant's `group-hover:` utilities change.
        let group = {
            let d = r.document();
            d.by_id("tw-group")[0]
        };
        let (x, y) = {
            let rect = r.eval("const q = document.getElementById('tw-group').getBoundingClientRect(); [q.left + q.width / 2, q.top + q.height / 2].join()").unwrap();
            let mut it = rect.split(',').map(|v| v.parse::<f64>().unwrap() as i32);
            (it.next().unwrap(), it.next().unwrap())
        };
        r.dispatch(UiEvent::PointerMove {
            x,
            y,
            modifiers: Modifiers::default(),
        });
        let _ = group;
        for p in ["color", "outline-color", "text-decoration-line"] {
            assert_eq!(
                normalise(p, &computed(&mut r, "tw-group-hover-target", p)),
                normalise(p, &hovered["tw-group-hover-target"][p]),
                "#tw-group-hover-target {p} while the group is hovered"
            );
        }
        // Focus the button: `focus:` and `focus-visible:` and the ring chain.
        let btn = {
            let d = r.document();
            d.by_id("tw-focus-btn")[0]
        };
        r.dispatch(UiEvent::Focus { node: Some(btn) });
        for p in [
            "box-shadow",
            "outline-width",
            "outline-color",
            "--tw-ring-color",
            "--tw-ring-offset-width",
        ] {
            assert_eq!(
                normalise(p, &computed(&mut r, "tw-focus-btn", p)),
                normalise(p, &hovered["tw-focus-btn"][p]),
                "#tw-focus-btn {p} while focused"
            );
        }
        // Moving the pointer away puts the group's descendant back.
        r.dispatch(UiEvent::PointerMove {
            x: 1,
            y: 1,
            modifiers: Modifiers::default(),
        });
        assert_eq!(
            normalise("color", &computed(&mut r, "tw-group-hover-target", "color")),
            normalise("color", &plain["tw-group-hover-target"]["color"])
        );
    }
}
