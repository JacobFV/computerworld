//! Rendering with React 18's semantics: hooks, batched updates, reconciliation of
//! templates, components, keyed lists and providers onto the document, and the
//! commit phase (layout effects before passive effects, cleanups before creates,
//! deletions parent-first, creates children-first).
//!
//! The update pass starts at the root and descends only into dirty subtrees
//! (`Instance::subtree_dirty`), re-rendering dirty instances where they are, so each
//! knows its DOM parent and insertion point without parent pointers into the DOM.

use std::collections::BTreeMap;
use std::rc::Rc;

use cw_web::dom::NodeId;

use crate::interp::{inspect, Frame};
use crate::ir::{self, ElementExpr, Expr, Hook, Module, Prop};
use crate::runtime::*;
use crate::value::*;

/// Whether skipping holes by the identity of their inputs is sound: nothing mutates
/// values in place, no component re-runs effects on every render, and no function
/// reads state identity cannot see (a ref's `.current`, the clock, randomness, a
/// reassigned module variable).
pub(crate) fn is_pure(m: &Module) -> bool {
    if m.mutates_shared || m.functions.iter().any(|f| f.has_depless_effect) {
        return false;
    }
    let mut pure = true;
    for f in &m.functions {
        let mut check = |e: &Expr| match e {
            Expr::Member(_, name, _) if name == "current" => pure = false,
            // Render-time side effects a skipped render would not repeat.
            Expr::Builtin(
                ir::Builtin::MathRandom
                | ir::Builtin::DateNow
                | ir::Builtin::DocumentTitle
                | ir::Builtin::ConsoleLog
                | ir::Builtin::ConsoleWarn
                | ir::Builtin::ConsoleError,
                _,
            ) => pure = false,
            Expr::Assign(lv, _, _) | Expr::Update(lv, _, _) => {
                if matches!(**lv, ir::LValue::Global(_)) {
                    pure = false;
                }
            }
            _ => {}
        };
        walk_stmts(&f.body, &mut check);
        for p in &f.params {
            walk_pattern(p, &mut check);
        }
    }
    pure
}

fn walk_pattern(p: &ir::Pattern, f: &mut impl FnMut(&Expr)) {
    match p {
        ir::Pattern::Default(inner, d) => {
            walk(d, f);
            walk_pattern(inner, f);
        }
        ir::Pattern::Array { items, rest } => {
            for p in items.iter().flatten() {
                walk_pattern(p, f);
            }
            if let Some(r) = rest {
                walk_pattern(r, f);
            }
        }
        ir::Pattern::Object { props, rest } => {
            for (_, p) in props {
                walk_pattern(p, f);
            }
            if let Some(r) = rest {
                walk_pattern(r, f);
            }
        }
        _ => {}
    }
}

fn walk_stmts(stmts: &[ir::Stmt], f: &mut impl FnMut(&Expr)) {
    use ir::Stmt as S;
    for s in stmts {
        match s {
            S::Let(p, e) => {
                walk_pattern(p, f);
                if let Some(e) = e {
                    walk(e, f);
                }
            }
            S::Expr(e) | S::Throw(e) => walk(e, f),
            S::Return(e) => {
                if let Some(e) = e {
                    walk(e, f);
                }
            }
            S::If(c, a, b) => {
                walk(c, f);
                walk_stmts(a, f);
                walk_stmts(b, f);
            }
            S::ForOf(p, e, b) => {
                walk_pattern(p, f);
                walk(e, f);
                walk_stmts(b, f);
            }
            S::For {
                init,
                test,
                update,
                body,
            } => {
                walk_stmts(init, f);
                if let Some(t) = test {
                    walk(t, f);
                }
                if let Some(u) = update {
                    walk(u, f);
                }
                walk_stmts(body, f);
            }
            S::Switch(d, cases) => {
                walk(d, f);
                for (t, b) in cases {
                    if let Some(t) = t {
                        walk(t, f);
                    }
                    walk_stmts(b, f);
                }
            }
            S::Block(b) => walk_stmts(b, f),
            S::Try {
                block,
                param,
                handler,
                finalizer,
            } => {
                walk_stmts(block, f);
                if let Some(p) = param {
                    walk_pattern(p, f);
                }
                if let Some(h) = handler {
                    walk_stmts(h, f);
                }
                if let Some(x) = finalizer {
                    walk_stmts(x, f);
                }
            }
            S::Break | S::Continue => {}
        }
    }
}

fn walk(e: &Expr, f: &mut impl FnMut(&Expr)) {
    f(e);
    let items = |items: &[ir::ArrayItem], f: &mut dyn FnMut(&Expr)| {
        for i in items {
            match i {
                ir::ArrayItem::Item(e) | ir::ArrayItem::Spread(e) => f(e),
            }
        }
    };
    let props = |ps: &[Prop], f: &mut dyn FnMut(&Expr)| {
        for p in ps {
            match p {
                Prop::KeyValue(_, v) | Prop::Spread(v) => f(v),
                Prop::Computed(k, v) => {
                    f(k);
                    f(v);
                }
            }
        }
    };
    match e {
        Expr::Template(_, es) | Expr::Seq(es) | Expr::Hook(_, es) => {
            for e in es {
                walk(e, f);
            }
        }
        Expr::Array(xs) | Expr::Builtin(_, xs) => items(xs, &mut |e| walk(e, f)),
        Expr::Object(ps) => props(ps, &mut |e| walk(e, f)),
        Expr::Member(o, _, _)
        | Expr::Unary(_, o)
        | Expr::TypeOf(o)
        | Expr::Chain(o)
        | Expr::Await(o) => walk(o, f),
        Expr::Index(o, k, _) => {
            walk(o, f);
            walk(k, f);
        }
        Expr::Call(c, args, _) => {
            walk(c, f);
            items(args, &mut |e| walk(e, f));
        }
        Expr::Method { recv, args, .. } => {
            walk(recv, f);
            items(args, &mut |e| walk(e, f));
        }
        Expr::Binary(_, a, b) | Expr::Logical(_, a, b) => {
            walk(a, f);
            walk(b, f);
        }
        Expr::Cond(a, b, c) => {
            walk(a, f);
            walk(b, f);
            walk(c, f);
        }
        Expr::Assign(lv, _, v) => {
            match &**lv {
                ir::LValue::Member(o, _) => walk(o, f),
                ir::LValue::Index(o, k) => {
                    walk(o, f);
                    walk(k, f);
                }
                _ => {}
            }
            walk(v, f);
        }
        Expr::Update(lv, _, _) => match &**lv {
            ir::LValue::Member(o, _) => walk(o, f),
            ir::LValue::Index(o, k) => {
                walk(o, f);
                walk(k, f);
            }
            _ => {}
        },
        Expr::Element(el) => match &**el {
            ElementExpr::Template { holes, key, .. } => {
                for h in holes {
                    walk(h, f);
                }
                if let Some(k) = key {
                    walk(k, f);
                }
            }
            ElementExpr::Component {
                callee,
                props: ps,
                children,
                key,
            } => {
                walk(callee, f);
                props(ps, &mut |e| walk(e, f));
                if let Some(c) = children {
                    walk(c, f);
                }
                if let Some(k) = key {
                    walk(k, f);
                }
            }
            ElementExpr::Fragment { children, key } => {
                for c in children {
                    walk(c, f);
                }
                if let Some(k) = key {
                    walk(k, f);
                }
            }
            ElementExpr::Provider {
                context,
                value,
                children,
                key,
            } => {
                walk(context, f);
                walk(value, f);
                for c in children {
                    walk(c, f);
                }
                if let Some(k) = key {
                    walk(k, f);
                }
            }
        },
        _ => {}
    }
}

