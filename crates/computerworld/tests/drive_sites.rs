//! drive.google.com and dropbox.com served as HTML by the drive service and driven end to end
//! through the agent API: the browser renders them with the web engine, the semantic
//! observation lists the navigation, the search box, the items and the forms by the ids the
//! service documents, and opening, starring, filing, uploading and searching all happen by
//! clicking and filling those ids.
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
        .step(session, vec![ActionEnvelope::new(channel, op, MACHINE, payload)])
        .unwrap();
    assert!(result.outcomes[0].success, "{op}: {:?}", result.outcomes[0]);
    result.outcomes[0].value.clone()
}
fn page(world: &World, session: &str) -> Value {
    world.observe(session).unwrap().channels["semantic.v1"][MACHINE].clone()
}
fn url(world: &World, session: &str) -> String {
    world.observe(session).unwrap().channels["browser.v1"][MACHINE]["url"].as_str().unwrap().to_owned()
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
fn by_id<'a>(all: &'a [Value], id: &str) -> &'a Value {
    all.iter().find(|e| e["id"] == id).unwrap_or_else(|| panic!("no element {id} in {all:?}"))
}
fn has(all: &[Value], id: &str) -> bool {
    all.iter().any(|e| e["id"] == id)
}
fn click(world: &mut World, session: &str, id: &str) {
    act(world, session, "browser.v1", "click", json!({"id": id}));
}
fn fill(world: &mut World, session: &str, id: &str, value: &str) {
    act(world, session, "browser.v1", "fill", json!({"id": id, "value": value}));
}

#[test]
fn google_drive_is_browsed_starred_filed_and_searched_through_the_agent_api() {
    let (mut world, session) = world();
    act(&mut world, &session, "browser.v1", "navigate", json!({"url":"http://drive.google.com/"}));
    let home = page(&world, &session);
    assert_eq!(home["title"], "My Drive · Google Drive");
    let all = elements(&home);
    // The chrome: navigation links, the search form and its labelled box.
    assert_eq!(by_id(&all, "nav-shared")["kind"], "link");
    assert_eq!(by_id(&all, "nav-shared")["url"], "http://drive.google.com/shared-with-me");
    assert_eq!(by_id(&all, "nav-starred")["url"], "http://drive.google.com/starred");
    assert_eq!(by_id(&all, "nav-trash")["url"], "http://drive.google.com/trash");
    assert_eq!(by_id(&all, "find")["kind"], "form");
    assert_eq!(by_id(&all, "find-q")["kind"], "input");
    assert_eq!(by_id(&all, "find-q")["label"], "Search files");
    assert_eq!(by_id(&all, "find-submit")["kind"], "button");
    // Folders are links, each one card.
    let atlas = by_id(&all, "item-f-atlas");
    assert_eq!(atlas["kind"], "link");
    assert_eq!(atlas["url"], "http://drive.google.com/drive/folders/f-atlas");
    assert!(atlas["text"].as_str().unwrap().contains("Atlas"), "{atlas:?}");

    // Open the folder, then a file in it.
    click(&mut world, &session, "item-f-atlas");
    assert_eq!(url(&world, &session), "http://drive.google.com/drive/folders/f-atlas");
    let folder = page(&world, &session);
    assert_eq!(folder["title"], "Atlas · Google Drive");
    let all = elements(&folder);
    assert_eq!(by_id(&all, "crumb-root")["url"], "http://drive.google.com/drive/folders/root");
    // The shortcut is a link out of the site, to the document it stands for.
    assert_eq!(by_id(&all, "item-atlas-launch")["url"], "http://docs.google.com/documents/atlas-launch");
    click(&mut world, &session, "item-press-kit");
    assert_eq!(url(&world, &session), "http://drive.google.com/file/press-kit");
    let file = page(&world, &session);
    assert_eq!(file["title"], "press-kit.txt · Google Drive");
    let all = elements(&file);
    assert!(all.iter().any(|e| e["text"].as_str().is_some_and(|t| t.contains("Press kit contents"))));
    assert_eq!(by_id(&all, "star")["kind"], "button");

    // Star it: the button posts, the page comes back starred, and Starred lists it.
    click(&mut world, &session, "star");
    let all = elements(&page(&world, &session));
    assert!(by_id(&all, "star")["text"].as_str().unwrap().contains("Starred"), "{:?}", by_id(&all, "star"));
    click(&mut world, &session, "nav-starred");
    assert_eq!(url(&world, &session), "http://drive.google.com/starred");
    assert!(has(&elements(&page(&world, &session)), "item-press-kit"));

    // File a folder and upload into a folder alice owns.
    act(&mut world, &session, "browser.v1", "navigate", json!({"url":"http://drive.google.com/drive/folders/f-personal"}));
    let all = elements(&page(&world, &session));
    assert_eq!(by_id(&all, "make")["kind"], "form");
    assert_eq!(by_id(&all, "make-parent")["value"], "f-personal");
    fill(&mut world, &session, "make-name", "Receipts");
    click(&mut world, &session, "make-submit");
    let all = elements(&page(&world, &session));
    let made = all
        .iter()
        .find(|e| e["kind"] == "link" && e["text"].as_str().is_some_and(|t| t.contains("Receipts")))
        .unwrap_or_else(|| panic!("the new folder is listed: {all:?}"));
    assert!(made["id"].as_str().unwrap().starts_with("item-"));
    fill(&mut world, &session, "upload-name", "monitor.txt");
    fill(&mut world, &session, "upload-content", "Monitor receipt 429.99");
    click(&mut world, &session, "upload-submit");
    let all = elements(&page(&world, &session));
    assert!(all.iter().any(|e| e["kind"] == "link" && e["text"].as_str().is_some_and(|t| t.contains("monitor.txt"))));

    // Search from the box: type, press Enter, land on the results.
    click(&mut world, &session, "find-q");
    act(&mut world, &session, "keyboard.v1", "type", json!({"text":"monitor"}));
    act(&mut world, &session, "browser.v1", "key", json!({"key":"Enter"}));
    let results = page(&world, &session);
    assert_eq!(results["title"], "monitor · Google Drive");
    let all = elements(&results);
    assert_eq!(by_id(&all, "find-q")["value"], "monitor");
    assert!(all.iter().any(|e| e["kind"] == "link" && e["text"].as_str().is_some_and(|t| t.contains("monitor.txt"))));
}

