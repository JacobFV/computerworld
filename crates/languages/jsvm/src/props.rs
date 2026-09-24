//! [[Get]], [[Set]], [[DefineOwnProperty]], [[Delete]], [[OwnPropertyKeys]]
//! including the exotic behaviour of arrays, strings, typed arrays,
//! errors (lazy `stack`) and proxies.

use crate::numconv::{array_index, number_to_string};
use crate::value::*;
use crate::vm::Vm;
use std::rc::Rc;

/// Maximum array length we materialise densely.
pub const MAX_DENSE: usize = 1 << 26;

impl<'h> Vm<'h> {
    /// Own property lookup with exotic elements materialised.
    pub fn get_own(&mut self, o: &Obj, key: &Key) -> JsResult<Option<Prop>> {
        let proxy = {
            let d = o.borrow();
            match &d.kind {
                Kind::Array(v) => {
                    if let Key::Str(k) = key {
                        if let Some(i) = array_index(k) {
                            return Ok(match v.get(i as usize) {
                                Some(Value::Empty) | None => None,
                                Some(x) => Some(Prop::data(
                                    x.clone(),
                                    if d.elems_frozen {
                                        ENUMERABLE
                                    } else if d.elems_sealed {
                                        ENUMERABLE | WRITABLE
                                    } else {
                                        ALL
                                    },
                                )),
                            });
                        }
                        if k.as_str() == "length" {
                            return Ok(Some(Prop::data(
                                Value::Num(v.len() as f64),
                                if d.elems_frozen { 0 } else { WRITABLE },
                            )));
                        }
                    }
                    None
                }
                Kind::String(s) => {
                    if let Key::Str(k) = key {
                        if let Some(i) = array_index(k) {
                            if let Some(u) = s.code_unit(i as usize) {
                                return Ok(Some(Prop::data(
                                    Value::string(String::from_utf16_lossy(&[u])),
                                    ENUMERABLE,
                                )));
                            }
                        }
                        if k.as_str() == "length" {
                            return Ok(Some(Prop::data(Value::Num(s.len16() as f64), 0)));
                        }
                    }
                    None
                }
                Kind::TypedArray { .. } => {
                    if let Key::Str(k) = key {
                        if let Some(i) = array_index(k) {
                            drop(d);
                            return Ok(self.typed_get(o, i as usize).map(|v| Prop::data(v, ALL)));
                        }
                    }
                    None
                }
                Kind::Proxy { target, handler } => Some((target.clone(), handler.clone())),
                Kind::Host(h) if !h.hooks.plain => {
                    let hooks = h.hooks;
                    drop(d);
                    let pk = self.prof_enter(|| format!("[host get] {}", hooks.class));
                    let r = (hooks.get)(self, o, key);
                    self.prof_leave(pk);
                    if let Some(v) = r? {
                        return Ok(Some(Prop::data(v, ALL)));
                    }
                    None
                }
                _ => None,
            }
        };
        if let Some((target, handler)) = proxy {
            let trap = self.get_str(&Value::Obj(handler.clone()), "getOwnPropertyDescriptor")?;
            if trap.is_callable() {
                let r = self.call(
                    &trap,
                    Value::Obj(handler),
                    vec![Value::Obj(target), key.to_value()],
                )?;
                if let Value::Obj(desc) = r {
                    let v = self.get_str(&Value::Obj(desc.clone()), "value")?;
                    let en = self
                        .get_str(&Value::Obj(desc.clone()), "enumerable")?
                        .truthy();
                    return Ok(Some(Prop::data(
                        v,
                        if en { ALL } else { WRITABLE | CONFIGURABLE },
                    )));
                }
                return Ok(None);
            }
            return self.get_own(&target, key);
        }
        let p = o.borrow().props.get(key).cloned();
        if let Some(Prop {
            slot: Slot::Data(Value::Empty),
            flags,
        }) = &p
        {
            // Lazily formatted error stack.
            let flags = *flags;
            let v = self.format_stack(o)?;
            return Ok(Some(Prop::data(v, flags)));
        }
        Ok(p)
    }

