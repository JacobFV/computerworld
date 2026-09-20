//! linear.app as HTML: the workspace sidebar, the projects table at `/`, a team's issues as
//! Linear's grouped list (the default), a board (`?view=board`) or the current cycle
//! (`?view=cycle`), and the issue page with its activity feed and properties column. The
//! stylesheet is `linear.css` next to this file. The plain skin never reaches this module.
//!
//! Element ids are the agent API and are the ones the `Page` version used: `workspace`,
//! `nav-<KEY>`, `team-<KEY>`, `col-<status>` (a status group, in the list and on the board),
//! `card-<id>` (an issue, as a row or as a card), `card-open-<id>` (its link),
//! `move-<id>-<status>` (the real status writes), `filter-all`, `filter-<who>`, `new-issue`
//! with `new-issue-title`, `new-issue-body`, `new-issue-submit`; on the issue page `back`,
//! `issue-key`, `issue-state`, `issue-title`, `body`, `comment-<n>`, `review-<n>`, the forms
//! `comment` (`comment-body`, `comment-submit`), `update` (`update-assignee`,
//! `update-submit`) and `review`, and the buttons `status-<status>`.
use crate::{Issue, Project};
use cw_protocol::{HttpResponse, PageTheme, Result};
use cw_service_common as wire;
use cw_service_common::html::{self, button, div, el, form, hidden, href, label, link, span, text_input, Document, Html};

const CSS: &str = include_str!("linear.css");

/// The four statuses the domain allows, in board order, with their column names.
pub const COLUMNS: [(&str, &str); 4] = [("open", "Todo"), ("in_progress", "In Progress"), ("blocked", "Blocked"), ("closed", "Done")];
pub fn column_label(status: &str) -> &'static str {
    COLUMNS.iter().find(|(id, _)| *id == status).map(|(_, label)| *label).unwrap_or("Todo")
}
/// Presentation context carried past the mutable borrow of the service state.
pub struct Look {
    pub workspace: String,
    pub theme: Option<PageTheme>,
}
impl Look {
    fn brand(&self) -> &str {
        if self.workspace.is_empty() {
            "Linear"
        } else {
            &self.workspace
        }
    }
}
/// How a team's issues are laid out: Linear's list, the board, or the current cycle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    List,
    Board,
    Cycle,
}
impl View {
    pub fn parse(s: Option<&str>) -> View {
        match s {
            Some("board") => View::Board,
            Some("cycle") => View::Cycle,
            _ => View::List,
        }
    }
    fn name(self) -> &'static str {
        match self {
            View::List => "list",
            View::Board => "board",
            View::Cycle => "cycle",
        }
    }
}

