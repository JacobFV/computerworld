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

#[test]
fn adding_properties_respects_prototypes_and_extensibility() {
    let out = eval(
        r#"
        'use strict';
        const log = [];
        const proto = { set watched(v) { log.push('setter ' + v); } };
        Object.defineProperty(proto, 'fixed', { value: 1, writable: false });
        const o = Object.create(proto);
        o.watched = 5;               // runs the inherited setter, adds nothing
        let threw = false;
        try { o.fixed = 2; } catch (e) { threw = e instanceof TypeError; }
        o.fresh = 3;                 // a new own property
        const sealed = Object.preventExtensions({ a: 1 });
        let threw2 = false;
        try { sealed.b = 2; } catch (e) { threw2 = e instanceof TypeError; }
        sealed.a = 9;
        [log.join(), Object.keys(o).join(), threw, o.fixed, o.fresh, threw2, sealed.a,
         'b' in sealed].join(' ')
        "#,
    );
    assert_eq!(out, "setter 5 fresh true 1 3 true 9 false");
}

#[test]
fn computed_keys_equality_and_literals_on_the_fast_paths() {
    let src = r#"
        const o = { a: 1, a: 2, [('b')]: 3 };
        const k = Object.keys(o);
        const arr = [1, 2];
        arr[2] = 3; arr[0] = 9; arr['1'] = 8;
        const target = {};
        for (const key of k) target[key] = o[key] * 10;
        const frozen = Object.freeze({ x: 1 });
        const setOn = (obj, key, v) => { try { obj[key] = v; return 'ok'; } catch (e) { return e.name; } };
        const eqs = [null == undefined, null != 0, undefined == 0, 1 == 1, 'a' == 'a', true == true,
                     o == o, o == {}, '1' == 1, 0 == false, null == false, NaN == NaN, 1n == 1];
        [k.join(), o.a, JSON.stringify(arr), arr.length, JSON.stringify(target),
         setOn(frozen, 'x', 2), frozen.x, eqs.join()].join(' ')
    "#;
    let out = eval(src);
    assert_eq!(
        out,
        "a,b 2 [9,8,3] 3 {\"a\":20,\"b\":30} ok 1 true,true,false,true,true,true,true,false,true,true,false,false,true"
    );
}
