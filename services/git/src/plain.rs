//! git.internal: the two pages the original surface always had, as plain HTML in the
//! manner of gitweb or cgit. `/` lists the repositories the actor can read (`repo-<name>`
//! links to `/repos/<name>`); `/repos/<name>` shows the refs (`ref-<refname>`) and the
//! files of the default branch (`file-<path>`), each with its content. No theme and no
//! chrome: a skinned instance serves these routes exactly the same way.
use cw_protocol::{HttpResponse, Result};
use cw_service_common::html::{self, div, el, link, span, Document};
use serde_json::{Map, Value};

const CSS: &str = include_str!("plain.css");

fn document(title: &str) -> Document {
    Document::new(title).lang("en").stylesheet(CSS).body_class("skin-plain")
}
fn masthead(trail: Option<&str>) -> html::Html {
    let mut bar = el("header").class("masthead").child(link("home", "/", "git").class("brand"));
    if let Some(name) = trail {
        bar = bar.child(span("sep").text("/")).child(span("here").text(name));
    }
    bar.child(span("tagline").text("repositories on this server"))
}
/// The landing page: one row per readable repository.
pub(crate) fn index(repos: &Map<String, Value>, visible: &[String]) -> Result<HttpResponse> {
    let mut table = el("table").class("list").child(
        el("thead").child(
            el("tr")
                .child(el("th").class("name").text("Name"))
                .child(el("th").text("Description"))
                .child(el("th").text("Owner"))
                .child(el("th").class("num").text("Commits")),
        ),
    );
    let mut rows = el("tbody");
    for name in visible {
        let repo = &repos[name];
        let text = |key: &str| repo[key].as_str().unwrap_or("").to_owned();
        let commits = repo["objects"].as_object().map_or(0, Map::len);
        rows = rows.child(
            el("tr")
                .child(el("td").class("name").child(link(&format!("repo-{name}"), format!("/repos/{name}"), name.as_str())))
                .child(el("td").class("muted").text(text("description")))
                .child(el("td").text(text("owner")))
                .child(el("td").class("num").text(commits.to_string())),
        );
    }
    table = table.child(rows);
    let doc = document("Git repositories").body([
        masthead(None),
        el("main")
            .child(el("h1").id("title").text("Git repositories"))
            .child(table)
            .when(visible.is_empty(), |n| n.child(el("p").id("empty").class("muted").text("No repositories."))),
        el("footer").text("Clone over HTTP: git clone http://<host>/repos/<name>"),
    ]);
    html::page(&doc)
}
/// One repository: its refs, and the default branch's files with their content.
pub(crate) fn repository(name: &str, repo: &Value) -> Result<HttpResponse> {
    let mut main = el("main").child(el("h1").id("repo-title").text(name));
    let refs = repo["refs"].as_object();
    let mut list = el("table").class("list refs").child(el("thead").child(el("tr").child(el("th").text("Ref")).child(el("th").text("Commit"))));
    let mut rows = el("tbody");
    for (refname, id) in refs.into_iter().flatten() {
        rows = rows.child(
            el("tr")
                .id(format!("ref-{refname}"))
                .child(el("td").class("mono").text(refname.as_str()))
                // A space keeps "name id" readable as text, the way the row always read.
                .child(el("td").class("mono muted").text(format!(" {}", id.as_str().unwrap_or("")))),
        );
    }
    list = list.child(rows);
    main = main.child(el("h2").text("Refs")).child(list);
    // The default branch: main when there is one, the first ref otherwise.
    let head = refs.and_then(|r| r.get("refs/heads/main").or_else(|| r.values().next())).and_then(Value::as_str);
    if let Some(files) = head.and_then(|id| repo["objects"][id]["files"].as_object()) {
        main = main.child(el("h2").text("Files"));
        for (path, content) in files {
            main = main.child(
                el("section")
                    .id(format!("file-{path}"))
                    .class("file")
                    .child(div("file-name mono").text(path.as_str()))
                    .child(el("pre").text(format!("\n{}", content.as_str().unwrap_or("")))),
            );
        }
    }
    let doc = document(&format!("Repository {name}")).body([
        masthead(Some(name)),
        main,
        el("footer").text(format!("git clone http://<host>/repos/{name}")),
    ]);
    html::page(&doc)
}
