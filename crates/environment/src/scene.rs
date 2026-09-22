use super::*;

impl Environment {
    pub fn scene(&self, id: &str, width: u32, height: u32) -> Result<Scene> {
        if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 16_777_216 {
            return Err(SimError::invalid("invalid viewport"));
        }
        let s = self.session(id)?;
        if !s
            .config
            .observations
            .iter()
            .any(|c| c == "semantic.v1" || c == "pixels.v1")
        {
            return Err(SimError::denied("visual observation is not permitted")
                .because(cw_protocol::reason::VISUAL_NOT_GRANTED));
        }
        let m = s
            .machines
            .get(&s.focused_machine)
            .ok_or_else(|| SimError::denied("machine unavailable"))?;
        if let Some(theme) = self.desktop_theme(id, &s.focused_machine) {
            let area = work_area(theme, width, height);
            // What a Share control inside a window would hand things to, resolved the
            // same way `shell:share` resolves it.
            let share_to = self
                .runtime
                .computer(&s.focused_machine)
                .ok()
                .and_then(|c| {
                    ["messages", "mail"]
                        .into_iter()
                        .find(|k| c.application_available(k))
                });
            // The platform's own image editor, first one installed, for Photos' Edit.
            let editor = self
                .runtime
                .computer(&s.focused_machine)
                .ok()
                .and_then(|c| {
                    let kinds: &[&'static str] = match theme {
                        DesktopTheme::Windows => &["paint"],
                        DesktopTheme::Macos => &["preview", "pixelmator"],
                        DesktopTheme::Ubuntu => &["gimp", "pinta"],
                        DesktopTheme::Ios | DesktopTheme::Android => &[],
                    };
                    kinds.iter().copied().find(|k| c.application_available(k))
                });
            // The places a file manager's sidebar may offer, read from the machine now.
            let home = m.desktop.home_folder();
            let trash = m.desktop.trash_folder();
            let files_env = cw_applications::FilesEnv {
                home: &home,
                folders: self
                    .runtime
                    .computer(&s.focused_machine)
                    .map(|c| {
                        cw_applications::standard_folders(theme)
                            .iter()
                            .filter(|name| {
                                c.vfs
                                    .stat(&format!("{}/{name}", home.trim_end_matches('/')))
                                    .is_ok_and(|m| m.is_dir)
                            })
                            .map(|name| (*name).to_owned())
                            .collect()
                    })
                    .unwrap_or_default(),
                trash: trash.clone(),
                starred: &m.desktop.starred,
            };
            let mut views = Vec::new();
            for window_id in m.desktop.ordered_windows() {
                let window = &m.desktop.windows[&window_id];
                // A window on another virtual desktop is elsewhere, not minimised: it is
                // not composed, and a frame never sees it.
                if window.workspace != m.desktop.workspace {
                    continue;
                }
                // Phones present every application edge to edge; retained desktop
                // frames only apply to freely positioned windows.
                let rect = if theme.mobile() {
                    area
                } else {
                    m.desktop.effective_frame(window_id, area)
                };
                let kind = match &window.state {
                    AppState::Browser { .. } => "browser",
                    AppState::Files { .. } => "files",
                    AppState::Editor { .. } => "editor",
                    AppState::Terminal { .. } => "terminal",
                    AppState::Native(app) => app.kind(),
                };
                let content_rect = window_content_rect_for_kind(theme, rect, kind);
                let content = if kind == "browser" {
                    let browser = if m.active_browser_window == Some(window_id) {
                        &m.browser
                    } else {
                        m.browser_windows.get(&window_id).unwrap_or(&m.browser)
                    };
                    browser.scene(content_rect.width.max(1), content_rect.height.max(1))
                } else {
                    cw_applications::desktop_scene::app_content_scrolled(
                        &window.state,
                        &cw_applications::AppEnv {
                            theme,
                            width: content_rect.width.max(1),
                            height: content_rect.height.max(1),
                            clock_us: m.desktop.clock_us,
                            settings: &m.desktop.settings,
                            clipboard: m.desktop.clipboard.as_ref(),
                            share_to,
                            files: files_env.clone(),
                            editor,
                            // Only the window in front sees the pointer, and only over
                            // its own content.
                            pointer: m
                                .pointer_position
                                .filter(|_| m.desktop.focused == Some(window_id))
                                .filter(|(x, y)| content_rect.contains(*x, *y))
                                .map(|(x, y)| (x - content_rect.x, y - content_rect.y)),
                        },
                        &window.scroll,
                    )
                };
                let (document, caption, modified) = match &window.state {
                    AppState::Browser { address } => {
                        let browser = if m.active_browser_window == Some(window_id) {
                            &m.browser
                        } else {
                            m.browser_windows.get(&window_id).unwrap_or(&m.browser)
                        };
                        (
                            browser.url().unwrap_or(address).to_owned(),
                            browser.title().unwrap_or_default(),
                            false,
                        )
                    }
                    // A list, a view or the Trash is named, not traced as a path.
                    AppState::Files { .. } => {
                        let caption = window
                            .state
                            .file_tab()
                            .filter(|tab| tab.is_place(&trash))
                            .map(|tab| tab.title(theme, &home, &trash))
                            .unwrap_or_default();
                        (window.state.file_path().to_owned(), caption, false)
                    }
                    AppState::Editor { path, dirty, .. } => (path.clone(), String::new(), *dirty),
                    // The prompt names the user, the host and the directory; a frame titles
                    // the window from it the way each platform's terminal does.
                    AppState::Terminal { prompt, .. } => (String::new(), prompt.clone(), false),
                    AppState::Native(app) => (app.document(), app.caption(), app.modified()),
                };
                let title = match &window.state {
                    AppState::Browser { address } => format!("{} — {}", window.title, address),
                    AppState::Native(app) => app.title(theme),
                    _ => window.title.clone(),
                };
                // Tab strips are drawn by the window frame, so the labels travel with the view.
                let (tabs, active_tab) = match &window.state {
                    AppState::Browser { .. } => {
                        let browser = if m.active_browser_window == Some(window_id) {
                            &m.browser
                        } else {
                            m.browser_windows.get(&window_id).unwrap_or(&m.browser)
                        };
                        (
                            browser
                                .tabs
                                .iter()
                                .map(|tab| match tab.history.get(tab.position) {
                                    Some(entry) if !entry.title().is_empty() => {
                                        entry.title().to_owned()
                                    }
                                    _ => "New tab".into(),
                                })
                                .collect(),
                            browser.active,
                        )
                    }
                    AppState::Files { tabs, active } => (
                        tabs.iter()
                            .map(|tab| tab.title(theme, &home, &trash))
                            .collect(),
                        *active,
                    ),
                    _ => (vec![], 0),
                };
                let (can_go_back, can_go_forward) = match &window.state {
                    AppState::Browser { .. } => {
                        let browser = if m.active_browser_window == Some(window_id) {
                            &m.browser
                        } else {
                            m.browser_windows.get(&window_id).unwrap_or(&m.browser)
                        };
                        let tab = browser.tab();
                        (tab.position > 0, tab.position + 1 < tab.history.len())
                    }
                    AppState::Files { .. } => window
                        .state
                        .file_tab()
                        .map(|tab| (tab.can_go_back(), tab.can_go_forward()))
                        .unwrap_or((false, false)),
                    _ => (false, false),
                };
                let (dark_chrome, chrome) = match &window.state {
                    AppState::Native(app) => {
                        let mut chrome = app.chrome();
                        // A phone's navigation bar carries the application's way to its
                        // parent screen, or Gmail's drawer button.
                        if let Some((kind, target, label)) =
                            app.phone_nav(theme).filter(|_| theme.mobile())
                        {
                            chrome.push(("nav".into(), format!("{kind}\t{target}\t{label}")));
                        }
                        (app.dark_chrome(), chrome)
                    }
                    // Whether the selected file is starred, for a context menu that
                    // offers Star or Unstar on it, and whether this tab is showing the
                    // Trash, so a menu can offer Restore there and Move to Trash
                    // everywhere else rather than painting both and refusing one.
                    AppState::Files { .. } => {
                        let tab = window.state.file_tab();
                        let mut facts = vec![(
                            "trash".to_owned(),
                            if tab.is_some_and(|t| t.in_trash(&m.desktop.trash_folder())) {
                                "1"
                            } else {
                                "0"
                            }
                            .to_owned(),
                        )];
                        if let Some(path) = tab.and_then(|t| t.selected_path()) {
                            facts.push((
                                "starred".to_owned(),
                                if m.desktop.is_starred(&path) {
                                    "1"
                                } else {
                                    "0"
                                }
                                .to_owned(),
                            ));
                        }
                        (false, facts)
                    }
                    _ => (false, vec![]),
                };
                views.push(WindowView {
                    dark_chrome,
                    chrome,
                    id: window_id,
                    title,
                    kind: kind.into(),
                    rect,
                    focused: m.desktop.focused == Some(window_id),
                    maximized: window.maximized,
                    minimized: window.minimized,
                    content: Some(content),
                    document,
                    caption,
                    home: home.clone(),
                    modified,
                    // A browser's address field, or a file manager's search field.
                    editing: (kind == "browser"
                        && m.address_focused
                        && m.desktop.focused == Some(window_id))
                        || window
                            .state
                            .file_tab()
                            .is_some_and(|t| t.searching && t.rename.is_none()),
                    tabs,
                    active_tab,
                    can_go_back,
                    can_go_forward,
                    workspace: window.workspace,
                    view_grid: window
                        .state
                        .file_tab()
                        .is_some_and(|t| t.view == cw_applications::FileView::Grid),
                    sort_key: window
                        .state
                        .file_tab()
                        .map(|t| format!("{:?}", t.sort).to_lowercase())
                        .unwrap_or_default(),
                    query: window
                        .state
                        .file_tab()
                        .map(|t| t.query.clone())
                        .unwrap_or_default(),
                    selection: window
                        .state
                        .file_tab()
                        .and_then(|t| t.selected_path())
                        .unwrap_or_default(),
                    zoom: match &window.state {
                        AppState::Browser { .. } if m.active_browser_window == Some(window_id) => {
                            m.browser.zoom()
                        }
                        AppState::Browser { .. } => m
                            .browser_windows
                            .get(&window_id)
                            .unwrap_or(&m.browser)
                            .zoom(),
                        _ => 100,
                    },
                });
            }
            if m.active_app.is_some() {
                let page = self.project_page(&s.config.actor, &s.focused_machine, m)?;
                let rect = area;
                let inner = window_content_rect_for_kind(theme, rect, "custom");
                views.push(WindowView {
                    id: u64::MAX,
                    title: page.title.clone(),
                    kind: "custom".into(),
                    rect,
                    focused: true,
                    maximized: true,
                    minimized: false,
                    content: Some(cw_browser::layout_page(
                        &page,
                        &BTreeMap::new(),
                        inner.width.max(1),
                        inner.height.max(1),
                        0,
                    )),
                    ..Default::default()
                });
            }
            let published = scene_windows(theme, &views);
            let mut scene = render_desktop_with_options(
                theme,
                width,
                height,
                self.runtime.tick(),
                m.desktop.launcher_open,
                views,
                ShellOptions {
                    installed_apps: self
                        .desktop_catalog(id, &s.focused_machine)
                        .into_iter()
                        .map(|app| app.id)
                        .collect(),
                    panel: m.desktop.panel.clone(),
                    search: m.desktop.search.clone(),
                    hover: m.pointer_position,
                    cursor: m.pointer_cursor.as_deref().map(CursorKind::from_css),
                    desktop_selection: m.desktop.desktop_selection.clone(),
                    settings: m.desktop.settings,
                    screen: m.desktop.screen,
                    panel_month: m.desktop.panel_month,
                    text_entry: text_entry_of(m, theme.mobile()),
                    keyboard: m.desktop.keyboard,
                    bookmarks: m.desktop.bookmarks.clone(),
                    downloads: m.desktop.downloads.clone(),
                    notifications: m.desktop.notifications.clone(),
                    workspaces: m.desktop.workspace_count(),
                    workspace: m.desktop.workspace,
                    library_group: m.desktop.library_group.clone(),
                    bookmarked: m.browser.url().is_some_and(|url| m.desktop.bookmarked(url)),
                    panel_over_launcher: m.desktop.panel_over_launcher,
                    typed: typed_of(m, theme.mobile()),
                    home_page: m.desktop.home_page,
                    user: self
                        .runtime
                        .computer(&s.focused_machine)
                        .map(|c| c.user.clone())
                        .unwrap_or_default(),
                    recents: m.desktop.recents.clone(),
                    home: m.desktop.home_folder(),
                    battery: self.has_battery(&s.focused_machine, theme),
                    anchor: m.desktop.panel_at.filter(|_| m.desktop.panel.is_some()),
                    overview: m.desktop.overview,
                    capture: m
                        .desktop
                        .pointer_capture
                        .as_ref()
                        .map(|capture| capture.operation.clone()),
                },
            );
            self.decorate(&mut scene, m, published, theme.mobile());
            return Ok(scene);
        }
        let mut scene = if m.browser_visible {
            m.browser.scene(width, height)
        } else {
            cw_browser::layout_page(
                &self.project_page(&s.config.actor, &s.focused_machine, m)?,
                &BTreeMap::new(),
                width,
                height,
                0,
            )
        };
        // No compositor here, but revisions and the focused field still apply.
        scene.focus = Some(Focus {
            interaction: m.focused_input.clone(),
            keyboard: Keyboard {
                route: if m.focused_input.is_some() {
                    "page"
                } else {
                    "none"
                }
                .into(),
                target: m.focused_input.clone(),
                text_entry: m.focused_input.is_some(),
                window: None,
            },
            ..Focus::default()
        });
        if let Some((node, caret)) = m
            .focused_input
            .as_deref()
            .and_then(|f| Some((f, m.browser.tab().fields.get(f)?.clone())))
            .and_then(|(f, v)| tail_caret(&scene, f, &v))
        {
            let focus = scene.focus.as_mut().expect("focus was just set");
            focus.node = Some(node);
            focus.caret = Some(caret);
        }
        scene.stamp();
        Ok(scene)
    }
}
pub(super) fn active_page(m: &MachineSession) -> Page {
    if let Some(page) = &m.custom_page {
        return page.clone();
    }
    if m.browser_visible {
        let mut page = m
            .browser
            .current_page()
            .map(std::borrow::Cow::into_owned)
            .unwrap_or_else(|| Page::new("Browser"));
        fn fields(elements: &mut [PageElement], values: &BTreeMap<String, String>) {
            for element in elements {
                match element {
                    PageElement::Input { id, value, .. } => {
                        if let Some(current) = values.get(id) {
                            *value = current.clone()
                        }
                    }
                    PageElement::Group { children, .. } | PageElement::Form { children, .. } => {
                        fields(children, values)
                    }
                    _ => {}
                }
            }
        }
        fields(&mut page.elements, &m.browser.tab().fields);
        return page;
    }
    if !m.desktop.windows.is_empty() {
        return m.desktop.page();
    }
    let mut page = Page::new("Terminal");
    if !m.terminal.is_null() {
        page.elements.push(PageElement::Text {
            id: "terminal-output".into(),
            text: format!(
                "{}{}",
                m.terminal
                    .get("stdout")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
                m.terminal
                    .get("stderr")
                    .and_then(Value::as_str)
                    .unwrap_or("")
            ),
        });
    }
    page
}
// ---- Perception contract: window identity, focus, occlusion, line provenance. ----
// Everything below derives from session state, so the same state yields the same
// scene and the same revision ids after a restore, a fork or a process restart.
use cw_scene::{Caret, Focus, Keyboard, Node, NodeState, Rect, SceneWindow, TextBuffer};

