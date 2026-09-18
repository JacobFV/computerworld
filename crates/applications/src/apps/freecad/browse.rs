//! Browsing in the native file dialogs: the sidebar's places, the path bar or
//! breadcrumb, Back/Forward history, Up, Explorer's Home, New Folder and the
//! "Replace?" question. Every listing is a real `ListDirectory` of the machine and every
//! new folder a real `CreateDirectory`; the dialog shows what came back.
use super::files::{join, parent};
use super::*;

/// Where the dialog was: a folder, or Explorer's Home view of the home folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub folder: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub home_view: bool,
}

/// The New Folder name prompt; its text is the `FolderName` field.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderPrompt {
    /// Why the last Create was refused, shown under the name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// History kept per dialog; older steps fall off.
const HISTORY_LIMIT: usize = 64;

/// Explorer names a new folder "New folder", then "New folder (2)", … — the first
/// name nothing in the folder already has.
pub fn fresh_folder_name(entries: &[String]) -> String {
    let taken = |n: &str| {
        entries
            .iter()
            .any(|e| e.trim_end_matches('/').eq_ignore_ascii_case(n))
    };
    let mut name = "New folder".to_owned();
    let mut i = 2;
    while taken(&name) {
        name = format!("New folder ({i})");
        i += 1;
    }
    name
}

impl Cad {
    fn file_dialog_mut(&mut self) -> Result<&mut files::FileDialog, String> {
        match &mut self.dialog {
            Some(Dialog::File(d)) => Ok(d),
            _ => Err("no file dialog is open".into()),
        }
    }

    /// Keep what is typed in the name box before the keyboard moves elsewhere.
    fn stash_name(&mut self) {
        if let (
            Some(Field {
                target: FieldTarget::FileName,
                text,
                ..
            }),
            Some(Dialog::File(d)),
        ) = (&self.field, &mut self.dialog)
        {
            d.name = text.trim().to_owned();
        }
    }

    /// A save dialog's name box takes the keyboard back.
    fn refocus_name(&mut self) {
        match &self.dialog {
            Some(Dialog::File(d)) if d.purpose.saving() => {
                self.field = Some(Field {
                    target: FieldTarget::FileName,
                    text: d.name.clone(),
                    replace: true,
                });
            }
            _ => self.field = None,
        }
    }

    /// Show `folder` (or Explorer's Home), recording where the dialog was for Back.
    fn navigate(
        &mut self,
        window: u64,
        folder: String,
        home_view: bool,
        record: bool,
    ) -> Result<Vec<AppEffect>, String> {
        let d = self.file_dialog_mut()?;
        let here = Location {
            folder: d.folder.clone(),
            home_view: d.home_view,
        };
        let to = Location { folder, home_view };
        if record && here != to {
            d.back.push(here);
            if d.back.len() > HISTORY_LIMIT {
                d.back.remove(0);
            }
            d.forward.clear();
        }
        d.folder = to.folder.clone();
        d.home_view = to.home_view;
        d.loading = true;
        d.entries.clear();
        d.error = None;
        d.selected = None;
        d.scroll = 0;
        d.reveal = false;
        if !d.purpose.saving() {
            // What Open would open was a file in the folder that was left.
            d.name.clear();
        }
        Ok(vec![AppEffect::ListDirectory {
            window,
            tab: 0,
            path: to.folder,
        }])
    }

    /// Double-click (or Return on) an entry: a folder opens, a file is chosen.
    pub(crate) fn open_entry(
        &mut self,
        window: u64,
        entry: &str,
    ) -> Result<Vec<AppEffect>, String> {
        let d = self.file_dialog_mut()?;
        if !d.visible().iter().any(|e| *e == entry) {
            return Err("no such entry".into());
        }
        if let Some(name) = entry.strip_suffix('/') {
            let path = join(&d.folder, name);
            return self.navigate(window, path, false, true);
        }
        self.select_entry(entry)?;
        self.file_command(window, "ok")
    }

