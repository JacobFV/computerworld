//! Timing and profiling harness for script on the framework-parity fixtures
//! (`tests/framework-parity/{react18,vue3}-tasks.html`, driven as in
//! `framework_parity.rs`): boot (load the page, run its scripts, settle), a click
//! that re-renders (`#check-2`), and a keystroke into a controlled input
//! (`#new-task`). Each phase is timed over several fresh realms (median reported),
//! then profiled once with the VM's built-in profiler. "First boot" loads with the
//! compile cache empty; "boot" is a later realm loading the same page.
//!
//!     cargo test --release -p cw-web --features pipeline --test script_perf -- --ignored --nocapture
//!
//! `SCRIPT_PERF_RUNS` sets the timed runs (default 7), `SCRIPT_PERF_TOP` the rows per
//! profile table (default 25), `SCRIPT_PERF_OP_TIME=1` times every instruction,
//! `SCRIPT_PERF_PROPS=1` counts property reads by key, `SCRIPT_PERF_FIXTURE` limits
//! the run to one fixture, `SCRIPT_PERF_PHASE` prints only one phase's profile and
//! `SCRIPT_PERF_NO_PROFILE=1` only times (for an external profiler such as
//! callgrind with `--toggle-collect=script_perf::perf::click_at`; the phases are
//! the functions `first_boot`, `boot`, `click_at` (two clicks) and `key`).

#[cfg(feature = "pipeline")]
mod perf {
    use cw_web::script::{MemoryHost, Modifiers, ProfileOptions, Realm, UiEvent};
    use std::path::PathBuf;
    use std::time::Instant;

    const BASE: &str = "https://example.test/";

    fn crate_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn host() -> MemoryHost {
        let dir = crate_dir().join("tests/vendor");
        let mut entries: Vec<_> = std::fs::read_dir(&dir)
            .expect("tests/vendor")
            .map(|e| e.unwrap().path())
            .filter(|p| p.is_file())
            .collect();
        entries.sort();
        let mut h = MemoryHost::new();
        for p in entries {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            let Ok(body) = std::fs::read_to_string(&p) else {
                continue;
            };
            let ty = if name.ends_with(".css") {
                "text/css"
            } else {
                "text/javascript"
            };
            h = h.with_response(&format!("{BASE}vendor/{name}"), ty, &body);
        }
        h
    }

    fn centre(r: &mut Realm, selector: &str) -> (i32, i32) {
        let src = format!(
            "(() => {{ const q = document.querySelector({selector:?}).getBoundingClientRect(); \
             return [q.left + q.width / 2, q.top + q.height / 2].join(); }})()"
        );
        let v = r.eval(&src).unwrap_or_else(|e| panic!("{selector}: {e}"));
        let mut it = v.split(',').map(|n| n.parse::<f64>().unwrap() as i32);
        (it.next().unwrap(), it.next().unwrap())
    }

    #[inline(never)]
    fn click_at(r: &mut Realm, (x, y): (i32, i32)) {
        let modifiers = Modifiers::default();
        r.dispatch(UiEvent::PointerMove { x, y, modifiers });
        r.dispatch(UiEvent::Click {
            x,
            y,
            button: 0,
            modifiers,
            detail: 1,
        });
        r.run_until_idle(20);
    }

    #[inline(never)]
    fn key(r: &mut Realm) {
        r.dispatch(UiEvent::TypeText { text: "x".into() });
        r.run_until_idle(20);
    }

    fn load(name: &str, html: &str) -> Realm {
        let mut r = Realm::new(html, &format!("{BASE}{name}.html"), Box::new(host()));
        r.run_document();
        r.run_until_idle(50);
        r
    }

    /// A load with nothing compiled yet on the thread.
    #[inline(never)]
    fn first_boot(name: &str, html: &str) -> Realm {
        cw_jsvm::codecache::clear();
        load(name, html)
    }

    /// A later load of the same page.
    #[inline(never)]
    fn boot(name: &str, html: &str) -> Realm {
        load(name, html)
    }