/// Publish what the compositor already knows, so spatial reasoning does not have to be
/// reconstructed from painted text. `views` is bottom-to-top.
fn scene_windows(theme: DesktopTheme, views: &[WindowView]) -> Vec<SceneWindow> {
    let mut windows: Vec<SceneWindow> = views
        .iter()
        .enumerate()
        .map(|(z, v)| SceneWindow {
            id: v.id,
            // The title the frame paints, not the raw application id the compositor
            // stores: what an agent reads here is what it can see on the screen.
            title: theme.window_title(v),
            app: v.kind.clone(),
            bounds: v.rect,
            content: window_content_rect_for_kind(theme, v.rect, &v.kind),
            z: z as u32,
            focused: v.focused,
            minimized: v.minimized,
            maximized: v.maximized,
            document: v.document.clone(),
            tabs: v.tabs.clone(),
            active_tab: v.active_tab,
            occluded_by: Vec::new(),
            exposed: None,
        })
        .collect();
    for i in 0..windows.len() {
        if windows[i].minimized {
            continue;
        }
        let bounds = windows[i].bounds;
        let above: Vec<(u64, Rect)> = windows[i + 1..]
            .iter()
            .filter(|w| !w.minimized && w.bounds.intersection(bounds).is_some())
            .map(|w| (w.id, w.bounds))
            .collect();
        windows[i].occluded_by = above.iter().map(|(id, _)| *id).collect();
        let covers: Vec<Rect> = above.iter().map(|(_, r)| *r).collect();
        windows[i].exposed = cw_scene::exposed(bounds, &covers);
    }
    windows
}

