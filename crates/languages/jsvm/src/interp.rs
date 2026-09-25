//! The bytecode run loop.

use crate::ast::FuncKind;
use crate::bytecode::{Capture, Code, Op};
use crate::call::Invoked;
use crate::conv::Arith;
use crate::value::*;
use crate::vm::*;
use std::cell::Cell;
use std::rc::Rc;

macro_rules! top {
    ($s:expr) => {
        $s.frames.last_mut().unwrap()
    };
}

impl<'h> Vm<'h> {
    #[inline]
    fn push(&mut self, v: Value) {
        top!(self).stack.push(v);
    }
    #[inline]
    fn pop(&mut self) -> Value {
        top!(self).stack.pop().unwrap_or(Value::Undefined)
    }
    #[inline]
    fn peek(&self, n: usize) -> &Value {
        let s = &self.frames.last().unwrap().stack;
        &s[s.len() - 1 - n]
    }
    fn pop_n(&mut self, n: usize) -> Vec<Value> {
        let mut out = self.pool.vals.pop().unwrap_or_default();
        let s = &mut self.frames.last_mut().unwrap().stack;
        let at = s.len() - n;
        out.extend(s.drain(at..));
        out
    }
    fn konst(&self, i: u32) -> Value {
        self.frames.last().unwrap().code.consts[i as usize].clone()
    }
    fn kstr(&self, i: u32) -> JsStr {
        match &self.frames.last().unwrap().code.consts[i as usize] {
            Value::Str(s) => s.clone(),
            _ => JsStr::new(""),
        }
    }
    fn is_strict(&self) -> bool {
        self.frames.last().map(|f| f.code.strict).unwrap_or(false)
    }

    /// Runs until the frame at index `base` completes or suspends.
    pub fn run(&mut self, base: usize) -> JsResult<Value> {
        let r = self.run_loop(base);
        if let Some(p) = self.prof.as_deref_mut() {
            p.sync(self.frames.len(), self.steps);
        }
        r
    }

    fn run_loop(&mut self, base: usize) -> JsResult<Value> {
        loop {
            if let Some(res) = top!(self).resume.take() {
                let r = match res {
                    Resume::Throw(v) => self.throw_into(v, base),
                    Resume::Return(v) => self.return_unwind(v, base),
                };
                match r {
                    Ok(Some(v)) => return Ok(v),
                    Ok(None) => continue,
                    Err(e) => return Err(e),
                }
            }
            match self.exec(base) {
                Ok(v) => return Ok(v),
                Err(Ctl::Throw(v)) => match self.throw_into(v, base) {
                    Ok(Some(r)) => return Ok(r),
                    Ok(None) => continue,
                    Err(e) => return Err(e),
                },
                Err(other) => {
                    self.frames.truncate(base);
                    return Err(other);
                }
            }
        }
    }

    /// Delivers an exception to the innermost handler at or above `base`.
    fn throw_into(&mut self, v: Value, base: usize) -> JsResult<Option<Value>> {
        // A debugger watching raised exceptions sees this one with the frames
        // that raised it still standing.
        if self.debug.is_some() {
            let handled = self
                .frames
                .iter()
                .skip(base)
                .any(|f| !f.handlers.is_empty());
            crate::debug::exception_hook(self, &v, !handled)?;
        }
        loop {
            {
                let f = top!(self);
                if let Some(h) = f.handlers.pop() {
                    f.stack.truncate(h.depth as usize);
                    f.stack.push(v);
                    if h.finally {
                        f.stack.push(Value::Num(1.0));
                    }
                    f.pc = h.pc as usize;
                    return Ok(None);
                }
            }
            let mut frame = self.frames.pop().unwrap();
            let at_base = self.frames.len() == base;
            let kind = std::mem::replace(&mut frame.kind, FrameKind::Normal);
            self.pool.recycle(frame);
            match kind {
                FrameKind::Async { promise, first, .. } => {
                    self.reject_promise(&promise, v);
                    let pv = Value::Obj(promise);
                    self.exit = Exit::Return;
                    if at_base {
                        return Ok(Some(pv));
                    }
                    if first {
                        self.push(pv);
                        return Ok(None);
                    }
                    return Ok(Some(Value::Undefined));
                }
                _ => {
                    if at_base {
                        return Err(Ctl::Throw(v));
                    }
                }
            }
            // keep unwinding into the caller
        }
    }

    fn return_unwind(&mut self, v: Value, base: usize) -> JsResult<Option<Value>> {
        {
            let f = top!(self);
            while let Some(h) = f.handlers.pop() {
                if h.finally {
                    f.stack.truncate(h.depth as usize);
                    f.stack.push(v);
                    f.stack.push(Value::Num(2.0));
                    f.pc = h.pc as usize;
                    return Ok(None);
                }
            }
        }
        self.return_value(v, base)
    }

    fn return_value(&mut self, v: Value, base: usize) -> JsResult<Option<Value>> {
        let mut frame = self.frames.pop().unwrap();
        let at_base = self.frames.len() == base;
        let kind = std::mem::replace(&mut frame.kind, FrameKind::Normal);
        let result = match kind {
            FrameKind::Normal => v,
            FrameKind::Construct(this) => {
                if let Value::Obj(_) = v {
                    v
                } else if frame.code.kind == FuncKind::DerivedConstructor {
                    let t = match frame.code.this_slot {
                        Some(s) => match &frame.locals[s as usize] {
                            Local::V(x) => x.clone(),
                            Local::C(c) => c.borrow().clone(),
                        },
                        None => Value::Empty,
                    };
                    if let Value::Empty = t {
                        let e = self.reference_error("Must call super constructor in derived class before accessing 'this' or returning from derived constructor");
                        let Ctl::Throw(ev) = e else { return Err(e) };
                        if at_base {
                            return Err(Ctl::Throw(ev));
                        }
                        return self.throw_into(ev, base);
                    }
                    if !v.is_undefined() {
                        let e = self
                            .type_error("Derived constructors may only return object or undefined");
                        let Ctl::Throw(ev) = e else { return Err(e) };
                        if at_base {
                            return Err(Ctl::Throw(ev));
                        }
                        return self.throw_into(ev, base);
                    }
                    t
                } else {
                    this
                }
            }
            FrameKind::Generator(_) => {
                self.exit = Exit::Return;
                v
            }
            FrameKind::Async { promise, .. } => {
                self.resolve_promise(&promise, v)?;
                Value::Obj(promise)
            }
        };
        self.pool.recycle(frame);
        if at_base {
            self.exit = Exit::Return;
            return Ok(Some(result));
        }
        self.push(result);
        Ok(None)
    }

    fn tdz_error(&mut self, name: &str) -> Ctl {
        if name == "this" {
            return self.reference_error("Must call super constructor in derived class before accessing 'this' or returning from derived constructor");
        }
        self.reference_error(format!("Cannot access '{name}' before initialization"))
    }

    pub fn make_closure(&mut self, code: Rc<Code>, caps: Rc<[CellRef]>) -> Obj {
        let ctor = match code.kind {
            FuncKind::Normal if !code.is_async && !code.is_generator => CtorKind::Base,
            FuncKind::BaseConstructor => CtorKind::Base,
            FuncKind::DerivedConstructor => CtorKind::Derived,
            _ => CtorKind::None,
        };
        let proto = if code.is_generator {
            if code.is_async {
                self.intr.async_generator_function_proto.clone()
            } else {
                self.intr.generator_function_proto.clone()
            }
        } else if code.is_async {
            self.intr.async_function_proto.clone()
        } else {
            self.intr.function_proto.clone()
        };
        let name = code.name.clone();
        let length = code.length;
        let is_gen = code.is_generator;
        let is_async_gen = code.is_generator && code.is_async;
        let f = self.obj_with(
            Some(proto),
            Kind::Function(Box::new(FuncData {
                imp: FuncImpl::Closure {
                    code,
                    captures: caps,
                },
                ctor,
                class_ctor: false,
                home: None,
                fields: None,
            })),
        );
        {
            let mut d = f.borrow_mut();
            d.props.insert(
                Key::str("length"),
                Prop::data(Value::Num(length as f64), CONFIGURABLE),
            );
            d.props
                .insert(Key::str("name"), Prop::data(Value::Str(name), CONFIGURABLE));
        }
        if ctor == CtorKind::Base {
            let p = self.new_object();
            p.set_hidden("constructor", Value::Obj(f.clone()));
            f.borrow_mut()
                .props
                .insert(Key::str("prototype"), Prop::data(Value::Obj(p), WRITABLE));
        } else if is_gen {
            let gp = if is_async_gen {
                self.intr.async_generator_proto.clone()
            } else {
                self.intr.generator_proto.clone()
            };
            let p = self.obj_with(Some(gp), Kind::Ordinary);
            f.borrow_mut()
                .props
                .insert(Key::str("prototype"), Prop::data(Value::Obj(p), WRITABLE));
        }
        f
    }

    fn closure_captures(&mut self, code: &Code) -> Rc<[CellRef]> {
        let f = top!(self);
        let mut caps = Vec::with_capacity(code.captures.len());
        for c in &code.captures {
            match c {
                Capture::Local(s) => {
                    let s = *s as usize;
                    let cell = match &f.locals[s] {
                        Local::C(c) => c.clone(),
                        Local::V(v) => {
                            let c = new_cell(v.clone());
                            f.locals[s] = Local::C(c.clone());
                            c
                        }
                    };
                    caps.push(cell);
                }
                Capture::Free(i) => caps.push(f.captures[*i as usize].clone()),
            }
        }
        caps.into()
    }

    fn set_home(f: &Value, home: &Obj) {
        if let Value::Obj(fo) = f {
            if let Kind::Function(fd) = &mut fo.borrow_mut().kind {
                fd.home = Some(home.clone());
            }
        }
    }

    fn define_method(&mut self, target: &Obj, key: Key, f: Value, kind: u8, enumerable: bool) {
        Self::set_home(&f, target);
        let flags = if enumerable { ALL } else { HIDDEN };
        match kind {
            0 => {
                target.borrow_mut().props.insert(key, Prop::data(f, flags));
            }
            _ => {
                // Accessor: merge with an existing half.
                if let Value::Obj(fo) = &f {
                    let kname = self.key_display(&key);
                    let prefix = if kind == 1 { "get" } else { "set" };
                    fo.set_prop(
                        "name",
                        Value::string(format!("{prefix} {kname}")),
                        CONFIGURABLE,
                    );
                }
                let fo = f.as_obj().cloned();
                let mut d = target.borrow_mut();
                let (mut g, mut s) = match d.props.get(&key) {
                    Some(Prop {
                        slot: Slot::Accessor(g, s),
                        ..
                    }) => (g.clone(), s.clone()),
                    _ => (None, None),
                };
                if kind == 1 {
                    g = fo;
                } else {
                    s = fo;
                }
                d.props.insert(
                    key,
                    Prop {
                        slot: Slot::Accessor(g, s),
                        flags: if enumerable {
                            ENUMERABLE | CONFIGURABLE
                        } else {
                            CONFIGURABLE
                        },
                    },
                );
            }
        }
    }

    fn strict_set_error(&mut self, obj: &Value, key: &Key) -> Ctl {
        let k = self.key_display(key);
        match obj {
            Value::Obj(o) => {
                let exists = o.borrow().props.get(key).is_some()
                    || matches!(o.borrow().kind, Kind::Array(_));
                let tn = self.constructor_name(o).unwrap_or_else(|| "Object".into());
                if exists || !o.borrow().extensible && o.borrow().props.get(key).is_some() {
                    self.type_error(format!(
                        "Cannot assign to read only property '{k}' of object '#<{tn}>'"
                    ))
                } else if !o.borrow().extensible {
                    self.type_error(format!("Cannot add property {k}, object is not extensible"))
                } else {
                    self.type_error(format!(
                        "Cannot assign to read only property '{k}' of object '#<{tn}>'"
                    ))
                }
            }
            Value::Str(s) => {
                let s = s.to_string();
                self.type_error(format!("Cannot create property '{k}' on string '{s}'"))
            }
            other => {
                let t = other.type_of();
                let d = self.display_primitive(other);
                self.type_error(format!("Cannot create property '{k}' on {t} '{d}'"))
            }
        }
    }