    fn median(mut v: Vec<f64>) -> f64 {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[v.len() / 2]
    }

    fn env_flag(name: &str) -> bool {
        std::env::var(name).map(|v| v == "1").unwrap_or(false)
    }

    fn run(name: &str) {
        let html = std::fs::read_to_string(
            crate_dir().join(format!("tests/framework-parity/{name}.html")),
        )
        .expect("fixture");
        let runs: usize = std::env::var("SCRIPT_PERF_RUNS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(7);
        let top: usize = std::env::var("SCRIPT_PERF_TOP")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(25);
        let (mut news, mut boots, mut clicks, mut keys) = (vec![], vec![], vec![], vec![]);
        let mut colds = vec![];
        for _ in 0..runs {
            let t = Instant::now();
            drop(first_boot(name, &html));
            colds.push(t.elapsed().as_secs_f64() * 1000.0);
            // Later loads reuse the compiled prelude and bundles.
            let t = Instant::now();
            let r = Realm::new(&html, &format!("{BASE}{name}.html"), Box::new(host()));
            news.push(t.elapsed().as_secs_f64() * 1000.0);
            drop(r);
            let t = Instant::now();
            let mut r = boot(name, &html);
            boots.push(t.elapsed().as_secs_f64() * 1000.0);
            let at = centre(&mut r, "#check-2");
            let t = Instant::now();
            click_at(&mut r, at);
            clicks.push(t.elapsed().as_secs_f64() * 1000.0);
            let at = centre(&mut r, "#new-task");
            click_at(&mut r, at);
            let t = Instant::now();
            key(&mut r);
            keys.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        eprintln!(
            "{name}: first boot {:.2} ms, boot {:.2} ms (of which Realm::new {:.2}), click {:.2} ms, key {:.2} ms (median of {runs}; boot min {:.2})",
            median(colds),
            median(boots.clone()),
            median(news),
            median(clicks),
            median(keys),
            boots.iter().cloned().fold(f64::MAX, f64::min),
        );

        if env_flag("SCRIPT_PERF_NO_PROFILE") {
            return;
        }
        let opts = ProfileOptions {
            op_time: env_flag("SCRIPT_PERF_OP_TIME"),
            prop_names: env_flag("SCRIPT_PERF_PROPS"),
        };
        let only = std::env::var("SCRIPT_PERF_PHASE").ok();
        let show = |phase: &str| only.as_deref().map(|p| p == phase).unwrap_or(true);
        let mut r = Realm::new(&html, &format!("{BASE}{name}.html"), Box::new(host()));
        r.profile_start(opts);
        r.run_document();
        r.run_until_idle(50);
        let p = r.profile_stop().unwrap();
        if show("boot") {
            eprintln!("\n==== {name} boot profile ====\n{}", p.to_text(top));
        }
        let at = centre(&mut r, "#check-2");
        r.profile_start(opts);
        click_at(&mut r, at);
        let p = r.profile_stop().unwrap();
        if show("click") {
            eprintln!("\n==== {name} click profile ====\n{}", p.to_text(top));
        }
        let at = centre(&mut r, "#new-task");
        click_at(&mut r, at);
        r.profile_start(opts);
        key(&mut r);
        let p = r.profile_stop().unwrap();
        if show("key") {
            eprintln!("\n==== {name} key profile ====\n{}", p.to_text(top));
        }
    }

    fn selected(name: &str) -> bool {
        std::env::var("SCRIPT_PERF_FIXTURE")
            .map(|f| f == name)
            .unwrap_or(true)
    }

    #[test]
    #[ignore = "timing harness: run with --release -- --ignored --nocapture"]
    fn react18_tasks() {
        if selected("react18-tasks") {
            run("react18-tasks");
        }
    }

    #[test]
    #[ignore = "timing harness: run with --release -- --ignored --nocapture"]
    fn vue3_tasks() {
        if selected("vue3-tasks") {
            run("vue3-tasks");
        }
    }
}