/// Attribute every node to a window. A `window:<id>:` interaction is authoritative; the
/// rest follow the compositor's per-window run, which opens with that window's focus
/// region. Shell chrome paints above every window, so the topmost window node bounds it.
fn attribute_windows(scene: &mut Scene) {
    let Some(top) = scene
        .nodes
        .iter()
        .filter_map(|n| {
            cw_scene::window_of(n.interaction.as_deref()?)?;
            Some(n.z)
        })
        .max()
    else {
        return;
    };
    let mut current = None;
    for n in &mut scene.nodes {
        if let Some(action) = n.interaction.as_deref() {
            current = cw_scene::window_of(action);
        }
        n.window = current.filter(|_| n.z <= top);
    }
}

/// Indices of the text nodes one window's content paints, in reading order.
fn painted_lines(scene: &Scene, window: u64, content: Rect) -> Vec<usize> {
    let mut rows: Vec<usize> = scene
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| {
            n.window == Some(window)
                && n.painted_text().is_some()
                && content.intersection(n.painted_bounds()).is_some()
        })
        .map(|(i, _)| i)
        .collect();
    rows.sort_by_key(|i| {
        let b = scene.nodes[*i].painted_bounds();
        (b.y, b.x, *i)
    });
    rows
}

/// Attach the pane's painted lines to its buffer, marking hard-wrapped continuations.
/// Without this a consumer that joins painted lines reads `initial commi` + `t`.
fn attach_buffer(scene: &mut Scene, buffer: &mut TextBuffer, content: Rect) {
    let Some(window) = buffer.window else { return };
    // One node per visual row: the column the most rows share. Line-number gutters and
    // shell prompts sit in other columns.
    let mut columns: BTreeMap<i32, Vec<usize>> = BTreeMap::new();
    for i in painted_lines(scene, window, content) {
        columns
            .entry(scene.nodes[i].painted_bounds().x)
            .or_default()
            .push(i);
    }
    let Some((_, mut column)) = columns
        .into_iter()
        .max_by_key(|(x, rows)| (rows.len(), std::cmp::Reverse(*x)))
    else {
        return;
    };
    let logical: Vec<&str> = buffer.lines.iter().map(String::as_str).collect();
    let mut lines = Vec::new();
    // Trailing rows a pane paints below its buffer (a shell prompt) are not in it.
    for _ in 0..4 {
        if column.is_empty() {
            return;
        }
        let visual: Vec<&str> = column
            .iter()
            .map(|i| scene.nodes[*i].painted_text().unwrap_or(""))
            .collect();
        lines = cw_scene::reflow(&logical, &visual);
        if !lines.is_empty() {
            break;
        }
        column.pop();
    }
    if lines.is_empty() {
        return;
    }
    buffer.first_visible = lines.first().map(|l| l.logical).unwrap_or(0);
    buffer.visible = lines.last().map(|l| l.logical + 1).unwrap_or(0) - buffer.first_visible;
    let mut head: BTreeMap<u32, u64> = BTreeMap::new();
    for (index, mut line) in lines.into_iter().enumerate() {
        line.pane = Some(buffer.handle.clone());
        line.wrapped_from = head.get(&line.logical).copied();
        let node = &mut scene.nodes[column[index]];
        head.entry(line.logical).or_insert(node.id);
        node.line = Some(line);
    }
}

/// Caret cell at the end of the text `anchor` paints, on that node's own grid.
fn caret_after(anchor: &Node, offset: u32) -> Caret {
    let (cw, ch) = anchor.cell().unwrap_or_else(|| cw_scene::text_cell(13));
    let b = anchor.painted_bounds();
    // Cells on the node's grid: a wide character (CJK, emoji) takes two.
    let painted = cw_scene::text::terminal::columns(anchor.painted_text().unwrap_or("")) as u32;
    Caret {
        bounds: Rect::new(
            b.x.saturating_add((painted * cw) as i32),
            b.y,
            cw,
            b.height.max(ch),
        ),
        line: 0,
        column: offset,
        offset,
    }
}

/// Caret at the end of the value a single-line control shows. Address bars and page
/// fields always append, so the world knows the insertion point is the end.
fn tail_caret(scene: &Scene, action: &str, value: &str) -> Option<(u64, Caret)> {
    let field = scene
        .nodes
        .iter()
        .find(|n| n.interaction.as_deref() == Some(action))?
        .painted_bounds();
    let offset = value.chars().count() as u32;
    let text = scene
        .nodes
        .iter()
        .filter(|n| {
            let b = n.painted_bounds();
            n.painted_text().is_some()
                && b.area() > 0
                && field.intersection(b).map(|i| i.area()) == Some(b.area())
        })
        .max_by_key(|n| n.painted_bounds().x);
    match text {
        Some(n) => Some((n.id, caret_after(n, offset))),
        // An empty field paints no text; place the caret at its text inset.
        None => {
            let (cw, _) = cw_scene::text_cell(13);
            Some((
                0,
                Caret {
                    bounds: Rect::new(
                        field.x.saturating_add(8),
                        field.y.saturating_add(4),
                        cw,
                        field.height.saturating_sub(8).max(1),
                    ),
                    line: 0,
                    column: offset,
                    offset,
                },
            ))
        }
    }
}

/// Terminal caret: `DesktopState` keeps the shell caret at the end of the input, which
/// the prompt row paints last.
fn terminal_caret(scene: &Scene, window: u64, content: Rect, input: &str) -> Option<(u64, Caret)> {
    let rows = painted_lines(scene, window, content);
    let bottom = rows
        .iter()
        .map(|i| scene.nodes[*i].painted_bounds().y)
        .max()?;
    let last = *rows
        .iter()
        .filter(|i| scene.nodes[**i].painted_bounds().y == bottom)
        .max_by_key(|i| scene.nodes[**i].painted_bounds().x)?;
    let n = &scene.nodes[last];
    Some((n.id, caret_after(n, input.chars().count() as u32)))
}

