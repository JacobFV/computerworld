//! Templates onto the document, and React DOM's prop semantics: which props become
//! attributes (and how), which are properties (`value`, `checked`), how a style
//! object becomes the `style` attribute, and form controls' mount and update rules
//! (ReactDOMInput/Textarea/Select/Option), so the document comes out attribute for
//! attribute as React 18 leaves it.

use std::rc::Rc;

use cw_web::css::parser::{component_values_to_string, parse_declaration_block};
use cw_web::css::Declaration;
use cw_web::dom::{Attribute, Namespace, NodeId, NodeKind};

use crate::program::{AttrView, TNodeView, TView, TemplateRef};
use crate::runtime::*;
use crate::value::*;

/// Where each hole of `t` lives, computed once.
pub(crate) fn template_info(t: TemplateRef<'_>) -> TemplateInfo {
    let n = t.n_holes();
    let mut info = TemplateInfo {
        sites: vec![HoleSite::Attr; n],
        props: vec![None; n],
        tags: Vec::new(),
    };
    fn walk<N: TNodeView>(n: &N, info: &mut TemplateInfo) -> Option<usize> {
        match n.view() {
            TView::Element {
                tag,
                attrs,
                children,
            } => {
                let idx = info.tags.len();
                info.tags.push(Rc::from(tag));
                for i in 0..attrs.len() {
                    if let AttrView::Dynamic(name, h) = attrs.get(i) {
                        info.props[h as usize] = Some(Rc::from(name));
                    }
                }
                let mut child_idx = Vec::new();
                for c in children {
                    child_idx.push(walk(c, info));
                }
                for (i, c) in children.iter().enumerate() {
                    if let TView::Hole(h) = c.view() {
                        let (next_static, next_hole) = match children.get(i + 1).map(|n| n.view()) {
                            Some(TView::Hole(nh)) => (None, Some(nh)),
                            Some(_) => (child_idx[i + 1], None),
                            None => (None, None),
                        };
                        info.sites[h as usize] = HoleSite::Child {
                            next_static,
                            next_hole,
                        };
                    }
                }
                Some(idx)
            }
            TView::Text(_) => {
                let idx = info.tags.len();
                info.tags.push(Rc::from(""));
                Some(idx)
            }
            TView::Hole(_) => None,
        }
    }
    match t {
        TemplateRef::Ir(t) => walk(&t.root, &mut info),
        TemplateRef::Static(t) => walk(&t.root, &mut info),
    };
    info
}

/// React's unitless CSS properties (numbers are written without `px`).
fn is_unitless(name: &str) -> bool {
    let base = name
        .strip_prefix("Webkit")
        .or_else(|| name.strip_prefix("Moz"))
        .or_else(|| name.strip_prefix("ms"))
        .or_else(|| name.strip_prefix("O"))
        .map(|b| {
            let mut c = b.chars();
            c.next()
                .map(|f| f.to_ascii_lowercase().to_string() + c.as_str())
                .unwrap_or_default()
        });
    let n = base.as_deref().unwrap_or(name);
    matches!(
        n,
        "animationIterationCount"
            | "aspectRatio"
            | "borderImageOutset"
            | "borderImageSlice"
            | "borderImageWidth"
            | "boxFlex"
            | "boxFlexGroup"
            | "boxOrdinalGroup"
            | "columnCount"
            | "columns"
            | "flex"
            | "flexGrow"
            | "flexPositive"
            | "flexShrink"
            | "flexNegative"
            | "flexOrder"
            | "gridArea"
            | "gridRow"
            | "gridRowEnd"
            | "gridRowSpan"
            | "gridRowStart"
            | "gridColumn"
            | "gridColumnEnd"
            | "gridColumnSpan"
            | "gridColumnStart"
            | "fontWeight"
            | "lineClamp"
            | "lineHeight"
            | "opacity"
            | "order"
            | "orphans"
            | "tabSize"
            | "widows"
            | "zIndex"
            | "zoom"
            | "fillOpacity"
            | "floodOpacity"
            | "stopOpacity"
            | "strokeDasharray"
            | "strokeDashoffset"
            | "strokeMiterlimit"
            | "strokeOpacity"
            | "strokeWidth"
    )
}

