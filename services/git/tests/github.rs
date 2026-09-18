//! The `github` skin end to end, and the promise the `plain` skin makes: `/repos/*` is frozen.
use cw_protocol::HttpRequest;
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
    assert!(repo.contains("determinism") && repo.contains("Issues (1)"));
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
