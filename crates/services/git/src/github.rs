//! The owner-namespaced surface as HTML: owner paths, code, commits, diffs, issues, pull
//! requests, stars and gists. One set of routes and one markup wears two looks. `github`
//! (and `plain`, which always borrowed it for these paths) is github.com as it looked in
//! 2024-2025: the grey header band with the underlined tab strip, bordered 6px boxes, the
//! About column. `gitlab` is gitlab.com: the left sidebar, the project header, blue confirm
//! buttons and "merge requests". The sheets are `base.css` (structure shared by both),
//! `github.css` and `gitlab.css`, next to this file.
//!
//! Element ids are the agent API and are the ones the `Page` version used: `mark`,
//! `crumb-<n>`, `tab-<name>-link`, `star`, `stargazers`, `watch-label`, `fork-label`,
//! `branch-name`, `branches`, `tags`, `history`, `latest-*`, `file-<n>`, `entry-message-<n>`,
//! `readme-*`, `about-*-link`, `commits-<n>-title|sha|author`, `diff-file-<n>-path`,
//! `thread-<n>`, `list-open`, `list-closed`, `new`, `comment` (form), `comment-body`,
//! `comment-submit`, `close`, `reopen`, `merge`, `approve`, `ready`, `new-issue-*`,
//! `new-pull-*`, `ptab-*-link`, `side-*`, `gist-<id>`, `hit-link-<name>`, `repo-<owner>-<name>`.
//!
//! Every one of those is a control that acts, and nothing here is a control that does not:
//! `watch` posts a real subscription (`/watchers` lists it), `go-to-file` reaches the file
//! finder (`/<owner>/<name>/find/<rev>`), `raw` serves the bytes as `text/plain`, `code`
//! goes to the source this server actually clones from, the header's `nav-issues`,
//! `nav-pulls` and `nav-inbox` reach `/issues`, `/pulls` and `/notifications`, and
//! `nav-avatar` is a link only when the actor has a profile. The pieces the real product
//! wires to a script and this world cannot — the create menu, blame, copy, download, edit,
//! the list filters, the merge-method menu, Write/Preview, the branch tabs — are gone
//! rather than drawn dead. `fork` and `tags` stayed as counts, set as text, not buttons.
use crate::{
    branch_tip, branches, commits_between, default_branch, diff_trees, language_color,
    language_stats, last_commit_per_path, list_tree, log, merge_base, resolve, short, Commit,
    DiffLine, FileDiff, GitState, Repository, Thread,
};
use cw_protocol::{HttpRequest, HttpResponse, Result};
use cw_sdk::ServiceContext;
use cw_service_common as web;
use cw_service_common::html::{
    self, button, div, el, empty, form, hidden, label, link, span, text_input, Document, Html,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

const BASE_CSS: &str = include_str!("base.css");
const GITHUB_CSS: &str = include_str!("github.css");
const GITLAB_CSS: &str = include_str!("gitlab.css");

/// Which product the pages stand in for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Look {
    Github,
    Gitlab,
}
impl Look {
    fn of(state: &GitState) -> Look {
        if state.skin.as_str() == "gitlab" {
            Look::Gitlab
        } else {
            Look::Github
        }
    }
    fn brand(self) -> &'static str {
        match self {
            Look::Github => "GitHub",
            Look::Gitlab => "GitLab",
        }
    }
    /// "Pull requests" on GitHub, "Merge requests" on GitLab.
    fn pulls(self) -> &'static str {
        match self {
            Look::Github => "Pull requests",
            Look::Gitlab => "Merge requests",
        }
    }
    fn pull(self) -> &'static str {
        match self {
            Look::Github => "pull request",
            Look::Gitlab => "merge request",
        }
    }
}
/// What every page needs: the site, its look, who is asking and when.
struct Cx<'a> {
    state: &'a GitState,
    look: Look,
    actor: &'a str,
    now: u64,
    /// What is in the header's search box: the query this page answered, if it is a
    /// search, so the box comes back filled the way the real one does.
    search: &'a str,
}

// ---- Time. A tick is an hour; tick 0 is Saturday 1 August 2026, and "now" is the ----
// ---- later of the request tick and the newest thing on the site, so nothing is  ----
// ---- ever "in 3 days".                                                           ----

const EPOCH_WEEKDAY: u64 = 6;
fn ago(now: u64, tick: u64) -> String {
    let hours = now.saturating_sub(tick);
    let days = hours / 24;
    let weeks = days / 7;
    let months = days / 30;
    match (hours, days, weeks, months) {
        (0, ..) => "just now".into(),
        (1, ..) => "1 hour ago".into(),
        (h, 0, ..) => format!("{h} hours ago"),
        (_, 1, ..) => "yesterday".into(),
        (_, d, 0, _) => format!("{d} days ago"),
        (_, _, 1, 0) => "last week".into(),
        (_, _, w, 0) => format!("{w} weeks ago"),
        (_, _, _, 1) => "last month".into(),
        (_, _, _, m) => format!("{m} months ago"),
    }
}
/// `(year, month, day)` of a tick, counted from the epoch.
fn calendar(tick: u64) -> (u64, u64, u64) {
    let mut days = tick / 24;
    let (mut year, mut month) = (2026, 8);
    loop {
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let length = match month {
            2 if leap => 29,
            2 => 28,
            4 | 6 | 9 | 11 => 30,
            _ => 31,
        };
        if days < length {
            return (year, month, days + 1);
        }
        days -= length;
        month += 1;
        if month > 12 {
            month = 1;
            year += 1;
        }
    }
}
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
fn date(tick: u64) -> String {
    let (y, m, d) = calendar(tick);
    format!("{} {d}, {y}", MONTHS[(m - 1) as usize])
}
/// The world clock counts microseconds; the site's own clock counts hours from its epoch.
/// "Now" is the newest thing on the site plus however long the world has been running,
/// so a seed reads as recent history and a new comment lands after everything seeded.
const TICKS_PER_HOUR: u64 = 3_600_000_000;
fn now(state: &GitState, ctx: &ServiceContext) -> u64 {
    latest_tick(state) + ctx.tick / TICKS_PER_HOUR
}
/// The newest tick anywhere on the site, so relative times never run negative.
fn latest_tick(state: &GitState) -> u64 {
    let threads = |t: &BTreeMap<u64, Thread>| {
        t.values()
            .flat_map(|t| {
                std::iter::once(t.tick)
                    .chain(t.comments.iter().map(|c| c.tick))
                    .chain(t.reviews.iter().map(|r| r.tick))
            })
            .max()
            .unwrap_or(0)
    };
    state
        .repositories
        .values()
        .flat_map(|r| {
            r.objects
                .values()
                .map(|c| c.tick)
                .chain([threads(&r.issues), threads(&r.pull_requests)])
        })
        .chain(state.gists.values().map(|g| g.tick))
        .max()
        .unwrap_or(0)
}

/// Colour of a label chip: GitHub's defaults for the well-known names, a stable tint
/// from the name for the rest.
fn label_colours(name: &str) -> (&'static str, &'static str) {
    match name {
        "bug" => ("#d73a4a", "#ffffff"),
        "enhancement" => ("#a2eeef", "#1f2328"),
        "good first issue" => ("#7057ff", "#ffffff"),
        "documentation" | "docs" => ("#0075ca", "#ffffff"),
        "help wanted" => ("#008672", "#ffffff"),
        "question" => ("#d876e3", "#1f2328"),
        "wontfix" => ("#e4e669", "#1f2328"),
        "duplicate" => ("#cfd3d7", "#1f2328"),
        "invalid" => ("#e4e669", "#1f2328"),
        "ci" => ("#bfd4f2", "#1f2328"),
        "launch" => ("#fbca04", "#1f2328"),
        _ => {
            const TINTS: [&str; 6] = [
                "#5319e7", "#0e8a16", "#b60205", "#1d76db", "#c2e0c6", "#f9d0c4",
            ];
            let mut h = 0xcbf29ce484222325u64;
            for b in name.bytes() {
                h = (h ^ u64::from(b)).wrapping_mul(0x100000001b3);
            }
            let fill = TINTS[(h % 6) as usize];
            (fill, if h % 6 >= 4 { "#1f2328" } else { "#ffffff" })
        }
    }
}
// ---- Small presentational pieces. ----

/// `node` with `id`, unless the id is empty.
fn idn(node: Html, id: &str) -> Html {
    if id.is_empty() {
        node
    } else {
        node.id(id)
    }
}
/// `<span id class>text</span>`.
fn sp(id: &str, class: &str, s: impl Into<String>) -> Html {
    idn(span(class), id).text(s)
}
/// A CSS-drawn or glyph icon; the sheet draws it from the `ic-<name>` class.
fn ic(id: &str, name: &str) -> Html {
    idn(span("ic"), id)
        .class(&format!("ic-{name}"))
        .attr("data-icon", name)
        .attr("aria-hidden", "true")
}
/// A round avatar tinted from the name, showing its initials.
fn avatar(id: &str, name: &str, size: u32) -> Html {
    let initials: String = name
        .split(|c: char| c.is_whitespace() || c == '-' || c == '_' || c == '.')
        .filter(|w| !w.is_empty())
        .take(2)
        .filter_map(|w| w.chars().next())
        .flat_map(char::to_uppercase)
        .collect();
    idn(span("avatar"), id)
        .class(&format!("s{size}"))
        .attr("title", name)
        .style(&format!("background: {}", web::avatar_tint(name)))
        .text(initials)
}
/// A link styled by class.
fn a(id: &str, class: &str, url: impl Into<String>, s: impl Into<String>) -> Html {
    link(id, url, s).class(class)
}
fn btn_link(id: &str, s: &str, url: impl Into<String>) -> Html {
    a(id, "btn", url, s)
}
fn primary_link(id: &str, s: &str, url: impl Into<String>) -> Html {
    a(id, "btn btn-primary", url, s)
}
/// A button that posts fixed fields: a one-button form, `<id>-form` around `<id>`.
fn post_button(id: &str, class: &str, s: &str, url: String, fields: &[(&str, &str)]) -> Html {
    form(&format!("{id}-form"), url, "post")
        .class("inline-form")
        .each(fields.iter(), |(k, v)| hidden(k, v))
        .child(button(id, s).class(class))
}
/// The grey number bubble after a tab or a chip label.
fn counter(id: &str, s: impl Into<String>) -> Html {
    sp(id, "counter", s)
}
/// A branch name the way it is set in prose: blue mono on a pale-blue chip.
fn ref_chip(id: &str, name: &str, url: &str) -> Html {
    a(id, "ref", url, name)
}
fn label_chip(id: &str, name: &str) -> Html {
    let (fill, ink) = label_colours(name);
    sp(id, "label", name).style(&format!("background: {fill}; color: {ink}"))
}
fn labels(prefix: &str, thread: &Thread) -> Vec<Html> {
    thread
        .labels
        .iter()
        .enumerate()
        .map(|(i, l)| label_chip(&format!("{prefix}-label-{i}"), l))
        .collect()
}
/// `(word, css class, icon)` of a thread's state.
fn state_of(thread: &Thread, pull: bool) -> (&'static str, &'static str, &'static str) {
    match (thread.state.as_str(), pull, thread.draft) {
        ("merged", ..) => ("Merged", "st-merged", "merge"),
        ("closed", true, _) => ("Closed", "st-closed", "pull-request"),
        ("closed", false, _) => ("Closed", "st-done", "issue-closed"),
        (_, true, true) => ("Draft", "st-draft", "pull-request"),
        (_, true, false) => ("Open", "st-open", "pull-request"),
        _ => ("Open", "st-open", "issue-open"),
    }
}
/// The state badge on a thread page: icon and word on a solid pill.
fn state_pill(id: &str, thread: &Thread, pull: bool) -> Html {
    let (word, class, icon) = state_of(thread, pull);
    span("state")
        .id(id)
        .class(class)
        .child(ic(&format!("{id}-icon"), icon))
        .child(sp(&format!("{id}-text"), "", word))
}
/// The small state icon in a list row.
fn state_icon(id: &str, thread: &Thread, pull: bool) -> Html {
    let (word, class, icon) = state_of(thread, pull);
    ic(id, icon).class(class).attr("title", word)
}
fn slug(repository: &Repository, name: &str) -> String {
    format!("{}/{name}", repository.owner)
}
fn open_count(threads: &BTreeMap<u64, Thread>) -> usize {
    threads.values().filter(|t| t.state == "open").count()
}
fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}
/// Text with its `http(s)` URLs as links (`<prefix>-link-<word index>`, the ids the Page
/// version gave them) and `` `code` `` spans as `<code>`. Whitespace is kept as written.
fn inline(prefix: &str, body: &str, code_spans: bool) -> Vec<Html> {
    let mut out = vec![];
    let mut plain = String::new();
    let mut word_index = 0usize;
    let mut rest = body;
    let flush = |plain: &mut String, out: &mut Vec<Html>| {
        if plain.is_empty() {
            return;
        }
        let s = std::mem::take(plain);
        if !code_spans || !s.contains('`') {
            out.push(Html::from(s));
            return;
        }
        for (i, piece) in s.split('`').enumerate() {
            if piece.is_empty() {
                continue;
            }
            out.push(if i % 2 == 1 {
                el("code").text(piece)
            } else {
                Html::from(piece)
            });
        }
    };
    while !rest.is_empty() {
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        if end == 0 {
            let ws = rest
                .find(|c: char| !c.is_whitespace())
                .unwrap_or(rest.len());
            plain.push_str(&rest[..ws]);
            rest = &rest[ws..];
            continue;
        }
        let word = &rest[..end];
        rest = &rest[end..];
        let url = word.trim_end_matches(['.', ',', ';', ')', ']']);
        let is_link = url::Url::parse(url)
            .ok()
            .is_some_and(|u| matches!(u.scheme(), "http" | "https"));
        if is_link {
            flush(&mut plain, &mut out);
            out.push(link(&format!("{prefix}-link-{word_index}"), url, url));
            plain.push_str(&word[url.len()..]);
        } else {
            plain.push_str(word);
        }
        word_index += 1;
    }
    flush(&mut plain, &mut out);
    out
}
/// A body of prose as written: paragraphs kept, URLs clickable.
fn prose(id: &str, body: &str) -> Html {
    div("prose").id(id).children(inline(id, body, false))
}

// ---- The shell: GitHub's grey header band with the tab strip, or GitLab's sidebar. ----

fn search_box(look: Look, query: &str) -> Html {
    form("search", "/search", "get")
        .class("site-search")
        // The magnifier is the button that sends the box, not a picture of one: with no
        // page script there is nothing else to submit it, and the semantic tree needs a
        // control an agent can press.
        .child(
            button("search-go", "")
                .class("search-go")
                .attr("aria-label", "Search")
                .child(ic("search-icon", "search")),
        )
        .child(
            text_input("search-q", "q", query)
                .attr(
                    "placeholder",
                    match look {
                        Look::Github => "Type / to search",
                        Look::Gitlab => "Search or go to…",
                    },
                )
                .attr("aria-label", "Search"),
        )
}
fn crumbs(trail: &[(&str, String)]) -> Html {
    let mut nav = el("nav").class("crumbs").attr("aria-label", "Breadcrumb");
    for (i, (text, url)) in trail.iter().enumerate() {
        if i > 0 {
            nav = nav.child(sp(&format!("crumb-sep-{i}"), "sep", "/"));
        }
        let last = i + 1 == trail.len();
        nav = nav.child(a(
            &format!("crumb-{i}"),
            if last { "crumb last" } else { "crumb" },
            url.clone(),
            *text,
        ));
    }
    nav
}
/// The whole page around `main`: `nav` is the repository's tab strip, if there is one.
fn shell(cx: &Cx, trail: &[(&str, String)], nav: Option<Html>, main: Vec<Html>) -> Vec<Html> {
    // Each of these reaches a page: the dashboards of everything open across the site, and
    // the profile of whoever is signed in. The real header's "+" menu is not here, because
    // nothing on this instance is created from the header.
    let tools = |class: &str| {
        let tool = |id: &str, icon: &str, title: &str, url: &str| {
            el("a")
                .id(id)
                .class("tool")
                .attr("href", url)
                .attr("title", title)
                .attr("aria-label", title)
                .child(ic("", icon))
        };
        let mut bar = div(class)
            .id("chrome-right")
            .child(tool("nav-issues", "issue-open", "Issues", "/issues"))
            .child(tool("nav-pulls", "pull-request", cx.look.pulls(), "/pulls"))
            .child(tool(
                "nav-inbox",
                "inbox",
                "Notifications",
                "/notifications",
            ));
        // The avatar is a link only when there is a profile behind it.
        bar = bar.child(if people(cx.state).contains(cx.actor) {
            el("a")
                .id("nav-profile")
                .class("nav-profile")
                .attr("href", format!("/{}", cx.actor))
                .attr("aria-label", format!("{}'s profile", cx.actor))
                .child(avatar("nav-avatar", cx.actor, 32))
        } else {
            avatar("nav-avatar", cx.actor, 32)
        });
        bar
    };
    match cx.look {
        Look::Github => vec![
            el("header")
                .id("chrome")
                .class("site-header")
                .child(
                    div("bar")
                        .child(
                            div("bar-left")
                                .id("chrome-left")
                                .child(
                                    el("a")
                                        .id("mark")
                                        .class("mark")
                                        .attr("href", "/")
                                        .attr("aria-label", "GitHub")
                                        .child(el("i").class("ear l"))
                                        .child(el("i").class("ear r"))
                                        .child(el("i").class("face")),
                                )
                                .child(if trail.is_empty() {
                                    sp("crumb-home", "crumb-text", "Dashboard")
                                } else {
                                    crumbs(trail)
                                }),
                        )
                        .child(search_box(cx.look, cx.search))
                        .child(tools("bar-right")),
                )
                .maybe(nav),
            el("main").class("container").children(main),
            el("footer")
                .class("site-footer")
                .child(sp("foot-copy", "", "© 2026 GitHub, Inc."))
                // Set as text, not as links: this instance has no terms or status page.
                .each(
                    ["Terms", "Privacy", "Security", "Status", "Docs", "Contact"],
                    |t| sp("", "foot-item", t),
                ),
        ],
        Look::Gitlab => vec![div("layout")
            .child(
                el("aside")
                    .id("chrome")
                    .class("sidebar")
                    .child(
                        div("side-top")
                            .id("chrome-left")
                            .child(
                                el("a")
                                    .id("mark")
                                    .class("mark")
                                    .attr("href", "/")
                                    .attr("aria-label", "GitLab")
                                    .child(el("i").class("ear l"))
                                    .child(el("i").class("ear r"))
                                    .child(el("i").class("face")),
                            )
                            .child(tools("side-tools")),
                    )
                    .child(search_box(cx.look, cx.search))
                    .child(match nav {
                        Some(nav) => nav,
                        None => el("nav")
                            .class("side-nav")
                            .child(sp("", "side-title", "Your work"))
                            .child(a("side-projects", "side-item active", "/", "Projects"))
                            .when(!cx.state.gists.is_empty(), |n| {
                                n.child(a("side-snippets", "side-item", "/gists", "Snippets"))
                            }),
                    })
                    .child(sp("", "side-help", "Help")),
            )
            .child(
                div("page")
                    .child(div("topbar").child(if trail.is_empty() {
                        el("nav").class("crumbs").child(sp(
                            "crumb-home",
                            "crumb-text",
                            "Your work / Projects",
                        ))
                    } else {
                        crumbs(trail)
                    }))
                    .child(el("main").class("container").children(main)),
            )],
    }
}
fn finish(cx: &Cx, title: &str, body: Vec<Html>) -> Result<HttpResponse> {
    let mut doc = Document::new(title)
        .lang("en")
        .stylesheet(BASE_CSS)
        .stylesheet(match cx.look {
            Look::Github => GITHUB_CSS,
            Look::Gitlab => GITLAB_CSS,
        })
        .body_class(match cx.look {
            Look::Github => "skin-github",
            Look::Gitlab => "skin-gitlab",
        })
        .body(body);
    // A seed may override the palette, as every skinned service allows.
    if let Some(theme) = &cx.state.theme {
        let vars: Vec<String> = [
            ("--accent", &theme.accent),
            ("--paper", &theme.background),
            ("--surface", &theme.surface),
            ("--ink", &theme.ink),
            ("--muted", &theme.muted),
        ]
        .iter()
        .filter_map(|(k, v)| v.as_ref().map(|v| format!("{k}: {v}")))
        .collect();
        if !vars.is_empty() {
            doc = doc.root_style(&vars.join("; "));
        }
    }
    html::page(&doc)
}