/// `backgroundColor` → `background-color` (as the engine's CSSOM names them).
fn css_name(prop: &str) -> String {
    if prop == "cssFloat" {
        return "float".into();
    }
    if prop.starts_with("--") || prop.contains('-') {
        return prop.to_owned();
    }
    let mut out = String::with_capacity(prop.len() + 4);
    let mut chars = prop.chars().peekable();
    if let Some(c) = chars.peek().copied() {
        if c.is_ascii_uppercase() {
            out.push('-');
            out.push(c.to_ascii_lowercase());
            chars.next();
        }
    }
    for c in chars {
        if c.is_ascii_uppercase() {
            out.push('-');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// React's `dangerousStyleValue`.
fn style_value(name: &str, v: &Value) -> String {
    match v {
        Value::Undefined | Value::Null | Value::Bool(_) => String::new(),
        Value::Str(s) if s.is_empty() => String::new(),
        Value::Num(n) if *n != 0.0 && !name.starts_with("--") && !is_unitless(name) => {
            format!("{}px", number_to_string(*n))
        }
        other => other.to_js_string().trim().to_owned(),
    }
}

fn settable_property(name: &str) -> bool {
    name.starts_with("--")
        || name.starts_with("-webkit-")
        || name.starts_with("-moz-")
        || name.starts_with("-ms-")
        || cw_web::style::cascade::is_known_property(name)
        || matches!(
            name,
            "fill"
                | "stroke"
                | "stroke-width"
                | "fill-opacity"
                | "stroke-opacity"
                | "stroke-dasharray"
                | "stroke-dashoffset"
                | "stroke-linecap"
                | "stroke-linejoin"
                | "orphans"
                | "all"
        )
}

/// The engine's `CSSStyleDeclaration` setter: validate, then replace in place or
/// append.
fn set_declaration(decls: &mut Vec<Declaration>, name: &str, value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() {
        let before = decls.len();
        decls.retain(|d| d.name != name);
        return decls.len() != before;
    }
    let parsed = parse_declaration_block(&format!("{name}: {value}"));
    let Some(d) = parsed.into_iter().next() else {
        return false;
    };
    if !settable_property(&d.name) {
        return false;
    }
    if cw_web::style::cascade::is_known_property(&d.name)
        && !d.name.starts_with("--")
        && !cw_web::style::cascade::is_supported_declaration(&d.name, &d.value)
    {
        let text = component_values_to_string(&d.value).to_ascii_lowercase();
        if !matches!(text.as_str(), "inherit" | "initial" | "unset" | "revert")
            && !text.contains("var(")
            && !text.starts_with("calc(")
            && !text.starts_with("min(")
            && !text.starts_with("max(")
            && !text.starts_with("clamp(")
        {
            return false;
        }
    }
    match decls.iter_mut().find(|x| x.name == d.name) {
        Some(x) => {
            if x.value == d.value && !x.important {
                return false;
            }
            *x = d;
        }
        None => decls.push(d),
    }
    true
}

fn serialize_declarations(decls: &[Declaration]) -> String {
    let mut out = String::new();
    for d in decls {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&d.name);
        out.push_str(": ");
        out.push_str(&component_values_to_string(&d.value));
        if d.important {
            out.push_str(" !important");
        }
        out.push(';');
    }
    out
}

const BOOLEAN_ATTRS: &[&str] = &[
    "allowFullScreen",
    "async",
    "autoFocus",
    "autoPlay",
    "controls",
    "default",
    "defer",
    "disabled",
    "disablePictureInPicture",
    "disableRemotePlayback",
    "formNoValidate",
    "hidden",
    "loop",
    "noModule",
    "noValidate",
    "open",
    "playsInline",
    "readOnly",
    "required",
    "reversed",
    "scoped",
    "seamless",
    "itemScope",
];

/// The attribute (name, value) React writes for a prop, or the name and `None` to
/// remove it.
fn attribute_for(prop: &str, v: &Value) -> (String, Option<String>) {
    let name = match prop {
        "tabIndex" => "tabindex".to_owned(),
        "crossOrigin" => "crossorigin".to_owned(),
        "contentEditable" => "contenteditable".to_owned(),
        "spellCheck" => "spellcheck".to_owned(),
        "rowSpan" => "rowspan".to_owned(),
        "colSpan" => "colspan".to_owned(),
        n if BOOLEAN_ATTRS.contains(&n) => n.to_ascii_lowercase(),
        n => crate::dom_attr_name(n),
    };
    let value = match v {
        Value::Undefined | Value::Null => None,
        Value::Func(_) | Value::Setter(..) | Value::Dispatch(..) => None,
        _ if BOOLEAN_ATTRS.contains(&prop) => v.truthy().then(String::new),
        Value::Bool(b) => match prop {
            "contentEditable" | "draggable" | "spellCheck" => Some(b.to_string()),
            "capture" | "download" => b.then(String::new),
            p if p.starts_with("data-") || p.starts_with("aria-") => Some(b.to_string()),
            _ => None,
        },
        Value::Num(n)
            if matches!(prop, "cols" | "rows" | "size" | "span") && (n.is_nan() || *n < 1.0) =>
        {
            None
        }
        Value::Num(n) if matches!(prop, "rowSpan" | "start") && n.is_nan() => None,
        other => Some(other.to_js_string()),
    };
    (name, value)
}

fn is_event_prop(name: &str) -> bool {
    name.len() > 2 && name.starts_with("on") && name.as_bytes()[2].is_ascii_uppercase()
}

fn is_form_prop(tag: &str, name: &str) -> bool {
    matches!(
        (tag, name),
        (
            "input",
            "value" | "defaultValue" | "checked" | "defaultChecked"
        ) | ("textarea", "value" | "defaultValue")
            | ("select", "value" | "defaultValue")
            | ("option", "value")
    )
}

impl Runtime {
    pub(crate) fn tag_of(&self, n: NodeId) -> &str {
        self.inner.doc.tag(n).unwrap_or("")
    }

    /// `setAttribute`/`removeAttribute` as the Realm does them: HTML names are
    /// lower-cased, SVG names keep their case.
    pub(crate) fn write_attr(&mut self, n: NodeId, name: &str, value: Option<&str>) {
        let html = matches!(
            self.inner.doc.kind(n),
            NodeKind::Element {
                ns: Namespace::Html,
                ..
            }
        );
        let name = if html {
            name.to_ascii_lowercase()
        } else {
            name.to_owned()
        };
        let old = self.inner.doc.attr(n, &name).map(str::to_owned);
        if old.as_deref() == value {
            return;
        }
        if html || !name.bytes().any(|b| b.is_ascii_uppercase()) {
            match value {
                Some(v) => self.inner.doc.set_attr(n, &name, v),
                None => self.inner.doc.remove_attr(n, &name),
            }
        } else {
            if let NodeKind::Element { attrs, .. } = &mut self.inner.doc.node_mut(n).kind {
                match value {
                    Some(v) => match attrs.iter_mut().find(|x| x.name == name) {
                        Some(x) => x.value = v.to_owned(),
                        None => attrs.push(Attribute {
                            name: name.clone(),
                            value: v.to_owned(),
                        }),
                    },
                    None => attrs.retain(|x| x.name != name),
                }
            }
            self.inner
                .doc
                .mutations
                .push(cw_web::dom::Mutation::AttributeChanged { node: n, name, old });
        }
    }

    fn set_style(&mut self, n: NodeId, old: &Value, new: &Value) {
        let text = self.inner.doc.attr(n, "style").unwrap_or("").to_owned();
        let mut decls = parse_declaration_block(&text);
        let mut changed = false;
        let old_pairs: Vec<(Str, Value)> = match old {
            Value::Object(o) => o.borrow().clone(),
            _ => Vec::new(),
        };
        let new_pairs: Vec<(Str, Value)> = match new {
            Value::Object(o) => o.borrow().clone(),
            _ => Vec::new(),
        };
        // Removed keys first, then changed keys in the new object's order (React's
        // `diffProperties` for `style`).
        for (k, _) in &old_pairs {
            if !new_pairs.iter().any(|(n, _)| n == k) {
                changed |= set_declaration(&mut decls, &css_name(k), "");
            }
        }
        for (k, v) in &new_pairs {
            let prev = old_pairs.iter().find(|(n, _)| n == k).map(|(_, v)| v);
            if prev.is_some_and(|p| same_value(p, v)) {
                continue;
            }
            changed |= set_declaration(&mut decls, &css_name(k), &style_value(k, v));
        }
        if changed {
            let s = serialize_declarations(&decls);
            self.write_attr(n, "style", Some(&s));
        }
    }

    fn set_handler(&mut self, n: NodeId, name: &str, v: &Value) {
        let list = self.handlers.entry(n).or_default();
        list.retain(|(k, _)| &**k != name);
        if !v.is_nullish() {
            list.push((Rc::from(name), v.clone()));
        }
        if list.is_empty() {
            self.handlers.remove(&n);
        }
    }

    /// Applies one prop change to an element. `old` is `Undefined` on mount.
    pub(crate) fn set_prop(
        &mut self,
        n: NodeId,
        name: &str,
        old: &Value,
        new: &Value,
        mounting: bool,
    ) {
        let tag = self.tag_of(n).to_owned();
        match name {
            "children"
            | "key"
            | "ref"
            | "suppressContentEditableWarning"
            | "suppressHydrationWarning" => {}
            "dangerouslySetInnerHTML" => self.set_inner_html(n, old, new, mounting),
            "style" => {
                // A style object the island made: its properties.
                let old = self.plain_object(old).unwrap_or_default();
                let new = self.plain_object(new).unwrap_or_default();
                self.set_style(n, &old, &new)
            }
            "autoFocus" => {
                if mounting && new.truthy() {
                    self.autofocus.push(n);
                }
            }
            _ if is_event_prop(name) => self.set_handler(n, name, new),
            _ if is_form_prop(&tag, name) => {
                self.form_props(n).set(name, new.clone());
                if !mounting {
                    self.update_form_control(n);
                }
            }
            _ => {
                if tag == "input" && name == "type" {
                    self.form_props(n).set(name, new.clone());
                }
                let (attr, value) = attribute_for(name, new);
                self.write_attr(n, &attr, value.as_deref());
            }
        }
    }

    /// `dangerouslySetInnerHTML={{ __html }}`: React DOM sets `innerHTML` when the
    /// markup changed, which the engine's fragment parser parses as the Realm's
    /// `innerHTML` setter does (scripts inside never run).
    fn set_inner_html(&mut self, n: NodeId, old: &Value, new: &Value, mounting: bool) {
        let html_of = |rt: &mut Self, v: &Value| -> Option<String> {
            let v = rt.plain_object(v).unwrap_or_default();
            match &v {
                Value::Object(o) => crate::interp::obj_get(&o.borrow(), "__html")
                    .filter(|h| !h.is_nullish())
                    .map(|h| h.to_js_string()),
                _ => None,
            }
        };
        let new_html = html_of(self, new);
        if !mounting && html_of(self, old) == new_html {
            return;
        }
        let kids: Vec<NodeId> = self.inner.doc.children(n).collect();
        for k in kids {
            self.detach(k);
        }
        if let Some(h) = new_html {
            let nodes = cw_web::html::parse_fragment(&mut self.inner.doc, n, &h);
            for c in nodes {
                let scripts: Vec<NodeId> = self
                    .inner
                    .doc
                    .descendants(c)
                    .filter(|x| self.inner.doc.is(*x, "script"))
                    .collect();
                self.inner.executed_scripts.extend(scripts);
                self.inner.doc.append(n, c);
            }
        }
    }

    fn form_props(&mut self, n: NodeId) -> &mut FormProps {
        self.form_props.entry(n).or_default()
    }

    /// ReactDOMInput/Textarea/Select/Option `postMountWrapper`: after every other
    /// prop was set.
    pub(crate) fn mount_form_control(&mut self, n: NodeId) {
        let tag = self.tag_of(n).to_owned();
        let p = self.form_props.get(&n).cloned().unwrap_or_default();
        match tag.as_str() {
            "input" => {
                let ty = p.ty();
                let initial_checked = match (&p.checked, &p.default_checked) {
                    (c, _) if !c.is_nullish() => c.truthy(),
                    (_, d) => d.truthy(),
                };
                // `checked` is always a host prop of an input (a property).
                self.inner.set_checked(n, initial_checked);
                let has_value = !p.value.is_nullish() || !p.default_value.is_nullish();
                let is_button = ty == "submit" || ty == "reset";
                if has_value && !(is_button && p.value.is_nullish()) {
                    let initial = if !p.value.is_nullish() {
                        &p.value
                    } else {
                        &p.default_value
                    };
                    let s = initial.to_js_string();
                    if self.inner.control_value(n) != s {
                        self.inner.set_value(n, &s);
                    }
                    self.write_attr(n, "value", Some(&s));
                }
                // `defaultChecked = !defaultChecked; defaultChecked = !!initialChecked`.
                self.write_attr(n, "checked", if initial_checked { Some("") } else { None });
                self.track_controlled(n, &tag, &p);
            }
            "textarea" => {
                let initial = if !p.value.is_nullish() {
                    p.value.to_js_string()
                } else if !p.default_value.is_nullish() {
                    p.default_value.to_js_string()
                } else {
                    String::new()
                };
                if !initial.is_empty() {
                    let t = self.inner.doc.create_text(&initial);
                    self.inner.doc.append(n, t);
                    // `node.value = textContent` (ReactDOMTextarea.postMountWrapper).
                    self.inner.set_value(n, &initial);
                }
                self.track_controlled(n, &tag, &p);
            }
            "select" => {
                let multiple = self.inner.doc.has_attr(n, "multiple");
                if !p.value.is_nullish() {
                    self.update_options(n, multiple, &p.value);
                } else if !p.default_value.is_nullish() {
                    self.update_options(n, multiple, &p.default_value);
                }
                self.track_controlled(n, &tag, &p);
            }
            "option" if !p.value.is_nullish() => {
                let s = p.value.to_js_string();
                self.write_attr(n, "value", Some(&s));
            }
            _ => {}
        }
    }

    fn track_controlled(&mut self, n: NodeId, tag: &str, p: &FormProps) {
        if tag == "input" && matches!(p.ty().as_str(), "checkbox" | "radio") {
            if !p.checked.is_nullish() {
                self.controlled
                    .insert(n, Controlled::Checked(p.checked.truthy()));
            } else {
                self.controlled.remove(&n);
            }
        } else if !p.value.is_nullish() {
            self.controlled
                .insert(n, Controlled::Value(p.value.to_js_string()));
        } else {
            self.controlled.remove(&n);
        }
    }

    /// ReactDOMInput/Textarea/Select `updateWrapper`.
    pub(crate) fn update_form_control(&mut self, n: NodeId) {
        let tag = self.tag_of(n).to_owned();
        let p = self.form_props.get(&n).cloned().unwrap_or_default();
        match tag.as_str() {
            "input" => {
                if !p.checked.is_nullish() {
                    self.inner.set_checked(n, p.checked.truthy());
                }
                let ty = p.ty();
                if !p.value.is_nullish() {
                    let s = p.value.to_js_string();
                    let cur = self.inner.control_value(n);
                    let differs = if ty == "number" {
                        (matches!(p.value, Value::Num(z) if z == 0.0) && cur.is_empty())
                            || string_to_number(&cur) != p.value.to_number()
                    } else {
                        cur != s
                    };
                    if differs {
                        self.inner.set_value(n, &s);
                    }
                } else if ty == "submit" || ty == "reset" {
                    self.write_attr(n, "value", None);
                    return;
                }
                let default = if !p.value.is_nullish() {
                    Some(p.value.to_js_string())
                } else if !p.default_value.is_nullish() {
                    Some(p.default_value.to_js_string())
                } else {
                    None
                };
                if let Some(d) = default {
                    if ty != "number" || self.inner.focused != Some(n) {
                        self.write_attr(n, "value", Some(&d));
                    }
                }
                if p.checked.is_nullish() && !p.default_checked.is_nullish() {
                    self.write_attr(
                        n,
                        "checked",
                        if p.default_checked.truthy() {
                            Some("")
                        } else {
                            None
                        },
                    );
                }
                self.track_controlled(n, &tag, &p);
            }
            "textarea" => {
                if !p.value.is_nullish() {
                    let s = p.value.to_js_string();
                    if self.inner.control_value(n) != s {
                        self.inner.set_value(n, &s);
                    }
                    if p.default_value.is_nullish() {
                        self.set_text_content(n, &s);
                    }
                }
                if !p.default_value.is_nullish() {
                    let s = p.default_value.to_js_string();
                    self.set_text_content(n, &s);
                }
                self.track_controlled(n, &tag, &p);
            }
            "select" => {
                if !p.value.is_nullish() {
                    let multiple = self.inner.doc.has_attr(n, "multiple");
                    self.update_options(n, multiple, &p.value);
                }
                self.track_controlled(n, &tag, &p);
            }
            "option" if !p.value.is_nullish() => {
                let s = p.value.to_js_string();
                self.write_attr(n, "value", Some(&s));
            }
            _ => {}
        }
    }

    fn set_text_content(&mut self, n: NodeId, s: &str) {
        if self.inner.doc.text_content(n) == s {
            return;
        }
        let kids: Vec<NodeId> = self.inner.doc.children(n).collect();
        for k in kids {
            self.detach(k);
        }
        if !s.is_empty() {
            let t = self.inner.doc.create_text(s);
            self.inner.doc.append(n, t);
        }
    }

    /// ReactDOMSelect `updateOptions`.
    fn update_options(&mut self, select: NodeId, multiple: bool, value: &Value) {
        let options = self.inner.options_of(select);
        if multiple {
            let wanted: Vec<String> = match value {
                Value::Array(a) => a.borrow().iter().map(|v| v.to_js_string()).collect(),
                v => vec![v.to_js_string()],
            };
            for o in options {
                let v = self.inner.option_value(o);
                let sel = wanted.contains(&v);
                if self.inner.is_checked(o) != sel {
                    self.inner.set_option_selected(o, sel);
                }
            }
            return;
        }
        let want = value.to_js_string();
        let mut first_enabled = None;
        for o in &options {
            if self.inner.option_value(*o) == want {
                self.inner.set_option_selected(*o, true);
                return;
            }
            if first_enabled.is_none() && !self.inner.is_disabled(*o) {
                first_enabled = Some(*o);
            }
        }
        if let Some(o) = first_enabled {
            self.inner.set_option_selected(o, true);
        }
    }

    /// React's `restoreControlledState` for the target of an event.
    pub(crate) fn restore_controlled(&mut self, n: NodeId) {
        match self.controlled.get(&n).cloned() {
            Some(Controlled::Value(v)) => {
                if self.tag_of(n) == "select" {
                    let multiple = self.inner.doc.has_attr(n, "multiple");
                    let p = self.form_props.get(&n).cloned().unwrap_or_default();
                    self.update_options(n, multiple, &p.value);
                } else if self.inner.control_value(n) != v {
                    self.inner.set_value(n, &v);
                    self.inner.touch();
                }
            }
            Some(Controlled::Checked(c)) if self.inner.is_checked(n) != c => {
                self.inner.set_checked(n, c);
                self.inner.touch();
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------------ templates

    /// Creates a template's DOM (detached) and mounts its holes. Returns the root
    /// and the mounted holes.
    pub(crate) fn instantiate(
        &mut self,
        tid: u32,
        values: &[Value],
        parent: NodeId,
    ) -> (NodeId, Vec<MHole>) {
        // React takes the namespace from where the element is mounted: inside an
        // `<svg>` (and not in its `<foreignObject>`) elements are SVG elements.
        let in_svg = matches!(
            self.inner.doc.kind(parent),
            NodeKind::Element { ns: Namespace::Svg, tag, .. } if tag != "foreignObject"
        );
        let program = self.program.clone();
        let info = self.templates[tid as usize].clone();
        let mut created: Vec<NodeId> = Vec::with_capacity(info.tags.len());
        let mut holes: Vec<Option<MHole>> = (0..values.len()).map(|_| None).collect();
        let n = program.templates_len();
        let root = if (tid as usize) < n {
            match program.template(tid) {
                TemplateRef::Ir(t) => {
                    self.build(&t.root, in_svg, &info, values, &mut created, &mut holes)
                }
                TemplateRef::Static(t) => {
                    self.build(&t.root, in_svg, &info, values, &mut created, &mut holes)
                }
            }
        } else {
            let t = self.dyn_templates[tid as usize - n].1.clone();
            self.build(&t.root, in_svg, &info, values, &mut created, &mut holes)
        };
        let holes = holes
            .into_iter()
            .map(|h| h.expect("every hole is placed"))
            .collect();
        (root.expect("a template's root is an element"), holes)
    }

    /// The template of one host element whose props are all dynamic, for an
    /// island's elements of tag `tag` (made once per tag): holes are the props (a
    /// spread), the ref and, but for a void element, the children.
    pub(crate) fn dyn_template(&mut self, tag: &str) -> u32 {
        let n = self.program.templates_len();
        if let Some(i) = self.dyn_templates.iter().position(|(t, _)| t == tag) {
            return (n + i) as u32;
        }
        let void = matches!(
            tag,
            "area"
                | "base"
                | "br"
                | "col"
                | "embed"
                | "hr"
                | "img"
                | "input"
                | "link"
                | "meta"
                | "source"
                | "track"
                | "wbr"
        );
        let hole = |ty| crate::ir::Hole {
            deps: Vec::new(),
            always: true,
            ty,
        };
        let mut holes = vec![hole(crate::ir::Ty::Unknown), hole(crate::ir::Ty::Unknown)];
        let mut children = Vec::new();
        if !void {
            holes.push(hole(crate::ir::Ty::Node));
            children.push(crate::ir::TNode::Hole(2));
        }
        let t = crate::ir::Template {
            root: crate::ir::TNode::Element {
                tag: tag.to_owned(),
                attrs: vec![crate::ir::TAttr::Spread(0), crate::ir::TAttr::Ref(1)],
                children,
            },
            holes,
        };
        self.templates.push(template_info(TemplateRef::Ir(&t)));
        self.dyn_templates.push((tag.to_owned(), t));
        (n + self.dyn_templates.len() - 1) as u32
    }

    /// Template `tid`'s tree (a program's, or one made while running).
    pub(crate) fn template_tree(&self, tid: u32) -> crate::ir::TNode {
        let n = self.program.templates_len();
        if (tid as usize) >= n {
            return self.dyn_templates[tid as usize - n].1.root.clone();
        }
        match self.program.template(tid) {
            TemplateRef::Ir(t) => t.root.clone(),
            TemplateRef::Static(t) => static_to_tnode(&t.root),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn build<N: TNodeView>(
        &mut self,
        n: &N,
        in_svg: bool,
        info: &TemplateInfo,
        values: &[Value],
        created: &mut Vec<NodeId>,
        holes: &mut Vec<Option<MHole>>,
    ) -> Option<NodeId> {
        match n.view() {
            TView::Text(s) => {
                let t = self.inner.doc.create_text(s);
                created.push(t);
                Some(t)
            }
            TView::Hole(_) => None,
            TView::Element {
                tag,
                attrs,
                children,
            } => {
                let svg = in_svg || tag == "svg";
                let el = if svg {
                    self.inner.doc.create(NodeKind::Element {
                        ns: Namespace::Svg,
                        tag: tag.to_owned(),
                        attrs: Vec::new(),
                    })
                } else {
                    self.inner.doc.create_element(tag, Vec::new())
                };
                created.push(el);
                let child_svg = svg && tag != "foreignObject";
                // Children first (React appends all children before it sets props).
                let mut child_nodes: Vec<Option<NodeId>> = Vec::with_capacity(children.len());
                for c in children {
                    let node = self.build(c, child_svg, info, values, created, holes);
                    if let Some(node) = node {
                        self.inner.doc.append(el, node);
                    }
                    child_nodes.push(node);
                }
                // Then the child holes, in order, each before its static successor.
                for (i, c) in children.iter().enumerate() {
                    if let TView::Hole(h) = c.view() {
                        let HoleSite::Child {
                            next_static,
                            next_hole,
                            ..
                        } = info.sites[h as usize]
                        else {
                            unreachable!()
                        };
                        let next_static = next_static.map(|_| child_nodes[i + 1].expect("static"));
                        // Holes mount in order, so everything after this one that is
                        // already in place is static: go before the first of those.
                        let anchor = child_nodes[i + 1..].iter().find_map(|n| *n);
                        let mounted = self.mount_value(&values[h as usize], el, anchor);
                        holes[h as usize] = Some(MHole::Child {
                            parent: el,
                            next_static,
                            next_hole,
                            value: values[h as usize].clone(),
                            mounted,
                        });
                    }
                }
                // Props, in JSX order; a form control's value and checkedness last.
                // React 19 sets an input's `type` after its other props, then its
                // value and checkedness, then its `name` (`initInput`).
                let is_form = matches!(tag, "input" | "textarea" | "select" | "option");
                let late = |name: &str| name == "type" || name == "name";
                let react19 = tag == "input" && !svg && self.program.react_major() >= 19;
                let mut deferred: Vec<(String, Value)> = Vec::new();
                for ai in 0..attrs.len() {
                    match attrs.get(ai) {
                        AttrView::Static(name, value) if react19 && late(name) => {
                            deferred.push((name.to_owned(), Value::str(value)));
                        }
                        AttrView::Static(name, value) => self.write_attr(el, name, Some(value)),
                        AttrView::Dynamic(name, h) => {
                            let v = &values[h as usize];
                            if react19 && late(name) {
                                deferred.push((name.to_owned(), v.clone()));
                            } else {
                                self.set_prop(el, name, &Value::Undefined, v, true);
                            }
                            holes[h as usize] = Some(MHole::Attr {
                                node: el,
                                value: v.clone(),
                            });
                        }
                        AttrView::Spread(h) => {
                            let v = values[h as usize].clone();
                            if react19 {
                                let obj = self.plain_object(&v).unwrap_or_default();
                                if let Value::Object(o) = &obj {
                                    let pairs: Vec<(Str, Value)> = o.borrow().clone();
                                    let (lates, rest): (Vec<_>, Vec<_>) =
                                        pairs.into_iter().partition(|(k, _)| late(k));
                                    for (k, v) in lates {
                                        deferred.retain(|(n, _)| *n != *k);
                                        deferred.push((k.to_string(), v));
                                    }
                                    self.apply_spread(
                                        el,
                                        &Value::Undefined,
                                        &Value::object(rest),
                                        true,
                                    );
                                }
                            } else {
                                self.apply_spread(el, &Value::Undefined, &v, true);
                            }
                            holes[h as usize] = Some(MHole::Spread { node: el, value: v });
                        }
                        AttrView::Ref(h) => {
                            let v = values[h as usize].clone();
                            if !v.is_nullish() {
                                self.ref_attach.push((v.clone(), el));
                            }
                            holes[h as usize] = Some(MHole::Ref { node: el, value: v });
                        }
                    }
                }
                if react19 {
                    deferred.sort_by_key(|(n, _)| n != "type");
                    for (name, v) in deferred.iter().filter(|(n, _)| n == "type") {
                        self.set_prop(el, name, &Value::Undefined, v, true);
                    }
                }
                if is_form {
                    self.mount_form_control(el);
                }
                for (name, v) in deferred.iter().filter(|(n, _)| n == "name") {
                    self.set_prop(el, name, &Value::Undefined, v, true);
                }
                Some(el)
            }
        }
    }

    pub(crate) fn apply_spread(&mut self, n: NodeId, old: &Value, new: &Value, mounting: bool) {
        // Props an island made (`{...getRootProps()}`): its object's properties.
        let (old, new) = (
            self.plain_object(old).unwrap_or_default(),
            self.plain_object(new).unwrap_or_default(),
        );
        let (old, new) = (&old, &new);
        let old_pairs: Vec<(Str, Value)> = match old {
            Value::Object(o) => o.borrow().clone(),
            _ => Vec::new(),
        };
        let new_pairs: Vec<(Str, Value)> = match new {
            Value::Object(o) => o.borrow().clone(),
            _ => Vec::new(),
        };
        for (k, v) in &old_pairs {
            if !new_pairs.iter().any(|(n, _)| n == k) {
                self.set_prop(n, k, v, &Value::Undefined, false);
            }
        }
        for (k, v) in &new_pairs {
            let prev = old_pairs
                .iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.clone())
                .unwrap_or_default();
            if !mounting && same_value(&prev, v) {
                continue;
            }
            self.set_prop(n, k, &prev, v, mounting);
        }
    }
}

/// The props of a form control that React reads in its wrappers.
#[derive(Clone, Debug, Default)]
pub(crate) struct FormProps {
    pub value: Value,
    pub default_value: Value,
    pub checked: Value,
    pub default_checked: Value,
    pub ty: Value,
}

impl FormProps {
    fn set(&mut self, name: &str, v: Value) {
        match name {
            "value" => self.value = v,
            "defaultValue" => self.default_value = v,
            "checked" => self.checked = v,
            "defaultChecked" => self.default_checked = v,
            "type" => self.ty = v,
            _ => {}
        }
    }
    fn ty(&self) -> String {
        if self.ty.is_nullish() {
            "text".into()
        } else {
            self.ty.to_js_string().to_ascii_lowercase()
        }
    }
}

/// A generated program's template node as an IR one.
fn static_to_tnode(n: &crate::program::STNode) -> crate::ir::TNode {
    use crate::program::{STAttr, STNode};
    match n {
        STNode::Text(s) => crate::ir::TNode::Text((*s).to_owned()),
        STNode::Hole(h) => crate::ir::TNode::Hole(*h),
        STNode::Element {
            tag,
            attrs,
            children,
        } => crate::ir::TNode::Element {
            tag: (*tag).to_owned(),
            attrs: attrs
                .iter()
                .map(|a| match a {
                    STAttr::Static(k, v) => {
                        crate::ir::TAttr::Static((*k).to_owned(), (*v).to_owned())
                    }
                    STAttr::Dynamic(k, h) => crate::ir::TAttr::Dynamic((*k).to_owned(), *h),
                    STAttr::Spread(h) => crate::ir::TAttr::Spread(*h),
                    STAttr::Ref(h) => crate::ir::TAttr::Ref(*h),
                })
                .collect(),
            children: children.iter().map(static_to_tnode).collect(),
        },
    }
}
