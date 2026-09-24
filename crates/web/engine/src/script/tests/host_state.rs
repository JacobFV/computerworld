//! The realm's host-driven state: `:hover` following content that moves under a
//! still pointer, overlay scrollbars, and message tasks draining within one
//! `run_until_idle`.

use super::*;
use crate::script::{Modifiers, UiEvent};

const HOVER_PAGE: &str = r#"<!DOCTYPE html><html><head><style>
  body { margin: 0 }
  #open { display: block; width: 200px; height: 40px; background-color: rgb(0, 0, 255) }
  #open:hover { background-color: rgb(255, 0, 0) }
  .overlay { position: fixed; left: 0; top: 0; width: 400px; height: 300px; background-color: rgb(0, 0, 0) }
</style></head><body><button id=open>Open</button><script>
  const log = [];
  const b = document.getElementById('open');
  for (const t of ['mouseover', 'mouseout', 'mouseenter', 'mouseleave', 'mousemove', 'pointerover', 'pointerout'])
    b.addEventListener(t, () => log.push('button ' + t));
  b.addEventListener('click', () => {
    const o = document.createElement('div');
    o.className = 'overlay';
    o.id = 'overlay';
    o.addEventListener('mouseover', () => log.push('overlay mouseover'));
    document.body.appendChild(o);
  });
</script></body></html>"#;

fn background(r: &mut Realm, id: &str) -> String {
    r.eval(&format!(
        "getComputedStyle(document.getElementById('{id}')).backgroundColor"
    ))
    .unwrap()
}

#[test]
fn hover_leaves_a_button_an_overlay_opens_over() {
    let mut r = realm(HOVER_PAGE);
    let modifiers = Modifiers::default();
    r.dispatch(UiEvent::PointerMove {
        x: 50,
        y: 20,
        modifiers,
    });
    assert_eq!(background(&mut r, "open"), "rgb(255, 0, 0)");
    r.dispatch(UiEvent::Click {
        x: 50,
        y: 20,
        button: 0,
        modifiers,
        detail: 1,
    });
    // The click itself moved the pointer onto the button; what follows is the
    // content changing under it.
    r.eval("log.length = 0").unwrap();
    r.run_until_idle(20);
    // The overlay now covers the pointer: the button is no longer hovered.
    let overlay = r.document().by_id("overlay")[0];
    assert_eq!(r.hovered(), Some(overlay));
    assert_eq!(background(&mut r, "open"), "rgb(0, 0, 255)");
    // Boundary events fired for the change; no move events (the pointer did not
    // move).
    let log = r.eval("log.join()").unwrap();
    assert!(
        log.contains("button mouseout") && log.contains("button mouseleave"),
        "{log}"
    );
    assert!(log.contains("overlay mouseover"), "{log}");
    assert!(!log.contains("mousemove"), "{log}");
}

#[test]
fn hover_stays_when_nothing_moved_under_the_pointer() {
    let mut r = realm(HOVER_PAGE);
    r.dispatch(UiEvent::PointerMove {
        x: 50,
        y: 20,
        modifiers: Modifiers::default(),
    });
    r.eval("log.length = 0").unwrap();
    r.run_until_idle(20);
    assert_eq!(background(&mut r, "open"), "rgb(255, 0, 0)");
    assert_eq!(r.eval("log.join()").unwrap(), "");
}

#[test]
fn overlay_scrollbars_give_scroll_containers_their_whole_width() {
    let page = r#"<!DOCTYPE html><html><body style="margin:0">
      <div id=s style="width:200px;height:100px;overflow:scroll"><div style="height:400px"></div></div>
    </body></html>"#;
    let mut r = realm(page);
    let classic = r.eval("document.getElementById('s').clientWidth").unwrap();
    r.set_overlay_scrollbars(true);
    let overlay = r.eval("document.getElementById('s').clientWidth").unwrap();
    assert_eq!((classic.as_str(), overlay.as_str()), ("185", "200"));
    // The setting is host state a restore keeps.
    let snap = r.snapshot();
    assert!(snap.overlay_scrollbars);
    let mut back = Realm::restore(&snap, Box::new(default_host()));
    assert_eq!(
        back.eval("document.getElementById('s').clientWidth")
            .unwrap(),
        "200"
    );
}

#[test]
fn a_chain_of_posted_messages_drains_in_one_run() {
    // React's scheduler continues its work by posting itself a message; a chain of
    // them must not stall behind the idle budget.
    let page = r#"<!DOCTYPE html><html><body><script>
      window.hops = 0;
      const ch = new MessageChannel();
      ch.port1.onmessage = () => { if (++window.hops < 200) ch.port2.postMessage(null); };
      setTimeout(() => ch.port2.postMessage(null), 0);
    </script></body></html>"#;
    let mut r = Realm::new(
        page,
        "https://example.test/page.html",
        Box::new(default_host()),
    );
    r.run_document();
    r.run_until_idle(5);
    assert_eq!(r.eval("String(window.hops)").unwrap(), "200");
}

/// An embedder that answers `__cw_host` calls: `echo` returns its payload.
struct EchoHost;
impl crate::script::ScriptHostDocument for EchoHost {
    fn host_call(&mut self, name: &str, payload: &str) -> Result<String, String> {
        match name {
            "echo" => Ok(format!("echo:{payload}")),
            _ => Err(format!("no such call: {name}")),
        }
    }
}

const HOST_CALL_PAGE: &str = r#"<!DOCTYPE html><html><body><script>
  window.answers = [__cw_host('echo', 'hi'), __cw_host('echo', JSON.stringify({ n: 1 }))];
  try { __cw_host('nope', ''); } catch (e) { answers.push(e.message); }
</script></body></html>"#;

#[test]
fn a_page_calls_its_embedder_synchronously() {
    let mut r = Realm::new(
        HOST_CALL_PAGE,
        "https://example.test/page.html",
        Box::new(EchoHost),
    );
    r.run_document();
    assert_eq!(
        r.eval("answers.join('|')").unwrap(),
        "echo:hi|echo:{\"n\":1}|no such call: nope"
    );
    // A restore replays the answers from the journal, whatever the new host says.
    let snap = r.snapshot();
    let mut back = Realm::restore(&snap, Box::new(default_host()));
    assert_eq!(
        back.eval("answers.join('|')").unwrap(),
        "echo:hi|echo:{\"n\":1}|no such call: nope"
    );
    // `__cw_host` is not an enumerable global.
    assert_eq!(
        r.eval("String(Object.keys(window).includes('__cw_host'))")
            .unwrap(),
        "false"
    );
}

#[test]
fn journaling_can_be_turned_off() {
    let page = "<!DOCTYPE html><html><body><script>setTimeout(() => { window.t = Date.now(); }, 5); localStorage.getItem('k');</script></body></html>";
    let mut on = Realm::new(
        page,
        "https://example.test/page.html",
        Box::new(default_host()),
    );
    on.run_document();
    on.run_until_idle(20);
    let mut off = Realm::new(
        page,
        "https://example.test/page.html",
        Box::new(default_host()),
    );
    off.set_journaling(false);
    off.run_document();
    off.run_until_idle(20);
    assert!(on.journal_len() > 2, "{}", on.journal_len());
    let before = off.journal_len();
    for _ in 0..10 {
        off.run_until_idle(20);
        off.eval("localStorage.getItem('k')").unwrap();
    }
    assert_eq!(off.journal_len(), before);
    // The page runs the same either way.
    assert_eq!(on.eval("typeof t").unwrap(), off.eval("typeof t").unwrap());
}
