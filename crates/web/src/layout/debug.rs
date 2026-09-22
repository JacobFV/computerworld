//! A text dump of a fragment tree for tests and the parity harness: one line per
//! fragment, indented by depth, with px-rounded rects and the style source.

use crate::dom::Document;
use crate::layout::fragment::{Fragment, FragmentKind, FragmentTree, Replaced, StyleSource};

/// Dumps the tree; sources are printed as `elem#5`, `before#5`, `anon#5`.
pub fn dump(tree: &FragmentTree) -> String {
    dump_with(None, tree)
}

/// Dumps the tree with element tag names from the document.
pub fn dump_doc(doc: &Document, tree: &FragmentTree) -> String {
    dump_with(Some(doc), tree)
}

fn dump_with(doc: Option<&Document>, tree: &FragmentTree) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "viewport {}x{} content {}x{}\n",
        tree.viewport_width.to_px_round(),
        tree.viewport_height.to_px_round(),
        tree.content_width.to_px_round(),
        tree.content_height.to_px_round()
    ));
    write(doc, &tree.root, 0, &mut out);
    out
}

fn source_name(doc: Option<&Document>, s: StyleSource) -> String {
    let (kind, n) = match s {
        StyleSource::Element(n) => ("elem", n),
        StyleSource::Before(n) => ("before", n),
        StyleSource::After(n) => ("after", n),
        StyleSource::Marker(n) => ("marker", n),
        StyleSource::Anonymous(n) => ("anon", n),
    };
    match doc.and_then(|d| if n.index() < d.len() { d.tag(n) } else { None }) {
        Some(tag) => format!("{kind}#{}({tag})", n.0),
        None => format!("{kind}#{}", n.0),
    }
}

fn write(doc: Option<&Document>, f: &Fragment, depth: usize, out: &mut String) {
    for _ in 0..depth {
        out.push_str("  ");
    }
    let r = f.rect;
    let rect = format!(
        "{},{} {}x{}",
        r.origin.x.to_px_round(),
        r.origin.y.to_px_round(),
        r.size.width.to_px_round(),
        r.size.height.to_px_round()
    );
    match &f.kind {
        FragmentKind::Box {
            source,
            padding,
            border,
            replaced,
            scroll,
            baseline,
        } => {
            out.push_str(&format!("Box {} {rect}", source_name(doc, *source)));
            if padding.horizontal() > crate::geom::Au::ZERO
                || padding.vertical() > crate::geom::Au::ZERO
            {
                out.push_str(&format!(
                    " pad[{} {} {} {}]",
                    padding.top.to_px_round(),
                    padding.right.to_px_round(),
                    padding.bottom.to_px_round(),
                    padding.left.to_px_round()
                ));
            }
            if border.horizontal() > crate::geom::Au::ZERO
                || border.vertical() > crate::geom::Au::ZERO
            {
                out.push_str(&format!(
                    " bdr[{} {} {} {}]",
                    border.top.to_px_round(),
                    border.right.to_px_round(),
                    border.bottom.to_px_round(),
                    border.left.to_px_round()
                ));
            }
            match replaced {
                Some(Replaced::Image { src, .. }) => out.push_str(&format!(" img({src})")),
                Some(Replaced::Control(k)) => out.push_str(&format!(" control({k:?})")),
                Some(Replaced::Placeholder(t)) => out.push_str(&format!(" placeholder({t})")),
                Some(Replaced::Marker(t)) => out.push_str(&format!(" marker({t:?})")),
                None => {}
            }
            if let Some(s) = scroll {
                out.push_str(&format!(
                    " scroll({}x{} @{},{}{}{})",
                    s.content_width.to_px_round(),
                    s.content_height.to_px_round(),
                    s.scroll_x.to_px_round(),
                    s.scroll_y.to_px_round(),
                    if s.shows_x_bar { " xbar" } else { "" },
                    if s.shows_y_bar { " ybar" } else { "" }
                ));
            }
            if let Some(b) = baseline {
                out.push_str(&format!(" bl={}", b.to_px_round()));
            }
            if f.collapsed_borders.is_some() {
                out.push_str(" collapsed");
            }
        }
        FragmentKind::InlineBox {
            source,
            first,
            last,
            ..
        } => {
            out.push_str(&format!("Inline {} {rect}", source_name(doc, *source)));
            if !*first {
                out.push_str(" cont");
            }
            if !*last {
                out.push_str(" open");
            }
        }
        FragmentKind::Text {
            source,
            text,
            baseline,
            ellipsis,
            ..
        } => {
            out.push_str(&format!(
                "Text {} {rect} {text:?} bl={}",
                source_name(doc, *source),
                baseline.to_px_round()
            ));
            if *ellipsis {
                out.push_str(" ellipsis");
            }
        }
        FragmentKind::Line => out.push_str(&format!("Line {rect}")),
    }
    if f.is_float {
        out.push_str(" float");
    }
    if f.is_positioned {
        out.push_str(" positioned");
    }
    if f.z_index != 0 {
        out.push_str(&format!(" z={}", f.z_index));
    }
    if f.establishes_stacking_context {
        out.push_str(" sc");
    }
    let o = f.overflow;
    if o.origin.x.0 != 0 || o.origin.y.0 != 0 || o.size != r.size {
        out.push_str(&format!(
            " overflow[{},{} {}x{}]",
            o.origin.x.to_px_round(),
            o.origin.y.to_px_round(),
            o.size.width.to_px_round(),
            o.size.height.to_px_round()
        ));
    }
    out.push('\n');
    for c in &f.children {
        write(doc, c, depth + 1, out);
    }
}