    fn select_entry(&mut self, entry: &str) -> Result<(), String> {
        let d = self.file_dialog_mut()?;
        if !d.visible().iter().any(|e| *e == entry) {
            return Err("no such entry".into());
        }
        d.selected = Some(entry.to_owned());
        let saving = d.purpose.saving();
        if entry.ends_with('/') {
            if !saving {
                d.name.clear();
            }
            return Ok(());
        }
        d.name = entry.to_owned();
        if saving {
            self.field = Some(Field {
                target: FieldTarget::FileName,
                text: entry.to_owned(),
                replace: true,
            });
        }
        Ok(())
    }

    /// The dialog's navigation commands; `None` when `rest` is not one of them.
    pub(crate) fn browse_command(
        &mut self,
        window: u64,
        rest: &str,
    ) -> Option<Result<Vec<AppEffect>, String>> {
        let (verb, arg) = rest.split_once(':').unwrap_or((rest, ""));
        let known = matches!(
            verb,
            "entry"
                | "open"
                | "place"
                | "crumb"
                | "home"
                | "back"
                | "forward"
                | "up"
                | "new-folder"
                | "folder-prompt"
                | "folder-create"
                | "folder-cancel"
                | "keep"
                | "collapse"
                | "list"
                | "scroll-by"
                | "side-scroll-by"
        );
        if !known {
            return None;
        }
        Some(self.browse(window, verb, arg))
    }

    fn browse(&mut self, window: u64, verb: &str, arg: &str) -> Result<Vec<AppEffect>, String> {
        let d = self.file_dialog_mut()?;
        // The question about replacing a file is modal over the dialog.
        if d.confirm.is_some() {
            if verb != "keep" {
                return Err("Answer whether to replace the file first".into());
            }
            d.confirm = None;
            self.refocus_name();
            return Ok(vec![]);
        }
        // Anything else closes the New Folder prompt, as a popover closes.
        if d.prompt.is_some() && !verb.starts_with("folder-") {
            d.prompt = None;
            self.refocus_name();
        }
        let d = self.file_dialog_mut()?;
        match verb {
            "entry" => {
                self.select_entry(arg)?;
                Ok(vec![])
            }
            "open" => self.open_entry(window, arg),
            "place" | "crumb" => {
                self.stash_name();
                let path = if arg.is_empty() { "/" } else { arg };
                self.navigate(window, path.to_owned(), false, true)
            }
            "home" => {
                if self.home.is_empty() || self.home == "/" {
                    return Err("This machine has no home folder".into());
                }
                self.stash_name();
                let home = self.home.clone();
                self.navigate(window, home, true, true)
            }
            "back" | "forward" => {
                self.stash_name();
                let d = self.file_dialog_mut()?;
                let here = Location {
                    folder: d.folder.clone(),
                    home_view: d.home_view,
                };
                let (from, to) = if verb == "back" {
                    (&mut d.back, &mut d.forward)
                } else {
                    (&mut d.forward, &mut d.back)
                };
                let Some(loc) = from.pop() else {
                    return Err(format!("There is nowhere to go {verb}"));
                };
                to.push(here);
                self.navigate(window, loc.folder, loc.home_view, false)
            }
            "up" => {
                if d.home_view || d.folder == "/" {
                    return Err("Already at the top".into());
                }
                self.stash_name();
                let up = parent(&self.file_dialog_mut()?.folder);
                self.navigate(window, up, false, true)
            }
            "new-folder" => {
                // Explorer makes "New folder" at once, highlighted in the list.
                if d.home_view || d.loading {
                    return Err("Open a folder first".into());
                }
                let name = fresh_folder_name(&d.entries);
                let path = join(&d.folder, &name);
                d.selected = Some(format!("{name}/"));
                let folder = d.folder.clone();
                Ok(vec![
                    AppEffect::CreateDirectory { window, path },
                    AppEffect::ListDirectory {
                        window,
                        tab: 0,
                        path: folder,
                    },
                ])
            }
            "folder-prompt" => {
                if d.home_view || d.loading {
                    return Err("Open a folder first".into());
                }
                self.stash_name();
                self.file_dialog_mut()?.prompt = Some(FolderPrompt::default());
                self.field = Some(Field {
                    target: FieldTarget::FolderName,
                    text: arg.to_owned(),
                    replace: true,
                });
                Ok(vec![])
            }
            "folder-cancel" => {
                if d.prompt.take().is_none() {
                    return Err("no folder is being named".into());
                }
                self.refocus_name();
                Ok(vec![])
            }
            "folder-create" => self.create_folder(window),
            "keep" => Err("nothing is waiting to be replaced".into()),
            "collapse" => {
                self.stash_name();
                let d = self.file_dialog_mut()?;
                if !d.purpose.saving() {
                    return Err("Only a save panel folds down".into());
                }
                d.collapsed = !d.collapsed;
                Ok(vec![])
            }
            // A click on the list's empty space: nothing selected. The target carries
            // how many rows the list shows, so Page Up/Down page by what is on screen.
            "list" => {
                d.page = arg.parse().map_err(|_| "bad list size")?;
                d.selected = None;
                if !d.purpose.saving() {
                    d.name.clear();
                }
                Ok(vec![])
            }
            "scroll-by" => {
                let n: i64 = arg.parse().map_err(|_| "bad scroll step")?;
                self.scroll_list(n)?;
                Ok(vec![])
            }
            "side-scroll-by" => {
                let n: i64 = arg.parse().map_err(|_| "bad scroll step")?;
                self.scroll_sidebar(n)?;
                Ok(vec![])
            }
            _ => unreachable!("browse_command only passes known verbs"),
        }
    }

