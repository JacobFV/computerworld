//! GitHub-skinned routes and pages: owner paths, code, commits, diffs, issues, pull
//! requests, stars and gists, laid out the way github.com laid them out in 2024-2025.
//! The plain skin never reaches this module, which is why `/repos/*` stays byte-identical.
use crate::{
    branch_tip, branches, commits_between, default_branch, diff_trees, language_color,
    language_stats, last_commit_per_path, list_tree, log, merge_base, short, Commit, DiffLine,
    FileDiff, GitState, Repository, Thread,
};
use cw_protocol::{HttpRequest, HttpResponse, PageAction, PageElement, PageTheme, Result, Style};
use cw_sdk::ServiceContext;
use cw_service_common as web;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const ACCENT: &str = "#0969da";
const INK: &str = "#1f2328";
const MUTED: &str = "#59636e";
const SURFACE: &str = "#f6f8fa";
const LINE: &str = "#d0d7de";
const GREEN: &str = "#1f883d";
const PURPLE: &str = "#8250df";
const RED: &str = "#cf222e";
const ORANGE: &str = "#fd8c73";
const CHROME: &str = "#24292f";
const CHROME_LINE: &str = "#57606a";
const FOLDER: &str = "#54aeff";
const ADD_BG: &str = "#dafbe1";
const DEL_BG: &str = "#ffebe9";
const HUNK_BG: &str = "#ddf4ff";
const CLEAR: &str = "#00000000";
/// Width of the About column and the issue sidebar.
const SIDE: u32 = 296;