fn deps_changed(old: &Option<Vec<Value>>, new: &Option<Vec<Value>>) -> bool {
    match (old, new) {
        (Some(a), Some(b)) => a.len() != b.len() || a.iter().zip(b).any(|(x, y)| !same_value(x, y)),
        _ => true,
    }
}

fn key_of(v: &Value) -> Option<Str> {
    match v {
        Value::Undefined | Value::Null => None,
        Value::Str(s) => Some(s.clone()),
        other => Some(Rc::from(other.to_js_string().as_str())),
    }
}

/// `useId`'s client format in React 18: `:r<n in base 32>:`.
fn react_id(n: u32) -> String {
    let mut digits = Vec::new();
    let mut v = n;
    loop {
        digits.push(std::char::from_digit(v % 32, 32).unwrap());
        v /= 32;
        if v == 0 {
            break;
        }
    }
    format!(":r{}:", digits.iter().rev().collect::<String>())
}

impl Runtime {
    // ------------------------------------------------------------------ hooks

    fn deps_arg(&mut self, frame: &mut Frame, e: Option<&Expr>) -> R<Option<Vec<Value>>> {
        match e {
            None => Ok(None),
            Some(e) => match self.eval(frame, e)? {
                Value::Array(a) => Ok(Some(a.borrow().clone())),
                Value::Undefined | Value::Null => Ok(None),
                other => type_error(format!(
                    "dependency list {} is not an array",
                    inspect(&other)
                )),
            },
        }
    }

    pub(crate) fn hook(&mut self, frame: &mut Frame, h: Hook, args: &[Expr]) -> R<Value> {
        let Some(ctx) = self.render.last_mut() else {
            return type_error(
                "Invalid hook call: hooks can only be called while rendering a component",
            );
        };
        let inst = ctx.inst;
        let idx = ctx.cursor;
        ctx.cursor += 1;
        let first = self
            .instances
            .get(&inst)
            .map(|i| idx >= i.hooks.len())
            .unwrap_or(true);
        match h {
            Hook::State => {
                let init = match args.first() {
                    Some(e) => self.eval(frame, e)?,
                    None => Value::Undefined,
                };
                if first {
                    let value = if matches!(init, Value::Func(_)) {
                        self.call_value(&init, vec![])?
                    } else {
                        init
                    };
                    self.instance(inst).hooks.push(HookState::State {
                        value: value.clone(),
                        queue: Vec::new(),
                    });
                    return Ok(Value::array(vec![value, Value::Setter(inst, idx as u32)]));
                }
                let (mut value, queue) = match &mut self.instance(inst).hooks[idx] {
                    HookState::State { value, queue } => (value.clone(), std::mem::take(queue)),
                    _ => return type_error("hook order changed between renders"),
                };
                let old = value.clone();
                for u in queue {
                    value = match u {
                        Update::Value(v) => v,
                        Update::Fn(f) => {
                            if matches!(f, Value::Func(_)) {
                                self.call_value(&f, vec![value])?
                            } else {
                                f
                            }
                        }
                    };
                }
                if !same_value(&old, &value) {
                    if let Some(c) = self.render.last_mut() {
                        c.state_changed = true;
                    }
                }
                if let HookState::State { value: v, .. } = &mut self.instance(inst).hooks[idx] {
                    *v = value.clone();
                }
                Ok(Value::array(vec![value, Value::Setter(inst, idx as u32)]))
            }
            Hook::Reducer => {
                let reducer = match args.first() {
                    Some(e) => self.eval(frame, e)?,
                    None => Value::Undefined,
                };
                let init = match args.get(1) {
                    Some(e) => self.eval(frame, e)?,
                    None => Value::Undefined,
                };
                if first {
                    let value = match args.get(2) {
                        Some(e) => {
                            let f = self.eval(frame, e)?;
                            self.call_value(&f, vec![init])?
                        }
                        None => init,
                    };
                    self.instance(inst).hooks.push(HookState::Reducer {
                        value: value.clone(),
                        reducer,
                        queue: Vec::new(),
                    });
                    return Ok(Value::array(vec![value, Value::Dispatch(inst, idx as u32)]));
                }
                let (mut value, queue) = match &mut self.instance(inst).hooks[idx] {
                    HookState::Reducer { value, queue, .. } => {
                        (value.clone(), std::mem::take(queue))
                    }
                    _ => return type_error("hook order changed between renders"),
                };
                let old = value.clone();
                for action in queue {
                    value = self.call_value(&reducer, vec![value, action])?;
                }
                if !same_value(&old, &value) {
                    if let Some(c) = self.render.last_mut() {
                        c.state_changed = true;
                    }
                }
                if let HookState::Reducer {
                    value: v,
                    reducer: r,
                    ..
                } = &mut self.instance(inst).hooks[idx]
                {
                    *v = value.clone();
                    *r = reducer;
                }
                Ok(Value::array(vec![value, Value::Dispatch(inst, idx as u32)]))
            }
            Hook::Memo | Hook::Callback => {
                let f = match args.first() {
                    Some(e) => self.eval(frame, e)?,
                    None => Value::Undefined,
                };
                let deps = self.deps_arg(frame, args.get(1))?;
                if !first {
                    if let HookState::Memo { value, deps: old } = &self.instance(inst).hooks[idx] {
                        if !deps_changed(old, &deps) {
                            return Ok(value.clone());
                        }
                    }
                }
                let value = if h == Hook::Memo {
                    self.call_value(&f, vec![])?
                } else {
                    f
                };
                let state = HookState::Memo {
                    value: value.clone(),
                    deps,
                };
                let i = self.instance(inst);
                if first {
                    i.hooks.push(state);
                } else {
                    i.hooks[idx] = state;
                }
                Ok(value)
            }
            Hook::Ref => {
                if first {
                    let init = match args.first() {
                        Some(e) => self.eval(frame, e)?,
                        None => Value::Undefined,
                    };
                    let r = Value::Ref(Rc::new(std::cell::RefCell::new(init)));
                    self.instance(inst).hooks.push(HookState::Ref(r.clone()));
                    return Ok(r);
                }
                match &self.instance(inst).hooks[idx] {
                    HookState::Ref(r) => Ok(r.clone()),
                    _ => type_error("hook order changed between renders"),
                }
            }
            Hook::Effect | Hook::LayoutEffect => {
                let create = match args.first() {
                    Some(e) => self.eval(frame, e)?,
                    None => Value::Undefined,
                };
                let deps = self.deps_arg(frame, args.get(1))?;
                let layout = h == Hook::LayoutEffect;
                if first {
                    self.instance(inst).hooks.push(HookState::Effect {
                        layout,
                        deps,
                        pending: Some(create),
                        cleanup: None,
                    });
                    return Ok(Value::Undefined);
                }
                if let HookState::Effect {
                    deps: old, pending, ..
                } = &mut self.instance(inst).hooks[idx]
                {
                    if deps_changed(old, &deps) {
                        *pending = Some(create);
                        *old = deps;
                    }
                }
                Ok(Value::Undefined)
            }
            Hook::Context => {
                let c = match args.first() {
                    Some(e) => self.eval(frame, e)?,
                    None => Value::Undefined,
                };
                let Value::Context(id) = c else {
                    return type_error("useContext needs a context");
                };
                if first {
                    self.instance(inst).hooks.push(HookState::Context(id));
                }
                Ok(self.context_value(id))
            }
            Hook::SyncExternalStore => {
                let subscribe = match args.first() {
                    Some(e) => self.eval(frame, e)?,
                    None => Value::Undefined,
                };
                let get = match args.get(1) {
                    Some(e) => self.eval(frame, e)?,
                    None => Value::Undefined,
                };
                let value = self.call_value(&get, vec![])?;
                if first {
                    self.instance(inst).hooks.push(HookState::Store {
                        value: value.clone(),
                        get,
                        subscribe,
                        unsubscribe: None,
                        needs_subscribe: true,
                    });
                } else if let HookState::Store {
                    value: v,
                    get: g,
                    subscribe: s,
                    needs_subscribe,
                    ..
                } = &mut self.instance(inst).hooks[idx]
                {
                    if !same_value(s, &subscribe) {
                        *needs_subscribe = true;
                        *s = subscribe;
                    }
                    *g = get;
                    let changed = !same_value(v, &value);
                    *v = value.clone();
                    if changed {
                        if let Some(c) = self.render.last_mut() {
                            c.state_changed = true;
                        }
                    }
                }
                Ok(value)
            }
            Hook::Id => {
                if first {
                    let s: Str = Rc::from(react_id(self.id_counter).as_str());
                    self.id_counter += 1;
                    self.instance(inst).hooks.push(HookState::Id(s.clone()));
                    return Ok(Value::Str(s));
                }
                match &self.instance(inst).hooks[idx] {
                    HookState::Id(s) => Ok(Value::Str(s.clone())),
                    _ => type_error("hook order changed between renders"),
                }
            }
        }
    }

