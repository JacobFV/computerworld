//! Native applications that ship with the simulator. Each one draws itself in the host
//! platform's idiom and reads real data — a service over `AppEffect::Http`, the machine's
//! filesystem, or the world clock. None of them invents content to fill a screen.
use crate::desktop_scene::{DesktopTheme, Painter};
use crate::AppEffect;
use serde::{Deserialize, Serialize};

pub mod look;

pub mod calculator;
pub mod calendar;
pub mod chat;
pub mod clock;
pub mod code;
pub mod contacts;
pub mod database;
pub mod docs;
pub mod freecad;
pub mod imaging;
pub mod kicad;
pub mod mail;
pub mod maps;
pub mod music;
pub mod notes;
pub mod photos;
pub mod settings;
pub mod sheet;
pub mod weather;

/// What an application may read about the machine while it draws itself. Time and the
/// system switches come from here so no app ever reaches for a host clock or guesses.
/// Split a launch argument into the service it talks to and what it was opened *on*:
/// `<service>#<thing>`, a bare service URL, or a bare thing on the default service.
pub fn launch_target(argument: &str) -> (&str, &str) {
    match argument.split_once('#') {
        Some(split) => split,
        None if argument.starts_with("http://") || argument.starts_with("https://") => {
            (argument, "")
        }
        None => ("", argument),
    }
}

pub struct AppEnv<'a> {
    pub theme: DesktopTheme,
    pub width: u32,
    pub height: u32,
    pub clock_us: u64,
    pub settings: &'a crate::SystemSettings,
    /// What a Paste would take. Carried so the control can grey itself instead of
    /// painting an affordance whose only outcome is a refusal.
    pub clipboard: Option<&'a crate::Clipboard>,
    /// The installed application a Share hands things to (Messages first, then Mail),
    /// or `None` when nothing on the machine can receive one.
    pub share_to: Option<&'static str>,
    /// Places a file manager's sidebar offers, read from the machine as it is now.
    pub files: FilesEnv<'a>,
    /// The installed image editor a photo's Edit button hands the file to (Paint,
    /// Preview, GIMP…), or `None` when the machine has none.
    pub editor: Option<&'static str>,
    /// Where the pointer is over this window's content, when it is (content
    /// coordinates), so an application can show what it would pick before a click.
    pub pointer: Option<(i32, i32)>,
}
/// What a file manager may know beyond its own tab. Every field is read from the
/// machine when the frame is drawn, so a sidebar never offers a folder that is gone.
#[derive(Clone, Debug, Default)]
pub struct FilesEnv<'a> {
    /// The user's home folder; empty when the machine states none.
    pub home: &'a str,
    /// Names from `standard_folders` that really are folders in `home` right now.
    pub folders: Vec<String>,
    /// Where deleted files go (`DesktopState::trash_folder`); empty when unknown.
    pub trash: String,
    /// `DesktopState::starred`, so a row can show whether it is starred.
    pub starred: &'a [String],
}
impl FilesEnv<'_> {
    pub fn has(&self, folder: &str) -> bool {
        self.folders.iter().any(|f| f == folder)
    }
    /// Absolute path of a folder inside home.
    pub fn folder(&self, name: &str) -> String {
        format!("{}/{name}", self.home.trim_end_matches('/'))
    }
    pub fn starred(&self, path: &str) -> bool {
        let path = path.trim_end_matches('/');
        self.starred.iter().any(|s| s.trim_end_matches('/') == path)
    }
}
impl AppEnv<'_> {
    pub fn switch(&self, name: &str) -> bool {
        self.settings.flag(name).unwrap_or(false)
    }
    pub fn level(&self, name: &str) -> u8 {
        self.settings.level(name).unwrap_or(0)
    }
}

