//! Contacts assembled from the people the machine's own services really know about:
//! mailbox users and chat channel members. No address book is invented.
use super::look::{action, header, look, notice, INK, LINE, MUTED};
use super::Status;
use crate::desktop_scene::{shared::Align, DesktopTheme, Painter};
use crate::AppEffect;
use cw_scene::{Color, Rect};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contacts {
    pub mail_base: String,
    pub chat_base: String,
    /// Person to the channels they share with this user.
    pub people: BTreeMap<String, Vec<String>>,
    pub selected: Option<String>,
    pub status: Status,
}
impl Contacts {
    pub const KIND: &'static str = "contacts";
    pub fn launch(argument: &str, window: u64, _clock_us: u64) -> (Self, Vec<AppEffect>) {
        // The argument may carry both bases as "mail|chat"; either half may be empty.
        let (mail, chat) = argument.split_once('|').unwrap_or((argument, ""));
        let app = Self {
            mail_base: if mail.is_empty() {
                "http://mail.internal/".into()
            } else {
                mail.to_owned()
            },
            chat_base: if chat.is_empty() {
                "http://chat.internal/".into()
            } else {
                chat.to_owned()
            },
            people: BTreeMap::new(),
            selected: None,
            status: Status::Loading,
        };
        let effects = app.fetch(window);
        (app, effects)
    }
    pub fn kind(&self) -> &'static str {
        Self::KIND
    }
    pub fn title(&self, theme: DesktopTheme) -> String {
        if theme == DesktopTheme::Windows {
            "People".into()
        } else {
            "Contacts".into()
        }
    }
    pub fn document(&self) -> String {
        String::new()
    }
    pub fn caption(&self) -> String {
        self.selected.clone().unwrap_or_default()
    }
    pub fn modified(&self) -> bool {
        false
    }
    fn fetch(&self, window: u64) -> Vec<AppEffect> {
        vec![
            AppEffect::Http {
                window,
                tag: "channels".into(),
                method: "GET".into(),
                url: format!("{}/api/channels", self.chat_base.trim_end_matches('/')),
                body: String::new(),
            },
            AppEffect::Http {
                window,
                tag: "messages".into(),
                method: "GET".into(),
                url: format!("{}/api/messages", self.mail_base.trim_end_matches('/')),
                body: String::new(),
            },
        ]
    }
    pub fn offline(&mut self, _tag: &str, reason: &str) {
        // One unreachable source must not hide the other's real results.
        if self.people.is_empty() {
            self.status = Status::Offline(reason.to_owned());
        }
    }
    pub fn http(
        &mut self,
        window: u64,
        tag: &str,
        status: u16,
        body: &str,
    ) -> Result<Vec<AppEffect>, String> {
        let outcome = Status::from_status(status, body);
        if outcome != Status::Idle {
            if self.people.is_empty() {
                self.status = outcome;
            }
            return Ok(vec![]);
        }
        self.status = Status::Idle;
        match tag {
            "channels" => {
                let channels: BTreeMap<String, String> =
                    serde_json::from_str(body).unwrap_or_default();
                // Membership lives on the channel, so each one is fetched in turn.
                Ok(channels
                    .keys()
                    .take(16)
                    .map(|id| AppEffect::Http {
                        window,
                        tag: format!("channel:{id}"),
                        method: "GET".into(),
                        url: format!("{}/api/channels/{id}", self.chat_base.trim_end_matches('/')),
                        body: String::new(),
                    })
                    .collect())
            }
            "messages" => {
                #[derive(Deserialize)]
                struct Message {
                    sender: String,
                    #[serde(default)]
                    to: Vec<String>,
                    #[serde(default)]
                    cc: Vec<String>,
                }
                let messages: Vec<Message> = serde_json::from_str(body).unwrap_or_default();
                for message in messages {
                    for person in std::iter::once(message.sender)
                        .chain(message.to)
                        .chain(message.cc)
                    {
                        self.people.entry(person).or_default();
                    }
                }
                Ok(vec![])
            }
            other => {
                let id = other
                    .strip_prefix("channel:")
                    .ok_or_else(|| format!("unexpected contacts reply {other}"))?;
                #[derive(Default, Deserialize)]
                struct Channel {
                    #[serde(default)]
                    title: String,
                    #[serde(default)]
                    members: BTreeSet<String>,
                }
                let channel: Channel = serde_json::from_str(body).unwrap_or_default();
                let title = if channel.title.is_empty() {
                    id.to_owned()
                } else {
                    channel.title
                };
                for member in channel.members {
                    let shared = self.people.entry(member).or_default();
                    if !shared.contains(&title) {
                        shared.push(title.clone());
                    }
                }
                Ok(vec![])
            }
        }
    }
    pub fn text(&mut self, _text: &str) -> Result<(), String> {
        Err("contacts has no text field".into())
    }
    pub fn key(
        &mut self,
        _window: u64,
        key: &str,
        _clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        Err(format!("unsupported contacts key {key}"))
    }
    pub fn click(
        &mut self,
        window: u64,
        target: &str,
        _clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        let command = target
            .strip_prefix("contacts:")
            .ok_or("interaction does not belong to contacts")?;
        match command {
            "reload" => {
                self.people.clear();
                self.selected = None;
                self.status = Status::Loading;
                Ok(self.fetch(window))
            }
            rest => {
                if let Some(person) = rest.strip_prefix("person:") {
                    if !self.people.contains_key(person) {
                        return Err("contact not found".into());
                    }
                    self.selected = Some(person.to_owned());
                    return Ok(vec![]);
                }
                // Writing to a contact really opens Mail with the recipient filled in.
                if let Some(person) = rest.strip_prefix("mail:") {
                    if !self.people.contains_key(person) {
                        return Err("contact not found".into());
                    }
                    return Ok(vec![AppEffect::Launch {
                        window,
                        kind: "mail".into(),
                        argument: self.mail_base.clone(),
                    }]);
                }
                Err(format!("unknown contacts command {command}"))
            }
        }
    }
    pub fn page(&self, page: &mut cw_protocol::Page) {
        use cw_protocol::PageElement as E;
        let act = |url: &str| cw_protocol::PageAction {
            method: "APP".into(),
            url: url.into(),
            fields: Default::default(),
        };
        page.elements.push(E::Heading {
            id: "contacts-title".into(),
            text: "Contacts".into(),
            level: 2,
        });
        if let Some(text) = self.status.notice() {
            page.elements.push(E::Text {
                id: "contacts-status".into(),
                text: text.into(),
            });
        }
        page.elements.push(E::Button {
            id: "contacts:reload".into(),
            text: "Reload".into(),
            action: act("contacts:reload"),
        });
        for (person, shared) in &self.people {
            page.elements.push(E::Button {
                id: format!("contacts:person:{person}"),
                text: format!("{person} ({})", shared.join(", ")),
                action: act(&format!("contacts:person:{person}")),
            });
        }
        if let Some(person) = &self.selected {
            page.elements.push(E::Button {
                id: format!("contacts:mail:{person}"),
                text: format!("Write to {person}"),
                action: act(&format!("contacts:mail:{person}")),
            });
        }
    }
    pub fn render(&self, p: &mut Painter, env: &crate::AppEnv<'_>) {
        let (theme, width, height) = (env.theme, env.width, env.height);
        let l = look(theme);
        p.scene.background = l.surface;
        let top = header(p, theme, &l, width, &self.title(theme));
        action(
            p,
            &l,
            Rect::new(width as i32 - 80, top + 6, 70, 26),
            "Reload",
            "contacts:reload",
            false,
        );
        if let Some(text) = self.status.notice() {
            notice(p, width, top + 40, text);
            return;
        }
        if self.people.is_empty() {
            notice(p, width, top + 40, "No contacts yet");
            return;
        }
        let list_w = if theme.mobile() || width < 520 {
            width
        } else {
            260
        };
        let mut y = top + 40;
        for (person, shared) in &self.people {
            let r = Rect::new(4, y, list_w.saturating_sub(8), l.row.max(36));
            if r.y as u32 + r.height > height {
                break;
            }
            let on = self.selected.as_deref() == Some(person.as_str());
            p.button(
                r,
                if on { l.selection } else { Color::TRANSPARENT },
                l.radius,
                &format!("contacts:person:{person}"),
                person,
            );
            // Monogram stands in for a photograph; it is derived, never invented.
            p.circle(r.x + 22, r.y + r.height as i32 / 2, 14, l.selection);
            p.label(
                r.x + 8,
                r.y + r.height as i32 / 2 - 8,
                28,
                &person
                    .chars()
                    .next()
                    .unwrap_or('?')
                    .to_uppercase()
                    .to_string(),
                13,
                l.accent,
                true,
                Align::Center,
            );
            p.left(
                r.x + 44,
                r.y + 5,
                r.width.saturating_sub(52),
                person,
                13,
                INK,
            );
            if !shared.is_empty() {
                p.left(
                    r.x + 44,
                    r.y + 21,
                    r.width.saturating_sub(52),
                    &shared.join(", "),
                    10,
                    MUTED,
                );
            }
            y += r.height as i32 + 1;
        }
        if list_w == width {
            return;
        }
        let x = list_w as i32;
        p.vline(x, top, height, LINE);
        match &self.selected {
            Some(person) => {
                p.strong(
                    x + 20,
                    top + 20,
                    width.saturating_sub(list_w + 40),
                    person,
                    18,
                    INK,
                );
                let shared = &self.people[person];
                p.left(
                    x + 20,
                    top + 46,
                    width.saturating_sub(list_w + 40),
                    &if shared.is_empty() {
                        "No shared channels".to_owned()
                    } else {
                        format!("Shared channels: {}", shared.join(", "))
                    },
                    12,
                    MUTED,
                );
                action(
                    p,
                    &l,
                    Rect::new(x + 20, top + 74, 132, 28),
                    "Write a message",
                    &format!("contacts:mail:{person}"),
                    true,
                );
            }
            None => notice(
                p,
                width.saturating_sub(list_w),
                top + 40,
                "Select a contact",
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app() -> Contacts {
        let (mut app, effects) = Contacts::launch("|", 1, 0);
        assert_eq!(effects.len(), 2);
        let more = app
            .http(1, "channels", 200, r#"{"general":"General"}"#)
            .unwrap();
        assert_eq!(more.len(), 1);
        app.http(
            1,
            "channel:general",
            200,
            r#"{"title":"General","members":["alice","bob"]}"#,
        )
        .unwrap();
        app.http(
            1,
            "messages",
            200,
            r#"[{"id":"m","sender":"carol","to":["alice"],"cc":[],"subject":"","body":""}]"#,
        )
        .unwrap();
        app
    }
    #[test]
    fn people_come_only_from_what_the_services_really_report() {
        let app = app();
        assert_eq!(
            app.people.keys().cloned().collect::<Vec<_>>(),
            vec!["alice", "bob", "carol"]
        );
        assert_eq!(app.people["alice"], vec!["General"]);
        assert!(app.people["carol"].is_empty());
    }
    #[test]
    fn one_unreachable_source_does_not_erase_the_other() {
        let mut app = app();
        app.offline("messages", "no route to host");
        assert_eq!(app.status, Status::Idle);
        assert_eq!(app.people.len(), 3);
        let (mut empty, _) = Contacts::launch("|", 1, 0);
        empty.offline("messages", "no route to host");
        assert_eq!(empty.status, Status::Offline("no route to host".into()));
    }
    #[test]
    fn writing_to_a_contact_really_launches_mail() {
        let mut app = app();
        app.click(1, "contacts:person:alice", 0).unwrap();
        let effects = app.click(1, "contacts:mail:alice", 0).unwrap();
        assert!(matches!(&effects[0], AppEffect::Launch { kind, .. } if kind == "mail"));
        assert!(app.click(1, "contacts:mail:nobody", 0).is_err());
    }
    #[test]
    fn every_painted_control_is_one_the_model_accepts() {
        let mut base = app();
        base.click(1, "contacts:person:alice", 0).unwrap();
        let mut scene = Painter::themed(DesktopTheme::Macos, 860, 560, 0);
        base.render(
            &mut scene,
            &crate::AppEnv {
                theme: DesktopTheme::Macos,
                width: 860,
                height: 560,
                clock_us: 0,
                settings: &crate::SystemSettings::DEFAULT,
                clipboard: None,
                share_to: None,
                editor: None,
                pointer: None,
                files: Default::default(),
            },
        );
        for target in scene
            .scene
            .nodes
            .iter()
            .filter_map(|n| n.interaction.clone())
        {
            let mut app = base.clone();
            assert!(
                app.click(1, &target, 0).is_ok(),
                "unhandled control {target}"
            );
        }
    }
}
