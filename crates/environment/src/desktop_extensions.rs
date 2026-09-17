//! Shell interaction helpers. Catalog entries describe installed applications;
//! browser aliases still navigate through the canonical network and permission gate.
use super::*;

#[derive(Clone, Debug)]
pub(crate) struct DesktopAlias {
    pub id: String,
    pub label: String,
    pub url: String,
}
#[derive(Clone, Debug)]
pub(crate) struct DesktopApp {
    pub id: String,
    pub label: String,
    #[allow(dead_code)] // Theme renderers currently select icons by installed application ID.
    pub icon: String,
}

impl Environment {
    pub(crate) fn desktop_alias(
        &self,
        id: &str,
        machine: &str,
        kind: &str,
    ) -> Result<Option<DesktopAlias>> {
        let Some(entries) = self
            .runtime
            .definition()
            .metadata
            .get("desktop_apps")
            .and_then(Value::as_array)
        else {
            return Ok(None);
        };
        let Some(entry) = entries
            .iter()
            .find(|entry| entry.get("id").and_then(Value::as_str) == Some(kind))
        else {
            return Ok(None);
        };
        if entry.get("kind").and_then(Value::as_str) != Some("browser") {
            return Err(SimError::invalid("unsupported desktop application alias"));
        }
        let computer = self.runtime.computer(machine)?;
        if !computer.application_available(kind) || !computer.application_available("browser") {
            return Err(SimError::not_found("application is not installed"));
        }
        let grants = &self.session(id)?.config.actions;
        if !grants.iter().any(|family| family == "application.v1")
            || !grants.iter().any(|family| family == "browser.v1")
        {
            return Err(SimError::denied(
                "browser application interaction is not permitted",
            ));
        }
        let url = entry
            .get("url")
            .and_then(Value::as_str)
            .filter(|url| !url.is_empty())
            .ok_or_else(|| SimError::invalid("desktop application URL required"))?;
        Ok(Some(DesktopAlias {
            id: kind.into(),
            label: entry
                .get("label")
                .and_then(Value::as_str)
                .unwrap_or(kind)
                .into(),
            url: url.into(),
        }))
    }

    pub(crate) fn desktop_catalog(&self, id: &str, machine: &str) -> Vec<DesktopApp> {
        let Ok(computer) = self.runtime.computer(machine) else {
            return Vec::new();
        };
        let Ok(session) = self.session(id) else {
            return Vec::new();
        };
        if !session
            .config
            .actions
            .iter()
            .any(|family| family == "application.v1")
        {
            return Vec::new();
        }
        let mut catalog = Vec::new();
        for (kind, alternate, label) in [
            ("files", "file_manager", "Files"),
            ("browser", "browser", "Browser"),
            ("terminal", "terminal", "Terminal"),
            ("editor", "text_editor", "Text Editor"),
        ] {
            if computer.application_available(kind) || computer.application_available(alternate) {
                if kind == "browser"
                    && !session
                        .config
                        .actions
                        .iter()
                        .any(|family| family == "browser.v1")
                {
                    continue;
                }
                catalog.push(DesktopApp {
                    id: kind.into(),
                    label: match (self.desktop_theme(id, machine), kind) {
                        (Some(DesktopTheme::Macos), "files") => "Finder",
                        (Some(DesktopTheme::Macos | DesktopTheme::Ios), "browser") => "Safari",
                        (Some(DesktopTheme::Macos), "editor") => "TextEdit",
                        (Some(DesktopTheme::Windows), "files") => "File Explorer",
                        (Some(DesktopTheme::Windows), "editor") => "Notepad",
                        (Some(DesktopTheme::Ubuntu), "browser") => "Web Browser",
                        (Some(DesktopTheme::Ios), "editor") => "Notes",
                        (Some(DesktopTheme::Android), "editor") => "Editor",
                        _ => label,
                    }
                    .into(),
                    icon: kind.into(),
                });
            }
        }
        if let Some(entries) = self
            .runtime
            .definition()
            .metadata
            .get("desktop_apps")
            .and_then(Value::as_array)
        {
            for entry in entries {
                let Some(kind) = entry.get("id").and_then(Value::as_str) else {
                    continue;
                };
                if catalog.iter().any(|app| app.id == kind) {
                    continue;
                }
                if let Ok(Some(alias)) = self.desktop_alias(id, machine, kind) {
                    catalog.push(DesktopApp {
                        id: alias.id,
                        label: if kind == "chat"
                            && matches!(self.desktop_theme(id, machine), Some(DesktopTheme::Ios))
                        {
                            "Messages".into()
                        } else {
                            alias.label
                        },
                        icon: entry
                            .get("icon")
                            .and_then(Value::as_str)
                            .unwrap_or("browser")
                            .into(),
                    });
                }
            }
        }
        catalog
    }

    pub(crate) fn desktop_panel_text(
        &mut self,
        id: &str,
        machine: &str,
        text: &str,
    ) -> Result<bool> {
        let desktop = &mut self.machine_mut(id, machine)?.desktop;
        if desktop.launcher_open || desktop.panel.as_deref() == Some("search") {
            // Bound retained input independent of host input method behavior.
            for ch in text.chars().filter(|ch| !ch.is_control()) {
                if desktop.search.len() + ch.len_utf8() > 1024 {
                    break;
                }
                desktop.search.push(ch);
            }
            return Ok(true);
        }
        Ok(false)
    }