/// Where a networked application stands with its service. `Offline` is a transport
/// failure and `Denied` is a refusal the service actually sent, so the two never blur.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    Idle,
    Loading,
    Offline(String),
    Denied(String),
}
impl Status {
    /// Message to show in place of content, or `None` when content should be shown.
    pub fn notice(&self) -> Option<&str> {
        match self {
            Self::Idle => None,
            Self::Loading => Some("Loading…"),
            Self::Offline(reason) | Self::Denied(reason) => Some(reason),
        }
    }
    /// Classify a service reply. 2xx is content; everything else is state, never an error.
    pub fn from_status(status: u16, body: &str) -> Self {
        if (200..300).contains(&status) {
            return Self::Idle;
        }
        let message = serde_json::from_str::<serde_json::Value>(body)
            .ok()
            .and_then(|v| v.get("error")?.as_str().map(str::to_owned))
            .unwrap_or_else(|| format!("service returned {status}"));
        if status == 403 || status == 401 {
            Self::Denied(message)
        } else {
            Self::Offline(message)
        }
    }
}

/// Bound retained text the way the shell's own fields are bounded.
pub fn push_bounded(field: &mut String, text: &str, limit: usize) {
    for ch in text.chars().filter(|ch| !ch.is_control()) {
        if field.len() + ch.len_utf8() > limit {
            break;
        }
        field.push(ch);
    }
}

