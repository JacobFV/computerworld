//! GitHub-skinned routes and pages: owner paths, issues, pull requests, stars and gists.
//! The plain skin never reaches this module, which is why `/repos/*` stays byte-identical.
use crate::{GitState, Repository, Thread};
use cw_protocol::{HttpRequest, HttpResponse, PageAction, PageElement, PageTheme, Result};
use cw_sdk::ServiceContext;
use cw_service_common as web;
use serde_json::{json, Value};

const ACCENT: &str = "#0969da";
const INK: &str = "#1f2328";
const MUTED: &str = "#59636e";
const SURFACE: &str = "#f6f8fa";
const LINE: &str = "#d0d7de";
const GREEN: &str = "#1f883d";
const PURPLE: &str = "#8250df";
const RED: &str = "#cf222e";

fn theme(state: &GitState) -> PageTheme {
    state.theme.clone().unwrap_or(PageTheme {
        accent: Some(ACCENT.into()),
        background: Some("#ffffff".into()),
        surface: Some(SURFACE.into()),
        ink: Some(INK.into()),
        muted: Some(MUTED.into()),
        content_width: Some(1000),
    })
}
/// Flat-colour stand-in for an avatar; the label is the accessible name, never a real photo.
fn avatar(id: &str, who: &str, size: u32) -> PageElement {
    web::thumbnail(
        id,
        who,
        web::style()
            .width(size)
            .height(size)
            .radius(size / 2)
            .background(SURFACE)
            .border(LINE)
            .color(MUTED)
            .size(11),
    )
}
fn pill(id: &str, text: &str, background: &str) -> PageElement {
    web::badge(
        id,
        text,
        web::style()
            .background(background)
            .color("#ffffff")
            .radius(10)
            .padding(4)
            .size(12)
            .medium(),
    )
}
fn counter(id: &str, text: String) -> PageElement {
    web::badge(
        id,
        text,
        web::style()
            .background(SURFACE)
            .color(MUTED)
            .border(LINE)
            .radius(10)
            .padding(4)
            .size(12),
    )
}
fn muted(id: &str, text: impl Into<String>) -> PageElement {
    web::styled(id, text, web::style().size(12).color(MUTED))
}
/// Top bar. The wordmark is a real link home; nothing else here claims to be a control.
fn chrome(trail: Vec<PageElement>) -> PageElement {
    let mut children = vec![web::link("wordmark", "GitHub", "/")];
    children.extend(trail);
    web::styled_row(
        "chrome",
        8,
        "center",
        web::style()
            .background("#24292f")
            .padding(12)
            .color("#ffffff"),
        children,
    )
}
fn state_pill(id: &str, thread: &Thread) -> PageElement {
    let (text, colour) = match thread.state.as_str() {
        "merged" => ("Merged", PURPLE),
        "closed" => ("Closed", RED),
        _ => ("Open", GREEN),
    };
    pill(id, text, colour)
}
fn labels(prefix: &str, thread: &Thread) -> Vec<PageElement> {
    thread
        .labels
        .iter()
        .enumerate()
        .map(|(i, l)| {
            web::badge(
                &format!("{prefix}-label-{i}"),
                l,
                web::style()
                    .background("#ddf4ff")
                    .color(ACCENT)
                    .border(ACCENT)
                    .radius(10)
                    .padding(3)
                    .size(11),
            )
        })
        .collect()
}
fn slug(repository: &Repository, name: &str) -> String {
    format!("{}/{name}", repository.owner)
}
fn tree(repository: &Repository) -> Vec<(String, String)> {
    repository
        .refs
        .get("refs/heads/main")
        .and_then(|id| repository.objects.get(id))
        .map(|c| {
            c.files
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        })
        .unwrap_or_default()
}
fn open_count(threads: &std::collections::BTreeMap<u64, Thread>) -> usize {
    threads.values().filter(|t| t.state == "open").count()
}