/// Editor caret: the model owns the offset, the scene owns the grid. The text region's
/// bounds start at the first painted line, and its action carries the scroll position.
fn editor_caret(
    scene: &Scene,
    window: u64,
    content: Rect,
    text: &str,
    cursor: usize,
) -> Option<(u64, Caret)> {
    let mut end = cursor.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let before = &text[..end];
    let row = before.bytes().filter(|b| *b == b'\n').count() as u32;
    let column = before.rsplit('\n').next().unwrap_or("").chars().count() as u32;
    let region = scene.nodes.iter().find(|n| {
        n.window == Some(window)
            && n.interaction
                .as_deref()
                .is_some_and(|a| a.contains(":content:editor-text:"))
    })?;
    // `…editor-text:<first row>[:<columns>]`, exactly as the view painted it.
    let mut grid = region
        .interaction
        .as_deref()?
        .split_once("editor-text:")?
        .1
        .split(':');
    let first: u32 = grid.next()?.parse().ok()?;
    let columns: usize = grid.next().and_then(|c| c.parse().ok()).unwrap_or(0);
    // Soft-wrapped rows put the caret on its visual row; `line`/`column` stay logical.
    let (visual_row, visual_column) = cw_applications::editor_caret_cell(text, end, columns);
    // Only the document's own rows: a toolbar or status bar in the same window paints
    // text too, and its spacing is not the document's line pitch.
    let text_area = region.painted_bounds();
    let rows: Vec<usize> = painted_lines(scene, window, content)
        .into_iter()
        .filter(|i| {
            let b = scene.nodes[*i].painted_bounds();
            b.x == text_area.x && b.y >= text_area.y
        })
        .collect();
    let (cw, ch) = rows
        .iter()
        .find_map(|i| scene.nodes[*i].cell())
        .unwrap_or_else(|| cw_scene::text_cell(13));
    // Row pitch is whatever the pane actually painted, not a constant copied from it.
    let mut ys: Vec<i32> = rows
        .iter()
        .map(|i| scene.nodes[*i].painted_bounds().y)
        .collect();
    ys.dedup();
    let pitch = ys
        .windows(2)
        .map(|w| (w[1] - w[0]).unsigned_abs())
        .find(|p| *p > 0)
        .unwrap_or(ch);
    let origin = region.painted_bounds();
    Some((
        region.id,
        Caret {
            bounds: Rect::new(
                origin.x.saturating_add((visual_column as u32 * cw) as i32),
                origin
                    .y
                    .saturating_add(((visual_row as u32).saturating_sub(first) * pitch) as i32),
                cw,
                pitch,
            ),
            line: row,
            column,
            offset: before.chars().count() as u32,
        },
    ))
}

/// The terminal's logical scrollback, in the order the pane paints it: echoed
/// command, streams, and the exit marker a failure adds.
fn transcript_lines(transcript: &[cw_applications::TerminalEntry]) -> Vec<String> {
    let mut lines = Vec::new();
    for entry in transcript {
        lines.push(entry.echo());
        lines.extend(entry.stdout.lines().map(str::to_owned));
        lines.extend(entry.stderr.lines().map(str::to_owned));
        if entry.failed() {
            lines.push(entry.status());
        }
    }
    lines
}

fn control<'a>(scene: &'a Scene, action: &str) -> Option<&'a Node> {
    scene
        .nodes
        .iter()
        .find(|n| n.interaction.as_deref() == Some(action))
}

