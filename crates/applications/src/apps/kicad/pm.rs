//! The KiCad project manager: the project tree, the launchers for the editors, and the
//! New/Open project dialogs over the machine's projects folder.
use super::widgets::{self as w, chrome, MenuItem};
use super::{act, icons, Dialog, Kicad, Project};
use crate::desktop_scene::{shared::Align, Painter};
use crate::AppEffect;
use cw_eda::pcb::Board;
use cw_eda::schematic::Schematic;
use cw_scene::{Color, Rect};

const TREE_W: u32 = 280;

fn menus(k: &Kicad) -> Vec<(&'static str, Vec<MenuItem>)> {
    let open = k.session.project.is_some();
    let need = |t: &str| -> Result<String, &'static str> {
        if open {
            Ok(t.to_owned())
        } else {
            Err("no project is open")
        }
    };
    vec![
        (
            "File",
            vec![
                MenuItem::new("New Project...", "Ctrl+N", Ok("kicad:pm:new".into())),
                MenuItem::new("Open Project...", "Ctrl+O", Ok("kicad:pm:open".into())),
                MenuItem::new("Close Project", "", need("kicad:pm:close")).sep(),
            ],
        ),
        (
            "View",
            vec![MenuItem::new("Refresh", "F5", need("kicad:pm:refresh"))],
        ),
        (
            "Tools",
            vec![
                MenuItem::new("Schematic Editor", "Ctrl+E", need("kicad:pm:launch:sch")),
                MenuItem::new("PCB Editor", "Ctrl+P", need("kicad:pm:launch:pcb")),
                MenuItem::new("Open Project Directory", "", need("kicad:pm:folder")).sep(),
            ],
        ),
        (
            "Help",
            vec![MenuItem::new("About KiCad", "", Ok("kicad:about".into()))],
        ),
    ]
}

/// A free project name: `untitled`, then `untitled-1`, … against the projects found.
fn free_name(found: &[String]) -> String {
    let taken = |n: &str| found.iter().any(|f| f.rsplit('/').nth(1) == Some(n));
    if !taken("untitled") {
        return "untitled".into();
    }
    (1..)
        .map(|i| format!("untitled-{i}"))
        .find(|n| !taken(n))
        .unwrap_or_default()
}

