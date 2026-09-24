use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppState {
    Terminal {
        input: String,
        /// Prompt on the line being typed now; mirrors `DesktopState::prompt` so the
        /// renderer and the semantic page agree without reaching back to the machine.
        #[serde(default)]
        prompt: String,
        /// Finished commands, oldest first, capped at `TRANSCRIPT_LIMIT`.
        #[serde(default)]
        transcript: Vec<TerminalEntry>,
        history: Vec<String>,
        /// Byte offset of the caret within `input`. Clicking the prompt line moves it,
        /// so typing and Backspace act where the pointer landed, not at the end.
        #[serde(default)]
        cursor: usize,
        /// Wrapped lines the view is lifted above the tail of the output. 0 follows new
        /// output, and any output pins it back to 0 the way a shell does.
        #[serde(default)]
        scroll: usize,
    },
    Editor {
        path: String,
        text: String,
        cursor: usize,
        dirty: bool,
    },
    Files {
        tabs: Vec<FileTab>,
        active: usize,
    },
    Browser {
        address: String,
    },
    /// Applications that ship with the simulator and draw their own platform chrome.
    Native(NativeApp),
}
impl AppState {
    pub fn file_tab(&self) -> Option<&FileTab> {
        match self {
            Self::Files { tabs, active } => tabs.get(*active).or_else(|| tabs.first()),
            _ => None,
        }
    }
    pub fn file_tab_mut(&mut self) -> Option<&mut FileTab> {
        match self {
            Self::Files { tabs, active } => {
                let index = (*active).min(tabs.len().checked_sub(1)?);
                tabs.get_mut(index)
            }
            _ => None,
        }
    }
    /// Folder shown by a file manager; empty for every other application.
    pub fn file_path(&self) -> &str {
        self.file_tab().map(|t| t.path.as_str()).unwrap_or("")
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Window {
    pub id: u64,
    pub title: String,
    #[serde(default)]
    pub app_id: String,
    pub state: AppState,
    #[serde(default)]
    pub minimized: bool,
    #[serde(default)]
    pub frame: Option<cw_scene::Rect>,
    #[serde(default)]
    pub restored_frame: Option<cw_scene::Rect>,
    #[serde(default)]
    pub maximized: bool,
    #[serde(default)]
    pub snapped: Option<WindowSnap>,
    /// Virtual desktop this window lives on.
    #[serde(default)]
    pub workspace: u32,
    /// Where each of the window's scrolling panes is scrolled to. Platform state, so
    /// every application scrolls the same way and a snapshot restores the view.
    #[serde(default, skip_serializing_if = "Scroll::is_empty")]
    pub scroll: Scroll,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesktopState {
    pub windows: BTreeMap<u64, Window>,
    pub focused: Option<u64>,
    pub(crate) next_id: u64,
    #[serde(default)]
    pub launcher_open: bool,
    #[serde(default)]
    pub panel: Option<String>,
    #[serde(default)]
    pub search: String,
    #[serde(default)]
    pub maximized: bool,
    /// Bottom to top. Unlike IDs, this order changes whenever a window is raised.
    #[serde(default)]
    pub stacking: Vec<u64>,
    #[serde(default)]
    pub pointer_capture: Option<PointerCapture>,
    /// Modifier keys the latest pointer action held (`apps::imaging::MOD_*` bits):
    /// `pointer.v1`'s `modifiers`, handed to an application's drag surface on a press.
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub pointer_modifiers: u8,
    /// The button of the latest pointer action (0 left, 1 middle, 2 right), handed to an
    /// application on a press so a right click can open its own context menu.
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub pointer_button: u8,
    /// Home folder of this machine's user; empty falls back to the root.
    #[serde(default)]
    pub home: String,
    /// Prompt this machine's shell would print next, refreshed after every command.
    /// Set from the machine like `home`; empty falls back to a bare sigil.
    #[serde(default)]
    pub prompt: String,
    /// World clock at the start of the current action, mirrored from `Runtime::tick()`.
    /// Applications read time from here and never from the host.
    #[serde(default)]
    pub clock_us: u64,
    /// Desktop icon the user has selected but not yet opened.
    #[serde(default)]
    pub desktop_selection: Option<String>,
    #[serde(default)]
    pub settings: SystemSettings,
    /// What the display is showing. Power controls really move between these.
    #[serde(default)]
    pub screen: ScreenState,
    /// Months the calendar panel is showing away from the world's current month, so a
    /// shell can page a month grid without inventing a clock of its own.
    #[serde(default)]
    pub panel_month: i32,
    /// Modifier and plane of the on-screen keyboard, so a painted shift key is a real one.
    #[serde(default)]
    pub keyboard: KeyboardState,
    /// What Cut and Copy put down and Paste picks up, shared by every file manager
    /// window. Capped at `CLIPBOARD_LIMIT` paths.
    #[serde(default)]
    pub clipboard: Option<Clipboard>,
    /// Text cut or copied in an editor, shared by every application on the machine.
    /// Capped at `TEXT_CLIPBOARD_LIMIT` bytes.
    #[serde(default)]
    pub clipboard_text: Option<String>,
    /// Documents opened from a file manager, newest first, capped at `RECENT_LIMIT`.
    /// Real history, not a guess: an entry is only here because it was opened.
    #[serde(default)]
    pub recents: Vec<String>,
    /// What the user starred (Files' Starred, Explorer's Favorites): absolute paths,
    /// folders with a trailing `/`, newest first, capped at `STAR_LIMIT`. Only a star
    /// control puts anything here.
    #[serde(default)]
    pub starred: Vec<String>,
    /// Pages the user really saved, shared by every browser window on the machine.
    #[serde(default)]
    pub bookmarks: Vec<Bookmark>,
    /// Files fetched out of the browser and written to the machine, newest first.
    #[serde(default)]
    pub downloads: Vec<Download>,
    /// Things the machine wants to tell the user. Posted by applications that saw them,
    /// never invented by the shell that draws them.
    #[serde(default)]
    pub notifications: Vec<Notice>,
    /// Virtual desktops. Index 0 always exists; `workspace` is the one on screen and
    /// every window records the one it belongs to.
    #[serde(default)]
    pub workspaces: u32,
    #[serde(default)]
    pub workspace: u32,
    /// App-library or launcher category the user has expanded, if any.
    #[serde(default)]
    pub library_group: Option<String>,
    /// A panel opened over the launcher rather than instead of it, the way a power
    /// flyout sits over an open Start menu.
    #[serde(default)]
    pub panel_over_launcher: bool,
    /// Page of a paged phone home screen (SpringBoard) on display, 0 first. Swipes and
    /// the page dots move it; shells clamp it to the pages the screen really has.
    #[serde(default)]
    pub home_page: u32,
    /// The desktop this machine's session shows, set by the environment at login. An
    /// application whose behaviour follows the platform (a native file dialog's
    /// default button) learns it from here when its window opens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<desktop_scene::DesktopTheme>,
    /// The view a new file manager window or tab starts in: the platform's own default
    /// (Files opens in the icon grid; Finder's and Explorer's windows here in the list).
    #[serde(default, skip_serializing_if = "FileView::is_list")]
    pub file_view: FileView,
    /// Where the pointer was when the open panel was opened, so a context menu stays
    /// where it was summoned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panel_at: Option<(i32, i32)>,
    /// Where a phone's Recents carousel is, and whether it is selecting text.
    #[serde(default, skip_serializing_if = "Overview::is_default")]
    pub overview: Overview,
}
/// Pixel Recents: which slot of the carousel is centred, and whether Select mode is
/// on. Slots are the windows in `ordered_windows` order (oldest first), and slot -1 is
/// the Clear all slot past the oldest card. `None` centres the focused window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Overview {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<i32>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub select: bool,
}
impl Overview {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}
/// Every string a semantic page shows, in page order, one per line: headings, text,
/// button and link labels, field values. What a phone's Recents "Select" can take
/// from an application card, read from the application's own projection rather than
/// recognised in its pixels.
pub fn page_text(page: &cw_protocol::Page) -> String {
    fn walk(value: &serde_json::Value, out: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(map) => {
                for key in ["title", "text", "label", "value"] {
                    if let Some(serde_json::Value::String(s)) = map.get(key) {
                        if !s.trim().is_empty() && out.last() != Some(s) {
                            out.push(s.clone());
                        }
                    }
                }
                for (key, child) in map {
                    if !matches!(key.as_str(), "title" | "text" | "label" | "value") {
                        walk(child, out);
                    }
                }
            }
            serde_json::Value::Array(items) => items.iter().for_each(|i| walk(i, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(
        &serde_json::to_value(&page.elements).unwrap_or_default(),
        &mut out,
    );
    out.join("\n")
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bookmark {
    pub title: String,
    pub url: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Download {
    pub name: String,
    pub path: String,
    pub url: String,
    pub bytes: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
    /// Application that posted it, so a shell can draw its icon.
    pub app: String,
    pub title: String,
    pub body: String,
    pub time_us: u64,
    /// What opening it does, as an interaction id the shell can dispatch.
    pub action: Option<String>,
    pub seen: bool,
}
/// Modifier and plane of an on-screen keyboard. A soft keyboard that cannot shift can
/// only type lowercase, so this is what lets a painted shift key be a real one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyboardState {
    pub shift: Shift,
    pub plane: Plane,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shift {
    #[default]
    Off,
    /// Applies to the next character only, then releases.
    Once,
    Lock,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Plane {
    #[default]
    Letters,
    Numbers,
    Symbols,
}
impl KeyboardState {
    /// Tap order of a real shift key: off, then next-character-only, then locked.
    pub fn cycle_shift(&mut self) {
        self.shift = match self.shift {
            Shift::Off => Shift::Once,
            Shift::Once => Shift::Lock,
            Shift::Lock => Shift::Off,
        };
    }
    pub fn upper(&self) -> bool {
        self.shift != Shift::Off
    }
    /// The character a letter key types. Typing releases a one-shot shift.
    pub fn apply(&mut self, ch: char) -> String {
        let out = if self.upper() {
            ch.to_uppercase().to_string()
        } else {
            ch.to_string()
        };
        if self.shift == Shift::Once {
            self.shift = Shift::Off;
        }
        out
    }
    pub fn set_plane(&mut self, plane: Plane) {
        self.plane = plane;
        // Leaving the letter plane drops a pending shift, as every phone keyboard does.
        if plane != Plane::Letters {
            self.shift = Shift::Off;
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenState {
    #[default]
    Active,
    Locked,
    Off,
}
/// Device state behind the quick settings, control centre and shade. These are real
/// switches: the shells read them back and every control that draws one flips one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemSettings {
    /// Percentages, 0 to 100.
    pub brightness: u8,
    pub volume: u8,
    pub wifi: bool,
    pub bluetooth: bool,
    pub airplane_mode: bool,
    pub do_not_disturb: bool,
    pub night_light: bool,
    pub dark_mode: bool,
    pub rotation_lock: bool,
    pub flashlight: bool,
    pub battery_saver: bool,
    pub hotspot: bool,
    /// Text editors soft-wrap long lines at the window edge instead of running them
    /// off it, as Notepad's Word wrap and TextEdit's Wrap to Window do.
    #[serde(default)]
    pub word_wrap: bool,
}
impl SystemSettings {
    pub const DEFAULT: Self = Self {
        brightness: 80,
        volume: 60,
        wifi: true,
        bluetooth: true,
        airplane_mode: false,
        do_not_disturb: false,
        night_light: false,
        dark_mode: false,
        rotation_lock: false,
        flashlight: false,
        battery_saver: false,
        hotspot: false,
        word_wrap: false,
    };
}
impl Default for SystemSettings {
    fn default() -> Self {
        Self::DEFAULT
    }
}
impl SystemSettings {
    pub fn toggle(&mut self, name: &str) -> Result<bool, String> {
        let flag = self.flag_mut(name)?;
        *flag = !*flag;
        let value = *flag;
        // Flight mode owns the radios, exactly as a phone does.
        if name == "airplane_mode" && value {
            self.wifi = false;
            self.bluetooth = false;
            self.hotspot = false;
        }
        if matches!(name, "wifi" | "bluetooth" | "hotspot") && value {
            self.airplane_mode = false;
        }
        Ok(value)
    }
    pub fn flag(&self, name: &str) -> Result<bool, String> {
        Ok(*Self::flag_of(self, name)?)
    }
    fn flag_of<'a>(&'a self, name: &str) -> Result<&'a bool, String> {
        Ok(match name {
            "wifi" => &self.wifi,
            "bluetooth" => &self.bluetooth,
            "airplane_mode" => &self.airplane_mode,
            "do_not_disturb" => &self.do_not_disturb,
            "night_light" => &self.night_light,
            "dark_mode" => &self.dark_mode,
            "rotation_lock" => &self.rotation_lock,
            "flashlight" => &self.flashlight,
            "battery_saver" => &self.battery_saver,
            "hotspot" => &self.hotspot,
            "word_wrap" => &self.word_wrap,
            _ => return Err(format!("unknown system switch {name}")),
        })
    }
    fn flag_mut(&mut self, name: &str) -> Result<&mut bool, String> {
        Ok(match name {
            "wifi" => &mut self.wifi,
            "bluetooth" => &mut self.bluetooth,
            "airplane_mode" => &mut self.airplane_mode,
            "do_not_disturb" => &mut self.do_not_disturb,
            "night_light" => &mut self.night_light,
            "dark_mode" => &mut self.dark_mode,
            "rotation_lock" => &mut self.rotation_lock,
            "flashlight" => &mut self.flashlight,
            "battery_saver" => &mut self.battery_saver,
            "hotspot" => &mut self.hotspot,
            "word_wrap" => &mut self.word_wrap,
            _ => return Err(format!("unknown system switch {name}")),
        })
    }
    /// Sliders are drawn as discrete steps so a click lands on an exact level.
    pub fn set_level(&mut self, name: &str, percent: u8) -> Result<(), String> {
        let percent = percent.min(100);
        match name {
            "brightness" => self.brightness = percent,
            "volume" => self.volume = percent,
            _ => return Err(format!("unknown system level {name}")),
        }
        Ok(())
    }
    pub fn level(&self, name: &str) -> Result<u8, String> {
        match name {
            "brightness" => Ok(self.brightness),
            "volume" => Ok(self.volume),
            _ => Err(format!("unknown system level {name}")),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowSnap {
    Left,
    Right,
    Full,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerPhase {
    Down,
    Move,
    Up,
    /// The drag was abandoned; an application discards what it was building.
    Cancel,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PointerCapture {
    pub window: u64,
    pub operation: String,
    pub start_x: i32,
    pub start_y: i32,
    pub original: cw_scene::Rect,
    pub moved: bool,
}
impl DesktopState {
    pub fn ordered_windows(&self) -> Vec<u64> {
        let mut ids: Vec<_> = self
            .stacking
            .iter()
            .copied()
            .filter(|id| self.windows.contains_key(id))
            .collect();
        for id in self.windows.keys() {
            if !ids.contains(id) {
                ids.push(*id);
            }
        }
        ids
    }
    pub fn effective_frame(&self, id: u64, area: cw_scene::Rect) -> cw_scene::Rect {
        let Some(window) = self.windows.get(&id) else {
            return area;
        };
        if window.maximized {
            return area;
        }
        if let Some(snap) = window.snapped {
            return snap_frame(snap, area);
        }
        let offset = (id % 7) as i32 * 28;
        clamp_frame(
            window.frame.unwrap_or(cw_scene::Rect::new(
                area.x + 56 + offset,
                area.y + 38 + offset,
                (area.width * 4 / 5).min(1000),
                (area.height * 4 / 5).min(680),
            )),
            area,
        )
    }
    pub fn maximize(&mut self, id: u64, area: cw_scene::Rect) -> Result<(), String> {
        let frame = self.effective_frame(id, area);
        let window = self.windows.get_mut(&id).ok_or("window not found")?;
        if window.maximized {
            window.frame = Some(clamp_frame(
                window.restored_frame.take().unwrap_or(frame),
                area,
            ));
            window.maximized = false;
            window.snapped = None;
        } else {
            if window.restored_frame.is_none() {
                window.restored_frame = Some(frame);
            }
            window.maximized = true;
            window.snapped = Some(WindowSnap::Full);
        }
        self.focus(id)
    }
    pub fn snap(&mut self, id: u64, snap: WindowSnap, area: cw_scene::Rect) -> Result<(), String> {
        let frame = self.effective_frame(id, area);
        let window = self.windows.get_mut(&id).ok_or("window not found")?;
        if window.restored_frame.is_none() {
            window.restored_frame = Some(frame);
        }
        window.snapped = Some(snap);
        window.maximized = snap == WindowSnap::Full;
        self.focus(id)
    }
    pub fn pointer_down(
        &mut self,
        id: u64,
        operation: &str,
        x: i32,
        y: i32,
        area: cw_scene::Rect,
    ) -> Result<(), String> {
        if operation != "drag"
            && !matches!(
                operation,
                "resize:n"
                    | "resize:ne"
                    | "resize:e"
                    | "resize:se"
                    | "resize:s"
                    | "resize:sw"
                    | "resize:w"
                    | "resize:nw"
            )
        {
            return Err("invalid window pointer operation".into());
        }
        let frame = self.effective_frame(id, area);
        self.focus(id)?;
        self.pointer_capture = Some(PointerCapture {
            window: id,
            operation: operation.into(),
            start_x: x,
            start_y: y,
            original: frame,
            moved: false,
        });
        Ok(())
    }
    pub fn pointer_move(&mut self, x: i32, y: i32, area: cw_scene::Rect) -> Result<bool, String> {
        let Some(mut capture) = self.pointer_capture.take() else {
            return Ok(false);
        };
        let dx = x.saturating_sub(capture.start_x);
        let dy = y.saturating_sub(capture.start_y);
        if !capture.moved && dx.unsigned_abs() < 3 && dy.unsigned_abs() < 3 {
            self.pointer_capture = Some(capture);
            return Ok(true);
        }
        let window = self
            .windows
            .get_mut(&capture.window)
            .ok_or("window not found")?;
        if !capture.moved
            && capture.operation == "drag"
            && (window.maximized || window.snapped.is_some())
        {
            let restored = window.restored_frame.take().unwrap_or(capture.original);
            let fraction =
                (capture.start_x - capture.original.x).clamp(0, capture.original.width as i32);
            capture.original = clamp_frame(
                cw_scene::Rect::new(
                    capture.start_x
                        - ((i64::from(fraction) * i64::from(restored.width))
                            / i64::from(capture.original.width.max(1)))
                            as i32,
                    capture.start_y - 16,
                    restored.width,
                    restored.height,
                ),
                area,
            );
        }
        capture.moved = true;
        window.maximized = false;
        window.snapped = None;
        window.restored_frame = None;
        let mut frame = capture.original;
        if capture.operation == "drag" {
            frame.x = frame.x.saturating_add(dx);
            frame.y = frame.y.saturating_add(dy);
        } else {
            let edge = capture.operation.trim_start_matches("resize:");
            let min_w = 320.min(area.width).min(frame.width) as i32;
            let min_h = 180.min(area.height).min(frame.height) as i32;
            let right = frame.x + frame.width as i32;
            let bottom = frame.y + frame.height as i32;
            if edge.contains('w') {
                frame.x = (frame.x + dx).clamp(area.x, right - min_w);
                frame.width = (right - frame.x) as u32;
            }
            if edge.contains('n') {
                frame.y = (frame.y + dy).clamp(area.y, bottom - min_h);
                frame.height = (bottom - frame.y) as u32;
            }
            if edge.contains('e') {
                frame.width = (frame.width as i32 + dx)
                    .clamp(min_w, (area.x + area.width as i32 - frame.x).max(min_w))
                    as u32;
            }
            if edge.contains('s') {
                frame.height = (frame.height as i32 + dy)
                    .clamp(min_h, (area.y + area.height as i32 - frame.y).max(min_h))
                    as u32;
            }
        }
        window.frame = Some(clamp_frame(frame, area));
        self.pointer_capture = Some(capture);
        Ok(true)
    }
    pub fn pointer_up(&mut self, x: i32, y: i32, area: cw_scene::Rect) -> Result<bool, String> {
        self.pointer_move(x, y, area)?;
        let Some(capture) = self.pointer_capture.take() else {
            return Ok(false);
        };
        if capture.moved && capture.operation == "drag" {
            let snap = if x <= area.x + 12 {
                Some(WindowSnap::Left)
            } else if x >= area.x + area.width as i32 - 12 {
                Some(WindowSnap::Right)
            } else if y <= area.y + 10 {
                Some(WindowSnap::Full)
            } else {
                None
            };
            if let Some(snap) = snap {
                self.snap(capture.window, snap, area)?;
            }
        }
        Ok(true)
    }
}
fn snap_frame(snap: WindowSnap, area: cw_scene::Rect) -> cw_scene::Rect {
    match snap {
        WindowSnap::Full => area,
        WindowSnap::Left => cw_scene::Rect::new(area.x, area.y, area.width / 2, area.height),
        WindowSnap::Right => cw_scene::Rect::new(
            area.x + (area.width / 2) as i32,
            area.y,
            area.width - area.width / 2,
            area.height,
        ),
    }
}
fn clamp_frame(frame: cw_scene::Rect, area: cw_scene::Rect) -> cw_scene::Rect {
    let width = frame.width.max(1).min(area.width.max(1));
    let height = frame.height.max(1).min(area.height.max(1));
    cw_scene::Rect::new(
        frame
            .x
            .clamp(area.x, area.x + area.width.saturating_sub(width) as i32),
        frame
            .y
            .clamp(area.y, area.y + area.height.saturating_sub(height) as i32),
        width,
        height,
    )
}

/// The application a file manager opens a document with: spreadsheets for workbooks
/// and CSV, the database client for SQLite files, FreeCAD for its documents and the
/// CAD exchange formats, KiCad for its projects, schematics and boards, the text
/// editor for the rest.
pub fn opener(name: &str) -> &'static str {
    let name = name.trim_end_matches('/');
    if apps::sheet::opens(name) {
        "spreadsheet"
    } else if apps::database::opens(name) {
        "database"
    } else if apps::freecad::opens(name) {
        apps::freecad::Freecad::KIND
    } else if apps::kicad::opens(name) {
        apps::kicad::Kicad::KIND
    } else {
        "editor"
    }
}

impl DesktopState {
    pub fn launch(&mut self, kind: &str, argument: &str) -> Result<(u64, Vec<AppEffect>), String> {
        let id = self.next_id;
        let (state, effects) = match kind {
            "terminal" => (
                AppState::Terminal {
                    input: String::new(),
                    prompt: self.prompt_line().to_owned(),
                    transcript: vec![],
                    history: vec![],
                    cursor: 0,
                    scroll: 0,
                },
                vec![],
            ),
            "editor" | "text_editor" => (
                AppState::Editor {
                    path: argument.into(),
                    text: String::new(),
                    cursor: 0,
                    dirty: false,
                },
                if argument.is_empty() {
                    vec![]
                } else {
                    vec![AppEffect::ReadFile {
                        window: id,
                        path: argument.into(),
                    }]
                },
            ),
            "files" | "file_manager" => {
                let path = if argument.is_empty() { "/" } else { argument };
                (
                    AppState::Files {
                        tabs: vec![FileTab {
                            view: self.file_view,
                            ..FileTab::new(path)
                        }],
                        active: 0,
                    },
                    vec![AppEffect::ListDirectory {
                        window: id,
                        tab: 0,
                        path: path.into(),
                    }],
                )
            }
            "browser" => (
                AppState::Browser {
                    address: argument.into(),
                },
                if argument.is_empty() {
                    vec![]
                } else {
                    vec![AppEffect::Navigate {
                        window: id,
                        url: argument.into(),
                    }]
                },
            ),
            _ => {
                let clock = self.clock_us;
                // KiCad keeps its projects under the user's Documents folder; other
                // document applications open on the Documents folder itself.
                let documents;
                let argument = if kind == apps::kicad::Kicad::KIND && argument.is_empty() {
                    documents = format!(
                        "{}/Documents/KiCad",
                        self.home_folder().trim_end_matches('/')
                    );
                    documents.as_str()
                } else if kind == apps::freecad::Freecad::KIND && argument.is_empty() {
                    // FreeCAD starts on the user's parts folder when there is one (it
                    // falls back to Documents itself when the folder is not there).
                    documents = format!(
                        "{}/Documents/Parts",
                        self.home_folder().trim_end_matches('/')
                    );
                    documents.as_str()
                } else if argument.is_empty() && NativeApp::opens_documents(kind) {
                    documents = format!("{}/Documents", self.home_folder().trim_end_matches('/'));
                    documents.as_str()
                } else {
                    argument
                };
                let theme = self.theme.unwrap_or(desktop_scene::DesktopTheme::Macos);
                let (mut app, mut effects) = NativeApp::launch_on(kind, argument, id, clock, theme)
                    .ok_or_else(|| format!("unknown application: {kind}"))??;
                // One editor of each kind per open project: asking again raises it.
                if let Some(key) = app.instance_key() {
                    let existing = self.windows.values().find_map(|w| match &w.state {
                        AppState::Native(other) if other.instance_key().as_ref() == Some(&key) => {
                            Some(w.id)
                        }
                        _ => None,
                    });
                    if let Some(existing) = existing {
                        self.focus(existing)?;
                        let window = self.windows.get_mut(&existing).expect("focused window");
                        let AppState::Native(other) = &mut window.state else {
                            unreachable!("instance keys belong to native applications")
                        };
                        let effects = other.reopen(existing, argument);
                        return Ok((existing, effects));
                    }
                }
                // A new frame of an application whose windows share one document starts
                // on that document, not on a fresh load of it.
                if let Some(link) = app.link_key() {
                    let sibling = self
                        .windows
                        .values()
                        .filter_map(|w| match &w.state {
                            AppState::Native(other) if other.link_key() == Some(link) => {
                                Some(other)
                            }
                            _ => None,
                        })
                        .max_by_key(|other| other.link_revision());
                    if let Some(sibling) = sibling {
                        app.share_from(sibling);
                        effects = app.reopen(id, argument);
                    }
                }
                // Visual Studio Code keeps its settings under the user's home and opens
                // `~/project` when it is launched on nothing.
                if let NativeApp::Code(code) = &mut app {
                    let mut first = code.attach(&self.home_folder(), &self.trash_folder(), id);
                    first.append(&mut effects);
                    effects = first;
                }
                // FreeCAD's file dialogs start in the user's Documents folder.
                if let NativeApp::Freecad(cad) = &mut app {
                    cad.attach(&self.home_folder(), self.theme);
                }
                (AppState::Native(app), effects)
            }
        };
        self.next_id += 1;
        self.windows.insert(
            id,
            Window {
                id,
                title: kind.into(),
                app_id: kind.into(),
                state,
                minimized: false,
                frame: None,
                restored_frame: None,
                maximized: false,
                snapped: None,
                // A new window opens on the desktop the user is looking at.
                workspace: self.workspace,
                scroll: Scroll::default(),
            },
        );
        self.stacking.retain(|window| *window != id);
        self.stacking.push(id);
        self.focused = Some(id);
        self.launcher_open = false;
        self.panel = None;
        self.search.clear();
        Ok((id, effects))
    }
    pub fn focus(&mut self, id: u64) -> Result<(), String> {
        if !self.windows.contains_key(&id) {
            return Err("window not found".into());
        }
        self.windows.get_mut(&id).unwrap().minimized = false;
        self.stacking.retain(|window| *window != id);
        self.stacking.push(id);
        self.focused = Some(id);
        self.launcher_open = false;
        self.panel = None;
        self.search.clear();
        Ok(())
    }
    pub fn close(&mut self, id: u64) -> Result<(), String> {
        self.windows.remove(&id).ok_or("window not found")?;
        self.stacking.retain(|window| *window != id);
        if self
            .pointer_capture
            .as_ref()
            .is_some_and(|capture| capture.window == id)
        {
            self.pointer_capture = None;
        }
        if self.focused == Some(id) {
            self.focused = self
                .ordered_windows()
                .into_iter()
                .rev()
                .find(|id| self.windows.get(id).is_some_and(|w| !w.minimized));
        }
        Ok(())
    }
    pub fn minimize(&mut self, id: u64) -> Result<(), String> {
        if self
            .pointer_capture
            .as_ref()
            .is_some_and(|capture| capture.window == id)
        {
            self.pointer_capture = None;
        }
        self.windows
            .get_mut(&id)
            .ok_or("window not found")?
            .minimized = true;
        if self.focused == Some(id) {
            self.focused = self
                .ordered_windows()
                .into_iter()
                .rev()
                .find(|id| self.windows.get(id).is_some_and(|w| !w.minimized));
        }
        Ok(())
    }
    pub fn home(&mut self) {
        // Going home from the home screen itself returns to its first page, as the
        // gesture does on a phone; from an application it keeps the page it left.
        if self.focused.is_none() && !self.launcher_open && self.panel.is_none() {
            self.home_page = 0;
        }
        self.pointer_capture = None;
        for window in self.windows.values_mut() {
            window.minimized = true;
        }
        self.focused = None;
        self.launcher_open = false;
        self.panel = None;
        self.search.clear();
    }
    pub fn cycle(&mut self) -> Result<(), String> {
        let ids = self.ordered_windows();
        let target = ids
            .iter()
            .rev()
            .copied()
            .find(|id| Some(*id) != self.focused)
            .or_else(|| ids.last().copied());
        if let Some(target) = target {
            self.focus(target)?;
        }
        Ok(())
    }
    /// The focused plain-text editor's caret and text length, to tell afterwards
    /// whether an edit or a caret move happened.
    fn editor_mark(&self) -> Option<(u64, usize, usize)> {
        let id = self.focused?;
        match &self.windows.get(&id)?.state {
            AppState::Editor { text, cursor, .. } => Some((id, *cursor, text.len())),
            _ => None,
        }
    }
    /// After an edit or a caret move in a plain-text editor, the view is brought back
    /// to the caret (as little as it takes), wherever it had been scrolled.
    fn reveal_editor_caret(&mut self, before: Option<(u64, usize, usize)>) {
        let Some((id, cursor, len)) = before else {
            return;
        };
        let moved = match self.windows.get(&id).map(|w| &w.state) {
            Some(AppState::Editor {
                text, cursor: c, ..
            }) => (*c, text.len()) != (cursor, len),
            _ => false,
        };
        if moved {
            if let Some(w) = self.windows.get_mut(&id) {
                w.scroll.reveal = Some(desktop_scene::EDITOR_PANE.into());
            }
        }
    }
    pub fn text(&mut self, text: &str) -> Result<(), String> {
        let mark = self.editor_mark();
        let result = self.text_inner(text);
        self.reveal_editor_caret(mark);
        result
    }
    fn text_inner(&mut self, text: &str) -> Result<(), String> {
        let window = self
            .focused
            .and_then(|id| self.windows.get_mut(&id))
            .ok_or("no focused window")?;
        match &mut window.state {
            AppState::Terminal { input, cursor, .. } => {
                if *cursor > input.len() || !input.is_char_boundary(*cursor) {
                    return Err("invalid terminal cursor".into());
                }
                input.insert_str(*cursor, text);
                *cursor += text.len();
            }
            AppState::Editor {
                text: content,
                cursor,
                dirty,
                ..
            } => {
                if *cursor > content.len() || !content.is_char_boundary(*cursor) {
                    return Err("invalid text cursor".into());
                }
                content.insert_str(*cursor, text);
                *cursor += text.len();
                *dirty = true;
            }
            AppState::Browser { address } => address.push_str(text),
            AppState::Files { tabs, active } => {
                let tab = tabs
                    .get_mut(*active)
                    .ok_or("tab not found")
                    .map_err(str::to_owned)?;
                let field = tab.field_mut().ok_or("file manager has no text focus")?;
                if field.chars().count() + text.chars().count() > FIELD_LIMIT {
                    return Err("file manager field is full".into());
                }
                field.push_str(text);
            }
            AppState::Native(app) => app.text(text)?,
        }
        Ok(())
    }
    pub fn key(&mut self, key: &str) -> Result<Vec<AppEffect>, String> {
        let mark = self.editor_mark();
        let result = self.key_inner(key);
        self.reveal_editor_caret(mark);
        result
    }
    fn key_inner(&mut self, key: &str) -> Result<Vec<AppEffect>, String> {
        let id = self.focused.ok_or("no focused window")?;
        let window = self.windows.get_mut(&id).ok_or("window not found")?;
        if let AppState::Editor { text, cursor, .. } = &window.state {
            if *cursor > text.len() || !text.is_char_boundary(*cursor) {
                return Err("invalid text cursor".into());
            }
        }
        let mut effects = vec![];
        match &mut window.state {
            AppState::Terminal {
                input,
                history,
                cursor,
                scroll,
                ..
            } => {
                if *cursor > input.len() || !input.is_char_boundary(*cursor) {
                    return Err("invalid terminal cursor".into());
                }
                match key {
                    "Enter" => {
                        let command = std::mem::take(input);
                        *cursor = 0;
                        // Running something brings the view back to the tail: the answer
                        // must not appear off screen below a scrolled-up frame.
                        *scroll = 0;
                        history.push(command.clone());
                        effects.push(AppEffect::Execute {
                            window: id,
                            command,
                        });
                    }
                    "Backspace" => {
                        if *cursor > 0 {
                            let at = input[..*cursor]
                                .char_indices()
                                .next_back()
                                .map(|(i, _)| i)
                                .unwrap_or(0);
                            input.drain(at..*cursor);
                            *cursor = at;
                        }
                    }
                    "Delete" => {
                        if let Some(ch) = input[*cursor..].chars().next() {
                            let end = *cursor + ch.len_utf8();
                            input.drain(*cursor..end);
                        }
                    }
                    "ArrowLeft" => {
                        *cursor = input[..*cursor]
                            .char_indices()
                            .next_back()
                            .map(|(i, _)| i)
                            .unwrap_or(0);
                    }
                    "ArrowRight" => {
                        if let Some(ch) = input[*cursor..].chars().next() {
                            *cursor += ch.len_utf8();
                        }
                    }
                    "Home" => *cursor = 0,
                    "End" => *cursor = input.len(),
                    _ => return Err(format!("unsupported terminal key {key}")),
                }
            }
            AppState::Editor {
                path,
                text,
                cursor,
                dirty,
            } => match key {
                "Ctrl+s" | "Meta+s" => {
                    if path.is_empty() {
                        return Err("editor has no save path".into());
                    }
                    effects.push(AppEffect::WriteFile {
                        window: id,
                        path: path.clone(),
                        content: text.clone(),
                    });
                }
                "Backspace" => {
                    if *cursor > 0 {
                        let p = text[..*cursor]
                            .char_indices()
                            .next_back()
                            .map(|(i, _)| i)
                            .unwrap_or(0);
                        text.drain(p..*cursor);
                        *cursor = p;
                        *dirty = true;
                    }
                }
                "Delete" => {
                    if *cursor < text.len() {
                        let end = *cursor + text[*cursor..].chars().next().unwrap().len_utf8();
                        text.drain(*cursor..end);
                        *dirty = true;
                    }
                }
                "ArrowLeft" => {
                    *cursor = text[..*cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                }
                "ArrowRight" => {
                    if let Some(ch) = text[*cursor..].chars().next() {
                        *cursor += ch.len_utf8();
                    }
                }
                // Up and Down keep the column, clamped to a shorter line's end.
                "ArrowUp" | "ArrowDown" => {
                    let start = text[..*cursor].rfind('\n').map_or(0, |i| i + 1);
                    let column = text[start..*cursor].chars().count();
                    let target = if key == "ArrowUp" {
                        (start > 0).then(|| text[..start - 1].rfind('\n').map_or(0, |i| i + 1))
                    } else {
                        text[*cursor..].find('\n').map(|i| *cursor + i + 1)
                    };
                    if let Some(line) = target {
                        let stop = text[line..].find('\n').map_or(text.len(), |i| line + i);
                        *cursor = text[line..stop]
                            .char_indices()
                            .nth(column)
                            .map_or(stop, |(i, _)| line + i);
                    }
                }
                "Home" => {
                    *cursor = text[..*cursor].rfind('\n').map(|i| i + 1).unwrap_or(0);
                }
                "End" => {
                    *cursor += text[*cursor..].find('\n').unwrap_or(text.len() - *cursor);
                }
                "Enter" => {
                    text.insert(*cursor, '\n');
                    *cursor += 1;
                    *dirty = true;
                }
                _ => return Err(format!("unsupported editor key {key}")),
            },
            AppState::Browser { address } => match key {
                "Enter" => effects.push(AppEffect::Navigate {
                    window: id,
                    url: address.clone(),
                }),
                "Backspace" => {
                    address.pop();
                }
                _ => return Err(format!("unsupported address key {key}")),
            },
            // Only the open field takes keys; a file manager has no general text focus.
            AppState::Files { tabs, active } => {
                let tab = tabs
                    .get_mut(*active)
                    .ok_or("tab not found")
                    .map_err(str::to_owned)?;
                match key {
                    "Escape" => {
                        // Cancelling a search clears the filter: a hidden filter and an
                        // empty folder look the same, and one of them is a lie.
                        if tab.rename.is_none() {
                            tab.query.clear();
                        }
                        tab.stop_editing();
                    }
                    "Backspace" => {
                        tab.field_mut()
                            .ok_or("file manager has no text focus")?
                            .pop();
                    }
                    "Enter" if tab.rename.is_some() => return self.commit_rename(),
                    "Enter" if tab.searching => tab.searching = false,
                    "Ctrl+h" | "Ctrl+H" if !tab.editing_text() => {
                        tab.show_hidden = !tab.show_hidden;
                    }
                    // Delete moves the selection to the trash, as it does in Files,
                    // Finder and Explorer. Never while a field is collecting keys: in
                    // a rename, Delete is a character being erased, not a file.
                    "Delete" if !tab.editing_text() => return self.files_command("move-to-trash"),
                    _ => return Err(format!("unsupported file manager key {key}")),
                }
            }
            AppState::Native(app) => {
                let clock = self.clock_us;
                let more = app.key(id, key, clock)?;
                return self.native_effects(more);
            }
        }
        Ok(effects)
    }
    /// Apply the effects an application asks of the desktop itself — the clipboard —
    /// and hand the rest on to the environment.
    fn native_effects(&mut self, effects: Vec<AppEffect>) -> Result<Vec<AppEffect>, String> {
        let mut out = Vec::with_capacity(effects.len());
        for effect in effects {
            match effect {
                AppEffect::CopyText { text, .. } => self.copy_text(&text)?,
                AppEffect::Paste { window } => {
                    // Pasting an empty clipboard pastes nothing, as it does everywhere.
                    let Some(text) = self.clipboard_text.clone() else {
                        continue;
                    };
                    let app = match self.windows.get_mut(&window).map(|w| &mut w.state) {
                        Some(AppState::Native(app)) => app,
                        _ => return Err("window is not a native application".into()),
                    };
                    out.extend(app.paste(window, &text)?);
                }
                other => out.push(other),
            }
        }
        Ok(out)
    }
    /// Put text on the machine's clipboard.
    pub fn copy_text(&mut self, text: &str) -> Result<(), String> {
        if text.len() > TEXT_CLIPBOARD_LIMIT {
            return Err("the selection is too large to copy".into());
        }
        self.clipboard_text = Some(text.to_owned());
        Ok(())
    }
    /// Typed text, and whatever the focused application needs done because of it.
    pub fn type_text(&mut self, text: &str) -> Result<Vec<AppEffect>, String> {
        let id = self.focused.ok_or("no focused window")?;
        if let Some(AppState::Native(app)) = self.windows.get_mut(&id).map(|w| &mut w.state) {
            let effects = app.text_effects(id, text)?;
            return self.native_effects(effects);
        }
        self.text(text).map(|()| vec![])
    }
    /// A pointer pressed on a control of the focused window, before it is released.
    pub fn press_at(&mut self, target: &str, dx: i32, dy: i32) -> Result<(), String> {
        let id = self.focused.ok_or("no focused window")?;
        let modifiers = self.pointer_modifiers;
        let button = self.pointer_button;
        match self.windows.get_mut(&id).map(|w| &mut w.state) {
            Some(AppState::Native(app)) => {
                app.pointer_modifiers(modifiers);
                app.pointer_button(button);
                app.press_at(target, dx, dy)
            }
            _ => Ok(()),
        }
    }
    fn code_mut(&mut self, id: u64) -> Result<&mut apps::code::Code, String> {
        match &mut self.windows.get_mut(&id).ok_or("window not found")?.state {
            AppState::Native(NativeApp::Code(code)) => Ok(code),
            _ => Err("window is not Visual Studio Code".into()),
        }
    }
    /// A folder tree an application asked for arrived, or could not be listed.
    pub fn tree_listed(
        &mut self,
        id: u64,
        path: &str,
        depth: u32,
        result: Result<Vec<String>, String>,
    ) -> Result<Vec<AppEffect>, String> {
        if let Some(AppState::Native(app)) = self.windows.get_mut(&id).map(|w| &mut w.state) {
            if !matches!(app, NativeApp::Code(_)) {
                return app.tree_listed(id, path, result);
            }
        }
        Ok(self.code_mut(id)?.tree_listed(id, path, depth, result))
    }
    /// Files an application asked to read, each with its content or why it failed.
    pub fn files_read(
        &mut self,
        id: u64,
        tag: &str,
        files: Vec<(String, Result<String, String>)>,
    ) -> Result<Vec<AppEffect>, String> {
        if let Some(AppState::Native(app)) = self.windows.get_mut(&id).map(|w| &mut w.state) {
            if !matches!(app, NativeApp::Code(_)) {
                return app.files_read(id, tag, files);
            }
        }
        Ok(self.code_mut(id)?.files_read(id, tag, files))
    }
    /// A shell session command an application ran finished.
    pub fn shell_ran(
        &mut self,
        id: u64,
        tag: &str,
        outcome: ShellOutcome,
    ) -> Result<Vec<AppEffect>, String> {
        Ok(self.code_mut(id)?.shell_ran(id, tag, outcome))
    }
    /// What the machine's debugger answered a `Debug` effect with.
    pub fn debug_reply(
        &mut self,
        id: u64,
        tag: &str,
        reply: Result<cw_protocol::debug::Reply, String>,
    ) -> Result<Vec<AppEffect>, String> {
        Ok(self.code_mut(id)?.debug_reply(id, tag, reply))
    }
    /// A write reached the disk. Editors learn which file, so the right one turns clean.
    pub fn file_written(
        &mut self,
        id: u64,
        path: &str,
        content: &str,
    ) -> Result<Vec<AppEffect>, String> {
        if let Some(app) = self.windows.get_mut(&id).and_then(|w| match &mut w.state {
            AppState::Native(app) => app.web_mut(),
            _ => None,
        }) {
            return app.written(id, path);
        }
        if let Ok(code) = self.code_mut(id) {
            return Ok(code.written(id, path, content));
        }
        if let Some(AppState::Native(NativeApp::Freecad(cad))) =
            self.windows.get_mut(&id).map(|w| &mut w.state)
        {
            cad.written(path);
            return Ok(vec![]);
        }
        if let Some(AppState::Native(app)) = self.windows.get_mut(&id).map(|w| &mut w.state) {
            if app.written(path) {
                return Ok(vec![]);
            }
        }
        self.file_saved(id, content).map(|()| vec![])
    }
    /// A wheel turn over a control of window `id`, at (`dx`, `dy`) inside it, for the
    /// application's own use of the wheel (a canvas zooms, a grid moves by rows, a
    /// terminal walks its scrollback). `false` leaves it to the pane under the pointer.
    pub fn wheel(
        &mut self,
        id: u64,
        target: &str,
        dx: i32,
        dy: i32,
        wheel: Wheel,
    ) -> Result<bool, String> {
        match &mut self.windows.get_mut(&id).ok_or("window not found")?.state {
            AppState::Native(app) => app.wheel(target, dx, dy, wheel),
            AppState::Terminal { scroll, .. } if wheel.dy != 0 => {
                // Rolling towards the user moves the view down, towards the tail.
                let lines = wheel.lines(TERMINAL_LINE);
                let next = (*scroll as i64 - i64::from(lines)).clamp(0, SCROLL_LIMIT as i64);
                let moved = next as usize != *scroll;
                *scroll = next as usize;
                Ok(moved)
            }
            _ => Ok(false),
        }
    }
    /// Scroll pane `pane` of window `id` to `offset` pixels. The caller clamps it to
    /// the extent the pane published. Returns whether the view moved.
    pub fn scroll_pane(&mut self, id: u64, pane: &str, offset: i32) -> Result<bool, String> {
        let window = self.windows.get_mut(&id).ok_or("window not found")?;
        Ok(window.scroll.set(pane, offset))
    }
    /// Whether a secondary-button press on `target` of window `id` is the application's.
    pub fn app_takes_secondary(&self, id: u64, target: &str) -> bool {
        matches!(self.windows.get(&id).map(|w| &w.state), Some(AppState::Native(app)) if app.takes_secondary(target))
    }
    /// Deliver successful effect results. A failed save must not mark an editor clean.
    pub fn file_loaded(&mut self, id: u64, content: String) -> Result<(), String> {
        match &mut self.windows.get_mut(&id).ok_or("window not found")?.state {
            AppState::Editor {
                text,
                cursor,
                dirty,
                ..
            } => {
                *text = content;
                *cursor = text.len();
                *dirty = false;
                Ok(())
            }
            _ => Err("window is not an editor".into()),
        }
    }
    pub fn file_saved(&mut self, id: u64, saved_content: &str) -> Result<(), String> {
        match &mut self.windows.get_mut(&id).ok_or("window not found")?.state {
            AppState::Editor { text, dirty, .. } => {
                if text == saved_content {
                    *dirty = false;
                }
                Ok(())
            }
            AppState::Native(NativeApp::Code(_)) => Ok(()),
            _ => Err("window is not an editor".into()),
        }
    }
    /// Deliver decoded pixels for a file the application asked to see.
    pub fn image_loaded(
        &mut self,
        id: u64,
        path: &str,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    ) -> Result<(), String> {
        match &mut self.windows.get_mut(&id).ok_or("window not found")?.state {
            AppState::Native(app) => app.image(path, width, height, rgba),
            _ => Err("window is not a native application".into()),
        }
    }
    /// The file could not be decoded. The application says so rather than showing a gap.
    pub fn image_failed(&mut self, id: u64, path: &str, reason: &str) -> Result<(), String> {
        match &mut self.windows.get_mut(&id).ok_or("window not found")?.state {
            AppState::Native(app) => {
                app.image_failed(path, reason);
                Ok(())
            }
            _ => Err("window is not a native application".into()),
        }
    }
    /// A picture the application encoded was written to `path`.
    pub fn image_saved(&mut self, id: u64, path: &str) -> Result<Vec<AppEffect>, String> {
        match &mut self.windows.get_mut(&id).ok_or("window not found")?.state {
            AppState::Native(app) => app.image_saved(id, path),
            _ => Err("window is not a native application".into()),
        }
    }
    /// The bytes of a file the application asked for, or why they could not be read.
    pub fn bytes_loaded(
        &mut self,
        id: u64,
        path: &str,
        result: Result<Vec<u8>, String>,
    ) -> Result<Vec<AppEffect>, String> {
        let clock = self.clock_us;
        match &mut self.windows.get_mut(&id).ok_or("window not found")?.state {
            AppState::Native(app) => app.bytes(id, path, result, clock),
            _ => Err("window is not a native application".into()),
        }
    }
    /// A `WriteBytes` finished, or failed with the reason.
    pub fn bytes_saved(
        &mut self,
        id: u64,
        path: &str,
        result: Result<(), String>,
    ) -> Result<Vec<AppEffect>, String> {
        match &mut self.windows.get_mut(&id).ok_or("window not found")?.state {
            AppState::Native(app) => app.bytes_saved(id, path, result),
            _ => Err("window is not a native application".into()),
        }
    }
    /// Coverage of a line of text the application asked to have rasterised.
    pub fn text_rasterized(
        &mut self,
        id: u64,
        width: u32,
        height: u32,
        alpha: Vec<u8>,
    ) -> Result<(), String> {
        match &mut self.windows.get_mut(&id).ok_or("window not found")?.state {
            AppState::Native(app) => app.text_rasterized(width, height, alpha),
            _ => Err("window is not a native application".into()),
        }
    }
    /// Whether `target` inside window `id` is a surface that follows a drag (a canvas,
    /// a slider) rather than a button that fires on release.
    pub fn app_drags(&self, id: u64, target: &str) -> bool {
        // A pane's scroll bar is a drag surface in every window, whatever it shows.
        if ScrollBar::parse(target).is_some() {
            return self.windows.contains_key(&id);
        }
        matches!(
            self.windows.get(&id).map(|w| &w.state),
            Some(AppState::Native(app)) if app.drags(target)
        )
    }
    /// Pointer pressed on an application drag surface. `bounds` is where the surface
    /// was painted; every later position is delivered relative to its top-left, so a
    /// drag that leaves the surface still maps to the same coordinates.
    pub fn app_pointer_down(
        &mut self,
        id: u64,
        target: &str,
        x: i32,
        y: i32,
        bounds: cw_scene::Rect,
    ) -> Result<Vec<AppEffect>, String> {
        self.app_pointer_down_with(id, target, x, y, bounds, 0)
    }
    /// A press with a particular button (0 left, 1 middle, 2 right).
    pub fn app_pointer_down_with(
        &mut self,
        id: u64,
        target: &str,
        x: i32,
        y: i32,
        bounds: cw_scene::Rect,
        button: u8,
    ) -> Result<Vec<AppEffect>, String> {
        self.focus(id)?;
        let clock = self.clock_us;
        let modifiers = self.pointer_modifiers;
        let bar = ScrollBar::parse(target).is_some();
        if let Some(AppState::Native(app)) = self.windows.get_mut(&id).map(|w| &mut w.state) {
            app.pointer_button(button);
            app.pointer_modifiers(modifiers);
        }
        let window = self.windows.get_mut(&id).ok_or("window not found")?;
        let effects = match &mut window.state {
            _ if bar => {
                window
                    .scroll
                    .drag(target, PointerPhase::Down, x - bounds.x, y - bounds.y)?;
                vec![]
            }
            AppState::Native(app) => app.pointer(
                id,
                target,
                PointerPhase::Down,
                x - bounds.x,
                y - bounds.y,
                clock,
            )?,
            _ => return Err("window is not a native application".into()),
        };
        self.pointer_capture = Some(PointerCapture {
            window: id,
            operation: format!("app:{target}"),
            start_x: x,
            start_y: y,
            original: bounds,
            moved: false,
        });
        Ok(effects)
    }
    /// Whether `target` inside window `id` tracks a pointer that is merely passing over.
    pub fn app_hovers(&self, id: u64, target: &str) -> bool {
        matches!(
            self.windows.get(&id).map(|w| &w.state),
            Some(AppState::Native(app)) if app.hovers(target)
        )
    }
    /// The pointer passed over a hover surface, `x`/`y` relative to its top-left.
    /// Returns what the application asks of the machine because of it.
    pub fn app_hover(&mut self, id: u64, target: &str, x: i32, y: i32) -> Vec<AppEffect> {
        match self.windows.get_mut(&id).map(|w| &mut w.state) {
            Some(AppState::Native(app)) => app.hover(id, target, x, y),
            _ => vec![],
        }
    }
    /// The pointer's shape over a hover surface of window `id`.
    pub fn app_hover_cursor(&self, id: u64, target: &str) -> &'static str {
        match self.windows.get(&id).map(|w| &w.state) {
            Some(AppState::Native(app)) => app.hover_cursor(target),
            _ => "default",
        }
    }
    /// The newest revision of each shared document, and whether any window lags it.
    fn linked_newest(&self) -> Vec<(u64, &'static str, u64)> {
        let mut newest: BTreeMap<&'static str, (u64, u64)> = BTreeMap::new();
        for w in self.windows.values() {
            if let AppState::Native(app) = &w.state {
                if let Some(key) = app.link_key() {
                    let rev = app.link_revision();
                    let entry = newest.entry(key).or_insert((rev, w.id));
                    if rev > entry.0 {
                        *entry = (rev, w.id);
                    }
                }
            }
        }
        newest
            .into_iter()
            .map(|(key, (rev, id))| (id, key, rev))
            .collect()
    }
    /// Whether some window shows an older copy of a document another window changed.
    pub fn needs_settle(&self) -> bool {
        let newest = self.linked_newest();
        self.windows.values().any(|w| match &w.state {
            AppState::Native(app) => app.link_key().is_some_and(|key| {
                newest
                    .iter()
                    .any(|(_, k, rev)| *k == key && app.link_revision() < *rev)
            }),
            _ => false,
        })
    }
    /// Bring every window that shares a document up to the newest copy of it, so an
    /// edit made in one frame shows in every other frame of that application.
    pub fn settle_linked(&mut self) {
        for (source, key, rev) in self.linked_newest() {
            let Some(AppState::Native(from)) = self.windows.get(&source).map(|w| w.state.clone())
            else {
                continue;
            };
            for w in self.windows.values_mut() {
                if w.id == source {
                    continue;
                }
                if let AppState::Native(app) = &mut w.state {
                    if app.link_key() == Some(key) && app.link_revision() < rev {
                        app.share_from(&from);
                    }
                }
            }
        }
    }
    /// Whether any window has work to do between actions, such as a video export.
    pub fn busy(&self) -> bool {
        self.windows
            .values()
            .any(|w| matches!(&w.state, AppState::Native(a) if a.busy()))
    }
    /// One simulation step of every window's background work; returns the effects it
    /// asks for (a finished export's files).
    pub fn background(&mut self) -> Vec<AppEffect> {
        let mut out = vec![];
        for (id, w) in self.windows.iter_mut() {
            if let AppState::Native(a) = &mut w.state {
                if a.busy() {
                    out.extend(a.background(*id));
                }
            }
        }
        out
    }
    /// Whether the pointer is captured by an application drag surface.
    pub fn app_captured(&self) -> bool {
        self.pointer_capture
            .as_ref()
            .is_some_and(|c| c.operation.starts_with("app:"))
    }
    /// Move or release a captured application drag. `None` when no application holds
    /// the pointer.
    pub fn app_pointer(
        &mut self,
        phase: PointerPhase,
        x: i32,
        y: i32,
    ) -> Option<Result<Vec<AppEffect>, String>> {
        let capture = self.pointer_capture.clone()?;
        let target = capture.operation.strip_prefix("app:")?.to_owned();
        if phase != PointerPhase::Move {
            self.pointer_capture = None;
        } else if let Some(c) = &mut self.pointer_capture {
            c.moved = true;
        }
        let clock = self.clock_us;
        let window = capture.window;
        if ScrollBar::parse(&target).is_some() {
            return Some(match self.windows.get_mut(&window) {
                Some(w) => w
                    .scroll
                    .drag(
                        &target,
                        phase,
                        x - capture.original.x,
                        y - capture.original.y,
                    )
                    .map(|_| vec![]),
                None => Err("window not found".into()),
            });
        }
        Some(match self.windows.get_mut(&window).map(|w| &mut w.state) {
            Some(AppState::Native(app)) => app.pointer(
                window,
                &target,
                phase,
                x - capture.original.x,
                y - capture.original.y,
                clock,
            ),
            _ => Err("window not found".into()),
        })
    }
    /// Deliver an application HTTP reply. Returns follow-up effects, so a successful
    /// mutation can refetch without the shell knowing what the application wanted.
    pub fn http_response(
        &mut self,
        id: u64,
        tag: &str,
        status: u16,
        body: &str,
    ) -> Result<Vec<AppEffect>, String> {
        match &mut self.windows.get_mut(&id).ok_or("window not found")?.state {
            AppState::Native(app) => app.http(id, tag, status, body),
            _ => Err("window is not a native application".into()),
        }
    }
    /// A folder could not be listed. Native applications show that in place of content;
    /// the file manager treats it as a real failure, so `false` asks the caller to raise it.
    pub fn directory_failed(&mut self, id: u64, reason: &str) -> Result<bool, String> {
        match &mut self.windows.get_mut(&id).ok_or("window not found")?.state {
            AppState::Native(app) => {
                app.offline("listing", reason);
                Ok(true)
            }
            _ => Ok(false),
        }
    }
    /// The request never reached a service. The application shows its own offline state.
    pub fn http_failed(&mut self, id: u64, tag: &str, reason: &str) -> Result<(), String> {
        match &mut self.windows.get_mut(&id).ok_or("window not found")?.state {
            AppState::Native(app) => {
                app.offline(tag, reason);
                Ok(())
            }
            _ => Err("window is not a native application".into()),
        }
    }
    /// Deliver a finished command to the terminal that asked for it.
    pub fn terminal_output(&mut self, id: u64, entry: TerminalEntry) -> Result<(), String> {
        self.deliver_to_terminal(id, Some(entry))
    }
    /// `clear` wipes the frame only: the cwd lives on the machine and recall history is
    /// a separate field, so neither is touched.
    pub fn terminal_clear(&mut self, id: u64) -> Result<(), String> {
        self.deliver_to_terminal(id, None)
    }
    /// `None` clears the frame. Every terminal's pending prompt is refreshed, not just
    /// this one: the cwd belongs to the machine, so a stale prompt elsewhere would lie.
    fn deliver_to_terminal(&mut self, id: u64, entry: Option<TerminalEntry>) -> Result<(), String> {
        match self.windows.get(&id).map(|w| &w.state) {
            Some(AppState::Terminal { .. }) => {}
            Some(_) => return Err("window is not a terminal".into()),
            None => return Err("window not found".into()),
        }
        let next = self.prompt_line().to_owned();
        for window in self.windows.values_mut() {
            let target = window.id == id;
            if let AppState::Terminal {
                transcript,
                prompt,
                scroll,
                ..
            } = &mut window.state
            {
                prompt.clone_from(&next);
                if !target {
                    continue;
                }
                // New output pins the view to the tail, so what just ran is on screen.
                *scroll = 0;
                match &entry {
                    Some(entry) => {
                        transcript.push(entry.clone());
                        let over = transcript.len().saturating_sub(TRANSCRIPT_LIMIT);
                        transcript.drain(..over);
                    }
                    None => transcript.clear(),
                }
            }
        }
        Ok(())
    }
    /// A listing that arrived as bare names: everything the machine could say about it
    /// is that these entries are there. Kept so callers that have nothing else to give
    /// stay honest about it.
    pub fn directory_loaded(
        &mut self,
        id: u64,
        tab: usize,
        values: Vec<String>,
    ) -> Result<(), String> {
        self.directory_listed(id, tab, values.iter().map(FileRow::named).collect())
    }
    /// A listing with the metadata the machine really reported for each entry. This is
    /// the path `AppEffect::ListDirectory` comes back on, so the size, the date and the
    /// kind a file manager draws are the filesystem's own answer.
    pub fn directory_listed(
        &mut self,
        id: u64,
        tab: usize,
        mut rows: Vec<FileRow>,
    ) -> Result<(), String> {
        rows.sort_by(|a, b| a.entry.cmp(&b.entry));
        let values: Vec<String> = rows.iter().map(|r| r.entry.clone()).collect();
        let (starred, recents) = (self.starred.clone(), self.recents.clone());
        match &mut self.windows.get_mut(&id).ok_or("window not found")?.state {
            AppState::Files { tabs, .. } => {
                let tab = tabs.get_mut(tab).ok_or("tab not found")?;
                let rows = match tab.scope {
                    FileScope::Folder => rows,
                    // Home: the pinned folders the listing proves exist, then what the
                    // user starred, then what they opened, each path once. The pinned
                    // folders keep the metadata the listing gave them; a starred or
                    // recent path is a path this listing says nothing about.
                    FileScope::QuickAccess => {
                        let base = tab.path.trim_end_matches('/').to_owned();
                        let mut shown: Vec<FileRow> = QUICK_ACCESS
                            .iter()
                            .filter_map(|name| {
                                let marked = format!("{name}/");
                                let row = rows.iter().find(|r| r.entry == marked)?;
                                Some(FileRow {
                                    entry: format!("{base}/{marked}"),
                                    ..row.clone()
                                })
                            })
                            .collect();
                        for entry in starred.into_iter().chain(recents) {
                            if !shown.iter().any(|s| s.name() == entry_name(&entry)) {
                                shown.push(FileRow::named(entry));
                            }
                        }
                        shown
                    }
                    FileScope::Gallery => rows
                        .into_iter()
                        .filter(|r| !r.entry.ends_with('/') && is_image(&r.entry))
                        .collect(),
                    // A listing that arrives while the tab has moved to a list belongs
                    // to the folder it left; dropping it beats overwriting the screen.
                    FileScope::Recents | FileScope::Starred => return Ok(()),
                };
                tab.set_rows(rows);
                tab.selected = None;
                // The entry being renamed may not have survived the refresh.
                tab.rename = None;
                Ok(())
            }
            AppState::Native(NativeApp::Photos(photos)) => {
                photos.listed(values);
                Ok(())
            }
            AppState::Native(app) => app.listed(values),
            _ => Err("window is not a file manager".into()),
        }
    }
}
impl DesktopState {
    /// Pure semantic projection; effects are completed separately by the caller.
    /// The semantic page of window `id`, as `page` projects the focused one.
    pub fn window_page(&self, id: u64) -> Option<cw_protocol::Page> {
        let mut view = self.clone();
        view.windows.get(&id)?;
        view.focused = Some(id);
        Some(view.page())
    }
    pub fn page(&self) -> cw_protocol::Page {
        use cw_protocol::PageElement as E;
        let mut page = cw_protocol::Page::new("Desktop");
        for window in self.windows.values() {
            page.elements.push(E::Button {
                id: format!("focus:{}", window.id),
                text: format!(
                    "{}{}",
                    if self.focused == Some(window.id) {
                        "● "
                    } else {
                        ""
                    },
                    window.title
                ),
                action: cw_protocol::PageAction {
                    method: "APP".into(),
                    url: format!("focus:{}", window.id),
                    fields: BTreeMap::new(),
                },
                style: None,
            });
        }
        if let Some(window) = self.focused.and_then(|id| self.windows.get(&id)) {
            match &window.state {
                AppState::Terminal {
                    input,
                    prompt,
                    transcript,
                    ..
                } => {
                    // One group per command, so a consumer reads boundaries and status as
                    // fields instead of parsing a screen or matching error text.
                    for (index, entry) in transcript.iter().enumerate() {
                        let mut children = vec![E::Text {
                            id: format!("terminal-entry:{index}:command"),
                            text: entry.echo(),
                        }];
                        if !entry.stdout.is_empty() {
                            children.push(E::Text {
                                id: format!("terminal-entry:{index}:stdout"),
                                text: entry.stdout.clone(),
                            });
                        }
                        if !entry.stderr.is_empty() {
                            children.push(E::Text {
                                id: format!("terminal-entry:{index}:stderr"),
                                text: entry.stderr.clone(),
                            });
                        }
                        // Always emitted, unlike the screen marker: a reader should never
                        // have to infer success from the absence of a line.
                        children.push(E::Text {
                            id: format!("terminal-entry:{index}:exit"),
                            text: entry.status(),
                        });
                        page.elements.push(E::Group {
                            id: format!("terminal-entry:{index}"),
                            children,
                        });
                    }
                    page.elements.push(E::Input {
                        id: "terminal-input".into(),
                        label: prompt_or_sigil(prompt).into(),
                        value: input.clone(),
                        placeholder: String::new(),
                    });
                }
                AppState::Editor {
                    path, text, dirty, ..
                } => {
                    page.elements.push(E::Heading {
                        id: "editor-path".into(),
                        text: format!("{path}{}", if *dirty { " *" } else { "" }),
                        level: 2,
                    });
                    page.elements.push(E::Input {
                        id: "editor-text".into(),
                        label: "Document".into(),
                        value: text.clone(),
                        placeholder: String::new(),
                    });
                }
                AppState::Files { tabs, active } => {
                    let action = |url: &str| cw_protocol::PageAction {
                        method: "APP".into(),
                        url: url.into(),
                        fields: BTreeMap::new(),
                    };
                    for (index, tab) in tabs.iter().enumerate() {
                        page.elements.push(E::Button {
                            id: format!("files-tab:{index}"),
                            text: format!(
                                "{}{}",
                                if index == *active { "\u{25cf} " } else { "" },
                                tab.name()
                            ),
                            action: action(&format!("files-tab:{index}")),
                            style: None,
                        });
                    }
                    page.elements.push(E::Button {
                        id: "files-newtab".into(),
                        text: "New tab".into(),
                        action: action("files-newtab"),
                        style: None,
                    });
                    let Some(tab) = tabs.get(*active) else {
                        return page;
                    };
                    page.elements.push(E::Heading {
                        id: "files-path".into(),
                        text: tab.path.clone(),
                        level: 2,
                    });
                    for (id, text, enabled) in [
                        ("files-back", "Back", tab.can_go_back()),
                        ("files-forward", "Forward", tab.can_go_forward()),
                        ("files-up", "Up", !tab.path.trim_matches('/').is_empty()),
                        ("files-reload", "Reload", true),
                        ("files-view", "View", true),
                        ("files-sort:name", "Sort by name", true),
                        ("files-sort:kind", "Sort by kind", true),
                    ] {
                        if enabled {
                            page.elements.push(E::Button {
                                id: id.into(),
                                text: text.into(),
                                action: action(id),
                                style: None,
                            });
                        }
                    }
                    page.elements.push(E::Input {
                        id: "files-search".into(),
                        label: "Search this folder".into(),
                        value: tab.query.clone(),
                        placeholder: String::new(),
                    });
                    // Screen order, so `open:<i>` means the same thing here as it does
                    // to a pointer: the row a reader counts is the row a click hits.
                    for (row, index) in tab.display().into_iter().enumerate() {
                        let entry = &tab.entries[index];
                        page.elements.push(E::Button {
                            id: format!("open:{row}"),
                            text: format!(
                                "{}{entry}",
                                if tab.selected == Some(index) {
                                    "\u{25cf} "
                                } else {
                                    ""
                                }
                            ),
                            action: action(entry),
                            style: None,
                        });
                    }
                }
                AppState::Browser { address } => page.elements.push(E::Input {
                    id: "browser-address".into(),
                    label: "Address".into(),
                    value: address.clone(),
                    placeholder: "https://".into(),
                }),
                AppState::Native(app) => app.page(&mut page),
            }
        }
        page
    }
    /// Prompt for the line being typed. Falls back to a bare sigil only before the
    /// machine has said who and where we are.
    pub fn prompt_line(&self) -> &str {
        prompt_or_sigil(&self.prompt)
    }
    /// Windows on the desktop currently on screen. Another workspace's windows are not
    /// minimised, they are simply elsewhere.
    pub fn on_this_workspace(&self, id: u64) -> bool {
        self.windows
            .get(&id)
            .is_some_and(|w| w.workspace == self.workspace)
    }
    pub fn workspace_count(&self) -> u32 {
        self.workspaces.max(1)
    }
    /// Add a desktop and move to it. Returns the new index.
    /// Panels that are drop-down menus rather than flyouts: choosing an entry in one
    /// closes it, as a menu does.
    pub const MENUS: &'static [&'static str] = &["file", "edit", "view", "format", "app-menu"];
    pub fn close_menu(&mut self) {
        if self
            .panel
            .as_deref()
            .is_some_and(|panel| Self::MENUS.contains(&panel))
        {
            self.panel = None;
        }
    }
    pub fn add_workspace(&mut self) -> Result<u32, String> {
        if self.workspace_count() >= WORKSPACE_LIMIT {
            return Err(format!("at most {WORKSPACE_LIMIT} desktops"));
        }
        self.workspaces = self.workspace_count() + 1;
        self.workspace = self.workspaces - 1;
        self.focused = None;
        Ok(self.workspace)
    }
    pub fn switch_workspace(&mut self, index: u32) -> Result<(), String> {
        if index >= self.workspace_count() {
            return Err("no such desktop".into());
        }
        self.workspace = index;
        // Focus follows the screen: a window you cannot see cannot be focused.
        self.focused = self
            .ordered_windows()
            .into_iter()
            .rev()
            .find(|id| self.on_this_workspace(*id) && !self.windows[id].minimized);
        Ok(())
    }
    /// Remove a desktop, moving its windows to the one before it so nothing is lost.
    pub fn close_workspace(&mut self, index: u32) -> Result<(), String> {
        if self.workspace_count() <= 1 {
            return Err("the last desktop cannot be closed".into());
        }
        if index >= self.workspace_count() {
            return Err("no such desktop".into());
        }
        for window in self.windows.values_mut() {
            if window.workspace == index {
                window.workspace = index.saturating_sub(1);
            } else if window.workspace > index {
                window.workspace -= 1;
            }
        }
        self.workspaces = self.workspace_count() - 1;
        self.switch_workspace(self.workspace.min(self.workspaces - 1))
    }
    /// Move a window to another desktop and follow it there.
    pub fn move_to_workspace(&mut self, id: u64, index: u32) -> Result<(), String> {
        if index >= self.workspace_count() {
            return Err("no such desktop".into());
        }
        self.windows
            .get_mut(&id)
            .ok_or("window not found")?
            .workspace = index;
        self.switch_workspace(index)
    }
    /// Save the page a browser window is showing. Saving the same URL twice is one
    /// bookmark, as every browser does.
    pub fn bookmark(&mut self, title: &str, url: &str) -> Result<(), String> {
        if url.is_empty() {
            return Err("an empty page cannot be saved".into());
        }
        if let Some(existing) = self.bookmarks.iter_mut().find(|b| b.url == url) {
            existing.title = title.into();
            return Ok(());
        }
        if self.bookmarks.len() >= BOOKMARK_LIMIT {
            return Err(format!("at most {BOOKMARK_LIMIT} bookmarks"));
        }
        self.bookmarks.push(Bookmark {
            title: title.into(),
            url: url.into(),
        });
        Ok(())
    }
    pub fn bookmarked(&self, url: &str) -> bool {
        self.bookmarks.iter().any(|b| b.url == url)
    }
    pub fn remove_bookmark(&mut self, url: &str) -> Result<(), String> {
        let before = self.bookmarks.len();
        self.bookmarks.retain(|b| b.url != url);
        if self.bookmarks.len() == before {
            return Err("no such bookmark".into());
        }
        Ok(())
    }
    /// Record a file the browser really wrote to the machine.
    pub fn record_download(&mut self, name: &str, path: &str, url: &str, bytes: u64) {
        self.downloads.retain(|d| d.path != path);
        self.downloads.insert(
            0,
            Download {
                name: name.into(),
                path: path.into(),
                url: url.into(),
                bytes,
            },
        );
        self.downloads.truncate(DOWNLOAD_LIMIT);
    }
    /// Post a notice. Applications call this when they *see* something; the shell that
    /// draws the shade never invents one.
    pub fn notify(&mut self, app: &str, title: &str, body: &str, action: Option<String>) {
        let time_us = self.clock_us;
        self.notifications
            .retain(|n| n.title != title || n.app != app);
        self.notifications.insert(
            0,
            Notice {
                app: app.into(),
                title: title.into(),
                body: body.into(),
                time_us,
                action,
                seen: false,
            },
        );
        self.notifications.truncate(NOTICE_LIMIT);
    }
    pub fn unseen_notices(&self) -> usize {
        self.notifications.iter().filter(|n| !n.seen).count()
    }
    pub fn mark_notices_seen(&mut self) {
        for notice in &mut self.notifications {
            notice.seen = true;
        }
    }
    pub fn home_folder(&self) -> String {
        if self.home.is_empty() {
            "/".into()
        } else {
            normalize_folder(&self.home)
        }
    }
    /// Active folder view of the focused window.
    fn focused_files(&mut self) -> Result<(u64, &mut Vec<FileTab>, &mut usize), String> {
        let id = self.focused.ok_or("no focused window")?;
        let window = self.windows.get_mut(&id).ok_or("window not found")?;
        match &mut window.state {
            AppState::Files { tabs, active } => Ok((id, tabs, active)),
            _ => Err("not a file manager".into()),
        }
    }
    pub(super) fn focused_tab(&self) -> Result<&FileTab, String> {
        self.focused
            .and_then(|id| self.windows.get(&id))
            .ok_or("no focused window")?
            .state
            .file_tab()
            .ok_or_else(|| "not a file manager".into())
    }
    pub(super) fn focused_tab_mut(&mut self) -> Result<&mut FileTab, String> {
        let id = self.focused.ok_or("no focused window")?;
        self.windows
            .get_mut(&id)
            .ok_or("window not found")?
            .state
            .file_tab_mut()
            .ok_or_else(|| "not a file manager".into())
    }
    /// Where this machine files what a file manager deletes. Deleting is a move here,
    /// never a hard remove, so a snapshot still holds what was thrown away.
    pub fn trash_folder(&self) -> String {
        format!(
            "{}/.local/share/Trash/files",
            self.home_folder().trim_end_matches('/')
        )
    }
    /// Remember a document that was opened, newest first and without duplicates.
    pub(super) fn remember(&mut self, path: &str) {
        self.recents.retain(|p| p != path);
        self.recents.insert(0, path.to_owned());
        self.recents.truncate(RECENT_LIMIT);
    }
    /// Whether `path` (with or without a trailing `/`) is starred.
    pub fn is_starred(&self, path: &str) -> bool {
        let path = entry_name(path);
        self.starred.iter().any(|s| entry_name(s) == path)
    }
    /// Star or unstar an absolute entry (folders end in `/`). Returns the new state.
    fn toggle_star(&mut self, entry: String) -> bool {
        if self.is_starred(&entry) {
            let path = entry_name(&entry).to_owned();
            self.starred.retain(|s| entry_name(s) != path);
            return false;
        }
        self.starred.insert(0, entry);
        self.starred.truncate(STAR_LIMIT);
        true
    }
    /// The absolute entry (folders with `/`) at display row `row` of the active tab,
    /// or the selection when `row` is `None`.
    fn tab_entry(&self, row: Option<usize>) -> Result<String, String> {
        let tab = self.focused_tab()?;
        let index = match row {
            Some(row) => *tab.display().get(row).ok_or("entry not found")?,
            None => tab.selected.ok_or("nothing is selected")?,
        };
        let entry = tab.entries.get(index).ok_or("entry not found")?;
        let name = entry_name(entry);
        let path = if tab.scope.absolute() {
            name.to_owned()
        } else {
            tab.child(name)
        };
        Ok(if entry.ends_with('/') {
            format!("{path}/")
        } else {
            path
        })
    }
    /// Show a list the desktop keeps (Recents, Starred) in the active tab. The folder
    /// the tab was on stays its `path`, so `files-browse` goes back to it.
    fn show_list(&mut self, scope: FileScope) -> Result<Vec<AppEffect>, String> {
        let entries = match scope {
            FileScope::Recents => self.recents.clone(),
            FileScope::Starred => self.starred.clone(),
            _ => return Err("not a list".into()),
        };
        let tab = self.focused_tab_mut()?;
        tab.scope = scope;
        tab.set_names(entries);
        tab.selected = None;
        tab.query.clear();
        tab.stop_editing();
        Ok(vec![])
    }
    /// Show a view built from a real listing: Explorer's Home from the home folder,
    /// its Gallery from Pictures. `directory_loaded` shapes the listing when it lands.
    fn show_listed(&mut self, scope: FileScope, path: String) -> Result<Vec<AppEffect>, String> {
        let (id, tabs, active) = self.focused_files()?;
        let index = *active;
        let tab = tabs.get_mut(index).ok_or("tab not found")?;
        tab.scope = scope;
        tab.path = path.clone();
        tab.set_rows(vec![]);
        tab.selected = None;
        tab.query.clear();
        tab.stop_editing();
        Ok(vec![AppEffect::ListDirectory {
            window: id,
            tab: index,
            path,
        }])
    }
    /// Paste the clipboard into the active folder. A copy that would collide is given a
    /// `(copy)` name from the listing the tab already has; a move that would collide is
    /// refused, because a move that renames itself is a move you did not ask for.
    fn paste(&mut self) -> Result<Vec<AppEffect>, String> {
        let Some(clipboard) = self.clipboard.clone() else {
            return Err("the clipboard is empty".into());
        };
        if clipboard.paths.is_empty() {
            return Err("the clipboard holds a picture, not files".into());
        }
        let (id, tabs, active) = self.focused_files()?;
        let index = *active;
        let tab = tabs.get_mut(index).ok_or("tab not found")?;
        if tab.scope != FileScope::Folder {
            return Err("this view is not a folder".into());
        }
        let folder = tab.path.clone();
        let mut effects = vec![];
        for from in &clipboard.paths {
            let name = entry_name(from.rsplit('/').next().unwrap_or_default());
            if name.is_empty() {
                return Err("clipboard entry has no name".into());
            }
            let (stem, extension) = match name.rsplit_once('.') {
                Some((stem, ext)) if !stem.is_empty() => (stem, format!(".{ext}")),
                _ => (name, String::new()),
            };
            let target = if clipboard.cut {
                if tab.entries.iter().any(|e| entry_name(e) == name) {
                    return Err("a file of that name is already here".into());
                }
                tab.child(name)
            } else {
                tab.child(&free_name(
                    &tab.entries,
                    &format!("{stem} (copy)"),
                    &extension,
                    " ",
                ))
            };
            if &target == from {
                return Err("that is already here".into());
            }
            effects.push(if clipboard.cut {
                AppEffect::MovePath {
                    window: id,
                    from: from.clone(),
                    to: target,
                }
            } else {
                AppEffect::CopyPath {
                    window: id,
                    from: from.clone(),
                    to: target,
                }
            });
        }
        // A cut is consumed by its paste; a copy stays on the clipboard.
        if clipboard.cut {
            self.clipboard = None;
        }
        effects.push(AppEffect::ListDirectory {
            window: id,
            tab: index,
            path: folder,
        });
        Ok(effects)
    }
    /// Commit the name being typed over the entry the rename started from.
    fn commit_rename(&mut self) -> Result<Vec<AppEffect>, String> {
        let (id, tabs, active) = self.focused_files()?;
        let index = *active;
        let tab = tabs.get_mut(index).ok_or("tab not found")?;
        let rename = tab.rename.take().ok_or("nothing is being renamed")?;
        let name = rename.name.trim();
        if name.is_empty()
            || name.contains(['/', '\\'])
            || name == "."
            || name == ".."
            || name.chars().count() > FIELD_LIMIT
        {
            return Err("that is not a usable file name".into());
        }
        if name == rename.from {
            return Ok(vec![]);
        }
        if tab.entries.iter().any(|e| entry_name(e) == name) {
            return Err("a file of that name is already here".into());
        }
        let (from, to) = (tab.child(&rename.from), tab.child(name));
        let path = tab.path.clone();
        Ok(vec![
            AppEffect::MovePath {
                window: id,
                from,
                to,
            },
            AppEffect::ListDirectory {
                window: id,
                tab: index,
                path,
            },
        ])
    }
    /// Show `path` in the active tab. `record` appends to the tab's history; the
    /// Back and Forward commands move within it instead.
    fn show_folder(&mut self, path: &str, record: bool) -> Result<Vec<AppEffect>, String> {
        let path = normalize_folder(path);
        let (id, tabs, active) = self.focused_files()?;
        let index = *active;
        let tab = tabs.get_mut(index).ok_or("tab not found")?;
        tab.path = path.clone();
        tab.set_rows(vec![]);
        tab.selected = None;
        // Moving folders leaves Recents and every text field: a filter typed for one
        // folder must not silently hide the contents of the next.
        tab.scope = FileScope::Folder;
        tab.query.clear();
        tab.stop_editing();
        if record {
            tab.history.truncate(tab.position + 1);
            if tab.history.last() != Some(&path) {
                tab.history.push(path.clone());
                tab.position = tab.history.len() - 1;
            }
            // Bound retained history so a long session cannot grow a snapshot without limit.
            if tab.history.len() > HISTORY_LIMIT {
                let excess = tab.history.len() - HISTORY_LIMIT;
                tab.history.drain(..excess);
                tab.position = tab.position.saturating_sub(excess);
            }
        }
        Ok(vec![AppEffect::ListDirectory {
            window: id,
            tab: index,
            path,
        }])
    }
    /// File manager commands. Every one of these is reachable from a real control.
    fn files_command(&mut self, command: &str) -> Result<Vec<AppEffect>, String> {
        match command {
            "up" | "parent" => {
                let path = self.focused_tab()?.path.clone();
                self.show_folder(parent_folder(&path), true)
            }
            "root" => self.show_folder("/", true),
            "home" => {
                let home = self.home_folder();
                self.show_folder(&home, true)
            }
            "reload" => {
                let tab = self.focused_tab()?;
                let (scope, path) = (tab.scope, tab.path.clone());
                match scope {
                    FileScope::Folder => self.show_folder(&path, false),
                    FileScope::Recents | FileScope::Starred => self.show_list(scope),
                    FileScope::QuickAccess | FileScope::Gallery => self.show_listed(scope, path),
                }
            }
            "starred" => self.show_list(FileScope::Starred),
            "hidden" => {
                let tab = self.focused_tab_mut()?;
                tab.show_hidden = !tab.show_hidden;
                Ok(vec![])
            }
            // Explorer's Home and Gallery: views over real listings of the home folder
            // and of Pictures, never a picture of what those folders might hold.
            "quick-access" => {
                let home = self.home_folder();
                self.show_listed(FileScope::QuickAccess, home)
            }
            "gallery" => {
                let pictures = format!("{}/Pictures", self.home_folder().trim_end_matches('/'));
                self.show_listed(FileScope::Gallery, pictures)
            }
            "trash" => {
                let trash = self.trash_folder();
                self.show_folder(&trash, true)
            }
            "star" => {
                let entry = self.tab_entry(None)?;
                self.toggle_star(entry);
                self.refresh_starred_view()
            }
            "back" | "forward" => {
                let (id, tabs, active) = self.focused_files()?;
                let index = *active;
                let tab = tabs.get_mut(index).ok_or("tab not found")?;
                let position = if command == "back" {
                    tab.position.checked_sub(1).ok_or("no earlier folder")?
                } else {
                    let next = tab.position + 1;
                    if next >= tab.history.len() {
                        return Err("no later folder".into());
                    }
                    next
                };
                tab.position = position;
                tab.path = tab.history[position].clone();
                tab.set_rows(vec![]);
                tab.selected = None;
                tab.scope = FileScope::Folder;
                tab.query.clear();
                tab.stop_editing();
                let path = tab.path.clone();
                Ok(vec![AppEffect::ListDirectory {
                    window: id,
                    tab: index,
                    path,
                }])
            }
            "newtab" => {
                let home = self.home_folder();
                let (id, tabs, active) = self.focused_files()?;
                if tabs.len() >= TAB_LIMIT {
                    return Err("tab limit reached".into());
                }
                let view = tabs.get(*active).map_or(FileView::List, |t| t.view);
                tabs.push(FileTab {
                    view,
                    ..FileTab::new(&home)
                });
                *active = tabs.len() - 1;
                let index = *active;
                Ok(vec![AppEffect::ListDirectory {
                    window: id,
                    tab: index,
                    path: home,
                }])
            }
            "open" => self.open_selection(),
            // List and grid show the same rows in the same order, so nothing moves
            // under a pointer when the layout changes.
            "view" => {
                let tab = self.focused_tab_mut()?;
                tab.view = match tab.view {
                    FileView::List => FileView::Grid,
                    FileView::Grid => FileView::List,
                };
                Ok(vec![])
            }
            "search" => {
                let tab = self.focused_tab_mut()?;
                tab.rename = None;
                tab.searching = true;
                Ok(vec![])
            }
            "search-clear" => {
                let tab = self.focused_tab_mut()?;
                if tab.query.is_empty() && !tab.searching {
                    return Err("no search to clear".into());
                }
                tab.query.clear();
                tab.searching = false;
                Ok(vec![])
            }
            "recents" => self.show_list(FileScope::Recents),
            "browse" => {
                let path = self.focused_tab()?.path.clone();
                self.show_folder(&path, false)
            }
            "new-folder" | "new-file" => {
                let folder = command == "new-folder";
                let (id, tabs, active) = self.focused_files()?;
                let index = *active;
                let tab = tabs.get_mut(index).ok_or("tab not found")?;
                if tab.scope != FileScope::Folder {
                    return Err("this view is not a folder".into());
                }
                let path = tab.path.clone();
                let name = if folder {
                    free_name(&tab.entries, "New folder", "", " ")
                } else {
                    free_name(&tab.entries, "Untitled", ".txt", " ")
                };
                let target = tab.child(&name);
                let create = if folder {
                    AppEffect::CreateDirectory {
                        window: id,
                        path: target,
                    }
                } else {
                    AppEffect::CreateFile {
                        window: id,
                        path: target,
                    }
                };
                // Relist rather than assume: what the folder holds afterwards is the
                // machine's answer, not this tab's guess.
                Ok(vec![
                    create,
                    AppEffect::ListDirectory {
                        window: id,
                        tab: index,
                        path,
                    },
                ])
            }
            "cut" | "copy" => {
                let tab = self.focused_tab()?;
                let path = tab.selected_path().ok_or("nothing is selected")?;
                self.clipboard = Some(Clipboard::new(vec![path], command == "cut"));
                Ok(vec![])
            }
            "paste" => self.paste(),
            "rename" => {
                let tab = self.focused_tab_mut()?;
                if tab.scope != FileScope::Folder {
                    return Err("this view is not a folder".into());
                }
                let from = entry_name(tab.selection().ok_or("nothing is selected")?).to_owned();
                tab.searching = false;
                tab.rename = Some(Rename {
                    name: from.clone(),
                    from,
                });
                Ok(vec![])
            }
            // "Delete" is what the control has always been called; "Move to Trash" is
            // what it does. Both spellings reach the same effect, so the action an
            // observer reads can be as honest as the button.
            "delete" | "move-to-trash" => {
                let trash = self.trash_folder();
                let (id, tabs, active) = self.focused_files()?;
                let index = *active;
                let tab = tabs.get_mut(index).ok_or("tab not found")?;
                if tab.scope != FileScope::Folder {
                    return Err("this view is not a folder".into());
                }
                let path = tab.selected_path().ok_or("nothing is selected")?;
                if path == trash || path.starts_with(&format!("{trash}/")) {
                    return Err("that is already in the trash".into());
                }
                let folder = tab.path.clone();
                Ok(vec![
                    AppEffect::TrashPath {
                        window: id,
                        path,
                        trash,
                    },
                    AppEffect::ListDirectory {
                        window: id,
                        tab: index,
                        path: folder,
                    },
                ])
            }
            // Put back what is selected in the Trash. Only the Trash offers it: a
            // Restore anywhere else has no record to read, and a control that could
            // only refuse does not belong on the screen.
            "restore" => self.restore_row(None),
            // Really destroy what is in the trash. Refused outside it, so the one
            // command that loses data cannot be reached from a folder of live files.
            "empty-trash" => {
                let trash = self.trash_folder();
                let (id, tabs, active) = self.focused_files()?;
                let index = *active;
                let tab = tabs.get_mut(index).ok_or("tab not found")?;
                if !tab.in_trash(&trash) {
                    return Err("this view is not the trash".into());
                }
                if tab.entries.is_empty() {
                    return Err("the trash is already empty".into());
                }
                Ok(vec![
                    AppEffect::EmptyTrash { window: id },
                    AppEffect::ListDirectory {
                        window: id,
                        tab: index,
                        path: trash,
                    },
                ])
            }
            rest => {
                if let Some(row) = rest.strip_prefix("restore:") {
                    let row: usize = row.parse().map_err(|_| "invalid entry")?;
                    return self.restore_row(Some(row));
                }
                if let Some(row) = rest.strip_prefix("star:") {
                    let row: usize = row.parse().map_err(|_| "invalid entry")?;
                    let entry = self.tab_entry(Some(row))?;
                    self.toggle_star(entry);
                    return self.refresh_starred_view();
                }
                if let Some(key) = rest.strip_prefix("sort:") {
                    let key = SortKey::parse(key).ok_or("unknown sort key")?;
                    let tab = self.focused_tab_mut()?;
                    // The same key again flips the direction; a different one starts
                    // ascending, which is what every file manager does.
                    tab.descending = tab.sort == key && !tab.descending;
                    tab.sort = key;
                    return Ok(vec![]);
                }
                if let Some(index) = rest.strip_prefix("tab:") {
                    let index: usize = index.parse().map_err(|_| "invalid tab")?;
                    let (id, tabs, active) = self.focused_files()?;
                    if index >= tabs.len() {
                        return Err("tab not found".into());
                    }
                    *active = index;
                    let path = tabs[index].path.clone();
                    // Re-list on switch: the folder may have changed while hidden.
                    return Ok(vec![AppEffect::ListDirectory {
                        window: id,
                        tab: index,
                        path,
                    }]);
                }
                if let Some(index) = rest.strip_prefix("closetab:") {
                    let index: usize = index.parse().map_err(|_| "invalid tab")?;
                    let (id, tabs, active) = self.focused_files()?;
                    if index >= tabs.len() {
                        return Err("tab not found".into());
                    }
                    if tabs.len() == 1 {
                        // Closing the last tab closes the window, as every shell does.
                        return self.close(id).map(|()| vec![]);
                    }
                    tabs.remove(index);
                    if *active > index {
                        *active -= 1;
                    }
                    *active = (*active).min(tabs.len() - 1);
                    return Ok(vec![]);
                }
                if let Some(path) = rest.strip_prefix("location:") {
                    let path = path.to_owned();
                    return self.show_folder(&path, true);
                }
                Err(format!("unknown file manager command {command}"))
            }
        }
    }
    /// Put one trashed thing back. `row` is a position on screen, or `None` for the
    /// selection. The machine reads the `.trashinfo` record and decides where it goes;
    /// the file manager only says which row, and then re-lists the Trash so what is on
    /// screen afterwards is the trash as it now is.
    fn restore_row(&mut self, row: Option<usize>) -> Result<Vec<AppEffect>, String> {
        let trash = self.trash_folder();
        if !self.focused_tab()?.in_trash(&trash) {
            return Err("this view is not the trash".into());
        }
        let path = self.tab_entry(row)?;
        let (id, tabs, active) = self.focused_files()?;
        let index = *active;
        let _ = tabs.get(index).ok_or("tab not found")?;
        Ok(vec![
            AppEffect::RestorePath {
                window: id,
                path: path.trim_end_matches('/').to_owned(),
            },
            AppEffect::ListDirectory {
                window: id,
                tab: index,
                path: trash,
            },
        ])
    }
    /// A star changed: a tab showing the Starred list shows the list as it now is.
    fn refresh_starred_view(&mut self) -> Result<Vec<AppEffect>, String> {
        if self.focused_tab()?.scope == FileScope::Starred {
            let starred = self.starred.clone();
            let tab = self.focused_tab_mut()?;
            let kept = tab.selection().cloned();
            tab.set_names(starred);
            tab.selected = kept.and_then(|k| tab.entries.iter().position(|e| *e == k));
        }
        Ok(vec![])
    }
    /// Open whatever the active tab has selected: folders in place, documents in an
    /// editor. A document opened this way is what makes the Recents list real.
    fn open_selection(&mut self) -> Result<Vec<AppEffect>, String> {
        let tab = self.focused_tab()?;
        let entry = tab.selection().ok_or("nothing is selected")?.clone();
        let target = tab.selected_path().ok_or("nothing is selected")?;
        if entry.ends_with('/') {
            self.show_folder(&target, true)
        } else {
            self.remember(&target);
            self.launch(opener(&entry), &target)
                .map(|(_, effects)| effects)
        }
    }
    /// A single click selects, focuses and switches; it never opens anything.
    pub fn click(&mut self, target: &str) -> Result<Vec<AppEffect>, String> {
        if let Some(id) = target.strip_prefix("focus:") {
            self.focus(id.parse().map_err(|_| "invalid window ID")?)?;
            return Ok(vec![]);
        }
        if target == "editor-save" {
            return self.key("Ctrl+s");
        }
        // Named rather than pointed at, a scroll bar pages forward by one thumb.
        if let Some(bar) = ScrollBar::parse(target) {
            let window = self
                .focused
                .and_then(|id| self.windows.get_mut(&id))
                .ok_or("no focused window")?;
            let offset = window.scroll.offset(bar.pane).min(bar.max);
            let page = (i64::from(bar.max) * i64::from(bar.thumb)
                / i64::from(bar.track.saturating_sub(bar.thumb).max(1)))
                as i32;
            window
                .scroll
                .set(bar.pane, (offset + page.max(1)).min(bar.max));
            return Ok(vec![]);
        }
        if let Some(window) = self.focused.and_then(|id| self.windows.get_mut(&id)) {
            if let AppState::Native(app) = &mut window.state {
                let (id, clock) = (window.id, self.clock_us);
                let effects = app.click(id, target, clock)?;
                return self.native_effects(effects);
            }
        }
        if let Some(command) = target.strip_prefix("files-") {
            return self.files_command(command);
        }
        if let Some(row) = target.strip_prefix("open:") {
            // `row` is a position on screen. Resolving it through `display()` is what
            // keeps a sorted or filtered view honest: the click selects the entry the
            // pointer was over, not the one at that index of the raw listing.
            let row: usize = row.parse().map_err(|_| "invalid entry")?;
            let (_, tabs, active) = self.focused_files()?;
            let active = *active;
            let tab = tabs.get_mut(active).ok_or("tab not found")?;
            let index = *tab.display().get(row).ok_or("entry not found")?;
            tab.selected = Some(index);
            tab.stop_editing();
            return Ok(vec![]);
        }
        if let Some(lines) = target.strip_prefix("terminal-scroll:") {
            let lines: usize = lines.parse().map_err(|_| "invalid scroll position")?;
            let id = self.focused.ok_or("no focused window")?;
            let state = &mut self.windows.get_mut(&id).ok_or("window not found")?.state;
            let AppState::Terminal { scroll, .. } = state else {
                return Err("window is not a terminal".into());
            };
            *scroll = lines.min(SCROLL_LIMIT);
            return Ok(vec![]);
        }
        let state = &self
            .focused
            .and_then(|id| self.windows.get(&id))
            .ok_or("no focused window")?
            .state;
        match (target, state) {
            (_, AppState::Editor { .. }) if target.starts_with("editor-text") => Ok(vec![]),
            ("terminal-input" | "terminal-line", AppState::Terminal { .. })
            | ("browser-address", AppState::Browser { .. }) => Ok(vec![]),
            _ => Err("interaction does not belong to focused application".into()),
        }
    }
    /// Click carrying the offset inside the control that was hit, so a text view can
    /// place its caret where the pointer actually landed.
    pub fn click_at(&mut self, target: &str, dx: i32, dy: i32) -> Result<Vec<AppEffect>, String> {
        let id = self.focused.ok_or("no focused window")?;
        // A click on a pane's scroll bar is a press and release where it landed: the
        // thumb jumps there, whatever the window shows.
        if ScrollBar::parse(target).is_some() {
            let window = self.windows.get_mut(&id).ok_or("window not found")?;
            window.scroll.drag(target, PointerPhase::Down, dx, dy)?;
            window.scroll.drag(target, PointerPhase::Up, dx, dy)?;
            return Ok(vec![]);
        }
        // A click on a drag surface is a press and release at one point: a dot from a
        // brush, a fill, a slider set to where it was clicked.
        let clock = self.clock_us;
        let modifiers = self.pointer_modifiers;
        if let Some(AppState::Native(app)) = self
            .windows
            .get_mut(&id)
            .map(|w| &mut w.state)
            .filter(|_| !target.starts_with("focus:"))
        {
            app.pointer_modifiers(modifiers);
            if app.drags(target) {
                let mut effects = app.pointer(id, target, PointerPhase::Down, dx, dy, clock)?;
                effects.extend(app.pointer(id, target, PointerPhase::Up, dx, dy, clock)?);
                return self.native_effects(effects);
            }
            let effects = app.click_at(id, target, dx, dy, clock)?;
            return self.native_effects(effects);
        }
        // `editor-text:<first row>[:<columns>]`: the scroll position, and the wrap width
        // when the view soft-wraps, both as the view painted them.
        let grid = target.strip_prefix("editor-text").map(|rest| {
            let mut parts = rest.trim_start_matches(':').split(':');
            let first = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
            let columns = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
            (first, columns)
        });
        let placed = match (self.windows.get_mut(&id).map(|w| &mut w.state), grid) {
            (Some(AppState::Editor { text, cursor, .. }), Some((first, columns))) => {
                *cursor = caret_for_point_wrapped(text, first, columns, dx, dy);
                true
            }
            // `terminal-line` is the prompt line alone, so `dx` is measured from the
            // first character of the input; the surrounding `terminal-input` body only
            // focuses the shell and leaves the caret where it was.
            (Some(AppState::Terminal { input, cursor, .. }), None) if target == "terminal-line" => {
                *cursor = caret_for_column(input, dx);
                true
            }
            _ => false,
        };
        if placed {
            return Ok(vec![]);
        }
        self.click(target)
    }
    /// A double click opens the thing that was clicked. Anything without a distinct
    /// double-click meaning falls back to the single-click behaviour.
    pub fn activate(&mut self, target: &str) -> Result<Vec<AppEffect>, String> {
        let clock = self.clock_us;
        if let Some(id) = self.focused {
            if let Some(AppState::Native(app)) = self
                .windows
                .get_mut(&id)
                .map(|w| &mut w.state)
                .filter(|_| !target.starts_with("focus:"))
            {
                let effects = app.activate(id, target, clock)?;
                return self.native_effects(effects);
            }
        }
        if target.starts_with("open:") {
            self.click(target)?;
            return self.open_selection();
        }
        self.click(target)
    }
}
/// Collapse a folder path to the canonical form the rest of the system stores.
fn normalize_folder(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".into()
    } else {
        trimmed.into()
    }
}
