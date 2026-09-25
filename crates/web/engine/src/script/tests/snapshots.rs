//! Heap snapshots of realms: a realm started from the thread's template of the
//! booted prelude is the realm that runs the prelude itself.

use crate::script::{set_realm_templates, MemoryHost, Realm, Viewport};

const PAGE: &str = "<!DOCTYPE html><title>t</title><p id=p>x</p><script>
  console.log(location.href, innerWidth, innerHeight, typeof Date.now(), Math.random());
  document.getElementById('p').textContent = String(performance.now() >= 0);
  setTimeout(() => console.log('later', Math.random()), 5);
</script>";

fn host(now: i64, seed: u64, width: u32) -> MemoryHost {
    let mut h = MemoryHost::new();
    h.now_micros = now;
    h.seed = seed;
    h.viewport = Viewport {
        width,
        height: 600,
        scale: 2,
        zoom: 100,
    };
    h
}

fn run(url: &str, h: MemoryHost, templates: bool) -> (Vec<u8>, Vec<u8>, String) {
    set_realm_templates(templates);
    let mut r = Realm::new(PAGE, url, Box::new(h));
    set_realm_templates(true);
    let fresh = r.heap_image().unwrap();
    r.run_document();
    r.run_until_idle(50);
    let logs: Vec<String> = r.logs().into_iter().map(|l| l.text).collect();
    let journal = serde_json::to_string(&r.snapshot().journal).unwrap();
    (
        fresh,
        r.heap_image().unwrap(),
        format!("{}\n{journal}", logs.join("\n")),
    )
}

#[test]
fn a_realm_from_the_template_is_the_realm_that_runs_the_prelude() {
    for (url, now, seed, width) in [
        ("https://a.test/x#frag", 0, 7, 1280),
        ("https://b.test/", 1_700_000_000_000_000, 99, 390),
        ("about:blank", 42, 0, 800),
    ] {
        let a = run(url, host(now, seed, width), false);
        let b = run(url, host(now, seed, width), true);
        assert!(a.0 == b.0, "{url}: the booted realms differ");
        assert!(a.1 == b.1, "{url}: the realms differ after the page ran");
        assert_eq!(a.2, b.2, "{url}");
    }
}
