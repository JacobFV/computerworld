//! Mail client over the `mail` service. Messages, folders, read state and sending are all
//! real service records; an empty mailbox renders empty rather than inventing senders.
use super::look::{action, header, look, notice, FAINT, INK, LINE, MUTED};
use super::{push_bounded, Status};
use crate::desktop_scene::{shared::Align, DesktopTheme, Painter};
use crate::AppEffect;
use cw_scene::{Color, Rect};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub sender: String,
    #[serde(default)]
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    pub subject: String,
    pub body: String,
    #[serde(default)]
    pub time: u64,
    /// Per-user mailbox metadata; the service returns only the reader's own entry.
    #[serde(default)]
    pub mailboxes: BTreeMap<String, Mailbox>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mailbox {
    #[serde(default)]
    pub folders: Vec<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub read: bool,
}
impl Message {
    pub fn unread(&self) -> bool {
        self.mailboxes.values().any(|m| !m.read)
    }
}
/// Compose sheet. Empty recipients are refused before a request is ever made.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Compose {
    pub to: String,
    pub subject: String,
    pub body: String,
    /// Which field keystrokes land in.
    pub field: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mail {
    pub base: String,
    pub folder: String,
    pub messages: Vec<Message>,
    pub selected: Option<String>,
    pub compose: Option<Compose>,
    pub status: Status,
}
pub const FOLDERS: [&str; 3] = ["inbox", "sent", "archive"];
impl Mail {
    pub const KIND: &'static str = "mail";
    pub fn launch(argument: &str, window: u64, _clock_us: u64) -> (Self, Vec<AppEffect>) {
        // Opened *on* something — a shared file or page — it starts a message about it,
        // addressed to nobody yet: the actor still chooses who gets it.
        let (base, shared) = super::launch_target(argument);
        let compose = (!shared.is_empty()).then(|| {
            let name = shared
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .filter(|n| !n.is_empty())
                .unwrap_or(shared);
            let mut subject = String::new();
            push_bounded(&mut subject, name, 200);
            let mut body = String::new();
            push_bounded(&mut body, shared, 8192);
            Compose {
                to: String::new(),
                subject,
                body,
                field: "to".into(),
            }
        });
        let app = Self {
            base: if base.is_empty() {
                "http://mail.internal/".into()
            } else {
                base.to_owned()
            },
            folder: "inbox".into(),
            messages: vec![],
            selected: None,
            compose,
            status: Status::Loading,
        };
        let effects = app.fetch(window);
        (app, effects)
    }
    pub fn kind(&self) -> &'static str {
        Self::KIND
    }
    pub fn title(&self, theme: DesktopTheme) -> String {
        match theme {
            DesktopTheme::Windows => "Outlook",
            DesktopTheme::Android => "Gmail",
            DesktopTheme::Ubuntu => "Thunderbird",
            _ => "Mail",
        }
        .into()
    }
    pub fn document(&self) -> String {
        String::new()
    }
    pub fn caption(&self) -> String {
        self.selected
            .as_ref()
            .and_then(|id| self.messages.iter().find(|m| &m.id == id))
            .map(|m| m.subject.clone())
            .unwrap_or_default()
    }
    pub fn modified(&self) -> bool {
        self.compose.is_some()
    }
    fn url(&self, suffix: &str) -> String {
        format!("{}{suffix}", self.base.trim_end_matches('/'))
    }
    fn fetch(&self, window: u64) -> Vec<AppEffect> {
        vec![AppEffect::Http {
            window,
            tag: "messages".into(),
            method: "GET".into(),
            url: self.url(&format!("/api/messages?folder={}", self.folder)),
            body: String::new(),
        }]
    }
    pub fn offline(&mut self, _tag: &str, reason: &str) {
        self.status = Status::Offline(reason.to_owned());
    }
    pub fn http(
        &mut self,
        window: u64,
        tag: &str,
        status: u16,
        body: &str,
    ) -> Result<Vec<AppEffect>, String> {
        self.status = Status::from_status(status, body);
        if self.status != Status::Idle {
            return Ok(vec![]);
        }
        match tag {
            "messages" => {
                self.messages = serde_json::from_str(body).unwrap_or_default();
                self.messages
                    .sort_by(|a, b| (b.time, &b.id).cmp(&(a.time, &a.id)));
                Ok(vec![])
            }
            "send" | "read" | "archive" => {
                self.compose = None;
                self.status = Status::Loading;
                Ok(self.fetch(window))
            }
            other => Err(format!("unexpected mail reply {other}")),
        }
    }
    pub fn text(&mut self, text: &str) -> Result<(), String> {
        let compose = self.compose.as_mut().ok_or("no message is being written")?;
        let (field, limit) = match compose.field.as_str() {
            "to" => (&mut compose.to, 320),
            "subject" => (&mut compose.subject, 200),
            _ => (&mut compose.body, 8192),
        };
        push_bounded(field, text, limit);
        Ok(())
    }
    pub fn key(&mut self, window: u64, key: &str, clock_us: u64) -> Result<Vec<AppEffect>, String> {
        match key {
            "Backspace" => {
                let compose = self.compose.as_mut().ok_or("no message is being written")?;
                match compose.field.as_str() {
                    "to" => compose.to.pop(),
                    "subject" => compose.subject.pop(),
                    _ => compose.body.pop(),
                };
                Ok(vec![])
            }
            "Enter" => {
                let compose = self.compose.as_mut().ok_or("no message is being written")?;
                if compose.field == "body" {
                    compose.body.push('\n');
                    return Ok(vec![]);
                }
                self.click(window, "mail:send", clock_us)
            }
            "Escape" => {
                self.compose = None;
                Ok(vec![])
            }
            other => Err(format!("unsupported mail key {other}")),
        }
    }
    pub fn click(
        &mut self,
        window: u64,
        target: &str,
        _clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        let command = target
            .strip_prefix("mail:")
            .ok_or("interaction does not belong to mail")?;
        match command {
            "reload" => {
                self.status = Status::Loading;
                Ok(self.fetch(window))
            }
            "compose" => {
                self.compose = Some(Compose {
                    field: "to".into(),
                    ..Default::default()
                });
                Ok(vec![])
            }
            "cancel" => {
                self.compose = None;
                Ok(vec![])
            }
            "send" => {
                let compose = self.compose.clone().ok_or("no message is being written")?;
                let to: Vec<_> = compose
                    .to
                    .split([',', ' ', ';'])
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect();
                if to.is_empty() {
                    return Err("a message needs a recipient".into());
                }
                self.status = Status::Loading;
                Ok(vec![AppEffect::Http {
                    window,
                    tag: "send".into(),
                    method: "POST".into(),
                    url: self.url("/api/messages"),
                    body: serde_json::json!({
                        "to": to,
                        "subject": compose.subject,
                        "body": compose.body,
                    })
                    .to_string(),
                }])
            }
            "archive" => {
                let id = self.selected.clone().ok_or("no message is selected")?;
                self.selected = None;
                self.status = Status::Loading;
                Ok(vec![AppEffect::Http {
                    window,
                    tag: "archive".into(),
                    method: "POST".into(),
                    url: self.url(&format!("/api/messages/{id}")),
                    body: serde_json::json!({ "archive": true }).to_string(),
                }])
            }
            "reply" => {
                let message = self
                    .selected
                    .as_ref()
                    .and_then(|id| self.messages.iter().find(|m| &m.id == id))
                    .ok_or("no message is selected")?;
                self.compose = Some(Compose {
                    to: message.sender.clone(),
                    subject: if message.subject.starts_with("Re: ") {
                        message.subject.clone()
                    } else {
                        format!("Re: {}", message.subject)
                    },
                    body: String::new(),
                    field: "body".into(),
                });
                Ok(vec![])
            }
            rest => {
                if let Some(folder) = rest.strip_prefix("folder:") {
                    if !FOLDERS.contains(&folder) {
                        return Err(format!("unknown folder {folder}"));
                    }
                    self.folder = folder.into();
                    self.selected = None;
                    self.status = Status::Loading;
                    return Ok(self.fetch(window));
                }
                if let Some(field) = rest.strip_prefix("field:") {
                    let compose = self.compose.as_mut().ok_or("no message is being written")?;
                    if !["to", "subject", "body"].contains(&field) {
                        return Err(format!("unknown field {field}"));
                    }
                    compose.field = field.into();
                    return Ok(vec![]);
                }
                if let Some(id) = rest.strip_prefix("open:") {
                    let message = self
                        .messages
                        .iter()
                        .find(|m| m.id == id)
                        .ok_or("message not found")?;
                    let unread = message.unread();
                    self.selected = Some(id.to_owned());
                    // Opening an unread message really marks it read on the service.
                    if unread {
                        return Ok(vec![AppEffect::Http {
                            window,
                            tag: "read".into(),
                            method: "POST".into(),
                            url: self.url(&format!("/api/messages/{id}")),
                            body: serde_json::json!({ "read": true }).to_string(),
                        }]);
                    }
                    return Ok(vec![]);
                }
                Err(format!("unknown mail command {command}"))
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
            id: "mail-folder".into(),
            text: self.folder.clone(),
            level: 2,
        });
        if let Some(text) = self.status.notice() {
            page.elements.push(E::Text {
                id: "mail-status".into(),
                text: text.into(),
            });
        }
        for folder in FOLDERS {
            page.elements.push(E::Button {
                id: format!("mail:folder:{folder}"),
                text: folder.into(),
                action: act(&format!("mail:folder:{folder}")),
            });
        }
        for (id, label) in [("mail:compose", "Compose"), ("mail:reload", "Reload")] {
            page.elements.push(E::Button {
                id: id.into(),
                text: label.into(),
                action: act(id),
            });
        }
        for message in &self.messages {
            page.elements.push(E::Button {
                id: format!("mail:open:{}", message.id),
                text: format!(
                    "{}{} — {}",
                    if message.unread() { "● " } else { "" },
                    message.sender,
                    message.subject
                ),
                action: act(&format!("mail:open:{}", message.id)),
            });
        }
        if let Some(message) = self
            .selected
            .as_ref()
            .and_then(|id| self.messages.iter().find(|m| &m.id == id))
        {
            page.elements.push(E::Text {
                id: "mail-body".into(),
                text: message.body.clone(),
            });
            for (id, label) in [("mail:reply", "Reply"), ("mail:archive", "Archive")] {
                page.elements.push(E::Button {
                    id: id.into(),
                    text: label.into(),
                    action: act(id),
                });
            }
        }
        if let Some(compose) = &self.compose {
            for (id, label, value) in [
                ("mail-to", "To", &compose.to),
                ("mail-subject", "Subject", &compose.subject),
                ("mail-body-draft", "Message", &compose.body),
            ] {
                page.elements.push(E::Input {
                    id: id.into(),
                    label: label.into(),
                    value: value.clone(),
                    placeholder: String::new(),
                });
            }
            for (id, label) in [("mail:send", "Send"), ("mail:cancel", "Cancel")] {
                page.elements.push(E::Button {
                    id: id.into(),
                    text: label.into(),
                    action: act(id),
                });
            }
        }
    }
    pub fn render(&self, p: &mut Painter, env: &crate::AppEnv<'_>) {
        let (theme, width, height) = (env.theme, env.width, env.height);
        let l = look(theme);
        p.scene.background = l.surface;
        let top = header(p, theme, &l, width, &self.title(theme));
        let sidebar = if theme.mobile() || width < 560 {
            0
        } else {
            168
        };
        if sidebar > 0 {
            p.box_(Rect::new(0, top, sidebar, height), l.chrome, 0);
            p.vline(sidebar as i32 - 1, top, height, LINE);
            for (index, folder) in FOLDERS.iter().enumerate() {
                let r = Rect::new(8, top + 10 + index as i32 * 30, sidebar - 16, 28);
                let on = self.folder == *folder;
                p.button(
                    r,
                    if on { l.selection } else { Color::TRANSPARENT },
                    l.radius,
                    &format!("mail:folder:{folder}"),
                    folder,
                );
                p.left(
                    r.x + 12,
                    r.y + 5,
                    r.width.saturating_sub(20),
                    &title_case(folder),
                    13,
                    if on { l.accent } else { INK },
                );
            }
            action(
                p,
                &l,
                Rect::new(8, top + 130, sidebar - 16, 30),
                "Compose",
                "mail:compose",
                true,
            );
            action(
                p,
                &l,
                Rect::new(8, top + 168, sidebar - 16, 28),
                "Reload",
                "mail:reload",
                false,
            );
        } else {
            action(
                p,
                &l,
                Rect::new(width as i32 - 104, top - 34, 94, 26),
                "Compose",
                "mail:compose",
                true,
            );
        }
        let x = sidebar as i32;
        let list_w = if width.saturating_sub(sidebar) > 620 {
            320
        } else {
            width.saturating_sub(sidebar)
        };
        if let Some(text) = self.status.notice() {
            notice(p, width.saturating_sub(sidebar), top + 20, text);
        }
        if self.messages.is_empty() && self.status.notice().is_none() {
            notice(p, width.saturating_sub(sidebar), top + 30, "No messages");
        }
        let mut y = top + 6;
        for message in &self.messages {
            if y as u32 + l.row + 14 > height {
                break;
            }
            let r = Rect::new(x + 4, y, list_w.saturating_sub(8), l.row + 14);
            let on = self.selected.as_deref() == Some(message.id.as_str());
            p.button(
                r,
                if on { l.selection } else { Color::TRANSPARENT },
                l.radius,
                &format!("mail:open:{}", message.id),
                &message.subject,
            );
            if message.unread() {
                p.circle(r.x + 10, r.y + 14, 3, l.accent);
            }
            p.label(
                r.x + 20,
                r.y + 5,
                r.width.saturating_sub(30),
                &message.sender,
                13,
                INK,
                message.unread(),
                Align::Left,
            );
            p.left(
                r.x + 20,
                r.y + 23,
                r.width.saturating_sub(30),
                &message.subject,
                12,
                MUTED,
            );
            p.hline(r.x, r.y + r.height as i32, r.width, LINE);
            y += r.height as i32 + 1;
        }
        // Reading pane, when the window is wide enough for one.
        if list_w < width.saturating_sub(sidebar) {
            let pane = x + list_w as i32;
            p.vline(pane, top, height, LINE);
            match self
                .selected
                .as_ref()
                .and_then(|id| self.messages.iter().find(|m| &m.id == id))
            {
                Some(message) => {
                    p.strong(
                        pane + 16,
                        top + 12,
                        width.saturating_sub(pane as u32 + 32),
                        &message.subject,
                        16,
                        INK,
                    );
                    p.left(
                        pane + 16,
                        top + 36,
                        width.saturating_sub(pane as u32 + 32),
                        &format!("From {} to {}", message.sender, message.to.join(", ")),
                        12,
                        MUTED,
                    );
                    action(
                        p,
                        &l,
                        Rect::new(pane + 16, top + 58, 76, 26),
                        "Reply",
                        "mail:reply",
                        false,
                    );
                    action(
                        p,
                        &l,
                        Rect::new(pane + 100, top + 58, 84, 26),
                        "Archive",
                        "mail:archive",
                        false,
                    );
                    p.paragraph(
                        pane + 16,
                        top + 96,
                        width.saturating_sub(pane as u32 + 32),
                        &message.body,
                        13,
                        INK,
                    );
                }
                None => notice(
                    p,
                    width.saturating_sub(pane as u32),
                    top + 40,
                    "Select a message",
                ),
            }
        }
        if let Some(compose) = &self.compose {
            self.composer(p, &l, width, height, compose);
        }
    }
    fn composer(
        &self,
        p: &mut Painter,
        l: &super::look::Look,
        width: u32,
        height: u32,
        compose: &Compose,
    ) {
        let h = 214.min(height.saturating_sub(16));
        let r = Rect::new(
            12,
            height as i32 - h as i32 - 8,
            width.saturating_sub(24),
            h,
        );
        p.drop_shadow(r, l.radius, 18, 60, 6);
        p.border(r, l.surface, l.radius, LINE);
        p.strong(r.x + 14, r.y + 10, 200, "New message", 14, INK);
        for (index, (field, label, value)) in [
            ("to", "To", &compose.to),
            ("subject", "Subject", &compose.subject),
            ("body", "Message", &compose.body),
        ]
        .iter()
        .enumerate()
        {
            let tall = *field == "body";
            let fr = Rect::new(
                r.x + 14,
                r.y + 36 + index as i32 * 34,
                r.width.saturating_sub(28),
                if tall { 76 } else { 28 },
            );
            let on = compose.field == *field;
            p.border(fr, Color::WHITE, 5, if on { l.accent } else { LINE });
            p.region(fr, &format!("mail:field:{field}"), label);
            p.left(
                fr.x + 8,
                fr.y + 6,
                fr.width.saturating_sub(16),
                if value.is_empty() { label } else { value },
                13,
                if value.is_empty() { FAINT } else { INK },
            );
        }
        action(
            p,
            l,
            Rect::new(r.x + 14, r.y + h as i32 - 38, 84, 28),
            "Send",
            "mail:send",
            true,
        );
        action(
            p,
            l,
            Rect::new(r.x + 106, r.y + h as i32 - 38, 84, 28),
            "Cancel",
            "mail:cancel",
            false,
        );
    }
}
fn title_case(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app() -> Mail {
        let (mut app, _) = Mail::launch("http://mail.internal/", 1, 0);
        app.http(
            1,
            "messages",
            200,
            r#"[{"id":"mail-1","sender":"carol","to":["alice"],"subject":"Launch","body":"Ship it","time":0,
                "mailboxes":{"alice":{"folders":["inbox"],"labels":[],"read":false}}}]"#,
        )
        .unwrap();
        app
    }
    #[test]
    fn opening_an_unread_message_marks_it_read_on_the_service() {
        let mut app = app();
        assert!(app.messages[0].unread());
        let effects = app.click(1, "mail:open:mail-1", 0).unwrap();
        let AppEffect::Http {
            method, url, body, ..
        } = &effects[0]
        else {
            panic!("expected a request");
        };
        assert_eq!(method, "POST");
        assert_eq!(url, "http://mail.internal/api/messages/mail-1");
        assert_eq!(body, r#"{"read":true}"#);
        // Opening an already-read message makes no request at all.
        app.messages[0].mailboxes.get_mut("alice").unwrap().read = true;
        assert!(app.click(1, "mail:open:mail-1", 0).unwrap().is_empty());
    }
    #[test]
    fn sending_needs_a_recipient_and_replies_prefill_from_real_data() {
        let mut app = app();
        app.click(1, "mail:compose", 0).unwrap();
        assert!(app.click(1, "mail:send", 0).is_err());
        app.click(1, "mail:open:mail-1", 0).unwrap();
        app.click(1, "mail:reply", 0).unwrap();
        let compose = app.compose.clone().unwrap();
        assert_eq!(compose.to, "carol");
        assert_eq!(compose.subject, "Re: Launch");
        app.text("On it").unwrap();
        let effects = app.click(1, "mail:send", 0).unwrap();
        let AppEffect::Http { body, tag, .. } = &effects[0] else {
            panic!("expected a request");
        };
        assert_eq!(tag, "send");
        let sent: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(sent["to"], serde_json::json!(["carol"]));
        assert_eq!(sent["body"], "On it");
    }
    #[test]
    fn refusal_and_transport_failure_stay_distinct() {
        let mut app = app();
        app.http(1, "messages", 403, r#"{"error":"mailbox unavailable"}"#)
            .unwrap();
        assert_eq!(app.status, Status::Denied("mailbox unavailable".into()));
        app.offline("messages", "no route to host");
        assert_eq!(app.status, Status::Offline("no route to host".into()));
    }
    #[test]
    fn every_painted_control_is_one_the_model_accepts() {
        let mut base = app();
        base.click(1, "mail:open:mail-1", 0).unwrap();
        base.click(1, "mail:compose", 0).unwrap();
        let mut scene = Painter::themed(DesktopTheme::Macos, 980, 660, 0);
        base.render(
            &mut scene,
            &crate::AppEnv {
                theme: DesktopTheme::Macos,
                width: 980,
                height: 660,
                clock_us: 0,
                settings: &crate::SystemSettings::DEFAULT,
                clipboard: None,
                share_to: None,
            },
        );
        for target in scene
            .scene
            .nodes
            .iter()
            .filter_map(|n| n.interaction.clone())
        {
            let mut app = base.clone();
            if target == "mail:send" {
                app.compose.as_mut().unwrap().to = "bob".into();
            }
            assert!(
                app.click(1, &target, 0).is_ok(),
                "unhandled control {target}"
            );
        }
    }
    #[test]
    fn opened_on_a_shared_file_it_starts_a_message_about_it() {
        let (app, effects) = Mail::launch("http://mail.example/#/home/alice/report.txt", 1, 0);
        assert_eq!(app.base, "http://mail.example/");
        assert!(!effects.is_empty(), "the inbox still loads");
        let compose = app.compose.expect("a message is being written");
        assert_eq!(compose.subject, "report.txt");
        assert_eq!(compose.body, "/home/alice/report.txt");
        assert!(
            compose.to.is_empty(),
            "the actor still chooses the recipient"
        );
        assert_eq!(compose.field, "to");
        let (plain, _) = Mail::launch("http://mail.example/", 1, 0);
        assert!(plain.compose.is_none());
    }
}
