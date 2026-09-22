//! Web Platform Tests reftests: for every pair in `tests/wpt/manifest.json` (written
//! by `tools/fetch-wpt.py`; see `tests/wpt/README.md`), the engine parses, cascades,
//! lays out and paints the test and its reference at 800×600, the WPT default, and
//! compares the rasters pixel for pixel. A `rel=match` pair passes when the two
//! rasters are identical, a `rel=mismatch` pair when they differ. Stylesheets linked
//! with `<link rel=stylesheet>` or `@import` are read from the corpus tree (absolute
//! `/css/support/...` paths resolve against the tree root), and `font-family: Ahem` is
//! mapped to the bundled JetBrains Mono on both sides so the pair still compares equal.
//!
//! The results land in `target-parity/wpt-report.md`, grouped by directory with
//! counts. The gate is `tests/wpt/expectations.json`: the test fails only when a
//! directory's pass count drops below its entry, which starts at 0 everywhere and is
//! raised by the integration step as the engine improves.
//!
//! Like the parity and reftest runners, the pipeline calls compile only with
//! `--features pipeline,wpt` (the full corpus takes about half an hour); without it only the manifest, the expectations and the file
//! layout are checked.

mod support;

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use support::*;

/// The WPT default viewport.
pub const WPT_WIDTH: u32 = 800;
pub const WPT_HEIGHT: u32 = 600;

#[derive(Debug, Deserialize)]
struct Manifest {
    commit: String,
    viewport: ManifestViewport,
    directories: Vec<Directory>,
}

#[derive(Debug, Deserialize)]
struct ManifestViewport {
    width: u32,
    height: u32,
}

#[derive(Debug, Deserialize)]
struct Directory {
    dir: String,
    pairs: Vec<Pair>,
}

#[derive(Debug, Deserialize)]
struct Pair {
    test: String,
    kind: String,
    #[serde(rename = "ref")]
    reference: String,
}

fn wpt_dir() -> PathBuf {
    crate_dir().join("tests/wpt")
}

