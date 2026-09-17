//! Native structured-page presentation. Every control is derived from a received
//! page element; this layer never reads services, users or privileged world state.
use super::ImageAsset;
use cw_protocol::{Page, PageElement};
use cw_scene::{wrap_text, Color, Node, Primitive, Rect, Scene, Semantic};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
const INK: Color = Color::rgb(37, 43, 54);
const MUTED: Color = Color::rgb(111, 120, 133);
const BORDER: Color = Color::rgb(225, 229, 235);
struct Layout<'a> {
    scene: Scene,
    fields: &'a BTreeMap<String, String>,
    images: &'a BTreeMap<String, Arc<ImageAsset>>,
    used: BTreeSet<u64>,
    decoration: u64,
    accent: Color,
}
impl Layout<'_> {
    fn id(&mut self, s: &str) -> u64 {
        let mut id = 0xcbf29ce484222325u64;
        for b in s.bytes() {
            id = (id ^ u64::from(b)).wrapping_mul(0x100000001b3);
        }
        id &= ((1u64 << 51) - 1) & !15;
        while self.used.contains(&id) {
            id = (id + 16) & (((1u64 << 51) - 1) & !15);
        }
        self.used.insert(id);
        id
    }
    fn node(
        &mut self,
        id: u64,
        r: Rect,
        p: Primitive,
        semantic: Option<Semantic>,
        action: Option<&str>,
    ) {
        let mut n = Node::new(id, r, p);
        n.z = self.scene.nodes.len() as i32;
        n.semantic = semantic;
        n.interaction = action.map(str::to_owned);
        n.clip = Some(Rect::new(0, 0, self.scene.width, self.scene.height));
        self.scene.nodes.push(n);
    }
    fn decor(&mut self, r: Rect, fill: Color, radius: u32, border: Option<Color>) {
        let id = self.decoration;
        self.decoration += 1;
        self.node(
            id,
            r,
            Primitive::RoundedBox {
                fill,
                border,
                border_width: u32::from(border.is_some()),
                radius,
            },
            None,
            None,
        );
    }
    fn text(&mut self, id: u64, r: Rect, text: &str, size: u16, color: Color, bold: bool) {
        self.node(
            id,
            r,
            if bold {
                Primitive::UiTextBold {
                    text: text.into(),
                    size,
                    color,
                }
            } else {
                Primitive::UiText {
                    text: text.into(),
                    size,
                    color,
                }
            },
            None,
            None,
        );
    }
    fn caption(&mut self, r: Rect, s: &str, size: u16, color: Color, bold: bool) {
        let id = self.decoration;
        self.decoration += 1;
        self.text(id, r, s, size, color, bold);
    }
    fn element(&mut self, e: &PageElement, x: i32, y: &mut i32, w: u32) {
        let id = self.id(e.id());
        match e {
            PageElement::Form {
                id: form, children, ..
            } => {
                let start = *y;
                self.decor(
                    Rect::new(x, start, w, form_height(children, w)),
                    Color::WHITE,
                    10,
                    Some(BORDER),
                );
                let title = match form.as_str() {
                    "compose" => "New message",
                    "create" => "Create",
                    "edit" => "Edit document",
                    "comment" => "Add a comment",
                    "send" => "Message",
                    _ => "Update",
                };
                self.text(
                    id,
                    Rect::new(x + 16, start + 16, w.saturating_sub(32), 24),
                    title,
                    14,
                    INK,
                    true,
                );
                *y += 48;
                for child in children {
                    self.element(child, x + 16, y, w.saturating_sub(32));
                }
                *y += 12;
            }
            PageElement::Group { children, .. } => {
                for child in children {
                    self.element(child, x, y, w);
                }
            }
            PageElement::Input {
                id: action,
                label,
                value,
                placeholder,
            } => {
                let value = self.fields.get(action).unwrap_or(value);
                let multiline =
                    action.ends_with("-body") || label == "Content" || label == "Message";
                let h = if multiline { 82 } else { 36 };
                self.text(id, Rect::new(x, *y, w, 18), label, 11, MUTED, false);
                let r = Rect::new(x, *y + 21, w, h);
                let semantic = Semantic {
                    role: "textbox".into(),
                    label: label.clone(),
                    value: Some(value.clone()),
                    focusable: true,
                    ..Semantic::default()
                };
                self.node(
                    id + 1,
                    r,
                    Primitive::RoundedBox {
                        fill: Color::rgb(253, 254, 255),
                        border: Some(Color::rgb(207, 215, 226)),
                        border_width: 1,
                        radius: 5,
                    },
                    Some(semantic),
                    Some(action),
                );
                self.text(
                    id + 2,
                    Rect::new(x + 10, *y + 30, w.saturating_sub(20), h.saturating_sub(12)),
                    if value.is_empty() { placeholder } else { value },
                    13,
                    if value.is_empty() { MUTED } else { INK },
                    false,
                );
                *y += h as i32 + 34;
            }
            PageElement::Button {
                id: action, text, ..
            } => {
                let text = if text == "Submit" {
                    if action.starts_with("compose-") || action.starts_with("send-") {
                        "Send"
                    } else if action.starts_with("create-") {
                        "Create"
                    } else if action.starts_with("comment-") {
                        "Post comment"
                    } else {
                        "Save changes"
                    }
                } else {
                    text
                };
                let bw = (text.chars().count() as u32 * 8 + 30).min(w);
                self.node(
                    id,
                    Rect::new(x, *y, bw, 34),
                    Primitive::RoundedBox {
                        fill: self.accent,
                        border: None,
                        border_width: 0,
                        radius: 6,
                    },
                    Some(Semantic {
                        role: "button".into(),
                        label: text.into(),
                        focusable: true,
                        ..Semantic::default()
                    }),
                    Some(action),
                );
                self.text(
                    id + 1,
                    Rect::new(x + 14, *y + 9, bw.saturating_sub(22), 19),
                    text,
                    12,
                    Color::WHITE,
                    true,
                );
                *y += 46;
            }
            PageElement::Link {
                id: action, text, ..
            } => {
                self.node(
                    id,
                    Rect::new(x, *y, w, 38),
                    Primitive::RoundedBox {
                        fill: Color::rgb(242, 246, 252),
                        border: None,
                        border_width: 0,
                        radius: 6,
                    },
                    Some(Semantic {
                        role: "link".into(),
                        label: text.clone(),
                        focusable: true,
                        ..Semantic::default()
                    }),
                    Some(action),
                );
                self.text(
                    id + 1,
                    Rect::new(x + 12, *y + 11, w.saturating_sub(24), 21),
                    text,
                    13,
                    self.accent,
                    false,
                );
                *y += 46;
            }
            PageElement::Heading { text, level, .. } => {
                let size = if *level <= 1 { 18 } else { 15 };
                self.node(
                    id,
                    Rect::new(x, *y, w, 30),
                    Primitive::UiTextBold {
                        text: text.clone(),
                        size,
                        color: INK,
                    },
                    Some(Semantic {
                        role: "heading".into(),
                        label: text.clone(),
                        ..Semantic::default()
                    }),
                    None,
                );
                *y += 38;
            }
            PageElement::Text { text, .. } => {
                let lines = wrap_text(text, (w / 8).max(1) as usize);
                let h = lines.len() as u32 * 21 + 7;
                self.node(
                    id,
                    Rect::new(x, *y, w, h),
                    Primitive::UiText {
                        text: lines.join("\n"),
                        size: 13,
                        color: INK,
                    },
                    Some(Semantic {
                        role: "text".into(),
                        label: text.clone(),
                        ..Semantic::default()
                    }),
                    None,
                );
                *y += h as i32 + 12;
            }
            PageElement::Image {
                id: asset_id,
                alt,
                width,
                height,
                ..
            } => {
                if let Some(a) = self.images.get(asset_id) {
                    let dw = if *width == 0 { a.width } else { *width }.min(w);
                    let dh = if *height == 0 { a.height } else { *height }.min(4096);
                    self.node(
                        id,
                        Rect::new(x, *y, dw, dh),
                        Primitive::Image {
                            width: a.width,
                            height: a.height,
                            rgba: a.rgba.clone(),
                        },
                        Some(Semantic {
                            role: "img".into(),
                            label: alt.clone(),
                            ..Semantic::default()
                        }),
                        None,
                    );
                    *y += dh as i32 + 12;
                } else {
                    self.text(id, Rect::new(x, *y, w, 24), alt, 13, MUTED, false);
                    *y += 32;
                }
            }
        }
    }
}
fn form_height(children: &[PageElement], _w: u32) -> u32 {
    60 + children
        .iter()
        .map(|e| match e {
            PageElement::Input { id, label, .. } => {
                if id.ends_with("-body") || label == "Content" || label == "Message" {
                    116
                } else {
                    70
                }
            }
            PageElement::Button { .. } => 46,
            _ => 40,
        })
        .sum::<u32>()
}
pub(super) fn layout(
    page: &Page,
    fields: &BTreeMap<String, String>,
    images: &BTreeMap<String, Arc<ImageAsset>>,
    width: u32,
    height: u32,
    scroll: i32,
) -> Scene {
    let accent = match page.title.as_str() {
        "Chat" => Color::rgb(89, 47, 112),
        "Calendar" => Color::rgb(31, 112, 201),
        "Documents" => Color::rgb(33, 132, 100),
        _ => Color::rgb(34, 112, 205),
    };
    let mut p = Layout {
        scene: Scene::new(width, height),
        fields,
        images,
        used: BTreeSet::new(),
        decoration: 1 << 52,
        accent,
    };
    p.scene.background = Color::rgb(248, 250, 253);
    let special = matches!(
        page.title.as_str(),
        "Mail" | "Chat" | "Calendar" | "Documents"
    );
    let sidebar = if special && width >= 720 { 176 } else { 0 };
    let mut y = if special { 88 } else { 16 };
    y -= scroll;
    if special {
        if sidebar > 0 {
            p.decor(
                Rect::new(0, 64, sidebar, height.saturating_sub(64)),
                if page.title == "Chat" {
                    Color::rgb(246, 242, 248)
                } else {
                    Color::rgb(241, 245, 250)
                },
                0,
                None,
            );
            p.caption(
                Rect::new(18, 84, 145, 20),
                match page.title.as_str() {
                    "Mail" => "MESSAGES",
                    "Chat" => "CHANNELS",
                    "Documents" => "DOCUMENTS",
                    _ => "AGENDA",
                },
                10,
                MUTED,
                true,
            );
        }
    } else {
        p.text(
            1,
            Rect::new(16, y, width.saturating_sub(32), 28),
            &page.title,
            20,
            INK,
            true,
        );
        y += 40;
    }
    let x = sidebar as i32 + if special { 24 } else { 16 };
    let total = width.saturating_sub(sidebar + if special { 48 } else { 32 });
    let two_column = matches!(page.title.as_str(), "Mail" | "Calendar") && total >= 600;
    let mainw = if two_column {
        total.saturating_sub(286)
    } else {
        total
    };
    let mut side_y = 116 - scroll;
    let mut form_y = 88 - scroll;
    let mut deferred = Vec::new();
    for e in &page.elements {
        if matches!(e,PageElement::Heading{text,..}if text==&page.title) {
            continue;
        }
        if sidebar > 0
            && matches!(page.title.as_str(), "Chat" | "Documents")
            && matches!(e, PageElement::Link { .. })
        {
            p.element(e, 12, &mut side_y, sidebar - 24);
            continue;
        }
        if matches!(page.title.as_str(), "Mail" | "Calendar")
            && matches!(e,PageElement::Form{id,..}if id=="compose"||id=="create")
        {
            if two_column {
                p.element(e, x + mainw as i32 + 24, &mut form_y, 262);
            } else {
                deferred.push(e);
            }
            continue;
        }
        if page.title == "Mail" && matches!(e, PageElement::Heading { .. }) {
            p.decor(
                Rect::new(x - 10, y - 9, mainw + 20, 39),
                Color::WHITE,
                6,
                Some(BORDER),
            );
        }
        if page.title == "Calendar" && matches!(e, PageElement::Heading { .. }) {
            p.decor(Rect::new(x - 9, y - 5, 3, 30), accent, 1, None);
        }
        p.element(e, x, &mut y, mainw);
    }
    for e in deferred {
        y += 18;
        p.element(e, x, &mut y, mainw);
    }
    if special {
        for node in &mut p.scene.nodes {
            node.clip = Some(Rect::new(0, 64, width, height.saturating_sub(64)));
        }
        p.decor(Rect::new(0, 0, width, 64), Color::WHITE, 0, None);
        p.decor(Rect::new(0, 63, width, 1), BORDER, 0, None);
        let icon = match page.title.as_str() {
            "Mail" => "mail",
            "Chat" => "messages",
            "Calendar" => "calendar",
            _ => "editor",
        };
        p.node(
            2,
            Rect::new(20, 16, 30, 30),
            Primitive::AssetImage {
                asset: format!("icon/common/{icon}"),
            },
            None,
            None,
        );
        p.caption(
            Rect::new(62, 22, width.saturating_sub(80), 26),
            &page.title,
            20,
            INK,
            true,
        );
    }
    p.scene
}

