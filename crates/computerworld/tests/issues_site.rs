//! linear.app and issues.internal served as HTML by the issues service and driven end to
//! end through the agent API: the browser renders them with the web engine, the semantic
//! observation lists the sidebar, the view switcher, the assignee filter, the cards and
//! the status buttons by the ids the service documents, and the flow a person would run —
//! open Linear, filter, open an issue, change its state — works by clicking those ids.
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
fn body_text(all: &[Value]) -> String {
    all.iter()
        .filter_map(|e| e["text"].as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Open Linear at the workspace, walk into a team, filter it down to one person, open one
/// of their issues, comment on it and move it to another status.
#[test]
fn linear_is_filtered_and_an_issue_is_opened_commented_and_moved() {
    let (mut world, session) = world();
    go(&mut world, &session, "http://linear.app/");
    assert_eq!(title(&world, &session), "Northstar · Linear");
    let home = all(&world, &session);
    // The rail: the workspace, then every team the actor can see.
    assert_eq!(link_to(&home, "workspace"), "http://linear.app/");
    for key in ["ATL", "INF", "ONB", "OPS", "REN", "WEB"] {
        assert_eq!(
            link_to(&home, &format!("nav-{key}")),
            format!("http://linear.app/projects/{key}")
        );
    }
    assert_eq!(link_to(&home, "team-ATL"), "http://linear.app/projects/ATL");
    assert!(
        body_text(&home).contains("6 unfinished"),
        "no open count for Atlas"
    );

    // Into the Atlas team: the three views and the grouped issue list.
    click(&mut world, &session, "team-ATL");
    assert_eq!(url(&world, &session), "http://linear.app/projects/ATL");
    let list = all(&world, &session);
    assert!(list
        .iter()
        .any(|e| e["kind"] == "heading" && e["text"] == "Atlas"));
    assert_eq!(
        link_to(&list, "view-board"),
        "http://linear.app/projects/ATL?view=board"
    );
    assert_eq!(
        link_to(&list, "view-cycle"),
        "http://linear.app/projects/ATL?view=cycle"
    );
    assert_eq!(
        text(&list, "card-open-1"),
        "Sort refs before iterating in BFS"
    );
    assert_eq!(
        link_to(&list, "card-open-1"),
        "http://linear.app/projects/ATL/issues/1"
    );
    // Every status column is there, and the list shows issues of every one of them.
    let shown = body_text(&list);
    for column in ["Todo", "In Progress", "Blocked", "Done"] {
        assert!(shown.contains(column), "no {column} group: {shown}");
    }
    assert!(has(&list, "card-open-4") && has(&list, "card-open-9") && has(&list, "card-open-3"));

    // The board is the same issues in columns, and a card can be moved from it.
    click(&mut world, &session, "view-board");
    let board = all(&world, &session);
    assert_eq!(by_id(&board, "move-4-in_progress")["kind"], "button");
    assert!(has(&board, "card-open-4"));

    // Filter to one assignee: only their issues survive.
    go(&mut world, &session, "http://linear.app/projects/ATL");
    click(&mut world, &session, "filter-bob");
    assert_eq!(
        url(&world, &session),
        "http://linear.app/projects/ATL?assignee=bob"
    );
    let mine = all(&world, &session);
    assert!(has(&mine, "card-open-4"), "bob's ATL-4 is missing");
    assert!(has(&mine, "card-open-9"), "bob's ATL-9 is missing");
    assert!(
        !has(&mine, "card-open-1"),
        "alice's ATL-1 survived the filter"
    );
    // The filter is kept when the view changes.
    assert_eq!(
        link_to(&mine, "view-board"),
        "http://linear.app/projects/ATL?assignee=bob&view=board"
    );
    assert_eq!(
        link_to(&mine, "filter-all"),
        "http://linear.app/projects/ATL"
    );

    // Open one of them: the properties rail, the activity feed, the composer.
    click(&mut world, &session, "card-open-4");
    assert_eq!(
        url(&world, &session),
        "http://linear.app/projects/ATL/issues/4"
    );
    assert_eq!(title(&world, &session), "ATL-4 Windows matrix in CI");
    let issue = all(&world, &session);
    assert!(issue
        .iter()
        .any(|e| e["kind"] == "heading" && e["text"] == "Windows matrix in CI"));
    assert_eq!(link_to(&issue, "back"), "http://linear.app/projects/ATL");
    assert_eq!(
        link_to(&issue, "issue-project"),
        "http://linear.app/projects/ATL"
    );
    assert!(body_text(&issue).contains("Todo"));
    // The current status has no button of its own; every other one does.
    assert!(!has(&issue, "status-open"));
    for status in ["in_progress", "blocked", "closed"] {
        assert_eq!(by_id(&issue, &format!("status-{status}"))["kind"], "button");
    }
    assert_eq!(text(&issue, "status-closed"), "Move to Done");

    // Comment, then move the issue to In Progress: both are real posts that land back on
    // the issue with the change made.
    act(
        &mut world,
        &session,
        "browser.v1",
        "fill",
        json!({"id":"comment-body","value":"Runner is green on the Windows image now."}),
    );
    click(&mut world, &session, "comment-submit");
    let commented = all(&world, &session);
    assert!(body_text(&commented).contains("Runner is green on the Windows image now."));
    click(&mut world, &session, "status-in_progress");
    let moved = all(&world, &session);
    assert!(body_text(&moved).contains("In Progress"));
    assert!(
        !has(&moved, "status-in_progress"),
        "In Progress is still offered after the move"
    );
    assert!(
        has(&moved, "status-open"),
        "Todo is not offered after the move"
    );

    // And the list agrees: ATL-4 offers the moves an In Progress issue offers, not the
    // ones a Todo issue offers.
    click(&mut world, &session, "back");
    let list = all(&world, &session);
    assert!(has(&list, "card-open-4"));
    assert!(
        !has(&list, "move-4-in_progress"),
        "ATL-4 is still listed as Todo"
    );
    assert!(has(&list, "move-4-open") && has(&list, "move-4-blocked"));

    // A new issue is created from the composer at the foot of the list.
    act(
        &mut world,
        &session,
        "browser.v1",
        "fill",
        json!({"id":"new-issue-title","value":"Pin the Windows image"}),
    );
    act(
        &mut world,
        &session,
        "browser.v1",
        "fill",
        json!({"id":"new-issue-body","value":"Float is how we got here."}),
    );
    click(&mut world, &session, "new-issue-submit");
    assert!(body_text(&all(&world, &session)).contains("Pin the Windows image"));
}

/// issues.internal is the same tracker with no branding: the plain table the older tasks
/// were written against, still HTML, still by the same ids.
#[test]
fn the_internal_tracker_stays_plain_and_keeps_its_ids() {
    let (mut world, session) = world();
    go(&mut world, &session, "http://issues.internal/");
    let home = all(&world, &session);
    assert!(
        !has(&home, "rail") && !has(&home, "workspace"),
        "the plain tracker grew a sidebar"
    );
    assert_eq!(
        link_to(&home, "project-OPS"),
        "http://issues.internal/projects/OPS"
    );
    click(&mut world, &session, "project-OPS");
    assert_eq!(url(&world, &session), "http://issues.internal/projects/OPS");
    let project = all(&world, &session);
    assert!(project
        .iter()
        .any(|e| e["kind"] == "heading" && e["text"] == "Operations"));
    assert_eq!(
        link_to(&project, "issue-1"),
        "http://issues.internal/projects/OPS/issues/1"
    );
    // The composer posts title and body to the project's issue route.
    assert_eq!(by_id(&project, "new-issue")["kind"], "form");
    assert_eq!(text(&project, "new-issue-submit"), "Submit");

    click(&mut world, &session, "issue-1");
    let issue = all(&world, &session);
    assert!(body_text(&issue).contains("Confirm Atlas launch checklist"));
    assert!(body_text(&issue).contains("Read the release code from mail"));
    assert_eq!(
        link_to(&issue, "back"),
        "http://issues.internal/projects/OPS"
    );
    // The plain issue page keeps the update form with the status and assignee fields.
    assert_eq!(by_id(&issue, "update")["kind"], "form");
    assert_eq!(by_id(&issue, "update-assignee")["kind"], "input");
    act(
        &mut world,
        &session,
        "browser.v1",
        "fill",
        json!({"id":"comment-body","value":"Warmer shipped."}),
    );
    click(&mut world, &session, "comment-submit");
    assert!(body_text(&all(&world, &session)).contains("Warmer shipped."));
}