    fn context_value(&self, id: u32) -> Value {
        for (c, v) in self.ctx_stack.iter().rev() {
            if *c == id {
                return v.clone();
            }
        }
        self.ctx_defaults.get(&id).cloned().unwrap_or_default()
    }

    /// A `useSyncExternalStore` subscription's callback: re-render when the
    /// snapshot changed.
    pub(crate) fn store_changed(&mut self, inst: u32, hook: u32) -> R<()> {
        let (get, old) = match self
            .instances
            .get(&inst)
            .and_then(|i| i.hooks.get(hook as usize))
        {
            Some(HookState::Store { get, value, .. }) => (get.clone(), value.clone()),
            _ => return Ok(()),
        };
        let now = self.call_value(&get, vec![])?;
        if !same_value(&now, &old) {
            self.mark_dirty(inst);
        }
        Ok(())
    }

    pub(crate) fn instance(&mut self, id: u32) -> &mut Instance {
        self.instances.get_mut(&id).expect("live instance")
    }

    pub(crate) fn set_state(&mut self, inst: u32, hook: u32, arg: Value) -> R<()> {
        let rendering_this = self.render.last().map(|r| r.inst) == Some(inst);
        let Some(i) = self.instances.get(&inst) else {
            return Ok(());
        };
        let idle = !i.dirty && !rendering_this;
        let (cur, queue_empty) = match i.hooks.get(hook as usize) {
            Some(HookState::State { value, queue }) => (value.clone(), queue.is_empty()),
            _ => return Ok(()),
        };
        let update = if idle && queue_empty {
            // React's eager state: compute now, and skip the render if unchanged.
            let new = if matches!(arg, Value::Func(_)) {
                self.call_value(&arg, vec![cur.clone()])?
            } else {
                arg
            };
            if same_value(&new, &cur) {
                return Ok(());
            }
            Update::Value(new)
        } else {
            Update::Fn(arg)
        };
        let Some(i) = self.instances.get_mut(&inst) else {
            return Ok(());
        };
        if let Some(HookState::State { queue, .. }) = i.hooks.get_mut(hook as usize) {
            queue.push(update);
        }
        if rendering_this {
            if let Some(r) = self.render.last_mut() {
                r.rerender = true;
            }
        } else {
            self.mark_dirty(inst);
        }
        Ok(())
    }

    pub(crate) fn dispatch_action(&mut self, inst: u32, hook: u32, action: Value) -> R<()> {
        let rendering_this = self.render.last().map(|r| r.inst) == Some(inst);
        let Some(i) = self.instances.get_mut(&inst) else {
            return Ok(());
        };
        if let Some(HookState::Reducer { queue, .. }) = i.hooks.get_mut(hook as usize) {
            queue.push(action);
        }
        if rendering_this {
            if let Some(r) = self.render.last_mut() {
                r.rerender = true;
            }
        } else {
            self.mark_dirty(inst);
        }
        Ok(())
    }

    pub(crate) fn mark_dirty(&mut self, inst: u32) {
        let Some(i) = self.instances.get_mut(&inst) else {
            return;
        };
        i.dirty = true;
        let mut p = i.parent;
        while let Some(id) = p {
            let Some(pi) = self.instances.get_mut(&id) else {
                break;
            };
            if pi.subtree_dirty {
                break;
            }
            pi.subtree_dirty = true;
            p = pi.parent;
        }
        self.pending_work = true;
    }

    // ------------------------------------------------------------------ elements

