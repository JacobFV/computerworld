//! Serializable desktop applications. Effects are requests to the environment,
//! never ambient filesystem access or subprocess execution.
pub mod desktop_scene;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppEffect {
    ReadFile {
        window: u64,
        path: String,
    },
    WriteFile {
        window: u64,
        path: String,
        content: String,
    },
    ListDirectory {
        window: u64,
        path: String,
    },
    Execute {
        window: u64,
        command: String,
    },
    Navigate {
        window: u64,
        url: String,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppState {
    Terminal {
        input: String,
        output: String,
        history: Vec<String>,
    },
    Editor {
        path: String,
        text: String,
        cursor: usize,
        dirty: bool,
    },
    Files {
        path: String,
        entries: Vec<String>,
    },
    Browser {
        address: String,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Window {
    pub id: u64,
    pub title: String,
    pub state: AppState,
    #[serde(default)]
    pub minimized: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesktopState {
    pub windows: BTreeMap<u64, Window>,
    pub focused: Option<u64>,
    next_id: u64,
    #[serde(default)]
    pub launcher_open: bool,
    #[serde(default)]
    pub maximized: bool,
}
impl DesktopState {
    pub fn launch(&mut self, kind: &str, argument: &str) -> Result<(u64, Vec<AppEffect>), String> {
        let id = self.next_id;
        let (state, effects) = match kind {
            "terminal" => (
                AppState::Terminal {
                    input: String::new(),
                    output: String::new(),
                    history: vec![],
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
                        path: path.into(),
                        entries: vec![],
                    },
                    vec![AppEffect::ListDirectory {
                        window: id,
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
            _ => return Err(format!("unknown application: {kind}")),
        };
        self.next_id += 1;
        self.windows.insert(
            id,
            Window {
                id,
                title: kind.into(),
                state,
                minimized: false,
            },
        );
        self.focused = Some(id);
        self.launcher_open = false;
        Ok((id, effects))
    }
    pub fn focus(&mut self, id: u64) -> Result<(), String> {
        if !self.windows.contains_key(&id) {
            return Err("window not found".into());
        }
        self.windows.get_mut(&id).unwrap().minimized = false;
        self.focused = Some(id);
        self.launcher_open = false;
        Ok(())
    }
    pub fn close(&mut self, id: u64) -> Result<(), String> {
        self.windows.remove(&id).ok_or("window not found")?;
        if self.focused == Some(id) {
            self.focused = self
                .windows
                .values()
                .rev()
                .find(|w| !w.minimized)
                .map(|w| w.id);
        }
        Ok(())
    }
    pub fn minimize(&mut self, id: u64) -> Result<(), String> {
        self.windows
            .get_mut(&id)
            .ok_or("window not found")?
            .minimized = true;
        if self.focused == Some(id) {
            self.focused = self
                .windows
                .values()
                .rev()
                .find(|w| !w.minimized)
                .map(|w| w.id);
        }
        Ok(())
    }
    pub fn home(&mut self) {
        for window in self.windows.values_mut() {
            window.minimized = true;
        }
        self.focused = None;
        self.launcher_open = false;
    }
    pub fn cycle(&mut self) -> Result<(), String> {
        let ids: Vec<_> = self.windows.keys().copied().collect();
        if ids.is_empty() {
            return Ok(());
        }
        let next = self
            .focused
            .and_then(|id| ids.iter().position(|candidate| *candidate == id))
            .map(|index| (index + 1) % ids.len())
            .unwrap_or(0);
        self.focus(ids[next])
    }
    pub fn text(&mut self, text: &str) -> Result<(), String> {
        let window = self
            .focused
            .and_then(|id| self.windows.get_mut(&id))
            .ok_or("no focused window")?;
        match &mut window.state {
            AppState::Terminal { input, .. } => input.push_str(text),
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
            AppState::Files { .. } => return Err("file manager has no text focus".into()),
        }
        Ok(())
    }
    pub fn key(&mut self, key: &str) -> Result<Vec<AppEffect>, String> {
        let id = self.focused.ok_or("no focused window")?;
        let window = self.windows.get_mut(&id).ok_or("window not found")?;
        if let AppState::Editor { text, cursor, .. } = &window.state {
            if *cursor > text.len() || !text.is_char_boundary(*cursor) {
                return Err("invalid text cursor".into());
            }
        }
        let mut effects = vec![];
        match &mut window.state {
            AppState::Terminal { input, history, .. } => match key {
                "Enter" => {
                    let command = std::mem::take(input);
                    history.push(command.clone());
                    effects.push(AppEffect::Execute {
                        window: id,
                        command,
                    });
                }
                "Backspace" => {
                    input.pop();
                }
                _ => return Err(format!("unsupported terminal key {key}")),
            },
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
            AppState::Files { .. } => return Err("file manager key unsupported".into()),
        }
        Ok(effects)
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
            _ => Err("window is not an editor".into()),
        }
    }
    pub fn terminal_output(&mut self, id: u64, value: &str) -> Result<(), String> {
        match &mut self.windows.get_mut(&id).ok_or("window not found")?.state {
            AppState::Terminal { output, .. } => {
                output.push_str(value);
                Ok(())
            }
            _ => Err("window is not a terminal".into()),
        }
    }
    pub fn directory_loaded(&mut self, id: u64, mut values: Vec<String>) -> Result<(), String> {
        values.sort();
        match &mut self.windows.get_mut(&id).ok_or("window not found")?.state {
            AppState::Files { entries, .. } => {
                *entries = values;
                Ok(())
            }
            _ => Err("window is not a file manager".into()),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_editor_effects_and_focus() {
        let mut d = DesktopState::default();
        let (editor, _) = d.launch("editor", "/note").unwrap();
        d.file_loaded(editor, "café".into()).unwrap();
        d.key("Backspace").unwrap();
        d.text("ø").unwrap();
        assert_eq!(
            d.key("Ctrl+s").unwrap(),
            vec![AppEffect::WriteFile {
                window: editor,
                path: "/note".into(),
                content: "cafø".into()
            }]
        );
        let (terminal, _) = d.launch("terminal", "").unwrap();
        d.text("pwd").unwrap();
        assert_eq!(
            d.key("Enter").unwrap(),
            vec![AppEffect::Execute {
                window: terminal,
                command: "pwd".into()
            }]
        );
        d.close(terminal).unwrap();
        assert_eq!(d.focused, Some(editor));
    }
    #[test]
    fn snapshot_restores_cursor_and_pending_edit() {
        let mut d = DesktopState::default();
        d.launch("editor", "/note").unwrap();
        d.text("😀hello").unwrap();
        d.key("Home").unwrap();
        d.key("Delete").unwrap();
        let restored: DesktopState =
            serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert_eq!(d, restored);
    }
}

impl DesktopState {
    /// Pure semantic projection; effects are completed separately by the caller.
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
            });
        }
        if let Some(window) = self.focused.and_then(|id| self.windows.get(&id)) {
            match &window.state {
                AppState::Terminal { input, output, .. } => {
                    page.elements.push(E::Text {
                        id: "terminal-output".into(),
                        text: output.clone(),
                    });
                    page.elements.push(E::Input {
                        id: "terminal-input".into(),
                        label: "$".into(),
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
                AppState::Files { path, entries } => {
                    page.elements.push(E::Heading {
                        id: "files-path".into(),
                        text: path.clone(),
                        level: 2,
                    });
                    for (index, entry) in entries.iter().enumerate() {
                        page.elements.push(E::Button {
                            id: format!("open:{index}"),
                            text: entry.clone(),
                            action: cw_protocol::PageAction {
                                method: "APP".into(),
                                url: entry.clone(),
                                fields: BTreeMap::new(),
                            },
                        });
                    }
                }
                AppState::Browser { address } => page.elements.push(E::Input {
                    id: "browser-address".into(),
                    label: "Address".into(),
                    value: address.clone(),
                    placeholder: "https://".into(),
                }),
            }
        }
        page
    }
    pub fn click(&mut self, target: &str) -> Result<Vec<AppEffect>, String> {
        if let Some(id) = target.strip_prefix("focus:") {
            self.focus(id.parse().map_err(|_| "invalid window ID")?)?;
            return Ok(vec![]);
        }
        if target == "editor-save" {
            return self.key("Ctrl+s");
        }
        if matches!(target, "files-up" | "files-parent" | "files-root") {
            let id = self.focused.ok_or("no focused window")?;
            let window = self.windows.get_mut(&id).ok_or("window not found")?;
            if let AppState::Files { path, .. } = &mut window.state {
                *path = if target == "files-root" {
                    "/".into()
                } else {
                    let parent = path
                        .trim_end_matches('/')
                        .rsplit_once('/')
                        .map(|(parent, _)| parent)
                        .unwrap_or("");
                    if parent.is_empty() {
                        "/".into()
                    } else {
                        parent.into()
                    }
                };
                return Ok(vec![AppEffect::ListDirectory {
                    window: id,
                    path: path.clone(),
                }]);
            }
            return Err("not a file manager".into());
        }
        if let Some(index) = target.strip_prefix("open:") {
            let index: usize = index.parse().map_err(|_| "invalid entry")?;
            let window = self
                .focused
                .and_then(|id| self.windows.get(&id))
                .ok_or("no focused window")?;
            if let AppState::Files { path, entries } = &window.state {
                let entry = entries.get(index).ok_or("entry not found")?;
                let selected = format!("{}/{}", path.trim_end_matches('/'), entry);
                let kind = if entry.ends_with('/') {
                    "files"
                } else {
                    "editor"
                };
                return self.launch(kind, &selected).map(|(_, effects)| effects);
            }
            return Err("not a file manager".into());
        }
        let state = &self
            .focused
            .and_then(|id| self.windows.get(&id))
            .ok_or("no focused window")?
            .state;
        match (target, state) {
            ("terminal-input", AppState::Terminal { .. })
            | ("editor-text", AppState::Editor { .. })
            | ("browser-address", AppState::Browser { .. }) => Ok(vec![]),
            _ => Err("interaction does not belong to focused application".into()),
        }
    }
}

/// Serialized instance data and declared module version. Registry code is supplied
/// by the embedding runtime and is never deserialized from a snapshot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AppInstance {
    pub kind: String,
    pub version: u32,
    pub state: serde_json::Value,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RegisteredApplications {
    pub instances: BTreeMap<String, AppInstance>,
}
impl RegisteredApplications {
    pub fn launch(
        &mut self,
        registry: &cw_sdk::Registry,
        kind: &str,
        id: &str,
        initial: serde_json::Value,
        context: &cw_sdk::AppContext,
    ) -> cw_protocol::Result<()> {
        if id.is_empty() || self.instances.contains_key(id) {
            return Err(cw_protocol::SimError::invalid(
                "empty or duplicate app instance",
            ));
        }
        let app = registry.application(kind)?;
        let state = app.initialize(initial, context)?;
        app.page(&state, context)?.validate()?;
        self.instances.insert(
            id.into(),
            AppInstance {
                kind: kind.into(),
                version: app.version(),
                state,
            },
        );
        Ok(())
    }
    pub fn event(
        &mut self,
        registry: &cw_sdk::Registry,
        id: &str,
        context: &cw_sdk::AppContext,
        event: &cw_sdk::AppEvent,
    ) -> cw_protocol::Result<Vec<cw_sdk::AppEffect>> {
        let instance = self
            .instances
            .get_mut(id)
            .ok_or_else(|| cw_protocol::SimError::not_found("app instance"))?;
        let app = registry.application(&instance.kind)?;
        if app.version() != instance.version {
            return Err(cw_protocol::SimError::invalid(
                "app module version mismatch",
            ));
        }
        // A plugin's failed event cannot leave a partially mutated app state.
        let mut next = instance.state.clone();
        let effects = app.event(&mut next, context, event)?;
        app.page(&next, context)?.validate()?;
        instance.state = next;
        Ok(effects)
    }
    pub fn page(
        &self,
        registry: &cw_sdk::Registry,
        id: &str,
        context: &cw_sdk::AppContext,
    ) -> cw_protocol::Result<cw_protocol::Page> {
        let instance = self
            .instances
            .get(id)
            .ok_or_else(|| cw_protocol::SimError::not_found("app instance"))?;
        let app = registry.application(&instance.kind)?;
        if app.version() != instance.version {
            return Err(cw_protocol::SimError::invalid(
                "app module version mismatch",
            ));
        }
        let page = app.page(&instance.state, context)?;
        page.validate()?;
        Ok(page)
    }
}
#[cfg(test)]
mod extension_tests {
    use super::*;
    struct Counter;
    impl cw_sdk::Application for Counter {
        fn kind(&self) -> &str {
            "counter"
        }
        fn event(
            &self,
            state: &mut serde_json::Value,
            _: &cw_sdk::AppContext,
            event: &cw_sdk::AppEvent,
        ) -> cw_protocol::Result<Vec<cw_sdk::AppEffect>> {
            *state = serde_json::json!(state.as_u64().unwrap_or(0) + 1);
            if event.kind == "fail" {
                return Err(cw_protocol::SimError::invalid("rejected"));
            }
            Ok(vec![])
        }
        fn page(
            &self,
            state: &serde_json::Value,
            _: &cw_sdk::AppContext,
        ) -> cw_protocol::Result<cw_protocol::Page> {
            Ok(cw_protocol::Page::new(format!("Count {state}")))
        }
    }
    #[test]
    fn custom_application_state_and_failed_event_atomicity() {
        let mut registry = cw_sdk::Registry::new();
        registry.register_application(Counter).unwrap();
        let mut apps = RegisteredApplications::default();
        let ctx = cw_sdk::AppContext {
            actor: "a".into(),
            machine: "m".into(),
            tick: 0,
            seed: 1,
            instance: "counter1".into(),
        };
        apps.launch(&registry, "counter", "counter1", serde_json::json!(0), &ctx)
            .unwrap();
        apps.event(
            &registry,
            "counter1",
            &ctx,
            &cw_sdk::AppEvent {
                kind: "increment".into(),
                target: None,
                data: serde_json::Value::Null,
            },
        )
        .unwrap();
        assert_eq!(
            apps.page(&registry, "counter1", &ctx).unwrap().title,
            "Count 1"
        );
        assert!(apps
            .event(
                &registry,
                "counter1",
                &ctx,
                &cw_sdk::AppEvent {
                    kind: "fail".into(),
                    target: None,
                    data: serde_json::Value::Null
                }
            )
            .is_err());
        assert_eq!(apps.instances["counter1"].state, serde_json::json!(1));
    }
}

#[cfg(test)]
mod focus_tests {
    use super::*;
    #[test]
    fn wrong_application_input_and_invalid_editor_cursor_rejected() {
        let mut desktop = DesktopState::default();
        let (id, _) = desktop.launch("editor", "/a").unwrap();
        assert!(desktop.click("terminal-input").is_err());
        desktop.click("editor-text").unwrap();
        if let AppState::Editor { text, cursor, .. } =
            &mut desktop.windows.get_mut(&id).unwrap().state
        {
            *text = "😀".into();
            *cursor = 1;
        }
        assert!(desktop.key("Backspace").is_err());
    }
}
