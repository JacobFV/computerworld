//! The shipped seed data for the sites this package owns. A page that does not initialise,
//! does not render or is not reachable from its own search entry is a broken site, and the
//! search engines WP-1 builds would hand an agent a link that 404s.
mod support;
use cw_protocol::{HttpRequest, Page};
use cw_sdk::{Service, ServiceContext};
use cw_service_git::GitService;
use serde_json::Value;

fn ctx(actor: &str) -> ServiceContext {
    ServiceContext {
        actor: actor.into(),
        source: "alice-mac".into(),
        tick: 60,
        seed: 1,
        instance: "github".into(),
    }
}
/// A site file as the reference world has it.
fn site(name: &str) -> Value {
    cw_service_common::reference::reference_site(name)
}
fn open(state: &mut Value, url: &str) -> (u16, Vec<u8>) {
    let r = GitService
        .handle(state, &ctx("alice"), &HttpRequest::get(url))
        .unwrap();
    (r.status, r.body)
}

#[test]
fn github_seed_initialises_and_every_page_renders() {
    let site = site("github");
    assert_eq!(site["initial_state"]["skin"], "github");
    let mut state = GitService
        .initialize(site["initial_state"].clone(), &ctx("alice"))
        .expect("github seed initialises");
    const PATHS: [&str; 32] = [
        "/",
        "/northstar",
        "/northstar/atlas",
        "/northstar/atlas/issues",
        "/northstar/atlas/issues/14",
        "/northstar/atlas/pulls",
        "/northstar/atlas/pull/15",
        "/northstar/atlas/stargazers",
        "/northstar/atlas/blob/src/bfs.rs",
        "/northstar/atlas/blob/main/src/bfs.rs",
        "/northstar/atlas/blob/main/LICENSE",
        "/northstar/atlas/tree/main/src",
        "/northstar/atlas/tree/sort-refs/tests",
        "/northstar/atlas/commits/main",
        "/northstar/atlas/commits/sort-refs",
        "/northstar/atlas/branches",
        "/northstar/atlas/issues?state=closed",
        "/northstar/atlas/issues/new",
        "/northstar/atlas/issues/9",
        "/northstar/atlas/compare",
        "/northstar/atlas/pull/11",
        "/northstar/atlas/pull/15/commits",
        "/northstar/atlas/pull/15/files",
        "/northstar/atlas/pull/19",
        "/northstar/atlas/actions",
        "/northstar/atlas/settings",
        "/search?q=atlas",
        "/alicechen",
        "/northstar/atlas-actions",
        "/alicechen/dotfiles",
        "/gists",
        "/gist/bfs-order",
    ];
    for path in PATHS {
        let (status, body) = open(&mut state, &format!("http://github.com{path}"));
        assert_eq!(status, 200, "{path}: {}", String::from_utf8_lossy(&body));
        // Every page is strict HTML the engine renders, with unique ids.
        let page = support::Page::parse(path, String::from_utf8(body).unwrap());
        assert!(
            page.has("chrome") && page.has("mark"),
            "{path}: no site chrome"
        );
    }
    // The same data under the other two looks: strict HTML there too.
    for skin in ["gitlab", "plain"] {
        let mut initial = site["initial_state"].clone();
        initial["skin"] = skin.into();
        let mut other = GitService
            .initialize(initial, &ctx("alice"))
            .expect("github seed initialises");
        for path in PATHS {
            let (status, body) = open(&mut other, &format!("http://github.com{path}"));
            assert_eq!(
                status,
                200,
                "{skin} {path}: {}",
                String::from_utf8_lossy(&body)
            );
            support::Page::parse(&format!("{skin} {path}"), String::from_utf8(body).unwrap());
        }
    }
    // Every commit on every branch has a page of its own.
    let repos = state["repositories"].as_object().unwrap().clone();
    for (name, repo) in &repos {
        let owner = repo["owner"].as_str().unwrap();
        for sha in repo["objects"].as_object().unwrap().keys() {
            let url = format!("http://github.com/{owner}/{name}/commit/{sha}");
            let (status, body) = open(&mut state, &url);
            assert_eq!(status, 200, "{url}");
            let page = support::Page::parse(&url, String::from_utf8(body).unwrap());
            assert!(page.has("diff-summary"), "{url}: no diff");
        }
    }
    for entry in site["search_entries"].as_array().unwrap() {
        let url = entry["url"].as_str().unwrap();
        assert_eq!(open(&mut state, url).0, 200, "search entry {url}");
    }
    // Storyline 3 lands on GitHub as a real, mergeable branch, not as prose about one.
    assert!(state["repositories"]["atlas"]["refs"]["refs/heads/sort-refs"].is_string());
}

