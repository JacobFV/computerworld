//! The browser's `UiEvent`s as DOM event sequences, delivered to React-style
//! handlers.
//!
//! The sequencing is the engine's own (`cw_web::script::bindings::events`, which the
//! JS `Realm` uses): hover and `:hover`, pointer down, focus, pointer up, click and
//! activation (form submission, link navigation, checkbox toggling), and key presses
//! that edit a control's value. Where the Realm calls into page script, this module
//! runs React's event system instead: the handlers registered on the target's
//! ancestors, capture then bubble (`onClickCapture`, `onClick`), with React's
//! plugins' mapping (`input` → `onInput` then `onChange` on a text control,
//! `click` → `onChange` on a checkbox, `focusin` → `onFocus`). Each native event's
//! updates are batched and rendered when its dispatch ends; then React restores a
//! controlled input's value, and microtasks run.

use std::cell::Cell;
use std::rc::Rc;

use cw_web::dom::{Document, NodeId};
use cw_web::geom::Au;
use cw_web::script::{DefaultAction, Modifiers, UiEvent};

use crate::runtime::*;
use crate::value::*;

/// What a native event carries into its synthetic events.
#[derive(Clone, Default)]
pub(crate) struct Init {
    pub key: String,
    pub code: String,
    pub mods: Modifiers,
    pub x: f64,
    pub y: f64,
    pub button: f64,
    pub detail: f64,
    pub delta_x: f64,
    pub delta_y: f64,
    pub repeat: bool,
}

fn event_obj(ty: &str, target: NodeId, init: &Init) -> EventObj {
    EventObj {
        ty: Rc::from(ty),
        target,
        current_target: Cell::new(target),
        key: Rc::from(init.key.as_str()),
        code: Rc::from(init.code.as_str()),
        mods: init.mods,
        client_x: init.x,
        client_y: init.y,
        button: init.button,
        detail: init.detail,
        delta_x: init.delta_x,
        delta_y: init.delta_y,
        repeat: init.repeat,
        prevented: Cell::new(false),
        stopped: Cell::new(false),
        extra: Vec::new(),
    }
}

/// The React props a native event dispatches, and whether the event bubbles
/// through React's tree.
fn react_props(ty: &str) -> (&'static [&'static str], bool) {
    match ty {
        "click" => (&["onClick"], true),
        "dblclick" => (&["onDoubleClick"], true),
        "mousedown" => (&["onMouseDown"], true),
        "mouseup" => (&["onMouseUp"], true),
        "mousemove" => (&["onMouseMove"], true),
        "mouseover" => (&["onMouseOver"], true),
        "mouseout" => (&["onMouseOut"], true),
        "mouseenter" => (&["onMouseEnter"], false),
        "mouseleave" => (&["onMouseLeave"], false),
        "pointerdown" => (&["onPointerDown"], true),
        "pointerup" => (&["onPointerUp"], true),
        "pointermove" => (&["onPointerMove"], true),
        "pointerover" => (&["onPointerOver"], true),
        "pointerout" => (&["onPointerOut"], true),
        "pointerenter" => (&["onPointerEnter"], false),
        "pointerleave" => (&["onPointerLeave"], false),
        "contextmenu" => (&["onContextMenu"], true),
        "keydown" => (&["onKeyDown"], true),
        "keyup" => (&["onKeyUp"], true),
        "keypress" => (&["onKeyPress"], true),
        "input" => (&["onInput"], true),
        "change" => (&[], true),
        "submit" => (&["onSubmit"], true),
        "reset" => (&["onReset"], true),
        "focusin" => (&["onFocus"], true),
        "focusout" => (&["onBlur"], true),
        "scroll" => (&["onScroll"], false),
        "wheel" => (&["onWheel"], true),
        _ => (&[], true),
    }
}

impl Runtime {
    fn is_text_like(&self, n: NodeId) -> bool {
        self.inner.is_text_control(n)
    }

    fn is_check(&self, n: NodeId) -> bool {
        self.inner.doc.is(n, "input")
            && matches!(
                self.inner
                    .doc
                    .attr(n, "type")
                    .map(|t| t.to_ascii_lowercase())
                    .as_deref(),
                Some("checkbox" | "radio")
            )
    }

