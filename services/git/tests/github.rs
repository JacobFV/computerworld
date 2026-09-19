//! The `github` skin end to end, and the promise the `plain` skin makes: `/repos/*` is frozen.
use cw_protocol::{HttpRequest, Page, PageElement};
use cw_sdk::{Service, ServiceContext};
use cw_service_git::GitService;
use serde_json::{json, Value};

fn ctx(actor: &str) -> ServiceContext {
    ServiceContext {
        actor: actor.into(),
        source: format!("{actor}-pc"),
        tick: 40,
        seed: 5,
        instance: "github".into(),
    }
}
fn get(state: &mut Value, actor: &str, path: &str) -> (u16, String) {
    let response = GitService
        .handle(
            state,
            &ctx(actor),
            &HttpRequest::get(format!("http://github.com{path}")),
        )
        .unwrap();
    (response.status, String::from_utf8(response.body).unwrap())
}
fn post(state: &mut Value, actor: &str, path: &str, body: Value) -> (u16, String) {
    let response = GitService
        .handle(
            state,
            &ctx(actor),
            &HttpRequest::json("POST", format!("http://github.com{path}"), &body).unwrap(),
        )
        .unwrap();
    (response.status, String::from_utf8(response.body).unwrap())
}
fn github() -> Value {
    GitService
        .initialize(
            json!({"skin":"github","repositories":{"atlas":{"owner":"northstar",
                "description":"Deterministic simulation runtime.","topics":["rust","determinism"],
                "stars":["bmartinez","praman"],"forks":37,"next_number":15,
                "files":{"README.md":"# Atlas\nRelease code lives in the launch doc.\n"},
                "writers":["alicechen","bmartinez","northstar-ops"],
                "issues":{"14":{"number":14,"title":"BFS path test fails on Windows only",
                    "body":"Hash-map iteration order.","state":"open","author":"bmartinez",
                    "labels":["bug"],"tick":30}}}},
                "gists":{"bfs-order":{"owner":"praman","description":"Stable BFS ordering",
                    "files":{"order.rs":"// sort before you iterate\n"},"tick":31}}}),
            &ctx("alicechen"),
        )
        .unwrap()
}

