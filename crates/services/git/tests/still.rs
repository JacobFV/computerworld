//! Stills of github.com and gitlab.com rendered through the engine, for the record under
//! `research/studies/site-stills/`. Ignored by default: a picture, not a gate.
//! `cargo test -p cw-service-git --test still -- --ignored`.
mod support;
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_git::GitService;
use serde_json::Value;

fn ctx() -> ServiceContext {
    ServiceContext {
        actor: "alicechen".into(),
        source: "alice-mac".into(),
        tick: 1,
        seed: 1,
        instance: "github".into(),
    }
}
fn site(name: &str, skin: &str) -> Value {
    let raw =
        std::fs::read_to_string(format!("../../../worlds/company-2026/sites/{name}.json")).unwrap();
    let mut site: Value = serde_json::from_str(&raw).unwrap();
    site["initial_state"]["skin"] = skin.into();
    GitService
        .initialize(site["initial_state"].clone(), &ctx())
        .unwrap()
}
fn shoot(state: &mut Value, url: &str, file: &str, height: u32) {
    let response = GitService
        .handle(state, &ctx(), &HttpRequest::get(url))
        .unwrap();
    assert_eq!(response.status, 200, "{url}");
    support::still(
        &String::from_utf8(response.body).unwrap(),
        file,
        1280,
        height,
    );
}

#[test]
#[ignore]
fn github_and_gitlab_stills() {
    let mut github = site("github", "github");
    shoot(
        &mut github,
        "http://github.com/northstar/atlas",
        "github.png",
        1400,
    );
    let sha = github["repositories"]["atlas"]["refs"]["refs/heads/sort-refs"]
        .as_str()
        .unwrap()
        .to_owned();
    shoot(
        &mut github,
        &format!("http://github.com/northstar/atlas/commit/{sha}"),
        "github-commit.png",
        1100,
    );
    shoot(
        &mut github,
        "http://github.com/northstar/atlas/commits/main",
        "github-commits.png",
        900,
    );
    shoot(
        &mut github,
        "http://github.com/northstar/atlas/tree/main/src",
        "github-tree.png",
        700,
    );
    shoot(
        &mut github,
        "http://github.com/northstar/atlas/issues",
        "github-issues.png",
        800,
    );
    shoot(
        &mut github,
        "http://github.com/northstar/atlas/pull/15",
        "github-pull.png",
        1800,
    );
    shoot(
        &mut github,
        "http://github.com/northstar/atlas/blob/main/src/bfs.rs",
        "github-blob.png",
        900,
    );
    shoot(
        &mut github,
        "http://github.com/northstar",
        "github-profile.png",
        900,
    );
    let mut gitlab = site("gitlab", "gitlab");
    shoot(
        &mut gitlab,
        "http://gitlab.com/opensim/replay-tools",
        "gitlab.png",
        1200,
    );
    shoot(
        &mut gitlab,
        "http://gitlab.com/opensim/replay-tools/issues",
        "gitlab-issues.png",
        800,
    );
    let mut plain = site("gitlab", "plain");
    shoot(&mut plain, "http://git.internal/", "git-internal.png", 500);
}