fn idn(node: Html, id: &str) -> Html {
    if id.is_empty() {
        node
    } else {
        node.id(id)
    }
}
fn sp(id: &str, class: &str, s: impl Into<String>) -> Html {
    idn(span(class), id).text(s)
}
/// A round avatar tinted from the name, showing its initial; never a real photo.
fn avatar(id: &str, who: &str, size: u32) -> Html {
    let initial: String = who.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
    let node = idn(span("avatar"), id).class(&format!("s{size}")).attr("title", if who.is_empty() { "Unassigned" } else { who });
    if who.is_empty() {
        node.class("nobody")
    } else {
        node.style(&format!("background: {}", wire::avatar_tint(who))).text(initial)
    }
}
/// The status ring: empty for Todo, half for In Progress, a bar for Blocked, a tick for Done.
fn status_icon(id: &str, status: &str) -> Html {
    idn(span("st"), id).class(&format!("st-{status}")).attr("title", column_label(status)).attr("data-status", status)
}
/// Priority, read from the `p0`..`p3` and `incident` labels the seed carries.
fn priority(issue: &Issue) -> (&'static str, u8) {
    let has = |l: &str| issue.labels.iter().any(|x| x.eq_ignore_ascii_case(l));
    if has("p0") || has("incident") || has("urgent") {
        ("Urgent", 4)
    } else if has("p1") {
        ("High", 3)
    } else if has("p2") {
        ("Medium", 2)
    } else if has("p3") {
        ("Low", 1)
    } else {
        ("No priority", 0)
    }
}
fn priority_icon(id: &str, issue: &Issue) -> Html {
    let (name, level) = priority(issue);
    let node = idn(span("prio"), id).class(&format!("prio-{level}")).attr("title", name);
    match level {
        4 => node.text("!"),
        0 => node.child(el("i")).child(el("i")).child(el("i")).class("none"),
        n => node.each(1..=3u8, |bar| el("i").class(if bar <= n { "on" } else { "" })),
    }
}
/// A label pill: a dot tinted from the name, then the name.
fn label_pill(id: &str, name: &str) -> Html {
    idn(span("pill"), id).child(el("i").class("dot").style(&format!("background: {}", wire::avatar_tint(name)))).child(Html::from(name))
}
fn label_pills(prefix: &str, issue: &Issue) -> Vec<Html> {
    issue.labels.iter().enumerate().map(|(i, l)| label_pill(&format!("{prefix}-label-{i}"), l)).collect()
}
/// Text with its `http(s)` URLs as links, `<prefix>-link-<word index>`.
fn linked(prefix: &str, body: &str) -> Vec<Html> {
    let mut out = vec![];
    let mut plain = String::new();
    let mut index = 0usize;
    let mut rest = body;
    while !rest.is_empty() {
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        if end == 0 {
            let ws = rest.find(|c: char| !c.is_whitespace()).unwrap_or(rest.len());
            plain.push_str(&rest[..ws]);
            rest = &rest[ws..];
            continue;
        }
        let word = &rest[..end];
        rest = &rest[end..];
        let url = word.trim_end_matches(['.', ',', ';', ')', ']']);
        if url::Url::parse(url).ok().is_some_and(|u| matches!(u.scheme(), "http" | "https")) {
            if !plain.is_empty() {
                out.push(Html::from(std::mem::take(&mut plain)));
            }
            out.push(link(&format!("{prefix}-link-{index}"), url, url));
            plain.push_str(&word[url.len()..]);
        } else {
            plain.push_str(word);
        }
        index += 1;
    }
    if !plain.is_empty() {
        out.push(Html::from(plain));
    }
    out
}

