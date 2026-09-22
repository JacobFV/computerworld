//! The object protocol: types, attributes, operators, comparison, hashing,
//! repr/str, iteration, subscripting and container helpers.
use crate::ast::{BinOp, CmpOp, UnaryOp};
use crate::bigint::BigInt;
use crate::value::*;
use crate::vm::*;
use std::cmp::Ordering;
use std::rc::Rc;

pub enum Num {
    I(i64),
    B(Rc<BigInt>),
    F(f64),
    C(f64, f64),
}

pub fn to_big(v: &Num) -> BigInt {
    match v {
        Num::I(i) => BigInt::from_i64(*i),
        Num::B(b) => (**b).clone(),
        _ => BigInt::zero(),
    }
}

impl<'h> Vm<'h> {
    // ------------------------------------------------------------------
    // Types
    // ------------------------------------------------------------------

    pub fn type_of(&self, v: &Value) -> Rc<Class> {
        let t = &self.t;
        match v {
            Value::Undefined => t.object.clone(),
            Value::None => t.none_type.clone(),
            Value::NotImplemented => t.notimpl_type.clone(),
            Value::Ellipsis => t.ellipsis_type.clone(),
            Value::Bool(_) => t.bool_.clone(),
            Value::Int(_) | Value::Big(_) => t.int.clone(),
            Value::Float(_) => t.float.clone(),
            Value::Complex(..) => t.complex.clone(),
            Value::Str(_) => t.str_.clone(),
            Value::Bytes(_) => t.bytes.clone(),
            Value::ByteArray(_) => t.bytearray.clone(),
            Value::Tuple(_) => t.tuple.clone(),
            Value::List(_) => t.list.clone(),
            Value::Dict(_) => t.dict.clone(),
            Value::Set(_) => t.set.clone(),
            Value::FrozenSet(_) => t.frozenset.clone(),
            Value::Range(_) => t.range.clone(),
            Value::Slice(_) => t.slice.clone(),
            Value::Func(_) => t.function.clone(),
            Value::Builtin(_) => t.builtin_function.clone(),
            Value::Method(_) => t.method.clone(),
            Value::Class(c) => c
                .metaclass
                .borrow()
                .clone()
                .unwrap_or_else(|| t.type_.clone()),
            Value::Instance(i) => i.class(),
            Value::Module(_) => t.module.clone(),
            Value::Gen(g) => {
                if g.borrow().is_coroutine {
                    t.coroutine.clone()
                } else {
                    t.generator.clone()
                }
            }
            Value::Iter(it) => match &*it.borrow() {
                IterObj::Enumerate { .. } => t.enumerate.clone(),
                IterObj::Zip { .. } => t.zip.clone(),
                IterObj::Map { .. } => t.map.clone(),
                IterObj::Filter { .. } => t.filter.clone(),
                IterObj::Reversed { .. } => t.reversed.clone(),
                IterObj::Range { .. } => t.range_iterator.clone(),
                IterObj::Str { .. } => t.str_iterator.clone(),
                IterObj::Dict { kind, .. } => match kind {
                    ViewKind::Keys => t.dict_keyiterator.clone(),
                    ViewKind::Values => t.dict_valueiterator.clone(),
                    ViewKind::Items => t.dict_itemiterator.clone(),
                },
                IterObj::Seq {
                    seq: Value::Tuple(_),
                    ..
                } => t.tuple_iterator.clone(),
                IterObj::List { .. } => t.set_iterator.clone(),
                _ => t.list_iterator.clone(),
            },
            Value::DictView(dv) => match dv.1 {
                ViewKind::Keys => t.dict_keys.clone(),
                ViewKind::Values => t.dict_values.clone(),
                ViewKind::Items => t.dict_items.clone(),
            },
            Value::Property(_) => t.property.clone(),
            Value::StaticMethod(_) => t.staticmethod.clone(),
            Value::ClassMethod(_) => t.classmethod.clone(),
            Value::Super(_) => t.super_.clone(),
            Value::File(_) => t.textio.clone(),
            Value::Cell(_) => t.cell.clone(),
            Value::Code(_) => t.code.clone(),
            Value::Native(n) => n.class.clone(),
        }
    }
    pub fn type_name(&self, v: &Value) -> String {
        self.type_of(v).name().to_string()
    }
    pub fn isinstance(&self, v: &Value, cls: &Rc<Class>) -> bool {
        self.type_of(v).is_subclass(cls)
    }
    /// The builtin value an instance of a builtin subclass wraps (or `v` itself).
    pub fn base_value(&self, v: &Value) -> Value {
        if let Value::Instance(i) = v {
            if let NativeData::Base(b) = &*i.native.borrow() {
                return b.clone();
            }
        }
        v.clone()
    }
    /// `id()`: value-derived for immediates; for heap objects a sequence number
    /// assigned on first request. The object is kept alive so its address can
    /// never be reused, which keeps ids (and default hashes) deterministic.
    /// Identical on every platform (64-bit arithmetic, never pointer values).
    pub fn object_id(&self, v: &Value) -> u64 {
        match v {
            Value::None => 0x9f7ee0,
            Value::NotImplemented => 0x9f8030,
            Value::Ellipsis => 0x9f8040,
            Value::Undefined => 1,
            Value::Bool(b) => 0x9f5fe0 + *b as u64 * 32,
            Value::Int(i) => 0x7f00_0000_0000u64.wrapping_add((*i as u64).wrapping_mul(32)),
            Value::Float(f) => f.to_bits(),
            _ => {
                let p = v.id();
                let mut m = self.id_map.borrow_mut();
                let n = m.len() as u64;
                m.entry(p)
                    .or_insert_with(|| (v.clone(), 0x7f3a_2c00_0000 + n * 0x30))
                    .1
            }
        }
    }

    // ------------------------------------------------------------------
    // Attributes
    // ------------------------------------------------------------------

    pub fn getattr_str(&mut self, obj: &Value, name: &str) -> PyResult<Value> {
        let n = Rc::new(PyStr::new(name.to_string()));
        self.getattr(obj, &n)
    }
    pub fn getattr_opt(&mut self, obj: &Value, name: &str) -> PyResult<Option<Value>> {
        match self.getattr_str(obj, name) {
            Ok(v) => Ok(Some(v)),
            Err(e) if self.err_matches(&e, "AttributeError") => Ok(None),
            Err(e) => Err(e),
        }
    }
    pub fn hasattr(&mut self, obj: &Value, name: &str) -> bool {
        matches!(self.getattr_opt(obj, name), Ok(Some(_)))
    }

    /// Finds a special method on the type (never the instance), as CPython does.
    pub fn lookup_special(&self, obj: &Value, name: &str) -> Option<Value> {
        let cls = self.type_of(obj);
        let v = cls.lookup(name)?;
        if v.is_none() {
            return None;
        }
        Some(v)
    }
    /// Calls a special method if the type defines it.
    pub fn call_special(
        &mut self,
        obj: &Value,
        name: &str,
        args: Vec<Value>,
    ) -> PyResult<Option<Value>> {
        match self.lookup_special(obj, name) {
            Some(m) => {
                let mut a = Vec::with_capacity(args.len() + 1);
                a.push(obj.clone());
                a.extend(args);
                // Static/class methods on the type still get the right binding.
                let m = match &m {
                    Value::StaticMethod(f) => return self.call(f, a[1..].to_vec()).map(Some),
                    Value::ClassMethod(f) => {
                        let cls = Value::Class(self.type_of(obj));
                        a[0] = cls;
                        return self.call(f, a).map(Some);
                    }
                    _ => m,
                };
                self.call(&m, a).map(Some)
            }
            None => Ok(None),
        }
    }

    pub fn bind_method(&self, attr: &Value, obj: &Value) -> Value {
        match attr {
            Value::Func(_) => Value::Method(Rc::new((obj.clone(), attr.clone()))),
            Value::Builtin(b) if b.owner.is_some() => {
                Value::Method(Rc::new((obj.clone(), attr.clone())))
            }
            Value::StaticMethod(f) => (**f).clone(),
            Value::ClassMethod(f) => {
                Value::Method(Rc::new((Value::Class(self.type_of(obj)), (**f).clone())))
            }
            _ => attr.clone(),
        }
    }

    fn is_data_descriptor(&self, attr: &Value) -> bool {
        match attr {
            Value::Property(_) => true,
            Value::Instance(i) => {
                let c = i.class();
                c.lookup("__set__").is_some() || c.lookup("__delete__").is_some()
            }
            _ => false,
        }
    }

    /// Applies the descriptor protocol for an attribute found on `cls`.
    fn descr_get(&mut self, attr: Value, obj: &Value, cls: &Rc<Class>) -> PyResult<Value> {
        match &attr {
            Value::Func(_) => Ok(Value::Method(Rc::new((obj.clone(), attr)))),
            Value::Builtin(b) if b.owner.is_some() => {
                Ok(Value::Method(Rc::new((obj.clone(), attr))))
            }
            Value::StaticMethod(f) => Ok((**f).clone()),
            Value::ClassMethod(f) => Ok(Value::Method(Rc::new((
                Value::Class(cls.clone()),
                (**f).clone(),
            )))),
            Value::Property(p) => {
                if p.fget.is_none() {
                    return Err(err("AttributeError", "property has no getter"));
                }
                let fget = p.fget.clone();
                self.call(&fget, vec![obj.clone()])
            }
            Value::Instance(i) => {
                let c = i.class();
                if let Some(get) = c.lookup("__get__") {
                    return self.call(
                        &get,
                        vec![attr.clone(), obj.clone(), Value::Class(cls.clone())],
                    );
                }
                Ok(attr)
            }
            _ => Ok(attr),
        }
    }

    pub fn getattr(&mut self, obj: &Value, name: &Rc<PyStr>) -> PyResult<Value> {
        match obj {
            Value::Instance(inst) => {
                let cls = inst.class();
                let attr = cls.lookup_pystr(name);
                if let Some(a) = &attr {
                    if self.is_data_descriptor(a) {
                        return self.descr_get(a.clone(), obj, &cls);
                    }
                }
                let own = inst.dict.borrow().get_pystr(name);
                if let Some(v) = own {
                    return Ok(v);
                }
                if let Some(a) = attr {
                    return self.descr_get(a, obj, &cls);
                }
                if let Some(v) = crate::builtins::instance_special_attr(self, obj, inst, &name.s)? {
                    return Ok(v);
                }
                if let Some(ga) = cls.lookup("__getattr__") {
                    return self.call(&ga, vec![obj.clone(), Value::Str(name.clone())]);
                }
                Err(self.attr_error(obj, &name.s))
            }
            Value::Class(cls) => self.class_getattr(cls, name),
            Value::Module(m) => {
                let v = m.dict.borrow().get_pystr(name);
                if let Some(v) = v {
                    return Ok(v);
                }
                if name.s == "__dict__" {
                    return Ok(Value::Dict(m.dict.clone()));
                }
                if let Some(ga) = m.dict.borrow().get_str("__getattr__") {
                    let ga = ga.clone();
                    return self.call(&ga, vec![Value::Str(name.clone())]);
                }
                let mut msg = format!("module '{}' has no attribute '{}'", m.name, name.s);
                let keys: Vec<String> = m
                    .dict
                    .borrow()
                    .keys()
                    .iter()
                    .filter_map(|k| k.as_pystr().map(|s| s.s.clone()))
                    .collect();
                if let Some(s) = crate::format::suggest(&name.s, &keys) {
                    msg.push_str(&format!(". Did you mean: '{s}'?"));
                }
                Err(err("AttributeError", msg))
            }
            Value::Super(sup) => self.super_getattr(sup, name),
            _ => {
                if let Some(v) = crate::builtins::special_attr(self, obj, &name.s)? {
                    return Ok(v);
                }
                let cls = self.type_of(obj);
                if let Some(a) = cls.lookup_pystr(name) {
                    return self.descr_get(a, obj, &cls);
                }
                Err(self.attr_error(obj, &name.s))
            }
        }
    }

    pub fn attr_error(&mut self, obj: &Value, name: &str) -> Box<PyErr> {
        let cls = self.type_of(obj);
        let mut candidates: Vec<String> = vec![];
        for c in cls.mro.borrow().iter() {
            for k in c.dict.borrow().keys() {
                if let Value::Str(s) = k {
                    candidates.push(s.s.clone());
                }
            }
        }
        if let Value::Instance(i) = obj {
            for k in i.dict.borrow().keys() {
                if let Value::Str(s) = k {
                    candidates.push(s.s.clone());
                }
            }
        }
        let mut msg = match obj {
            Value::Class(c) => format!("type object '{}' has no attribute '{}'", c.name(), name),
            _ => format!("'{}' object has no attribute '{}'", cls.name(), name),
        };
        if let Some(s) = crate::format::suggest(name, &candidates) {
            msg.push_str(&format!(". Did you mean: '{s}'?"));
        }
        err("AttributeError", msg)
    }

