//! github.com, gitlab.com and git.internal served as HTML by the git service and driven
//! end to end through the agent API: the browser renders them with the web engine, the
//! semantic observation lists the tabs, the branch selector, the file table, the diffs
//! and the merge box by the ids the service documents, and the flow a person would run —
//! browse a repository, open a file, read a commit diff, open a pull request, comment —
//! works by clicking those ids.
use computerworld::{reference_world, World};
use cw_protocol::{ActionEnvelope, EnvironmentConfig};
use serde_json::{json, Value};

const MACHINE: &str = "alice-mac";
const ACTOR: &str = "alice";

fn world() -> (World, String) {
    let mut world = World::new(reference_world(), 42).unwrap();
    let session = world
        .environment(EnvironmentConfig {
            actor: ACTOR.into(),
            machines: vec![MACHINE.into()],
            actions: vec!["browser.v1".into(), "keyboard.v1".into()],
            observations: vec!["semantic.v1".into(), "browser.v1".into()],
            action_budget: 1 << 20,
        })
        .unwrap();
    (world, session)
}
fn act(world: &mut World, session: &str, channel: &str, op: &str, payload: Value) -> Value {
    let result = world
        .step(
            session,
            vec![ActionEnvelope::new(channel, op, MACHINE, payload)],
        )
        .unwrap();
    assert!(result.outcomes[0].success, "{op}: {:?}", result.outcomes[0]);
    result.outcomes[0].value.clone()
}
fn click(world: &mut World, session: &str, id: &str) {
    act(world, session, "browser.v1", "click", json!({ "id": id }));
}
fn go(world: &mut World, session: &str, url: &str) {
    act(
        world,
        session,
        "browser.v1",
        "navigate",
        json!({ "url": url }),
    );
}
fn page(world: &World, session: &str) -> Value {
    world.observe(session).unwrap().channels["semantic.v1"][MACHINE].clone()
}
fn url(world: &World, session: &str) -> String {
    world.observe(session).unwrap().channels["browser.v1"][MACHINE]["url"]
        .as_str()
        .unwrap()
        .to_owned()
}
fn title(world: &World, session: &str) -> String {
    page(world, session)["title"]
        .as_str()
        .unwrap_or("")
        .to_owned()
}
/// Every element of the semantic tree, flattened.
fn elements(page: &Value) -> Vec<Value> {
    fn walk(elements: &[Value], out: &mut Vec<Value>) {
        for e in elements {
            out.push(e.clone());
            if let Some(children) = e["children"].as_array() {
                walk(children, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(page["elements"].as_array().unwrap(), &mut out);
    out
}
fn all(world: &World, session: &str) -> Vec<Value> {
    elements(&page(world, session))
}
fn by_id<'a>(all: &'a [Value], id: &str) -> &'a Value {
    all.iter()
        .find(|e| e["id"] == id)
        .unwrap_or_else(|| panic!("no element {id}"))
}
fn text(all: &[Value], id: &str) -> String {
    by_id(all, id)["text"].as_str().unwrap_or("").to_owned()
}
fn link_to(all: &[Value], id: &str) -> String {
    let element = by_id(all, id);
    assert_eq!(element["kind"], "link", "{id} is not a link");
    element["url"].as_str().unwrap_or("").to_owned()
}
fn has(all: &[Value], id: &str) -> bool {
    all.iter().any(|e| e["id"] == id)
}
/// The whole page's text, for the parts of a diff or a file that are prose, not widgets.
fn body_text(all: &[Value]) -> String {
    all.iter()
        .filter_map(|e| e["text"].as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The main GitHub flow: the repository home, down into a folder, into a file, out to a
/// commit's diff, then a pull request and a comment on it.
#[test]
fn github_is_browsed_from_a_repository_to_a_file_a_commit_and_a_pull_request() {
    let (mut world, session) = world();
    go(&mut world, &session, "http://github.com/northstar/atlas");
    assert!(
        title(&world, &session).starts_with("northstar/atlas: Deterministic simulation runtime")
    );
    let home = all(&world, &session);

    // The header: the mark, the owner/name crumbs and the repository tab strip with its
    // counters, each tab a link to its own route.
    assert_eq!(link_to(&home, "mark"), "http://github.com/");
    assert_eq!(text(&home, "crumb-0"), "northstar");
    assert_eq!(text(&home, "tab-issues-link"), "Issues 6");
    assert_eq!(
        link_to(&home, "tab-issues-link"),
        "http://github.com/northstar/atlas/issues"
    );
    assert_eq!(text(&home, "tab-pulls-link"), "Pull requests 2");
    for tab in [
        "code", "actions", "projects", "wiki", "security", "insights", "settings",
    ] {
        assert!(has(&home, &format!("tab-{tab}-link")), "no {tab} tab");
    }
    // The branch selector and the latest-commit bar over the file table.
    assert_eq!(text(&home, "branch-select"), "main");
    assert_eq!(text(&home, "branches"), "4 Branches");
    assert_eq!(text(&home, "history"), "7 Commits");
    assert_eq!(
        link_to(&home, "history"),
        "http://github.com/northstar/atlas/commits/main"
    );
    assert_eq!(text(&home, "latest-author"), "alicechen");
    assert_eq!(
        text(&home, "latest-message"),
        "Expand the README with a quick start and pin serde"
    );
    // Folders first, then files, each row a link to a tree or a blob.
    assert_eq!(text(&home, "file-0"), ".github");
    assert_eq!(
        link_to(&home, "file-0"),
        "http://github.com/northstar/atlas/tree/main/.github"
    );
    assert_eq!(text(&home, "file-6"), "README.md");
    assert_eq!(
        link_to(&home, "file-6"),
        "http://github.com/northstar/atlas/blob/main/README.md"
    );
    assert_eq!(
        text(&home, "entry-message-6"),
        "Expand the README with a quick start and pin serde"
    );
    // The README is rendered, not shown as source: headings, and its links are live.
    assert!(home
        .iter()
        .any(|e| e["kind"] == "heading" && e["text"] == "Quick start"));
    assert!(body_text(&home).contains("cargo run --example company"));
    assert!(home
        .iter()
        .any(|e| e["kind"] == "link" && e["url"] == "http://status.northstar.example/"));
    // The About column, and Star as a button because starring is a POST.
    assert_eq!(text(&home, "about-stars-link"), "7 stars");
    assert_eq!(
        link_to(&home, "about-license-link"),
        "http://github.com/northstar/atlas/blob/main/LICENSE"
    );
    assert_eq!(by_id(&home, "star")["kind"], "button");

    // Browse: into src, then into a file.
    click(&mut world, &session, "file-1");
    assert_eq!(
        url(&world, &session),
        "http://github.com/northstar/atlas/tree/main/src"
    );
    let tree = all(&world, &session);
    assert_eq!(
        link_to(&tree, "entry-up-link"),
        "http://github.com/northstar/atlas"
    );
    let names: Vec<String> = (0..4).map(|i| text(&tree, &format!("file-{i}"))).collect();
    assert_eq!(names, ["bfs.rs", "clock.rs", "lib.rs", "seed.rs"]);
    click(&mut world, &session, "file-0");
    assert_eq!(
        url(&world, &session),
        "http://github.com/northstar/atlas/blob/main/src/bfs.rs"
    );
    assert_eq!(
        title(&world, &session),
        "northstar/atlas/src/bfs.rs at main · GitHub"
    );
    let blob = all(&world, &session);
    // Raw is the one file action a page with no script can offer, and it really serves
    // the bytes: blame, copy, download and edit went rather than swallow a click.
    assert_eq!(
        link_to(&blob, "raw"),
        "http://github.com/northstar/atlas/raw/main/src/bfs.rs"
    );
    assert!(!has(&blob, "blame") && !has(&blob, "copy") && !has(&blob, "edit"));
    assert_eq!(
        link_to(&blob, "crumb-part-0"),
        "http://github.com/northstar/atlas/tree/main/src"
    );
    // The source is on the page, numbered line by line (the gutter runs 1..19 beside it).
    let source = body_text(&blob);
    assert!(
        source.contains("use std::collections::HashMap;"),
        "{source}"
    );
    assert!(source.contains("19 lines (18 loc)"), "{source}");
    assert!(
        source.contains("1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19"),
        "no line numbers: {source}"
    );

    // A commit: the file's history, then the newest commit's unified diff.
    click(&mut world, &session, "blob-history");
    assert_eq!(
        url(&world, &session),
        "http://github.com/northstar/atlas/commits/main"
    );
    let commits = all(&world, &session);
    assert_eq!(
        text(&commits, "commits-0-title"),
        "Expand the README with a quick start and pin serde"
    );
    assert_eq!(text(&commits, "commits-6-title"), "Initial commit");
    assert_eq!(text(&commits, "commits-0-sha"), "e261739");
    click(&mut world, &session, "commits-0-sha");
    let commit = all(&world, &session);
    assert!(title(&world, &session).ends_with("northstar/atlas@e261739"));
    assert_eq!(text(&commit, "commit-author"), "alicechen");
    assert_eq!(text(&commit, "diff-file-0-path"), "Cargo.toml");
    assert_eq!(text(&commit, "diff-file-1-path"), "README.md");
    let diff = body_text(&commit);
    assert!(diff.contains("Showing 2 changed files with"), "{diff}");
    assert!(diff.contains("@@ -"), "no hunk header: {diff}");
    assert!(diff.contains("serde = { version = \"1\""), "{diff}");
    // The parent is a link back into the history.
    assert!(link_to(&commit, "commit-parent-0")
        .starts_with("http://github.com/northstar/atlas/commit/"));

    // A pull request, from the tab strip, and a comment left on it.
    click(&mut world, &session, "tab-pulls-link");
    let pulls = all(&world, &session);
    assert_eq!(text(&pulls, "list-open"), "2 Open");
    assert_eq!(
        text(&pulls, "thread-15"),
        "Sort refs before iterating in BFS"
    );
    click(&mut world, &session, "thread-15");
    assert_eq!(
        url(&world, &session),
        "http://github.com/northstar/atlas/pull/15"
    );
    let pull = all(&world, &session);
    assert!(title(&world, &session)
        .starts_with("Sort refs before iterating in BFS by praman · Pull Request #15"));
    assert_eq!(
        link_to(&pull, "thread-base"),
        "http://github.com/northstar/atlas/tree/main"
    );
    assert_eq!(text(&pull, "thread-head-ref"), "sort-refs");
    assert_eq!(text(&pull, "ptab-files-link"), "Files changed 2");
    // The review timeline and the merge box.
    assert_eq!(text(&pull, "review-author-0"), "bmartinez");
    assert_eq!(text(&pull, "review-author-1"), "alicechen");
    assert!(pull
        .iter()
        .any(|e| e["kind"] == "heading" && e["text"] == "Changes approved"));
    assert_eq!(by_id(&pull, "merge")["kind"], "button");
    assert_eq!(text(&pull, "merge"), "Merge pull request");
    assert_eq!(text(&pull, "close"), "Close pull request");

    act(
        &mut world,
        &session,
        "browser.v1",
        "fill",
        json!({"id":"comment-body","value":"Re-ran the matrix on Windows; green."}),
    );
    click(&mut world, &session, "comment-submit");
    let after = all(&world, &session);
    assert!(
        body_text(&after).contains("Re-ran the matrix on Windows; green."),
        "the comment is not on the page"
    );
    assert_eq!(text(&after, "ptab-conversation-link"), "Conversation 4");
    // The comment box comes back empty, ready for the next one.
    assert_eq!(
        by_id(&after, "comment-body")["value"]
            .as_str()
            .unwrap_or(""),
        ""
    );

    // Files changed is the same diff, reached from the pull request's own tabs.
    click(&mut world, &session, "ptab-files-link");
    assert_eq!(
        url(&world, &session),
        "http://github.com/northstar/atlas/pull/15/files"
    );
    let files = body_text(&all(&world, &session));
    assert!(files.contains("@@ -1,8 +1,8 @@"), "{files}");
}

/// The same service in GitLab's clothes and in no clothes at all: gitlab.com keeps every
/// id and URL github.com has, and git.internal is still the plain repository index the
/// onboarding tasks clone from.
#[test]
fn gitlab_and_the_internal_git_server_serve_the_same_repositories() {
    let (mut world, session) = world();
    go(
        &mut world,
        &session,
        "http://gitlab.com/opensim/replay-tools",
    );
    let home = all(&world, &session);
    assert_eq!(text(&home, "branch-select"), "main");
    assert_eq!(text(&home, "branches"), "1 Branch");
    assert_eq!(text(&home, "history"), "5 Commits");
    assert_eq!(
        link_to(&home, "file-4"),
        "http://gitlab.com/opensim/replay-tools/blob/main/README.md"
    );
    // The README's indented shell block is a code block, not a run-on paragraph.
    assert!(body_text(&home).contains("replay shrink run.log --until 'exit != 0'"));
    assert_eq!(text(&home, "about-stars-link"), "5 stars");

    click(&mut world, &session, "tab-issues-link");
    assert_eq!(
        url(&world, &session),
        "http://gitlab.com/opensim/replay-tools/issues"
    );
    let issues = all(&world, &session);
    assert_eq!(text(&issues, "list-open"), "1 Open");
    assert_eq!(
        link_to(&issues, "new"),
        "http://gitlab.com/opensim/replay-tools/issues/new"
    );
    click(&mut world, &session, "thread-1");
    assert_eq!(
        url(&world, &session),
        "http://gitlab.com/opensim/replay-tools/issues/1"
    );
    let issue = all(&world, &session);
    assert!(body_text(&issue).contains("predicate script"));
    assert_eq!(by_id(&issue, "comment-body")["kind"], "input");

    // git.internal: the gitweb-like index, still plain and still linking to the repository
    // the onboarding checklist clones.
    go(&mut world, &session, "http://git.internal/");
    assert_eq!(title(&world, &session), "Git repositories");
    let index = all(&world, &session);
    assert_eq!(
        link_to(&index, "repo-onboarding"),
        "http://git.internal/repos/onboarding"
    );
    click(&mut world, &session, "repo-onboarding");
    let repository = all(&world, &session);
    assert!(repository
        .iter()
        .any(|e| e["kind"] == "heading" && e["text"] == "onboarding"));
    let files = body_text(&repository);
    assert!(files.contains("checklist.txt"), "{files}");
    assert!(files.contains("mail read: pending"), "{files}");
}
