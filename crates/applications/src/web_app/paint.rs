//! A web application's document painted into its window: the web engine paints it,
//! and the platform owns the parts a native application also leaves to it — the
//! palette and UI typeface (handed to the document as custom properties), and
//! scrolling, whose offsets live in the window and whose bars are the platform's.

use std::collections::BTreeMap;

use cw_scene::{Color, Scene};
use cw_web::dom::Document;
use cw_web::geom::{Au, Point};
use cw_web::paint::{self, PaintContext};
use cw_web::Viewport;

use super::runtime::View;
use crate::apps::look::{look, FAINT, INK, LINE, MUTED};
use crate::desktop_scene::{DesktopTheme, Painter};

fn css_color(c: Color) -> String {
    if c.3 == 255 {
        format!("rgb({}, {}, {})", c.0, c.1, c.2)
    } else {
        format!(
            "rgba({}, {}, {}, {:.4})",
            c.0,
            c.1,
            c.2,
            f64::from(c.3) / 255.0
        )
    }
}

/// The platform's palette and UI typeface as custom properties on `:root`, the
/// document's default font, and scroll containers without the engine's own bars
/// (the platform paints its bars over them).
pub fn theme_css(theme: DesktopTheme) -> String {
    let l = look(theme);
    let font = theme.typeface().family_name();
    format!(
        ":root{{--cw-accent:{};--cw-surface:{};--cw-chrome:{};--cw-selection:{};\
         --cw-ink:{};--cw-muted:{};--cw-faint:{};--cw-line:{};--cw-radius:{}px;\
         --cw-row:{}px;--cw-title:{}px;--cw-font:\"{font}\";}}\
         html{{font-family:\"{font}\",sans-serif;color:{};background:{};}}\
         *{{scrollbar-width:none;}}",
        css_color(l.accent),
        css_color(l.surface),
        css_color(l.chrome),
        css_color(l.selection),
        css_color(INK),
        css_color(MUTED),
        css_color(FAINT),
        css_color(LINE),
        l.radius,
        l.row,
        l.title,
        css_color(INK),
        css_color(l.surface),
    )
}

/// The pane name a scroll area is published under: the container's
/// `data-cw-pane`, else its id; `page` for the document. A name must be one segment
/// (no colon) to be a pane of the window, whose scroll bar target it goes into.
fn pane_of(doc: &Document, target: &str) -> Option<String> {
    let id = target.strip_prefix("pane:")?;
    let name = if id == "page" {
        id
    } else {
        let node = super::node_for(doc, id)?;
        doc.attr(node, "data-cw-pane").unwrap_or(id)
    };
    (!name.is_empty() && !name.contains(':') && !name.starts_with('/')).then(|| name.to_owned())
}

fn paint_with(v: &View<'_>, width: u32, height: u32, offsets: &BTreeMap<String, i32>) -> Scene {
    let mut ctx = PaintContext::default();
    for (pane, offset) in offsets {
        if pane == "page" {
            ctx.scroll = Point {
                x: Au::ZERO,
                y: Au::from_px_i32(*offset),
            };
        } else if let Some(node) = super::pane_node(v.doc, pane) {
            ctx.scroll_offsets.insert(
                node,
                Point {
                    x: Au::ZERO,
                    y: Au::from_px_i32(*offset),
                },
            );
        }
    }
    ctx.focused = v.focused;
    ctx.caret = v.focused.and_then(|f| {
        let (_, end) = *v.selection.get(&f)?;
        let len = v
            .values
            .get(&f)
            .map(|s| s.chars().count())
            .unwrap_or_else(|| v.doc.text_content(f).chars().count());
        (end < len).then_some(end)
    });
    ctx.values = v.values.clone();
    paint::paint(
        v.doc,
        v.styles,
        v.tree,
        Viewport {
            width,
            height,
            scale: 1,
            zoom: 100,
        },
        &ctx,
    )
}

/// Paints `v` into `p`, with the window's pane offsets clamped to what each pane can
/// scroll, and a pane a finger pulls past an end displaced by the pull, as
/// `Painter::end_pane` does for a native application.
pub fn paint(v: &View<'_>, p: &mut Painter, env: &crate::AppEnv<'_>) {
    let (width, height) = (env.width.max(1), env.height.max(1));
    let mut offsets: BTreeMap<String, i32> = p
        .scroll
        .offsets
        .iter()
        .map(|(k, v)| (k.clone(), (*v).max(0)))
        .collect();
    let mut scene = paint_with(v, width, height, &offsets);
    let mut changed = false;
    let mut stretched = vec![];
    for area in scene.scrolls.iter().filter(|a| !a.horizontal) {
        let Some(name) = pane_of(v.doc, &area.target) else {
            continue;
        };
        let span = area.bounds.height;
        let max = area.extent.saturating_sub(span) as i32;
        // An offset the content has shrunk under is pulled back.
        if let Some(offset) = offsets.get_mut(&name) {
            if *offset > max {
                *offset = max;
                changed = true;
            }
        }
        let offset = offsets.get(&name).copied().unwrap_or(0);
        let stretch = match p.scroll.stretch_of(&name) {
            s if s > 0 && offset == 0 => s,
            s if s < 0 && offset == max => s,
            _ => 0,
        }
        .clamp(-(span as i32), span as i32);
        if stretch != 0 {
            stretched.push((name, offset - stretch));
            changed = true;
        }
    }
    if changed {
        let mut painted = offsets.clone();
        painted.extend(stretched);
        scene = paint_with(v, width, height, &painted);
    }
    let l = look(env.theme);
    p.scene.background = l.surface;
    let z = p.z;
    let mut top = z;
    for mut n in scene.nodes {
        // What a pane scrolled wholly out of view paints nothing, and is dropped.
        if let Some(clip) = n.clip {
            if clip.intersection(n.transform.bounds(n.bounds)).is_none() {
                continue;
            }
        }
        n.id = p.next;
        p.next += 1;
        n.z += z;
        top = top.max(n.z);
        p.scene.nodes.push(n);
    }
    // The platform's scroll bars sit over the document, as over any application.
    p.z = top + 1;
    for mut area in scene.scrolls {
        let Some(name) = pane_of(v.doc, &area.target) else {
            continue;
        };
        area.target = format!("pane:{name}");
        area.offset = offsets.get(&name).copied().unwrap_or(0);
        if let Some((title, height)) = large_title(v.doc, &name) {
            area.title = Some(title);
            area.title_height = height;
        }
        let span = if area.horizontal {
            area.bounds.width
        } else {
            area.bounds.height
        };
        if area.extent > span && !area.horizontal {
            p.scroll_bar(&name, area.bounds, area.offset, area.extent);
        }
        p.scene.scrolls.push(area);
    }
    p.z = z;
}

/// A scroll container that opens with a large title (iOS) says so with
/// `data-cw-large-title="<title>"` and, optionally, `data-cw-large-title-height`.
fn large_title(doc: &Document, pane: &str) -> Option<(String, u32)> {
    let node = super::pane_node(doc, pane)?;
    let title = doc.attr(node, "data-cw-large-title")?.to_owned();
    let height = doc
        .attr(node, "data-cw-large-title-height")
        .and_then(|h| h.trim().trim_end_matches("px").parse().ok())
        .unwrap_or(crate::apps::look::LARGE_TITLE);
    Some((title, height))
}
