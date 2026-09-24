//! A page that declares a compiled TSX app (`data-cw-ui`) runs on `cw_ui`, not on a
//! JS realm; without its IR the same page runs its React fallback. Both must show
//! the same thing through the same browser actions, and the compiled page must
//! snapshot and restore like any other.
use cw_browser::BrowserState;
use cw_protocol::{HttpRequest, HttpResponse, Result};
use cw_scene::Scene;
use std::collections::BTreeMap;

const ORIGIN: &str = "https://tasks.test";
const W: u32 = 1280;
const H: u32 = 800;

struct Site {
    files: BTreeMap<String, (String, String)>,
    requests: Vec<String>,
}

impl Site {
    fn serve(&mut self, r: HttpRequest) -> Result<HttpResponse> {
        let url = url::Url::parse(&r.url).unwrap();
        self.requests.push(url.path().to_owned());
        match self.files.get(url.path()) {
            Some((kind, body)) => {
                let mut resp = HttpResponse::text(200, body.clone());
                resp.headers.insert("content-type".into(), kind.clone());
                Ok(resp)
            }
            None => Ok(HttpResponse::text(404, "not found")),
        }
    }
}

fn read(rel: &str) -> String {
    let p = format!("{}/../engine/tests/{rel}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{p}: {e}"))
}

/// The tracker fixture as a site; `with_ir` serves its compiled IR too.
fn site(with_ir: bool) -> Site {
    let mut files = BTreeMap::new();
    let mut add = |path: &str, kind: &str, body: String| {
        files.insert(path.to_owned(), (kind.to_owned(), body));
    };
    add("/", "text/html", read("framework-parity/tsx-tasks.html"));
    add(
        "/tsx-tasks.js",
        "text/javascript",
        read("framework-parity/tsx-tasks.js"),
    );
    for v in [
        "react-18.3.1.production.min.js",
        "react-dom-18.3.1.production.min.js",
    ] {
        add(
            &format!("/vendor/{v}"),
            "text/javascript",
            read(&format!("vendor/{v}")),
        );
    }
    if with_ir {
        add(
            "/tsx-tasks.ui.json",
            "application/json",
            read("framework-parity/tsx-tasks.ui.json"),
        );
    }
    Site {
        files,
        requests: Vec::new(),
    }
}

fn texts(scene: &Scene) -> Vec<String> {
    scene
        .nodes
        .iter()
        .filter_map(|n| serde_json::to_value(&n.primitive).ok())
        .filter_map(|v| v.get("text")?.as_str().map(str::to_owned))
        .collect()
}

fn browser() -> BrowserState {
    let mut b = BrowserState::default();
    b.set_entropy(42, "computer/test/browser");
    b.set_viewport(W, H);
    b
}

macro_rules! http {
    ($site:expr) => {
        &mut |r: HttpRequest| $site.serve(r)
    };
}

fn compiled(b: &BrowserState) -> bool {
    b.document()
        .and_then(|w| w.scripted())
        .is_some_and(|s| s.is_compiled())
}

/// The fixture's `added` steps, then a filter and the board, as browser actions.
fn drive(b: &mut BrowserState, site: &mut Site) -> Vec<Vec<String>> {
    let mut seen = vec![texts(&b.scene(W, H))];
    b.click("new-task", http!(site)).unwrap();
    b.fill_with("new-task", "Write the release notes", http!(site))
        .unwrap();
    b.key("Enter", http!(site)).unwrap();
    seen.push(texts(&b.scene(W, H)));
    b.click("check-2", http!(site)).unwrap();
    b.click("filter-open", http!(site)).unwrap();
    seen.push(texts(&b.scene(W, H)));
    b.click("tab-board", http!(site)).unwrap();
    seen.push(texts(&b.scene(W, H)));
    seen
}

#[test]
fn a_page_with_compiled_ir_runs_it_and_matches_its_react_fallback() {
    let mut fast_site = site(true);
    let mut fast = browser();
    fast.navigate(&format!("{ORIGIN}/"), http!(fast_site))
        .unwrap();
    assert!(compiled(&fast), "the IR was declared and served");
    assert!(
        !fast_site.requests.iter().any(|p| p.starts_with("/vendor/")),
        "a compiled page runs none of its scripts: {:?}",
        fast_site.requests
    );
    let mut slow_site = site(false);
    let mut slow = browser();
    slow.navigate(&format!("{ORIGIN}/"), http!(slow_site))
        .unwrap();
    assert!(
        !compiled(&slow),
        "no IR: the page runs on its React fallback"
    );
    assert!(slow_site.requests.iter().any(|p| p == "/tsx-tasks.ui.json"));
    let a = drive(&mut fast, &mut fast_site);
    let b = drive(&mut slow, &mut slow_site);
    assert!(a[0].iter().any(|t| t == "Tracker"), "{:?}", a[0]);
    assert!(
        a[1].iter().any(|t| t.contains("Write the release notes")),
        "{:?}",
        a[1]
    );
    for (i, (x, y)) in a.iter().zip(&b).enumerate() {
        assert_eq!(
            x, y,
            "step {i}: compiled and React pages show different text"
        );
    }
    assert!(fast.console().is_empty(), "{:?}", fast.console());
}

#[test]
fn a_compiled_page_snapshots_and_restores_without_replay() {
    let mut s = site(true);
    let mut b = browser();
    b.navigate(&format!("{ORIGIN}/"), http!(s)).unwrap();
    b.click("check-2", http!(s)).unwrap();
    let state = b.script_ui_state().expect("a compiled app runs the page");
    assert_eq!(b.document().unwrap().scripted().unwrap().journal_len(), 0);
    let json = serde_json::to_string(&b).unwrap();
    let mut restored: BrowserState = serde_json::from_str(&json).unwrap();
    assert!(compiled(&restored));
    assert_eq!(restored.script_ui_state().unwrap(), state);
    assert_eq!(texts(&restored.scene(W, H)), texts(&b.scene(W, H)));
    // Both carry on identically.
    b.click("tab-board", http!(s)).unwrap();
    restored.click("tab-board", http!(s)).unwrap();
    assert_eq!(texts(&restored.scene(W, H)), texts(&b.scene(W, H)));
    // A clone left behind keeps its own state while the live one moves on.
    let kept = b.clone();
    b.click("tab-list", http!(s)).unwrap();
    assert_ne!(texts(&kept.scene(W, H)), texts(&b.scene(W, H)));
}

#[test]
fn an_ir_that_does_not_parse_falls_back_to_react() {
    let mut s = site(true);
    s.files.insert(
        "/tsx-tasks.ui.json".into(),
        ("application/json".into(), "{\"version\": 999}".into()),
    );
    let mut b = browser();
    b.navigate(&format!("{ORIGIN}/"), http!(s)).unwrap();
    assert!(!compiled(&b));
    assert!(texts(&b.scene(W, H)).iter().any(|t| t == "Tracker"));
}