// ---- The repository frame: title row, action chips, and the tab strip. ----

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Code,
    Issues,
    Pulls,
    /// One of the tabs that has only a blank state behind it, by its route name.
    Stub(&'static str),
}
/// One tab: `<li id>` around `<a id="<id>-link">` with its icon, label and count.
fn tab(id: &str, icon: &str, text: &str, count: Option<usize>, url: String, active: bool) -> Html {
    let mut link = el("a")
        .id(format!("{id}-link"))
        .class("tab")
        .attr("href", url)
        .child(ic(&format!("{id}-icon"), icon))
        .child(span("tab-label").text(text));
    if active {
        link = link.class("active").attr("aria-current", "page");
    }
    if let Some(n) = count {
        link = link
            .child(Html::from(" "))
            .child(counter(&format!("{id}-count"), n.to_string()));
    }
    el("li")
        .id(id)
        .class(if active {
            "tab-item active"
        } else {
            "tab-item"
        })
        .child(link)
}
fn tabs(id: &str, class: &str, items: Vec<Html>) -> Html {
    el("nav")
        .class(class)
        .child(el("ul").id(id).children(items))
}
fn repo_frame(
    cx: &Cx,
    repository: &Repository,
    name: &str,
    active: Tab,
    body: Vec<Html>,
) -> Vec<Html> {
    let path = slug(repository, name);
    let starred = repository.stars.contains(cx.actor);
    let watching = repository.watchers.contains(cx.actor);
    let gitlab = cx.look == Look::Gitlab;
    let t = |id: &str, icon: &str, text: &str, count: Option<usize>, route: &str, on: bool| {
        tab(id, icon, text, count, format!("/{path}{route}"), on)
    };
    let mut items = vec![
        t("tab-code", "code", "Code", None, "", active == Tab::Code),
        t(
            "tab-issues",
            "issue-open",
            "Issues",
            Some(open_count(&repository.issues)),
            "/issues",
            active == Tab::Issues,
        ),
        t(
            "tab-pulls",
            "pull-request",
            cx.look.pulls(),
            Some(open_count(&repository.pull_requests)),
            "/pulls",
            active == Tab::Pulls,
        ),
        t(
            "tab-actions",
            "play",
            if gitlab { "Build" } else { "Actions" },
            None,
            "/actions",
            active == Tab::Stub("actions"),
        ),
        t(
            "tab-projects",
            "grid",
            if gitlab { "Plan" } else { "Projects" },
            None,
            "/projects",
            active == Tab::Stub("projects"),
        ),
        t(
            "tab-wiki",
            "book",
            "Wiki",
            None,
            "/wiki",
            active == Tab::Stub("wiki"),
        ),
        t(
            "tab-security",
            "shield",
            if gitlab { "Secure" } else { "Security" },
            None,
            "/security",
            active == Tab::Stub("security"),
        ),
        t(
            "tab-insights",
            "signal",
            if gitlab { "Analyze" } else { "Insights" },
            None,
            "/pulse",
            active == Tab::Stub("pulse"),
        ),
        t(
            "tab-settings",
            "gear",
            "Settings",
            None,
            "/settings",
            active == Tab::Stub("settings"),
        ),
    ];
    let nav = if gitlab {
        // The sidebar names the project above its sections.
        items.insert(
            0,
            el("li")
                .class("side-context")
                .child(avatar("side-project-avatar", name, 24).class("square"))
                .child(sp("side-project", "", name)),
        );
        tabs("repo-tabs", "side-nav", items)
    } else {
        tabs("repo-tabs", "tabs repo-tabs", items)
    };
    let head = div("repo-head")
        .id("repo-head")
        .child(
            div("repo-title")
                .child(if gitlab {
                    avatar("repo-icon", name, 48).class("square")
                } else {
                    avatar("repo-icon", &repository.owner, 24).class("square")
                })
                .child(a(
                    "repo-owner",
                    "repo-owner",
                    format!("/{}", repository.owner),
                    &repository.owner,
                ))
                .child(sp("repo-sep", "sep", "/"))
                .child(a("repo-name", "repo-name", format!("/{path}"), name))
                .child(sp(
                    "repo-visibility",
                    "pill",
                    if repository.readers.is_empty() {
                        "Public"
                    } else {
                        "Private"
                    },
                )),
        )
        .child(
            div("repo-actions")
                // Watching is a subscription the repository really keeps, so the button
                // posts like the star beside it and the count is the set's size.
                .child(
                    form("watch-form", format!("/{path}/watch"), "post")
                        .class("inline-form watch-group")
                        .child(
                            el("button")
                                .id("watch")
                                .attr("type", "submit")
                                .class(if watching {
                                    "btn btn-sm watching"
                                } else {
                                    "btn btn-sm"
                                })
                                .child(ic("watch-icon", if gitlab { "bell" } else { "eye" }))
                                .child(sp(
                                    "watch-label",
                                    "",
                                    match (gitlab, watching) {
                                        (true, true) => "Notifications on",
                                        (true, false) => "Notifications",
                                        (false, true) => "Unwatch",
                                        (false, false) => "Watch",
                                    },
                                )),
                        )
                        .child(a(
                            "watch-count",
                            "btn btn-sm star-count",
                            format!("/{path}/watchers"),
                            repository.watchers.len().to_string(),
                        )),
                )
                // Forks are a count this instance carries, not something it can make: a
                // statistic, set as one, with no click to swallow.
                .child(
                    span("chip")
                        .id("fork")
                        .child(ic("fork-icon", "fork"))
                        .child(sp("fork-count", "", repository.forks.to_string()))
                        .child(sp(
                            "fork-label",
                            "",
                            format!(" fork{}", plural(repository.forks as usize)),
                        )),
                )
                .child(
                    form("star-form", format!("/{path}/star"), "post")
                        .class("inline-form star-group")
                        .child(
                            el("button")
                                .id("star")
                                .attr("type", "submit")
                                .class(if starred {
                                    "btn btn-sm starred"
                                } else {
                                    "btn btn-sm"
                                })
                                .child(ic("star-icon", if starred { "star-fill" } else { "star" }))
                                .child(Html::from(if starred { "Starred" } else { "Star" })),
                        )
                        .child(a(
                            "stargazers",
                            "btn btn-sm star-count",
                            format!("/{path}/stargazers"),
                            repository.stars.len().to_string(),
                        )),
                ),
        );
    let mut main = vec![head];
    main.extend(body);
    shell(
        cx,
        &[
            (repository.owner.as_str(), format!("/{}", repository.owner)),
            (name, format!("/{path}")),
        ],
        Some(nav),
        main,
    )
}

// ---- Markdown, as much of it as a README needs. ----

