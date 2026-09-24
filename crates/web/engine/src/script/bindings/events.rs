//! `Realm::dispatch`: the browser's `UiEvent`s become the DOM event sequences
//! (through the prelude's dispatch hooks) with the interaction state updated here
//! (hover, active, focus, form control edits) and the default action reported.

use cw_jsvm::value::Value;

use super::dom::wrap_node;
use crate::dom::{Document, NodeId};
use crate::geom::Au;
use crate::script::{DefaultAction, Modifiers, Realm, UiEvent};

fn mods_value(realm: &mut Realm, m: Modifiers) -> Value {
    let vm = realm.vm();
    let o = cw_jsvm::builtins::new_obj_from(
        vm,
        vec![
            ("ctrlKey", Value::Bool(m.ctrl)),
            ("shiftKey", Value::Bool(m.shift)),
            ("altKey", Value::Bool(m.alt)),
            ("metaKey", Value::Bool(m.meta)),
        ],
    );
    Value::Obj(o)
}

fn wrap(realm: &mut Realm, n: Option<NodeId>) -> Value {
    match n {
        Some(n) => wrap_node(realm.vm(), n),
        None => Value::Null,
    }
}

/// Calls a hook and reads a boolean "default prevented" result.
fn hook_bool(realm: &mut Realm, name: &str, args: Vec<Value>) -> bool {
    realm.call_hook(name, args).truthy()
}

fn hit(realm: &mut Realm, x: i32, y: i32) -> Option<NodeId> {
    realm.inner.borrow_mut().element_from_point(x, y)
}

/// The element (or body) that receives an event at a point.
fn target_at(realm: &mut Realm, x: i32, y: i32) -> Option<NodeId> {
    hit(realm, x, y).or_else(|| {
        let i = realm.inner.borrow();
        i.doc.body().or_else(|| i.doc.document_element())
    })
}

/// Updates `:hover` and fires the mouse transition events.
fn update_hover(realm: &mut Realm, target: Option<NodeId>, x: i32, y: i32, m: Modifiers) {
    realm.inner.borrow_mut().pointer = Some((x, y));
    let old = realm.inner.borrow().hovered;
    if old != target {
        {
            let mut i = realm.inner.borrow_mut();
            let mut changed: Vec<NodeId> = Vec::new();
            if let Some(o) = old {
                changed.push(o);
                changed.extend(i.doc.ancestors(o).filter(|a| i.doc.is_element(*a)));
            }
            if let Some(t) = target {
                changed.push(t);
                changed.extend(i.doc.ancestors(t).filter(|a| i.doc.is_element(*a)));
            }
            i.hovered = target;
            for c in changed {
                i.touch_state(c);
            }
        }
        let ov = wrap(realm, old);
        let nv = wrap(realm, target);
        let mv = mods_value(realm, m);
        realm.call_hook(
            "hover",
            vec![
                ov,
                nv,
                Value::Num(x as f64),
                Value::Num(y as f64),
                mv,
                Value::Bool(false),
            ],
        );
    } else if let Some(t) = target {
        let tv = wrap(realm, Some(t));
        let mv = mods_value(realm, m);
        realm.call_hook(
            "pointer",
            vec![
                Value::str("move"),
                tv,
                Value::Num(x as f64),
                Value::Num(y as f64),
                Value::Num(0.0),
                mv,
                Value::Num(0.0),
            ],
        );
    }
}

/// Re-hit-tests the pointer where it last was, after the content under it may
/// have changed (a click that opens an overlay over the button, a list that
/// re-renders): when another element is now under it, `:hover` moves and the
/// boundary events fire (`pointerout`/`mouseout`/`…leave`, then `…over`/
/// `…enter`) with no move events, as Chromium updates hover after a layout.
/// Returns whether the hovered element changed.
pub fn refresh_hover(realm: &mut Realm) -> bool {
    let Some((x, y)) = realm.inner.borrow().pointer else {
        return false;
    };
    if realm.inner.borrow().doc.document_element().is_none() {
        return false;
    }
    let target = target_at(realm, x, y);
    let old = realm.inner.borrow().hovered;
    if old == target {
        return false;
    }
    {
        let mut i = realm.inner.borrow_mut();
        let mut changed: Vec<NodeId> = Vec::new();
        if let Some(o) = old {
            changed.push(o);
            changed.extend(i.doc.ancestors(o).filter(|a| i.doc.is_element(*a)));
        }
        if let Some(t) = target {
            changed.push(t);
            changed.extend(i.doc.ancestors(t).filter(|a| i.doc.is_element(*a)));
        }
        i.hovered = target;
        for c in changed {
            i.touch_state(c);
        }
    }
    let ov = wrap(realm, old);
    let nv = wrap(realm, target);
    let mv = mods_value(realm, Modifiers::default());
    realm.call_hook(
        "hover",
        vec![
            ov,
            nv,
            Value::Num(x as f64),
            Value::Num(y as f64),
            mv,
            Value::Bool(true),
        ],
    );
    true
}

