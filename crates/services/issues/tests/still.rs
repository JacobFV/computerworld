//! Stills of linear.app rendered through the engine, for the record under
//! `research/studies/site-stills/`. Ignored by default: a picture, not a gate.
//! `cargo test -p cw-service-issues --test still -- --ignored`.
mod support;
use cw_protocol::HttpRequest;
use cw_sdk::{Service, ServiceContext};
use cw_service_issues::IssuesService;
use serde_json::Value;

#[test]
#[ignore]
fn linear_stills() {
    let ctx = ServiceContext {
        actor: "alice".into(),
        source: "alice-mac".into(),
        tick: 1,
        seed: 1,
        instance: "linear".into(),
    };
    let raw = std::fs::read_to_string("../../../worlds/internet/sites/linear.json").unwrap();
    let site: Value = serde_json::from_str(&raw).unwrap();
    let mut state = IssuesService
        .initialize(site["initial_state"].clone(), &ctx)
        .unwrap();
    for (path, file, height) in [
        ("/projects/ATL", "linear.png", 800),
        ("/", "linear-projects.png", 600),
        ("/projects/ATL?view=board", "linear-board.png", 900),
        ("/projects/ATL?view=cycle", "linear-cycle.png", 800),
        ("/projects/OPS/issues/7", "linear-issue.png", 900),
    ] {
        let response = IssuesService
            .handle(
                &mut state,
                &ctx,
                &HttpRequest::get(format!("http://linear.app{path}")),
            )
            .unwrap();
        assert_eq!(response.status, 200, "{path}");
        support::still(
            &String::from_utf8(response.body).unwrap(),
            file,
            1280,
            height,
        );
    }
    let mut plain = site["initial_state"].clone();
    plain["skin"] = "plain".into();
    let mut plain = IssuesService.initialize(plain, &ctx).unwrap();
    let response = IssuesService
        .handle(
            &mut plain,
            &ctx,
            &HttpRequest::get("http://issues.internal/projects/OPS"),
        )
        .unwrap();
    support::still(
        &String::from_utf8(response.body).unwrap(),
        "issues-internal.png",
        1280,
        700,
    );
}