    /// Why the name in the New Folder box cannot be created, in the platform's words;
    /// `None` when it can. The dialog checks as the name is typed, as GTK's does.
    pub(crate) fn folder_name_problem(&self) -> Option<String> {
        let text = match &self.field {
            Some(Field {
                target: FieldTarget::FolderName,
                text,
                ..
            }) => text.trim().to_owned(),
            _ => String::new(),
        };
        let Some(Dialog::File(d)) = &self.dialog else {
            return Some("no file dialog is open".into());
        };
        let mac = self.platform == Some(DesktopTheme::Macos);
        let exists = |n: &str| d.entries.iter().any(|e| e.trim_end_matches('/') == n);
        if text.is_empty() {
            Some("Type a name for the new folder".to_owned())
        } else if text == "." || text == ".." {
            Some(format!("A folder cannot be called “{text}”"))
        } else if text.contains('/') {
            Some("Folder names cannot contain “/”".to_owned())
        } else if exists(&text) {
            Some(if mac {
                format!("The name “{text}” is already taken. Please choose a different name.")
            } else if d.entries.iter().any(|e| *e == format!("{text}/")) {
                "A folder with that name already exists".to_owned()
            } else {
                "A file with that name already exists".to_owned()
            })
        } else {
            None
        }
    }

    /// Create the folder the prompt names, then go into it (as the Mac panel and GTK's
    /// chooser do). A refusal stays in the prompt, in the platform's words.
    fn create_folder(&mut self, window: u64) -> Result<Vec<AppEffect>, String> {
        let refusal = self.folder_name_problem();
        let text = match &self.field {
            Some(Field { text, .. }) => text.trim().to_owned(),
            None => String::new(),
        };
        let d = self.file_dialog_mut()?;
        if d.prompt.is_none() {
            return Err("no folder is being named".into());
        }
        if let Some(why) = refusal {
            if let Some(p) = &mut d.prompt {
                p.error = Some(why.clone());
            }
            return Err(why);
        }
        let path = join(&d.folder, &text);
        d.prompt = None;
        self.refocus_name();
        let mut effects = vec![AppEffect::CreateDirectory {
            window,
            path: path.clone(),
        }];
        effects.extend(self.navigate(window, path, false, true)?);
        Ok(effects)
    }