/// Moves focus (with the events), returning whether it changed.
pub fn set_focus(realm: &mut Realm, target: Option<NodeId>, visible: bool) -> bool {
    let old = realm.inner.borrow().focused;
    if old == target {
        return false;
    }
    {
        let mut i = realm.inner.borrow_mut();
        if let Some(o) = old {
            i.touch_state(o);
        }
        if let Some(t) = target {
            i.touch_state(t);
        }
        i.focused = target;
        i.focus_visible = visible;
    }
    let ov = wrap(realm, old);
    let nv = wrap(realm, target);
    realm.call_hook("focusChange", vec![ov, nv]);
    true
}

/// The focusable element for a click on `target` (itself or an ancestor).
fn focus_target(realm: &Realm, target: NodeId) -> Option<NodeId> {
    let i = realm.inner.borrow();
    std::iter::once(target)
        .chain(i.doc.ancestors(target))
        .find(|n| i.is_focusable(*n))
}

/// The activation behaviour of a click on `target`.
fn activate(realm: &mut Realm, target: NodeId, m: Modifiers) -> DefaultAction {
    let chain: Vec<NodeId> = {
        let i = realm.inner.borrow();
        std::iter::once(target)
            .chain(i.doc.ancestors(target))
            .filter(|n| i.doc.is_element(*n))
            .collect()
    };
    for n in chain {
        let (tag, ty, href, disabled) = {
            let i = realm.inner.borrow();
            (
                i.doc.tag(n).unwrap_or("").to_owned(),
                i.doc.attr(n, "type").unwrap_or("").to_ascii_lowercase(),
                i.doc.attr(n, "href").map(|h| i.resolve_url(h)),
                i.is_disabled(n),
            )
        };
        if disabled {
            return DefaultAction::None;
        }
        match tag.as_str() {
            "a" | "area" => {
                if let Some(href) = href {
                    if m.ctrl || m.meta {
                        return DefaultAction::Navigate(href);
                    }
                    // Same-document fragment navigation is the realm's own.
                    let (cur, base_same) = {
                        let i = realm.inner.borrow();
                        let cur = i.url.clone();
                        let same =
                            cur.split('#').next() == href.split('#').next() && href.contains('#');
                        (cur, same)
                    };
                    if base_same {
                        change_hash(realm, &cur, &href, true);
                        return DefaultAction::None;
                    }
                    return DefaultAction::Navigate(href);
                }
            }
            "button" => {
                let form = realm.inner.borrow().form_owner(n);
                if let Some(form) = form {
                    match ty.as_str() {
                        "reset" => return reset_form(realm, form),
                        "button" => return DefaultAction::None,
                        _ => return submit_form(realm, form, Some(n)),
                    }
                }
                return DefaultAction::None;
            }
            "input" => {
                let form = realm.inner.borrow().form_owner(n);
                match ty.as_str() {
                    "submit" | "image" => {
                        if let Some(form) = form {
                            return submit_form(realm, form, Some(n));
                        }
                    }
                    "reset" => {
                        if let Some(form) = form {
                            return reset_form(realm, form);
                        }
                    }
                    "checkbox" | "radio" => {
                        // Already toggled before the click event; fire input/change.
                        let t = wrap(realm, Some(n));
                        realm.call_hook("input", vec![t.clone(), Value::Null, Value::str("")]);
                        realm.call_hook("change", vec![t]);
                        return DefaultAction::Toggle(n);
                    }
                    _ => {}
                }
                return DefaultAction::None;
            }
            "label" => {
                let control = {
                    let i = realm.inner.borrow();
                    match i.doc.attr(n, "for") {
                        Some(id) => i.doc.by_id(id).first().copied(),
                        None => i.doc.descendants(n).find(|c| {
                            *c != n
                                && matches!(
                                    i.doc.tag(*c),
                                    Some("input" | "select" | "textarea" | "button")
                                )
                        }),
                    }
                };
                if let Some(c) = control {
                    if c != target && !realm.inner.borrow().doc.ancestors(target).any(|a| a == c) {
                        return click_node(realm, c, m, 1);
                    }
                }
                return DefaultAction::None;
            }
            "summary" => {
                let details = realm
                    .inner
                    .borrow()
                    .doc
                    .parent(n)
                    .filter(|p| realm.inner.borrow().doc.is(*p, "details"));
                if let Some(d) = details {
                    let open = realm.inner.borrow().doc.has_attr(d, "open");
                    let _ = super::dom::set_attribute_value(
                        realm.vm(),
                        d,
                        "open",
                        if open { None } else { Some("") },
                    );
                    let dv = wrap(realm, Some(d));
                    realm.call_hook("toggle", vec![dv]);
                    return DefaultAction::Toggle(d);
                }
            }
            "option" => {
                let select = realm
                    .inner
                    .borrow()
                    .doc
                    .ancestors(n)
                    .find(|a| realm.inner.borrow().doc.is(*a, "select"));
                if let Some(s) = select {
                    let was = realm.inner.borrow().is_checked(n);
                    let multiple = realm.inner.borrow().doc.has_attr(s, "multiple");
                    realm
                        .inner
                        .borrow_mut()
                        .set_option_selected(n, if multiple { !was } else { true });
                    let sv = wrap(realm, Some(s));
                    realm.call_hook("input", vec![sv.clone(), Value::Null, Value::str("")]);
                    realm.call_hook("change", vec![sv]);
                    return DefaultAction::Toggle(s);
                }
            }
            _ => {}
        }
    }
    DefaultAction::None
}