    pub(crate) fn element(&mut self, frame: &mut Frame, el: &ElementExpr) -> R<Value> {
        Ok(match el {
            ElementExpr::Template {
                template,
                holes,
                key,
            } => {
                let key = match key {
                    Some(k) => key_of(&self.eval(frame, k)?),
                    None => None,
                };
                let module = self.module.clone();
                let meta = &module.templates[*template as usize].holes;
                // A component's own frame: skip holes whose inputs are unchanged.
                if let (Some(_), true) = (frame.inst, self.pure_render) {
                    let addr = el as *const ElementExpr as usize;
                    let occ = {
                        let o = frame.occ.entry(addr).or_insert(0);
                        *o += 1;
                        *o
                    };
                    let old = self
                        .render
                        .last_mut()
                        .and_then(|r| r.old_cache.entries.remove(&(addr, occ)));
                    let mut values = Vec::with_capacity(holes.len());
                    let mut deps_out = Vec::with_capacity(holes.len());
                    let mut all_same = old.is_some();
                    for (i, h) in holes.iter().enumerate() {
                        let m = &meta[i];
                        let cur: Vec<Value> = m.deps.iter().map(|d| frame.slot_value(d)).collect();
                        let reuse = match &old {
                            Some(o) if !m.always => {
                                o.deps[i].len() == cur.len()
                                    && o.deps[i].iter().zip(&cur).all(|(a, b)| same_dep(a, b))
                            }
                            _ => false,
                        };
                        if reuse {
                            values.push(old.as_ref().unwrap().holes[i].clone());
                            self.stats.holes_skipped += 1;
                        } else {
                            let v = self.eval(frame, h)?;
                            if let Some(o) = &old {
                                if !same_value(&o.holes[i], &v) {
                                    all_same = false;
                                }
                            }
                            values.push(v);
                            self.stats.holes_evaluated += 1;
                        }
                        deps_out.push(cur);
                    }
                    let elem = match &old {
                        Some(o) if all_same && o.elem.key() == key.as_ref() => {
                            self.stats.elements_reused += 1;
                            o.elem.clone()
                        }
                        _ => Rc::new(Elem::Template {
                            tid: *template,
                            holes: values.clone(),
                            key,
                        }),
                    };
                    if let Some(r) = self.render.last_mut() {
                        r.new_cache.entries.insert(
                            (addr, occ),
                            CacheEntry {
                                deps: deps_out,
                                holes: values,
                                elem: elem.clone(),
                            },
                        );
                    }
                    return Ok(Value::Elem(elem));
                }
                let mut values = Vec::with_capacity(holes.len());
                for h in holes {
                    values.push(self.eval(frame, h)?);
                }
                self.stats.holes_evaluated += holes.len() as u64;
                Value::Elem(Rc::new(Elem::Template {
                    tid: *template,
                    holes: values,
                    key,
                }))
            }
            ElementExpr::Component {
                callee,
                props,
                children,
                key,
            } => {
                let f = self.eval(frame, callee)?;
                let Value::Func(func) = f else {
                    return type_error(format!("element type is invalid: {}", inspect(&f)));
                };
                let mut out: Vec<(Str, Value)> = Vec::with_capacity(props.len() + 1);
                for p in props {
                    match p {
                        Prop::KeyValue(k, v) => {
                            let v = self.eval(frame, v)?;
                            crate::interp::obj_set(&mut out, Rc::from(k.as_str()), v);
                        }
                        Prop::Computed(k, v) => {
                            let k = self.eval(frame, k)?;
                            let v = self.eval(frame, v)?;
                            crate::interp::obj_set(
                                &mut out,
                                Rc::from(k.to_js_string().as_str()),
                                v,
                            );
                        }
                        Prop::Spread(v) => {
                            if let Value::Object(o) = self.eval(frame, v)? {
                                for (k, v) in o.borrow().iter() {
                                    if &**k == "key" || &**k == "ref" {
                                        continue;
                                    }
                                    crate::interp::obj_set(&mut out, k.clone(), v.clone());
                                }
                            }
                        }
                    }
                }
                if let Some(c) = children {
                    let c = self.eval(frame, c)?;
                    crate::interp::obj_set(&mut out, Rc::from("children"), c);
                }
                let key = match key {
                    Some(k) => key_of(&self.eval(frame, k)?),
                    None => None,
                };
                Value::Elem(Rc::new(Elem::Component {
                    func,
                    props: Value::object(out),
                    key,
                }))
            }
            ElementExpr::Fragment { children, key } => {
                let mut out = Vec::with_capacity(children.len());
                for c in children {
                    out.push(self.eval(frame, c)?);
                }
                let key = match key {
                    Some(k) => key_of(&self.eval(frame, k)?),
                    None => None,
                };
                Value::Elem(Rc::new(Elem::Fragment { children: out, key }))
            }
            ElementExpr::Provider {
                context,
                value,
                children,
                key,
            } => {
                let Value::Context(ctx) = self.eval(frame, context)? else {
                    return type_error("Provider of a value that is not a context");
                };
                let value = self.eval(frame, value)?;
                let mut out = Vec::with_capacity(children.len());
                for c in children {
                    out.push(self.eval(frame, c)?);
                }
                let key = match key {
                    Some(k) => key_of(&self.eval(frame, k)?),
                    None => None,
                };
                Value::Elem(Rc::new(Elem::Provider {
                    ctx,
                    value,
                    children: out,
                    key,
                }))
            }
        })
    }

    // ------------------------------------------------------------------ mount / reconcile

    fn insert(&mut self, parent: NodeId, node: NodeId, anchor: Option<NodeId>) {
        let anchor = anchor.filter(|a| self.inner.doc.parent(*a) == Some(parent));
        self.inner.doc.insert_before(parent, node, anchor);
    }

    pub(crate) fn mount_value(
        &mut self,
        v: &Value,
        parent: NodeId,
        anchor: Option<NodeId>,
    ) -> MNode {
        self.reconcile(MNode::Empty, v, parent, anchor)
    }

    /// The DOM nodes a mounted subtree contributes to its parent, in order.
    fn dom_nodes(&self, n: &MNode, out: &mut Vec<NodeId>) {
        match n {
            MNode::Empty => {}
            MNode::Text { node, .. } => out.push(*node),
            MNode::Template(t) => out.push(t.root),
            MNode::Component { inst } => {
                if let Some(i) = self.instances.get(inst) {
                    self.dom_nodes(&i.rendered, out);
                }
            }
            MNode::List { children, .. } | MNode::Provider { children, .. } => {
                for (_, c) in children {
                    self.dom_nodes(c, out);
                }
            }
        }
    }

    pub(crate) fn first_dom(&self, n: &MNode) -> Option<NodeId> {
        match n {
            MNode::Empty => None,
            MNode::Text { node, .. } => Some(*node),
            MNode::Template(t) => Some(t.root),
            MNode::Component { inst } => self
                .instances
                .get(inst)
                .and_then(|i| self.first_dom(&i.rendered)),
            MNode::List { children, .. } | MNode::Provider { children, .. } => {
                children.iter().find_map(|(_, c)| self.first_dom(c))
            }
        }
    }

    /// The node a child hole's content goes before.
    fn hole_anchor(&self, t: &MTemplate, h: usize) -> Option<NodeId> {
        let mut h = h;
        loop {
            match &t.holes[h] {
                MHole::Child {
                    next_static,
                    next_hole,
                    ..
                } => {
                    if let Some(s) = next_static {
                        return Some(*s);
                    }
                    let nh = (*next_hole)?;
                    if let MHole::Child { mounted, .. } = &t.holes[nh as usize] {
                        if let Some(n) = self.first_dom(mounted) {
                            return Some(n);
                        }
                    }
                    h = nh as usize;
                }
                _ => return None,
            }
        }
    }