    /// [[Get]] on any value.
    pub fn get(&mut self, v: &Value, key: &Key) -> JsResult<Value> {
        match v {
            Value::Obj(o) => self.get_from(o, key, v),
            Value::Str(s) => {
                if let Key::Str(k) = key {
                    if k.as_str() == "length" {
                        return Ok(Value::Num(s.len16() as f64));
                    }
                    if let Some(i) = array_index(k) {
                        if let Some(u) = s.code_unit(i as usize) {
                            return Ok(Value::string(String::from_utf16_lossy(&[u])));
                        }
                        return Ok(Value::Undefined);
                    }
                }
                let p = self.intr.string_proto.clone();
                self.get_from(&p, key, v)
            }
            Value::Num(_) => {
                let p = self.intr.number_proto.clone();
                self.get_from(&p, key, v)
            }
            Value::Bool(_) => {
                let p = self.intr.boolean_proto.clone();
                self.get_from(&p, key, v)
            }
            Value::Sym(_) => {
                let p = self.intr.symbol_proto.clone();
                self.get_from(&p, key, v)
            }
            Value::BigInt(_) => {
                let p = self.intr.bigint_proto.clone();
                self.get_from(&p, key, v)
            }
            Value::Undefined | Value::Null | Value::Empty => {
                let what = if matches!(v, Value::Null) {
                    "null"
                } else {
                    "undefined"
                };
                if let Key::Sym(s) = key {
                    if Rc::ptr_eq(s, &self.syms.iterator) {
                        let d = if matches!(v, Value::Null) {
                            "object null"
                        } else {
                            "undefined"
                        };
                        return Err(self.type_error(format!(
                            "{d} is not iterable (cannot read property Symbol(Symbol.iterator))"
                        )));
                    }
                }
                let k = self.key_display(key);
                Err(self.type_error(format!("Cannot read properties of {what} (reading '{k}')")))
            }
        }
    }

    pub fn key_display(&self, key: &Key) -> String {
        match key {
            Key::Str(s) => s.to_string(),
            Key::Sym(s) => format!(
                "Symbol({})",
                s.desc.as_ref().map(|d| d.to_string()).unwrap_or_default()
            ),
        }
    }

    pub fn get_str(&mut self, v: &Value, k: &str) -> JsResult<Value> {
        self.get(v, &Key::str(k))
    }

    pub fn get_from(&mut self, o: &Obj, key: &Key, receiver: &Value) -> JsResult<Value> {
        if self.prof.is_some() {
            return self.get_from_profiled(o, key, receiver);
        }
        self.get_from_inner(o, key, receiver)
    }

    #[cold]
    fn get_from_profiled(&mut self, o: &Obj, key: &Key, receiver: &Value) -> JsResult<Value> {
        use crate::profile::Counters;
        // Count the walk the read makes without running anything.
        let mut hops = 0u64;
        let mut own_fast = false;
        let mut accessor = false;
        {
            let mut cur = Some(o.clone());
            while let Some(c) = cur {
                let d = c.borrow();
                let ordinary = matches!(d.kind, Kind::Ordinary | Kind::Function(_));
                if let Some(p) = d.props.get(key) {
                    own_fast = hops == 0 && ordinary && matches!(p.slot, Slot::Data(_));
                    accessor = matches!(p.slot, Slot::Accessor(..));
                    break;
                }
                if !ordinary {
                    break;
                }
                cur = d.proto.clone();
                hops += 1;
                if hops > 64 {
                    break;
                }
            }
        }
        if let Some(p) = self.prof.as_deref_mut() {
            Counters::bump(&p.counters.get);
            Counters::add(&p.counters.get_hops, hops);
            if own_fast {
                Counters::bump(&p.counters.get_own_fast);
            }
            if accessor {
                Counters::bump(&p.counters.get_accessor);
            }
            if !matches!(receiver, Value::Obj(r) if r.ptr_eq(o)) {
                Counters::bump(&p.counters.get_primitive);
            }
            if p.opts.prop_names {
                let name = match key {
                    Key::Str(s) => s.to_string(),
                    Key::Sym(s) => format!(
                        "Symbol({})",
                        s.desc.as_ref().map(|d| d.to_string()).unwrap_or_default()
                    ),
                };
                p.prop_name(&name, hops);
            }
        }
        self.get_from_inner(o, key, receiver)
    }

