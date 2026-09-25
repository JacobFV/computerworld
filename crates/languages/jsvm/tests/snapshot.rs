//! Heap snapshots: a VM written to bytes and read back continues exactly as
//! the original does, and writes the same bytes again.
use cw_jsvm::snapshot::Options;
use cw_jsvm::value::Value;
use cw_jsvm::vm::Vm;
use cw_script_host::memory::MemoryHost;

const OPTS: Options = Options {
    hooks: &[],
    fingerprint: 0,
};

fn eval(vm: &mut Vm, src: &str) -> String {
    match vm.eval_source_with(src, "snap.js", false, true) {
        Ok(Value::Str(s)) => s.to_string(),
        Ok(_) => String::new(),
        Err(cw_jsvm::value::Ctl::Throw(v)) => {
            let m = vm.inspect_default(&v).unwrap_or_default();
            panic!("threw: {m}")
        }
        Err(_) => panic!("exited"),
    }
}

const SETUP: &str = r#"
globalThis.log = [];
const reg = Symbol.for('app.key'), own = Symbol('own');
class Base { #secret = 41; static count = 0; constructor(n) { this.n = n; Base.count++; }
  get secret() { return this.#secret; } bump() { return ++this.#secret; } }
class Derived extends Base { constructor(n) { super(n * 2); this.kind = 'd'; }
  describe() { return `${this.kind}:${this.n}:${super.bump()}`; } }
globalThis.d = new Derived(4);
let counter = 0;
globalThis.inc = () => ++counter;
inc(); inc();
globalThis.m = new Map([[1, 'a'], ['x', { deep: [1, , 3] }], [own, reg]]);
m.delete(1); m.set(2, 'b');
globalThis.s = new Set([1, 2, 3]); s.delete(2);
globalThis.wm = new WeakMap(); wm.set(d, 'weak');
function* gen() { let i = 0; while (true) { const got = yield i++; if (got) log.push('got ' + got); } }
globalThis.g = gen(); g.next(); g.next(); g.next('hello');
globalThis.pending = new Promise(r => { globalThis.resolveIt = r; });
globalThis.asyncDone = (async () => { const v = await pending; log.push('async ' + v); return v * 2; })();
asyncDone.then(v => log.push('then ' + v));
function tag(strs, ...vals) { return strs; }
globalThis.tpl = () => tag`a${1}b`;
globalThis.firstTpl = tpl();
globalThis.re = /(\w)(\d)/g; re.exec('a1 b2');
globalThis.buf = new ArrayBuffer(8); globalThis.u8 = new Uint8Array(buf); globalThis.i16 = new Int16Array(buf, 2, 2);
u8[3] = 7; i16[1] = -2;
globalThis.big = 12345678901234567890n * -3n;
globalThis.when = new Date(86400000);
globalThis.err = new TypeError('bad thing');
globalThis.prox = new Proxy({ a: 1 }, { get(t, k) { return k in t ? t[k] : 'dflt:' + String(k); } });
globalThis.bound = function (a, b) { return this.x + a + b; }.bind({ x: 10 }, 5);
globalThis.frozen = Object.freeze({ f: [1, 2] });
globalThis.negz = -0; globalThis.nan = NaN; globalThis.frac = 0.1 + 0.2;
Object.defineProperty(globalThis, 'acc', { get() { return counter * 100; }, configurable: true });
globalThis.iter = [10, 20, 30][Symbol.iterator](); iter.next();
globalThis.keysIt = m.keys(); keysIt.next();
setTimeout(() => log.push('timer ' + counter), 5);
queueMicrotask(() => log.push('micro'));
''
"#;

const CONTINUE: &str = r#"
resolveIt(21);
[d.describe(), d.secret, Base.count, inc(), [...m.entries()].map(e => String(e[0]) + '=' + JSON.stringify(e[1])).join(','),
 [...s].join(), wm.get(d), g.next('x').value, g.next().value, tpl() === firstTpl, re.lastIndex, re.exec('a1 b2')[0],
 Array.from(u8).join(), i16[1], big.toString(), when.toISOString(), err.name + ':' + err.message, err instanceof TypeError,
 prox.a, prox.zz, bound(1), Object.isFrozen(frozen), 1 / negz, Number.isNaN(nan), frac, acc, iter.next().value,
 String(keysIt.next().value), Symbol.for('app.key') === [...m.values()][1], typeof Derived, String(Symbol('q').description)].join('|')
"#;

fn run_to_idle(vm: &mut Vm) {
    if vm.event_loop().is_err() {
        panic!("event loop threw: {}", vm.stderr);
    }
}

#[test]
fn a_restored_vm_continues_as_the_original_and_writes_the_same_bytes() {
    let mut h1 = MemoryHost::default();
    let mut vm = Vm::new(&mut h1, vec!["/usr/bin/node".into()], vec![], None);
    eval(&mut vm, SETUP);
    let a = vm.heap_snapshot(&[], OPTS).expect("snapshot");
    let mut h2 = MemoryHost::default();
    let (mut back, roots) = Vm::from_heap_snapshot(&mut h2, &a, OPTS).expect("restore");
    assert!(roots.is_empty());
    let b = back.heap_snapshot(&[], OPTS).expect("snapshot of restored");
    assert!(
        a == b,
        "the restored VM writes different bytes ({} vs {})",
        a.len(),
        b.len()
    );

    let out1 = eval(&mut vm, CONTINUE);
    run_to_idle(&mut vm);
    let log1 = eval(&mut vm, "log.join(';')");
    let out2 = eval(&mut back, CONTINUE);
    run_to_idle(&mut back);
    let log2 = eval(&mut back, "log.join(';')");
    assert_eq!(out1, out2);
    assert_eq!(log1, log2);
    assert!(
        log1.contains("timer") && log1.contains("async 21"),
        "{log1}"
    );
    assert_eq!(vm.elapsed_ms, back.elapsed_ms);
    assert_eq!(vm.steps, back.steps);
    assert_eq!(vm.stdout, back.stdout);
    let c = vm.heap_snapshot(&[], OPTS).unwrap();
    let d = back.heap_snapshot(&[], OPTS).unwrap();
    assert!(
        c == d,
        "after continuing, the two VMs write different bytes"
    );
}

#[test]
fn embedder_roots_keep_their_identity() {
    let mut h1 = MemoryHost::default();
    let mut vm = Vm::new(&mut h1, vec!["/usr/bin/node".into()], vec![], None);
    eval(&mut vm, "globalThis.shared = { v: 1 }; ''");
    let shared = vm.global.own_value("shared").unwrap();
    let bytes = vm
        .heap_snapshot(&[shared, Value::Num(3.5), Value::Undefined], OPTS)
        .unwrap();
    let mut h2 = MemoryHost::default();
    let (back, roots) = Vm::from_heap_snapshot(&mut h2, &bytes, OPTS).unwrap();
    assert_eq!(roots.len(), 3);
    let Value::Obj(r) = &roots[0] else { panic!() };
    let Some(Value::Obj(g)) = back.global.own_value("shared") else {
        panic!()
    };
    assert!(r.ptr_eq(&g));
    assert!(matches!(roots[1], Value::Num(n) if n == 3.5));
}

#[test]
fn another_fingerprint_is_refused() {
    let mut h1 = MemoryHost::default();
    let vm = Vm::new(&mut h1, vec!["/usr/bin/node".into()], vec![], None);
    let bytes = vm.heap_snapshot(&[], OPTS).unwrap();
    let mut h2 = MemoryHost::default();
    let other = Options {
        hooks: &[],
        fingerprint: 1,
    };
    assert!(Vm::from_heap_snapshot(&mut h2, &bytes, other).is_err());
}