    fn same_type(&self, m: &MNode, v: &Value) -> bool {
        match (m, v) {
            (MNode::Empty, v) => is_empty_child(v),
            (MNode::Text { .. }, Value::Str(s)) => !s.is_empty(),
            (MNode::Text { .. }, Value::Num(_)) => true,
            (
                MNode::List {
                    fragment: false, ..
                },
                Value::Array(_),
            ) => true,
            (m, Value::Elem(e)) => match (m, &**e) {
                (MNode::Template(t), Elem::Template { tid, .. }) => t.tid == *tid,
                (MNode::Component { inst }, Elem::Component { func, .. }) => self
                    .instances
                    .get(inst)
                    .is_some_and(|i| Rc::ptr_eq(&i.func, func)),
                (MNode::List { fragment: true, .. }, Elem::Fragment { .. }) => true,
                (MNode::Provider { ctx, .. }, Elem::Provider { ctx: c, .. }) => ctx == c,
                _ => false,
            },
            _ => false,
        }
    }

    /// Updates `old` to render `new` in `parent` before `anchor`.
    pub(crate) fn reconcile(
        &mut self,
        old: MNode,
        new: &Value,
        parent: NodeId,
        anchor: Option<NodeId>,
    ) -> MNode {
        match new {
            v if is_empty_child(v) => {
                self.unmount(old, true);
                MNode::Empty
            }
            Value::Str(_) | Value::Num(_) => {
                let text = new.to_js_string();
                if let MNode::Text { node, text: t } = &old {
                    if **t != *text {
                        self.inner.doc.set_text(*node, &text);
                    }
                    return MNode::Text {
                        node: *node,
                        text: Rc::from(text.as_str()),
                    };
                }
                let node = self.inner.doc.create_text(&text);
                self.insert(parent, node, anchor);
                self.unmount(old, true);
                MNode::Text {
                    node,
                    text: Rc::from(text.as_str()),
                }
            }
            Value::Array(a) => {
                let items = a.borrow().clone();
                self.reconcile_list(old, &items, parent, anchor, false, None)
            }
            Value::Elem(e) => {
                let e = e.clone();
                match &*e {
                    Elem::Template { tid, key, .. } => {
                        if let MNode::Template(mt) = old {
                            if mt.tid == *tid && mt.key.as_ref() == key.as_ref() {
                                return self.patch_template(mt, &e, parent, anchor);
                            }
                            self.replace(MNode::Template(mt), &e, parent, anchor)
                        } else {
                            self.replace(old, &e, parent, anchor)
                        }
                    }
                    Elem::Component { func, .. } => {
                        if let MNode::Component { inst } = &old {
                            let same = self.instances.get(inst).is_some_and(|i| {
                                Rc::ptr_eq(&i.func, func)
                                    && i.elem.as_ref().map(|x| x.key()) == Some(e.key())
                            });
                            if same {
                                let inst = *inst;
                                self.update_component(inst, &e, parent, anchor);
                                return old;
                            }
                        }
                        self.replace(old, &e, parent, anchor)
                    }
                    Elem::Fragment { children, key } => {
                        if let MNode::List {
                            fragment: true,
                            key: k,
                            ..
                        } = &old
                        {
                            if k.as_ref() == key.as_ref() {
                                return self.reconcile_list(
                                    old,
                                    children,
                                    parent,
                                    anchor,
                                    true,
                                    key.clone(),
                                );
                            }
                        }
                        self.replace(old, &e, parent, anchor)
                    }
                    Elem::Provider {
                        ctx,
                        value,
                        children,
                        key,
                    } => {
                        if let MNode::Provider {
                            ctx: c,
                            value: old_value,
                            children: old_children,
                            key: k,
                        } = old
                        {
                            if c == *ctx && k.as_ref() == key.as_ref() {
                                let list = MNode::List {
                                    children: old_children,
                                    key: None,
                                    fragment: false,
                                };
                                if !same_value(&old_value, value) {
                                    self.propagate_context(*ctx, &list);
                                }
                                self.ctx_stack.push((*ctx, value.clone()));
                                let list = self
                                    .reconcile_list(list, children, parent, anchor, false, None);
                                self.ctx_stack.pop();
                                let MNode::List { children, .. } = list else {
                                    unreachable!()
                                };
                                return MNode::Provider {
                                    ctx: *ctx,
                                    value: value.clone(),
                                    children,
                                    key: key.clone(),
                                };
                            }
                            let old = MNode::Provider {
                                ctx: c,
                                value: old_value,
                                children: old_children,
                                key: k,
                            };
                            return self.replace(old, &e, parent, anchor);
                        }
                        self.replace(old, &e, parent, anchor)
                    }
                }
            }
            other => {
                self.crash(&format!(
                    "Error: Objects are not valid as a React child (found: {})",
                    inspect(other)
                ));
                old
            }
        }
    }

    /// Mounts `e` in place of `old`.
    fn replace(
        &mut self,
        old: MNode,
        e: &Rc<Elem>,
        parent: NodeId,
        anchor: Option<NodeId>,
    ) -> MNode {
        // The old content's position is before `anchor`; new content goes there too.
        let at = self.first_dom(&old).or(anchor);
        let new = self.mount_elem(e, parent, at);
        self.unmount(old, true);
        new
    }

    fn mount_elem(&mut self, e: &Rc<Elem>, parent: NodeId, anchor: Option<NodeId>) -> MNode {
        match &**e {
            Elem::Template { tid, holes, key } => {
                let (root, mholes) = self.instantiate(*tid, holes, parent);
                self.insert(parent, root, anchor);
                MNode::Template(Box::new(MTemplate {
                    tid: *tid,
                    key: key.clone(),
                    root,
                    elem: e.clone(),
                    holes: mholes,
                }))
            }
            Elem::Component { func, props, .. } => {
                let id = self.next_inst;
                self.next_inst += 1;
                let parent_inst = self.owner.last().copied();
                self.instances.insert(
                    id,
                    Instance {
                        func: func.clone(),
                        elem: Some(e.clone()),
                        props: props.clone(),
                        hooks: Vec::new(),
                        rendered: MNode::Empty,
                        parent: parent_inst,
                        dirty: false,
                        subtree_dirty: false,
                        context_changed: false,
                        cache: ElemCache::default(),
                    },
                );
                self.render_and_reconcile(id, parent, anchor, true);
                MNode::Component { inst: id }
            }
            Elem::Fragment { children, key } => {
                self.reconcile_list(MNode::Empty, children, parent, anchor, true, key.clone())
            }
            Elem::Provider {
                ctx,
                value,
                children,
                key,
            } => {
                self.ctx_stack.push((*ctx, value.clone()));
                let list = self.reconcile_list(MNode::Empty, children, parent, anchor, false, None);
                self.ctx_stack.pop();
                let MNode::List { children, .. } = list else {
                    unreachable!()
                };
                MNode::Provider {
                    ctx: *ctx,
                    value: value.clone(),
                    children,
                    key: key.clone(),
                }
            }
        }
    }