    /// Calls `prop` handlers along `path` (capture handlers first, root to target,
    /// then bubble handlers, target to root). Returns whether a handler called
    /// `preventDefault`, and whether one called `stopPropagation` (which React
    /// passes on to the native event, so `document` and `window` listeners behind
    /// the root do not see it).
    fn dispatch_synthetic(
        &mut self,
        ty: &str,
        prop: &str,
        target: NodeId,
        path: &[NodeId],
        bubbles: bool,
        init: &Init,
    ) -> (bool, bool) {
        let capture = format!("{prop}Capture");
        let has_any = path.iter().any(|n| {
            self.handlers
                .get(n)
                .is_some_and(|hs| hs.iter().any(|(k, _)| &**k == prop || **k == *capture))
        });
        if !has_any {
            return (false, false);
        }
        let ev = Rc::new(event_obj(ty, target, init));
        let mut calls: Vec<(NodeId, Value)> = Vec::new();
        let scope: Vec<NodeId> = if bubbles { path.to_vec() } else { vec![target] };
        for n in scope.iter().rev() {
            if let Some(h) = self
                .handlers
                .get(n)
                .and_then(|hs| hs.iter().find(|(k, _)| **k == *capture))
            {
                calls.push((*n, h.1.clone()));
            }
        }
        let captures = calls.len();
        for n in scope.iter() {
            if let Some(h) = self
                .handlers
                .get(n)
                .and_then(|hs| hs.iter().find(|(k, _)| &**k == prop))
            {
                calls.push((*n, h.1.clone()));
            }
        }
        let _ = captures;
        for (n, f) in calls {
            if ev.stopped.get() {
                break;
            }
            ev.current_target.set(n);
            if let Err(e) = self.call_value(&f, vec![Value::Event(ev.clone())]) {
                self.report(e);
            }
        }
        (ev.prevented.get(), ev.stopped.get())
    }

    /// Native listeners `window` and `document` hold for `ty`, in capture (window
    /// first) or bubble (document first) order. Returns whether one stopped
    /// propagation.
    pub(crate) fn fire_global(&mut self, ty: &str, ev: &Rc<EventObj>, capture: bool) -> bool {
        if self.global_listeners.is_empty() {
            return false;
        }
        let order = if capture {
            [true, false]
        } else {
            [false, true]
        };
        for window in order {
            let calls: Vec<Value> = self
                .global_listeners
                .iter()
                .filter(|l| l.window == window && l.capture == capture && &*l.ty == ty)
                .map(|l| l.f.clone())
                .collect();
            for f in calls {
                if let Err(e) = self.call_value(&f, vec![Value::Event(ev.clone())]) {
                    self.report(e);
                }
                if ev.stopped.get() {
                    return true;
                }
            }
        }
        false
    }

