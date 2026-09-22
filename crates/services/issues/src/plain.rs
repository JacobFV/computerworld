//! issues.internal: the unbranded tracker as plain HTML, in the manner of a bare Trac or
//! Bugzilla. `/` lists the projects (`project-<KEY>`), a project lists its issues
//! (`issue-<id>`) above the `new-issue` form, and an issue shows `title`, `status`, `body`,
//! its `comment-<n>` and `review-<n>` lines and the `comment`, `update` and `review` forms,
//! whose inputs are `<form>-<field>` and whose buttons are `<form>-submit`. The masthead
//! carries the `search` form (`search-q`, `search-go`) and `/search` answers it with
//! `result-<KEY>-<id>` links. The breadcrumb of a page you are not on is the link `crumb`;
//! the crumb of the page you are already on is text, not a link back to itself.
use crate::{Issue, Project};
use cw_protocol::{HttpResponse, Result};
use cw_service_common::html::{
    self, button, div, el, form, label, link, span, text_input, Document, Html,
};

const CSS: &str = include_str!("plain.css");

/// The masthead, its breadcrumb and the search box. A crumb with no url is the page being
/// read: it is rendered as text, since a link to the page you are on goes nowhere.
fn document(
    title: &str,
    query: &str,
    trail: &[(&str, Option<String>)],
    main: Html,
) -> Result<HttpResponse> {
    let mut bar = el("header")
        .class("masthead")
        .child(link("home", "/", "Issues").class("brand"));
    for (text, url) in trail {
        bar = bar.child(span("sep").text("/")).child(match url {
            Some(url) => el("a").id("crumb").attr("href", url.as_str()).text(*text),
            None => span("here").text(*text),
        });
    }
    bar = bar.child(
        form("search", "/search", "get")
            .class("search")
            .child(
                text_input("search-q", "q", query)
                    .attr("placeholder", "Search issues")
                    .attr("aria-label", "Search issues"),
            )
            .child(button("search-go", "Search")),
    );
    html::page(
        &Document::new(title)
            .lang("en")
            .stylesheet(CSS)
            .body_class("skin-plain")
            .body([bar, main]),
    )
}
/// `<form id>` posting to `url` with one labelled text field per `(key, label, value)`.
fn fields(id: &str, url: &str, title: &str, inputs: &[(&str, &str, &str)]) -> Html {
    let mut node = form(id, url, "post")
        .class("panel")
        .child(el("h2").text(title));
    for (key, text, value) in inputs {
        let input = format!("{id}-{key}");
        node = node.child(
            div("field")
                .child(label(&input, *text))
                .child(text_input(&input, key, value)),
        );
    }
    node.child(button(&format!("{id}-submit"), "Submit"))
}
pub(crate) fn projects(visible: &[(String, String)]) -> Result<HttpResponse> {
    let list = el("ul").class("list").each(visible.iter(), |(id, name)| {
        el("li")
            .child(link(
                &format!("project-{id}"),
                format!("/projects/{id}"),
                name.as_str(),
            ))
            .child(span("muted").text(format!(" {id}")))
    });
    document(
        "Projects",
        "",
        &[],
        el("main")
            .child(el("h1").id("title").text("Projects"))
            .child(list),
    )
}
pub(crate) fn project(key: &str, project: &Project) -> Result<HttpResponse> {
    let name = if project.name.is_empty() {
        key
    } else {
        &project.name
    };
    let mut table = el("table").class("issues").child(
        el("thead").child(
            el("tr")
                .child(el("th").text("Id"))
                .child(el("th").text("Summary"))
                .child(el("th").text("Status"))
                .child(el("th").text("Assignee")),
        ),
    );
    let mut rows = el("tbody");
    for (id, item) in &project.issues {
        rows = rows.child(
            el("tr")
                .child(el("td").class("mono").text(format!("{key}-{id}")))
                .child(el("td").child(link(
                    &format!("issue-{id}"),
                    format!("/projects/{key}/issues/{id}"),
                    item.title.as_str(),
                )))
                .child(
                    el("td").child(
                        span("status")
                            .class(&format!("s-{}", item.status))
                            .text(item.status.as_str()),
                    ),
                )
                .child(el("td").text(item.assignee.as_str())),
        );
    }
    table = table.child(rows);
    let main = el("main")
        .child(el("h1").id("project").text(name))
        .child(table)
        .child(fields(
            "new-issue",
            &format!("/projects/{key}/issues"),
            "New issue",
            &[("title", "Title", ""), ("body", "Description", "")],
        ));
    document(&format!("Project {key}"), "", &[(key, None)], main)
}
pub(crate) fn issue(project: &str, issue: &Issue) -> Result<HttpResponse> {
    let base = format!("/projects/{project}/issues/{}", issue.id);
    let mut main = el("main")
        .child(link("back", format!("/projects/{project}"), "Project").class("back"))
        .child(
            el("h1")
                .id("title")
                .text(format!("{project}-{} {}", issue.id, issue.title)),
        )
        .child(
            el("p")
                .id("status")
                .class("meta")
                .text(match issue.assignee.as_str() {
                    "" => format!("Status: {} · Unassigned", issue.status),
                    who => format!("Status: {} · Assigned: {who}", issue.status),
                }),
        )
        .child(el("p").id("body").class("body").text(&issue.body));
    for (i, c) in issue.comments.iter().enumerate() {
        main = main.child(
            el("p")
                .id(format!("comment-{i}"))
                .class("note")
                .text(format!("{}: {}", c.author, c.body)),
        );
    }
    for (i, r) in issue.reviews.iter().enumerate() {
        main = main.child(
            el("p")
                .id(format!("review-{i}"))
                .class("note review")
                .text(format!("{}: {} {}", r.author, r.decision, r.body)),
        );
    }
    main = main
        .child(fields(
            "comment",
            &format!("{base}/comments"),
            "Add a comment",
            &[("body", "Comment", "")],
        ))
        .child(fields(
            "update",
            &base,
            "Update",
            &[
                ("status", "Status", &issue.status),
                ("assignee", "Assignee", &issue.assignee),
            ],
        ));
    if issue.kind == "pull_request" {
        main = main.child(fields(
            "review",
            &format!("{base}/reviews"),
            "Review",
            &[("decision", "Decision", "approve"), ("body", "Review", "")],
        ));
    }
    document(
        &issue.title,
        "",
        &[(project, Some(format!("/projects/{project}")))],
        main,
    )
}
/// `/search`: every issue of every readable project that matches, as one list.
pub(crate) fn results(
    found: &[(String, Issue)],
    query: &str,
    assignee: Option<&str>,
) -> Result<HttpResponse> {
    let searching = !query.trim().is_empty() || assignee.is_some();
    let heading = match assignee {
        Some(who) if query.trim().is_empty() => format!("Issues assigned to {who}"),
        _ => "Search".to_owned(),
    };
    let summary = if !searching {
        "Type a word into the search box to find issues across every project.".to_owned()
    } else if found.is_empty() {
        "No issue matches.".to_owned()
    } else {
        format!(
            "{} {} found.",
            found.len(),
            if found.len() == 1 { "issue" } else { "issues" }
        )
    };
    let list = el("ul").class("list").each(found.iter(), |(key, item)| {
        el("li")
            .child(link(
                &format!("result-{key}-{}", item.id),
                format!("/projects/{key}/issues/{}", item.id),
                format!("{key}-{}: {}", item.id, item.title),
            ))
            .child(span("muted").text(format!(" {}", item.status)))
    });
    let main = el("main")
        .child(el("h1").id("title").text(&heading))
        .child(el("p").id("search-sub").class("meta").text(summary))
        .child(list);
    document(&heading, query, &[("Search", None)], main)
}
