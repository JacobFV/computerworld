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
        Expr::Method { recv, args, .. } | Expr::Invoke { recv, args, .. } => {
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

/// An element's `key` prop as React reads it.
pub fn key_of(v: &Value) -> Option<Str> {
    match v {
        Value::Undefined | Value::Null => None,
        Value::Str(s) => Some(s.clone()),
        other => Some(Rc::from(other.to_js_string().as_str())),
    }
}

/// The component a JSX element names.
pub fn component_callee(f: Value) -> R<ComponentFn> {
    let shown = inspect(&f);
    match ComponentFn::of(f) {
        Some(c) => Ok(c),
        None => type_error(format!("element type is invalid: {shown}")),
    }
}

/// `{...spread}` into a component's props (React drops `key` and `ref`).
pub fn props_spread(out: &mut Vec<(Str, Value)>, v: &Value) {
    if let Value::Object(o) = v {
        for (k, v) in o.borrow().iter() {
            if &**k == "key" || &**k == "ref" {
                continue;
            }
            crate::interp::obj_set(out, k.clone(), v.clone());
        }
    }
}

/// `<Comp {...props}>children</Comp>`.
pub fn component_elem(
    func: ComponentFn,
    mut props: Vec<(Str, Value)>,
    children: Option<Value>,
    key: Option<Str>,
) -> Value {
    if let Some(c) = children {
        crate::interp::obj_set(&mut props, Rc::from("children"), c);
    }
    Value::Elem(Rc::new(Elem::Component {
        func,
        props: Value::object(props),
        key,
    }))
}

/// The context a `<Ctx.Provider>` provides.
pub fn provider_context(v: Value) -> R<u32> {
    match v {
        Value::Context(ctx) => Ok(ctx),
        _ => type_error("Provider of a value that is not a context"),
    }
}

/// A template element under construction (see `Runtime::tpl_begin`).
pub struct TplBuilder {
    caching: bool,
    site: (usize, u32),
    old: Option<CacheEntry>,
    tid: u32,
    values: Vec<Value>,
    deps: Vec<Vec<Value>>,
    all_same: bool,
}

impl TplBuilder {
    /// Whether this render compares hole dependencies (else they need not be read).
    pub fn caching(&self) -> bool {
        self.caching
    }
}

/// A hook's argument evaluator: `(runtime, index) -> value`.
pub type HookArg<'a> = dyn FnMut(&mut Runtime, usize) -> R<Value> + 'a;

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

    /// A dependency list argument: absent (`nargs <= i`), an array, or nullish.
    fn deps_arg(&mut self, nargs: usize, arg: &mut HookArg<'_>, i: usize) -> R<Option<Vec<Value>>> {
        if i >= nargs {
            return Ok(None);
        }
        match arg(self, i)? {
            Value::Array(a) => Ok(Some(a.borrow().clone())),
            // An island's dependency list.
            Value::Foreign(f) if f.array => Ok(Some(self.foreign_items(&f)?)),
            Value::Undefined | Value::Null => Ok(None),
            other => type_error(format!(
                "dependency list {} is not an array",
                inspect(&other)
            )),
        }
    }

    pub(crate) fn hook(&mut self, frame: &mut Frame, h: Hook, args: &[Expr]) -> R<Value> {
        self.hook_with(h, args.len(), &mut |rt: &mut Runtime, i: usize| {
            rt.eval(frame, &args[i])
        })
    }

    /// A hook call with `nargs` arguments, each evaluated (in the order React's
    /// hook evaluates them, and only when it does) by `arg`.
    pub fn hook_with(&mut self, h: Hook, nargs: usize, arg: &mut HookArg<'_>) -> R<Value> {
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
                let init = if nargs > 0 {
                    arg(self, 0)?
                } else {
                    Value::Undefined
                };
                if first {
                    let value = if init.type_of() == "function" {
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
                            if f.type_of() == "function" {
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
                let reducer = if nargs > 0 {
                    arg(self, 0)?
                } else {
                    Value::Undefined
                };
                let init = if nargs > 1 {
                    arg(self, 1)?
                } else {
                    Value::Undefined
                };
                if first {
                    let value = if nargs > 2 {
                        let f = arg(self, 2)?;
                        self.call_value(&f, vec![init])?
                    } else {
                        init
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
                let f = if nargs > 0 {
                    arg(self, 0)?
                } else {
                    Value::Undefined
                };
                let deps = self.deps_arg(nargs, arg, 1)?;
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
                    let init = if nargs > 0 {
                        arg(self, 0)?
                    } else {
                        Value::Undefined
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
                let create = if nargs > 0 {
                    arg(self, 0)?
                } else {
                    Value::Undefined
                };
                let deps = self.deps_arg(nargs, arg, 1)?;
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
            Hook::ImperativeHandle => {
                // A layout effect whose create sets the ref (React appends the
                // ref to the dependencies).
                let r = if nargs > 0 {
                    arg(self, 0)?
                } else {
                    Value::Undefined
                };
                let create = if nargs > 1 {
                    arg(self, 1)?
                } else {
                    Value::Undefined
                };
                let deps = self.deps_arg(nargs, arg, 2)?.map(|mut d| {
                    d.push(r.clone());
                    d
                });
                let effect = Value::Native(Rc::new(NativeFn::ImperativeSet { r, create }));
                if first {
                    self.instance(inst).hooks.push(HookState::Effect {
                        layout: true,
                        deps,
                        pending: Some(effect),
                        cleanup: None,
                    });
                    return Ok(Value::Undefined);
                }
                if let HookState::Effect {
                    deps: old, pending, ..
                } = &mut self.instance(inst).hooks[idx]
                {
                    if deps_changed(old, &deps) {
                        *pending = Some(effect);
                        *old = deps;
                    }
                }
                Ok(Value::Undefined)
            }
            Hook::Context => {
                let c = if nargs > 0 {
                    arg(self, 0)?
                } else {
                    Value::Undefined
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
                let subscribe = if nargs > 0 {
                    arg(self, 0)?
                } else {
                    Value::Undefined
                };
                let get = if nargs > 1 {
                    arg(self, 1)?
                } else {
                    Value::Undefined
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
            let new = if arg.type_of() == "function" {
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
                let program = self.program.clone();
                // A component's own frame: skip holes whose inputs are unchanged. (A
                // generated program's templates carry no hole metadata: its own code
                // does this; the interpreter runs only its async functions, never a
                // render.)
                let meta = match program.template(*template) {
                    crate::program::TemplateRef::Ir(t) => Some(&t.holes),
                    crate::program::TemplateRef::Static(_) => None,
                };
                let caching = frame.inst.is_some() && self.pure_render && meta.is_some();
                let occ = if caching {
                    let addr = el as *const ElementExpr as usize;
                    let o = frame.occ.entry(addr).or_insert(0);
                    *o += 1;
                    (addr, *o)
                } else {
                    (0, 0)
                };
                let mut b = self.tpl_begin(caching, occ.0, occ.1, *template, holes.len());
                for (i, h) in holes.iter().enumerate() {
                    let (deps, always) = match meta {
                        Some(m) if caching => (
                            m[i].deps.iter().map(|d| frame.slot_value(d)).collect(),
                            m[i].always,
                        ),
                        _ => (Vec::new(), false),
                    };
                    match self.tpl_reuse(&b, i, always, &deps) {
                        Some(v) => self.tpl_push(&mut b, v, deps, false),
                        None => {
                            let v = self.eval(frame, h)?;
                            self.tpl_push(&mut b, v, deps, true);
                        }
                    }
                }
                self.tpl_finish(b, key)
            }
            ElementExpr::Component {
                callee,
                props,
                children,
                key,
            } => {
                let f = self.eval(frame, callee)?;
                let func = component_callee(f)?;
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
                            let v = self.eval(frame, v)?;
                            let v = self.plain_object(&v)?;
                            props_spread(&mut out, &v);
                        }
                    }
                }
                let children = match children {
                    Some(c) => Some(self.eval(frame, c)?),
                    None => None,
                };
                let key = match key {
                    Some(k) => key_of(&self.eval(frame, k)?),
                    None => None,
                };
                component_elem(func, out, children, key)
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
                let ctx = provider_context(self.eval(frame, context)?)?;
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

    /// Starts a template element: `caching` when this is a component's own render
    /// and hole skipping is sound, `(site, occ)` the element expression's place and
    /// occurrence in this render (the key of last render's entry).
    pub fn tpl_begin(
        &mut self,
        caching: bool,
        site: usize,
        occ: u32,
        tid: u32,
        n_holes: usize,
    ) -> TplBuilder {
        let old = if caching {
            self.render
                .last_mut()
                .and_then(|r| r.old_cache.entries.remove(&(site, occ)))
        } else {
            None
        };
        TplBuilder {
            caching,
            site: (site, occ),
            all_same: old.is_some(),
            old,
            tid,
            values: Vec::with_capacity(n_holes),
            deps: Vec::with_capacity(if caching { n_holes } else { 0 }),
        }
    }

    /// Hole `i`'s value from last render, when its dependency values (`deps`, the
    /// slots it reads, in the order the IR lists them) are unchanged.
    pub fn tpl_reuse(
        &mut self,
        b: &TplBuilder,
        i: usize,
        always: bool,
        deps: &[Value],
    ) -> Option<Value> {
        if !b.caching || always {
            return None;
        }
        let o = b.old.as_ref()?;
        let same = o.deps[i].len() == deps.len()
            && o.deps[i].iter().zip(deps).all(|(a, b)| same_dep(a, b));
        if !same {
            return None;
        }
        self.stats.holes_skipped += 1;
        Some(o.holes[i].clone())
    }

    /// Whether every hole's dependencies are unchanged since last render, when every
    /// hole of the template depends on the same slots, whose values are `deps`: then
    /// every hole takes last render's value, as `tpl_reuse` would give it hole by
    /// hole, in one comparison. A generated program resolves at compile time which
    /// templates qualify.
    pub fn tpl_reuse_all(&mut self, b: &mut TplBuilder, deps: &[Value]) -> bool {
        if !b.caching {
            return false;
        }
        let Some(o) = b.old.as_mut() else {
            return false;
        };
        let same = o.deps.iter().all(|old| {
            old.len() == deps.len() && old.iter().zip(deps).all(|(a, b)| same_dep(a, b))
        });
        if !same {
            return false;
        }
        self.stats.holes_skipped += o.holes.len() as u64;
        b.values = std::mem::take(&mut o.holes);
        b.deps = std::mem::take(&mut o.deps);
        true
    }

    /// Hole `i`'s value: reused, or `evaluated` now.
    pub fn tpl_push(&mut self, b: &mut TplBuilder, v: Value, deps: Vec<Value>, evaluated: bool) {
        if b.caching {
            if evaluated {
                let i = b.values.len();
                if let Some(o) = &b.old {
                    if !same_value(&o.holes[i], &v) {
                        b.all_same = false;
                    }
                }
                self.stats.holes_evaluated += 1;
            }
            b.deps.push(deps);
        }
        b.values.push(v);
    }

    /// The element, reusing last render's when no hole changed.
    pub fn tpl_finish(&mut self, b: TplBuilder, key: Option<Str>) -> Value {
        if !b.caching {
            self.stats.holes_evaluated += b.values.len() as u64;
            return Value::Elem(Rc::new(Elem::Template {
                tid: b.tid,
                holes: b.values,
                key,
            }));
        }
        let elem = match &b.old {
            Some(o) if b.all_same && o.elem.key() == key.as_ref() => {
                self.stats.elements_reused += 1;
                o.elem.clone()
            }
            _ => Rc::new(Elem::Template {
                tid: b.tid,
                holes: b.values.clone(),
                key,
            }),
        };
        if let Some(r) = self.render.last_mut() {
            r.new_cache.entries.insert(
                b.site,
                CacheEntry {
                    deps: b.deps,
                    holes: b.values,
                    elem: elem.clone(),
                },
            );
        }
        Value::Elem(elem)
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
            MNode::Portal { .. } => {}
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
            MNode::Portal { .. } => None,
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
                (MNode::Component { inst }, Elem::Component { func, .. }) => {
                    self.instances.get(inst).is_some_and(|i| i.func.same(func))
                }
                (MNode::List { fragment: true, .. }, Elem::Fragment { .. }) => true,
                (MNode::Provider { ctx, .. }, Elem::Provider { ctx: c, .. }) => ctx == c,
                (MNode::Portal { container, .. }, Elem::Portal { container: c, .. }) => {
                    container == c
                }
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
        // An island's array renders as the array it is.
        let owned;
        let new = match new {
            Value::Foreign(f) if f.array => {
                let f = f.clone();
                owned = Value::array(self.foreign_items(&f).unwrap_or_default());
                &owned
            }
            v => v,
        };
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
                                i.func.same(func)
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
                    Elem::Portal {
                        children,
                        container,
                        key,
                    } => {
                        if let MNode::Portal {
                            container: c,
                            children: old_children,
                            key: k,
                        } = old
                        {
                            if c == *container && k.as_ref() == key.as_ref() {
                                let list = MNode::List {
                                    children: old_children,
                                    key: None,
                                    fragment: false,
                                };
                                return self.portal_list(list, children, *container, key, parent);
                            }
                            let old = MNode::Portal {
                                container: c,
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
            Elem::Portal {
                children,
                container,
                key,
            } => self.portal_list(MNode::Empty, children, *container, key, parent),
        }
    }

    /// A portal's children reconciled in its container, and their top nodes
    /// recorded under `parent` for event bubbling.
    fn portal_list(
        &mut self,
        old: MNode,
        children: &[Value],
        container: NodeId,
        key: &Option<Str>,
        parent: NodeId,
    ) -> MNode {
        let list = self.reconcile_list(old, children, container, None, false, None);
        let MNode::List { children, .. } = list else {
            unreachable!()
        };
        let node = MNode::Portal {
            container,
            children,
            key: key.clone(),
        };
        self.record_portal(&node, parent);
        node
    }

    /// Records a portal's top DOM nodes as bubbling to `parent`.
    pub(crate) fn record_portal(&mut self, node: &MNode, parent: NodeId) {
        if let MNode::Portal { children, .. } = node {
            let mut tops = Vec::new();
            for (_, c) in children {
                self.dom_nodes(c, &mut tops);
            }
            for t in tops {
                self.portal_parents.insert(t, parent);
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
        // An island's array among the items (`[header, rows.map(…)]` built on the
        // VM) is a nested list, as a compiled one is.
        let converted: Vec<Value>;
        let items = if items
            .iter()
            .any(|v| matches!(v, Value::Foreign(f) if f.array))
        {
            converted = items
                .iter()
                .map(|v| match v {
                    Value::Foreign(f) if f.array => {
                        let f = f.clone();
                        Value::array(self.foreign_items(&f).unwrap_or_default())
                    }
                    v => v.clone(),
                })
                .collect();
            &converted[..]
        } else {
            items
        };
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
        // `ref` on a component is the element's, not a prop: a `forwardRef`
        // render function gets it as its second argument, others never see it.
        let (props, element_ref) = split_ref(&props);
        let forward_ref = match &func {
            ComponentFn::Compiled(c) => self.program.forward_ref(c.func),
            ComponentFn::Foreign(f) => self.foreign_forward_ref(f),
        };
        loop {
            let args = if forward_ref {
                vec![props.clone(), element_ref.clone().unwrap_or(Value::Null)]
            } else {
                vec![props.clone()]
            };
            result = match &func {
                ComponentFn::Compiled(c) => self.call_closure(c, args, Some(inst)),
                // A component of the island renders there, its hooks cw-ui's.
                ComponentFn::Foreign(f) => {
                    // A class component renders through the shim's host for it.
                    let host = self.class_host(f);
                    self.foreign_call(&host, args)
                }
            };
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
                let text = self.thrown_text(&v);
                self.crash(&text);
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
            MNode::List { children, .. }
            | MNode::Provider { children, .. }
            | MNode::Portal { children, .. } => {
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
            MNode::Portal {
                container,
                children,
                ..
            } => {
                let c = *container;
                self.visit_children(children, c, None);
                let mut tops = Vec::new();
                for (_, ch) in children.iter() {
                    self.dom_nodes(ch, &mut tops);
                }
                for t in tops {
                    self.portal_parents.insert(t, parent);
                }
            }
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

    /// Takes `n` out of the document. Focus and hover inside it are dropped, with
    /// no events, as the document's own removal steps do.
    pub(crate) fn detach(&mut self, n: NodeId) {
        let inside = |f: Option<NodeId>, doc: &cw_web::dom::Document| {
            f.is_some_and(|f| f == n || doc.ancestors(f).any(|a| a == n))
        };
        if inside(self.inner.focused, &self.inner.doc) {
            self.inner.focused = None;
        }
        if inside(self.inner.hovered, &self.inner.doc) {
            self.inner.hovered = None;
        }
        self.inner.doc.detach(n);
    }

    pub(crate) fn unmount(&mut self, n: MNode, remove: bool) {
        match n {
            MNode::Empty => {}
            MNode::Text { node, .. } => {
                if remove {
                    self.detach(node);
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
                    self.detach(t.root);
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
            MNode::Portal { children, .. } => {
                // In another container: removed whatever becomes of the parent.
                let mut tops = Vec::new();
                for (_, c) in &children {
                    self.dom_nodes(c, &mut tops);
                }
                for t in tops {
                    self.portal_parents.remove(&t);
                }
                for (_, c) in children {
                    self.unmount(c, true);
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
        // The DOM changed: a geometry read in a layout effect (or anything after)
        // lays out the document as it is now, as after a DOM call in a page.
        self.inner.touch();
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
        self.set_ref_value(r, v)
    }

    /// Sets a ref: an object ref's `current`, or a callback ref called with it.
    pub(crate) fn set_ref_value(&mut self, r: &Value, v: Value) {
        match r {
            Value::Ref(cell) => *cell.borrow_mut() = v,
            Value::Object(o) => crate::interp::obj_set(&mut o.borrow_mut(), Rc::from("current"), v),
            f if f.type_of() == "function" => {
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
                    let cleanup = (ret.type_of() == "function").then_some(ret);
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
            // The island's promise jobs, after cw-ui's.
            if self.run_js_jobs() {
                ran = true;
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
            self.detach(k);
        }
        let root = self.mount_value(&element, container, None);
        self.root = root;
        self.commit();
        self.inner.touch();
    }
}

/// A component's props without `ref`, and the `ref`.
fn split_ref(props: &Value) -> (Value, Option<Value>) {
    if let Value::Object(o) = props {
        let has = o.borrow().iter().any(|(k, _)| &**k == "ref");
        if has {
            let mut rest = o.borrow().clone();
            let at = rest.iter().position(|(k, _)| &**k == "ref").unwrap();
            let (_, r) = rest.remove(at);
            return (Value::object(rest), Some(r));
        }
    }
    (props.clone(), None)
}

/// `null`, `undefined`, booleans and `''` render nothing.
fn is_empty_child(v: &Value) -> bool {
    match v {
        Value::Undefined | Value::Null | Value::Bool(_) => true,
        Value::Str(s) => s.is_empty(),
        _ => false,
    }
}
