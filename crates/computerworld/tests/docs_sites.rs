//! docs.google.com and notion.so served as HTML by the docs service and driven end to end
//! through the agent API: the browser renders them with the web engine, the semantic
//! observation lists the file gallery, the sidebar, the prose, the grid and the forms by the
//! ids the service documents, and opening a document, starring it, commenting on it, editing
//! it and setting a cell all happen by clicking and filling those ids.
use computerworld::{reference_world, World};
use cw_protocol::{ActionEnvelope, EnvironmentConfig};
use serde_json::{json, Value};

const MACHINE: &str = "alice-mac";
const ACTOR: &str = "alice";

fn world() -> (World, String) {
    let mut definition = reference_world();
    // notion.so wears its own skin in `sites/notion.json`; a world file generated before that
    // still says `plain`, which is the Page rendering this test is not about.
    for service in &mut definition.services {
        if service.id == "notion" && service.initial_state["skin"] == "plain" {
            service.initial_state["skin"] = json!("notion");
        }
    }
    let mut world = World::new(definition, 42).unwrap();
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
fn act(world: &mut World, session: &str, op: &str, payload: Value) -> Value {
    let result = world
        .step(
            session,
            vec![ActionEnvelope::new("browser.v1", op, MACHINE, payload)],
        )
        .unwrap();
    assert!(result.outcomes[0].success, "{op}: {:?}", result.outcomes[0]);
    result.outcomes[0].value.clone()
}
fn url(world: &World, session: &str) -> String {
    world.observe(session).unwrap().channels["browser.v1"][MACHINE]["url"]
        .as_str()
        .unwrap()
        .to_owned()
}
/// The page and every element of its semantic tree, flattened.
fn elements(world: &World, session: &str) -> (Value, Vec<Value>) {
    fn walk(elements: &[Value], out: &mut Vec<Value>) {
        for e in elements {
            out.push(e.clone());
            if let Some(children) = e["children"].as_array() {
                walk(children, out);
            }
        }
    }
    let page = world.observe(session).unwrap().channels["semantic.v1"][MACHINE].clone();
    let mut out = Vec::new();
    walk(page["elements"].as_array().unwrap(), &mut out);
    (page, out)
}
fn by_id<'a>(all: &'a [Value], id: &str) -> &'a Value {
    all.iter()
        .find(|e| e["id"] == id)
        .unwrap_or_else(|| panic!("no element {id} in {all:?}"))
}
fn has(all: &[Value], id: &str) -> bool {
    all.iter().any(|e| e["id"] == id)
}
fn says(all: &[Value], text: &str) -> bool {
    all.iter()
        .any(|e| e["text"].as_str().is_some_and(|t| t.contains(text)))
}
fn click(world: &mut World, session: &str, id: &str) {
    act(world, session, "click", json!({"id": id}));
}
fn fill(world: &mut World, session: &str, id: &str, value: &str) {
    act(world, session, "fill", json!({"id": id, "value": value}));
}

