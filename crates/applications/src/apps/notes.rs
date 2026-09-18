//! Notes kept as real files on the machine's own filesystem under the user's Notes folder.
//! Nothing is cached that the filesystem does not actually hold.
use super::look::{action, header, look, notice, FAINT, INK, LINE, MUTED};
use super::push_bounded;
use crate::desktop_scene::{DesktopTheme, Painter};
use crate::AppEffect;
use cw_scene::{Color, Rect};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notes {
    /// Folder the notes live in, resolved by the shell against the machine's filesystem.
    pub folder: String,
    pub entries: Vec<String>,
    pub open: Option<String>,
    pub text: String,
    pub dirty: bool,
    /// Set when the folder could not be listed; shown instead of an empty list.
    pub problem: Option<String>,
}
impl Notes {
    pub const KIND: &'static str = "notes";
    pub fn launch(argument: &str, window: u64, _clock_us: u64) -> (Self, Vec<AppEffect>) {
        let folder = if argument.is_empty() {
            "Notes".into()
        } else {
            argument.trim_end_matches('/').to_owned()
        };
        let app = Self {
            folder: folder.clone(),
            entries: vec![],
            open: None,
            text: String::new(),
            dirty: false,
            problem: None,
        };
        (
            app,
            vec![AppEffect::ListDirectory {
                window,
                tab: 0,
                path: folder,
            }],
        )
    }
    pub fn kind(&self) -> &'static str {
        Self::KIND
    }
    pub fn title(&self, theme: DesktopTheme) -> String {
        match theme {
            DesktopTheme::Windows => "Sticky Notes",
            DesktopTheme::Android => "Keep",
            _ => "Notes",
        }
        .into()
    }
    pub fn document(&self) -> String {
        self.open
            .as_ref()
            .map(|name| format!("{}/{name}", self.folder))
            .unwrap_or_default()
    }
    pub fn caption(&self) -> String {
        self.open.clone().unwrap_or_default()
    }
    pub fn modified(&self) -> bool {
        self.dirty
    }
    pub fn offline(&mut self, _tag: &str, reason: &str) {
        self.problem = Some(reason.to_owned());
    }
    pub fn http(
        &mut self,
        _window: u64,
        _tag: &str,
        _status: u16,
        _body: &str,
    ) -> Result<Vec<AppEffect>, String> {
        Err("notes are local files and make no requests".into())
    }
    /// The shell delivers a listing of the notes folder here.
    pub fn listed(&mut self, entries: Vec<String>) {
        self.entries = entries.into_iter().filter(|e| !e.ends_with('/')).collect();
        self.problem = None;
    }
    pub fn loaded(&mut self, content: String) {
        self.text = content;
        self.dirty = false;
    }
    pub fn text_input(&mut self, text: &str) -> Result<(), String> {
        if self.open.is_none() {
            return Err("no note is open".into());
        }
        push_bounded(&mut self.text, text, 64 * 1024);
        self.dirty = true;
        Ok(())
    }
    pub fn text(&mut self, text: &str) -> Result<(), String> {
        self.text_input(text)
    }
    pub fn key(&mut self, window: u64, key: &str, clock_us: u64) -> Result<Vec<AppEffect>, String> {
        match key {
            "Backspace" => {
                if self.open.is_none() {
                    return Err("no note is open".into());
                }
                self.text.pop();
                self.dirty = true;
                Ok(vec![])
            }
            "Enter" => {
                if self.open.is_none() {
                    return Err("no note is open".into());
                }
                self.text.push('\n');
                self.dirty = true;
                Ok(vec![])
            }
            "Ctrl+s" | "Meta+s" => self.click(window, "notes:save", clock_us),
            other => Err(format!("unsupported notes key {other}")),
        }
    }
    pub fn click(
        &mut self,
        window: u64,
        target: &str,
        clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        let command = target
            .strip_prefix("notes:")
            .ok_or("interaction does not belong to notes")?;
        match command {
            "reload" => Ok(vec![AppEffect::ListDirectory {
                window,
                tab: 0,
                path: self.folder.clone(),
            }]),
            "new" => {
                // A new note is named from the world clock, so two machines agree.
                let name = format!("note-{}.txt", clock_us / 1_000_000);
                self.open = Some(name.clone());
                self.text.clear();
                self.dirty = true;
                if !self.entries.contains(&name) {
                    self.entries.push(name);
                    self.entries.sort();
                }
                Ok(vec![])
            }
            "save" => {
                let name = self.open.clone().ok_or("no note is open")?;
                self.dirty = false;
                Ok(vec![
                    // The folder may not exist yet on a machine that has never taken a note.
                    AppEffect::CreateDirectory {
                        window,
                        path: self.folder.clone(),
                    },
                    AppEffect::WriteFile {
                        window,
                        path: format!("{}/{name}", self.folder),
                        content: self.text.clone(),
                    },
                    AppEffect::ListDirectory {
                        window,
                        tab: 0,
                        path: self.folder.clone(),
                    },
                ])
            }
            "body" => Ok(vec![]),
            rest => {
                let name = rest
                    .strip_prefix("open:")
                    .ok_or_else(|| format!("unknown notes command {command}"))?;
                if !self.entries.iter().any(|e| e == name) {
                    return Err("note not found".into());
                }
                self.open = Some(name.to_owned());
                self.text.clear();
                self.dirty = false;
                Ok(vec![AppEffect::ReadFile {
                    window,
                    path: format!("{}/{name}", self.folder),
                }])
            }
        }
    }
    pub fn page(&self, page: &mut cw_protocol::Page) {
        use cw_protocol::PageElement as E;
        let act = |url: &str| cw_protocol::PageAction {
            method: "APP".into(),
            url: url.into(),
            fields: Default::default(),
        };
        page.elements.push(E::Heading {
            id: "notes-folder".into(),
            text: self.folder.clone(),
            level: 2,
        });
        if let Some(problem) = &self.problem {
            page.elements.push(E::Text {
                id: "notes-problem".into(),
                text: problem.clone(),
            });
        }
        for (id, label) in [("notes:new", "New note"), ("notes:reload", "Reload")] {
            page.elements.push(E::Button {
                id: id.into(),
                text: label.into(),
                action: act(id),
            });
        }
        for name in &self.entries {
            page.elements.push(E::Button {
                id: format!("notes:open:{name}"),
                text: name.clone(),
                action: act(&format!("notes:open:{name}")),
            });
        }
        if self.open.is_some() {
            page.elements.push(E::Input {
                id: "notes-body".into(),
                label: "Note".into(),
                value: self.text.clone(),
                placeholder: String::new(),
            });
            page.elements.push(E::Button {
                id: "notes:save".into(),
                text: "Save".into(),
                action: act("notes:save"),
            });
        }
    }
    pub fn render(&self, p: &mut Painter, env: &crate::AppEnv<'_>) {
        let (theme, width, height) = (env.theme, env.width, env.height);
        let l = look(theme);
        p.scene.background = l.surface;
        let top = header(p, theme, &l, width, &self.title(theme));
        let list_w = if theme.mobile() || width < 480 {
            width
        } else {
            200
        };
        p.box_(Rect::new(0, top, list_w, height), l.chrome, 0);
        action(
            p,
            &l,
            Rect::new(8, top + 8, list_w.saturating_sub(84), 28),
            "New note",
            "notes:new",
            true,
        );
        action(
            p,
            &l,
            Rect::new(list_w as i32 - 70, top + 8, 62, 28),
            "Reload",
            "notes:reload",
            false,
        );
        if let Some(problem) = &self.problem {
            notice(p, list_w, top + 48, problem);
        } else if self.entries.is_empty() {
            notice(p, list_w, top + 48, "No notes yet");
        }
        for (index, name) in self.entries.iter().enumerate() {
            let r = Rect::new(
                4,
                top + 44 + index as i32 * 30,
                list_w.saturating_sub(8),
                28,
            );
            if r.y as u32 + 28 > height {
                break;
            }
            let on = self.open.as_deref() == Some(name.as_str());
            p.button(
                r,
                if on { l.selection } else { Color::TRANSPARENT },
                l.radius,
                &format!("notes:open:{name}"),
                name,
            );
            p.left(
                r.x + 10,
                r.y + 5,
                r.width.saturating_sub(18),
                name.trim_end_matches(".txt"),
                13,
                if on { l.accent } else { INK },
            );
        }
        if list_w == width {
            return;
        }
        let x = list_w as i32;
        p.vline(x, top, height, LINE);
        match &self.open {
            Some(name) => {
                p.strong(
                    x + 16,
                    top + 10,
                    width.saturating_sub(list_w + 110),
                    name,
                    14,
                    INK,
                );
                action(
                    p,
                    &l,
                    Rect::new(width as i32 - 86, top + 8, 76, 26),
                    if self.dirty { "Save •" } else { "Save" },
                    "notes:save",
                    self.dirty,
                );
                let body = Rect::new(
                    x + 12,
                    top + 42,
                    width.saturating_sub(list_w + 24),
                    height.saturating_sub(top as u32 + 54),
                );
                p.region(body, "notes:body", "Note text");
                p.paragraph(
                    body.x + 4,
                    body.y + 4,
                    body.width.saturating_sub(8),
                    &self.text,
                    13,
                    INK,
                );
                if self.text.is_empty() {
                    p.left(body.x + 4, body.y + 4, body.width, "Empty note", 13, FAINT);
                }
            }
            None => notice(p, width.saturating_sub(list_w), top + 40, "Select a note"),
        }
        let _ = MUTED;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn notes_are_real_files_read_and_written_through_the_filesystem() {
        let (mut app, effects) = Notes::launch("/home/alice/Notes", 1, 0);
        assert!(
            matches!(&effects[0], AppEffect::ListDirectory { path, .. } if path == "/home/alice/Notes")
        );
        app.listed(vec!["ideas.txt".into(), "drafts/".into()]);
        assert_eq!(app.entries, vec!["ideas.txt"]);
        let effects = app.click(1, "notes:open:ideas.txt", 0).unwrap();
        assert!(
            matches!(&effects[0], AppEffect::ReadFile { path, .. } if path == "/home/alice/Notes/ideas.txt")
        );
        app.loaded("first line".into());
        assert!(!app.dirty);
        app.text(" more").unwrap();
        assert!(app.dirty);
        let effects = app.click(1, "notes:save", 0).unwrap();
        // The folder is created first: a machine that has never taken a note has none.
        assert!(
            matches!(&effects[0], AppEffect::CreateDirectory { path, .. } if path == "/home/alice/Notes")
        );
        let AppEffect::WriteFile { path, content, .. } = &effects[1] else {
            panic!("expected a write");
        };
        assert_eq!(path, "/home/alice/Notes/ideas.txt");
        assert_eq!(content, "first line more");
        assert!(!app.dirty);
    }
    #[test]
    fn editing_without_an_open_note_is_refused() {
        let (mut app, _) = Notes::launch("", 1, 0);
        assert!(app.text("x").is_err());
        assert!(app.click(1, "notes:save", 0).is_err());
        assert!(app.click(1, "notes:open:missing.txt", 0).is_err());
        assert!(app.click(1, "not-mine", 0).is_err());
    }
    #[test]
    fn a_new_note_is_named_from_simulation_time() {
        let (mut app, _) = Notes::launch("", 1, 0);
        app.click(1, "notes:new", 90_000_000).unwrap();
        assert_eq!(app.open.as_deref(), Some("note-90.txt"));
    }
}