fn markdown(prefix: &str, source: &str) -> Vec<Html> {
    let mut out = vec![];
    let mut paragraph: Vec<String> = vec![];
    let mut code: Option<Vec<String>> = None;
    // A four-space (or tab) indented block, the other way a README writes a shell transcript.
    let mut indented: Option<Vec<String>> = None;
    let mut list: Option<(bool, Vec<Html>)> = None;
    let mut n = 0usize;
    // Every block draws an id from one counter, in the order the Page version did.
    let mut next = |kind: &str| {
        n += 1;
        format!("{prefix}-{kind}-{n}")
    };
    fn flush(paragraph: &mut Vec<String>, out: &mut Vec<Html>, id: String) {
        if !paragraph.is_empty() {
            let body = paragraph.join(" ");
            out.push(el("p").id(id.clone()).children(inline(&id, &body, true)));
            paragraph.clear();
        }
    }
    fn close(list: &mut Option<(bool, Vec<Html>)>, out: &mut Vec<Html>) {
        if let Some((ordered, items)) = list.take() {
            out.push(el(if ordered { "ol" } else { "ul" }).children(items));
        }
    }
    /// One level of code indentation off the front of a line.
    fn dedent(line: &str) -> String {
        line.strip_prefix("    ")
            .or_else(|| line.strip_prefix('\t'))
            .unwrap_or(line)
            .to_owned()
    }
    for line in source.lines() {
        if let Some(block) = indented.as_mut() {
            if line.trim().is_empty() {
                block.push(String::new());
                continue;
            }
            if line.starts_with("    ") || line.starts_with('\t') {
                block.push(dedent(line));
                continue;
            }
            let mut block = indented.take().expect("in an indented block");
            while block.last().is_some_and(|l| l.is_empty()) {
                block.pop();
            }
            out.push(
                el("pre")
                    .id(next("code"))
                    .class("code")
                    .child(el("code").text(block.join("\n"))),
            );
        }
        if let Some(block) = code.as_mut() {
            if line.trim_start().starts_with("```") {
                out.push(
                    el("pre")
                        .id(next("code"))
                        .class("code")
                        .child(el("code").text(block.join("\n"))),
                );
                code = None;
            } else {
                block.push(line.to_owned());
            }
            continue;
        }
        if line.trim_start().starts_with("```") {
            flush(&mut paragraph, &mut out, next("p"));
            close(&mut list, &mut out);
            code = Some(vec![]);
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            flush(&mut paragraph, &mut out, next("p"));
            close(&mut list, &mut out);
            continue;
        }
        // An indented block only opens where a paragraph or a list is not already running,
        // so a wrapped list item keeps belonging to its item.
        if (line.starts_with("    ") || line.starts_with('\t'))
            && paragraph.is_empty()
            && list.is_none()
        {
            indented = Some(vec![dedent(line)]);
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix('#') {
            flush(&mut paragraph, &mut out, next("p"));
            close(&mut list, &mut out);
            let level = 1 + rest.chars().take_while(|c| *c == '#').count();
            let title = rest.trim_start_matches('#').trim().replace('`', "");
            out.push(el(&format!("h{}", level.min(6))).id(next("h")).text(title));
            if level <= 2 {
                // The rule under a first or second level heading is the heading's border
                // now; the counter still moves, so later ids stay where they were.
                let _ = next("hr");
            }
            continue;
        }
        let item = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .map(|s| (false, s))
            .or_else(|| {
                let (num, rest) = trimmed.split_once(". ")?;
                num.parse::<u32>().ok().map(|_| (true, rest))
            });
        if let Some((ordered, body)) = item {
            flush(&mut paragraph, &mut out, next("p"));
            if list.as_ref().is_some_and(|(o, _)| *o != ordered) {
                close(&mut list, &mut out);
            }
            let id = next("li");
            let node = el("li").id(id.clone()).children(inline(&id, body, true));
            list.get_or_insert_with(|| (ordered, vec![])).1.push(node);
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("> ") {
            flush(&mut paragraph, &mut out, next("p"));
            close(&mut list, &mut out);
            let id = next("quote");
            out.push(
                el("blockquote")
                    .id(id.clone())
                    .children(inline(&id, rest, true)),
            );
            continue;
        }
        close(&mut list, &mut out);
        paragraph.push(trimmed.to_owned());
    }
    if let Some(block) = code.take() {
        out.push(
            el("pre")
                .id(next("code"))
                .class("code")
                .child(el("code").text(block.join("\n"))),
        );
    }
    if let Some(mut block) = indented.take() {
        while block.last().is_some_and(|l| l.is_empty()) {
            block.pop();
        }
        out.push(
            el("pre")
                .id(next("code"))
                .class("code")
                .child(el("code").text(block.join("\n"))),
        );
    }
    flush(&mut paragraph, &mut out, next("p"));
    close(&mut list, &mut out);
    out
}

// ---- Diffs: one bordered file box each, two number gutters, red and green rows. ----

fn diff_block(prefix: &str, file: &FileDiff, url: &str) -> Html {
    // Five squares, split between green and red the way the change is.
    let total = (file.additions + file.deletions).max(1);
    let green = (file.additions * 5 + total / 2) / total;
    let green = match (file.additions, file.deletions) {
        (_, 0) => 5,
        (0, _) => 0,
        _ => green.clamp(1, 4),
    };
    let red = if file.deletions == 0 { 0 } else { 5 - green };
    let mut squares = span("diffstat").attr("aria-hidden", "true");
    for i in 0..5 {
        squares = squares.child(el("i").class(if i < green {
            "a"
        } else if i < green + red {
            "d"
        } else {
            "n"
        }));
    }
    let head = div("box-head diff-head")
        .id(format!("{prefix}-head"))
        .child(a(
            &format!("{prefix}-path"),
            "diff-path",
            url.to_owned(),
            &file.path,
        ))
        .child(sp(&format!("{prefix}-status"), "muted small", &file.status))
        .child(span("grow"))
        .child(sp(
            &format!("{prefix}-adds"),
            "adds",
            format!("+{}", file.additions),
        ))
        .child(sp(
            &format!("{prefix}-dels"),
            "dels",
            format!("-{}", file.deletions),
        ))
        .child(squares);
    let mut body = div("diff-body");
    let mut n = 0usize;
    for hunk in &file.hunks {
        n += 1;
        body = body.child(
            div("dl hunk")
                .id(format!("{prefix}-hunk-{n}"))
                .child(span("ln"))
                .child(span("ln"))
                .child(span("lc").text(hunk.header())),
        );
        let (mut old, mut new) = (hunk.old_start, hunk.new_start);
        let mut run: Vec<Html> = vec![];
        let mut kind: Option<&'static str> = None;
        let close =
            |run: &mut Vec<Html>, kind: Option<&'static str>, body: &mut Html, n: &mut usize| {
                if run.is_empty() {
                    return;
                }
                *n += 1;
                let rows = std::mem::take(run);
                let block = div("run")
                    .class(kind.unwrap_or("ctx"))
                    .id(format!("{prefix}-lines-{n}"))
                    .children(rows);
                *body = std::mem::replace(body, empty()).child(block);
            };
        for line in &hunk.lines {
            let (this, a, b, sign, s) = match line {
                DiffLine::Context(s) => {
                    let r = ("ctx", old.to_string(), new.to_string(), " ", s);
                    old += 1;
                    new += 1;
                    r
                }
                DiffLine::Add(s) => {
                    let r = ("add", String::new(), new.to_string(), "+", s);
                    new += 1;
                    r
                }
                DiffLine::Remove(s) => {
                    let r = ("del", old.to_string(), String::new(), "-", s);
                    old += 1;
                    r
                }
            };
            if kind != Some(this) {
                close(&mut run, kind, &mut body, &mut n);
                kind = Some(this);
            }
            run.push(
                div("dl")
                    .child(span("ln").text(a))
                    .child(span("ln").text(b))
                    .child(
                        span("lc")
                            .child(span("sign").text(sign))
                            .child(Html::from(s.as_str())),
                    ),
            );
        }
        close(&mut run, kind, &mut body, &mut n);
    }
    div("box diff-file").id(prefix).child(head).child(body)
}
fn diff_section(prefix: &str, files: &[FileDiff], path: &str, branch: &str) -> Vec<Html> {
    let (adds, dels) = files
        .iter()
        .fold((0, 0), |(a, d), f| (a + f.additions, d + f.deletions));
    let mut out = vec![el("p")
        .id(format!("{prefix}-summary"))
        .class("diff-summary")
        .text(format!(
            "Showing {} changed file{} with {adds} addition{} and {dels} deletion{}.",
            files.len(),
            plural(files.len()),
            plural(adds),
            plural(dels),
        ))];
    for (i, file) in files.iter().enumerate() {
        out.push(diff_block(
            &format!("{prefix}-file-{i}"),
            file,
            &format!("/{path}/blob/{branch}/{}", file.path),
        ));
    }
    out
}

// ---- Pages. ----

fn home(cx: &Cx) -> Result<HttpResponse> {
    // No "New" button: nothing on this instance creates a repository from the web.
    let mut side = el("aside")
        .id("home-side")
        .class("home-side")
        .child(div("side-head").child(el("h2").id("side-title").text("Top repositories")));
    let mut feed = vec![];
    for (name, repository) in &cx.state.repositories {
        if repository.owner.is_empty() {
            continue;
        }
        let path = slug(repository, name);
        side = side.child(
            div("side-repo")
                .id(format!("side-{name}"))
                .child(
                    avatar(&format!("side-avatar-{name}"), &repository.owner, 16).class("square"),
                )
                .child(a(
                    &format!("side-link-{name}"),
                    "",
                    format!("/{path}"),
                    &path,
                )),
        );
        let tip = branch_tip(repository, &default_branch(repository));
        let mut meta = div("meta").id(format!("repo-meta-{name}"));
        if let Some((language, ..)) = tip
            .as_ref()
            .and_then(|(_, c)| language_stats(&c.files).into_iter().next())
        {
            meta = meta.child(
                span("lang")
                    .child(
                        el("i")
                            .class("dot")
                            .style(&format!("background: {}", language_color(&language))),
                    )
                    .child(Html::from(language)),
            );
        }
        meta = meta
            .child(span("meta-item").child(ic("", "star")).child(sp(
                &format!("repo-stars-{name}"),
                "",
                format!(
                    "{} star{}",
                    repository.stars.len(),
                    plural(repository.stars.len())
                ),
            )))
            .child(span("meta-item").child(ic("", "issue-open")).child(sp(
                &format!("repo-issues-{name}"),
                "",
                format!(
                    "{} open issue{}",
                    open_count(&repository.issues),
                    plural(open_count(&repository.issues))
                ),
            )));
        if let Some((_, commit)) = tip {
            meta = meta.child(sp(
                &format!("repo-updated-{name}"),
                "",
                format!("Updated {}", ago(cx.now, commit.tick)),
            ));
        }
        feed.push(
            el("a")
                .id(format!("repo-{}-{name}", repository.owner))
                .class("repo-card")
                .attr("href", format!("/{path}"))
                .child(
                    div("repo-card-title")
                        .id(format!("repo-title-{name}"))
                        .child(
                            avatar(&format!("repo-avatar-{name}"), &repository.owner, 20)
                                .class("square"),
                        )
                        .child(sp(&format!("repo-name-{name}"), "name", &path))
                        .child(span("pill").text(if repository.readers.is_empty() {
                            "Public"
                        } else {
                            "Private"
                        })),
                )
                .child(sp(
                    &format!("repo-desc-{name}"),
                    "desc",
                    &repository.description,
                ))
                .child(meta),
        );
    }
    if !cx.state.gists.is_empty() {
        side = side.child(a(
            "gists",
            "side-more",
            "/gists",
            if cx.look == Look::Gitlab {
                "Browse snippets"
            } else {
                "Browse gists"
            },
        ));
    }
    let main = div("home-main")
        .id("home-main")
        .child(el("h1").id("home-title").text(if cx.look == Look::Gitlab {
            "Projects"
        } else {
            "Home"
        }))
        .child(
            el("p")
                .id("home-sub")
                .class("muted")
                .text("Repositories, issues and pull requests on this instance."),
        )
        .child(div("repo-grid").id("repos").children(feed));
    let body = shell(
        cx,
        &[],
        None,
        vec![div("home").id("home").child(side).child(main)],
    );
    finish(cx, cx.look.brand(), body)
}

fn search_page(cx: &Cx, query: &str) -> Result<HttpResponse> {
    let q = query.to_ascii_lowercase();
    let mut rows = vec![];
    for (name, repository) in &cx.state.repositories {
        if repository.owner.is_empty() {
            continue;
        }
        let path = slug(repository, name);
        let hay = format!(
            "{path} {} {}",
            repository.description,
            repository.topics.join(" ")
        )
        .to_ascii_lowercase();
        if !q.split_whitespace().all(|w| hay.contains(w)) {
            continue;
        }
        rows.push(
            div("hit")
                .id(format!("hit-{name}"))
                .child(avatar("", &repository.owner, 20).class("square"))
                .child(
                    div("hit-text")
                        .child(a(
                            &format!("hit-link-{name}"),
                            "hit-link",
                            format!("/{path}"),
                            &path,
                        ))
                        .child(
                            el("p")
                                .id(format!("hit-desc-{name}"))
                                .class("muted")
                                .text(&repository.description),
                        )
                        // The topic you are already looking at is the heading of this
                        // page, so it is set as a chip; the others search for themselves.
                        .child(div("topics").each(
                            repository.topics.iter().enumerate(),
                            |(i, t)| {
                                let id = format!("hit-topic-{name}-{i}");
                                if t.eq_ignore_ascii_case(query) {
                                    sp(&id, "topic active", t.as_str()).attr("aria-current", "page")
                                } else {
                                    a(&id, "topic", html::href("/search", &[("q", t)]), t.as_str())
                                }
                            },
                        )),
                ),
        );
    }
    let count = rows.len();
    let main = vec![
        el("h1")
            .id("search-title")
            .class("page-title")
            .text(format!("{count} repository result{}", plural(count))),
        div("box hits").children(rows),
    ];
    finish(cx, &format!("{query} · Search"), shell(cx, &[], None, main))
}

/// One row of a cross-repository list: which repository, which thread, and how it stands.
fn thread_row(
    cx: &Cx,
    prefix: &str,
    repository: &Repository,
    name: &str,
    thread: &Thread,
    pulls: bool,
) -> Html {
    let path = slug(repository, name);
    let route = if pulls { "pull" } else { "issues" };
    let n = thread.number;
    div("thread-row")
        .id(format!("{prefix}-{n}"))
        .child(state_icon(&format!("{prefix}-state-{n}"), thread, pulls))
        .child(
            div("row-main")
                .id(format!("{prefix}-main-{n}"))
                .child(
                    div("row-title")
                        .id(format!("{prefix}-title-{n}"))
                        .child(a(
                            &format!("{prefix}-repo-{n}"),
                            "muted",
                            format!("/{path}"),
                            format!("{path} "),
                        ))
                        .child(a(
                            &format!("{prefix}-link-{n}"),
                            "thread-link",
                            format!("/{path}/{route}/{n}"),
                            &thread.title,
                        ))
                        .children(labels(&format!("{prefix}-{n}"), thread)),
                )
                .child(div("row-meta").id(format!("{prefix}-meta-{n}")).child(sp(
                    &format!("{prefix}-when-{n}"),
                    "",
                    format!(
                        "#{n} opened {} by {}",
                        ago(cx.now, thread.tick),
                        thread.author
                    ),
                ))),
        )
}
/// The header's issue and pull-request dashboards: everything open across the instance.
fn dashboard(cx: &Cx, pulls: bool) -> Result<HttpResponse> {
    let what = if pulls {
        cx.look.pulls().to_owned()
    } else {
        "Issues".to_owned()
    };
    let prefix = if pulls { "dash-pull" } else { "dash-issue" };
    let mut rows = vec![];
    for (name, repository) in &cx.state.repositories {
        if repository.owner.is_empty() {
            continue;
        }
        let threads = if pulls {
            &repository.pull_requests
        } else {
            &repository.issues
        };
        for thread in threads.values().filter(|t| t.state == "open") {
            rows.push((
                thread.tick,
                thread_row(cx, prefix, repository, name, thread, pulls),
            ));
        }
    }
    rows.sort_by_key(|(tick, _)| std::cmp::Reverse(*tick));
    let count = rows.len();
    let mut list = div("box threads").id("threads");
    if rows.is_empty() {
        list = list.child(
            div("blank")
                .id("empty")
                .child(ic("", if pulls { "pull-request" } else { "issue-open" }))
                .child(el("h3").text("Nothing open across this instance.")),
        );
    }
    let main = vec![
        el("h1")
            .id("dash-title")
            .class("page-title ruled")
            .text(&what),
        el("p").id("dash-sub").class("muted").text(format!(
            "{count} open {} in every repository here.",
            if pulls {
                format!(
                    "{} request{}",
                    if cx.look == Look::Gitlab {
                        "merge"
                    } else {
                        "pull"
                    },
                    plural(count)
                )
            } else {
                format!("issue{}", plural(count))
            }
        )),
        list.children(rows.into_iter().map(|(_, row)| row)),
    ];
    finish(
        cx,
        &format!("{what} · {}", cx.look.brand()),
        shell(cx, &[], None, main),
    )
}
/// Notifications: the threads this actor is part of — filed, commented on, reviewed,
/// assigned or merged — newest first. Nothing is stored for it; it is a reading of the
/// threads themselves, which is what makes it always true.
fn notifications_page(cx: &Cx) -> Result<HttpResponse> {
    let mut rows = vec![];
    for (name, repository) in &cx.state.repositories {
        if repository.owner.is_empty() {
            continue;
        }
        for (pulls, threads) in [
            (false, &repository.issues),
            (true, &repository.pull_requests),
        ] {
            for thread in threads.values() {
                let last = thread
                    .comments
                    .iter()
                    .map(|c| c.tick)
                    .chain(thread.reviews.iter().map(|r| r.tick))
                    .max()
                    .unwrap_or(thread.tick);
                let mine = thread.author == cx.actor
                    || thread.assignee == cx.actor
                    || thread.comments.iter().any(|c| c.author == cx.actor)
                    || thread.reviews.iter().any(|r| r.author == cx.actor)
                    || repository.watchers.contains(cx.actor);
                if mine {
                    rows.push((
                        last,
                        thread_row(cx, "inbox", repository, name, thread, pulls),
                    ));
                }
            }
        }
    }
    rows.sort_by_key(|(tick, _)| std::cmp::Reverse(*tick));
    let count = rows.len();
    let mut list = div("box threads").id("threads");
    if rows.is_empty() {
        list = list.child(
            div("blank")
                .id("empty")
                .child(ic("", "inbox"))
                .child(el("h3").text("All caught up"))
                .child(el("p").class("muted").text("Nothing here mentions you yet. File an issue or watch a repository and it will show up.")),
        );
    }
    let main = vec![
        el("h1")
            .id("inbox-title")
            .class("page-title ruled")
            .text("Notifications"),
        el("p").id("inbox-sub").class("muted").text(format!(
            "{count} thread{} you have taken part in or are watching.",
            plural(count)
        )),
        list.children(rows.into_iter().map(|(_, row)| row)),
    ];
    finish(
        cx,
        &format!("Notifications · {}", cx.look.brand()),
        shell(
            cx,
            &[("notifications", "/notifications".into())],
            None,
            main,
        ),
    )
}

/// Everyone this instance knows by name: repository owners, the people who write the
/// commits, star or watch a repository, file and answer its threads, and publish gists.
/// Every `/<name>` link a page paints is one of these, so every one of them has a page.
fn people(state: &GitState) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    for repository in state.repositories.values() {
        if !repository.owner.is_empty() {
            out.insert(repository.owner.clone());
        }
        out.extend(
            repository
                .objects
                .values()
                .map(|c| c.author.clone())
                .filter(|a| !a.is_empty()),
        );
        out.extend(repository.stars.iter().cloned());
        out.extend(repository.watchers.iter().cloned());
        for thread in repository
            .issues
            .values()
            .chain(repository.pull_requests.values())
        {
            for who in std::iter::once(&thread.author)
                .chain(std::iter::once(&thread.assignee))
                .chain(std::iter::once(&thread.merged_by))
                .chain(thread.comments.iter().map(|c| &c.author))
                .chain(thread.reviews.iter().map(|r| &r.author))
            {
                if !who.is_empty() {
                    out.insert(who.clone());
                }
            }
        }
    }
    out.extend(
        state
            .gists
            .values()
            .map(|g| g.owner.clone())
            .filter(|o| !o.is_empty()),
    );
    out
}
/// A profile. `tab` picks the view the tab strip names: the overview, every repository
/// they own, or every repository they have starred.
fn owner_page(cx: &Cx, owner: &str, tab_name: &str) -> Result<HttpResponse> {
    let owned: Vec<_> = cx
        .state
        .repositories
        .iter()
        .filter(|(_, r)| r.owner == owner)
        .collect();
    if owned.is_empty() && !people(cx.state).contains(owner) {
        return web::error(404, "owner not found");
    }
    let starred: Vec<_> = cx
        .state
        .repositories
        .iter()
        .filter(|(_, r)| r.stars.contains(owner))
        .collect();
    let is_org = owned.len() > 1;
    // An organisation's graph is the work in its repositories; a person's is their own
    // commits, wherever they landed.
    let mut commits: Vec<u64> = vec![];
    if is_org {
        for (_, repository) in &owned {
            if let Some((id, _)) = branch_tip(repository, &default_branch(repository)) {
                commits.extend(log(repository, &id).iter().map(|(_, c)| c.tick));
            }
        }
    } else {
        for repository in cx.state.repositories.values() {
            commits.extend(
                repository
                    .objects
                    .values()
                    .filter(|c| c.author == owner)
                    .map(|c| c.tick),
            );
        }
    }
    let card = |name: &String, repository: &Repository, prefix: &str| {
        let branch = default_branch(repository);
        let mut meta = div("meta").id(format!("{prefix}-meta-{name}"));
        if let Some((_, tip)) = branch_tip(repository, &branch) {
            if let Some((language, ..)) = language_stats(&tip.files).first() {
                meta = meta.child(
                    span("lang")
                        .child(
                            el("i")
                                .id(format!("{prefix}-dot-{name}"))
                                .class("dot")
                                .style(&format!("background: {}", language_color(language))),
                        )
                        .child(sp(&format!("{prefix}-lang-{name}"), "", language)),
                );
            }
        }
        meta = meta
            .child(
                span("meta-item")
                    .child(ic(&format!("{prefix}-star-icon-{name}"), "star"))
                    .child(sp(
                        &format!("{prefix}-stars-{name}"),
                        "",
                        repository.stars.len().to_string(),
                    )),
            )
            .child(
                span("meta-item")
                    .child(ic(&format!("{prefix}-fork-icon-{name}"), "fork"))
                    .child(sp(
                        &format!("{prefix}-forks-{name}"),
                        "",
                        repository.forks.to_string(),
                    )),
            );
        div("pin")
            .id(format!("{prefix}-{name}"))
            .child(
                div("pin-head")
                    .id(format!("{prefix}-head-{name}"))
                    .child(ic(&format!("{prefix}-icon-{name}"), "book"))
                    .child(a(
                        &format!("{prefix}-name-{name}"),
                        "pin-name",
                        format!("/{}/{name}", repository.owner),
                        name.as_str(),
                    ))
                    .child(sp(
                        &format!("{prefix}-vis-{name}"),
                        "pill",
                        if repository.readers.is_empty() {
                            "Public"
                        } else {
                            "Private"
                        },
                    )),
            )
            .child(
                el("p")
                    .id(format!("{prefix}-desc-{name}"))
                    .class("pin-desc")
                    .text(&repository.description),
            )
            .child(meta)
    };
    // Twenty-six weeks of squares, Sunday to Saturday down each column, newest at the right.
    const WEEKS: u64 = 26;
    let today = cx.now / 24;
    let today_weekday = (EPOCH_WEEKDAY + today) % 7;
    let mut counts = vec![[0u32; 7]; WEEKS as usize];
    for tick in &commits {
        let day = tick / 24;
        if day > today {
            continue;
        }
        let weekday = (EPOCH_WEEKDAY + day) % 7;
        // Weeks between the Sunday that starts this week and the one that starts today's.
        let weeks_back =
            ((today as i64 - today_weekday as i64) - (day as i64 - weekday as i64)) / 7;
        if (0..WEEKS as i64).contains(&weeks_back) {
            counts[(WEEKS as i64 - 1 - weeks_back) as usize][weekday as usize] += 1;
        }
    }
    let mut cells = div("cells");
    for (week, column) in counts.iter().enumerate() {
        for (weekday, count) in column.iter().enumerate() {
            cells = cells.child(
                el("i")
                    .id(format!("cell-{week}-{weekday}"))
                    .class(&format!("cell l{}", (*count).min(4)))
                    .attr("title", format!("{count} contributions")),
            );
        }
    }
    let mut profile = el("aside")
        .id("owner-profile")
        .class("profile")
        .child(avatar("owner-avatar", owner, 260).when(is_org, |n| n.class("square")))
        .child(el("h1").id("owner-name").text(owner))
        .child(el("p").id("owner-login").class("login").text(if is_org {
            "Organization"
        } else {
            "User"
        }));
    // Following, a location and a home page are things this instance does not know; the
    // one number it does know is how many people have starred the work.
    if !owned.is_empty() {
        let mut fans = std::collections::BTreeSet::new();
        for (_, repository) in &owned {
            fans.extend(repository.stars.iter().cloned());
        }
        profile = profile.child(
            el("p")
                .id("owner-followers")
                .class("profile-line")
                .child(ic("owner-followers-icon", "star"))
                .child(sp(
                    "owner-followers-count",
                    "strong",
                    fans.len().to_string(),
                ))
                .child(sp(
                    "owner-followers-label",
                    "",
                    format!(
                        " {} starred {} {} repositor{}",
                        if fans.len() == 1 {
                            "person has"
                        } else {
                            "people have"
                        },
                        if is_org { "its" } else { "their" },
                        owned.len(),
                        if owned.len() == 1 { "y" } else { "ies" }
                    ),
                )),
        );
    }
    let o = |id: &str, icon: &str, text: &str, count: Option<usize>, route: &str, on: bool| {
        tab(
            id,
            icon,
            text,
            count,
            if route.is_empty() {
                format!("/{owner}")
            } else {
                html::href(&format!("/{owner}"), &[("tab", route)])
            },
            on,
        )
    };
    let mut strip = vec![
        o(
            "otab-overview",
            "book",
            "Overview",
            None,
            "",
            tab_name.is_empty(),
        ),
        o(
            "otab-repos",
            "archive",
            "Repositories",
            Some(owned.len()),
            "repositories",
            tab_name == "repositories",
        ),
    ];
    if !starred.is_empty() {
        strip.push(o(
            "otab-stars",
            "star",
            "Stars",
            Some(starred.len()),
            "stars",
            tab_name == "stars",
        ));
    }
    let mut main = div("profile-main")
        .id("owner-main")
        .child(tabs("owner-tabs", "tabs", strip));
    main = match tab_name {
        "repositories" => {
            let heading = format!(
                "{} repositor{}",
                owned.len(),
                if owned.len() == 1 { "y" } else { "ies" }
            );
            main.child(
                el("h2")
                    .id("pinned-title")
                    .class("section-title")
                    .text(heading),
            )
            .child(
                div("pins")
                    .id("owned")
                    .children(
                        owned
                            .iter()
                            .map(|(name, repository)| card(name, repository, "owned")),
                    )
                    .when(owned.is_empty(), |n| {
                        n.child(
                            el("p")
                                .id("owned-none")
                                .class("muted")
                                .text(format!("{owner} owns no repositories here.")),
                        )
                    }),
            )
        }
        "stars" => main
            .child(
                el("h2")
                    .id("starred-title")
                    .class("section-title")
                    .text(format!(
                        "{} starred repositor{}",
                        starred.len(),
                        if starred.len() == 1 { "y" } else { "ies" }
                    )),
            )
            .child(
                div("pins")
                    .id("starred")
                    .children(
                        starred
                            .iter()
                            .map(|(name, repository)| card(name, repository, "starred")),
                    )
                    .when(starred.is_empty(), |n| {
                        n.child(
                            el("p")
                                .id("starred-none")
                                .class("muted")
                                .text(format!("{owner} has not starred anything here.")),
                        )
                    }),
            ),
        _ => {
            main.child(el("h2").id("pinned-title").class("section-title").text(
                if owned.is_empty() {
                    "Repositories"
                } else {
                    "Popular repositories"
                },
            ))
            .child(
                div("pins")
                    .id("owned")
                    .children(
                        owned
                            .iter()
                            .take(6)
                            .map(|(name, repository)| card(name, repository, "owned")),
                    )
                    .when(owned.is_empty(), |n| {
                        n.child(
                            el("p")
                                .id("owned-none")
                                .class("muted")
                                .text(format!("{owner} owns no repositories here.")),
                        )
                    }),
            )
            .child(
                el("h2")
                    .id("contrib-title")
                    .class("section-title")
                    .text(format!(
                        "{} contribution{} in the last year",
                        commits.len(),
                        plural(commits.len())
                    )),
            )
            .child(
                div("box graph").id("graph").child(cells).child(
                    div("legend")
                        .id("graph-legend")
                        .child(sp("legend-less", "", "Less"))
                        .each(0..5, |l| {
                            el("i")
                                .id(format!("legend-{l}"))
                                .class(&format!("cell l{l}"))
                        })
                        .child(sp("legend-more", "", "More")),
                ),
            )
        }
    };
    let body = shell(
        cx,
        &[(owner, format!("/{owner}"))],
        None,
        vec![div("owner").id("owner").child(profile).child(main)],
    );
    finish(cx, &format!("{owner} · {}", cx.look.brand()), body)
}