/// §5.1: `skin: "plain"` plus `/repos/*` is a frozen byte contract — `behaviors.rs` clones it.
#[test]
fn plain_repos_output_is_byte_identical() {
    const STATE: &str = r##"{"repositories":{"onboarding":{"objects":{"2cea0d22873f219e1468c9496a3504dc9604088456bed23168d92c7e0b1492db":{"author":"system","files":{"README.md":"# Atlas onboarding\nUse http://intranet.internal/ to find the launch checklist.\n","checklist.txt":"mail read: pending\ndocument updated: pending\n"},"message":"Initial commit","parents":[],"tick":0}},"readers":["alice","bob","carol"],"refs":{"refs/heads/main":"2cea0d22873f219e1468c9496a3504dc9604088456bed23168d92c7e0b1492db"},"writers":["alice","bob","carol"]}}}"##;
    const PAGE: &str = r##"{"version":1,"title":"Repository onboarding","elements":[{"kind":"heading","id":"repo-title","text":"onboarding","level":1},{"kind":"text","id":"ref-refs/heads/main","text":"refs/heads/main 2cea0d22873f219e1468c9496a3504dc9604088456bed23168d92c7e0b1492db"},{"kind":"text","id":"file-README.md","text":"README.md\n# Atlas onboarding\nUse http://intranet.internal/ to find the launch checklist.\n"},{"kind":"text","id":"file-checklist.txt","text":"checklist.txt\nmail read: pending\ndocument updated: pending\n"}]}"##;
    const HOME: &str = r##"{"version":1,"title":"Git repositories","elements":[{"kind":"link","id":"repo-onboarding","text":"onboarding","url":"/repos/onboarding"}]}"##;
    let mut state = GitService
        .initialize(
            json!({"repositories":{"onboarding":{"files":{
                "README.md":"# Atlas onboarding\nUse http://intranet.internal/ to find the launch checklist.\n",
                "checklist.txt":"mail read: pending\ndocument updated: pending\n"},
                "readers":["alice","bob","carol"],"writers":["alice","bob","carol"]}}}),
            &ctx("alice"),
        )
        .unwrap();
    assert_eq!(serde_json::to_string(&state).unwrap(), STATE);
    assert_eq!(get(&mut state, "alice", "/repos/onboarding").1, PAGE);
    assert_eq!(get(&mut state, "alice", "/").1, HOME);
    // The clone route the behaviour test drives.
    let api = get(&mut state, "bob", "/api/git/repos/onboarding").1;
    let value: Value = serde_json::from_str(&api).unwrap();
    assert!(value["objects"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap()["files"]["checklist.txt"]
        .as_str()
        .unwrap()
        .contains("mail read: pending"));
    // The frozen route stays frozen on a skinned instance too: no theme, no chrome.
    let mut skinned = github();
    let legacy = get(&mut skinned, "alicechen", "/repos/atlas").1;
    assert!(legacy.starts_with(r#"{"version":1,"title":"Repository atlas""#));
    assert!(!legacy.contains("theme") && !legacy.contains("chrome"));
}

#[test]
fn issue_opens_comments_and_closes() {
    let mut state = github();
    let (status, page) = post(
        &mut state,
        "praman",
        "/northstar/atlas/issues",
        json!({"title":"Document the seed streams","body":"They are order-independent."}),
    );
    assert_eq!(status, 200);
    assert!(page.contains("Document the seed streams") && page.contains("Open"));
    assert_eq!(state["repositories"]["atlas"]["next_number"], 16);
    // Anyone who can read may comment; only a writer may close.
    assert_eq!(
        post(
            &mut state,
            "praman",
            "/northstar/atlas/issues/15/comments",
            json!({"body":"Worth a paragraph in the README."})
        )
        .0,
        200
    );
    assert_eq!(
        post(
            &mut state,
            "praman",
            "/northstar/atlas/issues/15/state",
            json!({"state":"closed"})
        )
        .0,
        403
    );
    let (status, page) = post(
        &mut state,
        "alicechen",
        "/northstar/atlas/issues/15/state",
        json!({"state":"closed"}),
    );
    assert_eq!(status, 200);
    assert!(page.contains("Closed") && page.contains("Worth a paragraph"));
    assert_eq!(
        state["repositories"]["atlas"]["issues"]["15"]["state"],
        "closed"
    );
    // An empty title is refused and changes nothing.
    let before = state.clone();
    assert_eq!(
        post(
            &mut state,
            "praman",
            "/northstar/atlas/issues",
            json!({"title":"  "})
        )
        .0,
        422
    );
    assert_eq!(before, state);
}

#[test]
fn pull_request_opens_reviews_and_merges_the_ref() {
    let mut state = github();
    let head = "refs/heads/sort-before-iterate";
    let main = state["repositories"]["atlas"]["refs"]["refs/heads/main"]
        .as_str()
        .unwrap()
        .to_owned();
    state["repositories"]["atlas"]["refs"][head] = json!(main);
    assert_eq!(
        post(
            &mut state,
            "bmartinez",
            "/northstar/atlas/pulls",
            json!({"title":"Sort before you iterate","head":"refs/heads/missing","base":"refs/heads/main"})
        )
        .0,
        422
    );
    let (status, page) = post(
        &mut state,
        "bmartinez",
        "/northstar/atlas/pulls",
        json!({"title":"Sort before you iterate","head":head,"base":"refs/heads/main"}),
    );
    assert_eq!(status, 200);
    assert!(page.contains("Sort before you iterate"));
    assert_eq!(
        post(
            &mut state,
            "alicechen",
            "/northstar/atlas/pull/15/reviews",
            json!({"decision":"approve","body":"Ship it."})
        )
        .0,
        200
    );
    let (status, page) = post(
        &mut state,
        "alicechen",
        "/northstar/atlas/pull/15/merge",
        json!({}),
    );
    assert_eq!(status, 200);
    assert!(page.contains("Merged"));
    // Merging really moves the base ref, so a later clone sees the change.
    assert_eq!(
        state["repositories"]["atlas"]["refs"]["refs/heads/main"],
        state["repositories"]["atlas"]["refs"][head]
    );
    assert_eq!(
        post(
            &mut state,
            "alicechen",
            "/northstar/atlas/pull/15/merge",
            json!({})
        )
        .0,
        409
    );
}

#[test]
fn starring_toggles_and_survives_a_round_trip() {
    let mut state = github();
    let (status, page) = post(&mut state, "alice", "/northstar/atlas/star", json!({}));
    assert_eq!(status, 200);
    assert!(page.contains("3 people starred"));
    let restored: Value = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
    assert_eq!(restored, state);
    let (_, page) = post(&mut state, "alice", "/northstar/atlas/star", json!({}));
    assert!(page.contains("2 people starred"));
}

#[test]
fn owner_repo_blob_and_gist_pages_resolve() {
    let mut state = github();
    assert!(get(&mut state, "alice", "/").1.contains("northstar/atlas"));
    assert!(get(&mut state, "alice", "/northstar").1.contains("atlas"));
    let repo = get(&mut state, "alice", "/northstar/atlas").1;
    assert!(repo.contains("determinism") && repo.contains("\"Issues\""));
    // The old `/blob/<path>` shape still resolves on the default branch.
    assert!(get(&mut state, "alice", "/northstar/atlas/blob/README.md")
        .1
        .contains("Release code lives in the launch doc"));
    assert!(get(&mut state, "alice", "/gist/bfs-order")
        .1
        .contains("sort before you iterate"));
    assert_eq!(get(&mut state, "alice", "/northstar/ghost").0, 404);
    assert_eq!(get(&mut state, "alice", "/nobody").0, 404);
    // The JSON mirror an agent scripts against.
    let issues = get(&mut state, "alice", "/api/northstar/atlas/issues").1;
    assert!(issues.contains("BFS path test fails on Windows only"));
}

fn page(state: &mut Value, path: &str) -> Page {
    let (status, body) = get(state, "alicechen", path);
    assert_eq!(status, 200, "{path}: {body}");
    let page: Page = serde_json::from_str(&body).unwrap();
    let mut seen = std::collections::BTreeSet::new();
    let dupes: Vec<&str> = flatten(&page.elements)
        .iter()
        .map(|e| e.id())
        .filter(|id| !seen.insert(*id))
        .collect();
    assert!(dupes.is_empty(), "{path}: duplicate ids {dupes:?}");
    page.validate().unwrap_or_else(|e| panic!("{path}: {e}"));
    page
}
/// Every element of a page, depth first.
fn flatten(elements: &[PageElement]) -> Vec<&PageElement> {
    let mut out = vec![];
    for e in elements {
        out.push(e);
        match e {
            PageElement::Row { children, .. }
            | PageElement::Grid { children, .. }
            | PageElement::Card { children, .. }
            | PageElement::Group { children, .. }
            | PageElement::Form { children, .. } => out.extend(flatten(children)),
            _ => (),
        }
    }
    out
}
fn seeded() -> Value {
    let raw = std::fs::read_to_string("../../worlds/company-2026/sites/github.json").unwrap();
    let site: Value = serde_json::from_str(&raw).unwrap();
    GitService
        .initialize(site["initial_state"].clone(), &ctx("alicechen"))
        .unwrap()
}

/// The repository home has GitHub's chrome: a pinned dark header, the tab strip with the
/// Code tab underlined, a branch selector, the latest-commit bar, folder icons, a README
/// and the About column with a languages bar.
#[test]
fn repository_home_has_tabs_branch_selector_files_and_about() {
    let mut state = seeded();
    let page = page(&mut state, "/northstar/atlas");
    let all = flatten(&page.elements);
    let PageElement::Row { style, .. } = &page.elements[0] else {
        panic!("header first")
    };
    assert_eq!(style.pin.as_deref(), Some("top"));
    let text = |id: &str| {
        all.iter()
            .find_map(|e| match e {
                PageElement::Styled { id: i, text, .. } | PageElement::Link { id: i, text, .. }
                    if i == id =>
                {
                    Some(text.clone())
                }
                PageElement::Badge { id: i, text, .. } if i == id => Some(text.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no element {id}"))
    };
    assert_eq!(text("tab-issues-link"), "Issues");
    assert_eq!(text("tab-issues-count"), "6");
    assert_eq!(text("tab-pulls-count"), "2");
    assert_eq!(text("branch-name"), "main");
    assert_eq!(text("branches"), "4 Branches");
    assert_eq!(text("history"), "7 Commits");
    assert_eq!(
        text("latest-message"),
        "Expand the README with a quick start and pin serde"
    );
    assert!(text("latest-when").contains("ago"));
    // The Code tab carries the orange underline; the Issues tab does not.
    let bar = |id: &str| {
        all.iter().find_map(|e| match e {
            PageElement::Thumbnail { id: i, style, .. } if i == id => style.background.clone(),
            _ => None,
        })
    };
    assert_eq!(bar("tab-code-bar").as_deref(), Some("#fd8c73"));
    assert_ne!(bar("tab-issues-bar").as_deref(), Some("#fd8c73"));
    // Folders first, with the folder glyph, then files; each row names its last commit.
    let icons: Vec<&str> = all
        .iter()
        .filter_map(|e| match e {
            PageElement::Icon { id, name, .. } if id.starts_with("entry-icon-") => {
                Some(name.as_str())
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        icons,
        vec!["folder", "folder", "folder", "file", "file", "file", "file"]
    );
    assert_eq!(text("file-0"), ".github");
    assert_eq!(
        text("entry-message-0"),
        "Run the test matrix on Windows too"
    );
    assert_eq!(
        text("entry-message-3"),
        "Merge pull request #11 from northstar/contributing"
    );
    assert_eq!(
        text("entry-message-6"),
        "Expand the README with a quick start and pin serde"
    );
    // README rendered as headings and a mono code block; About lists the languages.
    assert!(all.iter().any(|e| matches!(e, PageElement::Styled { id, text, .. } if id.starts_with("readme-h-") && text == "Quick start")));
    assert!(all.iter().any(|e| matches!(e, PageElement::Styled { id, style, .. } if id.starts_with("readme-code-") && style.mono == Some(true))));
    assert_eq!(text("language-name-0"), "Rust");
    assert_eq!(text("about-license-link"), "MIT license");
    assert_eq!(text("about-stars-link"), "7 stars");
}

#[test]
fn commits_commit_tree_blob_and_branches_pages_render_history() {
    let mut state = seeded();
    let commits = page(&mut state, "/northstar/atlas/commits/main");
    let all = flatten(&commits.elements);
    let titles: Vec<&str> = all
        .iter()
        .filter_map(|e| match e {
            PageElement::Link { id, text, .. }
                if id.starts_with("commits-") && id.ends_with("-title") =>
            {
                Some(text.as_str())
            }
            _ => None,
        })
        .collect();
    assert_eq!(titles.len(), 7);
    assert_eq!(
        titles[0],
        "Expand the README with a quick start and pin serde"
    );
    assert_eq!(titles[6], "Initial commit");
    assert!(all.iter().any(
        |e| matches!(e, PageElement::Styled { text, .. } if text.starts_with("Commits on Aug"))
    ));
    let sha = all
        .iter()
        .find_map(|e| match e {
            PageElement::Link {
                id,
                url,
                style: Some(style),
                ..
            } if id == "commits-0-sha" => {
                assert_eq!(style.mono, Some(true));
                Some(url.clone())
            }
            _ => None,
        })
        .unwrap();
    // A commit page shows the message, the author, the stats and a coloured unified diff.
    let commit = page(&mut state, &sha);
    let all = flatten(&commit.elements);
    let json = serde_json::to_string(&commit).unwrap();
    assert!(json.contains("Showing 2 changed files with"));
    assert!(all.iter().any(|e| matches!(e, PageElement::Styled { id, text, style, .. } if id.starts_with("diff-file-0-hunk-") && text.starts_with("@@ -") && style.mono == Some(true))));
    assert!(all.iter().any(|e| matches!(e, PageElement::Styled { style, .. } if style.background.as_deref() == Some("#dafbe1"))));
    assert!(all.iter().any(|e| matches!(e, PageElement::Link { id, text, .. } if id == "diff-file-0-path" && text == "Cargo.toml")));
    // A short SHA resolves too.
    assert_eq!(get(&mut state, "alice", &sha[..sha.len() - 30]).0, 200);
    // Tree pages list a folder; blob pages number their lines in mono.
    let tree = page(&mut state, "/northstar/atlas/tree/main/src");
    let json = serde_json::to_string(&tree).unwrap();
    assert!(json.contains("bfs.rs") && json.contains("clock.rs") && !json.contains("README\""));
    let blob = page(&mut state, "/northstar/atlas/blob/main/src/bfs.rs");
    let all = flatten(&blob.elements);
    let numbers = all
        .iter()
        .find_map(|e| match e {
            PageElement::Styled {
                id, text, style, ..
            } if id == "line-numbers" => {
                assert_eq!(style.mono, Some(true));
                Some(text.clone())
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(numbers.lines().count(), 19);
    assert!(all.iter().any(|e| matches!(e, PageElement::Styled { id, style, text, .. } if id == "blob-text" && style.mono == Some(true) && text.contains("use std::collections::HashMap;"))));
    assert!(all
        .iter()
        .any(|e| matches!(e, PageElement::Link { id, text, .. } if id == "raw" && text == "Raw")));
    assert!(all.iter().any(
        |e| matches!(e, PageElement::Link { id, text, .. } if id == "blame" && text == "Blame")
    ));
    assert!(serde_json::to_string(&blob)
        .unwrap()
        .contains("19 lines (18 loc)"));
    // A branch on another ref shows that ref's tree.
    let feature = page(&mut state, "/northstar/atlas/blob/sort-refs/src/bfs.rs");
    assert!(serde_json::to_string(&feature)
        .unwrap()
        .contains("BTreeMap"));
    let branches = page(&mut state, "/northstar/atlas/branches");
    let json = serde_json::to_string(&branches).unwrap();
    assert!(
        json.contains("sort-refs")
            && json.contains("Default")
            && json.contains("0 behind · 2 ahead")
    );
}

#[test]
fn issues_and_pull_requests_look_like_githubs() {
    let mut state = seeded();
    let issues = page(&mut state, "/northstar/atlas/issues");
    let all = flatten(&issues.elements);
    let state_icons: Vec<(&str, &str)> = all
        .iter()
        .filter_map(|e| match e {
            PageElement::Icon {
                id, name, style, ..
            } if id.starts_with("state-") => {
                Some((name.as_str(), style.color.as_deref().unwrap_or("")))
            }
            _ => None,
        })
        .collect();
    assert_eq!(state_icons.len(), 6);
    assert!(state_icons
        .iter()
        .all(|(n, c)| *n == "issue-open" && *c == "#1f883d"));
    assert!(all.iter().any(|e| matches!(e, PageElement::Badge { text, style, .. } if text == "bug" && style.background.as_deref() == Some("#d73a4a"))));
    assert!(all.iter().any(|e| matches!(e, PageElement::Badge { text, style, .. } if text == "good first issue" && style.background.as_deref() == Some("#7057ff"))));
    assert!(all.iter().any(|e| matches!(e, PageElement::Styled { id, text, .. } if id == "meta-14" && text.starts_with("#14 opened") && text.ends_with("by bmartinez"))));
    assert!(all.iter().any(
        |e| matches!(e, PageElement::Styled { id, text, .. } if id == "comments-14" && text == "3")
    ));
    assert!(all.iter().any(
        |e| matches!(e, PageElement::Link { id, text, .. } if id == "list-open" && text == "6 Open")
    ));
    let closed = page(&mut state, "/northstar/atlas/issues?state=closed");
    let all = flatten(&closed.elements);
    assert!(all.iter().any(|e| matches!(e, PageElement::Icon { id, name, style, .. } if id == "state-9" && name == "issue-closed" && style.color.as_deref() == Some("#8250df"))));
    assert!(!all
        .iter()
        .any(|e| matches!(e, PageElement::Icon { id, .. } if id == "state-14")));
    // An issue: state pill, timeline of comment cards, sidebar, composer.
    let issue = page(&mut state, "/northstar/atlas/issues/14");
    let all = flatten(&issue.elements);
    assert!(all.iter().any(|e| matches!(e, PageElement::Styled { id, text, .. } if id == "state-text" && text == "Open")));
    assert_eq!(
        all.iter()
            .filter(|e| matches!(e, PageElement::Card { id, .. } if id.starts_with("comment-")))
            .count(),
        3
    );
    assert!(all.iter().any(|e| matches!(e, PageElement::Link { id, text, .. } if id == "side-assignee-name" && text == "praman")));
    assert!(all.iter().any(|e| matches!(e, PageElement::Link { id, text, .. } if id == "side-dev-link-1" && text.starts_with("#15 "))));
    assert!(all
        .iter()
        .any(|e| matches!(e, PageElement::Input { id, .. } if id == "comment-body")));
    assert!(all.iter().any(|e| matches!(e, PageElement::Button { id, text, .. } if id == "close" && text == "Close issue")));
    // Pull requests: tabs, reviews, merge box, files with a real diff.
    let pull = page(&mut state, "/northstar/atlas/pull/15");
    let json = serde_json::to_string(&pull).unwrap();
    assert!(json.contains("wants to merge 2 commits into"));
    let all = flatten(&pull.elements);
    assert!(all.iter().any(|e| matches!(e, PageElement::Icon { id, name, style, .. } if id == "review-state-0" && name == "x-circle" && style.color.as_deref() == Some("#cf222e"))));
    assert!(all.iter().any(|e| matches!(e, PageElement::Icon { id, name, style, .. } if id == "review-state-1" && name == "check" && style.color.as_deref() == Some("#1f883d"))));
    assert!(all.iter().any(|e| matches!(e, PageElement::Button { id, text, style: Some(style), .. } if id == "merge" && text == "Merge pull request" && style.background.as_deref() == Some("#1f883d"))));
    assert!(all.iter().any(|e| matches!(e, PageElement::Badge { id, text, .. } if id == "ptab-files-count" && text == "2")));
    let files = page(&mut state, "/northstar/atlas/pull/15/files");
    let json = serde_json::to_string(&files).unwrap();
    assert!(
        json.contains("Showing 2 changed files with")
            && json.contains("#dafbe1")
            && json.contains("#ffebe9")
            && json.contains("@@ -1,8 +1,8 @@")
    );
    let commits = page(&mut state, "/northstar/atlas/pull/15/commits");
    assert!(serde_json::to_string(&commits)
        .unwrap()
        .contains("Pin the neighbour order in the path test"));
    let merged = page(&mut state, "/northstar/atlas/pull/11");
    let all = flatten(&merged.elements);
    assert!(all.iter().any(|e| matches!(e, PageElement::Styled { id, text, .. } if id == "state-text" && text == "Merged")));
    assert!(all.iter().any(|e| matches!(e, PageElement::Styled { id, text, .. } if id == "merged-status-title" && text.starts_with("Pull request successfully merged"))));
    let draft = page(&mut state, "/northstar/atlas/pull/19");
    let all = flatten(&draft.elements);
    assert!(all.iter().any(|e| matches!(e, PageElement::Styled { id, text, .. } if id == "state-text" && text == "Draft")));
    assert!(!all
        .iter()
        .any(|e| matches!(e, PageElement::Button { id, .. } if id == "merge")));
    // Owner and stargazers.
    let owner = page(&mut state, "/northstar");
    let json = serde_json::to_string(&owner).unwrap();
    assert!(
        json.contains("contributions in the last year") && json.contains("Popular repositories")
    );
    assert!(flatten(&owner.elements).iter().any(|e| matches!(e, PageElement::Thumbnail { id, style, .. } if id.starts_with("cell-") && style.background.as_deref() == Some("#9be9a8"))));
    assert!(
        serde_json::to_string(&page(&mut state, "/northstar/atlas/stargazers"))
            .unwrap()
            .contains("7 people starred")
    );
}
