use super::*;

impl Environment {
    pub(super) fn interface_versions(&self) -> BTreeMap<String, u32> {
        self.extensions
            .iter()
            .map(|(k, v)| (format!("action:{k}"), v.version()))
            .chain(
                self.observation_extensions
                    .iter()
                    .map(|(k, v)| (format!("observation:{k}"), v.version())),
            )
            .collect()
    }
    pub(super) fn validate_snapshot(&self, snapshot: &Snapshot) -> Result<()> {
        if snapshot.interface_modules != self.interface_versions() {
            return Err(SimError::invalid("interface module versions differ"));
        }
        if snapshot.app_modules != self.app_registry.module_versions() {
            return Err(SimError::invalid("application module versions differ"));
        }
        for session in snapshot.sessions.values() {
            if !session.machines.contains_key(&session.focused_machine)
                || session.config.machines.len() != session.machines.len()
            {
                return Err(SimError::invalid("invalid actor snapshot"));
            }
            for (id, m) in &session.machines {
                if !session.config.machines.contains(id)
                    || !snapshot
                        .kernel
                        .definition()
                        .computers
                        .iter()
                        .any(|c| &c.id == id)
                {
                    return Err(SimError::invalid("invalid machine grant"));
                }
                if m.browser.tabs.is_empty()
                    || m.browser.active >= m.browser.tabs.len()
                    || m.browser
                        .tabs
                        .iter()
                        .any(|t| !t.history.is_empty() && t.position >= t.history.len())
                {
                    return Err(SimError::invalid("invalid browser checkpoint"));
                }
                m.browser.validate_assets()?;
                for (window, browser) in &m.browser_windows {
                    if !m
                        .desktop
                        .windows
                        .get(window)
                        .is_some_and(|w| matches!(w.state, AppState::Browser { .. }))
                        || browser.tabs.is_empty()
                        || browser.active >= browser.tabs.len()
                        || browser
                            .tabs
                            .iter()
                            .any(|t| !t.history.is_empty() && t.position >= t.history.len())
                    {
                        return Err(SimError::invalid("invalid background browser checkpoint"));
                    }
                    browser.validate_assets()?;
                }
                if m.desktop
                    .stacking
                    .iter()
                    .any(|id| !m.desktop.windows.contains_key(id))
                    || m.desktop
                        .stacking
                        .iter()
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
                        != m.desktop.stacking.len()
                {
                    return Err(SimError::invalid("invalid window stacking"));
                }
                if m.desktop
                    .focused
                    .is_some_and(|id| !m.desktop.windows.get(&id).is_some_and(|w| !w.minimized))
                {
                    return Err(SimError::invalid("invalid window focus"));
                }
                for window in m.desktop.windows.values() {
                    for frame in [window.frame, window.restored_frame].into_iter().flatten() {
                        if frame.width == 0
                            || frame.height == 0
                            || frame.width > 32768
                            || frame.height > 32768
                            || frame.x.unsigned_abs() > 32768
                            || frame.y.unsigned_abs() > 32768
                        {
                            return Err(SimError::invalid("invalid window geometry"));
                        }
                    }
                }
                if let Some(capture) = &m.desktop.pointer_capture {
                    if !m.desktop.windows.contains_key(&capture.window)
                        || !matches!(
                            capture.operation.as_str(),
                            "drag"
                                | "resize:n"
                                | "resize:ne"
                                | "resize:e"
                                | "resize:se"
                                | "resize:s"
                                | "resize:sw"
                                | "resize:w"
                                | "resize:nw"
                        )
                    {
                        return Err(SimError::invalid("invalid pointer capture"));
                    }
                }

                for instance in m.registered.instances.values() {
                    if self.app_registry.application(&instance.kind)?.version() != instance.version
                    {
                        return Err(SimError::invalid("application version mismatch"));
                    }
                }
            }
        }
        Ok(())
    }
}