    fn class_getattr(&mut self, cls: &Rc<Class>, name: &Rc<PyStr>) -> PyResult<Value> {
        // Metaclass data descriptors first.
        let meta = cls.metaclass.borrow().clone();
        if let Some(meta) = &meta {
            if let Some(a) = meta.lookup_pystr(name) {
                if matches!(a, Value::Property(_)) {
                    return self.descr_get(a, &Value::Class(cls.clone()), meta);
                }
            }
        }
        if let Some(a) = cls.lookup_pystr(name) {
            return Ok(match &a {
                Value::ClassMethod(f) => {
                    Value::Method(Rc::new((Value::Class(cls.clone()), (**f).clone())))
                }
                Value::StaticMethod(f) => (**f).clone(),
                Value::Instance(i) => {
                    let c = i.class();
                    if let Some(get) = c.lookup("__get__") {
                        return self.call(
                            &get,
                            vec![a.clone(), Value::None, Value::Class(cls.clone())],
                        );
                    }
                    a
                }
                _ => a,
            });
        }
        if let Some(v) = crate::builtins::class_special_attr(self, cls, &name.s)? {
            return Ok(v);
        }
        if let Some(meta) = &meta {
            if let Some(a) = meta.lookup_pystr(name) {
                return self.descr_get(a, &Value::Class(cls.clone()), meta);
            }
        }
        // Methods of `type` itself (mro(), __subclasses__…).
        if let Some(a) = self.t.type_.lookup_pystr(name) {
            let tt = self.t.type_.clone();
            return self.descr_get(a, &Value::Class(cls.clone()), &tt);
        }
        Err(self.attr_error(&Value::Class(cls.clone()), &name.s))
    }

    fn super_getattr(&mut self, sup: &Rc<(Rc<Class>, Value)>, name: &Rc<PyStr>) -> PyResult<Value> {
        let (start, obj) = (&sup.0, &sup.1);
        let obj_cls = match obj {
            Value::Class(c) => c.clone(),
            _ => self.type_of(obj),
        };
        let mro = obj_cls.mro.borrow().clone();
        let pos = mro.iter().position(|c| Rc::ptr_eq(c, start));
        if let Some(pos) = pos {
            for c in &mro[pos + 1..] {
                let found = c.dict.borrow().get_pystr(name);
                if let Some(a) = found {
                    return match obj {
                        Value::Class(_) => Ok(match &a {
                            Value::ClassMethod(f) => {
                                Value::Method(Rc::new((obj.clone(), (**f).clone())))
                            }
                            Value::StaticMethod(f) => (**f).clone(),
                            Value::Func(_) => a,
                            Value::Builtin(b) if b.owner.is_some() && &*name.s == "__new__" => a,
                            _ => a,
                        }),
                        _ => self.descr_get(a, obj, &obj_cls),
                    };
                }
            }
        }
        if name.s == "__class__" {
            return Ok(Value::Class(self.t.super_.clone()));
        }
        Err(err(
            "AttributeError",
            format!("'super' object has no attribute '{}'", name.s),
        ))
    }

    pub fn setattr(&mut self, obj: &Value, name: &Rc<PyStr>, v: Value) -> PyResult<()> {
        match obj {
            Value::Instance(inst) => {
                let cls = inst.class();
                if let Some(sa) = cls.lookup("__setattr__") {
                    if !matches!(&sa, Value::Builtin(b) if &*b.name == "object.__setattr__") {
                        self.call(&sa, vec![obj.clone(), Value::Str(name.clone()), v])?;
                        return Ok(());
                    }
                }
                self.object_setattr(obj, inst, name, v)
            }
            Value::Class(cls) => {
                if cls.builtin {
                    return Err(type_err(format!(
                        "cannot set '{}' attribute of immutable type '{}'",
                        name.s,
                        cls.name()
                    )));
                }
                match &*name.s {
                    "__name__" => {
                        if let Value::Str(s) = &v {
                            *cls.name.borrow_mut() = s.s.as_str().into();
                        }
                    }
                    "__qualname__" => {
                        if let Value::Str(s) = &v {
                            *cls.qualname.borrow_mut() = s.s.as_str().into();
                        }
                    }
                    _ => {}
                }
                cls.dict.borrow_mut().set_pystr(name.clone(), v);
                Ok(())
            }
            Value::Module(m) => {
                m.dict.borrow_mut().set_pystr(name.clone(), v);
                Ok(())
            }
            Value::Func(f) => {
                match &*name.s {
                    "__name__" => {
                        if let Value::Str(s) = &v {
                            *f.name.borrow_mut() = s.s.as_str().into();
                        }
                    }
                    "__qualname__" => {
                        if let Value::Str(s) = &v {
                            *f.qualname.borrow_mut() = s.s.as_str().into();
                        }
                    }
                    "__doc__" => *f.doc.borrow_mut() = v,
                    "__defaults__" => {
                        *f.defaults.borrow_mut() = match v {
                            Value::Tuple(t) => (*t).clone(),
                            _ => vec![],
                        }
                    }
                    _ => f.dict.borrow_mut().set_pystr(name.clone(), v),
                }
                Ok(())
            }
            _ => {
                let tn = self.type_name(obj);
                let cls = self.type_of(obj);
                if cls.lookup_pystr(name).is_some() {
                    return Err(err(
                        "AttributeError",
                        format!("'{tn}' object attribute '{}' is read-only", name.s),
                    ));
                }
                Err(err(
                    "AttributeError",
                    format!("'{tn}' object has no attribute '{}'", name.s),
                ))
            }
        }
    }

    pub fn object_setattr(
        &mut self,
        obj: &Value,
        inst: &Rc<Instance>,
        name: &Rc<PyStr>,
        v: Value,
    ) -> PyResult<()> {
        let cls = inst.class();
        if let Some(a) = cls.lookup_pystr(name) {
            match &a {
                Value::Property(p) => {
                    if p.fset.is_none() {
                        return Err(err(
                            "AttributeError",
                            format!(
                                "property '{}' of '{}' object has no setter",
                                name.s,
                                cls.name()
                            ),
                        ));
                    }
                    let fset = p.fset.clone();
                    self.call(&fset, vec![obj.clone(), v])?;
                    return Ok(());
                }
                Value::Instance(i) => {
                    if let Some(set) = i.class().lookup("__set__") {
                        self.call(&set, vec![a.clone(), obj.clone(), v])?;
                        return Ok(());
                    }
                }
                _ => {}
            }
        }
        if let Some(r) = crate::builtins::instance_set_special(self, inst, &name.s, &v)? {
            return Ok(r);
        }
        inst.dict.borrow_mut().set_pystr(name.clone(), v);
        Ok(())
    }

    pub fn delattr(&mut self, obj: &Value, name: &Rc<PyStr>) -> PyResult<()> {
        match obj {
            Value::Instance(inst) => {
                let cls = inst.class();
                if let Some(da) = cls.lookup("__delattr__") {
                    if !matches!(&da, Value::Builtin(_)) {
                        self.call(&da, vec![obj.clone(), Value::Str(name.clone())])?;
                        return Ok(());
                    }
                }
                if let Some(Value::Property(p)) = cls.lookup_pystr(name) {
                    if !p.fdel.is_none() {
                        let fdel = p.fdel.clone();
                        self.call(&fdel, vec![obj.clone()])?;
                        return Ok(());
                    }
                }
                if inst.dict.borrow_mut().del_str(&name.s).is_none() {
                    return Err(self.attr_error(obj, &name.s));
                }
                Ok(())
            }
            Value::Class(c) => {
                if c.dict.borrow_mut().del_str(&name.s).is_none() {
                    return Err(self.attr_error(obj, &name.s));
                }
                Ok(())
            }
            Value::Module(m) => {
                if m.dict.borrow_mut().del_str(&name.s).is_none() {
                    return Err(err(
                        "AttributeError",
                        format!("module '{}' has no attribute '{}'", m.name, name.s),
                    ));
                }
                Ok(())
            }
            Value::Func(f) => {
                f.dict.borrow_mut().del_str(&name.s);
                Ok(())
            }
            _ => Err(self.attr_error(obj, &name.s)),
        }
    }

    // ------------------------------------------------------------------
    // Numbers
    // ------------------------------------------------------------------

    pub fn as_num(&self, v: &Value) -> Option<Num> {
        match v {
            Value::Int(i) => Some(Num::I(*i)),
            Value::Bool(b) => Some(Num::I(*b as i64)),
            Value::Big(b) => Some(Num::B(b.clone())),
            Value::Float(f) => Some(Num::F(*f)),
            Value::Complex(a, b) => Some(Num::C(*a, *b)),
            Value::Instance(i) => match &*i.native.borrow() {
                NativeData::Base(b) => self.as_num(b),
                _ => None,
            },
            _ => None,
        }
    }
    pub fn num_to_f64(&self, n: &Num) -> PyResult<f64> {
        match n {
            Num::I(i) => Ok(*i as f64),
            Num::B(b) => b
                .to_f64()
                .ok_or_else(|| err("OverflowError", "int too large to convert to float")),
            Num::F(f) => Ok(*f),
            Num::C(..) => Err(type_err("can't convert complex to float")),
        }
    }

    fn int_binop(&mut self, a: &Num, b: &Num, op: BinOp) -> PyResult<Value> {
        if let (Num::I(x), Num::I(y)) = (a, b) {
            let (x, y) = (*x, *y);
            let r = match op {
                BinOp::Add => x.checked_add(y),
                BinOp::Sub => x.checked_sub(y),
                BinOp::Mul => x.checked_mul(y),
                BinOp::FloorDiv => {
                    if y == 0 {
                        return Err(err(
                            "ZeroDivisionError",
                            "integer division or modulo by zero",
                        ));
                    }
                    x.checked_div_euclid(y).map(|_| {
                        let q = x / y;
                        if (x % y != 0) && ((x < 0) != (y < 0)) {
                            q - 1
                        } else {
                            q
                        }
                    })
                }
                BinOp::Mod => {
                    if y == 0 {
                        return Err(err("ZeroDivisionError", "integer modulo by zero"));
                    }
                    x.checked_rem(y).map(|r| {
                        if r != 0 && ((r < 0) != (y < 0)) {
                            r + y
                        } else {
                            r
                        }
                    })
                }
                BinOp::BitAnd => Some(x & y),
                BinOp::BitOr => Some(x | y),
                BinOp::BitXor => Some(x ^ y),
                BinOp::LShift => {
                    if y < 0 {
                        return Err(value_err("negative shift count"));
                    }
                    if y < 63 && x.unsigned_abs() < (1u64 << (62 - y)) {
                        Some(x << y)
                    } else if x == 0 {
                        Some(0)
                    } else {
                        None
                    }
                }
                BinOp::RShift => {
                    if y < 0 {
                        return Err(value_err("negative shift count"));
                    }
                    Some(if y >= 64 {
                        if x < 0 {
                            -1
                        } else {
                            0
                        }
                    } else {
                        x >> y
                    })
                }
                BinOp::Div => {
                    if y == 0 {
                        return Err(err("ZeroDivisionError", "division by zero"));
                    }
                    if x.unsigned_abs() < (1 << 53) && y.unsigned_abs() < (1 << 53) {
                        return Ok(Value::Float(x as f64 / y as f64));
                    }
                    None
                }
                BinOp::Pow => {
                    if y < 0 {
                        if x == 0 {
                            return Err(err(
                                "ZeroDivisionError",
                                "0.0 cannot be raised to a negative power",
                            ));
                        }
                        return Ok(Value::Float((x as f64).powf(y as f64)));
                    }
                    if y <= u32::MAX as i64 {
                        x.checked_pow(y as u32)
                    } else {
                        None
                    }
                }
                BinOp::MatMul => return Ok(Value::NotImplemented),
            };
            if let Some(r) = r {
                return Ok(Value::Int(r));
            }
        }
        let (x, y) = (to_big(a), to_big(b));
        let r = match op {
            BinOp::Add => x.add(&y),
            BinOp::Sub => x.sub(&y),
            BinOp::Mul => {
                if x.bit_length() + y.bit_length() > 8_000_000 {
                    return Err(err("MemoryError", ""));
                }
                x.mul(&y)
            }
            BinOp::FloorDiv | BinOp::Mod => {
                if y.is_zero() {
                    return Err(err(
                        "ZeroDivisionError",
                        if op == BinOp::Mod {
                            "integer modulo by zero"
                        } else {
                            "integer division or modulo by zero"
                        },
                    ));
                }
                let (q, r) = x.divmod_floor(&y);
                if op == BinOp::FloorDiv {
                    q
                } else {
                    r
                }
            }
            BinOp::Div => {
                if y.is_zero() {
                    return Err(err("ZeroDivisionError", "division by zero"));
                }
                return Ok(Value::Float(big_true_div(&x, &y)?));
            }
            BinOp::BitAnd => x.bitop(&y, '&'),
            BinOp::BitOr => x.bitop(&y, '|'),
            BinOp::BitXor => x.bitop(&y, '^'),
            BinOp::LShift => {
                if y.is_negative() {
                    return Err(value_err("negative shift count"));
                }
                let s = y.to_i64().unwrap_or(i64::MAX);
                if s as u64 + x.bit_length() > 8_000_000 {
                    return Err(err("OverflowError", "too many digits in integer"));
                }
                x.shl(s as u64)
            }
            BinOp::RShift => {
                if y.is_negative() {
                    return Err(value_err("negative shift count"));
                }
                match y.to_i64() {
                    Some(s) => x.shr(s as u64),
                    None => BigInt::from_i64(if x.is_negative() { -1 } else { 0 }),
                }
            }
            BinOp::Pow => {
                if y.is_negative() {
                    let fx = x.to_f64().unwrap_or(f64::INFINITY);
                    let fy = y.to_f64().unwrap_or(f64::NEG_INFINITY);
                    return Ok(Value::Float(fx.powf(fy)));
                }
                let e = y.to_i64().unwrap_or(i64::MAX) as u64;
                if x.bit_length() > 1 && x.bit_length().saturating_mul(e) > 8_000_000 {
                    return Err(err("MemoryError", ""));
                }
                if x.bit_length() <= 1 {
                    // 0, 1, -1
                    let base = x.to_i64().unwrap_or(0);
                    return Ok(Value::Int(match base {
                        0 => i64::from(e == 0),
                        1 => 1,
                        _ => {
                            if e.is_multiple_of(2) {
                                1
                            } else {
                                -1
                            }
                        }
                    }));
                }
                x.pow(e)
            }
            BinOp::MatMul => return Ok(Value::NotImplemented),
        };
        Ok(Value::big(r))
    }

