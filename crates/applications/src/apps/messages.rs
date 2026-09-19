//! Messages over the `messages` service: texting between handles. Conversations are the
//! people in them, a text goes over iMessage or SMS, receipts and tapbacks are the
//! service's records, and sending posts a real message the other phone then fetches.
use super::look::{action, look, notice, screen, screen_from_end, FAINT, INK, LINE, MUTED};
use super::{push_bounded, Status};
use crate::desktop_scene::{shared::Align, DesktopTheme, Painter};
use crate::AppEffect;
use cw_scene::{Color, Rect};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// iMessage blue, SMS green, and the grey an incoming bubble wears.
pub const IMESSAGE: Color = Color::rgb(11, 132, 254);
pub const SMS: Color = Color::rgb(52, 199, 89);
pub const INCOMING: Color = Color::rgb(233, 233, 235);
/// The tapbacks a bubble can be given, in the order the picker shows them.
pub const TAPBACKS: [&str; 6] = [
    "loved",
    "liked",
    "disliked",
    "laughed",
    "emphasized",
    "questioned",
];

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub preview: String,
    #[serde(default)]
    pub unread: u64,
    #[serde(default)]
    pub service: String,
    #[serde(default)]
    pub time: u64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Text {
    pub id: String,
    #[serde(default)]
    pub from: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub time: u64,
    #[serde(default)]
    pub service: String,
    #[serde(default)]
    pub delivered: Vec<String>,
    #[serde(default)]
    pub read: BTreeMap<String, u64>,
    #[serde(default)]
    pub tapbacks: BTreeMap<String, Vec<String>>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Thread {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub title: String,
    /// The handle this device sends from.
    #[serde(default)]
    pub me: String,
    #[serde(default)]
    pub participants: Vec<String>,
    /// Handle to display name.
    #[serde(default)]
    pub people: BTreeMap<String, String>,
    #[serde(default)]
    pub service: String,
    #[serde(default)]
    pub messages: Vec<Text>,
}
impl Thread {
    pub fn group(&self) -> bool {
        self.participants.len() > 2
    }
    pub fn name_of(&self, handle: &str) -> String {
        self.people
            .get(handle)
            .cloned()
            .unwrap_or_else(|| handle.to_owned())
    }
    /// What sits under the last text this device sent: delivered, read, or sent as SMS.
    pub fn status_line(&self) -> Option<(usize, String)> {
        let (index, last) = self
            .messages
            .iter()
            .enumerate()
            .rev()
            .find(|(_, m)| m.from == self.me)?;
        let line = if last.service == "sms" {
            "Sent as Text Message".to_owned()
        } else if let Some((who, _)) = last.read.iter().max_by_key(|(_, at)| **at) {
            if self.group() {
                format!("Read by {}", self.name_of(who))
            } else {
                "Read".to_owned()
            }
        } else if last.delivered.is_empty() {
            "Sending…".to_owned()
        } else {
            "Delivered".to_owned()
        };
        Some((index, line))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Messages {
    pub base: String,
    /// Every conversation this device is in, newest first, as the service lists them.
    pub conversations: Vec<Summary>,
    pub open: Option<String>,
    pub thread: Thread,
    pub draft: String,
    pub status: Status,
    /// The composer has been tapped, so on a phone it has the keyboard. A desktop's
    /// composer has the focus whenever a conversation is open.
    #[serde(default)]
    pub composing: bool,
    /// A phone is showing its conversation list, the root screen of iOS Messages and
    /// Google Messages; a conversation is a tap away and Back returns here. A desktop
    /// always has the list in its sidebar and ignores it.
    #[serde(default)]
    pub listing: bool,
}
impl Messages {
    pub const KIND: &'static str = "messages";
    pub fn launch(argument: &str, window: u64, _clock_us: u64) -> (Self, Vec<AppEffect>) {
        // Opened *on* something — a shared file or page — it starts as the draft.
        let (base, shared) = super::launch_target(argument);
        let mut draft = String::new();
        push_bounded(&mut draft, shared, 4096);
        let app = Self {
            base: if base.is_empty() {
                "http://messages.internal/".into()
            } else {
                base.to_owned()
            },
            conversations: vec![],
            open: None,
            thread: Thread::default(),
            draft,
            status: Status::Loading,
            composing: false,
            // Opened on something to share, a phone still starts on the list: the
            // actor chooses who gets it.
            listing: true,
        };
        let effects = vec![app.request(
            window,
            "conversations",
            "GET",
            "/api/conversations",
            String::new(),
        )];
        (app, effects)
    }
    pub fn kind(&self) -> &'static str {
        Self::KIND
    }
    pub fn title(&self, theme: DesktopTheme) -> String {
        match theme {
            DesktopTheme::Windows => "Phone Link",
            _ => "Messages",
        }
        .into()
    }
    pub fn document(&self) -> String {
        String::new()
    }
    pub fn caption(&self) -> String {
        self.thread.title.clone()
    }
    pub fn modified(&self) -> bool {
        !self.draft.is_empty()
    }
    fn request(
        &self,
        window: u64,
        tag: &str,
        method: &str,
        suffix: &str,
        body: String,
    ) -> AppEffect {
        AppEffect::Http {
            window,
            tag: tag.into(),
            method: method.into(),
            url: format!("{}{suffix}", self.base.trim_end_matches('/')),
            body,
        }
    }
    /// Fetch a conversation. Opening it by hand reads it, which is the receipt the other
    /// side sees; a refresh behind the list does not.
    fn fetch_thread(&self, window: u64, id: &str, read: bool) -> AppEffect {
        if read {
            self.request(
                window,
                "thread",
                "POST",
                &format!("/api/conversations/{id}/read"),
                "{}".into(),
            )
        } else {
            self.request(
                window,
                "thread",
                "GET",
                &format!("/api/conversations/{id}"),
                String::new(),
            )
        }
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
            "conversations" => {
                self.conversations = serde_json::from_str(body).unwrap_or_default();
                // Show the newest conversation; it is not read until it is opened.
                match self.conversations.first().map(|c| c.id.clone()) {
                    Some(first) if self.open.is_none() => {
                        self.open = Some(first.clone());
                        self.status = Status::Loading;
                        Ok(vec![self.fetch_thread(window, &first, false)])
                    }
                    _ => Ok(vec![]),
                }
            }
            "thread" => {
                self.thread = serde_json::from_str(body).unwrap_or_default();
                if let Some(row) = self
                    .conversations
                    .iter_mut()
                    .find(|c| Some(&c.id) == self.open.as_ref())
                {
                    row.unread = self
                        .thread
                        .messages
                        .iter()
                        .filter(|m| {
                            m.from != self.thread.me && !m.read.contains_key(&self.thread.me)
                        })
                        .count() as u64;
                    if let Some(last) = self.thread.messages.last() {
                        row.preview = last.text.clone();
                        row.time = last.time;
                    }
                }
                Ok(vec![])
            }
            "send" => {
                self.draft.clear();
                let Some(id) = self.open.clone() else {
                    return Ok(vec![]);
                };
                self.status = Status::Loading;
                Ok(vec![self.fetch_thread(window, &id, true)])
            }
            other => Err(format!("unexpected messages reply {other}")),
        }
    }
    pub fn text(&mut self, text: &str) -> Result<(), String> {
        if self.open.is_none() {
            return Err("no conversation is open to write in".into());
        }
        push_bounded(&mut self.draft, text, 4096);
        Ok(())
    }
    pub fn key(&mut self, window: u64, key: &str, clock_us: u64) -> Result<Vec<AppEffect>, String> {
        match key {
            "Backspace" => {
                self.draft.pop();
                Ok(vec![])
            }
            "Enter" => self.click(window, "messages:send", clock_us),
            other => Err(format!("unsupported messages key {other}")),
        }
    }
    pub fn click(
        &mut self,
        window: u64,
        target: &str,
        _clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        let command = target
            .strip_prefix("messages:")
            .ok_or("interaction does not belong to messages")?;
        match command {
            "reload" => {
                self.status = Status::Loading;
                Ok(vec![self.request(
                    window,
                    "conversations",
                    "GET",
                    "/api/conversations",
                    String::new(),
                )])
            }
            "compose" => {
                if self.open.is_none() {
                    return Err("no conversation is open to write in".into());
                }
                self.composing = true;
                self.listing = false;
                Ok(vec![])
            }
            // A phone's back: from a conversation to the conversation list.
            "back" => {
                if self.listing {
                    return Err("the conversation list has nothing behind it".into());
                }
                self.listing = true;
                self.composing = false;
                Ok(vec![])
            }
            "send" => {
                let id = self.open.clone().ok_or("no conversation is open")?;
                if self.draft.trim().is_empty() {
                    return Err("a message needs text".into());
                }
                self.status = Status::Loading;
                Ok(vec![self.request(
                    window,
                    "send",
                    "POST",
                    &format!("/api/conversations/{id}/messages"),
                    serde_json::json!({ "text": self.draft }).to_string(),
                )])
            }
            rest => {
                if let Some(id) = rest.strip_prefix("open:") {
                    if !self.conversations.iter().any(|c| c.id == id) {
                        return Err("conversation not found".into());
                    }
                    self.open = Some(id.to_owned());
                    self.composing = false;
                    self.listing = false;
                    self.status = Status::Loading;
                    return Ok(vec![self.fetch_thread(window, id, true)]);
                }
                if let Some(rest) = rest.strip_prefix("tapback:") {
                    let (message, tapback) = rest.split_once(':').ok_or("invalid tapback")?;
                    if !TAPBACKS.contains(&tapback) {
                        return Err(format!("unknown tapback {tapback}"));
                    }
                    let id = self.open.clone().ok_or("no conversation is open")?;
                    self.status = Status::Loading;
                    return Ok(vec![self.request(
                        window,
                        "send",
                        "POST",
                        &format!("/api/conversations/{id}/messages/{message}/tapbacks"),
                        serde_json::json!({ "tapback": tapback }).to_string(),
                    )]);
                }
                Err(format!("unknown messages command {command}"))
            }
        }
    }
    /// Where Back goes inside Messages on a phone: a conversation returns to the list.
    pub fn phone_back(&self) -> Option<&'static str> {
        (!self.listing).then_some("messages:back")
    }
    /// The navigation bar's leading control on a phone, as (kind, target, label).
    pub fn phone_nav(&self) -> Option<(&'static str, String, String)> {
        (!self.listing).then(|| ("back", "messages:back".into(), String::new()))
    }
    fn initials(name: &str) -> String {
        let letters: String = name
            .split_whitespace()
            .filter_map(|w| w.chars().next())
            .take(2)
            .collect();
        if letters.is_empty() {
            "?".into()
        } else {
            letters.to_uppercase()
        }
    }
    /// One row of the conversation list, on a phone's root screen or a desktop's sidebar.
    fn conversation_row(
        &self,
        p: &mut Painter,
        r: Rect,
        c: &Summary,
        selected: bool,
        accent: Color,
    ) {
        p.button(
            r,
            if selected {
                Color(0, 0, 0, 14)
            } else {
                Color::TRANSPARENT
            },
            8,
            &format!("messages:open:{}", c.id),
            &c.title,
        );
        let row = r.height as i32;
        if c.unread > 0 {
            p.circle(r.x + 12, r.y + row / 2, 5, IMESSAGE);
        }
        p.circle(r.x + 42, r.y + row / 2, 20, Color(0, 0, 0, 22));
        p.center(
            r.x + 22,
            r.y + row / 2 - 9,
            40,
            &Self::initials(&c.title),
            14,
            MUTED,
        );
        let text_x = r.x + 72;
        let text_w = r.width.saturating_sub(140);
        p.label(
            text_x,
            r.y + 12,
            text_w,
            &c.title,
            15,
            INK,
            c.unread > 0,
            Align::Left,
        );
        p.right(
            r.x + r.width as i32 - 72,
            r.y + 14,
            60,
            if c.unread > 0 { "New" } else { "" },
            11,
            FAINT,
        );
        p.left(
            text_x,
            r.y + 34,
            r.width.saturating_sub(96),
            if c.preview.is_empty() {
                "No messages yet"
            } else {
                &c.preview
            },
            13,
            MUTED,
        );
        let _ = accent;
    }
    /// A phone's root screen: every conversation, newest first, with its latest text
    /// and a blue dot where something is unread. A tap opens it.
    fn conversations(&self, p: &mut Painter, theme: DesktopTheme, width: u32, height: u32) {
        let l = look(theme);
        let screen = screen(p, theme, &l, width, height as i32, &self.title(theme));
        let top = screen.top;
        if let Some(text) = self.status.notice() {
            notice(p, width, top + 16, text);
        }
        if self.conversations.is_empty() && self.status.notice().is_none() {
            notice(p, width, top + 30, "No conversations");
        }
        let row = 72;
        for (index, c) in self.conversations.iter().enumerate() {
            let r = Rect::new(0, top + 4 + index as i32 * row, width, row as u32);
            self.conversation_row(p, r, c, false, l.accent);
            p.symbol(
                "chevron-right",
                width as i32 - 26,
                r.y + row / 2 - 8,
                16,
                FAINT,
            );
            p.hline(r.x + 72, r.bottom() - 1, width.saturating_sub(72), LINE);
        }
        screen.end(p);
    }
    pub fn page(&self, page: &mut cw_protocol::Page) {
        use cw_protocol::PageElement as E;
        let act = |url: &str| cw_protocol::PageAction {
            method: "APP".into(),
            url: url.into(),
            fields: Default::default(),
        };
        let button = |id: String, text: String| E::Button {
            action: act(&id),
            id,
            text,
            style: None,
        };
        page.elements.push(E::Heading {
            id: "messages-title".into(),
            text: self.thread.title.clone(),
            level: 2,
        });
        if let Some(text) = self.status.notice() {
            page.elements.push(E::Text {
                id: "messages-status".into(),
                text: text.into(),
            });
        }
        for c in &self.conversations {
            page.elements.push(button(
                format!("messages:open:{}", c.id),
                if c.unread > 0 {
                    format!("{} ({} unread)", c.title, c.unread)
                } else {
                    c.title.clone()
                },
            ));
        }
        for m in &self.thread.messages {
            let who = if m.from == self.thread.me {
                "Me".to_owned()
            } else {
                self.thread.name_of(&m.from)
            };
            page.elements.push(E::Text {
                id: format!("messages-text-{}", m.id),
                text: format!("{who}: {} [{}]", m.text, m.service),
            });
        }
        if let Some((_, line)) = self.thread.status_line() {
            page.elements.push(E::Text {
                id: "messages-receipt".into(),
                text: line,
            });
        }
        page.elements.push(E::Input {
            id: "messages-draft".into(),
            label: if self.thread.service == "sms" {
                "Text Message"
            } else {
                "iMessage"
            }
            .into(),
            value: self.draft.clone(),
            placeholder: "Write a message".into(),
        });
        for (id, label) in [("messages:send", "Send"), ("messages:reload", "Reload")] {
            page.elements.push(button(id.into(), label.into()));
        }
    }
    /// The transcript as bubbles: outgoing on the right in the service's colour,
    /// incoming on the left in grey, the sender named in a group, tapbacks on the
    /// bubble and the receipt under the last outgoing text.
    fn bubbles(&self, p: &mut Painter, x: i32, top: i32, pane: u32, radius: u32) -> i32 {
        let mut y = top + 8;
        let group = self.thread.group();
        let max_w = (pane * 3 / 4).saturating_sub(24).max(80);
        let receipt = self.thread.status_line();
        for (index, m) in self.thread.messages.iter().enumerate() {
            let mine = m.from == self.thread.me;
            let fill = match (mine, m.service.as_str()) {
                (true, "sms") => SMS,
                (true, _) => IMESSAGE,
                (false, _) => INCOMING,
            };
            let ink = if mine { Color::WHITE } else { INK };
            if !mine && group {
                p.left(x + 24, y, max_w, &self.thread.name_of(&m.from), 11, MUTED);
                y += 16;
            }
            let measured = p.measure(&m.text, 14, false);
            let text_w = measured.min(max_w).max(20);
            let lines = measured.div_ceil(max_w.max(1)).max(1);
            let h = 12 + lines * 20;
            let bubble_w = text_w + 24;
            let bx = if mine {
                x + pane as i32 - 16 - bubble_w as i32
            } else {
                x + 16
            };
            let bubble = Rect::new(bx, y, bubble_w, h);
            p.box_(bubble, fill, 16);
            p.paragraph(bx + 12, y + 6, text_w, &m.text, 14, ink);
            // A tap on a bubble gives it the first tapback; the picker itself is the
            // set of targets under it, so every one is a real route.
            p.region(
                bubble,
                &format!("messages:tapback:{}:loved", m.id),
                "Tapback",
            );
            y += h as i32;
            if !m.tapbacks.is_empty() {
                let chips: Vec<String> = m
                    .tapbacks
                    .iter()
                    .map(|(name, who)| format!("{name} {}", who.len()))
                    .collect();
                let line = chips.join("  ");
                let w = p.measure(&line, 11, false) + 16;
                let cx = if mine {
                    x + pane as i32 - 16 - w as i32
                } else {
                    x + 16
                };
                let chip = Rect::new(cx, y - 6, w, 18);
                p.border(chip, Color::WHITE, 9, LINE);
                p.center(chip.x, chip.y + 3, chip.width, &line, 11, MUTED);
                y += 14;
            }
            if let Some((at, ref line)) = receipt {
                if at == index {
                    p.right(x, y + 2, pane.saturating_sub(16), line, 11, MUTED);
                    y += 16;
                }
            }
            y += 6;
        }
        let _ = radius;
        y
    }
    pub fn render(&self, p: &mut Painter, env: &crate::AppEnv<'_>) {
        let (theme, width, height) = (env.theme, env.width, env.height);
        let l = look(theme);
        p.scene.background = l.surface;
        let composer_h: i32 = 46;
        // A phone starts on its conversation list; nothing is open until one is tapped.
        if theme.mobile() && (self.listing || self.open.is_none()) {
            self.conversations(p, theme, width, height);
            return;
        }
        // A conversation opens on its newest text; older ones are a scroll away. A
        // phone titles it with the conversation; a desktop with the application.
        let heading = if theme.mobile() {
            self.thread.title.clone()
        } else {
            self.title(theme)
        };
        let screen = screen_from_end(p, theme, &l, width, height as i32 - composer_h, &heading);
        let top = screen.top;
        let sidebar = if theme.mobile() || width < 560 {
            0
        } else {
            260
        };
        if sidebar > 0 {
            p.box_(Rect::new(0, top, sidebar, height), l.chrome, 0);
            p.vline(sidebar as i32 - 1, top, height, LINE);
            let list = screen.column(
                p,
                "conversations",
                Rect::new(0, top, sidebar, (height as i32 - top - 48).max(1) as u32),
            );
            for (index, c) in self.conversations.iter().enumerate() {
                let r = Rect::new(6, list.top + 4 + index as i32 * 64, sidebar - 12, 62);
                let on = self.open.as_deref() == Some(c.id.as_str());
                self.conversation_row(p, r, c, on, l.accent);
            }
            list.end(p);
            action(
                p,
                &l,
                Rect::new(8, height as i32 - 40, sidebar - 16, 28),
                "Reload",
                "messages:reload",
                false,
            );
        }
        let x = sidebar as i32;
        let pane = width.saturating_sub(sidebar);
        if theme.mobile() || sidebar > 0 {
            // The conversation's header: who it is with and which service carries it.
            let service = if self.thread.service == "sms" {
                "SMS"
            } else {
                "iMessage"
            };
            let label = if self.thread.group() {
                format!(
                    "{} · {} people · {service}",
                    self.thread.title,
                    self.thread.participants.len()
                )
            } else {
                format!("{} · {service}", self.thread.title)
            };
            p.center(x, top + 6, pane, &label, 11, MUTED);
        }
        if let Some(text) = self.status.notice() {
            notice(p, pane, top + 24, text);
        }
        if self.thread.messages.is_empty() && self.status.notice().is_none() {
            notice(p, pane, top + 40, "Say something");
        }
        let messages = screen.column_from_end(
            p,
            "messages",
            Rect::new(
                x,
                top + 22,
                pane,
                (height as i32 - composer_h - top - 22).max(1) as u32,
            ),
        );
        self.bubbles(p, x, messages.top, pane, l.radius);
        messages.end(p);
        screen.end(p);
        let bar = Rect::new(
            x + 10,
            height as i32 - composer_h + 6,
            pane.saturating_sub(96),
            32,
        );
        let focused = self.open.is_some() && (self.composing || !theme.mobile());
        p.border(
            bar,
            Color::WHITE,
            16,
            if focused && theme.mobile() {
                l.accent
            } else {
                LINE
            },
        );
        p.region(bar, "messages:compose", "Message");
        let placeholder = if self.thread.service == "sms" {
            "Text Message"
        } else {
            "iMessage"
        };
        p.left(
            bar.x + 12,
            bar.y + 8,
            bar.width.saturating_sub(24),
            if self.draft.is_empty() {
                placeholder
            } else {
                &self.draft
            },
            13,
            if self.draft.is_empty() { FAINT } else { INK },
        );
        let send = Rect::new(x + pane as i32 - 80, height as i32 - composer_h + 6, 70, 32);
        p.button(
            send,
            if self.thread.service == "sms" {
                SMS
            } else {
                IMESSAGE
            },
            16,
            "messages:send",
            "Send",
        );
        p.label(
            send.x,
            send.y + 7,
            send.width,
            "Send",
            13,
            Color::WHITE,
            true,
            Align::Center,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const AB: &str = "+14155550100|+14155550101";
    fn app() -> Messages {
        let (mut app, _) = Messages::launch("http://messages.internal/", 1, 0);
        let more = app
            .http(
                1,
                "conversations",
                200,
                &format!(r#"[{{"id":"{AB}","title":"Bob Martinez","preview":"lunch?","unread":1,"service":"imessage","time":30}}]"#),
            )
            .unwrap();
        assert_eq!(more.len(), 1);
        app.http(
            1,
            "thread",
            200,
            &format!(r#"{{"id":"{AB}","title":"Bob Martinez","me":"+14155550100","participants":["+14155550100","+14155550101"],"people":{{"+14155550100":"Alice Chen","+14155550101":"Bob Martinez"}},"service":"imessage","messages":[{{"id":"sms-1","from":"+14155550101","text":"lunch?","time":30,"service":"imessage","delivered":["+14155550100"]}},{{"id":"sms-2","from":"+14155550100","text":"yes","time":31,"service":"imessage","delivered":["+14155550101"],"read":{{"+14155550101":32}}}}]}}"#),
        )
        .unwrap();
        app
    }
    #[test]
    fn the_newest_conversation_opens_and_its_texts_are_service_records() {
        let app = app();
        assert_eq!(app.open.as_deref(), Some(AB));
        assert_eq!(app.thread.messages[0].from, "+14155550101");
        assert_eq!(app.thread.name_of("+14155550101"), "Bob Martinez");
        assert_eq!(app.thread.status_line().unwrap(), (1, "Read".to_owned()));
        // Showing a thread behind the list is not reading it.
        assert_eq!(app.conversations[0].unread, 1);
    }
    #[test]
    fn opening_reads_and_sending_posts_then_refetches() {
        let mut app = app();
        let effects = app.click(1, &format!("messages:open:{AB}"), 0).unwrap();
        let AppEffect::Http { url, method, .. } = &effects[0] else {
            panic!("expected a request");
        };
        assert_eq!(method, "POST");
        assert_eq!(
            url,
            &format!("http://messages.internal/api/conversations/{AB}/read")
        );
        assert!(app.click(1, "messages:send", 0).is_err());
        app.text("On my way").unwrap();
        let effects = app.click(1, "messages:send", 0).unwrap();
        let AppEffect::Http { url, body, .. } = &effects[0] else {
            panic!("expected a request");
        };
        assert_eq!(
            url,
            &format!("http://messages.internal/api/conversations/{AB}/messages")
        );
        assert_eq!(body, r#"{"text":"On my way"}"#);
        let more = app.http(1, "send", 200, "{}").unwrap();
        assert!(app.draft.is_empty());
        assert_eq!(more.len(), 1);
        let effects = app.click(1, "messages:tapback:sms-1:loved", 0).unwrap();
        let AppEffect::Http { url, body, .. } = &effects[0] else {
            panic!("expected a request");
        };
        assert!(url.ends_with("/messages/sms-1/tapbacks"));
        assert_eq!(body, r#"{"tapback":"loved"}"#);
        assert!(app.click(1, "messages:tapback:sms-1:wow", 0).is_err());
    }
    #[test]
    fn every_painted_control_is_one_the_model_accepts() {
        let base = app();
        for (theme, size) in [
            (DesktopTheme::Ubuntu, (900, 600)),
            (DesktopTheme::Macos, (1100, 700)),
            (DesktopTheme::Ios, (390, 844)),
        ] {
            let mut scene = Painter::themed(theme, size.0, size.1, 0);
            let mut shown = base.clone();
            if theme.mobile() {
                shown.listing = false;
            }
            shown.render(
                &mut scene,
                &crate::AppEnv {
                    theme,
                    width: size.0,
                    height: size.1,
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
                let mut app = shown.clone();
                if target == "messages:send" {
                    app.text("x").unwrap();
                }
                assert!(
                    app.click(1, &target, 0).is_ok(),
                    "{theme:?}: unhandled control {target}"
                );
            }
        }
    }
    #[test]
    fn opened_on_a_shared_page_it_starts_as_the_draft() {
        let (app, _) = Messages::launch("http://messages.example/#http://intranet.internal/", 1, 0);
        assert_eq!(app.base, "http://messages.example/");
        assert_eq!(app.draft, "http://intranet.internal/");
        let (plain, _) = Messages::launch("http://messages.example/", 1, 0);
        assert_eq!(plain.base, "http://messages.example/");
        assert!(plain.draft.is_empty());
        let (bare, _) = Messages::launch("", 1, 0);
        assert_eq!(bare.base, "http://messages.internal/");
    }
}