/// The page palette; a seed may override it, as every skinned service allows.
fn theme(state: &GitState) -> PageTheme {
    state.theme.clone().unwrap_or(PageTheme {
        accent: Some(ACCENT.into()),
        background: Some("#ffffff".into()),
        surface: Some(SURFACE.into()),
        ink: Some(INK.into()),
        muted: Some(MUTED.into()),
        content_width: Some(1280),
        font: None,
    })
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

// ---- Small presentational pieces. ----

fn style() -> Style {
    web::style()
}
fn text(id: &str, s: impl Into<String>, size: u16, color: &str) -> PageElement {
    web::styled(id, s, style().size(size).color(color))
}
fn bold(id: &str, s: impl Into<String>, size: u16, color: &str) -> PageElement {
    web::styled(id, s, style().size(size).color(color).bold())
}
fn muted(id: &str, s: impl Into<String>) -> PageElement {
    text(id, s, 12, MUTED)
}
fn mono(id: &str, s: impl Into<String>, size: u16, color: &str) -> PageElement {
    web::styled(id, s, style().size(size).color(color).mono())
}
fn ic(id: &str, name: &str, size: u16, color: &str) -> PageElement {
    web::icon(id, name, name, style().size(size).color(color))
}
/// A row of children at their own widths, left to right.
fn chips(id: &str, gap: u32, style: Style, children: Vec<PageElement>) -> PageElement {
    web::styled_row(id, gap, "center", style.justify("start"), children)
}
/// `left` chips at the left edge and `right` chips at the right edge.
fn between(id: &str, style: Style, left: Vec<PageElement>, right: Vec<PageElement>) -> PageElement {
    web::styled_row(
        id,
        8,
        "center",
        style.justify("space-between"),
        vec![
            chips(&format!("{id}-l"), 8, web::style(), left),
            chips(&format!("{id}-r"), 8, web::style(), right),
        ],
    )
}
fn column(id: &str, gap: u32, style: Style, children: Vec<PageElement>) -> PageElement {
    web::column(id, gap, style, children)
}
/// GitHub's secondary button: grey fill, hairline border, 6px corners.
fn button_style() -> Style {
    style()
        .background(SURFACE)
        .border(LINE)
        .radius(6)
        .padding(6)
        .size(12)
        .medium()
        .color(INK)
}
fn grey_link(id: &str, s: &str, url: impl Into<String>) -> PageElement {
    web::styled_link(id, s, url, button_style())
}
fn green_link(id: &str, s: &str, url: impl Into<String>) -> PageElement {
    web::styled_link(
        id,
        s,
        url,
        button_style()
            .background(GREEN)
            .border(GREEN)
            .color("#ffffff"),
    )
}
fn post(url: String, fields: &[(&str, &str)]) -> PageAction {
    PageAction {
        method: "POST".into(),
        url,
        fields: fields
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
    }
}
fn grey_button(id: &str, s: &str, action: PageAction) -> PageElement {
    web::styled_button(id, s, action, button_style().padding(10))
}
fn green_button(id: &str, s: &str, action: PageAction) -> PageElement {
    web::styled_button(
        id,
        s,
        action,
        button_style()
            .padding(10)
            .background(GREEN)
            .border(GREEN)
            .color("#ffffff"),
    )
}
/// A bordered chip made of an icon, a label and (optionally) a count: Watch 12, Fork 37.
fn icon_chip(id: &str, icon: &str, label: &str, count: Option<String>, url: &str) -> PageElement {
    let mut children = vec![
        ic(&format!("{id}-icon"), icon, 16, MUTED),
        web::inline_link(&format!("{id}-label"), label, url, 12, INK),
    ];
    if let Some(count) = count {
        children.push(counter(&format!("{id}-count"), count));
    }
    chips(
        id,
        6,
        style()
            .background(SURFACE)
            .border(LINE)
            .radius(6)
            .padding(5),
        children,
    )
}
/// The grey number bubble after a tab or a chip label.
fn counter(id: &str, s: String) -> PageElement {
    web::chip(id, s, "#e7ebef", INK, style().size(11).padding(5).medium())
}
/// A branch name the way GitHub sets it in prose: blue mono on a pale-blue chip.
fn ref_chip(id: &str, name: &str, url: &str) -> PageElement {
    web::styled_link(
        id,
        name,
        url,
        style()
            .size(12)
            .mono()
            .color(ACCENT)
            .background(HUNK_BG)
            .radius(6)
            .padding(3)
            .one_line(),
    )
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
fn label_chip(id: &str, name: &str) -> PageElement {
    let (fill, ink) = label_colours(name);
    web::chip(id, name, fill, ink, style().size(11).padding(6).medium())
}
fn labels(prefix: &str, thread: &Thread) -> Vec<PageElement> {
    thread
        .labels
        .iter()
        .enumerate()
        .map(|(i, l)| label_chip(&format!("{prefix}-label-{i}"), l))
        .collect()
}
/// The state badge on a thread page: icon and word on a solid pill.
fn state_pill(id: &str, thread: &Thread, pull: bool) -> PageElement {
    let (text, colour, icon) = match (thread.state.as_str(), pull, thread.draft) {
        ("merged", ..) => ("Merged", PURPLE, "merge"),
        ("closed", true, _) => ("Closed", RED, "pull-request"),
        ("closed", false, _) => ("Closed", PURPLE, "issue-closed"),
        (_, true, true) => ("Draft", MUTED, "pull-request"),
        (_, true, false) => ("Open", GREEN, "pull-request"),
        _ => ("Open", GREEN, "issue-open"),
    };
    chips(
        id,
        4,
        style().background(colour).radius(20).padding(6),
        vec![
            ic(&format!("{id}-icon"), icon, 16, "#ffffff"),
            bold(&format!("{id}-text"), text, 14, "#ffffff"),
        ],
    )
}
/// The small state icon in a list row.
fn state_icon(id: &str, thread: &Thread, pull: bool) -> PageElement {
    let (icon, colour) = match (thread.state.as_str(), pull, thread.draft) {
        ("merged", ..) => ("merge", PURPLE),
        ("closed", true, _) => ("pull-request", RED),
        ("closed", false, _) => ("issue-closed", PURPLE),
        (_, true, true) => ("pull-request", MUTED),
        (_, true, false) => ("pull-request", GREEN),
        _ => ("issue-open", GREEN),
    };
    ic(id, icon, 16, colour)
}
fn slug(repository: &Repository, name: &str) -> String {
    format!("{}/{name}", repository.owner)
}
fn open_count(threads: &BTreeMap<u64, Thread>) -> usize {
    threads.values().filter(|t| t.state == "open").count()
}
/// Links the text mentions, as elements after it, so a URL in an issue is clickable.
fn with_links(prefix: &str, body: &str, size: u16) -> Vec<PageElement> {
    let mut out = vec![text(prefix, body, size, INK)];
    let links = web::links(prefix, body);
    if !links.is_empty() {
        out.push(chips(&format!("{prefix}-links"), 8, style(), links));
    }
    out
}

// ---- The global header: dark bar, mark, breadcrumb, search, the right-side icons. ----

fn header(trail: &[(&str, String)], actor: &str) -> PageElement {
    let mut left = vec![web::icon_action(
        "mark",
        "code",
        "GitHub",
        style()
            .size(18)
            .padding(7)
            .radius(16)
            .background("#ffffff")
            .color(CHROME),
        web::visit("/"),
    )];
    for (i, (label, url)) in trail.iter().enumerate() {
        if i > 0 {
            left.push(text(&format!("crumb-sep-{i}"), "/", 14, CHROME_LINE));
        }
        let last = i + 1 == trail.len();
        left.push(web::styled_link(
            &format!("crumb-{i}"),
            *label,
            url.clone(),
            style()
                .size(14)
                .color("#ffffff")
                .one_line()
                .weight(if last { "bold" } else { "regular" }),
        ));
    }
    let search = chips(
        "search",
        8,
        style().border(CHROME_LINE).radius(6).padding(6).width(320),
        vec![
            ic("search-icon", "search", 14, "#8b949e"),
            text("search-hint", "Type / to search", 13, "#8b949e"),
        ],
    );
    let square = |id: &str, name: &str| {
        web::icon(
            id,
            name,
            name,
            style()
                .size(16)
                .color("#ffffff")
                .padding(6)
                .border(CHROME_LINE)
                .radius(6),
        )
    };
    let right = vec![
        square("nav-plus", "plus"),
        square("nav-issues", "issue-open"),
        square("nav-pulls", "pull-request"),
        square("nav-inbox", "inbox"),
        web::avatar("nav-avatar", actor, 30),
    ];
    web::styled_row(
        "chrome",
        16,
        "center",
        style()
            .background(CHROME)
            .padding(12)
            .justify("space-between")
            .pin("top"),
        vec![
            chips("chrome-left", 8, style(), left),
            search,
            chips("chrome-right", 8, style(), right),
        ],
    )
}
trait StyleExt {
    fn weight(self, w: &str) -> Style;
}
impl StyleExt for Style {
    fn weight(mut self, w: &str) -> Style {
        self.weight = Some(w.into());
        self
    }
}

// ---- The repository frame: title row, action chips, and the tab strip. ----

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Code,
    Issues,
    Pulls,
    Other,
}
fn tab(
    id: &str,
    icon: &str,
    label: &str,
    count: Option<usize>,
    url: String,
    active: bool,
) -> PageElement {
    let mut row = vec![
        ic(&format!("{id}-icon"), icon, 16, MUTED),
        web::styled_link(
            &format!("{id}-link"),
            label,
            url,
            style().size(14).color(INK).one_line().weight(if active {
                "medium"
            } else {
                "regular"
            }),
        ),
    ];
    if let Some(n) = count {
        row.push(counter(&format!("{id}-count"), n.to_string()));
    }
    column(
        id,
        6,
        style(),
        vec![
            chips(&format!("{id}-row"), 6, style().padding(4), row),
            web::thumbnail(
                &format!("{id}-bar"),
                "",
                style()
                    .height(2)
                    .background(if active { ORANGE } else { CLEAR }),
            ),
        ],
    )
}
fn repo_frame(
    repository: &Repository,
    name: &str,
    actor: &str,
    active: Tab,
    body: Vec<PageElement>,
) -> Vec<PageElement> {
    let path = slug(repository, name);
    let starred = repository.stars.contains(actor);
    let mut elements = vec![
        header(
            &[
                (repository.owner.as_str(), format!("/{}", repository.owner)),
                (name, format!("/{path}")),
            ],
            actor,
        ),
        web::spacer("repo-lead", 8),
        between(
            "repo-head",
            style(),
            vec![
                ic("repo-icon", "book", 16, MUTED),
                web::inline_link(
                    "repo-owner",
                    &repository.owner,
                    format!("/{}", repository.owner),
                    20,
                    ACCENT,
                ),
                text("repo-sep", "/", 20, MUTED),
                web::styled_link(
                    "repo-name",
                    name,
                    format!("/{path}"),
                    style().size(20).bold().color(ACCENT).one_line(),
                ),
                web::chip(
                    "repo-visibility",
                    "Public",
                    "#ffffff",
                    MUTED,
                    style().size(11).padding(5).border(LINE).medium(),
                ),
            ],
            vec![
                icon_chip(
                    "watch",
                    "eye",
                    "Watch",
                    Some((repository.stars.len() * 2 + 3).to_string()),
                    &format!("/{path}/stargazers"),
                ),
                icon_chip(
                    "fork",
                    "fork",
                    "Fork",
                    Some(repository.forks.to_string()),
                    &format!("/{path}/branches"),
                ),
                web::styled_button(
                    "star",
                    if starred { "Starred" } else { "Star" },
                    post(format!("/{path}/star"), &[]),
                    button_style().padding(6),
                ),
                web::styled_link(
                    "stargazers",
                    repository.stars.len().to_string(),
                    format!("/{path}/stargazers"),
                    style()
                        .size(11)
                        .medium()
                        .color(INK)
                        .background("#e7ebef")
                        .radius(10)
                        .padding(5),
                ),
            ],
        ),
        chips(
            "repo-tabs",
            12,
            style(),
            vec![
                tab(
                    "tab-code",
                    "code",
                    "Code",
                    None,
                    format!("/{path}"),
                    active == Tab::Code,
                ),
                tab(
                    "tab-issues",
                    "issue-open",
                    "Issues",
                    Some(open_count(&repository.issues)),
                    format!("/{path}/issues"),
                    active == Tab::Issues,
                ),
                tab(
                    "tab-pulls",
                    "pull-request",
                    "Pull requests",
                    Some(open_count(&repository.pull_requests)),
                    format!("/{path}/pulls"),
                    active == Tab::Pulls,
                ),
                tab(
                    "tab-actions",
                    "play",
                    "Actions",
                    None,
                    format!("/{path}/actions"),
                    false,
                ),
                tab(
                    "tab-projects",
                    "grid",
                    "Projects",
                    None,
                    format!("/{path}/projects"),
                    false,
                ),
                tab(
                    "tab-wiki",
                    "book",
                    "Wiki",
                    None,
                    format!("/{path}/wiki"),
                    false,
                ),
                tab(
                    "tab-security",
                    "shield",
                    "Security",
                    None,
                    format!("/{path}/security"),
                    false,
                ),
                tab(
                    "tab-insights",
                    "signal",
                    "Insights",
                    None,
                    format!("/{path}/pulse"),
                    false,
                ),
                tab(
                    "tab-settings",
                    "gear",
                    "Settings",
                    None,
                    format!("/{path}/settings"),
                    false,
                ),
            ],
        ),
        web::divider("tabs-rule"),
        web::spacer("tabs-gap", 8),
    ];
    elements.extend(body);
    elements
}

// ---- Markdown, as much of it as a README needs. ----

fn markdown(prefix: &str, source: &str) -> Vec<PageElement> {
    let mut out = vec![];
    let mut paragraph: Vec<String> = vec![];
    let mut code: Option<Vec<String>> = None;
    let mut n = 0usize;
    let mut next = |kind: &str| {
        n += 1;
        format!("{prefix}-{kind}-{n}")
    };
    let flush = |paragraph: &mut Vec<String>, out: &mut Vec<PageElement>, id: String| {
        if !paragraph.is_empty() {
            let body = paragraph.join(" ").replace('`', "");
            out.extend(with_links(&id, &body, 14));
            paragraph.clear();
        }
    };
    for line in source.lines() {
        if let Some(block) = code.as_mut() {
            if line.trim_start().starts_with("```") {
                out.push(web::styled(
                    &next("code"),
                    block.join("\n"),
                    style()
                        .size(12)
                        .mono()
                        .color(INK)
                        .background(SURFACE)
                        .radius(6)
                        .padding(12),
                ));
                code = None;
            } else {
                block.push(line.to_owned());
            }
            continue;
        }
        if line.trim_start().starts_with("```") {
            flush(&mut paragraph, &mut out, next("p"));
            code = Some(vec![]);
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            flush(&mut paragraph, &mut out, next("p"));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix('#') {
            flush(&mut paragraph, &mut out, next("p"));
            let level = 1 + rest.chars().take_while(|c| *c == '#').count();
            let title = rest.trim_start_matches('#').trim().replace('`', "");
            let size = match level {
                1 => 26,
                2 => 20,
                _ => 16,
            };
            out.push(bold(&next("h"), title, size, INK));
            if level <= 2 {
                out.push(web::divider(&next("hr")));
            }
            continue;
        }
        let item = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .map(|s| ("•".to_owned(), s))
            .or_else(|| {
                let (num, rest) = trimmed.split_once(". ")?;
                num.parse::<u32>().ok().map(|_| (format!("{num}."), rest))
            });
        if let Some((marker, body)) = item {
            flush(&mut paragraph, &mut out, next("p"));
            let id = next("li");
            let body = body.replace('`', "");
            let mut row = vec![
                web::styled(
                    &format!("{id}-dot"),
                    marker,
                    style().size(14).color(INK).width(24).align("right"),
                ),
                web::styled(
                    &format!("{id}-text"),
                    body.split(" http").next().unwrap_or(&body).trim_end(),
                    style().size(14).color(INK).flex(1),
                ),
            ];
            row.extend(web::links(&id, &body));
            out.push(web::styled_row(&id, 6, "start", style(), row));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("> ") {
            flush(&mut paragraph, &mut out, next("p"));
            out.push(web::styled(
                &next("quote"),
                rest.replace('`', ""),
                style().size(14).color(MUTED).italic().padding(8),
            ));
            continue;
        }
        paragraph.push(trimmed.to_owned());
    }
    if let Some(block) = code.take() {
        out.push(mono(&next("code"), block.join("\n"), 12, INK));
    }
    flush(&mut paragraph, &mut out, next("p"));
    out
}

// ---- Diffs. Runs of one kind collapse into one block, so a hunk reads as a hunk. ----

fn diff_block(prefix: &str, file: &FileDiff, url: &str) -> PageElement {
    let mut rows = vec![between(
        &format!("{prefix}-head"),
        style().background(SURFACE).padding(8),
        vec![
            ic(&format!("{prefix}-chev"), "chevron-down", 14, MUTED),
            web::styled_link(
                &format!("{prefix}-path"),
                &file.path,
                url.to_owned(),
                style().size(12).mono().bold().color(INK).one_line(),
            ),
            web::styled(
                &format!("{prefix}-status"),
                &file.status,
                style().size(11).color(MUTED),
            ),
        ],
        vec![
            bold(
                &format!("{prefix}-adds"),
                format!("+{}", file.additions),
                12,
                GREEN,
            ),
            bold(
                &format!("{prefix}-dels"),
                format!("-{}", file.deletions),
                12,
                RED,
            ),
        ],
    )];
    let mut n = 0usize;
    for hunk in &file.hunks {
        n += 1;
        rows.push(web::styled(
            &format!("{prefix}-hunk-{n}"),
            hunk.header(),
            style()
                .size(12)
                .mono()
                .color(MUTED)
                .background(HUNK_BG)
                .padding(6),
        ));
        let (mut old, mut new) = (hunk.old_start, hunk.new_start);
        let mut run: Vec<String> = vec![];
        let mut kind: Option<&'static str> = None;
        let flush = |run: &mut Vec<String>,
                     kind: Option<&str>,
                     rows: &mut Vec<PageElement>,
                     n: &mut usize| {
            if run.is_empty() {
                return;
            }
            *n += 1;
            let (fill, ink) = match kind {
                Some("add") => (Some(ADD_BG), INK),
                Some("del") => (Some(DEL_BG), INK),
                _ => (None, INK),
            };
            let mut s = style().size(12).mono().color(ink).padding(4);
            if let Some(fill) = fill {
                s = s.background(fill);
            }
            rows.push(web::styled(
                &format!("{prefix}-lines-{n}"),
                run.join("\n"),
                s,
            ));
            run.clear();
        };
        for line in &hunk.lines {
            let (this, shown) = match line {
                DiffLine::Context(s) => {
                    let t = format!("{old:>4} {new:>4}   {s}");
                    old += 1;
                    new += 1;
                    ("ctx", t)
                }
                DiffLine::Add(s) => {
                    let t = format!("     {new:>4} + {s}");
                    new += 1;
                    ("add", t)
                }
                DiffLine::Remove(s) => {
                    let t = format!("{old:>4}      - {s}");
                    old += 1;
                    ("del", t)
                }
            };
            if kind != Some(this) {
                flush(&mut run, kind, &mut rows, &mut n);
                kind = Some(this);
            }
            run.push(shown);
        }
        flush(&mut run, kind, &mut rows, &mut n);
    }
    web::card(prefix, style().border(LINE).radius(6).padding(0), rows)
}
fn diff_section(prefix: &str, files: &[FileDiff], path: &str, branch: &str) -> Vec<PageElement> {
    let (adds, dels) = files
        .iter()
        .fold((0, 0), |(a, d), f| (a + f.additions, d + f.deletions));
    let mut out = vec![text(
        &format!("{prefix}-summary"),
        format!(
            "Showing {} changed file{} with {adds} addition{} and {dels} deletion{}.",
            files.len(),
            if files.len() == 1 { "" } else { "s" },
            if adds == 1 { "" } else { "s" },
            if dels == 1 { "" } else { "s" },
        ),
        14,
        INK,
    )];
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

fn home(state: &GitState, actor: &str, now: u64) -> Result<HttpResponse> {
    let mut side = vec![bold("side-title", "Top repositories", 14, INK)];
    let mut feed = vec![];
    for (name, repository) in &state.repositories {
        if repository.owner.is_empty() {
            continue;
        }
        let path = slug(repository, name);
        side.push(chips(
            &format!("side-{name}"),
            8,
            style(),
            vec![
                web::avatar(&format!("side-avatar-{name}"), &repository.owner, 16),
                web::inline_link(
                    &format!("side-link-{name}"),
                    &path,
                    format!("/{path}"),
                    13,
                    INK,
                ),
            ],
        ));
        let tip = branch_tip(repository, &default_branch(repository));
        let mut meta = vec![
            counter(
                &format!("repo-stars-{name}"),
                format!("{} stars", repository.stars.len()),
            ),
            counter(
                &format!("repo-issues-{name}"),
                format!("{} open issues", open_count(&repository.issues)),
            ),
        ];
        if let Some((_, commit)) = tip {
            meta.push(muted(
                &format!("repo-updated-{name}"),
                format!("Updated {}", ago(now, commit.tick)),
            ));
        }
        feed.push(web::card_action(
            &format!("repo-{}-{name}", repository.owner),
            style()
                .background("#ffffff")
                .border(LINE)
                .radius(6)
                .padding(16),
            web::visit(format!("/{path}")),
            vec![
                chips(
                    &format!("repo-title-{name}"),
                    8,
                    style(),
                    vec![
                        web::avatar(&format!("repo-avatar-{name}"), &repository.owner, 20),
                        bold(&format!("repo-name-{name}"), &path, 15, ACCENT),
                    ],
                ),
                text(
                    &format!("repo-desc-{name}"),
                    &repository.description,
                    13,
                    MUTED,
                ),
                chips(&format!("repo-meta-{name}"), 8, style(), meta),
            ],
        ));
    }
    if !state.gists.is_empty() {
        side.push(web::divider("side-rule"));
        side.push(web::inline_link(
            "gists",
            "Browse gists",
            "/gists",
            13,
            ACCENT,
        ));
    }
    let elements = vec![
        header(&[], actor),
        web::spacer("home-lead", 16),
        web::styled_row(
            "home",
            24,
            "start",
            style(),
            vec![
                column("home-side", 10, style().width(SIDE), side),
                column(
                    "home-main",
                    12,
                    style().flex(1),
                    vec![
                        bold("home-title", "Home", 22, INK),
                        muted(
                            "home-sub",
                            "Repositories, issues and pull requests on this instance.",
                        ),
                        web::grid("repos", 2, 16, feed),
                    ],
                ),
            ],
        ),
    ];
    web::themed_page("GitHub", theme(state), elements)
}

fn search_page(state: &GitState, actor: &str, query: &str) -> Result<HttpResponse> {
    let q = query.to_ascii_lowercase();
    let mut rows = vec![];
    for (name, repository) in &state.repositories {
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
        rows.push(web::card(
            &format!("hit-{name}"),
            style().border(LINE).radius(6).padding(12),
            vec![
                web::inline_link(
                    &format!("hit-link-{name}"),
                    &path,
                    format!("/{path}"),
                    15,
                    ACCENT,
                ),
                text(
                    &format!("hit-desc-{name}"),
                    &repository.description,
                    13,
                    MUTED,
                ),
            ],
        ));
    }
    let count = rows.len();
    let mut elements = vec![
        header(&[], actor),
        web::spacer("search-lead", 16),
        bold(
            "search-title",
            format!("{count} repository results"),
            20,
            INK,
        ),
    ];
    elements.extend(rows);
    web::themed_page(&format!("{query} · Search"), theme(state), elements)
}

fn owner_page(state: &GitState, owner: &str, actor: &str, now: u64) -> Result<HttpResponse> {
    let owned: Vec<_> = state
        .repositories
        .iter()
        .filter(|(_, r)| r.owner == owner)
        .collect();
    if owned.is_empty() {
        return web::error(404, "owner not found");
    }
    let mut cards = vec![];
    let mut commits: Vec<u64> = vec![];
    let mut followers = std::collections::BTreeSet::new();
    for (name, repository) in &owned {
        followers.extend(repository.stars.iter().cloned());
        let branch = default_branch(repository);
        let mut meta = vec![];
        if let Some((id, tip)) = branch_tip(repository, &branch) {
            commits.extend(log(repository, &id).iter().map(|(_, c)| c.tick));
            if let Some((language, ..)) = language_stats(&tip.files).first() {
                meta.push(web::thumbnail(
                    &format!("owned-dot-{name}"),
                    "",
                    style()
                        .width(10)
                        .height(10)
                        .radius(5)
                        .background(language_color(language)),
                ));
                meta.push(muted(&format!("owned-lang-{name}"), language));
            }
        }
        meta.push(ic(&format!("owned-star-icon-{name}"), "star", 14, MUTED));
        meta.push(muted(
            &format!("owned-stars-{name}"),
            repository.stars.len().to_string(),
        ));
        meta.push(ic(&format!("owned-fork-icon-{name}"), "fork", 14, MUTED));
        meta.push(muted(
            &format!("owned-forks-{name}"),
            repository.forks.to_string(),
        ));
        cards.push(web::card(
            &format!("owned-{name}"),
            style()
                .background("#ffffff")
                .border(LINE)
                .radius(6)
                .padding(16),
            vec![
                chips(
                    &format!("owned-head-{name}"),
                    8,
                    style(),
                    vec![
                        ic(&format!("owned-icon-{name}"), "book", 16, MUTED),
                        web::inline_link(
                            &format!("owned-name-{name}"),
                            *name,
                            format!("/{owner}/{name}"),
                            14,
                            ACCENT,
                        ),
                        web::chip(
                            &format!("owned-vis-{name}"),
                            "Public",
                            "#ffffff",
                            MUTED,
                            style().size(11).padding(5).border(LINE),
                        ),
                    ],
                ),
                text(
                    &format!("owned-desc-{name}"),
                    &repository.description,
                    12,
                    MUTED,
                ),
                chips(&format!("owned-meta-{name}"), 6, style(), meta),
            ],
        ));
    }
    // Twenty weeks of squares, Sunday to Saturday down each column, newest at the right.
    const WEEKS: u64 = 26;
    let today = now / 24;
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
    let mut graph_rows = vec![];
    for weekday in 0..7 {
        let cells = counts
            .iter()
            .enumerate()
            .map(|(week, column)| {
                let fill = match column[weekday] {
                    0 => "#ebedf0",
                    1 => "#9be9a8",
                    2 => "#40c463",
                    3 => "#30a14e",
                    _ => "#216e39",
                };
                web::thumbnail(
                    &format!("cell-{week}-{weekday}"),
                    "",
                    style().width(11).height(11).radius(2).background(fill),
                )
            })
            .collect();
        graph_rows.push(chips(&format!("graph-row-{weekday}"), 3, style(), cells));
    }
    let is_org = owned.len() > 1;
    let profile = vec![
        web::avatar("owner-avatar", owner, 260),
        bold("owner-name", owner, 24, INK),
        text(
            "owner-login",
            if is_org { "Organization" } else { "User" },
            16,
            MUTED,
        ),
        grey_link("follow", "Follow", format!("/{owner}")),
        chips(
            "owner-followers",
            6,
            style(),
            vec![
                ic("owner-followers-icon", "person", 14, MUTED),
                bold(
                    "owner-followers-count",
                    followers.len().to_string(),
                    12,
                    INK,
                ),
                muted("owner-followers-label", "followers ·"),
                bold("owner-following-count", "12", 12, INK),
                muted("owner-following-label", "following"),
            ],
        ),
        chips(
            "owner-location",
            6,
            style(),
            vec![
                ic("owner-location-icon", "location", 14, MUTED),
                muted("owner-location-text", "Lisbon, Portugal"),
            ],
        ),
        chips(
            "owner-site",
            6,
            style(),
            vec![
                ic("owner-site-icon", "link", 14, MUTED),
                web::inline_link(
                    "owner-site-link",
                    format!("{owner}.example"),
                    format!("http://{owner}.example/"),
                    12,
                    INK,
                ),
            ],
        ),
    ];
    let main = vec![
        chips(
            "owner-tabs",
            12,
            style(),
            vec![
                tab(
                    "otab-overview",
                    "book",
                    "Overview",
                    None,
                    format!("/{owner}"),
                    true,
                ),
                tab(
                    "otab-repos",
                    "archive",
                    "Repositories",
                    Some(owned.len()),
                    format!("/{owner}"),
                    false,
                ),
                tab(
                    "otab-projects",
                    "grid",
                    "Projects",
                    None,
                    format!("/{owner}"),
                    false,
                ),
                tab(
                    "otab-packages",
                    "archive",
                    "Packages",
                    None,
                    format!("/{owner}"),
                    false,
                ),
                tab(
                    "otab-stars",
                    "star",
                    "Stars",
                    None,
                    format!("/{owner}"),
                    false,
                ),
            ],
        ),
        web::divider("owner-tabs-rule"),
        bold("pinned-title", "Popular repositories", 16, INK),
        web::grid("owned", 2, 16, cards),
        bold(
            "contrib-title",
            format!("{} contributions in the last year", commits.len()),
            16,
            INK,
        ),
        web::card(
            "graph",
            style().border(LINE).radius(6).padding(12),
            graph_rows,
        ),
        chips(
            "graph-legend",
            4,
            style(),
            vec![
                muted("legend-less", "Less"),
                web::thumbnail(
                    "legend-0",
                    "",
                    style().width(11).height(11).radius(2).background("#ebedf0"),
                ),
                web::thumbnail(
                    "legend-1",
                    "",
                    style().width(11).height(11).radius(2).background("#9be9a8"),
                ),
                web::thumbnail(
                    "legend-2",
                    "",
                    style().width(11).height(11).radius(2).background("#40c463"),
                ),
                web::thumbnail(
                    "legend-3",
                    "",
                    style().width(11).height(11).radius(2).background("#30a14e"),
                ),
                web::thumbnail(
                    "legend-4",
                    "",
                    style().width(11).height(11).radius(2).background("#216e39"),
                ),
                muted("legend-more", "More"),
            ],
        ),
    ];
    web::themed_page(
        &format!("{owner} · GitHub"),
        theme(state),
        vec![
            header(&[(owner, format!("/{owner}"))], actor),
            web::spacer("owner-lead", 16),
            web::styled_row(
                "owner",
                24,
                "start",
                style(),
                vec![
                    column("owner-profile", 10, style().width(SIDE), profile),
                    column("owner-main", 14, style().flex(1), main),
                ],
            ),
        ],
    )
}

/// The file table for `prefix` of `branch`, with the latest-commit bar above it.
fn file_table(
    repository: &Repository,
    name: &str,
    branch: &str,
    tip_id: &str,
    tip: &Commit,
    prefix: &str,
    now: u64,
) -> PageElement {
    let path = slug(repository, name);
    let last = last_commit_per_path(repository, tip_id);
    let history = log(repository, tip_id);
    let mut rows = vec![between(
        "latest",
        style().background(SURFACE).padding(10),
        vec![
            web::avatar("latest-avatar", &tip.author, 20),
            web::inline_link(
                "latest-author",
                &tip.author,
                format!("/{}", tip.author),
                13,
                INK,
            ),
            web::styled(
                "latest-message",
                tip.message.lines().next().unwrap_or(""),
                style().size(13).color(MUTED).one_line().width(360),
            ),
        ],
        vec![
            web::styled_link(
                "latest-sha",
                short(tip_id),
                format!("/{path}/commit/{tip_id}"),
                style().size(12).mono().color(MUTED).one_line(),
            ),
            muted("latest-when", format!("· {}", ago(now, tip.tick))),
            ic("history-icon", "clock", 16, MUTED),
            web::inline_link(
                "history",
                format!("{} Commits", history.len()),
                format!("/{path}/commits/{branch}"),
                12,
                MUTED,
            ),
        ],
    )];
    if !prefix.is_empty() {
        let parent = prefix.rsplit_once('/').map_or("", |(p, _)| p);
        rows.push(web::divider("entry-rule-up"));
        rows.push(chips(
            "entry-up",
            8,
            style().padding(8),
            vec![
                ic("entry-up-icon", "folder", 16, FOLDER),
                web::inline_link(
                    "entry-up-link",
                    "..",
                    if parent.is_empty() {
                        format!("/{path}")
                    } else {
                        format!("/{path}/tree/{branch}/{parent}")
                    },
                    13,
                    INK,
                ),
            ],
        ));
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
        let mut row = vec![
            ic(
                &format!("entry-icon-{i}"),
                if entry.dir { "folder" } else { "file" },
                16,
                if entry.dir { FOLDER } else { MUTED },
            ),
            web::styled_link(
                &format!("file-{i}"),
                &entry.name,
                url,
                style().size(13).color(INK).one_line().width(220),
            ),
        ];
        match touched {
            Some((tick, id, message)) => {
                row.push(web::styled_link(
                    &format!("entry-message-{i}"),
                    message.lines().next().unwrap_or(""),
                    format!("/{path}/commit/{id}"),
                    style().size(12).color(MUTED).one_line().flex(1),
                ));
                row.push(web::styled(
                    &format!("entry-when-{i}"),
                    ago(now, tick),
                    style()
                        .size(12)
                        .color(MUTED)
                        .one_line()
                        .width(110)
                        .align("right"),
                ));
            }
            None => row.push(web::styled("", "", style().flex(1))),
        }
        rows.push(web::divider(&format!("entry-rule-{i}")));
        rows.push(web::styled_row(
            &format!("entry-{i}"),
            8,
            "center",
            style().padding(8),
            row,
        ));
    }
    web::card("files", style().border(LINE).radius(6).padding(0), rows)
}
fn branch_bar(repository: &Repository, name: &str, branch: &str, count_tags: usize) -> PageElement {
    let path = slug(repository, name);
    between(
        "branch-bar",
        style(),
        vec![
            chips(
                "branch-select",
                6,
                style()
                    .background(SURFACE)
                    .border(LINE)
                    .radius(6)
                    .padding(6),
                vec![
                    ic("branch-icon", "branch", 16, MUTED),
                    web::inline_link("branch-name", branch, format!("/{path}/branches"), 13, INK),
                    ic("branch-chevron", "chevron-down", 12, MUTED),
                ],
            ),
            ic("branches-icon", "branch", 16, MUTED),
            web::inline_link(
                "branches",
                format!("{} Branches", branches(repository).len()),
                format!("/{path}/branches"),
                12,
                MUTED,
            ),
            ic("tags-icon", "tag", 16, MUTED),
            web::inline_link(
                "tags",
                format!("{count_tags} Tags"),
                format!("/{path}/branches"),
                12,
                MUTED,
            ),
        ],
        vec![
            grey_link("go-to-file", "Go to file", format!("/{path}/tree/{branch}")),
            web::styled_link(
                "add-file",
                "+",
                format!("/{path}/tree/{branch}"),
                button_style(),
            ),
            green_link("code", "<> Code", format!("/{path}/tree/{branch}")),
        ],
    )
}
fn about(repository: &Repository, name: &str, tip: &Commit) -> PageElement {
    let path = slug(repository, name);
    let mut items = vec![
        bold("about-title", "About", 16, INK),
        text("repo-description", &repository.description, 14, INK),
    ];
    if !repository.topics.is_empty() {
        items.push(chips(
            "repo-topics",
            6,
            style(),
            repository
                .topics
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    web::chip(
                        &format!("topic-{i}"),
                        t,
                        HUNK_BG,
                        ACCENT,
                        style().size(12).padding(6).medium(),
                    )
                })
                .collect(),
        ));
    }
    let line = |id: &str, icon: &str, label: String, url: String| {
        chips(
            id,
            8,
            style(),
            vec![
                ic(&format!("{id}-icon"), icon, 16, MUTED),
                web::inline_link(&format!("{id}-link"), label, url, 13, MUTED),
            ],
        )
    };
    items.push(line(
        "about-readme",
        "book",
        "Readme".into(),
        format!("/{path}"),
    ));
    if tip.files.keys().any(|f| f.eq_ignore_ascii_case("LICENSE")) {
        items.push(line(
            "about-license",
            "shield",
            "MIT license".into(),
            format!("/{path}/blob/main/LICENSE"),
        ));
    }
    items.push(line(
        "about-activity",
        "signal",
        "Activity".into(),
        format!("/{path}/commits/main"),
    ));
    items.push(line(
        "about-stars",
        "star",
        format!("{} stars", repository.stars.len()),
        format!("/{path}/stargazers"),
    ));
    items.push(line(
        "about-watching",
        "eye",
        format!("{} watching", repository.stars.len() * 2 + 3),
        format!("/{path}/stargazers"),
    ));
    items.push(line(
        "about-forks",
        "fork",
        format!("{} forks", repository.forks),
        format!("/{path}/branches"),
    ));
    items.push(web::divider("about-rule-1"));
    items.push(bold("releases-title", "Releases", 14, INK));
    items.push(muted("releases-none", "No releases published"));
    items.push(web::divider("about-rule-2"));
    let mut authors: Vec<&str> = repository
        .objects
        .values()
        .map(|c| c.author.as_str())
        .collect();
    authors.sort_unstable();
    authors.dedup();
    items.push(bold(
        "contributors-title",
        format!("Contributors {}", authors.len()),
        14,
        INK,
    ));
    items.push(chips(
        "contributors",
        4,
        style(),
        authors
            .iter()
            .take(8)
            .map(|a| web::avatar(&format!("contributor-{a}"), a, 32))
            .collect(),
    ));
    items.push(web::divider("about-rule-3"));
    items.push(bold("languages-title", "Languages", 14, INK));
    let stats = language_stats(&tip.files);
    if stats.is_empty() {
        items.push(muted("languages-none", "No languages detected"));
    } else {
        let usable = SIDE - 2 * (stats.len() as u32 - 1);
        items.push(chips(
            "language-bar",
            2,
            style(),
            stats
                .iter()
                .enumerate()
                .map(|(i, (language, _, share))| {
                    web::thumbnail(
                        &format!("language-bar-{i}"),
                        "",
                        style()
                            .width((usable * share / 1000).max(4))
                            .height(8)
                            .radius(2)
                            .background(language_color(language)),
                    )
                })
                .collect(),
        ));
        items.push(chips(
            "language-list",
            12,
            style(),
            stats
                .iter()
                .enumerate()
                .map(|(i, (language, _, share))| {
                    chips(
                        &format!("language-{i}"),
                        4,
                        style(),
                        vec![
                            web::thumbnail(
                                &format!("language-dot-{i}"),
                                "",
                                style()
                                    .width(10)
                                    .height(10)
                                    .radius(5)
                                    .background(language_color(language)),
                            ),
                            bold(&format!("language-name-{i}"), language, 12, INK),
                            muted(
                                &format!("language-share-{i}"),
                                format!("{}.{}%", share / 10, share % 10),
                            ),
                        ],
                    )
                })
                .collect(),
        ));
    }
    column("about", 10, style().width(SIDE), items)
}
fn readme_card(tip: &Commit, prefix: &str) -> Option<PageElement> {
    let (file, readme) = tip.files.iter().find(|(f, _)| {
        let dir = f.rsplit_once('/').map_or("", |(d, _)| d);
        dir == prefix
            && f.rsplit('/')
                .next()
                .unwrap_or(f)
                .eq_ignore_ascii_case("README.md")
    })?;
    let _ = file;
    let mut children = vec![
        chips(
            "readme-head",
            8,
            style().padding(10),
            vec![
                ic("readme-icon", "list-view", 16, MUTED),
                bold("readme-title", "README", 13, INK),
                ic("license-icon", "shield", 16, MUTED),
                muted("license-title", "MIT license"),
            ],
        ),
        web::divider("readme-rule"),
    ];
    children.push(column(
        "readme-body",
        8,
        style().padding(24),
        markdown("readme", readme),
    ));
    Some(web::card(
        "readme",
        style().border(LINE).radius(6).padding(0),
        children,
    ))
}
fn code_page(
    state: &GitState,
    repository: &Repository,
    name: &str,
    actor: &str,
    branch: &str,
    prefix: &str,
    now: u64,
) -> Result<HttpResponse> {
    let path = slug(repository, name);
    let Some((tip_id, tip)) = branch_tip(repository, branch) else {
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
    let mut left = vec![branch_bar(repository, name, branch, 0)];
    if !prefix.is_empty() {
        let mut crumbs = vec![web::inline_link(
            "crumb-root",
            name,
            format!("/{path}"),
            16,
            ACCENT,
        )];
        let mut so_far = String::new();
        for (i, part) in prefix.split('/').enumerate() {
            if !so_far.is_empty() {
                so_far.push('/');
            }
            so_far.push_str(part);
            crumbs.push(text(&format!("crumb-slash-{i}"), "/", 16, MUTED));
            crumbs.push(web::inline_link(
                &format!("crumb-part-{i}"),
                part,
                format!("/{path}/tree/{branch}/{so_far}"),
                16,
                if i + 1 == prefix.split('/').count() {
                    INK
                } else {
                    ACCENT
                },
            ));
        }
        left.push(chips("tree-crumbs", 4, style(), crumbs));
    }
    left.push(file_table(
        repository, name, branch, &tip_id, tip, prefix, now,
    ));
    if let Some(card) = readme_card(tip, prefix) {
        left.push(card);
    }
    let mut columns = vec![column("code-main", 16, style().flex(1), left)];
    if prefix.is_empty() {
        columns.push(about(repository, name, tip));
    }
    let body = vec![web::styled_row(
        "code-columns",
        24,
        "start",
        style(),
        columns,
    )];
    let title = if prefix.is_empty() {
        format!("{path}: {}", repository.description)
    } else {
        format!("{path}/{prefix} at {branch}")
    };
    web::themed_page(
        &format!("{title} · GitHub"),
        theme(state),
        repo_frame(repository, name, actor, Tab::Code, body),
    )
}

fn blob_page(
    state: &GitState,
    repository: &Repository,
    name: &str,
    actor: &str,
    branch: &str,
    file: &str,
    now: u64,
) -> Result<HttpResponse> {
    let path = slug(repository, name);
    let Some((tip_id, tip)) = branch_tip(repository, branch) else {
        return web::error(404, "branch not found");
    };
    let Some(content) = tip.files.get(file) else {
        return web::error(404, "file not found");
    };
    let lines: Vec<&str> = content.lines().collect();
    let loc = lines.iter().filter(|l| !l.trim().is_empty()).count();
    let numbers: Vec<String> = (1..=lines.len()).map(|n| n.to_string()).collect();
    let last = last_commit_per_path(repository, &tip_id);
    let mut crumbs = vec![web::inline_link(
        "crumb-root",
        name,
        format!("/{path}"),
        16,
        ACCENT,
    )];
    let parts: Vec<&str> = file.split('/').collect();
    let mut so_far = String::new();
    for (i, part) in parts.iter().enumerate() {
        if !so_far.is_empty() {
            so_far.push('/');
        }
        so_far.push_str(part);
        crumbs.push(text(&format!("crumb-slash-{i}"), "/", 16, MUTED));
        if i + 1 == parts.len() {
            crumbs.push(bold(&format!("crumb-part-{i}"), *part, 16, INK));
        } else {
            crumbs.push(web::inline_link(
                &format!("crumb-part-{i}"),
                *part,
                format!("/{path}/tree/{branch}/{so_far}"),
                16,
                ACCENT,
            ));
        }
    }
    let mut body = vec![
        branch_bar(repository, name, branch, 0),
        chips("blob-crumbs", 4, style(), crumbs),
    ];
    if let Some((id, commit)) = last.get(file) {
        body.push(between(
            "blob-latest",
            style()
                .background(SURFACE)
                .border(LINE)
                .radius(6)
                .padding(10),
            vec![
                web::avatar("blob-latest-avatar", &commit.author, 20),
                web::inline_link(
                    "blob-latest-author",
                    &commit.author,
                    format!("/{}", commit.author),
                    13,
                    INK,
                ),
                web::styled(
                    "blob-latest-message",
                    commit.message.lines().next().unwrap_or(""),
                    style().size(13).color(MUTED).one_line().width(420),
                ),
            ],
            vec![
                web::styled_link(
                    "blob-latest-sha",
                    short(id),
                    format!("/{path}/commit/{id}"),
                    style().size(12).mono().color(MUTED).one_line(),
                ),
                muted("blob-latest-when", format!("· {}", ago(now, commit.tick))),
                ic("blob-history-icon", "clock", 16, MUTED),
                web::inline_link(
                    "blob-history",
                    "History",
                    format!("/{path}/commits/{branch}"),
                    12,
                    MUTED,
                ),
            ],
        ));
    }
    let header_row = between(
        "blob-head",
        style().background(SURFACE).padding(8),
        vec![
            web::styled_link(
                "blob-code-tab",
                "Code",
                format!("/{path}/blob/{branch}/{file}"),
                button_style().background("#ffffff"),
            ),
            web::styled_link(
                "blame",
                "Blame",
                format!("/{path}/blob/{branch}/{file}"),
                button_style().background(SURFACE).border(SURFACE),
            ),
            muted(
                "blob-stats",
                format!(
                    "{} lines ({loc} loc) · {} Bytes",
                    lines.len(),
                    content.len()
                ),
            ),
        ],
        vec![
            grey_link("raw", "Raw", format!("/{path}/raw/{branch}/{file}")),
            web::icon("copy", "copy", "Copy raw file", button_style().size(16)),
            web::icon("download", "download", "Download", button_style().size(16)),
            web::icon("edit", "pencil", "Edit file", button_style().size(16)),
            web::icon("more", "more", "More options", button_style().size(16)),
        ],
    );
    let code = web::styled_row(
        "blob-body",
        12,
        "start",
        style().padding(8),
        vec![
            web::styled(
                "line-numbers",
                numbers.join("\n"),
                style()
                    .size(12)
                    .mono()
                    .color(MUTED)
                    .width(40)
                    .align("right"),
            ),
            web::styled(
                "blob-text",
                content.trim_end_matches('\n'),
                style().size(12).mono().color(INK).flex(1),
            ),
        ],
    );
    body.push(web::card(
        "blob",
        style().border(LINE).radius(6).padding(0),
        vec![header_row, web::divider("blob-rule"), code],
    ));
    web::themed_page(
        &format!("{path}/{file} at {branch} · GitHub"),
        theme(state),
        repo_frame(repository, name, actor, Tab::Code, body),
    )
}

/// One commit as a row of a history list.
fn commit_row(prefix: &str, path: &str, id: &str, commit: &Commit, now: u64) -> PageElement {
    let title = commit.message.lines().next().unwrap_or("").to_owned();
    web::styled_row(
        prefix,
        12,
        "center",
        style().padding(12).justify("space-between"),
        vec![
            column(
                &format!("{prefix}-main"),
                4,
                style(),
                vec![
                    web::inline_link(
                        &format!("{prefix}-title"),
                        title,
                        format!("/{path}/commit/{id}"),
                        14,
                        INK,
                    ),
                    chips(
                        &format!("{prefix}-meta"),
                        6,
                        style(),
                        vec![
                            web::avatar(&format!("{prefix}-avatar"), &commit.author, 20),
                            web::inline_link(
                                &format!("{prefix}-author"),
                                &commit.author,
                                format!("/{}", commit.author),
                                12,
                                INK,
                            ),
                            muted(
                                &format!("{prefix}-when"),
                                format!("committed {}", ago(now, commit.tick)),
                            ),
                        ],
                    ),
                ],
            ),
            chips(
                &format!("{prefix}-side"),
                6,
                style(),
                vec![
                    web::styled_link(
                        &format!("{prefix}-sha"),
                        short(id),
                        format!("/{path}/commit/{id}"),
                        button_style().mono().weight("regular"),
                    ),
                    web::icon(
                        &format!("{prefix}-browse"),
                        "code",
                        "Browse the repository at this point in the history",
                        button_style().size(14),
                    ),
                ],
            ),
        ],
    )
}
/// Commits grouped by day, the way the history page reads.
fn commit_groups(
    prefix: &str,
    path: &str,
    history: &[(String, &Commit)],
    now: u64,
) -> Vec<PageElement> {
    let mut out = vec![];
    let mut day: Option<u64> = None;
    let mut rows: Vec<PageElement> = vec![];
    let mut group = 0usize;
    let close = |rows: &mut Vec<PageElement>, out: &mut Vec<PageElement>, group: usize| {
        if !rows.is_empty() {
            out.push(web::card(
                &format!("{prefix}-group-{group}"),
                style().border(LINE).radius(6).padding(0),
                std::mem::take(rows),
            ));
        }
    };
    for (i, (id, commit)) in history.iter().enumerate() {
        let this = commit.tick / 24;
        if day != Some(this) {
            close(&mut rows, &mut out, group);
            group += 1;
            day = Some(this);
            out.push(chips(
                &format!("{prefix}-day-{group}"),
                6,
                style(),
                vec![
                    ic(&format!("{prefix}-day-icon-{group}"), "commit", 16, MUTED),
                    muted(
                        &format!("{prefix}-day-label-{group}"),
                        format!("Commits on {}", date(commit.tick)),
                    ),
                ],
            ));
        } else {
            rows.push(web::divider(&format!("{prefix}-rule-{i}")));
        }
        rows.push(commit_row(&format!("{prefix}-{i}"), path, id, commit, now));
    }
    close(&mut rows, &mut out, group);
    out
}
fn commits_page(
    state: &GitState,
    repository: &Repository,
    name: &str,
    actor: &str,
    branch: &str,
    now: u64,
) -> Result<HttpResponse> {
    let path = slug(repository, name);
    let Some((tip_id, _)) = branch_tip(repository, branch) else {
        return web::error(404, "branch not found");
    };
    let history = log(repository, &tip_id);
    let mut body = vec![
        bold("commits-title", "Commits", 20, INK),
        between(
            "commits-bar",
            style(),
            vec![chips(
                "branch-select",
                6,
                style()
                    .background(SURFACE)
                    .border(LINE)
                    .radius(6)
                    .padding(6),
                vec![
                    ic("branch-icon", "branch", 16, MUTED),
                    web::inline_link("branch-name", branch, format!("/{path}/branches"), 13, INK),
                    ic("branch-chevron", "chevron-down", 12, MUTED),
                ],
            )],
            vec![
                grey_link(
                    "filter-user",
                    "All users ▾",
                    format!("/{path}/commits/{branch}"),
                ),
                grey_link(
                    "filter-time",
                    "All time ▾",
                    format!("/{path}/commits/{branch}"),
                ),
            ],
        ),
    ];
    body.extend(commit_groups("commits", &path, &history, now));
    web::themed_page(
        &format!("Commits · {path}"),
        theme(state),
        repo_frame(repository, name, actor, Tab::Code, body),
    )
}
fn commit_page(
    state: &GitState,
    repository: &Repository,
    name: &str,
    actor: &str,
    id: &str,
    now: u64,
) -> Result<HttpResponse> {
    let path = slug(repository, name);
    let Some(commit) = repository.objects.get(id) else {
        return web::error(404, "commit not found");
    };
    let empty = BTreeMap::new();
    let parent = commit
        .parents
        .first()
        .and_then(|p| repository.objects.get(p))
        .map_or(&empty, |p| &p.files);
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
    let mut head = vec![bold("commit-title", title, 20, INK)];
    if !rest.is_empty() {
        head.push(text("commit-body", rest.join("\n"), 13, INK));
    }
    let mut branch_chips: Vec<PageElement> = on
        .iter()
        .enumerate()
        .map(|(i, b)| {
            chips(
                &format!("commit-branch-{i}"),
                4,
                style(),
                vec![
                    ic(&format!("commit-branch-icon-{i}"), "branch", 14, MUTED),
                    web::inline_link(
                        &format!("commit-branch-link-{i}"),
                        b,
                        format!("/{path}/tree/{b}"),
                        12,
                        MUTED,
                    ),
                ],
            )
        })
        .collect();
    if branch_chips.is_empty() {
        branch_chips.push(muted("commit-branch-none", "not on any branch"));
    }
    head.push(chips("commit-branches", 8, style(), branch_chips));
    let mut parents = vec![muted(
        "commit-parents-label",
        format!(
            "{} parent{}",
            commit.parents.len(),
            if commit.parents.len() == 1 { "" } else { "s" }
        ),
    )];
    for (i, p) in commit.parents.iter().enumerate() {
        parents.push(web::styled_link(
            &format!("commit-parent-{i}"),
            short(p),
            format!("/{path}/commit/{p}"),
            style().size(12).mono().color(ACCENT).one_line(),
        ));
    }
    parents.push(muted("commit-sha-label", "commit"));
    parents.push(mono("commit-sha", id, 12, MUTED));
    let meta = between(
        "commit-meta",
        style().background(SURFACE).padding(12),
        vec![
            web::avatar("commit-avatar", &commit.author, 20),
            web::inline_link(
                "commit-author",
                &commit.author,
                format!("/{}", commit.author),
                13,
                INK,
            ),
            muted(
                "commit-when",
                format!(
                    "committed on {} · {}",
                    date(commit.tick),
                    ago(now, commit.tick)
                ),
            ),
        ],
        parents,
    );
    let mut body = vec![web::card(
        "commit-card",
        style().border(LINE).radius(6).padding(0),
        vec![
            between(
                "commit-head",
                style().padding(16),
                vec![column("commit-head-main", 6, style(), head)],
                vec![grey_link(
                    "browse-files",
                    "Browse files",
                    format!("/{path}/tree/{}", on.first().map_or("main", |b| b.as_str())),
                )],
            ),
            web::divider("commit-rule"),
            meta,
        ],
    )];
    body.extend(diff_section(
        "diff",
        &files,
        &path,
        on.first().map_or("main", |b| b.as_str()),
    ));
    web::themed_page(
        &format!(
            "{} · {path}@{}",
            commit.message.lines().next().unwrap_or(""),
            short(id)
        ),
        theme(state),
        repo_frame(repository, name, actor, Tab::Code, body),
    )
}
fn branches_page(
    state: &GitState,
    repository: &Repository,
    name: &str,
    actor: &str,
    now: u64,
) -> Result<HttpResponse> {
    let path = slug(repository, name);
    let default = default_branch(repository);
    let main_tip = branch_tip(repository, &default).map(|(id, _)| id);
    let mut rows = vec![between(
        "branches-head",
        style().background(SURFACE).padding(10),
        vec![bold("branches-head-label", "Branch", 12, MUTED)],
        vec![
            muted("branches-head-updated", "Updated"),
            muted("branches-head-check", "Check status"),
            muted("branches-head-behind", "Behind | Ahead"),
            muted("branches-head-pr", "Pull request"),
        ],
    )];
    for (i, branch) in branches(repository).iter().enumerate() {
        let Some((tip, commit)) = branch_tip(repository, branch) else {
            continue;
        };
        let mut left = vec![
            ic(&format!("branch-icon-{i}"), "branch", 16, MUTED),
            web::styled_link(
                &format!("branch-{i}"),
                branch,
                format!("/{path}/tree/{branch}"),
                style().size(13).mono().color(ACCENT).one_line(),
            ),
        ];
        if *branch == default {
            left.push(web::chip(
                &format!("branch-default-{i}"),
                "Default",
                "#ffffff",
                MUTED,
                style().size(11).padding(5).border(LINE),
            ));
        }
        left.push(muted(
            &format!("branch-updated-{i}"),
            format!("Updated {} by {}", ago(now, commit.tick), commit.author),
        ));
        let mut right = vec![];
        if let Some(main_tip) = &main_tip {
            if *branch != default {
                let ahead = commits_between(repository, main_tip, &tip).len();
                let behind = commits_between(repository, &tip, main_tip).len();
                right.push(muted(
                    &format!("branch-ahead-{i}"),
                    format!("{behind} behind · {ahead} ahead"),
                ));
            }
        }
        if let Some(pull) = repository
            .pull_requests
            .values()
            .find(|p| p.head == format!("refs/heads/{branch}"))
        {
            right.push(state_icon(&format!("branch-pr-icon-{i}"), pull, true));
            right.push(web::inline_link(
                &format!("branch-pr-{i}"),
                format!("#{}", pull.number),
                format!("/{path}/pull/{}", pull.number),
                12,
                ACCENT,
            ));
        } else if *branch != default {
            right.push(grey_link(
                &format!("branch-new-pr-{i}"),
                "New pull request",
                format!("/{path}/compare"),
            ));
        }
        rows.push(web::divider(&format!("branch-rule-{i}")));
        rows.push(between(
            &format!("branch-row-{i}"),
            style().padding(10),
            left,
            right,
        ));
    }
    let body = vec![
        bold("branches-title", "Branches", 20, INK),
        chips(
            "branches-tabs",
            12,
            style(),
            ["Overview", "Yours", "Active", "Stale", "All"]
                .iter()
                .enumerate()
                .map(|(i, label)| {
                    tab(
                        &format!("btab-{i}"),
                        "branch",
                        label,
                        None,
                        format!("/{path}/branches"),
                        i == 0,
                    )
                })
                .collect(),
        ),
        web::divider("branches-tabs-rule"),
        web::card(
            "branches-list",
            style().border(LINE).radius(6).padding(0),
            rows,
        ),
    ];
    web::themed_page(
        &format!("Branches · {path}"),
        theme(state),
        repo_frame(repository, name, actor, Tab::Code, body),
    )
}

fn stargazers_page(
    state: &GitState,
    repository: &Repository,
    name: &str,
    actor: &str,
) -> Result<HttpResponse> {
    let path = slug(repository, name);
    let cards = repository
        .stars
        .iter()
        .enumerate()
        .map(|(i, who)| {
            web::card(
                &format!("stargazer-{i}"),
                style().border(LINE).radius(6).padding(12),
                vec![chips(
                    &format!("stargazer-row-{i}"),
                    10,
                    style(),
                    vec![
                        web::avatar(&format!("stargazer-avatar-{i}"), who, 48),
                        column(
                            &format!("stargazer-text-{i}"),
                            2,
                            style(),
                            vec![
                                web::inline_link(
                                    &format!("stargazer-name-{i}"),
                                    who,
                                    format!("/{who}"),
                                    15,
                                    ACCENT,
                                ),
                                muted(&format!("stargazer-login-{i}"), format!("@{who}")),
                            ],
                        ),
                    ],
                )],
            )
        })
        .collect();
    let body = vec![
        bold("stars-title", "Stargazers", 20, INK),
        muted(
            "stars-count",
            format!("{} people starred {path}", repository.stars.len()),
        ),
        web::grid("stargazer-grid", 3, 12, cards),
    ];
    web::themed_page(
        &format!("Stargazers · {path}"),
        theme(state),
        repo_frame(repository, name, actor, Tab::Code, body),
    )
}

fn list_page(
    state: &GitState,
    repository: &Repository,
    name: &str,
    actor: &str,
    pulls: bool,
    filter: &str,
    now: u64,
) -> Result<HttpResponse> {
    let path = slug(repository, name);
    let threads = if pulls {
        &repository.pull_requests
    } else {
        &repository.issues
    };
    let (title, route, kind) = if pulls {
        ("Pull requests", "pull", "pr")
    } else {
        ("Issues", "issues", "issue")
    };
    let list_route = if pulls { "pulls" } else { "issues" };
    let closed = filter == "closed";
    let open = open_count(threads);
    let shut = threads.len() - open;
    let mut rows = vec![between(
        "list-head",
        style().background(SURFACE).padding(10),
        vec![
            ic(
                "list-open-icon",
                if pulls { "pull-request" } else { "issue-open" },
                16,
                if closed { MUTED } else { INK },
            ),
            web::styled_link(
                "list-open",
                format!("{open} Open"),
                format!("/{path}/{list_route}"),
                style()
                    .size(13)
                    .color(if closed { MUTED } else { INK })
                    .weight(if closed { "regular" } else { "bold" })
                    .one_line(),
            ),
            ic(
                "list-closed-icon",
                "check",
                16,
                if closed { INK } else { MUTED },
            ),
            web::styled_link(
                "list-closed",
                format!("{shut} Closed"),
                format!("/{path}/{list_route}?state=closed"),
                style()
                    .size(13)
                    .color(if closed { INK } else { MUTED })
                    .weight(if closed { "bold" } else { "regular" })
                    .one_line(),
            ),
        ],
        [
            "Author",
            "Labels",
            "Projects",
            "Milestones",
            "Assignees",
            "Sort",
        ]
        .iter()
        .map(|f| {
            chips(
                &format!("filter-{}", f.to_ascii_lowercase()),
                3,
                style(),
                vec![
                    muted(&format!("filter-{}-label", f.to_ascii_lowercase()), *f),
                    ic(
                        &format!("filter-{}-chevron", f.to_ascii_lowercase()),
                        "chevron-down",
                        12,
                        MUTED,
                    ),
                ],
            )
        })
        .collect(),
    )];
    let mut shown: Vec<&Thread> = threads
        .values()
        .filter(|t| (t.state == "open") != closed)
        .collect();
    shown.sort_by_key(|t| std::cmp::Reverse(t.number));
    if shown.is_empty() {
        rows.push(web::divider("empty-rule"));
        rows.push(web::styled(
            "empty",
            "No results matched your search.",
            style().size(14).color(MUTED).padding(24).align("center"),
        ));
    }
    for thread in shown {
        let n = thread.number;
        let mut title_row = vec![web::inline_link(
            &format!("thread-{n}"),
            &thread.title,
            format!("/{path}/{route}/{n}"),
            16,
            INK,
        )];
        title_row.extend(labels(&format!("thread-{n}"), thread));
        let status = match thread.state.as_str() {
            "merged" => format!("by {} was merged {}", thread.author, ago(now, thread.tick)),
            "closed" => format!("by {} was closed {}", thread.author, ago(now, thread.tick)),
            _ => format!("opened {} by {}", ago(now, thread.tick), thread.author),
        };
        let mut meta = vec![muted(&format!("meta-{n}"), format!("#{n} {status}"))];
        if pulls && thread.draft {
            meta.push(muted(&format!("meta-draft-{n}"), "· Draft"));
        }
        if !thread.reviews.is_empty() {
            let approved = thread.reviews.iter().any(|r| r.decision == "approve");
            meta.push(ic(
                &format!("meta-review-icon-{n}"),
                if approved { "check" } else { "x-circle" },
                12,
                if approved { GREEN } else { RED },
            ));
            meta.push(muted(
                &format!("meta-review-{n}"),
                if approved {
                    "Approved"
                } else {
                    "Changes requested"
                },
            ));
        }
        let mut right = vec![];
        if !thread.assignee.is_empty() {
            right.push(web::avatar(&format!("assignee-{n}"), &thread.assignee, 20));
        }
        if !thread.comments.is_empty() {
            right.push(ic(&format!("comments-icon-{n}"), "comment", 16, MUTED));
            right.push(muted(
                &format!("comments-{n}"),
                thread.comments.len().to_string(),
            ));
        }
        rows.push(web::divider(&format!("rule-{n}")));
        rows.push(web::styled_row(
            &format!("row-{n}"),
            10,
            "start",
            style().padding(10),
            vec![
                web::styled(&format!("row-pad-{n}"), "", style().width(2)),
                state_icon(&format!("state-{n}"), thread, pulls),
                column(
                    &format!("row-main-{n}"),
                    4,
                    style().flex(1),
                    vec![
                        chips(&format!("row-title-{n}"), 6, style(), title_row),
                        chips(&format!("row-meta-{n}"), 6, style(), meta),
                    ],
                ),
                chips(
                    &format!("row-side-{n}"),
                    6,
                    style().width(120).justify("end"),
                    right,
                ),
            ],
        ));
    }
    let body = vec![
        between(
            "list-bar",
            style(),
            vec![chips(
                "list-search",
                8,
                style()
                    .border(LINE)
                    .radius(6)
                    .padding(6)
                    .width(520)
                    .background(SURFACE),
                vec![
                    ic("list-search-icon", "search", 14, MUTED),
                    text(
                        "list-search-hint",
                        format!("is:{kind} is:{}", if closed { "closed" } else { "open" }),
                        13,
                        MUTED,
                    ),
                ],
            )],
            vec![
                grey_link("labels-link", "Labels", format!("/{path}/{list_route}")),
                grey_link(
                    "milestones-link",
                    "Milestones",
                    format!("/{path}/{list_route}"),
                ),
                green_link(
                    "new",
                    if pulls {
                        "New pull request"
                    } else {
                        "New issue"
                    },
                    if pulls {
                        format!("/{path}/compare")
                    } else {
                        format!("/{path}/issues/new")
                    },
                ),
            ],
        ),
        web::card("threads", style().border(LINE).radius(6).padding(0), rows),
    ];
    web::themed_page(
        &format!("{title} · {path}"),
        theme(state),
        repo_frame(
            repository,
            name,
            actor,
            if pulls { Tab::Pulls } else { Tab::Issues },
            body,
        ),
    )
}
fn new_thread_page(
    state: &GitState,
    repository: &Repository,
    name: &str,
    actor: &str,
    pulls: bool,
) -> Result<HttpResponse> {
    let path = slug(repository, name);
    let form = if pulls {
        web::form(
            "new-pull",
            &format!("/{path}/pulls"),
            &[
                ("title", "Title", ""),
                ("head", "Compare branch (refs/heads/...)", "refs/heads/"),
                ("base", "Base branch", "refs/heads/main"),
            ],
        )
    } else {
        web::form(
            "new-issue",
            &format!("/{path}/issues"),
            &[
                ("title", "Add a title", ""),
                ("body", "Add a description", ""),
            ],
        )
    };
    let body = vec![
        bold(
            "new-title",
            if pulls {
                "Open a pull request"
            } else {
                "Create new issue"
            },
            20,
            INK,
        ),
        web::card(
            "new-card",
            style().border(LINE).radius(6).padding(16),
            vec![form],
        ),
    ];
    web::themed_page(
        &format!(
            "{} · {path}",
            if pulls {
                "Comparing changes"
            } else {
                "New Issue"
            }
        ),
        theme(state),
        repo_frame(
            repository,
            name,
            actor,
            if pulls { Tab::Pulls } else { Tab::Issues },
            body,
        ),
    )
}

/// A comment card: the author strip on grey, the body beneath.
fn comment_card(
    prefix: &str,
    author: &str,
    verb: &str,
    body: &str,
    badge: Option<&str>,
) -> PageElement {
    let mut strip = vec![
        web::avatar(&format!("{prefix}-avatar"), author, 24),
        web::inline_link(
            &format!("{prefix}-author"),
            author,
            format!("/{author}"),
            13,
            INK,
        ),
        muted(&format!("{prefix}-when"), verb),
    ];
    if let Some(badge) = badge {
        strip.push(web::chip(
            &format!("{prefix}-badge"),
            badge,
            "#ffffff",
            MUTED,
            style().size(10).padding(4).border(LINE),
        ));
    }
    let mut children = vec![
        chips(
            &format!("{prefix}-strip"),
            8,
            style().background(SURFACE).padding(10),
            strip,
        ),
        web::divider(&format!("{prefix}-rule")),
    ];
    let shown = if body.trim().is_empty() {
        "No description provided.".to_owned()
    } else {
        body.to_owned()
    };
    children.push(column(
        &format!("{prefix}-body"),
        6,
        style().padding(14),
        with_links(&format!("{prefix}-text"), &shown, 14),
    ));
    web::card(prefix, style().border(LINE).radius(6).padding(0), children)
}
/// The sidebar of a thread page.
fn thread_sidebar(
    prefix: &str,
    repository: &Repository,
    name: &str,
    thread: &Thread,
    pulls: bool,
) -> PageElement {
    let path = slug(repository, name);
    let mut items = vec![];
    let section =
        |items: &mut Vec<PageElement>, id: &str, title: &str, content: Vec<PageElement>| {
            items.push(chips(
                &format!("{prefix}-{id}-head"),
                4,
                style(),
                vec![
                    bold(&format!("{prefix}-{id}-title"), title, 12, MUTED),
                    ic(&format!("{prefix}-{id}-gear"), "gear", 12, MUTED),
                ],
            ));
            items.extend(content);
            items.push(web::divider(&format!("{prefix}-{id}-rule")));
        };
    if pulls {
        let mut reviewers = vec![];
        for (i, review) in thread.reviews.iter().enumerate() {
            let (icon, colour) = match review.decision.as_str() {
                "approve" => ("check", GREEN),
                "request_changes" => ("x-circle", RED),
                _ => ("comment", MUTED),
            };
            reviewers.push(chips(
                &format!("{prefix}-reviewer-{i}"),
                6,
                style(),
                vec![
                    web::avatar(&format!("{prefix}-reviewer-avatar-{i}"), &review.author, 20),
                    text(
                        &format!("{prefix}-reviewer-name-{i}"),
                        &review.author,
                        12,
                        INK,
                    ),
                    ic(&format!("{prefix}-reviewer-icon-{i}"), icon, 14, colour),
                ],
            ));
        }
        if reviewers.is_empty() {
            reviewers.push(muted(&format!("{prefix}-reviewers-none"), "No reviews"));
        }
        section(&mut items, "reviewers", "Reviewers", reviewers);
    }
    let assignees = if thread.assignee.is_empty() {
        vec![muted(&format!("{prefix}-assignee-none"), "No one assigned")]
    } else {
        vec![chips(
            &format!("{prefix}-assignee"),
            6,
            style(),
            vec![
                web::avatar(&format!("{prefix}-assignee-avatar"), &thread.assignee, 20),
                web::inline_link(
                    &format!("{prefix}-assignee-name"),
                    &thread.assignee,
                    format!("/{}", thread.assignee),
                    12,
                    INK,
                ),
            ],
        )]
    };
    section(&mut items, "assignees", "Assignees", assignees);
    let label_chips = if thread.labels.is_empty() {
        vec![muted(&format!("{prefix}-labels-none"), "None yet")]
    } else {
        vec![chips(
            &format!("{prefix}-labels"),
            4,
            style(),
            labels(&format!("{prefix}-side"), thread),
        )]
    };
    section(&mut items, "labels", "Labels", label_chips);
    section(
        &mut items,
        "projects",
        "Projects",
        vec![muted(&format!("{prefix}-projects-none"), "None yet")],
    );
    section(
        &mut items,
        "milestone",
        "Milestone",
        vec![muted(&format!("{prefix}-milestone-none"), "No milestone")],
    );
    // Development: the pull request that closes this issue, or the issues this pull closes.
    let mut development = vec![];
    if pulls {
        for (i, issue) in repository.issues.values().enumerate() {
            if thread.body.contains(&format!("#{}", issue.number)) {
                development.push(chips(
                    &format!("{prefix}-dev-{i}"),
                    6,
                    style(),
                    vec![
                        state_icon(&format!("{prefix}-dev-icon-{i}"), issue, false),
                        web::inline_link(
                            &format!("{prefix}-dev-link-{i}"),
                            format!("#{} {}", issue.number, issue.title),
                            format!("/{path}/issues/{}", issue.number),
                            12,
                            INK,
                        ),
                    ],
                ));
            }
        }
    } else {
        for (i, pull) in repository.pull_requests.values().enumerate() {
            if pull.body.contains(&format!("#{}", thread.number)) {
                development.push(chips(
                    &format!("{prefix}-dev-{i}"),
                    6,
                    style(),
                    vec![
                        state_icon(&format!("{prefix}-dev-icon-{i}"), pull, true),
                        web::inline_link(
                            &format!("{prefix}-dev-link-{i}"),
                            format!("#{} {}", pull.number, pull.title),
                            format!("/{path}/pull/{}", pull.number),
                            12,
                            INK,
                        ),
                    ],
                ));
            }
        }
    }
    if development.is_empty() {
        development.push(muted(
            &format!("{prefix}-dev-none"),
            if pulls {
                "Successfully merging this pull request may close these issues."
            } else {
                "No branches or pull requests"
            },
        ));
    }
    section(&mut items, "development", "Development", development);
    let mut people: Vec<&str> = std::iter::once(thread.author.as_str())
        .chain(thread.comments.iter().map(|c| c.author.as_str()))
        .chain(thread.reviews.iter().map(|r| r.author.as_str()))
        .collect();
    people.sort_unstable();
    people.dedup();
    items.push(bold(
        &format!("{prefix}-participants-title"),
        format!("{} participants", people.len()),
        12,
        MUTED,
    ));
    items.push(chips(
        &format!("{prefix}-participants"),
        4,
        style(),
        people
            .iter()
            .map(|p| web::avatar(&format!("{prefix}-participant-{p}"), p, 26))
            .collect(),
    ));
    column(&format!("{prefix}-sidebar"), 10, style().width(256), items)
}
/// The comment box at the foot of every thread, with the state buttons beside it.
fn composer(base: &str, thread: &Thread, pulls: bool, actor: &str) -> PageElement {
    let comment = post(format!("{base}/comments"), &[("body", "$comment-body")]);
    let mut buttons = vec![];
    match thread.state.as_str() {
        "open" => buttons.push(grey_button(
            "close",
            if pulls {
                "Close pull request"
            } else {
                "Close issue"
            },
            post(format!("{base}/state"), &[("state", "closed")]),
        )),
        "closed" => buttons.push(grey_button(
            "reopen",
            if pulls {
                "Reopen pull request"
            } else {
                "Reopen issue"
            },
            post(format!("{base}/state"), &[("state", "open")]),
        )),
        _ => (),
    }
    buttons.push(green_button("comment-submit", "Comment", comment.clone()));
    web::styled_row(
        "composer",
        12,
        "start",
        style(),
        vec![
            web::avatar("composer-avatar", actor, 40),
            web::card(
                "composer-card",
                style().border(LINE).radius(6).padding(12).flex(1),
                vec![PageElement::Form {
                    id: "comment".into(),
                    action: comment,
                    children: vec![
                        PageElement::Input {
                            id: "comment-body".into(),
                            label: "Comment".into(),
                            value: String::new(),
                            placeholder: "Add your comment here...".into(),
                        },
                        web::styled_row(
                            "composer-buttons",
                            8,
                            "center",
                            style().justify("end"),
                            buttons,
                        ),
                    ],
                }],
            ),
        ],
    )
}
fn thread_head(
    repository: &Repository,
    name: &str,
    thread: &Thread,
    pulls: bool,
    now: u64,
) -> Vec<PageElement> {
    let path = slug(repository, name);
    let n = thread.number;
    let mut meta = vec![state_pill("state", thread, pulls)];
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
                if count == 1 { "" } else { "s" }
            ),
            _ => format!(
                "{} wants to merge {count} commit{} into",
                thread.author,
                if count == 1 { "" } else { "s" }
            ),
        };
        meta.push(text("thread-verb", verb, 14, MUTED));
        meta.push(ref_chip(
            "thread-base",
            base,
            &format!("/{path}/tree/{base}"),
        ));
        meta.push(text("thread-from", "from", 14, MUTED));
        meta.push(ref_chip(
            "thread-head-ref",
            head,
            &format!("/{path}/tree/{head}"),
        ));
    } else {
        meta.push(text(
            "thread-meta",
            format!(
                "{} opened this issue {} · {} comment{}",
                thread.author,
                ago(now, thread.tick),
                thread.comments.len(),
                if thread.comments.len() == 1 { "" } else { "s" }
            ),
            14,
            MUTED,
        ));
    }
    vec![
        between(
            "thread-title-row",
            style(),
            vec![
                web::styled(
                    "thread-title",
                    &thread.title,
                    style().size(26).color(INK).bold().width(760),
                ),
                text("thread-number", format!("#{n}"), 26, MUTED),
            ],
            vec![
                grey_link(
                    "edit",
                    "Edit",
                    format!("/{path}/{}/{n}", if pulls { "pull" } else { "issues" }),
                ),
                green_link(
                    "new-from-thread",
                    if pulls {
                        "New pull request"
                    } else {
                        "New issue"
                    },
                    if pulls {
                        format!("/{path}/compare")
                    } else {
                        format!("/{path}/issues/new")
                    },
                ),
            ],
        ),
        chips("thread-head", 8, style(), meta),
        web::divider("thread-head-rule"),
    ]
}
fn issue_page(
    state: &GitState,
    repository: &Repository,
    name: &str,
    actor: &str,
    thread: &Thread,
    now: u64,
) -> Result<HttpResponse> {
    let path = slug(repository, name);
    let n = thread.number;
    let base = format!("/{path}/issues/{n}");
    let mut timeline = vec![comment_card(
        "thread-body",
        &thread.author,
        &format!("opened {}", ago(now, thread.tick)),
        &thread.body,
        Some("Author"),
    )];
    for (i, comment) in thread.comments.iter().enumerate() {
        timeline.push(comment_card(
            &format!("comment-{i}"),
            &comment.author,
            &format!("commented {}", ago(now, comment.tick)),
            &comment.body,
            (comment.author == thread.author).then_some("Author"),
        ));
    }
    if thread.state == "closed" {
        let last = thread.comments.last().map_or(thread.tick, |c| c.tick);
        timeline.push(chips(
            "closed-event",
            8,
            style().padding(4),
            vec![
                ic("closed-event-icon", "issue-closed", 16, PURPLE),
                text(
                    "closed-event-text",
                    format!(
                        "{} closed this as completed {}",
                        if thread.assignee.is_empty() {
                            &thread.author
                        } else {
                            &thread.assignee
                        },
                        ago(now, last)
                    ),
                    13,
                    MUTED,
                ),
            ],
        ));
    }
    timeline.push(web::divider("timeline-rule"));
    timeline.push(composer(&base, thread, false, actor));
    let mut body = thread_head(repository, name, thread, false, now);
    body.push(web::styled_row(
        "thread",
        24,
        "start",
        style(),
        vec![
            column("timeline", 12, style().flex(1), timeline),
            thread_sidebar("side", repository, name, thread, false),
        ],
    ));
    web::themed_page(
        &format!("{} · Issue #{n} · {path}", thread.title),
        theme(state),
        repo_frame(repository, name, actor, Tab::Issues, body),
    )
}
/// The green (open), grey (draft), purple (merged) or red (closed) box at the foot of a
/// pull request's conversation.
fn merge_box(base: &str, thread: &Thread) -> PageElement {
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
    let status = |id: &str, icon: &str, colour: &str, title: &str, detail: &str| {
        web::styled_row(
            id,
            12,
            "start",
            style().padding(14),
            vec![
                web::icon(
                    &format!("{id}-icon"),
                    icon,
                    icon,
                    style()
                        .size(18)
                        .color("#ffffff")
                        .background(colour)
                        .radius(16)
                        .padding(7),
                ),
                column(
                    &format!("{id}-text"),
                    2,
                    style().flex(1),
                    vec![
                        bold(&format!("{id}-title"), title, 14, INK),
                        muted(&format!("{id}-detail"), detail),
                    ],
                ),
            ],
        )
    };
    let rows = match (thread.state.as_str(), thread.draft) {
        ("merged", _) => vec![
            status(
                "merged-status",
                "merge",
                PURPLE,
                "Pull request successfully merged and closed",
                &format!("You're all set — the {head} branch can be safely deleted."),
            ),
            web::divider("merge-rule-1"),
            chips(
                "merge-actions",
                8,
                style().padding(12),
                vec![grey_link(
                    "delete-branch",
                    "Delete branch",
                    base.to_string(),
                )],
            ),
        ],
        ("closed", _) => vec![
            status(
                "closed-status",
                "pull-request",
                RED,
                "Closed with unmerged commits",
                &format!(
                    "This pull request is closed, but the {head} branch has unmerged changes."
                ),
            ),
            web::divider("merge-rule-1"),
            chips(
                "merge-actions",
                8,
                style().padding(12),
                vec![grey_button(
                    "reopen",
                    "Reopen pull request",
                    post(format!("{base}/state"), &[("state", "open")]),
                )],
            ),
        ],
        (_, true) => vec![
            status(
                "draft-status",
                "pull-request",
                MUTED,
                "This pull request is still a work in progress",
                "Draft pull requests cannot be merged.",
            ),
            web::divider("merge-rule-1"),
            chips(
                "merge-actions",
                8,
                style().padding(12),
                vec![grey_button(
                    "ready",
                    "Ready for review",
                    post(
                        format!("{base}/reviews"),
                        &[("decision", "comment"), ("body", "Ready for review")],
                    ),
                )],
            ),
        ],
        _ => {
            let (icon, colour, title, detail) = if changes > 0 && approvals == 0 {
                (
                    "x-circle",
                    RED,
                    "Changes requested".to_owned(),
                    format!(
                        "{changes} review{} requesting changes",
                        if changes == 1 { "" } else { "s" }
                    ),
                )
            } else if approvals > 0 {
                (
                    "check",
                    GREEN,
                    "Changes approved".to_owned(),
                    format!(
                        "{approvals} approving review{} by reviewers with write access.",
                        if approvals == 1 { "" } else { "s" }
                    ),
                )
            } else {
                (
                    "eye",
                    MUTED,
                    "Review required".to_owned(),
                    "At least 1 approving review is required by reviewers with write access."
                        .to_owned(),
                )
            };
            vec![
                status("review-status", icon, colour, &title, &detail),
                web::divider("merge-rule-1"),
                status(
                    "conflict-status",
                    "check",
                    GREEN,
                    "This branch has no conflicts with the base branch",
                    "Merging can be performed automatically.",
                ),
                web::divider("merge-rule-2"),
                chips(
                    "merge-actions",
                    8,
                    style().padding(12),
                    vec![
                        green_button(
                            "merge",
                            "Merge pull request",
                            post(format!("{base}/merge"), &[]),
                        ),
                        web::styled_button(
                            "merge-options",
                            "▾",
                            post(format!("{base}/merge"), &[]),
                            button_style()
                                .padding(10)
                                .background(GREEN)
                                .border(GREEN)
                                .color("#ffffff"),
                        ),
                        muted(
                            "merge-hint",
                            "You can also merge this with the command line.",
                        ),
                    ],
                ),
            ]
        }
    };
    web::card("merge-box", style().border(LINE).radius(6).padding(0), rows)
}
fn pull_page(
    state: &GitState,
    repository: &Repository,
    name: &str,
    actor: &str,
    thread: &Thread,
    view: &str,
    now: u64,
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
    let mut body = thread_head(repository, name, thread, true, now);
    body.push(chips(
        "pull-tabs",
        12,
        style(),
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
                "ptab-checks",
                "check",
                "Checks",
                Some(0),
                base.clone(),
                false,
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
    ));
    body.push(web::divider("pull-tabs-rule"));
    match view {
        "commits" => body.extend(commit_groups("pull-commits", &path, &commits, now)),
        "files" => {
            body.push(between(
                "files-bar",
                style(),
                vec![muted("files-hint", "Changes from all commits")],
                vec![web::styled_button(
                    "approve",
                    "Review changes ▾",
                    post(
                        format!("{base}/reviews"),
                        &[("decision", "approve"), ("body", "Looks good.")],
                    ),
                    button_style()
                        .padding(10)
                        .background(GREEN)
                        .border(GREEN)
                        .color("#ffffff"),
                )],
            ));
            body.extend(diff_section("diff", &files, &path, head_name));
        }
        _ => {
            let mut timeline = vec![comment_card(
                "thread-body",
                &thread.author,
                &format!("commented {}", ago(now, thread.tick)),
                &thread.body,
                Some("Author"),
            )];
            // Comments and reviews interleave by time, as one conversation.
            let mut events: Vec<(u64, usize, PageElement)> = vec![];
            for (i, comment) in thread.comments.iter().enumerate() {
                events.push((
                    comment.tick,
                    i,
                    comment_card(
                        &format!("comment-{i}"),
                        &comment.author,
                        &format!("commented {}", ago(now, comment.tick)),
                        &comment.body,
                        (comment.author == thread.author).then_some("Author"),
                    ),
                ));
            }
            for (i, review) in thread.reviews.iter().enumerate() {
                let (icon, colour, verb) = match review.decision.as_str() {
                    "approve" => ("check", GREEN, "approved these changes"),
                    "request_changes" => ("x-circle", RED, "requested changes"),
                    _ => ("comment", MUTED, "reviewed"),
                };
                let mut children = vec![chips(
                    &format!("review-{i}-head"),
                    8,
                    style().padding(4),
                    vec![
                        ic(&format!("review-state-{i}"), icon, 16, colour),
                        web::avatar(&format!("review-avatar-{i}"), &review.author, 20),
                        web::inline_link(
                            &format!("review-author-{i}"),
                            &review.author,
                            format!("/{}", review.author),
                            13,
                            INK,
                        ),
                        muted(
                            &format!("review-verb-{i}"),
                            format!("{verb} {}", ago(now, review.tick)),
                        ),
                    ],
                )];
                if !review.body.trim().is_empty() {
                    children.push(web::card(
                        &format!("review-body-{i}"),
                        style().border(LINE).radius(6).padding(12),
                        with_links(&format!("review-text-{i}"), &review.body, 14),
                    ));
                }
                events.push((
                    review.tick,
                    100 + i,
                    column(&format!("review-{i}"), 6, style(), children),
                ));
            }
            events.sort_by_key(|(tick, order, _)| (*tick, *order));
            timeline.extend(events.into_iter().map(|(_, _, e)| e));
            if thread.state == "merged" {
                timeline.push(chips(
                    "merged-event",
                    8,
                    style().padding(4),
                    vec![
                        ic("merged-event-icon", "merge", 16, PURPLE),
                        text(
                            "merged-event-text",
                            format!(
                                "{} merged commit into {base_name} {}",
                                thread.merged_by,
                                ago(now, thread.tick)
                            ),
                            13,
                            MUTED,
                        ),
                    ],
                ));
            }
            timeline.push(merge_box(&base, thread));
            timeline.push(web::divider("timeline-rule"));
            timeline.push(composer(&base, thread, true, actor));
            body.push(web::styled_row(
                "thread",
                24,
                "start",
                style(),
                vec![
                    column("timeline", 12, style().flex(1), timeline),
                    thread_sidebar("side", repository, name, thread, true),
                ],
            ));
        }
    }
    web::themed_page(
        &format!(
            "{} by {} · Pull Request #{n} · {path}",
            thread.title, thread.author
        ),
        theme(state),
        repo_frame(repository, name, actor, Tab::Pulls, body),
    )
}
/// The tabs that have no data behind them: Actions, Projects, Wiki, Security, Insights, Settings.
fn stub_page(
    state: &GitState,
    repository: &Repository,
    name: &str,
    actor: &str,
    which: &str,
) -> Result<HttpResponse> {
    let (title, blurb) = match which {
        "actions" => ("Get started with GitHub Actions", "Build, test, and deploy your code. Make code reviews, branch management, and issue triaging work the way you want."),
        "projects" => ("Welcome to the all-new projects", "Built like a spreadsheet, project tables give you a live canvas to filter, sort, and group issues and pull requests."),
        "wiki" => ("Welcome to the wiki!", "Wikis provide a place in your repository to lay out the roadmap of your project, show the current status, and document software better, together."),
        "security" => ("Security overview", "Security policy, advisories and Dependabot alerts for this repository."),
        "pulse" => ("Pulse", "Activity over the last month: merged pull requests, closed issues and new commits."),
        _ => ("Settings", "General settings for this repository."),
    };
    let body = vec![web::card(
        "stub",
        style().border(LINE).radius(6).padding(32),
        vec![
            web::styled(
                "stub-title",
                title,
                style().size(22).bold().color(INK).align("center"),
            ),
            web::styled(
                "stub-blurb",
                blurb,
                style().size(14).color(MUTED).align("center"),
            ),
        ],
    )];
    web::themed_page(
        &format!("{title} · {}", slug(repository, name)),
        theme(state),
        repo_frame(repository, name, actor, Tab::Other, body),
    )
}