/// `submit`: validation, the `submit` event, then the encoded data set.
pub fn submit_form(realm: &mut Realm, form: NodeId, submitter: Option<NodeId>) -> DefaultAction {
    let no_validate = {
        let i = realm.inner.borrow();
        i.doc.has_attr(form, "novalidate")
            || submitter
                .map(|s| i.doc.has_attr(s, "formnovalidate"))
                .unwrap_or(false)
    };
    if !no_validate {
        let fv = wrap(realm, Some(form));
        let valid = realm.call_hook("validate", vec![fv]);
        if matches!(valid, Value::Bool(false)) {
            return DefaultAction::Prevented;
        }
    }
    let fv = wrap(realm, Some(form));
    let sv = wrap(realm, submitter);
    let prevented = hook_bool(realm, "submit", vec![fv, sv]);
    if prevented {
        return DefaultAction::Prevented;
    }
    let i = realm.inner.borrow();
    let attr = |name: &str| {
        submitter
            .and_then(|s| i.doc.attr(s, &format!("form{name}")))
            .or_else(|| i.doc.attr(form, name))
            .map(str::to_owned)
    };
    let action = attr("action")
        .map(|a| i.resolve_url(&a))
        .unwrap_or_else(|| i.url.clone());
    let method = attr("method")
        .map(|m| m.to_ascii_lowercase())
        .filter(|m| m == "post" || m == "dialog")
        .unwrap_or_else(|| "get".into());
    let enctype = attr("enctype").unwrap_or_else(|| "application/x-www-form-urlencoded".into());
    let data = i.form_data_set(form, submitter);
    DefaultAction::Submit {
        form,
        action,
        method,
        enctype,
        data,
    }
}

fn reset_form(realm: &mut Realm, form: NodeId) -> DefaultAction {
    let fv = wrap(realm, Some(form));
    let prevented = hook_bool(realm, "reset", vec![fv]);
    if prevented {
        return DefaultAction::Prevented;
    }
    let mut i = realm.inner.borrow_mut();
    let els = i.form_elements(form);
    for e in els {
        i.form.values.remove(&e);
        i.form.checked.remove(&e);
        i.form.indeterminate.remove(&e);
        for o in i.options_of(e) {
            i.form.checked.remove(&o);
        }
        i.touch_state(e);
    }
    DefaultAction::None
}

fn change_hash(realm: &mut Realm, old_url: &str, new_url: &str, push: bool) {
    {
        let mut i = realm.inner.borrow_mut();
        i.url = new_url.to_owned();
        i.doc.url = new_url.to_owned();
        i.target_id = new_url
            .split_once('#')
            .map(|(_, h)| h.to_owned())
            .filter(|h| !h.is_empty());
        if push {
            let idx = i.history_index + 1;
            i.history.truncate(idx);
            i.history.push(crate::script::inner::HistoryEntry {
                url: new_url.to_owned(),
                state: None,
            });
            i.history_index = idx;
        }
        if let Some(root) = i.doc.document_element() {
            i.touch_state(root);
        }
        i.sheet_changed();
    }
    // Scroll the target into view.
    let target = {
        let i = realm.inner.borrow();
        i.target_id
            .clone()
            .and_then(|t| i.doc.by_id(&t).first().copied())
    };
    if let Some(t) = target {
        let mut i = realm.inner.borrow_mut();
        let rects = i.rects_of(t);
        if let Some(r) = rects.first() {
            let y = r.origin.y;
            let (sx, _) = i.window_scroll();
            i.set_scroll(Document::ROOT, sx, y);
        }
    }
    realm.call_hook("hashchange", vec![Value::str(old_url), Value::str(new_url)]);
}