macro_rules! native_apps {
    ($($variant:ident => $module:ident),+ $(,)?) => {
        #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(tag = "app", rename_all = "snake_case")]
        pub enum NativeApp {
            $($variant($module::$variant),)+
        }
        impl NativeApp {
            pub fn kind(&self) -> &'static str {
                match self { $(Self::$variant(a) => a.kind(),)+ }
            }
            pub fn title(&self, theme: DesktopTheme) -> String {
                match self { $(Self::$variant(a) => a.title(theme),)+ }
            }
            pub fn document(&self) -> String {
                match self { $(Self::$variant(a) => a.document(),)+ }
            }
            pub fn caption(&self) -> String {
                match self { $(Self::$variant(a) => a.caption(),)+ }
            }
            pub fn modified(&self) -> bool {
                match self { $(Self::$variant(a) => a.modified(),)+ }
            }
            pub fn text(&mut self, text: &str) -> Result<(), String> {
                match self { $(Self::$variant(a) => a.text(text),)+ }
            }
            pub fn key(
                &mut self,
                window: u64,
                key: &str,
                clock_us: u64,
            ) -> Result<Vec<AppEffect>, String> {
                match self { $(Self::$variant(a) => a.key(window, key, clock_us),)+ }
            }
            pub fn click(
                &mut self,
                window: u64,
                target: &str,
                clock_us: u64,
            ) -> Result<Vec<AppEffect>, String> {
                match self { $(Self::$variant(a) => a.click(window, target, clock_us),)+ }
            }
            pub fn http(
                &mut self,
                window: u64,
                tag: &str,
                status: u16,
                body: &str,
            ) -> Result<Vec<AppEffect>, String> {
                match self { $(Self::$variant(a) => a.http(window, tag, status, body),)+ }
            }
            pub fn offline(&mut self, tag: &str, reason: &str) {
                match self { $(Self::$variant(a) => a.offline(tag, reason),)+ }
            }
            /// Decoded pixels for a file this application asked to see.
            pub fn image(
                &mut self,
                path: &str,
                width: u32,
                height: u32,
                rgba: Vec<u8>,
            ) -> Result<(), String> {
                match self {
                    Self::Photos(a) => a.image(path, width, height, rgba),
                    other => match other.studio_mut() {
                        Some(studio) => studio.image(path, width, height, rgba),
                        None => Err("this application shows no images".into()),
                    },
                }
            }
            /// One file could not be decoded. That is about the file, not the library, so
            /// it marks the file rather than putting the whole application offline.
            pub fn image_failed(&mut self, path: &str, reason: &str) {
                match self {
                    Self::Photos(a) => a.image_failed(path, reason),
                    other => match other.studio_mut() {
                        Some(studio) => studio.image_failed(path, reason),
                        None => other.offline(path, reason),
                    },
                }
            }
            /// A click that carries where inside its control it landed, for controls
            /// that place a caret; everything else treats it as a plain click.
            pub fn click_at(
                &mut self,
                window: u64,
                target: &str,
                dx: i32,
                dy: i32,
                clock_us: u64,
            ) -> Result<Vec<AppEffect>, String> {
                match self {
                    Self::Code(a) => a.click_at(window, target, dx, dy, clock_us),
                    Self::Freecad(a) => a.click_at(window, target, dx, dy, clock_us),
                    Self::Kicad(a) => a.click_at(window, target, dx, dy, clock_us),
                    other => other.click(window, target, clock_us),
                }
            }
            /// A pointer pressed on a control, before it is released: a text view
            /// anchors a drag selection here.
            pub fn press_at(&mut self, target: &str, dx: i32, dy: i32) -> Result<(), String> {
                match self {
                    Self::Code(a) => a.press_at(target, dx, dy),
                    _ => Ok(()),
                }
            }
            /// A double click on one of this application's controls.
            pub fn activate(
                &mut self,
                window: u64,
                target: &str,
                clock_us: u64,
            ) -> Result<Vec<AppEffect>, String> {
                match self {
                    Self::Code(a) => a.activate(window, target, clock_us),
                    Self::Freecad(a) => a.activate(window, target, clock_us),
                    Self::Spreadsheet(a) => a.activate(window, target, clock_us),
                    Self::Excel(a) => a.activate(window, target, clock_us),
                    Self::Database(a) => a.activate(window, target, clock_us),
                    Self::Kicad(a) => a.activate(window, target, clock_us),
                    other => other.click(window, target, clock_us),
                }
            }
            /// Typed text that may need work done, such as a search as you type.
            pub fn text_effects(&mut self, window: u64, text: &str) -> Result<Vec<AppEffect>, String> {
                match self {
                    Self::Code(a) => a.text_effects(window, text),
                    Self::Kicad(a) => a.text_effects(window, text),
                    other => other.text(text).map(|()| vec![]),
                }
            }
            /// Text from the machine's clipboard, pasted where the application's focus is.
            pub fn paste(&mut self, window: u64, text: &str) -> Result<Vec<AppEffect>, String> {
                match self {
                    Self::Code(a) => a.paste(window, text),
                    Self::Spreadsheet(a) => a.paste(text).map(|()| vec![]),
                    Self::Excel(a) => a.paste(text).map(|()| vec![]),
                    Self::Database(a) => a.0.paste(text).map(|()| vec![]),
                    other => other.text(text).map(|()| vec![]),
                }
            }
            /// The application draws a dark theme, and its frame should match.
            pub fn dark_chrome(&self) -> bool {
                match self {
                    Self::Code(a) => a.dark(),
                    _ => false,
                }
            }
            /// Facts a frame drawing this application's title bar needs.
            pub fn chrome(&self) -> Vec<(String, String)> {
                match self {
                    Self::Code(a) => vec![
                        ("menu".into(), a.menu.clone().unwrap_or_default()),
                        ("sidebar".into(), if a.sidebar { "1" } else { "0" }.into()),
                        ("panel".into(), if a.panel_open { "1" } else { "0" }.into()),
                        ("enabled".into(), a.enabled_menu_commands().join(",")),
                    ],
                    Self::Freecad(a) => a.chrome(),
                    _ => vec![],
                }
            }
            /// Semantic projection, so an agent can drive the app without pixels.
            pub fn page(&self, page: &mut cw_protocol::Page) {
                match self { $(Self::$variant(a) => a.page(page),)+ }
            }
            pub fn render(&self, p: &mut Painter, env: &AppEnv<'_>) {
                match self { $(Self::$variant(a) => a.render(p, env),)+ }
            }
            /// Build the application named `kind`, or `None` when it is not a native app.
            pub fn launch(
                kind: &str,
                argument: &str,
                window: u64,
                clock_us: u64,
            ) -> Option<(Self, Vec<AppEffect>)> {
                $(if kind == $module::$variant::KIND {
                    let (app, effects) = $module::$variant::launch(argument, window, clock_us);
                    return Some((Self::$variant(app), effects));
                })+
                None
            }
            pub const KINDS: &'static [&'static str] = &[$($module::$variant::KIND,)+];
        }
    };
}