fn gist_index(state: &GitState, actor: &str, now: u64) -> Result<HttpResponse> {
    let mut rows = vec![];
    for (id, gist) in &state.gists {
        let file = gist.files.keys().next().cloned().unwrap_or_default();
        rows.push(web::card(
            &format!("gist-row-{id}"),
            style().border(LINE).radius(6).padding(12),
            vec![
                chips(
                    &format!("gist-head-{id}"),
                    6,
                    style(),
                    vec![
                        web::avatar(&format!("gist-avatar-{id}"), &gist.owner, 24),
                        web::inline_link(
                            &format!("gist-owner-{id}"),
                            &gist.owner,
                            format!("/{}", gist.owner),
                            14,
                            ACCENT,
                        ),
                        text(&format!("gist-slash-{id}"), "/", 14, MUTED),
                        web::styled_link(
                            &format!("gist-{id}"),
                            &file,
                            format!("/gist/{id}"),
                            style().size(14).bold().color(ACCENT).one_line(),
                        ),
                    ],
                ),
                muted(
                    &format!("gist-when-{id}"),
                    format!("Created {}", ago(now, gist.tick)),
                ),
                text(&format!("gist-desc-{id}"), &gist.description, 13, INK),
            ],
        ));
    }
    let mut elements = vec![
        header(&[("gists", "/gists".into())], actor),
        web::spacer("gists-lead", 16),
        bold("gists-title", "Discover gists", 20, INK),
    ];
    elements.extend(rows);
    web::themed_page("Discover gists · GitHub", theme(state), elements)
}