    fn get_from_inner(&mut self, o: &Obj, key: &Key, receiver: &Value) -> JsResult<Value> {
        let mut cur = o.clone();
        let mut hops = 0;
        loop {
            // Fast path: ordinary own data property.
            let fast = {
                let d = cur.borrow();
                match d.kind.ordinary_props() {
                    true => match d.props.get(key) {
                        Some(Prop {
                            slot: Slot::Data(v),
                            ..
                        }) if !matches!(v, Value::Empty) => Some(Some(v.clone())),
                        Some(_) => None,
                        None => Some(None),
                    },
                    false => None,
                }
            };
            let found = match fast {
                Some(Some(v)) => return Ok(v),
                Some(None) => None,
                None => {
                    let px = match &cur.borrow().kind {
                        Kind::Proxy { target, handler } => Some((target.clone(), handler.clone())),
                        _ => None,
                    };
                    if let Some((t, h)) = px {
                        return self.proxy_get(&t, &h, key, receiver);
                    }
                    self.get_own(&cur, key)?
                }
            };
            match found {
                Some(Prop {
                    slot: Slot::Data(v),
                    ..
                }) => return Ok(v),
                Some(Prop {
                    slot: Slot::Accessor(g, _),
                    ..
                }) => {
                    return match g {
                        Some(g) => self.call(&Value::Obj(g), receiver.clone(), vec![]),
                        None => Ok(Value::Undefined),
                    }
                }
                None => {
                    let next = cur.proto();
                    match next {
                        Some(p) => cur = p,
                        None => return Ok(Value::Undefined),
                    }
                }
            }
            hops += 1;
            if hops > 10_000 {
                return Ok(Value::Undefined);
            }
        }
    }

    fn proxy_get(
        &mut self,
        target: &Obj,
        handler: &Obj,
        key: &Key,
        receiver: &Value,
    ) -> JsResult<Value> {
        let trap = self.get_str(&Value::Obj(handler.clone()), "get")?;
        if trap.is_callable() {
            return self.call(
                &trap,
                Value::Obj(handler.clone()),
                vec![Value::Obj(target.clone()), key.to_value(), receiver.clone()],
            );
        }
        self.get_from(target, key, receiver)
    }

    /// [[Set]]; returns false when the assignment failed (strict callers throw).
    pub fn set(&mut self, target: &Value, key: Key, v: Value) -> JsResult<bool> {
        match target {
            Value::Obj(o) => self.set_on(o, key, v, target),
            Value::Undefined | Value::Null | Value::Empty => {
                let what = if matches!(target, Value::Null) {
                    "null"
                } else {
                    "undefined"
                };
                let k = self.key_display(&key);
                Err(self.type_error(format!("Cannot set properties of {what} (setting '{k}')")))
            }
            _ => {
                // Primitives: only setters on the prototype chain run.
                let proto = match target {
                    Value::Str(_) => self.intr.string_proto.clone(),
                    Value::Num(_) => self.intr.number_proto.clone(),
                    Value::Bool(_) => self.intr.boolean_proto.clone(),
                    Value::Sym(_) => self.intr.symbol_proto.clone(),
                    _ => self.intr.bigint_proto.clone(),
                };
                let mut cur = Some(proto);
                while let Some(c) = cur {
                    if let Some(Prop {
                        slot: Slot::Accessor(_, s),
                        ..
                    }) = c.borrow().props.get(&key).cloned()
                    {
                        if let Some(s) = s {
                            self.call(&Value::Obj(s), target.clone(), vec![v])?;
                            return Ok(true);
                        }
                        return Ok(false);
                    }
                    cur = c.proto();
                }
                Ok(false)
            }
        }
    }

    pub fn set_str(&mut self, target: &Value, k: &str, v: Value) -> JsResult<bool> {
        self.set(target, Key::str(k), v)
    }