/// Whether the next keystroke inserts text rather than invoking a command. The shells
/// need this *before* the scene exists, to decide whether to paint a soft keyboard, so
/// it cannot be read back off the composed scene; `focus_and_text_entry_agree` asserts
/// this and `focus_of` never disagree.
fn text_entry_of(m: &MachineSession, phone: bool) -> bool {
    if m.desktop.launcher_open || m.desktop.panel.as_deref() == Some("search") {
        return true;
    }
    if phone && phone_overlay(m) {
        return false;
    }
    if m.active_app.is_some() {
        return m.focused_input.is_some();
    }
    let Some(id) = m.desktop.focused else {
        return false;
    };
    if m.address_focused {
        return true;
    }
    if m.browser_visible {
        return m.focused_input.is_some();
    }
    match m.desktop.windows.get(&id).map(|w| &w.state) {
        Some(AppState::Terminal { .. } | AppState::Editor { .. } | AppState::Browser { .. }) => {
            true
        }
        // A native application takes text while it has a field focused; a music player
        // with none takes no text, so a phone paints no keyboard over it.
        Some(AppState::Native(app)) => app.takes_text(phone),
        // A file manager takes text only while it is searching or renaming, which is
        // exactly the condition `DesktopState::text` checks.
        Some(state @ AppState::Files { .. }) => {
            state.file_tab().is_some_and(|tab| tab.editing_text())
        }
        _ => false,
    }
}
/// The end of what the field taking keystrokes holds before its caret, following the
/// same order as `text_entry_of`. Empty where the text is an application's own.
fn typed_of(m: &MachineSession, phone: bool) -> String {
    let text = if !text_entry_of(m, phone) {
        String::new()
    } else if m.desktop.launcher_open || m.desktop.panel.as_deref() == Some("search") {
        m.desktop.search.clone()
    } else if m.active_app.is_some() {
        String::new()
    } else if m.address_focused || !m.browser_visible {
        let state = m
            .desktop
            .focused
            .and_then(|id| m.desktop.windows.get(&id))
            .map(|w| &w.state);
        match state {
            Some(AppState::Browser { address }) => address.clone(),
            Some(AppState::Terminal { input, .. }) => input.clone(),
            Some(AppState::Editor { text, cursor, .. }) => {
                let mut end = (*cursor).min(text.len());
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                text[..end].to_owned()
            }
            Some(files @ AppState::Files { .. }) => files
                .file_tab()
                .map(|tab| match &tab.rename {
                    Some(rename) => rename.name.clone(),
                    None => tab.query.clone(),
                })
                .unwrap_or_default(),
            _ => String::new(),
        }
    } else {
        m.focused_input
            .as_deref()
            .and_then(|f| m.browser.tab().fields.get(f).cloned())
            .unwrap_or_default()
    };
    let skip = text.chars().count().saturating_sub(64);
    text.chars().skip(skip).collect()
}
/// How far a finger may drift before a touch is a drag rather than a tap: past it, a
/// press in an application's content scrolls it instead of pressing what it started on.
pub(super) const SWIPE_SLOP: i32 = 12;
/// A finger's travel handed to an application's own wheel use at a time while it
/// drags on a phone: about a row, so a grid or a scrollback moves as the finger does.
pub(super) const TOUCH_STEP: i32 = 16;
/// One display frame at 60 Hz, in µs: the interval assumed between two finger samples
/// when the world clock did not move between them, as touch samples arrive per frame.
pub(super) const FRAME_US: u64 = 16_667;
/// A finger that rested this long (µs of world clock) before lifting has stopped, and
/// the list does not fling (Android's VelocityTracker assumes the same 40 ms).
pub(super) const FINGER_STOPPED_US: u64 = 40_000;
/// How far (in rubber-banded pixels) a list must be pulled down past its top for the
/// release to refresh it.
pub(super) const PULL_TO_REFRESH: i32 = 56;
/// A phone's system surface over the screen — Control Center, the shade, the App
/// Switcher, Settings, a sheet — other than Search. It is modal on the device: it has
/// no text field, the soft keyboard goes down under it, and keystrokes reach nothing
/// behind it until it is put away.
pub(crate) fn phone_overlay(m: &MachineSession) -> bool {
    m.desktop
        .panel
        .as_deref()
        .is_some_and(|panel| panel != "search")
}
/// Where the next keystroke is delivered. Mirrors the `keyboard.v1` dispatch order
/// exactly, so the published answer is what typing would actually do.
fn focus_of(m: &MachineSession, scene: &Scene, windows: &[SceneWindow], phone: bool) -> Focus {
    let window = m
        .desktop
        .focused
        .filter(|id| windows.iter().any(|w| w.id == *id && !w.minimized));
    let mut focus = Focus {
        window,
        ..Focus::default()
    };
    let bind = |focus: &mut Focus, route: &str, action: Option<String>, text_entry: bool| {
        focus.keyboard = Keyboard {
            route: route.into(),
            window: focus.window,
            target: action.clone(),
            text_entry,
        };
        if let Some(node) = action.as_deref().and_then(|a| control(scene, a)) {
            focus.node = Some(node.id);
            if let Some(s) = &node.semantic {
                focus.role = s.role.clone();
                focus.label = s.label.clone();
            }
        }
        focus.interaction = action;
    };
    if m.desktop.launcher_open || m.desktop.panel.as_deref() == Some("search") {
        focus.window = None;
        bind(&mut focus, "panel", None, true);
        focus.role = "searchbox".into();
        focus.label = "Search".into();
        focus.value = Some(m.desktop.search.clone());
        return focus;
    }
    if phone && phone_overlay(m) {
        bind(&mut focus, "panel", None, false);
        return focus;
    }
    if m.active_app.is_some() {
        // A registered application draws into a full-screen window of its own.
        focus.window = Some(u64::MAX);
        let target = m.focused_input.clone();
        let text = target.is_some();
        bind(&mut focus, "application", target, text);
        return focus;
    }
    let Some(id) = window else {
        bind(&mut focus, "none", None, false);
        return focus;
    };
    let state = m.desktop.windows.get(&id).map(|w| &w.state);
    let content = windows
        .iter()
        .find(|w| w.id == id)
        .map(|w| w.content)
        .unwrap_or_default();
    if m.address_focused {
        let address = match state {
            Some(AppState::Browser { address }) => address.clone(),
            _ => String::new(),
        };
        let action = format!("window:{id}:content:shell:address");
        let caret = tail_caret(scene, &action, &address);
        bind(&mut focus, "address", Some(action), true);
        focus.value = Some(address);
        focus.caret = caret.map(|(_, c)| c);
        return focus;
    }
    if m.browser_visible {
        let field = m.focused_input.clone();
        let value = field
            .as_deref()
            .and_then(|f| m.browser.tab().fields.get(f).cloned());
        let action = field.map(|f| format!("window:{id}:content:{f}"));
        let caret = action
            .as_deref()
            .zip(value.as_deref())
            .and_then(|(a, v)| tail_caret(scene, a, v));
        let text = action.is_some();
        bind(&mut focus, "page", action, text);
        focus.value = value;
        focus.caret = caret.map(|(_, c)| c);
        return focus;
    }
    match state {
        Some(AppState::Terminal { input, .. }) => {
            let caret = terminal_caret(scene, id, content, input);
            bind(
                &mut focus,
                "terminal",
                Some(format!("window:{id}:content:terminal-input")),
                true,
            );
            focus.value = Some(input.clone());
            if let Some((node, c)) = caret {
                focus.node = Some(node);
                focus.caret = Some(c);
            }
        }
        Some(AppState::Editor { text, cursor, .. }) => {
            let caret = editor_caret(scene, id, content, text, *cursor);
            let action = scene
                .nodes
                .iter()
                .find(|n| {
                    n.window == Some(id)
                        && n.interaction
                            .as_deref()
                            .is_some_and(|a| a.contains(":content:editor-text:"))
                })
                .and_then(|n| n.interaction.clone());
            bind(&mut focus, "editor", action, true);
            // The document is in `Scene::buffers`; repeating it here would double the
            // cost of every scene for a large file.
            if let Some((node, c)) = caret {
                focus.node = Some(node);
                focus.caret = Some(c);
            }
        }
        Some(AppState::Browser { address }) => {
            let action = format!("window:{id}:content:shell:address");
            bind(&mut focus, "address", Some(action), true);
            focus.value = Some(address.clone());
        }
        // The application's focused text field, when it has one, is the target: the
        // same answer that decides whether a phone paints its keyboard.
        Some(AppState::Native(app)) => match app.text_field(phone) {
            Some(field) => {
                bind(
                    &mut focus,
                    "application",
                    Some(format!("window:{id}:content:{field}")),
                    true,
                );
                // Whatever the control is painted as, what has the focus is a field.
                focus.role = "textbox".into();
            }
            None => bind(
                &mut focus,
                "application",
                Some(format!("window:{id}:focus")),
                false,
            ),
        },
        // A file manager takes text only while it is searching or renaming.
        Some(files @ AppState::Files { .. }) if text_entry_of(m, phone) => {
            let tab = files.file_tab();
            let (control, value) = match tab {
                Some(tab) if tab.rename.is_some() => {
                    ("files-rename", tab.rename.as_ref().map(|r| r.name.clone()))
                }
                Some(tab) => ("files-search", Some(tab.query.clone())),
                None => ("files-search", None),
            };
            let action = format!("window:{id}:content:{control}");
            let caret = value.as_deref().and_then(|v| tail_caret(scene, &action, v));
            bind(&mut focus, "files", Some(action), true);
            focus.value = value;
            focus.caret = caret.map(|(_, c)| c);
        }
        _ => bind(
            &mut focus,
            "window",
            Some(format!("window:{id}:focus")),
            false,
        ),
    }
    focus
}

/// Publish the state a shell otherwise encodes only in its painting: switch positions,
/// open panels, the selected tab and window, and which control has focus.
fn annotate_states(scene: &mut Scene, m: &MachineSession, windows: &[SceneWindow], focus: &Focus) {
    let focused = focus.interaction.clone();
    // The page a paged home screen really shows: the one asked for, within the pages
    // the shell painted a dot for.
    let last_page = scene
        .nodes
        .iter()
        .filter_map(|n| n.interaction.as_deref()?.strip_prefix("shell:home-page:"))
        .filter_map(|page| page.parse::<u32>().ok())
        .max();
    let shown_page = last_page.map(|last| m.desktop.home_page.min(last));
    for n in &mut scene.nodes {
        let Some(action) = n.interaction.clone() else {
            continue;
        };
        // `window:<id>:<local>`; unnamespaced shell controls are their own local name.
        let local = match cw_scene::window_of(&action) {
            Some(_) => action.splitn(3, ':').nth(2).unwrap_or_default(),
            None => action.as_str(),
        };
        let inner = local.strip_prefix("content:").unwrap_or(local);
        let tab = |prefix: &str| -> Option<bool> {
            let index: usize = inner.strip_prefix(prefix)?.parse().ok()?;
            Some(windows.iter().find(|w| Some(w.id) == n.window)?.active_tab == index)
        };
        let mut state = NodeState {
            focused: focused.as_deref() == Some(action.as_str()),
            checked: inner
                .strip_prefix("shell:toggle:")
                .and_then(|name| m.desktop.settings.flag(name).ok()),
            expanded: match inner {
                "shell:launcher" => Some(m.desktop.launcher_open),
                _ => inner
                    .strip_prefix("shell:panel:")
                    .map(|name| m.desktop.panel.as_deref() == Some(name)),
            },
            selected: tab("shell:tab:select:")
                .or_else(|| tab("files-tab:"))
                .or_else(|| {
                    let page: u32 = inner.strip_prefix("shell:home-page:")?.parse().ok()?;
                    Some(shown_page == Some(page))
                })
                .or_else(|| {
                    (local == "focus").then(|| n.window.is_some() && n.window == m.desktop.focused)
                })
                .or_else(|| {
                    let icon = inner
                        .strip_prefix("shell:open:")
                        .or_else(|| inner.strip_prefix("shell:launch:"))?;
                    Some(m.desktop.desktop_selection.as_deref() == Some(icon))
                }),
        };
        if state.focused && n.semantic.is_none() {
            state.focused = false;
        }
        n.state = (!state.is_empty()).then_some(state);
    }
}

