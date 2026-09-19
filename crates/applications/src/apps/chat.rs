//! Messages over the `chat` service: real channels, real messages, real sending.
use super::look::{action, look, notice, screen, screen_from_end, FAINT, INK, LINE, MUTED};
use super::{push_bounded, Status};
use crate::desktop_scene::{shared::Align, DesktopTheme, Painter};
use crate::AppEffect;
use cw_scene::{Color, Rect};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Post {
    pub id: String,
    pub author: String,
    pub text: String,
    #[serde(default)]
    pub time: u64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Channel {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub members: Vec<String>,
    #[serde(default)]
    pub messages: Vec<Post>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chat {
    pub base: String,
    /// Channel id to display title, as the service lists them.
    pub channels: BTreeMap<String, String>,
    pub open: Option<String>,
    pub channel: Channel,
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
impl Chat {
    pub const KIND: &'static str = "chat";
    pub fn launch(argument: &str, window: u64, _clock_us: u64) -> (Self, Vec<AppEffect>) {
        // Opened *on* something — a shared file or page — it starts as the draft.
        let (base, shared) = super::launch_target(argument);
        let mut draft = String::new();
        push_bounded(&mut draft, shared, 4096);
        let app = Self {
            base: if base.is_empty() {
                "http://chat.internal/".into()
            } else {
                base.to_owned()
            },
            channels: BTreeMap::new(),
            open: None,
            channel: Channel::default(),
            draft,
            status: Status::Loading,
            composing: false,
            // Opened on something to share, a phone still starts on the list: the
            // actor chooses who gets it.
            listing: true,
        };
        let effects = vec![app.request(window, "channels", "GET", "/api/channels", String::new())];
        (app, effects)
    }
    pub fn kind(&self) -> &'static str {
        Self::KIND
    }
    pub fn title(&self, theme: DesktopTheme) -> String {
        match theme {
            DesktopTheme::Windows => "Teams",
            DesktopTheme::Ubuntu => "Chat",
            _ => "Messages",
        }
        .into()
    }
    pub fn document(&self) -> String {
        String::new()
    }
    pub fn caption(&self) -> String {
        self.channel.title.clone()
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
    fn open_channel(&self, window: u64, id: &str) -> AppEffect {
        self.request(
            window,
            "channel",
            "GET",
            &format!("/api/channels/{id}"),
            String::new(),
        )
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
            "channels" => {
                self.channels = serde_json::from_str(body).unwrap_or_default();
                // Open the first channel the actor is really a member of.
                match self.channels.keys().next().cloned() {
                    Some(first) if self.open.is_none() => {
                        self.open = Some(first.clone());
                        self.status = Status::Loading;
                        Ok(vec![self.open_channel(window, &first)])
                    }
                    _ => Ok(vec![]),
                }
            }
            "channel" => {
                self.channel = serde_json::from_str(body).unwrap_or_default();
                Ok(vec![])
            }
            "send" => {
                self.draft.clear();
                let Some(id) = self.open.clone() else {
                    return Ok(vec![]);
                };
                self.status = Status::Loading;
                Ok(vec![self.open_channel(window, &id)])
            }
            other => Err(format!("unexpected chat reply {other}")),
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
            "Enter" => self.click(window, "chat:send", clock_us),
            other => Err(format!("unsupported chat key {other}")),
        }
    }
    pub fn click(
        &mut self,
        window: u64,
        target: &str,
        _clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        let command = target
            .strip_prefix("chat:")
            .ok_or("interaction does not belong to messages")?;
        match command {
            "reload" => {
                self.status = Status::Loading;
                Ok(vec![self.request(
                    window,
                    "channels",
                    "GET",
                    "/api/channels",
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
                let id = self.open.clone().ok_or("no channel is open")?;
                if self.draft.trim().is_empty() {
                    return Err("a message needs text".into());
                }
                self.status = Status::Loading;
                Ok(vec![self.request(
                    window,
                    "send",
                    "POST",
                    &format!("/api/channels/{id}/messages"),
                    serde_json::json!({ "text": self.draft }).to_string(),
                )])
            }
            rest => {
                if let Some(id) = rest.strip_prefix("channel:") {
                    if !self.channels.contains_key(id) {
                        return Err("channel not found".into());
                    }
                    self.open = Some(id.to_owned());
                    self.composing = false;
                    self.listing = false;
                    self.status = Status::Loading;
                    return Ok(vec![self.open_channel(window, id)]);
                }
                if let Some(rest) = rest.strip_prefix("react:") {
                    let (message, reaction) = rest.split_once(':').ok_or("invalid reaction")?;
                    let id = self.open.clone().ok_or("no channel is open")?;
                    self.status = Status::Loading;
                    return Ok(vec![self.request(
                        window,
                        "send",
                        "POST",
                        &format!("/api/channels/{id}/messages/{message}/reactions"),
                        serde_json::json!({ "reaction": reaction }).to_string(),
                    )]);
                }
                Err(format!("unknown chat command {command}"))
            }
        }
    }
    /// Where Back goes inside Messages on a phone: a conversation returns to the list.
    pub fn phone_back(&self) -> Option<&'static str> {
        (!self.listing).then_some("chat:back")
    }
    /// The navigation bar's leading control on a phone, as (kind, target, label).
    pub fn phone_nav(&self) -> Option<(&'static str, String, String)> {
        (!self.listing).then(|| ("back", "chat:back".into(), String::new()))
    }
    /// A phone's root screen: every conversation, newest service order, the open one's
    /// latest message under its name. A tap opens it.
    fn conversations(&self, p: &mut Painter, theme: DesktopTheme, width: u32, height: u32) {
        let l = look(theme);
        let screen = screen(p, theme, &l, width, height as i32, &self.title(theme));
        let top = screen.top;
        if let Some(text) = self.status.notice() {
            notice(p, width, top + 16, text);
        }
        if self.channels.is_empty() && self.status.notice().is_none() {
            notice(p, width, top + 30, "No conversations");
        }
        let row = 72;
        for (index, (id, title)) in self.channels.iter().enumerate() {
            let r = Rect::new(0, top + 4 + index as i32 * row, width, row as u32);
            p.button(
                r,
                Color::TRANSPARENT,
                0,
                &format!("chat:channel:{id}"),
                title,
            );
            p.circle(r.x + 38, r.y + row / 2, 22, Color(0, 0, 0, 20));
            let initial: String = title.chars().take(1).collect::<String>().to_uppercase();
            p.center(r.x + 16, r.y + row / 2 - 10, 44, &initial, 16, MUTED);
            // Only the conversation this device has fetched has a latest message to show.
            let preview = if self.open.as_deref() == Some(id.as_str()) {
                self.channel
                    .messages
                    .last()
                    .map(|m| format!("{}: {}", m.author, m.text))
            } else {
                None
            };
            p.label(
                r.x + 72,
                r.y + if preview.is_some() { 14 } else { 26 },
                width.saturating_sub(100),
                title,
                15,
                INK,
                true,
                Align::Left,
            );
            if let Some(preview) = preview {
                p.left(
                    r.x + 72,
                    r.y + 38,
                    width.saturating_sub(100),
                    &preview,
                    13,
                    MUTED,
                );
            }
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
        page.elements.push(E::Heading {
            id: "chat-title".into(),
            text: self.channel.title.clone(),
            level: 2,
        });
        if let Some(text) = self.status.notice() {
            page.elements.push(E::Text {
                id: "chat-status".into(),
                text: text.into(),
            });
        }
        for (id, title) in &self.channels {
            page.elements.push(E::Button {
                id: format!("chat:channel:{id}"),
                text: title.clone(),
                action: act(&format!("chat:channel:{id}")),
            });
        }
        for post in &self.channel.messages {
            page.elements.push(E::Text {
                id: format!("chat-post-{}", post.id),
                text: format!("{}: {}", post.author, post.text),
            });
        }
        page.elements.push(E::Input {
            id: "chat-draft".into(),
            label: "Message".into(),
            value: self.draft.clone(),
            placeholder: "Write a message".into(),
        });
        for (id, label) in [("chat:send", "Send"), ("chat:reload", "Reload")] {
            page.elements.push(E::Button {
                id: id.into(),
                text: label.into(),
                action: act(id),
            });
        }
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
        // A conversation opens on its newest message; older ones are a scroll away. A
        // phone titles it with the conversation; a desktop with the application.
        let heading = if theme.mobile() {
            self.channel.title.clone()
        } else {
            self.title(theme)
        };
        let screen = screen_from_end(p, theme, &l, width, height as i32 - composer_h, &heading);
        let top = screen.top;
        let sidebar = if theme.mobile() || width < 520 {
            0
        } else {
            180
        };
        if sidebar > 0 {
            p.box_(Rect::new(0, top, sidebar, height), l.chrome, 0);
            p.vline(sidebar as i32 - 1, top, height, LINE);
            p.left(14, top + 10, sidebar - 24, "Channels", 11, MUTED);
            let list = screen.column(
                p,
                "channels",
                Rect::new(
                    0,
                    top + 28,
                    sidebar,
                    (height as i32 - top - 76).max(1) as u32,
                ),
            );
            for (index, (id, title)) in self.channels.iter().enumerate() {
                let r = Rect::new(8, list.top + 2 + index as i32 * 30, sidebar - 16, 28);
                let on = self.open.as_deref() == Some(id.as_str());
                p.button(
                    r,
                    if on { l.selection } else { Color::TRANSPARENT },
                    l.radius,
                    &format!("chat:channel:{id}"),
                    title,
                );
                p.left(
                    r.x + 12,
                    r.y + 5,
                    r.width.saturating_sub(20),
                    &format!("# {title}"),
                    13,
                    if on { l.accent } else { INK },
                );
            }
            list.end(p);
            action(
                p,
                &l,
                Rect::new(8, height as i32 - 40, sidebar - 16, 28),
                "Reload",
                "chat:reload",
                false,
            );
        }
        let x = sidebar as i32;
        let pane = width.saturating_sub(sidebar);
        if let Some(text) = self.status.notice() {
            notice(p, pane, top + 16, text);
        }
        if self.channel.messages.is_empty() && self.status.notice().is_none() {
            notice(p, pane, top + 30, "No messages yet");
        }
        let messages = screen.column_from_end(
            p,
            "messages",
            Rect::new(
                x,
                top,
                pane,
                (height as i32 - composer_h - top).max(1) as u32,
            ),
        );
        let mut y = messages.top + 8;
        for post in &self.channel.messages {
            let text_w = pane.saturating_sub(32);
            let lines = p
                .measure(&post.text, 13, false)
                .div_ceil(text_w.max(1))
                .max(1);
            let h = 22 + lines * 18;
            p.strong(x + 16, y, text_w, &post.author, 12, l.accent);
            p.paragraph(x + 16, y + 16, text_w, &post.text, 13, INK);
            // Reactions are a real service route, so the control is real.
            let react = Rect::new(x + pane as i32 - 40, y, 30, 20);
            p.button(
                react,
                Color::TRANSPARENT,
                l.radius,
                &format!("chat:react:{}:+1", post.id),
                "React",
            );
            p.center(react.x, react.y + 2, react.width, "+1", 11, MUTED);
            y += h as i32 + 6;
        }
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
            l.radius,
            if focused && theme.mobile() {
                l.accent
            } else {
                LINE
            },
        );
        p.region(bar, "chat:compose", "Message");
        p.left(
            bar.x + 10,
            bar.y + 8,
            bar.width.saturating_sub(20),
            if self.draft.is_empty() {
                "Write a message"
            } else {
                &self.draft
            },
            13,
            if self.draft.is_empty() { FAINT } else { INK },
        );
        action(
            p,
            &l,
            Rect::new(x + pane as i32 - 80, height as i32 - composer_h + 6, 70, 32),
            "Send",
            "chat:send",
            true,
        );
        let _ = Align::Left;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app() -> Chat {
        let (mut app, _) = Chat::launch("http://chat.internal/", 1, 0);
        let more = app
            .http(1, "channels", 200, r#"{"general":"General"}"#)
            .unwrap();
        assert_eq!(more.len(), 1);
        app.http(
            1,
            "channel",
            200,
            r#"{"title":"General","members":["alice"],"messages":[{"id":"m1","author":"carol","text":"Hello","time":0}]}"#,
        )
        .unwrap();
        app
    }
    #[test]
    fn the_first_real_channel_opens_and_its_messages_are_service_records() {
        let app = app();
        assert_eq!(app.open.as_deref(), Some("general"));
        assert_eq!(app.channel.messages[0].author, "carol");
    }
    #[test]
    fn sending_posts_the_draft_and_refetches_rather_than_guessing() {
        let mut app = app();
        assert!(app.click(1, "chat:send", 0).is_err());
        app.text("On my way").unwrap();
        let effects = app.click(1, "chat:send", 0).unwrap();
        let AppEffect::Http { url, body, .. } = &effects[0] else {
            panic!("expected a request");
        };
        assert_eq!(url, "http://chat.internal/api/channels/general/messages");
        assert_eq!(body, r#"{"text":"On my way"}"#);
        let more = app.http(1, "send", 200, "{}").unwrap();
        assert!(app.draft.is_empty());
        assert_eq!(more.len(), 1);
    }
    #[test]
    fn every_painted_control_is_one_the_model_accepts() {
        let base = app();
        let mut scene = Painter::themed(DesktopTheme::Ubuntu, 900, 600, 0);
        base.render(
            &mut scene,
            &crate::AppEnv {
                theme: DesktopTheme::Ubuntu,
                width: 900,
                height: 600,
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
            if target == "chat:send" {
                app.text("x").unwrap();
            }
            assert!(
                app.click(1, &target, 0).is_ok(),
                "unhandled control {target}"
            );
        }
    }
    #[test]
    fn opened_on_a_shared_page_it_starts_as_the_draft() {
        let (app, _) = Chat::launch("http://chat.example/#http://intranet.internal/", 1, 0);
        assert_eq!(app.base, "http://chat.example/");
        assert_eq!(app.draft, "http://intranet.internal/");
        let (plain, _) = Chat::launch("http://chat.example/", 1, 0);
        assert_eq!(plain.base, "http://chat.example/");
        assert!(plain.draft.is_empty());
        let (bare, _) = Chat::launch("", 1, 0);
        assert_eq!(bare.base, "http://chat.internal/");
    }
}
