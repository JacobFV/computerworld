//! Real open-source web apps as sites on the internet (worlds/oss-web,
//! docs/oss-webapps.md), used end to end through the agent API the way an agent
//! uses any site: browser.v1 to navigate, click, fill and press keys, and the
//! semantic.v1 observation to read what the page shows. Every page here is the
//! app's own build running on the engine, and every API call it makes is answered
//! by the app's own backend running on the in-world JavaScript VM.
//!
//!     cargo test -p computerworld --features oss-web --test oss_webapps
#![cfg(feature = "oss-web")]

use computerworld::World;
use cw_protocol::{ActionEnvelope, EnvironmentConfig, WorldDefinition};
use serde_json::{json, Value};

const MACHINE: &str = "workstation";

fn definition() -> WorldDefinition {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../worlds/oss-web/world.json"
    ))
    .unwrap();
    WorldDefinition::from_json(&text).unwrap()
}

struct Agent {
    world: World,
    session: String,
    /// The same session on the React build (the browser's compiled-app path off),
    /// acted on alongside and compared after every action.
    twin: Option<Box<Agent>>,
}

impl Agent {
    /// An agent whose every action also runs on a second world where pages run
    /// their React build; after each, the two must show the same elements.
    fn twinned() -> Agent {
        let mut a = Agent::new();
        a.twin = Some(Box::new(Agent::new()));
        a
    }

    /// The twin takes the same action, and must show what this agent shows.
    fn mirror(&mut self, what: &str, f: impl FnOnce(&mut Agent)) {
        let Some(twin) = self.twin.as_mut() else {
            return;
        };
        cw_browser::page_script::set_compiled_apps(false);
        f(twin);
        cw_browser::page_script::set_compiled_apps(true);
        let (mine, theirs) = (self.dump(), self.twin.as_ref().unwrap().dump());
        if mine != theirs {
            if let Some(dir) = std::env::var_os("CW_OSS_DUMPS") {
                let dir = std::path::PathBuf::from(dir);
                std::fs::write(dir.join("compiled.txt"), &mine).ok();
                std::fs::write(dir.join("react.txt"), &theirs).ok();
            }
            let line = mine
                .lines()
                .zip(theirs.lines())
                .position(|(a, b)| a != b)
                .unwrap_or(mine.lines().count().min(theirs.lines().count()));
            panic!(
                "after {what}, the compiled page (left) and its React build (right) differ at line {}:\n{}\n---\n{}\n--- compiled console\n{}",
                line + 1,
                mine.lines().skip(line.saturating_sub(2)).take(6).collect::<Vec<_>>().join("\n"),
                theirs.lines().skip(line.saturating_sub(2)).take(6).collect::<Vec<_>>().join("\n"),
                self.console(),
            );
        }
    }

    fn new() -> Agent {
        let mut world = World::new(definition(), 42).unwrap();
        let session = world
            .environment(EnvironmentConfig {
                actor: "ada".into(),
                machines: vec![MACHINE.into()],
                actions: vec!["browser.v1".into(), "keyboard.v1".into(), "http.v1".into()],
                observations: vec!["semantic.v1".into(), "browser.v1".into()],
                action_budget: 1 << 20,
            })
            .unwrap();
        Agent {
            world,
            session,
            twin: None,
        }
    }

    fn act(&mut self, channel: &str, op: &str, payload: Value) -> Value {
        cw_browser::page_script::set_compiled_apps(true);
        let v = self.act_here(channel, op, payload.clone());
        self.mirror(&format!("{op} {payload}"), |t| {
            t.act_here(channel, op, payload);
        });
        v
    }

    fn act_here(&mut self, channel: &str, op: &str, payload: Value) -> Value {
        let result = self
            .world
            .step(
                &self.session,
                vec![ActionEnvelope::new(channel, op, MACHINE, payload.clone())],
            )
            .unwrap();
        assert!(
            result.outcomes[0].success,
            "{op} {payload}: {:?}\non {}",
            result.outcomes[0],
            self.dump()
        );
        result.outcomes[0].value.clone()
    }

    /// Lets `ms` of world time pass, then takes an empty step: what the environment
    /// does between an agent's actions, where page timers (a fake provider's
    /// latency, a debounce) get their turn.
    fn wait(&mut self, ms: u64) {
        self.world.runtime_mut().advance(ms * 1000).unwrap();
        self.world.step(&self.session, vec![]).unwrap();
        self.mirror(&format!("waiting {ms} ms"), |t| {
            t.world.runtime_mut().advance(ms * 1000).unwrap();
            t.world.step(&t.session, vec![]).unwrap();
        });
    }