native_apps! {
    Calendar => calendar,
    Mail => mail,
    Chat => chat,
    Docs => docs,
    Notes => notes,
    Contacts => contacts,
    Settings => settings,
    Calculator => calculator,
    Clock => clock,
    Photos => photos,
    Music => music,
    Maps => maps,
    Weather => weather,
    Code => code,
    Freecad => freecad,
    Paint => imaging,
    Preview => imaging,
    Pixelmator => imaging,
    Gimp => imaging,
    Pinta => imaging,
    Sketchbook => imaging,
    Spreadsheet => sheet,
    Excel => sheet,
    Database => database,
    Kicad => kicad,
}

/// Hooks only image applications have: drag surfaces, rasterised text, finished saves
/// and folder listings for their file sheets.
impl NativeApp {
    fn studio(&self) -> Option<&imaging::Studio> {
        match self {
            Self::Paint(a) => Some(a.0.as_ref()),
            Self::Preview(a) => Some(a.0.as_ref()),
            Self::Pixelmator(a) => Some(a.0.as_ref()),
            Self::Gimp(a) => Some(a.0.as_ref()),
            Self::Pinta(a) => Some(a.0.as_ref()),
            Self::Sketchbook(a) => Some(a.0.as_ref()),
            Self::Photos(a) => a.editing.as_deref(),
            _ => None,
        }
    }
    fn studio_mut(&mut self) -> Option<&mut imaging::Studio> {
        match self {
            Self::Paint(a) => Some(a.0.as_mut()),
            Self::Preview(a) => Some(a.0.as_mut()),
            Self::Pixelmator(a) => Some(a.0.as_mut()),
            Self::Gimp(a) => Some(a.0.as_mut()),
            Self::Pinta(a) => Some(a.0.as_mut()),
            Self::Sketchbook(a) => Some(a.0.as_mut()),
            Self::Photos(a) => a.editing.as_deref_mut(),
            _ => None,
        }
    }
    /// Whether a keystroke inserts text. An image editor takes text only while its text
    /// tool or a file name is being typed, so a phone shows no keyboard over a canvas.
    pub fn accepts_text(&self) -> bool {
        match self {
            Self::Photos(a) => a.editing.as_ref().is_some_and(|e| e.accepts_text()),
            Self::Spreadsheet(a) => a.accepts_text(),
            Self::Excel(a) => a.accepts_text(),
            Self::Database(a) => a.0.accepts_text(),
            Self::Kicad(a) => a.accepts_text(),
            other => other.studio().is_none_or(|s| s.accepts_text()),
        }
    }
    /// Whether `target` follows a pointer drag (a canvas, a slider).
    pub fn drags(&self, target: &str) -> bool {
        match self {
            Self::Kicad(a) => a.drags(target),
            Self::Freecad(a) => a.drags(target),
            Self::Spreadsheet(_) | Self::Excel(_) => sheet::Book::drags(target),
            other => other.studio().is_some_and(|s| s.drags(target)),
        }
    }
    fn book_mut(&mut self) -> Option<&mut sheet::Book> {
        match self {
            Self::Spreadsheet(a) => Some(&mut a.0),
            Self::Excel(a) => Some(&mut a.0),
            _ => None,
        }
    }
    /// Applications that work on documents and open on the user's Documents folder
    /// when launched on nothing.
    pub fn opens_documents(kind: &str) -> bool {
        kind == sheet::Spreadsheet::KIND
            || kind == sheet::Excel::KIND
            || kind == database::Database::KIND
    }
    /// A file's bytes (or why they could not be read) for an application that asked.
    pub fn bytes(
        &mut self,
        _window: u64,
        path: &str,
        result: Result<Vec<u8>, String>,
        clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        if let Self::Freecad(a) = self {
            a.bytes_loaded(path, result)?;
            return Ok(vec![]);
        }
        if let Self::Database(a) = self {
            a.0.bytes(path, result, clock_us);
            return Ok(vec![]);
        }
        let book = self.book_mut().ok_or("this application reads no files")?;
        book.bytes(path, result, clock_us);
        Ok(vec![])
    }
    /// A file this application wrote was saved, or could not be.
    pub fn bytes_saved(
        &mut self,
        _window: u64,
        path: &str,
        result: Result<(), String>,
    ) -> Result<Vec<AppEffect>, String> {
        // FreeCAD reports a failed write in its Report view; the action fails with it.
        if let Self::Freecad(a) = self {
            result?;
            a.written(path);
            return Ok(vec![]);
        }
        if let Self::Database(a) = self {
            a.0.saved(path, result);
            return Ok(vec![]);
        }
        let book = self.book_mut().ok_or("this application writes no files")?;
        book.saved(path, result);
        Ok(vec![])
    }
    /// The button (0 left, 1 middle, 2 right) of a press about to reach a drag surface.
    pub fn pointer_button(&mut self, button: u8) {
        if let Self::Freecad(a) = self {
            a.pointer_button(button);
        }
    }
    /// Whether a secondary-button press on `target` belongs to the application (a
    /// right-drag that pans a 3D view) rather than opening the context menu.
    pub fn takes_secondary(&self, target: &str) -> bool {
        matches!(self, Self::Freecad(a) if a.drags(target))
    }
    /// A wheel turn over `target`, at (`x`, `y`) inside it, for applications that give
    /// the wheel a meaning of their own; `false` leaves it to the platform, which
    /// scrolls the pane under the pointer.
    pub fn wheel(
        &mut self,
        target: &str,
        x: i32,
        y: i32,
        wheel: crate::Wheel,
    ) -> Result<bool, String> {
        match self {
            Self::Freecad(a) => a.wheel(target, x, y, wheel.dy),
            Self::Code(a) => a.wheel(target, wheel),
            Self::Spreadsheet(a) => a.0.wheel(target, wheel),
            Self::Excel(a) => a.0.wheel(target, wheel),
            Self::Kicad(a) => a.wheel(target, x, y, wheel),
            Self::Database(a) => a.0.wheel(target, wheel),
            other => match other.studio_mut() {
                Some(studio) => studio.wheel(target, x, y, wheel),
                None => Ok(false),
            },
        }
    }
    /// Menu entries this application puts in the Mac's global menu bar panel `panel`.
    #[allow(clippy::type_complexity)]
    pub fn mac_menu(&self, panel: &str) -> Option<Vec<freecad::MenuEntry>> {
        match self {
            Self::Freecad(a) => a.mac_menu(panel),
            _ => None,
        }
    }
    /// Whether `target` wants to know where the pointer is while no button is down: a
    /// canvas that draws the wire or track being placed under the cursor.
    pub fn hovers(&self, target: &str) -> bool {
        match self {
            Self::Kicad(a) => a.drags(target),
            _ => false,
        }
    }
    /// The pointer moved over `target` with no button down, relative to its top-left.
    /// Returns whether anything on screen changes because of it.
    pub fn hover(&mut self, target: &str, x: i32, y: i32) -> bool {
        match self {
            Self::Kicad(a) => a.hover(target, x, y),
            _ => false,
        }
    }
    /// A pointer press, move or release on a drag surface, relative to its top-left.
    pub fn pointer(
        &mut self,
        window: u64,
        target: &str,
        phase: crate::PointerPhase,
        x: i32,
        y: i32,
        clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        if let Self::Freecad(a) = self {
            return a.pointer(window, target, phase, x, y);
        }
        if let Some(book) = self.book_mut() {
            return book.pointer(target, phase, x, y);
        }
        if let Self::Kicad(a) = self {
            return a.pointer(window, target, phase, x, y, clock_us);
        }
        let studio = self
            .studio_mut()
            .ok_or("this application has no drag surfaces")?;
        let command = target
            .strip_prefix(studio.product.prefix())
            .and_then(|t| t.strip_prefix(':'))
            .ok_or("that surface belongs to another application")?
            .to_owned();
        studio.pointer(window, &command, phase, x, y)
    }
    /// A folder listing an image editor's file sheet asked for.
    pub fn listed(&mut self, entries: Vec<String>) -> Result<(), String> {
        if let Self::Freecad(a) = self {
            a.listed(entries);
            return Ok(());
        }
        if let Some(book) = self.book_mut() {
            book.listed(entries);
            return Ok(());
        }
        if let Self::Database(a) = self {
            a.0.listed(entries);
            return Ok(());
        }
        if let Self::Kicad(a) = self {
            a.listed(entries);
            return Ok(());
        }
        let studio = self.studio_mut().ok_or("window is not a file manager")?;
        studio.listed(entries);
        Ok(())
    }
    /// A picture this application encoded was written.
    pub fn image_saved(&mut self, window: u64, path: &str) -> Result<Vec<AppEffect>, String> {
        match self {
            Self::Photos(a) => a.image_saved(window, path),
            other => {
                let studio = other
                    .studio_mut()
                    .ok_or("this application saves no images")?;
                studio.saved(path);
                Ok(vec![])
            }
        }
    }
    /// Glyph coverage for text this application asked to have rasterised.
    pub fn text_rasterized(
        &mut self,
        width: u32,
        height: u32,
        alpha: Vec<u8>,
    ) -> Result<(), String> {
        self.studio_mut()
            .ok_or("this application draws no text into images")?
            .text_rasterized(width, height, alpha)
    }
    /// The text field that has the keyboard focus, named by the control that shows it,
    /// or `None` when nothing in the application is taking text. This is the one answer
    /// the platform uses for text focus: a phone paints its soft keyboard exactly when
    /// there is a field, the keystroke router sends typing to it, and the published
    /// focus names it.
    ///
    /// `mobile` is whether the application runs on a phone, where a field must be
    /// tapped before it has the focus (Messages' composer, a note's body) while the same
    /// field is focused on a desktop as soon as its conversation or note is open.
    pub fn text_field(&self, mobile: bool) -> Option<String> {
        let field = |on: bool, name: &str| on.then(|| name.to_owned());
        match self {
            Self::Calendar(a) => field(a.draft.is_some(), "cal:title"),
            Self::Mail(a) => a
                .compose
                .as_ref()
                .map(|c| format!("mail:field:{}", c.field)),
            Self::Chat(a) => field(a.open.is_some() && (a.composing || !mobile), "chat:compose"),
            Self::Docs(a) => field(a.open.is_some() && (a.editing || !mobile), "docs:body"),
            Self::Notes(a) => field(a.open.is_some() && (a.editing || !mobile), "notes:body"),
            Self::Maps(a) => field(a.typing, "maps:search-field"),
            Self::Music(a) => field(a.takes_text(), "music:search"),
            // Visual Studio Code types into whatever has its focus, bar the Explorer
            // tree and an editor group with no file open.
            Self::Code(a) => match a.focus {
                code::Focus::Explorer => None,
                code::Focus::Editor if a.active.is_none() => None,
                code::Focus::Editor => Some("code:editor".into()),
                code::Focus::Terminal => Some("code:terminal".into()),
                code::Focus::Search | code::Focus::SearchReplace => Some("code:search".into()),
                code::Focus::ScmMessage => Some("code:scm-message".into()),
                code::Focus::Quick => Some("code:quick".into()),
                code::Focus::Find | code::Focus::Replace => Some("code:find".into()),
                code::Focus::Inline => Some("code:inline".into()),
            },
            Self::Freecad(a) => field(a.0.field.is_some(), "freecad:field"),
            // No field anywhere: typed digits on a desktop are the calculator's keys,
            // not text, and the rest have nothing to type into.
            Self::Contacts(_)
            | Self::Settings(_)
            | Self::Calculator(_)
            | Self::Clock(_)
            | Self::Weather(_) => None,
            other => field(other.accepts_text(), "field"),
        }
    }
    /// Whether keystrokes insert text: a field has the focus (see `text_field`).
    pub fn takes_text(&self, mobile: bool) -> bool {
        self.text_field(mobile).is_some()
    }
}