fn gist_page(state: &GitState, id: &str, actor: &str, now: u64) -> Result<HttpResponse> {
    let Some(gist) = state.gists.get(id) else {
        return web::error(404, "gist not found");
    };
    let mut elements = vec![
        header(
            &[("gists", "/gists".into()), (id, format!("/gist/{id}"))],
            actor,
        ),
        web::spacer("gist-lead", 16),
        chips(
            "gist-head",
            8,
            style(),
            vec![
                web::avatar("gist-avatar", &gist.owner, 32),
                web::inline_link(
                    "gist-owner",
                    &gist.owner,
                    format!("/{}", gist.owner),
                    18,
                    ACCENT,
                ),
                text("gist-slash", "/", 18, MUTED),
                bold("gist-id", id, 18, ACCENT),
            ],
        ),
        muted("gist-when", format!("Created {}", ago(now, gist.tick))),
        text("gist-description", &gist.description, 14, INK),
    ];
    for (i, (file, content)) in gist.files.iter().enumerate() {
        let numbers: Vec<String> = (1..=content.lines().count())
            .map(|n| n.to_string())
            .collect();
        elements.push(web::card(
            &format!("gist-file-{i}"),
            style().border(LINE).radius(6).padding(0),
            vec![
                chips(
                    &format!("gist-file-head-{i}"),
                    6,
                    style().background(SURFACE).padding(10),
                    vec![
                        ic(&format!("gist-file-icon-{i}"), "file", 14, MUTED),
                        bold(&format!("gist-file-name-{i}"), file, 13, ACCENT),
                    ],
                ),
                web::divider(&format!("gist-file-rule-{i}")),
                web::styled_row(
                    &format!("gist-file-body-{i}"),
                    12,
                    "start",
                    style().padding(8),
                    vec![
                        web::styled(
                            &format!("gist-file-numbers-{i}"),
                            numbers.join("\n"),
                            style()
                                .size(12)
                                .mono()
                                .color(MUTED)
                                .width(32)
                                .align("right"),
                        ),
                        web::styled(
                            &format!("gist-file-text-{i}"),
                            content.trim_end_matches('\n'),
                            style().size(12).mono().color(INK).flex(1),
                        ),
                    ],
                ),
            ],
        ));
    }
    web::themed_page(
        &format!("{} · gist", gist.description),
        theme(state),
        elements,
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
        let now = now(&s, ctx);
        let repo = |owner: &str, name: &str| repository(&s, owner, name);
        let missing = || web::error(404, "repository not found");
        return match parts.as_slice() {
            [] => home(&s, actor, now),
            ["search"] => search_page(&s, actor, &web::query(req, "q").unwrap_or_default()),
            ["gists"] => gist_index(&s, actor, now),
            ["gist", id] => gist_page(&s, id, actor, now),
            [owner] => owner_page(&s, owner, actor, now),
            [owner, name] => match repo(owner, name) {
                Some(r) if api => HttpResponse::json(200, &json!(r)),
                Some(r) => code_page(&s, r, name, actor, &default_branch(r), "", now),
                None => missing(),
            },
            [owner, name, "tree", branch, rest @ ..] => match repo(owner, name) {
                Some(r) => code_page(&s, r, name, actor, branch, &rest.join("/"), now),
                None => missing(),
            },
            [owner, name, "blob" | "raw", rest @ ..] if !rest.is_empty() => match repo(owner, name)
            {
                Some(r) => {
                    // `/blob/<branch>/<path>` is GitHub's shape; `/blob/<path>` is the older
                    // one this site used, and still resolves on the default branch.
                    let (branch, file) = if rest.len() > 1
                        && r.refs.contains_key(&format!("refs/heads/{}", rest[0]))
                    {
                        (rest[0].to_owned(), rest[1..].join("/"))
                    } else {
                        (default_branch(r), rest.join("/"))
                    };
                    blob_page(&s, r, name, actor, &branch, &file, now)
                }
                None => missing(),
            },
            [owner, name, "commits"] => match repo(owner, name) {
                Some(r) => commits_page(&s, r, name, actor, &default_branch(r), now),
                None => missing(),
            },
            [owner, name, "commits", branch] => match repo(owner, name) {
                Some(r) => commits_page(&s, r, name, actor, branch, now),
                None => missing(),
            },
            [owner, name, "commit", sha] => match repo(owner, name) {
                Some(r) => {
                    let full = r.objects.keys().find(|k| k.starts_with(sha)).cloned();
                    match full {
                        Some(id) => commit_page(&s, r, name, actor, &id, now),
                        None => web::error(404, "commit not found"),
                    }
                }
                None => missing(),
            },
            [owner, name, "branches"] => match repo(owner, name) {
                Some(r) => branches_page(&s, r, name, actor, now),
                None => missing(),
            },
            [owner, name, "stargazers"] => match repo(owner, name) {
                Some(r) if api => HttpResponse::json(200, &json!(r.stars)),
                Some(r) => stargazers_page(&s, r, name, actor),
                None => missing(),
            },
            [owner, name, "issues", "new"] => match repo(owner, name) {
                Some(r) => new_thread_page(&s, r, name, actor, false),
                None => missing(),
            },
            [owner, name, "compare"] => match repo(owner, name) {
                Some(r) => new_thread_page(&s, r, name, actor, true),
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
                Some(r) => list_page(
                    &s,
                    r,
                    name,
                    actor,
                    *kind == "pulls",
                    &web::query(req, "state").unwrap_or_default(),
                    now,
                ),
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
                            pull_page(&s, r, name, actor, t, view, now)
                        }
                        Some(t) if !pulls && view == "conversation" => {
                            issue_page(&s, r, name, actor, t, now)
                        }
                        Some(_) => web::error(404, "route not found"),
                        None => web::error(404, "thread not found"),
                    },
                    (Some(_), Err(_)) => web::error(404, "thread not found"),
                    (None, _) => missing(),
                }
            }
            [owner, name, which @ ("actions" | "projects" | "wiki" | "security" | "pulse" | "settings")] => {
                match repo(owner, name) {
                    Some(r) => stub_page(&s, r, name, actor, which),
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
    fn markdown_makes_headings_lists_and_code() {
        let out = markdown("md", "# Title\n\nSome `prose` here.\n\n- one http://a.example/\n- two\n\n```\nlet x = 1;\n```\n");
        let kinds: Vec<String> = out.iter().map(|e| e.id().to_owned()).collect();
        assert!(kinds[0].starts_with("md-h-"));
        assert!(out.iter().any(|e| matches!(e, PageElement::Styled { text, style, .. } if text == "let x = 1;" && style.mono == Some(true))));
        assert!(out
            .iter()
            .any(|e| matches!(e, PageElement::Styled { text, .. } if text == "Some prose here.")));
        assert!(out.iter().any(|e| matches!(e, PageElement::Row { children, .. } if children.iter().any(|c| matches!(c, PageElement::Link { url, .. } if url == "http://a.example/")))));
    }
    #[test]
    fn label_colours_are_githubs_for_known_names_and_stable_otherwise() {
        assert_eq!(label_colours("bug").0, "#d73a4a");
        assert_eq!(label_colours("good first issue").0, "#7057ff");
        assert_eq!(label_colours("launchpad"), label_colours("launchpad"));
    }
}
