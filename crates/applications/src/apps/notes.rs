//! Notes kept as real files on the machine's own filesystem under the user's Notes
//! folder. Nothing is cached that the filesystem does not actually hold.
//!
//! Notes is a web application: its interface is `web/notes/Notes.tsx`, a React app
//! the web-app host (`crate::web_app`) runs in the window. This type is the window's
//! handle on it, saved as the state the app declares (`NotesState`), which is the
//! shape Notes has always been saved in.
use crate::desktop_scene::{DesktopTheme, Painter};
use crate::web_app::WebApp;
use crate::AppEffect;
use serde::{Deserialize, Serialize};

/// What Notes declares, and so what a snapshot of a Notes window holds.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotesState {
    /// Folder the notes live in, resolved by the shell against the machine's filesystem.
    pub folder: String,
    pub entries: Vec<String>,
    pub open: Option<String>,
    pub text: String,
    pub dirty: bool,
    /// Set when the folder could not be listed; shown instead of an empty list.
    pub problem: Option<String>,
    /// The note's body has been tapped (or the note was just created), so on a phone
    /// it has the keyboard. A desktop focuses the body whenever a note is open.
    #[serde(default)]
    pub editing: bool,
}

#[derive(Clone, Debug)]
pub struct Notes(pub WebApp);

impl Notes {
    pub const KIND: &'static str = "notes";
    pub fn launch(argument: &str, window: u64, clock_us: u64) -> (Self, Vec<AppEffect>) {
        Self::launch_on(argument, window, clock_us, DesktopTheme::Macos)
            .expect("the built-in Notes boots")
    }
    pub fn launch_on(
        argument: &str,
        window: u64,
        clock_us: u64,
        theme: DesktopTheme,
    ) -> Result<(Self, Vec<AppEffect>), String> {
        WebApp::launch(Self::KIND, argument, window, clock_us, theme)
            .map(|(app, effects)| (Self(app), effects))
    }
    /// The declared state; a default one when the application declared none.
    pub fn state(&self) -> NotesState {
        serde_json::from_value(self.0.state().clone()).unwrap_or_default()
    }
    pub fn kind(&self) -> &'static str {
        Self::KIND
    }
    pub fn title(&self, theme: DesktopTheme) -> String {
        self.0.title(theme)
    }
    pub fn document(&self) -> String {
        let s = self.state();
        s.open
            .map(|name| format!("{}/{name}", s.folder))
            .unwrap_or_default()
    }
    pub fn caption(&self) -> String {
        self.state().open.unwrap_or_default()
    }
    pub fn modified(&self) -> bool {
        self.state().dirty
    }
    pub fn offline(&mut self, tag: &str, reason: &str) {
        self.0.offline(tag, reason)
    }
    pub fn http(
        &mut self,
        window: u64,
        tag: &str,
        status: u16,
        body: &str,
    ) -> Result<Vec<AppEffect>, String> {
        self.0.http(window, tag, status, body)
    }
    pub fn text(&mut self, text: &str) -> Result<(), String> {
        self.0.text(text)
    }
    pub fn key(&mut self, window: u64, key: &str, clock_us: u64) -> Result<Vec<AppEffect>, String> {
        self.0.key(window, key, clock_us)
    }
    pub fn click(
        &mut self,
        window: u64,
        target: &str,
        clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        self.0.click(window, target, clock_us)
    }
    pub fn page(&self, page: &mut cw_protocol::Page) {
        self.0.page(page)
    }
    pub fn render(&self, p: &mut Painter, env: &crate::AppEnv<'_>) {
        self.0.render(p, env)
    }
}

/// Two Notes windows are equal when they hold the same notes: the state is all a
/// snapshot keeps.
impl PartialEq for Notes {
    fn eq(&self, other: &Self) -> bool {
        self.0.state() == other.0.state()
    }
}
impl Eq for Notes {}

impl Serialize for Notes {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.state().serialize(s)
    }
}
impl<'de> Deserialize<'de> for Notes {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let state = NotesState::deserialize(d)?;
        let value = serde_json::to_value(&state).map_err(serde::de::Error::custom)?;
        WebApp::restored(Self::KIND, 1, String::new(), value, Default::default())
            .map(Self)
            .map_err(serde::de::Error::custom)
    }
}