impl Environment {
    /// Items 1-6 of the perception contract, applied to a composed desktop scene.
    fn decorate(
        &self,
        scene: &mut Scene,
        m: &MachineSession,
        windows: Vec<SceneWindow>,
        phone: bool,
    ) {
        attribute_windows(scene);
        let mut buffers = Vec::new();
        for w in windows.iter().filter(|w| !w.minimized) {
            let Some(window) = m.desktop.windows.get(&w.id) else {
                continue;
            };
            let (kind, lines) = match &window.state {
                AppState::Terminal { transcript, .. } => ("terminal", transcript_lines(transcript)),
                AppState::Editor { text, .. } => (
                    "editor",
                    text.split('\n').map(str::to_owned).collect::<Vec<_>>(),
                ),
                _ => continue,
            };
            let mut buffer = TextBuffer {
                handle: format!("window:{}:{kind}", w.id),
                window: Some(w.id),
                kind: kind.into(),
                lines,
                first_visible: 0,
                visible: 0,
                truncated: false,
            }
            .bound();
            attach_buffer(scene, &mut buffer, w.content);
            buffers.push(buffer);
        }
        let focus = focus_of(m, scene, &windows, phone);
        annotate_states(scene, m, &windows, &focus);
        scene.windows = windows;
        scene.buffers = buffers;
        scene.focus = Some(focus);
        scene.stamp();
    }
}

// ---- Action-result contract: what an action changed, not just whether it was accepted.
/// Actor-visible projection of one machine: everything an observation or a scene could
/// reveal, cheap enough to take before and after every action in a batch.
#[derive(Clone, PartialEq, Eq, Serialize)]
pub(super) struct Visible {
    focused: Option<u64>,
    windows: Vec<VisibleWindow>,
    url: Option<String>,
    app: Option<String>,
    page: u64,
    panel: Option<String>,
    launcher: bool,
    home_page: u32,
    address_focused: bool,
    focused_input: Option<String>,
    terminal: u64,
    /// Every browser's tabs, fields and zoom, their scroll positions aside.
    browser: u64,
    /// Every browser tab's scroll position.
    browser_scroll: u64,
    clipboard: u64,
    notifications: u64,
    /// System settings and the screen's power state.
    settings: u64,
    /// Shell state that is no focus change (see `effect::SHELL`).
    shell: u64,
    /// Recents, stars, bookmarks and downloads.
    library: u64,
    /// An application's own wheel use moved its view during the action. It carries no
    /// state of its own (the view it moved is in `content`), so it is left out of the
    /// digest: equal digests still mean equal observable state.
    #[serde(skip)]
    scrolled: bool,
}
#[derive(Clone, PartialEq, Eq, Serialize)]
struct VisibleWindow {
    id: u64,
    title: String,
    frame: Option<Rect>,
    minimized: bool,
    maximized: bool,
    document: String,
    /// Digest of the window's whole application state: text, caret, tabs, dirty flag.
    /// A terminal's scrollback position is its `scroll`, not its content.
    content: u64,
    /// Digest of where the window's panes are scrolled (and a pane pulled past its end),
    /// and of a terminal's scrollback position.
    scroll: u64,
}
/// Path, URL or document a window presents; empty when it presents none.
pub(super) fn presented(state: &AppState) -> String {
    match state {
        AppState::Browser { address } => address.clone(),
        AppState::Editor { path, .. } => path.clone(),
        AppState::Files { .. } => state.file_path().to_owned(),
        AppState::Native(app) => app.document(),
        AppState::Terminal { .. } => String::new(),
    }
}
/// A browser's state as it bears on what is shown, split into its scroll positions and
/// everything else. Storage and cookies are not shown and are left out.
pub(super) fn browser_view(b: &BrowserState) -> (u64, u64) {
    // The hash of exactly the bytes `cw_scene::digest(&(tabs, b.active, &b.zoom,
    // &b.pending))` hashes, where `tabs` is one `(&t.history, t.position, &t.focused,
    // &t.fields)` per tab — written out here rather than through one `Serialize` so
    // that each tab's back/forward stack can resume from the hash state it left behind
    // (`History::hash_into`). Hashing every page and image a session ever fetched, on
    // both sides of every action, was the whole of the growth in what a step costs.
    // `browser_view_hashes_what_serializing_would` pins the two against each other.
    let mut hasher = cw_scene::Digest::new();
    let put = |hasher: &mut cw_scene::Digest, bytes: &[u8]| {
        let _ = std::io::Write::write_all(hasher, bytes);
    };
    put(&mut hasher, b"[[");
    for (index, tab) in b.tabs.iter().enumerate() {
        put(&mut hasher, if index == 0 { b"[" } else { b",[" });
        tab.history.hash_into(&mut hasher);
        put(&mut hasher, b",");
        let _ = serde_json::to_writer(&mut hasher, &tab.position);
        put(&mut hasher, b",");
        let _ = serde_json::to_writer(&mut hasher, &tab.focused);
        put(&mut hasher, b",");
        let _ = serde_json::to_writer(&mut hasher, &tab.fields);
        put(&mut hasher, b"]");
    }
    put(&mut hasher, b"],");
    let _ = serde_json::to_writer(&mut hasher, &b.active);
    put(&mut hasher, b",");
    let _ = serde_json::to_writer(&mut hasher, &b.zoom);
    put(&mut hasher, b",");
    let _ = serde_json::to_writer(&mut hasher, &b.pending);
    put(&mut hasher, b"]");
    let scrolls: Vec<i32> = b.tabs.iter().map(|t| t.scroll_y).collect();
    (hasher.finish(), cw_scene::digest(&scrolls))
}
pub(super) fn visible(m: &MachineSession) -> Visible {
    let browsers: Vec<(u64, u64)> = std::iter::once(&m.browser)
        .chain(m.browser_windows.values())
        .map(browser_view)
        .collect();
    let d = &m.desktop;
    Visible {
        focused: d.focused,
        windows: d
            .windows
            .values()
            .map(|w| {
                let (content, scroll) = match &w.state {
                    AppState::Terminal { scroll, .. } => {
                        let mut still = w.state.clone();
                        if let AppState::Terminal { scroll, .. } = &mut still {
                            *scroll = 0;
                        }
                        (
                            cw_scene::digest(&still),
                            cw_scene::digest(&(&w.scroll, scroll)),
                        )
                    }
                    state => (cw_scene::digest(state), cw_scene::digest(&w.scroll)),
                };
                VisibleWindow {
                    id: w.id,
                    title: w.title.clone(),
                    frame: w.frame,
                    minimized: w.minimized,
                    maximized: w.maximized,
                    document: presented(&w.state),
                    content,
                    scroll,
                }
            })
            .collect(),
        url: m.browser.url().map(str::to_owned),
        app: m.active_app.clone(),
        page: cw_scene::digest(&m.custom_page),
        panel: d.panel.clone(),
        launcher: d.launcher_open,
        home_page: d.home_page,
        address_focused: m.address_focused,
        focused_input: m.focused_input.clone(),
        terminal: cw_scene::digest(&m.terminal),
        browser: cw_scene::digest(&browsers.iter().map(|b| b.0).collect::<Vec<_>>()),
        browser_scroll: cw_scene::digest(&browsers.iter().map(|b| b.1).collect::<Vec<_>>()),
        clipboard: cw_scene::digest(&(&d.clipboard, &d.clipboard_text)),
        notifications: cw_scene::digest(&d.notifications),
        settings: cw_scene::digest(&(&d.settings, &d.screen)),
        shell: cw_scene::digest(&(
            &d.search,
            &d.desktop_selection,
            (d.workspace, d.workspaces),
            &d.library_group,
            &d.overview,
            d.panel_month,
            &d.keyboard,
            &d.file_view,
        )),
        library: cw_scene::digest(&(&d.recents, &d.starred, &d.bookmarks, &d.downloads)),
        scrolled: m.scrolled,
    }
}
pub(super) fn effect_of(before: &Visible, after: &Visible) -> ActionEffect {
    let mut changed: BTreeSet<&str> = BTreeSet::new();
    let ids = |v: &Visible| v.windows.iter().map(|w| w.id).collect::<BTreeSet<_>>();
    let opened: Vec<u64> = ids(after).difference(&ids(before)).copied().collect();
    let closed: Vec<u64> = ids(before).difference(&ids(after)).copied().collect();
    if !opened.is_empty() {
        changed.insert(effect::WINDOW_OPENED);
    }
    if !closed.is_empty() {
        changed.insert(effect::WINDOW_CLOSED);
    }
    for w in &after.windows {
        let Some(old) = before.windows.iter().find(|o| o.id == w.id) else {
            continue;
        };
        if old.title != w.title {
            changed.insert(effect::WINDOW_TITLE);
        }
        if (old.frame, old.minimized, old.maximized) != (w.frame, w.minimized, w.maximized) {
            changed.insert(effect::WINDOW_MOVED);
        }
        if old.document != w.document {
            changed.insert(effect::DOCUMENT);
        }
        if old.content != w.content {
            changed.insert(effect::CONTENT);
        }
    }
    if before.focused != after.focused {
        changed.insert(effect::WINDOW_FOCUSED);
    }
    if before.url != after.url {
        changed.insert(effect::NAVIGATE);
    }
    if (
        &before.panel,
        before.launcher,
        before.home_page,
        before.address_focused,
        &before.focused_input,
    ) != (
        &after.panel,
        after.launcher,
        after.home_page,
        after.address_focused,
        &after.focused_input,
    ) {
        changed.insert(effect::FOCUS);
    }
    if (&before.app, before.page) != (&after.app, after.page) {
        changed.insert(effect::APPLICATION);
    }
    if before.terminal != after.terminal {
        changed.insert(effect::TERMINAL);
    }
    if before.browser != after.browser {
        changed.insert(effect::CONTENT);
    }
    let window_scrolled = after.windows.iter().any(|w| {
        before
            .windows
            .iter()
            .any(|o| o.id == w.id && o.scroll != w.scroll)
    });
    if window_scrolled || before.browser_scroll != after.browser_scroll || after.scrolled {
        changed.insert(effect::SCROLL);
    }
    for (tag, was, is) in [
        (effect::CLIPBOARD, before.clipboard, after.clipboard),
        (
            effect::NOTIFICATIONS,
            before.notifications,
            after.notifications,
        ),
        (effect::SETTINGS, before.settings, after.settings),
        (effect::SHELL, before.shell, after.shell),
        (effect::LIBRARY, before.library, after.library),
    ] {
        if was != is {
            changed.insert(tag);
        }
    }
    ActionEffect {
        changed: changed.into_iter().map(str::to_owned).collect(),
        windows_opened: opened,
        windows_closed: closed,
        focused_window: after.focused,
        url: after.url.clone(),
        state: cw_scene::digest(after),
    }
}