    fn float_binop(&mut self, x: f64, y: f64, op: BinOp) -> PyResult<Value> {
        Ok(Value::Float(match op {
            BinOp::Add => x + y,
            BinOp::Sub => x - y,
            BinOp::Mul => x * y,
            BinOp::Div => {
                if y == 0.0 {
                    return Err(err("ZeroDivisionError", "float division by zero"));
                }
                x / y
            }
            BinOp::FloorDiv => {
                if y == 0.0 {
                    return Err(err("ZeroDivisionError", "float floor division by zero"));
                }
                float_divmod(x, y).0
            }
            BinOp::Mod => {
                if y == 0.0 {
                    return Err(err("ZeroDivisionError", "float modulo"));
                }
                float_divmod(x, y).1
            }
            BinOp::Pow => {
                if x == 0.0 && y < 0.0 {
                    return Err(err(
                        "ZeroDivisionError",
                        "0.0 cannot be raised to a negative power",
                    ));
                }
                if x < 0.0 && y.fract() != 0.0 && y.is_finite() {
                    // Negative base, fractional exponent: complex result.
                    let r = (-x).powf(y);
                    let th = std::f64::consts::PI * y;
                    return Ok(Value::Complex(r * th.cos(), r * th.sin()));
                }
                let r = x.powf(y);
                if r.is_infinite() && x.is_finite() && y.is_finite() {
                    return Err(err(
                        "OverflowError",
                        "(34, 'Numerical result out of range')",
                    ));
                }
                r
            }
            _ => return Ok(Value::NotImplemented),
        }))
    }

    fn complex_binop(&mut self, a: (f64, f64), b: (f64, f64), op: BinOp) -> PyResult<Value> {
        let (ar, ai) = a;
        let (br, bi) = b;
        Ok(match op {
            BinOp::Add => Value::Complex(ar + br, ai + bi),
            BinOp::Sub => Value::Complex(ar - br, ai - bi),
            BinOp::Mul => Value::Complex(ar * br - ai * bi, ar * bi + ai * br),
            BinOp::Div => {
                let d = br * br + bi * bi;
                if d == 0.0 {
                    return Err(err("ZeroDivisionError", "complex division by zero"));
                }
                Value::Complex((ar * br + ai * bi) / d, (ai * br - ar * bi) / d)
            }
            BinOp::Pow => {
                if br == 0.0 && bi == 0.0 {
                    return Ok(Value::Complex(1.0, 0.0));
                }
                if ar == 0.0 && ai == 0.0 {
                    return Ok(Value::Complex(0.0, 0.0));
                }
                // Integer exponent: repeated multiplication for exactness.
                if bi == 0.0 && br.fract() == 0.0 && br.abs() <= 100.0 {
                    let mut acc = (1.0, 0.0);
                    let n = br.abs() as i32;
                    for _ in 0..n {
                        acc = (acc.0 * ar - acc.1 * ai, acc.0 * ai + acc.1 * ar);
                    }
                    if br < 0.0 {
                        let d = acc.0 * acc.0 + acc.1 * acc.1;
                        acc = (acc.0 / d, -acc.1 / d);
                    }
                    return Ok(Value::Complex(acc.0, acc.1));
                }
                let r = (ar * ar + ai * ai).sqrt();
                let th = ai.atan2(ar);
                let lr = r.ln();
                let nr = (br * lr - bi * th).exp();
                let nt = bi * lr + br * th;
                Value::Complex(nr * nt.cos(), nr * nt.sin())
            }
            _ => Value::NotImplemented,
        })
    }

    /// Native arithmetic; `NotImplemented` when the types don't apply.
    pub fn native_binop(&mut self, a: &Value, b: &Value, op: BinOp) -> PyResult<Value> {
        match (a, b) {
            // `str.__mod__` formats any object (only a str subclass's `__rmod__`
            // would get the first chance).
            (Value::Str(_), Value::Instance(i))
                if op == BinOp::Mod
                    && !matches!(&*i.native.borrow(), NativeData::Base(Value::Str(_))) =>
            {
                let s = crate::format::percent_format(self, a, b)?;
                return Ok(Value::string(s));
            }
            (Value::Instance(_), _) | (_, Value::Instance(_)) => return Ok(Value::NotImplemented),
            _ => {}
        }
        if let (Some(x), Some(y)) = (self.as_num(a), self.as_num(b)) {
            return match (&x, &y) {
                (Num::C(..), _) | (_, Num::C(..)) => {
                    let ca = match x {
                        Num::C(r, i) => (r, i),
                        other => (self.num_to_f64(&other)?, 0.0),
                    };
                    let cb = match y {
                        Num::C(r, i) => (r, i),
                        other => (self.num_to_f64(&other)?, 0.0),
                    };
                    self.complex_binop(ca, cb, op)
                }
                (Num::F(_), _) | (_, Num::F(_)) => {
                    if matches!(
                        op,
                        BinOp::BitAnd
                            | BinOp::BitOr
                            | BinOp::BitXor
                            | BinOp::LShift
                            | BinOp::RShift
                    ) {
                        return Ok(Value::NotImplemented);
                    }
                    let fx = self.num_to_f64(&x)?;
                    let fy = self.num_to_f64(&y)?;
                    self.float_binop(fx, fy, op)
                }
                _ => {
                    if let (Value::Bool(p), Value::Bool(q)) = (a, b) {
                        match op {
                            BinOp::BitAnd => return Ok(Value::Bool(*p & *q)),
                            BinOp::BitOr => return Ok(Value::Bool(*p | *q)),
                            BinOp::BitXor => return Ok(Value::Bool(*p ^ *q)),
                            _ => {}
                        }
                    }
                    self.int_binop(&x, &y, op)
                }
            };
        }
        match (a, b, op) {
            (Value::Str(x), Value::Str(y), BinOp::Add) => {
                let mut s = String::with_capacity(x.s.len() + y.s.len());
                s.push_str(&x.s);
                s.push_str(&y.s);
                Ok(Value::string(s))
            }
            (Value::Str(x), _, BinOp::Mul) | (_, Value::Str(x), BinOp::Mul) => {
                let other = if matches!(a, Value::Str(_)) { b } else { a };
                match self.repeat_count(other) {
                    Some(n) => {
                        if n as usize * x.s.len() > 200_000_000 {
                            return Err(err("MemoryError", ""));
                        }
                        Ok(Value::string(x.s.repeat(n.max(0) as usize)))
                    }
                    None => Ok(Value::NotImplemented),
                }
            }
            (Value::Str(_), _, BinOp::Mod) => {
                let s = crate::format::percent_format(self, a, b)?;
                Ok(Value::string(s))
            }
            (Value::List(x), Value::List(y), BinOp::Add) => {
                let mut v = x.borrow().clone();
                v.extend(y.borrow().iter().cloned());
                Ok(Value::list(v))
            }
            (Value::Tuple(x), Value::Tuple(y), BinOp::Add) => {
                let mut v = (**x).clone();
                v.extend(y.iter().cloned());
                Ok(Value::tuple(v))
            }
            (Value::Bytes(x), Value::Bytes(y), BinOp::Add) => {
                let mut v = (**x).clone();
                v.extend(y.iter());
                Ok(Value::Bytes(Rc::new(v)))
            }
            (Value::ByteArray(x), Value::Bytes(y), BinOp::Add) => {
                let mut v = x.borrow().clone();
                v.extend(y.iter());
                Ok(Value::ByteArray(new_ref(v)))
            }
            (Value::List(_) | Value::Tuple(_) | Value::Bytes(_), _, BinOp::Mul)
            | (_, Value::List(_) | Value::Tuple(_) | Value::Bytes(_), BinOp::Mul) => {
                let (seq, other) =
                    if matches!(a, Value::List(_) | Value::Tuple(_) | Value::Bytes(_)) {
                        (a, b)
                    } else {
                        (b, a)
                    };
                let Some(n) = self.repeat_count(other) else {
                    return Ok(Value::NotImplemented);
                };
                let n = n.max(0) as usize;
                match seq {
                    Value::List(l) => {
                        let l = l.borrow();
                        if l.len().saturating_mul(n) > 50_000_000 {
                            return Err(err("MemoryError", ""));
                        }
                        let mut v = Vec::with_capacity(l.len() * n);
                        for _ in 0..n {
                            v.extend(l.iter().cloned());
                        }
                        Ok(Value::list(v))
                    }
                    Value::Tuple(t) => {
                        if t.len().saturating_mul(n) > 50_000_000 {
                            return Err(err("MemoryError", ""));
                        }
                        let mut v = Vec::with_capacity(t.len() * n);
                        for _ in 0..n {
                            v.extend(t.iter().cloned());
                        }
                        Ok(Value::tuple(v))
                    }
                    Value::Bytes(bs) => Ok(Value::Bytes(Rc::new(bs.repeat(n)))),
                    _ => Ok(Value::NotImplemented),
                }
            }
            (Value::Set(_) | Value::FrozenSet(_), Value::Set(_) | Value::FrozenSet(_), _)
            | (
                Value::DictView(_),
                Value::Set(_)
                | Value::FrozenSet(_)
                | Value::DictView(_)
                | Value::List(_)
                | Value::Tuple(_)
                | Value::Str(_)
                | Value::Dict(_)
                | Value::Range(_)
                | Value::Iter(_)
                | Value::Gen(_),
                BinOp::BitOr | BinOp::BitAnd | BinOp::Sub | BinOp::BitXor,
            )
            | (Value::Set(_) | Value::FrozenSet(_), Value::DictView(_), _) => {
                crate::builtins::set_binop(self, a, b, op)
            }
            (Value::Dict(x), Value::Dict(y), BinOp::BitOr) => {
                let d = new_ref(x.borrow().clone());
                let items = y.borrow().items();
                for (k, v) in items {
                    self.dict_set(&d, k, v)?;
                }
                Ok(Value::Dict(d))
            }
            _ => Ok(Value::NotImplemented),
        }
    }

    fn repeat_count(&self, v: &Value) -> Option<i64> {
        match v {
            Value::Int(i) => Some(*i),
            Value::Bool(b) => Some(*b as i64),
            Value::Big(b) => Some(if b.is_negative() { 0 } else { i64::MAX }),
            _ => None,
        }
    }

    pub fn binary_op(&mut self, a: &Value, b: &Value, op: BinOp) -> PyResult<Value> {
        let r = self.native_binop(a, b, op)?;
        if !matches!(r, Value::NotImplemented) {
            return Ok(r);
        }
        let (l, rname, _) = op.dunder();
        let ta = self.type_of(a);
        let tb = self.type_of(b);
        // A subclass's reflected method gets the first chance.
        let b_first = !Rc::ptr_eq(&ta, &tb)
            && tb.is_subclass(&ta)
            && tb.lookup(rname).is_some()
            && !matches!(
                (tb.lookup(rname), ta.lookup(rname)),
                (Some(x), Some(y)) if x.is(&y)
            );
        if b_first {
            if let Some(r) = self.call_special(b, rname, vec![a.clone()])? {
                if !matches!(r, Value::NotImplemented) {
                    return Ok(r);
                }
            }
        }
        if let Some(r) = self.call_special(a, l, vec![b.clone()])? {
            if !matches!(r, Value::NotImplemented) {
                return Ok(r);
            }
        }
        if !b_first && !Rc::ptr_eq(&ta, &tb) {
            if let Some(r) = self.call_special(b, rname, vec![a.clone()])? {
                if !matches!(r, Value::NotImplemented) {
                    return Ok(r);
                }
            }
        }
        Err(self.binop_type_error(a, b, op))
    }