/// The full click sequence on a node: pointer down, focus, pointer up, click,
/// activation.
fn click_node(realm: &mut Realm, target: NodeId, m: Modifiers, detail: u32) -> DefaultAction {
    let (x, y) = {
        let mut i = realm.inner.borrow_mut();
        let rects = i.rects_of(target);
        let (sx, sy) = i.window_scroll();
        match rects.first() {
            Some(r) => (
                (r.origin.x - sx + r.size.width.scale(1, 2)).to_px_round(),
                (r.origin.y - sy + r.size.height.scale(1, 2)).to_px_round(),
            ),
            None => (0, 0),
        }
    };
    click_at(realm, target, x, y, 0, m, detail)
}

fn click_at(
    realm: &mut Realm,
    target: NodeId,
    x: i32,
    y: i32,
    button: u8,
    m: Modifiers,
    detail: u32,
) -> DefaultAction {
    let disabled = realm.inner.borrow().is_disabled(target)
        && matches!(
            realm.inner.borrow().doc.tag(target),
            Some("button" | "input" | "select" | "textarea" | "fieldset" | "option")
        );
    let tv = wrap(realm, Some(target));
    let mv = mods_value(realm, m);
    // Pointer down: `:active`, then focus unless prevented.
    {
        let mut i = realm.inner.borrow_mut();
        i.active = Some(target);
        i.touch_state(target);
    }
    let down_prevented = !disabled
        && hook_bool(
            realm,
            "pointer",
            vec![
                Value::str("down"),
                tv.clone(),
                Value::Num(x as f64),
                Value::Num(y as f64),
                Value::Num(button as f64),
                mv.clone(),
                Value::Num(detail as f64),
            ],
        );
    let mut focused_now = None;
    if !down_prevented && button == 0 {
        let ft = focus_target(realm, target);
        let changed = set_focus(realm, ft, false);
        if changed {
            focused_now = ft;
        }
    }
    {
        let mut i = realm.inner.borrow_mut();
        i.active = None;
        i.touch_state(target);
    }
    if !disabled {
        hook_bool(
            realm,
            "pointer",
            vec![
                Value::str("up"),
                tv.clone(),
                Value::Num(x as f64),
                Value::Num(y as f64),
                Value::Num(button as f64),
                mv.clone(),
                Value::Num(detail as f64),
            ],
        );
    }
    if button == 2 || disabled {
        if !disabled {
            realm.call_hook(
                "contextmenu",
                vec![tv, Value::Num(x as f64), Value::Num(y as f64), mv],
            );
        }
        return match focused_now {
            Some(f) => DefaultAction::Focus(f),
            None => DefaultAction::None,
        };
    }
    if button == 1 {
        realm.call_hook(
            "auxclick",
            vec![tv, Value::Num(x as f64), Value::Num(y as f64), mv],
        );
        return DefaultAction::None;
    }
    // Checkbox/radio pre-activation: toggle before `click`, revert if prevented.
    let (is_check, was) = {
        let i = realm.inner.borrow();
        let ty = i
            .doc
            .attr(target, "type")
            .unwrap_or("")
            .to_ascii_lowercase();
        let is = i.doc.is(target, "input") && (ty == "checkbox" || ty == "radio");
        (is, if is { i.is_checked(target) } else { false })
    };
    if is_check {
        let ty = realm
            .inner
            .borrow()
            .doc
            .attr(target, "type")
            .unwrap_or("")
            .to_ascii_lowercase();
        let new = if ty == "radio" { true } else { !was };
        realm.inner.borrow_mut().set_checked(target, new);
    }
    let prevented = hook_bool(
        realm,
        "click",
        vec![
            tv.clone(),
            Value::Num(x as f64),
            Value::Num(y as f64),
            mv.clone(),
            Value::Num(detail as f64),
        ],
    );
    if prevented {
        if is_check {
            realm.inner.borrow_mut().set_checked(target, was);
        }
        return DefaultAction::Prevented;
    }
    let action = activate(realm, target, m);
    match action {
        DefaultAction::None => match focused_now {
            Some(f) => DefaultAction::Focus(f),
            None => DefaultAction::None,
        },
        other => other,
    }
}

/// A click that is an activation only: the `click` event and the activation
/// behaviour, with no pointer events and no focus change. This is what implicit
/// form submission does to the default button (HTML "fire a click event"), so the
/// caret stays in the field the user pressed Enter in, as in Chromium.
fn activation_click(realm: &mut Realm, target: NodeId, m: Modifiers) -> DefaultAction {
    if realm.inner.borrow().is_disabled(target) {
        return DefaultAction::None;
    }
    let tv = wrap(realm, Some(target));
    let mv = mods_value(realm, m);
    let (x, y) = {
        let mut i = realm.inner.borrow_mut();
        let rects = i.rects_of(target);
        let (sx, sy) = i.window_scroll();
        match rects.first() {
            Some(r) => (
                (r.origin.x - sx + r.size.width.scale(1, 2)).to_px_round(),
                (r.origin.y - sy + r.size.height.scale(1, 2)).to_px_round(),
            ),
            None => (0, 0),
        }
    };
    let prevented = hook_bool(
        realm,
        "click",
        vec![
            tv,
            Value::Num(x as f64),
            Value::Num(y as f64),
            mv,
            Value::Num(0.0),
        ],
    );
    if prevented {
        return DefaultAction::Prevented;
    }
    activate(realm, target, m)
}

