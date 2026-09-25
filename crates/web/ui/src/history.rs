//! Session history and same-document navigation, as the Realm does them
//! (`cw_web::script`'s `06-window.js` location and history, and its
//! `change_hash`): the entries live in the engine state both share
//! (`Inner::history`), a fragment navigation fires `popstate` then `hashchange`
//! at once, `history.go` traverses on a zero-delay timer, and anything that
//! leaves the document goes to the host.

use std::rc::Rc;

use cw_web::dom::Document;
use cw_web::script::inner::HistoryEntry;

use crate::runtime::{Runtime, Timer, R};
use crate::value::{EventObj, Value};

impl Runtime {
    /// The document's URL is `url` (and `:target` follows its fragment).
    pub(crate) fn set_url(&mut self, url: &str) {
        let i = &mut self.inner;
        i.url = url.to_owned();
        i.doc.url = url.to_owned();
        let target = url
            .split_once('#')
            .map(|(_, h)| h.to_owned())
            .filter(|h| !h.is_empty());
        if target != i.target_id {
            i.target_id = target;
            if let Some(root) = i.doc.document_element() {
                i.touch_state(root);
            }
            i.sheet_changed();
        }
    }

    /// A fragment navigation from the page (a link, the address bar's hash):
    /// the Realm's `change_hash`.
    pub(crate) fn change_hash(&mut self, old: &str, new: &str, push: bool) {
        {
            let i = &mut self.inner;
            i.url = new.to_owned();
            i.doc.url = new.to_owned();
            i.target_id = new
                .split_once('#')
                .map(|(_, h)| h.to_owned())
                .filter(|h| !h.is_empty());
            if push {
                let idx = i.history_index + 1;
                i.history.truncate(idx);
                i.history.push(HistoryEntry {
                    url: new.to_owned(),
                    state: None,
                });
                i.history_index = idx;
            }
            if let Some(root) = i.doc.document_element() {
                i.touch_state(root);
            }
            i.sheet_changed();
        }
        let target = {
            let i = &self.inner;
            i.target_id
                .clone()
                .and_then(|t| i.doc.by_id(&t).first().copied())
        };
        if let Some(t) = target {
            let i = &mut self.inner;
            let rects = i.rects_of(t);
            if let Some(r) = rects.first() {
                let y = r.origin.y;
                let (sx, _) = i.window_scroll();
                i.set_scroll(Document::ROOT, sx, y);
            }
        }
        self.fire_popstate(None);
        self.fire_hashchange(old, new);
    }

    /// `location.href = …`, `location.assign(…)`, a part set: the prelude's
    /// `navigateTo`.
    pub(crate) fn navigate_to(&mut self, href: &str) {
        let target = self.inner.resolve_url(href);
        let cur = self.inner.url.clone();
        if same_document(&cur, &target) {
            if target == cur {
                self.scroll_to_fragment();
                return;
            }
            self.history_entry(None, &target, false);
            self.scroll_to_fragment();
            self.fire_popstate(None);
            self.fire_hashchange(&cur, &target);
            return;
        }
        self.inner.host_navigate(&target);
    }

    /// `location.replace(url)`.
    pub(crate) fn location_replace(&mut self, href: &str) {
        let target = self.inner.resolve_url(href);
        let cur = self.inner.url.clone();
        if same_document(&cur, &target) {
            self.history_entry(None, &target, true);
            self.scroll_to_fragment();
            self.fire_popstate(None);
            self.fire_hashchange(&cur, &target);
            return;
        }
        self.inner.host_navigate(&target);
    }

    /// `location[part] = value`.
    pub(crate) fn location_set(&mut self, part: &str, value: &str) {
        if part == "origin" {
            return;
        }
        if part == "href" {
            self.navigate_to(value);
            return;
        }
        let url = self.inner.url.clone();
        let (base, hash) = match url.split_once('#') {
            Some((b, h)) => (b.to_owned(), format!("#{h}")),
            None => (url.clone(), String::new()),
        };
        let (path_part, search) = match base.split_once('?') {
            Some((p, s)) => (p.to_owned(), format!("?{s}")),
            None => (base.clone(), String::new()),
        };
        let with = |prefix: char, v: &str| -> String {
            let v = v.strip_prefix(prefix).unwrap_or(v);
            if v.is_empty() && prefix == '?' {
                String::new()
            } else {
                format!("{prefix}{v}")
            }
        };
        let next = match part {
            // `URL.hash = ''` keeps a bare `#` off; `'#'` too.
            "hash" => format!("{base}{}", with('#', value)),
            "search" => format!("{path_part}{}{hash}", with('?', value)),
            "pathname" => {
                let origin = crate::interp::location_part(&url, "origin");
                let p = if value.starts_with('/') {
                    value.to_owned()
                } else {
                    format!("/{value}")
                };
                format!("{origin}{p}{search}{hash}")
            }
            _ => return,
        };
        self.navigate_to(&next);
    }

    /// `history.pushState` / `replaceState` (a state already as JSON).
    pub(crate) fn history_entry(&mut self, state: Option<String>, url: &str, replace: bool) {
        let url = if url.is_empty() {
            self.inner.url.clone()
        } else {
            self.inner.resolve_url(url)
        };
        let entry = HistoryEntry {
            url: url.clone(),
            state,
        };
        let i = &mut self.inner;
        if replace {
            let idx = i.history_index;
            i.history[idx] = entry;
        } else {
            let idx = i.history_index + 1;
            i.history.truncate(idx);
            i.history.push(entry);
            i.history_index = idx;
        }
        self.set_url(&url);
    }