fn home(state: &GitState) -> Result<HttpResponse> {
    let mut cards = vec![];
    for (name, repository) in &state.repositories {
        if repository.owner.is_empty() {
            continue;
        }
        let path = slug(repository, name);
        cards.push(web::card_action(
            &format!("repo-{}-{name}", repository.owner),
            web::style()
                .background("#ffffff")
                .border(LINE)
                .radius(6)
                .padding(16),
            web::visit(format!("/{path}")),
            vec![
                web::styled(
                    &format!("repo-name-{name}"),
                    &path,
                    web::style().size(16).bold().color(ACCENT),
                ),
                muted(&format!("repo-desc-{name}"), &repository.description),
                web::styled_row(
                    &format!("repo-meta-{name}"),
                    8,
                    "center",
                    web::style(),
                    vec![
                        counter(
                            &format!("repo-stars-{name}"),
                            format!("★ {}", repository.stars.len()),
                        ),
                        counter(
                            &format!("repo-issues-{name}"),
                            format!("{} open issues", open_count(&repository.issues)),
                        ),
                    ],
                ),
            ],
        ));
    }
    let mut elements = vec![
        chrome(vec![]),
        web::spacer("home-lead", 16),
        web::styled(
            "home-title",
            "Where the world builds software",
            web::style().size(28).bold().color(INK),
        ),
        muted(
            "home-sub",
            "Repositories, issues and pull requests on this instance.",
        ),
        web::spacer("home-gap", 12),
    ];
    elements.push(web::grid("repos", 2, 16, cards));
    if !state.gists.is_empty() {
        elements.push(web::spacer("gist-gap", 16));
        elements.push(web::link("gists", "Browse gists", "/gists"));
    }
    web::themed_page("GitHub", theme(state), elements)
}

fn owner_page(state: &GitState, owner: &str) -> Result<HttpResponse> {
    let owned: Vec<_> = state
        .repositories
        .iter()
        .filter(|(_, r)| r.owner == owner)
        .collect();
    if owned.is_empty() {
        return web::error(404, "owner not found");
    }
    let mut cards = vec![];
    for (name, repository) in &owned {
        cards.push(web::card_action(
            &format!("owned-{name}"),
            web::style()
                .background("#ffffff")
                .border(LINE)
                .radius(6)
                .padding(16),
            web::visit(format!("/{owner}/{name}")),
            vec![
                web::styled(
                    &format!("owned-name-{name}"),
                    *name,
                    web::style().size(16).bold().color(ACCENT),
                ),
                muted(&format!("owned-desc-{name}"), &repository.description),
            ],
        ));
    }
    web::themed_page(
        &format!("{owner} · GitHub"),
        theme(state),
        vec![
            chrome(vec![]),
            web::spacer("owner-lead", 16),
            web::styled_row(
                "owner-head",
                12,
                "center",
                web::style(),
                vec![
                    avatar("owner-avatar", owner, 64),
                    web::styled("owner-name", owner, web::style().size(24).bold().color(INK)),
                ],
            ),
            web::divider("owner-rule"),
            web::grid("owned", 2, 16, cards),
        ],
    )
}