/// The key sequence: keydown, keypress, beforeinput, edit, input, keyup.
fn key_press(
    realm: &mut Realm,
    key: &str,
    code: &str,
    m: Modifiers,
    repeat: bool,
    down: bool,
    up: bool,
) -> DefaultAction {
    let target = {
        let i = realm.inner.borrow();
        i.focused
            .or_else(|| i.doc.body())
            .or_else(|| i.doc.document_element())
    };
    let Some(target) = target else {
        return DefaultAction::None;
    };
    let code_s = if code.is_empty() {
        key_code(key)
    } else {
        code.to_owned()
    };
    let tv = wrap(realm, Some(target));
    let mv = mods_value(realm, m);
    let mut action = DefaultAction::None;
    if down {
        let prevented = hook_bool(
            realm,
            "key",
            vec![
                Value::str("keydown"),
                tv.clone(),
                Value::str(key),
                Value::str(&code_s),
                mv.clone(),
                Value::Bool(repeat),
            ],
        );
        if !prevented {
            action = key_default(realm, target, key, m, &tv, &mv, &code_s);
        } else {
            action = DefaultAction::Prevented;
        }
    }
    if up {
        hook_bool(
            realm,
            "key",
            vec![
                Value::str("keyup"),
                tv.clone(),
                Value::str(key),
                Value::str(&code_s),
                mv.clone(),
                Value::Bool(false),
            ],
        );
        if down && key == " " && !matches!(action, DefaultAction::Prevented) {
            let is_button = matches!(realm.inner.borrow().doc.tag(target), Some("button"))
                || matches!(
                    realm.inner.borrow().doc.attr(target, "type"),
                    Some("checkbox" | "radio" | "submit" | "button" | "reset")
                );
            if is_button {
                action = click_node(realm, target, m, 1);
            }
        }
    }
    action
}

fn key_code(key: &str) -> String {
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_alphabetic() => format!("Key{}", c.to_ascii_uppercase()),
        (Some(c), None) if c.is_ascii_digit() => format!("Digit{c}"),
        (Some(' '), None) => "Space".into(),
        _ => key.to_owned(),
    }
}