    /// Fires a native event at `target`: React's handlers run, updates render, a
    /// controlled control is restored, microtasks run. Returns whether the default
    /// was prevented.
    pub(crate) fn fire(&mut self, ty: &str, target: NodeId, init: &Init) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        let started = std::time::Instant::now();
        self.fire_depth += 1;
        let prevented = self.fire_inner(ty, target, init);
        self.fire_depth -= 1;
        #[cfg(not(target_arch = "wasm32"))]
        if self.fire_depth == 0 {
            let nanos = started.elapsed().as_nanos() as u64;
            self.stats.script_nanos += nanos;
            self.stats.script_micros = self.stats.script_nanos / 1000;
        }
        prevented
    }

    fn fire_inner(&mut self, ty: &str, target: NodeId, init: &Init) -> bool {
        // React's tree: the target and its ancestors up to (and including) the root
        // container.
        let mut path = vec![target];
        path.extend(
            self.inner
                .doc
                .ancestors(target)
                .filter(|a| self.inner.doc.is_element(*a)),
        );
        let (props, bubbles) = react_props(ty);
        let native = Rc::new(event_obj(ty, target, init));
        let mut stopped = self.fire_global(ty, &native, true);
        let mut prevented = native.prevented.get();
        if !stopped {
            for p in props {
                let (pr, st) = self.dispatch_synthetic(ty, p, target, &path, bubbles, init);
                prevented |= pr;
                stopped |= st;
            }
        }
        // ChangeEventPlugin: `onChange` after the simple event.
        let change = match ty {
            "input" => self.is_text_like(target) || self.inner.doc.is(target, "textarea"),
            "change" => {
                self.inner.doc.is(target, "select")
                    || self.inner.doc.attr(target, "type") == Some("file")
            }
            "click" => self.is_check(target),
            _ => false,
        };
        if change && !stopped {
            let (pr, st) = self.dispatch_synthetic(ty, "onChange", target, &path, true, init);
            prevented |= pr;
            stopped |= st;
        }
        if bubbles && !stopped {
            self.fire_global(ty, &native, false);
        }
        prevented |= native.prevented.get();
        self.flush();
        if change {
            self.restore_controlled(target);
        }
        self.settle();
        prevented
    }

    // ------------------------------------------------------------------ sequencing

    fn target_at(&mut self, x: i32, y: i32) -> Option<NodeId> {
        self.inner.element_from_point(x, y).or_else(|| {
            self.inner
                .doc
                .body()
                .or_else(|| self.inner.doc.document_element())
        })
    }

    fn pointer_init(x: i32, y: i32, button: u8, mods: Modifiers, detail: u32) -> Init {
        Init {
            mods,
            x: x as f64,
            y: y as f64,
            button: button as f64,
            detail: detail as f64,
            ..Init::default()
        }
    }

    fn update_hover(&mut self, target: Option<NodeId>, x: i32, y: i32, m: Modifiers) {
        // `target` was hit-tested just now.
        self.inner.pointer = Some((x, y));
        self.inner.hover_generation = self.inner.generation;
        self.hover_to(target, x, y, m, true);
    }

    /// Re-hit-tests the pointer where it last was, after the content under it may
    /// have changed (a list that re-rendered, an error message that pushed the
    /// button down): when another element is now under it, `:hover` moves and the
    /// boundary events fire with no move events, as Chromium updates hover after a
    /// layout and as the JS `Realm` does. Returns whether the hovered element
    /// changed.
    pub(crate) fn refresh_hover(&mut self) -> bool {
        let Some((x, y)) = self.inner.pointer else {
            return false;
        };
        if self.inner.doc.document_element().is_none()
            || self.inner.hover_generation == self.inner.generation
        {
            return false;
        }
        let target = self.target_at(x, y);
        self.inner.hover_generation = self.inner.generation;
        if self.inner.hovered == target {
            return false;
        }
        self.hover_to(target, x, y, Modifiers::default(), false);
        self.inner.hover_generation = self.inner.generation;
        true
    }

    fn hover_to(&mut self, target: Option<NodeId>, x: i32, y: i32, m: Modifiers, moved: bool) {
        let old = self.inner.hovered;
        let init = Self::pointer_init(x, y, 0, m, 0);
        if old != target {
            let mut changed: Vec<NodeId> = Vec::new();
            for n in [old, target].into_iter().flatten() {
                changed.push(n);
                changed.extend(
                    self.inner
                        .doc
                        .ancestors(n)
                        .filter(|a| self.inner.doc.is_element(*a)),
                );
            }
            self.inner.hovered = target;
            for c in changed {
                self.inner.touch_state(c);
            }
            if !self.handlers.is_empty() {
                let contains = |doc: &Document, a: NodeId, b: NodeId| {
                    a == b || doc.ancestors(b).any(|x| x == a)
                };
                if let Some(o) = old {
                    self.fire("pointerout", o, &init);
                    self.fire("mouseout", o, &init);
                    let mut n = Some(o);
                    while let Some(c) = n {
                        if !self.inner.doc.is_element(c) {
                            break;
                        }
                        if !target.is_some_and(|t| contains(&self.inner.doc, c, t)) {
                            self.fire("pointerleave", c, &init);
                            self.fire("mouseleave", c, &init);
                        }
                        n = self.inner.doc.parent(c);
                    }
                }
                if let Some(t) = target {
                    self.fire("pointerover", t, &init);
                    self.fire("mouseover", t, &init);
                    let mut chain = Vec::new();
                    let mut n = Some(t);
                    while let Some(c) = n {
                        if !self.inner.doc.is_element(c) {
                            break;
                        }
                        if !old.is_some_and(|o| contains(&self.inner.doc, c, o)) {
                            chain.push(c);
                        }
                        n = self.inner.doc.parent(c);
                    }
                    for c in chain.iter().rev() {
                        self.fire("pointerenter", *c, &init);
                        self.fire("mouseenter", *c, &init);
                    }
                    if moved {
                        self.fire("pointermove", t, &init);
                        self.fire("mousemove", t, &init);
                    }
                }
            }
        } else if let (Some(t), true) = (target, moved) {
            if !self.handlers.is_empty() {
                self.fire("pointermove", t, &init);
                self.fire("mousemove", t, &init);
            }
        }
    }

    /// Moves focus, firing `focusout`/`focusin` (`onBlur`/`onFocus`).
    pub(crate) fn set_focus(&mut self, target: Option<NodeId>, visible: bool) -> bool {
        let old = self.inner.focused;
        if old == target {
            return false;
        }
        if let Some(o) = old {
            self.inner.touch_state(o);
        }
        if let Some(t) = target {
            self.inner.touch_state(t);
        }
        self.inner.focused = target;
        self.inner.focus_visible = visible;
        let init = Init::default();
        if let Some(o) = old {
            self.fire("focusout", o, &init);
        }
        if let Some(t) = target {
            self.fire("focusin", t, &init);
        }
        true
    }

    fn focus_target(&self, target: NodeId) -> Option<NodeId> {
        std::iter::once(target)
            .chain(self.inner.doc.ancestors(target))
            .find(|n| self.inner.is_focusable(*n))
    }

    fn activate(&mut self, target: NodeId, m: Modifiers) -> DefaultAction {
        let chain: Vec<NodeId> = std::iter::once(target)
            .chain(self.inner.doc.ancestors(target))
            .filter(|n| self.inner.doc.is_element(*n))
            .collect();
        for n in chain {
            let tag = self.inner.doc.tag(n).unwrap_or("").to_owned();
            let ty = self
                .inner
                .doc
                .attr(n, "type")
                .unwrap_or("")
                .to_ascii_lowercase();
            let href = self
                .inner
                .doc
                .attr(n, "href")
                .map(|h| self.inner.resolve_url(h));
            if self.inner.is_disabled(n) {
                return DefaultAction::None;
            }
            match tag.as_str() {
                "a" | "area" => {
                    if let Some(href) = href {
                        let cur = self.inner.url.clone();
                        let same =
                            cur.split('#').next() == href.split('#').next() && href.contains('#');
                        if same && !(m.ctrl || m.meta) {
                            // Same-document fragment navigation, as the Realm's.
                            self.change_hash(&cur, &href, true);
                            return DefaultAction::None;
                        }
                        return DefaultAction::Navigate(href);
                    }
                }
                "button" => {
                    let form = self.inner.form_owner(n);
                    if let Some(form) = form {
                        match ty.as_str() {
                            "reset" => return self.reset_form(form),
                            "button" => return DefaultAction::None,
                            _ => return self.submit_form(form, Some(n)),
                        }
                    }
                    return DefaultAction::None;
                }
                "input" => {
                    let form = self.inner.form_owner(n);
                    match ty.as_str() {
                        "submit" | "image" => {
                            if let Some(form) = form {
                                return self.submit_form(form, Some(n));
                            }
                        }
                        "reset" => {
                            if let Some(form) = form {
                                return self.reset_form(form);
                            }
                        }
                        "checkbox" | "radio" => {
                            let init = Init::default();
                            self.fire("input", n, &init);
                            self.fire("change", n, &init);
                            return DefaultAction::Toggle(n);
                        }
                        _ => {}
                    }
                    return DefaultAction::None;
                }
                "label" => {
                    let control = match self.inner.doc.attr(n, "for") {
                        Some(id) => self.inner.doc.by_id(id).first().copied(),
                        None => self.inner.doc.descendants(n).find(|c| {
                            *c != n
                                && matches!(
                                    self.inner.doc.tag(*c),
                                    Some("input" | "select" | "textarea" | "button")
                                )
                        }),
                    };
                    if let Some(c) = control {
                        if c != target && !self.inner.doc.ancestors(target).any(|a| a == c) {
                            return self.click_node(c, m, 1);
                        }
                    }
                    return DefaultAction::None;
                }
                "summary" => {
                    if let Some(d) = self
                        .inner
                        .doc
                        .parent(n)
                        .filter(|p| self.inner.doc.is(*p, "details"))
                    {
                        let open = self.inner.doc.has_attr(d, "open");
                        self.write_attr(d, "open", if open { None } else { Some("") });
                        self.inner.touch();
                        return DefaultAction::Toggle(d);
                    }
                }
                "option" => {
                    if let Some(s) = self
                        .inner
                        .doc
                        .ancestors(n)
                        .find(|a| self.inner.doc.is(*a, "select"))
                    {
                        let was = self.inner.is_checked(n);
                        let multiple = self.inner.doc.has_attr(s, "multiple");
                        self.inner
                            .set_option_selected(n, if multiple { !was } else { true });
                        let init = Init::default();
                        self.fire("input", s, &init);
                        self.fire("change", s, &init);
                        return DefaultAction::Toggle(s);
                    }
                }
                _ => {}
            }
        }
        DefaultAction::None
    }

    /// A key on a focused, closed select (a menu list), as Chromium on Linux and
    /// the Realm (`cw_web::script::bindings::events::select_key`) handle it: the
    /// arrows step to the next or previous enabled option, PageUp/PageDown three,
    /// Home/End to the first or last, and a printable key selects by the options'
    /// labels (type-ahead). A change fires `input` then `change` at once. List
    /// boxes (`multiple` or `size` > 1) are left alone.
    fn select_key(&mut self, select: NodeId, key: &str) -> DefaultAction {
        const TYPEAHEAD_TIMEOUT_MS: f64 = 1000.0;
        let (options, current, labels, valid) = {
            let i = &self.inner;
            let size = i
                .doc
                .attr(select, "size")
                .and_then(|v| v.trim().parse::<u32>().ok())
                .unwrap_or(0);
            if i.doc.has_attr(select, "multiple") || size > 1 || i.is_disabled(select) {
                return DefaultAction::None;
            }
            let options = i.options_of(select);
            let current = i
                .selected_options(select)
                .first()
                .and_then(|c| options.iter().position(|o| o == c));
            let valid: Vec<bool> = options.iter().map(|o| !i.is_disabled(*o)).collect();
            let labels: Vec<String> = options
                .iter()
                .map(
                    |o| match i.doc.attr(*o, "label").filter(|l| !l.is_empty()) {
                        Some(l) => l.to_owned(),
                        None => cw_web::script::inner::collapse_ws(&i.doc.text_content(*o)),
                    },
                )
                .collect();
            (options, current, labels, valid)
        };
        let n = options.len() as isize;
        // Blink's `NextValidOption`.
        let next_valid = |from: isize, dir: isize, mut skip: isize| -> Option<usize> {
            let mut good = None;
            let mut at = from + dir;
            while at >= 0 && at < n {
                skip -= 1;
                if valid[at as usize] {
                    good = Some(at as usize);
                    if skip <= 0 {
                        break;
                    }
                }
                at += dir;
            }
            good
        };
        let cur = current.map(|c| c as isize).unwrap_or(-1);
        let mut chars = key.chars();
        let single = match (chars.next(), chars.next()) {
            (Some(c), None) => Some(c),
            _ => None,
        };
        let pick = match key {
            "ArrowDown" | "ArrowRight" => next_valid(cur, 1, 1),
            "ArrowUp" | "ArrowLeft" => next_valid(cur, -1, 1),
            "PageDown" => next_valid(cur, 1, 3),
            "PageUp" => next_valid(cur, -1, 3),
            "Home" => next_valid(-1, 1, 1),
            "End" => next_valid(n, -1, 1),
            _ => match single {
                Some(c) => {
                    let now = self.clock_ms;
                    let ta = self.inner.form.typeahead.entry(select).or_default();
                    let fresh = now - ta.last_ms > TYPEAHEAD_TIMEOUT_MS;
                    if fresh {
                        ta.buffer.clear();
                    }
                    if c == ' ' && ta.buffer.is_empty() {
                        return DefaultAction::None;
                    }
                    ta.last_ms = now;
                    ta.buffer.push(c);
                    let (prefix, offset) = if ta.repeating == Some(c) {
                        (c.to_string(), 1)
                    } else if ta.buffer.chars().count() > 1 {
                        ta.repeating = None;
                        (ta.buffer.clone(), 0)
                    } else {
                        ta.repeating = Some(c);
                        (ta.buffer.clone(), 1)
                    };
                    let prefix = prefix.to_lowercase();
                    if n == 0 {
                        None
                    } else {
                        let start = (current.unwrap_or(0) + offset) % n as usize;
                        (0..n as usize)
                            .map(|k| (start + k) % n as usize)
                            .find(|&k| {
                                valid[k]
                                    && labels[k].trim_start().to_lowercase().starts_with(&prefix)
                            })
                    }
                }
                None => return DefaultAction::None,
            },
        };
        match pick {
            Some(p) if Some(p) != current => {
                self.inner.set_option_selected(options[p], true);
                let init = Init::default();
                self.fire("input", select, &init);
                self.fire("change", select, &init);
                DefaultAction::Toggle(select)
            }
            _ => DefaultAction::None,
        }
    }

    pub(crate) fn submit_form(&mut self, form: NodeId, submitter: Option<NodeId>) -> DefaultAction {
        let prevented = self.fire("submit", form, &Init::default());
        if prevented {
            return DefaultAction::Prevented;
        }
        let i = &self.inner;
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

    fn reset_form(&mut self, form: NodeId) -> DefaultAction {
        if self.fire("reset", form, &Init::default()) {
            return DefaultAction::Prevented;
        }
        let els = self.inner.form_elements(form);
        for e in els {
            self.inner.form.values.remove(&e);
            self.inner.form.checked.remove(&e);
            self.inner.form.indeterminate.remove(&e);
            for o in self.inner.options_of(e) {
                self.inner.form.checked.remove(&o);
            }
            self.inner.touch_state(e);
        }
        DefaultAction::None
    }

    fn centre_of(&mut self, n: NodeId) -> (i32, i32) {
        let rects = self.inner.rects_of(n);
        let (sx, sy) = self.inner.window_scroll();
        match rects.first() {
            Some(r) => (
                (r.origin.x - sx + r.size.width.scale(1, 2)).to_px_round(),
                (r.origin.y - sy + r.size.height.scale(1, 2)).to_px_round(),
            ),
            None => (0, 0),
        }
    }

    fn click_node(&mut self, target: NodeId, m: Modifiers, detail: u32) -> DefaultAction {
        let (x, y) = self.centre_of(target);
        self.click_at(target, x, y, 0, m, detail)
    }

    fn click_at(
        &mut self,
        target: NodeId,
        x: i32,
        y: i32,
        button: u8,
        m: Modifiers,
        detail: u32,
    ) -> DefaultAction {
        let disabled = self.inner.is_disabled(target)
            && matches!(
                self.inner.doc.tag(target),
                Some("button" | "input" | "select" | "textarea" | "fieldset" | "option")
            );
        let init = Self::pointer_init(x, y, button, m, detail);
        self.inner.active = Some(target);
        self.inner.touch_state(target);
        let mut down_prevented = false;
        if !disabled {
            down_prevented = self.fire("pointerdown", target, &init);
            if !down_prevented {
                down_prevented = self.fire("mousedown", target, &init);
            }
        }
        let mut focused_now = None;
        if !down_prevented && button == 0 {
            let ft = self.focus_target(target);
            if self.set_focus(ft, false) {
                focused_now = ft;
            }
        }
        self.inner.active = None;
        self.inner.touch_state(target);
        if !disabled {
            self.fire("pointerup", target, &init);
            self.fire("mouseup", target, &init);
        }
        if button == 2 || disabled {
            if !disabled {
                self.fire("contextmenu", target, &init);
            }
            return match focused_now {
                Some(f) => DefaultAction::Focus(f),
                None => DefaultAction::None,
            };
        }
        if button == 1 {
            return DefaultAction::None;
        }
        let is_check = self.is_check(target);
        let was = is_check && self.inner.is_checked(target);
        if is_check {
            let radio = self
                .inner
                .doc
                .attr(target, "type")
                .map(|t| t.eq_ignore_ascii_case("radio"))
                == Some(true);
            self.inner
                .set_checked(target, if radio { true } else { !was });
        }
        let prevented = self.fire("click", target, &init);
        if detail == 2 {
            self.fire("dblclick", target, &init);
        }
        if prevented {
            if is_check {
                self.inner.set_checked(target, was);
            }
            return DefaultAction::Prevented;
        }
        match self.activate(target, m) {
            DefaultAction::None => match focused_now {
                Some(f) => DefaultAction::Focus(f),
                None => DefaultAction::None,
            },
            other => other,
        }
    }

    /// Implicit submission clicks the default button: the click and its activation
    /// only, no pointer events and no focus change.
    fn activation_click(&mut self, target: NodeId, m: Modifiers) -> DefaultAction {
        if self.inner.is_disabled(target) {
            return DefaultAction::None;
        }
        let (x, y) = self.centre_of(target);
        let init = Self::pointer_init(x, y, 0, m, 0);
        if self.fire("click", target, &init) {
            return DefaultAction::Prevented;
        }
        self.activate(target, m)
    }

    fn key_press(
        &mut self,
        key: &str,
        code: &str,
        m: Modifiers,
        repeat: bool,
        down: bool,
        up: bool,
    ) -> DefaultAction {
        let target = self
            .inner
            .focused
            .or_else(|| self.inner.doc.body())
            .or_else(|| self.inner.doc.document_element());
        let Some(target) = target else {
            return DefaultAction::None;
        };
        let code = if code.is_empty() {
            key_code(key)
        } else {
            code.to_owned()
        };
        let init = Init {
            key: key.to_owned(),
            code: code.clone(),
            mods: m,
            repeat,
            ..Init::default()
        };
        let mut action = DefaultAction::None;
        if down {
            if self.fire("keydown", target, &init) {
                action = DefaultAction::Prevented;
            } else {
                action = self.key_default(target, key, m, &init);
            }
        }
        if up {
            let up_init = Init {
                repeat: false,
                ..init.clone()
            };
            self.fire("keyup", target, &up_init);
            if down && key == " " && !matches!(action, DefaultAction::Prevented) {
                let is_button = self.inner.doc.is(target, "button")
                    || matches!(
                        self.inner.doc.attr(target, "type"),
                        Some("checkbox" | "radio" | "submit" | "button" | "reset")
                    );
                if is_button {
                    action = self.click_node(target, m, 1);
                }
            }
        }
        action
    }

    fn key_default(
        &mut self,
        target: NodeId,
        key: &str,
        m: Modifiers,
        init: &Init,
    ) -> DefaultAction {
        let printable = key.chars().count() == 1 && !m.ctrl && !m.meta && !m.alt;
        let is_text = self.inner.is_text_control(target);
        let is_textarea = self.inner.doc.is(target, "textarea");
        let tag = self.inner.doc.tag(target).unwrap_or("").to_owned();
        let ty = self
            .inner
            .doc
            .attr(target, "type")
            .unwrap_or("")
            .to_ascii_lowercase();
        if printable && self.fire("keypress", target, init) {
            return DefaultAction::Prevented;
        }
        if tag == "select" && !m.ctrl && !m.meta && !m.alt && key != "Tab" {
            return self.select_key(target, key);
        }
        if key == "Tab" {
            let order = self.inner.focus_order();
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
                self.set_focus(next, true);
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
                return self.click_node(target, m, 1);
            }
            if tag == "input" {
                if let Some(form) = self.inner.form_owner(target) {
                    let submitter = {
                        let i = &self.inner;
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
                    return match submitter {
                        Some(s) => self.activation_click(s, m),
                        None => self.submit_form(form, None),
                    };
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
            let len = self.inner.control_value(target).chars().count();
            self.inner.form.selection.insert(target, (0, len));
            return DefaultAction::None;
        } else {
            return DefaultAction::None;
        };
        if self.inner.doc.has_attr(target, "readonly") {
            return DefaultAction::None;
        }
        if self.fire("beforeinput", target, init) {
            return DefaultAction::Prevented;
        }
        let value = self.inner.control_value(target);
        let chars: Vec<char> = value.chars().collect();
        let (s, e) = self
            .inner
            .form
            .selection
            .get(&target)
            .copied()
            .unwrap_or((chars.len(), chars.len()));
        let (s, e) = (s.min(chars.len()), e.min(chars.len()));
        let (s, e) = (s.min(e), s.max(e));
        let maxlen: Option<usize> = self
            .inner
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
        self.inner.form.values.insert(target, new_value);
        self.inner.form.selection.insert(target, (caret, caret));
        self.inner.touch_state(target);
        self.fire("input", target, init);
        DefaultAction::None
    }

    fn scroll_container_at(&mut self, t: NodeId) -> NodeId {
        self.inner.ensure_layout();
        let mut cur = Some(t);
        while let Some(c) = cur {
            if let Some(tree) = self.inner.tree.as_ref() {
                if let Some((f, _)) = cw_web::script::inner::fragment_of(tree, c) {
                    if let cw_web::layout::FragmentKind::Box {
                        scroll: Some(s), ..
                    } = &f.kind
                    {
                        if s.content_height > f.rect.size.height
                            && c != Document::ROOT
                            && !self.inner.doc.is(c, "html")
                        {
                            return c;
                        }
                    }
                }
            }
            cur = self.inner.doc.parent(c);
        }
        Document::ROOT
    }

    pub(crate) fn dispatch_ui(&mut self, ev: UiEvent) -> DefaultAction {
        match ev {
            UiEvent::PointerMove { x, y, modifiers } => {
                let t = self.target_at(x, y);
                self.update_hover(t, x, y, modifiers);
                DefaultAction::None
            }
            UiEvent::Click {
                x,
                y,
                button,
                modifiers,
                detail,
            } => {
                let Some(t) = self.target_at(x, y) else {
                    return DefaultAction::None;
                };
                self.update_hover(Some(t), x, y, modifiers);
                self.click_at(t, x, y, button, modifiers, detail.max(1))
            }
            UiEvent::ClickNode {
                node,
                modifiers,
                detail,
            } => {
                if self.inner.doc.node(node).detached && node != Document::ROOT {
                    return DefaultAction::None;
                }
                self.click_node(node, modifiers, detail.max(1))
            }
            UiEvent::PointerDown {
                x,
                y,
                button,
                modifiers,
            } => {
                let Some(t) = self.target_at(x, y) else {
                    return DefaultAction::None;
                };
                self.update_hover(Some(t), x, y, modifiers);
                self.inner.active = Some(t);
                self.inner.touch_state(t);
                let init = Self::pointer_init(x, y, button, modifiers, 1);
                if self.fire("pointerdown", t, &init) || self.fire("mousedown", t, &init) {
                    return DefaultAction::Prevented;
                }
                let ft = self.focus_target(t);
                if self.set_focus(ft, false) {
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
                let Some(t) = self.target_at(x, y) else {
                    return DefaultAction::None;
                };
                let was_active = self.inner.active;
                self.inner.active = None;
                self.inner.touch_state(t);
                let init = Self::pointer_init(x, y, button, modifiers, 1);
                self.fire("pointerup", t, &init);
                self.fire("mouseup", t, &init);
                if was_active == Some(t) && button == 0 {
                    if self.fire("click", t, &init) {
                        return DefaultAction::Prevented;
                    }
                    return self.activate(t, modifiers);
                }
                DefaultAction::None
            }
            UiEvent::Key {
                key,
                code,
                modifiers,
                repeat,
            } => self.key_press(&key, &code, modifiers, repeat, true, true),
            UiEvent::KeyHalf {
                key,
                code,
                modifiers,
                down,
            } => self.key_press(&key, &code, modifiers, false, down, !down),
            UiEvent::TypeText { text } => {
                let mut last = DefaultAction::None;
                for c in text.chars() {
                    let key = if c == '\n' {
                        "Enter".to_owned()
                    } else {
                        c.to_string()
                    };
                    last = self.key_press(&key, "", Modifiers::default(), false, true, true);
                }
                last
            }
            UiEvent::SetValue {
                node,
                value,
                commit,
            } => {
                if self.is_check(node) {
                    let on = matches!(value.as_str(), "true" | "on" | "1" | "checked");
                    self.inner.set_checked(node, on);
                } else if self.inner.doc.is(node, "select") {
                    let options = self.inner.options_of(node);
                    let hit = options.iter().copied().find(|o| {
                        self.inner.option_value(*o) == value
                            || self.inner.doc.text_content(*o).trim() == value
                    });
                    if let Some(h) = hit {
                        self.inner.set_option_selected(h, true);
                    }
                } else {
                    self.inner.set_value(node, &value);
                }
                self.inner.touch_state(node);
                self.fire("input", node, &Init::default());
                if commit {
                    self.fire("change", node, &Init::default());
                }
                DefaultAction::None
            }
            UiEvent::Scroll { node, x, y } => {
                let n = node.unwrap_or(Document::ROOT);
                let before = self.inner.scroll.get(&n).copied();
                self.inner
                    .set_scroll(n, Au::from_px_i32(x), Au::from_px_i32(y));
                self.inner.ensure_layout();
                let after = self.inner.scroll.get(&n).copied();
                if before != after {
                    if let Some(t) = node {
                        self.fire("scroll", t, &Init::default());
                    }
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
                let Some(t) = self.target_at(x, y) else {
                    return DefaultAction::None;
                };
                let init = Init {
                    delta_x: delta_x as f64,
                    delta_y: delta_y as f64,
                    ..Self::pointer_init(x, y, 0, modifiers, 0)
                };
                if self.fire("wheel", t, &init) {
                    return DefaultAction::Prevented;
                }
                let container = self.scroll_container_at(t);
                let before = self
                    .inner
                    .scroll
                    .get(&container)
                    .copied()
                    .unwrap_or((Au::ZERO, Au::ZERO));
                self.inner.set_scroll(
                    container,
                    before.0 + Au::from_px_i32(delta_x),
                    before.1 + Au::from_px_i32(delta_y),
                );
                self.inner.ensure_layout();
                let after = self
                    .inner
                    .scroll
                    .get(&container)
                    .copied()
                    .unwrap_or((Au::ZERO, Au::ZERO));
                if before != after && container != Document::ROOT {
                    self.fire("scroll", container, &Init::default());
                }
                DefaultAction::None
            }
            UiEvent::Focus { node } => {
                if !node.map(|n| self.inner.is_focusable(n)).unwrap_or(true) {
                    return DefaultAction::None;
                }
                if self.set_focus(node, true) {
                    if let Some(n) = node {
                        return DefaultAction::Focus(n);
                    }
                }
                DefaultAction::None
            }
            UiEvent::Resize { width, height } => {
                self.inner.viewport.width = width;
                self.inner.viewport.height = height;
                self.inner.sheet_changed();
                let target = self.inner.doc.body().unwrap_or(Document::ROOT);
                let ev = Rc::new(event_obj("resize", target, &Init::default()));
                self.fire_global("resize", &ev, false);
                self.flush();
                self.settle();
                DefaultAction::None
            }
            UiEvent::HashChange { hash } => {
                let old = self.inner.url.clone();
                let base = old.split('#').next().unwrap_or("").to_owned();
                let h = hash.trim_start_matches('#');
                let new = if h.is_empty() {
                    base
                } else {
                    format!("{base}#{h}")
                };
                if new != old {
                    self.change_hash(&old, &new, true);
                    self.flush();
                    self.settle();
                }
                DefaultAction::None
            }
            UiEvent::Visibility { hidden } => {
                self.inner.hidden = hidden;
                DefaultAction::None
            }
            UiEvent::HistoryGo { delta } => {
                let before = self.inner.url.clone();
                self.history_traverse(delta as i64, &before);
                self.flush();
                self.settle();
                DefaultAction::None
            }
            UiEvent::PageShow | UiEvent::Unload => DefaultAction::None,
        }
    }
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