    fn patch_template(
        &mut self,
        mut mt: Box<MTemplate>,
        e: &Rc<Elem>,
        parent: NodeId,
        anchor: Option<NodeId>,
    ) -> MNode {
        if Rc::ptr_eq(&mt.elem, e) {
            let mut node = MNode::Template(mt);
            self.visit(&mut node, parent, anchor);
            return node;
        }
        let Elem::Template { holes: values, .. } = &**e else {
            unreachable!()
        };
        let info = self.templates[mt.tid as usize].clone();
        let mut form_nodes: Vec<NodeId> = Vec::new();
        for (i, v) in values.iter().enumerate() {
            let hole = std::mem::replace(
                &mut mt.holes[i],
                MHole::Attr {
                    node: NodeId(0),
                    value: Value::Undefined,
                },
            );
            let new_hole = match hole {
                MHole::Attr { node, value } => {
                    if !same_value(&value, v) {
                        let prop = info.props[i].clone().expect("attribute hole");
                        self.set_prop(node, &prop, &value, v, false);
                    }
                    MHole::Attr {
                        node,
                        value: v.clone(),
                    }
                }
                MHole::Spread { node, value } => {
                    if !same_value(&value, v) {
                        self.apply_spread(node, &value, v, false);
                        if matches!(
                            self.tag_of(node),
                            "input" | "textarea" | "select" | "option"
                        ) {
                            form_nodes.push(node);
                        }
                    }
                    MHole::Spread {
                        node,
                        value: v.clone(),
                    }
                }
                MHole::Ref { node, value } => {
                    if !same_value(&value, v) {
                        if !value.is_nullish() {
                            self.ref_detach.push(value);
                        }
                        if !v.is_nullish() {
                            self.ref_attach.push((v.clone(), node));
                        }
                    }
                    MHole::Ref {
                        node,
                        value: v.clone(),
                    }
                }
                child @ MHole::Child { .. } => {
                    // Put it back to compute the anchor against the current holes.
                    mt.holes[i] = child;
                    let a = self.hole_anchor(&mt, i);
                    let MHole::Child {
                        parent: p,
                        next_static,
                        next_hole,
                        value,
                        mounted,
                    } = std::mem::replace(
                        &mut mt.holes[i],
                        MHole::Attr {
                            node: NodeId(0),
                            value: Value::Undefined,
                        },
                    )
                    else {
                        unreachable!()
                    };
                    let mounted = if same_value(&value, v) {
                        let mut m = mounted;
                        self.visit(&mut m, p, a);
                        m
                    } else {
                        self.reconcile(mounted, v, p, a)
                    };
                    MHole::Child {
                        parent: p,
                        next_static,
                        next_hole,
                        value: v.clone(),
                        mounted,
                    }
                }
            };
            mt.holes[i] = new_hole;
        }
        for n in form_nodes {
            self.update_form_control(n);
        }
        mt.elem = e.clone();
        MNode::Template(mt)
    }

    fn reconcile_list(
        &mut self,
        old: MNode,
        items: &[Value],
        parent: NodeId,
        anchor: Option<NodeId>,
        fragment: bool,
        key: Option<Str>,
    ) -> MNode {
        let old_children = match old {
            MNode::List {
                children,
                fragment: f,
                key: k,
            } if f == fragment && k == key => children,
            other => {
                self.unmount(other, true);
                Vec::new()
            }
        };
        let fresh = old_children.is_empty();
        let mut index: BTreeMap<ListKey, usize> = BTreeMap::new();
        for (i, (k, _)) in old_children.iter().enumerate() {
            index.entry(k.clone()).or_insert(i);
        }
        let mut slots: Vec<Option<MNode>> =
            old_children.into_iter().map(|(_, m)| Some(m)).collect();
        let mut out: Vec<(ListKey, MNode)> = Vec::with_capacity(items.len());
        for (i, v) in items.iter().enumerate() {
            let k = match v {
                Value::Elem(e) => match e.key() {
                    Some(k) => ListKey::Key(k.clone()),
                    None => ListKey::Index(i),
                },
                _ => ListKey::Index(i),
            };
            let matched = index.get(&k).and_then(|j| slots[*j].take());
            let node = match matched {
                Some(m) if self.same_type(&m, v) => self.reconcile(m, v, parent, anchor),
                Some(m) => {
                    self.unmount(m, true);
                    self.mount_value(v, parent, anchor)
                }
                None => self.mount_value(v, parent, anchor),
            };
            out.push((k, node));
        }
        for m in slots.into_iter().flatten() {
            self.unmount(m, true);
        }
        if !fresh {
            // Place every child's nodes, right to left, before the next child's.
            let mut next = anchor.filter(|a| self.inner.doc.parent(*a) == Some(parent));
            let mut buf = Vec::new();
            for (_, child) in out.iter().rev() {
                buf.clear();
                self.dom_nodes(child, &mut buf);
                for n in buf.iter().rev() {
                    let placed = self.inner.doc.parent(*n) == Some(parent)
                        && self.inner.doc.next_sibling(*n) == next;
                    if !placed {
                        self.inner.doc.insert_before(parent, *n, next);
                    }
                    next = Some(*n);
                }
            }
        }
        MNode::List {
            children: out,
            key,
            fragment,
        }
    }

    fn update_component(
        &mut self,
        inst: u32,
        e: &Rc<Elem>,
        parent: NodeId,
        anchor: Option<NodeId>,
    ) {
        let (same, dirty) = {
            let i = &self.instances[&inst];
            (
                i.elem.as_ref().is_some_and(|x| Rc::ptr_eq(x, e)),
                i.dirty || i.context_changed,
            )
        };
        if same && !dirty {
            // React's bailout: the same element, no pending work here.
            let mut node = MNode::Component { inst };
            self.visit(&mut node, parent, anchor);
            return;
        }
        if let Elem::Component { props, .. } = &**e {
            let i = self.instance(inst);
            i.props = props.clone();
            i.elem = Some(e.clone());
        }
        self.render_and_reconcile(inst, parent, anchor, true);
    }