impl Kicad {
    pub(super) fn pm_command(&mut self, window: u64, rest: &str) -> Result<Vec<AppEffect>, String> {
        let list = || AppEffect::ListTree {
            window,
            path: String::new(),
            depth: 2,
        };
        let mut tree = list();
        if let AppEffect::ListTree { path, .. } = &mut tree {
            path.clone_from(&self.session.projects_dir);
        }
        match rest.split_once(':').unwrap_or((rest, "")) {
            ("new", _) => {
                self.ui.dialog = Some(Dialog::NewProject {
                    name: free_name(&self.session.found),
                    error: String::new(),
                });
                self.ui.focus = Some("name".into());
                // Know which names are taken before one is chosen.
                Ok(vec![tree])
            }
            ("open", _) => {
                self.ui.dialog = Some(Dialog::OpenProject { selected: None });
                Ok(vec![tree])
            }
            ("close", _) => {
                self.session.project.as_ref().ok_or("no project is open")?;
                *self.session = super::Session {
                    revision: self.session.revision,
                    projects_dir: self.session.projects_dir.clone(),
                    found: self.session.found.clone(),
                    ..Default::default()
                };
                self.ui.status = "Project closed".into();
                Ok(vec![])
            }
            ("refresh", _) => {
                let p = self.session.project.as_ref().ok_or("no project is open")?;
                Ok(vec![AppEffect::ListDirectory {
                    window,
                    tab: 0,
                    path: p.dir.clone(),
                }])
            }
            ("folder", _) => {
                let p = self.session.project.as_ref().ok_or("no project is open")?;
                Ok(vec![AppEffect::Launch {
                    window,
                    kind: "files".into(),
                    argument: p.dir.clone(),
                }])
            }
            ("launch", frame) => match frame {
                "sch" | "pcb" => self.launch_frame(window, frame),
                other => Err(format!("unknown editor {other}")),
            },
            ("file", i) => {
                let i: usize = i.parse().map_err(|_| "bad row")?;
                if i >= self.session.listing.len() {
                    return Err("row not found".into());
                }
                self.ui.tree_row = Some(i);
                Ok(vec![])
            }
            (other, _) => Err(format!("unknown project manager command {other}")),
        }
    }
    /// Double-clicking a file in the tree opens it in its editor.
    pub(super) fn open_tree_row(
        &mut self,
        window: u64,
        i: usize,
    ) -> Result<Vec<AppEffect>, String> {
        let name = self.session.listing.get(i).ok_or("row not found")?.clone();
        self.ui.tree_row = Some(i);
        if name.ends_with(".kicad_sch") {
            self.launch_frame(window, "sch")
        } else if name.ends_with(".kicad_pcb") {
            self.launch_frame(window, "pcb")
        } else if name.ends_with('/') {
            let p = self.session.project.as_ref().ok_or("no project is open")?;
            Ok(vec![AppEffect::Launch {
                window,
                kind: "files".into(),
                argument: format!("{}/{}", p.dir, name.trim_end_matches('/')),
            }])
        } else {
            let p = self.session.project.as_ref().ok_or("no project is open")?;
            Ok(vec![AppEffect::Launch {
                window,
                kind: "editor".into(),
                argument: format!("{}/{name}", p.dir),
            }])
        }
    }
    pub(super) fn pm_dialog(&mut self, window: u64, rest: &str) -> Result<Vec<AppEffect>, String> {
        let dialog = self.ui.dialog.clone().ok_or("no dialog is open")?;
        match (dialog, rest.split_once(':').unwrap_or((rest, ""))) {
            (Dialog::About, ("ok", _)) => {
                self.close_dialog();
                Ok(vec![])
            }
            (Dialog::OpenProject { .. }, ("project", i)) => {
                let i: usize = i.parse().map_err(|_| "bad row")?;
                if i >= self.session.found.len() {
                    return Err("project not found".into());
                }
                self.ui.dialog = Some(Dialog::OpenProject { selected: Some(i) });
                Ok(vec![])
            }
            (Dialog::OpenProject { selected }, ("ok", _)) => {
                let path = selected
                    .and_then(|i| self.session.found.get(i))
                    .ok_or("select a project to open")?
                    .clone();
                let project = Project::from_pro(&path).ok_or("not a project file")?;
                self.close_dialog();
                Ok(self.load(window, project))
            }
            (Dialog::NewProject { name, .. }, ("ok", _)) => {
                let name = name.trim().to_owned();
                let error = if name.is_empty() {
                    Some("A project needs a name.")
                } else if name.contains(['/', '\\', ':']) || name.starts_with('.') {
                    Some("Project names cannot contain '/', '\\' or ':' or start with '.'.")
                } else if self
                    .session
                    .found
                    .iter()
                    .any(|f| f.rsplit('/').nth(1) == Some(name.as_str()))
                {
                    Some("A project with this name already exists in the projects folder.")
                } else {
                    None
                };
                if let Some(error) = error {
                    self.ui.dialog = Some(Dialog::NewProject {
                        name,
                        error: error.into(),
                    });
                    return Ok(vec![]);
                }
                let project = Project {
                    dir: format!("{}/{name}", self.session.projects_dir.trim_end_matches('/')),
                    name: name.clone(),
                };
                self.close_dialog();
                let schematic = Schematic::new(&project.pro());
                let board = Board::new(&project.pro());
                let effects = vec![
                    AppEffect::CreateDirectory {
                        window,
                        path: project.dir.clone(),
                    },
                    AppEffect::WriteFile {
                        window,
                        path: project.pro(),
                        content: cw_eda::files::write_project(&name, &board.rules),
                    },
                    AppEffect::WriteFile {
                        window,
                        path: project.file("kicad_sch"),
                        content: cw_eda::files::write_schematic(&schematic, &name),
                    },
                    AppEffect::WriteFile {
                        window,
                        path: project.file("kicad_pcb"),
                        content: cw_eda::files::write_board(&board),
                    },
                    AppEffect::ListDirectory {
                        window,
                        tab: 0,
                        path: project.dir.clone(),
                    },
                ];
                self.session.found.push(project.pro());
                self.session.schematic = schematic;
                self.session.board = board;
                self.session.sch_history.clear();
                self.session.pcb_history.clear();
                self.session.sch_dirty = false;
                self.session.pcb_dirty = false;
                self.session.erc = None;
                self.session.drc = None;
                self.session.problem = None;
                self.session.sim = Default::default();
                self.ui.status = format!("Created {}", project.pro());
                self.session.project = Some(project);
                Ok(effects)
            }
            (_, (other, _)) => Err(format!("unknown dialog command {other}")),
        }
    }

