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
        match entry.get("kind").and_then(Value::as_str) {
            Some("browser") => {}
            // A native application is not an alias; it is launched in its own right.
            Some("native") => return Ok(None),
            _ => return Err(SimError::invalid("unsupported desktop application alias")),
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

    /// Launch argument for a native application: the world says which service backs it.
    /// An entry's `urls` may name a different service per platform (`{"android": ...}`),
    /// because the same kind of application is a different product on each: Android's
    /// music player is YouTube Music, backed by music.youtube.com.
    pub(crate) fn native_argument(&self, id: &str, machine: &str, kind: &str) -> String {
        let Some(entry) = self
            .runtime
            .definition()
            .metadata
            .get("desktop_apps")
            .and_then(Value::as_array)
            .and_then(|entries| {
                entries
                    .iter()
                    .find(|entry| entry.get("id").and_then(Value::as_str) == Some(kind))
            })
        else {
            return String::new();
        };
        let platform = self
            .desktop_theme(id, machine)
            .map(DesktopTheme::platform)
            .and_then(|platform| entry.get("urls")?.get(platform)?.as_str());
        platform
            .or_else(|| entry.get("url").and_then(Value::as_str))
            .unwrap_or_default()
            .to_owned()
    }
    /// Product name of a native application on this platform.
    fn native_label(theme: Option<DesktopTheme>, kind: &str) -> &'static str {
        match (theme, kind) {
            (_, "calendar") => "Calendar",
            (Some(DesktopTheme::Windows), "mail") => "Outlook",
            (Some(DesktopTheme::Android), "mail") => "Gmail",
            (Some(DesktopTheme::Ubuntu), "mail") => "Thunderbird",
            (_, "mail") => "Mail",
            (Some(DesktopTheme::Windows), "chat") => "Teams",
            (Some(DesktopTheme::Ubuntu), "chat") => "Chat",
            (_, "chat") => "Messages",
            (Some(DesktopTheme::Windows), "docs") => "Word",
            (Some(DesktopTheme::Ubuntu), "docs") => "Writer",
            (Some(DesktopTheme::Android), "docs") => "Docs",
            (_, "docs") => "Pages",
            (Some(DesktopTheme::Windows), "notes") => "Sticky Notes",
            (Some(DesktopTheme::Android), "notes") => "Keep",
            (_, "notes") => "Notes",
            (Some(DesktopTheme::Windows), "contacts") => "People",
            (_, "contacts") => "Contacts",
            (Some(DesktopTheme::Macos), "settings") => "System Settings",
            (_, "settings") => "Settings",
            (Some(DesktopTheme::Ubuntu), "clock") => "Clocks",
            (_, "clock") => "Clock",
            (_, "calculator") => "Calculator",
            (Some(DesktopTheme::Ubuntu), "photos") => "Image Viewer",
            (Some(DesktopTheme::Android), "photos") => "Google Photos",
            (_, "photos") => "Photos",
            (Some(DesktopTheme::Ubuntu), "music") => "Rhythmbox",
            (Some(DesktopTheme::Windows), "music") => "Media Player",
            (Some(DesktopTheme::Android), "music") => "YouTube Music",
            (_, "music") => "Music",
            (_, "maps") => "Maps",
            (_, "weather") => "Weather",
            (_, other) => {
                debug_assert!(false, "unnamed native application {other}");
                "Application"
            }
        }
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
        // Applications that ship with the simulator, when the machine has them installed.
        let theme = self.desktop_theme(id, machine);
        for kind in cw_applications::NativeApp::KINDS {
            if catalog.iter().any(|app| &app.id == kind) || !computer.application_available(kind) {
                continue;
            }
            catalog.push(DesktopApp {
                id: (*kind).into(),
                label: Self::native_label(theme, kind).into(),
                icon: (*kind).into(),
            });
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
        // A phone's system surface is modal: typing reaches nothing behind it.
        let phone = self
            .desktop_theme(id, machine)
            .is_some_and(DesktopTheme::mobile);
        Ok(phone && phone_overlay(&self.session(id)?.machines[machine]))
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
            let phone = self
                .desktop_theme(id, machine)
                .is_some_and(DesktopTheme::mobile);
            return Ok(phone && phone_overlay(&self.session(id)?.machines[machine]));
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

    /// What a finger dragged from `from` to `to` on a phone amounts to, as the shell
    /// target the same gesture names, or `None` when it is no gesture at all (a tap, or
    /// a drag nothing answers). `pressed` is the control the finger came down on.
    ///
    /// iOS: down from the status bar opens Notification Center (left of the Dynamic
    /// Island's right edge) or Control Center (right of it); up from the bottom edge goes
    /// home, or opens the App Switcher when it is long; sideways on the home screen walks
    /// the pages, with Today View before the first and the App Library after the last;
    /// down on the home screen opens Search; sideways along the bottom edge in an
    /// application switches to the previous one. Android: down from the status bar, or
    /// anywhere on the home screen, opens the shade and a second pull expands it; up on
    /// the home screen opens the app drawer; the navigation bar takes presses, not
    /// swipes. On both, a card swiped up in the overview closes its application.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn touch_gesture(
        &self,
        id: &str,
        machine: &str,
        theme: DesktopTheme,
        from: (i32, i32),
        to: (i32, i32),
        size: (u32, u32),
        pressed: Option<&str>,
    ) -> Result<Option<String>> {
        let (dx, dy) = (to.0 - from.0, to.1 - from.1);
        let (w, h) = (size.0 as i32, size.1 as i32);
        let vertical = dy.abs() > 70 && dy.abs() > dx.abs();
        let horizontal = dx.abs() > 70 && dx.abs() > dy.abs();
        let d = &self.session(id)?.machines[machine].desktop;
        let target = |t: &str| Ok(Some(t.to_owned()));
        match d.screen {
            // Swiping up the lock screen opens the phone; a dark one only wakes on a tap.
            cw_applications::ScreenState::Locked if vertical && dy < 0 => {
                return target("shell:power:wake")
            }
            cw_applications::ScreenState::Active => {}
            _ => return Ok(None),
        }
        if !vertical && !horizontal {
            return Ok(None);
        }
        let panel = d.panel.as_deref();
        // The home screen itself: no application in front, nothing pulled over it.
        let home = d.focused.is_none() && panel.is_none() && !d.launcher_open;
        if vertical && dy < 0 && panel == Some("overview") {
            if let Some(window) = pressed
                .and_then(|t| t.strip_prefix("window:"))
                .and_then(|rest| rest.strip_suffix(":focus"))
            {
                return Ok(Some(format!("window:{window}:close")));
            }
        }
        match theme {
            DesktopTheme::Ios if vertical => {
                if dy > 0 && from.1 < 50 {
                    return target(if from.0 > w / 2 + 66 {
                        "shell:gesture:control-center"
                    } else {
                        "shell:gesture:notifications"
                    });
                }
                if dy < 0 && from.1 > h - 60 {
                    return target(if dy < -(h / 3) {
                        "shell:gesture:overview"
                    } else {
                        "shell:gesture:home"
                    });
                }
                if dy < 0 && matches!(panel, Some("quick" | "notifications")) {
                    return target("shell:dismiss");
                }
                if dy > 0 && home {
                    return target("shell:search");
                }
                Ok(None)
            }
            DesktopTheme::Ios => {
                let installed: Vec<String> = self
                    .desktop_catalog(id, machine)
                    .into_iter()
                    .map(|app| app.id)
                    .collect();
                let pages = home_page_count(theme, &installed, size.0, size.1).max(1);
                let page = d.home_page.min(pages - 1);
                if panel == Some("calendar") && dx < 0 {
                    return target("shell:home-page:0");
                }
                if d.launcher_open && panel.is_none() && dx > 0 {
                    return Ok(Some(format!("shell:home-page:{}", pages - 1)));
                }
                if home {
                    return Ok(Some(match (dx < 0, page) {
                        (true, page) if page + 1 < pages => format!("shell:home-page:{}", page + 1),
                        (true, _) => "shell:launcher".to_owned(),
                        (false, 0) => "shell:panel:calendar".to_owned(),
                        (false, page) => format!("shell:home-page:{}", page - 1),
                    }));
                }
                if d.focused.is_some() && panel.is_none() && from.1 > h - 40 && dx > 0 {
                    return target("shell:switcher");
                }
                Ok(None)
            }
            DesktopTheme::Android if vertical => {
                if from.1 >= h - cw_applications::desktop_scene::ANDROID_NAV_BAR {
                    return Ok(None);
                }
                if dy > 0 {
                    if d.launcher_open {
                        return target("shell:dismiss");
                    }
                    if from.1 < 40 || home || panel == Some("notifications") {
                        return target("shell:gesture:notifications");
                    }
                    return Ok(None);
                }
                if matches!(panel, Some("quick" | "notifications")) {
                    return target("shell:dismiss");
                }
                if home {
                    return target("shell:launcher");
                }
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    pub(crate) fn desktop_panel_action(
        &mut self,
        id: &str,
        machine: &str,
        actor: &str,
        target: &str,
    ) -> Result<Option<Value>> {
        // A gesture named rather than dragged: what the same swipe would do right now.
        if let Some(gesture) = target.strip_prefix("shell:gesture:") {
            let theme = self.desktop_theme(id, machine);
            let d = &self.session(id)?.machines[machine].desktop;
            let panel = d.panel.as_deref();
            let next = match gesture {
                // Up from the home indicator: a sheet pulled over the screen goes away,
                // the App Library returns to the first page, everything else goes home.
                "home" => match panel {
                    Some("quick" | "notifications" | "search") => "shell:dismiss",
                    Some("calendar") => "shell:home-page:0",
                    None if d.launcher_open => "shell:home-page:0",
                    _ => "shell:home",
                },
                "overview" if panel == Some("overview") => return Ok(Some(Value::Null)),
                "overview" => "shell:overview",
                // Android's second pull expands the shade into Quick Settings.
                "notifications"
                    if theme == Some(DesktopTheme::Android) && panel == Some("notifications") =>
                {
                    "shell:quick-settings"
                }
                "notifications" if panel == Some("notifications") => return Ok(Some(Value::Null)),
                "notifications" => "shell:notifications",
                "control-center" if panel == Some("quick") => return Ok(Some(Value::Null)),
                "control-center" => "shell:control-center",
                _ => return Err(SimError::invalid("unknown gesture")),
            };
            return self.shell_action(id, machine, actor, next).map(Some);
        }
        // A page of a paged home screen: a page dot, or a swipe between pages. It is the
        // home screen that is shown, so the App Library and any panel give way to it.
        if let Some(page) = target.strip_prefix("shell:home-page:") {
            let page: u32 = page
                .parse()
                .map_err(|_| SimError::invalid("invalid home screen page"))?;
            let desktop = &mut self.machine_mut(id, machine)?.desktop;
            desktop.home_page = page;
            desktop.launcher_open = false;
            desktop.panel = None;
            desktop.search.clear();
            return Ok(Some(json!({ "page": page })));
        }
        if let Some(name) = target.strip_prefix("shell:panel:") {
            let name = match name {
                "apple" => "apple",
                "file" => "file",
                "edit" => "edit",
                "view" => "view",
                "window" => "window",
                "help" => "help",
                "go" => "go",
                "spotlight" | "search" => "search",
                "control" | "quick" => "quick",
                "calendar" | "clock" => "calendar",
                "notifications" => "notifications",
                "settings" => "settings",
                "overview" => "overview",
                "context" => "context",
                "power" => "power",
                "format" => "format",
                "app-menu" => "app-menu",
                "app-settings" => "app-settings",
                "page" => "page",
                _ => return Err(SimError::invalid("unknown shell panel")),
            };
            let desktop = &mut self.machine_mut(id, machine)?.desktop;
            let closing = desktop.panel.as_deref() == Some(name);
            desktop.panel = if closing { None } else { Some(name.into()) };
            // A power flyout sits over an open launcher rather than replacing it, which
            // is what Start does; every other panel takes the screen.
            desktop.panel_over_launcher = !closing && name == "power" && desktop.launcher_open;
            if !desktop.panel_over_launcher {
                desktop.launcher_open = false;
            }
            desktop.search.clear();
            desktop.panel_month = 0;
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
        // Paging a calendar panel's month grid. Bounded so a held control cannot walk the
        // grid somewhere a Gregorian date cannot be computed.
        if let Some(op) = target.strip_prefix("shell:month:") {
            let desktop = &mut self.machine_mut(id, machine)?.desktop;
            desktop.panel_month = match op {
                "prev" => desktop.panel_month.saturating_sub(1).max(-1200),
                "next" => desktop.panel_month.saturating_add(1).min(1200),
                "today" => 0,
                _ => return Err(SimError::invalid("unknown month interaction")),
            };
            return Ok(Some(json!({ "month": desktop.panel_month })));
        }
        // Save the page on screen, capture the screen, or hand something to another app.
        if target == "shell:download" {
            let url = self.session(id)?.machines[machine]
                .browser
                .url()
                .unwrap_or_default()
                .to_owned();
            if url.is_empty() {
                return Err(SimError::invalid("there is no page to save"));
            }
            let window = self.session(id)?.machines[machine]
                .desktop
                .focused
                .unwrap_or_default();
            self.effects(
                id,
                machine,
                actor,
                vec![cw_applications::AppEffect::Download { window, url }],
            )?;
            return Ok(Some(Value::Null));
        }
        if target == "shell:screenshot" {
            let window = self.session(id)?.machines[machine]
                .desktop
                .focused
                .unwrap_or_default();
            self.effects(
                id,
                machine,
                actor,
                vec![cw_applications::AppEffect::Screenshot {
                    window,
                    path: String::new(),
                }],
            )?;
            return Ok(Some(Value::Null));
        }
        // Sharing is a real hand-off: the thing on screen opens in a messaging app.
        if let Some(via) = target.strip_prefix("shell:share") {
            let via = via.trim_start_matches(':');
            let kind = if via.is_empty() { "chat" } else { via };
            if !matches!(kind, "chat" | "mail") {
                return Err(SimError::invalid("nothing here can receive a share"));
            }
            if !self.runtime.computer(machine)?.application_available(kind) {
                return Err(SimError::not_found("no application to share with"));
            }
            let state = &self.session(id)?.machines[machine];
            let subject = state
                .desktop
                .focused
                .and_then(|w| state.desktop.windows.get(&w))
                .map(|w| match &w.state {
                    // The selected item, or the folder itself when nothing is selected.
                    AppState::Files { .. } => w
                        .state
                        .file_tab()
                        .and_then(|t| t.selected_path())
                        .unwrap_or_else(|| w.state.file_path().to_owned()),
                    // An editor's document is a real thing to hand over, and it is what
                    // a proxy icon would drag if anything consumed a drag.
                    AppState::Editor { path, .. } => path.clone(),
                    AppState::Native(app) if !app.document().is_empty() => app.document(),
                    _ => state.browser.url().unwrap_or_default().to_owned(),
                })
                .unwrap_or_default();
            if subject.is_empty() {
                return Err(SimError::invalid("there is nothing to share"));
            }
            // The receiving application opens with the thing already in hand: a Messages
            // draft, or a Mail message being written, that the actor still has to send.
            let result = self.dispatch(
                id,
                actor,
                &ActionEnvelope::new(
                    "application.v1",
                    "launch",
                    machine,
                    json!({"kind": kind, "argument": subject}),
                ),
            )?;
            self.machine_mut(id, machine)?.desktop.notify(
                kind,
                "Ready to share",
                &subject,
                Some(format!("shell:launch:{kind}")),
            );
            return Ok(Some(result));
        }
        // The Trash is a folder, so opening it is opening a folder.
        if target == "shell:trash" {
            let home = self.session(id)?.machines[machine].desktop.home_folder();
            let path = format!("{}/.local/share/Trash/files", home.trim_end_matches('/'));
            return self
                .dispatch(
                    id,
                    actor,
                    &ActionEnvelope::new(
                        "application.v1",
                        "launch",
                        machine,
                        json!({"kind":"files","argument":path}),
                    ),
                )
                .map(Some);
        }
        // Virtual desktops.
        if let Some(op) = target.strip_prefix("shell:workspace:") {
            let desktop = &mut self.machine_mut(id, machine)?.desktop;
            let result = match op {
                "new" => desktop.add_workspace().map(|_| ()),
                "close" => {
                    let here = desktop.workspace;
                    desktop.close_workspace(here)
                }
                index => match index.strip_prefix("move:") {
                    Some(index) => {
                        let index = index
                            .parse()
                            .map_err(|_| "invalid desktop".to_string())
                            .and_then(|i| {
                                desktop
                                    .focused
                                    .ok_or("no window to move".into())
                                    .map(|w| (w, i))
                            });
                        index.and_then(|(w, i)| desktop.move_to_workspace(w, i))
                    }
                    None => index
                        .parse()
                        .map_err(|_| "invalid desktop".to_string())
                        .and_then(|i| desktop.switch_workspace(i)),
                },
            };
            result.map_err(SimError::invalid)?;
            let workspace = self.machine_mut(id, machine)?.desktop.workspace;
            self.sync_desktop_visibility(id, machine)?;
            return Ok(Some(json!({ "workspace": workspace })));
        }
        // Bookmarks, shared by every browser window on the machine.
        if let Some(op) = target.strip_prefix("shell:bookmark") {
            let page = self.session(id)?.machines[machine]
                .browser
                .page()
                .map(|p| p.title.clone())
                .unwrap_or_default();
            let url = self.session(id)?.machines[machine]
                .browser
                .url()
                .unwrap_or_default()
                .to_owned();
            let desktop = &mut self.machine_mut(id, machine)?.desktop;
            match op {
                "" | ":toggle" => {
                    if desktop.bookmarked(&url) {
                        desktop.remove_bookmark(&url).map_err(SimError::invalid)?;
                    } else {
                        desktop.bookmark(&page, &url).map_err(SimError::invalid)?;
                    }
                    return Ok(Some(json!({ "bookmarked": desktop.bookmarked(&url) })));
                }
                rest => {
                    let index: usize = rest
                        .strip_prefix(":open:")
                        .and_then(|i| i.parse().ok())
                        .ok_or_else(|| SimError::invalid("unknown bookmark interaction"))?;
                    let target = desktop
                        .bookmarks
                        .get(index)
                        .ok_or_else(|| SimError::not_found("bookmark"))?
                        .url
                        .clone();
                    return self
                        .browser_action(
                            id,
                            actor,
                            &ActionEnvelope::new(
                                "browser.v1",
                                "navigate",
                                machine,
                                json!({ "url": target }),
                            ),
                        )
                        .map(Some);
                }
            }
        }
        // Notifications the machine really posted.
        if target == "shell:notifications:seen" {
            self.machine_mut(id, machine)?.desktop.mark_notices_seen();
            return Ok(Some(Value::Null));
        }
        if let Some(index) = target.strip_prefix("shell:notice:") {
            let index: usize = index
                .parse()
                .map_err(|_| SimError::invalid("invalid notice"))?;
            let action = self.session(id)?.machines[machine]
                .desktop
                .notifications
                .get(index)
                .ok_or_else(|| SimError::not_found("notice"))?
                .action
                .clone();
            if let Some(notice) = self
                .machine_mut(id, machine)?
                .desktop
                .notifications
                .get_mut(index)
            {
                notice.seen = true;
            }
            return match action {
                Some(action) => self.shell_action(id, machine, actor, &action).map(Some),
                None => Ok(Some(Value::Null)),
            };
        }
        // A launcher category the user expanded.
        if let Some(group) = target.strip_prefix("shell:group:") {
            let desktop = &mut self.machine_mut(id, machine)?.desktop;
            desktop.library_group = match (group, desktop.library_group.as_deref()) {
                ("", _) => None,
                (g, Some(open)) if open == g => None,
                (g, _) => Some(g.to_owned()),
            };
            return Ok(Some(Value::Null));
        }
        // Power controls really change what the display shows.
        if let Some(op) = target.strip_prefix("shell:power:") {
            let desktop = &mut self.machine_mut(id, machine)?.desktop;
            desktop.panel = None;
            desktop.launcher_open = false;
            match op {
                "lock" => desktop.screen = cw_applications::ScreenState::Locked,
                "off" | "shutdown" => {
                    desktop.home();
                    desktop.windows.clear();
                    desktop.stacking.clear();
                    desktop.screen = cw_applications::ScreenState::Off;
                }
                "restart" => {
                    desktop.home();
                    desktop.windows.clear();
                    desktop.stacking.clear();
                    desktop.screen = cw_applications::ScreenState::Locked;
                }
                "wake" | "unlock" => desktop.screen = cw_applications::ScreenState::Active,
                _ => return Err(SimError::invalid("unknown power action")),
            }
            self.sync_desktop_visibility(id, machine)?;
            return Ok(Some(Value::Null));
        }
        // Real device switches and levels: quick settings, control centre and the shade.
        // GNOME Terminal's Reset and Clear: the focused terminal's scrollback is emptied,
        // exactly as running `clear` in it does.
        if target == "shell:terminal:clear" {
            let desktop = &mut self.machine_mut(id, machine)?.desktop;
            let window = desktop
                .focused
                .ok_or_else(|| SimError::invalid("no terminal"))?;
            match desktop.windows.get_mut(&window).map(|w| &mut w.state) {
                Some(AppState::Terminal {
                    transcript, scroll, ..
                }) => {
                    transcript.clear();
                    *scroll = 0;
                }
                _ => return Err(SimError::invalid("the focused window is not a terminal")),
            }
            desktop.close_menu();
            return Ok(Some(Value::Null));
        }
        // Page zoom for the browser on screen: `shell:zoom:in|out|reset`.
        if let Some(step) = target.strip_prefix("shell:zoom:") {
            let step = step.to_owned();
            let m = self.machine_mut(id, machine)?;
            let zoom = m.browser.step_zoom(&step)?;
            m.desktop.close_menu();
            return Ok(Some(json!({ "zoom": zoom })));
        }
        if let Some(name) = target.strip_prefix("shell:toggle:") {
            let name = name.to_owned();
            let desktop = &mut self.machine_mut(id, machine)?.desktop;
            let value = desktop.settings.toggle(&name).map_err(SimError::invalid)?;
            // A menu entry closes its menu; a switch in a settings flyout stays put.
            desktop.close_menu();
            return Ok(Some(json!({ "setting": name, "value": value })));
        }
        if let Some(rest) = target.strip_prefix("shell:set:") {
            let (name, percent) = rest
                .split_once(':')
                .ok_or_else(|| SimError::invalid("system level requires a value"))?;
            let percent: u8 = percent
                .parse()
                .map_err(|_| SimError::invalid("invalid system level"))?;
            let name = name.to_owned();
            self.machine_mut(id, machine)?
                .desktop
                .settings
                .set_level(&name, percent)
                .map_err(SimError::invalid)?;
            return Ok(Some(json!({ "setting": name, "value": percent.min(100) })));
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
                // The file manager's own back stack: the folder above, until the root.
                let inside_folder = state
                    .desktop
                    .focused
                    .and_then(|window| state.desktop.windows.get(&window))
                    .is_some_and(|window| {
                        matches!(window.state, AppState::Files { .. })
                            && !window.state.file_path().trim_end_matches('/').is_empty()
                    });
                if inside_folder {
                    let effects = self
                        .machine_mut(id, machine)?
                        .desktop
                        .click("files-up")
                        .map_err(SimError::invalid)?;
                    self.effects(id, machine, actor, effects)?;
                    return Ok(Some(Value::Null));
                }
                self.machine_mut(id, machine)?.desktop.home();
                self.sync_desktop_visibility(id, machine)?;
                Ok(Some(Value::Null))
            }
            "shell:dismiss" => {
                let desktop = &mut self.machine_mut(id, machine)?.desktop;
                desktop.panel = None;
                // Closing a flyout that sat over the launcher returns to the launcher.
                desktop.launcher_open = desktop.panel_over_launcher;
                desktop.panel_over_launcher = false;
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