/// Google Docs: the gallery, a document read and commented on and edited, the star, and the
/// spreadsheet's formula bar.
#[test]
fn google_docs_opens_a_document_comments_edits_and_sets_a_cell_through_the_agent_api() {
    let (mut world, session) = world();
    act(
        &mut world,
        &session,
        "navigate",
        json!({"url":"http://docs.google.com/"}),
    );
    let (page, all) = elements(&world, &session);
    assert_eq!(page["title"], "Google Docs");
    // The chrome: the file-type switcher, each entry a link to a route the service serves.
    for (id, href) in [
        ("nav-all", "/"),
        ("nav-doc", "/?type=doc"),
        ("nav-sheet", "/?type=sheet"),
        ("nav-slides", "/?type=slides"),
        ("nav-starred", "/starred"),
    ] {
        let e = by_id(&all, id);
        assert_eq!(e["kind"], "link", "{id}");
        assert_eq!(e["url"], format!("http://docs.google.com{href}"), "{id}");
    }
    assert!(says(&all, "4 files shared with you"));
    // Every file is one link carrying its caption; the new-file form posts the Page's fields.
    let card = by_id(&all, "file-atlas-launch");
    assert_eq!(card["kind"], "link");
    assert_eq!(card["url"], "http://docs.google.com/documents/atlas-launch");
    assert!(
        card["text"]
            .as_str()
            .unwrap()
            .contains("Atlas launch checklist"),
        "{card:?}"
    );
    assert!(by_id(&all, "file-q3-metrics")["text"]
        .as_str()
        .unwrap()
        .contains("Sheet"));
    assert_eq!(by_id(&all, "create")["kind"], "form");
    for id in [
        "create-title",
        "create-readers",
        "create-writers",
        "create-body",
    ] {
        assert_eq!(by_id(&all, id)["kind"], "input", "{id}");
    }
    assert_eq!(by_id(&all, "create-submit")["kind"], "button");

    // Open the checklist: the prose is the body, the links in it are real links.
    click(&mut world, &session, "file-atlas-launch");
    assert_eq!(
        url(&world, &session),
        "http://docs.google.com/documents/atlas-launch"
    );
    let (page, all) = elements(&world, &session);
    assert_eq!(page["title"], "Atlas launch checklist · Google Docs");
    assert_eq!(by_id(&all, "head-title")["text"], "Atlas launch checklist");
    assert!(says(&all, "owner carol · revision 4"));
    assert!(says(&all, "Release code: ATLAS-2026"));
    assert!(all
        .iter()
        .any(|e| e["kind"] == "link" && e["url"] == "http://github.com/northstar/atlas"));
    assert!(all.iter().any(
        |e| e["kind"] == "link" && e["url"] == "http://drive.google.com/drive/folders/f-atlas"
    ));
    // The seeded comments are on the page, and the comment box is a form.
    assert!(says(&all, "Tagged. Item 1 is done."));
    assert_eq!(by_id(&all, "comment")["kind"], "form");
    assert_eq!(by_id(&all, "comment-text")["kind"], "input");
    // alice may write here, so the editor is there with the revision the page is showing.
    assert_eq!(by_id(&all, "edit-revision")["value"], "4");

    // Comment: the form posts and the new note comes back on the page.
    fill(
        &mut world,
        &session,
        "comment-text",
        "Walkthrough is up; item 2 is done.",
    );
    click(&mut world, &session, "comment-submit");
    let (_, all) = elements(&world, &session);
    assert!(says(&all, "Walkthrough is up; item 2 is done."));
    assert!(says(&all, "Comments (3)"));

    // The star is a button in a form of its own; it toggles and Starred lists what it marked.
    assert_eq!(
        by_id(&all, "star")["text"],
        "Starred",
        "alice starred the checklist in the seed"
    );
    click(&mut world, &session, "star");
    let (_, all) = elements(&world, &session);
    assert_eq!(by_id(&all, "star")["text"], "Star");
    click(&mut world, &session, "star");
    click(&mut world, &session, "nav-starred");
    assert_eq!(url(&world, &session), "http://docs.google.com/starred");
    let (page, all) = elements(&world, &session);
    assert_eq!(page["title"], "Starred · Google Docs");
    assert_eq!(
        by_id(&all, "file-atlas-launch")["url"],
        "http://docs.google.com/documents/atlas-launch"
    );

    // Edit: an optimistic write with the revision the editor was showing.
    act(
        &mut world,
        &session,
        "navigate",
        json!({"url":"http://docs.google.com/documents/atlas-launch"}),
    );
    let (_, all) = elements(&world, &session);
    let revision = by_id(&all, "edit-revision")["value"]
        .as_str()
        .unwrap()
        .to_owned();
    fill(&mut world, &session, "edit-revision", &revision);
    fill(
        &mut world,
        &session,
        "edit-body",
        "Atlas launch checklist\n\nSigned off by alice.\n",
    );
    click(&mut world, &session, "edit-submit");
    let (_, all) = elements(&world, &session);
    assert!(says(&all, "Signed off by alice."));
    assert!(says(
        &all,
        &format!("revision {}", revision.parse::<u64>().unwrap() + 1)
    ));

    // The sheet is the same document read as a grid, and the formula bar is the cell form.
    click(&mut world, &session, "nav-sheet");
    assert_eq!(url(&world, &session), "http://docs.google.com/?type=sheet");
    let (_, sheets) = elements(&world, &session);
    assert!(
        !has(&sheets, "file-atlas-launch"),
        "the Sheets tab lists sheets only"
    );
    click(&mut world, &session, "file-q3-metrics");
    assert_eq!(
        url(&world, &session),
        "http://docs.google.com/documents/q3-metrics"
    );
    let (page, all) = elements(&world, &session);
    assert_eq!(page["title"], "Q3 metrics · Google Sheets");
    // The grid's cells carry `sheet-<A1>` in the DOM; the semantic projection flattens a
    // table to its text, so the observation is read by what the cells say.
    assert!(says(&all, "Latency p95") && says(&all, "184 ms"));
    assert_eq!(by_id(&all, "cell")["kind"], "form");
    fill(&mut world, &session, "cell-cell", "E1");
    fill(&mut world, &session, "cell-value", "Signed off");
    click(&mut world, &session, "cell-submit");
    let (_, all) = elements(&world, &session);
    assert!(
        says(&all, "Signed off"),
        "the new cell is drawn in the grid"
    );

    // The deck is the third reading: alice may read it, so there are slides and no editor.
    act(
        &mut world,
        &session,
        "navigate",
        json!({"url":"http://docs.google.com/documents/atlas-launch-review"}),
    );
    let (page, all) = elements(&world, &session);
    assert_eq!(page["title"], "Atlas launch review · Google Slides");
    assert!(by_id(&all, "slide-title-0")["text"]
        .as_str()
        .unwrap()
        .contains("Where we are"));
    assert!(
        !has(&all, "edit-submit"),
        "carol owns the deck and alice may only read it"
    );
}