/// The file table for `prefix` of `branch`, with the latest-commit bar above it.
fn file_table(
    cx: &Cx,
    repository: &Repository,
    name: &str,
    branch: &str,
    tip_id: &str,
    tip: &Commit,
    prefix: &str,
) -> Html {
    let path = slug(repository, name);
    let last = last_commit_per_path(repository, tip_id);
    let history = log(repository, tip_id);
    let mut table = div("box files").id("files").child(
        div("box-head latest")
            .id("latest")
            .child(avatar("latest-avatar", &tip.author, 20))
            .child(a(
                "latest-author",
                "strong-link",
                format!("/{}", tip.author),
                &tip.author,
            ))
            .child(a(
                "latest-message",
                "latest-message",
                format!("/{path}/commit/{tip_id}"),
                tip.message.lines().next().unwrap_or(""),
            ))
            .child(span("grow"))
            .child(a(
                "latest-sha",
                "sha",
                format!("/{path}/commit/{tip_id}"),
                short(tip_id),
            ))
            .child(sp(
                "latest-when",
                "muted",
                format!("· {}", ago(cx.now, tip.tick)),
            ))
            .child(
                el("a")
                    .id("history")
                    .class("history")
                    .attr("href", format!("/{path}/commits/{branch}"))
                    .child(ic("history-icon", "clock"))
                    .child(Html::from(format!(
                        "{} Commit{}",
                        history.len(),
                        plural(history.len())
                    ))),
            ),
    );
    if !prefix.is_empty() {
        let parent = prefix.rsplit_once('/').map_or("", |(p, _)| p);
        table = table.child(
            div("entry").id("entry-up").child(
                div("entry-name")
                    .child(ic("entry-up-icon", "folder"))
                    .child(a(
                        "entry-up-link",
                        "entry-link",
                        if parent.is_empty() {
                            format!("/{path}")
                        } else {
                            format!("/{path}/tree/{branch}/{parent}")
                        },
                        "..",
                    )),
            ),
        );
    }
    for (i, entry) in list_tree(&tip.files, prefix).iter().enumerate() {
        // A folder's last commit is the newest commit to anything beneath it.
        let touched = last
            .iter()
            .filter(|(p, _)| {
                if entry.dir {
                    p.starts_with(&format!("{}/", entry.path))
                } else {
                    **p == entry.path
                }
            })
            .map(|(_, (id, c))| (c.tick, id.clone(), c.message.clone()))
            .max();
        let url = if entry.dir {
            format!("/{path}/tree/{branch}/{}", entry.path)
        } else {
            format!("/{path}/blob/{branch}/{}", entry.path)
        };
        let mut row = div("entry").id(format!("entry-{i}")).child(
            div("entry-name")
                .child(ic(
                    &format!("entry-icon-{i}"),
                    if entry.dir { "folder" } else { "file" },
                ))
                .child(a(&format!("file-{i}"), "entry-link", url, &entry.name)),
        );
        if let Some((tick, id, message)) = touched {
            row = row
                .child(div("entry-message").child(a(
                    &format!("entry-message-{i}"),
                    "",
                    format!("/{path}/commit/{id}"),
                    message.lines().next().unwrap_or(""),
                )))
                .child(sp(
                    &format!("entry-when-{i}"),
                    "entry-when",
                    ago(cx.now, tick),
                ));
        }
        table = table.child(row);
    }
    table
}
fn branch_select(path: &str, branch: &str) -> Html {
    // The real one drops a menu open; this one goes to the page that lists the branches,
    // so there is no chevron promising a menu.
    el("a")
        .id("branch-select")
        .class("btn branch-select")
        .attr("href", format!("/{path}/branches"))
        .attr("title", "Switch branches")
        .child(ic("branch-icon", "branch"))
        .child(sp("branch-name", "", branch))
}
fn branch_bar(
    cx: &Cx,
    repository: &Repository,
    name: &str,
    branch: &str,
    count_tags: usize,
) -> Html {
    let path = slug(repository, name);
    div("branch-bar")
        .id("branch-bar")
        .child(branch_select(&path, branch))
        .child(
            el("a")
                .id("branches")
                .class("quiet")
                .attr("href", format!("/{path}/branches"))
                .child(ic("branches-icon", "branch"))
                .child(Html::from({
                    let n = branches(repository).len();
                    format!("{n} {}", if n == 1 { "Branch" } else { "Branches" })
                })),
        )
        // Tags are a statistic here: this instance stores heads, not tags, so the count
        // is read rather than clicked.
        .child(
            span("quiet")
                .id("tags")
                .child(ic("tags-icon", "tag"))
                .child(Html::from(format!(
                    "{count_tags} Tag{}",
                    plural(count_tags)
                ))),
        )
        .child(span("grow"))
        .child(btn_link(
            "go-to-file",
            if cx.look == Look::Gitlab {
                "Find file"
            } else {
                "Go to file"
            },
            format!("/{path}/find/{branch}"),
        ))
        // "Code" opens a clone dialog on the real thing. Here it goes to the page this
        // server actually serves the source from, which is the thing that dialog is about.
        .child(
            el("a")
                .id("code")
                .class("btn btn-primary")
                .attr("href", format!("/repos/{name}"))
                .attr("title", "Browse and clone the source over HTTP")
                .child(ic("", "code"))
                .child(Html::from("Code")),
        )
}
fn about(cx: &Cx, repository: &Repository, name: &str, tip: &Commit) -> Html {
    let path = slug(repository, name);
    let mut side = el("aside")
        .id("about")
        .class("about")
        .child(el("h2").id("about-title").text(if cx.look == Look::Gitlab {
            "Project information"
        } else {
            "About"
        }));
    side = side.child(
        el("p")
            .id("repo-description")
            .class("about-desc")
            .text(&repository.description),
    );
    if !repository.topics.is_empty() {
        // A topic searches for itself, the way it does on the real thing.
        side = side.child(div("topics").id("repo-topics").each(
            repository.topics.iter().enumerate(),
            |(i, t)| {
                a(
                    &format!("topic-{i}"),
                    "topic",
                    html::href("/search", &[("q", t)]),
                    t.as_str(),
                )
            },
        ));
    }
    let branch = default_branch(repository);
    let line = |id: &str, icon: &str, text: String, url: String| {
        div("about-line")
            .id(id)
            .child(ic(&format!("{id}-icon"), icon))
            .child(a(&format!("{id}-link"), "", url, text))
    };
    let fact = |id: &str, icon: &str, text: String| {
        div("about-line")
            .id(id)
            .child(ic(&format!("{id}-icon"), icon))
            .child(sp(&format!("{id}-text"), "", text))
    };
    // Readme and licence point at the files themselves; the counts point at the people.
    if let Some(readme) = tip
        .files
        .keys()
        .find(|f| f.eq_ignore_ascii_case("README.md"))
    {
        side = side.child(line(
            "about-readme",
            "book",
            "Readme".into(),
            format!("/{path}/blob/{branch}/{readme}"),
        ));
    }
    if let Some(license) = tip.files.keys().find(|f| f.eq_ignore_ascii_case("LICENSE")) {
        side = side.child(line(
            "about-license",
            "law",
            "MIT license".into(),
            format!("/{path}/blob/{branch}/{license}"),
        ));
    }
    side = side
        .child(line(
            "about-activity",
            "signal",
            "Activity".into(),
            format!("/{path}/commits/{branch}"),
        ))
        .child(line(
            "about-stars",
            "star",
            format!(
                "{} star{}",
                repository.stars.len(),
                plural(repository.stars.len())
            ),
            format!("/{path}/stargazers"),
        ))
        .child(line(
            "about-watching",
            "eye",
            format!("{} watching", repository.watchers.len()),
            format!("/{path}/watchers"),
        ))
        .child(fact(
            "about-forks",
            "fork",
            format!(
                "{} fork{}",
                repository.forks,
                plural(repository.forks as usize)
            ),
        ));
    let mut authors: Vec<&str> = repository
        .objects
        .values()
        .map(|c| c.author.as_str())
        .collect();
    authors.sort_unstable();
    authors.dedup();
    side = side
        .child(
            div("about-section")
                .child(el("h3").id("releases-title").text("Releases"))
                .child(
                    el("p")
                        .id("releases-none")
                        .class("muted small")
                        .text("No releases published"),
                ),
        )
        .child(
            div("about-section")
                // A count beside a heading, not a badge on a button: in the GitLab sheet
                // the counter bubble is painted exactly like a button, and this one is
                // attached to nothing that can be pressed.
                .child(
                    el("h3")
                        .id("contributors-title")
                        .text("Contributors ")
                        .child(sp("contributors-count", "muted", authors.len().to_string())),
                )
                .child(
                    div("faces")
                        .id("contributors")
                        .each(authors.iter().take(8), |who| {
                            avatar(&format!("contributor-{who}"), who, 32)
                        }),
                ),
        );
    let stats = language_stats(&tip.files);
    let mut languages =
        div("about-section").child(el("h3").id("languages-title").text("Languages"));
    if stats.is_empty() {
        languages = languages.child(
            el("p")
                .id("languages-none")
                .class("muted small")
                .text("No languages detected"),
        );
    } else {
        languages = languages
            .child(div("language-bar").id("language-bar").each(
                stats.iter().enumerate(),
                |(i, (language, _, share))| {
                    el("i").id(format!("language-bar-{i}")).style(&format!(
                        "background: {}; flex-grow: {}",
                        language_color(language),
                        (*share).max(8)
                    ))
                },
            ))
            .child(el("ul").id("language-list").class("language-list").each(
                stats.iter().enumerate(),
                |(i, (language, _, share))| {
                    el("li")
                        .id(format!("language-{i}"))
                        .child(
                            el("i")
                                .id(format!("language-dot-{i}"))
                                .class("dot")
                                .style(&format!("background: {}", language_color(language))),
                        )
                        .child(sp(&format!("language-name-{i}"), "strong", language))
                        .child(sp(
                            &format!("language-share-{i}"),
                            "muted",
                            format!("{}.{}%", share / 10, share % 10),
                        ))
                },
            ));
    }
    side.child(languages)
}
/// The README below the file table. The real one has a tab strip beside it that swaps in
/// the licence; here the heading names what is shown, and About links to the licence file.
fn readme_card(tip: &Commit, prefix: &str) -> Option<Html> {
    let (_, readme) = tip.files.iter().find(|(f, _)| {
        let dir = f.rsplit_once('/').map_or("", |(d, _)| d);
        dir == prefix
            && f.rsplit('/')
                .next()
                .unwrap_or(f)
                .eq_ignore_ascii_case("README.md")
    })?;
    Some(
        div("box readme")
            .id("readme")
            .child(
                div("readme-head").id("readme-head").child(
                    span("readme-tab active")
                        .child(ic("readme-icon", "book"))
                        .child(sp("readme-title", "", "README")),
                ),
            )
            .child(
                el("article")
                    .id("readme-body")
                    .class("markdown")
                    .children(markdown("readme", readme)),
            ),
    )
}
/// `name / dir / file` above a tree or a blob; the last part is where you are.
fn path_crumbs(id: &str, path: &str, name: &str, branch: &str, target: &str) -> Html {
    let mut crumbs =
        div("path-crumbs")
            .id(id)
            .child(a("crumb-root", "root", format!("/{path}"), name));
    let parts: Vec<&str> = target.split('/').collect();
    let mut so_far = String::new();
    for (i, part) in parts.iter().enumerate() {
        if !so_far.is_empty() {
            so_far.push('/');
        }
        so_far.push_str(part);
        crumbs = crumbs.child(sp(&format!("crumb-slash-{i}"), "sep", "/"));
        crumbs = crumbs.child(if i + 1 == parts.len() && id == "blob-crumbs" {
            sp(&format!("crumb-part-{i}"), "here", *part)
        } else {
            a(
                &format!("crumb-part-{i}"),
                if i + 1 == parts.len() { "here" } else { "" },
                format!("/{path}/tree/{branch}/{so_far}"),
                *part,
            )
        });
    }
    crumbs
}
fn code_page(
    cx: &Cx,
    repository: &Repository,
    name: &str,
    branch: &str,
    prefix: &str,
) -> Result<HttpResponse> {
    let path = slug(repository, name);
    let Some((tip_id, tip)) = resolve(repository, branch) else {
        return web::error(404, "branch not found");
    };
    if !prefix.is_empty()
        && !tip
            .files
            .keys()
            .any(|f| f.starts_with(&format!("{prefix}/")))
    {
        return web::error(404, "path not found");
    }
    let mut left = div("code-main")
        .id("code-main")
        .child(branch_bar(cx, repository, name, branch, 0));
    if !prefix.is_empty() {
        left = left.child(path_crumbs("tree-crumbs", &path, name, branch, prefix));
    }
    left = left
        .child(file_table(
            cx, repository, name, branch, &tip_id, tip, prefix,
        ))
        .maybe(readme_card(tip, prefix));
    let mut columns = div("code-columns").id("code-columns").child(left);
    if prefix.is_empty() {
        columns = columns.child(about(cx, repository, name, tip));
    }
    let title = if prefix.is_empty() {
        format!("{path}: {}", repository.description)
    } else {
        format!("{path}/{prefix} at {branch}")
    };
    finish(
        cx,
        &format!("{title} · {}", cx.look.brand()),
        repo_frame(cx, repository, name, Tab::Code, vec![columns]),
    )
}