fn manifest() -> Manifest {
    let path = wpt_dir().join("manifest.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e} (run tools/fetch-wpt.py)", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

fn expectations() -> BTreeMap<String, usize> {
    let path = wpt_dir().join("expectations.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

#[test]
fn manifest_expectations_and_files_agree() {
    let m = manifest();
    assert_eq!(m.commit.len(), 40, "manifest commit is not a full SHA");
    assert_eq!((m.viewport.width, m.viewport.height), (WPT_WIDTH, WPT_HEIGHT), "manifest viewport is not the WPT default");
    assert!(wpt_dir().join("LICENSE.md").exists(), "tests/wpt/LICENSE.md (the WPT licence) is missing");
    assert!(wpt_dir().join("README.md").exists(), "tests/wpt/README.md is missing");
    let expectations = expectations();
    let mut total = 0;
    for d in &m.directories {
        assert!(!d.pairs.is_empty(), "{}: no pairs", d.dir);
        assert!(expectations.contains_key(&d.dir), "{}: no entry in expectations.json", d.dir);
        assert!(expectations[&d.dir] <= d.pairs.len(), "{}: expectation above the pair count", d.dir);
        for p in &d.pairs {
            assert!(p.test.starts_with(&format!("{}/", d.dir)), "{}: {} is not in its directory", d.dir, p.test);
            assert!(matches!(p.kind.as_str(), "match" | "mismatch"), "{}: kind {}", p.test, p.kind);
            for f in [&p.test, &p.reference] {
                assert!(wpt_dir().join(f).exists(), "{}: missing file {f}", d.dir);
            }
        }
        total += d.pairs.len();
    }
    for dir in expectations.keys() {
        assert!(m.directories.iter().any(|d| &d.dir == dir), "expectations.json names `{dir}` but the manifest has no such directory");
    }
    assert!(total >= 1000, "expected at least a thousand pairs, found {total}");
}

// ---------------------------------------------------------------------------------
// Resolving a WPT document: linked stylesheets, @import, Ahem
// ---------------------------------------------------------------------------------

/// Resolves `href` against the document at `base` (a path relative to the corpus
/// root) the way the WPT server would: absolute paths from the root, relative paths
/// from the document's directory, query and fragment dropped.
fn resolve(base: &str, href: &str) -> Option<PathBuf> {
    let href = href.split(['#', '?']).next().unwrap_or("");
    if href.is_empty() || href.contains("://") || href.starts_with("data:") {
        return None;
    }
    let mut parts: Vec<&str> = if let Some(abs) = href.strip_prefix('/') {
        abs.split('/').collect()
    } else {
        let mut dir: Vec<&str> = base.split('/').collect();
        dir.pop();
        dir.extend(href.split('/'));
        dir
    };
    let mut out: Vec<&str> = Vec::new();
    for p in parts.drain(..) {
        match p {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            p => out.push(p),
        }
    }
    Some(wpt_dir().join(out.join("/")))
}

fn read_relative(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// Replaces every `@import` statement in `css` with the imported sheet's text (itself
/// resolved), so the cascade sees one flat sheet in source order. `depth` bounds
/// cycles.
fn inline_imports(css: &str, base: &Path, depth: usize) -> String {
    if depth > 8 {
        return css.to_string();
    }
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(at) = rest.find("@import") {
        out.push_str(&rest[..at]);
        let stmt_end = rest[at..].find(';').map(|i| at + i + 1).unwrap_or(rest.len());
        let stmt = &rest[at + "@import".len()..stmt_end.min(rest.len())];
        rest = &rest[stmt_end..];
        let target = stmt.trim().trim_end_matches(';').trim();
        let target = target.strip_prefix("url(").map(|t| t.trim_end_matches(')')).unwrap_or(target);
        let target = target.trim().trim_matches(|c| c == '"' || c == '\'');
        let target = target.split_whitespace().next().unwrap_or("").trim_matches(|c| c == '"' || c == '\'');
        let base_rel = base.strip_prefix(wpt_dir()).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        if let Some(path) = resolve(&base_rel, target) {
            if path.file_name().is_some_and(|n| n == "ahem.css") {
                continue;
            }
            if let Some(text) = read_relative(&path) {
                out.push_str(&inline_imports(&text, &path, depth + 1));
                out.push('\n');
            }
        }
    }
    out.push_str(rest);
    out
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

/// Rewrites the family name `Ahem` (bare or quoted, any case) to the bundled
/// monospace face the engine shapes it with. Both documents of a pair get the same
/// rewrite, so a pair that only depends on Ahem's square glyphs still compares equal.
pub fn map_ahem(text: &str) -> String {
    const REPLACEMENT: &str = "\"JetBrains Mono\"";
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        if i + 4 <= bytes.len() && bytes[i..i + 4].eq_ignore_ascii_case(b"ahem") {
            let before = text[..i].chars().next_back();
            let after = text[i + 4..].chars().next();
            let quoted_before = matches!(before, Some('"' | '\''));
            let quoted_after = matches!(after, Some('"' | '\''));
            let bare = !before.is_some_and(is_ident_char) && !after.is_some_and(is_ident_char);
            if quoted_before && quoted_after {
                out.pop();
                out.push_str(REPLACEMENT);
                i += 5;
                continue;
            }
            if bare {
                out.push_str(REPLACEMENT);
                i += 4;
                continue;
            }
        }
        let c = text[i..].chars().next().unwrap();
        out.push(c);
        i += c.len_utf8();
    }
    out
}

/// One rendered document of a pair, or why it could not be rendered.
#[cfg(feature = "pipeline")]
fn render(rel: &str) -> Result<cw_scene::Scene, String> {
    use cw_web::css::{parse_stylesheet, MatchContext, Media, Origin};
    use cw_web::dom::Document;
    use cw_web::{Strictness, Viewport};
    let path = wpt_dir().join(rel);
    let html = std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let html = map_ahem(&html);
    let viewport = Viewport { width: WPT_WIDTH, height: WPT_HEIGHT, scale: 1, zoom: 100 };
    let doc = cw_web::html::parse(&html);
    let mut sheets = Vec::new();
    for node in doc.descendants(Document::ROOT) {
        let css = if doc.is(node, "style") {
            Some(inline_imports(&doc.text_content(node), &path, 0))
        } else if doc.is(node, "link") {
            let rel_attr = doc.attr(node, "rel").unwrap_or("");
            let is_sheet = rel_attr.split_whitespace().any(|r| r.eq_ignore_ascii_case("stylesheet"));
            match (is_sheet, doc.attr(node, "href")) {
                (true, Some(href)) => match resolve(rel, href) {
                    Some(p) if p.file_name().is_some_and(|n| n == "ahem.css") => None,
                    Some(p) => read_relative(&p).map(|t| inline_imports(&map_ahem(&t), &p, 0)),
                    None => None,
                },
                _ => None,
            }
        } else {
            None
        };
        if let Some(css) = css {
            match parse_stylesheet(&css, Origin::Author, Strictness::Lenient) {
                Ok(sheet) => sheets.push(sheet),
                Err(e) => return Err(format!("stylesheet: {e}")),
            }
        }
    }
    let media = Media::with_size(viewport.width as i32, viewport.height as i32);
    let styles = cw_web::style::cascade(&doc, &sheets, &media, &MatchContext::new(), Strictness::Lenient).map_err(|e| format!("cascade: {e}"))?;
    let tree = cw_web::layout::layout(&doc, &styles, viewport);
    let scene = cw_web::paint::paint(&doc, &styles, &tree, viewport, &cw_web::paint::PaintContext::default());
    Ok(scene)
}

#[cfg(feature = "pipeline")]
fn render_caught(rel: &str) -> Result<cw_scene::Scene, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| render(rel))).unwrap_or_else(|payload| {
        let msg = payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_else(|| "panic".to_string());
        Err(format!("panic: {}", msg.lines().next().unwrap_or("")))
    })
}

#[cfg(feature = "pipeline")]
struct PairOutcome {
    test: String,
    kind: String,
    passed: bool,
    /// Differing pixels between the two rasters, or the error that stopped one side.
    detail: String,
}

#[cfg(feature = "pipeline")]
struct DirOutcome {
    dir: String,
    outcomes: Vec<PairOutcome>,
}

#[cfg(feature = "pipeline")]
impl DirOutcome {
    fn passed(&self) -> usize {
        self.outcomes.iter().filter(|o| o.passed).count()
    }
}

#[cfg(feature = "pipeline")]
fn run_pair(p: &Pair) -> PairOutcome {
    let a = render_caught(&p.test);
    let b = render_caught(&p.reference);
    let (passed, detail) = match (a, b) {
        (Ok(sa), Ok(sb)) => {
            // Identical scenes rasterise to identical pixels (the renderer is
            // deterministic), so the rasters are only made when the scenes differ.
            let differing = if content_digest(&sa) == content_digest(&sb) {
                0
            } else {
                let (fa, fb) = (rasterise(&sa), rasterise(&sb));
                fa.rgba.chunks(4).zip(fb.rgba.chunks(4)).filter(|(x, y)| x != y).count() + fa.rgba.len().abs_diff(fb.rgba.len()) / 4
            };
            let same = differing == 0;
            (if p.kind == "match" { same } else { !same }, format!("{differing} pixels differ"))
        }
        (Err(e), _) => (false, format!("test: {e}")),
        (_, Err(e)) => (false, format!("reference: {e}")),
    };
    PairOutcome { test: p.test.clone(), kind: p.kind.clone(), passed, detail }
}

#[cfg(feature = "pipeline")]
fn run_all(m: &Manifest) -> Vec<DirOutcome> {
    // The engine may still panic on some inputs; one panic is one failed pair, not a
    // failed run, and the default hook would print a backtrace line per pair.
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    // Pairs are independent, so they are spread over a few worker threads; the results
    // are put back in manifest order, so the report does not depend on scheduling.
    let flat: Vec<(usize, &Pair)> = m.directories.iter().enumerate().flat_map(|(di, d)| d.pairs.iter().map(move |p| (di, p))).collect();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: std::sync::Mutex<Vec<Option<PairOutcome>>> = std::sync::Mutex::new(flat.iter().map(|_| None).collect());
    let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).clamp(1, 4);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some((_, p)) = flat.get(i) else { break };
                let outcome = run_pair(p);
                results.lock().unwrap()[i] = Some(outcome);
            });
        }
    });
    let mut out: Vec<DirOutcome> = m.directories.iter().map(|d| DirOutcome { dir: d.dir.clone(), outcomes: Vec::new() }).collect();
    for ((di, _), outcome) in flat.iter().zip(results.into_inner().unwrap()) {
        out[*di].outcomes.push(outcome.expect("every pair ran"));
    }
    std::panic::set_hook(previous);
    out
}