fn key_default(
    realm: &mut Realm,
    target: NodeId,
    key: &str,
    m: Modifiers,
    tv: &Value,
    mv: &Value,
    code: &str,
) -> DefaultAction {
    let _ = code;
    let printable = key.chars().count() == 1 && !m.ctrl && !m.meta && !m.alt;
    let (is_text, is_textarea, tag, ty) = {
        let i = realm.inner.borrow();
        (
            i.is_text_control(target),
            i.doc.is(target, "textarea"),
            i.doc.tag(target).unwrap_or("").to_owned(),
            i.doc
                .attr(target, "type")
                .unwrap_or("")
                .to_ascii_lowercase(),
        )
    };
    if printable {
        let prevented = hook_bool(
            realm,
            "key",
            vec![
                Value::str("keypress"),
                tv.clone(),
                Value::str(key),
                Value::str(code),
                mv.clone(),
                Value::Bool(false),
            ],
        );
        if prevented {
            return DefaultAction::Prevented;
        }
    }
    if key == "Tab" {
        let order = realm.inner.borrow().focus_order();
        if !order.is_empty() {
            let pos = order.iter().position(|n| *n == target);
            let next = match (pos, m.shift) {
                (Some(p), false) => order.get(p + 1).copied(),
                (Some(p), true) => {
                    if p == 0 {
                        None
                    } else {
                        order.get(p - 1).copied()
                    }
                }
                (None, false) => order.first().copied(),
                (None, true) => order.last().copied(),
            };
            set_focus(realm, next, true);
            return match next {
                Some(n) => DefaultAction::Focus(n),
                None => DefaultAction::None,
            };
        }
        return DefaultAction::None;
    }
    if key == "Enter" && !is_textarea {
        if tag == "button"
            || tag == "a"
            || tag == "summary"
            || (tag == "input"
                && matches!(
                    ty.as_str(),
                    "button" | "submit" | "reset" | "checkbox" | "radio"
                ))
        {
            return click_node(realm, target, m, 1);
        }
        if tag == "input" {
            let form = realm.inner.borrow().form_owner(target);
            if let Some(form) = form {
                let submitter = {
                    let i = realm.inner.borrow();
                    i.form_elements(form).into_iter().find(|e| {
                        (i.doc.is(*e, "button")
                            && !matches!(
                                i.doc
                                    .attr(*e, "type")
                                    .map(|t| t.to_ascii_lowercase())
                                    .as_deref(),
                                Some("button" | "reset")
                            ))
                            || (i.doc.is(*e, "input")
                                && matches!(
                                    i.doc
                                        .attr(*e, "type")
                                        .map(|t| t.to_ascii_lowercase())
                                        .as_deref(),
                                    Some("submit" | "image")
                                ))
                    })
                };
                match submitter {
                    Some(s) => return activation_click(realm, s, m),
                    None => return submit_form(realm, form, None),
                }
            }
        }
        return DefaultAction::None;
    }
    if !is_text {
        return DefaultAction::None;
    }
    let (data, input_type): (Option<String>, &str) = if printable {
        (Some(key.to_owned()), "insertText")
    } else if key == "Enter" && is_textarea {
        (Some("\n".into()), "insertLineBreak")
    } else if key == "Backspace" {
        (None, "deleteContentBackward")
    } else if key == "Delete" {
        (None, "deleteContentForward")
    } else if m.ctrl && key.eq_ignore_ascii_case("a") {
        let len = realm.inner.borrow().control_value(target).chars().count();
        realm
            .inner
            .borrow_mut()
            .form
            .selection
            .insert(target, (0, len));
        return DefaultAction::None;
    } else {
        return DefaultAction::None;
    };
    if realm.inner.borrow().doc.has_attr(target, "readonly") {
        return DefaultAction::None;
    }
    let dv = data.as_ref().map(|d| Value::str(d)).unwrap_or(Value::Null);
    let prevented = hook_bool(
        realm,
        "beforeInput",
        vec![tv.clone(), dv.clone(), Value::str(input_type)],
    );
    if prevented {
        return DefaultAction::Prevented;
    }
    // Apply the edit to the control's value at the selection.
    {
        let mut i = realm.inner.borrow_mut();
        let value = i.control_value(target);
        let chars: Vec<char> = value.chars().collect();
        let (s, e) = i
            .form
            .selection
            .get(&target)
            .copied()
            .unwrap_or((chars.len(), chars.len()));
        let (s, e) = (s.min(chars.len()), e.min(chars.len()));
        let (s, e) = (s.min(e), s.max(e));
        let maxlen: Option<usize> = i
            .doc
            .attr(target, "maxlength")
            .and_then(|v| v.trim().parse().ok());
        let (new_chars, caret): (Vec<char>, usize) = match input_type {
            "insertText" | "insertLineBreak" => {
                let ins: Vec<char> = data.clone().unwrap_or_default().chars().collect();
                if let Some(mx) = maxlen {
                    if chars.len() - (e - s) + ins.len() > mx {
                        return DefaultAction::None;
                    }
                }
                let mut v = chars[..s].to_vec();
                v.extend(ins.iter());
                v.extend(chars[e..].iter());
                (v, s + ins.len())
            }
            "deleteContentBackward" => {
                if s != e {
                    let mut v = chars[..s].to_vec();
                    v.extend(chars[e..].iter());
                    (v, s)
                } else if s > 0 {
                    let mut v = chars[..s - 1].to_vec();
                    v.extend(chars[s..].iter());
                    (v, s - 1)
                } else {
                    return DefaultAction::None;
                }
            }
            _ => {
                if s != e {
                    let mut v = chars[..s].to_vec();
                    v.extend(chars[e..].iter());
                    (v, s)
                } else if e < chars.len() {
                    let mut v = chars[..s].to_vec();
                    v.extend(chars[e + 1..].iter());
                    (v, s)
                } else {
                    return DefaultAction::None;
                }
            }
        };
        let new_value: String = new_chars.into_iter().collect();
        i.form.values.insert(target, new_value);
        i.form.selection.insert(target, (caret, caret));
        i.touch_state(target);
    }
    realm.call_hook("input", vec![tv.clone(), dv, Value::str(input_type)]);
    DefaultAction::None
}

