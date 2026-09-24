//! The realm's host-driven state: overlay scrollbars, and message tasks
//! draining within one `run_until_idle`.

use super::*;

#[test]
fn overlay_scrollbars_give_scroll_containers_their_whole_width() {
    let page = r#"<!DOCTYPE html><html><body style="margin:0">
      <div id=s style="width:200px;height:100px;overflow:scroll"><div style="height:400px"></div></div>
    </body></html>"#;
    let mut r = realm(page);
    let classic = r
        .eval("document.getElementById('s').clientWidth")
        .unwrap();
    r.set_overlay_scrollbars(true);
    let overlay = r
        .eval("document.getElementById('s').clientWidth")
        .unwrap();
    assert_eq!((classic.as_str(), overlay.as_str()), ("185", "200"));
    // The setting is host state a restore keeps.
    let snap = r.snapshot();
    assert!(snap.overlay_scrollbars);
    let mut back = Realm::restore(&snap, Box::new(default_host()));
    assert_eq!(
        back.eval("document.getElementById('s').clientWidth").unwrap(),
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
    let mut r = Realm::new(page, "https://example.test/page.html", Box::new(default_host()));
    r.run_document();
    r.run_until_idle(5);
    assert_eq!(r.eval("String(window.hops)").unwrap(), "200");
}