/// The workspace sidebar. The wordmark is a real link home; the team rows are the teams.
fn rail(look: &Look, projects: &[(String, String)], current: Option<&str>, view: View) -> Html {
    let initial: String = look.brand().chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
    let mut nav = el("nav").class("teams").child(sp("teams-label", "rail-label", "Your teams"));
    for (key, name) in projects {
        let here = current == Some(key.as_str());
        nav = nav.child(
            el("a")
                .id(format!("nav-{key}"))
                .class(if here { "team current" } else { "team" })
                .attr("href", format!("/projects/{key}"))
                .child(span("team-icon").style(&format!("background: {}", wire::avatar_tint(key))).text(key.chars().next().map(String::from).unwrap_or_default()))
                .child(sp(&format!("nav-{key}-text"), "team-name", name.as_str()))
                .child(span("team-key").text(key.as_str())),
        );
        if here {
            let sub = |id: &str, text: &str, v: View| {
                let params = [("view", v.name())];
                link(&format!("nav-{key}-{id}"), href(&format!("/projects/{key}"), if v == View::List { &[] } else { &params }), text)
                    .class(if v == view { "team-sub current" } else { "team-sub" })
            };
            nav = nav.child(sub("issues", "Issues", View::List)).child(sub("board", "Board", View::Board)).child(sub("cycle", "Cycles", View::Cycle));
        }
    }
    el("aside")
        .id("rail")
        .class("rail")
        .child(
            div("rail-top")
                .child(el("a").id("workspace").class("workspace").attr("href", "/").child(span("workspace-logo").text(initial)).child(Html::from(look.brand())).child(span("chev").text("▾")))
                .child(span("rail-tool").attr("title", "Search").child(span("mag")))
                .child(span("rail-tool compose").attr("title", "New issue").text("✎")),
        )
        .child(
            div("rail-links")
                .child(span("rail-item").child(span("glyph").text("✉")).child(Html::from("Inbox")))
                .child(span("rail-item").child(span("glyph").text("◎")).child(Html::from("My issues"))),
        )
        .child(
            div("rail-links")
                .child(sp("", "rail-label", "Workspace"))
                .child(link("nav-projects", "/", "Projects").class(if current.is_none() { "rail-item current" } else { "rail-item" }))
                .child(span("rail-item").text("Views")),
        )
        .child(nav)
}
fn document(look: &Look, title: &str, rail: Html, main: Html) -> Result<HttpResponse> {
    let mut doc = Document::new(title).lang("en").stylesheet(CSS).body_class("skin-linear").body([div("shell").id("shell").child(rail).child(main)]);
    if let Some(theme) = &look.theme {
        let vars: Vec<String> = [("--accent", &theme.accent), ("--paper", &theme.background), ("--surface", &theme.surface), ("--ink", &theme.ink), ("--muted", &theme.muted)]
            .iter()
            .filter_map(|(k, v)| v.as_ref().map(|v| format!("{k}: {v}")))
            .collect();
        if !vars.is_empty() {
            doc = doc.root_style(&vars.join("; "));
        }
    }
    html::page(&doc)
}
fn progress(done: usize, total: usize) -> Html {
    let percent = (done * 100).checked_div(total).unwrap_or(0);
    span("progress").child(span("bar").child(el("i").style(&format!("width: {percent}%")))).child(span("percent").text(format!("{percent}%")))
}
/// Workspace home: Linear's projects table, one row per team the actor can read.
pub fn home(look: &Look, projects: &[(String, Project)]) -> Result<HttpResponse> {
    let names: Vec<(String, String)> = projects.iter().map(|(k, p)| (k.clone(), p.name.clone())).collect();
    let mut table = div("table").id("teams").child(
        div("tr head")
            .child(span("c-name").text("Name"))
            .child(span("c-health").text("Health"))
            .child(span("c-lead").text("Lead"))
            .child(span("c-open").text("Issues"))
            .child(span("c-progress").text("Status")),
    );
    for (key, project) in projects {
        let total = project.issues.len();
        let done = project.issues.values().filter(|i| i.status == "closed").count();
        let blocked = project.issues.values().filter(|i| i.status == "blocked").count();
        // The lead is whoever carries the most issues; ties go to the first name.
        let mut load: std::collections::BTreeMap<&str, usize> = Default::default();
        for issue in project.issues.values().filter(|i| !i.assignee.is_empty()) {
            *load.entry(issue.assignee.as_str()).or_default() += 1;
        }
        let lead = load.iter().max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0))).map(|(who, _)| *who).unwrap_or("");
        table = table.child(
            el("a")
                .id(format!("team-{key}"))
                .class("tr")
                .attr("href", format!("/projects/{key}"))
                .child(
                    span("c-name")
                        .child(span("team-icon").style(&format!("background: {}", wire::avatar_tint(key))).text(key.chars().next().map(String::from).unwrap_or_default()))
                        .child(sp(&format!("team-name-{key}"), "strong", &project.name))
                        .child(sp(&format!("team-key-{key}"), "muted", key.as_str())),
                )
                .child(span("c-health").child(span(if blocked > 0 { "health at-risk" } else { "health" })).child(Html::from(if blocked > 0 { "At risk" } else { "On track" })))
                .child(span("c-lead").child(avatar("", lead, 18)).child(Html::from(if lead.is_empty() { "No lead" } else { lead })))
                .child(span("c-open").child(sp(&format!("team-open-{key}"), "count", format!("{} unfinished", total - done))))
                .child(span("c-progress").child(progress(done, total))),
        );
    }
    let main = el("main")
        .id("main")
        .class("main")
        .child(el("header").class("topbar").child(el("h1").id("home-title").text("Projects")).child(span("tabs").child(span("tab current").text("All projects"))).child(span("grow")).child(span("ghost").text("Filter")).child(span("ghost").text("Display")))
        .child(el("p").id("home-sub").class("sub").text("Your teams: boards, cycles and everything still open."))
        .child(table);
    document(look, &format!("{} · Linear", look.brand()), rail(look, &names, None, View::List), main)
}
/// The real moves available to an issue: a status write that lands back on this view.
fn moves(key: &str, issue: &Issue, view: View) -> Html {
    let id = issue.id;
    let at = COLUMNS.iter().position(|(s, _)| *s == issue.status);
    let mut row = span("moves").id(format!("card-moves-{id}"));
    for (delta, arrow) in [(-1i32, "←"), (1, "→")] {
        let Some(next) = at.and_then(|i| usize::try_from(i as i32 + delta).ok()).and_then(|i| COLUMNS.get(i)) else {
            continue;
        };
        row = row.child(
            form(&format!("move-{id}-{}-form", next.0), format!("/projects/{key}/issues/{id}"), "post")
                .class("inline")
                .child(hidden("status", next.0))
                .child(hidden("view", "board"))
                .child(hidden("layout", view.name()))
                .child(button(&format!("move-{id}-{}", next.0), format!("{arrow} {}", next.1)).class("move")),
        );
    }
    row
}
/// One issue as a list row: priority, key, status, title, then labels, assignee and moves.
fn list_row(key: &str, issue: &Issue, view: View) -> Html {
    let id = issue.id;
    div("row")
        .id(format!("card-{id}"))
        .child(priority_icon(&format!("card-priority-{id}"), issue))
        .child(sp(&format!("card-key-{id}"), "key", format!("{key}-{id}")))
        .child(status_icon(&format!("card-state-{id}"), &issue.status))
        .child(el("a").id(format!("card-open-{id}")).class("title").attr("href", format!("/projects/{key}/issues/{id}")).child(sp(&format!("card-title-{id}"), "", &issue.title)))
        .child(span("grow"))
        .child(span("pills").id(format!("card-head-{id}")).children(label_pills(&format!("card-{id}"), issue)))
        .child(moves(key, issue, view))
        .child(span("who").id(format!("card-who-{id}")).child(avatar(&format!("card-avatar-{id}"), &issue.assignee, 18)).child(sp(&format!("card-assignee-{id}"), "assignee", &issue.assignee)))
}
/// One issue as a board card.
fn board_card(key: &str, issue: &Issue, view: View) -> Html {
    let id = issue.id;
    div("card")
        .id(format!("card-{id}"))
        .child(div("card-top").id(format!("card-head-{id}")).child(sp(&format!("card-key-{id}"), "key", format!("{key}-{id}"))).child(span("grow")).child(avatar(&format!("card-avatar-{id}"), &issue.assignee, 18)))
        .child(
            div("card-title")
                .child(status_icon(&format!("card-state-{id}"), &issue.status))
                .child(el("a").id(format!("card-open-{id}")).class("title").attr("href", format!("/projects/{key}/issues/{id}")).child(sp(&format!("card-title-{id}"), "", &issue.title))),
        )
        .child(div("card-foot").child(priority_icon(&format!("card-priority-{id}"), issue)).children(label_pills(&format!("card-{id}"), issue)).child(sp(&format!("card-assignee-{id}"), "assignee hidden", &issue.assignee)))
        .child(moves(key, issue, view))
}
/// A team's issues, grouped by status: the list, the board or the current cycle, filtered
/// to one assignee when asked.
pub fn board(look: &Look, key: &str, project: &Project, projects: &[(String, String)], assignee: Option<&str>, view: View) -> Result<HttpResponse> {
    let visible: Vec<&Issue> = project
        .issues
        .values()
        .filter(|i| assignee.is_none_or(|who| i.assignee == who))
        // The cycle is the work in flight: everything started, plus what is assigned.
        .filter(|i| view != View::Cycle || i.status != "open" || !i.assignee.is_empty())
        .collect();
    let mut groups = div(if view == View::Board { "board" } else { "list" }).id("board");
    for (status, name) in COLUMNS {
        let members: Vec<&&Issue> = visible.iter().filter(|i| i.status == status).collect();
        let mut group = el("section").id(format!("col-{status}")).class(if view == View::Board { "column" } else { "group" }).child(
            div("group-head")
                .id(format!("col-head-{status}"))
                .child(status_icon(&format!("col-dot-{status}"), status))
                .child(span("group-name").text(name))
                .child(sp(&format!("col-count-{status}"), "count", members.len().to_string()))
                .child(span("grow"))
                .child(span("plus").text("+")),
        );
        for issue in members {
            group = group.child(if view == View::Board { board_card(key, issue, view) } else { list_row(key, issue, view) });
        }
        groups = groups.child(group);
    }
    // Assignee filters are links, so the filtered view is a real, shareable URL.
    let mut people: Vec<&str> = project.issues.values().map(|i| i.assignee.as_str()).filter(|a| !a.is_empty()).collect();
    people.sort_unstable();
    people.dedup();
    let to = |who: Option<&str>, v: View| {
        let mut params: Vec<(&str, &str)> = vec![];
        if let Some(who) = who {
            params.push(("assignee", who));
        }
        if v != View::List {
            params.push(("view", v.name()));
        }
        href(&format!("/projects/{key}"), &params)
    };
    let mut filters = div("filters").id("filters").child(span("ghost").text("Filter")).child(link("filter-all", to(None, view), "All").class(if assignee.is_none() { "chip current" } else { "chip" }));
    for who in people {
        filters = filters.child(
            el("a")
                .id(format!("filter-{who}"))
                .class(if assignee == Some(who) { "chip current" } else { "chip" })
                .attr("href", to(Some(who), view))
                .child(avatar("", who, 14))
                .child(Html::from(who)),
        );
    }
    let tab = |id: &str, text: &str, v: View| link(id, to(assignee, v), text).class(if v == view { "tab current" } else { "tab" });
    let total = project.issues.len();
    let done = project.issues.values().filter(|i| i.status == "closed").count();
    let started = project.issues.values().filter(|i| i.status == "in_progress" || i.status == "blocked").count();
    let mut main = el("main").id("main").class("main").child(
        el("header")
            .class("topbar")
            .id("board-head")
            .child(span("team-icon").style(&format!("background: {}", wire::avatar_tint(key))).text(key.chars().next().map(String::from).unwrap_or_default()))
            .child(el("h1").id("board-title").text(&project.name))
            .child(sp("board-key", "key", key))
            .child(span("tabs").child(tab("view-list", "All issues", View::List)).child(tab("view-board", "Board", View::Board)).child(tab("view-cycle", "Current cycle", View::Cycle)))
            .child(span("grow"))
            .child(span("ghost").text("Display")),
    );
    if view == View::Cycle {
        main = main.child(
            div("cycle")
                .id("cycle")
                .child(div("cycle-title").child(el("h2").id("cycle-title").text("Cycle 12")).child(span("pill current").text("Current")).child(span("muted").text("Two weeks · ends Friday")))
                .child(
                    div("cycle-stats")
                        .child(span("stat").child(span("muted").text("Scope")).child(sp("cycle-scope", "strong", total.to_string())))
                        .child(span("stat").child(span("muted").text("Started")).child(sp("cycle-started", "strong", started.to_string())))
                        .child(span("stat").child(span("muted").text("Completed")).child(sp("cycle-done", "strong", done.to_string())))
                        .child(progress(done, total)),
                ),
        );
    }
    main = main.child(filters).child(groups).child(
        el("section")
            .class("composer")
            .child(el("h2").id("new-title").text("New issue"))
            .child(
                form("new-issue", format!("/projects/{key}/issues"), "post")
                    .class("new-issue")
                    .child(text_input("new-issue-title", "title", "").attr("placeholder", "Issue title").attr("aria-label", "Title"))
                    .child(el("textarea").id("new-issue-body").attr("name", "body").attr("rows", "3").attr("placeholder", "Add description…").attr("aria-label", "Description"))
                    .child(div("actions").child(button("new-issue-submit", "Create issue").class("primary"))),
            ),
    );
    document(look, &format!("{} · {}", project.name, look.brand()), rail(look, projects, Some(key), view), main)
}
/// One issue: the description and the activity feed, with the properties column beside it.
pub fn issue(look: &Look, key: &str, item: &Issue, projects: &[(String, String)]) -> Result<HttpResponse> {
    let id = item.id;
    let base = format!("/projects/{key}/issues/{id}");
    let team = projects.iter().find(|(k, _)| k == key).map_or(key, |(_, n)| n.as_str());
    let mut feed = div("feed").id("activity").child(
        div("event")
            .child(avatar("", &item.author, 18))
            .child(sp("issue-author", "strong", &item.author))
            .child(Html::from(if item.kind == "pull_request" { " opened the pull request" } else { " created the issue" })),
    );
    if !item.source_ref.is_empty() {
        feed = feed.child(div("event").child(span("glyph").text("⎇")).child(sp("issue-refs", "mono", format!("{} → {}", item.source_ref, item.target_ref))));
    }
    for (i, c) in item.comments.iter().enumerate() {
        feed = feed.child(
            div("comment")
                .id(format!("comment-{i}"))
                .child(
                    div("comment-head")
                        .id(format!("comment-head-{i}"))
                        .child(avatar(&format!("comment-avatar-{i}"), &c.author, 20))
                        .child(sp(&format!("comment-author-{i}"), "strong", &c.author))
                        .child(sp(&format!("comment-tick-{i}"), "muted", format!("tick {}", c.tick))),
                )
                .child(div("prose").id(format!("comment-body-{i}")).children(linked(&format!("comment-body-{i}"), &c.body))),
        );
    }
    for (i, r) in item.reviews.iter().enumerate() {
        feed = feed.child(
            div("event")
                .id(format!("review-{i}"))
                .child(avatar("", &r.author, 18))
                .child(sp(&format!("review-state-{i}"), if r.decision == "approve" { "pill approve" } else { "pill" }, &r.decision))
                .child(sp(&format!("review-body-{i}"), "", format!("{}: {}", r.author, r.body))),
        );
    }
    feed = feed.child(
        form("comment", format!("{base}/comments"), "post")
            .class("comment-box")
            .child(el("textarea").id("comment-body").attr("name", "body").attr("rows", "3").attr("placeholder", "Leave a comment…").attr("aria-label", "Comment"))
            .child(div("actions").child(button("comment-submit", "Comment").class("primary"))),
    );
    let mut props = el("aside").id("properties").class("props").child(sp("", "rail-label", "Properties"));
    let prop = |name: &str, value: Html| div("prop").child(span("prop-name").text(name)).child(span("prop-value").child(value));
    props = props
        .child(prop("Status", Html::Fragment(vec![status_icon("", &item.status), sp("issue-state", "", column_label(&item.status))])))
        .child(prop("Priority", Html::Fragment(vec![priority_icon("", item), sp("issue-priority", "", priority(item).0)])))
        .child(prop(
            "Assignee",
            Html::Fragment(vec![
                avatar("issue-avatar", &item.assignee, 18),
                sp(
                    "issue-assignee",
                    "",
                    if item.assignee.is_empty() { format!("opened by {}", item.author) } else { format!("{} · assigned to {}", item.author, item.assignee) },
                ),
            ]),
        ))
        .child(prop("Labels", span("pills").children(label_pills("issue", item)).when(item.labels.is_empty(), |n| n.child(span("muted").text("No labels")))))
        .child(prop("Project", link("issue-project", format!("/projects/{key}"), team)));
    let mut moves = div("status-moves").id("status-moves").child(sp("", "rail-label", "Move to"));
    for (status, name) in COLUMNS.iter().filter(|(status, _)| *status != item.status) {
        moves = moves.child(
            form(&format!("status-{status}-form"), base.clone(), "post")
                .class("inline")
                .child(hidden("status", status))
                .child(el("button").id(format!("status-{status}")).attr("type", "submit").class("move wide").child(status_icon("", status)).child(Html::from(format!("Move to {name}")))),
        );
    }
    props = props.child(moves).child(
        form("update", base.clone(), "post")
            .class("update")
            .child(label("update-assignee", "Assignee"))
            .child(text_input("update-assignee", "assignee", &item.assignee).attr("placeholder", "Unassigned"))
            .child(button("update-submit", "Save").class("move")),
    );
    if item.kind == "pull_request" {
        props = props.child(
            form("review", format!("{base}/reviews"), "post")
                .class("update")
                .child(label("review-decision", "Decision"))
                .child(text_input("review-decision", "decision", "approve"))
                .child(label("review-body", "Review"))
                .child(text_input("review-body", "body", ""))
                .child(button("review-submit", "Submit review").class("move")),
        );
    }
    let main = el("main")
        .id("main")
        .class("main issue")
        .child(
            el("header")
                .class("topbar")
                .id("issue-head")
                .child(link("back", format!("/projects/{key}"), team).class("crumb"))
                .child(span("crumb-sep").text("›"))
                .child(sp("issue-key", "key", format!("{key}-{id}")))
                .child(span("grow"))
                .child(span("ghost").text("☆"))
                .child(span("ghost").text("⋯")),
        )
        .child(
            div("issue-columns")
                .child(
                    el("article")
                        .class("issue-main")
                        .child(el("h1").id("issue-title").text(&item.title))
                        .child(div("prose body").id("issue-body").child(idn(div(""), "body").children(if item.body.trim().is_empty() { vec![span("muted").text("Add description…")] } else { linked("body", &item.body) })))
                        .child(el("h2").class("feed-title").text("Activity"))
                        .child(feed),
                )
                .child(props),
        );
    document(look, &format!("{key}-{id} {}", item.title), rail(look, projects, Some(key), View::List), main)
}