    fn binop_type_error(&self, a: &Value, b: &Value, op: BinOp) -> Box<PyErr> {
        let (ta, tb) = (self.type_name(a), self.type_name(b));
        match (a, op) {
            (Value::Str(_), BinOp::Add) => {
                return type_err(format!("can only concatenate str (not \"{tb}\") to str"))
            }
            (Value::List(_), BinOp::Add) => {
                return type_err(format!("can only concatenate list (not \"{tb}\") to list"))
            }
            (Value::Tuple(_), BinOp::Add) => {
                return type_err(format!(
                    "can only concatenate tuple (not \"{tb}\") to tuple"
                ))
            }
            (Value::Str(_) | Value::List(_) | Value::Tuple(_), BinOp::Mul) => {
                return type_err(format!("can't multiply sequence by non-int of type '{tb}'"))
            }
            _ => {}
        }
        if op == BinOp::Mul && matches!(b, Value::Str(_) | Value::List(_) | Value::Tuple(_)) {
            return type_err(format!("can't multiply sequence by non-int of type '{ta}'"));
        }
        let sym = if op == BinOp::Pow {
            "** or pow()"
        } else {
            op.symbol()
        };
        type_err(format!(
            "unsupported operand type(s) for {sym}: '{ta}' and '{tb}'"
        ))
    }

    pub fn inplace_op(&mut self, a: &Value, b: &Value, op: BinOp) -> PyResult<Value> {
        match (a, op) {
            (Value::List(l), BinOp::Add) => {
                let items = self.iterate(b).map_err(|e| {
                    if self.err_matches(&e, "TypeError") {
                        type_err(format!("'{}' object is not iterable", self.type_name(b)))
                    } else {
                        e
                    }
                })?;
                l.borrow_mut().extend(items);
                return Ok(a.clone());
            }
            (Value::List(l), BinOp::Mul) => {
                let Some(n) = self.repeat_count(b) else {
                    return Err(self.binop_type_error(a, b, op));
                };
                let items = l.borrow().clone();
                let mut v = Vec::new();
                for _ in 0..n.max(0) {
                    v.extend(items.iter().cloned());
                }
                *l.borrow_mut() = v;
                return Ok(a.clone());
            }
            (Value::Set(s), BinOp::BitOr | BinOp::BitAnd | BinOp::Sub | BinOp::BitXor)
                if matches!(b, Value::Set(_) | Value::FrozenSet(_)) =>
            {
                let r = crate::builtins::set_binop(self, a, b, op)?;
                if let Value::Set(r) = r {
                    let data = r.borrow().clone();
                    *s.borrow_mut() = data;
                }
                return Ok(a.clone());
            }
            (Value::Dict(d), BinOp::BitOr) => {
                let items = self.mapping_items(b)?;
                for (k, v) in items {
                    self.dict_set(d, k, v)?;
                }
                return Ok(a.clone());
            }
            (Value::ByteArray(x), BinOp::Add) => {
                let add: Vec<u8> = match b {
                    Value::Bytes(y) => (**y).clone(),
                    Value::ByteArray(y) => y.borrow().clone(),
                    _ => return Err(self.binop_type_error(a, b, op)),
                };
                x.borrow_mut().extend(add);
                return Ok(a.clone());
            }
            _ => {}
        }
        if let Value::Instance(_) = a {
            let (_, _, iname) = op.dunder();
            if let Some(r) = self.call_special(a, iname, vec![b.clone()])? {
                if !matches!(r, Value::NotImplemented) {
                    return Ok(r);
                }
            }
        }
        self.binary_op(a, b, op).map_err(|e| {
            if self.err_matches(&e, "TypeError") {
                if let ErrKind::Lazy(_, args) = &e.kind {
                    if let Some(Value::Str(s)) = args.first() {
                        if s.s.starts_with("unsupported operand") {
                            let fixed = s.s.replacen(
                                &format!("for {}:", op.symbol()),
                                &format!("for {}=:", op.symbol()),
                                1,
                            );
                            return type_err(fixed);
                        }
                    }
                }
            }
            e
        })
    }

    pub fn unary_op(&mut self, a: &Value, op: UnaryOp) -> PyResult<Value> {
        if op == UnaryOp::Not {
            return Ok(Value::Bool(!self.truthy(a)?));
        }
        if !matches!(a, Value::Instance(_)) {
            if let Some(n) = self.as_num(a) {
                return Ok(match (op, n) {
                    (UnaryOp::Neg, Num::I(i)) => match i.checked_neg() {
                        Some(v) => Value::Int(v),
                        None => Value::big(BigInt::from_i64(i).neg()),
                    },
                    (UnaryOp::Neg, Num::B(b)) => Value::big(b.neg()),
                    (UnaryOp::Neg, Num::F(f)) => Value::Float(-f),
                    (UnaryOp::Neg, Num::C(r, i)) => Value::Complex(-r, -i),
                    (UnaryOp::Pos, Num::I(i)) => Value::Int(i),
                    (UnaryOp::Pos, Num::B(b)) => Value::Big(b),
                    (UnaryOp::Pos, Num::F(f)) => Value::Float(f),
                    (UnaryOp::Pos, Num::C(r, i)) => Value::Complex(r, i),
                    (UnaryOp::Invert, Num::I(i)) => Value::Int(!i),
                    (UnaryOp::Invert, Num::B(b)) => Value::big(b.neg().sub(&BigInt::from_i64(1))),
                    _ => {
                        return Err(type_err(format!(
                            "bad operand type for unary {}: '{}'",
                            unary_symbol(op),
                            self.type_name(a)
                        )))
                    }
                });
            }
        }
        let name = match op {
            UnaryOp::Neg => "__neg__",
            UnaryOp::Pos => "__pos__",
            _ => "__invert__",
        };
        if let Some(r) = self.call_special(a, name, vec![])? {
            return Ok(r);
        }
        Err(type_err(format!(
            "bad operand type for unary {}: '{}'",
            unary_symbol(op),
            self.type_name(a)
        )))
    }

    // ------------------------------------------------------------------
    // Comparison
    // ------------------------------------------------------------------

    pub fn truthy(&mut self, v: &Value) -> PyResult<bool> {
        Ok(match v {
            Value::None | Value::Undefined => false,
            Value::Bool(b) => *b,
            Value::Int(i) => *i != 0,
            Value::Big(b) => !b.is_zero(),
            Value::Float(f) => *f != 0.0,
            Value::Complex(a, b) => *a != 0.0 || *b != 0.0,
            Value::Str(s) => !s.s.is_empty(),
            Value::Bytes(b) => !b.is_empty(),
            Value::ByteArray(b) => !b.borrow().is_empty(),
            Value::Tuple(t) => !t.is_empty(),
            Value::List(l) => !l.borrow().is_empty(),
            Value::Dict(d) => !d.borrow().is_empty(),
            Value::Set(s) => !s.borrow().is_empty(),
            Value::FrozenSet(s) => !s.is_empty(),
            Value::Range(r) => !r.is_empty(),
            Value::DictView(dv) => !dv.0.borrow().is_empty(),
            Value::Instance(_) | Value::Native(_) => {
                if let Some(r) = self.call_special(v, "__bool__", vec![])? {
                    return match r {
                        Value::Bool(b) => Ok(b),
                        _ => Err(type_err(format!(
                            "__bool__ should return bool, returned {}",
                            self.type_name(&r)
                        ))),
                    };
                }
                if let Some(r) = self.call_special(v, "__len__", vec![])? {
                    return Ok(self.index_of(&r)? != 0);
                }
                true
            }
            _ => true,
        })
    }

    /// `a == b` as a bool.
    pub fn eq(&mut self, a: &Value, b: &Value) -> PyResult<bool> {
        if let Some(r) = fast_eq(a, b) {
            return Ok(r);
        }
        let r = self.rich_compare(a, b, CmpOp::Eq)?;
        self.truthy(&r)
    }