#[cfg(test)]
mod perception_tests {
    use super::*;
    use cw_sdk::Registry;
    fn desktop() -> (Environment, String) {
        let definition = WorldDefinition::from_json(
            r#"{"id":"t","profiles":[{"id":"linux","family":"linux"}],"computers":[{"id":"a",
            "profile":"linux","address":"10.0.0.1","user":"alice","installed_apps":["terminal",
            "editor","files","browser"],"initial_files":{"/home/alice/notes.txt":"initial commit of a long line that the pane will hard wrap\nsecond"}}],
            "metadata":{"desktop_themes":{"a":"ubuntu"}}}"#,
        )
        .unwrap();
        let mut e = Environment::new(Runtime::new(definition, 7, Registry::new()).unwrap());
        let id = e
            .environment(EnvironmentConfig::desktop("alice", "a"))
            .unwrap();
        (e, id)
    }
    fn launch(e: &mut Environment, id: &str, kind: &str, argument: &str) -> u64 {
        let r = e
            .step(
                id,
                vec![ActionEnvelope::new(
                    "application.v1",
                    "launch",
                    "a",
                    json!({"kind":kind,"argument":argument}),
                )],
            )
            .unwrap();
        assert!(r.outcomes[0].success, "{:?}", r.outcomes[0].error);
        r.outcomes[0].value["window"].as_u64().unwrap()
    }
    #[test]
    fn windows_are_published_with_identity_stacking_and_occlusion() {
        let (mut e, id) = desktop();
        let first = launch(&mut e, &id, "terminal", "");
        let second = launch(&mut e, &id, "editor", "/home/alice/notes.txt");
        let scene = e.scene(&id, 1280, 800).unwrap();
        assert_eq!(scene.windows.len(), 2);
        let bottom = scene.window(first).unwrap();
        let top = scene.window(second).unwrap();
        assert_eq!(bottom.app, "terminal");
        assert_eq!(top.app, "editor");
        assert_eq!(top.document, "/home/alice/notes.txt");
        assert!(top.focused && !bottom.focused);
        assert!(bottom.z < top.z);
        assert!(bottom.bounds.area() > 0 && bottom.content.area() > 0);
        // The overlap is reported as geometry, not as a boolean the agent must guess.
        assert_eq!(bottom.occluded_by, vec![second]);
        assert!(top.occluded_by.is_empty());
        let exposed = bottom.exposed.expect("partly visible");
        assert!(exposed.area() > 0 && exposed.area() < bottom.bounds.area());
        // A point inside the exposed part really does reach the lower window.
        let point = (
            exposed.x + exposed.width as i32 / 2,
            exposed.y + exposed.height as i32 / 2,
        );
        let stack = scene.hit_stack(point.0, point.1);
        assert!(stack
            .iter()
            .find(|h| h.interaction.is_some())
            .is_some_and(|h| h.window == Some(first)));
        // ... and the covered part reaches the upper one.
        let covered = top.bounds;
        let over = (covered.x + 4, covered.y + covered.height as i32 / 2);
        assert_eq!(
            scene.hit_test(over.0, over.1).and_then(|n| n.window),
            Some(second)
        );
    }
    #[test]
    fn accessibility_tree_names_window_owned_controls() {
        let (mut e, id) = desktop();
        let window = launch(&mut e, &id, "terminal", "");
        let scene = e.scene(&id, 1280, 800).unwrap();
        let ax = scene.accessibility();
        let close = ax
            .iter()
            .find(|a| a.id == format!("window:{window}:close"))
            .expect("close control");
        assert_eq!(close.window, Some(window));
        assert!(!close.role.is_empty() && !close.name.is_empty());
        assert!(close.enabled && close.hit.is_some() && close.revision != 0);
        // Merging is by interaction id: a control painted as several nodes is one entry.
        assert_eq!(ax.iter().filter(|a| a.id == close.id).count(), 1);
        let focus = scene.focus.as_ref().unwrap();
        assert_eq!(focus.window, Some(window));
        assert_eq!(focus.keyboard.route, "terminal");
        assert!(focus.keyboard.text_entry);
        assert!(ax.iter().any(|a| a.focused));
    }
    #[test]
    fn caret_is_published_instead_of_being_found_in_the_pixels() {
        let (mut e, id) = desktop();
        let window = launch(&mut e, &id, "terminal", "");
        e.step(
            &id,
            vec![ActionEnvelope::new(
                "keyboard.v1",
                "type",
                "a",
                json!({"text":"echo hi"}),
            )],
        )
        .unwrap();
        let scene = e.scene(&id, 1280, 800).unwrap();
        let focus = scene.focus.clone().unwrap();
        assert_eq!(focus.value.as_deref(), Some("echo hi"));
        let caret = focus.caret.expect("caret");
        assert_eq!(caret.offset, 7);
        assert!(caret.bounds.area() > 0);
        assert!(scene
            .window(window)
            .unwrap()
            .content
            .intersection(caret.bounds)
            .is_some());
        // The caret advances by exactly one cell per character typed.
        e.step(
            &id,
            vec![ActionEnvelope::new(
                "keyboard.v1",
                "type",
                "a",
                json!({"text":"!"}),
            )],
        )
        .unwrap();
        let next = e
            .scene(&id, 1280, 800)
            .unwrap()
            .focus
            .unwrap()
            .caret
            .unwrap();
        assert_eq!(next.offset, 8);
        assert_eq!(next.bounds.y, caret.bounds.y);
        assert_eq!(next.bounds.x - caret.bounds.x, caret.bounds.width as i32);
    }
    #[test]
    fn editor_caret_and_buffer_expose_the_whole_document() {
        let (mut e, id) = desktop();
        let window = launch(&mut e, &id, "editor", "/home/alice/notes.txt");
        let scene = e.scene(&id, 1000, 700).unwrap();
        let buffer = scene
            .buffers
            .iter()
            .find(|b| b.window == Some(window))
            .expect("editor buffer");
        assert_eq!(buffer.kind, "editor");
        assert_eq!(buffer.lines.len(), 2);
        assert!(buffer.lines[0].starts_with("initial commit"));
        let focus = scene.focus.as_ref().unwrap();
        assert_eq!(focus.keyboard.route, "editor");
        let caret = focus.caret.expect("caret");
        // The editor opens with the caret at the end of the file, and says so.
        assert_eq!(
            (caret.line, caret.column, caret.offset),
            (1, 6, buffer.lines.join("\n").chars().count() as u32)
        );
    }
    #[test]
    fn hard_wrapped_terminal_lines_are_marked_as_continuations() {
        let (mut e, id) = desktop();
        let window = launch(&mut e, &id, "terminal", "");
        e.step(
            &id,
            vec![
                ActionEnvelope::new(
                    "keyboard.v1",
                    "type",
                    "a",
                    json!({"text":"cat /home/alice/notes.txt"}),
                ),
                ActionEnvelope::new("keyboard.v1", "key", "a", json!({"key":"Enter"})),
            ],
        )
        .unwrap();
        // A pane narrow enough to hard wrap the file's first line mid-word.
        let scene = e.scene(&id, 560, 520).unwrap();
        let buffer = scene
            .buffers
            .iter()
            .find(|b| b.window == Some(window))
            .expect("terminal buffer");
        assert!(buffer
            .lines
            .iter()
            .any(|l| l.starts_with("initial commit of a long line")));
        let mut lines: Vec<&Node> = scene
            .nodes
            .iter()
            .filter(|n| {
                n.line
                    .as_ref()
                    .is_some_and(|l| l.pane.as_deref() == Some(&buffer.handle))
            })
            .collect();
        assert!(
            lines.len() > buffer.lines.len(),
            "the pane must have wrapped"
        );
        lines.sort_by_key(|n| n.painted_bounds().y);
        // Joining painted lines using the flags reproduces the buffer exactly.
        let mut joined: Vec<String> = Vec::new();
        for n in &lines {
            let line = n.line.as_ref().unwrap();
            if line.continuation {
                joined
                    .last_mut()
                    .unwrap()
                    .push_str(n.painted_text().unwrap());
            } else {
                joined.push(n.painted_text().unwrap().to_owned());
            }
        }
        let start = buffer.first_visible as usize;
        assert_eq!(joined, buffer.lines[start..start + joined.len()]);
        // Continuations point back at the fragment they continue.
        let wrapped = lines
            .iter()
            .find(|n| n.line.as_ref().unwrap().continuation)
            .unwrap();
        let line = wrapped.line.as_ref().unwrap();
        assert!(line.offset > 0);
        assert_eq!(
            line.wrapped_from,
            lines
                .iter()
                .find(|n| {
                    let l = n.line.as_ref().unwrap();
                    l.logical == line.logical && !l.continuation
                })
                .map(|n| n.id)
        );
    }
    #[test]
    fn revisions_are_stable_across_snapshot_restore_and_detect_no_op_steps() {
        let (mut e, id) = desktop();
        launch(&mut e, &id, "terminal", "");
        let before = e.scene(&id, 1024, 768).unwrap();
        assert!(before.digest != 0 && before.nodes.iter().all(|n| n.revision != 0));
        // Re-asking for the same viewport in the same state is bit-identical.
        assert_eq!(e.scene(&id, 1024, 768).unwrap(), before);
        assert!(!e.scene(&id, 1024, 768).unwrap().diff(&before).changed);
        let snapshot = e.snapshot();
        e.step(
            &id,
            vec![ActionEnvelope::new(
                "keyboard.v1",
                "type",
                "a",
                json!({"text":"ls"}),
            )],
        )
        .unwrap();
        let typed = e.scene(&id, 1024, 768).unwrap();
        let delta = typed.diff(&before);
        assert!(delta.changed && delta.focus);
        assert!(!delta.updated.is_empty());
        // Restoring reproduces the same ids: the counter is the content, not a tick.
        e.restore(&snapshot).unwrap();
        let restored = e.scene(&id, 1024, 768).unwrap();
        assert_eq!(restored.digest, before.digest);
        assert!(!restored.diff(&before).changed);
    }
    #[test]
    fn outcomes_report_the_app_level_consequence() {
        let (mut e, id) = desktop();
        let launched = e
            .step(
                &id,
                vec![ActionEnvelope::new(
                    "application.v1",
                    "launch",
                    "a",
                    json!({"kind":"terminal"}),
                )],
            )
            .unwrap();
        let effect = launched.outcomes[0].effect.clone().expect("effect");
        assert!(effect.changed.contains(&effect::WINDOW_OPENED.to_owned()));
        assert_eq!(effect.windows_opened.len(), 1);
        assert_eq!(effect.focused_window, Some(effect.windows_opened[0]));
        // Typing changes pane content and nothing structural.
        let typed = e
            .step(
                &id,
                vec![ActionEnvelope::new(
                    "keyboard.v1",
                    "type",
                    "a",
                    json!({"text":"ls"}),
                )],
            )
            .unwrap();
        let typed = typed.outcomes[0].effect.clone().unwrap();
        assert_eq!(typed.changed, vec![effect::CONTENT.to_owned()]);
        assert!(typed.state != effect.state);
        // An accepted action that changes nothing observable says so.
        let noop = e
            .step(
                &id,
                vec![ActionEnvelope::new(
                    "application.v1",
                    "focus",
                    "a",
                    json!({"window":effect.windows_opened[0]}),
                )],
            )
            .unwrap();
        let noop = noop.outcomes[0].effect.clone().unwrap();
        assert!(noop.is_noop(), "{:?}", noop.changed);
        assert_eq!(noop.state, typed.state);
        // A denied action never reached a machine, so it attributes no effect.
        let denied = e
            .step(
                &id,
                vec![ActionEnvelope::new("poison.v1", "run", "a", Value::Null)],
            )
            .unwrap();
        assert!(!denied.outcomes[0].success && denied.outcomes[0].effect.is_none());
    }
}