    pub fn set_on(&mut self, o: &Obj, key: Key, v: Value, receiver: &Value) -> JsResult<bool> {
        let same = matches!(receiver, Value::Obj(r) if r.ptr_eq(o));
        if let Some(p) = &self.prof {
            use crate::profile::Counters;
            Counters::bump(&p.counters.set);
            let d = o.borrow();
            match d.props.get(&key) {
                Some(Prop {
                    slot: Slot::Data(_),
                    ..
                }) if same => Counters::bump(&p.counters.set_own_fast),
                None if same && matches!(d.kind, Kind::Ordinary | Kind::Function(_)) => {
                    Counters::bump(&p.counters.set_add)
                }
                _ => {}
            }
        }
        if same {
            // Exotic own elements.
            enum Ex {
                Done(bool),
                Length,
                Typed(usize),
                Proxy(Obj, Obj),
                Host(&'static HostHooks),
                No,
            }
            let ex = {
                let mut d = o.borrow_mut();
                let frozen = d.elems_frozen;
                let sealed = d.elems_sealed || !d.extensible;
                match &mut d.kind {
                    Kind::Array(arr) => match &key {
                        Key::Str(k) => {
                            if let Some(i) = array_index(k) {
                                let i = i as usize;
                                if frozen {
                                    Ex::Done(false)
                                } else if i < arr.len() {
                                    if sealed && matches!(arr[i], Value::Empty) {
                                        Ex::Done(false)
                                    } else {
                                        arr[i] = v.clone();
                                        Ex::Done(true)
                                    }
                                } else if sealed {
                                    Ex::Done(false)
                                } else if i < MAX_DENSE {
                                    arr.resize(i, Value::Empty);
                                    arr.push(v.clone());
                                    Ex::Done(true)
                                } else {
                                    Ex::Done(false)
                                }
                            } else if k.as_str() == "length" {
                                Ex::Length
                            } else {
                                Ex::No
                            }
                        }
                        _ => Ex::No,
                    },
                    Kind::TypedArray { .. } => match &key {
                        Key::Str(k) => match array_index(k) {
                            Some(i) => Ex::Typed(i as usize),
                            None => Ex::No,
                        },
                        _ => Ex::No,
                    },
                    Kind::Proxy { target, handler } => Ex::Proxy(target.clone(), handler.clone()),
                    Kind::Host(h) if !h.hooks.plain => Ex::Host(h.hooks),
                    Kind::String(s) => match &key {
                        Key::Str(k) => {
                            if k.as_str() == "length"
                                || array_index(k)
                                    .map(|i| (i as usize) < s.len16())
                                    .unwrap_or(false)
                            {
                                Ex::Done(false)
                            } else {
                                Ex::No
                            }
                        }
                        _ => Ex::No,
                    },
                    _ => Ex::No,
                }
            };
            match ex {
                Ex::Done(b) => return Ok(b),
                Ex::Length => {
                    let n = self.to_number(&v)?;
                    return self.set_array_length(o, n);
                }
                Ex::Typed(i) => {
                    self.typed_set(o, i, &v)?;
                    return Ok(true);
                }
                Ex::Proxy(t, h) => {
                    let trap = self.get_str(&Value::Obj(h.clone()), "set")?;
                    if trap.is_callable() {
                        let r = self.call(
                            &trap,
                            Value::Obj(h),
                            vec![Value::Obj(t), key.to_value(), v, receiver.clone()],
                        )?;
                        return Ok(r.truthy());
                    }
                    return self.set_on(&t, key, v, &Value::Obj(t.clone()));
                }
                Ex::Host(hooks) => {
                    let pk = self.prof_enter(|| format!("[host set] {}", hooks.class));
                    let r = (hooks.set)(self, o, &key, &v);
                    self.prof_leave(pk);
                    if let Some(ok) = r? {
                        return Ok(ok);
                    }
                }
                Ex::No => {}
            }
            // Fast path: own writable data property.
            {
                let mut d = o.borrow_mut();
                if let Some(p) = d.props.get_mut(&key) {
                    if let Slot::Data(_) = p.slot {
                        if p.writable() {
                            p.slot = Slot::Data(v);
                            return Ok(true);
                        }
                        return Ok(false);
                    }
                }
            }
        }
        // Walk the chain for setters / read-only inherited properties.
        let mut cur = Some(o.clone());
        let mut hops = 0;
        while let Some(c) = cur {
            if hops > 0 || !same {
                let p = {
                    let d = c.borrow();
                    if let Kind::Proxy { .. } = d.kind {
                        None
                    } else {
                        d.props.get(&key).cloned()
                    }
                };
                if let Some(p) = p {
                    match p.slot {
                        Slot::Accessor(_, s) => {
                            return match s {
                                Some(s) => {
                                    self.call(&Value::Obj(s), receiver.clone(), vec![v])?;
                                    Ok(true)
                                }
                                None => Ok(false),
                            };
                        }
                        Slot::Data(_) => {
                            if !p.writable() {
                                return Ok(false);
                            }
                            break;
                        }
                    }
                }
            } else {
                let p = c.borrow().props.get(&key).cloned();
                if let Some(Prop {
                    slot: Slot::Accessor(_, s),
                    ..
                }) = p
                {
                    return match s {
                        Some(s) => {
                            self.call(&Value::Obj(s), receiver.clone(), vec![v])?;
                            Ok(true)
                        }
                        None => Ok(false),
                    };
                }
            }
            cur = c.proto();
            hops += 1;
            if hops > 10_000 {
                break;
            }
        }
        // Create or update on the receiver.
        let Value::Obj(r) = receiver else {
            return Ok(false);
        };
        if !same {
            // A proxy receiver (`Reflect.set(target, key, value, proxy)` from a `set`
            // trap): its [[DefineOwnProperty]] runs the `defineProperty` trap or
            // forwards to its target.
            let mut rr = r.clone();
            loop {
                let next = match &rr.borrow().kind {
                    Kind::Proxy { target, handler } => Some((target.clone(), handler.clone())),
                    _ => None,
                };
                let Some((t, h)) = next else { break };
                let trap = self.get_str(&Value::Obj(h.clone()), "defineProperty")?;
                if trap.is_callable() {
                    let desc = self.new_object();
                    desc.set_prop("value", v, ALL);
                    if self.get_own(&t, &key)?.is_none() {
                        for k in ["writable", "enumerable", "configurable"] {
                            desc.set_prop(k, Value::Bool(true), ALL);
                        }
                    }
                    let ok = self.call(
                        &trap,
                        Value::Obj(h),
                        vec![Value::Obj(t), key.to_value(), Value::Obj(desc)],
                    )?;
                    return Ok(ok.truthy());
                }
                rr = t;
            }
            if rr.ptr_eq(o) {
                return self.set_on(o, key, v, &Value::Obj(o.clone()));
            }
            return self.create_data_property(&rr, key, v);
        }
        let mut d = r.borrow_mut();
        if let Some(p) = d.props.get_mut(&key) {
            if let Slot::Data(_) = p.slot {
                if p.writable() {
                    p.slot = Slot::Data(v);
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        if !d.extensible {
            return Ok(false);
        }
        d.props.insert(key, Prop::data(v, ALL));
        Ok(true)
    }

    pub fn set_array_length(&mut self, o: &Obj, n: f64) -> JsResult<bool> {
        if n < 0.0 || n.fract() != 0.0 || n > 4294967295.0 {
            return Err(self.range_error("Invalid array length"));
        }
        let n = n as usize;
        if n > MAX_DENSE {
            return Err(self.range_error("Invalid array length"));
        }
        let mut d = o.borrow_mut();
        if d.elems_frozen {
            return Ok(false);
        }
        if let Kind::Array(arr) = &mut d.kind {
            arr.resize(n, Value::Empty);
        }
        Ok(true)
    }

    /// CreateDataProperty: defines an own enumerable data property.
    pub fn create_data_property(&mut self, o: &Obj, key: Key, v: Value) -> JsResult<bool> {
        let mut d = o.borrow_mut();
        match &mut d.kind {
            Kind::Array(arr) => {
                if let Key::Str(k) = &key {
                    if let Some(i) = array_index(k) {
                        let i = i as usize;
                        if i < arr.len() {
                            arr[i] = v;
                        } else if i < MAX_DENSE {
                            arr.resize(i, Value::Empty);
                            arr.push(v);
                        }
                        return Ok(true);
                    }
                    if k.as_str() == "length" {
                        drop(d);
                        let n = self.to_number(&v)?;
                        return self.set_array_length(o, n);
                    }
                }
            }
            Kind::TypedArray { .. } => {
                if let Key::Str(k) = &key {
                    if let Some(i) = array_index(k) {
                        drop(d);
                        self.typed_set(o, i as usize, &v)?;
                        return Ok(true);
                    }
                }
            }
            _ => {}
        }
        if let Some(p) = d.props.get_mut(&key) {
            if !p.configurable() && !p.writable() {
                return Ok(false);
            }
            *p = Prop::data(v, ALL);
            return Ok(true);
        }
        if !d.extensible {
            return Ok(false);
        }
        d.props.insert(key, Prop::data(v, ALL));
        Ok(true)
    }

    /// Defines a property with full descriptor semantics (simplified checks).
    pub fn define_own(&mut self, o: &Obj, key: Key, prop: Prop) -> JsResult<bool> {
        {
            let mut d = o.borrow_mut();
            if let Kind::Array(arr) = &mut d.kind {
                if let Key::Str(k) = &key {
                    if let Some(i) = array_index(k) {
                        if let Slot::Data(v) = &prop.slot {
                            if prop.flags == ALL {
                                let i = i as usize;
                                if i < arr.len() {
                                    arr[i] = v.clone();
                                } else if i < MAX_DENSE {
                                    arr.resize(i, Value::Empty);
                                    arr.push(v.clone());
                                }
                                return Ok(true);
                            }
                        }
                        // Non-default attributes on an element: convert to a
                        // generic element (kept dense; attributes ignored).
                        if let Slot::Data(v) = &prop.slot {
                            let i = i as usize;
                            if i < arr.len() {
                                arr[i] = v.clone();
                            } else if i < MAX_DENSE {
                                arr.resize(i, Value::Empty);
                                arr.push(v.clone());
                            }
                            return Ok(true);
                        }
                    } else if k.as_str() == "length" {
                        if let Slot::Data(v) = &prop.slot {
                            let v = v.clone();
                            let frozen = prop.flags & WRITABLE == 0;
                            drop(d);
                            let n = self.to_number(&v)?;
                            self.set_array_length(o, n)?;
                            if frozen {
                                o.borrow_mut().elems_frozen = true;
                            }
                            return Ok(true);
                        }
                    }
                }
            }
        }
        let mut d = o.borrow_mut();
        if let Some(existing) = d.props.get_mut(&key) {
            if !existing.configurable() {
                // Only a writable data value may change.
                match (&existing.slot, &prop.slot) {
                    (Slot::Data(_), Slot::Data(v)) if existing.writable() => {
                        existing.slot = Slot::Data(v.clone());
                        if prop.flags & WRITABLE == 0 {
                            existing.flags &= !WRITABLE;
                        }
                        return Ok(true);
                    }
                    _ => return Ok(false),
                }
            }
            *existing = prop;
            return Ok(true);
        }
        if !d.extensible {
            return Ok(false);
        }
        d.props.insert(key, prop);
        Ok(true)
    }

    pub fn has_own(&mut self, o: &Obj, key: &Key) -> JsResult<bool> {
        Ok(self.get_own(o, key)?.is_some())
    }

    pub fn has_property(&mut self, o: &Obj, key: &Key) -> JsResult<bool> {
        let mut cur = Some(o.clone());
        while let Some(c) = cur {
            let proxy = match &c.borrow().kind {
                Kind::Proxy { target, handler } => Some((target.clone(), handler.clone())),
                _ => None,
            };
            if let Some((t, h)) = proxy {
                let trap = self.get_str(&Value::Obj(h.clone()), "has")?;
                if trap.is_callable() {
                    return Ok(self
                        .call(&trap, Value::Obj(h), vec![Value::Obj(t), key.to_value()])?
                        .truthy());
                }
                cur = Some(t);
                continue;
            }
            let host = c.host_hooks().filter(|h| !h.plain);
            if let Some(h) = host {
                let pk = self.prof_enter(|| format!("[host has] {}", h.class));
                let r = (h.get)(self, &c, key);
                self.prof_leave(pk);
                if r?.is_some() {
                    return Ok(true);
                }
            }
            let has = {
                let d = c.borrow();
                match &d.kind {
                    Kind::Array(v) => match key {
                        Key::Str(k) => match array_index(k) {
                            Some(i) => {
                                matches!(v.get(i as usize), Some(x) if !matches!(x, Value::Empty))
                            }
                            None => k.as_str() == "length" || d.props.find(key).is_some(),
                        },
                        _ => d.props.find(key).is_some(),
                    },
                    Kind::String(s) => match key {
                        Key::Str(k) => {
                            k.as_str() == "length"
                                || array_index(k)
                                    .map(|i| (i as usize) < s.len16())
                                    .unwrap_or(false)
                                || d.props.find(key).is_some()
                        }
                        _ => d.props.find(key).is_some(),
                    },
                    Kind::TypedArray { len, .. } => match key {
                        Key::Str(k) => match array_index(k) {
                            Some(i) => (i as usize) < *len,
                            None => d.props.find(key).is_some(),
                        },
                        _ => d.props.find(key).is_some(),
                    },
                    _ => d.props.find(key).is_some(),
                }
            };
            if has {
                return Ok(true);
            }
            cur = c.proto();
        }
        Ok(false)
    }

    /// [[Delete]]: false when the property is non-configurable.
    pub fn delete(&mut self, o: &Obj, key: &Key) -> JsResult<bool> {
        let proxy = match &o.borrow().kind {
            Kind::Proxy { target, handler } => Some((target.clone(), handler.clone())),
            _ => None,
        };
        if let Some((t, h)) = proxy {
            let trap = self.get_str(&Value::Obj(h.clone()), "deleteProperty")?;
            if trap.is_callable() {
                return Ok(self
                    .call(&trap, Value::Obj(h), vec![Value::Obj(t), key.to_value()])?
                    .truthy());
            }
            return self.delete(&t, key);
        }
        if let Some(h) = o.host_hooks().filter(|h| !h.plain) {
            if let Some(ok) = (h.delete)(self, o, key)? {
                return Ok(ok);
            }
        }
        let mut d = o.borrow_mut();
        let frozen = d.elems_frozen || d.elems_sealed;
        if let Kind::Array(arr) = &mut d.kind {
            if let Key::Str(k) = key {
                if let Some(i) = array_index(k) {
                    if frozen {
                        return Ok(false);
                    }
                    let i = i as usize;
                    if i < arr.len() {
                        arr[i] = Value::Empty;
                    }
                    return Ok(true);
                }
                if k.as_str() == "length" {
                    return Ok(false);
                }
            }
        }
        match d.props.get(key) {
            Some(p) if !p.configurable() => Ok(false),
            Some(_) => {
                d.props.remove(key);
                Ok(true)
            }
            None => Ok(true),
        }
    }

    /// [[OwnPropertyKeys]]: indices ascending, strings in insertion order,
    /// then symbols. Private names are never listed.
    pub fn own_keys(&mut self, o: &Obj) -> JsResult<Vec<Key>> {
        let proxy = match &o.borrow().kind {
            Kind::Proxy { target, handler } => Some((target.clone(), handler.clone())),
            _ => None,
        };
        if let Some((t, h)) = proxy {
            let trap = self.get_str(&Value::Obj(h.clone()), "ownKeys")?;
            if trap.is_callable() {
                let r = self.call(&trap, Value::Obj(h), vec![Value::Obj(t)])?;
                let items = self.iterable_to_vec(&r)?;
                let mut out = vec![];
                for it in items {
                    out.push(self.to_key(&it)?);
                }
                return Ok(out);
            }
            return self.own_keys(&t);
        }
        let host_keys = match o.host_hooks() {
            Some(h) if !h.plain => (h.keys)(self, o)?,
            _ => vec![],
        };
        let d = o.borrow();
        let mut idx: Vec<(u32, Key)> = vec![];
        let mut strs: Vec<Key> = vec![];
        let mut syms: Vec<Key> = vec![];
        for k in host_keys {
            match &k {
                Key::Str(s) => match array_index(s) {
                    Some(i) => idx.push((i, k.clone())),
                    None => strs.push(k.clone()),
                },
                Key::Sym(_) => syms.push(k.clone()),
            }
        }
        match &d.kind {
            Kind::Array(v) => {
                for (i, x) in v.iter().enumerate() {
                    if !matches!(x, Value::Empty) {
                        idx.push((i as u32, Key::Str(JsStr::new(i.to_string()))));
                    }
                }
            }
            Kind::String(s) => {
                for i in 0..s.len16() {
                    idx.push((i as u32, Key::Str(JsStr::new(i.to_string()))));
                }
            }
            Kind::TypedArray { len, .. } => {
                for i in 0..*len {
                    idx.push((i as u32, Key::Str(JsStr::new(i.to_string()))));
                }
            }
            _ => {}
        }
        let base_idx = idx.len();
        for (k, _) in &d.props.entries {
            match k {
                Key::Str(s) => match array_index(s) {
                    Some(i) => idx.push((i, k.clone())),
                    None => strs.push(k.clone()),
                },
                Key::Sym(sym) => {
                    if !sym.private {
                        syms.push(k.clone())
                    }
                }
            }
        }
        if idx.len() > base_idx || base_idx == 0 {
            idx.sort_by_key(|(i, _)| *i);
        }
        let mut out: Vec<Key> = idx.into_iter().map(|(_, k)| k).collect();
        if let Kind::Array(_) = d.kind {
            out.push(Key::str("length"));
        }
        if let Kind::String(_) = d.kind {
            out.push(Key::str("length"));
        }
        out.extend(strs);
        out.extend(syms);
        Ok(out)
    }

    /// Own enumerable string keys (Object.keys order).
    pub fn own_enum_keys(&mut self, o: &Obj) -> JsResult<Vec<JsStr>> {
        let is_plain = {
            let d = o.borrow();
            matches!(
                d.kind,
                Kind::Ordinary | Kind::Function(_) | Kind::Error(_) | Kind::Arguments
            )
        };
        if is_plain {
            let d = o.borrow();
            let mut idx: Vec<(u32, JsStr)> = vec![];
            let mut strs: Vec<JsStr> = vec![];
            for (k, p) in &d.props.entries {
                if let Key::Str(s) = k {
                    if !p.enumerable() {
                        continue;
                    }
                    match array_index(s) {
                        Some(i) => idx.push((i, s.clone())),
                        None => strs.push(s.clone()),
                    }
                }
            }
            idx.sort_by_key(|(i, _)| *i);
            let mut out: Vec<JsStr> = idx.into_iter().map(|(_, k)| k).collect();
            out.extend(strs);
            return Ok(out);
        }
        let keys = self.own_keys(o)?;
        let mut out = vec![];
        for k in keys {
            if let Key::Str(s) = &k {
                if let Some(p) = self.get_own(o, &k)? {
                    if p.enumerable() {
                        out.push(s.clone());
                    }
                }
            }
        }
        Ok(out)
    }

    pub fn array_len(&self, o: &Obj) -> Option<usize> {
        match &o.borrow().kind {
            Kind::Array(v) => Some(v.len()),
            _ => None,
        }
    }

    /// Length of an array-like (`length` property).
    pub fn length_of(&mut self, v: &Value) -> JsResult<usize> {
        if let Value::Obj(o) = v {
            if let Some(n) = self.array_len(o) {
                return Ok(n);
            }
        }
        let l = self.get_str(v, "length")?;
        self.to_length(&l)
    }

    pub fn index_key(i: usize) -> Key {
        Key::Str(JsStr::new(i.to_string()))
    }

    pub fn get_index(&mut self, v: &Value, i: usize) -> JsResult<Value> {
        if let Value::Obj(o) = v {
            if let Kind::Array(arr) = &o.borrow().kind {
                if let Some(x) = arr.get(i) {
                    if !matches!(x, Value::Empty) {
                        return Ok(x.clone());
                    }
                }
            }
        }
        self.get(v, &Self::index_key(i))
    }

    pub fn num_key(n: f64) -> Key {
        Key::Str(JsStr::new(number_to_string(n)))
    }
}
