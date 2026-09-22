use super::*;
use cw_protocol::Page;
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
/// A form post the way the browser sends one.
fn send_form(state: &mut Value, actor: &str, url: &str, body: &str) -> HttpResponse {
    let mut request = HttpRequest::get(url);
    request.method = "POST".into();
    request.headers.insert(
        "content-type".into(),
        "application/x-www-form-urlencoded".into(),
    );
    request.body = body.as_bytes().to_vec();
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
/// Every skinned page, parsed, after the strict validator has accepted its HTML and CSS.
fn html(response: &HttpResponse) -> cw_web::dom::Document {
    assert_eq!(
        response.header("content-type"),
        Some(web::html::HTML_MEDIA_TYPE)
    );
    let text = String::from_utf8(response.body.clone()).unwrap();
    web::html::validate_strict(&text).unwrap_or_else(|e| panic!("{e:?}\n{text}"));
    cw_web::html::parse(&text)
}
fn node(doc: &cw_web::dom::Document, id: &str) -> cw_web::dom::NodeId {
    *doc.by_id(id).first().unwrap_or_else(|| panic!("no #{id}"))
}
fn text_of(doc: &cw_web::dom::Document, id: &str) -> String {
    cw_web::paint::semantics::collapse(&doc.text_content(node(doc, id)))
}
fn attr_of(doc: &cw_web::dom::Document, id: &str, name: &str) -> String {
    doc.attr(node(doc, id), name).unwrap_or_default().to_owned()
}
const SCREENS: [&str; 8] = [
    "http://drive.google.com/",
    "http://drive.google.com/drive/folders/f-atlas",
    "http://drive.google.com/file/f-logo",
    "http://drive.google.com/file/d-launch",
    "http://drive.google.com/shared-with-me",
    "http://drive.google.com/starred",
    "http://drive.google.com/trash",
    "http://drive.google.com/search?q=atlas",
];
/// Both skins must produce pages the engine renders strictly, and every control on them must
/// carry a real route.
#[test]
fn skinned_pages_validate_and_only_promise_real_routes() {
    for skin in ["gdrive", "dropbox"] {
        let mut state = state(skin, "alice");
        for url in SCREENS {
            let response = get(&mut state, "alice", url);
            assert_eq!(response.status, 200, "{skin} {url}");
            let page = html(&response);
            // The chrome is on every screen, with the ids and routes the Page version had.
            assert_eq!(attr_of(&page, "nav-drive", "href"), "/");
            assert_eq!(attr_of(&page, "nav-shared", "href"), "/shared-with-me");
            assert_eq!(attr_of(&page, "nav-starred", "href"), "/starred");
            assert_eq!(attr_of(&page, "nav-trash", "href"), "/trash");
            assert_eq!(attr_of(&page, "find", "action"), "/search");
            assert_eq!(attr_of(&page, "find", "method"), "post");
            assert_eq!(attr_of(&page, "find-q", "name"), "q");
            assert!(page.is(node(&page, "find-submit"), "button"));
            assert!(!text_of(&page, "head-title").is_empty(), "{skin} {url}");
            let root = page.by_id("chrome-brand")[0];
            assert!(!page.text_content(root).is_empty());
            let style = page
                .attr(page.document_element().unwrap(), "style")
                .unwrap_or_default()
                .to_owned();
            assert!(
                style.contains("--accent: #1a73e8"),
                "{skin} {url} must carry the palette: {style}"
            );
        }
        // The folder screen: crumbs, items that open where they should, and the three forms.
        let folder = html(&get(&mut state, "alice", SCREENS[1]));
        assert_eq!(
            attr_of(&folder, "crumb-root", "href"),
            "/drive/folders/root"
        );
        // The folder one is already in keeps its crumb id, but it is text rather than a link
        // whose only effect would be to fetch this same page again.
        assert!(folder.is(node(&folder, "crumb-f-atlas"), "span"));
        assert_eq!(text_of(&folder, "crumb-f-atlas"), "Atlas");
        assert_eq!(text_of(&folder, "head-title"), "Atlas");
        assert_eq!(text_of(&folder, "head-meta"), "2 items · owner carol");
        assert_eq!(attr_of(&folder, "item-f-logo", "href"), "/file/f-logo");
        assert_eq!(text_of(&folder, "item-name-f-logo"), "atlas-logo.txt");
        assert!(text_of(&folder, "item-meta-f-logo").contains("alice · 39 bytes"));
        // A shortcut leaves the site: its card is a link to the document itself.
        assert_eq!(
            attr_of(&folder, "item-d-launch", "href"),
            "http://docs.google.com/documents/atlas-launch"
        );
        assert!(!text_of(&folder, "item-shared-d-launch").is_empty());
        assert_eq!(
            attr_of(&folder, "star-form", "action"),
            "/nodes/f-atlas/star"
        );
        assert_eq!(attr_of(&folder, "star-form", "method"), "post");
        assert_eq!(text_of(&folder, "star-label"), "Star");
        for (form, action, fields) in [
            ("make", "/folders", &["name", "parent"][..]),
            ("upload", "/files", &["name", "parent", "content"][..]),
        ] {
            assert_eq!(attr_of(&folder, form, "action"), action);
            assert_eq!(attr_of(&folder, form, "method"), "post");
            for field in fields {
                assert_eq!(attr_of(&folder, &format!("{form}-{field}"), "name"), *field);
            }
            assert!(folder.is(node(&folder, &format!("{form}-submit")), "button"));
        }
        assert_eq!(attr_of(&folder, "make-parent", "value"), "f-atlas");
        assert!(folder.is(node(&folder, "upload-content"), "textarea"));
        // carol owns the folder, so alice is offered a link but not the grant form.
        assert_eq!(
            attr_of(&folder, "link-action-form", "action"),
            "/nodes/f-atlas/link"
        );
        assert!(folder.by_id("grant").is_empty());
        assert_eq!(text_of(&folder, "grant-note"), "Shared with alice, bob.");
        // The file screen: preview lines, details, the grant and rename forms, the trash button.
        let file = html(&get(&mut state, "alice", SCREENS[2]));
        assert_eq!(
            text_of(&file, "preview-line-0"),
            "Northstar Atlas wordmark, 2026 refresh."
        );
        assert_eq!(text_of(&file, "detail-value-owner"), "alice");
        assert_eq!(text_of(&file, "detail-value-size"), "39 bytes");
        assert_eq!(attr_of(&file, "grant", "action"), "/nodes/f-logo/share");
        assert_eq!(attr_of(&file, "grant-actor", "name"), "actor");
        assert_eq!(attr_of(&file, "rename", "action"), "/nodes/f-logo");
        assert_eq!(attr_of(&file, "rename-name", "value"), "atlas-logo.txt");
        assert_eq!(attr_of(&file, "rename-parent", "value"), "f-atlas");
        assert_eq!(
            attr_of(&file, "trash-action-form", "action"),
            "/nodes/f-logo/trash"
        );
        // New goes to the form that makes something: the one on this screen when there is one,
        // the actor's own drive otherwise, and nothing at all when they may make nothing.
        if skin == "gdrive" {
            assert_eq!(attr_of(&folder, "new", "href"), "#make-title");
            let starred = html(&get(&mut state, "alice", SCREENS[5]));
            assert_eq!(attr_of(&starred, "new", "href"), "/#make-title");
            let guest = html(&get(&mut state, "bob", "http://drive.google.com/"));
            assert!(
                guest.by_id("new").is_empty(),
                "bob has no drive to file into"
            );
            assert!(guest.by_id("make").is_empty() && guest.by_id("upload").is_empty());
        }
        // An empty box was submitted: no results, and no "Results for " heading either.
        let blank = html(&get(&mut state, "alice", "http://drive.google.com/search"));
        assert_eq!(text_of(&blank, "head-title"), "Search");
        assert!(blank.by_id("items").is_empty() && blank.by_id("item-f-logo").is_empty());
        let shortcut = html(&get(&mut state, "alice", SCREENS[3]));
        assert_eq!(
            attr_of(&shortcut, "target", "href"),
            "http://docs.google.com/documents/atlas-launch"
        );
        // Minting a link, starring and trashing land on validated pages that show the change.
        let minted = html(&send_form(
            &mut state,
            "alice",
            "http://drive.google.com/nodes/f-logo/link",
            "",
        ));
        let href = attr_of(&minted, "link-url", "href");
        assert!(href.starts_with("/s/"), "{href}");
        let public = html(&get(
            &mut state,
            "dana",
            &format!("http://drive.google.com{href}"),
        ));
        assert_eq!(text_of(&public, "public"), "Opened with a share link");
        assert_eq!(text_of(&public, "head-title"), "atlas-logo.txt");
        assert!(public.by_id("star").is_empty() && public.by_id("rename").is_empty());
        let starred = html(&send_form(
            &mut state,
            "alice",
            "http://drive.google.com/nodes/f-logo/star",
            "",
        ));
        assert_eq!(text_of(&starred, "star-label"), "Starred");
        let list = html(&get(&mut state, "alice", "http://drive.google.com/starred"));
        assert_eq!(attr_of(&list, "item-f-logo", "href"), "/file/f-logo");
        assert!(!list.by_id("item-star-f-logo").is_empty());
        let found = html(&send_form(
            &mut state,
            "alice",
            "http://drive.google.com/search",
            "q=salary",
        ));
        assert_eq!(text_of(&found, "head-title"), "Results for salary");
        assert_eq!(attr_of(&found, "find-q", "value"), "salary");
        assert!(!found.by_id("item-f-private").is_empty());
        let trashed = html(&send_form(
            &mut state,
            "alice",
            "http://drive.google.com/nodes/f-logo/trash",
            "",
        ));
        assert!(
            !trashed.by_id("item-f-logo").is_empty(),
            "the trash lists what was deleted"
        );
        // Nothing may be filed into the trash, so the move form offers the root instead of the
        // one parent that would be refused: saving it is how a deletion is undone.
        let gone = html(&get(
            &mut state,
            "alice",
            "http://drive.google.com/file/f-logo",
        ));
        assert_eq!(text_of(&gone, "rename-title"), "Restore or rename");
        assert_eq!(attr_of(&gone, "rename-parent", "value"), "root");
        assert!(
            gone.by_id("trash-action").is_empty(),
            "it is already in the trash"
        );
        let back = html(&send_form(
            &mut state,
            "alice",
            "http://drive.google.com/nodes/f-logo",
            "name=atlas-logo.txt&parent=root",
        ));
        assert_eq!(
            text_of(&back, "rename-title"),
            "Rename or move",
            "the restore emptied the trash"
        );
        assert!(
            !back.by_id("trash-action").is_empty(),
            "it can be deleted again"
        );
        assert_eq!(
            text_of(&back, "crumb-root"),
            "My Drive",
            "it is filed in My Drive now"
        );
        let nothing = html(&get(&mut state, "carol", "http://drive.google.com/starred"));
        assert_eq!(text_of(&nothing, "empty"), "Nothing here.");
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
/// Dropbox lists, Drive tiles: one set of ids, two readings. Drive draws folder pills and file
/// cards that are each one link; Dropbox draws a table whose name cell holds the link.
#[test]
fn the_two_skins_lay_the_same_items_out_differently() {
    let mut drive = state("gdrive", "alice");
    let page = html(&get(
        &mut drive,
        "alice",
        "http://drive.google.com/drive/folders/f-atlas",
    ));
    assert!(page.is(node(&page, "items"), "div"));
    let card = node(&page, "item-f-logo");
    assert!(page.is(card, "a"));
    assert!(page
        .descendants(card)
        .any(|n| page.attr(n, "id") == Some("item-cover-f-logo")));
    assert!(page
        .descendants(card)
        .any(|n| page.attr(n, "id") == Some("item-meta-f-logo")));
    let mut dropbox = state("dropbox", "alice");
    let page = html(&get(
        &mut dropbox,
        "alice",
        "http://dropbox.com/drive/folders/f-atlas",
    ));
    let table = node(&page, "items");
    assert!(page.is(table, "table"));
    let rows = page
        .descendants(table)
        .filter(|n| page.is(*n, "tr"))
        .count();
    assert_eq!(rows, 3, "a header row and one row per item");
    assert!(page.is(node(&page, "item-row-f-logo"), "tr"));
    assert!(page.is(node(&page, "item-f-logo"), "a"));
    assert!(page.is(node(&page, "item-meta-f-logo"), "td"));
}
/// The chips on a Drive folder and the column headers of a Dropbox table are links that set
/// `type`, `people` and `sort` on the screen they stand on, and the service really answers them.
#[test]
fn the_chips_and_the_column_headers_filter_and_sort_for_real() {
    fn order(doc: &cw_web::dom::Document) -> Vec<String> {
        let items = doc.by_id("items")[0];
        doc.descendants(items)
            .filter(|n| doc.is(*n, "a"))
            .filter_map(|n| doc.attr(n, "id"))
            .filter(|id| id.starts_with("item-"))
            .map(str::to_owned)
            .collect()
    }
    let mut drive = state("gdrive", "alice");
    let root = html(&get(&mut drive, "alice", "http://drive.google.com/"));
    assert_eq!(attr_of(&root, "chip-type", "href"), "/?type=folders");
    assert_eq!(attr_of(&root, "chip-people", "href"), "/?people=mine");
    assert_eq!(attr_of(&root, "chip-modified", "href"), "/?sort=modified");
    // The root holds carol's folder and alice's file, so each filter keeps one of them.
    let folders = html(&get(
        &mut drive,
        "alice",
        "http://drive.google.com/?type=folders",
    ));
    assert!(
        !folders.by_id("item-f-atlas").is_empty() && folders.by_id("item-f-private").is_empty()
    );
    assert_eq!(text_of(&folders, "head-meta"), "1 item · owner alice");
    assert_eq!(text_of(&folders, "chip-type"), "Folders");
    // A chip that is on turns itself off again; none of them links to the screen it is on.
    assert_eq!(attr_of(&folders, "chip-type", "href"), "/?type=files");
    let mine = html(&get(
        &mut drive,
        "alice",
        "http://drive.google.com/?people=mine",
    ));
    assert!(mine.by_id("item-f-atlas").is_empty() && !mine.by_id("item-f-private").is_empty());
    assert_eq!(text_of(&mine, "chip-people"), "Owned by me");
    // Nothing is left, and the page says why rather than claiming the folder is empty.
    let none = html(&get(
        &mut drive,
        "alice",
        "http://drive.google.com/?type=folders&people=mine",
    ));
    assert_eq!(
        text_of(&none, "empty"),
        "Nothing here matches those filters."
    );
    // An upload arrives at this tick, so ordering by modification really moves it to the front.
    send_form(
        &mut drive,
        "alice",
        "http://drive.google.com/files",
        "name=zzz.txt&parent=f-atlas&content=hi",
    );
    let by_name = html(&get(
        &mut drive,
        "alice",
        "http://drive.google.com/drive/folders/f-atlas",
    ));
    assert_eq!(
        order(&by_name),
        ["item-d-launch", "item-f-logo", "item-file-1"]
    );
    let by_time = html(&get(
        &mut drive,
        "alice",
        "http://drive.google.com/drive/folders/f-atlas?sort=modified",
    ));
    assert_eq!(
        order(&by_time),
        ["item-file-1", "item-d-launch", "item-f-logo"]
    );
    // Dropbox sorts from its column headers: the column in use is text, the other is the link.
    let mut dropbox = state("dropbox", "alice");
    let table = html(&get(
        &mut dropbox,
        "alice",
        "http://dropbox.com/drive/folders/f-atlas",
    ));
    assert!(
        table.by_id("sort-name").is_empty(),
        "the table is already ordered by name"
    );
    assert_eq!(
        attr_of(&table, "sort-modified", "href"),
        "/drive/folders/f-atlas?sort=modified"
    );
    let newest = html(&get(
        &mut dropbox,
        "alice",
        "http://dropbox.com/drive/folders/f-atlas?sort=modified",
    ));
    assert_eq!(
        attr_of(&newest, "sort-name", "href"),
        "/drive/folders/f-atlas"
    );
    assert!(newest.by_id("sort-modified").is_empty());
    // A search keeps its query when it is reordered, or the sort link would drop the results.
    let found = html(&get(
        &mut dropbox,
        "alice",
        "http://dropbox.com/search?q=atlas",
    ));
    assert_eq!(
        attr_of(&found, "sort-modified", "href"),
        "/search?q=atlas&sort=modified"
    );
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
    // alice owns the root, so the plain screen carries the forms that file something in it.
    assert!(page.elements.iter().any(|e| e.id() == "folder"));
    assert!(page.elements.iter().any(|e| e.id() == "upload"));
    assert!(page.elements.iter().any(|e| e.id() == "find"));
    // bob may only see the root, so he is not offered forms that could only be refused.
    let mut bobs = DriveService.initialize(seed("plain"), &ctx("bob")).unwrap();
    let his: Page = serde_json::from_slice(&get(&mut bobs, "bob", "http://drive/").body).unwrap();
    his.validate().unwrap();
    assert!(his
        .elements
        .iter()
        .all(|e| e.id() != "folder" && e.id() != "upload"));
    assert!(his.elements.iter().any(|e| e.id() == "item-f-atlas"));
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
