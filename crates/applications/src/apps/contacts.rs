//! Contacts assembled from the people the machine's own services really know about:
//! mailbox users and the Messages directory of names and handles. No address book is
//! invented.
use super::look::{action, look, notice, screen, INK, LINE, MUTED};
use super::Status;
use crate::desktop_scene::{shared::Align, DesktopTheme, Painter};
use crate::AppEffect;
use cw_scene::{Color, Rect};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contacts {
    pub mail_base: String,
    #[serde(alias = "chat_base")]
    pub messages_base: String,
    /// Person to what is known about them: their name and the handles Messages reaches
    /// them on, or nothing but the address mail came from.
    pub people: BTreeMap<String, Vec<String>>,
    pub selected: Option<String>,
    pub status: Status,
}
impl Contacts {
    pub const KIND: &'static str = "contacts";
    pub fn launch(argument: &str, window: u64, _clock_us: u64) -> (Self, Vec<AppEffect>) {
        // The argument may carry both bases as "mail|messages"; either half may be empty.
        let (mail, messages) = argument.split_once('|').unwrap_or((argument, ""));
        let app = Self {
            mail_base: if mail.is_empty() {
                "http://mail.internal/".into()
            } else {
                mail.to_owned()
            },
            messages_base: if messages.is_empty() {
                "http://messages.internal/".into()
            } else {
                messages.to_owned()
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
                tag: "contacts".into(),
                method: "GET".into(),
                url: format!("{}/api/contacts", self.messages_base.trim_end_matches('/')),
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
            "contacts" => {
                #[derive(Deserialize)]
                struct Contact {
                    user: String,
                    #[serde(default)]
                    name: String,
                    #[serde(default)]
                    handles: Vec<String>,
                }
                let contacts: Vec<Contact> = serde_json::from_str(body).unwrap_or_default();
                for contact in contacts {
                    let known = self.people.entry(contact.user).or_default();
                    for line in std::iter::once(contact.name)
                        .chain(contact.handles)
                        .filter(|l| !l.is_empty())
                    {
                        if !known.contains(&line) {
                            known.push(line);
                        }
                    }
                }
                let _ = window;
                Ok(vec![])
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
            other => Err(format!("unexpected contacts reply {other}")),
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
            // A phone's back button: from a contact card to the list.
            "back" => {
                self.selected.take().ok_or("no contact is open")?;
                Ok(vec![])
            }
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
            style: None,
        });
        for (person, shared) in &self.people {
            page.elements.push(E::Button {
                id: format!("contacts:person:{person}"),
                text: format!("{person} ({})", shared.join(", ")),
                action: act(&format!("contacts:person:{person}")),
                style: None,
            });
        }
        if let Some(person) = &self.selected {
            page.elements.push(E::Button {
                id: format!("contacts:mail:{person}"),
                text: format!("Write to {person}"),
                action: act(&format!("contacts:mail:{person}")),
                style: None,
            });
        }
    }
    pub fn render(&self, p: &mut Painter, env: &crate::AppEnv<'_>) {
        let (theme, width, height) = (env.theme, env.width, env.height);
        let l = look(theme);
        p.scene.background = l.surface;
        let narrow = theme.mobile() || width < 520;
        // A phone shows a contact's card in place of the list.
        if narrow {
            if let Some(person) = self
                .selected
                .as_ref()
                .filter(|p| self.people.contains_key(*p))
            {
                let back = Rect::new(4, 6, 110, 30);
                p.button(
                    back,
                    Color::TRANSPARENT,
                    l.radius,
                    "contacts:back",
                    "Back to contacts",
                );
                p.symbol("chevron-left", back.x + 2, back.y + 5, 20, l.accent);
                p.left(
                    back.x + 24,
                    back.y + 6,
                    86,
                    &self.title(theme),
                    15,
                    l.accent,
                );
                self.card(p, &l, width, person, 0, 40);
                return;
            }
        }
        let screen = screen(p, theme, &l, width, height as i32, &self.title(theme));
        let top = screen.top;
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
            screen.end(p);
            return;
        }
        if self.people.is_empty() {
            notice(p, width, top + 40, "No contacts yet");
            screen.end(p);
            return;
        }
        let list_w = if narrow { width } else { 260 };
        let list = screen.column(
            p,
            "list",
            Rect::new(
                0,
                top + 38,
                list_w,
                (height as i32 - top - 38).max(1) as u32,
            ),
        );
        let mut y = list.top + 2;
        for (person, shared) in &self.people {
            let r = Rect::new(4, y, list_w.saturating_sub(8), l.row.max(36));
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
        list.end(p);
        screen.end(p);
        if narrow {
            return;
        }
        let x = list_w as i32;
        p.vline(x, top, height, LINE);
        match &self.selected {
            Some(person) => self.card(p, &l, width, person, x, top),
            None => notice(
                p,
                width.saturating_sub(list_w),
                top + 40,
                "Select a contact",
            ),
        }
    }
    /// A contact's card from `x` rightwards and `top` down.
    fn card(
        &self,
        p: &mut Painter,
        l: &super::look::Look,
        width: u32,
        person: &str,
        x: i32,
        top: i32,
    ) {
        let text_w = width.saturating_sub(x as u32 + 40);
        p.strong(x + 20, top + 20, text_w, person, 18, INK);
        let shared = self.people.get(person).cloned().unwrap_or_default();
        p.left(
            x + 20,
            top + 46,
            text_w,
            &if shared.is_empty() {
                "Known only from mail".to_owned()
            } else {
                format!("Reach at: {}", shared.join(", "))
            },
            12,
            MUTED,
        );
        action(
            p,
            l,
            Rect::new(x + 20, top + 74, 132, 28),
            "Write a message",
            &format!("contacts:mail:{person}"),
            true,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app() -> Contacts {
        let (mut app, effects) = Contacts::launch("|", 1, 0);
        assert_eq!(effects.len(), 2);
        let more = app
            .http(
                1,
                "contacts",
                200,
                r#"[{"user":"alice","name":"Alice Chen","handles":["+14155550100"],"imessage":true},{"user":"bob","name":"Bob Martinez","handles":["+14155550101"],"imessage":true}]"#,
            )
            .unwrap();
        assert!(more.is_empty());
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
        assert_eq!(app.people["alice"], vec!["Alice Chen", "+14155550100"]);
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