    /// Waits in half-second steps of world time, as an agent looks again, until the
    /// page shows `text` (at most ten seconds).
    fn wait_for(&mut self, text: &str) {
        for _ in 0..20 {
            if self.shows(text) {
                return;
            }
            self.wait(500);
        }
        panic!(
            "{text:?} never appeared on {}:\n{}\n--- console\n{}",
            self.url(),
            self.dump(),
            self.console()
        );
    }

    /// Waits, as `wait_for` does, until the page no longer shows `text`.
    fn wait_until_gone(&mut self, text: &str) {
        for _ in 0..20 {
            if !self.shows(text) {
                return;
            }
            self.wait(500);
        }
        panic!(
            "{text:?} is still on {}:\n{}\n--- console\n{}",
            self.url(),
            self.dump(),
            self.console()
        );
    }

    /// Navigates with the browser's compiled-app path as it is set on this thread.
    fn navigate_here(&mut self, url: &str) {
        self.act_here("browser.v1", "navigate", json!({ "url": url }));
    }

    fn navigate(&mut self, url: &str) {
        self.act("browser.v1", "navigate", json!({ "url": url }));
    }

    fn page(&self) -> Value {
        self.world.observe(&self.session).unwrap().channels["semantic.v1"][MACHINE].clone()
    }

    fn url(&self) -> String {
        let b = self.world.observe(&self.session).unwrap().channels["browser.v1"][MACHINE].clone();
        b["url"].as_str().unwrap_or_default().to_owned()
    }