/// gitlab.com: the same service in the `gitlab` skin; every page of every project renders as
/// strict HTML, under the skin the seed names and under the other two.
#[test]
fn gitlab_seed_initialises_and_every_page_renders_in_every_skin() {
    let site = site("gitlab");
    assert_eq!(site["initial_state"]["skin"], "gitlab");
    for skin in ["gitlab", "github", "plain"] {
        let mut initial = site["initial_state"].clone();
        initial["skin"] = skin.into();
        let mut state = GitService
            .initialize(initial, &ctx("alice"))
            .expect("gitlab seed initialises");
        let repos = state["repositories"].as_object().unwrap().clone();
        let mut paths = vec![
            "/".to_owned(),
            "/search?q=replay".to_owned(),
            "/opensim".to_owned(),
        ];
        for (name, repo) in &repos {
            let owner = repo["owner"].as_str().unwrap();
            let base = format!("/{owner}/{name}");
            for tail in [
                "",
                "/issues",
                "/issues?state=closed",
                "/pulls",
                "/issues/new",
                "/compare",
                "/branches",
                "/commits/main",
                "/stargazers",
                "/wiki",
                "/pulse",
            ] {
                paths.push(format!("{base}{tail}"));
            }
            for sha in repo["objects"].as_object().unwrap().keys() {
                paths.push(format!("{base}/commit/{sha}"));
            }
            let tip = repo["refs"]["refs/heads/main"].as_str().unwrap();
            for file in repo["objects"][tip]["files"].as_object().unwrap().keys() {
                paths.push(format!("{base}/blob/main/{file}"));
            }
            for number in repo["issues"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(n, _)| n)
            {
                paths.push(format!("{base}/issues/{number}"));
            }
            paths.push(format!("/repos/{name}"));
        }
        for path in paths {
            let (status, body) = open(&mut state, &format!("http://gitlab.com{path}"));
            assert_eq!(
                status,
                200,
                "{skin} {path}: {}",
                String::from_utf8_lossy(&body)
            );
            support::Page::parse(&format!("{skin} {path}"), String::from_utf8(body).unwrap());
        }
        for entry in site["search_entries"].as_array().unwrap() {
            let url = entry["url"].as_str().unwrap();
            assert_eq!(open(&mut state, url).0, 200, "search entry {url}");
        }
    }
}

/// The four `static-site` seeds in this package. That crate is not ours, but the page data is.
#[test]
fn company_site_pages_validate_and_back_their_search_entries() {
    for name in ["northstar-www", "northstar-status", "intranet", "guide"] {
        let site = site(name);
        let pages = site["initial_state"]["pages"].as_object().unwrap();
        for (path, raw) in pages {
            let page: Page =
                serde_json::from_value(raw.clone()).unwrap_or_else(|e| panic!("{name}{path}: {e}"));
            page.validate()
                .unwrap_or_else(|e| panic!("{name}{path}: {e}"));
        }
        for entry in site["search_entries"].as_array().unwrap() {
            let url = url::Url::parse(entry["url"].as_str().unwrap()).unwrap();
            assert!(
                pages.contains_key(url.path()),
                "{name}: search entry {url} has no page"
            );
        }
    }
}

/// §5.1 freezes two things about the intranet: its title, and the ids `behaviors.rs` clicks.
#[test]
fn intranet_keeps_its_title_and_legacy_element_ids() {
    let site = site("intranet");
    let home = &site["initial_state"]["pages"]["/"];
    assert_eq!(home["title"], "Northstar Workshop");
    let page: Page = serde_json::from_value(home.clone()).unwrap();
    let mut ids = vec![];
    fn walk(elements: &[cw_protocol::PageElement], ids: &mut Vec<(String, String)>) {
        for element in elements {
            // A quick-link card carries its destination in an action, not in a Link.
            match element {
                cw_protocol::PageElement::Link { id, url, .. } => {
                    ids.push((id.clone(), url.clone()))
                }
                cw_protocol::PageElement::Card {
                    id,
                    action: Some(a),
                    ..
                }
                | cw_protocol::PageElement::Thumbnail {
                    id,
                    action: Some(a),
                    ..
                } => ids.push((id.clone(), a.url.clone())),
                _ => (),
            }
            match element {
                cw_protocol::PageElement::Row { children, .. }
                | cw_protocol::PageElement::Grid { children, .. }
                | cw_protocol::PageElement::Card { children, .. }
                | cw_protocol::PageElement::Group { children, .. }
                | cw_protocol::PageElement::Form { children, .. } => walk(children, ids),
                _ => (),
            }
        }
    }
    walk(&page.elements, &mut ids);
    for (id, url) in [
        ("handbook", "http://docs.internal/"),
        ("mail", "http://mail.internal/"),
        ("chat", "http://chat.internal/"),
        ("git", "http://git.internal/"),
        ("issues", "http://issues.internal/"),
        ("calendar", "http://calendar.internal/"),
        ("public-guide", "http://guide.example/"),
    ] {
        assert!(
            ids.contains(&(id.into(), url.into())),
            "intranet lost link {id} -> {url}"
        );
    }
    // §4.3: the home page must reach one page on each of these.
    let hosts = [
        "mail.google.com",
        "slack.com",
        "linear.app",
        "github.com",
        "drive.google.com",
        "calendar.google.com",
        "status.northstar.example",
    ];
    for host in hosts {
        assert!(
            ids.iter().any(|(_, u)| u.contains(host)),
            "intranet does not link {host}"
        );
    }
}