/// Hooks for applications that open several windows onto one document — KiCad's
/// project manager, schematic, board and simulator frames share one open project, as
/// the real program's frames share one process.
impl NativeApp {
    /// Windows with the same link key show one shared document.
    pub fn link_key(&self) -> Option<&'static str> {
        match self {
            Self::Kicad(_) => Some("kicad"),
            _ => None,
        }
    }
    /// Counts changes to the shared document, so the newest copy wins.
    pub fn link_revision(&self) -> u64 {
        match self {
            Self::Kicad(a) => a.session.revision,
            _ => 0,
        }
    }
    /// Take the shared document from a sibling window.
    pub fn share_from(&mut self, other: &NativeApp) {
        if let (Self::Kicad(a), Self::Kicad(b)) = (self, other) {
            a.adopt(b);
        }
    }
    /// A launch that should raise an existing window rather than open another: the
    /// key the new window would have. KiCad opens one editor of each kind.
    pub fn instance_key(&self) -> Option<String> {
        match self {
            Self::Kicad(a) => Some(a.instance_key()),
            _ => None,
        }
    }
    /// An existing window asked for again with `argument` (Update PCB from the
    /// schematic raises the board editor with its dialog open).
    pub fn reopen(&mut self, window: u64, argument: &str) -> Vec<AppEffect> {
        match self {
            Self::Kicad(a) => a.reopen(window, argument),
            _ => vec![],
        }
    }
    /// Files the application asked for with `ReadFiles`.
    pub fn files_read(
        &mut self,
        window: u64,
        tag: &str,
        files: Vec<(String, Result<String, String>)>,
    ) -> Result<Vec<AppEffect>, String> {
        match self {
            Self::Kicad(a) => Ok(a.files_read(window, tag, files)),
            _ => Err("this application reads no files".into()),
        }
    }
    /// A folder tree the application asked for with `ListTree`.
    pub fn tree_listed(
        &mut self,
        window: u64,
        path: &str,
        result: Result<Vec<String>, String>,
    ) -> Result<Vec<AppEffect>, String> {
        match self {
            Self::Kicad(a) => Ok(a.tree_listed(window, path, result)),
            _ => Err("this application lists no folder trees".into()),
        }
    }
    /// A file this application wrote reached the disk.
    pub fn written(&mut self, path: &str) -> bool {
        match self {
            Self::Kicad(a) => {
                a.written(path);
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_native_kind_launches_and_round_trips() {
        for kind in NativeApp::KINDS {
            let (app, _) = NativeApp::launch(kind, "", 1, 0).expect("kind launches");
            assert_eq!(app.kind(), *kind);
            let restored: NativeApp =
                serde_json::from_str(&serde_json::to_string(&app).unwrap()).unwrap();
            assert_eq!(app, restored);
        }
        assert!(NativeApp::launch("not-an-app", "", 1, 0).is_none());
    }
    #[test]
    fn service_replies_separate_refusal_from_transport_failure() {
        assert_eq!(Status::from_status(200, ""), Status::Idle);
        assert_eq!(
            Status::from_status(403, r#"{"error":"mailbox unavailable"}"#),
            Status::Denied("mailbox unavailable".into())
        );
        assert!(matches!(Status::from_status(500, "{}"), Status::Offline(_)));
    }
    #[test]
    fn kicad_zooms_about_the_pointer_and_pans_with_modifiers() {
        let (mut app, _) = NativeApp::launch("kicad", "", 1, 0).unwrap();
        let NativeApp::Kicad(k) = &mut app else {
            unreachable!()
        };
        // Wheel away from a canvas, or in the project manager, is not KiCad's.
        assert!(!k
            .wheel("kicad:open", 10, 10, crate::Wheel::vertical(-120))
            .unwrap());
        let target = "kicad:canvas:sch:0:0:100:800:600";
        let before = kicad::View {
            x0: 0,
            y0: 0,
            zoom: 100,
            fit: false,
        };
        // A notch towards the screen zooms in, keeping the world point under the pointer.
        assert!(k
            .wheel(target, 400, 300, crate::Wheel::vertical(-120))
            .unwrap());
        let v = k.ui.view;
        assert!(v.zoom > before.zoom);
        assert_eq!(v.world(400, 300), before.world(400, 300));
        // Shift pans up and down, Ctrl left and right, at the zoom the canvas showed.
        let shift = crate::Wheel {
            dy: 120,
            shift: true,
            ..Default::default()
        };
        assert!(k.wheel(target, 400, 300, shift).unwrap());
        assert_eq!((k.ui.view.x0, k.ui.view.y0), (0, 1200));
        let ctrl = crate::Wheel {
            dy: 120,
            ctrl: true,
            ..Default::default()
        };
        assert!(k.wheel(target, 400, 300, ctrl).unwrap());
        assert_eq!((k.ui.view.x0, k.ui.view.y0), (1200, 0));
    }
    #[test]
    fn the_code_terminal_walks_its_scrollback_by_lines() {
        let (mut app, _) = NativeApp::launch("code", "", 1, 0).unwrap();
        let NativeApp::Code(c) = &mut app else {
            unreachable!()
        };
        if c.terminals.is_empty() {
            c.terminals.push(Default::default());
        }
        assert!(c
            .wheel("code:terminal", crate::Wheel::vertical(-120))
            .unwrap());
        let lifted = c.terminals[c.term].scroll;
        assert!(lifted > 0, "rolling back lifts the view off the tail");
        assert!(c
            .wheel("code:terminal", crate::Wheel::vertical(10_000))
            .unwrap());
        assert_eq!(c.terminals[c.term].scroll, 0);
        assert!(!c
            .wheel("code:terminal", crate::Wheel::vertical(120))
            .unwrap());
        // No file is open: the editor has nothing to scroll.
        assert!(!c
            .wheel("code:editor:0:0:0:30", crate::Wheel::vertical(120))
            .unwrap());
    }
    #[test]
    fn text_focus_is_reported_truthfully() {
        let launch = |kind: &str| NativeApp::launch(kind, "", 1, 0).unwrap().0;
        for kind in ["contacts", "settings", "calculator", "clock", "weather"] {
            assert_eq!(launch(kind).text_field(false), None, "{kind}");
            assert_eq!(launch(kind).text_field(true), None, "{kind}");
        }
        let mut notes = launch("notes");
        assert_eq!(notes.text_field(false), None, "no note is open");
        notes.click(1, "notes:new", 0).unwrap();
        assert_eq!(notes.text_field(true).as_deref(), Some("notes:body"));
        let mut mail = launch("mail");
        assert_eq!(mail.text_field(true), None);
        mail.click(1, "mail:compose", 0).unwrap();
        assert_eq!(mail.text_field(true).as_deref(), Some("mail:field:to"));
        mail.click(1, "mail:field:subject", 0).unwrap();
        assert_eq!(mail.text_field(true).as_deref(), Some("mail:field:subject"));
    }
    #[test]
    fn page_text_reads_what_a_page_shows_in_order() {
        use cw_protocol::PageElement as E;
        let mut page = cw_protocol::Page::new("T");
        page.elements.push(E::Heading {
            id: "h".into(),
            text: "Inbox".into(),
            level: 1,
        });
        page.elements.push(E::Text {
            id: "t".into(),
            text: "From carol".into(),
        });
        assert_eq!(crate::page_text(&page), "Inbox\nFrom carol");
    }
    #[test]
    fn retained_text_is_bounded_and_drops_control_characters() {
        let mut field = String::new();
        push_bounded(&mut field, "ab\ncd", 4);
        assert_eq!(field, "abcd");
        push_bounded(&mut field, "efg", 4);
        assert_eq!(field, "abcd");
    }
}