fn repo_page(
    state: &GitState,
    repository: &Repository,
    name: &str,
    actor: &str,
) -> Result<HttpResponse> {
    let path = slug(repository, name);
    let starred = repository.stars.contains(actor);
    let mut elements = vec![
        chrome(vec![]),
        web::spacer("repo-lead", 12),
        web::styled_row(
            "repo-head",
            8,
            "center",
            web::style(),
            vec![
                avatar("repo-avatar", &repository.owner, 32),
                web::link(
                    "repo-owner",
                    &repository.owner,
                    format!("/{}", repository.owner),
                ),
                web::styled("repo-sep", "/", web::style().size(18).color(MUTED)),
                web::styled(
                    "repo-name",
                    name,
                    web::style().size(18).bold().color(ACCENT),
                ),
                pill("repo-visibility", "Public", MUTED),
            ],
        ),
        web::styled(
            "repo-description",
            &repository.description,
            web::style().size(14).color(INK),
        ),
    ];
    if !repository.topics.is_empty() {
        elements.push(web::styled_row(
            "repo-topics",
            6,
            "center",
            web::style(),
            repository
                .topics
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    web::badge(
                        &format!("topic-{i}"),
                        t,
                        web::style()
                            .background("#ddf4ff")
                            .color(ACCENT)
                            .radius(10)
                            .padding(4)
                            .size(12),
                    )
                })
                .collect(),
        ));
    }
    elements.push(web::styled_row(
        "repo-bar",
        10,
        "center",
        web::style()
            .padding(8)
            .background(SURFACE)
            .border(LINE)
            .radius(6),
        vec![
            PageElement::Button {
                id: "star".into(),
                text: if starred { "★ Unstar" } else { "☆ Star" }.into(),
                action: PageAction {
                    method: "POST".into(),
                    url: format!("/{path}/star"),
                    fields: Default::default(),
                },
            },
            web::link(
                "stargazers",
                format!("{} stars", repository.stars.len()),
                format!("/{path}/stargazers"),
            ),
            counter("forks", format!("{} forks", repository.forks)),
            web::link(
                "issues",
                format!("Issues ({})", open_count(&repository.issues)),
                format!("/{path}/issues"),
            ),
            web::link(
                "pulls",
                format!("Pull requests ({})", open_count(&repository.pull_requests)),
                format!("/{path}/pulls"),
            ),
        ],
    ));
    let files = tree(repository);
    let mut rows = vec![];
    for (i, (file, _)) in files.iter().enumerate() {
        rows.push(web::styled_row(
            &format!("file-row-{i}"),
            8,
            "center",
            web::style().padding(8),
            vec![web::link(
                &format!("file-{i}"),
                file,
                format!("/{path}/blob/{file}"),
            )],
        ));
        rows.push(web::divider(&format!("file-rule-{i}")));
    }
    elements.push(web::card(
        "files",
        web::style().border(LINE).radius(6),
        rows,
    ));
    if let Some((_, readme)) = files
        .iter()
        .find(|(f, _)| f.eq_ignore_ascii_case("README.md"))
    {
        elements.push(web::spacer("readme-gap", 12));
        elements.push(web::card(
            "readme",
            web::style().border(LINE).radius(6).padding(16),
            vec![
                web::styled(
                    "readme-title",
                    "README.md",
                    web::style().size(14).bold().color(INK),
                ),
                web::styled("readme-body", readme, web::style().size(13).color(INK)),
            ],
        ));
    }
    web::themed_page(&format!("{path} · GitHub"), theme(state), elements)
}

fn blob_page(
    state: &GitState,
    repository: &Repository,
    name: &str,
    file: &str,
) -> Result<HttpResponse> {
    let path = slug(repository, name);
    let Some((_, content)) = tree(repository).into_iter().find(|(f, _)| f == file) else {
        return web::error(404, "file not found");
    };
    web::themed_page(
        &format!("{file} · {path}"),
        theme(state),
        vec![
            chrome(vec![]),
            web::spacer("blob-lead", 12),
            web::link("blob-back", &path, format!("/{path}")),
            web::styled("blob-name", file, web::style().size(18).bold().color(INK)),
            web::card(
                "blob",
                web::style()
                    .background(SURFACE)
                    .border(LINE)
                    .radius(6)
                    .padding(16),
                vec![web::styled(
                    "blob-body",
                    content,
                    web::style().size(13).color(INK),
                )],
            ),
        ],
    )
}

fn stargazers_page(state: &GitState, repository: &Repository, name: &str) -> Result<HttpResponse> {
    let path = slug(repository, name);
    let mut rows = vec![];
    for (i, who) in repository.stars.iter().enumerate() {
        rows.push(web::styled_row(
            &format!("stargazer-{i}"),
            8,
            "center",
            web::style().padding(8),
            vec![
                avatar(&format!("stargazer-avatar-{i}"), who, 24),
                web::styled(
                    &format!("stargazer-name-{i}"),
                    who,
                    web::style().size(14).color(INK),
                ),
            ],
        ));
    }
    web::themed_page(
        &format!("Stargazers · {path}"),
        theme(state),
        vec![
            chrome(vec![]),
            web::spacer("stars-lead", 12),
            web::link("stars-back", &path, format!("/{path}")),
            web::styled(
                "stars-title",
                format!("{} people starred {path}", repository.stars.len()),
                web::style().size(20).bold().color(INK),
            ),
            web::card("stargazers", web::style().border(LINE).radius(6), rows),
        ],
    )
}