    fn do_set(&mut self, obj: &Value, key: Key, v: Value) -> JsResult<()> {
        let ok = self.set(obj, key.clone(), v)?;
        if !ok && self.is_strict() {
            return Err(self.strict_set_error(obj, &key));
        }
        Ok(())
    }

    fn get_elem(&mut self, obj: &Value, key: &Value) -> JsResult<Value> {
        if let (Value::Obj(o), Value::Num(n)) = (obj, key) {
            let i = *n as usize;
            if i as f64 == *n {
                if let Kind::Array(v) = &o.borrow().kind {
                    if let Some(x) = v.get(i) {
                        if !matches!(x, Value::Empty) {
                            return Ok(x.clone());
                        }
                    }
                }
            }
        }
        if let (Value::Str(s), Value::Num(n)) = (obj, key) {
            let i = *n as usize;
            if i as f64 == *n && s.is_ascii() {
                return Ok(match s.as_bytes().get(i) {
                    Some(b) => Value::string((*b as char).to_string()),
                    None => Value::Undefined,
                });
            }
        }
        if obj.is_nullish() {
            let k = self.to_key(key)?;
            return self.get(obj, &k);
        }
        let k = self.to_key(key)?;
        self.get(obj, &k)
    }

    fn set_elem(&mut self, obj: &Value, key: &Value, v: Value) -> JsResult<()> {
        if let (Value::Obj(o), Value::Num(n)) = (obj, key) {
            let i = *n as usize;
            if i as f64 == *n {
                let mut d = o.borrow_mut();
                let plain = !d.elems_frozen && !d.elems_sealed && d.extensible;
                if let Kind::Array(arr) = &mut d.kind {
                    if plain {
                        if i < arr.len() {
                            arr[i] = v;
                            return Ok(());
                        }
                        if i == arr.len() {
                            arr.push(v);
                            return Ok(());
                        }
                    }
                }
            }
        }
        let k = self.to_key(key)?;
        self.do_set(obj, k, v)
    }

    fn private_key(&mut self, k: &Value) -> Rc<Symbol> {
        match k {
            Value::Sym(s) => s.clone(),
            _ => crate::vm::new_symbol(Some("#?")),
        }
    }

    fn private_name(sym: &Rc<Symbol>) -> String {
        sym.desc.as_ref().map(|d| d.to_string()).unwrap_or_default()
    }

    /// Iterator record for `obj` (`text` names the expression in errors).
    pub fn get_iterator_text(
        &mut self,
        obj: &Value,
        text: Option<&str>,
        spread: bool,
    ) -> JsResult<(Value, Value)> {
        let m = if obj.is_nullish() {
            Value::Undefined
        } else {
            self.get(obj, &Key::Sym(self.syms.iterator.clone()))?
        };
        if !m.is_callable() {
            let msg = match text {
                Some(t) => format!("{t} is not iterable"),
                None => {
                    if spread && !obj.is_nullish() {
                        "Spread syntax requires ...iterable[Symbol.iterator] to be a function"
                            .to_string()
                    } else {
                        let d = match obj {
                            Value::Obj(o) if o.is_callable() => "function".to_string(),
                            Value::Obj(_) => "object".to_string(),
                            Value::Undefined => "undefined".to_string(),
                            Value::Null => "object null".to_string(),
                            Value::Str(s) => format!("string \"{}\"", s.as_str()),
                            other => {
                                let t = other.type_of();
                                let d = self.display_primitive(other);
                                format!("{t} {d}")
                            }
                        };
                        format!(
                            "{d} is not iterable (cannot read property Symbol(Symbol.iterator))"
                        )
                    }
                }
            };
            return Err(self.type_error(msg));
        }
        let it = self.call(&m, obj.clone(), vec![])?;
        if !matches!(it, Value::Obj(_)) {
            return Err(self.type_error("Result of the Symbol.iterator method is not an object"));
        }
        let next = self.get_str(&it, "next")?;
        Ok((it, next))
    }

    pub fn get_iterator(&mut self, obj: &Value) -> JsResult<(Value, Value)> {
        self.get_iterator_text(obj, None, false)
    }

    /// One step: Some(value) or None when done.
    pub fn iter_step(&mut self, it: &Value, next: &Value) -> JsResult<Option<Value>> {
        // Fast path: built-in array iterator.
        if let (Value::Obj(io), Value::Obj(no)) = (it, next) {
            if no.ptr_eq(&self.intr.array_iter_next) {
                if let Some(r) = self.array_iter_fast(io)? {
                    return Ok(r);
                }
            }
        }
        let r = self.call(next, it.clone(), vec![])?;
        if !matches!(r, Value::Obj(_)) {
            let d = self.describe_for_error(&r);
            return Err(self.type_error(format!("Iterator result {d} is not an object")));
        }
        let done = self.get_str(&r, "done")?.truthy();
        if done {
            return Ok(None);
        }
        Ok(Some(self.get_str(&r, "value")?))
    }

    /// Steps a built-in array iterator without allocating result objects.
    /// Outer None: not applicable.
    pub fn array_iter_fast(&mut self, io: &Obj) -> JsResult<Option<Option<Value>>> {
        let (target, index, kind, done) = match &io.borrow().kind {
            Kind::ArrayIter {
                target,
                index,
                kind,
                done,
            } => (target.clone(), *index, *kind, *done),
            _ => return Ok(None),
        };
        if done {
            return Ok(Some(None));
        }
        let len = match &target {
            Value::Obj(t) => match &t.borrow().kind {
                Kind::Array(v) => Some(v.len()),
                _ => None,
            },
            _ => None,
        };
        let len = match len {
            Some(l) => l,
            None => self.length_of(&target)?,
        };
        let set_done = |io: &Obj, idx: usize, d: bool| {
            if let Kind::ArrayIter { index, done, .. } = &mut io.borrow_mut().kind {
                *index = idx;
                *done = d;
            }
        };
        if index >= len {
            set_done(io, index, true);
            return Ok(Some(None));
        }
        set_done(io, index + 1, false);
        let v = match kind {
            IterKind::Keys => Value::Num(index as f64),
            IterKind::Values => self.get_index(&target, index)?,
            IterKind::Entries => {
                let x = self.get_index(&target, index)?;
                self.arr(vec![Value::Num(index as f64), x])
            }
        };
        Ok(Some(Some(v)))
    }

    pub fn iter_close(&mut self, it: &Value) -> JsResult<()> {
        if let Value::Obj(o) = it {
            if let Kind::ArrayIter { .. } = o.borrow().kind {
                return Ok(());
            }
        }
        let r = self.get_str(it, "return")?;
        if r.is_callable() {
            let res = self.call(&r, it.clone(), vec![])?;
            if !matches!(res, Value::Obj(_)) {
                return Err(self.type_error("Iterator result undefined is not an object"));
            }
        }
        Ok(())
    }

    /// Collects an iterable into a vector (fast for plain arrays).
    pub fn iterable_to_vec(&mut self, v: &Value) -> JsResult<Vec<Value>> {
        self.iterable_to_vec_text(v, None, false)
    }

    pub fn iterable_to_vec_text(
        &mut self,
        v: &Value,
        text: Option<&str>,
        spread: bool,
    ) -> JsResult<Vec<Value>> {
        if let Value::Obj(o) = v {
            if self.is_plain_array_iteration(o) {
                if let Kind::Array(arr) = &o.borrow().kind {
                    return Ok(arr
                        .iter()
                        .map(|x| {
                            if let Value::Empty = x {
                                Value::Undefined
                            } else {
                                x.clone()
                            }
                        })
                        .collect());
                }
            }
        }
        if let Value::Str(s) = v {
            if s.is_ascii() {
                return Ok(s
                    .bytes()
                    .map(|b| Value::string((b as char).to_string()))
                    .collect());
            }
        }
        let (it, next) = self.get_iterator_text(v, text, spread)?;
        let mut out = vec![];
        while let Some(x) = self.iter_step(&it, &next)? {
            out.push(x);
            if out.len() > crate::props::MAX_DENSE {
                return Err(self.range_error("Invalid array length"));
            }
        }
        Ok(out)
    }

    /// Array whose iteration is unobservably the default one.
    pub fn is_plain_array_iteration(&self, o: &Obj) -> bool {
        let d = o.borrow();
        if !matches!(d.kind, Kind::Array(_)) {
            return false;
        }
        if d.props.get(&Key::Sym(self.syms.iterator.clone())).is_some() {
            return false;
        }
        let Some(p) = &d.proto else { return false };
        if !p.ptr_eq(&self.intr.array_proto) {
            return false;
        }
        let ap = self.intr.array_proto.borrow();
        matches!(ap.props.get(&Key::Sym(self.syms.iterator.clone())), Some(Prop { slot: Slot::Data(Value::Obj(f)), .. }) if f.ptr_eq(&self.intr.array_values))
    }

    fn copy_data(&mut self, target: &Obj, src: &Value, exclude: &[Key]) -> JsResult<()> {
        let src_obj = match src {
            Value::Undefined | Value::Null => return Ok(()),
            Value::Obj(o) => o.clone(),
            Value::Str(_) => self.to_object(src)?,
            _ => return Ok(()),
        };
        let keys = self.own_keys(&src_obj)?;
        for k in keys {
            if exclude.iter().any(|e| e.same(&k)) {
                continue;
            }
            let Some(p) = self.get_own(&src_obj, &k)? else {
                continue;
            };
            if !p.enumerable() {
                continue;
            }
            let v = match p.slot {
                Slot::Data(v) => v,
                Slot::Accessor(Some(g), _) => {
                    self.call(&Value::Obj(g), Value::Obj(src_obj.clone()), vec![])?
                }
                Slot::Accessor(None, _) => Value::Undefined,
            };
            self.create_data_property(target, k, v)?;
        }
        Ok(())
    }

    fn for_in_keys(&mut self, v: &Value) -> JsResult<Vec<JsStr>> {
        let mut out: Vec<JsStr> = vec![];
        let mut seen: Vec<JsStr> = vec![];
        let mut cur = match v {
            Value::Undefined | Value::Null => return Ok(out),
            Value::Obj(o) => Some(o.clone()),
            _ => Some(self.to_object(v)?),
        };
        while let Some(c) = cur {
            let keys = self.own_keys(&c)?;
            for k in keys {
                if let Key::Str(s) = &k {
                    if seen.iter().any(|x| x == s) {
                        continue;
                    }
                    seen.push(s.clone());
                    if let Some(p) = self.get_own(&c, &k)? {
                        if p.enumerable() {
                            out.push(s.clone());
                        }
                    }
                }
            }
            cur = c.proto();
        }
        Ok(out)
    }

    fn template_object(&mut self, site: u32) -> Value {
        let code = self.frames.last().unwrap().code.clone();
        if let Some(o) = self.templates.get(&(code.uid, site)) {
            return Value::Obj(o.clone());
        }
        let (cooked, raw) = &code.templates[site as usize];
        let c: Vec<Value> = cooked
            .iter()
            .map(|x| x.clone().map(Value::Str).unwrap_or(Value::Undefined))
            .collect();
        let r: Vec<Value> = raw.iter().map(|x| Value::Str(x.clone())).collect();
        let arr = self.new_array(c);
        let raw_arr = self.new_array(r);
        raw_arr.borrow_mut().elems_frozen = true;
        raw_arr.borrow_mut().extensible = false;
        arr.borrow_mut()
            .props
            .insert(Key::str("raw"), Prop::data(Value::Obj(raw_arr), 0));
        arr.borrow_mut().elems_frozen = true;
        arr.borrow_mut().extensible = false;
        self.templates.insert((code.uid, site), arr.clone());
        Value::Obj(arr)
    }