    /// Keys while a file dialog is up: Return and Escape answer whatever is frontmost,
    /// and the platforms' history keys move through the folders.
    pub(crate) fn file_dialog_key(
        &mut self,
        window: u64,
        key: &str,
    ) -> Result<Vec<AppEffect>, String> {
        let d = self.file_dialog_mut()?;
        if d.confirm.is_some() {
            return match key {
                // GTK's Replace is the default response; the Mac's alert and Windows'
                // Confirm Save As default to the safe answer (Cancel, No).
                "Enter" if self.platform == Some(DesktopTheme::Ubuntu) => {
                    self.file_command(window, "replace")
                }
                "Enter" | "Escape" => self.file_command(window, "keep"),
                _ => Err(format!("unsupported key {key} in a dialog")),
            };
        }
        if d.prompt.is_some() {
            return match key {
                "Enter" => self.file_command(window, "folder-create"),
                "Escape" => self.file_command(window, "folder-cancel"),
                _ => Err(format!("unsupported key {key} in a dialog")),
            };
        }
        match key {
            "Enter" => self.file_command(window, "ok"),
            "Escape" => self.file_command(window, "cancel"),
            "Alt+ArrowLeft" | "Ctrl+[" => self.file_command(window, "back"),
            "Alt+ArrowRight" | "Ctrl+]" => self.file_command(window, "forward"),
            "Alt+ArrowUp" | "Ctrl+ArrowUp" => self.file_command(window, "up"),
            "ArrowUp" | "ArrowDown" | "PageUp" | "PageDown" | "Home" | "End" => {
                self.move_selection(key)?;
                Ok(vec![])
            }
            _ => Err(format!("unsupported key {key} in a dialog")),
        }
    }

    /// Up/Down/Page Up/Page Down/Home/End move the selection through the list and
    /// bring it into view, as each platform's list does.
    fn move_selection(&mut self, key: &str) -> Result<(), String> {
        let d = self.file_dialog_mut()?;
        let rows: Vec<String> = d.visible().into_iter().cloned().collect();
        if rows.is_empty() {
            return Err("The folder is empty".into());
        }
        let last = rows.len() - 1;
        let page = d.page_rows();
        let at = d
            .selected
            .as_ref()
            .and_then(|s| rows.iter().position(|r| r == s));
        let to = match (key, at) {
            ("Home", _) => 0,
            ("End", _) => last,
            ("ArrowDown", None) | ("PageDown", None) => 0,
            (_, None) => last,
            ("ArrowDown", Some(i)) => (i + 1).min(last),
            ("ArrowUp", Some(i)) => i.saturating_sub(1),
            ("PageDown", Some(i)) => (i + page.saturating_sub(1).max(1)).min(last),
            (_, Some(i)) => i.saturating_sub(page.saturating_sub(1).max(1)),
        };
        self.select_entry(&rows[to])?;
        let d = self.file_dialog_mut()?;
        if to < d.scroll {
            d.scroll = to;
        } else if to >= d.scroll + page {
            d.scroll = to + 1 - page;
        }
        d.reveal = true;
        Ok(())
    }