fn list_page(
    state: &GitState,
    repository: &Repository,
    name: &str,
    pulls: bool,
) -> Result<HttpResponse> {
    let path = slug(repository, name);
    let threads = if pulls {
        &repository.pull_requests
    } else {
        &repository.issues
    };
    let (title, route) = if pulls {
        ("Pull requests", "pull")
    } else {
        ("Issues", "issues")
    };
    let mut rows = vec![];
    for thread in threads.values() {
        let n = thread.number;
        let mut line = vec![
            state_pill(&format!("state-{n}"), thread),
            web::link(
                &format!("thread-{n}"),
                &thread.title,
                format!("/{path}/{route}/{n}"),
            ),
        ];
        line.extend(labels(&format!("thread-{n}"), thread));
        rows.push(web::styled_row(
            &format!("row-{n}"),
            8,
            "center",
            web::style().padding(10),
            line,
        ));
        rows.push(muted(
            &format!("meta-{n}"),
            format!(
                "#{n} opened by {} · {} comments",
                thread.author,
                thread.comments.len()
            ),
        ));
        rows.push(web::divider(&format!("rule-{n}")));
    }
    let mut elements = vec![
        chrome(vec![]),
        web::spacer("list-lead", 12),
        web::link("list-back", &path, format!("/{path}")),
        web::styled_row(
            "list-head",
            8,
            "center",
            web::style(),
            vec![
                web::styled("list-title", title, web::style().size(20).bold().color(INK)),
                counter("list-open", format!("{} open", open_count(threads))),
            ],
        ),
        web::card("threads", web::style().border(LINE).radius(6), rows),
    ];
    elements.push(web::spacer("new-gap", 16));
    if pulls {
        elements.push(web::styled(
            "new-title",
            "Open a pull request",
            web::style().size(16).bold().color(INK),
        ));
        elements.push(web::form(
            "new-pull",
            &format!("/{path}/pulls"),
            &[
                ("title", "Title", ""),
                ("head", "Head ref", "refs/heads/"),
                ("base", "Base ref", "refs/heads/main"),
            ],
        ));
    } else {
        elements.push(web::styled(
            "new-title",
            "Open a new issue",
            web::style().size(16).bold().color(INK),
        ));
        elements.push(web::form(
            "new-issue",
            &format!("/{path}/issues"),
            &[("title", "Title", ""), ("body", "Description", "")],
        ));
    }
    web::themed_page(&format!("{title} · {path}"), theme(state), elements)
}

