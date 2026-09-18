//! `_random`: CPython's Mersenne Twister core, bit-for-bit, so seeded sequences
//! match CPython. Unseeded generators draw their seed from the world's entropy.
use super::{new_module, set_val};
use crate::bigint::BigInt;
use crate::builtins::*;
use crate::value::*;
use crate::vm::*;
use std::rc::Rc;

const N: usize = 624;
const M: usize = 397;

#[derive(Clone)]
pub struct Mt {
    mt: [u32; N],
    index: usize,
}

impl Mt {
    pub fn new() -> Self {
        let mut m = Mt {
            mt: [0; N],
            index: N + 1,
        };
        m.init_genrand(5489);
        m
    }
    fn init_genrand(&mut self, s: u32) {
        self.mt[0] = s;
        for i in 1..N {
            self.mt[i] = 1812433253u32
                .wrapping_mul(self.mt[i - 1] ^ (self.mt[i - 1] >> 30))
                .wrapping_add(i as u32);
        }
        self.index = N;
    }
    pub fn init_by_array(&mut self, key: &[u32]) {
        self.init_genrand(19650218);
        let mut i = 1usize;
        let mut j = 0usize;
        let len = key.len().max(1);
        let mut k = N.max(len);
        while k > 0 {
            let prev = self.mt[i - 1];
            self.mt[i] = (self.mt[i] ^ (prev ^ (prev >> 30)).wrapping_mul(1664525))
                .wrapping_add(*key.get(j).unwrap_or(&0))
                .wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i >= N {
                self.mt[0] = self.mt[N - 1];
                i = 1;
            }
            if j >= len {
                j = 0;
            }
            k -= 1;
        }
        k = N - 1;
        while k > 0 {
            let prev = self.mt[i - 1];
            self.mt[i] = (self.mt[i] ^ (prev ^ (prev >> 30)).wrapping_mul(1566083941))
                .wrapping_sub(i as u32);
            i += 1;
            if i >= N {
                self.mt[0] = self.mt[N - 1];
                i = 1;
            }
            k -= 1;
        }
        self.mt[0] = 0x8000_0000;
        self.index = N;
    }
    pub fn genrand_u32(&mut self) -> u32 {
        const UPPER: u32 = 0x8000_0000;
        const LOWER: u32 = 0x7fff_ffff;
        const MATRIX_A: u32 = 0x9908_b0df;
        if self.index >= N {
            for kk in 0..N {
                let y = (self.mt[kk] & UPPER) | (self.mt[(kk + 1) % N] & LOWER);
                let mag = if y & 1 == 1 { MATRIX_A } else { 0 };
                self.mt[kk] = self.mt[(kk + M) % N] ^ (y >> 1) ^ mag;
            }
            self.index = 0;
        }
        let mut y = self.mt[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^= y >> 18;
        y
    }
    pub fn random(&mut self) -> f64 {
        let a = self.genrand_u32() >> 5;
        let b = self.genrand_u32() >> 6;
        (a as f64 * 67108864.0 + b as f64) * (1.0 / 9007199254740992.0)
    }
    pub fn seed_big(&mut self, n: &BigInt) {
        let bytes = n
            .abs()
            .to_bytes_le((n.bit_length().div_ceil(8) + 1) as usize, false)
            .unwrap_or_default();
        let mut key: Vec<u32> = bytes
            .chunks(4)
            .map(|c| {
                let mut b = [0u8; 4];
                b[..c.len()].copy_from_slice(c);
                u32::from_le_bytes(b)
            })
            .collect();
        while key.len() > 1 && key.last() == Some(&0) {
            key.pop();
        }
        if key.is_empty() {
            key.push(0);
        }
        self.init_by_array(&key);
    }
    pub fn getrandbits(&mut self, k: u64) -> BigInt {
        if k == 0 {
            return BigInt::zero();
        }
        if k <= 32 {
            return BigInt::from_u64((self.genrand_u32() >> (32 - k)) as u64);
        }
        let words = ((k - 1) / 32 + 1) as usize;
        let mut bytes = Vec::with_capacity(words * 4);
        let mut rem = k as i64;
        for _ in 0..words {
            let mut r = self.genrand_u32();
            if rem < 32 {
                r >>= 32 - rem;
            }
            bytes.extend_from_slice(&r.to_le_bytes());
            rem -= 32;
        }
        BigInt::from_bytes_le(&bytes, false)
    }
}
impl Default for Mt {
    fn default() -> Self {
        Self::new()
    }
}

fn mt_of(vm: &Vm, v: &Value) -> PyResult<Rc<Native>> {
    match vm.base_value(v) {
        Value::Native(n) if matches!(&*n.data.borrow(), NativeKind::Random(_)) => Ok(n),
        _ => Err(type_err("descriptor requires a '_random.Random' object")),
    }
}

fn with_mt<R>(n: &Rc<Native>, f: impl FnOnce(&mut Mt) -> R) -> R {
    match &mut *n.data.borrow_mut() {
        NativeKind::Random(m) => f(m),
        _ => unreachable!(),
    }
}

fn seed_from(vm: &mut Vm, mt: &mut Mt, arg: Option<&Value>) -> PyResult<()> {
    match arg {
        None | Some(Value::None) => {
            let mut key = vec![];
            for _ in 0..8 {
                let r = vm.host.random_u64();
                key.push(r as u32);
                key.push((r >> 32) as u32);
            }
            mt.init_by_array(&key);
        }
        Some(Value::Int(i)) => mt.seed_big(&BigInt::from_i64(*i)),
        Some(Value::Bool(b)) => mt.seed_big(&BigInt::from_i64(*b as i64)),
        Some(Value::Big(b)) => mt.seed_big(b),
        Some(other) => {
            let h = vm.hash(other)?;
            mt.seed_big(&BigInt::from_u64(h as u64));
        }
    }
    Ok(())
}

fn r_new(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let cls = match a.args.remove(0) {
        Value::Class(c) => c,
        _ => return Err(type_err("expected class")),
    };
    let mut mt = Mt::new();
    seed_from(vm, &mut mt, a.args.first())?;
    let native = Value::Native(Rc::new(Native {
        class: cls.clone(),
        data: std::cell::RefCell::new(NativeKind::Random(Box::new(mt))),
    }));
    let inst = vm.new_instance(&cls);
    if let Value::Instance(i) = &inst {
        *i.native.borrow_mut() = NativeData::Base(native);
    }
    Ok(inst)
}
fn r_random(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let n = mt_of(vm, &a.args[0])?;
    Ok(Value::Float(with_mt(&n, |m| m.random())))
}
fn r_seed(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let n = mt_of(vm, &a.args[0])?;
    let mut mt = with_mt(&n, |m| m.clone());
    seed_from(vm, &mut mt, a.args.get(1))?;
    with_mt(&n, |m| *m = mt);
    Ok(Value::None)
}
fn r_getrandbits(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let n = mt_of(vm, &a.args[0])?;
    let k = to_int_arg(vm, a.args.get(1).unwrap_or(&Value::Int(0)))?;
    if k < 0 {
        return Err(value_err("number of bits must be non-negative"));
    }
    Ok(Value::big(with_mt(&n, |m| m.getrandbits(k as u64))))
}
fn r_getstate(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let n = mt_of(vm, &a.args[0])?;
    let (words, idx) = with_mt(&n, |m| (m.mt.to_vec(), m.index));
    let mut items: Vec<Value> = words.iter().map(|w| Value::Int(*w as i64)).collect();
    items.push(Value::Int(idx as i64));
    Ok(Value::tuple(items))
}
fn r_setstate(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let n = mt_of(vm, &a.args[0])?;
    let items = vm.iterate(a.args.get(1).unwrap_or(&Value::None))?;
    if items.len() != N + 1 {
        return Err(value_err("state vector is the wrong size"));
    }
    let mut words = [0u32; N];
    for (i, w) in words.iter_mut().enumerate() {
        *w = to_int_arg(vm, &items[i])? as u32;
    }
    let idx = to_int_arg(vm, &items[N])? as usize;
    with_mt(&n, |m| {
        m.mt = words;
        m.index = idx.min(N);
    });
    Ok(Value::None)
}

pub fn make(vm: &mut Vm) -> Value {
    let m = new_module("_random");
    let cls = new_class("Random", vec![vm.t.object.clone()], Kind::Object, true);
    cls.dict
        .borrow_mut()
        .set_str("__module__", Value::str("_random"));
    let b = Builtin {
        name: "Random.__new__".into(),
        func: r_new,
        data: Value::None,
        owner: Some("type"),
    };
    cls.dict.borrow_mut().set_str(
        "__new__",
        Value::StaticMethod(Rc::new(Value::Builtin(Rc::new(b)))),
    );
    add_fn(&cls, "random", r_random);
    add_fn(&cls, "seed", r_seed);
    add_fn(&cls, "getrandbits", r_getrandbits);
    add_fn(&cls, "getstate", r_getstate);
    add_fn(&cls, "setstate", r_setstate);
    add_fn(&cls, "__init__", |_, _| Ok(Value::None));
    set_val(&m, "Random", Value::Class(cls));
    Value::Module(m)
}
