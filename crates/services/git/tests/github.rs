//! The `github` and `gitlab` looks end to end as HTML, and the promise the original surface
//! makes: `/repos/*` state and API are frozen, and its two pages are plain HTML.
mod support;
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_git::GitService;
use serde_json::{json, Value};
use support::Page;

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
/// What the browser sends when a form is submitted: urlencoded fields.
fn submit(state: &mut Value, actor: &str, path: &str, fields: &[(&str, &str)]) -> (u16, String) {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(fields.iter().copied())
        .finish();
    let mut request = HttpRequest::get(format!("http://github.com{path}"));
    request.method = "POST".into();
    request.headers.insert(
        "content-type".into(),
        "application/x-www-form-urlencoded".into(),
    );
    request.body = body.into_bytes();
    let response = GitService.handle(state, &ctx(actor), &request).unwrap();
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

/// §5.1: plain state and the `/api/git/repos/*` clone route are a frozen contract that
/// `behaviors.rs` clones through. The two pages of that surface are HTML now, with the ids
/// they always had.
#[test]
fn plain_state_and_api_are_frozen_and_its_pages_are_html() {
    const STATE: &str = r##"{"repositories":{"onboarding":{"objects":{"2cea0d22873f219e1468c9496a3504dc9604088456bed23168d92c7e0b1492db":{"author":"system","files":{"README.md":"# Atlas onboarding\nUse http://intranet.internal/ to find the launch checklist.\n","checklist.txt":"mail read: pending\ndocument updated: pending\n"},"message":"Initial commit","parents":[],"tick":0}},"readers":["alice","bob","carol"],"refs":{"refs/heads/main":"2cea0d22873f219e1468c9496a3504dc9604088456bed23168d92c7e0b1492db"},"writers":["alice","bob","carol"]}}}"##;
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
    let (status, body) = get(&mut state, "alice", "/repos/onboarding");
    assert_eq!(status, 200);
    let page = Page::parse("/repos/onboarding", body);
    assert_eq!(page.title(), "Repository onboarding");
    assert_eq!(page.text("repo-title"), "onboarding");
    assert_eq!(
        page.text("ref-refs/heads/main"),
        "refs/heads/main 2cea0d22873f219e1468c9496a3504dc9604088456bed23168d92c7e0b1492db"
    );
    assert!(page
        .raw_text("file-README.md")
        .contains("# Atlas onboarding\nUse http://intranet.internal/"));
    assert!(page
        .raw_text("file-checklist.txt")
        .starts_with("checklist.txt"));
    assert!(page
        .raw_text("file-checklist.txt")
        .contains("mail read: pending\ndocument updated: pending\n"));
    let home = Page::parse("/", get(&mut state, "alice", "/").1);
    assert_eq!(home.title(), "Git repositories");
    assert_eq!(home.tag("repo-onboarding"), "a");
    assert_eq!(home.attr("repo-onboarding", "href"), "/repos/onboarding");
    assert_eq!(home.text("repo-onboarding"), "onboarding");
    // A reader who is not on the list sees no row at all.
    assert!(!Page::parse("/", get(&mut state, "mallory", "/").1).has("repo-onboarding"));
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
    assert_eq!(
        get(&mut state, "bob", "/api/git/repos").1,
        r#"["onboarding"]"#
    );
    // The legacy route is the same plain page on a skinned instance: no theme, no chrome.
    let mut skinned = github();
    let legacy = Page::parse(
        "/repos/atlas",
        get(&mut skinned, "alicechen", "/repos/atlas").1,
    );
    assert_eq!(legacy.title(), "Repository atlas");
    assert!(!legacy.has("chrome") && !legacy.has("repo-tabs"));
}

#[test]
fn issue_opens_comments_and_closes() {
    let mut state = github();
    let (status, body) = post(
        &mut state,
        "praman",
        "/northstar/atlas/issues",
        json!({"title":"Document the seed streams","body":"They are order-independent."}),
    );
    assert_eq!(status, 200);
    let page = Page::parse("new issue", body);
    assert_eq!(page.text("thread-title"), "Document the seed streams");
    assert_eq!(page.text("state-text"), "Open");
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
    let (status, body) = post(
        &mut state,
        "alicechen",
        "/northstar/atlas/issues/15/state",
        json!({"state":"closed"}),
    );
    assert_eq!(status, 200);
    let page = Page::parse("closed issue", body);
    assert_eq!(page.text("state-text"), "Closed");
    assert_eq!(
        page.text("comment-0-text"),
        "Worth a paragraph in the README."
    );
    assert!(page.has("reopen") && !page.has("close"));
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

/// The forms on the pages post what the routes read: the same flow as above, driven with
/// the urlencoded bodies a browser sends from `new-issue`, `comment` and the `close` button.
#[test]
fn the_pages_forms_drive_the_same_routes() {
    let mut state = github();
    let new = Page::parse(
        "/issues/new",
        get(&mut state, "alicechen", "/northstar/atlas/issues/new").1,
    );
    let (action, method, fields) = new.form("new-issue");
    assert_eq!(
        (action.as_str(), method.as_str()),
        ("/northstar/atlas/issues", "post")
    );
    assert_eq!(
        fields.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
        vec!["title", "body"]
    );
    assert_eq!(new.attr("new-issue-title", "name"), "title");
    assert_eq!(new.tag("new-issue-body"), "textarea");
    assert_eq!(new.form_of("new-issue-submit"), "new-issue");
    let (status, body) = submit(
        &mut state,
        "alicechen",
        &action,
        &[
            ("title", "From the form"),
            ("body", "See http://example.com/x."),
        ],
    );
    assert_eq!(status, 200);
    let issue = Page::parse("issue", body);
    assert_eq!(issue.text("thread-title"), "From the form");
    // The URL in the body is a link, with the id the Page version gave it.
    assert_eq!(
        issue.attr("thread-body-text-link-1", "href"),
        "http://example.com/x"
    );
    let (action, method, fields) = issue.form("comment");
    assert_eq!(
        (action.as_str(), method.as_str()),
        ("/northstar/atlas/issues/15/comments", "post")
    );
    assert_eq!(fields, vec![("body".to_owned(), String::new())]);
    assert_eq!(issue.tag("comment-body"), "textarea");
    assert_eq!(issue.attr("comment-body", "aria-label"), "Comment");
    assert_eq!(issue.form_of("comment-submit"), "comment");
    // `close` lives in the comment form and posts `state=closed` to the state route.
    assert_eq!(issue.form_of("close"), "comment");
    assert_eq!(
        issue.attr("close", "formaction"),
        "/northstar/atlas/issues/15/state"
    );
    assert_eq!(
        (issue.attr("close", "name"), issue.attr("close", "value")),
        ("state".to_owned(), "closed".to_owned())
    );
    assert_eq!(
        submit(
            &mut state,
            "alicechen",
            &action,
            &[("body", "A comment from the form.")]
        )
        .0,
        200
    );
    let (_, body) = submit(
        &mut state,
        "alicechen",
        "/northstar/atlas/issues/15/state",
        &[("body", ""), ("state", "closed")],
    );
    let closed = Page::parse("closed", body);
    assert_eq!(closed.text("state-text"), "Closed");
    assert_eq!(closed.text("comment-0-text"), "A comment from the form.");
    assert_eq!(
        (
            closed.attr("reopen", "name"),
            closed.attr("reopen", "value")
        ),
        ("state".to_owned(), "open".to_owned())
    );
    // Star is a one-button form; search is a GET form on `q`.
    let (action, method, _) = closed.form("star-form");
    assert_eq!(
        (action.as_str(), method.as_str()),
        ("/northstar/atlas/star", "post")
    );
    assert_eq!(closed.form_of("star"), "star-form");
    let (action, method, fields) = closed.form("search");
    assert_eq!(
        (action.as_str(), method.as_str(), fields[0].0.as_str()),
        ("/search", "get", "q")
    );
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
    let compare = Page::parse(
        "/compare",
        get(&mut state, "bmartinez", "/northstar/atlas/compare").1,
    );
    let (action, method, fields) = compare.form("new-pull");
    assert_eq!(
        (action.as_str(), method.as_str()),
        ("/northstar/atlas/pulls", "post")
    );
    assert_eq!(
        fields,
        vec![
            ("title".to_owned(), String::new()),
            ("head".to_owned(), "refs/heads/".to_owned()),
            ("base".to_owned(), "refs/heads/main".to_owned())
        ]
    );
    assert_eq!(compare.form_of("new-pull-submit"), "new-pull");
    assert_eq!(
        post(&mut state, "bmartinez", "/northstar/atlas/pulls", json!({"title":"Sort before you iterate","head":"refs/heads/missing","base":"refs/heads/main"})).0,
        422
    );
    let (status, body) = post(
        &mut state,
        "bmartinez",
        "/northstar/atlas/pulls",
        json!({"title":"Sort before you iterate","head":head,"base":"refs/heads/main"}),
    );
    assert_eq!(status, 200);
    let pull = Page::parse("pull", body);
    assert_eq!(pull.text("thread-title"), "Sort before you iterate");
    assert_eq!(pull.text("review-status-title"), "Review required");
    let (action, method, _) = pull.form("merge-form");
    assert_eq!(
        (action.as_str(), method.as_str()),
        ("/northstar/atlas/pull/15/merge", "post")
    );
    assert_eq!(pull.form_of("merge"), "merge-form");
    // The files view's review button approves with a fixed body.
    let files = Page::parse(
        "files",
        get(&mut state, "alicechen", "/northstar/atlas/pull/15/files").1,
    );
    let (action, _, fields) = files.form("approve-form");
    assert_eq!(action, "/northstar/atlas/pull/15/reviews");
    assert_eq!(
        fields,
        vec![
            ("decision".to_owned(), "approve".to_owned()),
            ("body".to_owned(), "Looks good.".to_owned())
        ]
    );
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
    let (status, body) = submit(
        &mut state,
        "alicechen",
        "/northstar/atlas/pull/15/merge",
        &[],
    );
    assert_eq!(status, 200);
    let merged = Page::parse("merged", body);
    assert_eq!(merged.text("state-text"), "Merged");
    assert!(!merged.has("merge") && merged.has("delete-branch"));
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
    let (status, body) = post(&mut state, "alice", "/northstar/atlas/star", json!({}));
    assert_eq!(status, 200);
    let page = Page::parse("stargazers", body);
    assert_eq!(page.text("stars-count"), "3 people starred northstar/atlas");
    assert!(page.text("star").contains("Starred"));
    let restored: Value = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
    assert_eq!(restored, state);
    let (_, body) = post(&mut state, "alice", "/northstar/atlas/star", json!({}));
    assert_eq!(
        Page::parse("stargazers", body).text("stars-count"),
        "2 people starred northstar/atlas"
    );
}

#[test]
fn owner_repo_blob_and_gist_pages_resolve() {
    let mut state = github();
    let home = Page::parse("/", get(&mut state, "alice", "/").1);
    assert_eq!(
        home.attr("repo-northstar-atlas", "href"),
        "/northstar/atlas"
    );
    assert_eq!(home.text("repo-name-atlas"), "northstar/atlas");
    assert_eq!(home.attr("gists", "href"), "/gists");
    assert_eq!(
        Page::parse("/northstar", get(&mut state, "alice", "/northstar").1)
            .attr("owned-name-atlas", "href"),
        "/northstar/atlas"
    );
    let repo = Page::parse(
        "/northstar/atlas",
        get(&mut state, "alice", "/northstar/atlas").1,
    );
    assert_eq!(repo.text("topic-1"), "determinism");
    assert_eq!(repo.text("tab-issues-link"), "Issues 1");
    // The old `/blob/<path>` shape still resolves on the default branch.
    assert!(Page::parse(
        "blob",
        get(&mut state, "alice", "/northstar/atlas/blob/README.md").1
    )
    .raw_text("blob-text")
    .contains("Release code lives in the launch doc"));
    assert!(
        Page::parse("gist", get(&mut state, "alice", "/gist/bfs-order").1)
            .raw_text("gist-file-text-0")
            .contains("sort before you iterate")
    );
    assert_eq!(
        Page::parse("gists", get(&mut state, "alice", "/gists").1).attr("gist-bfs-order", "href"),
        "/gist/bfs-order"
    );
    let hits = Page::parse("search", get(&mut state, "alice", "/search?q=simulation").1);
    assert_eq!(hits.text("search-title"), "1 repository result");
    assert_eq!(hits.attr("hit-link-atlas", "href"), "/northstar/atlas");
    assert_eq!(get(&mut state, "alice", "/northstar/ghost").0, 404);
    assert_eq!(get(&mut state, "alice", "/nobody").0, 404);
    // The JSON mirror an agent scripts against.
    let issues = get(&mut state, "alice", "/api/northstar/atlas/issues").1;
    assert!(issues.contains("BFS path test fails on Windows only"));
}

fn page(state: &mut Value, path: &str) -> Page {
    let (status, body) = get(state, "alicechen", path);
    assert_eq!(status, 200, "{path}: {body}");
    Page::parse(path, body)
}
fn seeded(site: &str, skin: &str) -> Value {
    let raw =
        std::fs::read_to_string(cw_service_common::reference::reference_site_path(site)).unwrap();
    let mut site: Value = serde_json::from_str(&raw).unwrap();
    site["initial_state"]["skin"] = skin.into();
    GitService
        .initialize(site["initial_state"].clone(), &ctx("alicechen"))
        .unwrap()
}

/// The repository home has GitHub's chrome: the header band with the tab strip and the
/// Code tab underlined, a branch selector, the latest-commit bar, folder icons, a README
/// and the About column with a languages bar.
#[test]
fn repository_home_has_tabs_branch_selector_files_and_about() {
    let mut state = seeded("github", "github");
    let page = page(&mut state, "/northstar/atlas");
    assert_eq!(page.title(), "northstar/atlas: Deterministic simulation runtime: processes, filesystem, network and browser, replayable byte for byte. · GitHub");
    assert_eq!(page.tag("chrome"), "header");
    assert_eq!(page.attr("mark", "href"), "/");
    assert_eq!(
        (page.text("crumb-0"), page.attr("crumb-1", "href")),
        ("northstar".to_owned(), "/northstar/atlas".to_owned())
    );
    assert_eq!(page.text("tab-issues-link"), "Issues 6");
    assert_eq!(
        page.attr("tab-issues-link", "href"),
        "/northstar/atlas/issues"
    );
    assert_eq!(page.text("tab-issues-count"), "6");
    assert_eq!(page.text("tab-pulls-count"), "2");
    assert_eq!(page.text("tab-pulls-link"), "Pull requests 2");
    assert_eq!(page.text("branch-name"), "main");
    assert_eq!(page.text("branches"), "4 Branches");
    assert_eq!(page.text("history"), "7 Commits");
    assert_eq!(
        page.attr("history", "href"),
        "/northstar/atlas/commits/main"
    );
    assert_eq!(
        page.text("latest-message"),
        "Expand the README with a quick start and pin serde"
    );
    assert!(page.text("latest-when").contains("ago"));
    // The Code tab is the current one; the Issues tab is not.
    assert!(page.has_class("tab-code", "active") && page.has_class("tab-code-link", "active"));
    assert!(!page.has_class("tab-issues", "active"));
    // Folders first, with the folder glyph, then files; each row names its last commit.
    let icons: Vec<String> = page
        .ids_with_prefix("entry-icon-")
        .iter()
        .map(|id| page.attr(id, "data-icon"))
        .collect();
    assert_eq!(
        icons,
        vec!["folder", "folder", "folder", "file", "file", "file", "file"]
    );
    assert_eq!(page.text("file-0"), ".github");
    assert_eq!(
        page.attr("file-0", "href"),
        "/northstar/atlas/tree/main/.github"
    );
    assert_eq!(
        page.attr("file-3", "href"),
        "/northstar/atlas/blob/main/CONTRIBUTING.md"
    );
    assert_eq!(
        page.text("entry-message-0"),
        "Run the test matrix on Windows too"
    );
    assert_eq!(
        page.text("entry-message-3"),
        "Merge pull request #11 from northstar/contributing"
    );
    assert_eq!(
        page.text("entry-message-6"),
        "Expand the README with a quick start and pin serde"
    );
    // README rendered as headings, a mono code block and lists with live links.
    let headings = page.ids_with_prefix("readme-h-");
    assert!(headings
        .iter()
        .any(|id| page.text(id) == "Quick start" && page.tag(id) == "h2"));
    let code = page.ids_with_prefix("readme-code-");
    assert!(page.tag(&code[0]) == "pre" && page.raw_text(&code[0]).contains("cargo add atlas"));
    assert!(page
        .ids_with_prefix("readme-li-")
        .iter()
        .any(|id| id.contains("-link-")
            && page.attr(id, "href") == "http://status.northstar.example/"));
    assert_eq!(page.text("language-name-0"), "Rust");
    assert_eq!(page.text("about-license-link"), "MIT license");
    assert_eq!(page.text("about-stars-link"), "7 stars");
    assert_eq!(
        page.attr("stargazers", "href"),
        "/northstar/atlas/stargazers"
    );
    assert_eq!(page.text("star"), "Starred");
}

#[test]
fn commits_commit_tree_blob_and_branches_pages_render_history() {
    let mut state = seeded("github", "github");
    let commits = page(&mut state, "/northstar/atlas/commits/main");
    let titles: Vec<String> = commits
        .ids_with_prefix("commits-")
        .into_iter()
        .filter(|id| id.ends_with("-title") && id != "commits-title")
        .map(|id| commits.text(&id))
        .collect();
    assert_eq!(titles.len(), 7);
    assert_eq!(
        titles[0],
        "Expand the README with a quick start and pin serde"
    );
    assert_eq!(titles[6], "Initial commit");
    assert!(
        commits
            .text("commits-day-label-1")
            .starts_with("Commits on Aug")
            || commits
                .text("commits-day-label-1")
                .starts_with("Commits on Sep")
    );
    assert!(commits.has_class("commits-0-sha", "sha-btn"));
    let sha = commits.attr("commits-0-sha", "href");
    // A commit page shows the message, the author, the stats and a coloured unified diff.
    let commit = page(&mut state, &sha);
    assert!(commit
        .text("diff-summary")
        .starts_with("Showing 2 changed files with"));
    let hunk = commit.ids_with_prefix("diff-file-0-hunk-");
    assert!(commit.text(&hunk[0]).starts_with("@@ -") && commit.has_class(&hunk[0], "hunk"));
    let runs = commit.ids_with_prefix("diff-file-0-lines-");
    assert!(runs.iter().any(|id| commit.has_class(id, "add")));
    assert_eq!(commit.text("diff-file-0-path"), "Cargo.toml");
    assert!(commit.html.contains("--add: #dafbe1") && commit.html.contains("--del: #ffebe9"));
    // A short SHA resolves too.
    assert_eq!(get(&mut state, "alice", &sha[..sha.len() - 30]).0, 200);
    // Tree pages list a folder; blob pages number their lines in mono.
    let tree = page(&mut state, "/northstar/atlas/tree/main/src");
    let names: Vec<String> = tree
        .ids_with_prefix("file-")
        .iter()
        .map(|id| tree.text(id))
        .collect();
    assert_eq!(names, vec!["bfs.rs", "clock.rs", "lib.rs", "seed.rs"]);
    assert_eq!(tree.attr("entry-up-link", "href"), "/northstar/atlas");
    assert!(!tree.has("readme") && !tree.has("about"));
    let blob = page(&mut state, "/northstar/atlas/blob/main/src/bfs.rs");
    assert_eq!(blob.tag("line-numbers"), "pre");
    assert_eq!(blob.raw_text("line-numbers").lines().count(), 19);
    assert!(blob
        .raw_text("blob-text")
        .contains("use std::collections::HashMap;"));
    assert_eq!(
        (blob.text("raw"), blob.attr("raw", "href")),
        (
            "Raw".to_owned(),
            "/northstar/atlas/raw/main/src/bfs.rs".to_owned()
        )
    );
    // Blame, copy, download and edit are gone: this instance has no blame view, no
    // clipboard and no editor. Raw is the one file action left, and it serves the bytes.
    assert!(!blob.has("blame") && !blob.has("copy") && !blob.has("download") && !blob.has("edit"));
    let raw = GitService
        .handle(
            &mut state,
            &ctx("alice"),
            &HttpRequest::get("http://github.com/northstar/atlas/raw/main/src/bfs.rs"),
        )
        .unwrap();
    assert_eq!(raw.status, 200);
    assert_eq!(raw.headers["content-type"], "text/plain; charset=utf-8");
    assert!(String::from_utf8(raw.body)
        .unwrap()
        .contains("use std::collections::HashMap;"));
    assert_eq!(blob.text("blob-stats"), "19 lines (18 loc) · 665 Bytes");
    // A branch on another ref shows that ref's tree.
    assert!(
        page(&mut state, "/northstar/atlas/blob/sort-refs/src/bfs.rs")
            .raw_text("blob-text")
            .contains("BTreeMap")
    );
    let branches = page(&mut state, "/northstar/atlas/branches");
    let rows = branches.ids_with_prefix("branch-row-");
    assert_eq!(rows.len(), 4);
    assert_eq!(branches.text("branch-default-0"), "Default");
    assert!(rows.iter().any(|id| branches.text(id).contains("sort-refs")
        && branches.text(id).contains("0 behind · 2 ahead")));
}

#[test]
fn issues_and_pull_requests_look_like_githubs() {
    let mut state = seeded("github", "github");
    let issues = page(&mut state, "/northstar/atlas/issues");
    let icons = issues.ids_with_prefix("state-");
    assert_eq!(icons.len(), 6);
    assert!(icons
        .iter()
        .all(|id| issues.attr(id, "data-icon") == "issue-open" && issues.has_class(id, "st-open")));
    assert!(issues
        .attr("thread-14-label-0", "style")
        .contains("#d73a4a"));
    assert_eq!(issues.text("thread-14-label-0"), "bug");
    assert!(issues
        .attr("thread-13-label-1", "style")
        .contains("#7057ff"));
    assert!(
        issues.text("meta-14").starts_with("#14 opened")
            && issues.text("meta-14").ends_with("by bmartinez")
    );
    assert_eq!(issues.text("comments-14"), "3");
    assert_eq!(issues.text("list-open"), "6 Open");
    assert_eq!(
        issues.attr("thread-14", "href"),
        "/northstar/atlas/issues/14"
    );
    assert_eq!(issues.attr("new", "href"), "/northstar/atlas/issues/new");
    let closed = page(&mut state, "/northstar/atlas/issues?state=closed");
    assert_eq!(closed.attr("state-9", "data-icon"), "issue-closed");
    assert!(closed.has_class("state-9", "st-done"));
    assert!(!closed.has("state-14"));
    // An issue: state pill, timeline of comment cards, sidebar, composer.
    let issue = page(&mut state, "/northstar/atlas/issues/14");
    assert_eq!(issue.text("state-text"), "Open");
    assert_eq!(
        ["comment-0", "comment-1", "comment-2", "comment-3"]
            .iter()
            .filter(|id| issue.has(id))
            .count(),
        3
    );
    assert_eq!(
        issue.attr("comment-1-text-link-8", "href"),
        "http://stackoverflow.com/questions/t-4411"
    );
    assert_eq!(issue.text("side-assignee-name"), "praman");
    assert!(issue.text("side-dev-link-1").starts_with("#15 "));
    assert_eq!(
        issue.attr("side-dev-link-1", "href"),
        "/northstar/atlas/pull/15"
    );
    assert_eq!(issue.tag("comment-body"), "textarea");
    assert_eq!(issue.text("close"), "Close issue");
    // Pull requests: tabs, reviews, merge box, files with a real diff.
    let pull = page(&mut state, "/northstar/atlas/pull/15");
    assert_eq!(
        pull.text("thread-verb"),
        "praman wants to merge 2 commits into"
    );
    assert_eq!(
        (pull.text("thread-base"), pull.text("thread-head-ref")),
        ("main".to_owned(), "sort-refs".to_owned())
    );
    assert_eq!(pull.attr("review-state-0", "data-icon"), "x-circle");
    assert_eq!(pull.attr("review-state-1", "data-icon"), "check");
    assert_eq!(pull.text("merge"), "Merge pull request");
    assert!(pull.has_class("merge", "btn-primary"));
    assert_eq!(pull.text("ptab-files-count"), "2");
    assert_eq!(
        pull.attr("ptab-files-link", "href"),
        "/northstar/atlas/pull/15/files"
    );
    let files = page(&mut state, "/northstar/atlas/pull/15/files");
    assert!(files
        .text("diff-summary")
        .starts_with("Showing 2 changed files with"));
    assert!(files.all_text().contains("@@ -1,8 +1,8 @@"));
    assert!(files.html.contains("class=\"run add\"") && files.html.contains("class=\"run del\""));
    let commits = page(&mut state, "/northstar/atlas/pull/15/commits");
    assert!(commits
        .all_text()
        .contains("Pin the neighbour order in the path test"));
    let merged = page(&mut state, "/northstar/atlas/pull/11");
    assert_eq!(merged.text("state-text"), "Merged");
    assert!(merged
        .text("merged-status-title")
        .starts_with("Pull request successfully merged"));
    let draft = page(&mut state, "/northstar/atlas/pull/19");
    assert_eq!(draft.text("state-text"), "Draft");
    assert!(!draft.has("merge") && draft.has("ready"));
    // Owner and stargazers.
    let owner = page(&mut state, "/northstar");
    assert!(owner
        .text("contrib-title")
        .ends_with("contributions in the last year"));
    assert_eq!(owner.text("pinned-title"), "Popular repositories");
    assert!(owner
        .ids_with_prefix("cell-")
        .iter()
        .any(|id| owner.has_class(id, "l1")));
    assert_eq!(
        page(&mut state, "/northstar/atlas/stargazers").text("stars-count"),
        "7 people starred northstar/atlas"
    );
}

/// The same routes and ids in GitLab's clothes: a sidebar instead of a tab strip, merge
/// requests instead of pull requests, the blue confirm button.
#[test]
fn the_gitlab_look_keeps_the_ids_and_changes_the_words() {
    let mut state = seeded("gitlab", "gitlab");
    let home = page(&mut state, "/");
    assert_eq!(home.title(), "GitLab");
    assert_eq!(home.tag("chrome"), "aside");
    assert_eq!(
        home.attr("repo-opensim-replay-tools", "href"),
        "/opensim/replay-tools"
    );
    let project = page(&mut state, "/opensim/replay-tools");
    assert!(project.title().ends_with("· GitLab"));
    assert!(project.html.contains("body class=\"skin-gitlab\""));
    assert_eq!(project.text("tab-pulls-link"), "Merge requests 0");
    assert_eq!(
        project.attr("tab-issues-link", "href"),
        "/opensim/replay-tools/issues"
    );
    assert_eq!(project.text("side-project"), "replay-tools");
    assert_eq!(project.text("about-title"), "Project information");
    assert!(project.has("file-0") && project.has("readme") && project.has("branch-name"));
    let pulls = page(&mut state, "/opensim/replay-tools/pulls");
    assert_eq!(pulls.text("new"), "New merge request");
    let issues = page(&mut state, "/opensim/replay-tools/issues");
    assert_eq!(issues.text("list-open"), "1 Open");
    // A plain-skinned instance keeps the GitHub look on its owner paths, as it always did.
    let mut plain = seeded("gitlab", "plain");
    assert!(page(&mut plain, "/opensim/replay-tools")
        .html
        .contains("body class=\"skin-github\""));
    assert_eq!(
        Page::parse("/", get(&mut plain, "alice", "/").1).attr("repo-replay-tools", "href"),
        "/repos/replay-tools"
    );
}