fn thread_page(
    state: &GitState,
    repository: &Repository,
    name: &str,
    thread: &Thread,
    pulls: bool,
) -> Result<HttpResponse> {
    let path = slug(repository, name);
    let n = thread.number;
    let route = if pulls { "pull" } else { "issues" };
    let base = format!("/{path}/{route}/{n}");
    let mut head = vec![state_pill("state", thread)];
    head.extend(labels("thread", thread));
    let mut elements = vec![
        chrome(vec![]),
        web::spacer("thread-lead", 12),
        web::link("thread-back", &path, format!("/{path}")),
        web::styled(
            "thread-title",
            format!("{} #{n}", thread.title),
            web::style().size(22).bold().color(INK),
        ),
        web::styled_row("thread-head", 8, "center", web::style(), head),
        muted(
            "thread-meta",
            format!(
                "{} opened this {} at tick {}{}",
                thread.author,
                if pulls { "pull request" } else { "issue" },
                thread.tick,
                if thread.assignee.is_empty() {
                    String::new()
                } else {
                    format!(" · assigned to {}", thread.assignee)
                }
            ),
        ),
    ];
    if pulls && !thread.head.is_empty() {
        elements.push(muted(
            "thread-refs",
            format!("{} → {}", thread.head, thread.base),
        ));
    }
    elements.push(web::card(
        "thread-body",
        web::style().border(LINE).radius(6).padding(16),
        vec![
            web::styled_row(
                "thread-author",
                8,
                "center",
                web::style(),
                vec![
                    avatar("thread-avatar", &thread.author, 24),
                    web::styled(
                        "thread-author-name",
                        &thread.author,
                        web::style().size(13).bold().color(INK),
                    ),
                ],
            ),
            web::styled(
                "thread-text",
                &thread.body,
                web::style().size(14).color(INK),
            ),
        ],
    ));
    for (i, comment) in thread.comments.iter().enumerate() {
        elements.push(web::card(
            &format!("comment-{i}"),
            web::style().border(LINE).radius(6).padding(16),
            vec![
                web::styled_row(
                    &format!("comment-head-{i}"),
                    8,
                    "center",
                    web::style(),
                    vec![
                        avatar(&format!("comment-avatar-{i}"), &comment.author, 24),
                        web::styled(
                            &format!("comment-author-{i}"),
                            &comment.author,
                            web::style().size(13).bold().color(INK),
                        ),
                        muted(
                            &format!("comment-tick-{i}"),
                            format!("tick {}", comment.tick),
                        ),
                    ],
                ),
                web::styled(
                    &format!("comment-body-{i}"),
                    &comment.body,
                    web::style().size(14).color(INK),
                ),
            ],
        ));
    }
    for (i, review) in thread.reviews.iter().enumerate() {
        elements.push(web::styled_row(
            &format!("review-{i}"),
            8,
            "center",
            web::style().padding(8),
            vec![
                pill(
                    &format!("review-state-{i}"),
                    &review.decision,
                    if review.decision == "approve" {
                        GREEN
                    } else {
                        MUTED
                    },
                ),
                web::styled(
                    &format!("review-body-{i}"),
                    format!("{}: {}", review.author, review.body),
                    web::style().size(13).color(INK),
                ),
            ],
        ));
    }
    elements.push(web::spacer("comment-gap", 12));
    elements.push(web::form(
        "comment",
        &format!("{base}/comments"),
        &[("body", "Comment", "")],
    ));
    if thread.state == "open" {
        let mut controls = vec![PageElement::Button {
            id: "close".into(),
            text: if pulls {
                "Close pull request"
            } else {
                "Close issue"
            }
            .into(),
            action: PageAction {
                method: "POST".into(),
                url: format!("{base}/state"),
                fields: [("state".to_string(), "closed".to_string())]
                    .into_iter()
                    .collect(),
            },
        }];
        if pulls {
            controls.push(PageElement::Button {
                id: "merge".into(),
                text: "Merge pull request".into(),
                action: PageAction {
                    method: "POST".into(),
                    url: format!("{base}/merge"),
                    fields: Default::default(),
                },
            });
            controls.push(PageElement::Button {
                id: "approve".into(),
                text: "Approve".into(),
                action: PageAction {
                    method: "POST".into(),
                    url: format!("{base}/reviews"),
                    fields: [("decision".to_string(), "approve".to_string())]
                        .into_iter()
                        .collect(),
                },
            });
        }
        elements.push(web::styled_row(
            "controls",
            8,
            "center",
            web::style(),
            controls,
        ));
    } else if thread.state == "closed" {
        elements.push(PageElement::Button {
            id: "reopen".into(),
            text: "Reopen".into(),
            action: PageAction {
                method: "POST".into(),
                url: format!("{base}/state"),
                fields: [("state".to_string(), "open".to_string())]
                    .into_iter()
                    .collect(),
            },
        });
    }
    web::themed_page(
        &format!("{} · {path}#{n}", thread.title),
        theme(state),
        elements,
    )
}

fn gist_index(state: &GitState) -> Result<HttpResponse> {
    let mut rows = vec![];
    for (id, gist) in &state.gists {
        rows.push(web::styled_row(
            &format!("gist-row-{id}"),
            8,
            "center",
            web::style().padding(10),
            vec![
                avatar(&format!("gist-avatar-{id}"), &gist.owner, 24),
                web::link(
                    &format!("gist-{id}"),
                    &gist.description,
                    format!("/gist/{id}"),
                ),
            ],
        ));
        rows.push(web::divider(&format!("gist-rule-{id}")));
    }
    web::themed_page(
        "Discover gists · GitHub",
        theme(state),
        vec![
            chrome(vec![]),
            web::spacer("gists-lead", 12),
            web::styled(
                "gists-title",
                "Gists",
                web::style().size(20).bold().color(INK),
            ),
            web::card("gists", web::style().border(LINE).radius(6), rows),
        ],
    )
}

