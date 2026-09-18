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
pub mod contacts;
pub mod docs;
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
                    _ => Err("this application shows no images".into()),
                }
            }
            /// One file could not be decoded. That is about the file, not the library, so
            /// it marks the file rather than putting the whole application offline.
            pub fn image_failed(&mut self, path: &str, reason: &str) {
                match self {
                    Self::Photos(a) => a.image_failed(path),
                    other => other.offline(path, reason),
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
}

impl NativeApp {
    /// Whether keystrokes insert text. Every application but the music player has a
    /// field that is always ready for typing; the player takes text only while its search
    /// or playlist-title field is focused, so a phone shows no keyboard over it otherwise.
    pub fn takes_text(&self) -> bool {
        match self {
            Self::Music(app) => app.takes_text(),
            _ => true,
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
