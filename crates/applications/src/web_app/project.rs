//! The semantic page of a web application: every element that has an id, in
//! document order, as the `Page` a native application projects. The id is the
//! element's `data-page-id` when it has one (a page name may differ from the control
//! id the scene uses), else its `id`. Headings are headings, buttons act on their own
//! id, links go to their `href`, text controls are inputs with their live value, and
//! `p`, `span`, `label` and elements whose role is `status`, `alert` or `note` are
//! text. A subtree marked `aria-hidden="true"` is left out.

use cw_protocol::{Page, PageAction, PageElement as E};
use cw_web::dom::{Document, NodeId};
use cw_web::paint::semantics::collapse;

use super::runtime::View;

fn label(doc: &Document, node: NodeId) -> String {
    doc.attr(node, "aria-label")
        .map(collapse)
        .unwrap_or_else(|| collapse(&doc.text_content(node)))
}

pub fn project(v: &View<'_>, page: &mut Page) {
    let doc = v.doc;
    let Some(body) = doc.body() else {
        return;
    };
    let mut stack = vec![body];
    let mut order = Vec::new();
    // Document order, pruning hidden subtrees.
    while let Some(node) = stack.pop() {
        if doc.attr(node, "aria-hidden") == Some("true") {
            continue;
        }
        order.push(node);
        let children: Vec<NodeId> = doc.element_children(node).collect();
        stack.extend(children.into_iter().rev());
    }
    for node in order {
        let Some(tag) = doc.tag(node) else {
            continue;
        };
        let Some(id) = doc
            .attr(node, "data-page-id")
            .or_else(|| doc.attr(node, "id"))
            .filter(|id| !id.trim().is_empty())
        else {
            continue;
        };
        let control = doc.attr(node, "id").unwrap_or(id);
        let role = doc.attr(node, "role").unwrap_or("");
        let element = match tag {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => E::Heading {
                id: id.into(),
                text: collapse(&doc.text_content(node)),
                level: tag[1..].parse().unwrap_or(2),
            },
            "button" => E::Button {
                id: id.into(),
                text: label(doc, node),
                action: PageAction {
                    method: "APP".into(),
                    url: control.into(),
                    fields: Default::default(),
                },
                style: None,
            },
            "a" if doc.has_attr(node, "href") => E::Link {
                id: id.into(),
                text: label(doc, node),
                url: doc.attr(node, "href").unwrap_or_default().into(),
                style: None,
            },
            "input" | "textarea" | "select" => E::Input {
                id: id.into(),
                label: doc.attr(node, "aria-label").unwrap_or_default().into(),
                value: v.values.get(&node).cloned().unwrap_or_else(|| {
                    if tag == "textarea" {
                        doc.text_content(node)
                    } else {
                        doc.attr(node, "value").unwrap_or_default().into()
                    }
                }),
                placeholder: doc.attr(node, "placeholder").unwrap_or_default().into(),
            },
            _ if matches!(tag, "p" | "span" | "label")
                || matches!(role, "status" | "alert" | "note") =>
            {
                E::Text {
                    id: id.into(),
                    text: collapse(&doc.text_content(node)),
                }
            }
            _ => continue,
        };
        page.elements.push(element);
    }
}