fn gist_page(state: &GitState, id: &str) -> Result<HttpResponse> {
    let Some(gist) = state.gists.get(id) else {
        return web::error(404, "gist not found");
    };
    let mut elements = vec![
        chrome(vec![]),
        web::spacer("gist-lead", 12),
        web::styled_row(
            "gist-head",
            8,
            "center",
            web::style(),
            vec![
                avatar("gist-avatar", &gist.owner, 32),
                web::styled(
                    "gist-owner",
                    &gist.owner,
                    web::style().size(16).bold().color(INK),
                ),
                muted("gist-id", id),
            ],
        ),
        web::styled(
            "gist-description",
            &gist.description,
            web::style().size(14).color(INK),
        ),
    ];
    for (i, (file, content)) in gist.files.iter().enumerate() {
        elements.push(web::card(
            &format!("gist-file-{i}"),
            web::style().border(LINE).radius(6).padding(16),
            vec![
                web::styled(
                    &format!("gist-file-name-{i}"),
                    file,
                    web::style().size(13).bold().color(INK),
                ),
                web::styled(
                    &format!("gist-file-body-{i}"),
                    content,
                    web::style().size(13).color(INK),
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
    if method == "GET" {
        return match parts.as_slice() {
            [] => home(&s),
            ["gists"] => gist_index(&s),
            ["gist", id] => gist_page(&s, id),
            [owner] => owner_page(&s, owner),
            [owner, name] => match repository(&s, owner, name) {
                Some(r) if api => HttpResponse::json(200, &json!(r)),
                Some(r) => repo_page(&s, r, name, &ctx.actor),
                None => web::error(404, "repository not found"),
            },
            [owner, name, "blob", file @ ..] => match repository(&s, owner, name) {
                Some(r) => blob_page(&s, r, name, &file.join("/")),
                None => web::error(404, "repository not found"),
            },
            [owner, name, "stargazers"] => match repository(&s, owner, name) {
                Some(r) if api => HttpResponse::json(200, &json!(r.stars)),
                Some(r) => stargazers_page(&s, r, name),
                None => web::error(404, "repository not found"),
            },
            [owner, name, kind @ ("issues" | "pulls")] => match repository(&s, owner, name) {
                Some(r) if api => HttpResponse::json(
                    200,
                    &json!(if *kind == "pulls" {
                        &r.pull_requests
                    } else {
                        &r.issues
                    }),
                ),
                Some(r) => list_page(&s, r, name, *kind == "pulls"),
                None => web::error(404, "repository not found"),
            },
            [owner, name, kind @ ("issues" | "pull"), number] => {
                let pulls = *kind == "pull";
                match (repository(&s, owner, name), number.parse::<u64>()) {
                    (Some(r), Ok(n)) => match r.thread(pulls, n) {
                        Some(t) if api => HttpResponse::json(200, &json!(t)),
                        Some(t) => thread_page(&s, r, name, t, pulls),
                        None => web::error(404, "thread not found"),
                    },
                    (Some(_), Err(_)) => web::error(404, "thread not found"),
                    (None, _) => web::error(404, "repository not found"),
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
                ctx.tick,
            )
            .map(|n| format!("{path}/issues/{n}")),
        [_, _, "pulls"] => repo
            .open_pull(
                &actor,
                &web::text(&input, "title"),
                &web::text(&input, "head"),
                &web::text(&input, "base"),
                ctx.tick,
            )
            .map(|n| format!("{path}/pull/{n}")),
        [_, _, kind @ ("issues" | "pull"), number, op] => {
            let pulls = *kind == "pull";
            match number.parse::<u64>() {
                Err(_) => Err((404, "thread not found".into())),
                Ok(n) => {
                    let done = match *op {
                        "comments" => {
                            repo.comment(pulls, n, &actor, &web::text(&input, "body"), ctx.tick)
                        }
                        "state" => repo.set_state(pulls, n, &actor, &web::text(&input, "state")),
                        "reviews" if pulls => repo.review(
                            n,
                            &actor,
                            &web::text(&input, "decision"),
                            &web::text(&input, "body"),
                            ctx.tick,
                        ),
                        "merge" if pulls => repo.merge(n, &actor, ctx.tick),
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