/// Notion: the workspace sidebar, a page of blocks, and the comment the page takes.
#[test]
fn notion_opens_a_page_from_the_sidebar_and_comments_through_the_agent_api() {
    let (mut world, session) = world();
    act(
        &mut world,
        &session,
        "navigate",
        json!({"url":"http://notion.so/"}),
    );
    let (page, all) = elements(&world, &session);
    assert_eq!(page["title"], "Notion");
    assert!(says(&all, "Good morning, Alice"));
    assert_eq!(by_id(&all, "side-home")["url"], "http://notion.so/");
    assert_eq!(
        by_id(&all, "side-starred")["url"],
        "http://notion.so/starred"
    );
    // The plain page's one link per document is the sidebar's page list, still keyed by doc id.
    let entry = by_id(&all, "onboarding-checklist");
    assert_eq!(entry["kind"], "link");
    assert_eq!(
        entry["url"],
        "http://notion.so/documents/onboarding-checklist"
    );
    // The home cards open the same pages.
    assert_eq!(
        by_id(&all, "recent-team-wiki-home")["url"],
        "http://notion.so/documents/team-wiki-home"
    );
    assert_eq!(by_id(&all, "create")["kind"], "form");

    // Open a page from the sidebar: the breadcrumb, the properties and the blocks.
    click(&mut world, &session, "onboarding-checklist");
    assert_eq!(
        url(&world, &session),
        "http://notion.so/documents/onboarding-checklist"
    );
    let (page, all) = elements(&world, &session);
    assert_eq!(page["title"], "Onboarding checklist · Notion");
    // `title` is the breadcrumb root the plain page used; inline chrome flattens into the
    // crumb's text run, so the observation reads it there.
    assert!(says(&all, "Documents"));
    assert_eq!(
        by_id(&all, "document-title")["text"],
        "Onboarding checklist"
    );
    assert!(says(&all, "Revision 9 · owner carol"));
    assert!(says(&all, "Day 1"));
    assert!(says(&all, "Laptop, accounts, keys."));
    assert!(all
        .iter()
        .any(|e| e["kind"] == "link" && e["url"] == "http://github.com/northstar/atlas"));
    // alice writes here, so the editor is on the page under the ids the plain page used.
    assert_eq!(by_id(&all, "edit")["kind"], "form");
    assert_eq!(by_id(&all, "edit-revision")["value"], "9");

    // Favorites: alice starred this page in the seed, and the breadcrumb star toggles it.
    assert_eq!(by_id(&all, "star")["kind"], "button");
    click(&mut world, &session, "side-starred");
    assert_eq!(url(&world, &session), "http://notion.so/starred");
    let (page, all) = elements(&world, &session);
    assert_eq!(page["title"], "Favorites · Notion");
    assert_eq!(
        by_id(&all, "starred-onboarding-checklist")["url"],
        "http://notion.so/documents/onboarding-checklist"
    );

    // A comment on a page alice may read is one fill and one click.
    act(
        &mut world,
        &session,
        "navigate",
        json!({"url":"http://notion.so/documents/team-wiki-home"}),
    );
    let (_, all) = elements(&world, &session);
    assert!(says(&all, "Added the Lisbon page to the list."));
    assert_eq!(by_id(&all, "comment-text")["kind"], "input");
    fill(
        &mut world,
        &session,
        "comment-text",
        "Linked the onboarding checklist here too.",
    );
    click(&mut world, &session, "comment-submit");
    let (_, all) = elements(&world, &session);
    assert!(says(&all, "Linked the onboarding checklist here too."));
    assert!(
        says(&all, "Added the Lisbon page to the list."),
        "the earlier comment is still there"
    );
}