    /// What the tab's pages wrote to the console.
    fn console(&self) -> String {
        let b = self.world.observe(&self.session).unwrap().channels["browser.v1"][MACHINE].clone();
        b["console"]
            .as_array()
            .map(|l| {
                l.iter()
                    .map(|e| {
                        format!(
                            "{}: {}",
                            e["level"].as_str().unwrap_or(""),
                            e["text"].as_str().unwrap_or("")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default()
    }

    fn elements(&self) -> Vec<Value> {
        fn walk(elements: &[Value], out: &mut Vec<Value>) {
            for e in elements {
                out.push(e.clone());
                if let Some(children) = e["children"].as_array() {
                    walk(children, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(self.page()["elements"].as_array().unwrap(), &mut out);
        out
    }

    fn dump(&self) -> String {
        self.elements()
            .iter()
            .map(|e| {
                format!(
                    "{} {:?} {}",
                    e["kind"].as_str().unwrap_or(""),
                    e["text"]
                        .as_str()
                        .or(e["label"].as_str())
                        .or(e["placeholder"].as_str())
                        .unwrap_or(""),
                    e["id"].as_str().unwrap_or("")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Everything the page says, one element per line.
    fn text(&self) -> String {
        self.elements()
            .iter()
            .filter_map(|e| {
                e["text"]
                    .as_str()
                    .or(e["value"].as_str())
                    .map(str::to_owned)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn shows(&self, needle: &str) -> bool {
        self.text().contains(needle)
    }

    /// The first element of `kind` whose text (or label, or placeholder) is `name`.
    fn find(&self, kind: &str, name: &str) -> Option<Value> {
        self.elements().into_iter().find(|e| {
            e["kind"] == kind
                && [&e["text"], &e["label"], &e["placeholder"], &e["name"]]
                    .iter()
                    .any(|v| v.as_str().is_some_and(|t| t.trim() == name))
        })
    }

    fn id_of(&self, kind: &str, name: &str) -> String {
        match self.find(kind, name) {
            Some(e) => e["id"].as_str().unwrap().to_owned(),
            None => panic!(
                "no {kind} {name:?} on {}:\n{}\n--- console\n{}",
                self.url(),
                self.dump(),
                self.console()
            ),
        }
    }

    fn click(&mut self, kind: &str, name: &str) {
        let id = self.id_of(kind, name);
        self.act("browser.v1", "click", json!({ "id": id }));
    }

    fn fill(&mut self, name: &str, value: &str) {
        let id = self.id_of("input", name);
        self.act("browser.v1", "fill", json!({ "id": id, "value": value }));
    }

    /// Types into a field the way a person does, key by key (no `change` until the
    /// field is committed), after clicking into it.
    fn type_into(&mut self, name: &str, text: &str) {
        let id = self.id_of("input", name);
        self.act("browser.v1", "click", json!({ "id": id }));
        self.act("keyboard.v1", "type", json!({ "text": text }));
    }

    fn key(&mut self, key: &str) {
        self.act("browser.v1", "key", json!({ "key": key }));
    }

    fn http(&mut self, method: &str, url: &str, body: Option<Value>) -> (u16, Value) {
        let mut req = json!({"method": method, "url": url, "headers": {"accept": "application/json"}, "body": []});
        if let Some(b) = body {
            req["headers"]["content-type"] = json!("application/json");
            req["body"] = json!(serde_json::to_vec(&b).unwrap());
        }
        let r = self.act("http.v1", "request", req);
        let bytes: Vec<u8> = serde_json::from_value(r["body"].clone()).unwrap();
        let v = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
        (r["status"].as_u64().unwrap() as u16, v)
    }
}

/// Prints the semantic tree of `OSS_URL` (development aid).
#[test]
#[ignore]
fn smoke() {
    let mut a = Agent::new();
    let url = std::env::var("OSS_URL").unwrap();
    let t = std::time::Instant::now();
    a.navigate(&url);
    eprintln!("navigate took {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);
    eprintln!("{}", a.dump());
}

/// TodoMVC, twice: the React and the Vue builds are the same product, driven the
/// same way. Add three todos, complete one, filter by the hash routes, clear the
/// completed one, and delete another.
fn todomvc(url: &str) {
    let mut a = Agent::new();
    a.navigate(url);
    assert!(a.shows("todos"));
    let input = "What needs to be done?";
    for todo in [
        "Review the CI caching PR",
        "Reply to Wren",
        "Book the offsite room",
    ] {
        a.fill(input, todo);
        a.key("Enter");
    }
    for todo in [
        "Review the CI caching PR",
        "Reply to Wren",
        "Book the offsite room",
    ] {
        assert!(a.shows(todo), "{todo} was not added:\n{}", a.dump());
    }
    assert!(a.shows("3 items left"), "{}", a.text());
    // Each todo's checkbox, which the semantic tree shows as `[ ]` or `[x]`.
    let checkbox = |a: &Agent, n: usize| -> String {
        a.elements()
            .into_iter()
            .find(|e| {
                e["text"].as_str().is_some_and(|t| t == "[ ]" || t == "[x]")
                    && e["id"].as_str().unwrap_or("").contains(&format!("li[{n}]"))
            })
            .unwrap_or_else(|| panic!("no checkbox for todo {n}:\n{}", a.dump()))["id"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let first = checkbox(&a, 1);
    a.act("browser.v1", "click", json!({ "id": first }));
    assert!(a.shows("2 items left"), "{}", a.text());
    a.click("link", "Active");
    assert!(a.url().ends_with("#/active"), "{}", a.url());
    assert!(!a.shows("Review the CI caching PR"));
    assert!(a.shows("Reply to Wren"));
    a.click("link", "Completed");
    assert!(a.shows("Review the CI caching PR"));
    assert!(!a.shows("Reply to Wren"));
    a.click("link", "All");
    a.click("button", "Clear completed");
    assert!(!a.shows("Review the CI caching PR"));
    assert!(a.shows("2 items left"));
}

#[test]
fn todomvc_react_adds_completes_filters_and_clears() {
    todomvc("http://todomvc.com/examples/react/dist/");
}

/// TodoMVC React runs on cw-ui in the world: its page names the app compiled from
/// its sources (`data-cw-ui`, scripts/oss-web/build.sh), and the browser runs that
/// instead of the React build. The same flow on the compiled page and on the React
/// build's page (`react.html`), side by side: after every action the two pages
/// show the agent the same elements.
#[test]
fn todomvc_react_compiled_shows_what_its_react_build_shows() {
    let mut compiled = Agent::new();
    let mut react = Agent::new();
    compiled.navigate("http://todomvc.com/examples/react/dist/");
    react.navigate("http://todomvc.com/examples/react/dist/react.html");
    let runs_compiled = |a: &Agent| {
        a.world.observe(&a.session).unwrap().channels["browser.v1"][MACHINE]["compiled"]
            == json!(true)
    };
    assert!(runs_compiled(&compiled), "the page did not run compiled");
    assert!(!runs_compiled(&react));
    let same = |c: &Agent, r: &Agent, what: &str| {
        let (a, b) = (c.dump(), r.dump());
        assert_eq!(
            a, b,
            "after {what}: compiled (left) and React (right) differ"
        );
    };
    same(&compiled, &react, "loading");
    let input = "What needs to be done?";
    for todo in [
        "Review the CI caching PR",
        "Reply to Wren",
        "Book the offsite room",
    ] {
        for a in [&mut compiled, &mut react] {
            a.fill(input, todo);
            a.key("Enter");
        }
        same(&compiled, &react, todo);
    }
    let first = |a: &Agent| -> String {
        a.elements()
            .into_iter()
            .find(|e| {
                e["text"].as_str().is_some_and(|t| t == "[ ]" || t == "[x]")
                    && e["id"].as_str().unwrap_or("").contains("li[1]")
            })
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    for a in [&mut compiled, &mut react] {
        let id = first(a);
        a.act("browser.v1", "click", json!({ "id": id }));
    }
    same(&compiled, &react, "completing the first");
    for (kind, name) in [
        ("link", "Active"),
        ("link", "Completed"),
        ("link", "All"),
        ("button", "Clear completed"),
    ] {
        for a in [&mut compiled, &mut react] {
            a.click(kind, name);
        }
        same(&compiled, &react, name);
        assert_eq!(
            compiled.url(),
            react.url().replace("react.html", ""),
            "after {name}"
        );
    }
    assert!(compiled.shows("2 items left"));
}

#[test]
fn todomvc_vue_adds_completes_filters_and_clears() {
    todomvc("http://todomvc.com/examples/vue/dist/");
}

/// Conduit, the RealWorld blogging platform, in React/Redux: the global feed from
/// the seeded API, a tag filter, signing in, writing, commenting on, editing and
/// deleting an article, and favouriting one. The API behind it is gothinkster's
/// Express backend running in the world.
#[test]
fn conduit_react_filters_signs_in_writes_comments_edits_deletes_and_favourites() {
    // Compiled (the page's app.ui.json on cw-ui), with the React build alongside:
    // after every action both show the agent the same page.
    let mut a = Agent::twinned();
    a.navigate("http://conduit.realworld.show/");
    assert!(a.shows("A place to share your knowledge."));
    assert!(
        a.shows("Stop writing ETL, start writing contracts"),
        "{}",
        a.dump()
    );
    // The tag filter: the feed narrows to one tag's articles.
    a.click("button", "reliability");
    assert!(
        a.shows("I deleted our retry logic and the outages stopped"),
        "{}",
        a.dump()
    );
    assert!(!a.shows("Spacing is a system"), "{}", a.dump());
    // Sign in with the account in ~/accounts.txt.
    a.click("link", "Sign in");
    a.fill("Email", "ada@okonkwo.dev");
    a.fill("Password", "conduit-ada-2026");
    a.click("button", "Sign in");
    assert!(
        a.find("link", "ada").is_some(),
        "not signed in:\n{}",
        a.dump()
    );
    assert!(a.find("button", "Your Feed").is_some(), "{}", a.dump());
    // Write an article.
    a.click("link", "New Post");
    a.fill(
        "Article Title",
        "What we learned moving CI to a content-addressed cache",
    );
    a.fill(
        "What's this article about?",
        "Six months of build numbers, and the two mistakes.",
    );
    let body_id = a
        .elements()
        .into_iter()
        .find(|e| {
            e["kind"] == "input"
                && e["placeholder"]
                    .as_str()
                    .is_some_and(|p| p.starts_with("Write your article"))
        })
        .unwrap_or_else(|| panic!("no body field:\n{}", a.dump()))["id"]
        .as_str()
        .unwrap()
        .to_owned();
    a.act("browser.v1", "fill", json!({"id": body_id, "value": "We measured every build for six months.\n\nThe cache paid for itself in the second week."}));
    // A tag: this editor adds one on Enter's keyup. Its form has three text fields
    // and no submit button, so Enter does not submit it (implicit submission).
    a.type_into("Enter tags", "caching");
    a.key("Enter");
    assert!(a.shows("caching"), "{}", a.dump());
    a.click("button", "Publish Article");
    // This frontend goes home after publishing; the article leads the global feed.
    // (Its profile pages never render: the route is "/@:username", which React
    // Router 6 does not treat as a parameter, in Chromium as here.)
    a.click("button", "Global Feed");
    let article = a
        .elements()
        .into_iter()
        .find(|e| {
            e["kind"] == "link"
                && e["text"]
                    .as_str()
                    .is_some_and(|t| t.starts_with("What we learned moving CI"))
        })
        .unwrap_or_else(|| {
            panic!(
                "the new article is not in the feed:\n{}\n--- console\n{}",
                a.dump(),
                a.console()
            )
        })["id"]
        .as_str()
        .unwrap()
        .to_owned();
    a.act("browser.v1", "click", json!({ "id": article }));
    assert!(
        a.url().contains("/article/What-we-learned-moving-CI"),
        "{}\n{}",
        a.url(),
        a.dump()
    );
    assert!(
        a.shows("The cache paid for itself in the second week."),
        "{}",
        a.dump()
    );
    // Comment on it.
    a.fill(
        "Write a comment...",
        "Numbers for the Windows runners are coming next week.",
    );
    a.click("button", "Post Comment");
    assert!(
        a.shows("Numbers for the Windows runners are coming next week."),
        "{}",
        a.dump()
    );
    // Edit it.
    a.click("link", "Edit Article");
    a.fill(
        "Article Title",
        "What we learned moving CI to a content-addressed build cache",
    );
    a.click("button", "Publish Article");
    a.click("button", "Global Feed");
    assert!(
        a.shows("What we learned moving CI to a content-addressed build cache"),
        "{}",
        a.dump()
    );
    // The API holds what the page shows.
    let (status, v) = a.http(
        "GET",
        "http://api.realworld.show/api/articles?author=ada",
        None,
    );
    assert_eq!(status, 200);
    assert!(
        v.to_string().contains("content-addressed build cache"),
        "{v}"
    );
    assert!(v.to_string().contains("\"caching\""), "{v}");
    // Delete it from its page; the feed no longer lists it.
    let article = a
        .elements()
        .into_iter()
        .find(|e| {
            e["kind"] == "link"
                && e["text"]
                    .as_str()
                    .is_some_and(|t| t.starts_with("What we learned moving CI"))
        })
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    a.act("browser.v1", "click", json!({ "id": article }));
    assert!(
        a.shows("Numbers for the Windows runners are coming next week."),
        "{}",
        a.dump()
    );
    a.click("button", "Delete Article");
    a.navigate("http://conduit.realworld.show/");
    a.click("button", "Global Feed");
    assert!(!a.shows("content-addressed"), "{}", a.dump());
    // Favourite someone else's article from the feed: the heart button on its card.
    a.click("button", "Global Feed");
    let card = a
        .elements()
        .into_iter()
        .find(|e| {
            e["kind"] == "link"
                && e["text"]
                    .as_str()
                    .is_some_and(|t| t.starts_with("Idempotency keys"))
        })
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .trim_end_matches("/a[1]")
        .to_owned();
    let heart = a
        .elements()
        .into_iter()
        .find(|e| e["kind"] == "button" && e["id"].as_str().is_some_and(|id| id.starts_with(&card)))
        .unwrap_or_else(|| panic!("no favourite button on {card}:\n{}", a.dump()));
    assert_eq!(heart["text"], "0");
    a.act("browser.v1", "click", json!({ "id": heart["id"] }));
    let after = a
        .elements()
        .into_iter()
        .find(|e| e["id"] == heart["id"])
        .unwrap();
    assert_eq!(after["text"], "1", "{}", a.dump());
    let (_, v) = a.http(
        "GET",
        "http://api.realworld.show/api/articles/Idempotency-keys-explained-with-a-coffee-order-2",
        None,
    );
    assert_eq!(v["article"]["favoritesCount"], 1, "{v}");
}

/// The same product in Vue 3 against the same API: what one frontend writes the
/// other reads.
#[test]
fn conduit_vue_filters_signs_in_writes_follows_and_updates_settings() {
    let mut a = Agent::new();
    a.navigate("http://vue.realworld.show/");
    assert!(
        a.shows("Stop writing ETL, start writing contracts"),
        "{}",
        a.dump()
    );
    a.click("link", "Sign in");
    a.fill("Email", "ada@okonkwo.dev");
    a.fill("Password", "conduit-ada-2026");
    a.click("button", "Sign in");
    assert!(
        a.find("link", "ada").is_some(),
        "not signed in:\n{}\n{}",
        a.dump(),
        a.console()
    );
    // Tag filter.
    a.click("link", "reliability");
    assert!(a.shows("The queue that ate our Tuesdays"), "{}", a.dump());
    assert!(!a.shows("Passkeys for the rest of us"), "{}", a.dump());
    // Write an article, tags included (this editor adds one on Enter).
    a.click("link", "New Post");
    a.fill("Title", "Flaky tests are a budget, not a bug list");
    a.fill(
        "Description",
        "How we decided which flaky tests to fix first.",
    );
    a.fill(
        "Body",
        "Every flaky test costs reruns. We priced them.\n\nThen we fixed the expensive ones.",
    );
    a.type_into("Tags", "ci");
    a.key("Enter");
    a.type_into("Tags", "testing");
    a.key("Enter");
    a.click("button", "Publish Article");
    assert!(
        a.url()
            .contains("/article/Flaky-tests-are-a-budget-not-a-bug-list-1"),
        "{}\n{}\n{}",
        a.url(),
        a.dump(),
        a.console()
    );
    assert!(a.shows("Then we fixed the expensive ones."), "{}", a.dump());
    let (_, v) = a.http(
        "GET",
        "http://api.realworld.show/api/articles/Flaky-tests-are-a-budget-not-a-bug-list-1",
        None,
    );
    // Exactly the two tags: the emptied field is not committed on blur (its value
    // is what it was when the last tag was added), so no empty tag.
    let mut tags: Vec<String> = v["article"]["tagList"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_str().unwrap().to_owned())
        .collect();
    tags.sort();
    assert_eq!(tags, ["ci", "testing"], "{v}");
    // Someone else's article: favourite it and follow its author from its page.
    a.navigate("http://vue.realworld.show/#/article/Stop-writing-ETL-start-writing-contracts-4");
    assert!(
        a.shows("We now publish a schema for every table"),
        "{}",
        a.dump()
    );
    let button = |a: &Agent, prefix: &str| -> String {
        a.elements()
            .into_iter()
            .find(|e| {
                e["kind"] == "button"
                    && e["text"]
                        .as_str()
                        .is_some_and(|t| t.trim().starts_with(prefix))
            })
            .unwrap_or_else(|| panic!("no {prefix:?} button:\n{}", a.dump()))["id"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    // The article's own buttons carry their state in their accessible names.
    let fav = button(&a, "Favorite article");
    a.act("browser.v1", "click", json!({ "id": fav }));
    assert!(
        a.find("button", "Unfavorite article").is_some(),
        "{}",
        a.dump()
    );
    let (_, v) = a.http(
        "GET",
        "http://api.realworld.show/api/articles/Stop-writing-ETL-start-writing-contracts-4",
        None,
    );
    assert_eq!(v["article"]["favoritesCount"], 1, "{v}");
    // Favouriting answered with the article as ada sees it: she follows priya.
    // Unfollow her from here.
    let unfollow = button(&a, "Unfollow");
    a.act("browser.v1", "click", json!({ "id": unfollow }));
    assert!(a.find("button", "Follow").is_some(), "{}", a.dump());
    let (_, login) = a.http(
        "POST",
        "http://api.realworld.show/api/users/login",
        Some(json!({"user": {"email": "ada@okonkwo.dev", "password": "conduit-ada-2026"}})),
    );
    let token = login["user"]["token"].as_str().unwrap().to_owned();
    let r = a.act("http.v1", "request", json!({"method": "GET", "url": "http://api.realworld.show/api/profiles/priya.natarajan", "headers": {"authorization": format!("Token {token}")}, "body": []}));
    let profile: Value =
        serde_json::from_slice(&serde_json::from_value::<Vec<u8>>(r["body"].clone()).unwrap())
            .unwrap();
    assert_eq!(profile["profile"]["following"], false, "{profile}");
    // Settings: a new bio shows on the profile.
    a.click("link", "Settings");
    let bio = a
        .elements()
        .into_iter()
        .find(|e| {
            e["kind"] == "input"
                && e["value"]
                    .as_str()
                    .is_some_and(|v| v.starts_with("Platform engineer"))
        })
        .unwrap_or_else(|| panic!("no bio field:\n{}", a.dump()))["id"]
        .as_str()
        .unwrap()
        .to_owned();
    a.act(
        "browser.v1",
        "fill",
        json!({ "id": bio, "value": "Platform engineer. Now also chasing flaky tests." }),
    );
    a.click("button", "Update Settings");
    a.navigate("http://vue.realworld.show/#/profile/ada");
    assert!(a.shows("Now also chasing flaky tests."), "{}", a.dump());
    assert!(
        a.shows("Flaky tests are a budget, not a bug list"),
        "{}",
        a.dump()
    );
}

/// react-admin's demo: its post list over the fake REST provider (with its 300 ms
/// latency, which passes as world time), full-text search, editing a post, creating
/// one and deleting one with the undoable delete's grace period.
#[test]
fn react_admin_lists_searches_edits_creates_and_deletes_posts() {
    let mut a = Agent::new();
    a.navigate("http://react-admin.marmelab.com/");
    a.wait_for("Writing incident reviews people read");
    assert!(a.shows("1-10 of 13"), "{}", a.dump());
    // Full-text search, debounced as the user types.
    a.type_into("Search", "queue");
    a.wait_until_gone("Passkeys six months in");
    assert!(
        a.shows("Choosing a queue for background work"),
        "{}",
        a.dump()
    );
    // Edit the post the search found.
    a.click("link", "Edit");
    a.wait_for("Choosing a queue for background work");
    assert!(a.url().contains("#/posts/11"), "{}", a.url());
    a.fill("Title *", "Choosing a queue for background jobs");
    a.click("button", "Save");
    a.wait_for("Choosing a queue for background jobs");
    // Create a tag.
    a.click("link", "TagsG T");
    a.wait_for("Technology");
    a.click("link", "Create");
    a.wait_for("Create Tag");
    a.fill("Name *", "Observability");
    // The name is required in every locale the form edits.
    a.click("button", "Fr");
    a.fill("Name *", "Observabilité");
    a.click("button", "Save");
    a.wait(1500);
    assert!(a.url().ends_with("#/tags"), "{}\n{}", a.url(), a.dump());
    a.wait_for("Observability");
    // Delete a post from its edit page; the delete is undoable, then final.
    a.click("link", "PostsG P");
    // The list remembers its search; clear it.
    a.wait_for("Choosing a queue for background jobs");
    a.click("button", "Clear value");
    a.wait_for("Writing incident reviews people read");
    a.click("link", "Edit");
    a.wait_for("Post \"Writing incident reviews people read\"");
    a.click("button", "Delete");
    a.wait_until_gone("Writing incident reviews people read");
    a.wait(6000);
    assert!(a.shows("1-10 of 12"), "{}", a.dump());
    assert!(
        !a.shows("Writing incident reviews people read"),
        "{}",
        a.dump()
    );
}

/// JSON Server: its home page lists the seeded resources; the REST API it serves
/// from one JSON file answers the browser and HTTP clients alike, with filters,
/// full-text search, and writes that later reads see.
#[test]
fn json_server_serves_browses_searches_and_writes_its_resources() {
    let mut a = Agent::new();
    a.navigate("http://json-server.typicode.com/");
    a.wait_for("Congrats!");
    for resource in ["/books", "/projects", "/tasks", "/profile"] {
        assert!(
            a.find("link", resource).is_some(),
            "{resource}:\n{}",
            a.dump()
        );
    }
    a.click("link", "/tasks");
    assert!(a.url().ends_with("/tasks"), "{}", a.url());
    assert!(a.shows("Split the test job by package"), "{}", a.dump());
    // Writes from an HTTP client.
    let (status, created) = a.http(
        "POST",
        "http://json-server.typicode.com/tasks",
        Some(json!({"projectId": 1, "title": "Cache the Docker layers too", "done": false, "due": "2026-09-30"})),
    );
    assert_eq!(
        (status, created["id"].clone()),
        (201, json!(6)),
        "{created}"
    );
    let (status, _) = a.http(
        "PATCH",
        "http://json-server.typicode.com/tasks/3",
        Some(json!({"done": true})),
    );
    assert_eq!(status, 200);
    let (status, _) = a.http("DELETE", "http://json-server.typicode.com/books/5", None);
    assert_eq!(status, 200);
    // Reads in the browser see them: a filter, an embed and a search.
    a.navigate("http://json-server.typicode.com/tasks?done=false");
    assert!(a.shows("Cache the Docker layers too"), "{}", a.dump());
    assert!(!a.shows("Split the test job by package"), "{}", a.dump());
    a.navigate("http://json-server.typicode.com/projects/1?_embed=tasks");
    assert!(a.shows("Cache the Docker layers too"), "{}", a.dump());
    a.navigate("http://json-server.typicode.com/books?q=Kleppmann");
    assert!(
        a.shows("Designing Data-Intensive Applications"),
        "{}",
        a.dump()
    );
    assert!(!a.shows("Crafting Interpreters"), "{}", a.dump());
    let (_, books) = a.http("GET", "http://json-server.typicode.com/books", None);
    assert_eq!(books.as_array().unwrap().len(), 4, "{books}");
}

fn rss_kib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmRSS:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|n| n.parse().ok())
        })
        .unwrap_or(0)
}

/// Per-app numbers for docs/oss-webapps.md, in a fresh world each: the wall time of
/// the navigation and of the world time it then takes for the first screen's content
/// to show, the process's resident memory before and after, and the exported
/// snapshot's size before the visit and after it. Run in release:
///
///     cargo test --release -p computerworld --features oss-web --test oss_webapps measure -- --ignored --nocapture --test-threads 1
#[test]
#[ignore]
fn measure() {
    let apps = [
        (
            "TodoMVC React",
            "http://todomvc.com/examples/react/dist/",
            "Double-click to edit a todo",
        ),
        (
            "TodoMVC Vue",
            "http://todomvc.com/examples/vue/dist/",
            "Double-click to edit a todo",
        ),
        (
            "Conduit React",
            "http://conduit.realworld.show/",
            "Stop writing ETL, start writing contracts",
        ),
        (
            "Conduit Vue",
            "http://vue.realworld.show/",
            "Stop writing ETL, start writing contracts",
        ),
        (
            "react-admin",
            "http://react-admin.marmelab.com/",
            "Writing incident reviews people read",
        ),
        ("JSON Server", "http://json-server.typicode.com/", "/books"),
    ];
    println!("| App | first load: navigate (ms) | first load: until content (ms) | world time waited (ms) | second load: until content (ms) | RSS before → after (MiB) | snapshot before → after (bytes) |");
    println!("|---|---|---|---|---|---|---|");
    for (name, url, content) in apps {
        let mut a = Agent::new();
        let before_snapshot = a.world.export_snapshot().unwrap().len();
        let rss_before = rss_kib();
        let t = std::time::Instant::now();
        a.navigate(url);
        let nav = t.elapsed().as_secs_f64() * 1000.0;
        let mut waited = 0;
        while !a.shows(content) && waited < 10_000 {
            a.wait(100);
            waited += 100;
        }
        assert!(a.shows(content), "{name}: {}", a.dump());
        let total = t.elapsed().as_secs_f64() * 1000.0;
        let rss_after = rss_kib();
        let after_snapshot = a.world.export_snapshot().unwrap().len();
        // The same page again in the same process: the scripts' compiled code is cached.
        let t = std::time::Instant::now();
        a.navigate("http://todomvc.com/license.md");
        a.navigate(url);
        let mut waited_again = 0;
        while !a.shows(content) && waited_again < 10_000 {
            a.wait(100);
            waited_again += 100;
        }
        let again = t.elapsed().as_secs_f64() * 1000.0;
        println!(
            "| {name} | {nav:.0} | {total:.0} | {waited} | {again:.0} | {:.0} → {:.0} | {before_snapshot} → {after_snapshot} |",
            rss_before as f64 / 1024.0,
            rss_after as f64 / 1024.0
        );
    }
}

/// The compiled apps against their React builds, each in a fresh world in one
/// process (the React build: the browser's compiled-app path off, as Chrome sees
/// the page): first and second load and one action, medians of five.
///
///     cargo test --release -p computerworld --features oss-web --test oss_webapps -- --ignored --nocapture measure_compiled
#[test]
#[ignore]
fn measure_compiled_against_react() {
    let median = |mut v: Vec<f64>| {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[v.len() / 2]
    };
    type Act = fn(&mut Agent);
    let apps: [(&str, &str, &str, &str, Act); 2] = [
        (
            "TodoMVC React",
            "http://todomvc.com/examples/react/dist/",
            "todos",
            "add a todo",
            |a| {
                a.fill("What needs to be done?", "Reply to Wren");
                a.key("Enter");
                assert!(a.shows("1 item left"));
            },
        ),
        (
            "Conduit React",
            "http://conduit.realworld.show/",
            "Stop writing ETL, start writing contracts",
            "filter by a tag",
            |a| {
                a.click("button", "reliability");
                assert!(a.shows("I deleted our retry logic and the outages stopped"));
            },
        ),
    ];
    println!("| App | runs | first load (ms) | second load (ms) | action | action (ms) | snapshot after (bytes) |");
    println!("|---|---|---|---|---|---|---|");
    for (name, url, content, action, act) in apps {
        for compiled in [true, false] {
            let (mut first, mut second, mut acted, mut snap) = (vec![], vec![], vec![], vec![]);
            for _ in 0..5 {
                cw_browser::page_script::set_compiled_apps(compiled);
                let mut a = Agent::new();
                let t = std::time::Instant::now();
                a.navigate_here(url);
                first.push(t.elapsed().as_secs_f64() * 1000.0);
                assert!(a.shows(content), "{name}: {}", a.dump());
                let t = std::time::Instant::now();
                a.navigate_here("http://todomvc.com/license.md");
                a.navigate_here(url);
                second.push(t.elapsed().as_secs_f64() * 1000.0);
                let t = std::time::Instant::now();
                act(&mut a);
                acted.push(t.elapsed().as_secs_f64() * 1000.0);
                snap.push(a.world.export_snapshot().unwrap().len() as f64);
            }
            cw_browser::page_script::set_compiled_apps(true);
            println!(
                "| {name} | {} | {:.1} | {:.1} | {action} | {:.1} | {:.0} |",
                if compiled {
                    "compiled (cw-ui)"
                } else {
                    "React build"
                },
                median(first),
                median(second),
                median(acted),
                median(snap)
            );
        }
    }
}
