//! Settings. Every switch and level here is the same state the quick settings, control
//! centre and shade read, so the two surfaces can never disagree.
use super::look::{header, look, INK, LINE, MUTED};
use crate::desktop_scene::{shared::Align, DesktopTheme, Painter};
use crate::AppEffect;
use cw_scene::{Color, Rect};
use serde::{Deserialize, Serialize};

/// Sections, and the switches each one owns. Names match `SystemSettings::flag`.
pub const SECTIONS: [(&str, &[&str]); 4] = [
    (
        "Network",
        &["wifi", "bluetooth", "airplane_mode", "hotspot"],
    ),
    ("Display", &["dark_mode", "night_light", "rotation_lock"]),
    ("Sound & Focus", &["do_not_disturb", "flashlight"]),
    ("Battery", &["battery_saver"]),
];
pub const LEVELS: [(&str, &str); 2] = [("brightness", "Brightness"), ("volume", "Volume")];

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// Index into `SECTIONS`.
    pub section: usize,
}
impl Settings {
    pub const KIND: &'static str = "settings";
    pub fn launch(_argument: &str, _window: u64, _clock_us: u64) -> (Self, Vec<AppEffect>) {
        (Self::default(), vec![])
    }
    pub fn kind(&self) -> &'static str {
        Self::KIND
    }
    pub fn title(&self, theme: DesktopTheme) -> String {
        if theme == DesktopTheme::Macos {
            "System Settings".into()
        } else {
            "Settings".into()
        }
    }
    pub fn document(&self) -> String {
        String::new()
    }
    pub fn caption(&self) -> String {
        SECTIONS[self.section.min(SECTIONS.len() - 1)].0.into()
    }
    pub fn modified(&self) -> bool {
        false
    }
    pub fn offline(&mut self, _tag: &str, _reason: &str) {}
    pub fn http(
        &mut self,
        _window: u64,
        _tag: &str,
        _status: u16,
        _body: &str,
    ) -> Result<Vec<AppEffect>, String> {
        Err("settings makes no requests".into())
    }
    pub fn text(&mut self, _text: &str) -> Result<(), String> {
        Err("settings has no text field".into())
    }
    pub fn key(
        &mut self,
        _window: u64,
        key: &str,
        _clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        Err(format!("unsupported settings key {key}"))
    }
    /// Only section changes belong to the app; switches and levels are shell actions
    /// (`shell:toggle:*`, `shell:set:*`) so one store backs every surface that shows them.
    pub fn click(
        &mut self,
        _window: u64,
        target: &str,
        _clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        let index = target
            .strip_prefix("settings:section:")
            .ok_or("interaction does not belong to settings")?;
        let index: usize = index.parse().map_err(|_| "invalid section")?;
        if index >= SECTIONS.len() {
            return Err("section not found".into());
        }
        self.section = index;
        Ok(vec![])
    }
    pub fn page(&self, page: &mut cw_protocol::Page) {
        use cw_protocol::PageElement as E;
        let act = |url: &str| cw_protocol::PageAction {
            method: "APP".into(),
            url: url.into(),
            fields: Default::default(),
        };
        for (index, (name, _)) in SECTIONS.iter().enumerate() {
            page.elements.push(E::Button {
                id: format!("settings:section:{index}"),
                text: (*name).into(),
                action: act(&format!("settings:section:{index}")),
            });
        }
        for switch in SECTIONS[self.section.min(SECTIONS.len() - 1)].1 {
            page.elements.push(E::Button {
                id: format!("shell:toggle:{switch}"),
                text: label_for(switch).into(),
                action: act(&format!("shell:toggle:{switch}")),
            });
        }
        if self.section == 1 {
            for (name, label) in LEVELS {
                page.elements.push(E::Button {
                    id: format!("shell:set:{name}:100"),
                    text: format!("{label} to 100%"),
                    action: act(&format!("shell:set:{name}:100")),
                });
            }
        }
    }
    pub fn render(&self, p: &mut Painter, env: &crate::AppEnv<'_>) {
        let (theme, width, height) = (env.theme, env.width, env.height);
        let l = look(theme);
        p.scene.background = l.surface;
        let top = header(p, theme, &l, width, &self.title(theme));
        let section = self.section.min(SECTIONS.len() - 1);
        let list_w = if theme.mobile() || width < 520 {
            0
        } else {
            200
        };
        if list_w > 0 {
            p.box_(Rect::new(0, top, list_w, height), l.chrome, 0);
            p.vline(list_w as i32 - 1, top, height, LINE);
        }
        for (index, (name, _)) in SECTIONS.iter().enumerate() {
            let r = if list_w > 0 {
                Rect::new(8, top + 10 + index as i32 * 32, list_w - 16, 30)
            } else {
                let w = width / SECTIONS.len() as u32;
                Rect::new(index as i32 * w as i32, top + 4, w, 30)
            };
            let on = index == section;
            p.button(
                r,
                if on { l.selection } else { Color::TRANSPARENT },
                l.radius,
                &format!("settings:section:{index}"),
                name,
            );
            p.label(
                r.x + if list_w > 0 { 12 } else { 0 },
                r.y + 6,
                r.width.saturating_sub(20),
                name,
                12,
                if on { l.accent } else { INK },
                on,
                if list_w > 0 {
                    Align::Left
                } else {
                    Align::Center
                },
            );
        }
        let x = list_w as i32;
        let pane = width.saturating_sub(list_w);
        let mut y = top + if list_w > 0 { 12 } else { 42 };
        for switch in SECTIONS[section].1 {
            let r = Rect::new(x + 16, y, pane.saturating_sub(32), 38);
            if r.y as u32 + 38 > height {
                break;
            }
            switch_row(p, &l, r, switch, env.switch(switch));
            y += 42;
        }
        if section == 1 {
            for (name, label) in LEVELS {
                if y as u32 + 52 > height {
                    break;
                }
                let level = env.level(name);
                p.left(
                    x + 16,
                    y,
                    pane.saturating_sub(32),
                    &format!("{label}  {level}%"),
                    12,
                    MUTED,
                );
                slider(
                    p,
                    &l,
                    Rect::new(x + 16, y + 18, pane.saturating_sub(32), 24),
                    name,
                    level,
                );
                y += 54;
            }
        }
    }
}
/// One switch row, drawn in its true position and clicking to flip the shared store.
fn switch_row(p: &mut Painter, l: &super::look::Look, r: Rect, switch: &str, on: bool) {
    p.button(
        r,
        Color::TRANSPARENT,
        l.radius,
        &format!("shell:toggle:{switch}"),
        label_for(switch),
    );
    p.hline(r.x, r.y + r.height as i32, r.width, LINE);
    p.left(
        r.x + 4,
        r.y + 10,
        r.width.saturating_sub(80),
        label_for(switch),
        13,
        INK,
    );
    let track = Rect::new(r.x + r.width as i32 - 52, r.y + 8, 44, 22);
    p.box_(track, if on { l.accent } else { Color(0, 0, 0, 34) }, 11);
    p.circle(
        if on { track.x + 33 } else { track.x + 11 },
        track.y + 11,
        9,
        Color::WHITE,
    );
}
/// Discrete steps, so a click lands on an exact percentage.
fn slider(p: &mut Painter, l: &super::look::Look, r: Rect, name: &str, filled: u8) {
    const STEPS: u32 = 20;
    let step_w = r.width / STEPS;
    for index in 0..STEPS {
        let percent = (index + 1) * 100 / STEPS;
        let cell = Rect::new(
            r.x + (index * step_w) as i32,
            r.y,
            step_w.saturating_sub(1),
            r.height,
        );
        p.button(
            cell,
            if percent <= u32::from(filled) {
                l.accent
            } else {
                Color(0, 0, 0, 18)
            },
            0,
            &format!("shell:set:{name}:{percent}"),
            &format!("{name} {percent}%"),
        );
    }
}
fn label_for(switch: &str) -> &str {
    match switch {
        "wifi" => "Wi-Fi",
        "bluetooth" => "Bluetooth",
        "airplane_mode" => "Airplane mode",
        "hotspot" => "Personal hotspot",
        "dark_mode" => "Dark appearance",
        "night_light" => "Night light",
        "rotation_lock" => "Rotation lock",
        "do_not_disturb" => "Do not disturb",
        "flashlight" => "Flashlight",
        "battery_saver" => "Battery saver",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sections_switch_and_unknown_ones_are_refused() {
        let (mut app, _) = Settings::launch("", 1, 0);
        assert_eq!(app.section, 0);
        app.click(1, "settings:section:2", 0).unwrap();
        assert_eq!(app.section, 2);
        assert!(app.click(1, "settings:section:9", 0).is_err());
        assert!(app.click(1, "shell:toggle:wifi", 0).is_err());
    }
    #[test]
    fn switches_and_levels_are_painted_as_shell_actions_not_app_state() {
        let (app, _) = Settings::launch("", 1, 0);
        let mut scene = Painter::themed(DesktopTheme::Macos, 820, 560, 0);
        app.render(
            &mut scene,
            &crate::AppEnv {
                theme: DesktopTheme::Macos,
                width: 820,
                height: 560,
                clock_us: 0,
                settings: &crate::SystemSettings::DEFAULT,
                clipboard: None,
                share_to: None,
            },
        );
        let targets: Vec<_> = scene
            .scene
            .nodes
            .iter()
            .filter_map(|n| n.interaction.clone())
            .collect();
        assert!(targets.iter().any(|t| t == "shell:toggle:wifi"));
        assert!(targets
            .iter()
            .all(|t| t.starts_with("settings:section:") || t.starts_with("shell:")));
    }
    #[test]
    fn every_switch_name_is_one_the_system_store_knows() {
        for (_, switches) in SECTIONS {
            for switch in switches {
                assert!(
                    crate::SystemSettings::DEFAULT.flag(switch).is_ok(),
                    "{switch} is not a real system switch"
                );
            }
        }
        for (name, _) in LEVELS {
            assert!(crate::SystemSettings::DEFAULT.level(name).is_ok());
        }
    }
}