pub fn dispatch(realm: &mut Realm, ev: UiEvent) -> DefaultAction {
    match ev {
        UiEvent::PointerMove { x, y, modifiers } => {
            let t = target_at(realm, x, y);
            update_hover(realm, t, x, y, modifiers);
            DefaultAction::None
        }
        UiEvent::Click {
            x,
            y,
            button,
            modifiers,
            detail,
        } => {
            let Some(t) = target_at(realm, x, y) else {
                return DefaultAction::None;
            };
            update_hover(realm, Some(t), x, y, modifiers);
            click_at(realm, t, x, y, button, modifiers, detail.max(1))
        }
        UiEvent::ClickNode {
            node,
            modifiers,
            detail,
        } => {
            if realm.inner.borrow().doc.node(node).detached && node != Document::ROOT {
                return DefaultAction::None;
            }
            click_node(realm, node, modifiers, detail.max(1))
        }
        UiEvent::PointerDown {
            x,
            y,
            button,
            modifiers,
        } => {
            let Some(t) = target_at(realm, x, y) else {
                return DefaultAction::None;
            };
            update_hover(realm, Some(t), x, y, modifiers);
            {
                let mut i = realm.inner.borrow_mut();
                i.active = Some(t);
                i.touch_state(t);
            }
            let tv = wrap(realm, Some(t));
            let mv = mods_value(realm, modifiers);
            let prevented = hook_bool(
                realm,
                "pointer",
                vec![
                    Value::str("down"),
                    tv,
                    Value::Num(x as f64),
                    Value::Num(y as f64),
                    Value::Num(button as f64),
                    mv,
                    Value::Num(1.0),
                ],
            );
            if prevented {
                return DefaultAction::Prevented;
            }
            let ft = focus_target(realm, t);
            if set_focus(realm, ft, false) {
                if let Some(f) = ft {
                    return DefaultAction::Focus(f);
                }
            }
            DefaultAction::None
        }
        UiEvent::PointerUp {
            x,
            y,
            button,
            modifiers,
        } => {
            let Some(t) = target_at(realm, x, y) else {
                return DefaultAction::None;
            };
            realm.inner.borrow_mut().pointer = Some((x, y));
            let was_active = realm.inner.borrow().active;
            {
                let mut i = realm.inner.borrow_mut();
                i.active = None;
                i.touch_state(t);
            }
            let tv = wrap(realm, Some(t));
            let mv = mods_value(realm, modifiers);
            hook_bool(
                realm,
                "pointer",
                vec![
                    Value::str("up"),
                    tv.clone(),
                    Value::Num(x as f64),
                    Value::Num(y as f64),
                    Value::Num(button as f64),
                    mv.clone(),
                    Value::Num(1.0),
                ],
            );
            if was_active == Some(t) && button == 0 {
                let prevented = hook_bool(
                    realm,
                    "click",
                    vec![
                        tv,
                        Value::Num(x as f64),
                        Value::Num(y as f64),
                        mv,
                        Value::Num(1.0),
                    ],
                );
                if prevented {
                    return DefaultAction::Prevented;
                }
                return activate(realm, t, modifiers);
            }
            DefaultAction::None
        }
        UiEvent::Key {
            key,
            code,
            modifiers,
            repeat,
        } => key_press(realm, &key, &code, modifiers, repeat, true, true),
        UiEvent::KeyHalf {
            key,
            code,
            modifiers,
            down,
        } => key_press(realm, &key, &code, modifiers, false, down, !down),
        UiEvent::TypeText { text } => {
            let mut last = DefaultAction::None;
            for c in text.chars() {
                let key = if c == '\n' {
                    "Enter".to_owned()
                } else {
                    c.to_string()
                };
                last = key_press(realm, &key, "", Modifiers::default(), false, true, true);
            }
            last
        }
        UiEvent::SetValue {
            node,
            value,
            commit,
        } => {
            let is_check = {
                let i = realm.inner.borrow();
                i.doc.is(node, "input")
                    && matches!(
                        i.doc
                            .attr(node, "type")
                            .map(|t| t.to_ascii_lowercase())
                            .as_deref(),
                        Some("checkbox" | "radio")
                    )
            };
            if is_check {
                let on = matches!(value.as_str(), "true" | "on" | "1" | "checked");
                realm.inner.borrow_mut().set_checked(node, on);
            } else if realm.inner.borrow().doc.is(node, "select") {
                let mut i = realm.inner.borrow_mut();
                let options = i.options_of(node);
                let hit = options.iter().copied().find(|o| {
                    i.option_value(*o) == value || i.doc.text_content(*o).trim() == value
                });
                if let Some(h) = hit {
                    i.set_option_selected(h, true);
                }
            } else {
                realm.inner.borrow_mut().set_value(node, &value);
            }
            let tv = wrap(realm, Some(node));
            realm.call_hook(
                "input",
                vec![tv.clone(), Value::string(value), Value::str("insertText")],
            );
            if commit {
                realm.call_hook("change", vec![tv]);
            }
            DefaultAction::None
        }
        UiEvent::Scroll { node, x, y } => {
            let n = node.unwrap_or(Document::ROOT);
            let before = realm.inner.borrow().scroll.get(&n).copied();
            {
                let mut i = realm.inner.borrow_mut();
                i.set_scroll(n, Au::from_px_i32(x), Au::from_px_i32(y));
                i.ensure_layout();
            }
            let after = realm.inner.borrow().scroll.get(&n).copied();
            if before != after {
                let tv = wrap(realm, node);
                realm.call_hook("scroll", vec![tv]);
            }
            DefaultAction::None
        }
        UiEvent::Wheel {
            x,
            y,
            delta_x,
            delta_y,
            modifiers,
        } => {
            let Some(t) = target_at(realm, x, y) else {
                return DefaultAction::None;
            };
            let tv = wrap(realm, Some(t));
            let mv = mods_value(realm, modifiers);
            let prevented = hook_bool(
                realm,
                "wheel",
                vec![
                    tv,
                    Value::Num(x as f64),
                    Value::Num(y as f64),
                    Value::Num(delta_x as f64),
                    Value::Num(delta_y as f64),
                    mv,
                ],
            );
            if prevented {
                return DefaultAction::Prevented;
            }
            // Scroll the nearest scroll container that can move, else the window.
            let container = {
                let mut i = realm.inner.borrow_mut();
                i.ensure_layout();
                let mut cur = Some(t);
                let mut found = None;
                while let Some(c) = cur {
                    if let Some(tree) = i.tree.as_ref() {
                        if let Some((f, _)) = crate::script::inner::fragment_of(tree, c) {
                            if let crate::layout::FragmentKind::Box {
                                scroll: Some(s), ..
                            } = &f.kind
                            {
                                if s.content_height > f.rect.size.height
                                    && c != Document::ROOT
                                    && !i.doc.is(c, "html")
                                {
                                    found = Some(c);
                                    break;
                                }
                            }
                        }
                    }
                    cur = i.doc.parent(c);
                }
                found.unwrap_or(Document::ROOT)
            };
            let before = realm
                .inner
                .borrow()
                .scroll
                .get(&container)
                .copied()
                .unwrap_or((Au::ZERO, Au::ZERO));
            {
                let mut i = realm.inner.borrow_mut();
                i.set_scroll(
                    container,
                    before.0 + Au::from_px_i32(delta_x),
                    before.1 + Au::from_px_i32(delta_y),
                );
                i.ensure_layout();
            }
            let after = realm
                .inner
                .borrow()
                .scroll
                .get(&container)
                .copied()
                .unwrap_or((Au::ZERO, Au::ZERO));
            if before != after {
                let tv = if container == Document::ROOT {
                    Value::Null
                } else {
                    wrap(realm, Some(container))
                };
                realm.call_hook("scroll", vec![tv]);
            }
            DefaultAction::None
        }
        UiEvent::Focus { node } => {
            let ok = node
                .map(|n| realm.inner.borrow().is_focusable(n))
                .unwrap_or(true);
            if !ok {
                return DefaultAction::None;
            }
            if set_focus(realm, node, true) {
                if let Some(n) = node {
                    return DefaultAction::Focus(n);
                }
            }
            DefaultAction::None
        }
        UiEvent::Resize { width, height } => {
            {
                let mut i = realm.inner.borrow_mut();
                i.viewport.width = width;
                i.viewport.height = height;
                i.sheet_changed();
            }
            realm.call_hook("resize", vec![]);
            DefaultAction::None
        }
        UiEvent::HashChange { hash } => {
            let old = realm.inner.borrow().url.clone();
            let base = old.split('#').next().unwrap_or("").to_owned();
            let h = hash.trim_start_matches('#');
            let new = if h.is_empty() {
                base
            } else {
                format!("{base}#{h}")
            };
            if new != old {
                change_hash(realm, &old, &new, true);
            }
            DefaultAction::None
        }
        UiEvent::HistoryGo { delta } => {
            let r = {
                let mut i = realm.inner.borrow_mut();
                let target = i.history_index as i64 + delta as i64;
                if target < 0 || target >= i.history.len() as i64 || delta == 0 {
                    None
                } else {
                    i.history_index = target as usize;
                    let e = i.history[target as usize].clone();
                    Some(e)
                }
            };
            if let Some(e) = r {
                let old = realm.inner.borrow().url.clone();
                {
                    let mut i = realm.inner.borrow_mut();
                    i.url = e.url.clone();
                    i.doc.url = e.url.clone();
                    i.target_id = e
                        .url
                        .split_once('#')
                        .map(|(_, h)| h.to_owned())
                        .filter(|h| !h.is_empty());
                    i.sheet_changed();
                }
                let st = e.state.map(Value::string).unwrap_or(Value::Null);
                realm.call_hook("popstate", vec![st]);
                if old.split('#').next() == e.url.split('#').next() && old != e.url {
                    realm.call_hook("hashchange", vec![Value::string(old), Value::string(e.url)]);
                }
            }
            DefaultAction::None
        }
        UiEvent::Visibility { hidden } => {
            realm.inner.borrow_mut().hidden = hidden;
            realm.call_hook("visibility", vec![]);
            DefaultAction::None
        }
        UiEvent::PageShow => {
            realm.call_hook("pageshow", vec![]);
            DefaultAction::None
        }
        UiEvent::Unload => {
            let r = realm.call_hook("unload", vec![]);
            match r {
                Value::Str(s) if !s.is_empty() => DefaultAction::ConfirmUnload(s.to_string()),
                _ => DefaultAction::None,
            }
        }
    }
}
