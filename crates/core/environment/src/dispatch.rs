use super::scene::{
    presented, FINGER_STOPPED_US, FRAME_US, PULL_TO_REFRESH, SWIPE_SLOP, TOUCH_STEP,
};
use super::*;

impl Environment {
    pub(super) fn machine_mut(&mut self, id: &str, machine: &str) -> Result<&mut MachineSession> {
        Arc::make_mut(&mut self.sessions)
            .get_mut(id)
            .and_then(|s| s.machines.get_mut(machine))
            .ok_or_else(|| SimError::denied("machine unavailable"))
    }
    pub(super) fn dispatch(
        &mut self,
        id: &str,
        actor: &str,
        action: &ActionEnvelope,
    ) -> Result<Value> {
        let machine = &action.machine;
        let p = &action.payload;
        Arc::make_mut(&mut self.sessions)
            .get_mut(id)
            .unwrap()
            .focused_machine = machine.clone();
        let tick = self.runtime.tick();
        if let Ok(state) = self.machine_mut(id, machine) {
            state.desktop.clock_us = tick;
        }
        match (action.family.as_str(), action.op.as_str()) {
            ("terminal.v1", "execute") => {
                let mut result = self
                    .runtime
                    .execute(machine, actor, string(p, "command")?)?;
                self.open_from_shell(id, machine, actor, &mut result)?;
                let value = serde_json::to_value(&result)?;
                self.machine_mut(id, machine)?.terminal = value.clone();
                Ok(value)
            }
            ("filesystem.v1", "read") => {
                let bytes = self.runtime.read_file(machine, string(p, "path")?)?;
                Ok(json!({"content":String::from_utf8_lossy(&bytes),"bytes":bytes}))
            }
            ("filesystem.v1", "write") => {
                self.runtime.write_file(
                    machine,
                    actor,
                    string(p, "path")?,
                    string(p, "content")?.as_bytes(),
                )?;
                Ok(Value::Null)
            }
            ("filesystem.v1", "list") => {
                let c = self.runtime.computer(machine)?;
                let path = c.resolve(string(p, "path")?);
                c.vfs
                    .check_access(&path, &c.user, true, false, true)
                    .map_err(|_| SimError::denied("directory access denied"))?;
                Ok(json!(c
                    .vfs
                    .list(&path)
                    .map_err(|_| SimError::not_found("directory"))?))
            }
            ("filesystem.v1", "stat") => {
                let c = self.runtime.computer(machine)?;
                let path = c.resolve(string(p, "path")?);
                Ok(serde_json::to_value(
                    c.vfs.stat(&path).map_err(|_| SimError::not_found("path"))?,
                )?)
            }
            ("http.v1", "request") => {
                let request: HttpRequest = serde_json::from_value(p.clone())?;
                Ok(serde_json::to_value(
                    self.runtime.http(machine, actor, request)?,
                )?)
            }
            ("browser.v1", _) => self.browser_action(id, actor, action),
            ("application.v1", "event") => self.custom_event(id, machine, actor, p),
            // What is on this machine, what this session may open, and where it cannot
            // the documented reason why. An agent should never have to guess an id.
            ("application.v1", "list") => {
                let all = p.get("installed").and_then(Value::as_bool) == Some(false);
                Ok(Value::Array(
                    self.application_inventory(id, machine)
                        .into_iter()
                        .filter(|entry| entry.installed || all)
                        .map(|entry| {
                            json!({
                                "id": entry.id,
                                "label": entry.label,
                                "kind": entry.kind,
                                "installed": entry.installed,
                                "launchable": entry.launchable,
                                "blocked_by": entry.blocked_by,
                            })
                        })
                        .collect(),
                ))
            }
            ("application.v1", "launch") => {
                let requested = string(p, "kind")?;
                let alias = self.desktop_alias(id, machine, requested)?;
                let canonical = match requested {
                    "text_editor" => "editor",
                    "file_manager" => "files",
                    other => other,
                };
                if canonical == "browser"
                    && !self
                        .session(id)?
                        .config
                        .actions
                        .iter()
                        .any(|family| family == "browser.v1")
                {
                    return Err(SimError::denied(
                        "browser application interaction is not permitted",
                    )
                    .because(cw_protocol::reason::BROWSER_FAMILY_REQUIRED));
                }
                let computer = self.runtime.computer(machine)?;
                if alias.is_none()
                    && !computer.application_available(requested)
                    && !computer.application_available(canonical)
                {
                    return Err(SimError::not_found("application is not installed")
                        .because(cw_protocol::reason::APPLICATION_NOT_INSTALLED));
                }
                if self.app_registry.application(string(p, "kind")?).is_ok() {
                    return self.custom_launch(id, machine, actor, p);
                }
                self.machine_mut(id, machine)?.active_app = None;
                self.machine_mut(id, machine)?.custom_page = None;
                self.machine_mut(id, machine)?.browser_visible = false;
                self.machine_mut(id, machine)?.address_focused = false;
                self.machine_mut(id, machine)?.focused_input = None;
                let kind = if alias.is_some() {
                    "browser"
                } else {
                    requested
                };
                // A native application is told which service backs it in this world.
                let native = cw_applications::NativeApp::KINDS.contains(&kind);
                let supplied = p.get("argument").and_then(Value::as_str).unwrap_or("");
                let world_url = if native {
                    self.native_argument(id, machine, kind)
                } else {
                    String::new()
                };
                // A native application's argument is the service it talks to. Opening one
                // *on* something — a calendar on a date — keeps that service and carries
                // the place as a fragment, the web's own way to say "here, at this spot".
                let from_world = match (world_url.is_empty(), supplied.is_empty()) {
                    (false, true) => world_url,
                    (false, false) if world_url.starts_with("http") => {
                        format!("{world_url}#{supplied}")
                    }
                    _ => String::new(),
                };
                let arg =
                    alias
                        .as_ref()
                        .map(|a| a.url.as_str())
                        .unwrap_or(if from_world.is_empty() {
                            supplied
                        } else {
                            &from_world
                        });
                // Files, Finder and Explorer open on the user's home; a phone's file
                // browser opens on its storage root.
                let home;
                let arg = if arg.is_empty()
                    && matches!(kind, "files" | "file_manager")
                    && self.desktop_theme(id, machine).is_some_and(|t| !t.mobile())
                {
                    home = self.session(id)?.machines[machine].desktop.home_folder();
                    home.as_str()
                } else {
                    arg
                };
                // GNOME Files opens in its icon grid; the list is a toggle away.
                let grid = self.desktop_theme(id, machine) == Some(DesktopTheme::Ubuntu);
                self.machine_mut(id, machine)?.desktop.file_view = if grid {
                    cw_applications::FileView::Grid
                } else {
                    cw_applications::FileView::List
                };
                let (window, effects) = self
                    .machine_mut(id, machine)?
                    .desktop
                    .launch(kind, arg)
                    .map_err(SimError::invalid)?;
                let empty_argument = arg.is_empty();
                if let Some(alias) = alias {
                    let w = self
                        .machine_mut(id, machine)?
                        .desktop
                        .windows
                        .get_mut(&window)
                        .unwrap();
                    w.app_id = alias.id;
                    w.title = alias.label;
                }
                self.sync_desktop_visibility(id, machine)?;
                self.effects(id, machine, actor, effects)?;
                if kind == "browser" && self.desktop_theme(id, machine).is_some() {
                    let state = self.machine_mut(id, machine)?;
                    state.browser_visible = true;
                    state.address_focused = empty_argument;
                }
                Ok(json!({"window":window}))
            }
            ("application.v1", "home" | "launcher" | "minimize" | "maximize" | "switcher") => {
                self.shell_action(id, machine, actor, &format!("shell:{}", action.op))
            }
            // Invoke a shell control by name. The same targets a pointer reaches by
            // hit-testing, addressable directly, through the identical handler and grants.
            ("application.v1", "shell") => {
                let target = string(p, "target")?.to_owned();
                if !target.starts_with("shell:") && !target.starts_with("window:") {
                    return Err(SimError::invalid("not a shell interaction target")
                        .because(cw_protocol::reason::UNKNOWN_SHELL_TARGET));
                }
                if let Some(rest) = target.strip_prefix("window:") {
                    // Window-namespaced controls belong to the window they name.
                    let (window, operation) = rest
                        .split_once(':')
                        .ok_or_else(|| SimError::invalid("invalid window interaction"))?;
                    let window: u64 = window
                        .parse()
                        .map_err(|_| SimError::invalid("invalid window id"))?;
                    self.machine_mut(id, machine)?
                        .desktop
                        .focus(window)
                        .map_err(SimError::invalid)?;
                    self.sync_desktop_visibility(id, machine)?;
                    let inner = operation.strip_prefix("content:").unwrap_or(operation);
                    if inner.starts_with("shell:") {
                        return self.shell_action(id, machine, actor, inner);
                    }
                    let effects = self
                        .machine_mut(id, machine)?
                        .desktop
                        .click(inner)
                        .map_err(SimError::invalid)?;
                    self.effects(id, machine, actor, effects)?;
                    self.sync_desktop_visibility(id, machine)?;
                    return Ok(Value::Null);
                }
                self.shell_action(id, machine, actor, &target)
            }
            ("application.v1", "focus" | "close") => {
                self.machine_mut(id, machine)?.browser_visible = false;
                self.machine_mut(id, machine)?.active_app = None;
                self.machine_mut(id, machine)?.custom_page = None;
                self.machine_mut(id, machine)?.focused_input = None;
                let window = p
                    .get("window")
                    .and_then(integer_u64)
                    .ok_or_else(|| SimError::invalid("window required"))?;
                let d = &mut self.machine_mut(id, machine)?.desktop;
                match action.op.as_str() {
                    "focus" => d.focus(window),
                    _ => d.close(window),
                }
                .map_err(SimError::invalid)?;
                self.sync_desktop_visibility(id, machine)?;
                Ok(Value::Null)
            }
            ("keyboard.v1", "type") => {
                if self.desktop_panel_text(id, machine, string(p, "text")?)? {
                    return Ok(Value::Null);
                }
                if self.machine_mut(id, machine)?.active_app.is_some() {
                    return self.custom_event(id, machine, actor, &json!({"kind":"text","data":p}));
                }
                let text = string(p, "text")?;
                if self.machine_mut(id, machine)?.address_focused {
                    self.machine_mut(id, machine)?
                        .desktop
                        .text(text)
                        .map_err(SimError::invalid)?;
                } else if self.machine_mut(id, machine)?.browser_visible {
                    self.prime_browsers(id, machine);
                    let runtime = &mut self.runtime;
                    let m = Arc::make_mut(&mut self.sessions)
                        .get_mut(id)
                        .and_then(|s| s.machines.get_mut(machine))
                        .ok_or_else(|| SimError::denied("machine unavailable"))?;
                    let mut http = |r| runtime.http(machine, actor, r);
                    m.browser.text_with(text, &mut http)?;
                } else {
                    let effects = self
                        .machine_mut(id, machine)?
                        .desktop
                        .type_text(text)
                        .map_err(SimError::invalid)?;
                    self.effects(id, machine, actor, effects)?;
                }
                Ok(Value::Null)
            }
            ("keyboard.v1", "key") => {
                let key = string(p, "key")?;
                if self.desktop_panel_key(id, machine, actor, key)? {
                    return Ok(Value::Null);
                }
                if matches!(key, "Meta" | "Super" | "Meta+Space" | "Ctrl+Escape")
                    && self.desktop_theme(id, machine).is_some()
                {
                    return self.shell_action(id, machine, actor, "shell:launcher");
                }
                if key == "Alt+Tab" && self.desktop_theme(id, machine).is_some() {
                    return self.shell_action(id, machine, actor, "shell:switcher");
                }
                if self.machine_mut(id, machine)?.address_focused {
                    if key == "Enter"
                        && !self
                            .session(id)?
                            .config
                            .actions
                            .iter()
                            .any(|family| family == "browser.v1")
                    {
                        return Err(SimError::denied("browser navigation is not permitted"));
                    }
                    let effects = self
                        .machine_mut(id, machine)?
                        .desktop
                        .key(key)
                        .map_err(SimError::invalid)?;
                    self.effects(id, machine, actor, effects)?;
                    if key == "Enter" {
                        self.machine_mut(id, machine)?.address_focused = false;
                    }
                    return Ok(Value::Null);
                }
                if self.machine_mut(id, machine)?.active_app.is_some() {
                    return self.custom_event(id, machine, actor, &json!({"kind":"key","data":p}));
                }
                if self.machine_mut(id, machine)?.browser_visible {
                    // Browser zoom chords, on both platforms' modifier.
                    let step = match key {
                        "Ctrl+=" | "Ctrl++" | "Meta+=" | "Meta++" => Some("in"),
                        "Ctrl+-" | "Meta+-" => Some("out"),
                        "Ctrl+0" | "Meta+0" => Some("reset"),
                        _ => None,
                    };
                    if let Some(step) = step {
                        return self.shell_action(
                            id,
                            machine,
                            actor,
                            &format!("shell:zoom:{step}"),
                        );
                    }
                    return self.browser_action(
                        id,
                        actor,
                        &ActionEnvelope::new("browser.v1", "key", machine, p.clone()),
                    );
                }
                let effects = self
                    .machine_mut(id, machine)?
                    .desktop
                    .key(string(p, "key")?)
                    .map_err(SimError::invalid)?;
                self.effects(id, machine, actor, effects)?;
                Ok(Value::Null)
            }
            ("pointer.v1", "wheel") => {
                let coord = |k: &str| -> Result<i32> {
                    Ok(p.get(k)
                        .and_then(integer_i64)
                        .ok_or_else(|| SimError::invalid(format!("{k} required")))?
                        .clamp(-32768, 32768) as i32)
                };
                let (x, y) = (coord("x")?, coord("y")?);
                let (width, height) = self.pointer_size(id, machine, p)?;
                let delta = |k: &str| {
                    p.get(k)
                        .and_then(integer_i64)
                        .unwrap_or(0)
                        .clamp(-100_000, 100_000) as i32
                };
                let held = |name: &str| {
                    p.get("modifiers")
                        .and_then(Value::as_array)
                        .is_some_and(|m| {
                            m.iter()
                                .filter_map(Value::as_str)
                                .any(|m| m.eq_ignore_ascii_case(name))
                        })
                };
                let wheel = cw_applications::Wheel {
                    dx: delta("delta_x"),
                    dy: delta("delta_y"),
                    shift: held("shift"),
                    ctrl: held("ctrl") || held("control") || held("meta"),
                };
                self.machine_mut(id, machine)?.pointer_position = Some((x, y));
                self.record_viewport(id, machine, p, (width, height))?;
                let handled = self.scroll_at(id, machine, (x, y), (width, height), wheel)?;
                Ok(json!({"handled": handled}))
            }
            ("pointer.v1", "click" | "down" | "move" | "up" | "cancel" | "double_click") => {
                let x = p
                    .get("x")
                    .and_then(integer_i64)
                    .ok_or_else(|| SimError::invalid("x required"))?
                    .clamp(-32768, 32768) as i32;
                let y = p
                    .get("y")
                    .and_then(integer_i64)
                    .ok_or_else(|| SimError::invalid("y required"))?
                    .clamp(-32768, 32768) as i32;
                let (width, height) = self.pointer_size(id, machine, p)?;
                self.machine_mut(id, machine)?.pointer_position = Some((x, y));
                self.record_viewport(id, machine, p, (width, height))?;
                // Keys held with the pointer (`["ctrl"]`, `["alt"]`…): an image editor's
                // Ctrl- or Option-click sets a clone source.
                let modifiers = match p.get("modifiers") {
                    None | Some(Value::Null) => 0,
                    Some(Value::Array(names)) => {
                        let names: Vec<&str> = names.iter().filter_map(Value::as_str).collect();
                        if names.len() != p["modifiers"].as_array().map_or(0, Vec::len) {
                            return Err(SimError::invalid("modifiers are key names"));
                        }
                        cw_applications::modifier_bits(&names).map_err(SimError::invalid)?
                    }
                    Some(_) => return Err(SimError::invalid("modifiers are a list of key names")),
                };
                self.machine_mut(id, machine)?.desktop.pointer_modifiers = modifiers;
                self.machine_mut(id, machine)?.desktop.pointer_button =
                    p.get("button").and_then(integer_u64).unwrap_or(0).min(2) as u8;
                let released_press = if action.op == "up" {
                    self.machine_mut(id, machine)?.pointer_press.take()
                } else {
                    None
                };
                if matches!(action.op.as_str(), "down" | "cancel") {
                    self.machine_mut(id, machine)?.pointer_press = None;
                }
                let theme = self.desktop_theme(id, machine);
                let area = theme.map(|theme| work_area(theme, width, height));
                let mobile = matches!(theme, Some(DesktopTheme::Ios | DesktopTheme::Android));
                if mobile && action.op == "down" {
                    self.machine_mut(id, machine)?.touch_start = Some((x, y));
                    self.end_touch_scroll(id, machine)?;
                }
                if action.op == "cancel" {
                    self.machine_mut(id, machine)?.touch_start = None;
                    self.end_touch_scroll(id, machine)?;
                }
                // A drag surface inside an application (a canvas, a slider) holds the
                // pointer from press to release: moves and the release go to it before
                // any shell gesture or hit test, even when they leave its bounds.
                if matches!(action.op.as_str(), "move" | "up" | "cancel")
                    && self.session(id)?.machines[machine].desktop.app_captured()
                {
                    let phase = match action.op.as_str() {
                        "move" => cw_applications::PointerPhase::Move,
                        "up" => cw_applications::PointerPhase::Up,
                        _ => cw_applications::PointerPhase::Cancel,
                    };
                    if phase != cw_applications::PointerPhase::Move {
                        self.machine_mut(id, machine)?.touch_start = None;
                    }
                    let delivered = self
                        .machine_mut(id, machine)?
                        .desktop
                        .app_pointer(phase, x, y);
                    if let Some(result) = delivered {
                        let effects = result.map_err(SimError::invalid)?;
                        self.effects(id, machine, actor, effects)?;
                        return Ok(if phase == cw_applications::PointerPhase::Move {
                            json!({"cursor":"crosshair"})
                        } else {
                            Value::Null
                        });
                    }
                }
                // A finger that has taken a list moves it with every sample, and on
                // release lets it fling; neither is a tap or a shell gesture.
                if let (true, Some(theme)) = (mobile, theme) {
                    if action.op == "move" {
                        if let Some(value) =
                            self.touch_scroll_move(id, machine, (x, y), (width, height), theme)?
                        {
                            return Ok(value);
                        }
                    }
                    if action.op == "up"
                        && self.session(id)?.machines[machine].touch_scroll.is_some()
                    {
                        self.machine_mut(id, machine)?.touch_start = None;
                        self.touch_scroll_release(id, machine, y, (width, height), theme)?;
                        return Ok(Value::Null);
                    }
                }
                if mobile && action.op == "up" {
                    if let Some(start) = self.machine_mut(id, machine)?.touch_start.take() {
                        let pressed = released_press.as_ref().map(|(target, _)| target.as_str());
                        let gesture = self.touch_gesture(
                            id,
                            machine,
                            theme.expect("a phone theme"),
                            start,
                            (x, y),
                            (width, height),
                            pressed,
                        )?;
                        // A drag that is no shell gesture and starts inside an
                        // application scrolls what is under the finger, as on every
                        // phone: the content follows the finger, so an upward swipe
                        // moves further down the list.
                        let (dx, dy) = (x - start.0, y - start.1);
                        if gesture.is_none() && dy.abs() > SWIPE_SLOP && dy.abs() >= dx.abs() {
                            self.scroll_at(
                                id,
                                machine,
                                start,
                                (width, height),
                                cw_applications::Wheel::vertical(-dy),
                            )?;
                            return Ok(Value::Null);
                        }
                        // A sideways drag moves a shelf that scrolls sideways under it.
                        if gesture.is_none()
                            && dx.abs() > SWIPE_SLOP
                            && self.scroll_at(
                                id,
                                machine,
                                start,
                                (width, height),
                                cw_applications::Wheel {
                                    dx: -dx,
                                    ..Default::default()
                                },
                            )?
                        {
                            return Ok(Value::Null);
                        }
                        if let Some(target) = gesture {
                            // A card swiped up in the overview closes that application.
                            if let Some(window) = target
                                .strip_prefix("window:")
                                .and_then(|rest| rest.strip_suffix(":close"))
                                .and_then(|window| window.parse::<u64>().ok())
                            {
                                if !self
                                    .session(id)?
                                    .config
                                    .actions
                                    .iter()
                                    .any(|family| family == "application.v1")
                                {
                                    return Err(SimError::denied(
                                        "application interaction is not permitted",
                                    )
                                    .because(cw_protocol::reason::APPLICATION_FAMILY_REQUIRED));
                                }
                                self.machine_mut(id, machine)?
                                    .desktop
                                    .close(window)
                                    .map_err(SimError::invalid)?;
                                self.sync_desktop_visibility(id, machine)?;
                                return Ok(Value::Null);
                            }
                            return self.shell_action(id, machine, actor, &target);
                        }
                    }
                }
                if action.op == "down" && p.get("button").and_then(integer_u64) == Some(2) {
                    // A right-drag on an application's drag surface that uses it (a 3D
                    // view pans) belongs to the application, not the context menu.
                    let scene = self.scene(id, width, height)?;
                    let secondary = scene
                        .hit_test(x, y)
                        .and_then(|n| n.interaction.as_deref())
                        .and_then(|t| t.strip_prefix("window:"))
                        .and_then(|t| t.split_once(':'))
                        .and_then(|(w, op)| {
                            Some((
                                w.parse::<u64>().ok()?,
                                op.strip_prefix("content:")?.to_owned(),
                            ))
                        })
                        .is_some_and(|(w, inner)| {
                            self.session(id).is_ok_and(|s| {
                                s.machines[machine].desktop.app_takes_secondary(w, &inner)
                            })
                        });
                    if !secondary {
                        return self.shell_action(id, machine, actor, "shell:panel:context");
                    }
                }
                if action.op == "cancel" {
                    self.machine_mut(id, machine)?.desktop.pointer_capture = None;
                    return Ok(Value::Null);
                }
                if let Some(area) = area {
                    if matches!(action.op.as_str(), "move" | "up") {
                        let desktop = &mut self.machine_mut(id, machine)?.desktop;
                        let captured = if action.op == "move" {
                            desktop.pointer_move(x, y, area)
                        } else {
                            desktop.pointer_up(x, y, area)
                        }
                        .map_err(SimError::invalid)?;
                        if captured {
                            let cursor = self.session(id)?.machines[machine]
                                .desktop
                                .pointer_capture
                                .as_ref()
                                .map(|capture| CursorKind::for_target(&capture.operation, true))
                                .unwrap_or(CursorKind::Default)
                                .css_name();
                            return Ok(json!({"cursor":cursor}));
                        }
                    }
                }
                // The secondary button opens a menu on the press. Its release activates
                // nothing, so it never clicks what happens to be under the pointer.
                if action.op == "up" && p.get("button").and_then(integer_u64) == Some(2) {
                    return Ok(Value::Null);
                }
                let scene = self.scene(id, width, height)?;
                if action.op == "move" {
                    // A canvas that draws what is being placed under the pointer (a wire,
                    // a track) follows it even with no button down.
                    if let Some((window, target, bounds)) = scene.hit_test(x, y).and_then(|n| {
                        let (window, rest) = n
                            .interaction
                            .as_deref()?
                            .strip_prefix("window:")?
                            .split_once(':')?;
                        let target = rest.strip_prefix("content:")?;
                        Some((
                            window.parse::<u64>().ok()?,
                            target.to_owned(),
                            n.transform.bounds(n.bounds),
                        ))
                    }) {
                        if self.session(id)?.machines[machine]
                            .desktop
                            .app_hovers(window, &target)
                        {
                            let effects = self.machine_mut(id, machine)?.desktop.app_hover(
                                window,
                                &target,
                                x - bounds.x,
                                y - bounds.y,
                            );
                            self.effects(id, machine, actor, effects)?;
                            let cursor = self.session(id)?.machines[machine]
                                .desktop
                                .app_hover_cursor(window, &target);
                            return Ok(json!({"cursor":cursor}));
                        }
                    }
                    if let Some(cursor) = self.browser_hover(id, machine, &scene, x, y)? {
                        return Ok(json!({"cursor":cursor}));
                    }
                    self.machine_mut(id, machine)?.pointer_cursor = None;
                    let cursor = scene
                        .hit_test(x, y)
                        .and_then(|n| n.interaction.as_deref())
                        .map_or(CursorKind::Default, |target| {
                            CursorKind::for_target(target, false)
                        });
                    return Ok(json!({"cursor":cursor.css_name()}));
                }
                let (mut target, hit) = if action.op == "up" {
                    let Some((target, bounds)) = released_press else {
                        return Ok(Value::Null);
                    };
                    if !bounds.contains(x, y) {
                        return Ok(Value::Null);
                    }
                    (target, bounds)
                } else {
                    let Some(node) = scene.hit_test(x, y) else {
                        return Ok(Value::Null);
                    };
                    let Some(target) = node.interaction.clone() else {
                        return Ok(Value::Null);
                    };
                    let bounds = node.transform.bounds(node.bounds);
                    if action.op == "down" {
                        self.machine_mut(id, machine)?.pointer_press =
                            Some((target.clone(), bounds));
                    }
                    (target, bounds)
                };
                // Choosing an entry in a drop-down menu closes the menu, whatever the entry
                // does; clicks on the menu's own body or on another menu title do not.
                if !matches!(action.op.as_str(), "down" | "move" | "cancel")
                    && target != "shell:noop"
                    && !target.starts_with("shell:panel:")
                    && !target.starts_with("shell:gesture:")
                {
                    self.machine_mut(id, machine)?.desktop.close_menu();
                }
                let custom_prefix = format!("window:{}:content:", u64::MAX);
                if self.session(id)?.machines[machine].active_app.is_some()
                    && target.starts_with(&custom_prefix)
                {
                    if action.op == "down" {
                        return Ok(Value::Null);
                    }
                    return self.custom_event(id,machine,actor,&json!({"kind":"click","target":target.trim_start_matches(&custom_prefix),"data":p}));
                }
                if self.session(id)?.machines[machine].active_app.is_some()
                    && target.starts_with(&format!("window:{}:", u64::MAX))
                {
                    if action.op == "down" {
                        return Ok(Value::Null);
                    }
                    if target.ends_with(":close") || target.ends_with(":minimize") {
                        self.machine_mut(id, machine)?.active_app = None;
                        self.machine_mut(id, machine)?.custom_page = None;
                    }
                    return Ok(Value::Null);
                }
                if let Some(namespaced) = target.strip_prefix("window:") {
                    let (window, operation) = namespaced
                        .split_once(':')
                        .ok_or_else(|| SimError::invalid("invalid window interaction"))?;
                    let window: u64 = window
                        .parse()
                        .map_err(|_| SimError::invalid("invalid window id"))?;
                    let operation = operation.to_owned();
                    if !self
                        .session(id)?
                        .config
                        .actions
                        .iter()
                        .any(|family| family == "application.v1")
                    {
                        return Err(SimError::denied("application interaction is not permitted")
                            .because(cw_protocol::reason::APPLICATION_FAMILY_REQUIRED));
                    }
                    // Pressing an application's drag surface captures the pointer at
                    // once, on a phone as on a desktop: a finger drawing on a canvas is a
                    // stroke, not a shell gesture.
                    if action.op == "down" {
                        if let Some(content) = operation.strip_prefix("content:") {
                            if self.session(id)?.machines[machine]
                                .desktop
                                .app_drags(window, content)
                            {
                                let content = content.to_owned();
                                self.machine_mut(id, machine)?.touch_start = None;
                                let button =
                                    p.get("button").and_then(integer_u64).unwrap_or(0).min(2) as u8;
                                let effects = self
                                    .machine_mut(id, machine)?
                                    .desktop
                                    .app_pointer_down_with(window, &content, x, y, hit, button)
                                    .map_err(SimError::invalid)?;
                                self.sync_desktop_visibility(id, machine)?;
                                self.effects(id, machine, actor, effects)?;
                                return Ok(Value::Null);
                            }
                        }
                    }
                    // A finger coming down only presses; the release decides, so a swipe
                    // that starts on a card or a control is still free to be a gesture.
                    if mobile && action.op == "down" {
                        return Ok(Value::Null);
                    }
                    self.machine_mut(id, machine)?
                        .desktop
                        .focus(window)
                        .map_err(SimError::invalid)?;
                    self.sync_desktop_visibility(id, machine)?;
                    let area = area
                        .ok_or_else(|| SimError::invalid("window interaction requires desktop"))?;
                    if action.op == "down" {
                        if !mobile && (operation == "drag" || operation.starts_with("resize:")) {
                            self.machine_mut(id, machine)?
                                .desktop
                                .pointer_down(window, &operation, x, y, area)
                                .map_err(SimError::invalid)?;
                        }
                        // A press in a text view anchors a drag selection; the release,
                        // delivered as the click, extends it to where the pointer let go.
                        if let Some(inner) = operation.strip_prefix("content:") {
                            self.machine_mut(id, machine)?
                                .desktop
                                .press_at(inner, x - hit.x, y - hit.y)
                                .map_err(SimError::invalid)?;
                        }
                        return Ok(Value::Null);
                    }
                    if operation == "drag" && action.op == "double_click" {
                        self.machine_mut(id, machine)?
                            .desktop
                            .maximize(window, area)
                            .map_err(SimError::invalid)?;
                        return Ok(Value::Null);
                    }
                    match operation.as_str() {
                        "maximize" => {
                            self.machine_mut(id, machine)?
                                .desktop
                                .maximize(window, area)
                                .map_err(SimError::invalid)?;
                            return Ok(Value::Null);
                        }
                        "minimize" => {
                            self.machine_mut(id, machine)?
                                .desktop
                                .minimize(window)
                                .map_err(SimError::invalid)?;
                            self.sync_desktop_visibility(id, machine)?;
                            return Ok(Value::Null);
                        }
                        "close" => {
                            self.machine_mut(id, machine)?
                                .desktop
                                .close(window)
                                .map_err(SimError::invalid)?;
                            self.sync_desktop_visibility(id, machine)?;
                            return Ok(Value::Null);
                        }
                        "focus" | "drag" => return Ok(Value::Null),
                        resize if resize.starts_with("resize:") => return Ok(Value::Null),
                        content => {
                            target = content
                                .strip_prefix("content:")
                                .ok_or_else(|| SimError::invalid("unknown window interaction"))?
                                .to_owned();
                        }
                    }
                } else if action.op == "down" {
                    return Ok(Value::Null);
                }
                // Pointers on a desktop select on the first click and open on the second;
                // a touch screen has no such distinction and opens immediately.
                let opening = mobile || action.op == "double_click";
                // A gesture affordance — the home indicator, a status bar — is reached by
                // dragging from it. Tapped, it does what the glass does: nothing.
                if target.starts_with("shell:gesture:") {
                    return Ok(Value::Null);
                }
                if target.starts_with("shell:") {
                    if let Some(kind) = target.strip_prefix("shell:open:") {
                        let kind = kind.to_owned();
                        if !opening {
                            self.machine_mut(id, machine)?.desktop.desktop_selection =
                                Some(kind.clone());
                            return Ok(Value::Null);
                        }
                        self.machine_mut(id, machine)?.desktop.desktop_selection = None;
                        // The Recycle Bin is a folder, not an application.
                        let open = if kind == "trash" {
                            "shell:trash".to_owned()
                        } else {
                            format!("shell:launch:{kind}")
                        };
                        return self.shell_action(id, machine, actor, &open);
                    }
                    if action.op == "double_click" {
                        return Ok(Value::Null);
                    }
                    self.machine_mut(id, machine)?.desktop.desktop_selection = None;
                    return self.shell_action(id, machine, actor, &target);
                }
                // Double clicks mean something to file-manager rows and to applications
                // that give them a meaning of their own (a code editor's tabs and words).
                if action.op == "double_click"
                    && !target.starts_with("open:")
                    && !target.starts_with("code:")
                    && !target.starts_with("freecad:")
                    && !target.starts_with("sheet:")
                    && !target.starts_with("db:")
                    && !target.starts_with("kicad:")
                {
                    return Ok(Value::Null);
                }
                self.machine_mut(id, machine)?.address_focused = false;
                if self.machine_mut(id, machine)?.active_app.is_some() {
                    self.custom_event(
                        id,
                        machine,
                        actor,
                        &json!({"kind":"click","target":target,"data":p}),
                    )
                } else if self.machine_mut(id, machine)?.browser_visible {
                    let a =
                        ActionEnvelope::new("browser.v1", "click", machine, json!({"id":target}));
                    self.browser_action(id, actor, &a)
                } else {
                    if let Some(index) = target
                        .strip_prefix("open:")
                        .and_then(|i| i.parse::<usize>().ok())
                    {
                        let desktop = &self.session(id)?.machines[machine].desktop;
                        if let Some(window) =
                            desktop.focused.and_then(|id| desktop.windows.get(&id))
                        {
                            if let Some(tab) = window.state.file_tab() {
                                if let Some(entry) = tab.entries.get(index).filter(|_| opening) {
                                    // Folders open in the same window; a document needs the
                                    // application that opens its kind: workbooks the
                                    // spreadsheet, databases the database client, the rest
                                    // the text editor.
                                    if !entry.ends_with('/')
                                        && !self
                                            .runtime
                                            .computer(machine)?
                                            .application_available(cw_applications::opener(entry))
                                    {
                                        return Err(SimError::not_found(
                                            "application is not installed",
                                        ));
                                    }
                                }
                            }
                        }
                    }
                    let desktop = &mut self.machine_mut(id, machine)?.desktop;
                    // A tap on a spreadsheet or database grid selects, as a click does;
                    // editing takes a second tap (a double click), as on the phones' own.
                    let grid = target.starts_with("sheet:") || target.starts_with("db:");
                    let effects = if opening && !(grid && action.op != "double_click") {
                        desktop.activate(&target)
                    } else {
                        desktop.click_at(&target, x - hit.x, y - hit.y)
                    }
                    .map_err(SimError::invalid)?;
                    self.effects(id, machine, actor, effects)?;
                    self.sync_desktop_visibility(id, machine)?;
                    Ok(Value::Null)
                }
            }
            _ => {
                if let Some(extension) = self.extensions.get(&action.family).cloned() {
                    extension.execute(&mut self.runtime, actor, action)
                } else {
                    Err(SimError::invalid("unsupported action operation")
                        .because(cw_protocol::reason::UNSUPPORTED_OPERATION))
                }
            }
        }
    }
    /// Scroll whatever is under `(x, y)` by `wheel`: first the application's own use
    /// of the wheel (a canvas zooms, a grid moves by rows, a terminal walks its
    /// scrollback), then the innermost published pane that can still move that way,
    /// then the panes around it. Returns whether anything moved.
    pub(super) fn scroll_at(
        &mut self,
        id: &str,
        machine: &str,
        at: (i32, i32),
        size: (u32, u32),
        wheel: cw_applications::Wheel,
    ) -> Result<bool> {
        let moved = self.scroll_under(id, machine, at, size, wheel)?;
        if moved {
            self.machine_mut(id, machine)?.scrolled = true;
        }
        Ok(moved)
    }
    pub(super) fn scroll_under(
        &mut self,
        id: &str,
        machine: &str,
        (x, y): (i32, i32),
        (width, height): (u32, u32),
        wheel: cw_applications::Wheel,
    ) -> Result<bool> {
        let scene = self.scene(id, width, height)?;
        let Some(node) = scene.hit_test(x, y) else {
            return Ok(false);
        };
        let bounds = node.transform.bounds(node.bounds);
        let Some((window, inner)) = node
            .interaction
            .as_deref()
            .and_then(|t| t.strip_prefix("window:"))
            .and_then(|t| t.split_once(':'))
            .and_then(|(w, op)| {
                Some((
                    w.parse::<u64>().ok()?,
                    op.strip_prefix("content:").unwrap_or("").to_owned(),
                ))
            })
        else {
            return Ok(false);
        };
        if !self
            .session(id)?
            .config
            .actions
            .iter()
            .any(|family| family == "application.v1")
        {
            return Err(SimError::denied("application interaction is not permitted")
                .because(cw_protocol::reason::APPLICATION_FAMILY_REQUIRED));
        }
        if self
            .machine_mut(id, machine)?
            .desktop
            .wheel(window, &inner, x - bounds.x, y - bounds.y, wheel)
            .map_err(SimError::invalid)?
        {
            return Ok(true);
        }
        if wheel.dy == 0 && wheel.dx == 0 {
            return Ok(false);
        }
        for area in scene.scrolls_at(Some(window), x, y) {
            // A sideways pane takes the wheel's x, or its y with Shift held, as every
            // desktop does; an upright one takes y, and ignores a Shift-turn.
            let delta = match (area.horizontal, wheel.dx, wheel.shift) {
                (true, 0, true) => wheel.dy,
                (true, dx, _) => dx,
                (false, _, true) if wheel.dx == 0 => 0,
                (false, _, _) => wheel.dy,
            };
            if delta == 0 {
                continue;
            }
            let next = (area.offset.saturating_add(delta)).clamp(0, area.max_offset());
            if next == area.offset {
                continue;
            }
            let Some(pane) = area
                .target
                .split_once(":content:pane:")
                .map(|(_, pane)| pane.to_owned())
            else {
                continue;
            };
            let horizontal = area.horizontal;
            return self.set_pane_offset(id, machine, window, &pane, next, horizontal);
        }
        Ok(false)
    }
    /// The pointer moved to `(x, y)` over a browser window showing an HTML document:
    /// the document's `:hover` follows it and its computed `cursor` names the pointer
    /// shape. `None` when the point is not over such a document.
    pub(super) fn browser_hover(
        &mut self,
        id: &str,
        machine: &str,
        scene: &cw_scene::Scene,
        x: i32,
        y: i32,
    ) -> Result<Option<String>> {
        let hit_window = scene
            .hit_test(x, y)
            .and_then(|n| n.interaction.as_deref())
            .and_then(cw_scene::window_of);
        let Some((window, bounds)) = scene.scrolls_at(None, x, y).into_iter().find_map(|a| {
            let (prefix, pane) = a.target.split_once(":content:pane:")?;
            let window: u64 = prefix.strip_prefix("window:")?.parse().ok()?;
            (pane == "page" && (hit_window.is_none() || hit_window == Some(window)))
                .then_some((window, a.bounds))
        }) else {
            return Ok(None);
        };
        self.prime_browsers(id, machine);
        let actor = self.session(id)?.config.actor.clone();
        let runtime = &mut self.runtime;
        let m = Arc::make_mut(&mut self.sessions)
            .get_mut(id)
            .and_then(|s| s.machines.get_mut(machine))
            .ok_or_else(|| SimError::denied("machine unavailable"))?;
        if !matches!(
            m.desktop.windows.get(&window).map(|w| &w.state),
            Some(AppState::Browser { .. })
        ) {
            return Ok(None);
        }
        let browser = if m.active_browser_window == Some(window) {
            &mut m.browser
        } else if let Some(state) = m.browser_windows.get_mut(&window) {
            state
        } else {
            &mut m.browser
        };
        let mut http = |r| runtime.http(machine, &actor, r);
        let cursor = browser
            .hover_at_with(
                x - bounds.x,
                y - bounds.y,
                bounds.width,
                bounds.height,
                &mut http,
            )
            .map(|css| CursorKind::from_css(css).css_name().to_owned());
        if cursor.is_some() {
            m.pointer_cursor = cursor.clone();
        }
        Ok(cursor)
    }
    /// The content viewport (scene px) of the browser window on `machine`: what a
    /// scripted page is laid out for and sees as `innerWidth`/`innerHeight`.
    pub(super) fn browser_viewport(
        &self,
        id: &str,
        machine: &str,
        window: Option<u64>,
    ) -> Option<(u32, u32)> {
        let theme = self.desktop_theme(id, machine)?;
        let (width, height) = self.screen_size(id, machine).ok()?;
        let m = self.session(id).ok()?.machines.get(machine)?;
        let window = window.or(m.active_browser_window)?;
        let area = work_area(theme, width, height);
        let rect = if theme.mobile() {
            area
        } else {
            m.desktop.effective_frame(window, area)
        };
        let content = window_content_rect_for_kind(theme, rect, "browser");
        Some((content.width.max(1), content.height.max(1)))
    }
    /// What page scripts on `machine` read from the world before a browser acts: the
    /// clock, the seeded entropy their streams are named under
    /// (`computer/<machine>/browser/tab/<id>/page-script`), and the viewport.
    /// The search engine the world's browsers send omnibox queries to: whatever its
    /// definition states in `metadata.search_engine` (a template, `%s` for the query),
    /// or google.com. A world with no google.com names the engine it does have.
    pub(crate) fn world_search_engine(&self) -> String {
        self.runtime
            .definition()
            .metadata
            .get("search_engine")
            .and_then(Value::as_str)
            .filter(|engine| !engine.trim().is_empty())
            .unwrap_or(cw_browser::DEFAULT_SEARCH_ENGINE)
            .to_owned()
    }
    pub(super) fn prime_browsers(&mut self, id: &str, machine: &str) {
        let now = self.runtime.tick();
        let seed = self.runtime.seed();
        let viewport = self.browser_viewport(id, machine, None);
        let engine = self.world_search_engine();
        let scope = format!("computer/{machine}/browser");
        let Some(m) = Arc::make_mut(&mut self.sessions)
            .get_mut(id)
            .and_then(|s| s.machines.get_mut(machine))
        else {
            return;
        };
        for browser in std::iter::once(&mut m.browser).chain(m.browser_windows.values_mut()) {
            browser.set_clock(now);
            browser.set_entropy(seed, &scope);
            browser.set_search_engine(&engine);
        }
        if let Some((w, h)) = viewport {
            m.browser.set_viewport(w, h);
        }
    }
    /// Refresh every browser page on `machine` whose `refresh` is due (see
    /// `cw_browser::BrowserState::refresh`), and advance page script with the world
    /// clock (`cw_browser::BrowserState::tick`: due timers, animation frames). A failed
    /// refresh keeps the page it had.
    pub(super) fn refresh_pages(&mut self, id: &str, machine: &str, actor: &str) {
        let now = self.runtime.tick();
        let wanted = self
            .session(id)
            .ok()
            .and_then(|s| s.machines.get(machine))
            .is_some_and(|m| {
                m.browser.refresh_pending(now)
                    || m.browser_windows.values().any(|b| b.refresh_pending(now))
            });
        if !wanted {
            return;
        }
        self.prime_browsers(id, machine);
        let runtime = &mut self.runtime;
        let Some(state) = Arc::make_mut(&mut self.sessions)
            .get_mut(id)
            .and_then(|s| s.machines.get_mut(machine))
        else {
            return;
        };
        let mut failures = vec![];
        for browser in std::iter::once(&mut state.browser).chain(state.browser_windows.values_mut())
        {
            let mut http = |r| runtime.http(machine, actor, r);
            if let Err(e) = browser.tick(now, &mut http) {
                failures.push(e.message);
            }
            if !browser.refresh_due(now) {
                continue;
            }
            if let Err(e) = browser.refresh(now, &mut http) {
                failures.push(e.message);
            }
        }
        for message in failures {
            self.runtime.record_event(
                "browser.refresh_failed",
                Some(machine),
                Some(actor),
                json!({"session": id, "error": message}),
            );
        }
    }
    /// Scroll pane `pane` of window `window` to `offset`: a browser's page scrolls its
    /// tab, every other pane is the window's own. Returns whether the view moved.
    pub(super) fn set_pane_offset(
        &mut self,
        id: &str,
        machine: &str,
        window: u64,
        pane: &str,
        offset: i32,
        horizontal: bool,
    ) -> Result<bool> {
        let is_browser = self.session(id)?.machines.get(machine).is_some_and(|m| {
            matches!(
                m.desktop.windows.get(&window).map(|w| &w.state),
                Some(AppState::Browser { .. })
            )
        });
        if is_browser {
            self.prime_browsers(id, machine);
            let actor = self.session(id)?.config.actor.clone();
            let runtime = &mut self.runtime;
            let m = Arc::make_mut(&mut self.sessions)
                .get_mut(id)
                .and_then(|s| s.machines.get_mut(machine))
                .ok_or_else(|| SimError::denied("machine unavailable"))?;
            let state = if m.active_browser_window == Some(window) {
                &mut m.browser
            } else if let Some(state) = m.browser_windows.get_mut(&window) {
                state
            } else {
                &mut m.browser
            };
            let mut http = |r| runtime.http(machine, &actor, r);
            let moved = state.scroll_pane_with(pane, offset, horizontal, &mut http);
            m.scrolled |= moved;
            return Ok(moved);
        }
        let m = self.machine_mut(id, machine)?;
        let moved = m
            .desktop
            .scroll_pane(window, pane, offset)
            .map_err(SimError::invalid)?;
        m.scrolled |= moved;
        Ok(moved)
    }
    /// Pull `window`'s pane past an end by `stretch` pixels (0 lets it go).
    pub(super) fn set_stretch(
        &mut self,
        id: &str,
        machine: &str,
        window: u64,
        pane: &str,
        stretch: i32,
    ) {
        if let Ok(m) = self.machine_mut(id, machine) {
            if let Some(w) = m.desktop.windows.get_mut(&window) {
                w.scroll.stretch = (stretch != 0).then(|| (pane.to_owned(), stretch));
            }
        }
    }
    /// The screen a pointer action addresses: the `width` and `height` it states,
    /// each falling back to the machine's screen as last addressed (or its native one).
    pub(super) fn pointer_size(&self, id: &str, machine: &str, p: &Value) -> Result<(u32, u32)> {
        let screen = self.screen_size(id, machine)?;
        let side = |key: &str, fallback: u32| {
            p.get(key)
                .and_then(integer_u64)
                .unwrap_or(u64::from(fallback))
                .min(8192) as u32
        };
        Ok((side("width", screen.0), side("height", screen.1)))
    }
    /// Remember the screen size a pointer action addressed the machine at, when it
    /// stated one.
    pub(super) fn record_viewport(
        &mut self,
        id: &str,
        machine: &str,
        payload: &Value,
        size: (u32, u32),
    ) -> Result<()> {
        if payload.get("width").is_some()
            && payload.get("height").is_some()
            && size.0 > 0
            && size.1 > 0
        {
            self.machine_mut(id, machine)?.viewport = Some(size);
        }
        Ok(())
    }
    /// The screen of `machine` as the actor last addressed it, or the shell's native
    /// screen (portrait on a phone) before any action has said: what a screenshot of it
    /// captures, at its real size and orientation.
    pub(super) fn screen_size(&self, id: &str, machine: &str) -> Result<(u32, u32)> {
        let recorded = self
            .session(id)?
            .machines
            .get(machine)
            .and_then(|m| m.viewport);
        Ok(recorded.unwrap_or_else(|| {
            self.desktop_theme(id, machine)
                .unwrap_or(DesktopTheme::Ubuntu)
                .native_screen()
        }))
    }
    /// A finger that stops touching lets go of any list it held: a rubber band springs
    /// back.
    pub(super) fn end_touch_scroll(&mut self, id: &str, machine: &str) -> Result<()> {
        if let Some(drag) = self.machine_mut(id, machine)?.touch_scroll.take() {
            if let Some(pane) = &drag.pane {
                self.set_stretch(id, machine, drag.window, pane, 0);
            }
        }
        Ok(())
    }
    /// A finger moving on a phone. Once it has travelled past the slop, mostly
    /// vertically, from somewhere no shell gesture starts, inside the application in
    /// front, it takes the list under it; from then on every move scrolls that list so
    /// the content stays under the finger, pulling past an end with the rubber band.
    /// `None` when the move is not a list's.
    pub(super) fn touch_scroll_move(
        &mut self,
        id: &str,
        machine: &str,
        (x, y): (i32, i32),
        size: (u32, u32),
        theme: DesktopTheme,
    ) -> Result<Option<Value>> {
        let tick = self.runtime.tick();
        let m = &self.session(id)?.machines[machine];
        if let Some(mut drag) = m.touch_scroll.clone() {
            drag.prev = Some(drag.last);
            drag.last = (y, tick);
            self.touch_scroll_to(id, machine, &mut drag, y, size)?;
            self.machine_mut(id, machine)?.touch_scroll = Some(drag);
            return Ok(Some(json!({"cursor": "default"})));
        }
        let Some(start) = m.touch_start else {
            return Ok(None);
        };
        let (dx, dy) = (x - start.0, y - start.1);
        if dy.abs() <= SWIPE_SLOP || dy.abs() < dx.abs() {
            return Ok(None);
        }
        let d = &m.desktop;
        if d.screen != cw_applications::ScreenState::Active
            || d.panel.is_some()
            || d.launcher_open
            || m.active_app.is_some()
        {
            return Ok(None);
        }
        let Some(focused) = d.focused else {
            return Ok(None);
        };
        // Where the platform's own gestures start: the status bar and the home
        // indicator on iOS, the status bar and the navigation bar on Android.
        let h = size.1 as i32;
        let edge = match theme {
            DesktopTheme::Ios => start.1 < 50 || start.1 > h - 60,
            _ => start.1 < 40 || start.1 >= h - cw_applications::desktop_scene::ANDROID_NAV_BAR,
        };
        if edge
            || self
                .touch_gesture(id, machine, theme, start, (x, y), size, None)?
                .is_some()
        {
            return Ok(None);
        }
        if !self
            .session(id)?
            .config
            .actions
            .iter()
            .any(|family| family == "application.v1")
        {
            return Ok(None);
        }
        let scene = self.scene(id, size.0, size.1)?;
        let window = scene
            .hit_test(start.0, start.1)
            .and_then(|n| n.interaction.as_deref())
            .and_then(|t| t.strip_prefix("window:"))
            .and_then(|t| t.split_once(':'))
            .and_then(|(w, _)| w.parse::<u64>().ok());
        if window != Some(focused) {
            return Ok(None);
        }
        // The innermost pane that can scroll at all takes the finger, even at an end:
        // pulled further, it stretches.
        let areas = scene.scrolls_at(Some(focused), start.0, start.1);
        // A list that refreshes by pulling bounces even when it is short, as iOS's and
        // Android's refreshable lists do.
        let refreshes = matches!(
            m.desktop.windows.get(&focused).map(|w| &w.state),
            Some(AppState::Native(app)) if app.pull_to_refresh().is_some()
        );
        let area = areas
            .iter()
            .copied()
            .find(|a| a.max_offset() > 0)
            .or_else(|| {
                areas
                    .iter()
                    .copied()
                    .find(|a| refreshes && a.target.ends_with(":content:pane:main"))
            });
        let pane = area.and_then(|a| {
            a.target
                .split_once(":content:pane:")
                .map(|(_, pane)| pane.to_owned())
        });
        let mut drag = TouchScroll {
            window: focused,
            pane: pane.clone(),
            at: start,
            origin: area.filter(|_| pane.is_some()).map_or(0, |a| a.offset),
            anchor: start.1,
            max: area.map_or(0, |a| a.max_offset()),
            height: area.map_or(size.1, |a| a.bounds.height),
            applied: start.1,
            last: (y, tick),
            // The finger came down at the start; with the world clock unmoved since,
            // that sample is a frame before this one.
            prev: Some((start.1, tick)),
        };
        // The press was not a tap on whatever the finger came down on.
        self.machine_mut(id, machine)?.pointer_press = None;
        self.touch_scroll_to(id, machine, &mut drag, y, size)?;
        self.machine_mut(id, machine)?.touch_scroll = Some(drag);
        Ok(Some(json!({"cursor": "default"})))
    }
    /// Move the list a finger holds so the content is under the finger at `y`.
    pub(super) fn touch_scroll_to(
        &mut self,
        id: &str,
        machine: &str,
        drag: &mut TouchScroll,
        y: i32,
        size: (u32, u32),
    ) -> Result<bool> {
        match drag.pane.clone() {
            Some(pane) => {
                let raw = drag.origin + (drag.anchor - y);
                let offset = raw.clamp(0, drag.max.max(0));
                // Past an end the content still follows, but resists: pulled down at the
                // top it comes down after the finger; pushed up at the end, it goes up.
                let stretch =
                    -cw_applications::desktop_scene::scroll::rubber_band(raw - offset, drag.height);
                let moved = self.set_pane_offset(id, machine, drag.window, &pane, offset, false)?;
                self.set_stretch(id, machine, drag.window, &pane, stretch);
                Ok(moved)
            }
            None => {
                // An application's own use of the wheel moves in its own steps (rows,
                // lines), so it is handed the finger's travel a step's worth at a time.
                let delta = drag.applied - y;
                if delta.abs() < TOUCH_STEP {
                    return Ok(false);
                }
                drag.applied = y;
                self.scroll_at(
                    id,
                    machine,
                    drag.at,
                    size,
                    cw_applications::Wheel::vertical(delta),
                )
            }
        }
    }
    /// The finger holding a list lifts at `y`. The list lands under it, then keeps
    /// going as far as the platform's deceleration carries the finger's last velocity
    /// (measured from the last two samples and the world clock between them), stopping
    /// at an end; a rubber band springs back. A finger that rested before lifting does
    /// not fling. Returns whether the list moved.
    pub(super) fn touch_scroll_release(
        &mut self,
        id: &str,
        machine: &str,
        y: i32,
        size: (u32, u32),
        theme: DesktopTheme,
    ) -> Result<bool> {
        let tick = self.runtime.tick();
        let Some(mut drag) = self.machine_mut(id, machine)?.touch_scroll.take() else {
            return Ok(false);
        };
        if y != drag.last.0 {
            drag.prev = Some(drag.last);
            drag.last = (y, tick);
        }
        let velocity = match drag.prev {
            _ if tick.saturating_sub(drag.last.1) > FINGER_STOPPED_US => 0,
            Some(prev) => {
                let elapsed = drag.last.1.saturating_sub(prev.1).max(FRAME_US);
                i64::from(drag.last.0 - prev.0) * 1_000_000 / elapsed as i64
            }
            None => 0,
        };
        // The content follows the finger, so a finger flung upwards carries the list
        // further down it.
        let fling = cw_applications::desktop_scene::scroll::fling_distance(
            theme == DesktopTheme::Android,
            velocity,
        );
        match drag.pane.clone() {
            Some(pane) => {
                // Pulled far enough down past the top of a phone screen's list, the
                // release refreshes it, as iOS's refresh control and Android's
                // swipe-to-refresh do.
                let pulled = cw_applications::desktop_scene::scroll::rubber_band(
                    -(drag.origin + (drag.anchor - y)),
                    drag.height,
                );
                let refresh = (pane == "main" && pulled >= PULL_TO_REFRESH)
                    .then(|| {
                        let m = self.session(id).ok()?.machines.get(machine)?;
                        match &m.desktop.windows.get(&drag.window)?.state {
                            AppState::Native(app) => app.pull_to_refresh(),
                            _ => None,
                        }
                    })
                    .flatten();
                let raw = drag.origin + (drag.anchor - y) - fling;
                let offset = raw.clamp(0, drag.max.max(0));
                self.set_stretch(id, machine, drag.window, &pane, 0);
                let moved = self.set_pane_offset(id, machine, drag.window, &pane, offset, false)?;
                if let Some(target) = refresh {
                    let actor = self.session(id)?.config.actor.clone();
                    let effects = self
                        .machine_mut(id, machine)?
                        .desktop
                        .click(target)
                        .map_err(SimError::invalid)?;
                    self.effects(id, machine, &actor, effects)?;
                }
                Ok(moved)
            }
            None => {
                let delta = drag.applied - y - fling;
                if delta == 0 {
                    return Ok(false);
                }
                self.scroll_at(
                    id,
                    machine,
                    drag.at,
                    size,
                    cw_applications::Wheel::vertical(delta),
                )
            }
        }
    }
    pub(super) fn browser_action(
        &mut self,
        id: &str,
        actor: &str,
        a: &ActionEnvelope,
    ) -> Result<Value> {
        if !self
            .session(id)?
            .config
            .actions
            .iter()
            .any(|family| family == "browser.v1")
        {
            return Err(SimError::denied("browser interaction is not permitted"));
        }
        let themed = self.desktop_theme(id, &a.machine).is_some();
        self.prime_browsers(id, &a.machine);
        let engine = self.world_search_engine();
        let runtime = &mut self.runtime;
        let machine = Arc::make_mut(&mut self.sessions)
            .get_mut(id)
            .and_then(|s| s.machines.get_mut(&a.machine))
            .ok_or_else(|| SimError::denied("machine unavailable"))?;
        machine.browser_visible = true;
        machine.address_focused = false;
        machine.desktop.launcher_open = false;
        if themed {
            if let Some(window) = machine
                .desktop
                .windows
                .values()
                .filter(|w| matches!(w.state, AppState::Browser { .. }))
                .max_by_key(|w| (machine.desktop.focused == Some(w.id), w.id))
                .map(|w| w.id)
            {
                machine.desktop.focus(window).map_err(SimError::invalid)?;
            } else {
                machine
                    .desktop
                    .launch("browser", "")
                    .map_err(SimError::invalid)?;
            }
        }
        if themed && machine.active_browser_window != machine.desktop.focused {
            if let Some(previous) = machine.active_browser_window {
                machine
                    .browser_windows
                    .insert(previous, std::mem::take(&mut machine.browser));
            }
            machine.browser = machine
                .desktop
                .focused
                .and_then(|window| machine.browser_windows.remove(&window))
                .unwrap_or_default();
            machine.active_browser_window = machine.desktop.focused;
        }
        machine.browser.set_search_engine(&engine);
        machine.active_app = None;
        machine.custom_page = None;
        let mut http = |r| runtime.http(&a.machine, actor, r);
        match a.op.as_str() {
            "navigate" => machine
                .browser
                .navigate(string(&a.payload, "url")?, &mut http)?,
            "back" => machine.browser.back(&mut http)?,
            "forward" => machine.browser.forward(&mut http)?,
            "reload" => machine.browser.reload(&mut http)?,
            "fill" => {
                let input = string(&a.payload, "id")?;
                machine
                    .browser
                    .fill_with(input, string(&a.payload, "value")?, &mut http)?;
                machine.focused_input = Some(input.into());
            }
            "key" => machine.browser.key(string(&a.payload, "key")?, &mut http)?,
            "new_tab" => {
                machine.browser.new_tab();
            }
            "switch_tab" | "close_tab" => {
                let tab = a
                    .payload
                    .get("tab")
                    .and_then(integer_u64)
                    .ok_or_else(|| SimError::invalid("tab required"))?
                    as usize;
                if a.op == "switch_tab" {
                    machine.browser.switch_tab(tab)?
                } else {
                    machine.browser.close_tab(tab)?
                }
            }
            "scroll" => {
                let to = |key: &str| {
                    a.payload
                        .get(key)
                        .and_then(integer_i64)
                        .unwrap_or(0)
                        .clamp(0, i32::MAX as i64) as i32
                };
                // `{"row": id, "x": n}` scrolls one of the page's sideways shelves (or
                // an HTML scroll container sideways); `{"pane": id, "y": n}` an HTML
                // scroll container down; plain `{"y": n}` the page itself.
                let row = a.payload.get("row").and_then(Value::as_str);
                let pane = a.payload.get("pane").and_then(Value::as_str);
                match (row, pane) {
                    (Some(row), _) => {
                        machine.browser.scroll_pane_with(
                            &format!("row:{row}"),
                            to("x"),
                            true,
                            &mut http,
                        );
                    }
                    (None, Some(pane)) => {
                        machine
                            .browser
                            .scroll_pane_with(pane, to("y"), false, &mut http);
                    }
                    (None, None) => {
                        machine
                            .browser
                            .scroll_pane_with("page", to("y"), false, &mut http);
                    }
                }
            }
            "submit" => machine
                .browser
                .submit(string(&a.payload, "id")?, &mut http)?,
            "click" => {
                let target = string(&a.payload, "id")?;
                if machine.browser.has_input(target) {
                    machine.focused_input = Some(target.into());
                    machine.browser.click(target, &mut http)?;
                } else {
                    machine.focused_input = None;
                    machine.browser.click(target, &mut http)?;
                }
            }
            _ => return Err(SimError::invalid("unsupported browser operation")),
        };
        // A file the page sent as an attachment lands in Downloads, written through
        // the kernel like any other file the actor creates, so it is real and survives
        // a snapshot; the browser itself has no disk.
        for download in machine.browser.take_downloads() {
            let home = machine.desktop.home_folder();
            let folder = format!("{}/Downloads", home.trim_end_matches('/'));
            runtime.create_directory(&a.machine, actor, &folder)?;
            let name = download_file_name(&download.name, &download.url);
            let path = format!("{folder}/{name}");
            runtime.write_file(&a.machine, actor, &path, &download.body)?;
            machine.desktop.record_download(
                &name,
                &path,
                &download.url,
                download.body.len() as u64,
            );
        }
        machine.focused_input = machine.browser.tab().focused.clone();
        if let Some(window) = machine
            .desktop
            .focused
            .and_then(|id| machine.desktop.windows.get_mut(&id))
        {
            if let AppState::Browser { address } = &mut window.state {
                *address = machine.browser.url().unwrap_or("").to_string();
            }
        }
        Ok(serde_json::to_value(active_page(machine))?)
    }
    pub(super) fn effects(
        &mut self,
        id: &str,
        machine: &str,
        actor: &str,
        effects: Vec<cw_applications::AppEffect>,
    ) -> Result<()> {
        use cw_applications::AppEffect::*;
        let mut pending: std::collections::VecDeque<_> = effects.into();
        let mut budget = 0u32;
        while let Some(effect) = pending.pop_front() {
            budget += 1;
            if budget > 64 {
                return Err(SimError::invalid("application effect budget exceeded"));
            }
            effect.validate().map_err(SimError::invalid)?;
            match effect {
                ReadFile { window, path } => {
                    let content = String::from_utf8_lossy(&self.runtime.read_file(machine, &path)?)
                        .into_owned();
                    self.machine_mut(id, machine)?
                        .desktop
                        .file_loaded(window, content)
                        .map_err(SimError::invalid)?;
                }
                WriteFile {
                    window,
                    path,
                    content,
                } => {
                    self.runtime
                        .write_file(machine, actor, &path, content.as_bytes())?;
                    let more = self
                        .machine_mut(id, machine)?
                        .desktop
                        .file_written(window, &path, &content)
                        .map_err(SimError::invalid)?;
                    pending.extend(more);
                }
                ListTree {
                    window,
                    path,
                    depth,
                } => {
                    let result = list_tree(self.runtime.computer(machine)?, &path, depth);
                    let more = self
                        .machine_mut(id, machine)?
                        .desktop
                        .tree_listed(window, &path, depth, result)
                        .map_err(SimError::invalid)?;
                    pending.extend(more);
                }
                ReadFiles { window, tag, paths } => {
                    // Each file answers for itself: one unreadable file is that file's
                    // problem, not the whole request's.
                    let mut budget_bytes = READ_FILES_BYTES;
                    let files = paths
                        .into_iter()
                        .take(READ_FILES_LIMIT)
                        .map(|path| {
                            let result = self
                                .runtime
                                .read_file(machine, &path)
                                .map_err(|e| actor_error(e).message)
                                .and_then(|bytes| {
                                    if bytes.len() > budget_bytes {
                                        return Err("the file is too large to open".into());
                                    }
                                    budget_bytes -= bytes.len();
                                    if bytes.contains(&0) {
                                        return Err("the file is binary".into());
                                    }
                                    String::from_utf8(bytes)
                                        .map_err(|_| "the file is not UTF-8 text".to_owned())
                                });
                            (path, result)
                        })
                        .collect();
                    let more = self
                        .machine_mut(id, machine)?
                        .desktop
                        .files_read(window, &tag, files)
                        .map_err(SimError::invalid)?;
                    pending.extend(more);
                }
                ShellRun {
                    window,
                    tag,
                    cwd,
                    command,
                } => {
                    let prompt_at = |env: &Self, dir: &str| -> Result<String> {
                        let c = env.runtime.computer(machine)?;
                        Ok(cw_applications::shell_prompt(
                            &c.user, &c.id, dir, &c.dialect,
                        ))
                    };
                    let before = self.runtime.computer(machine)?.resolve(&cwd);
                    let prompt = prompt_at(self, &before)?;
                    let outcome = if command.trim().is_empty() {
                        cw_applications::ShellOutcome {
                            entry: None,
                            prompt,
                            cwd: before,
                            clear: false,
                        }
                    } else {
                        match self.runtime.execute_in(machine, actor, &cwd, &command) {
                            Ok((mut result, after)) => {
                                self.open_from_shell(id, machine, actor, &mut result)?;
                                let entry = cw_applications::TerminalEntry::new(
                                    &prompt,
                                    command,
                                    &result.stdout,
                                    &result.stderr,
                                    result.exit_code,
                                );
                                let clear = result.clear;
                                self.machine_mut(id, machine)?.terminal =
                                    serde_json::to_value(&result)?;
                                cw_applications::ShellOutcome {
                                    entry: Some(entry),
                                    prompt: prompt_at(self, &after)?,
                                    cwd: after,
                                    clear,
                                }
                            }
                            // The session's folder is gone: the shell says so, as a
                            // shell started in a deleted directory would.
                            Err(e) => cw_applications::ShellOutcome {
                                entry: Some(cw_applications::TerminalEntry::new(
                                    &prompt,
                                    command,
                                    "",
                                    &format!("shell: {}\n", actor_error(e).message),
                                    1,
                                )),
                                prompt,
                                cwd: before,
                                clear: false,
                            },
                        }
                    };
                    let more = self
                        .machine_mut(id, machine)?
                        .desktop
                        .shell_ran(window, &tag, outcome)
                        .map_err(SimError::invalid)?;
                    pending.extend(more);
                }
                Debug {
                    window,
                    tag,
                    request,
                } => {
                    // The machine's debugger, or the reason it has none: either way the
                    // view is told, and shows only what came back.
                    let reply = self
                        .runtime
                        .debug(machine, actor, &request)
                        .map_err(|e| e.message);
                    let more = self
                        .machine_mut(id, machine)?
                        .desktop
                        .debug_reply(window, &tag, reply)
                        .map_err(SimError::invalid)?;
                    pending.extend(more);
                }
                CopyText { text, .. } => {
                    self.machine_mut(id, machine)?
                        .desktop
                        .copy_text(&text)
                        .map_err(SimError::invalid)?;
                }
                // The desktop resolves a paste against its clipboard before the effect
                // leaves it; one that reaches here has nothing left to do.
                Paste { .. } => {}
                ListDirectory { window, tab, path } => {
                    let c = self.runtime.computer(machine)?;
                    let base = c.resolve(&path);
                    let readable = c
                        .vfs
                        .check_access(&base, &c.user, true, false, true)
                        .is_ok();
                    let listing = if readable {
                        c.vfs.list(&base).ok()
                    } else {
                        None
                    };
                    let Some(listing) = listing else {
                        // An application that cannot read a folder shows that it cannot,
                        // rather than failing the click that opened it.
                        let reason = if readable {
                            "folder not found"
                        } else {
                            "folder access denied"
                        };
                        if self
                            .machine_mut(id, machine)?
                            .desktop
                            .directory_failed(window, reason)
                            .map_err(SimError::invalid)?
                        {
                            continue;
                        }
                        return Err(if readable {
                            SimError::not_found("directory")
                        } else {
                            SimError::denied("directory access denied")
                        });
                    };
                    // Where a trashed thing came from, when this is the trash. The
                    // `.trashinfo` records are the only place that fact lives, so a
                    // Trash view that shows an original path is reading the machine.
                    let home = c
                        .env
                        .get("HOME")
                        .cloned()
                        .unwrap_or_else(|| format!("/home/{}", c.user));
                    let origins: BTreeMap<String, String> =
                        if base.trim_end_matches('/') == cw_computer::trash::files_dir(&home) {
                            cw_computer::trash::list(&c.vfs, &home)
                                .into_iter()
                                .map(|e| (e.name, e.original))
                                .collect()
                        } else {
                            BTreeMap::new()
                        };
                    // The listing carries what the machine really knows about each
                    // entry: `lstat` for what it is (a symlink stays a symlink) and
                    // `stat` for the classifier, so a link to a folder still opens
                    // like one. Nothing here is guessed from a name.
                    let entries = listing
                        .into_iter()
                        .map(|name| {
                            let path = format!("{}/{}", base.trim_end_matches('/'), name);
                            let link = c.vfs.lstat(&path).ok();
                            let meta = c.vfs.stat(&path).ok();
                            let is_dir = meta.as_ref().is_some_and(|m| m.is_dir);
                            let kind = match &link {
                                Some(m) if m.is_symlink => cw_applications::EntryKind::Symlink,
                                _ if is_dir => cw_applications::EntryKind::Directory,
                                _ => cw_applications::EntryKind::File,
                            };
                            cw_applications::FileRow {
                                entry: if is_dir {
                                    format!("{name}/")
                                } else {
                                    name.clone()
                                },
                                kind,
                                // A folder has no byte count any file manager shows.
                                size: link.as_ref().filter(|_| !is_dir).map(|m| m.size as u64),
                                mode: link.as_ref().map(|m| m.mode),
                                modified: link.as_ref().map(|m| m.modified),
                                original: origins.get(&name).cloned(),
                            }
                        })
                        .collect();
                    self.machine_mut(id, machine)?
                        .desktop
                        .directory_listed(window, tab, entries)
                        .map_err(SimError::invalid)?;
                    // A photo library asks for the pixels of whatever the listing put on
                    // screen; nothing else needs decoding, so nothing else asks.
                    if let Some(cw_applications::AppState::Native(
                        cw_applications::NativeApp::Photos(photos),
                    )) = self.session(id)?.machines[machine]
                        .desktop
                        .windows
                        .get(&window)
                        .map(|w| &w.state)
                    {
                        pending.extend(photos.undecoded(window));
                    }
                }
                Execute { window, command } => {
                    // The prompt is captured before the command runs, so `cd` is echoed
                    // under the directory it was typed in, not the one it moved to.
                    let prompt = self.machine_mut(id, machine)?.desktop.prompt.clone();
                    let mut result = self.runtime.execute_at_terminal(machine, actor, &command)?;
                    self.open_from_shell(id, machine, actor, &mut result)?;
                    let entry = cw_applications::TerminalEntry::new(
                        &prompt,
                        command,
                        &result.stdout,
                        &result.stderr,
                        result.exit_code,
                    );
                    let clear = result.clear;
                    let next = {
                        let c = self.runtime.computer(machine)?;
                        // A runtime waiting for input owns the prompt.
                        match c.session_prompt() {
                            Some(p) => p.to_string(),
                            None => {
                                let home = c.env.get("HOME").map_or("", String::as_str);
                                cw_applications::shell_prompt_at(
                                    &c.user,
                                    &c.id,
                                    &c.cwd,
                                    home,
                                    prompt_dialect(&c.os_family, &c.dialect),
                                )
                            }
                        }
                    };
                    let desktop = &mut self.machine_mut(id, machine)?.desktop;
                    desktop.prompt = next;
                    if clear {
                        desktop.terminal_clear(window).map_err(SimError::invalid)?;
                    } else {
                        desktop
                            .terminal_output(window, entry)
                            .map_err(SimError::invalid)?;
                    }
                    self.machine_mut(id, machine)?.terminal = serde_json::to_value(result)?;
                }
                Navigate { window: _, url } => {
                    self.browser_action(
                        id,
                        actor,
                        &ActionEnvelope::new("browser.v1", "navigate", machine, json!({"url":url})),
                    )?;
                }
                Http {
                    window,
                    tag,
                    method,
                    url,
                    body,
                } => {
                    // Applications reach services through the same gateway as the browser.
                    if !self
                        .session(id)?
                        .config
                        .actions
                        .iter()
                        .any(|family| family == "browser.v1")
                    {
                        return Err(SimError::denied(
                            "network application interaction is not permitted",
                        ));
                    }
                    let request = HttpRequest {
                        method,
                        url,
                        headers: BTreeMap::from([(
                            "content-type".into(),
                            "application/json".into(),
                        )]),
                        body: body.into_bytes(),
                    };
                    match self.runtime.http(machine, actor, request) {
                        Ok(response) => {
                            let text = String::from_utf8_lossy(&response.body).into_owned();
                            let more = self
                                .machine_mut(id, machine)?
                                .desktop
                                .http_response(window, &tag, response.status, &text)
                                .map_err(SimError::invalid)?;
                            pending.extend(more);
                        }
                        // A transport failure is application state, not an action failure:
                        // otherwise an actor could map the gateway by watching clicks fail.
                        Err(e) => self
                            .machine_mut(id, machine)?
                            .desktop
                            .http_failed(window, &tag, &actor_error(e).message)
                            .map_err(SimError::invalid)?,
                    }
                }
                CreateDirectory { window: _, path } => {
                    self.runtime.create_directory(machine, actor, &path)?;
                    // The refresh is the caller's: it queues a listing of the folder it
                    // is actually showing. Listing the new folder here instead put the
                    // wrong contents in a file manager's tab, because the folder that
                    // was created is not usually the folder that is on screen.
                }
                // Mutations go through the kernel, which applies the same access checks
                // `read_file` and `write_file` do: a file manager is not a way around a
                // permission a shell would have refused. Each of these is followed by a
                // `ListDirectory` the application queued, so the folder on screen is
                // the machine's answer and never an assumption about what happened.
                CreateFile { window: _, path } => {
                    self.runtime.create_file(machine, actor, &path)?;
                }
                CopyPath {
                    window: _,
                    from,
                    to,
                } => self.runtime.copy_path(machine, actor, &from, &to)?,
                MovePath {
                    window: _,
                    from,
                    to,
                } => self.runtime.move_path(machine, actor, &from, &to)?,
                // Delete is a move into the machine's trash; nothing is hard-removed.
                TrashPath {
                    window: _,
                    path,
                    trash,
                } => {
                    self.runtime.trash_path(machine, actor, &path, &trash)?;
                }
                // Put back is the delete undone, and it is the machine that knows
                // where: the `.trashinfo` record names the folder, not the view.
                RestorePath { window: _, path } => {
                    self.runtime.restore_path(machine, actor, &path)?;
                }
                EmptyTrash { window: _ } => {
                    self.runtime.empty_trash(machine, actor)?;
                }
                Download { window, url } => {
                    if !self
                        .session(id)?
                        .config
                        .actions
                        .iter()
                        .any(|family| family == "browser.v1")
                    {
                        return Err(SimError::denied("network access is not permitted"));
                    }
                    // Fetched through the same gateway the browser uses, then written to
                    // the machine: a download that does not leave a file is a pretence.
                    let request = HttpRequest {
                        method: "GET".into(),
                        url: url.clone(),
                        headers: BTreeMap::new(),
                        body: vec![],
                    };
                    let response = self.runtime.http(machine, actor, request)?;
                    let name = download_name(&url);
                    let home = self.session(id)?.machines[machine].desktop.home_folder();
                    let folder = format!("{}/Downloads", home.trim_end_matches('/'));
                    self.runtime.create_directory(machine, actor, &folder)?;
                    let path = format!("{folder}/{name}");
                    let bytes = response.body.len() as u64;
                    self.runtime
                        .write_file(machine, actor, &path, &response.body)?;
                    self.machine_mut(id, machine)?
                        .desktop
                        .record_download(&name, &path, &url, bytes);
                    let _ = window;
                }
                Screenshot { window, path } => {
                    if !self
                        .session(id)?
                        .config
                        .observations
                        .iter()
                        .any(|c| c == "pixels.v1")
                    {
                        return Err(SimError::denied("pixel capture is not permitted")
                            .because(cw_protocol::reason::PIXELS_NOT_GRANTED));
                    }
                    // The screen is rasterised from the same scene an observer sees, so a
                    // screenshot cannot show something the actor could not, at the size
                    // and orientation the machine's screen really has.
                    let (width, height) = self.screen_size(id, machine)?;
                    let scene = self.scene(id, width, height)?;
                    let png = self
                        .capture
                        .clone()
                        .ok_or_else(|| {
                            SimError::invalid("this build has no rasterizer to capture with")
                        })?
                        .png(&scene)
                        .map_err(SimError::invalid)?;
                    let home = self.session(id)?.machines[machine].desktop.home_folder();
                    let path = if path.is_empty() {
                        format!("{}/Pictures", home.trim_end_matches('/'))
                    } else {
                        path
                    };
                    self.runtime.create_directory(machine, actor, &path)?;
                    let tick = self.runtime.tick();
                    let file = format!("{path}/screen-{tick}.png");
                    self.runtime.write_file(machine, actor, &file, &png)?;
                    self.machine_mut(id, machine)?.desktop.notify(
                        "screenshot",
                        "Screenshot saved",
                        &file,
                        Some("shell:launch:files".into()),
                    );
                    let _ = window;
                }
                ReadBytes { window, path } => {
                    // A document the machine cannot give (missing, unreadable) is the
                    // application's to report, not a failure of the click that asked.
                    let result = self.runtime.read_file(machine, &path).map_err(file_problem);
                    let more = self
                        .machine_mut(id, machine)?
                        .desktop
                        .bytes_loaded(window, &path, result)
                        .map_err(SimError::invalid)?;
                    pending.extend(more);
                }
                WriteBytes {
                    window,
                    path,
                    bytes,
                } => {
                    let result = self
                        .runtime
                        .write_file(machine, actor, &path, &bytes)
                        .map_err(file_problem);
                    let more = self
                        .machine_mut(id, machine)?
                        .desktop
                        .bytes_saved(window, &path, result)
                        .map_err(SimError::invalid)?;
                    pending.extend(more);
                }
                ReadImage { window, path } => {
                    // A picture is only shown if the machine really holds one and this
                    // build can decode it; otherwise the application says why.
                    let outcome = self
                        .capture
                        .clone()
                        .ok_or_else(|| "this build cannot decode images".to_owned())
                        .and_then(|raster| {
                            let bytes = self
                                .runtime
                                .read_file(machine, &path)
                                .map_err(|e| actor_error(e).message)?;
                            raster.decode(&bytes)
                        });
                    match outcome {
                        Ok((width, height, rgba)) => {
                            let more = self
                                .machine_mut(id, machine)?
                                .desktop
                                .image_loaded(window, &path, width, height, rgba);
                            more.map_err(SimError::invalid)?;
                        }
                        Err(reason) => self
                            .machine_mut(id, machine)?
                            .desktop
                            .image_failed(window, &path, &reason)
                            .map_err(SimError::invalid)?,
                    }
                }
                WriteImage {
                    window,
                    path,
                    width,
                    height,
                    rgba,
                } => {
                    // Encoding needs the rasterizer's codec; the application only drew.
                    let png = self
                        .capture
                        .clone()
                        .ok_or_else(|| SimError::invalid("this build cannot encode images"))?
                        .encode(width, height, &rgba)
                        .map_err(SimError::invalid)?;
                    self.runtime.write_file(machine, actor, &path, &png)?;
                    let more = self
                        .machine_mut(id, machine)?
                        .desktop
                        .image_saved(window, &path)
                        .map_err(SimError::invalid)?;
                    pending.extend(more);
                }
                RasterText {
                    window,
                    text,
                    size,
                    bold,
                } => {
                    // The text tool stamps exactly what the renderer draws: the line is
                    // rendered white on black in this platform's font and the red channel
                    // becomes the glyph coverage.
                    let typeface = self
                        .desktop_theme(id, machine)
                        .map(DesktopTheme::typeface)
                        .unwrap_or_default();
                    let size = size.clamp(6, 400);
                    let width = cw_scene::metrics::text_width(typeface, bold, &text, size) + 4;
                    let height = u32::from(size) + u32::from(size) / 2 + 4;
                    let outcome = if width > 8192 {
                        Err("that text is too wide to draw".to_owned())
                    } else {
                        let mut scene = Scene::new(width, height);
                        scene.background = cw_scene::Color::BLACK;
                        scene.typeface = typeface;
                        let bounds = cw_scene::Rect::new(1, 1, width - 2, height - 2);
                        scene.nodes.push(if bold {
                            cw_scene::Node::ui_text_bold(
                                1,
                                bounds,
                                text,
                                size,
                                cw_scene::Color::WHITE,
                            )
                        } else {
                            cw_scene::Node::ui_text(1, bounds, text, size, cw_scene::Color::WHITE)
                        });
                        self.capture
                            .clone()
                            .ok_or_else(|| "this build cannot draw text into images".to_owned())
                            .and_then(|raster| raster.pixels(&scene))
                    };
                    let desktop = &mut self.machine_mut(id, machine)?.desktop;
                    match outcome {
                        Ok((w, h, rgba)) => desktop
                            .text_rasterized(window, w, h, rgba.chunks(4).map(|p| p[0]).collect())
                            .map_err(SimError::invalid)?,
                        Err(reason) => desktop
                            .image_failed(window, cw_applications::TEXT_IMAGE, &reason)
                            .map_err(SimError::invalid)?,
                    }
                }
                CopyImage {
                    window: _,
                    width,
                    height,
                    rgba,
                } => {
                    if u64::from(width) * u64::from(height) > 4096 * 4096 {
                        return Err(SimError::invalid("that is too large to copy"));
                    }
                    let picture = cw_raster::Canvas::from_rgba(width, height, rgba)
                        .map_err(SimError::invalid)?;
                    self.machine_mut(id, machine)?.desktop.clipboard =
                        Some(cw_applications::Clipboard::picture(picture));
                }
                PasteImage { window } => {
                    let picture = self.session(id)?.machines[machine]
                        .desktop
                        .clipboard
                        .as_ref()
                        .and_then(|c| c.image.clone());
                    let desktop = &mut self.machine_mut(id, machine)?.desktop;
                    match picture {
                        Some(picture) => desktop
                            .image_loaded(
                                window,
                                cw_applications::CLIPBOARD_IMAGE,
                                picture.width(),
                                picture.height(),
                                picture.into_pixels(),
                            )
                            .map_err(SimError::invalid)?,
                        None => desktop
                            .image_failed(
                                window,
                                cw_applications::CLIPBOARD_IMAGE,
                                "The clipboard holds no picture",
                            )
                            .map_err(SimError::invalid)?,
                    }
                }
                Launch {
                    window: _,
                    kind,
                    argument,
                } => {
                    self.dispatch(
                        id,
                        actor,
                        &ActionEnvelope::new(
                            "application.v1",
                            "launch",
                            machine,
                            json!({"kind":kind,"argument":argument}),
                        ),
                    )?;
                }
                // Named data a web application emits goes to the world's event log,
                // as a registered application's does.
                Emit {
                    window: _,
                    name,
                    data,
                } => {
                    self.runtime
                        .record_event(&name, Some(machine), Some(actor), data);
                }
            }
        }
        Ok(())
    }
    /// How the world presents `machine`: `desktop`, `laptop`, `phone` or `server`,
    /// as the computer states it (`presentation`) or else the world's
    /// `device_presentations` metadata. A phone shell is a phone whatever is stated; a
    /// machine nothing is (truthfully) said about is a desktop computer.
    pub(crate) fn form_factor(&self, machine: &str, theme: DesktopTheme) -> String {
        if theme.mobile() {
            return "phone".into();
        }
        let definition = self.runtime.definition();
        let stated = definition
            .computers
            .iter()
            .find(|c| c.id == machine)
            .and_then(|c| c.presentation.clone())
            .or_else(|| {
                definition
                    .metadata
                    .get("device_presentations")
                    .and_then(|p| p.get(machine))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            });
        match stated.as_deref() {
            Some(kind @ ("desktop" | "laptop" | "server")) => kind.to_owned(),
            _ => "desktop".to_owned(),
        }
    }
    /// Whether the machine has a battery for its shell to report: laptops and phones
    /// do, desktop computers and servers do not.
    pub(super) fn has_battery(&self, machine: &str, theme: DesktopTheme) -> bool {
        matches!(
            self.form_factor(machine, theme).as_str(),
            "laptop" | "phone"
        )
    }
    pub(super) fn desktop_theme(&self, id: &str, machine: &str) -> Option<DesktopTheme> {
        let session = self.session(id).ok()?;
        if !session
            .config
            .actions
            .iter()
            .any(|family| family == "application.v1")
        {
            return None;
        }
        let definition = self.runtime.definition();
        let computer = definition
            .computers
            .iter()
            .find(|computer| computer.id == machine)?;
        let configured = definition
            .metadata
            .get("desktop_themes")
            .and_then(|themes| themes.get(machine))
            .and_then(Value::as_str);
        // Presentation is opt-in, so adding OS chrome does not alter browser-only or
        // legacy structured-app layouts and their coordinate contracts.
        if configured.is_none() && !computer.profile.starts_with("virtual-") {
            return None;
        }
        DesktopTheme::from_profile(configured.unwrap_or(&computer.profile))
    }
    /// Open what `xdg-open` asked for. The shell can check that a target exists; only
    /// the interface layer can open a window, and only it knows whether this session is
    /// allowed to. A refusal is appended to the command's own stderr with the status
    /// `xdg-open` uses for "no application found", so the shell tells the truth.
    pub(crate) fn open_from_shell(
        &mut self,
        id: &str,
        machine: &str,
        actor: &str,
        result: &mut cw_computer::CommandResult,
    ) -> Result<()> {
        // `dispatch` is past the grant gate `step` applies, so this path applies it
        // itself: running a command must never be a way to drive applications the
        // session was not granted.
        let granted = self
            .session(id)?
            .config
            .actions
            .iter()
            .any(|family| family == "application.v1");
        for target in std::mem::take(&mut result.open) {
            if !granted {
                result.stderr.push_str(&format!(
                    "xdg-open: cannot open {target}: {}\n",
                    actor_error(
                        SimError::denied("application interaction is not permitted")
                            .because(cw_protocol::reason::APPLICATION_FAMILY_REQUIRED)
                    )
                    .message
                ));
                result.exit_code = 3;
                continue;
            }
            let url = target.starts_with("http://")
                || target.starts_with("https://")
                || target.starts_with("file://");
            let kind = if url {
                "browser".to_owned()
            } else if self
                .runtime
                .computer(machine)?
                .vfs
                .stat(&target)
                .is_ok_and(|m| m.is_dir)
            {
                "files".to_owned()
            } else {
                // The same rule a file manager uses when a document is double-clicked.
                cw_applications::opener(&target).to_owned()
            };
            let launch = ActionEnvelope::new(
                "application.v1",
                "launch",
                machine,
                json!({"kind":kind,"argument":target}),
            );
            if let Err(e) = self.dispatch(id, actor, &launch) {
                let refusal = actor_error(e);
                result.stderr.push_str(&format!(
                    "xdg-open: cannot open {target} with {kind}: {}\n",
                    refusal.message
                ));
                result.exit_code = 3;
            }
        }
        Ok(())
    }
    /// Keep the machine's process table and the session's open windows in step, in both
    /// directions: a window that opened gets a process, a window that closed loses it,
    /// and a process an actor killed from the shell closes its window. An application on
    /// the screen is something the machine is running, and `ps` says so.
    pub(crate) fn sync_window_processes(&mut self, id: &str, machine: &str) -> Result<()> {
        let Ok(session) = self.session(id) else {
            return Ok(());
        };
        let Some(state) = session.machines.get(machine) else {
            return Ok(());
        };
        // A process that is gone takes its window with it.
        let killed: Vec<u64> = state
            .window_processes
            .iter()
            .filter(|(window, pid)| {
                state.desktop.windows.contains_key(window)
                    && !self.runtime.process_alive(machine, **pid)
            })
            .map(|(window, _)| *window)
            .collect();
        let closed = !killed.is_empty();
        for window in killed {
            let desktop = &mut self.machine_mut(id, machine)?.desktop;
            let _ = desktop.close(window);
            self.machine_mut(id, machine)?
                .window_processes
                .remove(&window);
            self.machine_mut(id, machine)?
                .browser_windows
                .remove(&window);
        }
        if closed {
            // The same tidy-up closing a window by hand does: focus, the visible
            // browser, and any per-window browser state the window owned.
            self.sync_desktop_visibility(id, machine)?;
        }
        // A window that closed ends its process.
        let ended: Vec<(u64, u64)> = {
            let state = &self.session(id)?.machines[machine];
            state
                .window_processes
                .iter()
                .filter(|(window, _)| !state.desktop.windows.contains_key(window))
                .map(|(window, pid)| (*window, *pid))
                .collect()
        };
        for (window, pid) in ended {
            self.runtime.end_window_process(machine, pid)?;
            self.machine_mut(id, machine)?
                .window_processes
                .remove(&window);
        }
        // A window that opened starts one.
        let started: Vec<(u64, String, u64)> = {
            let state = &self.session(id)?.machines[machine];
            let computer = self.runtime.computer(machine)?;
            state
                .desktop
                .windows
                .values()
                .filter(|w| !state.window_processes.contains_key(&w.id))
                .map(|w| {
                    let document = presented(&w.state);
                    // What the window really holds: the bytes of the document it opened.
                    let holding = computer
                        .vfs
                        .stat(&document)
                        .map(|m| m.size as u64)
                        .unwrap_or(0);
                    let command = if document.is_empty() {
                        w.app_id.clone()
                    } else {
                        format!("{} {document}", w.app_id)
                    };
                    (
                        w.id,
                        command,
                        holding.saturating_add(window_footprint(&w.app_id)),
                    )
                })
                .collect()
        };
        for (window, command, holding) in started {
            let pid = self
                .runtime
                .start_window_process(machine, &command, holding)?;
            self.machine_mut(id, machine)?
                .window_processes
                .insert(window, pid);
        }
        Ok(())
    }
    pub(super) fn sync_desktop_visibility(&mut self, id: &str, machine: &str) -> Result<()> {
        let state = self.machine_mut(id, machine)?;
        state.browser_visible = state
            .desktop
            .focused
            .and_then(|id| state.desktop.windows.get(&id))
            .is_some_and(|w| matches!(w.state, AppState::Browser { .. }));
        if state.browser_visible {
            let target = state.desktop.focused;
            if state.active_browser_window != target {
                if let Some(previous) = state.active_browser_window {
                    state
                        .browser_windows
                        .insert(previous, std::mem::take(&mut state.browser));
                }
                state.browser = target
                    .and_then(|window| state.browser_windows.remove(&window))
                    .unwrap_or_default();
                state.active_browser_window = target;
            }
        }
        state
            .browser_windows
            .retain(|window, _| state.desktop.windows.contains_key(window));
        if state
            .active_browser_window
            .is_some_and(|window| !state.desktop.windows.contains_key(&window))
        {
            state.active_browser_window = None;
            state.browser = BrowserState::default();
        }
        state.address_focused = false;
        state.active_app = None;
        state.custom_page = None;
        Ok(())
    }
    pub(super) fn shell_action(
        &mut self,
        id: &str,
        machine: &str,
        actor: &str,
        target: &str,
    ) -> Result<Value> {
        if !self
            .session(id)?
            .config
            .actions
            .iter()
            .any(|family| family == "application.v1")
        {
            return Err(SimError::denied("application interaction is not permitted")
                .because(cw_protocol::reason::APPLICATION_FAMILY_REQUIRED));
        }
        if let Some(result) = self.desktop_panel_action(id, machine, actor, target)? {
            return Ok(result);
        }
        // Insert a whole word, for a suggestion chip or a paste control. One character
        // at a time is `shell:type:`; this is the same pipeline, not a second one.
        if let Some(text) = target.strip_prefix("shell:insert:") {
            if text.is_empty() || text.chars().any(char::is_control) || text.len() > 256 {
                return Err(SimError::invalid("insertable text must be one short line"));
            }
            let text = text.to_owned();
            // An insert chosen from a menu (Notepad's Time/Date) closes the menu first,
            // so the text lands in the document rather than in an open panel.
            self.machine_mut(id, machine)?.desktop.close_menu();
            return self.dispatch(
                id,
                actor,
                &ActionEnvelope::new("keyboard.v1", "type", machine, json!({ "text": text })),
            );
        }
        // A desktop icon named directly, rather than reached by double-clicking it. The
        // name says open, so it opens: the Recycle Bin is a folder, the rest are apps.
        if let Some(kind) = target.strip_prefix("shell:open:") {
            let open = if kind == "trash" {
                "shell:trash".to_owned()
            } else {
                format!("shell:launch:{kind}")
            };
            self.machine_mut(id, machine)?.desktop.desktop_selection = None;
            return self.shell_action(id, machine, actor, &open);
        }
        if let Some(kind) = target.strip_prefix("shell:launch:") {
            // `shell:launch:<kind>/<argument>` opens an application on something: a
            // calendar on a date, a file manager on a folder.
            if let Some((kind, argument)) = kind.split_once('/') {
                let (kind, argument) = (kind.to_owned(), argument.to_owned());
                if !self.runtime.computer(machine)?.application_available(&kind) {
                    return Err(SimError::not_found("application is not installed")
                        .because(cw_protocol::reason::APPLICATION_NOT_INSTALLED));
                }
                return self.dispatch(
                    id,
                    actor,
                    &ActionEnvelope::new(
                        "application.v1",
                        "launch",
                        machine,
                        json!({ "kind": kind, "argument": argument }),
                    ),
                );
            }
            if self.desktop_alias(id, machine, kind)?.is_none()
                && !self.runtime.computer(machine)?.application_available(kind)
            {
                return Err(SimError::not_found("application is not installed")
                    .because(cw_protocol::reason::APPLICATION_NOT_INSTALLED));
            }
            let existing = self.session(id)?.machines[machine]
                .desktop
                .windows
                .values()
                .find(|window| {
                    window.app_id == kind
                        || (window.app_id.is_empty()
                            && matches!(
                                (&window.state, kind),
                                (AppState::Browser { .. }, "browser")
                                    | (AppState::Terminal { .. }, "terminal")
                                    | (AppState::Editor { .. }, "editor")
                                    | (AppState::Files { .. }, "files")
                            ))
                })
                .map(|window| window.id);
            if let Some(window) = existing {
                self.machine_mut(id, machine)?
                    .desktop
                    .focus(window)
                    .map_err(SimError::invalid)?;
                self.sync_desktop_visibility(id, machine)?;
                return Ok(json!({"window":window}));
            }
            return self.dispatch(
                id,
                actor,
                &ActionEnvelope::new("application.v1", "launch", machine, json!({"kind":kind})),
            );
        }
        // On-screen keyboards. A painted key dispatches the same input an actor's own
        // keyboard action would, so the two paths can never diverge.
        // Keyboard modifiers live on the desktop so a painted shift key is a real one.
        if target == "shell:key:Shift" {
            let keyboard = &mut self.machine_mut(id, machine)?.desktop.keyboard;
            keyboard.cycle_shift();
            return Ok(json!({ "shift": keyboard.shift }));
        }
        if let Some(plane) = target.strip_prefix("shell:plane:") {
            let plane = match plane {
                "letters" => cw_applications::Plane::Letters,
                "numbers" => cw_applications::Plane::Numbers,
                "symbols" => cw_applications::Plane::Symbols,
                _ => return Err(SimError::invalid("unknown keyboard plane")),
            };
            let keyboard = &mut self.machine_mut(id, machine)?.desktop.keyboard;
            keyboard.set_plane(plane);
            return Ok(json!({ "plane": plane }));
        }
        if let Some(text) = target.strip_prefix("shell:type:") {
            if text.chars().count() != 1 || text.chars().any(char::is_control) {
                return Err(SimError::invalid("a key types exactly one character"));
            }
            // A letter key types what the modifier says, and spends a one-shot shift.
            let text = match text.chars().next() {
                Some(ch) if ch.is_alphabetic() => {
                    self.machine_mut(id, machine)?.desktop.keyboard.apply(ch)
                }
                _ => text.to_owned(),
            };
            return self.dispatch(
                id,
                actor,
                &ActionEnvelope::new("keyboard.v1", "type", machine, json!({ "text": text })),
            );
        }
        if let Some(key) = target.strip_prefix("shell:key:") {
            let key = key.to_owned();
            return self.dispatch(
                id,
                actor,
                &ActionEnvelope::new("keyboard.v1", "key", machine, json!({ "key": key })),
            );
        }
        // Browser tab strip. Tabs already exist in the browser session; these are the
        // controls that reach them.
        if let Some(rest) = target.strip_prefix("shell:tab:") {
            let (op, payload) = match rest.split_once(':') {
                Some(("select", index)) => (
                    "switch_tab",
                    json!({"tab":index.parse::<u64>().map_err(|_| SimError::invalid("invalid tab"))?}),
                ),
                Some(("close", index)) => (
                    "close_tab",
                    json!({"tab":index.parse::<u64>().map_err(|_| SimError::invalid("invalid tab"))?}),
                ),
                None if rest == "new" => ("new_tab", json!({})),
                _ => return Err(SimError::invalid("unknown tab interaction")),
            };
            return self.browser_action(
                id,
                actor,
                &ActionEnvelope::new("browser.v1", op, machine, payload),
            );
        }
        if matches!(target, "shell:back" | "shell:forward" | "shell:reload") {
            if !self
                .session(id)?
                .config
                .actions
                .iter()
                .any(|family| family == "browser.v1")
            {
                return Err(SimError::denied("browser action is not permitted"));
            }
            return self.browser_action(
                id,
                actor,
                &ActionEnvelope::new(
                    "browser.v1",
                    target.trim_start_matches("shell:"),
                    machine,
                    json!({}),
                ),
            );
        }
        let state = self.machine_mut(id, machine)?;
        match target {
            "shell:launcher" => {
                state.desktop.launcher_open = !state.desktop.launcher_open;
                return Ok(Value::Null);
            }
            "shell:maximize" => {
                if let Some(window) = state.desktop.focused {
                    state
                        .desktop
                        .maximize(window, cw_scene::Rect::new(0, 28, 1024, 660))
                        .map_err(SimError::invalid)?;
                }
                return Ok(Value::Null);
            }
            "shell:home" => state.desktop.home(),
            "shell:minimize" => {
                if let Some(window) = state.desktop.focused {
                    state.desktop.minimize(window).map_err(SimError::invalid)?;
                }
            }
            "shell:close" => {
                if let Some(window) = state.desktop.focused {
                    state.desktop.close(window).map_err(SimError::invalid)?;
                }
            }
            "shell:switcher" => state.desktop.cycle().map_err(SimError::invalid)?,
            "shell:address" => {
                if state.browser_visible {
                    state.address_focused = true;
                    if let Some(window) = state
                        .desktop
                        .focused
                        .and_then(|id| state.desktop.windows.get_mut(&id))
                    {
                        if let AppState::Browser { address } = &mut window.state {
                            address.clear();
                        }
                    }
                }
                return Ok(Value::Null);
            }
            _ => {
                return Err(SimError::invalid("unknown shell interaction")
                    .because(cw_protocol::reason::UNKNOWN_SHELL_TARGET))
            }
        }
        self.sync_desktop_visibility(id, machine)?;
        Ok(Value::Null)
    }
}
