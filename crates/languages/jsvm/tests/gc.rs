//! The cycle collector frees garbage that reference counting cannot (objects
//! that refer to each other and to nothing live) and leaves everything a
//! program can still reach working.
use cw_jsvm::gc;
use cw_jsvm::value::{live_objects, Value};
use cw_jsvm::vm::Vm;
use cw_script_host::memory::MemoryHost;

fn eval(vm: &mut Vm, src: &str) -> String {
    match vm.eval_source(src, "gc.js", false) {
        Ok(Value::Str(s)) => s.to_string(),
        Ok(_) => panic!("not a string"),
        Err(_) => panic!("threw"),
    }
}

#[test]
fn cycles_are_freed_and_live_objects_keep_working() {
    let mut host = MemoryHost::default();
    let mut vm = Vm::new(&mut host, vec!["/usr/bin/node".into()], vec![], None);
    eval(
        &mut vm,
        r#"
        globalThis.keep = [];
        globalThis.churn = function (n) {
            for (let i = 0; i < n; i++) {
                const a = { i }, b = { a };
                a.b = b;                                  // an object cycle
                function f() { return f; }                // fn <-> prototype
                let count = 0;
                const inc = () => ++count;                // closure over a cell
                a.inc = inc;
                if (i % 100 === 0) keep.push(a);          // some stay reachable
            }
            return '';
        };
        ''
        "#,
    );
    gc::collect();
    let before = live_objects();
    eval(&mut vm, "churn(2000)");
    let grown = live_objects();
    let stats = gc::collect();
    let after = live_objects();
    assert!(stats.freed > 0, "{stats:?}");
    assert!(grown - before > 10_000, "{before} -> {grown}");
    assert!(after - before < 200, "{before} -> {grown} -> {after}");
    let out = eval(
        &mut vm,
        "keep.length + ' ' + keep[3].b.a.i + ' ' + keep[3].inc() + keep[3].inc() + ' ' + keep[19].i",
    );
    assert_eq!(out, "20 300 12 1900");
}
