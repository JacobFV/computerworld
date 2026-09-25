//! A long-lived page must not grow its heap without bound. Typing into a
//! framework-controlled input makes garbage in cycles (a closure and its
//! `prototype`, React's circular update queues), which reference counting alone
//! never frees; the VM's cycle collector (`cw_jsvm::gc`) must. After a warm-up,
//! a thousand more keystrokes leave the live object count where it was. And a
//! realm, once dropped, leaves nothing behind.
//!
//!     cargo test --release -p cw-web --features pipeline --test heap_growth -- --nocapture

#[cfg(feature = "pipeline")]
mod heap {
    use cw_web::script::{MemoryHost, Modifiers, Realm, UiEvent};
    use std::path::PathBuf;

    const BASE: &str = "https://example.test/";

    fn crate_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn host() -> MemoryHost {
        let dir = crate_dir().join("tests/vendor");
        let mut h = MemoryHost::new();
        let mut entries: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.is_file())
            .collect();
        entries.sort();
        for p in entries {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            let Ok(body) = std::fs::read_to_string(&p) else {
                continue;
            };
            h = h.with_response(&format!("{BASE}vendor/{name}"), "text/javascript", &body);
        }
        h
    }

    /// Live JS objects after a full collection.
    fn live() -> i64 {
        cw_jsvm::gc::collect();
        cw_jsvm::value::live_objects()
    }

    /// Boots `fixture`, focuses its task input and types `warm` then `more`
    /// keystrokes (clearing the field every 50 so the value itself stays
    /// small); returns the live objects after each batch.
    fn typing(fixture: &str, warm: usize, more: usize) -> (i64, i64) {
        let html = std::fs::read_to_string(
            crate_dir().join(format!("tests/framework-parity/{fixture}.html")),
        )
        .unwrap();
        let mut r = Realm::new(&html, &format!("{BASE}{fixture}.html"), Box::new(host()));
        r.run_document();
        r.run_until_idle(50);
        let at = r
            .eval("(() => { const q = document.querySelector('#new-task').getBoundingClientRect(); return [q.left + 5, q.top + q.height / 2].join(); })()")
            .unwrap();
        let mut it = at.split(',').map(|n| n.parse::<f64>().unwrap() as i32);
        let (x, y) = (it.next().unwrap(), it.next().unwrap());
        r.dispatch(UiEvent::Click {
            x,
            y,
            button: 0,
            modifiers: Modifiers::default(),
            detail: 1,
        });
        r.run_until_idle(20);
        let key = |r: &mut Realm, n: usize| {
            for i in 0..n {
                if i % 50 == 49 {
                    r.dispatch(UiEvent::Key {
                        key: "a".into(),
                        code: String::new(),
                        modifiers: Modifiers {
                            ctrl: true,
                            ..Modifiers::default()
                        },
                        repeat: false,
                    });
                    r.dispatch(UiEvent::Key {
                        key: "Backspace".into(),
                        code: String::new(),
                        modifiers: Modifiers::default(),
                        repeat: false,
                    });
                } else {
                    r.dispatch(UiEvent::TypeText { text: "x".into() });
                }
                r.run_until_idle(20);
            }
        };
        key(&mut r, warm);
        let before = live();
        key(&mut r, more);
        let after = live();
        (before, after)
    }

    fn check(fixture: &str) {
        let (before, after) = typing(fixture, 200, 1000);
        let (runs, freed) = cw_jsvm::gc::totals();
        eprintln!(
            "{fixture}: {before} live objects after 200 keystrokes, {after} after 1,200 \
             ({:+.2} per keystroke); {runs} collections freed {freed} nodes",
            (after - before) as f64 / 1000.0
        );
        assert!(
            after - before < 500,
            "{fixture}: the heap grew by {} objects over 1,000 keystrokes",
            after - before
        );
    }

    #[test]
    fn typing_into_react_keeps_the_heap_bounded() {
        check("react18-tasks");
    }

    #[test]
    fn typing_into_vue_keeps_the_heap_bounded() {
        check("vue3-tasks");
    }

    #[test]
    fn a_dropped_realm_frees_its_heap() {
        let before = live();
        let html =
            std::fs::read_to_string(crate_dir().join("tests/framework-parity/react18-tasks.html"))
                .unwrap();
        let mut r = Realm::new(
            &html,
            &format!("{BASE}react18-tasks.html"),
            Box::new(host()),
        );
        r.run_document();
        r.run_until_idle(50);
        let booted = cw_jsvm::value::live_objects();
        drop(r);
        let after = cw_jsvm::value::live_objects();
        eprintln!("react18-tasks: {booted} objects live while booted, {after} after the drop");
        assert!(booted - before > 5_000, "{before} -> {booted}");
        assert_eq!(after, before);
    }
}