#[cfg(feature = "pipeline")]
fn report(commit: &str, dirs: &[DirOutcome], expectations: &BTreeMap<String, usize>) -> String {
    let total: usize = dirs.iter().map(|d| d.outcomes.len()).sum();
    let passed: usize = dirs.iter().map(DirOutcome::passed).sum();
    let mut md = String::new();
    md.push_str("# WPT reftest report\n\n");
    md.push_str(&format!("web-platform-tests `{commit}`, {WPT_WIDTH}x{WPT_HEIGHT}, pixel comparison through `cw-render`.\n\n"));
    md.push_str(&format!("**{passed} / {total} pairs pass ({:.1}%).**\n\n", if total == 0 { 0.0 } else { passed as f64 / total as f64 * 100.0 }));
    md.push_str("| Directory | Pass | Pairs | Rate | Expected | Status |\n|---|---|---|---|---|---|\n");
    for d in dirs {
        let n = d.outcomes.len();
        let p = d.passed();
        let e = expectations.get(&d.dir).copied().unwrap_or(0);
        md.push_str(&format!(
            "| `{}` | {p} | {n} | {:.1}% | {e} | {} |\n",
            d.dir,
            if n == 0 { 0.0 } else { p as f64 / n as f64 * 100.0 },
            if p >= e { "ok" } else { "BELOW EXPECTATION" }
        ));
    }
    for d in dirs {
        md.push_str(&format!("\n## `{}`: {} / {}\n\n", d.dir, d.passed(), d.outcomes.len()));
        let mut errors: BTreeMap<String, usize> = BTreeMap::new();
        for o in &d.outcomes {
            if !o.passed && (o.detail.starts_with("test:") || o.detail.starts_with("reference:")) {
                let key = o.detail.split_once(": ").map(|(_, e)| e).unwrap_or(&o.detail);
                *errors.entry(key.to_string()).or_default() += 1;
            }
        }
        if !errors.is_empty() {
            md.push_str("Errors:\n\n");
            let mut errors: Vec<_> = errors.into_iter().collect();
            errors.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            for (e, n) in errors {
                md.push_str(&format!("- {n}× {e}\n"));
            }
            md.push('\n');
        }
        for o in &d.outcomes {
            let name = o.test.strip_prefix(&format!("{}/", d.dir)).unwrap_or(&o.test);
            md.push_str(&format!("- {} `{name}` ({}) — {}\n", if o.passed { "PASS" } else { "FAIL" }, o.kind, o.detail));
        }
    }
    md
}

