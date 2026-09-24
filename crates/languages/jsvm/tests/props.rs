//! Property maps key on canonical strings and find a key by its address; these
//! programs reach properties through every kind of key the VM makes (literal,
//! computed at run time, from `Object.keys`, numeric, symbol) on small and
//! large objects, with deletes and re-adds, and check what they read.
use cw_jsvm::value::Value;
use cw_jsvm::vm::Vm;
use cw_script_host::memory::MemoryHost;

fn eval(src: &str) -> String {
    let mut host = MemoryHost::default();
    let mut vm = Vm::new(&mut host, vec!["/usr/bin/node".into()], vec![], None);
    match vm.eval_source(src, "props.js", false) {
        Ok(Value::Str(s)) => s.to_string(),
        Ok(_) => panic!("not a string"),
        Err(_) => panic!("threw"),
    }
}

#[test]
fn keys_made_at_run_time_find_literal_properties() {
    let out = eval(
        r#"
        const o = { alpha: 1, beta: 2 };
        const k = ['al', 'pha'].join('');
        const big = {};
        for (let i = 0; i < 40; i++) big['key' + i] = i;
        const viaKeys = Object.keys(o).map((k) => o[k]).join(',');
        [o[k], o['be' + 'ta'], 'alpha' in o, ('gam' + 'ma') in o, big.key7, big['key' + 39],
         big['nope' + 1], viaKeys, JSON.stringify(JSON.parse('{"alpha":5,"zeta":6}').alpha)].join(' ')
        "#,
    );
    assert_eq!(out, "1 2 true false 7 39  1,2 5");
}

#[test]
fn deletes_readds_and_order_survive() {
    let out = eval(
        r#"
        const o = {};
        for (let i = 0; i < 20; i++) o['p' + i] = i;
        delete o.p3; delete o['p' + 15];
        o.p3 = 'back';
        const s1 = Symbol('s'), s2 = Symbol('s');
        o[s1] = 'one'; o[s2] = 'two';
        o[7] = 'seven'; o['8'] = 'eight';
        const keys = Object.keys(o);
        [keys.length, keys[0], keys[1], keys[keys.length - 1], o.p3, o.p15, o[s1], o[s2],
         o['7'], o[8], Object.getOwnPropertySymbols(o).length].join(' ')
        "#,
    );
    assert_eq!(out, "21 7 8 p3 back  one two seven eight 2");
}

#[test]
fn prototype_chains_and_shadowing() {
    let out = eval(
        r#"
        class A { m() { return 'A'; } get g() { return 'gA'; } }
        class B extends A { m() { return 'B' + super.m(); } }
        const b = new B();
        const name = 'm';
        const before = b[name]();
        b.m = () => 'own';
        const own = b.m();
        delete b.m;
        const d = Object.create(null);
        d['__proto__'] = 1;
        [before, own, b.m(), b.g, b.hasOwnProperty('m'), d.__proto__, Object.keys(d).length].join(' ')
        "#,
    );
    assert_eq!(out, "BA own BA gA false 1 1");
}
