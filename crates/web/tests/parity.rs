//! Layout parity against Chromium. For every `tests/parity/<name>.html`, the engine
//! parses, cascades, lays out and paints the page at the same viewport Chromium dumped
//! it at (`<name>.chromium.json`, made by `scripts/web-parity/dump.mjs`), writes
//! `target-parity/<name>.engine.json` and `.engine.png` in the same shape, compares the
//! two dumps, writes `target-parity/<name>.report.md`, and fails when the pass rate is
//! below the fixture's threshold in `tests/parity/thresholds.json`.
//!
//! The pipeline calls compile only with `--features pipeline` (see `support/mod.rs`);
//! until the html, css and style modules land the pipeline tests are ignored.

mod support;

use std::path::Path;
use support::*;

fn fixtures() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(parity_dir())
        .expect("tests/parity")
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.strip_suffix(".html").map(str::to_owned)
        })
        .collect();
    names.sort();
    names
}

#[test]
fn every_fixture_has_a_chromium_dump_and_a_threshold() {
    let thresholds = thresholds();
    let names = fixtures();
    assert!(!names.is_empty(), "no fixtures under tests/parity");
    for name in &names {
        let dump_path = parity_dir().join(format!("{name}.chromium.json"));
        assert!(
            dump_path.exists(),
            "{name}: missing {} (run scripts/web-parity/dump.mjs)",
            dump_path.display()
        );
        let dump = read_dump(&dump_path);
        assert_eq!(dump.engine, "chromium", "{name}: dump is not Chromium's");
        assert_eq!(
            (dump.viewport.width, dump.viewport.height),
            (WIDTH, HEIGHT),
            "{name}: dumped at another viewport"
        );
        assert_eq!(
            dump.properties, PROPERTIES,
            "{name}: property list drifted from support::PROPERTIES"
        );
        assert!(
            dump.nodes
                .iter()
                .any(|n| matches!(n, DumpNode::Element { .. })),
            "{name}: no elements dumped"
        );
        let t = thresholds
            .get(name.as_str())
            .unwrap_or_else(|| panic!("{name}: no entry in thresholds.json"));
        assert!(
            (0.0..=1.0).contains(t),
            "{name}: threshold {t} out of range"
        );
    }
    for name in thresholds.keys() {
        assert!(
            names.contains(name),
            "thresholds.json names `{name}` but there is no fixture"
        );
    }
}

#[test]
fn comparing_a_dump_with_itself_passes_everything() {
    for name in fixtures() {
        let dump = read_dump(&parity_dir().join(format!("{name}.chromium.json")));
        let report = compare(&dump, &dump);
        assert_eq!(
            report.passed,
            report.total,
            "{name}: self-comparison failed: {:?}",
            report.worst(3)
        );
        assert_eq!(report.missing, 0);
    }
}

#[test]
fn comparison_tolerances_apply() {
    let dump = read_dump(&parity_dir().join("google-1998.chromium.json"));
    let mut shifted = dump.clone();
    for n in &mut shifted.nodes {
        match n {
            DumpNode::Element { rect, .. } => rect.x += 0.75,
            DumpNode::Text { rects, .. } => rects.iter_mut().for_each(|r| {
                r.y += 0.5;
                r.width += 1.5;
            }),
        }
    }
    let report = compare(&dump, &shifted);
    assert_eq!(
        report.passed,
        report.total,
        "within tolerance: {:?}",
        report.worst(3)
    );
    for n in &mut shifted.nodes {
        if let DumpNode::Element { rect, computed, .. } = n {
            rect.x += 1.0;
            computed.insert("display".into(), "flex".into());
        }
    }
    let report = compare(&dump, &shifted);
    assert!(report.passed < report.total);
    assert!(report.by_property.contains_key("display"));
    assert!(report.by_property.contains_key("rect"));
    let md = report.to_markdown(&[], 0.5);
    assert!(md.contains("FAIL") && md.contains("Worst offenders"));
}

fn run_fixture(name: &str) -> Report {
    let html = std::fs::read_to_string(parity_dir().join(format!("{name}.html"))).expect("fixture");
    let expected = read_dump(&parity_dir().join(format!("{name}.chromium.json")));
    let vp = viewport();
    let rendered = run(&html, vp);
    let got = engine_dump(&format!("{name}.html"), &rendered, vp);
    let out = out_dir();
    std::fs::write(
        out.join(format!("{name}.engine.json")),
        serde_json::to_string_pretty(&got).unwrap(),
    )
    .expect("write engine dump");
    let frame = rasterise(&rendered.scene);
    write_png(&out.join(format!("{name}.engine.png")), &frame);
    let report = compare(&expected, &got);
    let threshold = thresholds().get(name).copied().unwrap_or(0.0);
    std::fs::write(
        out.join(format!("{name}.report.md")),
        report.to_markdown(&got.fonts, threshold),
    )
    .expect("write report");
    eprintln!(
        "{name}: {}/{} nodes within tolerance ({:.1}%, threshold {:.1}%)",
        report.passed,
        report.total,
        report.pass_rate() * 100.0,
        threshold * 100.0
    );
    report
}

fn assert_threshold(name: &str) {
    let report = run_fixture(name);
    let threshold = thresholds().get(name).copied().unwrap_or(0.0);
    assert!(
        report.pass_rate() >= threshold,
        "{name}: pass rate {:.1}% below threshold {:.1}%; see {}",
        report.pass_rate() * 100.0,
        threshold * 100.0,
        Path::new("target-parity")
            .join(format!("{name}.report.md"))
            .display()
    );
}

macro_rules! parity_fixture {
    ($test:ident, $name:literal) => {
        #[test]
        #[cfg_attr(
            not(feature = "pipeline"),
            ignore = "needs the html, css and style modules (`--features pipeline`)"
        )]
        fn $test() {
            assert_threshold($name);
        }
    };
}

parity_fixture!(parity_google_1998, "google-1998");
parity_fixture!(parity_wikipedia_article, "wikipedia-article");
parity_fixture!(parity_docs_page, "docs-page");
parity_fixture!(parity_hn_front, "hn-front");
parity_fixture!(parity_acid1, "acid1");
parity_fixture!(parity_tables, "tables");
parity_fixture!(parity_google_modern, "google-modern");
parity_fixture!(parity_github_repo, "github-repo");
parity_fixture!(parity_stripe_marketing, "stripe-marketing");
parity_fixture!(parity_amazon_grid, "amazon-grid");
parity_fixture!(parity_slack_shell, "slack-shell");
parity_fixture!(parity_acid2, "acid2");

#[test]
#[cfg_attr(
    not(feature = "pipeline"),
    ignore = "needs the html, css and style modules (`--features pipeline`)"
)]
fn every_fixture_has_a_parity_test() {
    // The macro list above must name every fixture on disk, or a new fixture would be
    // dumped but never gated.
    let listed = [
        "google-1998",
        "wikipedia-article",
        "docs-page",
        "hn-front",
        "acid1",
        "tables",
        "google-modern",
        "github-repo",
        "stripe-marketing",
        "amazon-grid",
        "slack-shell",
        "acid2",
    ];
    for name in fixtures() {
        assert!(
            listed.contains(&name.as_str()),
            "add a parity_fixture! entry for {name}"
        );
    }
}