    pub(super) fn render_pm(&self, p: &mut Painter, env: &crate::AppEnv<'_>) {
        let (w, h) = (env.width, env.height);
        let c = chrome(env.theme);
        p.scene.background = c.panel;
        let menus = menus(self);
        let titles: Vec<&str> = menus.iter().map(|m| m.0).collect();
        let top = w::MENU_H as i32;
        // Left toolbar.
        p.box_(Rect::new(0, top, w::SIDE_W, h), c.bar, 0);
        p.vline(w::SIDE_W as i32, top, h, w::EDGE);
        let open = self.session.project.is_some();
        let need = |t: &str| -> Result<String, &'static str> {
            if open {
                Ok(t.to_owned())
            } else {
                Err("no project is open")
            }
        };
        let mut y = top + 4;
        for (icon, target, tip) in [
            (
                icons::new_project as w::Icon,
                Ok("kicad:pm:new".to_owned()),
                "Create new project",
            ),
            (
                icons::open,
                Ok("kicad:pm:open".to_owned()),
                "Open existing project",
            ),
            (
                icons::refresh,
                need("kicad:pm:refresh"),
                "Refresh project tree",
            ),
            (
                icons::folder,
                need("kicad:pm:folder"),
                "Open project directory in file browser",
            ),
        ] {
            w::tool(p, &c, 3, y, icon, target, tip, false);
            y += 32;
        }
        // Project tree.
        let tx = w::SIDE_W as i32 + 1;
        p.box_(Rect::new(tx, top, TREE_W, h), w::WHITE, 0);
        p.vline(tx + TREE_W as i32, top, h, w::EDGE);
        p.box_(Rect::new(tx, top, TREE_W, 26), c.bar, 0);
        p.label(
            tx + 8,
            top + 5,
            TREE_W - 16,
            "Project Files",
            12,
            w::MUTED,
            true,
            Align::Left,
        );
        match &self.session.project {
            Some(project) => {
                let mut y = top + 32;
                icons::folder(p, tx + 6, y);
                p.label(
                    tx + 30,
                    y + 2,
                    TREE_W - 36,
                    &format!("{}.kicad_pro", project.name),
                    13,
                    w::INK,
                    true,
                    Align::Left,
                );
                y += 26;
                for (i, entry) in self.session.listing.iter().enumerate() {
                    if y + 24 > h as i32 {
                        break;
                    }
                    let r = Rect::new(tx + 18, y, TREE_W - 24, 24);
                    let selected = self.ui.tree_row == Some(i);
                    p.button(
                        r,
                        if selected {
                            c.selection
                        } else {
                            Color::TRANSPARENT
                        },
                        2,
                        &format!("kicad:pm:file:{i}"),
                        entry,
                    );
                    let icon: w::Icon = if entry.ends_with(".kicad_sch") {
                        icons::schematic
                    } else if entry.ends_with(".kicad_pcb") {
                        icons::board
                    } else if entry.ends_with('/') {
                        icons::folder
                    } else {
                        icons::properties
                    };
                    icon(p, r.x + 2, y + 2);
                    p.label(
                        r.x + 26,
                        y + 3,
                        r.width - 30,
                        entry.trim_end_matches('/'),
                        13,
                        w::INK,
                        false,
                        Align::Left,
                    );
                    y += 24;
                }
            }
            None => {
                p.paragraph(tx + 12, top + 40, TREE_W - 24, "No project is open. Create a new project or open an existing one from the File menu or the toolbar.", 13, w::MUTED);
            }
        }
        // Launchers.
        let lx = tx + TREE_W as i32 + 1;
        let lw = w.saturating_sub(lx as u32);
        p.box_(Rect::new(lx, top, lw, h), c.panel, 0);
        let mut y = top + 20;
        if let Some(problem) = &self.session.problem {
            p.label(
                lx + 20,
                y,
                lw.saturating_sub(40),
                problem,
                13,
                Color::rgb(190, 30, 30),
                false,
                Align::Left,
            );
            y += 26;
        }
        for (frame, icon, title, desc) in [
            (
                "sch",
                icons::schematic as w::Icon,
                "Schematic Editor",
                "Edit the project schematic",
            ),
            (
                "pcb",
                icons::board,
                "PCB Editor",
                "Edit the project PCB design",
            ),
        ] {
            let r = Rect::new(lx + 20, y, lw.saturating_sub(40).min(460), 64);
            match need(&format!("kicad:pm:launch:{frame}")) {
                Ok(t) => {
                    p.button(r, Color::TRANSPARENT, c.radius, &t, title);
                    p.border(r, w::WHITE, c.radius, w::EDGE);
                }
                Err(why) => {
                    p.border(r, Color::rgb(245, 245, 245), c.radius, w::EDGE);
                    p.disabled(&format!("{title} — {why}"));
                }
            }
            // A launcher icon is the toolbar icon drawn large.
            let mark = p.scene.nodes.len();
            icon(p, 0, 0);
            for n in &mut p.scene.nodes[mark..] {
                n.transform.a = 2048;
                n.transform.d = 2048;
                n.transform.tx = r.x + 12;
                n.transform.ty = y + 12;
                if !open {
                    n.opacity = 90;
                }
            }
            let ink = if open { w::INK } else { w::FAINT };
            p.label(
                r.x + 64,
                y + 12,
                r.width - 76,
                title,
                14,
                ink,
                true,
                Align::Left,
            );
            p.label(
                r.x + 64,
                y + 34,
                r.width - 76,
                desc,
                12,
                w::MUTED,
                false,
                Align::Left,
            );
            y += 76;
        }
        if !self.ui.status.is_empty() {
            p.label(
                lx + 20,
                h as i32 - 28,
                lw.saturating_sub(40),
                &self.ui.status,
                12,
                w::MUTED,
                false,
                Align::Left,
            );
        }
        // Menus last, so an open one is on top.
        let xs = w::menubar(p, &c, w, &titles, self.ui.menu.as_deref());
        if let Some(open) = &self.ui.menu {
            if let Some(i) = titles.iter().position(|t| t == open) {
                p.z += 40;
                w::menu_panel(p, &c, xs[i], &menus[i].1);
                p.z -= 40;
            }
        }
    }
    pub(super) fn render_pm_dialog(
        &self,
        p: &mut Painter,
        env: &crate::AppEnv<'_>,
        dialog: &Dialog,
    ) {
        let c = chrome(env.theme);
        let (w, h) = (env.width, env.height);
        match dialog {
            Dialog::NewProject { name, error } => {
                let body = w::dialog(p, &c, env.theme, w, h, 480, 210, dialog.title());
                w::label_field(
                    p,
                    &c,
                    body.x,
                    body.y,
                    110,
                    body.width - 110,
                    "Project name:",
                    name,
                    "name",
                    self.ui.focus.as_deref() == Some("name"),
                );
                let folder = format!(
                    "{}/{}",
                    self.session.projects_dir.trim_end_matches('/'),
                    name.trim()
                );
                p.label(
                    body.x,
                    body.y + 38,
                    body.width,
                    &format!("Folder: {folder}"),
                    12,
                    w::MUTED,
                    false,
                    Align::Left,
                );
                if !error.is_empty() {
                    p.label(
                        body.x,
                        body.y + 62,
                        body.width,
                        error,
                        12,
                        Color::rgb(190, 30, 30),
                        false,
                        Align::Left,
                    );
                }
                let by = body.y + body.height as i32 - 30;
                w::button(
                    p,
                    &c,
                    Rect::new(body.x + body.width as i32 - 180, by, 84, 28),
                    "Cancel",
                    "kicad:dlg:cancel",
                    false,
                );
                w::button(
                    p,
                    &c,
                    Rect::new(body.x + body.width as i32 - 88, by, 88, 28),
                    "Save",
                    "kicad:dlg:ok",
                    true,
                );
            }
            Dialog::OpenProject { selected } => {
                let body = w::dialog(p, &c, env.theme, w, h, 560, 360, dialog.title());
                p.label(
                    body.x,
                    body.y,
                    body.width,
                    &format!("Projects in {}", self.session.projects_dir),
                    12,
                    w::MUTED,
                    false,
                    Align::Left,
                );
                let list = Rect::new(body.x, body.y + 22, body.width, body.height - 64);
                p.border(list, w::WHITE, 2, w::EDGE);
                if self.session.found.is_empty() {
                    p.label(
                        list.x + 10,
                        list.y + 10,
                        list.width - 20,
                        "No KiCad projects were found in this folder.",
                        13,
                        w::MUTED,
                        false,
                        Align::Left,
                    );
                }
                for (i, path) in self.session.found.iter().enumerate() {
                    let r = Rect::new(list.x + 2, list.y + 2 + i as i32 * 26, list.width - 4, 24);
                    if r.y + 24 > list.y + list.height as i32 {
                        break;
                    }
                    let rel = path
                        .strip_prefix(self.session.projects_dir.trim_end_matches('/'))
                        .unwrap_or(path)
                        .trim_start_matches('/');
                    w::row(
                        p,
                        &c,
                        r,
                        rel,
                        &format!("kicad:dlg:project:{i}"),
                        *selected == Some(i),
                        w::INK,
                    );
                }
                let by = body.y + body.height as i32 - 30;
                w::button(
                    p,
                    &c,
                    Rect::new(body.x + body.width as i32 - 180, by, 84, 28),
                    "Cancel",
                    "kicad:dlg:cancel",
                    false,
                );
                if selected.is_some() {
                    w::button(
                        p,
                        &c,
                        Rect::new(body.x + body.width as i32 - 88, by, 88, 28),
                        "Open",
                        "kicad:dlg:ok",
                        true,
                    );
                } else {
                    w::button_disabled(
                        p,
                        &c,
                        Rect::new(body.x + body.width as i32 - 88, by, 88, 28),
                        "Open",
                        "select a project first",
                    );
                }
            }
            Dialog::About => {
                let body = w::dialog(p, &c, env.theme, w, h, 420, 230, dialog.title());
                icons::board(p, body.x, body.y);
                p.label(
                    body.x + 30,
                    body.y,
                    body.width - 30,
                    "KiCad EDA",
                    16,
                    w::INK,
                    true,
                    Align::Left,
                );
                p.label(
                    body.x,
                    body.y + 34,
                    body.width,
                    "Version: 8.0.4, release build",
                    13,
                    w::INK,
                    false,
                    Align::Left,
                );
                p.paragraph(body.x, body.y + 58, body.width, "Schematic capture, SPICE simulation and printed circuit board layout. Libraries: Device, power, Simulation_SPICE, Connector, Switch, 74xGxx, Timer, MCU_Microchip_ATtiny.", 12, w::MUTED);
                let by = body.y + body.height as i32 - 30;
                w::button(
                    p,
                    &c,
                    Rect::new(body.x + body.width as i32 - 88, by, 88, 28),
                    "OK",
                    "kicad:dlg:ok",
                    true,
                );
            }
            _ => {}
        }
    }
    pub(super) fn pm_page(&self, page: &mut cw_protocol::Page) {
        use cw_protocol::PageElement as E;
        for (id, label) in [
            ("kicad:pm:new", "New Project"),
            ("kicad:pm:open", "Open Project"),
            ("kicad:pm:launch:sch", "Schematic Editor"),
            ("kicad:pm:launch:pcb", "PCB Editor"),
        ] {
            page.elements.push(E::Button {
                id: id.into(),
                text: label.into(),
                action: act(id),
            });
        }
        for (i, entry) in self.session.listing.iter().enumerate() {
            let id = format!("kicad:pm:file:{i}");
            page.elements.push(E::Button {
                id: id.clone(),
                text: entry.clone(),
                action: act(&id),
            });
        }
        if let Some(Dialog::OpenProject { .. }) = &self.ui.dialog {
            for (i, path) in self.session.found.iter().enumerate() {
                let id = format!("kicad:dlg:project:{i}");
                page.elements.push(E::Button {
                    id: id.clone(),
                    text: path.clone(),
                    action: act(&id),
                });
            }
        }
    }
}