#[cfg(test)]
mod tests {
    use super::*;
    use cw_protocol::PageAction;
    #[test]
    fn mail_uses_one_title_and_fields_remain_hit_testable() {
        let mut page = Page::new("Mail");
        page.elements = vec![
            PageElement::Heading {
                id: "title".into(),
                text: "Mail".into(),
                level: 1,
            },
            PageElement::Form {
                id: "compose".into(),
                action: PageAction {
                    method: "POST".into(),
                    url: "/send".into(),
                    fields: BTreeMap::new(),
                },
                children: vec![PageElement::Input {
                    id: "compose-to".into(),
                    label: "Recipients".into(),
                    value: "alice".into(),
                    placeholder: String::new(),
                }],
            },
            PageElement::Heading {
                id: "message".into(),
                text: "Actual subject".into(),
                level: 1,
            },
        ];
        let scene = layout(&page, &BTreeMap::new(), &BTreeMap::new(), 1100, 700, 0);
        scene.validate().unwrap();
        assert_eq!(
            scene
                .nodes
                .iter()
                .filter(|n| matches!(&n.primitive,Primitive::UiTextBold{text,..}if text=="Mail"))
                .count(),
            1
        );
        let input = scene
            .nodes
            .iter()
            .find(|n| n.interaction.as_deref() == Some("compose-to"))
            .unwrap();
        assert!(input.bounds.x > 700);
        assert_eq!(
            scene
                .hit_test(input.bounds.x + 3, input.bounds.y + 3)
                .unwrap()
                .interaction
                .as_deref(),
            Some("compose-to")
        );
        assert_eq!(
            input.semantic.as_ref().unwrap().value.as_deref(),
            Some("alice")
        );
    }
    #[test]
    fn document_links_are_real_sidebar_controls_and_header_masks_scroll() {
        let mut page = Page::new("Documents");
        page.elements.push(PageElement::Link {
            id: "doc".into(),
            text: "Project plan".into(),
            url: "/documents/doc".into(),
        });
        let scene = layout(&page, &BTreeMap::new(), &BTreeMap::new(), 900, 500, 100);
        assert!(scene
            .nodes
            .iter()
            .any(|n| n.interaction.as_deref() == Some("doc")));
        assert!(scene.hit_test(20, 30).is_none());
        assert_eq!(
            scene,
            layout(&page, &BTreeMap::new(), &BTreeMap::new(), 900, 500, 100)
        );
    }
}