    pub(crate) fn desktop_panel_key(
        &mut self,
        id: &str,
        machine: &str,
        actor: &str,
        key: &str,
    ) -> Result<bool> {
        let desktop = &mut self.machine_mut(id, machine)?.desktop;
        if key == "Escape" && (desktop.panel.is_some() || desktop.launcher_open) {
            desktop.panel = None;
            desktop.launcher_open = false;
            desktop.search.clear();
            return Ok(true);
        }
        if !desktop.launcher_open && desktop.panel.as_deref() != Some("search") {
            return Ok(false);
        }
        match key {
            "Backspace" => {
                desktop.search.pop();
            }
            "Enter" => {
                let query = desktop.search.to_lowercase();
                let app = self.desktop_catalog(id, machine).into_iter().find(|app| {
                    app.label.to_lowercase().contains(&query)
                        || app.id.to_lowercase().contains(&query)
                });
                if let Some(app) = app {
                    self.shell_action(id, machine, actor, &format!("shell:launch:{}", app.id))?;
                    let desktop = &mut self.machine_mut(id, machine)?.desktop;
                    desktop.launcher_open = false;
                    desktop.panel = None;
                    desktop.search.clear();
                }
            }
            _ => {}
        }
        Ok(true)
    }

    pub(crate) fn desktop_panel_action(
        &mut self,
        id: &str,
        machine: &str,
        actor: &str,
        target: &str,
    ) -> Result<Option<Value>> {
        if let Some(name) = target.strip_prefix("shell:panel:") {
            let name = match name {
                "apple" => "apple",
                "file" => "file",
                "edit" => "edit",
                "view" => "view",
                "window" => "window",
                "help" => "help",
                "spotlight" | "search" => "search",
                "control" | "quick" => "quick",
                "calendar" | "clock" => "calendar",
                "notifications" => "notifications",
                "settings" => "settings",
                "overview" => "overview",
                "context" => "context",
                _ => return Err(SimError::invalid("unknown shell panel")),
            };
            let desktop = &mut self.machine_mut(id, machine)?.desktop;
            desktop.panel = if desktop.panel.as_deref() == Some(name) {
                None
            } else {
                Some(name.into())
            };
            desktop.launcher_open = false;
            desktop.search.clear();
            return Ok(Some(Value::Null));
        }
        if target == "shell:desktop" {
            self.machine_mut(id, machine)?.desktop.home();
            self.machine_mut(id, machine)?.desktop.panel = None;
            self.sync_desktop_visibility(id, machine)?;
            return Ok(Some(Value::Null));
        }
        let panel = match target {
            "shell:search" | "shell:spotlight" => Some("search"),
            "shell:settings" => Some("settings"),
            "shell:overview" | "shell:recents" => Some("overview"),
            "shell:clock" | "shell:calendar" => Some("calendar"),
            "shell:notifications" => Some("notifications"),
            "shell:control-center" | "shell:quick-settings" | "shell:system" => Some("quick"),
            "shell:menu:File" => Some("file"),
            "shell:menu:Edit" => Some("edit"),
            "shell:menu:View" => Some("view"),
            "shell:menu:Window" => Some("window"),
            "shell:menu:Help" => Some("help"),
            "shell:menu:Apple" | "shell:menu:apple" => Some("apple"),
            _ => None,
        };
        if let Some(panel) = panel {
            let desktop = &mut self.machine_mut(id, machine)?.desktop;
            let already_searching = panel == "search"
                && (desktop.panel.as_deref() == Some("search") || desktop.launcher_open);
            desktop.panel = if panel != "search" && desktop.panel.as_deref() == Some(panel) {
                None
            } else {
                Some(panel.into())
            };
            desktop.launcher_open = false;
            if !already_searching {
                desktop.search.clear();
            }
            return Ok(Some(Value::Null));
        }
        match target {
            "shell:noop" => Ok(Some(Value::Null)),
            "shell:mobile-back" => {
                let state = &self.session(id)?.machines[machine];
                if state.desktop.panel.is_some() || state.desktop.launcher_open {
                    return self.desktop_panel_action(id, machine, actor, "shell:dismiss");
                }
                if state.browser_visible && state.browser.tab().position > 0 {
                    return self
                        .shell_action(id, machine, actor, "shell:back")
                        .map(Some);
                }
                self.machine_mut(id, machine)?.desktop.home();
                self.sync_desktop_visibility(id, machine)?;
                Ok(Some(Value::Null))
            }
            "shell:dismiss" => {
                let desktop = &mut self.machine_mut(id, machine)?.desktop;
                desktop.panel = None;
                desktop.launcher_open = false;
                desktop.search.clear();
                Ok(Some(Value::Null))
            }
            "shell:new" => {
                let kind = self
                    .machine_mut(id, machine)?
                    .desktop
                    .focused
                    .and_then(|window| {
                        self.session(id).ok()?.machines[machine]
                            .desktop
                            .windows
                            .get(&window)
                            .map(|window| window.app_id.clone())
                    })
                    .unwrap_or_else(|| "files".into());
                let result = self.dispatch(
                    id,
                    actor,
                    &ActionEnvelope::new("application.v1", "launch", machine, json!({"kind":kind})),
                )?;
                self.machine_mut(id, machine)?.desktop.panel = None;
                Ok(Some(result))
            }
            "shell:save" => {
                let effects = self
                    .machine_mut(id, machine)?
                    .desktop
                    .key("Ctrl+s")
                    .map_err(SimError::invalid)?;
                self.effects(id, machine, actor, effects)?;
                self.machine_mut(id, machine)?.desktop.panel = None;
                Ok(Some(Value::Null))
            }
            _ => Ok(None),
        }
    }
}
