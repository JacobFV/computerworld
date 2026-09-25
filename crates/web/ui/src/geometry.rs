//! Geometry reads that flush layout, for compiled apps: `getBoundingClientRect`,
//! `offset*`, `client*`, `scroll*`, `scrollIntoView`. The numbers are the JS
//! `Realm`'s (`cw_web::script::bindings::layout`), computed the same way from the
//! same engine layout, so a compiled app measures what its React fallback does.

use cw_web::dom::{Document, NodeId};
use cw_web::geom::{Au, Rect};
use cw_web::layout::FragmentKind;
use cw_web::script::inner::fragment_of;
use cw_web::script::Inner;
use cw_web::style::Position;

use crate::value::Value;

fn px(a: Au) -> f64 {
    a.to_f64_px()
}

/// The union of the element's fragment rects in viewport coordinates.
pub(crate) fn client_rect(i: &mut Inner, n: NodeId) -> Option<Rect> {
    let rects = i.rects_of(n);
    let mut it = rects.into_iter();
    let first = it.next()?;
    let u = it.fold(first, |acc, r| acc.union(r));
    let (sx, sy) = i.window_scroll();
    Some(u.translate(-sx, -sy))
}

/// Each of the element's fragment rects in viewport coordinates.
pub(crate) fn client_rects(i: &mut Inner, n: NodeId) -> Vec<Rect> {
    let rects = i.rects_of(n);
    let (sx, sy) = i.window_scroll();
    rects.into_iter().map(|r| r.translate(-sx, -sy)).collect()
}

/// A `DOMRect` as a plain object with its eight readable fields.
pub(crate) fn rect_value(r: Option<Rect>) -> Value {
    let (x, y, w, h) = match r {
        Some(r) => (
            px(r.origin.x),
            px(r.origin.y),
            px(r.size.width),
            px(r.size.height),
        ),
        None => (0.0, 0.0, 0.0, 0.0),
    };
    let f = |k: &str, v: f64| (std::rc::Rc::from(k), Value::Num(v));
    Value::object(vec![
        f("x", x),
        f("y", y),
        f("width", w),
        f("height", h),
        f("top", y.min(y + h)),
        f("right", x.max(x + w)),
        f("bottom", y.max(y + h)),
        f("left", x.min(x + w)),
    ])
}

/// `[offsetLeft, offsetTop, offsetWidth, offsetHeight, clientLeft, clientTop,
/// clientWidth, clientHeight, scrollWidth, scrollHeight]`.
pub(crate) fn metrics(i: &mut Inner, n: NodeId) -> [f64; 10] {
    i.ensure_layout();
    let Some(tree) = i.tree.as_ref() else {
        return [0.0; 10];
    };
    if Some(n) == i.doc.document_element() {
        let vw = px(tree.viewport_width);
        let vh = px(tree.viewport_height);
        let rects = tree.rects_of(n);
        let (w, h) = rects
            .first()
            .map(|r| (px(r.size.width), px(r.size.height)))
            .unwrap_or((vw, vh));
        return [
            0.0,
            0.0,
            w.round(),
            h.round(),
            0.0,
            0.0,
            vw.round(),
            vh.round(),
            px(tree.content_width.max(tree.viewport_width)).round(),
            px(tree.content_height.max(tree.viewport_height)).round(),
        ];
    }
    let Some((f, abs)) = fragment_of(tree, n) else {
        let rects = tree.rects_of(n);
        if rects.is_empty() {
            return [0.0; 10];
        }
        let u = rects.iter().skip(1).fold(rects[0], |a, r| a.union(*r));
        let (ol, ot) = offset_origin(i, n);
        return [
            px(u.origin.x - ol),
            px(u.origin.y - ot),
            u.size.width.to_px_round() as f64,
            u.size.height.to_px_round() as f64,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
        ];
    };
    let (border, scroll) = match &f.kind {
        FragmentKind::Box { border, scroll, .. } => (*border, *scroll),
        _ => (cw_web::geom::Edges::ZERO, None),
    };
    let w = abs.size.width;
    let h = abs.size.height;
    let bar = |on: bool| if on { Au::from_px_i32(15) } else { Au::ZERO };
    let bar_w = scroll.map(|s| bar(s.shows_y_bar)).unwrap_or(Au::ZERO);
    let bar_h = scroll.map(|s| bar(s.shows_x_bar)).unwrap_or(Au::ZERO);
    let client_w = (w - border.horizontal() - bar_w).max(Au::ZERO);
    let client_h = (h - border.vertical() - bar_h).max(Au::ZERO);
    let (scroll_w, scroll_h) = match scroll {
        Some(s) => (
            s.content_width.max(client_w),
            s.content_height.max(client_h),
        ),
        None => {
            let ov = f.overflow;
            let inner_w = (w - border.horizontal()).max(Au::ZERO);
            let inner_h = (h - border.vertical()).max(Au::ZERO);
            (
                (ov.right() - border.left).max(inner_w),
                (ov.bottom() - border.top).max(inner_h),
            )
        }
    };
    let is_inline = i
        .styles
        .get(n)
        .map(|s| {
            s.display.is_inline_level() && !matches!(s.display, cw_web::style::Display::InlineBlock)
        })
        .unwrap_or(false);
    let (ol, ot) = offset_origin(i, n);
    let round = |a: Au| a.to_px_round() as f64;
    if is_inline {
        return [
            round(abs.origin.x - ol),
            round(abs.origin.y - ot),
            round(w),
            round(h),
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
        ];
    }
    [
        round(abs.origin.x - ol),
        round(abs.origin.y - ot),
        round(w),
        round(h),
        round(border.left),
        round(border.top),
        round(client_w),
        round(client_h),
        round(scroll_w),
        round(scroll_h),
    ]
}