/// "Go to file": every path in the tree, narrowed by a substring. The real one filters in
/// the browser; this one submits the same box to the same route and filters on the server.
fn find_page(
    cx: &Cx,
    repository: &Repository,
    name: &str,
    branch: &str,
    query: &str,
) -> Result<HttpResponse> {
    let path = slug(repository, name);
    let Some((_, tip)) = resolve(repository, branch) else {
        return web::error(404, "branch not found");
    };
    let needle = query.to_ascii_lowercase();
    let hits: Vec<&String> = tip
        .files
        .keys()
        .filter(|f| f.to_ascii_lowercase().contains(&needle))
        .collect();
    let mut list = div("box files").id("find-results");
    for (i, file) in hits.iter().enumerate() {
        list = list.child(
            div("entry").id(format!("find-{i}")).child(
                div("entry-name")
                    .child(ic(&format!("find-icon-{i}"), "file"))
                    .child(a(
                        &format!("find-link-{i}"),
                        "entry-link mono",
                        format!("/{path}/blob/{branch}/{file}"),
                        file.as_str(),
                    )),
            ),
        );
    }
    let body = vec![
        el("h1")
            .id("find-title")
            .class("page-title")
            .text(if cx.look == Look::Gitlab {
                "Find file"
            } else {
                "Go to file"
            }),
        form("find", format!("/{path}/find/{branch}"), "get")
            .class("branch-bar")
            .child(branch_select(&path, branch))
            .child(
                text_input("find-q", "q", query)
                    .attr("aria-label", "Filter files by name")
                    .attr("placeholder", "Filter files by name"),
            )
            .child(button("find-submit", "Search").class("btn")),
        el("p").id("find-count").class("muted").text(format!(
            "{} of {} file{} on {branch}",
            hits.len(),
            tip.files.len(),
            plural(tip.files.len())
        )),
        list,
    ];
    finish(
        cx,
        &format!("Find a file · {path}"),
        repo_frame(cx, repository, name, Tab::Code, body),
    )
}

fn blob_page(
    cx: &Cx,
    repository: &Repository,
    name: &str,
    branch: &str,
    file: &str,
) -> Result<HttpResponse> {
    let path = slug(repository, name);
    let Some((tip_id, tip)) = resolve(repository, branch) else {
        return web::error(404, "branch not found");
    };
    let Some(content) = tip.files.get(file) else {
        return web::error(404, "file not found");
    };
    let lines: Vec<&str> = content.lines().collect();
    let loc = lines.iter().filter(|l| !l.trim().is_empty()).count();
    let numbers: Vec<String> = (1..=lines.len()).map(|n| n.to_string()).collect();
    let last = last_commit_per_path(repository, &tip_id);
    let mut body = vec![
        branch_bar(cx, repository, name, branch, 0),
        path_crumbs("blob-crumbs", &path, name, branch, file),
    ];
    if let Some((id, commit)) = last.get(file) {
        body.push(
            div("box latest-box").id("blob-latest").child(
                div("latest")
                    .child(avatar("blob-latest-avatar", &commit.author, 20))
                    .child(a(
                        "blob-latest-author",
                        "strong-link",
                        format!("/{}", commit.author),
                        &commit.author,
                    ))
                    .child(sp(
                        "blob-latest-message",
                        "latest-message",
                        commit.message.lines().next().unwrap_or(""),
                    ))
                    .child(span("grow"))
                    .child(a(
                        "blob-latest-sha",
                        "sha",
                        format!("/{path}/commit/{id}"),
                        short(id),
                    ))
                    .child(sp(
                        "blob-latest-when",
                        "muted",
                        format!("· {}", ago(cx.now, commit.tick)),
                    ))
                    .child(
                        el("a")
                            .id("blob-history")
                            .class("history")
                            .attr("href", format!("/{path}/commits/{branch}"))
                            .child(ic("blob-history-icon", "clock"))
                            .child(Html::from("History")),
                    ),
            ),
        );
    }
    // Copy, download, edit and "more" are the browser's clipboard and the site's editor on
    // the real thing; neither exists here. Raw stays, and really serves the bytes.
    let head = div("box-head blob-head")
        .id("blob-head")
        .child(sp("blob-code-tab", "strong small", "Code"))
        .child(sp(
            "blob-stats",
            "muted small",
            format!(
                "{} lines ({loc} loc) · {} Bytes",
                lines.len(),
                content.len()
            ),
        ))
        .child(span("grow"))
        .child(a(
            "raw",
            "btn btn-sm",
            format!("/{path}/raw/{branch}/{file}"),
            "Raw",
        ));
    let code = div("blob-body")
        .id("blob-body")
        .child(
            el("pre")
                .id("line-numbers")
                .class("line-numbers")
                .attr("aria-hidden", "true")
                .text(numbers.join("\n")),
        )
        .child(
            el("pre")
                .id("blob-text")
                .class("blob-text")
                .text(content.trim_end_matches('\n')),
        );
    body.push(div("box blob").id("blob").child(head).child(code));
    finish(
        cx,
        &format!("{path}/{file} at {branch} · {}", cx.look.brand()),
        repo_frame(cx, repository, name, Tab::Code, body),
    )
}

/// One commit as a row of a history list.
fn commit_row(cx: &Cx, prefix: &str, path: &str, id: &str, commit: &Commit) -> Html {
    let title = commit.message.lines().next().unwrap_or("").to_owned();
    div("commit-row")
        .id(prefix)
        .child(
            div("commit-main")
                .id(format!("{prefix}-main"))
                .child(a(
                    &format!("{prefix}-title"),
                    "commit-title",
                    format!("/{path}/commit/{id}"),
                    title,
                ))
                .child(
                    div("commit-meta")
                        .id(format!("{prefix}-meta"))
                        .child(avatar(&format!("{prefix}-avatar"), &commit.author, 20))
                        .child(a(
                            &format!("{prefix}-author"),
                            "strong-link",
                            format!("/{}", commit.author),
                            &commit.author,
                        ))
                        .child(sp(
                            &format!("{prefix}-when"),
                            "muted",
                            format!("committed {}", ago(cx.now, commit.tick)),
                        )),
                ),
        )
        .child(
            // No clipboard here, so the "copy SHA" button is gone; browsing the tree at a
            // commit is real, because a tree page takes a commit id as its revision.
            div("commit-side")
                .id(format!("{prefix}-side"))
                .child(a(
                    &format!("{prefix}-sha"),
                    "sha-btn",
                    format!("/{path}/commit/{id}"),
                    short(id),
                ))
                .child(
                    el("a")
                        .id(format!("{prefix}-browse"))
                        .class("icon-btn quiet-btn")
                        .attr("href", format!("/{path}/tree/{id}"))
                        .attr(
                            "title",
                            "Browse the repository at this point in the history",
                        )
                        .attr(
                            "aria-label",
                            "Browse the repository at this point in the history",
                        )
                        .child(ic("", "code")),
                ),
        )
}
/// Commits grouped by day, the way the history page reads.
fn commit_groups(cx: &Cx, prefix: &str, path: &str, history: &[(String, &Commit)]) -> Html {
    let mut out = div("timeline-commits");
    let mut day: Option<u64> = None;
    let mut rows: Vec<Html> = vec![];
    let mut group = 0usize;
    let close = |rows: &mut Vec<Html>, out: &mut Html, group: usize| {
        if !rows.is_empty() {
            let boxed = div("box commit-group")
                .id(format!("{prefix}-group-{group}"))
                .children(std::mem::take(rows));
            *out = std::mem::replace(out, empty()).child(boxed);
        }
    };
    for (i, (id, commit)) in history.iter().enumerate() {
        let this = commit.tick / 24;
        if day != Some(this) {
            close(&mut rows, &mut out, group);
            group += 1;
            day = Some(this);
            out = out.child(
                div("commit-day")
                    .id(format!("{prefix}-day-{group}"))
                    .child(ic(&format!("{prefix}-day-icon-{group}"), "commit"))
                    .child(sp(
                        &format!("{prefix}-day-label-{group}"),
                        "",
                        format!("Commits on {}", date(commit.tick)),
                    )),
            );
        }
        rows.push(commit_row(cx, &format!("{prefix}-{i}"), path, id, commit));
    }
    close(&mut rows, &mut out, group);
    out
}
fn commits_page(
    cx: &Cx,
    repository: &Repository,
    name: &str,
    branch: &str,
) -> Result<HttpResponse> {
    let path = slug(repository, name);
    let Some((tip_id, _)) = resolve(repository, branch) else {
        return web::error(404, "branch not found");
    };
    let history = log(repository, &tip_id);
    let body = vec![
        el("h1")
            .id("commits-title")
            .class("page-title ruled")
            .text("Commits"),
        // The author and date menus the real page carries filter in the browser; with no
        // filter to apply they would only reload this page, so the bar is the selector.
        div("branch-bar")
            .id("commits-bar")
            .child(branch_select(&path, branch))
            .child(span("grow")),
        commit_groups(cx, "commits", &path, &history),
    ];
    finish(
        cx,
        &format!("Commits · {path}"),
        repo_frame(cx, repository, name, Tab::Code, body),
    )
}
fn commit_page(cx: &Cx, repository: &Repository, name: &str, id: &str) -> Result<HttpResponse> {
    let path = slug(repository, name);
    let Some(commit) = repository.objects.get(id) else {
        return web::error(404, "commit not found");
    };
    let none = BTreeMap::new();
    let parent = commit
        .parents
        .first()
        .and_then(|p| repository.objects.get(p))
        .map_or(&none, |p| &p.files);
    let files = diff_trees(parent, &commit.files);
    let on: Vec<String> = branches(repository)
        .into_iter()
        .filter(|b| {
            branch_tip(repository, b)
                .is_some_and(|(tip, _)| log(repository, &tip).iter().any(|(c, _)| c == id))
        })
        .collect();
    let mut lines = commit.message.lines();
    let title = lines.next().unwrap_or("").to_owned();
    let rest: Vec<&str> = lines.filter(|l| !l.trim().is_empty()).collect();
    let home_branch = on.first().map_or("main", |b| b.as_str());
    let mut head = div("commit-head-main")
        .id("commit-head-main")
        .child(el("h1").id("commit-title").text(title));
    if !rest.is_empty() {
        head = head.child(
            el("pre")
                .id("commit-body")
                .class("commit-body")
                .text(rest.join("\n")),
        );
    }
    let mut on_branches = div("commit-branches").id("commit-branches");
    for (i, b) in on.iter().enumerate() {
        on_branches = on_branches.child(
            span("commit-branch")
                .id(format!("commit-branch-{i}"))
                .child(ic(&format!("commit-branch-icon-{i}"), "branch"))
                .child(a(
                    &format!("commit-branch-link-{i}"),
                    "",
                    format!("/{path}/tree/{b}"),
                    b,
                )),
        );
    }
    if on.is_empty() {
        on_branches = on_branches.child(sp("commit-branch-none", "muted", "not on any branch"));
    }
    head = head.child(on_branches);
    let mut parents = span("commit-parents").child(sp(
        "commit-parents-label",
        "muted",
        format!(
            "{} parent{} ",
            commit.parents.len(),
            plural(commit.parents.len())
        ),
    ));
    for (i, p) in commit.parents.iter().enumerate() {
        parents = parents
            .child(a(
                &format!("commit-parent-{i}"),
                "sha",
                format!("/{path}/commit/{p}"),
                short(p),
            ))
            .child(Html::from(" "));
    }
    parents = parents
        .child(sp("commit-sha-label", "muted", "commit "))
        .child(sp("commit-sha", "sha", id));
    let card = div("box commit-card")
        .id("commit-card")
        .child(
            div("commit-head")
                .id("commit-head")
                .child(head)
                .child(btn_link(
                    "browse-files",
                    "Browse files",
                    format!("/{path}/tree/{home_branch}"),
                )),
        )
        .child(
            div("commit-meta-bar")
                .id("commit-meta")
                .child(avatar("commit-avatar", &commit.author, 20))
                .child(a(
                    "commit-author",
                    "strong-link",
                    format!("/{}", commit.author),
                    &commit.author,
                ))
                .child(sp(
                    "commit-when",
                    "muted",
                    format!(
                        "committed on {} · {}",
                        date(commit.tick),
                        ago(cx.now, commit.tick)
                    ),
                ))
                .child(span("grow"))
                .child(parents),
        );
    let mut body = vec![card];
    body.extend(diff_section("diff", &files, &path, home_branch));
    finish(
        cx,
        &format!(
            "{} · {path}@{}",
            commit.message.lines().next().unwrap_or(""),
            short(id)
        ),
        repo_frame(cx, repository, name, Tab::Code, body),
    )
}
fn branches_page(cx: &Cx, repository: &Repository, name: &str) -> Result<HttpResponse> {
    let path = slug(repository, name);
    let default = default_branch(repository);
    let main_tip = branch_tip(repository, &default).map(|(id, _)| id);
    let mut list = div("box branches").id("branches-list").child(
        div("box-head branch-row head")
            .id("branches-head")
            .child(sp("branches-head-label", "branch-cell name", "Branch"))
            .child(sp(
                "branches-head-updated",
                "branch-cell updated",
                "Updated",
            ))
            .child(sp(
                "branches-head-behind",
                "branch-cell behind",
                "Behind | Ahead",
            ))
            .child(sp(
                "branches-head-pr",
                "branch-cell pr",
                cx.look.pulls().trim_end_matches('s'),
            )),
    );
    for (i, branch) in branches(repository).iter().enumerate() {
        let Some((tip, commit)) = branch_tip(repository, branch) else {
            continue;
        };
        let mut name_cell = span("branch-cell name")
            .child(ic(&format!("branch-icon-{i}"), "branch"))
            .child(a(
                &format!("branch-{i}"),
                "ref",
                format!("/{path}/tree/{branch}"),
                branch,
            ));
        if *branch == default {
            name_cell = name_cell.child(sp(&format!("branch-default-{i}"), "pill", "Default"));
        }
        let mut behind_cell = span("branch-cell behind");
        if let Some(main_tip) = &main_tip {
            if *branch != default {
                let ahead = commits_between(repository, main_tip, &tip).len();
                let behind = commits_between(repository, &tip, main_tip).len();
                behind_cell = behind_cell.child(sp(
                    &format!("branch-ahead-{i}"),
                    "muted",
                    format!("{behind} behind · {ahead} ahead"),
                ));
            }
        }
        let mut pr_cell = span("branch-cell pr");
        if let Some(pull) = repository
            .pull_requests
            .values()
            .find(|p| p.head == format!("refs/heads/{branch}"))
        {
            pr_cell = pr_cell
                .child(state_icon(&format!("branch-pr-icon-{i}"), pull, true))
                .child(a(
                    &format!("branch-pr-{i}"),
                    "",
                    format!("/{path}/pull/{}", pull.number),
                    format!("#{}", pull.number),
                ));
        } else if *branch != default {
            pr_cell = pr_cell.child(a(
                &format!("branch-new-pr-{i}"),
                "btn btn-sm",
                format!("/{path}/compare"),
                format!("New {}", cx.look.pull()),
            ));
        }
        list = list.child(
            div("branch-row")
                .id(format!("branch-row-{i}"))
                .child(name_cell)
                .child(
                    span("branch-cell updated")
                        .child(avatar("", &commit.author, 16))
                        .child(sp(
                            &format!("branch-updated-{i}"),
                            "muted",
                            format!("Updated {} by {}", ago(cx.now, commit.tick), commit.author),
                        )),
                )
                .child(behind_cell)
                .child(pr_cell),
        );
    }
    // Overview / Yours / Active / Stale / All are five views of one list on the real page;
    // this one has a single list, so it is not dressed up as five tabs.
    let body = vec![
        el("h1")
            .id("branches-title")
            .class("page-title")
            .text("Branches"),
        el("p").id("branches-sub").class("muted").text(format!(
            "Every branch of {path}, newest commit first in each row."
        )),
        list,
    ];
    finish(
        cx,
        &format!("Branches · {path}"),
        repo_frame(cx, repository, name, Tab::Code, body),
    )
}

/// Stargazers, and the same page for watchers: two sets of people the repository keeps.
fn people_page(
    cx: &Cx,
    repository: &Repository,
    name: &str,
    watchers: bool,
) -> Result<HttpResponse> {
    let path = slug(repository, name);
    let who = if watchers {
        &repository.watchers
    } else {
        &repository.stars
    };
    let prefix = if watchers { "watcher" } else { "stargazer" };
    let cards = who.iter().enumerate().map(|(i, name)| {
        div("stargazer")
            .id(format!("{prefix}-{i}"))
            .child(avatar(&format!("{prefix}-avatar-{i}"), name, 48))
            .child(
                div("stargazer-text")
                    .id(format!("{prefix}-text-{i}"))
                    .child(a(
                        &format!("{prefix}-name-{i}"),
                        "stargazer-name",
                        format!("/{name}"),
                        name.as_str(),
                    ))
                    .child(sp(
                        &format!("{prefix}-login-{i}"),
                        "muted small",
                        format!("@{name}"),
                    )),
            )
    });
    let n = who.len();
    let (title, count_id, sentence) = if watchers {
        (
            "Watchers",
            "watchers-count",
            format!(
                "{n} {} watching {path}",
                if n == 1 { "person is" } else { "people are" }
            ),
        )
    } else {
        (
            "Stargazers",
            "stars-count",
            format!(
                "{n} {} starred {path}",
                if n == 1 { "person" } else { "people" }
            ),
        )
    };
    let body = vec![
        el("h1")
            .id(if watchers {
                "watchers-title"
            } else {
                "stars-title"
            })
            .class("page-title ruled")
            .text(title),
        el("p").id(count_id).class("muted").text(sentence),
        div("stargazers")
            .id(format!("{prefix}-grid"))
            .children(cards),
    ];
    finish(
        cx,
        &format!("{title} · {path}"),
        repo_frame(cx, repository, name, Tab::Code, body),
    )
}