    /// Renders an instance and reconciles its output. Without `forced` (a state
    /// update), a render that changed no state bails out as React's does.
    fn render_and_reconcile(
        &mut self,
        inst: u32,
        parent: NodeId,
        anchor: Option<NodeId>,
        forced: bool,
    ) {
        let (func, props, context_changed) = {
            let i = &self.instances[&inst];
            (i.func.clone(), i.props.clone(), i.context_changed)
        };
        let first = self.instances[&inst].hooks.is_empty();
        let old_cache = std::mem::take(&mut self.instance(inst).cache);
        self.render.push(RenderCtx {
            inst,
            cursor: 0,
            state_changed: false,
            rerender: false,
            old_cache,
            new_cache: ElemCache::default(),
        });
        let mut result;
        let mut guard = 0;
        loop {
            result = self.call_closure(&func, vec![props.clone()], Some(inst));
            let r = self.render.last_mut().unwrap();
            if r.rerender && result.is_ok() && guard < 25 {
                r.rerender = false;
                r.cursor = 0;
                guard += 1;
                continue;
            }
            break;
        }
        let ctx = self.render.pop().unwrap();
        self.stats.renders += 1;
        if let Some(i) = self.instances.get_mut(&inst) {
            i.cache = ctx.new_cache;
            i.dirty = false;
            i.context_changed = false;
        }
        let value = match result {
            Ok(v) => v,
            Err(Throw::Value(v)) => {
                self.crash(&inspect(&v));
                Value::Null
            }
            Err(Throw::Short) => Value::Undefined,
        };
        if !forced && !first && !ctx.state_changed && !context_changed {
            // Nothing this component reads changed: keep what it rendered.
            for h in self.instance(inst).hooks.iter_mut() {
                if let HookState::Effect { pending, .. } = h {
                    *pending = None;
                }
            }
            let subtree = self.instances[&inst].subtree_dirty;
            if subtree {
                self.instance(inst).subtree_dirty = false;
                let mut rendered = std::mem::take(&mut self.instance(inst).rendered);
                self.owner.push(inst);
                self.visit(&mut rendered, parent, anchor);
                self.owner.pop();
                self.instance(inst).rendered = rendered;
            }
            return;
        }
        // A component returning an unkeyed fragment renders its children directly.
        let value = match &value {
            Value::Elem(e) => match &**e {
                Elem::Fragment {
                    children,
                    key: None,
                } => Value::array(children.clone()),
                _ => value,
            },
            _ => value,
        };
        let old = std::mem::take(&mut self.instance(inst).rendered);
        self.instance(inst).subtree_dirty = false;
        self.owner.push(inst);
        let rendered = self.reconcile(old, &value, parent, anchor);
        self.owner.pop();
        if let Some(i) = self.instances.get_mut(&inst) {
            i.rendered = rendered;
        } else {
            // Unmounted while rendering its own subtree (cannot happen) — drop.
            self.unmount(rendered, true);
        }
        self.effect_list.push(inst);
    }

    /// Marks every instance below `node` that reads context `ctx` for re-render.
    fn propagate_context(&mut self, ctx: u32, node: &MNode) {
        let mut stack: Vec<u32> = Vec::new();
        self.collect_instances(node, &mut stack);
        while let Some(id) = stack.pop() {
            let reads = self.instances[&id]
                .hooks
                .iter()
                .any(|h| matches!(h, HookState::Context(c) if *c == ctx));
            if reads {
                self.instance(id).context_changed = true;
                self.mark_dirty(id);
            }
            let rendered = std::mem::take(&mut self.instance(id).rendered);
            self.collect_instances(&rendered, &mut stack);
            self.instance(id).rendered = rendered;
        }
    }

    fn collect_instances(&self, n: &MNode, out: &mut Vec<u32>) {
        match n {
            MNode::Component { inst } => out.push(*inst),
            MNode::Template(t) => {
                for h in &t.holes {
                    if let MHole::Child { mounted, .. } = h {
                        self.collect_instances(mounted, out);
                    }
                }
            }
            MNode::List { children, .. } | MNode::Provider { children, .. } => {
                for (_, c) in children {
                    self.collect_instances(c, out);
                }
            }
            _ => {}
        }
    }

    /// Re-renders dirty instances below `node`, in document order.
    pub(crate) fn visit(&mut self, node: &mut MNode, parent: NodeId, anchor: Option<NodeId>) {
        match node {
            MNode::Empty | MNode::Text { .. } => {}
            MNode::Component { inst } => {
                let inst = *inst;
                let Some(i) = self.instances.get(&inst) else {
                    return;
                };
                if i.dirty || i.context_changed {
                    self.render_and_reconcile(inst, parent, anchor, false);
                } else if i.subtree_dirty {
                    self.instance(inst).subtree_dirty = false;
                    let mut rendered = std::mem::take(&mut self.instance(inst).rendered);
                    self.owner.push(inst);
                    self.visit(&mut rendered, parent, anchor);
                    self.owner.pop();
                    self.instance(inst).rendered = rendered;
                }
            }
            MNode::Template(t) => {
                for i in 0..t.holes.len() {
                    if !matches!(t.holes[i], MHole::Child { .. }) {
                        continue;
                    }
                    let a = self.hole_anchor(t, i);
                    if let MHole::Child {
                        parent: p, mounted, ..
                    } = &mut t.holes[i]
                    {
                        let p = *p;
                        let mut m = std::mem::take(mounted);
                        self.visit(&mut m, p, a);
                        if let MHole::Child { mounted, .. } = &mut t.holes[i] {
                            *mounted = m;
                        }
                    }
                }
            }
            MNode::List { children, .. } => self.visit_children(children, parent, anchor),
            MNode::Provider {
                ctx,
                value,
                children,
                ..
            } => {
                self.ctx_stack.push((*ctx, value.clone()));
                self.visit_children(children, parent, anchor);
                self.ctx_stack.pop();
            }
        }
    }

    fn visit_children(
        &mut self,
        children: &mut [(ListKey, MNode)],
        parent: NodeId,
        anchor: Option<NodeId>,
    ) {
        // Anchors right to left before visiting left to right.
        let mut anchors = vec![anchor; children.len()];
        let mut next = anchor;
        for i in (0..children.len()).rev() {
            anchors[i] = next;
            if let Some(f) = self.first_dom(&children[i].1) {
                next = Some(f);
            }
        }
        for (i, (_, c)) in children.iter_mut().enumerate() {
            self.visit(c, parent, anchors[i]);
        }
    }

    pub(crate) fn unmount(&mut self, n: MNode, remove: bool) {
        match n {
            MNode::Empty => {}
            MNode::Text { node, .. } => {
                if remove {
                    self.inner.doc.detach(node);
                }
            }
            MNode::Template(t) => {
                for h in t.holes {
                    match h {
                        MHole::Attr { node, .. } | MHole::Spread { node, .. } => {
                            self.handlers.remove(&node);
                            self.controlled.remove(&node);
                            self.form_props.remove(&node);
                        }
                        MHole::Ref { value, .. } => {
                            if !value.is_nullish() {
                                self.ref_detach.push(value);
                            }
                        }
                        MHole::Child { mounted, .. } => self.unmount(mounted, false),
                    }
                }
                if remove {
                    self.inner.doc.detach(t.root);
                }
            }
            MNode::Component { inst } => {
                let Some(i) = self.instances.remove(&inst) else {
                    return;
                };
                for h in &i.hooks {
                    match h {
                        HookState::Effect {
                            layout,
                            cleanup: Some(c),
                            ..
                        } => {
                            if *layout {
                                self.deleted_layout.push(c.clone());
                            } else {
                                self.deleted_passive.push(c.clone());
                            }
                        }
                        HookState::Store {
                            unsubscribe: Some(u),
                            ..
                        } => self.deleted_passive.push(u.clone()),
                        _ => {}
                    }
                }
                self.unmount(i.rendered, remove);
            }
            MNode::List { children, .. } | MNode::Provider { children, .. } => {
                for (_, c) in children {
                    self.unmount(c, remove);
                }
            }
        }
    }

    pub(crate) fn crash(&mut self, msg: &str) {
        self.log(cw_web::script::LogLevel::Error, &format!("Uncaught {msg}"));
        self.crashed = true;
    }

    // ------------------------------------------------------------------ commit