/// The origin `offsetTop/Left` are relative to: the offsetParent's padding edge.
fn offset_origin(i: &mut Inner, n: NodeId) -> (Au, Au) {
    match offset_parent(i, n) {
        Some(p)
            if i.doc.is(p, "body")
                && !i.styles.get(p).map(|s| s.is_positioned()).unwrap_or(false) =>
        {
            (Au::ZERO, Au::ZERO)
        }
        Some(p) => {
            let Some(tree) = i.tree.as_ref() else {
                return (Au::ZERO, Au::ZERO);
            };
            match fragment_of(tree, p) {
                Some((f, abs)) => {
                    let border = match &f.kind {
                        FragmentKind::Box { border, .. } => *border,
                        _ => cw_web::geom::Edges::ZERO,
                    };
                    (abs.origin.x + border.left, abs.origin.y + border.top)
                }
                None => (Au::ZERO, Au::ZERO),
            }
        }
        None => (Au::ZERO, Au::ZERO),
    }
}

/// `el.offsetParent`.
pub(crate) fn offset_parent(i: &Inner, n: NodeId) -> Option<NodeId> {
    let style = i.styles.get(n)?;
    if style.display.is_none()
        || matches!(style.position, Position::Fixed)
        || i.doc.is(n, "body")
        || i.doc.is(n, "html")
    {
        return None;
    }
    let mut cur = i.doc.parent(n);
    while let Some(p) = cur {
        if !i.doc.is_element(p) {
            return None;
        }
        if i.doc.is(p, "body") {
            return Some(p);
        }
        if let Some(s) = i.styles.get(p) {
            if s.is_positioned() || i.doc.is(p, "td") || i.doc.is(p, "th") || i.doc.is(p, "table") {
                return Some(p);
            }
        }
        cur = i.doc.parent(p);
    }
    None
}

/// `document.documentElement.scrollTop` scrolls the viewport.
pub(crate) fn scrolling_node(i: &Inner, n: NodeId) -> NodeId {
    if Some(n) == i.doc.document_element() {
        Document::ROOT
    } else {
        n
    }
}

/// An element's (or, for `Document::ROOT`, the window's) scroll offset.
pub(crate) fn scroll_of(i: &mut Inner, n: NodeId) -> (f64, f64) {
    let n = scrolling_node(i, n);
    i.ensure_layout();
    let (x, y) = i.scroll.get(&n).copied().unwrap_or((Au::ZERO, Au::ZERO));
    (px(x), px(y))
}

/// Scrolls an element (or the window) to `x`/`y`, keeping the axis left `None`.
pub(crate) fn scroll_to(i: &mut Inner, n: NodeId, x: Option<f64>, y: Option<f64>) {
    let n = scrolling_node(i, n);
    let (cx, cy) = i.scroll.get(&n).copied().unwrap_or((Au::ZERO, Au::ZERO));
    let nx = x.map(Au::from_f64_px).unwrap_or(cx);
    let ny = y.map(Au::from_f64_px).unwrap_or(cy);
    i.set_scroll(n, nx, ny);
    i.ensure_layout();
}

/// `el.scrollIntoView(alignToTop)`: scrolls the window so the element shows.
pub(crate) fn scroll_into_view(i: &mut Inner, n: NodeId, top: bool, center: bool) {
    let rects = i.rects_of(n);
    let Some(r) = rects.first().copied() else {
        return;
    };
    let vh = Au::from_px_i32(i.viewport.height as i32);
    let (sx, sy) = i.window_scroll();
    let ny = if center {
        r.origin.y - (vh - r.size.height).scale(1, 2)
    } else if top {
        r.origin.y
    } else {
        r.origin.y + r.size.height - vh
    };
    let ny = if !top && !center && r.origin.y >= sy && r.bottom() <= sy + vh {
        sy
    } else {
        ny
    };
    i.set_scroll(Document::ROOT, sx, ny.max(Au::ZERO));
    i.ensure_layout();
}