    /// `f(...)` / `o.f(...)` straight from the operand stack, for the two
    /// common callees: an ordinary closure that does not read its argument
    /// list after entry (its frame is built from the caller's stack, no
    /// argument vector in between), and a native function (called with a
    /// pooled argument vector). Returns false, having changed nothing, for
    /// every other callee (bound functions, class constructors, generators,
    /// async functions, proxies, non-callables), when the stack is full, and
    /// while profiling; the general call path handles those.
    #[inline]
    fn call_from_stack(&mut self, argc: usize, method: bool) -> JsResult<bool> {
        enum Target {
            Closure(Obj, Rc<Code>, Rc<[CellRef]>),
            Native(Obj, NativeFn),
        }
        if self.prof.is_some() || self.frames.len() >= MAX_FRAMES {
            return Ok(false);
        }
        let target = {
            let f = self.frames.last().unwrap();
            let Value::Obj(fo) = &f.stack[f.stack.len() - argc - 1] else {
                return Ok(false);
            };
            let d = fo.borrow();
            let Kind::Function(fd) = &d.kind else {
                return Ok(false);
            };
            match &fd.imp {
                FuncImpl::Closure { code, captures }
                    if !(fd.class_ctor
                        || code.needs_args
                        || code.is_generator
                        || code.is_async) =>
                {
                    Target::Closure(fo.clone(), code.clone(), captures.clone())
                }
                FuncImpl::Native { f, .. } => Target::Native(fo.clone(), *f),
                _ => return Ok(false),
            }
        };
        match target {
            Target::Closure(fo, code, caps) => {
                self.charge_first_run(&code);
                let n = code.nlocals as usize;
                let k = (code.simple_params.unwrap_or(0) as usize).min(argc).min(n);
                let mut locals = self.pool.locals.pop().unwrap_or_default();
                locals.reserve(n);
                let this = {
                    let f = self.frames.last_mut().unwrap();
                    let at = f.stack.len() - argc;
                    // The parameters' arguments move into their slots; the rest
                    // of the arguments are dropped, the other slots start
                    // undefined.
                    if argc > 0 {
                        locals.extend(f.stack.drain(at..at + k).map(Local::V));
                        f.stack.truncate(at);
                    }
                    for _ in k..n {
                        locals.push(Local::V(Value::Undefined));
                    }
                    f.stack.pop();
                    if method {
                        f.stack.pop().unwrap_or(Value::Undefined)
                    } else {
                        Value::Undefined
                    }
                };
                let frame = self.finish_frame(
                    Some(&fo),
                    code,
                    caps,
                    this,
                    locals,
                    Vec::new(),
                    Value::Undefined,
                    FrameKind::Normal,
                );
                self.frames.push(frame);
            }
            Target::Native(fo, native) => {
                let mut args = self.pool.vals.pop().unwrap_or_default();
                let this = {
                    let f = self.frames.last_mut().unwrap();
                    let at = f.stack.len() - argc;
                    args.extend(f.stack.drain(at..));
                    f.stack.pop();
                    if method {
                        f.stack.pop().unwrap_or(Value::Undefined)
                    } else {
                        Value::Undefined
                    }
                };
                self.natives.push(NativeMark {
                    depth: self.frames.len(),
                    callee: fo.clone(),
                    this: this.clone(),
                    construct: false,
                });
                let mut a = Args {
                    this,
                    args,
                    new_target: None,
                    callee: fo,
                };
                let r = native(self, &mut a);
                self.natives.pop();
                self.pool.give_vals(std::mem::take(&mut a.args));
                let v = r?;
                self.push(v);
            }
        }
        Ok(true)
    }

    fn call_op(&mut self, f: Value, this: Value, args: Vec<Value>, text: u32) -> JsResult<()> {
        // Fast check for non-callables to build V8's message.
        match self.invoke(&f, this, args, None) {
            Ok(Invoked::Done(v)) => {
                self.push(v);
                Ok(())
            }
            Ok(Invoked::Pushed) => Ok(()),
            Err(e) => {
                if !f.is_callable() {
                    let t = self.kstr(text);
                    return Err(self.type_error(format!("{} is not a function", t.as_str())));
                }
                Err(e)
            }
        }
    }

    fn spread_args(&mut self, arr: Value) -> Vec<Value> {
        match &arr {
            Value::Obj(o) => match &o.borrow().kind {
                Kind::Array(v) => v
                    .iter()
                    .map(|x| {
                        if let Value::Empty = x {
                            Value::Undefined
                        } else {
                            x.clone()
                        }
                    })
                    .collect(),
                _ => vec![],
            },
            _ => vec![],
        }
    }

    /// The uncatchable error that stops a program which ran out of steps.
    #[cold]
    #[inline(never)]
    fn step_limit(&mut self) -> Ctl {
        self.set_site_here();
        let e = self.make_error(ErrKind::Error, "execution step limit exceeded");
        // A TimeoutError "class" so reports read `TimeoutError: ...`.
        let proto = self.new_object();
        proto.borrow_mut().proto = Some(self.intr.error_protos[0].clone());
        proto.set_hidden("name", Value::str("TimeoutError"));
        let ctor = self.native_fn("TimeoutError", 1, |_vm, _a| Ok(Value::Undefined));
        proto.set_hidden("constructor", Value::Obj(ctor));
        e.borrow_mut().proto = Some(proto);
        Ctl::Fatal(Value::Obj(e))
    }

    /// Fetches, counts and budget-checks the next instruction with the
    /// debugger and profiler hooks (the slow way, one at a time).
    #[inline(never)]
    fn fetch_hooked(&mut self) -> JsResult<Op> {
        // A debugger sees every source line before it runs.
        if self.debug.is_some() {
            crate::debug::line_hook(self)?;
        }
        let op = {
            let f = top!(self);
            let pc = f.pc;
            f.pc += 1;
            f.code.ops[pc]
        };
        self.steps += 1;
        if let Some(p) = self.prof.as_deref_mut() {
            let f = self.frames.last().unwrap();
            p.on_op(self.frames.len(), &f.code, &op, self.steps - 1);
        }
        if self.steps > self.budget {
            return Err(self.step_limit());
        }
        Ok(op)
    }

    /// Runs, inline, the instructions that touch nothing but the current
    /// frame (loads, stores, constants, stack shuffles, jumps, number
    /// arithmetic and comparisons, reads and writes of plain own or inherited
    /// data properties, array elements) and returns the first one that needs
    /// the general path, already fetched and counted. Each instruction is
    /// counted and budget-checked exactly as the general path does, and each
    /// either completes here with the general path's result or is handed over
    /// before it has changed anything.
    #[inline(always)]
    fn exec_fast(&mut self) -> JsResult<()> {
        let fp: *mut Frame = self.frames.last_mut().unwrap();
        // SAFETY: nothing below touches `self.frames` (the arms use the frame,
        // the step counter and the budget), so the top frame stays where it is
        // while `f` is used; `code` lives in the `Rc` the frame holds.
        let f = unsafe { &mut *fp };
        let code: &Code = unsafe { &*Rc::as_ptr(&f.code) };
        let ops = &code.ops[..];
        // The program counter and the step count live in registers here and
        // are written back whenever control leaves the loop.
        let mut pc = f.pc;
        let mut steps = self.steps;
        let budget = self.budget;
        // Leaves the instruction at `pc - 1` to the general path.
        macro_rules! leave {
            () => {{
                f.pc = pc;
                self.steps = steps;
                return Ok(());
            }};
        }
        // A binary operator on two numbers, replacing them with the result;
        // anything else is left to the general path.
        macro_rules! num2 {
            (|$x:ident, $y:ident| $r:expr) => {{
                let n = f.stack.len();
                let (Value::Num($x), Value::Num($y)) = (&f.stack[n - 2], &f.stack[n - 1]) else {
                    leave!();
                };
                let ($x, $y) = (*$x, *$y);
                let r = $r;
                // SAFETY: both operands are numbers, which own nothing, so they
                // can be overwritten and forgotten without dropping.
                unsafe {
                    std::ptr::write(f.stack.as_mut_ptr().add(n - 2), r);
                    f.stack.set_len(n - 1);
                }
            }};
        }
        loop {
            let op = &ops[pc];
            pc += 1;
            steps += 1;
            if steps > budget {
                f.pc = pc;
                self.steps = steps;
                return Err(self.step_limit());
            }
            match *op {
                Op::Undef => f.stack.push(Value::Undefined),
                Op::Null => f.stack.push(Value::Null),
                Op::True => f.stack.push(Value::Bool(true)),
                Op::False => f.stack.push(Value::Bool(false)),
                Op::Num(n) => f.stack.push(Value::Num(n)),
                Op::Const(i) => f.stack.push(code.consts[i as usize].clone()),
                Op::Pop => {
                    f.stack.pop();
                }
                Op::Dup => {
                    let v = f.stack.last().unwrap().clone();
                    f.stack.push(v);
                }
                Op::Load(s) => {
                    let v = match &f.locals[s as usize] {
                        Local::V(v) => v.clone(),
                        Local::C(c) => c.borrow().clone(),
                    };
                    if let Value::Empty = v {
                        leave!();
                    }
                    f.stack.push(v);
                }
                Op::Store(s) => {
                    let slot = &mut f.locals[s as usize];
                    match slot {
                        Local::V(x) => {
                            if let Value::Empty = x {
                                leave!();
                            }
                            *x = f.stack.pop().unwrap();
                        }
                        Local::C(c) => {
                            let mut c = c.borrow_mut();
                            if let Value::Empty = *c {
                                leave!();
                            }
                            *c = f.stack.pop().unwrap();
                        }
                    }
                }
                Op::Init(s) => {
                    let v = f.stack.pop().unwrap();
                    match &mut f.locals[s as usize] {
                        Local::V(x) => *x = v,
                        Local::C(c) => *c.borrow_mut() = v,
                    }
                }
                Op::LoadFree(i) => {
                    let v = f.captures[i as usize].borrow().clone();
                    if let Value::Empty = v {
                        leave!();
                    }
                    f.stack.push(v);
                }
                Op::StoreFree(i) => {
                    let mut c = f.captures[i as usize].borrow_mut();
                    if let Value::Empty = *c {
                        leave!();
                    }
                    *c = f.stack.pop().unwrap();
                }
                Op::InitFree(i) => {
                    let v = f.stack.pop().unwrap();
                    *f.captures[i as usize].borrow_mut() = v;
                }
                Op::DeclLet(s) => {
                    f.locals[s as usize] = if code.is_cell[s as usize] {
                        Local::C(new_cell(Value::Empty))
                    } else {
                        Local::V(Value::Empty)
                    };
                }
                Op::CopyCell(s) => {
                    if let Local::C(c) = &f.locals[s as usize] {
                        let v = c.borrow().clone();
                        f.locals[s as usize] = Local::C(new_cell(v));
                    }
                }
                Op::LoadGlobal(c) => {
                    // An own data property of the global object; anything else
                    // (accessors, the prototype chain, named elements, a
                    // ReferenceError) takes the general path.
                    let Value::Str(name) = &code.consts[c as usize] else {
                        leave!();
                    };
                    if !name.is_canon() {
                        leave!();
                    }
                    let id = Rc::as_ptr(&name.0) as *const u8 as usize;
                    let v = {
                        let g = self.global.borrow();
                        match g.props.find_ident(id).map(|i| &g.props.entries[i].1.slot) {
                            Some(Slot::Data(v)) => v.clone(),
                            _ => leave!(),
                        }
                    };
                    f.stack.push(v);
                }
                Op::Jump(t) => pc = t as usize,
                Op::JumpIfFalse(t) => {
                    if !f.stack.pop().unwrap().truthy() {
                        pc = t as usize;
                    }
                }
                Op::JumpIfTrue(t) => {
                    if f.stack.pop().unwrap().truthy() {
                        pc = t as usize;
                    }
                }
                Op::JumpIfFalseKeep(t) => {
                    if !f.stack.last().unwrap().truthy() {
                        pc = t as usize;
                    } else {
                        f.stack.pop();
                    }
                }
                Op::JumpIfTrueKeep(t) => {
                    if f.stack.last().unwrap().truthy() {
                        pc = t as usize;
                    } else {
                        f.stack.pop();
                    }
                }
                Op::JumpIfNotNullishKeep(t) => {
                    if !f.stack.last().unwrap().is_nullish() {
                        pc = t as usize;
                    } else {
                        f.stack.pop();
                    }
                }
                Op::JumpIfNotUndefKeep(t) => {
                    if !f.stack.last().unwrap().is_undefined() {
                        pc = t as usize;
                    } else {
                        f.stack.pop();
                    }
                }
                Op::StrictEq => {
                    let b = f.stack.pop().unwrap();
                    let a = f.stack.pop().unwrap();
                    f.stack.push(Value::Bool(strict_equals(&a, &b)));
                }
                Op::StrictNe => {
                    let b = f.stack.pop().unwrap();
                    let a = f.stack.pop().unwrap();
                    f.stack.push(Value::Bool(!strict_equals(&a, &b)));
                }
                Op::Not => {
                    let v = f.stack.pop().unwrap();
                    f.stack.push(Value::Bool(!v.truthy()));
                }
                Op::Typeof => {
                    let v = f.stack.pop().unwrap();
                    f.stack.push(Value::Str(JsStr::intern(v.type_of())));
                }
                Op::Add => num2!(|x, y| Value::Num(x + y)),
                Op::Sub => num2!(|x, y| Value::Num(x - y)),
                Op::Mul => num2!(|x, y| Value::Num(x * y)),
                Op::Div => num2!(|x, y| Value::Num(x / y)),
                Op::Mod => num2!(|x, y| Value::Num(crate::conv::num_arith(Arith::Mod, x, y))),
                Op::BitAnd => num2!(|x, y| Value::Num(crate::conv::num_arith(Arith::BitAnd, x, y))),
                Op::BitOr => num2!(|x, y| Value::Num(crate::conv::num_arith(Arith::BitOr, x, y))),
                Op::BitXor => num2!(|x, y| Value::Num(crate::conv::num_arith(Arith::BitXor, x, y))),
                Op::Shl => num2!(|x, y| Value::Num(crate::conv::num_arith(Arith::Shl, x, y))),
                Op::Shr => num2!(|x, y| Value::Num(crate::conv::num_arith(Arith::Shr, x, y))),
                Op::UShr => num2!(|x, y| Value::Num(crate::conv::num_arith(Arith::UShr, x, y))),
                Op::Lt => num2!(|x, y| Value::Bool(x < y)),
                Op::Gt => num2!(|x, y| Value::Bool(x > y)),
                Op::Le => num2!(|x, y| Value::Bool(x <= y)),
                Op::Ge => num2!(|x, y| Value::Bool(x >= y)),
                Op::Inc => {
                    let Some(Value::Num(x)) = f.stack.last_mut() else {
                        leave!();
                    };
                    *x += 1.0;
                }
                Op::Dec => {
                    let Some(Value::Num(x)) = f.stack.last_mut() else {
                        leave!();
                    };
                    *x -= 1.0;
                }
                Op::EnterTry(h, fin) => {
                    let depth = f.stack.len() as u32;
                    f.handlers.push(Handler {
                        pc: h,
                        depth,
                        finally: fin,
                    });
                }
                Op::ExitTry => {
                    f.handlers.pop();
                }
                Op::PushPc(t) => f.stack.push(Value::Num(t as f64)),
                Op::Nop => {}
                Op::Arg(i) => {
                    let v = f.args.get(i as usize).cloned().unwrap_or(Value::Undefined);
                    f.stack.push(v);
                }
                Op::GetProp(c) | Op::GetPropKeep(c) => {
                    let Value::Str(name) = &code.consts[c as usize] else {
                        leave!();
                    };
                    let recv = f.stack.last().unwrap();
                    let v = match plain_get(recv, name, &code.hints[pc - 1]) {
                        Some(v) => v,
                        None => {
                            // A method of a primitive (`s.charCodeAt`, `n.toFixed`):
                            // looked up on its prototype, whose exotic parts
                            // (a string's indices and length) the name misses.
                            let proto = match recv {
                                Value::Str(_) if crate::numconv::array_index(name).is_none() => {
                                    &self.intr.string_proto
                                }
                                Value::Num(_) => &self.intr.number_proto,
                                Value::Bool(_) => &self.intr.boolean_proto,
                                _ => leave!(),
                            };
                            match proto_get(proto, name, &code.hints[pc - 1]) {
                                Some(v) => v,
                                None => leave!(),
                            }
                        }
                    };
                    if let Op::GetProp(_) = op {
                        *f.stack.last_mut().unwrap() = v;
                    } else {
                        f.stack.push(v);
                    }
                }
                Op::SetProp(c) => {
                    let Value::Str(name) = &code.consts[c as usize] else {
                        leave!();
                    };
                    let n = f.stack.len();
                    if !plain_set(&f.stack[n - 2], name, &f.stack[n - 1], &code.hints[pc - 1]) {
                        leave!();
                    }
                    // [obj value] -> value
                    let v = f.stack.pop().unwrap();
                    *f.stack.last_mut().unwrap() = v;
                }
                Op::GetElem => {
                    let n = f.stack.len();
                    let Some(v) =
                        element_get(&f.stack[n - 2], &f.stack[n - 1], &code.hints[pc - 1])
                    else {
                        leave!();
                    };
                    f.stack.truncate(n - 2);
                    f.stack.push(v);
                }
                Op::SetElem => {
                    // [obj key value] -> value
                    let n = f.stack.len();
                    if !element_set(
                        &f.stack[n - 3],
                        &f.stack[n - 2],
                        &f.stack[n - 1],
                        &code.hints[pc - 1],
                    ) {
                        leave!();
                    }
                    let v = f.stack.pop().unwrap();
                    f.stack.truncate(n - 3);
                    f.stack.push(v);
                }
                Op::DefineField(c) => {
                    // [obj value] -> obj, on an object literal being built.
                    let Value::Str(name) = &code.consts[c as usize] else {
                        leave!();
                    };
                    let n = f.stack.len();
                    if !define_field(&f.stack[n - 2], name, &f.stack[n - 1]) {
                        leave!();
                    }
                    f.stack.pop();
                }
                Op::Eq | Op::Ne => {
                    let n = f.stack.len();
                    let Some(eq) = simple_loose_eq(&f.stack[n - 2], &f.stack[n - 1]) else {
                        leave!();
                    };
                    let r = if matches!(*op, Op::Eq) { eq } else { !eq };
                    f.stack.truncate(n - 2);
                    f.stack.push(Value::Bool(r));
                }
                Op::NewObject => {
                    let proto = self.intr.object_proto.clone();
                    f.stack.push(Value::Obj(Obj::new(ObjData::new(
                        Some(proto),
                        Kind::Ordinary,
                    ))));
                }
                _ => leave!(),
            }
        }
    }

