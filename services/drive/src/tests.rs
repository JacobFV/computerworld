use super::*;
use cw_protocol::{Page, PageElement};
fn ctx(actor: &str) -> ServiceContext {
    ServiceContext {
        actor: actor.into(),
        source: "pc".into(),
        tick: 9,
        seed: 1,
        instance: "drive".into(),
    }
}
/// The Drive seed in miniature: a root alice owns, a folder carol shares, a shortcut and a file.
fn seed(skin: &str) -> Value {
    json!({
        "skin": skin,
        "brand": "Google Drive",
        "theme": {"accent": "#1a73e8"},
        "root": "root",
        "nodes": {
            "root": {"kind": "folder", "name": "My Drive", "owner": "alice"},
            "f-atlas": {"kind": "folder", "name": "Atlas", "parent": "root", "owner": "carol",
                        "shared_with": ["alice", "bob"]},
            "d-launch": {"kind": "shortcut", "name": "Atlas launch checklist", "parent": "f-atlas",
                         "owner": "carol", "shared_with": ["alice"],
                         "target_url": "http://docs.google.com/documents/atlas-launch"},
            "f-logo": {"kind": "file", "name": "atlas-logo.txt", "parent": "f-atlas",
                       "owner": "alice", "content": "Northstar Atlas wordmark, 2026 refresh."},
            "f-private": {"kind": "file", "name": "salary.txt", "parent": "root",
                          "owner": "alice", "content": "not for bob"}
        }
    })
}
fn state(skin: &str, actor: &str) -> Value {
    DriveService.initialize(seed(skin), &ctx(actor)).unwrap()
}
fn get(state: &mut Value, actor: &str, url: &str) -> HttpResponse {
    DriveService
        .handle(state, &ctx(actor), &HttpRequest::get(url))
        .unwrap()
}
fn send(state: &mut Value, actor: &str, method: &str, url: &str, body: Value) -> HttpResponse {
    let mut request = HttpRequest::get(url);
    request.method = method.into();
    request.body = serde_json::to_vec(&body).unwrap();
    DriveService.handle(state, &ctx(actor), &request).unwrap()
}
fn page(response: &HttpResponse) -> Page {
    serde_json::from_slice(&response.body).unwrap()
}
fn load(state: &Value) -> DriveState {
    serde_json::from_value(state.clone()).unwrap()
}
/// Seed typos hide whole subtrees, so they are refused at world load rather than at render.
#[test]
fn seed_shape_is_gated_at_load() {
    let c = ctx("alice");
    assert!(DriveService.initialize(json!([]), &c).is_err());
    assert!(DriveService
        .initialize(json!({"skin": "nonesuch"}), &c)
        .is_err());
    assert!(DriveService.initialize(Value::Null, &c).is_ok());
    assert!(DriveService
        .initialize(
            json!({"root":"root","nodes":{"root":{"owner":"alice"},"x":{"parent":"gone"}}}),
            &c
        )
        .is_err());
    assert!(DriveService
        .initialize(json!({"root":"root","nodes":{"x":{"owner":"alice"}}}), &c)
        .is_err());
    // A file's size is derived from its content when the seed leaves it out.
    let s = load(&state("gdrive", "alice"));
    assert_eq!(s.nodes["f-logo"].size, 39);
}
/// Visibility is the whole product: bob sees the shared folder and nothing else of alice's.
#[test]
fn a_share_is_the_only_thing_that_opens_a_tree() {
    let s = load(&state("gdrive", "alice"));
    assert!(s.visible("bob", "f-atlas"));
    assert!(s.visible("bob", "root"), "the path to a share must open");
    assert!(!s.visible("bob", "f-private"));
    assert!(!s.visible("bob", "d-launch"), "a share is not inherited");
    assert!(s.visible("alice", "d-launch"));
    // Seeing a folder because something inside it was shared grants nothing over the folder.
    assert!(!s.granted_to("bob", "root"));
    assert!(s.granted_to("bob", "f-atlas"));
    let names: Vec<_> = s.children("bob", "root").iter().map(|n| &n.name).collect();
    assert_eq!(names, ["Atlas"]);
    assert_eq!(s.shared_with_me("bob").len(), 1);
    assert_eq!(
        s.shared_with_me("alice").len(),
        2,
        "the folder and the shortcut"
    );
    assert_eq!(s.shared_with_me("carol").len(), 0);
}
#[test]
fn folders_uploads_moves_and_the_trash_are_real_and_refused_when_they_should_be() {
    let mut s = load(&state("gdrive", "alice"));
    assert!(s.create_folder("bob", "Nope", "root", 1).is_err());
    assert!(s.create_folder("alice", "  ", "root", 1).is_err());
    assert!(s.create_folder("alice", "Notes", "f-logo", 1).is_err());
    let notes = s.create_folder("alice", "Notes", "root", 1).unwrap();
    assert!(
        s.create_folder("alice", "notes", "root", 2).is_err(),
        "no twins"
    );
    let file = s
        .upload("alice", "readme.txt", &notes.id, "hello", 2)
        .unwrap();
    assert_eq!(file.size, 5);
    assert_eq!(s.children("alice", &notes.id).len(), 1);
    // A folder cannot be filed inside itself, directly or through its own descendants.
    assert!(s.relocate("alice", &notes.id, "", &notes.id).is_err());
    assert!(s.relocate("alice", "root", "Root", "").is_err());
    s.relocate("alice", &file.id, "notes.txt", "root").unwrap();
    assert_eq!(s.nodes[&file.id].name, "notes.txt");
    assert_eq!(s.children("alice", &notes.id).len(), 0);
    s.delete("alice", &file.id).unwrap();
    assert!(s.trashed(&file.id));
    assert!(s.delete("alice", &file.id).is_err());
    assert_eq!(s.trash("alice").len(), 1);
    assert!(!s.children("alice", "root").iter().any(|n| n.id == file.id));
    let restored: DriveState = serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
    assert_eq!(s, restored);
}
#[test]
fn sharing_stars_and_links_behave_per_actor() {
    let mut s = load(&state("gdrive", "alice"));
    assert!(s.share("bob", "f-private", "carol").is_err());
    assert!(s.share("alice", "f-private", " ").is_err());
    assert!(s.share("alice", "f-private", "alice").is_err());
    s.share("alice", "f-private", "bob").unwrap();
    assert!(s.visible("bob", "f-private"), "a share lands immediately");
    assert!(s.star("alice", "f-logo").unwrap());
    assert!(s.star("bob", "f-private").unwrap());
    assert_eq!(s.starred("alice").len(), 1);
    assert_eq!(s.starred("carol").len(), 0);
    assert!(!s.star("alice", "f-logo").unwrap());
    assert!(s.starred.is_empty() || !s.starred.contains_key("alice"));
    let minted = s.mint_link("alice", "f-logo", 9).unwrap();
    assert!(!minted.link.is_empty());
    // Re-minting is idempotent, so a link that was pasted somewhere keeps working.
    assert_eq!(
        s.mint_link("alice", "f-logo", 40).unwrap().link,
        minted.link
    );
    assert_eq!(s.by_link(&minted.link).unwrap().id, "f-logo");
    assert!(s.by_link("deadbeef").is_none());
}
/// Both skins must produce legal pages, and every control on them must carry a real route.
#[test]
fn skinned_pages_validate_and_only_promise_real_routes() {
    for skin in ["gdrive", "dropbox"] {
        let mut state = state(skin, "alice");
        for url in [
            "http://drive.google.com/",
            "http://drive.google.com/drive/folders/f-atlas",
            "http://drive.google.com/file/f-logo",
            "http://drive.google.com/file/d-launch",
            "http://drive.google.com/shared-with-me",
            "http://drive.google.com/starred",
            "http://drive.google.com/trash",
            "http://drive.google.com/search?q=atlas",
        ] {
            let response = get(&mut state, "alice", url);
            assert_eq!(response.status, 200, "{skin} {url}");
            let page = page(&response);
            page.validate()
                .unwrap_or_else(|e| panic!("{skin} {url}: {e}"));
            assert!(page.theme.is_some(), "{skin} {url} must carry the palette");
        }
        // A folder the actor cannot see is refused, not quietly rendered empty.
        assert_eq!(
            get(&mut state, "bob", "http://drive.google.com/file/f-private").status,
            403
        );
        assert_eq!(
            get(&mut state, "alice", "http://drive.google.com/nope").status,
            404
        );
    }
}
/// Dropbox lists, Drive tiles: one page model, two readings, and the list is one column.
#[test]
fn the_two_skins_lay_the_same_items_out_differently() {
    let columns = |skin: &str| {
        let mut state = state(skin, "alice");
        let page = page(&get(&mut state, "alice", "http://drive.google.com/"));
        page.elements
            .iter()
            .find_map(|e| match e {
                PageElement::Grid { columns, .. } => Some(*columns),
                _ => None,
            })
            .expect("a folder listing is a grid")
    };
    assert_eq!(columns("gdrive"), 3);
    assert_eq!(columns("dropbox"), 1);
}
/// The plain skin has no chrome and no palette: it is the rendering a bare `drive` instance gets.
#[test]
fn plain_renders_without_a_theme_and_still_navigates() {
    let mut state = state("plain", "alice");
    let response = get(&mut state, "alice", "http://drive/");
    let page = page(&response);
    page.validate().unwrap();
    assert!(page.theme.is_none());
    assert!(page
        .elements
        .iter()
        .any(|e| e.id() == "item-f-atlas" || e.id() == "item-f-private"));
    assert_eq!(serde_json::to_value(load(&state)).unwrap(), state);
}
/// A browser drives the skinned pages: upload, star and share, each visible on the next request.
#[test]
fn a_browser_can_upload_star_and_share() {
    let mut state = state("gdrive", "alice");
    let mut browser = cw_browser::BrowserState::default();
    {
        let c = ctx("alice");
        let mut transport = |r: HttpRequest| DriveService.handle(&mut state, &c, &r);
        browser
            .navigate(
                "http://drive.google.com/drive/folders/f-atlas",
                &mut transport,
            )
            .unwrap();
        browser.fill("upload-name", "atlas-brief.txt").unwrap();
        browser.fill("upload-content", "Ship on the 14th.").unwrap();
        browser.click("upload-submit", &mut transport).unwrap();
        browser.click("item-f-logo", &mut transport).unwrap();
        browser.click("star", &mut transport).unwrap();
        browser.fill("grant-actor", "carol").unwrap();
        browser.click("grant-submit", &mut transport).unwrap();
    }
    let s = load(&state);
    let brief = s
        .nodes
        .values()
        .find(|n| n.name == "atlas-brief.txt")
        .expect("the upload landed");
    assert_eq!(brief.parent.as_deref(), Some("f-atlas"));
    assert_eq!(brief.owner, "alice");
    assert!(s.is_starred("alice", "f-logo"));
    assert!(s.nodes["f-logo"].shared_with.contains("carol"));
    assert!(s.visible("carol", "f-logo"));
}
/// The JSON API is the same drive the pages show, including the trash and the share link.
#[test]
fn the_api_mutates_the_same_drive() {
    let mut state = state("gdrive", "alice");
    let made = send(
        &mut state,
        "alice",
        "POST",
        "http://drive.google.com/api/folders",
        json!({"name": "Reports", "parent": "root"}),
    );
    assert_eq!(made.status, 200);
    let folder: Node = serde_json::from_slice(&made.body).unwrap();
    let shared = send(
        &mut state,
        "alice",
        "POST",
        &format!("http://drive.google.com/api/nodes/{}/share", folder.id),
        json!({"actor": "bob"}),
    );
    assert_eq!(shared.status, 200);
    let seen = get(
        &mut state,
        "bob",
        "http://drive.google.com/api/shared-with-me",
    );
    let nodes: Vec<Node> = serde_json::from_slice(&seen.body).unwrap();
    assert!(nodes.iter().any(|n| n.id == folder.id));
    let linked = send(
        &mut state,
        "alice",
        "POST",
        &format!("http://drive.google.com/api/nodes/{}/link", folder.id),
        json!({}),
    );
    let token = serde_json::from_slice::<Node>(&linked.body).unwrap().link;
    // A share link opens for an actor with no grant at all; that is what a share link is for.
    let public = get(
        &mut state,
        "dana",
        &format!("http://drive.google.com/s/{token}"),
    );
    assert_eq!(public.status, 200);
    assert!(String::from_utf8(public.body).unwrap().contains("Reports"));
    let gone = send(
        &mut state,
        "alice",
        "DELETE",
        &format!("http://drive.google.com/api/nodes/{}", folder.id),
        json!({}),
    );
    assert_eq!(gone.status, 200);
    assert!(load(&state).trashed(&folder.id));
    let refused = send(
        &mut state,
        "bob",
        "POST",
        "http://drive.google.com/api/folders",
        json!({"name": "Mine", "parent": "root"}),
    );
    assert_eq!(refused.status, 403);
}
/// Rendering must never write, or a checkpoint taken after a GET would differ from one before it.
#[test]
fn rendering_is_pure() {
    for skin in ["plain", "gdrive", "dropbox"] {
        let mut state = state(skin, "alice");
        let before = state.clone();
        let first = get(&mut state, "alice", "http://drive.google.com/");
        let second = get(&mut state, "alice", "http://drive.google.com/");
        assert_eq!(first, second);
        assert_eq!(before, state, "{skin} rendering must not mutate state");
    }
}