    /// `history.go(delta)`: a reload for 0, else a traversal on a zero-delay timer.
    pub(crate) fn history_go(&mut self, delta: i64) {
        if delta == 0 {
            let url = self.inner.url.clone();
            self.inner.host_navigate(&url);
            return;
        }
        let before = self.inner.url.clone();
        let id = self.next_timer;
        self.next_timer += 1;
        self.timers.push(Timer {
            id,
            due: self.clock_ms,
            interval: None,
            callback: Value::Native(Rc::new(crate::runtime::NativeFn::Builtin(
                crate::ir::Builtin::HistoryTraverse,
            ))),
            args: vec![Value::Num(delta as f64), Value::str(&before)],
        });
    }

    /// The traversal `history.go` scheduled (`before`: the URL when it was
    /// called), or the browser's back and forward.
    pub(crate) fn history_traverse(&mut self, delta: i64, before: &str) {
        let i = &mut self.inner;
        let target = i.history_index as i64 + delta;
        if target < 0 || target >= i.history.len() as i64 || delta == 0 {
            return;
        }
        i.history_index = target as usize;
        let e = i.history[target as usize].clone();
        self.set_url(&e.url);
        self.fire_popstate(e.state.as_deref());
        if before.split('#').next() == e.url.split('#').next() && before != e.url {
            self.fire_hashchange(before, &e.url);
        }
    }

    /// `history.state`.
    pub(crate) fn history_state(&mut self) -> Value {
        let s = self
            .inner
            .history
            .get(self.inner.history_index)
            .and_then(|e| e.state.clone());
        match s {
            Some(s) => crate::json::parse(&s).unwrap_or(Value::Null),
            None => Value::Null,
        }
    }

    fn scroll_to_fragment(&mut self) {
        let url = self.inner.url.clone();
        let Some((_, h)) = url.split_once('#') else {
            return;
        };
        if h.is_empty() {
            return;
        }
        let id = percent_decode(h);
        let mut el = self.inner.doc.by_id(&id).first().copied();
        if el.is_none() {
            let sel = format!("a[name=\"{}\"]", h.replace('"', "\\\""));
            el = self
                .select(Document::ROOT, &sel, true)
                .ok()
                .and_then(|v| v.first().copied());
        }
        if let Some(el) = el {
            crate::geometry::scroll_into_view(&mut self.inner, el, true, false);
        }
    }

    fn fire_popstate(&mut self, state: Option<&str>) {
        let state = match state {
            Some(s) => crate::json::parse(s).unwrap_or(Value::Null),
            None => Value::Null,
        };
        self.fire_window_event("popstate", vec![(Rc::from("state"), state)]);
    }

    fn fire_hashchange(&mut self, old: &str, new: &str) {
        self.fire_window_event(
            "hashchange",
            vec![
                (Rc::from("oldURL"), Value::str(old)),
                (Rc::from("newURL"), Value::str(new)),
            ],
        );
    }

    fn fire_window_event(&mut self, ty: &str, extra: Vec<(crate::value::Str, Value)>) {
        let target = Document::ROOT;
        let ev = Rc::new(EventObj {
            ty: Rc::from(ty),
            target,
            current_target: std::cell::Cell::new(target),
            key: Rc::from(""),
            code: Rc::from(""),
            mods: Default::default(),
            client_x: 0.0,
            client_y: 0.0,
            button: 0.0,
            detail: 0.0,
            delta_x: 0.0,
            delta_y: 0.0,
            repeat: false,
            prevented: std::cell::Cell::new(false),
            stopped: std::cell::Cell::new(false),
            extra,
        });
        self.fire_global(ty, &ev, false);
    }

    /// Runs the history builtins (`crate::ir::Builtin::History…`, `Location…`).
    pub(crate) fn history_builtin(&mut self, b: crate::ir::Builtin, args: &[Value]) -> R<Value> {
        use crate::ir::Builtin as B;
        let arg = |i: usize| args.get(i).cloned().unwrap_or_default();
        let s = |v: Value| v.to_js_string();
        Ok(match b {
            B::HistoryPush | B::HistoryReplace => {
                let state = arg(0);
                let state = match &state {
                    s if s.is_nullish() => None,
                    Value::Foreign(f) => {
                        let f = f.clone();
                        self.foreign_json(&f)
                    }
                    s => crate::json::stringify(s, &Value::Undefined),
                };
                let url = arg(2);
                let url = if url.is_nullish() {
                    String::new()
                } else {
                    s(url)
                };
                self.history_entry(state, &url, b == B::HistoryReplace);
                Value::Undefined
            }
            B::HistoryGo => {
                let d = arg(0).to_number();
                self.history_go(if d.is_nan() { 0 } else { d as i64 });
                Value::Undefined
            }
            B::HistoryTraverse => {
                let d = arg(0).to_number() as i64;
                let before = s(arg(1));
                self.history_traverse(d, &before);
                Value::Undefined
            }
            B::HistoryLength => Value::Num(self.inner.history.len() as f64),
            B::HistoryState => self.history_state(),
            B::LocationSet => {
                let (part, v) = (s(arg(0)), s(arg(1)));
                self.location_set(&part, &v);
                Value::Undefined
            }
            B::LocationAssign => {
                self.navigate_to(&s(arg(0)));
                Value::Undefined
            }
            B::LocationReplace => {
                self.location_replace(&s(arg(0)));
                Value::Undefined
            }
            B::LocationReload => {
                let url = self.inner.url.clone();
                self.inner.host_navigate(&url);
                Value::Undefined
            }
            _ => Value::Undefined,
        })
    }
}

fn same_document(cur: &str, target: &str) -> bool {
    target.split('#').next() == cur.split('#').next() && (target.contains('#') || cur.contains('#'))
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