    /// Executes instructions until the base frame completes or suspends.
    fn exec(&mut self, base: usize) -> JsResult<Value> {
        loop {
            let op = if self.debug.is_some() || self.prof.is_some() {
                self.fetch_hooked()?
            } else {
                self.exec_fast()?;
                let f = self.frames.last().unwrap();
                f.code.ops[f.pc - 1]
            };
            match op {
                Op::Undef => self.push(Value::Undefined),
                Op::Null => self.push(Value::Null),
                Op::True => self.push(Value::Bool(true)),
                Op::False => self.push(Value::Bool(false)),
                Op::Num(n) => self.push(Value::Num(n)),
                Op::Const(i) => {
                    let v = self.konst(i);
                    self.push(v);
                }
                Op::Pop => {
                    self.pop();
                }
                Op::Dup => {
                    let v = self.peek(0).clone();
                    self.push(v);
                }
                Op::Dup2 => {
                    let a = self.peek(1).clone();
                    let b = self.peek(0).clone();
                    self.push(a);
                    self.push(b);
                }
                Op::Swap => {
                    let s = &mut top!(self).stack;
                    let n = s.len();
                    s.swap(n - 1, n - 2);
                }
                Op::Rot3 => {
                    let s = &mut top!(self).stack;
                    let c = s.pop().unwrap();
                    let n = s.len();
                    s.insert(n - 2, c);
                }
                Op::Rot4 => {
                    let s = &mut top!(self).stack;
                    let d = s.pop().unwrap();
                    let n = s.len();
                    s.insert(n - 3, d);
                }
                Op::Load(s) => {
                    let f = top!(self);
                    let v = match &f.locals[s as usize] {
                        Local::V(v) => v.clone(),
                        Local::C(c) => c.borrow().clone(),
                    };
                    if let Value::Empty = v {
                        let name = f.code.local_names[s as usize].to_string();
                        return Err(self.tdz_error(&name));
                    }
                    f.stack.push(v);
                }
                Op::Store(s) => {
                    let f = top!(self);
                    let v = f.stack.pop().unwrap();
                    let slot = &mut f.locals[s as usize];
                    let empty = match slot {
                        Local::V(x) => matches!(x, Value::Empty),
                        Local::C(c) => matches!(*c.borrow(), Value::Empty),
                    };
                    if empty {
                        let name = f.code.local_names[s as usize].to_string();
                        return Err(self.tdz_error(&name));
                    }
                    match slot {
                        Local::V(x) => *x = v,
                        Local::C(c) => *c.borrow_mut() = v,
                    }
                }
                Op::Init(s) => {
                    let f = top!(self);
                    let v = f.stack.pop().unwrap();
                    match &mut f.locals[s as usize] {
                        Local::V(x) => *x = v,
                        Local::C(c) => *c.borrow_mut() = v,
                    }
                }
                Op::LoadFree(i) => {
                    let f = top!(self);
                    let v = f.captures[i as usize].borrow().clone();
                    if let Value::Empty = v {
                        let name = f.code.free_names[i as usize].to_string();
                        return Err(self.tdz_error(&name));
                    }
                    f.stack.push(v);
                }
                Op::StoreFree(i) => {
                    let f = top!(self);
                    let v = f.stack.pop().unwrap();
                    let c = f.captures[i as usize].clone();
                    if let Value::Empty = *c.borrow() {
                        let name = f.code.free_names[i as usize].to_string();
                        return Err(self.tdz_error(&name));
                    }
                    *c.borrow_mut() = v;
                }
                Op::InitFree(i) => {
                    let f = top!(self);
                    let v = f.stack.pop().unwrap();
                    *f.captures[i as usize].borrow_mut() = v;
                }
                Op::DeclLet(s) => {
                    let f = top!(self);
                    f.locals[s as usize] = if f.code.is_cell[s as usize] {
                        Local::C(new_cell(Value::Empty))
                    } else {
                        Local::V(Value::Empty)
                    };
                }
                Op::CopyCell(s) => {
                    let f = top!(self);
                    if let Local::C(c) = &f.locals[s as usize] {
                        let v = c.borrow().clone();
                        f.locals[s as usize] = Local::C(new_cell(v));
                    }
                }
                Op::ConstAssign => return Err(self.type_error("Assignment to constant variable.")),
                Op::LoadGlobal(c) => {
                    let name = self.kstr(c);
                    let fast = match self.global.borrow().props.get(&Key::Str(name.clone())) {
                        Some(Prop {
                            slot: Slot::Data(v),
                            ..
                        }) => Some(v.clone()),
                        _ => None,
                    };
                    let v = match fast {
                        Some(v) => v,
                        None => {
                            let g = self.global.clone();
                            let k = Key::Str(name.clone());
                            if !self.has_property(&g, &k)? {
                                return Err(self
                                    .reference_error(format!("{} is not defined", name.as_str())));
                            }
                            self.get_from(&g, &k, &Value::Obj(g.clone()))?
                        }
                    };
                    self.push(v);
                }
                Op::StoreGlobal(c) => {
                    let name = self.kstr(c);
                    let v = self.pop();
                    let g = self.global.clone();
                    let k = Key::Str(name.clone());
                    if self.is_strict() && !self.has_property(&g, &k)? {
                        return Err(
                            self.reference_error(format!("{} is not defined", name.as_str()))
                        );
                    }
                    self.set_on(&g, k, v, &Value::Obj(g.clone()))?;
                }
                Op::TypeofGlobal(c) => {
                    let name = self.kstr(c);
                    let g = self.global.clone();
                    let k = Key::Str(name);
                    let t = if self.has_property(&g, &k)? {
                        let v = self.get_from(&g, &k, &Value::Obj(g.clone()))?;
                        v.type_of()
                    } else {
                        "undefined"
                    };
                    self.push(Value::Str(JsStr::intern(t)));
                }
                Op::GetProp(c) => {
                    let obj = self.pop();
                    let key = Key::Str(self.kstr(c));
                    let v = self.get(&obj, &key)?;
                    self.push(v);
                }
                Op::GetPropKeep(c) => {
                    let obj = self.peek(0).clone();
                    let key = Key::Str(self.kstr(c));
                    let v = self.get(&obj, &key)?;
                    self.push(v);
                }
                Op::SetProp(c) => {
                    let v = self.pop();
                    let obj = self.pop();
                    let key = Key::Str(self.kstr(c));
                    self.do_set(&obj, key, v.clone())?;
                    self.push(v);
                }
                Op::GetElem => {
                    let key = self.pop();
                    let obj = self.pop();
                    let v = self.get_elem(&obj, &key)?;
                    self.push(v);
                }
                Op::GetElemKeep => {
                    let key = self.pop();
                    let obj = self.peek(0).clone();
                    let v = self.get_elem(&obj, &key)?;
                    self.push(v);
                }
                Op::SetElem => {
                    let v = self.pop();
                    let key = self.pop();
                    let obj = self.pop();
                    if obj.is_nullish() {
                        let k = self.to_key(&key)?;
                        self.set(&obj, k, v.clone())?;
                    }
                    self.set_elem(&obj, &key, v.clone())?;
                    self.push(v);
                }
                Op::DeleteProp(c) => {
                    let obj = self.pop();
                    let key = Key::Str(self.kstr(c));
                    let r = self.delete_op(&obj, &key)?;
                    self.push(Value::Bool(r));
                }
                Op::DeleteElem => {
                    let key = self.pop();
                    let obj = self.pop();
                    let k = self.to_key(&key)?;
                    let r = self.delete_op(&obj, &k)?;
                    self.push(Value::Bool(r));
                }
                Op::SuperGet(c) => {
                    let home = self.pop();
                    let this = self.pop();
                    let key = Key::Str(self.kstr(c));
                    let v = self.super_get(&home, &key, &this)?;
                    self.push(v);
                }
                Op::SuperGetElem => {
                    let k = self.pop();
                    let home = self.pop();
                    let this = self.pop();
                    let key = self.to_key(&k)?;
                    let v = self.super_get(&home, &key, &this)?;
                    self.push(v);
                }
                Op::SuperGetKeep(c) => {
                    let home = self.pop();
                    let this = self.peek(0).clone();
                    let key = Key::Str(self.kstr(c));
                    let v = self.super_get(&home, &key, &this)?;
                    self.push(v);
                }
                Op::SuperGetElemKeep => {
                    let k = self.pop();
                    let home = self.pop();
                    let this = self.peek(0).clone();
                    let key = self.to_key(&k)?;
                    let v = self.super_get(&home, &key, &this)?;
                    self.push(v);
                }
                Op::SuperSet(c) => {
                    let v = self.pop();
                    let home = self.pop();
                    let this = self.pop();
                    let key = Key::Str(self.kstr(c));
                    if let Value::Obj(h) = &home {
                        if let Some(p) = h.proto() {
                            self.set_on(&p, key, v.clone(), &this)?;
                        }
                    }
                    self.push(v);
                }
                Op::SuperSetElem => {
                    let v = self.pop();
                    let k = self.pop();
                    let home = self.pop();
                    let this = self.pop();
                    let key = self.to_key(&k)?;
                    if let Value::Obj(h) = &home {
                        if let Some(p) = h.proto() {
                            self.set_on(&p, key, v.clone(), &this)?;
                        }
                    }
                    self.push(v);
                }
                Op::GetPrivate | Op::GetPrivateKeep => {
                    let k = self.pop();
                    let obj = if op == Op::GetPrivate {
                        self.pop()
                    } else {
                        self.peek(0).clone()
                    };
                    let sym = self.private_key(&k);
                    let v = self.private_get(&obj, &sym)?;
                    self.push(v);
                }
                Op::SetPrivate => {
                    let v = self.pop();
                    let k = self.pop();
                    let obj = self.pop();
                    let sym = self.private_key(&k);
                    self.private_set(&obj, &sym, v.clone())?;
                    self.push(v);
                }
                Op::DefinePrivate => {
                    let v = self.pop();
                    let k = self.pop();
                    let obj = self.peek(0).clone();
                    let sym = self.private_key(&k);
                    if let Value::Obj(o) = &obj {
                        let key = Key::Sym(sym.clone());
                        if o.borrow().props.get(&key).is_some() {
                            let n = Self::private_name(&sym);
                            return Err(self.type_error(format!(
                                "Cannot initialize {n} twice on the same object"
                            )));
                        }
                        o.borrow_mut().props.insert(key, Prop::data(v, WRITABLE));
                    }
                }
                Op::HasPrivate => {
                    let obj = self.pop();
                    let k = self.pop();
                    let sym = self.private_key(&k);
                    let Value::Obj(o) = &obj else {
                        let n = Self::private_name(&sym);
                        let d = self.display_primitive(&obj);
                        return Err(self.type_error(format!(
                            "Cannot use 'in' operator to search for '{n}' in {d}"
                        )));
                    };
                    let has = o.borrow().props.get(&Key::Sym(sym)).is_some();
                    self.push(Value::Bool(has));
                }
                Op::Call(argc, text) => {
                    if self.call_from_stack(argc as usize, false)? {
                        continue;
                    }
                    let args = self.pop_n(argc as usize);
                    let f = self.pop();
                    self.call_op(f, Value::Undefined, args, text)?;
                }
                Op::CallMethod(argc, text) => {
                    if self.call_from_stack(argc as usize, true)? {
                        continue;
                    }
                    let args = self.pop_n(argc as usize);
                    let f = self.pop();
                    let this = self.pop();
                    self.call_op(f, this, args, text)?;
                }
                Op::CallSpread(text) => {
                    let arr = self.pop();
                    let args = self.spread_args(arr);
                    let f = self.pop();
                    self.call_op(f, Value::Undefined, args, text)?;
                }
                Op::CallMethodSpread(text) => {
                    let arr = self.pop();
                    let args = self.spread_args(arr);
                    let f = self.pop();
                    let this = self.pop();
                    self.call_op(f, this, args, text)?;
                }
                Op::New(_, text) | Op::NewSpread(text) => {
                    let args = if let Op::New(argc, _) = op {
                        self.pop_n(argc as usize)
                    } else {
                        let arr = self.pop();
                        self.spread_args(arr)
                    };
                    let f = self.pop();
                    let t = self.kstr(text);
                    match self.construct_invoke(&f, args, None, Some(&t))? {
                        Invoked::Done(v) => self.push(v),
                        Invoked::Pushed => {}
                    }
                }
                Op::SuperCall(_) | Op::SuperCallSpread => {
                    let args = if let Op::SuperCall(argc) = op {
                        self.pop_n(argc as usize)
                    } else {
                        let arr = self.pop();
                        self.spread_args(arr)
                    };
                    let nt = self.pop();
                    let func = self.pop();
                    let parent = match &func {
                        Value::Obj(f) => f.proto().map(Value::Obj).unwrap_or(Value::Null),
                        _ => Value::Null,
                    };
                    if !self.is_constructor(&parent) {
                        let pd = match &parent {
                            Value::Null => "null".to_string(),
                            Value::Obj(p) if p.ptr_eq(&self.intr.function_proto) => {
                                "null".to_string()
                            }
                            v => self.describe_for_error(v),
                        };
                        let cn = match &func {
                            Value::Obj(f) => Self::func_name(f),
                            _ => String::new(),
                        };
                        let cn = if cn.is_empty() {
                            "anonymous class".to_string()
                        } else {
                            cn
                        };
                        return Err(self.type_error(format!(
                            "Super constructor {pd} of {cn} is not a constructor"
                        )));
                    }
                    let nt = nt.as_obj().cloned();
                    match self.construct_invoke(&parent, args, nt, None)? {
                        Invoked::Done(v) => self.push(v),
                        Invoked::Pushed => {}
                    }
                }
                Op::Return => {
                    let v = self.pop();
                    if let Some(r) = self.return_value(v, base)? {
                        return Ok(r);
                    }
                }
                Op::ReturnUnwind => {
                    let v = self.pop();
                    if let Some(r) = self.return_unwind(v, base)? {
                        return Ok(r);
                    }
                }
                Op::Add => {
                    let b = self.pop();
                    let a = self.pop();
                    let r = match (&a, &b) {
                        (Value::Num(x), Value::Num(y)) => Value::Num(x + y),
                        (Value::Str(x), Value::Str(y)) => {
                            let mut x = x.clone();
                            drop(a);
                            JsStr::append(&mut x, y);
                            Value::Str(x)
                        }
                        _ => self.op_add(&a, &b)?,
                    };
                    self.push(r);
                }
                Op::Sub
                | Op::Mul
                | Op::Div
                | Op::Mod
                | Op::Exp
                | Op::Shl
                | Op::Shr
                | Op::UShr
                | Op::BitAnd
                | Op::BitOr
                | Op::BitXor => {
                    let b = self.pop();
                    let a = self.pop();
                    let ar = match op {
                        Op::Sub => Arith::Sub,
                        Op::Mul => Arith::Mul,
                        Op::Div => Arith::Div,
                        Op::Mod => Arith::Mod,
                        Op::Exp => Arith::Exp,
                        Op::Shl => Arith::Shl,
                        Op::Shr => Arith::Shr,
                        Op::UShr => Arith::UShr,
                        Op::BitAnd => Arith::BitAnd,
                        Op::BitOr => Arith::BitOr,
                        _ => Arith::BitXor,
                    };
                    let r = self.arith(ar, &a, &b)?;
                    self.push(r);
                }
                Op::Eq | Op::Ne => {
                    let b = self.pop();
                    let a = self.pop();
                    let r = self.loose_eq(&a, &b)?;
                    self.push(Value::Bool(if op == Op::Eq { r } else { !r }));
                }
                Op::StrictEq => {
                    let b = self.pop();
                    let a = self.pop();
                    self.push(Value::Bool(strict_equals(&a, &b)));
                }
                Op::StrictNe => {
                    let b = self.pop();
                    let a = self.pop();
                    self.push(Value::Bool(!strict_equals(&a, &b)));
                }
                Op::Lt | Op::Gt | Op::Le | Op::Ge => {
                    let b = self.pop();
                    let a = self.pop();
                    let r = if let (Value::Num(x), Value::Num(y)) = (&a, &b) {
                        match op {
                            Op::Lt => x < y,
                            Op::Gt => x > y,
                            Op::Le => x <= y,
                            _ => x >= y,
                        }
                    } else {
                        match op {
                            Op::Lt => self.less_than(&a, &b, true)? == Some(true),
                            Op::Gt => self.less_than(&b, &a, false)? == Some(true),
                            Op::Le => self.less_than(&b, &a, false)? == Some(false),
                            _ => self.less_than(&a, &b, true)? == Some(false),
                        }
                    };
                    self.push(Value::Bool(r));
                }
                Op::In => {
                    let obj = self.pop();
                    let key = self.pop();
                    let r = self.op_in(&key, &obj)?;
                    self.push(Value::Bool(r));
                }
                Op::WithHas(c) => {
                    let obj = self.pop();
                    let key = Key::Str(self.kstr(c));
                    let mut found = false;
                    if let Value::Obj(o) = &obj {
                        if self.has_property(o, &key)? {
                            found = true;
                            let unscopables =
                                self.get(&obj, &Key::Sym(self.syms.unscopables.clone()))?;
                            if let Value::Obj(_) = &unscopables {
                                if self.get(&unscopables, &key)?.truthy() {
                                    found = false;
                                }
                            }
                        }
                    }
                    self.push(Value::Bool(found));
                }
                Op::ToObject => {
                    let v = self.pop();
                    let o = self.to_object(&v)?;
                    self.push(Value::Obj(o));
                }
                Op::InstanceOf => {
                    let t = self.pop();
                    let v = self.pop();
                    let r = self.instance_of(&v, &t)?;
                    self.push(Value::Bool(r));
                }
                Op::Neg => {
                    let v = self.pop();
                    let r = match v {
                        Value::Num(n) => Value::Num(-n),
                        _ => self.neg(&v)?,
                    };
                    self.push(r);
                }
                Op::Plus => {
                    let v = self.pop();
                    let n = self.to_number(&v)?;
                    self.push(Value::Num(n));
                }
                Op::Not => {
                    let v = self.pop();
                    self.push(Value::Bool(!v.truthy()));
                }
                Op::BitNot => {
                    let v = self.pop();
                    let r = self.bitnot(&v)?;
                    self.push(r);
                }
                Op::Typeof => {
                    let v = self.pop();
                    self.push(Value::Str(JsStr::intern(v.type_of())));
                }
                Op::ToNumeric => {
                    let v = self.pop();
                    let r = self.to_numeric(&v)?;
                    self.push(r);
                }
                Op::Inc | Op::Dec => {
                    let v = self.pop();
                    let r = self.inc_dec(&v, op == Op::Inc)?;
                    self.push(r);
                }
                Op::ToStr => {
                    let v = self.pop();
                    let s = match v {
                        Value::Str(s) => s,
                        other => self.to_string(&other)?,
                    };
                    self.push(Value::Str(s));
                }
                Op::ToPropertyKey => {
                    let v = self.pop();
                    let k = self.to_key(&v)?;
                    self.push(k.to_value());
                }
                Op::Concat(n) => {
                    let parts = self.pop_n(n as usize);
                    let mut s = String::new();
                    for p in &parts {
                        if let Value::Str(x) = p {
                            s.push_str(x);
                        }
                    }
                    self.push(Value::string(s));
                }
                Op::Jump(t) => top!(self).pc = t as usize,
                Op::JumpIfFalse(t) => {
                    let v = self.pop();
                    if !v.truthy() {
                        top!(self).pc = t as usize;
                    }
                }
                Op::JumpIfTrue(t) => {
                    let v = self.pop();
                    if v.truthy() {
                        top!(self).pc = t as usize;
                    }
                }
                Op::JumpIfFalseKeep(t) => {
                    if !self.peek(0).truthy() {
                        top!(self).pc = t as usize;
                    } else {
                        self.pop();
                    }
                }
                Op::JumpIfTrueKeep(t) => {
                    if self.peek(0).truthy() {
                        top!(self).pc = t as usize;
                    } else {
                        self.pop();
                    }
                }
                Op::JumpIfNotNullishKeep(t) => {
                    if !self.peek(0).is_nullish() {
                        top!(self).pc = t as usize;
                    } else {
                        self.pop();
                    }
                }
                Op::JumpIfNotUndefKeep(t) => {
                    if !self.peek(0).is_undefined() {
                        top!(self).pc = t as usize;
                    } else {
                        self.pop();
                    }
                }
                Op::OptCheck(t, n) => {
                    if self.peek(0).is_nullish() {
                        for _ in 0..n {
                            self.pop();
                        }
                        top!(self).pc = t as usize;
                    }
                }
                Op::EnterTry(h, fin) => {
                    let f = top!(self);
                    let depth = f.stack.len() as u32;
                    f.handlers.push(Handler {
                        pc: h,
                        depth,
                        finally: fin,
                    });
                }
                Op::ExitTry => {
                    top!(self).handlers.pop();
                }
                Op::Throw => {
                    let v = self.pop();
                    self.set_site_here();
                    return Err(Ctl::Throw(v));
                }
                Op::Rethrow => {
                    let v = self.pop();
                    return Err(Ctl::Throw(v));
                }
                Op::EndFinally(ks, vs) => {
                    let (kind, val) = {
                        let f = top!(self);
                        let k = match &f.locals[ks as usize] {
                            Local::V(v) => v.clone(),
                            Local::C(c) => c.borrow().clone(),
                        };
                        let v = match &f.locals[vs as usize] {
                            Local::V(v) => v.clone(),
                            Local::C(c) => c.borrow().clone(),
                        };
                        (k, v)
                    };
                    let k = match kind {
                        Value::Num(n) => n as u32,
                        _ => 0,
                    };
                    match k {
                        1 => return Err(Ctl::Throw(val)),
                        2 => {
                            if let Some(r) = self.return_unwind(val, base)? {
                                return Ok(r);
                            }
                        }
                        3 => {
                            if let Value::Num(n) = val {
                                top!(self).pc = n as usize;
                            }
                        }
                        _ => {}
                    }
                }
                Op::PushPc(t) => self.push(Value::Num(t as f64)),
                Op::ThrowError(kind, msg) => {
                    let m = self.kstr(msg);
                    let k = match kind {
                        1 => ErrKind::ReferenceError,
                        2 => ErrKind::SyntaxError,
                        _ => ErrKind::TypeError,
                    };
                    return Err(self.error(k, m.as_str()));
                }
                Op::GetIter(text) => {
                    let obj = self.pop();
                    let t = if text >= u32::MAX - 1 {
                        None
                    } else {
                        Some(self.kstr(text).to_string())
                    };
                    // Array fast path: an iterator object is still created.
                    let (it, next) = self.get_iterator_text(&obj, t.as_deref(), false)?;
                    self.push(it);
                    self.push(next);
                }
                Op::GetAsyncIter(text) => {
                    let obj = self.pop();
                    let m = if obj.is_nullish() {
                        Value::Undefined
                    } else {
                        self.get(&obj, &Key::Sym(self.syms.async_iterator.clone()))?
                    };
                    if m.is_callable() {
                        let it = self.call(&m, obj, vec![])?;
                        let next = self.get_str(&it, "next")?;
                        self.push(it);
                        self.push(next);
                    } else {
                        let t = if text >= u32::MAX - 1 {
                            None
                        } else {
                            Some(self.kstr(text).to_string())
                        };
                        let (it, next) = self.get_iterator_text(&obj, t.as_deref(), false)?;
                        self.push(it);
                        self.push(next);
                    }
                }
                Op::IterNext(t) => {
                    let next = self.peek(0).clone();
                    let it = self.peek(1).clone();
                    match self.iter_step(&it, &next)? {
                        Some(v) => self.push(v),
                        None => top!(self).pc = t as usize,
                    }
                }
                Op::IterStep => {
                    let next = self.peek(0).clone();
                    let it = self.peek(1).clone();
                    if let Value::Empty = next {
                        self.push(Value::Undefined);
                    } else {
                        match self.iter_step(&it, &next)? {
                            Some(v) => self.push(v),
                            None => {
                                let s = &mut top!(self).stack;
                                let n = s.len();
                                s[n - 1] = Value::Empty;
                                self.push(Value::Undefined);
                            }
                        }
                    }
                }
                Op::IterRest => {
                    let next = self.peek(0).clone();
                    let it = self.peek(1).clone();
                    let mut out = vec![];
                    if !matches!(next, Value::Empty) {
                        while let Some(v) = self.iter_step(&it, &next)? {
                            out.push(v);
                        }
                        let s = &mut top!(self).stack;
                        let n = s.len();
                        s[n - 1] = Value::Empty;
                    }
                    let a = self.arr(out);
                    self.push(a);
                }
                Op::IterClose => {
                    let next = self.pop();
                    let it = self.pop();
                    if !matches!(next, Value::Empty) {
                        self.iter_close(&it)?;
                    }
                }
                Op::IterCloseCompletion => {
                    let kind = self.pop();
                    let val = self.pop();
                    let _next = self.pop();
                    let it = self.pop();
                    let k = match kind {
                        Value::Num(n) => n as u32,
                        _ => 1,
                    };
                    if k == 1 {
                        let saved = self.throw_site.clone();
                        let _ = self.iter_close(&it);
                        self.throw_site = saved;
                        return Err(Ctl::Throw(val));
                    }
                    self.iter_close(&it)?;
                    if let Some(r) = self.return_unwind(val, base)? {
                        return Ok(r);
                    }
                }
                Op::ForInPrep => {
                    let obj = self.pop();
                    let keys = self.for_in_keys(&obj)?;
                    let target = match &obj {
                        Value::Obj(o) => o.clone(),
                        Value::Undefined | Value::Null => self.new_object(),
                        other => self.to_object(other)?,
                    };
                    let fi = self.obj_with(
                        None,
                        Kind::ForIn {
                            keys,
                            index: 0,
                            obj: target,
                        },
                    );
                    self.push(Value::Obj(fi));
                }
                Op::ForInNext(t) => {
                    let fi = self.peek(0).clone();
                    let Value::Obj(fo) = fi else { unreachable!() };
                    loop {
                        let next = {
                            let mut d = fo.borrow_mut();
                            if let Kind::ForIn { keys, index, obj } = &mut d.kind {
                                if *index < keys.len() {
                                    let k = keys[*index].clone();
                                    *index += 1;
                                    Some((k, obj.clone()))
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        };
                        match next {
                            None => {
                                top!(self).pc = t as usize;
                                break;
                            }
                            Some((k, obj)) => {
                                // Skip keys deleted during iteration.
                                if self.has_property(&obj, &Key::Str(k.clone()))? {
                                    self.push(Value::Str(k));
                                    break;
                                }
                            }
                        }
                    }
                }
                Op::AsyncIterNext => {
                    let next = self.peek(0).clone();
                    let it = self.peek(1).clone();
                    let r = self.call(&next, it, vec![])?;
                    self.push(r);
                }
                Op::IterResult(t) => {
                    let r = self.pop();
                    if !matches!(r, Value::Obj(_)) {
                        let d = self.describe_for_error(&r);
                        return Err(
                            self.type_error(format!("Iterator result {d} is not an object"))
                        );
                    }
                    if self.get_str(&r, "done")?.truthy() {
                        top!(self).pc = t as usize;
                    } else {
                        let v = self.get_str(&r, "value")?;
                        self.push(v);
                    }
                }
                Op::NewObject => {
                    let o = self.new_object();
                    self.push(Value::Obj(o));
                }
                Op::NewArray(n) => {
                    let items = self.pop_n(n as usize);
                    let a = self.arr(items);
                    self.push(a);
                }
                Op::ArrayPush => {
                    let v = self.pop();
                    if let Value::Obj(o) = self.peek(0) {
                        if let Kind::Array(a) = &mut o.borrow_mut().kind {
                            a.push(v);
                        }
                    }
                }
                Op::ArrayHole => {
                    if let Value::Obj(o) = self.peek(0) {
                        if let Kind::Array(a) = &mut o.borrow_mut().kind {
                            a.push(Value::Empty);
                        }
                    }
                }
                Op::ArraySpread(text) => {
                    let v = self.pop();
                    let t = if text >= u32::MAX - 1 {
                        None
                    } else {
                        Some(self.kstr(text).to_string())
                    };
                    let items = self.iterable_to_vec_text(&v, t.as_deref(), true)?;
                    if let Value::Obj(o) = self.peek(0) {
                        if let Kind::Array(a) = &mut o.borrow_mut().kind {
                            a.extend(items);
                        }
                    }
                }
                Op::DefineField(c) => {
                    let v = self.pop();
                    let key = Key::Str(self.kstr(c));
                    if let Value::Obj(o) = self.peek(0).clone() {
                        self.create_data_property(&o, key, v)?;
                    }
                }
                Op::DefineElem => {
                    let v = self.pop();
                    let k = self.pop();
                    let key = self.to_key(&k)?;
                    if let Value::Obj(o) = self.peek(0).clone() {
                        self.create_data_property(&o, key, v)?;
                    }
                }
                Op::DefineMethod(c, kind, en) => {
                    let f = self.pop();
                    let key = Key::Str(self.kstr(c));
                    if let Value::Obj(o) = self.peek(0).clone() {
                        self.define_method(&o, key, f, kind, en);
                    }
                }
                Op::DefineMethodElem(kind, en) => {
                    let f = self.pop();
                    let k = self.pop();
                    let key = self.to_key(&k)?;
                    self.name_anon_fn(&f, &key);
                    if let Value::Obj(o) = self.peek(0).clone() {
                        self.define_method(&o, key, f, kind, en);
                    }
                }
                Op::CopyData => {
                    let src = self.pop();
                    if let Value::Obj(o) = self.peek(0).clone() {
                        self.copy_data(&o, &src, &[])?;
                    }
                }
                Op::CopyDataExcluding(n) => {
                    let ks = self.pop_n(n as usize);
                    let src = self.pop();
                    let mut ex = vec![];
                    for k in ks {
                        ex.push(self.to_key(&k)?);
                    }
                    if let Value::Obj(o) = self.peek(0).clone() {
                        self.copy_data(&o, &src, &ex)?;
                    }
                }
                Op::SetProtoLit => {
                    let v = self.pop();
                    if let Value::Obj(o) = self.peek(0) {
                        match v {
                            Value::Obj(p) => o.borrow_mut().proto = Some(p),
                            Value::Null => o.borrow_mut().proto = None,
                            _ => {}
                        }
                    }
                }
                Op::RequireCoercible(c) => {
                    let v = self.peek(0).clone();
                    if v.is_nullish() {
                        let what = if matches!(v, Value::Null) {
                            "null"
                        } else {
                            "undefined"
                        };
                        let msg = if c >= u32::MAX - 1 {
                            format!("Cannot destructure '{what}' as it is {what}.")
                        } else {
                            let k = self.kstr(c);
                            format!(
                                "Cannot destructure property '{}' of '{what}' as it is {what}.",
                                k.as_str()
                            )
                        };
                        return Err(self.type_error(msg));
                    }
                }
                Op::Closure(i) => {
                    if let Some(p) = &self.prof {
                        crate::profile::Counters::bump(&p.counters.closures);
                    }
                    let code = self.frames.last().unwrap().code.codes[i as usize].clone();
                    let caps = self.closure_captures(&code);
                    let f = self.make_closure(code, caps);
                    self.push(Value::Obj(f));
                }
                Op::NewPrivateName(c) => {
                    let d = self.kstr(c);
                    let s = Rc::new(Symbol {
                        desc: Some(d),
                        private: true,
                        registered: false,
                    });
                    self.push(Value::Sym(s));
                }
                Op::RegExp(p, f) => {
                    let ps = self.kstr(p);
                    let fs = self.kstr(f);
                    let r = self.regexp_create(&ps, &fs, None)?;
                    self.push(Value::Obj(r));
                }
                Op::ClassMethod(c, kind, is_static) => {
                    let f = self.pop();
                    let proto = self.peek(0).clone();
                    let ctor = self.peek(1).clone();
                    let target = if is_static { ctor } else { proto };
                    let key = Key::Str(self.kstr(c));
                    if let Value::Obj(t) = target {
                        self.define_method(&t, key, f, kind, false);
                    }
                }
                Op::ClassMethodElem(kind, is_static) => {
                    let f = self.pop();
                    let k = self.pop();
                    let key = self.to_key(&k)?;
                    self.name_anon_fn(&f, &key);
                    let proto = self.peek(0).clone();
                    let ctor = self.peek(1).clone();
                    let target = if is_static { ctor } else { proto };
                    if let Value::Obj(t) = target {
                        self.define_method(&t, key, f, kind, false);
                    }
                }
                Op::ExportGetter(c) => {
                    let f = self.pop();
                    let key = Key::Str(self.kstr(c));
                    if let Value::Obj(ns) = self.peek(0) {
                        ns.borrow_mut().props.insert(
                            key,
                            Prop {
                                slot: Slot::Accessor(f.as_obj().cloned(), None),
                                flags: ENUMERABLE,
                            },
                        );
                    }
                }
                Op::SetFnNameElem(_) => {
                    let f = self.peek(0).clone();
                    let k = self.peek(1).clone();
                    let key = self.to_key(&k)?;
                    self.name_anon_fn(&f, &key);
                }
                Op::Class(i, has_super) => {
                    self.make_class(i, has_super)?;
                }
                Op::SetFieldInit => {
                    let f = self.pop();
                    let proto = self.peek(0).clone();
                    let ctor = self.peek(1).clone();
                    if let (Value::Obj(c), Value::Obj(fo)) = (&ctor, &f) {
                        if let Value::Obj(p) = &proto {
                            Self::set_home(&f, p);
                        }
                        if let Kind::Function(fd) = &mut c.borrow_mut().kind {
                            fd.fields = Some(fo.clone());
                        }
                    }
                }
                Op::RunFields => {
                    let this = self.pop();
                    let func = self.pop();
                    let init = match &func {
                        Value::Obj(f) => match &f.borrow().kind {
                            Kind::Function(fd) => fd.fields.clone(),
                            _ => None,
                        },
                        _ => None,
                    };
                    if let Some(init) = init {
                        self.call(&Value::Obj(init), this, vec![])?;
                    }
                }
                Op::BindThis(i, free) => {
                    let v = self.peek(0).clone();
                    let f = top!(self);
                    let already = if free {
                        let c = f.captures[i as usize].clone();
                        let e = !matches!(*c.borrow(), Value::Empty);
                        if !e {
                            *c.borrow_mut() = v;
                        }
                        e
                    } else {
                        match &mut f.locals[i as usize] {
                            Local::V(x) => {
                                let e = !matches!(x, Value::Empty);
                                if !e {
                                    *x = v;
                                }
                                e
                            }
                            Local::C(c) => {
                                let e = !matches!(*c.borrow(), Value::Empty);
                                if !e {
                                    *c.borrow_mut() = v;
                                }
                                e
                            }
                        }
                    };
                    if already {
                        return Err(
                            self.reference_error("Super constructor may only be called once")
                        );
                    }
                }
                Op::RestParam(i) => {
                    let rest: Vec<Value> =
                        top!(self).args.iter().skip(i as usize).cloned().collect();
                    let a = self.arr(rest);
                    self.push(a);
                }
                Op::Arg(i) => {
                    let v = top!(self)
                        .args
                        .get(i as usize)
                        .cloned()
                        .unwrap_or(Value::Undefined);
                    self.push(v);
                }
                Op::TemplateObj(site) => {
                    let t = self.template_object(site);
                    self.push(t);
                }
                Op::Yield => {
                    let v = self.pop();
                    if let Some(r) = self.suspend_yield(v, base)? {
                        return Ok(r);
                    }
                }
                Op::YieldStar(_) => {
                    if let Some(r) = self.yield_star(base)? {
                        return Ok(r);
                    }
                }
                Op::Await => {
                    let v = self.pop();
                    if let Some(r) = self.suspend_await(v, base)? {
                        return Ok(r);
                    }
                }
                Op::SetCompletion => {
                    self.completion = self.pop();
                }
                Op::TakeCompletion => {
                    let v = std::mem::replace(&mut self.completion, Value::Undefined);
                    self.push(v);
                }
                Op::Debugger | Op::Nop => {}
                Op::ImportMeta => {
                    let m = match &self.import_meta {
                        Some(m) => Value::Obj(m.clone()),
                        None => Value::Undefined,
                    };
                    self.push(m);
                }
                Op::DynImport => {
                    let spec = self.pop();
                    let p = self.dynamic_import(&spec)?;
                    self.push(p);
                }
            }
        }
    }

    fn name_anon_fn(&mut self, f: &Value, key: &Key) {
        if let Value::Obj(fo) = f {
            if !fo.is_callable() {
                return;
            }
            let cur = fo.own_value("name");
            if matches!(cur, Some(Value::Str(ref s)) if s.is_empty()) || cur.is_none() {
                let n = match key {
                    Key::Str(s) => s.to_string(),
                    Key::Sym(s) => match &s.desc {
                        Some(d) => format!("[{}]", d.as_str()),
                        None => String::new(),
                    },
                };
                fo.set_prop("name", Value::string(n), CONFIGURABLE);
            }
        }
    }

    fn delete_op(&mut self, obj: &Value, key: &Key) -> JsResult<bool> {
        let o = match obj {
            Value::Obj(o) => o.clone(),
            Value::Undefined | Value::Null => {
                return Err(self.type_error("Cannot convert undefined or null to object"))
            }
            _ => return Ok(true),
        };
        let r = self.delete(&o, key)?;
        if !r && self.is_strict() {
            let k = self.key_display(key);
            let tn = self.constructor_name(&o).unwrap_or_else(|| "Object".into());
            return Err(self.type_error(format!("Cannot delete property '{k}' of #<{tn}>")));
        }
        Ok(r)
    }

    fn super_get(&mut self, home: &Value, key: &Key, this: &Value) -> JsResult<Value> {
        let Value::Obj(h) = home else {
            return Ok(Value::Undefined);
        };
        match h.proto() {
            Some(p) => self.get_from(&p, key, this),
            None => Ok(Value::Undefined),
        }
    }

    fn private_get(&mut self, obj: &Value, sym: &Rc<Symbol>) -> JsResult<Value> {
        let p = match obj {
            Value::Obj(o) => o.borrow().props.get(&Key::Sym(sym.clone())).cloned(),
            _ => None,
        };
        match p {
            Some(Prop {
                slot: Slot::Data(v),
                ..
            }) => Ok(v),
            Some(Prop {
                slot: Slot::Accessor(Some(g), _),
                ..
            }) => self.call(&Value::Obj(g), obj.clone(), vec![]),
            Some(_) => {
                let n = Self::private_name(sym);
                Err(self.type_error(format!("'{n}' was defined without a getter")))
            }
            None => {
                let n = Self::private_name(sym);
                Err(self.type_error(format!(
                    "Cannot read private member {n} from an object whose class did not declare it"
                )))
            }
        }
    }

    fn private_set(&mut self, obj: &Value, sym: &Rc<Symbol>, v: Value) -> JsResult<()> {
        let Value::Obj(o) = obj else {
            let n = Self::private_name(sym);
            return Err(self.type_error(format!(
                "Cannot write private member {n} to an object whose class did not declare it"
            )));
        };
        let key = Key::Sym(sym.clone());
        let p = o.borrow().props.get(&key).cloned();
        match p {
            Some(Prop {
                slot: Slot::Data(_),
                flags,
            }) => {
                if flags & WRITABLE == 0 {
                    let n = Self::private_name(sym);
                    return Err(self.type_error(format!("Private method '{n}' is not writable")));
                }
                o.borrow_mut().props.insert(key, Prop::data(v, flags));
                Ok(())
            }
            Some(Prop {
                slot: Slot::Accessor(_, Some(s)),
                ..
            }) => {
                self.call(&Value::Obj(s), obj.clone(), vec![v])?;
                Ok(())
            }
            Some(_) => {
                let n = Self::private_name(sym);
                Err(self.type_error(format!("'{n}' was defined without a setter")))
            }
            None => {
                let n = Self::private_name(sym);
                Err(self.type_error(format!(
                    "Cannot write private member {n} to an object whose class did not declare it"
                )))
            }
        }
    }

    fn make_class(&mut self, idx: u32, has_super: bool) -> JsResult<()> {
        let code = self.frames.last().unwrap().code.codes[idx as usize].clone();
        let (proto_parent, ctor_parent) = if has_super {
            let sup = self.pop();
            match &sup {
                Value::Null => (None, self.intr.function_proto.clone()),
                Value::Obj(s) if self.is_constructor(&sup) => {
                    let pp = self.get_str(&sup, "prototype")?;
                    match pp {
                        Value::Obj(p) => (Some(p), s.clone()),
                        Value::Null => (None, s.clone()),
                        other => {
                            let d = self.describe_for_error(&other);
                            return Err(self.type_error(format!(
                                "Class extends value does not have valid prototype property {d}"
                            )));
                        }
                    }
                }
                other => {
                    let d = self.describe_for_error(other);
                    return Err(self.type_error(format!(
                        "Class extends value {d} is not a constructor or null"
                    )));
                }
            }
        } else {
            (
                Some(self.intr.object_proto.clone()),
                self.intr.function_proto.clone(),
            )
        };
        let proto = self.obj_with(proto_parent, Kind::Ordinary);
        let caps = self.closure_captures(&code);
        let ctor = self.make_closure(code, caps);
        {
            let mut d = ctor.borrow_mut();
            d.proto = Some(ctor_parent);
            if let Kind::Function(fd) = &mut d.kind {
                fd.class_ctor = true;
                fd.ctor = if has_super {
                    CtorKind::Derived
                } else {
                    CtorKind::Base
                };
                fd.home = Some(proto.clone());
            }
            d.props.insert(
                Key::str("prototype"),
                Prop::data(Value::Obj(proto.clone()), 0),
            );
        }
        proto.set_hidden("constructor", Value::Obj(ctor.clone()));
        self.push(Value::Obj(ctor));
        self.push(Value::Obj(proto));
        Ok(())
    }

    fn suspend_yield(&mut self, v: Value, base: usize) -> JsResult<Option<Value>> {
        let frame = self.frames.pop().unwrap();
        let g = match &frame.kind {
            FrameKind::Generator(g) => g.clone(),
            _ => return Err(self.syntax_error("yield outside generator")),
        };
        if let Kind::Generator(gd) = &mut g.borrow_mut().kind {
            gd.frame = Some(Box::new(frame));
        }
        self.exit = Exit::Yield;
        if self.frames.len() == base {
            return Ok(Some(v));
        }
        // Generator frames always run as the base of a nested run.
        Ok(Some(v))
    }

    fn yield_star(&mut self, base: usize) -> JsResult<Option<Value>> {
        let mode = std::mem::take(&mut top!(self).ystar_mode);
        let received = self.pop();
        let next = self.peek(0).clone();
        let it = self.peek(1).clone();
        let is_async = matches!(&top!(self).kind, FrameKind::Generator(g) if matches!(&g.borrow().kind, Kind::Generator(gd) if gd.is_async));
        let r = match mode {
            0 => self.call(&next, it.clone(), vec![received])?,
            1 => {
                let t = self.get_str(&it, "throw")?;
                if !t.is_callable() {
                    let _ = self.iter_close(&it);
                    return Err(self.type_error("The iterator does not provide a 'throw' method"));
                }
                self.call(&t, it.clone(), vec![received])?
            }
            _ => {
                let rf = self.get_str(&it, "return")?;
                if !rf.is_callable() {
                    self.pop();
                    self.pop();
                    return self.return_unwind(received, base);
                }
                self.call(&rf, it.clone(), vec![received])?
            }
        };
        let r = if is_async {
            // Async delegation: results are promises; settle synchronously
            // when already resolved.
            self.settled_value(&r)?
        } else {
            r
        };
        if !matches!(r, Value::Obj(_)) {
            let d = self.describe_for_error(&r);
            return Err(self.type_error(format!("Iterator result {d} is not an object")));
        }
        let done = self.get_str(&r, "done")?.truthy();
        let value = self.get_str(&r, "value")?;
        if done {
            self.pop();
            self.pop();
            if mode == 2 {
                return self.return_unwind(value, base);
            }
            self.push(value);
            return Ok(None);
        }
        // Suspend at this instruction; resumption re-executes it.
        self.push(Value::Undefined);
        top!(self).pc -= 1;
        self.suspend_yield(value, base)
    }

    fn suspend_await(&mut self, v: Value, base: usize) -> JsResult<Option<Value>> {
        let promise = self.promise_resolve(&v)?;
        let mut frame = self.frames.pop().unwrap();
        let at_base = self.frames.len() == base;
        match &mut frame.kind {
            FrameKind::Async {
                promise: fp,
                co,
                first,
            } => {
                let fp = fp.clone();
                let co = co.clone();
                let was_first = *first;
                *first = false;
                if let Kind::Coroutine(slot) = &mut co.borrow_mut().kind {
                    *slot = Some(Box::new(frame));
                }
                self.promise_react(&promise, Reaction::Resume(co.clone()));
                self.exit = Exit::Await;
                if at_base {
                    return Ok(Some(Value::Obj(fp)));
                }
                if was_first {
                    self.push(Value::Obj(fp));
                    return Ok(None);
                }
                Ok(Some(Value::Undefined))
            }
            FrameKind::Generator(g) => {
                let g = g.clone();
                if let Kind::Generator(gd) = &mut g.borrow_mut().kind {
                    gd.frame = Some(Box::new(frame));
                }
                self.promise_react(&promise, Reaction::Resume(g.clone()));
                self.exit = Exit::Await;
                Ok(Some(Value::Undefined))
            }
            _ => {
                // `await` in a non-async frame (should not compile): resume
                // synchronously.
                self.frames.push(frame);
                let r = self.settled_value(&Value::Obj(promise))?;
                self.push(r);
                Ok(None)
            }
        }
    }
}

/// A property read the fast path can answer: `obj.name` on an ordinary object
/// or function (or a chain of them) that holds `name` as a data property, or
/// the length of a string. `None` sends the read down the general path, which
/// answers everything else (getters, exotic objects, proxies, primitives'
/// prototypes, errors' lazy stacks) and raises the errors.
#[inline]
fn plain_get(obj: &Value, name: &JsStr, hint: &Cell<u16>) -> Option<Value> {
    if !name.is_canon() {
        return None;
    }
    match obj {
        Value::Obj(o) => {
            if let Kind::Array(v) = &o.borrow().kind {
                return match name.as_str() {
                    "length" => Some(Value::Num(v.len() as f64)),
                    _ => None,
                };
            }
            plain_get_ident(o, std::rc::Rc::as_ptr(&name.0) as *const u8 as usize, hint)
        }
        Value::Str(s) if name.as_str() == "length" => Some(Value::Num(s.len16() as f64)),
        _ => None,
    }
}

/// A primitive's property found on its wrapper prototype `proto` (a string's
/// `length` and indices are answered before this): an own data property of
/// the prototype, or the ordinary chain above it. `None` for getters and
/// anything exotic.
#[inline]
fn proto_get(proto: &Obj, name: &JsStr, hint: &Cell<u16>) -> Option<Value> {
    if !name.is_canon() {
        return None;
    }
    let id = std::rc::Rc::as_ptr(&name.0) as *const u8 as usize;
    let d = proto.borrow();
    if let Some(i) = d.props.find_hinted(id, hint) {
        return match &d.props.entries[i].1.slot {
            Slot::Data(Value::Empty) => None,
            Slot::Data(v) => Some(v.clone()),
            Slot::Accessor(..) => None,
        };
    }
    match &d.proto {
        Some(p) => plain_get_ident(p, id, hint),
        None => Some(Value::Undefined),
    }
}

/// `plain_get` on an object by key identity (`Key::ident`).
#[inline]
fn plain_get_ident(o: &Obj, id: usize, hint: &Cell<u16>) -> Option<Value> {
    let mut cur: *const Obj = o;
    for _ in 0..64 {
        // SAFETY: `cur` is `o` or a prototype reached from it; each is owned by
        // the object before it in the chain, and nothing runs during the walk
        // that could change a prototype link or drop an object.
        let d = unsafe { &*cur }.borrow();
        if !d.kind.ordinary_props() {
            return None;
        }
        if let Some(i) = d.props.find_hinted(id, hint) {
            return match &d.props.entries[i].1.slot {
                Slot::Data(Value::Empty) => None,
                Slot::Data(v) => Some(v.clone()),
                Slot::Accessor(..) => None,
            };
        }
        match &d.proto {
            Some(p) => cur = p,
            None => return Some(Value::Undefined),
        }
    }
    None
}

/// A property write the fast path can do: `obj.name = v` where the ordinary
/// object or function already has `name` as its own writable data property.
#[inline]
fn plain_set(obj: &Value, name: &JsStr, v: &Value, hint: &Cell<u16>) -> bool {
    let Value::Obj(o) = obj else {
        return false;
    };
    if !name.is_canon() {
        return false;
    }
    let mut d = o.borrow_mut();
    if !d.kind.ordinary_props() {
        return false;
    }
    let id = std::rc::Rc::as_ptr(&name.0) as *const u8 as usize;
    let Some(i) = d.props.find_hinted(id, hint) else {
        // A new property: added here when the object is extensible and no
        // prototype has the name (a setter or a read-only property there
        // would decide otherwise, which the general path handles).
        if !d.extensible || !absent_from_prototypes(d.proto.as_ref(), id) {
            return false;
        }
        d.props
            .push_absent(Key::Str(name.clone()), Prop::data(v.clone(), ALL));
        return true;
    };
    let p = &mut d.props.entries[i].1;
    if !p.writable() {
        return false;
    }
    match &mut p.slot {
        Slot::Data(x) => {
            *x = v.clone();
            true
        }
        Slot::Accessor(..) => false,
    }
}

/// `==` when no conversion is involved: both nullish or one of them nullish,
/// two numbers, strings, booleans or objects. `None` for the rest.
#[inline]
fn simple_loose_eq(a: &Value, b: &Value) -> Option<bool> {
    Some(match (a, b) {
        (Value::Undefined | Value::Null, Value::Undefined | Value::Null) => true,
        (
            Value::Undefined | Value::Null,
            Value::Num(_) | Value::Str(_) | Value::Bool(_) | Value::Obj(_),
        )
        | (
            Value::Num(_) | Value::Str(_) | Value::Bool(_) | Value::Obj(_),
            Value::Undefined | Value::Null,
        ) => false,
        (Value::Num(x), Value::Num(y)) => x == y,
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Obj(x), Value::Obj(y)) => x.ptr_eq(y),
        _ => return None,
    })
}

/// An object literal's field `name: v` (CreateDataProperty) on an ordinary
/// object: replaces a configurable or writable own property, or adds one to
/// an extensible object.
#[inline]
fn define_field(obj: &Value, name: &JsStr, v: &Value) -> bool {
    let Value::Obj(o) = obj else {
        return false;
    };
    if !name.is_canon() {
        return false;
    }
    let mut d = o.borrow_mut();
    if !d.kind.ordinary_props() {
        return false;
    }
    let id = std::rc::Rc::as_ptr(&name.0) as *const u8 as usize;
    match d.props.find_ident(id) {
        Some(i) => {
            let p = &mut d.props.entries[i].1;
            if !p.configurable() && !p.writable() {
                return false;
            }
            *p = Prop::data(v.clone(), ALL);
        }
        None => {
            if !d.extensible {
                return false;
            }
            d.props
                .push_absent(Key::Str(name.clone()), Prop::data(v.clone(), ALL));
        }
    }
    true
}

/// An element write the fast path can do: an array element by an integer
/// index within (or just past) a plain array, or a canonical string key on an
/// ordinary object (as `plain_set`).
#[inline]
fn element_set(obj: &Value, key: &Value, v: &Value, hint: &Cell<u16>) -> bool {
    match (obj, key) {
        (Value::Obj(o), Value::Num(n)) => {
            let i = *n as usize;
            if i as f64 != *n {
                return false;
            }
            let mut d = o.borrow_mut();
            let plain = !d.elems_frozen && !d.elems_sealed && d.extensible;
            if let Kind::Array(arr) = &mut d.kind {
                if plain {
                    if i < arr.len() {
                        arr[i] = v.clone();
                        return true;
                    }
                    if i == arr.len() {
                        arr.push(v.clone());
                        return true;
                    }
                }
            }
            false
        }
        (Value::Obj(o), Value::Str(s)) => {
            !matches!(o.borrow().kind, Kind::Array(_))
                && crate::numconv::array_index(s).is_none()
                && plain_set(obj, s, v, hint)
        }
        _ => false,
    }
}

/// Whether no object on the prototype chain starting at `proto` has an own
/// property with identity `id`, all of them being ordinary. False (inconclusive)
/// for exotic prototypes and very long chains.
#[inline]
fn absent_from_prototypes(proto: Option<&Obj>, id: usize) -> bool {
    let Some(first) = proto else {
        return true;
    };
    let mut cur: *const Obj = first;
    for _ in 0..64 {
        // SAFETY: as in `plain_get_ident`, each prototype is owned by the object
        // before it, and nothing runs during the walk.
        let d = unsafe { &*cur }.borrow();
        if !d.kind.ordinary_props() || d.props.find_ident(id).is_some() {
            return false;
        }
        match &d.proto {
            Some(p) => cur = p,
            None => return true,
        }
    }
    false
}

/// An element read the fast path can answer: a present element of an array
/// by an integer index, or a symbol-keyed data property of a plain object.
#[inline]
fn element_get(obj: &Value, key: &Value, hint: &Cell<u16>) -> Option<Value> {
    if let (Value::Obj(o), Value::Sym(s)) = (obj, key) {
        return plain_get_ident(o, std::rc::Rc::as_ptr(s) as *const u8 as usize, hint);
    }
    if let (Value::Obj(o), Value::Str(s)) = (obj, key) {
        // A canonical name (a key from `for…in` or `Object.keys`, a literal);
        // an array's `length` and its index strings are the general path's.
        if s.is_canon() && !matches!(o.borrow().kind, Kind::Array(_)) {
            return plain_get_ident(o, std::rc::Rc::as_ptr(&s.0) as *const u8 as usize, hint);
        }
        return None;
    }
    if let (Value::Obj(o), Value::Num(n)) = (obj, key) {
        let i = *n as usize;
        if i as f64 == *n {
            if let Kind::Array(v) = &o.borrow().kind {
                if let Some(x) = v.get(i) {
                    if !matches!(x, Value::Empty) {
                        return Some(x.clone());
                    }
                }
            }
        }
    }
    None
}
