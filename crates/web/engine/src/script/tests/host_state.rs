//! The realm's host-driven state: message tasks draining within one
//! `run_until_idle`.

use super::*;

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