    fn seq_eq(&mut self, x: &[Value], y: &[Value]) -> PyResult<bool> {
        if x.len() != y.len() {
            return Ok(false);
        }
        for (p, q) in x.iter().zip(y.iter()) {
            if p.is(q) {
                continue;
            }
            if !self.eq(p, q)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Native equality; None when the types need user methods.
    fn native_eq(&mut self, a: &Value, b: &Value) -> PyResult<Option<bool>> {
        if let Some(r) = fast_eq(a, b) {
            return Ok(Some(r));
        }
        Ok(Some(match (a, b) {
            (Value::Instance(_), _) | (_, Value::Instance(_)) => return Ok(None),
            (Value::Big(_) | Value::Float(_), Value::Big(_) | Value::Float(_)) => {
                match (self.as_num(a), self.as_num(b)) {
                    (Some(x), Some(y)) => {
                        let nan = matches!(x, Num::F(f) if f.is_nan())
                            || matches!(y, Num::F(f) if f.is_nan());
                        !nan && num_cmp(&x, &y) == Ordering::Equal
                    }
                    _ => false,
                }
            }
            (Value::List(x), Value::List(y)) => {
                if Rc::ptr_eq(x, y) {
                    return Ok(Some(true));
                }
                let (x, y) = (x.borrow().clone(), y.borrow().clone());
                self.seq_eq(&x, &y)?
            }
            (Value::Tuple(x), Value::Tuple(y)) => {
                let (x, y) = (x.clone(), y.clone());
                self.seq_eq(&x, &y)?
            }
            (Value::Dict(x), Value::Dict(y)) => {
                if x.borrow().len() != y.borrow().len() {
                    return Ok(Some(false));
                }
                let items = x.borrow().items();
                for (k, v) in items {
                    match self.dict_get(y, &k)? {
                        Some(w) => {
                            if !v.is(&w) && !self.eq(&v, &w)? {
                                return Ok(Some(false));
                            }
                        }
                        None => return Ok(Some(false)),
                    }
                }
                true
            }
            (Value::Set(_) | Value::FrozenSet(_), Value::Set(_) | Value::FrozenSet(_)) => {
                let xa = crate::builtins::set_items(a);
                let yb = crate::builtins::set_items(b);
                if xa.len() != yb.len() {
                    return Ok(Some(false));
                }
                for it in xa {
                    if !self.contains(b, &it)? {
                        return Ok(Some(false));
                    }
                }
                true
            }
            (Value::Range(x), Value::Range(y)) => {
                let (lx, ly) = (x.len(), y.len());
                lx == ly && (lx == 0 || (x.start == y.start && (lx == 1 || x.step == y.step)))
            }
            (Value::ByteArray(x), Value::ByteArray(y)) => *x.borrow() == *y.borrow(),
            (Value::ByteArray(x), Value::Bytes(y)) | (Value::Bytes(y), Value::ByteArray(x)) => {
                *x.borrow() == **y
            }
            (Value::Complex(ar, ai), Value::Complex(br, bi)) => ar == br && ai == bi,
            (Value::Complex(ar, ai), other) | (other, Value::Complex(ar, ai)) => {
                match self.as_num(other) {
                    Some(n) => *ai == 0.0 && self.num_to_f64(&n).map(|f| f == *ar).unwrap_or(false),
                    None => false,
                }
            }
            (Value::DictView(x), Value::DictView(y))
                if x.1 == ViewKind::Keys && y.1 == ViewKind::Keys =>
            {
                let ka = x.0.borrow().keys();
                if ka.len() != y.0.borrow().len() {
                    return Ok(Some(false));
                }
                for k in ka {
                    if self.dict_get(&y.0, &k)?.is_none() {
                        return Ok(Some(false));
                    }
                }
                true
            }
            (Value::Slice(x), Value::Slice(y)) => self.seq_eq(&x[..], &y[..])?,
            _ => a.is(b),
        }))
    }

    pub fn rich_compare(&mut self, a: &Value, b: &Value, op: CmpOp) -> PyResult<Value> {
        let (name, rname) = match op {
            CmpOp::Eq => ("__eq__", "__eq__"),
            CmpOp::NotEq => ("__ne__", "__ne__"),
            CmpOp::Lt => ("__lt__", "__gt__"),
            CmpOp::LtE => ("__le__", "__ge__"),
            CmpOp::Gt => ("__gt__", "__lt__"),
            CmpOp::GtE => ("__ge__", "__le__"),
            _ => unreachable!(),
        };
        let a_inst = matches!(a, Value::Instance(_) | Value::Native(_));
        let b_inst = matches!(b, Value::Instance(_) | Value::Native(_));
        if !a_inst && !b_inst {
            match op {
                CmpOp::Eq => {
                    if let Some(r) = self.native_eq(a, b)? {
                        return Ok(Value::Bool(r));
                    }
                }
                CmpOp::NotEq => {
                    if let Some(r) = self.native_eq(a, b)? {
                        return Ok(Value::Bool(!r));
                    }
                }
                _ => {
                    if let Some(o) = self.native_order(a, b)? {
                        return Ok(Value::Bool(match op {
                            CmpOp::Lt => o == Ordering::Less,
                            CmpOp::LtE => o != Ordering::Greater,
                            CmpOp::Gt => o == Ordering::Greater,
                            _ => o != Ordering::Less,
                        }));
                    }
                    if let Some(r) = crate::builtins::set_compare(self, a, b, op)? {
                        return Ok(Value::Bool(r));
                    }
                }
            }
        }
        let ta = self.type_of(a);
        let tb = self.type_of(b);
        let b_first = b_inst && !Rc::ptr_eq(&ta, &tb) && tb.is_subclass(&ta);
        if b_first {
            if let Some(r) = self.call_special(b, rname, vec![a.clone()])? {
                if !matches!(r, Value::NotImplemented) {
                    return Ok(r);
                }
            }
        }
        if let Some(r) = self.call_special(a, name, vec![b.clone()])? {
            if !matches!(r, Value::NotImplemented) {
                return Ok(r);
            }
        }
        if !b_first {
            if let Some(r) = self.call_special(b, rname, vec![a.clone()])? {
                if !matches!(r, Value::NotImplemented) {
                    return Ok(r);
                }
            }
        }
        match op {
            CmpOp::Eq => Ok(Value::Bool(a.is(b))),
            CmpOp::NotEq => Ok(Value::Bool(!a.is(b))),
            _ => Err(type_err(format!(
                "'{}' not supported between instances of '{}' and '{}'",
                op.symbol(),
                self.type_name(a),
                self.type_name(b)
            ))),
        }
    }

    /// Ordering for builtin types; None if not comparable natively.
    pub fn native_order(&mut self, a: &Value, b: &Value) -> PyResult<Option<Ordering>> {
        if let (Value::Int(x), Value::Int(y)) = (a, b) {
            return Ok(Some(x.cmp(y)));
        }
        if let (Value::Str(x), Value::Str(y)) = (a, b) {
            return Ok(Some(x.s.cmp(&y.s)));
        }
        if matches!(a, Value::Complex(..)) || matches!(b, Value::Complex(..)) {
            return Ok(None);
        }
        if let (Some(x), Some(y)) = (self.as_num(a), self.as_num(b)) {
            if matches!(a, Value::Instance(_)) || matches!(b, Value::Instance(_)) {
                return Ok(None);
            }
            return Ok(Some(num_cmp(&x, &y)));
        }
        match (a, b) {
            (Value::Bytes(x), Value::Bytes(y)) => Ok(Some(x.cmp(y))),
            (Value::List(x), Value::List(y)) => {
                let (x, y) = (x.borrow().clone(), y.borrow().clone());
                self.seq_order(&x, &y)
            }
            (Value::Tuple(x), Value::Tuple(y)) => {
                let (x, y) = (x.clone(), y.clone());
                self.seq_order(&x, &y)
            }
            _ => Ok(None),
        }
    }
    fn seq_order(&mut self, x: &[Value], y: &[Value]) -> PyResult<Option<Ordering>> {
        for (p, q) in x.iter().zip(y.iter()) {
            if p.is(q) || self.eq(p, q)? {
                continue;
            }
            let lt = self.rich_compare(p, q, CmpOp::Lt)?;
            return Ok(Some(if self.truthy(&lt)? {
                Ordering::Less
            } else {
                Ordering::Greater
            }));
        }
        Ok(Some(x.len().cmp(&y.len())))
    }

    /// Total order used by sort(): `a < b`.
    pub fn less_than(&mut self, a: &Value, b: &Value) -> PyResult<bool> {
        if let (Value::Int(x), Value::Int(y)) = (a, b) {
            return Ok(x < y);
        }
        if let (Value::Str(x), Value::Str(y)) = (a, b) {
            return Ok(x.s < y.s);
        }
        let r = self.rich_compare(a, b, CmpOp::Lt)?;
        self.truthy(&r)
    }

    pub fn compare_op(&mut self, a: &Value, b: &Value, op: CmpOp) -> PyResult<Value> {
        match op {
            CmpOp::Is => Ok(Value::Bool(a.is(b))),
            CmpOp::IsNot => Ok(Value::Bool(!a.is(b))),
            CmpOp::In => Ok(Value::Bool(self.contains(b, a)?)),
            CmpOp::NotIn => Ok(Value::Bool(!self.contains(b, a)?)),
            _ => self.rich_compare(a, b, op),
        }
    }

    // ------------------------------------------------------------------
    // Hashing
    // ------------------------------------------------------------------

    pub fn hash(&mut self, v: &Value) -> PyResult<i64> {
        Ok(match v {
            Value::None => 0x9f7ee0 >> 4,
            Value::Bool(b) => *b as i64,
            Value::Int(i) => hash_i64(*i),
            Value::Big(b) => hash_big(b),
            Value::Float(f) => hash_f64(*f),
            Value::Complex(r, i) => {
                let h = hash_f64(*r).wrapping_add(hash_f64(*i).wrapping_mul(1000003));
                if h == -1 {
                    -2
                } else {
                    h
                }
            }
            Value::Str(s) => s.hash(),
            Value::Bytes(b) => {
                let s = PyStr::new(b.iter().map(|&c| c as char).collect());
                s.hash()
            }
            Value::Tuple(t) => {
                let mut hs = Vec::with_capacity(t.len());
                for x in t.iter() {
                    hs.push(self.hash(x)?);
                }
                hash_tuple(hs.into_iter())
            }
            Value::FrozenSet(s) => {
                // CPython's frozenset hash (order independent).
                let mut h: u64 = 0;
                for (_, eh) in s.items_hashed() {
                    let x = eh as u64;
                    h ^= ((x ^ (x << 16)) ^ 89869747).wrapping_mul(3644798167);
                }
                h ^= (s.len() as u64 + 1).wrapping_mul(1927868237);
                h ^= (h >> 11) ^ (h >> 25);
                h = h.wrapping_mul(69069).wrapping_add(907133923);
                let h = h as i64;
                if h == -1 {
                    590923713
                } else {
                    h
                }
            }
            Value::Range(r) => {
                hash_tuple([hash_i64(r.len()), hash_i64(r.start), hash_i64(r.step)].into_iter())
            }
            Value::List(_)
            | Value::Dict(_)
            | Value::Set(_)
            | Value::ByteArray(_)
            | Value::Slice(_)
            | Value::DictView(_) => {
                return Err(type_err(format!(
                    "unhashable type: '{}'",
                    self.type_name(v)
                )))
            }
            Value::Instance(i) => {
                let cls = i.class();
                match cls.lookup("__hash__") {
                    Some(Value::None) => {
                        return Err(type_err(format!("unhashable type: '{}'", cls.name())))
                    }
                    Some(h @ Value::Func(_)) => {
                        let r = self.call(&h, vec![v.clone()])?;
                        match r {
                            Value::Int(i) => {
                                if i == -1 {
                                    -2
                                } else {
                                    i
                                }
                            }
                            Value::Big(b) => hash_big(&b),
                            Value::Bool(b) => b as i64,
                            _ => return Err(type_err("__hash__ method should return an integer")),
                        }
                    }
                    _ => {
                        if let NativeData::Base(b) = &*i.native.borrow() {
                            let b = b.clone();
                            return self.hash(&b);
                        }
                        (self.object_id(v) >> 4) as i64
                    }
                }
            }
            _ => (self.object_id(v) >> 4) as i64,
        })
    }

    // ------------------------------------------------------------------
    // Dict and set helpers (user-level equality resolved outside borrows)
    // ------------------------------------------------------------------

    fn dict_find(&mut self, d: &Ref<Dict>, key: &Value, h: i64) -> PyResult<Option<usize>> {
        if let Ok(r) = d.borrow().find_fast(h, key) {
            return Ok(r);
        }
        let cands = d.borrow().candidates(h);
        for (i, k) in cands {
            if k.is(key) || self.eq(&k, key)? {
                return Ok(Some(i));
            }
        }
        Ok(None)
    }
    pub fn dict_get(&mut self, d: &Ref<Dict>, key: &Value) -> PyResult<Option<Value>> {
        let h = self.hash(key)?;
        let i = self.dict_find(d, key, h)?;
        Ok(i.and_then(|i| d.borrow().entry_value(i).cloned()))
    }
    pub fn dict_set(&mut self, d: &Ref<Dict>, key: Value, v: Value) -> PyResult<()> {
        let h = self.hash(&key)?;
        match self.dict_find(d, &key, h)? {
            Some(i) => d.borrow_mut().set_at(i, v),
            None => d.borrow_mut().push_new(key, v, h),
        }
        Ok(())
    }
    pub fn dict_del(&mut self, d: &Ref<Dict>, key: &Value) -> PyResult<Option<Value>> {
        let h = self.hash(key)?;
        match self.dict_find(d, key, h)? {
            Some(i) => Ok(d.borrow_mut().remove_at(i).map(|(_, v)| v)),
            None => Ok(None),
        }
    }
    fn set_probe(&mut self, s: &SetData, key: &Value, h: i64) -> PyResult<Probe> {
        let mut skip = vec![];
        loop {
            match s.probe(key, h, &skip) {
                Probe::Check(slot, k) => {
                    if k.is(key) || self.eq(&k, key)? {
                        return Ok(Probe::Found(slot));
                    }
                    skip.push(slot);
                }
                other => return Ok(other),
            }
        }
    }
    pub fn set_add(&mut self, s: &Ref<SetData>, key: Value) -> PyResult<()> {
        let h = self.hash(&key)?;
        let snapshot = s.borrow().clone();
        match self.set_probe(&snapshot, &key, h)? {
            Probe::Found(_) => {}
            Probe::Vacant(slot, empty) => s.borrow_mut().insert_at(slot, empty, key, h),
            Probe::Check(..) => unreachable!(),
        }
        Ok(())
    }
    pub fn set_add_data(&mut self, s: &mut SetData, key: Value) -> PyResult<()> {
        let h = self.hash(&key)?;
        match self.set_probe(s, &key, h)? {
            Probe::Found(_) => {}
            Probe::Vacant(slot, empty) => s.insert_at(slot, empty, key, h),
            Probe::Check(..) => unreachable!(),
        }
        Ok(())
    }
    pub fn set_contains_data(&mut self, s: &SetData, key: &Value) -> PyResult<bool> {
        let h = match self.hash(key) {
            Ok(h) => h,
            Err(e) => {
                // `[] in {1}` is a TypeError in CPython too.
                return Err(e);
            }
        };
        Ok(matches!(self.set_probe(s, key, h)?, Probe::Found(_)))
    }
    pub fn set_discard(&mut self, s: &Ref<SetData>, key: &Value) -> PyResult<bool> {
        let h = self.hash(key)?;
        let snapshot = s.borrow().clone();
        match self.set_probe(&snapshot, key, h)? {
            Probe::Found(slot) => {
                s.borrow_mut().remove_slot(slot);
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// Items of any mapping (dict, dict subclass, or object with keys()).
    pub fn mapping_items(&mut self, v: &Value) -> PyResult<Vec<(Value, Value)>> {
        match v {
            Value::Dict(d) => Ok(d.borrow().items()),
            _ => {
                if let Value::Instance(i) = v {
                    if let NativeData::Base(Value::Dict(d)) = &*i.native.borrow() {
                        if i.class()
                            .lookup("keys")
                            .is_some_and(|k| matches!(k, Value::Builtin(_)))
                        {
                            return Ok(d.borrow().items());
                        }
                    }
                }
                let keys = self.getattr_str(v, "keys")?;
                let keys = self.call(&keys, vec![])?;
                let keys = self.iterate(&keys)?;
                let mut out = Vec::with_capacity(keys.len());
                for k in keys {
                    let val = self.getitem(v, &k)?;
                    out.push((k, val));
                }
                Ok(out)
            }
        }
    }

    // ------------------------------------------------------------------
    // Iteration
    // ------------------------------------------------------------------

    pub fn get_iter(&mut self, v: &Value) -> PyResult<Value> {
        let it = match v {
            Value::List(_) | Value::Tuple(_) | Value::Bytes(_) | Value::ByteArray(_) => {
                IterObj::Seq {
                    seq: v.clone(),
                    idx: 0,
                }
            }
            Value::Str(s) => IterObj::Str {
                s: s.clone(),
                byte: 0,
            },
            Value::Range(r) => IterObj::Range {
                cur: r.start,
                step: r.step,
                remaining: r.len(),
            },
            Value::Dict(d) => IterObj::Dict {
                dict: d.clone(),
                slot: 0,
                kind: ViewKind::Keys,
                version_len: d.borrow().len(),
            },
            Value::DictView(dv) => IterObj::Dict {
                dict: dv.0.clone(),
                slot: 0,
                kind: dv.1,
                version_len: dv.0.borrow().len(),
            },
            Value::Set(s) => IterObj::List {
                items: s.borrow().items(),
                idx: 0,
            },
            Value::FrozenSet(s) => IterObj::List {
                items: s.items(),
                idx: 0,
            },
            Value::Iter(_) | Value::Gen(_) => return Ok(v.clone()),
            Value::File(_) => return Ok(v.clone()),
            Value::Native(n) => {
                if let NativeKind::Deque(d, _) = &*n.data.borrow() {
                    IterObj::List {
                        items: d.iter().cloned().collect(),
                        idx: 0,
                    }
                } else {
                    return Err(type_err(format!(
                        "'{}' object is not iterable",
                        self.type_name(v)
                    )));
                }
            }
            _ => {
                if let Some(r) = self.call_special(v, "__iter__", vec![])? {
                    let is_iter = matches!(r, Value::Iter(_) | Value::Gen(_) | Value::File(_))
                        || self.lookup_special(&r, "__next__").is_some();
                    if !is_iter {
                        return Err(type_err(format!(
                            "iter() returned non-iterator of type '{}'",
                            self.type_name(&r)
                        )));
                    }
                    return Ok(r);
                }
                if let Value::Instance(_) = v {
                    if self.lookup_special(v, "__getitem__").is_some() {
                        return Ok(Value::Iter(new_ref(IterObj::GetItem {
                            obj: v.clone(),
                            idx: 0,
                        })));
                    }
                }
                if let Value::Class(c) = v {
                    if let Some(meta) = c.metaclass.borrow().clone() {
                        if let Some(it) = meta.lookup("__iter__") {
                            return self.call(&it, vec![v.clone()]);
                        }
                    }
                }
                return Err(type_err(format!(
                    "'{}' object is not iterable",
                    self.type_name(v)
                )));
            }
        };
        Ok(Value::Iter(new_ref(it)))
    }

    /// Advances an iterator: `Ok(None)` when exhausted.
    pub fn next(&mut self, it: &Value) -> PyResult<Option<Value>> {
        match it {
            Value::Iter(cell) => self.iter_next(cell),
            Value::Gen(g) => match self.gen_resume(g, Value::None, None)? {
                GenResult::Yielded(v) => Ok(Some(v)),
                GenResult::Returned(_) => Ok(None),
            },
            Value::File(f) => {
                let line = crate::builtins::file_readline(self, f, -1)?;
                let empty = match &line {
                    Value::Str(s) => s.s.is_empty(),
                    Value::Bytes(b) => b.is_empty(),
                    _ => true,
                };
                if empty {
                    Ok(None)
                } else {
                    Ok(Some(line))
                }
            }
            _ => {
                let m = self.lookup_special(it, "__next__").ok_or_else(|| {
                    type_err(format!(
                        "'{}' object is not an iterator",
                        self.type_name(it)
                    ))
                })?;
                match self.call(&m, vec![it.clone()]) {
                    Ok(v) => Ok(Some(v)),
                    Err(e) if self.err_matches(&e, "StopIteration") => Ok(None),
                    Err(e) => Err(e),
                }
            }
        }
    }

    fn iter_next(&mut self, cell: &Ref<IterObj>) -> PyResult<Option<Value>> {
        let mut st = cell.borrow_mut();
        match &mut *st {
            IterObj::Done => Ok(None),
            IterObj::Seq { seq, idx } => {
                let v = match seq {
                    Value::List(l) => l.borrow().get(*idx).cloned(),
                    Value::Tuple(t) => t.get(*idx).cloned(),
                    Value::Bytes(b) => b.get(*idx).map(|c| Value::Int(*c as i64)),
                    Value::ByteArray(b) => b.borrow().get(*idx).map(|c| Value::Int(*c as i64)),
                    _ => None,
                };
                match v {
                    Some(v) => {
                        *idx += 1;
                        Ok(Some(v))
                    }
                    None => {
                        *st = IterObj::Done;
                        Ok(None)
                    }
                }
            }
            IterObj::Str { s, byte } => {
                let rest = &s.s[*byte..];
                match rest.chars().next() {
                    Some(c) => {
                        *byte += c.len_utf8();
                        Ok(Some(Value::string(c.to_string())))
                    }
                    None => Ok(None),
                }
            }
            IterObj::Range {
                cur,
                step,
                remaining,
            } => {
                if *remaining <= 0 {
                    return Ok(None);
                }
                let v = *cur;
                *cur = cur.wrapping_add(*step);
                *remaining -= 1;
                Ok(Some(Value::Int(v)))
            }
            IterObj::List { items, idx } => {
                let v = items.get(*idx).cloned();
                *idx += 1;
                Ok(v)
            }
            IterObj::Dict {
                dict,
                slot,
                kind,
                version_len,
            } => {
                let d = dict.borrow();
                if d.len() != *version_len {
                    *version_len = usize::MAX;
                    drop(d);
                    return Err(err(
                        "RuntimeError",
                        "dictionary changed size during iteration",
                    ));
                }
                while *slot < d.slots() {
                    let s = *slot;
                    *slot += 1;
                    if let Some((k, v, _)) = d.entry(s) {
                        return Ok(Some(match kind {
                            ViewKind::Keys => k.clone(),
                            ViewKind::Values => v.clone(),
                            ViewKind::Items => Value::tuple(vec![k.clone(), v.clone()]),
                        }));
                    }
                }
                Ok(None)
            }
            IterObj::Reversed { seq, idx } => {
                if *idx < 0 {
                    return Ok(None);
                }
                let i = *idx as usize;
                *idx -= 1;
                let seq = seq.clone();
                drop(st);
                match &seq {
                    Value::List(l) => Ok(l.borrow().get(i).cloned()),
                    Value::Tuple(t) => Ok(t.get(i).cloned()),
                    Value::Str(s) => Ok(s.char_at(i).map(|c| Value::string(c.to_string()))),
                    Value::Range(r) => Ok(Some(Value::Int(r.get(i as i64)))),
                    _ => self.getitem(&seq, &Value::Int(i as i64)).map(Some),
                }
            }
            IterObj::Enumerate { it, count } => {
                let (inner, c) = (it.clone(), count.clone());
                drop(st);
                match self.next(&inner)? {
                    Some(v) => {
                        let next = self.binary_op(&c, &Value::Int(1), BinOp::Add)?;
                        if let IterObj::Enumerate { count, .. } = &mut *cell.borrow_mut() {
                            *count = next;
                        }
                        Ok(Some(Value::tuple(vec![c, v])))
                    }
                    None => Ok(None),
                }
            }
            IterObj::Zip { its, strict } => {
                let (its, strict) = (its.clone(), *strict);
                drop(st);
                if its.is_empty() {
                    return Ok(None);
                }
                let mut out = Vec::with_capacity(its.len());
                for (i, it) in its.iter().enumerate() {
                    match self.next(it)? {
                        Some(v) => out.push(v),
                        None => {
                            if strict {
                                if i > 0 {
                                    return Err(value_err(format!(
                                        "zip() argument {} is shorter than argument{} 1{}",
                                        i + 1,
                                        if i > 1 { "s" } else { "" },
                                        if i > 1 {
                                            format!("-{i}")
                                        } else {
                                            String::new()
                                        }
                                    )));
                                }
                                for (j, other) in its.iter().enumerate().skip(1) {
                                    if self.next(other)?.is_some() {
                                        return Err(value_err(format!(
                                            "zip() argument {} is longer than argument{} 1{}",
                                            j + 1,
                                            if j > 1 { "s" } else { "" },
                                            if j > 1 {
                                                format!("-{j}")
                                            } else {
                                                String::new()
                                            }
                                        )));
                                    }
                                }
                            }
                            *cell.borrow_mut() = IterObj::Done;
                            return Ok(None);
                        }
                    }
                }
                Ok(Some(Value::tuple(out)))
            }
            IterObj::Map { func, its } => {
                let (func, its) = (func.clone(), its.clone());
                drop(st);
                let mut args = Vec::with_capacity(its.len());
                for it in &its {
                    match self.next(it)? {
                        Some(v) => args.push(v),
                        None => return Ok(None),
                    }
                }
                self.call(&func, args).map(Some)
            }
            IterObj::Filter { func, it } => {
                let (func, it) = (func.clone(), it.clone());
                drop(st);
                loop {
                    let Some(v) = self.next(&it)? else {
                        return Ok(None);
                    };
                    let keep = if func.is_none() {
                        self.truthy(&v)?
                    } else {
                        let r = self.call(&func, vec![v.clone()])?;
                        self.truthy(&r)?
                    };
                    if keep {
                        return Ok(Some(v));
                    }
                }
            }
            IterObj::Callable { func, sentinel } => {
                let (func, sentinel) = (func.clone(), sentinel.clone());
                drop(st);
                let v = self.call(&func, vec![])?;
                if self.eq(&v, &sentinel)? {
                    *cell.borrow_mut() = IterObj::Done;
                    return Ok(None);
                }
                Ok(Some(v))
            }
            IterObj::GetItem { obj, idx } => {
                let (obj, i) = (obj.clone(), *idx);
                *idx += 1;
                drop(st);
                match self.getitem(&obj, &Value::Int(i)) {
                    Ok(v) => Ok(Some(v)),
                    Err(e)
                        if self.err_matches(&e, "IndexError")
                            || self.err_matches(&e, "StopIteration") =>
                    {
                        *cell.borrow_mut() = IterObj::Done;
                        Ok(None)
                    }
                    Err(e) => Err(e),
                }
            }
        }
    }

    /// Collects any iterable into a vector.
    pub fn iterate(&mut self, v: &Value) -> PyResult<Vec<Value>> {
        match v {
            Value::List(l) => return Ok(l.borrow().clone()),
            Value::Tuple(t) => return Ok((**t).clone()),
            Value::Range(r) => {
                let n = r.len();
                if n > 50_000_000 {
                    return Err(err("MemoryError", ""));
                }
                return Ok((0..n).map(|i| Value::Int(r.get(i))).collect());
            }
            Value::Dict(d) => return Ok(d.borrow().keys()),
            Value::Set(s) => return Ok(s.borrow().items()),
            Value::FrozenSet(s) => return Ok(s.items()),
            _ => {}
        }
        let it = self.get_iter(v)?;
        let mut out = vec![];
        while let Some(x) = self.next(&it)? {
            out.push(x);
            if out.len() > 50_000_000 {
                return Err(err("MemoryError", ""));
            }
        }
        Ok(out)
    }

    // ------------------------------------------------------------------
    // Length, indexing, containment
    // ------------------------------------------------------------------

    pub fn len(&mut self, v: &Value) -> PyResult<usize> {
        Ok(match v {
            Value::Str(s) => s.nchars,
            Value::Bytes(b) => b.len(),
            Value::ByteArray(b) => b.borrow().len(),
            Value::Tuple(t) => t.len(),
            Value::List(l) => l.borrow().len(),
            Value::Dict(d) => d.borrow().len(),
            Value::Set(s) => s.borrow().len(),
            Value::FrozenSet(s) => s.len(),
            Value::Range(r) => r.len() as usize,
            Value::DictView(dv) => dv.0.borrow().len(),
            Value::Native(n) => match &*n.data.borrow() {
                NativeKind::Deque(d, _) => d.len(),
                _ => {
                    return Err(type_err(format!(
                        "object of type '{}' has no len()",
                        n.class.name()
                    )))
                }
            },
            _ => {
                if let Some(r) = self.call_special(v, "__len__", vec![])? {
                    let n = self.index_of(&r)?;
                    if n < 0 {
                        return Err(value_err("__len__() should return >= 0"));
                    }
                    return Ok(n as usize);
                }
                return Err(type_err(format!(
                    "object of type '{}' has no len()",
                    self.type_name(v)
                )));
            }
        })
    }

    /// `__index__` conversion to i64.
    pub fn index_of(&mut self, v: &Value) -> PyResult<i64> {
        match v {
            Value::Int(i) => Ok(*i),
            Value::Bool(b) => Ok(*b as i64),
            Value::Big(_) => Err(err(
                "IndexError",
                "cannot fit 'int' into an index-sized integer",
            )),
            _ => {
                if let Some(r) = self.call_special(v, "__index__", vec![])? {
                    return self.index_of(&r);
                }
                Err(type_err(format!(
                    "'{}' object cannot be interpreted as an integer",
                    self.type_name(v)
                )))
            }
        }
    }

    pub fn getitem(&mut self, obj: &Value, idx: &Value) -> PyResult<Value> {
        match obj {
            Value::Dict(d) => {
                return match self.dict_get(d, idx)? {
                    Some(v) => Ok(v),
                    None => Err(err_args("KeyError", vec![idx.clone()])),
                }
            }
            Value::Instance(_) => {
                if let Some(r) = self.call_special(obj, "__getitem__", vec![idx.clone()])? {
                    return Ok(r);
                }
                return Err(type_err(format!(
                    "'{}' object is not subscriptable",
                    self.type_name(obj)
                )));
            }
            Value::Class(c) => {
                if let Some(cg) = c.lookup("__class_getitem__") {
                    let f = match cg {
                        Value::ClassMethod(f) => (*f).clone(),
                        other => other,
                    };
                    return self.call(&f, vec![obj.clone(), idx.clone()]);
                }
                if let Some(meta) = c.metaclass.borrow().clone() {
                    if let Some(gi) = meta.lookup("__getitem__") {
                        return self.call(&gi, vec![obj.clone(), idx.clone()]);
                    }
                }
                if c.builtin {
                    // list[int], dict[str, int]: a generic alias; typing only.
                    return Ok(obj.clone());
                }
                return Err(type_err(format!(
                    "type '{}' is not subscriptable",
                    c.name()
                )));
            }
            _ => {}
        }
        crate::builtins::seq_getitem(self, obj, idx)
    }

    pub fn setitem(&mut self, obj: &Value, idx: &Value, v: Value) -> PyResult<()> {
        match obj {
            Value::Dict(d) => self.dict_set(d, idx.clone(), v),
            Value::List(_) | Value::ByteArray(_) => {
                crate::builtins::list_setitem(self, obj, idx, v)
            }
            Value::Instance(_) => {
                if self
                    .call_special(obj, "__setitem__", vec![idx.clone(), v])?
                    .is_some()
                {
                    return Ok(());
                }
                Err(type_err(format!(
                    "'{}' object does not support item assignment",
                    self.type_name(obj)
                )))
            }
            Value::Native(_) => crate::builtins::native_setitem(self, obj, idx, v),
            _ => Err(type_err(format!(
                "'{}' object does not support item assignment",
                self.type_name(obj)
            ))),
        }
    }

    pub fn delitem(&mut self, obj: &Value, idx: &Value) -> PyResult<()> {
        match obj {
            Value::Dict(d) => match self.dict_del(d, idx)? {
                Some(_) => Ok(()),
                None => Err(err_args("KeyError", vec![idx.clone()])),
            },
            Value::List(_) | Value::ByteArray(_) => crate::builtins::list_delitem(self, obj, idx),
            Value::Instance(_) => {
                if self
                    .call_special(obj, "__delitem__", vec![idx.clone()])?
                    .is_some()
                {
                    return Ok(());
                }
                Err(type_err(format!(
                    "'{}' object doesn't support item deletion",
                    self.type_name(obj)
                )))
            }
            _ => Err(type_err(format!(
                "'{}' object doesn't support item deletion",
                self.type_name(obj)
            ))),
        }
    }

    pub fn contains(&mut self, container: &Value, item: &Value) -> PyResult<bool> {
        match container {
            Value::Str(s) => match item {
                Value::Str(sub) => Ok(s.s.contains(&*sub.s)),
                _ => Err(type_err(format!(
                    "'in <string>' requires string as left operand, not {}",
                    self.type_name(item)
                ))),
            },
            Value::List(l) => {
                let items = l.borrow().clone();
                for x in items.iter() {
                    if x.is(item) || self.eq(x, item)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Value::Tuple(t) => {
                for x in t.iter() {
                    if x.is(item) || self.eq(x, item)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Value::Dict(d) => Ok(self.dict_get(d, item)?.is_some()),
            Value::DictView(dv) => match dv.1 {
                ViewKind::Keys => Ok(self.dict_get(&dv.0, item)?.is_some()),
                ViewKind::Values => {
                    let vals = dv.0.borrow().values();
                    for x in vals {
                        if self.eq(&x, item)? {
                            return Ok(true);
                        }
                    }
                    Ok(false)
                }
                ViewKind::Items => {
                    if let Value::Tuple(t) = item {
                        if t.len() == 2 {
                            if let Some(v) = self.dict_get(&dv.0, &t[0])? {
                                return self.eq(&v, &t[1]);
                            }
                        }
                    }
                    Ok(false)
                }
            },
            Value::Set(s) => {
                let snap = s.borrow().clone();
                self.set_contains_data(&snap, item)
            }
            Value::FrozenSet(s) => {
                let s = s.clone();
                self.set_contains_data(&s, item)
            }
            Value::Range(r) => {
                let i = match item {
                    Value::Int(i) => *i,
                    Value::Bool(b) => *b as i64,
                    Value::Float(f) if f.fract() == 0.0 => *f as i64,
                    _ => return Ok(false),
                };
                if r.step > 0 {
                    Ok(i >= r.start && i < r.stop && (i - r.start) % r.step == 0)
                } else {
                    Ok(i <= r.start && i > r.stop && (r.start - i) % (-r.step) == 0)
                }
            }
            Value::Bytes(b) => match item {
                Value::Int(i) => Ok(b.contains(&(*i as u8))),
                Value::Bytes(sub) => {
                    Ok(sub.is_empty() || b.windows(sub.len()).any(|w| w == &sub[..]))
                }
                _ => Err(type_err("a bytes-like object is required")),
            },
            _ => {
                if let Some(r) = self.call_special(container, "__contains__", vec![item.clone()])? {
                    return self.truthy(&r);
                }
                if let Value::Class(c) = container {
                    if let Some(meta) = c.metaclass.borrow().clone() {
                        if let Some(m) = meta.lookup("__contains__") {
                            let r = self.call(&m, vec![container.clone(), item.clone()])?;
                            return self.truthy(&r);
                        }
                    }
                }
                let it = self.get_iter(container).map_err(|_| {
                    type_err(format!(
                        "argument of type '{}' is not iterable",
                        self.type_name(container)
                    ))
                })?;
                while let Some(x) = self.next(&it)? {
                    if x.is(item) || self.eq(&x, item)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
        }
    }

    // ------------------------------------------------------------------
    // repr / str / format
    // ------------------------------------------------------------------

    pub fn repr(&mut self, v: &Value) -> PyResult<String> {
        Ok(match v {
            Value::Undefined => "<NULL>".into(),
            Value::None => "None".into(),
            Value::NotImplemented => "NotImplemented".into(),
            Value::Ellipsis => "Ellipsis".into(),
            Value::Bool(b) => if *b { "True" } else { "False" }.into(),
            Value::Int(i) => i.to_string(),
            Value::Big(b) => self.big_to_str(b)?,
            Value::Float(f) => crate::format::float_repr(*f),
            Value::Complex(r, i) => crate::format::complex_repr(*r, *i),
            Value::Str(s) => crate::format::str_repr(&s.s),
            Value::Bytes(b) => crate::format::bytes_repr(b),
            Value::ByteArray(b) => format!("bytearray({})", crate::format::bytes_repr(&b.borrow())),
            Value::Tuple(t) => {
                if t.is_empty() {
                    return Ok("()".into());
                }
                if !self.repr_enter(v) {
                    return Ok("(...)".into());
                }
                let t = t.clone();
                let r = self.join_reprs(&t);
                self.repr_leave();
                let r = r?;
                if t.len() == 1 {
                    format!("({r},)")
                } else {
                    format!("({r})")
                }
            }
            Value::List(l) => {
                if !self.repr_enter(v) {
                    return Ok("[...]".into());
                }
                let items = l.borrow().clone();
                let r = self.join_reprs(&items);
                self.repr_leave();
                format!("[{}]", r?)
            }
            Value::Dict(d) => {
                if !self.repr_enter(v) {
                    return Ok("{...}".into());
                }
                let r = self.dict_repr(d);
                self.repr_leave();
                r?
            }
            Value::Set(s) => {
                let items = s.borrow().items();
                if items.is_empty() {
                    return Ok("set()".into());
                }
                if !self.repr_enter(v) {
                    return Ok("{...}".into());
                }
                let r = self.join_reprs(&items);
                self.repr_leave();
                format!("{{{}}}", r?)
            }
            Value::FrozenSet(s) => {
                let items = s.items();
                if items.is_empty() {
                    return Ok("frozenset()".into());
                }
                format!("frozenset({{{}}})", self.join_reprs(&items)?)
            }
            Value::Range(r) => {
                if r.step == 1 {
                    format!("range({}, {})", r.start, r.stop)
                } else {
                    format!("range({}, {}, {})", r.start, r.stop, r.step)
                }
            }
            Value::Slice(s) => {
                let s = s.clone();
                format!(
                    "slice({}, {}, {})",
                    self.repr(&s[0])?,
                    self.repr(&s[1])?,
                    self.repr(&s[2])?
                )
            }
            Value::Func(f) => format!("<function {} at {}>", f.qualname.borrow(), self.addr(v)),
            Value::Builtin(b) => match b.owner {
                Some(o) => format!(
                    "<method '{}' of '{}' objects>",
                    b.name.rsplit('.').next().unwrap_or(&b.name),
                    o
                ),
                None => format!(
                    "<built-in function {}>",
                    b.name.rsplit('.').next().unwrap_or(&b.name)
                ),
            },
            Value::Method(m) => {
                let m = m.clone();
                match &m.1 {
                    Value::Builtin(b) => format!(
                        "<built-in method {} of {} object at {}>",
                        b.name.rsplit('.').next().unwrap_or(&b.name),
                        self.type_name(&m.0),
                        self.addr(&m.0)
                    ),
                    Value::Func(f) => {
                        let r = self.repr(&m.0)?;
                        format!("<bound method {} of {}>", f.qualname.borrow(), r)
                    }
                    _ => "<bound method>".into(),
                }
            }
            Value::Class(c) => {
                if let Some(meta) = c.metaclass.borrow().clone() {
                    if let Some(r) = meta.lookup("__repr__") {
                        if matches!(r, Value::Func(_)) {
                            let s = self.call(&r, vec![v.clone()])?;
                            return self.str_of(&s);
                        }
                    }
                }
                let module = c
                    .dict
                    .borrow()
                    .get_str("__module__")
                    .and_then(|m| m.as_pystr().map(|s| s.s.clone()));
                match module {
                    Some(m) if m != "builtins" => {
                        format!("<class '{}.{}'>", m, c.qualname.borrow())
                    }
                    _ => format!("<class '{}'>", c.qualname.borrow()),
                }
            }
            Value::Instance(_) => {
                if let Some(r) = self.call_special(v, "__repr__", vec![])? {
                    return match r {
                        Value::Str(s) => Ok(s.s.clone()),
                        _ => Err(type_err(format!(
                            "__repr__ returned non-string (type {})",
                            self.type_name(&r)
                        ))),
                    };
                }
                self.default_repr(v)
            }
            Value::Module(m) => {
                let file = m.dict.borrow().get_str("__file__");
                match file {
                    Some(Value::Str(f)) => format!("<module '{}' from '{}'>", m.name, f.s),
                    _ => format!("<module '{}' (built-in)>", m.name),
                }
            }
            Value::Gen(g) => {
                let g = g.borrow();
                let kind = if g.is_coroutine {
                    "coroutine"
                } else {
                    "generator"
                };
                let q = g.qualname.clone();
                drop(g);
                format!("<{kind} object {q} at {}>", self.addr(v))
            }
            Value::Iter(_) => format!("<{} object at {}>", self.type_name(v), self.addr(v)),
            Value::DictView(dv) => {
                let (d, k) = (dv.0.clone(), dv.1);
                let name = self.type_name(v);
                let items: Vec<Value> = match k {
                    ViewKind::Keys => d.borrow().keys(),
                    ViewKind::Values => d.borrow().values(),
                    ViewKind::Items => d
                        .borrow()
                        .items()
                        .into_iter()
                        .map(|(a, b)| Value::tuple(vec![a, b]))
                        .collect(),
                };
                format!("{name}([{}])", self.join_reprs(&items)?)
            }
            Value::Property(_) => format!("<property object at {}>", self.addr(v)),
            Value::StaticMethod(_) => format!("<staticmethod object at {}>", self.addr(v)),
            Value::ClassMethod(_) => format!("<classmethod object at {}>", self.addr(v)),
            Value::Super(s) => {
                let tn = self.type_name(&s.1);
                format!("<super: <class '{}'>, <{} object>>", s.0.name(), tn)
            }
            Value::File(f) => {
                let f = f.borrow();
                if f.binary {
                    format!(
                        "<_io.BufferedReader name={}>",
                        crate::format::str_repr(&f.path)
                    )
                } else {
                    format!(
                        "<_io.TextIOWrapper name={} mode='{}' encoding='utf-8'>",
                        match &f.name {
                            Value::Str(s) => crate::format::str_repr(&s.s),
                            Value::Int(i) => i.to_string(),
                            _ => "?".into(),
                        },
                        f.mode
                    )
                }
            }
            Value::Cell(_) => format!("<cell at {}>", self.addr(v)),
            Value::Code(c) => format!(
                "<code object {} at {}, file \"{}\", line {}>",
                c.name,
                self.addr(v),
                c.filename,
                c.firstlineno
            ),
            Value::Native(_) => crate::builtins::native_repr(self, v)?,
        })
    }

    pub fn default_repr(&mut self, v: &Value) -> String {
        let cls = self.type_of(v);
        let module = cls
            .dict
            .borrow()
            .get_str("__module__")
            .and_then(|m| m.as_pystr().map(|s| s.s.clone()))
            .unwrap_or_else(|| "builtins".into());
        let q = cls.qualname.borrow().clone();
        if module == "builtins" {
            format!("<{} object at {}>", q, self.addr(v))
        } else {
            format!("<{}.{} object at {}>", module, q, self.addr(v))
        }
    }

    pub fn addr(&self, v: &Value) -> String {
        // Deterministic pseudo-addresses: never leak host allocator state.
        format!("{:#x}", self.object_id(v))
    }

    fn repr_enter(&mut self, v: &Value) -> bool {
        let id = v.id();
        if self.repr_guard.contains(&id) {
            return false;
        }
        self.repr_guard.push(id);
        true
    }
    fn repr_leave(&mut self) {
        self.repr_guard.pop();
    }
    fn join_reprs(&mut self, items: &[Value]) -> PyResult<String> {
        let mut out = String::new();
        for (i, x) in items.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(&self.repr(x)?);
        }
        Ok(out)
    }
    fn dict_repr(&mut self, d: &Ref<Dict>) -> PyResult<String> {
        let items = d.borrow().items();
        let mut out = String::from("{");
        for (i, (k, v)) in items.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(&self.repr(k)?);
            out.push_str(": ");
            out.push_str(&self.repr(v)?);
        }
        out.push('}');
        Ok(out)
    }

    pub fn big_to_str(&self, b: &BigInt) -> PyResult<String> {
        // CPython refuses to print ints beyond sys.get_int_max_str_digits().
        if self.int_max_str_digits > 0
            && b.bit_length() as f64 * std::f64::consts::LOG10_2
                > self.int_max_str_digits as f64 + 1.0
        {
            let s = b.to_str_radix(10);
            let digits = s.trim_start_matches('-').len();
            if digits > self.int_max_str_digits {
                return Err(value_err(format!(
                    "Exceeds the limit ({}) for integer string conversion; use sys.set_int_max_str_digits() to increase the limit",
                    self.int_max_str_digits
                )));
            }
            return Ok(s);
        }
        Ok(b.to_str_radix(10))
    }

    pub fn str_of(&mut self, v: &Value) -> PyResult<String> {
        match v {
            Value::Str(s) => Ok(s.s.clone()),
            Value::Instance(i) => {
                let cls = i.class();
                if let Some(m) = cls.lookup("__str__") {
                    let is_default =
                        matches!(&m, Value::Builtin(b) if &*b.name == "object.__str__");
                    if !is_default {
                        let r = self.call(&m, vec![v.clone()])?;
                        return match r {
                            Value::Str(s) => Ok(s.s.clone()),
                            _ => Err(type_err(format!(
                                "__str__ returned non-string (type {})",
                                self.type_name(&r)
                            ))),
                        };
                    }
                }
                if cls.kind == Kind::Exception
                    && cls
                        .lookup("__repr__")
                        .is_some_and(|r| matches!(r, Value::Builtin(_)))
                {
                    return crate::builtins::exc_str(self, v);
                }
                self.repr(v)
            }
            _ => self.repr(v),
        }
    }

    pub fn format(&mut self, v: &Value, spec: &str) -> PyResult<String> {
        if let Value::Instance(i) = v {
            let cls = i.class();
            if let Some(m) = cls.lookup("__format__") {
                if matches!(m, Value::Func(_)) {
                    let r = self.call(&m, vec![v.clone(), Value::str(spec)])?;
                    return self.str_of(&r);
                }
            }
            if let NativeData::Base(b) = &*i.native.borrow() {
                let b = b.clone();
                if !matches!(b, Value::Str(_)) || spec.is_empty() {
                    return crate::format::format_value(self, &b, spec);
                }
            }
            if spec.is_empty() {
                return self.str_of(v);
            }
            return Err(type_err(format!(
                "unsupported format string passed to {}.__format__",
                cls.name()
            )));
        }
        crate::format::format_value(self, v, spec)
    }

    // ------------------------------------------------------------------
    // Classes
    // ------------------------------------------------------------------

    pub fn call_class(
        &mut self,
        cls: &Rc<Class>,
        args: Vec<Value>,
        kwargs: Vec<(Rc<str>, Value)>,
    ) -> PyResult<Value> {
        // type(x)
        if Rc::ptr_eq(cls, &self.t.type_) && args.len() == 1 && kwargs.is_empty() {
            return Ok(Value::Class(self.type_of(&args[0])));
        }
        // A metaclass with __call__.
        if let Some(meta) = cls.metaclass.borrow().clone() {
            if let Some(call) = meta.lookup("__call__") {
                if matches!(call, Value::Func(_)) {
                    let mut a = vec![Value::Class(cls.clone())];
                    a.extend(args);
                    return self.call_kw(&call, a, kwargs);
                }
            }
        }
        if !cls.abstract_methods.borrow().is_empty() {
            let mut names: Vec<String> = cls
                .abstract_methods
                .borrow()
                .iter()
                .map(|s| format!("'{s}'"))
                .collect();
            names.sort();
            let n = names.len();
            return Err(type_err(format!(
                "Can't instantiate abstract class {} without an implementation for abstract method{} {}",
                cls.name(),
                if n == 1 { "" } else { "s" },
                names.join(", ")
            )));
        }
        let new = cls.lookup("__new__").unwrap_or(Value::None);
        if cls.builtin && !matches!(&new, Value::Builtin(b) if &*b.name == "object.__new__") {
            let new_f = match &new {
                Value::StaticMethod(f) => (**f).clone(),
                other => other.clone(),
            };
            let mut a = Vec::with_capacity(args.len() + 1);
            a.push(Value::Class(cls.clone()));
            a.extend(args);
            return self.call_kw(&new_f, a, kwargs);
        }
        let init = cls.lookup("__init__");
        let default_new = matches!(&new, Value::Builtin(b) if &*b.name == "object.__new__");
        let default_init =
            matches!(&init, Some(Value::Builtin(b)) if &*b.name == "object.__init__");
        if default_new && default_init && (!args.is_empty() || !kwargs.is_empty()) {
            return Err(type_err(format!("{}() takes no arguments", cls.name())));
        }
        let obj = if default_new {
            self.new_instance(cls)
        } else {
            let new_f = match &new {
                Value::StaticMethod(f) => (**f).clone(),
                other => other.clone(),
            };
            let mut a = Vec::with_capacity(args.len() + 1);
            a.push(Value::Class(cls.clone()));
            a.extend(args.iter().cloned());
            self.call_kw(&new_f, a, kwargs.clone())?
        };
        if !self.isinstance(&obj, cls) {
            return Ok(obj);
        }
        if let Some(init) = init {
            if !default_init {
                let mut a = Vec::with_capacity(args.len() + 1);
                a.push(obj.clone());
                a.extend(args);
                let r = self.call_kw(&init, a, kwargs)?;
                if !r.is_none() {
                    return Err(type_err(format!(
                        "__init__() should return None, not '{}'",
                        self.type_name(&r)
                    )));
                }
            }
        }
        Ok(obj)
    }

    // ------------------------------------------------------------------
    // Pattern matching helpers
    // ------------------------------------------------------------------

    pub fn match_sequence(&mut self, subject: &Value, spec: u32) -> PyResult<bool> {
        let star = spec & 0x8000_0000 != 0;
        let n = (spec & 0x7fff_ffff) as usize;
        let len = match subject {
            Value::List(l) => l.borrow().len(),
            Value::Tuple(t) => t.len(),
            Value::Range(r) => r.len() as usize,
            Value::Instance(i) => match &*i.native.borrow() {
                NativeData::Base(Value::List(l)) => l.borrow().len(),
                NativeData::Base(Value::Tuple(t)) => t.len(),
                _ => return Ok(false),
            },
            Value::Native(nv) => match &*nv.data.borrow() {
                NativeKind::Deque(d, _) => d.len(),
                _ => return Ok(false),
            },
            _ => return Ok(false),
        };
        Ok(if star { len >= n } else { len == n })
    }

    pub fn match_keys(
        &mut self,
        subject: &Value,
        keys: &Value,
        with_rest: bool,
    ) -> PyResult<(Value, Option<Value>)> {
        let keys = match keys {
            Value::Tuple(t) => (**t).clone(),
            _ => vec![],
        };
        let mut vals = vec![];
        for k in &keys {
            let has = self.contains(subject, k)?;
            if !has {
                let rest = if with_rest { Some(Value::None) } else { None };
                return Ok((Value::None, rest));
            }
            vals.push(self.getitem(subject, k)?);
        }
        let rest = if with_rest {
            let d = new_ref(Dict::new());
            let items = self.mapping_items(subject)?;
            for (k, v) in items {
                let mut skip = false;
                for key in &keys {
                    if self.eq(&k, key)? {
                        skip = true;
                    }
                }
                if !skip {
                    self.dict_set(&d, k, v)?;
                }
            }
            Some(Value::Dict(d))
        } else {
            None
        };
        Ok((Value::tuple(vals), rest))
    }

    pub fn match_class(
        &mut self,
        subject: &Value,
        cls: &Value,
        nargs: usize,
        names: &Value,
    ) -> PyResult<Value> {
        let Value::Class(c) = cls else {
            return Err(type_err("called match pattern must be a class"));
        };
        if !self.isinstance(subject, c) {
            return Ok(Value::None);
        }
        let mut out = vec![];
        if nargs > 0 {
            // Builtins like int(x) / str(x) match the subject itself.
            let self_match = c.builtin
                && matches!(
                    c.kind,
                    Kind::Int
                        | Kind::Bool
                        | Kind::Float
                        | Kind::Str
                        | Kind::Bytes
                        | Kind::List
                        | Kind::Tuple
                        | Kind::Dict
                        | Kind::Set
                        | Kind::FrozenSet
                );
            if self_match {
                if nargs > 1 {
                    return Err(type_err(format!(
                        "{}() accepts 1 positional sub-pattern ({nargs} given)",
                        c.name()
                    )));
                }
                out.push(subject.clone());
            } else {
                let ma = c.lookup("__match_args__");
                let ma = match ma {
                    Some(Value::Tuple(t)) => (*t).clone(),
                    _ => {
                        return Err(type_err(format!(
                            "{}() accepts 0 positional sub-patterns ({nargs} given)",
                            c.name()
                        )))
                    }
                };
                if nargs > ma.len() {
                    return Err(type_err(format!(
                        "{}() accepts {} positional sub-pattern{} ({nargs} given)",
                        c.name(),
                        ma.len(),
                        if ma.len() == 1 { "" } else { "s" }
                    )));
                }
                for name in &ma[..nargs] {
                    let Value::Str(n) = name else {
                        return Err(type_err("__match_args__ elements must be strings"));
                    };
                    match self.getattr_opt(subject, &n.s)? {
                        Some(v) => out.push(v),
                        None => return Ok(Value::None),
                    }
                }
            }
        }
        if let Value::Tuple(ns) = names {
            for n in ns.iter() {
                if let Value::Str(n) = n {
                    match self.getattr_opt(subject, &n.s)? {
                        Some(v) => out.push(v),
                        None => return Ok(Value::None),
                    }
                }
            }
        }
        Ok(Value::tuple(out))
    }
}

pub fn num_cmp(x: &Num, y: &Num) -> Ordering {
    match (x, y) {
        (Num::I(a), Num::I(b)) => a.cmp(b),
        (Num::F(a), Num::F(b)) => a.partial_cmp(b).unwrap_or(Ordering::Equal),
        (Num::F(a), Num::I(b)) => cmp_f_i(*a, *b),
        (Num::I(a), Num::F(b)) => cmp_f_i(*b, *a).reverse(),
        (Num::F(a), Num::B(b)) => cmp_f_big(*a, b),
        (Num::B(a), Num::F(b)) => cmp_f_big(*b, a).reverse(),
        _ => to_big(x).cmp(&to_big(y)),
    }
}
fn cmp_f_i(f: f64, i: i64) -> Ordering {
    if f.is_nan() {
        return Ordering::Equal;
    }
    let fi = i as f64;
    if f < fi {
        Ordering::Less
    } else if f > fi {
        Ordering::Greater
    } else if f.abs() < 9.0e15 {
        Ordering::Equal
    } else {
        // Exact comparison near the precision boundary.
        let fb = BigInt::from_f64(f);
        fb.cmp(&BigInt::from_i64(i))
    }
}
fn cmp_f_big(f: f64, b: &BigInt) -> Ordering {
    if f.is_infinite() {
        return if f > 0.0 {
            Ordering::Greater
        } else {
            Ordering::Less
        };
    }
    if f.is_nan() {
        return Ordering::Equal;
    }
    let fb = BigInt::from_f64(f);
    match fb.cmp(b) {
        Ordering::Equal => {
            let frac = f - f.trunc();
            if frac > 0.0 {
                Ordering::Greater
            } else if frac < 0.0 {
                Ordering::Less
            } else {
                Ordering::Equal
            }
        }
        o => o,
    }
}

pub fn float_divmod(x: f64, y: f64) -> (f64, f64) {
    let mut m = x % y;
    let mut div = (x - m) / y;
    if m != 0.0 {
        if (y < 0.0) != (m < 0.0) {
            m += y;
            div -= 1.0;
        }
    } else {
        m = 0.0f64.copysign(y);
    }
    let floordiv = if div != 0.0 {
        let mut f = div.floor();
        if div - f > 0.5 {
            f += 1.0;
        }
        f
    } else {
        0.0f64.copysign(x / y)
    };
    (floordiv, m)
}

/// Correctly-rounded-enough big integer true division.
fn big_true_div(x: &BigInt, y: &BigInt) -> PyResult<f64> {
    if let (Some(a), Some(b)) = (x.to_f64(), y.to_f64()) {
        if a.is_finite() && b.is_finite() && x.bit_length() <= 53 && y.bit_length() <= 53 {
            return Ok(a / b);
        }
    }
    // Scale so the quotient carries ~64 significant bits.
    let shift = 64i64 + y.bit_length() as i64 - x.bit_length() as i64;
    let (num, den) = if shift > 0 {
        (x.shl(shift as u64), y.clone())
    } else {
        (x.clone(), y.shl((-shift) as u64))
    };
    let (q, r) = num.divrem_trunc(&den);
    let mut q = q;
    if !r.is_zero() {
        // Sticky bit for rounding.
        q = q
            .shl(1)
            .add(&BigInt::from_i64(if q.is_negative() { -1 } else { 1 }));
        let qf = q.to_f64().unwrap_or(0.0);
        let v = qf * 2f64.powi(-(shift as i32) - 1);
        if v.is_infinite() {
            return Err(err(
                "OverflowError",
                "integer division result too large for a float",
            ));
        }
        return Ok(v);
    }
    let qf = q.to_f64().unwrap_or(0.0);
    let v = qf * 2f64.powi(-(shift as i32));
    if v.is_infinite() {
        return Err(err(
            "OverflowError",
            "integer division result too large for a float",
        ));
    }
    Ok(v)
}