fn list_page(
    cx: &Cx,
    repository: &Repository,
    name: &str,
    pulls: bool,
    filter: &str,
    query: &str,
) -> Result<HttpResponse> {
    let path = slug(repository, name);
    let threads = if pulls {
        &repository.pull_requests
    } else {
        &repository.issues
    };
    let (title, route, kind) = if pulls {
        (cx.look.pulls(), "pull", "pr")
    } else {
        ("Issues", "issues", "issue")
    };
    let list_route = if pulls { "pulls" } else { "issues" };
    let closed = filter == "closed";
    let open = open_count(threads);
    let shut = threads.len() - open;
    let list_url = |state: &str| {
        html::href(
            &format!("/{path}/{list_route}"),
            &[("state", state), ("q", query)],
        )
    };
    let toggle = |id: &str, icon: &str, text: String, url: String, on: bool| {
        el("a")
            .id(id)
            .class(if on {
                "list-toggle active"
            } else {
                "list-toggle"
            })
            .attr("href", url)
            .child(ic(&format!("{id}-icon"), icon))
            .child(Html::from(text))
    };
    // The real head carries six menus that filter in the browser. The one filter this
    // list can really apply is the open/closed split and the text box below it.
    let head = div("box-head list-head")
        .id("list-head")
        .child(toggle(
            "list-open",
            if pulls { "pull-request" } else { "issue-open" },
            format!("{open} Open"),
            list_url("open"),
            !closed,
        ))
        .child(toggle(
            "list-closed",
            "check",
            format!("{shut} Closed"),
            list_url("closed"),
            closed,
        ))
        .child(span("grow"));
    let mut list = div("box threads").id("threads").child(head);
    let needle = query.to_ascii_lowercase();
    let mut shown: Vec<&Thread> = threads
        .values()
        .filter(|t| (t.state == "open") != closed)
        .filter(|t| {
            needle.is_empty()
                || t.title.to_ascii_lowercase().contains(&needle)
                || t.body.to_ascii_lowercase().contains(&needle)
                || t.labels
                    .iter()
                    .any(|l| l.to_ascii_lowercase().contains(&needle))
        })
        .collect();
    shown.sort_by_key(|t| std::cmp::Reverse(t.number));
    if shown.is_empty() {
        list = list.child(
            div("blank")
                .id("empty")
                .child(ic("", if pulls { "pull-request" } else { "issue-open" }))
                .child(el("h3").text(if needle.is_empty() {
                    "Nothing here yet."
                } else {
                    "No results matched your search."
                }))
                .child(el("p").class("muted").text(if needle.is_empty() {
                    format!(
                        "There are no {} {} in this repository.",
                        if closed { "closed" } else { "open" },
                        if pulls { "pull requests" } else { "issues" }
                    )
                } else {
                    format!(
                        "Nothing {} matches \u{201c}{query}\u{201d}.",
                        if closed { "closed" } else { "open" }
                    )
                })),
        );
    }
    for thread in shown {
        let n = thread.number;
        let status = match thread.state.as_str() {
            "merged" => format!(
                "by {} was merged {}",
                thread.author,
                ago(cx.now, thread.tick)
            ),
            "closed" => format!(
                "by {} was closed {}",
                thread.author,
                ago(cx.now, thread.tick)
            ),
            _ => format!("opened {} by {}", ago(cx.now, thread.tick), thread.author),
        };
        let mut meta = div("row-meta").id(format!("row-meta-{n}")).child(sp(
            &format!("meta-{n}"),
            "",
            format!("#{n} {status}"),
        ));
        if pulls && thread.draft {
            meta = meta.child(sp(&format!("meta-draft-{n}"), "", " · Draft"));
        }
        if !thread.reviews.is_empty() {
            let approved = thread.reviews.iter().any(|r| r.decision == "approve");
            meta = meta
                .child(Html::from(" · "))
                .child(
                    ic(
                        &format!("meta-review-icon-{n}"),
                        if approved { "check" } else { "x-circle" },
                    )
                    .class(if approved { "st-open" } else { "st-closed" }),
                )
                .child(sp(
                    &format!("meta-review-{n}"),
                    "",
                    if approved {
                        " Approved"
                    } else {
                        " Changes requested"
                    },
                ));
        }
        let mut side = div("row-side").id(format!("row-side-{n}"));
        if !thread.assignee.is_empty() {
            side = side.child(avatar(&format!("assignee-{n}"), &thread.assignee, 20));
        }
        if !thread.comments.is_empty() {
            side = side.child(
                span("row-comments")
                    .child(ic(&format!("comments-icon-{n}"), "comment"))
                    .child(sp(
                        &format!("comments-{n}"),
                        "",
                        thread.comments.len().to_string(),
                    )),
            );
        }
        list = list.child(
            div("thread-row")
                .id(format!("row-{n}"))
                .child(state_icon(&format!("state-{n}"), thread, pulls))
                .child(
                    div("row-main")
                        .id(format!("row-main-{n}"))
                        .child(
                            div("row-title")
                                .id(format!("row-title-{n}"))
                                .child(a(
                                    &format!("thread-{n}"),
                                    "thread-link",
                                    format!("/{path}/{route}/{n}"),
                                    &thread.title,
                                ))
                                .children(labels(&format!("thread-{n}"), thread)),
                        )
                        .child(meta),
                )
                .child(side),
        );
    }
    // The search box really searches: it submits to this route and narrows the list. The
    // Labels and Milestones buttons went, because this instance has neither page.
    let body = vec![
        div("list-bar")
            .id("list-bar")
            .child(
                form("list-search", format!("/{path}/{list_route}"), "get")
                    .class("list-search")
                    .child(ic("list-search-icon", "search"))
                    .child(hidden("state", if closed { "closed" } else { "open" }))
                    .child(
                        text_input("list-search-q", "q", query)
                            .attr(
                                "aria-label",
                                format!(
                                    "Search {}",
                                    if pulls { "pull requests" } else { "issues" }
                                ),
                            )
                            .attr(
                                "placeholder",
                                format!(
                                    "Search is:{kind} is:{}",
                                    if closed { "closed" } else { "open" }
                                ),
                            ),
                    )
                    .child(button("list-search-submit", "Search").class("btn")),
            )
            .child(primary_link(
                "new",
                &if pulls {
                    format!("New {}", cx.look.pull())
                } else {
                    "New issue".to_owned()
                },
                if pulls {
                    format!("/{path}/compare")
                } else {
                    format!("/{path}/issues/new")
                },
            )),
        list,
    ];
    finish(
        cx,
        &format!("{title} · {path}"),
        repo_frame(
            cx,
            repository,
            name,
            if pulls { Tab::Pulls } else { Tab::Issues },
            body,
        ),
    )
}
fn new_thread_page(
    cx: &Cx,
    repository: &Repository,
    name: &str,
    pulls: bool,
) -> Result<HttpResponse> {
    let path = slug(repository, name);
    let field = |form_id: &str, key: &str, text: &str, value: &str, long: bool| {
        let id = format!("{form_id}-{key}");
        div("field").child(label(&id, text)).child(if long {
            el("textarea")
                .id(id)
                .attr("name", key)
                .attr("rows", "8")
                .attr("placeholder", "Type your description here…")
                .text(value)
        } else {
            text_input(&id, key, value)
        })
    };
    let form = if pulls {
        form("new-pull", format!("/{path}/pulls"), "post")
            .class("new-form")
            .child(field("new-pull", "title", "Title", "", false))
            .child(field(
                "new-pull",
                "head",
                "Compare branch (refs/heads/...)",
                "refs/heads/",
                false,
            ))
            .child(field(
                "new-pull",
                "base",
                "Base branch",
                "refs/heads/main",
                false,
            ))
            .child(
                div("form-actions").child(
                    button("new-pull-submit", format!("Create {}", cx.look.pull()))
                        .class("btn btn-primary"),
                ),
            )
    } else {
        form("new-issue", format!("/{path}/issues"), "post")
            .class("new-form")
            .child(field("new-issue", "title", "Add a title", "", false))
            .child(field("new-issue", "body", "Add a description", "", true))
            .child(
                div("form-actions")
                    .child(button("new-issue-submit", "Create").class("btn btn-primary")),
            )
    };
    let heading = if pulls {
        format!("Open a {}", cx.look.pull())
    } else {
        "Create new issue".to_owned()
    };
    let body = vec![div("new-thread")
        .child(avatar("new-avatar", cx.actor, 40))
        .child(
            div("new-main")
                .child(el("h1").id("new-title").class("page-title").text(heading))
                .child(div("box new-card").id("new-card").child(form)),
        )];
    finish(
        cx,
        &format!(
            "{} · {path}",
            if pulls {
                "Comparing changes"
            } else {
                "New Issue"
            }
        ),
        repo_frame(
            cx,
            repository,
            name,
            if pulls { Tab::Pulls } else { Tab::Issues },
            body,
        ),
    )
}

/// A timeline comment: the avatar in the gutter, the author strip on grey, the body beneath.
fn comment_card(prefix: &str, author: &str, verb: &str, body: &str, badge: Option<&str>) -> Html {
    let mut strip = div("comment-head")
        .id(format!("{prefix}-strip"))
        .child(a(
            &format!("{prefix}-author"),
            "strong-link",
            format!("/{author}"),
            author,
        ))
        .child(sp(&format!("{prefix}-when"), "muted", verb))
        .child(span("grow"));
    if let Some(badge) = badge {
        strip = strip.child(sp(&format!("{prefix}-badge"), "pill", badge));
    }
    let shown = if body.trim().is_empty() {
        "No description provided."
    } else {
        body
    };
    div("timeline-item")
        .child(avatar(&format!("{prefix}-avatar"), author, 40).class("gutter"))
        .child(
            div("box comment").id(prefix).child(strip).child(
                div("comment-body")
                    .id(format!("{prefix}-body"))
                    .child(prose(&format!("{prefix}-text"), shown)),
            ),
        )
}
/// A one-line timeline event with its round badge.
fn event(id: &str, icon: &str, class: &str, children: Vec<Html>) -> Html {
    div("timeline-event")
        .id(id)
        .child(
            span("event-badge")
                .class(class)
                .child(ic(&format!("{id}-icon"), icon)),
        )
        .child(div("event-text").children(children))
}
/// The sidebar of a thread page.
fn thread_sidebar(
    cx: &Cx,
    prefix: &str,
    repository: &Repository,
    name: &str,
    thread: &Thread,
    pulls: bool,
) -> Html {
    let path = slug(repository, name);
    // Each gear opened a picker on the real thing; nothing here assigns or labels, so the
    // sections are headed by their name alone.
    let section = |id: &str, title: &str, content: Vec<Html>| {
        div("side-section")
            .child(
                div("side-section-head")
                    .id(format!("{prefix}-{id}-head"))
                    .child(sp(&format!("{prefix}-{id}-title"), "", title)),
            )
            .children(content)
    };
    let mut side = el("aside")
        .id(format!("{prefix}-sidebar"))
        .class("thread-side");
    if pulls {
        let mut reviewers = vec![];
        for (i, review) in thread.reviews.iter().enumerate() {
            let (icon, class) = match review.decision.as_str() {
                "approve" => ("check", "st-open"),
                "request_changes" => ("x-circle", "st-closed"),
                _ => ("comment", "st-draft"),
            };
            reviewers.push(
                div("side-person")
                    .id(format!("{prefix}-reviewer-{i}"))
                    .child(avatar(
                        &format!("{prefix}-reviewer-avatar-{i}"),
                        &review.author,
                        20,
                    ))
                    .child(sp(
                        &format!("{prefix}-reviewer-name-{i}"),
                        "strong",
                        &review.author,
                    ))
                    .child(span("grow"))
                    .child(ic(&format!("{prefix}-reviewer-icon-{i}"), icon).class(class)),
            );
        }
        if reviewers.is_empty() {
            reviewers.push(sp(
                &format!("{prefix}-reviewers-none"),
                "side-none",
                "No reviews",
            ));
        }
        side = side.child(section("reviewers", "Reviewers", reviewers));
    }
    let assignees = if thread.assignee.is_empty() {
        vec![sp(
            &format!("{prefix}-assignee-none"),
            "side-none",
            "No one assigned",
        )]
    } else {
        vec![div("side-person")
            .id(format!("{prefix}-assignee"))
            .child(avatar(
                &format!("{prefix}-assignee-avatar"),
                &thread.assignee,
                20,
            ))
            .child(a(
                &format!("{prefix}-assignee-name"),
                "strong-link",
                format!("/{}", thread.assignee),
                &thread.assignee,
            ))]
    };
    side = side.child(section("assignees", "Assignees", assignees));
    let label_chips = if thread.labels.is_empty() {
        vec![sp(
            &format!("{prefix}-labels-none"),
            "side-none",
            "None yet",
        )]
    } else {
        vec![div("side-labels")
            .id(format!("{prefix}-labels"))
            .children(labels(&format!("{prefix}-side"), thread))]
    };
    side = side
        .child(section("labels", "Labels", label_chips))
        .child(section(
            "projects",
            "Projects",
            vec![sp(
                &format!("{prefix}-projects-none"),
                "side-none",
                "None yet",
            )],
        ))
        .child(section(
            "milestone",
            "Milestone",
            vec![sp(
                &format!("{prefix}-milestone-none"),
                "side-none",
                "No milestone",
            )],
        ));
    // Development: the pull request that closes this issue, or the issues this pull closes.
    let mut development = vec![];
    let others = if pulls {
        &repository.issues
    } else {
        &repository.pull_requests
    };
    for (i, other) in others.values().enumerate() {
        let linked = if pulls {
            thread.body.contains(&format!("#{}", other.number))
        } else {
            other.body.contains(&format!("#{}", thread.number))
        };
        if linked {
            development.push(
                div("side-dev")
                    .id(format!("{prefix}-dev-{i}"))
                    .child(state_icon(&format!("{prefix}-dev-icon-{i}"), other, !pulls))
                    .child(a(
                        &format!("{prefix}-dev-link-{i}"),
                        "strong-link",
                        format!(
                            "/{path}/{}/{}",
                            if pulls { "issues" } else { "pull" },
                            other.number
                        ),
                        format!("#{} {}", other.number, other.title),
                    )),
            );
        }
    }
    if development.is_empty() {
        development.push(sp(
            &format!("{prefix}-dev-none"),
            "side-none",
            if pulls {
                format!(
                    "Successfully merging this {} may close these issues.",
                    cx.look.pull()
                )
            } else {
                format!("No branches or {}s", cx.look.pull())
            },
        ));
    }
    side = side.child(section("development", "Development", development));
    let mut people: Vec<&str> = std::iter::once(thread.author.as_str())
        .chain(thread.comments.iter().map(|c| c.author.as_str()))
        .chain(thread.reviews.iter().map(|r| r.author.as_str()))
        .collect();
    people.sort_unstable();
    people.dedup();
    side.child(
        div("side-section last")
            .child(div("side-section-head").child(sp(
                &format!("{prefix}-participants-title"),
                "",
                format!("{} participant{}", people.len(), plural(people.len())),
            )))
            .child(
                div("faces")
                    .id(format!("{prefix}-participants"))
                    .each(people.iter(), |p| {
                        avatar(&format!("{prefix}-participant-{p}"), p, 26)
                    }),
            ),
    )
}
/// The comment box at the foot of every thread, with the state buttons beside it.
fn composer(cx: &Cx, base: &str, thread: &Thread, pulls: bool) -> Html {
    let what = if pulls { cx.look.pull() } else { "issue" };
    let mut buttons = div("form-actions").id("composer-buttons");
    // Closing and reopening post the state to their own route from inside the comment form.
    match thread.state.as_str() {
        "open" => {
            buttons = buttons.child(
                button("close", format!("Close {what}"))
                    .class("btn")
                    .attr("name", "state")
                    .attr("value", "closed")
                    .attr("formaction", format!("{base}/state")),
            )
        }
        "closed" => {
            buttons = buttons.child(
                button("reopen", format!("Reopen {what}"))
                    .class("btn")
                    .attr("name", "state")
                    .attr("value", "open")
                    .attr("formaction", format!("{base}/state")),
            )
        }
        _ => (),
    }
    buttons = buttons.child(button("comment-submit", "Comment").class("btn btn-primary"));
    div("timeline-item composer")
        .id("composer")
        .child(avatar("composer-avatar", cx.actor, 40).class("gutter"))
        .child(
            div("composer-main")
                .child(el("h3").class("composer-title").text("Add a comment"))
                .child(
                    // Write / Preview is a script on the real thing; there is one box here.
                    form("comment", format!("{base}/comments"), "post")
                        .class("box composer-card")
                        .child(
                            el("textarea")
                                .id("comment-body")
                                .attr("name", "body")
                                .attr("rows", "5")
                                .attr("aria-label", "Comment")
                                .attr("placeholder", "Add your comment here..."),
                        )
                        .child(buttons),
                ),
        )
}
fn thread_head(
    cx: &Cx,
    repository: &Repository,
    name: &str,
    thread: &Thread,
    pulls: bool,
) -> Vec<Html> {
    let path = slug(repository, name);
    let n = thread.number;
    let mut meta = div("thread-meta")
        .id("thread-head")
        .child(state_pill("state", thread, pulls));
    if pulls {
        let head = thread.head.trim_start_matches("refs/heads/");
        let base = thread.base.trim_start_matches("refs/heads/");
        let count = match (branch_tip(repository, base), branch_tip(repository, head)) {
            (Some((b, _)), Some((h, _))) => commits_between(repository, &b, &h).len(),
            _ => 0,
        };
        let verb = match thread.state.as_str() {
            "merged" => format!(
                "{} merged {count} commit{} into",
                thread.merged_by,
                plural(count)
            ),
            _ => format!(
                "{} wants to merge {count} commit{} into",
                thread.author,
                plural(count)
            ),
        };
        meta = meta
            .child(sp("thread-verb", "muted", verb))
            .child(ref_chip(
                "thread-base",
                base,
                &format!("/{path}/tree/{base}"),
            ))
            .child(sp("thread-from", "muted", "from"))
            .child(ref_chip(
                "thread-head-ref",
                head,
                &format!("/{path}/tree/{head}"),
            ));
    } else {
        meta = meta.child(sp(
            "thread-meta",
            "muted",
            format!(
                "{} opened this issue {} · {} comment{}",
                thread.author,
                ago(cx.now, thread.tick),
                thread.comments.len(),
                plural(thread.comments.len())
            ),
        ));
    }
    vec![
        div("thread-title-row")
            .id("thread-title-row")
            .child(
                el("h1")
                    .child(sp("thread-title", "", &thread.title))
                    .child(Html::from(" "))
                    .child(sp("thread-number", "number", format!("#{n}"))),
            )
            .child(
                // Editing a title is not something this instance can do, so there is no
                // Edit button to press; New is a real route.
                div("thread-actions").child(primary_link(
                    "new-from-thread",
                    &if pulls {
                        format!("New {}", cx.look.pull())
                    } else {
                        "New issue".to_owned()
                    },
                    if pulls {
                        format!("/{path}/compare")
                    } else {
                        format!("/{path}/issues/new")
                    },
                )),
            ),
        meta,
    ]
}
fn issue_page(
    cx: &Cx,
    repository: &Repository,
    name: &str,
    thread: &Thread,
) -> Result<HttpResponse> {
    let path = slug(repository, name);
    let n = thread.number;
    let base = format!("/{path}/issues/{n}");
    let mut timeline = div("timeline").id("timeline").child(comment_card(
        "thread-body",
        &thread.author,
        &format!("opened {}", ago(cx.now, thread.tick)),
        &thread.body,
        Some("Author"),
    ));
    for (i, comment) in thread.comments.iter().enumerate() {
        timeline = timeline.child(comment_card(
            &format!("comment-{i}"),
            &comment.author,
            &format!("commented {}", ago(cx.now, comment.tick)),
            &comment.body,
            (comment.author == thread.author).then_some("Author"),
        ));
    }
    if thread.state == "closed" {
        let last = thread.comments.last().map_or(thread.tick, |c| c.tick);
        let who = if thread.assignee.is_empty() {
            &thread.author
        } else {
            &thread.assignee
        };
        timeline = timeline.child(event(
            "closed-event",
            "issue-closed",
            "done",
            vec![sp(
                "closed-event-text",
                "",
                format!("{who} closed this as completed {}", ago(cx.now, last)),
            )],
        ));
    }
    timeline = timeline.child(composer(cx, &base, thread, false));
    let mut body = thread_head(cx, repository, name, thread, false);
    body.push(
        div("thread")
            .id("thread")
            .child(timeline)
            .child(thread_sidebar(cx, "side", repository, name, thread, false)),
    );
    finish(
        cx,
        &format!("{} · Issue #{n} · {path}", thread.title),
        repo_frame(cx, repository, name, Tab::Issues, body),
    )
}
/// The green (open), grey (draft), purple (merged) or red (closed) box at the foot of a
/// pull request's conversation.
fn merge_box(cx: &Cx, repository: &Repository, base: &str, thread: &Thread) -> Html {
    let branch_gone = thread.head.is_empty() || !repository.refs.contains_key(&thread.head);
    let approvals = thread
        .reviews
        .iter()
        .filter(|r| r.decision == "approve")
        .count();
    let changes = thread
        .reviews
        .iter()
        .filter(|r| r.decision == "request_changes")
        .count();
    let head = thread.head.trim_start_matches("refs/heads/");
    let what = cx.look.pull();
    let status = |id: &str, icon: &str, class: &str, title: &str, detail: &str| {
        div("merge-status")
            .id(id)
            .child(
                span("status-badge")
                    .class(class)
                    .child(ic(&format!("{id}-icon"), icon)),
            )
            .child(
                div("merge-status-text")
                    .id(format!("{id}-text"))
                    .child(el("h3").id(format!("{id}-title")).text(title))
                    .child(
                        el("p")
                            .id(format!("{id}-detail"))
                            .class("muted")
                            .text(detail),
                    ),
            )
    };
    let actions = |children: Vec<Html>| div("merge-actions").id("merge-actions").children(children);
    let (class, badge_icon, rows) = match (thread.state.as_str(), thread.draft) {
        ("merged", _) => (
            "merged",
            "merge",
            vec![
                status(
                    "merged-status",
                    "merge",
                    "merged",
                    &format!("{} successfully merged and closed", capitalise(what)),
                    &format!("You're all set — the {head} branch can be safely deleted."),
                ),
                actions(if branch_gone {
                    vec![sp(
                        "branch-deleted",
                        "muted small",
                        format!("The {head} branch has been deleted."),
                    )]
                } else {
                    vec![post_button(
                        "delete-branch",
                        "btn",
                        "Delete branch",
                        format!("{base}/delete-branch"),
                        &[],
                    )]
                }),
            ],
        ),
        ("closed", _) => (
            "closed",
            "pull-request",
            vec![
                status(
                    "closed-status",
                    "pull-request",
                    "closed",
                    "Closed with unmerged commits",
                    &format!("This {what} is closed, but the {head} branch has unmerged changes."),
                ),
                // The composer below carries `reopen`; this one is the merge box's own.
                actions(vec![post_button(
                    "merge-reopen",
                    "btn",
                    &format!("Reopen {what}"),
                    format!("{base}/state"),
                    &[("state", "open")],
                )]),
            ],
        ),
        (_, true) => (
            "draft",
            "pull-request",
            vec![
                status(
                    "draft-status",
                    "pull-request",
                    "draft",
                    &format!("This {what} is still a work in progress"),
                    &format!("Draft {what}s cannot be merged."),
                ),
                actions(vec![post_button(
                    "ready",
                    "btn",
                    "Ready for review",
                    format!("{base}/ready"),
                    &[],
                )]),
            ],
        ),
        _ => {
            let (icon, tone, title, detail) = if changes > 0 && approvals == 0 {
                (
                    "x-circle",
                    "closed",
                    "Changes requested".to_owned(),
                    format!("{changes} review{} requesting changes", plural(changes)),
                )
            } else if approvals > 0 {
                (
                    "check",
                    "open",
                    "Changes approved".to_owned(),
                    format!(
                        "{approvals} approving review{} by reviewers with write access.",
                        plural(approvals)
                    ),
                )
            } else {
                (
                    "eye",
                    "draft",
                    "Review required".to_owned(),
                    "At least 1 approving review is required by reviewers with write access."
                        .to_owned(),
                )
            };
            (
                "open",
                "merge",
                vec![
                    status("review-status", icon, tone, &title, &detail),
                    status(
                        "conflict-status",
                        "check",
                        "open",
                        "This branch has no conflicts with the base branch",
                        "Merging can be performed automatically.",
                    ),
                    actions(vec![
                        // One merge, no menu of merge methods: this server fast-forwards
                        // the base ref and that is the only way it merges.
                        form("merge-form", format!("{base}/merge"), "post")
                            .class("inline-form")
                            .child(
                                button("merge", format!("Merge {what}")).class("btn btn-primary"),
                            ),
                        sp(
                            "merge-hint",
                            "muted small",
                            "You can also merge this with the command line.",
                        ),
                    ]),
                ],
            )
        }
    };
    div("timeline-item merge")
        .child(
            span("merge-badge gutter")
                .class(class)
                .child(ic("", badge_icon)),
        )
        .child(
            div("box merge-box")
                .id("merge-box")
                .class(class)
                .children(rows),
        )
}
fn capitalise(s: &str) -> String {
    let mut chars = s.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}