#[test]
fn dropbox_lists_a_table_of_files_and_shares_by_link_through_the_agent_api() {
    let (mut world, session) = world();
    act(&mut world, &session, "browser.v1", "navigate", json!({"url":"http://dropbox.com/"}));
    let home = page(&world, &session);
    assert_eq!(home["title"], "Dropbox · Dropbox");
    let all = elements(&home);
    assert_eq!(by_id(&all, "nav-drive")["url"], "http://dropbox.com/");
    assert_eq!(by_id(&all, "find-q")["label"], "Search files");
    let assets = by_id(&all, "item-atlas-assets");
    assert_eq!(assets["kind"], "link");
    assert_eq!(assets["url"], "http://dropbox.com/drive/folders/atlas-assets");

    click(&mut world, &session, "item-atlas-assets");
    assert_eq!(url(&world, &session), "http://dropbox.com/drive/folders/atlas-assets");
    let all = elements(&page(&world, &session));
    for id in ["item-atlas-logo", "item-benchmarks", "item-faq", "item-press-kit"] {
        assert_eq!(by_id(&all, id)["kind"], "link", "{id}");
    }
    // The folder ships with a share link, and the link opens the public view.
    let shared = by_id(&all, "link-url")["url"].as_str().unwrap().to_owned();
    assert!(shared.starts_with("http://dropbox.com/s/"), "{shared}");
    // alice holds a grant here, so she may upload.
    fill(&mut world, &session, "upload-name", "notes-from-alice.txt");
    fill(&mut world, &session, "upload-content", "Moved the rest to Drive.");
    click(&mut world, &session, "upload-submit");
    let all = elements(&page(&world, &session));
    assert!(all.iter().any(|e| e["kind"] == "link" && e["text"].as_str().is_some_and(|t| t.contains("notes-from-alice.txt"))));

    click(&mut world, &session, "item-faq");
    assert_eq!(url(&world, &session), "http://dropbox.com/file/faq");
    click(&mut world, &session, "star");
    click(&mut world, &session, "nav-starred");
    assert!(has(&elements(&page(&world, &session)), "item-faq"));

    act(&mut world, &session, "browser.v1", "navigate", json!({"url": shared}));
    let public = page(&world, &session);
    assert_eq!(public["title"], "Atlas assets · Dropbox");
    assert!(elements(&public).iter().any(|e| e["text"].as_str().is_some_and(|t| t.contains("Opened with a share link"))));
}
