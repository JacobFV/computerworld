//! Documents over the `docs` service: real documents, real revisions, real edits.
use super::look::{action, look, notice, screen, INK, LINE, MUTED};
use super::{push_bounded, Status};
use crate::desktop_scene::{DesktopTheme, Painter};
use crate::AppEffect;
use cw_scene::Rect;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Docs {
    pub base: String,
    pub documents: Vec<Document>,
    pub open: Option<Document>,
    pub draft: String,
    pub dirty: bool,
    pub status: Status,
    /// The body has been tapped, so on a phone it has the keyboard. A desktop focuses
    /// the body whenever a document is open.
    #[serde(default)]
    pub editing: bool,
}
impl Docs {
    pub const KIND: &'static str = "docs";
    pub fn launch(argument: &str, window: u64, _clock_us: u64) -> (Self, Vec<AppEffect>) {
        let app = Self {
            base: if argument.is_empty() {
                "http://docs.internal/".into()
            } else {
                argument.to_owned()
            },
            documents: vec![],
            open: None,
            draft: String::new(),
            dirty: false,
            status: Status::Loading,
            editing: false,
        };
        let effects = vec![app.request(window, "list", "GET", "/api/documents", String::new())];
        (app, effects)
    }
    pub fn kind(&self) -> &'static str {
        Self::KIND
    }
    pub fn title(&self, theme: DesktopTheme) -> String {
        match theme {
            DesktopTheme::Windows => "Word",
            DesktopTheme::Ubuntu => "Writer",
            DesktopTheme::Android => "Docs",
            _ => "Pages",
        }
        .into()
    }
    pub fn document(&self) -> String {
        self.open
            .as_ref()
            .map(|d| d.title.clone())
            .unwrap_or_default()
    }
    pub fn caption(&self) -> String {
        self.open
            .as_ref()
            .map(|d| format!("Revision {}", d.revision))
            .unwrap_or_default()
    }
    pub fn modified(&self) -> bool {
        self.dirty
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
            "list" => {
                self.documents = serde_json::from_str(body).unwrap_or_default();
                Ok(vec![])
            }
            "open" => {
                let document: Document = serde_json::from_str(body).unwrap_or_default();
                self.draft = document.body.clone();
                self.dirty = false;
                self.open = Some(document);
                Ok(vec![])
            }
            // A save returns the committed document, which carries the new revision; an
            // editor that kept its own guess would silently diverge from the service.
            "save" => {
                let document: Document = serde_json::from_str(body).unwrap_or_default();
                self.draft = document.body.clone();
                self.dirty = false;
                self.open = Some(document);
                self.status = Status::Loading;
                Ok(vec![self.request(
                    window,
                    "list",
                    "GET",
                    "/api/documents",
                    String::new(),
                )])
            }
            other => Err(format!("unexpected documents reply {other}")),
        }
    }
    pub fn text(&mut self, text: &str) -> Result<(), String> {
        if self.open.is_none() {
            return Err("no document is open".into());
        }
        push_bounded(&mut self.draft, text, 64 * 1024);
        self.dirty = true;
        Ok(())
    }
    pub fn key(&mut self, window: u64, key: &str, clock_us: u64) -> Result<Vec<AppEffect>, String> {
        match key {
            "Backspace" => {
                if self.open.is_none() {
                    return Err("no document is open".into());
                }
                self.draft.pop();
                self.dirty = true;
                Ok(vec![])
            }
            "Enter" => {
                if self.open.is_none() {
                    return Err("no document is open".into());
                }
                self.draft.push('\n');
                self.dirty = true;
                Ok(vec![])
            }
            "Ctrl+s" | "Meta+s" => self.click(window, "docs:save", clock_us),
            other => Err(format!("unsupported documents key {other}")),
        }
    }
    pub fn click(
        &mut self,
        window: u64,
        target: &str,
        _clock_us: u64,
    ) -> Result<Vec<AppEffect>, String> {
        let command = target
            .strip_prefix("docs:")
            .ok_or("interaction does not belong to documents")?;
        match command {
            "reload" => {
                self.status = Status::Loading;
                Ok(vec![self.request(
                    window,
                    "list",
                    "GET",
                    "/api/documents",
                    String::new(),
                )])
            }
            "body" => {
                if self.open.is_none() {
                    return Err("no document is open".into());
                }
                self.editing = true;
                Ok(vec![])
            }
            // A phone's back button returns to the list. An edit not yet saved would be
            // lost, so it is refused rather than dropped: Save first.
            "close" => {
                if self.open.is_none() {
                    return Err("no document is open".into());
                }
                if self.dirty {
                    return Err("save the document before closing it".into());
                }
                self.open = None;
                self.editing = false;
                self.draft.clear();
                Ok(vec![])
            }
            "save" => {
                let document = self.open.clone().ok_or("no document is open")?;
                self.status = Status::Loading;
                Ok(vec![self.request(
                    window,
                    "save",
                    "POST",
                    &format!("/api/documents/{}", document.id),
                    serde_json::json!({
                        "revision": document.revision,
                        "body": self.draft,
                    })
                    .to_string(),
                )])
            }
            rest => {
                let id = rest
                    .strip_prefix("open:")
                    .ok_or_else(|| format!("unknown documents command {command}"))?;
                if !self.documents.iter().any(|d| d.id == id) {
                    return Err("document not found".into());
                }
                self.editing = false;
                self.status = Status::Loading;
                Ok(vec![self.request(
                    window,
                    "open",
                    "GET",
                    &format!("/api/documents/{id}"),
                    String::new(),
                )])
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
            id: "docs-title".into(),
            text: self.document(),
            level: 2,
        });
        if let Some(text) = self.status.notice() {
            page.elements.push(E::Text {
                id: "docs-status".into(),
                text: text.into(),
            });
        }
        page.elements.push(E::Button {
            id: "docs:reload".into(),
            text: "Reload".into(),
            action: act("docs:reload"),
            style: None,
        });
        for document in &self.documents {
            page.elements.push(E::Button {
                id: format!("docs:open:{}", document.id),
                text: document.title.clone(),
                action: act(&format!("docs:open:{}", document.id)),
                style: None,
            });
        }
        if self.open.is_some() {
            page.elements.push(E::Input {
                id: "docs-body".into(),
                label: "Content".into(),
                value: self.draft.clone(),
                placeholder: String::new(),
            });
            page.elements.push(E::Button {
                id: "docs:save".into(),
                text: "Save".into(),
                action: act("docs:save"),
                style: None,
            });
        }
    }
    pub fn render(&self, p: &mut Painter, env: &crate::AppEnv<'_>) {
        let (theme, width, height) = (env.theme, env.width, env.height);
        let l = look(theme);
        p.scene.background = l.surface;
        let narrow = theme.mobile() || width < 520;
        // A phone shows the open document, or the list: one thing at a time.
        if narrow {
            if let Some(document) = &self.open {
                self.document_view(p, &l, width, height, document, 0, 0);
                return;
            }
        }
        let screen = screen(p, theme, &l, width, height as i32, &self.title(theme));
        let top = screen.top;
        let list_w = if narrow { width } else { 216 };
        if !screen.scrolls() {
            p.box_(Rect::new(0, top, list_w, height), l.chrome, 0);
        }
        action(
            p,
            &l,
            Rect::new(8, top + 8, list_w.saturating_sub(16), 26),
            "Reload",
            "docs:reload",
            false,
        );
        if let Some(text) = self.status.notice() {
            notice(p, list_w, top + 44, text);
        } else if self.documents.is_empty() {
            notice(p, list_w, top + 44, "No documents");
        }
        let list = screen.column(
            p,
            "list",
            Rect::new(
                0,
                top + 40,
                list_w,
                (height as i32 - top - 40).max(1) as u32,
            ),
        );
        for (index, document) in self.documents.iter().enumerate() {
            let r = Rect::new(
                4,
                list.top + 2 + index as i32 * 32,
                list_w.saturating_sub(8),
                30,
            );
            let on = self.open.as_ref().is_some_and(|d| d.id == document.id);
            p.button(
                r,
                if on {
                    l.selection
                } else {
                    cw_scene::Color::TRANSPARENT
                },
                l.radius,
                &format!("docs:open:{}", document.id),
                &document.title,
            );
            p.left(
                r.x + 10,
                r.y + 3,
                r.width.saturating_sub(18),
                &document.title,
                13,
                if on { l.accent } else { INK },
            );
            p.left(
                r.x + 10,
                r.y + 17,
                r.width.saturating_sub(18),
                &format!("{} · rev {}", document.owner, document.revision),
                10,
                MUTED,
            );
        }
        list.end(p);
        screen.end(p);
        if narrow {
            return;
        }
        let x = list_w as i32;
        p.vline(x, top, height, LINE);
        match &self.open {
            Some(document) => self.document_view(p, &l, width, height, document, x, top),
            None => notice(
                p,
                width.saturating_sub(list_w),
                top + 40,
                "Select a document",
            ),
        }
    }
    /// The open document from `x` rightwards and `top` down; its body scrolls. On a
    /// phone it fills the screen and a back button returns to the list.
    #[allow(clippy::too_many_arguments)]
    fn document_view(
        &self,
        p: &mut Painter,
        l: &super::look::Look,
        width: u32,
        height: u32,
        document: &Document,
        x: i32,
        top: i32,
    ) {
        let mut left = x + 20;
        if x == 0 {
            let back = Rect::new(4, top + 6, 40, 30);
            p.button(
                back,
                cw_scene::Color::TRANSPARENT,
                l.radius,
                "docs:close",
                "Back to documents",
            );
            if self.dirty {
                p.disabled("Save the document before going back to the list");
            }
            p.symbol("chevron-left", back.x + 8, back.y + 5, 20, l.accent);
            left = back.x + back.width as i32 + 4;
        }
        p.strong(
            left,
            top + 12,
            (width as i32 - left - 100).max(0) as u32,
            &document.title,
            16,
            INK,
        );
        action(
            p,
            l,
            Rect::new(width as i32 - 92, top + 10, 82, 26),
            if self.dirty { "Save •" } else { "Save" },
            "docs:save",
            self.dirty,
        );
        let body = Rect::new(
            x + 20,
            top + 46,
            width.saturating_sub(x as u32 + 40),
            (height as i32 - top - 58).max(1) as u32,
        );
        p.region(body, "docs:body", "Document body");
        let pane = p.pane("body", body);
        p.paragraph(
            body.x,
            pane.top(),
            body.width.saturating_sub(14),
            &self.draft,
            13,
            INK,
        );
        p.end_pane(pane, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app() -> Docs {
        let (mut app, _) = Docs::launch("http://docs.internal/", 1, 0);
        app.http(
            1,
            "list",
            200,
            r#"[{"id":"launch","title":"Atlas launch checklist","owner":"carol","body":"","revision":1}]"#,
        )
        .unwrap();
        app
    }
    #[test]
    fn saving_sends_the_revision_the_service_last_committed() {
        let mut app = app();
        app.click(1, "docs:open:launch", 0).unwrap();
        app.http(
            1,
            "open",
            200,
            r#"{"id":"launch","title":"Atlas launch checklist","owner":"carol","body":"Status: pending","revision":1}"#,
        )
        .unwrap();
        assert_eq!(app.draft, "Status: pending");
        app.text(" done").unwrap();
        assert!(app.dirty);
        let effects = app.click(1, "docs:save", 0).unwrap();
        let AppEffect::Http { url, body, .. } = &effects[0] else {
            panic!("expected a request");
        };
        assert_eq!(url, "http://docs.internal/api/documents/launch");
        let sent: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(sent["revision"], 1);
        assert_eq!(sent["body"], "Status: pending done");
        // The committed reply, not the local guess, becomes the new state.
        app.http(
            1,
            "save",
            200,
            r#"{"id":"launch","title":"Atlas launch checklist","owner":"carol","body":"Status: pending done","revision":2}"#,
        )
        .unwrap();
        assert_eq!(app.open.unwrap().revision, 2);
        assert!(!app.dirty);
    }
    #[test]
    fn a_conflict_is_shown_and_never_silently_overwrites() {
        let mut app = app();
        app.click(1, "docs:open:launch", 0).unwrap();
        app.http(
            1,
            "open",
            200,
            r#"{"id":"launch","title":"T","revision":1,"body":"a"}"#,
        )
        .unwrap();
        app.text("b").unwrap();
        app.click(1, "docs:save", 0).unwrap();
        app.http(1, "save", 409, r#"{"error":"revision conflict"}"#)
            .unwrap();
        assert_eq!(app.status, Status::Offline("revision conflict".into()));
        assert!(app.dirty, "the unsaved edit survives a rejected save");
    }
    #[test]
    fn every_painted_control_is_one_the_model_accepts() {
        let mut base = app();
        base.click(1, "docs:open:launch", 0).unwrap();
        base.http(
            1,
            "open",
            200,
            r#"{"id":"launch","title":"T","revision":1,"body":"a"}"#,
        )
        .unwrap();
        let mut scene = Painter::themed(DesktopTheme::Windows, 900, 600, 0);
        base.render(
            &mut scene,
            &crate::AppEnv {
                theme: DesktopTheme::Windows,
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
            assert!(
                app.click(1, &target, 0).is_ok(),
                "unhandled control {target}"
            );
        }
    }
}
