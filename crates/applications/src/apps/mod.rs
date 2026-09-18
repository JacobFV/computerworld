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
pub mod docs;
pub mod imaging;
pub mod mail;
pub mod maps;
pub mod music;
pub mod notes;
pub mod photos;
pub mod settings;
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
                    other => other.click(window, target, clock_us),
                }
            }
            /// Typed text that may need work done, such as a search as you type.
            pub fn text_effects(&mut self, window: u64, text: &str) -> Result<Vec<AppEffect>, String> {
                match self {
                    Self::Code(a) => a.text_effects(window, text),
                    other => other.text(text).map(|()| vec![]),
                }
            }
            /// Text from the machine's clipboard, pasted where the application's focus is.
            pub fn paste(&mut self, window: u64, text: &str) -> Result<Vec<AppEffect>, String> {
                match self {
                    Self::Code(a) => a.paste(window, text),
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
    Paint => imaging,
    Preview => imaging,
    Pixelmator => imaging,
    Gimp => imaging,
    Pinta => imaging,
    Sketchbook => imaging,
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
            other => other.studio().is_none_or(|s| s.accepts_text()),
        }
    }
    /// Whether `target` follows a pointer drag (a canvas, a slider).
    pub fn drags(&self, target: &str) -> bool {
        self.studio().is_some_and(|s| s.drags(target))
    }
    /// A pointer press, move or release on a drag surface, relative to its top-left.
    pub fn pointer(
        &mut self,
        window: u64,
        target: &str,
        phase: crate::PointerPhase,
        x: i32,
        y: i32,
        _clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
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
    /// Whether keystrokes insert text. Every application but the music player has a
    /// field that is always ready for typing; the player takes text only while its search
    /// or playlist-title field is focused, so a phone shows no keyboard over it otherwise.
    pub fn takes_text(&self) -> bool {
        match self {
            Self::Music(app) => app.takes_text(),
            other => other.accepts_text(),
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
    fn retained_text_is_bounded_and_drops_control_characters() {
        let mut field = String::new();
        push_bounded(&mut field, "ab\ncd", 4);
        assert_eq!(field, "abcd");
        push_bounded(&mut field, "efg", 4);
        assert_eq!(field, "abcd");
    }
}
