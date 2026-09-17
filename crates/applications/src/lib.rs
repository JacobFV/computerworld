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
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesktopState {
    pub windows: BTreeMap<u64, Window>,
    pub focused: Option<u64>,
    next_id: u64,
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
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowSnap {
    Left,
    Right,
    Full,
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
                app_id: kind.into(),
                state,
                minimized: false,
                frame: None,
                restored_frame: None,
                maximized: false,
                snapped: None,
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

#[cfg(test)]
mod window_geometry_tests {
    use super::*;
    use cw_scene::Rect;
    #[test]
    fn dragging_capture_survives_snapshot_and_focus_is_mru() {
        let area = Rect::new(0, 28, 1200, 700);
        let mut d = DesktopState::default();
        let (a, _) = d.launch("editor", "").unwrap();
        let (b, _) = d.launch("terminal", "").unwrap();
        let before = d.effective_frame(a, area);
        d.pointer_down(a, "drag", before.x + 100, before.y + 12, area)
            .unwrap();
        assert_eq!(d.ordered_windows(), vec![b, a]);
        d.pointer_move(before.x + 150, before.y + 42, area).unwrap();
        let mut restored: DesktopState =
            serde_json::from_value(serde_json::to_value(&d).unwrap()).unwrap();
        restored
            .pointer_up(before.x + 150, before.y + 42, area)
            .unwrap();
        assert_eq!(
            restored.effective_frame(a, area),
            Rect::new(before.x + 50, before.y + 30, before.width, before.height)
        );
        restored.minimize(a).unwrap();
        assert_eq!(restored.focused, Some(b));
        restored.focus(a).unwrap();
        assert!(!restored.windows[&a].minimized);
    }
    #[test]
    fn maximize_restore_is_per_window_and_snap_tearoff_restores_size() {
        let area = Rect::new(0, 28, 1200, 700);
        let mut d = DesktopState::default();
        let (a, _) = d.launch("files", "").unwrap();
        let (b, _) = d.launch("terminal", "").unwrap();
        let original = d.effective_frame(a, area);
        d.maximize(a, area).unwrap();
        assert_eq!(d.effective_frame(a, area), area);
        assert!(!d.windows[&b].maximized);
        d.maximize(a, area).unwrap();
        assert_eq!(d.effective_frame(a, area), original);
        d.snap(a, WindowSnap::Left, area).unwrap();
        assert_eq!(d.effective_frame(a, area).width, 600);
        d.pointer_down(a, "drag", 200, 45, area).unwrap();
        d.pointer_up(450, 200, area).unwrap();
        assert_eq!(d.effective_frame(a, area).width, original.width);
        assert_eq!(d.windows[&a].snapped, None);
    }
    #[test]
    fn all_resize_edges_preserve_opposite_edge_and_reject_bad_handles() {
        let area = Rect::new(0, 0, 1400, 1000);
        for edge in ["n", "ne", "e", "se", "s", "sw", "w", "nw"] {
            let mut d = DesktopState::default();
            let (id, _) = d.launch("terminal", "").unwrap();
            d.windows.get_mut(&id).unwrap().frame = Some(Rect::new(200, 200, 600, 400));
            d.pointer_down(id, &format!("resize:{edge}"), 300, 300, area)
                .unwrap();
            d.pointer_up(320, 330, area).unwrap();
            let r = d.effective_frame(id, area);
            assert_eq!(r.x, if edge.contains('w') { 220 } else { 200 });
            assert_eq!(r.y, if edge.contains('n') { 230 } else { 200 });
            assert_eq!(
                r.width,
                if edge.contains('w') {
                    580
                } else if edge.contains('e') {
                    620
                } else {
                    600
                }
            );
            assert_eq!(
                r.height,
                if edge.contains('n') {
                    370
                } else if edge.contains('s') {
                    430
                } else {
                    400
                }
            );
            assert!(d.pointer_down(id, "resize:bad", 0, 0, area).is_err());
        }
    }
}