    fn run_cleanup(&mut self, f: &Value) {
        if let Err(e) = self.call_value(f, vec![]) {
            self.report(e);
        }
    }

    fn commit(&mut self) {
        for c in std::mem::take(&mut self.deleted_layout) {
            self.run_cleanup(&c);
        }
        let list = std::mem::take(&mut self.effect_list);
        // Layout effect cleanups of updated components.
        for inst in &list {
            let cleanups = self.take_cleanups(*inst, true);
            for c in cleanups {
                self.run_cleanup(&c);
            }
        }
        for r in std::mem::take(&mut self.ref_detach) {
            self.set_ref(&r, Value::Null);
        }
        for (r, n) in std::mem::take(&mut self.ref_attach) {
            self.set_ref(&r, Value::Node(n));
        }
        for inst in &list {
            self.run_creates(*inst, true);
        }
        for n in std::mem::take(&mut self.autofocus) {
            if self.inner.doc.parent(n).is_some() && self.inner.is_focusable(n) {
                self.set_focus(Some(n), false);
            }
        }
        for c in std::mem::take(&mut self.deleted_passive) {
            self.run_cleanup(&c);
        }
        for inst in &list {
            let cleanups = self.take_cleanups(*inst, false);
            for c in cleanups {
                self.run_cleanup(&c);
            }
        }
        for inst in &list {
            self.run_creates(*inst, false);
            self.subscribe_stores(*inst);
        }
    }

    /// Subscribes (or resubscribes) an instance's external stores.
    fn subscribe_stores(&mut self, inst: u32) {
        let n = match self.instances.get(&inst) {
            Some(i) => i.hooks.len(),
            None => return,
        };
        for idx in 0..n {
            let job = match self
                .instances
                .get_mut(&inst)
                .and_then(|i| i.hooks.get_mut(idx))
            {
                Some(HookState::Store {
                    subscribe,
                    unsubscribe,
                    needs_subscribe: true,
                    ..
                }) => Some((subscribe.clone(), unsubscribe.take())),
                _ => None,
            };
            let Some((subscribe, old)) = job else {
                continue;
            };
            if let Some(u) = old {
                self.run_cleanup(&u);
            }
            let cb = Value::Native(Rc::new(NativeFn::StoreChanged {
                inst,
                hook: idx as u32,
            }));
            let un = match self.call_value(&subscribe, vec![cb]) {
                Ok(v) => matches!(v, Value::Func(_) | Value::Native(_)).then_some(v),
                Err(e) => {
                    self.report(e);
                    None
                }
            };
            if let Some(HookState::Store {
                unsubscribe,
                needs_subscribe,
                ..
            }) = self
                .instances
                .get_mut(&inst)
                .and_then(|i| i.hooks.get_mut(idx))
            {
                *unsubscribe = un;
                *needs_subscribe = false;
            }
            // A change between render and subscription is caught up now.
            let _ = self.store_changed(inst, idx as u32);
        }
    }

    fn set_ref(&mut self, r: &Value, v: Value) {
        match r {
            Value::Ref(cell) => *cell.borrow_mut() = v,
            f @ Value::Func(_) => {
                if let Err(e) = self.call_value(f, vec![v]) {
                    self.report(e);
                }
            }
            _ => {}
        }
    }

    /// The cleanups of `inst`'s effects that will re-run.
    fn take_cleanups(&mut self, inst: u32, layout_phase: bool) -> Vec<Value> {
        let Some(i) = self.instances.get_mut(&inst) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for h in i.hooks.iter_mut() {
            if let HookState::Effect {
                layout,
                pending: Some(_),
                cleanup,
                ..
            } = h
            {
                if *layout == layout_phase {
                    if let Some(c) = cleanup.take() {
                        out.push(c);
                    }
                }
            }
        }
        out
    }

    fn run_creates(&mut self, inst: u32, layout_phase: bool) {
        let n = match self.instances.get(&inst) {
            Some(i) => i.hooks.len(),
            None => return,
        };
        for idx in 0..n {
            let create = match self
                .instances
                .get_mut(&inst)
                .and_then(|i| i.hooks.get_mut(idx))
            {
                Some(HookState::Effect {
                    layout, pending, ..
                }) if *layout == layout_phase => pending.take(),
                _ => None,
            };
            let Some(create) = create else { continue };
            match self.call_value(&create, vec![]) {
                Ok(ret) => {
                    let cleanup = matches!(ret, Value::Func(_)).then_some(ret);
                    if let Some(HookState::Effect { cleanup: c, .. }) = self
                        .instances
                        .get_mut(&inst)
                        .and_then(|i| i.hooks.get_mut(idx))
                    {
                        *c = cleanup;
                    }
                }
                Err(e) => self.report(e),
            }
        }
    }

    // ------------------------------------------------------------------ scheduling

    /// Renders and commits until no update is pending (updates scheduled by layout
    /// effects render synchronously, after the passive effects of the commit that
    /// scheduled them).
    pub(crate) fn flush(&mut self) {
        let mut guard = 0;
        let worked = self.pending_work;
        while self.pending_work && !self.crashed {
            guard += 1;
            if guard > 50 {
                self.log(
                    cw_web::script::LogLevel::Error,
                    "Uncaught Error: Maximum update depth exceeded.",
                );
                break;
            }
            self.pending_work = false;
            let mut root = std::mem::take(&mut self.root);
            let container = self.container;
            self.visit(&mut root, container, None);
            self.root = root;
            self.commit();
        }
        if self.crashed {
            let root = std::mem::take(&mut self.root);
            self.unmount(root, true);
            self.effect_list.clear();
            self.pending_work = false;
        }
        if worked {
            self.inner.touch();
        }
    }

    /// Drains microtasks, rendering after each batch, until quiet.
    pub(crate) fn settle(&mut self) {
        let mut guard = 0;
        loop {
            let mut ran = false;
            while let Some(m) = self.microtasks.pop_front() {
                self.run_microtask(m);
                ran = true;
                guard += 1;
                if guard > 100_000 {
                    self.log(
                        cw_web::script::LogLevel::Error,
                        "Uncaught Error: microtask loop",
                    );
                    self.microtasks.clear();
                    break;
                }
            }
            self.flush();
            if !ran && self.microtasks.is_empty() {
                break;
            }
            if self.microtasks.is_empty() && !self.pending_work {
                break;
            }
        }
    }

    /// Mounts the module's root element into the container.
    pub(crate) fn mount_root(&mut self, element: Value) {
        let container = self.container;
        let kids: Vec<NodeId> = self.inner.doc.children(container).collect();
        for k in kids {
            self.inner.doc.detach(k);
        }
        let root = self.mount_value(&element, container, None);
        self.root = root;
        self.commit();
        self.inner.touch();
    }
}

/// `null`, `undefined`, booleans and `''` render nothing.
fn is_empty_child(v: &Value) -> bool {
    match v {
        Value::Undefined | Value::Null | Value::Bool(_) => true,
        Value::Str(s) => s.is_empty(),
        _ => false,
    }
}