    /// Scroll the list by `n` rows (negative: up), within its rows.
    pub(crate) fn scroll_list(&mut self, n: i64) -> Result<(), String> {
        let d = self.file_dialog_mut()?;
        let top = d.max_scroll() as i64;
        d.scroll = (d.scroll as i64 + n).clamp(0, top) as usize;
        d.reveal = false;
        Ok(())
    }
    pub(crate) fn scroll_sidebar(&mut self, n: i64) -> Result<(), String> {
        let most = self.sidebar_rows().saturating_sub(1) as i64;
        let d = self.file_dialog_mut()?;
        // The painter also stops once the last place is on screen.
        d.side_scroll = (d.side_scroll as i64 + n).clamp(0, most) as usize;
        Ok(())
    }
    /// How many rows the dialog's sidebar can hold on this platform: the standard
    /// places with every standard folder present.
    pub(crate) fn sidebar_rows(&self) -> usize {
        let theme = self.platform.unwrap_or(DesktopTheme::Ubuntu);
        let files = crate::FilesEnv {
            home: &self.home,
            folders: crate::standard_folders(theme)
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
            trash: "trash".into(),
            starred: &[],
        };
        crate::desktop_scene::standard_places(theme, &files).len()
    }

    /// A click on a scrollbar's track (`freecad:file:scrollbar:<rows>:<height>`, or
    /// `side-scrollbar:` for the sidebar): the view jumps so the clicked point of the
    /// track is the middle of what shows, as a click on GTK's and the Mac's tracks
    /// does.
    pub(crate) fn scrollbar_click(
        &mut self,
        rest: &str,
        at: Option<(i32, i32)>,
    ) -> Result<Vec<AppEffect>, String> {
        let (side, spec) = match rest.strip_prefix("side-scrollbar:") {
            Some(spec) => (true, spec),
            None => (
                false,
                rest.strip_prefix("scrollbar:").ok_or("not a scrollbar")?,
            ),
        };
        let nums: Vec<usize> = spec
            .split(':')
            .map(|n| n.parse().map_err(|_| "bad scrollbar"))
            .collect::<Result<_, _>>()?;
        // List: rows shown, track height. Sidebar: rows shown, rows in all, height.
        let (rows, side_total, height) = match (side, nums.as_slice()) {
            (false, [rows, h]) => (*rows, 0, *h as i64),
            (true, [rows, total, h]) => (*rows, *total, *h as i64),
            _ => return Err("bad scrollbar".into()),
        };
        let (_, dy) = at.unwrap_or((0, 0));
        let d = self.file_dialog_mut()?;
        if d.confirm.is_some() || d.prompt.is_some() {
            return Err("Finish the question first".into());
        }
        let total = if side { side_total } else { d.visible().len() };
        let point = (i64::from(dy).clamp(0, height.max(1)) * total as i64) / height.max(1);
        let first = (point - rows as i64 / 2).max(0) as usize;
        if side {
            d.side_scroll = first.min(total.saturating_sub(rows));
        } else {
            d.page = rows;
            d.scroll = first.min(d.max_scroll());
            d.reveal = false;
        }
        Ok(vec![])
    }

    /// The wheel over the dialog's list or sidebar; `false` when it is elsewhere.
    pub(crate) fn file_dialog_wheel(&mut self, target: &str, delta: i32) -> bool {
        let Some(rest) = target.strip_prefix("freecad:file:") else {
            return false;
        };
        let Some(Dialog::File(d)) = &self.dialog else {
            return false;
        };
        if d.confirm.is_some() {
            return false;
        }
        // A notch (120) moves three rows, as the platforms' lists do by default.
        let rows = (i64::from(delta) / 40).clamp(-1000, 1000);
        let rows = if rows == 0 {
            i64::from(delta.signum())
        } else {
            rows
        };
        let sidebar =
            rest.starts_with("place:") || rest == "home" || rest.starts_with("side-scrollbar:");
        let list = rest.starts_with("entry:")
            || rest.starts_with("list:")
            || rest.starts_with("scrollbar:");
        if sidebar {
            self.scroll_sidebar(rows).is_ok()
        } else if list {
            if let (Some(n), Some(Dialog::File(d))) = (
                rest.strip_prefix("list:").and_then(|n| n.parse().ok()),
                &mut self.dialog,
            ) {
                d.page = n;
            }
            self.scroll_list(rows).is_ok()
        } else {
            false
        }
    }
}