#[test]
#[cfg_attr(not(feature = "wpt"), ignore = "the full WPT run takes about half an hour (`--features pipeline,wpt`)")]
fn wpt_reftests_meet_expectations() {
    #[cfg(feature = "pipeline")]
    {
        let m = manifest();
        let expectations = expectations();
        let dirs = run_all(&m);
        let md = report(&m.commit, &dirs, &expectations);
        let path = out_dir().join("wpt-report.md");
        std::fs::write(&path, md).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
        let total: usize = dirs.iter().map(|d| d.outcomes.len()).sum();
        let passed: usize = dirs.iter().map(DirOutcome::passed).sum();
        eprintln!("wpt: {passed}/{total} pairs pass; see {}", path.display());
        let mut failures = Vec::new();
        for d in &dirs {
            let p = d.passed();
            let e = expectations.get(&d.dir).copied().unwrap_or(0);
            eprintln!("  {}: {p}/{} (expected >= {e})", d.dir, d.outcomes.len());
            if p < e {
                failures.push(format!("{}: {p} passed, expected at least {e}", d.dir));
            }
        }
        assert!(failures.is_empty(), "directories below expectations.json:\n{}", failures.join("\n"));
    }
}

#[test]
fn ahem_is_mapped_to_the_bundled_monospace_face() {
    assert_eq!(map_ahem("font: 20px/1 Ahem;"), "font: 20px/1 \"JetBrains Mono\";");
    assert_eq!(map_ahem("font-family: 'Ahem', sans-serif"), "font-family: \"JetBrains Mono\", sans-serif");
    assert_eq!(map_ahem("font-family: \"ahem\""), "font-family: \"JetBrains Mono\"");
    assert_eq!(map_ahem("ahem-visible"), "ahem-visible");
    assert_eq!(map_ahem("aé\u{301}x Ahem"), "aé\u{301}x \"JetBrains Mono\"");
    assert_eq!(map_ahem("Ahem.ttf"), "\"JetBrains Mono\".ttf");
}

#[test]
fn imports_resolve_inside_the_tree() {
    let base = wpt_dir().join("css/css-flexbox/x.html");
    let css = inline_imports("@import url(\"/css/support/nonexistent.css\"); p { color: red }", &base, 0);
    assert_eq!(css.trim(), "p { color: red }");
    assert_eq!(resolve("css/css-flexbox/x.html", "../reference/ref.xht").unwrap(), wpt_dir().join("css/reference/ref.xht"));
    assert_eq!(resolve("css/css-flexbox/x.html", "/css/support/a.css?x#y").unwrap(), wpt_dir().join("css/support/a.css"));
    assert!(resolve("css/css-flexbox/x.html", "http://example.com/a.css").is_none());
}