fn pull_page(
    cx: &Cx,
    repository: &Repository,
    name: &str,
    thread: &Thread,
    view: &str,
) -> Result<HttpResponse> {
    let path = slug(repository, name);
    let n = thread.number;
    let base = format!("/{path}/pull/{n}");
    let head_name = thread.head.trim_start_matches("refs/heads/");
    let base_name = thread.base.trim_start_matches("refs/heads/");
    let tips = (
        branch_tip(repository, base_name),
        branch_tip(repository, head_name),
    );
    let (commits, files) = match &tips {
        (Some((b, _)), Some((h, head))) => {
            let commits = commits_between(repository, b, h);
            let start = merge_base(repository, b, h)
                .and_then(|m| repository.objects.get(&m))
                .map(|c| c.files.clone())
                .unwrap_or_default();
            (commits, diff_trees(&start, &head.files))
        }
        _ => (vec![], vec![]),
    };
    let (adds, dels) = files
        .iter()
        .fold((0, 0), |(a, d), f| (a + f.additions, d + f.deletions));
    let mut body = thread_head(cx, repository, name, thread, true);
    body.push(
        tabs(
            "pull-tabs",
            "tabs pull-tabs",
            vec![
                tab(
                    "ptab-conversation",
                    "comment",
                    "Conversation",
                    Some(thread.comments.len() + thread.reviews.len()),
                    base.clone(),
                    view == "conversation",
                ),
                tab(
                    "ptab-commits",
                    "commit",
                    "Commits",
                    Some(commits.len()),
                    format!("{base}/commits"),
                    view == "commits",
                ),
                tab(
                    "ptab-files",
                    "file",
                    "Files changed",
                    Some(files.len()),
                    format!("{base}/files"),
                    view == "files",
                ),
            ],
        )
        .child(
            span("pull-stat")
                .child(sp("pull-adds", "adds", format!("+{adds}")))
                .child(Html::from(" "))
                .child(sp("pull-dels", "dels", format!("−{dels}"))),
        ),
    );
    match view {
        "commits" => body.push(commit_groups(cx, "pull-commits", &path, &commits)),
        "files" => {
            body.push(
                div("branch-bar")
                    .id("files-bar")
                    .child(sp("files-hint", "muted", "Changes from all commits"))
                    .child(span("grow"))
                    // One button, one review: approving. The menu it wears on the real
                    // site chooses between comment, approve and request changes.
                    .child(post_button(
                        "approve",
                        "btn btn-primary",
                        "Approve changes",
                        format!("{base}/reviews"),
                        &[("decision", "approve"), ("body", "Looks good.")],
                    )),
            );
            body.extend(diff_section("diff", &files, &path, head_name));
        }
        _ => {
            let mut timeline = div("timeline").id("timeline").child(comment_card(
                "thread-body",
                &thread.author,
                &format!("commented {}", ago(cx.now, thread.tick)),
                &thread.body,
                Some("Author"),
            ));
            // Comments and reviews interleave by time, as one conversation.
            let mut events: Vec<(u64, usize, Html)> = vec![];
            for (i, comment) in thread.comments.iter().enumerate() {
                events.push((
                    comment.tick,
                    i,
                    comment_card(
                        &format!("comment-{i}"),
                        &comment.author,
                        &format!("commented {}", ago(cx.now, comment.tick)),
                        &comment.body,
                        (comment.author == thread.author).then_some("Author"),
                    ),
                ));
            }
            for (i, review) in thread.reviews.iter().enumerate() {
                let (icon, tone, verb) = match review.decision.as_str() {
                    "approve" => ("check", "open", "approved these changes"),
                    "request_changes" => ("x-circle", "closed", "requested changes"),
                    _ => ("eye", "draft", "reviewed"),
                };
                let mut node = div("review").id(format!("review-{i}")).child(
                    div("timeline-event")
                        .id(format!("review-{i}-head"))
                        .child(
                            span("event-badge").class(tone).child(
                                ic(&format!("review-state-{i}"), icon).attr("data-tone", tone),
                            ),
                        )
                        .child(
                            div("event-text")
                                .child(avatar(&format!("review-avatar-{i}"), &review.author, 20))
                                .child(a(
                                    &format!("review-author-{i}"),
                                    "strong-link",
                                    format!("/{}", review.author),
                                    &review.author,
                                ))
                                .child(sp(
                                    &format!("review-verb-{i}"),
                                    "muted",
                                    format!(" {verb} {}", ago(cx.now, review.tick)),
                                )),
                        ),
                );
                if !review.body.trim().is_empty() {
                    node = node.child(
                        div("box review-body")
                            .id(format!("review-body-{i}"))
                            .child(prose(&format!("review-text-{i}"), &review.body)),
                    );
                }
                events.push((review.tick, 100 + i, node));
            }
            events.sort_by_key(|(tick, order, _)| (*tick, *order));
            timeline = timeline.children(events.into_iter().map(|(_, _, e)| e));
            if thread.state == "merged" {
                timeline = timeline.child(event(
                    "merged-event",
                    "merge",
                    "merged",
                    vec![sp(
                        "merged-event-text",
                        "",
                        format!(
                            "{} merged commit into {base_name} {}",
                            thread.merged_by,
                            ago(cx.now, thread.tick)
                        ),
                    )],
                ));
            }
            timeline = timeline
                .child(merge_box(cx, repository, &base, thread))
                .child(composer(cx, &base, thread, true));
            body.push(
                div("thread")
                    .id("thread")
                    .child(timeline)
                    .child(thread_sidebar(cx, "side", repository, name, thread, true)),
            );
        }
    }
    finish(
        cx,
        &format!(
            "{} by {} · {} #{n} · {path}",
            thread.title,
            thread.author,
            if cx.look == Look::Gitlab {
                "Merge Request"
            } else {
                "Pull Request"
            }
        ),
        repo_frame(cx, repository, name, Tab::Pulls, body),
    )
}
/// The tabs with nothing but a blank state behind them. Each says what is true of this
/// repository rather than advertising a feature that is not here.
fn stub_page(
    cx: &Cx,
    repository: &Repository,
    name: &str,
    which: &'static str,
) -> Result<HttpResponse> {
    let gitlab = cx.look == Look::Gitlab;
    let (title, blurb, icon) = match which {
        "actions" => (
            if gitlab {
                "No pipelines have run"
            } else {
                "No workflow runs yet"
            },
            "This repository has no workflows, so nothing has been built here.",
            "play",
        ),
        "projects" => (
            "No projects yet",
            "This repository is not on any project board.",
            "grid",
        ),
        "wiki" => (
            "No wiki pages yet",
            "This repository has no wiki; what documentation there is lives in the tree.",
            "book",
        ),
        "security" => (
            "No security advisories",
            "This repository has published no advisories and has no alerts open.",
            "shield",
        ),
        _ => (
            "Nothing to configure here",
            "Access to this repository is set by the server operator, not from the web.",
            "gear",
        ),
    };
    let body = vec![div("box blank stub")
        .id("stub")
        .child(ic("stub-icon", icon))
        .child(el("h2").id("stub-title").text(title))
        .child(el("p").id("stub-blurb").class("muted").text(blurb))];
    finish(
        cx,
        &format!("{title} · {}", slug(repository, name)),
        repo_frame(cx, repository, name, Tab::Stub(which), body),
    )
}

/// Insights: what really happened in this repository over the last thirty days, each row
/// linking to the list it counts.
fn pulse_page(cx: &Cx, repository: &Repository, name: &str) -> Result<HttpResponse> {
    let path = slug(repository, name);
    let since = cx.now.saturating_sub(30 * 24);
    let merged = repository
        .pull_requests
        .values()
        .filter(|p| p.state == "merged" && p.tick >= since)
        .count();
    let opened = repository
        .pull_requests
        .values()
        .filter(|p| p.state == "open" && p.tick >= since)
        .count();
    let closed = repository
        .issues
        .values()
        .filter(|i| i.state == "closed" && i.tick >= since)
        .count();
    let filed = repository
        .issues
        .values()
        .filter(|i| i.state == "open" && i.tick >= since)
        .count();
    let branch = default_branch(repository);
    let commits = branch_tip(repository, &branch)
        .map(|(id, _)| {
            log(repository, &id)
                .iter()
                .filter(|(_, c)| c.tick >= since)
                .count()
        })
        .unwrap_or(0);
    let authors: std::collections::BTreeSet<&str> = branch_tip(repository, &branch)
        .map(|(id, _)| {
            log(repository, &id)
                .into_iter()
                .filter(|(_, c)| c.tick >= since)
                .map(|(_, c)| c.author.as_str())
                .collect()
        })
        .unwrap_or_default();
    let row = |id: &str, icon: &str, text: String, url: String| {
        div("about-line")
            .id(id)
            .child(ic(&format!("{id}-icon"), icon))
            .child(a(&format!("{id}-link"), "", url, text))
    };
    let what = cx.look.pull();
    let body = vec![
        el("h1")
            .id("pulse-title")
            .class("page-title ruled")
            .text("Pulse"),
        el("p")
            .id("pulse-period")
            .class("muted")
            .text("The last 30 days in this repository."),
        div("box pulse")
            .id("pulse")
            .child(row(
                "pulse-merged",
                "merge",
                format!("{merged} {what}{} merged", plural(merged)),
                format!("/{path}/pulls?state=closed"),
            ))
            .child(row(
                "pulse-opened",
                "pull-request",
                format!("{opened} open {what}{}", plural(opened)),
                format!("/{path}/pulls"),
            ))
            .child(row(
                "pulse-closed",
                "issue-closed",
                format!("{closed} issue{} closed", plural(closed)),
                format!("/{path}/issues?state=closed"),
            ))
            .child(row(
                "pulse-filed",
                "issue-open",
                format!("{filed} issue{} open", plural(filed)),
                format!("/{path}/issues"),
            ))
            .child(row(
                "pulse-commits",
                "commit",
                format!(
                    "{commits} commit{} to {branch} by {} author{}",
                    plural(commits),
                    authors.len(),
                    plural(authors.len())
                ),
                format!("/{path}/commits/{branch}"),
            )),
    ];
    finish(
        cx,
        &format!("Pulse · {path}"),
        repo_frame(cx, repository, name, Tab::Stub("pulse"), body),
    )
}

