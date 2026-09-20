//! Reftests: `tests/ref/<name>.html` and `<name>-ref.html` must paint to identical
//! scenes. Two documents written differently (a `<b>` and a `font-weight: bold`, a
//! `cellpadding` and a `padding`) are laid out and painted at the same viewport; the
//! scene digests, with node ids erased so differing DOM shapes do not count, must match.
//! No Chromium is needed, so these run anywhere the crate builds.

mod support;

use support::*;

fn pairs() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(ref_dir())
        .expect("tests/ref")
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let stem = name.strip_suffix(".html")?;
            if stem.ends_with("-ref") {
                None
            } else {
                Some(stem.to_owned())
            }
        })
        .collect();
    names.sort();
    names
}

#[test]
fn every_test_has_a_reference() {
    let names = pairs();
    assert!(names.len() >= 20, "expected at least twenty reftest pairs, found {}", names.len());
    for name in &names {
        let test = ref_dir().join(format!("{name}.html"));
        let reference = ref_dir().join(format!("{name}-ref.html"));
        assert!(reference.exists(), "{name}: missing {}", reference.display());
        let a = std::fs::read_to_string(&test).unwrap();
        let b = std::fs::read_to_string(&reference).unwrap();
        assert!(a.contains("<body") && b.contains("<body"), "{name}: both files need a body");
        assert_ne!(a, b, "{name}: the test and its reference are the same file");
    }
}

/// Pairs that are compared after a fragment navigation: `(pair, fragment)`. Acid2 is
/// written to be viewed at `#top` (the face sits 100em below an intro box and the
/// scalp, the eyes' backgrounds and the chin's are fixed to the viewport), and its
/// pixel reference is compared the same way, as WPT's `acid/acid2/reftest.html` does.
const NAVIGATE: &[(&str, &str)] = &[("acid2", "top")];

/// Pairs allowed a few differing pixels, each with its reason: `(pair, pixels, why)`.
/// Everything else must be identical.
const TOLERANCE: &[(&str, usize, &str)] = &[];

fn fragment_of(name: &str) -> Option<&'static str> {
    NAVIGATE.iter().find(|(n, _)| *n == name).map(|(_, f)| *f)
}

fn tolerance_of(name: &str) -> usize {
    TOLERANCE.iter().find(|(n, ..)| *n == name).map(|(_, px, _)| *px).unwrap_or(0)
}

struct Outcome {
    name: String,
    digest_test: u64,
    digest_ref: u64,
    nodes_test: usize,
    nodes_ref: usize,
    /// Pixels that differ between the two rasters through `cw-render`.
    differing_pixels: usize,
}

/// Runs every pair, keeps rasters of the ones whose scenes differ, and returns each
/// pair's digests and pixel difference.
fn run_pairs() -> Vec<Outcome> {
    let vp = viewport();
    let mut out = Vec::new();
    for name in pairs() {
        let test = std::fs::read_to_string(ref_dir().join(format!("{name}.html"))).unwrap();
        let reference = std::fs::read_to_string(ref_dir().join(format!("{name}-ref.html"))).unwrap();
        let a = run_at(&test, vp, fragment_of(&name));
        let b = run_at(&reference, vp, fragment_of(&name));
        let (da, db) = (content_digest(&a.scene), content_digest(&b.scene));
        let (fa, fb) = (rasterise(&a.scene), rasterise(&b.scene));
        let differing_pixels = fa.rgba.chunks(4).zip(fb.rgba.chunks(4)).filter(|(x, y)| x != y).count() + fa.rgba.len().abs_diff(fb.rgba.len()) / 4;
        if da != db || differing_pixels > 0 {
            // Keep both rasters for a human to look at.
            let dir = out_dir();
            write_png(&dir.join(format!("ref-{name}.test.png")), &fa);
            write_png(&dir.join(format!("ref-{name}.ref.png")), &fb);
        }
        out.push(Outcome { name, digest_test: da, digest_ref: db, nodes_test: a.scene.nodes.len(), nodes_ref: b.scene.nodes.len(), differing_pixels });
    }
    out
}

/// The gate: both documents must rasterise to the same pixels through the shared
/// renderer.
#[test]
#[cfg_attr(not(feature = "pipeline"), ignore = "needs the html, css and style modules (`--features pipeline`)")]
fn reftests_paint_identical_pixels() {
    let failures: Vec<String> = run_pairs()
        .into_iter()
        .filter(|o| o.differing_pixels > tolerance_of(&o.name))
        .map(|o| format!("{}: {} pixels differ (see target-parity/ref-{}.test.png and .ref.png)", o.name, o.differing_pixels, o.name))
        .collect();
    assert!(failures.is_empty(), "reftest failures:\n{}", failures.join("\n"));
}

/// Pairs whose two documents paint the same pixels from a legitimately different
/// node order: a table paints every cell's background before any cell's text
/// (CSS 2.1 Appendix E, steps 4 and 7 of the table's stacking), while floats paint
/// each box with its own text, so the scene comparison for them is the pixel one.
/// Acid2 builds the face from floats, tables, fixed boxes and generated content and
/// its reference from fourteen plain lines, so only the pixels can agree.
const DIFFERENT_DECOMPOSITION: &[&str] = &["float-vs-table", "acid2"];

/// The strict form: identical scene digests (`Scene::stamp`, node ids and the
/// accessibility regions erased, see `support::content_digest`). A pair that passes
/// the pixel gate but fails this one paints the same picture with a different node
/// decomposition (a table's cell boxes against a float's, say), which the paint
/// track may or may not want to unify.
#[test]
#[cfg_attr(not(feature = "pipeline"), ignore = "needs the html, css and style modules (`--features pipeline`)")]
fn reftests_paint_identical_scenes() {
    let failures: Vec<String> = run_pairs()
        .into_iter()
        .filter(|o| if DIFFERENT_DECOMPOSITION.contains(&o.name.as_str()) { o.differing_pixels > tolerance_of(&o.name) } else { o.digest_test != o.digest_ref })
        .map(|o| {
            format!(
                "{}: test {:016x} != ref {:016x} ({} vs {} scene nodes; {} pixels differ)",
                o.name, o.digest_test, o.digest_ref, o.nodes_test, o.nodes_ref, o.differing_pixels
            )
        })
        .collect();
    assert!(failures.is_empty(), "scene digest mismatches:\n{}", failures.join("\n"));
}
