//! Linear-skinned board, issue and workspace pages. The plain skin never reaches this module,
//! so issues.internal keeps rendering exactly as it always has.
use crate::{Issue, Project};
use cw_protocol::{HttpResponse, PageAction, PageElement, PageTheme, Result};
use cw_service_common as wire;

const ACCENT: &str = "#5e6ad2";
const INK: &str = "#282a30";
const MUTED: &str = "#6f6e77";
const SURFACE: &str = "#f9f8f9";
const LINE: &str = "#e4e2e4";
const SIDEBAR: &str = "#f4f2f4";

/// The four statuses the domain allows, in board order, with their column names and colours.
pub const COLUMNS: [(&str, &str, &str); 4] = [
    ("open", "Todo", "#8a8f98"),
    ("in_progress", "In Progress", "#f2c94c"),
    ("blocked", "Blocked", "#eb5757"),
    ("closed", "Done", ACCENT),
];
pub fn column_label(status: &str) -> &'static str {
    COLUMNS
        .iter()
        .find(|(id, ..)| *id == status)
        .map(|(_, label, _)| *label)
        .unwrap_or("Todo")
}
fn column_colour(status: &str) -> &'static str {
    COLUMNS
        .iter()
        .find(|(id, ..)| *id == status)
        .map(|(.., colour)| *colour)
        .unwrap_or(MUTED)
}
/// Presentation context carried past the mutable borrow of the service state.
pub struct Look {
    pub workspace: String,
    pub theme: Option<PageTheme>,
}
impl Look {
    pub fn theme(&self) -> PageTheme {
        self.theme.clone().unwrap_or(PageTheme {
            accent: Some(ACCENT.into()),
            background: Some("#ffffff".into()),
            surface: Some(SURFACE.into()),
            ink: Some(INK.into()),
            muted: Some(MUTED.into()),
            content_width: Some(1160),
            font: None,
        })
    }
    fn brand(&self) -> &str {
        if self.workspace.is_empty() {
            "Linear"
        } else {
            &self.workspace
        }
    }
}
fn muted(id: &str, text: impl Into<String>) -> PageElement {
    wire::styled(id, text, wire::style().size(12).color(MUTED))
}
/// Flat-colour stand-in for an avatar; the label is the accessible name, never a real photo.
fn avatar(id: &str, who: &str, size: u32) -> PageElement {
    wire::thumbnail(
        id,
        who,
        wire::style()
            .width(size)
            .height(size)
            .radius(size / 2)
            .background(ACCENT)
            .color("#ffffff")
            .size(10)
            .align("center"),
    )
}
fn status_badge(id: &str, status: &str) -> PageElement {
    wire::badge(
        id,
        column_label(status),
        wire::style()
            .background(column_colour(status))
            .color("#ffffff")
            .radius(10)
            .padding(4)
            .size(11)
            .medium(),
    )
}
fn label_badges(prefix: &str, issue: &Issue) -> Vec<PageElement> {
    issue
        .labels
        .iter()
        .enumerate()
        .map(|(i, l)| {
            wire::badge(
                &format!("{prefix}-label-{i}"),
                l,
                wire::style()
                    .background("#ffffff")
                    .color(MUTED)
                    .border(LINE)
                    .radius(10)
                    .padding(3)
                    .size(11),
            )
        })
        .collect()
}
/// Workspace rail. The wordmark is a real link home; nothing else here claims to be a control.
fn rail(look: &Look, projects: &[(String, String)], current: Option<&str>) -> PageElement {
    let mut items = vec![
        wire::styled(
            "workspace",
            look.brand(),
            wire::style().size(15).bold().color(INK),
        ),
        wire::divider("rail-rule"),
        muted("teams-label", "Teams"),
    ];
    for (key, name) in projects {
        items.push(wire::card_action(
            &format!("nav-{key}"),
            wire::style()
                .padding(6)
                .radius(6)
                .background(if current == Some(key.as_str()) {
                    "#ffffff"
                } else {
                    SIDEBAR
                }),
            wire::visit(format!("/projects/{key}")),
            vec![wire::styled(
                &format!("nav-{key}-text"),
                format!("{key} · {name}"),
                wire::style().size(13).color(INK),
            )],
        ));
    }
    wire::card(
        "rail",
        wire::style()
            .background(SIDEBAR)
            .padding(12)
            .width(220)
            .flex(0),
        items,
    )
}
fn shell(rail: PageElement, main: Vec<PageElement>) -> Vec<PageElement> {
    vec![wire::styled_row(
        "shell",
        0,
        "stretch",
        wire::style(),
        vec![
            rail,
            wire::card("main", wire::style().padding(16).flex(3), main),
        ],
    )]
}
/// Workspace home: every team the actor can read, with its open count.
pub fn home(look: &Look, projects: &[(String, Project)]) -> Result<HttpResponse> {
    let names: Vec<(String, String)> = projects
        .iter()
        .map(|(k, p)| (k.clone(), p.name.clone()))
        .collect();
    let cards = projects
        .iter()
        .map(|(key, project)| {
            let open = project
                .issues
                .values()
                .filter(|i| i.status != "closed")
                .count();
            wire::card_action(
                &format!("team-{key}"),
                wire::style()
                    .background("#ffffff")
                    .border(LINE)
                    .radius(8)
                    .padding(16),
                wire::visit(format!("/projects/{key}")),
                vec![
                    wire::styled(
                        &format!("team-name-{key}"),
                        &project.name,
                        wire::style().size(16).bold().color(INK),
                    ),
                    muted(&format!("team-key-{key}"), key),
                    wire::badge(
                        &format!("team-open-{key}"),
                        format!("{open} unfinished"),
                        wire::style()
                            .background(SURFACE)
                            .color(MUTED)
                            .border(LINE)
                            .radius(10)
                            .padding(4)
                            .size(12),
                    ),
                ],
            )
        })
        .collect();
    let main = vec![
        wire::styled(
            "home-title",
            "Your teams",
            wire::style().size(22).bold().color(INK),
        ),
        muted("home-sub", "Boards, cycles and everything still open."),
        wire::spacer("home-gap", 12),
        wire::grid("teams", 2, 16, cards),
    ];
    wire::themed_page(
        &format!("{} · Linear", look.brand()),
        look.theme(),
        shell(rail(look, &names, None), main),
    )
}
/// One issue card on the board, with the real moves available to it.
fn board_card(key: &str, issue: &Issue) -> PageElement {
    let id = issue.id;
    let mut head = vec![
        muted(&format!("card-key-{id}"), format!("{key}-{id}")),
        status_badge(&format!("card-state-{id}"), &issue.status),
    ];
    head.extend(label_badges(&format!("card-{id}"), issue));
    let mut children = vec![
        wire::styled_row(&format!("card-head-{id}"), 6, "center", wire::style(), head),
        wire::card_action(
            &format!("card-open-{id}"),
            wire::style(),
            wire::visit(format!("/projects/{key}/issues/{id}")),
            vec![wire::styled(
                &format!("card-title-{id}"),
                &issue.title,
                wire::style().size(14).medium().color(INK),
            )],
        ),
    ];
    if !issue.assignee.is_empty() {
        children.push(wire::styled_row(
            &format!("card-who-{id}"),
            6,
            "center",
            wire::style(),
            vec![
                avatar(&format!("card-avatar-{id}"), &issue.assignee, 20),
                muted(&format!("card-assignee-{id}"), &issue.assignee),
            ],
        ));
    }
    // Moving a card is a real status write, and it lands back on the board it was moved on.
    let at = COLUMNS.iter().position(|(s, ..)| *s == issue.status);
    let mut moves = vec![];
    for (delta, text) in [(-1i32, "←"), (1, "→")] {
        let Some(next) = at
            .and_then(|i| usize::try_from(i as i32 + delta).ok())
            .and_then(|i| COLUMNS.get(i))
        else {
            continue;
        };
        moves.push(PageElement::Button {
            id: format!("move-{id}-{}", next.0),
            text: format!("{text} {}", next.1),
            action: PageAction {
                method: "POST".into(),
                url: format!("/projects/{key}/issues/{id}"),
                fields: [
                    ("status".to_string(), next.0.to_string()),
                    ("view".to_string(), "board".to_string()),
                ]
                .into_iter()
                .collect(),
            },
            style: None,
        });
    }
    children.push(wire::styled_row(
        &format!("card-moves-{id}"),
        6,
        "center",
        wire::style(),
        moves,
    ));
    wire::card(
        &format!("card-{id}"),
        wire::style()
            .background("#ffffff")
            .border(LINE)
            .radius(8)
            .padding(10),
        children,
    )
}
/// The board: one column per status, filtered to one assignee when asked.
pub fn board(
    look: &Look,
    key: &str,
    project: &Project,
    projects: &[(String, String)],
    assignee: Option<&str>,
) -> Result<HttpResponse> {
    let visible: Vec<&Issue> = project
        .issues
        .values()
        .filter(|i| assignee.is_none_or(|who| i.assignee == who))
        .collect();
    let mut columns = vec![];
    for (status, label, colour) in COLUMNS {
        let mut children = vec![wire::styled_row(
            &format!("col-head-{status}"),
            6,
            "center",
            wire::style(),
            vec![
                wire::badge(
                    &format!("col-dot-{status}"),
                    label,
                    wire::style()
                        .background(colour)
                        .color("#ffffff")
                        .radius(10)
                        .padding(4)
                        .size(11)
                        .medium(),
                ),
                muted(
                    &format!("col-count-{status}"),
                    visible
                        .iter()
                        .filter(|i| i.status == status)
                        .count()
                        .to_string(),
                ),
            ],
        )];
        for issue in visible.iter().filter(|i| i.status == status) {
            children.push(board_card(key, issue));
        }
        columns.push(wire::card(
            &format!("col-{status}"),
            wire::style().background(SURFACE).radius(8).padding(8),
            children,
        ));
    }
    // Assignee filters are links, so the filtered board is a real, shareable URL.
    let mut people: Vec<&str> = project
        .issues
        .values()
        .map(|i| i.assignee.as_str())
        .filter(|a| !a.is_empty())
        .collect();
    people.sort_unstable();
    people.dedup();
    let mut filters = vec![wire::link("filter-all", "All", format!("/projects/{key}"))];
    for who in people {
        filters.push(wire::link(
            &format!("filter-{who}"),
            who,
            format!("/projects/{key}?assignee={who}"),
        ));
    }
    let main = vec![
        wire::styled_row(
            "board-head",
            8,
            "center",
            wire::style(),
            vec![
                wire::styled(
                    "board-title",
                    &project.name,
                    wire::style().size(20).bold().color(INK),
                ),
                wire::badge(
                    "board-key",
                    key,
                    wire::style()
                        .background(SURFACE)
                        .color(MUTED)
                        .border(LINE)
                        .radius(10)
                        .padding(4)
                        .size(12),
                ),
            ],
        ),
        wire::styled_row("filters", 8, "center", wire::style(), filters),
        wire::divider("board-rule"),
        wire::grid("board", 4, 12, columns),
        wire::spacer("new-gap", 16),
        wire::styled(
            "new-title",
            "New issue",
            wire::style().size(15).bold().color(INK),
        ),
        wire::form(
            "new-issue",
            &format!("/projects/{key}/issues"),
            &[("title", "Title", ""), ("body", "Description", "")],
        ),
    ];
    wire::themed_page(
        &format!("{} · {}", project.name, look.brand()),
        look.theme(),
        shell(rail(look, projects, Some(key)), main),
    )
}
/// One issue, with its comments, its reviews and every status it can legally move to.
pub fn issue(
    look: &Look,
    key: &str,
    item: &Issue,
    projects: &[(String, String)],
) -> Result<HttpResponse> {
    let id = item.id;
    let base = format!("/projects/{key}/issues/{id}");
    let mut head = vec![
        muted("issue-key", format!("{key}-{id}")),
        status_badge("issue-state", &item.status),
    ];
    head.extend(label_badges("issue", item));
    let mut main = vec![
        wire::link("back", "Board", format!("/projects/{key}")),
        wire::styled_row("issue-head", 8, "center", wire::style(), head),
        wire::styled(
            "issue-title",
            &item.title,
            wire::style().size(22).bold().color(INK),
        ),
        wire::styled_row(
            "issue-meta",
            8,
            "center",
            wire::style(),
            vec![
                avatar("issue-avatar", &item.assignee, 24),
                muted(
                    "issue-assignee",
                    if item.assignee.is_empty() {
                        format!("opened by {}", item.author)
                    } else {
                        format!("{} · assigned to {}", item.author, item.assignee)
                    },
                ),
            ],
        ),
        wire::card(
            "issue-body",
            wire::style().border(LINE).radius(8).padding(16),
            vec![wire::styled(
                "body",
                &item.body,
                wire::style().size(14).color(INK),
            )],
        ),
    ];
    if !item.source_ref.is_empty() {
        main.push(muted(
            "issue-refs",
            format!("{} → {}", item.source_ref, item.target_ref),
        ));
    }
    for (i, c) in item.comments.iter().enumerate() {
        main.push(wire::card(
            &format!("comment-{i}"),
            wire::style().border(LINE).radius(8).padding(12),
            vec![
                wire::styled_row(
                    &format!("comment-head-{i}"),
                    6,
                    "center",
                    wire::style(),
                    vec![
                        avatar(&format!("comment-avatar-{i}"), &c.author, 20),
                        wire::styled(
                            &format!("comment-author-{i}"),
                            &c.author,
                            wire::style().size(13).bold().color(INK),
                        ),
                        muted(&format!("comment-tick-{i}"), format!("tick {}", c.tick)),
                    ],
                ),
                wire::styled(
                    &format!("comment-body-{i}"),
                    &c.body,
                    wire::style().size(14).color(INK),
                ),
            ],
        ));
    }
    for (i, r) in item.reviews.iter().enumerate() {
        main.push(wire::styled_row(
            &format!("review-{i}"),
            8,
            "center",
            wire::style().padding(6),
            vec![
                wire::badge(
                    &format!("review-state-{i}"),
                    &r.decision,
                    wire::style()
                        .background(if r.decision == "approve" {
                            ACCENT
                        } else {
                            MUTED
                        })
                        .color("#ffffff")
                        .radius(10)
                        .padding(3)
                        .size(11),
                ),
                wire::styled(
                    &format!("review-body-{i}"),
                    format!("{}: {}", r.author, r.body),
                    wire::style().size(13).color(INK),
                ),
            ],
        ));
    }
    main.push(wire::form(
        "comment",
        &format!("{base}/comments"),
        &[("body", "Comment", "")],
    ));
    let moves = COLUMNS
        .iter()
        .filter(|(status, ..)| *status != item.status)
        .map(|(status, label, _)| PageElement::Button {
            id: format!("status-{status}"),
            text: format!("Move to {label}"),
            action: PageAction {
                method: "POST".into(),
                url: base.clone(),
                fields: [("status".to_string(), (*status).to_string())]
                    .into_iter()
                    .collect(),
            },
            style: None,
        })
        .collect();
    main.push(wire::styled_row(
        "status-moves",
        8,
        "center",
        wire::style(),
        moves,
    ));
    main.push(wire::form(
        "update",
        &base,
        &[("assignee", "Assignee", &item.assignee)],
    ));
    if item.kind == "pull_request" {
        main.push(wire::form(
            "review",
            &format!("{base}/reviews"),
            &[("decision", "Decision", "approve"), ("body", "Review", "")],
        ));
    }
    wire::themed_page(
        &format!("{key}-{id} {}", item.title),
        look.theme(),
        shell(rail(look, projects, Some(key)), main),
    )
}