fn gist_index(cx: &Cx) -> Result<HttpResponse> {
    let mut main = vec![el("h1").id("gists-title").class("page-title ruled").text(
        if cx.look == Look::Gitlab {
            "Explore snippets"
        } else {
            "Discover gists"
        },
    )];
    for (id, gist) in &cx.state.gists {
        let file = gist.files.keys().next().cloned().unwrap_or_default();
        let preview: String = gist
            .files
            .values()
            .next()
            .map(|c| c.lines().take(6).collect::<Vec<_>>().join("\n"))
            .unwrap_or_default();
        main.push(
            div("gist-row")
                .id(format!("gist-row-{id}"))
                .child(
                    div("gist-head")
                        .id(format!("gist-head-{id}"))
                        .child(avatar(&format!("gist-avatar-{id}"), &gist.owner, 32))
                        .child(
                            div("gist-title")
                                .child(a(
                                    &format!("gist-owner-{id}"),
                                    "",
                                    format!("/{}", gist.owner),
                                    &gist.owner,
                                ))
                                .child(sp(&format!("gist-slash-{id}"), "sep", " / "))
                                .child(a(
                                    &format!("gist-{id}"),
                                    "strong-accent",
                                    format!("/gist/{id}"),
                                    &file,
                                ))
                                .child(
                                    el("p")
                                        .id(format!("gist-when-{id}"))
                                        .class("muted small")
                                        .text(format!("Created {}", ago(cx.now, gist.tick))),
                                )
                                .child(
                                    el("p")
                                        .id(format!("gist-desc-{id}"))
                                        .class("small")
                                        .text(&gist.description),
                                ),
                        ),
                )
                .child(div("box gist-preview").child(el("pre").class("blob-text").text(preview))),
        );
    }
    finish(
        cx,
        &format!("Discover gists · {}", cx.look.brand()),
        shell(cx, &[("gists", "/gists".into())], None, main),
    )
}

fn gist_page(cx: &Cx, id: &str) -> Result<HttpResponse> {
    let Some(gist) = cx.state.gists.get(id) else {
        return web::error(404, "gist not found");
    };
    let mut main = vec![
        div("gist-head")
            .id("gist-head")
            .child(avatar("gist-avatar", &gist.owner, 32))
            .child(
                div("gist-title")
                    .child(a("gist-owner", "", format!("/{}", gist.owner), &gist.owner))
                    .child(sp("gist-slash", "sep", " / "))
                    .child(sp("gist-id", "strong-accent", id))
                    .child(
                        el("p")
                            .id("gist-when")
                            .class("muted small")
                            .text(format!("Created {}", ago(cx.now, gist.tick))),
                    ),
            ),
        el("p")
            .id("gist-description")
            .class("gist-description")
            .text(&gist.description),
    ];
    for (i, (file, content)) in gist.files.iter().enumerate() {
        let numbers: Vec<String> = (1..=content.lines().count())
            .map(|n| n.to_string())
            .collect();
        main.push(
            div("box blob")
                .id(format!("gist-file-{i}"))
                .child(
                    div("box-head blob-head")
                        .id(format!("gist-file-head-{i}"))
                        .child(ic(&format!("gist-file-icon-{i}"), "file"))
                        .child(sp(
                            &format!("gist-file-name-{i}"),
                            "strong-accent mono",
                            file,
                        ))
                        .child(span("grow")),
                )
                .child(
                    div("blob-body")
                        .id(format!("gist-file-body-{i}"))
                        .child(
                            el("pre")
                                .id(format!("gist-file-numbers-{i}"))
                                .class("line-numbers")
                                .attr("aria-hidden", "true")
                                .text(numbers.join("\n")),
                        )
                        .child(
                            el("pre")
                                .id(format!("gist-file-text-{i}"))
                                .class("blob-text")
                                .text(content.trim_end_matches('\n')),
                        ),
                ),
        );
    }
    finish(
        cx,
        &format!("{} · gist", gist.description),
        shell(
            cx,
            &[("gists", "/gists".into()), (id, format!("/gist/{id}"))],
            None,
            main,
        ),
    )
}

/// Owner-namespaced routes. `raw` is the path with surrounding slashes already trimmed.
pub fn handle(
    state: &mut Value,
    ctx: &ServiceContext,
    req: &HttpRequest,
    raw: &str,
) -> Result<HttpResponse> {
    let mut s: GitState = web::load(state)?;
    let mut parts: Vec<&str> = raw.split('/').filter(|p| !p.is_empty()).collect();
    let api = parts.first() == Some(&"api");
    if api {
        parts.remove(0);
    }
    let method = req.method.to_ascii_uppercase();
    let actor = ctx.actor.as_str();
    if method == "GET" {
        let query = web::query(req, "q").unwrap_or_default();
        let cx = Cx {
            state: &s,
            look: Look::of(&s),
            actor,
            now: now(&s, ctx),
            search: if parts.as_slice() == ["search"] {
                &query
            } else {
                ""
            },
        };
        let repo = |owner: &str, name: &str| repository(&s, owner, name);
        let missing = || web::error(404, "repository not found");
        let state_filter = web::query(req, "state").unwrap_or_default();
        return match parts.as_slice() {
            [] => home(&cx),
            ["search"] => search_page(&cx, &query),
            ["issues"] => dashboard(&cx, false),
            ["pulls"] => dashboard(&cx, true),
            ["notifications"] => notifications_page(&cx),
            ["gists"] => gist_index(&cx),
            ["gist", id] => gist_page(&cx, id),
            [owner] => owner_page(&cx, owner, &web::query(req, "tab").unwrap_or_default()),
            [owner, name] => match repo(owner, name) {
                Some(r) if api => HttpResponse::json(200, &json!(r)),
                Some(r) => code_page(&cx, r, name, &default_branch(r), ""),
                None => missing(),
            },
            [owner, name, "tree", branch, rest @ ..] => match repo(owner, name) {
                Some(r) => code_page(&cx, r, name, branch, &rest.join("/")),
                None => missing(),
            },
            [owner, name, "find", branch] => match repo(owner, name) {
                Some(r) => find_page(&cx, r, name, branch, &query),
                None => missing(),
            },
            [owner, name, kind @ ("blob" | "raw"), rest @ ..] if !rest.is_empty() => {
                match repo(owner, name) {
                    Some(r) => {
                        // `/blob/<branch>/<path>` is GitHub's shape; `/blob/<path>` is the older
                        // one this site used, and still resolves on the default branch.
                        // `/blob/<rev>/<path>` where the revision is a branch or a commit id;
                        // a tree browsed at a commit links its files that way.
                        let (branch, file) = if rest.len() > 1 && resolve(r, rest[0]).is_some() {
                            (rest[0].to_owned(), rest[1..].join("/"))
                        } else {
                            (default_branch(r), rest.join("/"))
                        };
                        // Raw is the bytes: the Raw button on a blob really serves the file.
                        if *kind == "raw" {
                            return match resolve(r, &branch)
                                .and_then(|(_, tip)| tip.files.get(&file))
                            {
                                Some(content) => Ok(HttpResponse::text(200, content.clone())),
                                None => web::error(404, "file not found"),
                            };
                        }
                        blob_page(&cx, r, name, &branch, &file)
                    }
                    None => missing(),
                }
            }
            [owner, name, "commits"] => match repo(owner, name) {
                Some(r) => commits_page(&cx, r, name, &default_branch(r)),
                None => missing(),
            },
            [owner, name, "commits", branch] => match repo(owner, name) {
                Some(r) => commits_page(&cx, r, name, branch),
                None => missing(),
            },
            [owner, name, "commit", sha] => match repo(owner, name) {
                Some(r) => {
                    let full = r.objects.keys().find(|k| k.starts_with(sha)).cloned();
                    match full {
                        Some(id) => commit_page(&cx, r, name, &id),
                        None => web::error(404, "commit not found"),
                    }
                }
                None => missing(),
            },
            [owner, name, "branches"] => match repo(owner, name) {
                Some(r) => branches_page(&cx, r, name),
                None => missing(),
            },
            [owner, name, "stargazers"] => match repo(owner, name) {
                Some(r) if api => HttpResponse::json(200, &json!(r.stars)),
                Some(r) => people_page(&cx, r, name, false),
                None => missing(),
            },
            [owner, name, "watchers"] => match repo(owner, name) {
                Some(r) if api => HttpResponse::json(200, &json!(r.watchers)),
                Some(r) => people_page(&cx, r, name, true),
                None => missing(),
            },
            [owner, name, "issues", "new"] => match repo(owner, name) {
                Some(r) => new_thread_page(&cx, r, name, false),
                None => missing(),
            },
            [owner, name, "compare"] => match repo(owner, name) {
                Some(r) => new_thread_page(&cx, r, name, true),
                None => missing(),
            },
            [owner, name, kind @ ("issues" | "pulls")] => match repo(owner, name) {
                Some(r) if api => HttpResponse::json(
                    200,
                    &json!(if *kind == "pulls" {
                        &r.pull_requests
                    } else {
                        &r.issues
                    }),
                ),
                Some(r) => list_page(&cx, r, name, *kind == "pulls", &state_filter, &query),
                None => missing(),
            },
            [owner, name, kind @ ("issues" | "pull"), number, view @ ..] if view.len() <= 1 => {
                let pulls = *kind == "pull";
                let view = view.first().copied().unwrap_or("conversation");
                match (repo(owner, name), number.parse::<u64>()) {
                    (Some(r), Ok(n)) => match r.thread(pulls, n) {
                        Some(t) if api => HttpResponse::json(200, &json!(t)),
                        Some(t)
                            if pulls && ["conversation", "commits", "files"].contains(&view) =>
                        {
                            pull_page(&cx, r, name, t, view)
                        }
                        Some(t) if !pulls && view == "conversation" => issue_page(&cx, r, name, t),
                        Some(_) => web::error(404, "route not found"),
                        None => web::error(404, "thread not found"),
                    },
                    (Some(_), Err(_)) => web::error(404, "thread not found"),
                    (None, _) => missing(),
                }
            }
            [owner, name, "pulse"] => match repo(owner, name) {
                Some(r) => pulse_page(&cx, r, name),
                None => missing(),
            },
            [owner, name, which @ ("actions" | "projects" | "wiki" | "security" | "settings")] => {
                match repo(owner, name) {
                    // The arm's own patterns are the only strings that reach here, so the
                    // stub's name is as static as the route.
                    Some(r) => stub_page(
                        &cx,
                        r,
                        name,
                        match *which {
                            "actions" => "actions",
                            "projects" => "projects",
                            "wiki" => "wiki",
                            "security" => "security",
                            _ => "settings",
                        },
                    ),
                    None => missing(),
                }
            }
            _ => web::error(404, "route not found"),
        };
    }
    if method != "POST" {
        return web::error(405, "method not allowed");
    }
    let input = match web::body(req) {
        Ok(v) => v,
        Err(_) => return web::error(400, "malformed request body"),
    };
    let (owner, name) = match parts.as_slice() {
        [owner, name, ..] => (owner.to_string(), name.to_string()),
        _ => return web::error(404, "route not found"),
    };
    let tick = now(&s, ctx);
    let Some(repo) = s.repositories.get_mut(&name).filter(|r| r.owner == owner) else {
        return web::error(404, "repository not found");
    };
    let actor = ctx.actor.clone();
    let path = format!("{owner}/{name}");
    // Every arm names the page the caller lands on, so a browser POST behaves like a redirect
    // and the API answers with the resource the mutation produced.
    let landing: std::result::Result<String, (u16, String)> = match parts.as_slice() {
        [_, _, "star"] => {
            repo.star(&actor);
            Ok(format!("{path}/stargazers"))
        }
        [_, _, "watch"] => {
            repo.watch(&actor);
            Ok(format!("{path}/watchers"))
        }
        [_, _, "issues"] => repo
            .open_issue(
                &actor,
                &web::text(&input, "title"),
                &web::text(&input, "body"),
                tick,
            )
            .map(|n| format!("{path}/issues/{n}")),
        [_, _, "pulls"] => repo
            .open_pull(
                &actor,
                &web::text(&input, "title"),
                &web::text(&input, "head"),
                &web::text(&input, "base"),
                tick,
            )
            .map(|n| format!("{path}/pull/{n}")),
        [_, _, kind @ ("issues" | "pull"), number, op] => {
            let pulls = *kind == "pull";
            match number.parse::<u64>() {
                Err(_) => Err((404, "thread not found".into())),
                Ok(n) => {
                    let done = match *op {
                        "comments" => {
                            repo.comment(pulls, n, &actor, &web::text(&input, "body"), tick)
                        }
                        "state" => repo.set_state(pulls, n, &actor, &web::text(&input, "state")),
                        "reviews" if pulls => repo.review(
                            n,
                            &actor,
                            &web::text(&input, "decision"),
                            &web::text(&input, "body"),
                            tick,
                        ),
                        "merge" if pulls => repo.merge(n, &actor, tick),
                        "ready" if pulls => repo.ready(n, &actor),
                        "delete-branch" if pulls => repo.delete_branch(n, &actor),
                        _ => Err((404, "route not found".into())),
                    };
                    done.map(|()| format!("{path}/{kind}/{n}"))
                }
            }
        }
        _ => Err((404, "route not found".into())),
    };
    match landing {
        Err((code, message)) => web::error(code, message),
        Ok(landing) => {
            web::save(state, &s)?;
            let route = if api {
                format!("api/{landing}")
            } else {
                landing
            };
            handle(
                state,
                ctx,
                &HttpRequest::get(format!("http://github.com/{route}")),
                &route,
            )
        }
    }
}
fn repository<'a>(state: &'a GitState, owner: &str, name: &str) -> Option<&'a Repository> {
    state.repositories.get(name).filter(|r| r.owner == owner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cw_service_common::html::fragment;
    use cw_web::dom::Document as Dom;
    #[test]
    fn ticks_read_as_dates_and_ages() {
        assert_eq!(date(0), "Aug 1, 2026");
        assert_eq!(date(24 * 31), "Sep 1, 2026");
        assert_eq!(date(24 * 153), "Jan 1, 2027");
        assert_eq!(ago(100, 100), "just now");
        assert_eq!(ago(100, 97), "3 hours ago");
        assert_eq!(ago(100, 60), "yesterday");
        assert_eq!(ago(1000, 900), "4 days ago");
        assert_eq!(ago(1000, 700), "last week");
        assert_eq!(ago(1000, 300), "4 weeks ago");
        assert_eq!(ago(2000, 800), "last month");
        assert_eq!(ago(3000, 300), "3 months ago");
    }
    #[test]
    fn markdown_makes_headings_lists_code_and_inline_links() {
        let out = markdown("md", "# Title\n\nSome `prose` here.\n\n- one http://a.example/\n- two\n\n```\nlet x = 1;\n```\n");
        let html = fragment(out).render();
        let doc = cw_web::html::parse(&html);
        let h1 = doc
            .descendants(Dom::ROOT)
            .find(|n| doc.is(*n, "h1"))
            .unwrap();
        assert!(doc.attr(h1, "id").unwrap().starts_with("md-h-"));
        assert_eq!(doc.text_content(h1), "Title");
        let p = doc
            .descendants(Dom::ROOT)
            .find(|n| doc.is(*n, "p"))
            .unwrap();
        assert_eq!(doc.text_content(p), "Some prose here.");
        assert!(html.contains("<code>prose</code>"));
        let pre = doc
            .descendants(Dom::ROOT)
            .find(|n| doc.is(*n, "pre"))
            .unwrap();
        assert_eq!(doc.text_content(pre), "let x = 1;");
        // The list is one <ul>; the URL in its first item is a link with the Page version's id.
        let items: Vec<_> = doc
            .descendants(Dom::ROOT)
            .filter(|n| doc.is(*n, "li"))
            .collect();
        assert_eq!(items.len(), 2);
        let link = doc.descendants(items[0]).find(|n| doc.is(*n, "a")).unwrap();
        assert_eq!(doc.attr(link, "href"), Some("http://a.example/"));
        assert_eq!(
            doc.attr(link, "id"),
            Some(format!("{}-link-1", doc.attr(items[0], "id").unwrap()).as_str())
        );
    }
    #[test]
    fn markdown_reads_a_four_space_block_as_code_but_not_a_wrapped_list_item() {
        let out = markdown(
            "md",
            "Intro:\n\n    one --flag\n    two\n\nAfter.\n\n- item that\n    wraps on\n- second\n",
        );
        let html = fragment(out).render();
        let doc = cw_web::html::parse(&html);
        let pre: Vec<_> = doc
            .descendants(Dom::ROOT)
            .filter(|n| doc.is(*n, "pre"))
            .collect();
        assert_eq!(pre.len(), 1, "{html}");
        assert_eq!(doc.text_content(pre[0]), "one --flag\ntwo");
        // The indented continuation of a list item is not a code block (this renderer puts
        // it in a paragraph of its own; it never turns the rest of a list into code).
        let items: Vec<_> = doc
            .descendants(Dom::ROOT)
            .filter(|n| doc.is(*n, "li"))
            .collect();
        assert_eq!(items.len(), 2);
        assert!(html.contains("wraps on") && !doc.text_content(pre[0]).contains("wraps"));
    }
    #[test]
    fn inline_keeps_whitespace_and_trailing_punctuation_outside_the_link() {
        let html = fragment(inline(
            "t",
            "See http://a.example/x, then\n\nhttp://b.example/.",
            false,
        ))
        .render();
        assert_eq!(
            html,
            "See <a id=\"t-link-1\" href=\"http://a.example/x\">http://a.example/x</a>, then\n\n<a id=\"t-link-3\" href=\"http://b.example/\">http://b.example/</a>."
        );
    }
    #[test]
    fn label_colours_are_githubs_for_known_names_and_stable_otherwise() {
        assert_eq!(label_colours("bug").0, "#d73a4a");
        assert_eq!(label_colours("good first issue").0, "#7057ff");
        assert_eq!(label_colours("launchpad"), label_colours("launchpad"));
    }
}
