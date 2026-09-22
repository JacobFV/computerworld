//! The fixture pages under `crates/web/tests/script/`: a vanilla tabs/accordion/menu
//! page, jQuery 3.7 and Preact 10 with hooks, run unmodified through the realm and
//! driven with `dispatch`.

use super::*;
use crate::dom::NodeId;
use crate::script::{DefaultAction, Modifiers, UiEvent};

fn fixture(name: &str) -> String {
    let p = format!("{}/tests/script/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{p}: {e}"))
}

fn vendor(name: &str) -> String {
    let p = format!("{}/tests/vendor/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{p}: {e}"))
}

fn host() -> MemoryHost {
    MemoryHost::new()
        .with_response(
            "https://example.test/vendor/jquery-3.7.1.min.js",
            "text/javascript",
            &vendor("jquery-3.7.1.min.js"),
        )
        .with_response(
            "https://example.test/vendor/preact-10.19.3.umd.js",
            "text/javascript",
            &vendor("preact-10.19.3.umd.js"),
        )
        .with_response(
            "https://example.test/vendor/preact-hooks-10.19.3.umd.js",
            "text/javascript",
            &vendor("preact-hooks-10.19.3.umd.js"),
        )
        .with_response(
            "https://example.test/api/items",
            "application/json",
            "{\"items\":[1,2,3]}",
        )
        .with_response("https://example.test/api/text", "text/plain", "plain text")
}

fn id(r: &Realm, id: &str) -> NodeId {
    r.document().by_id(id)[0]
}

fn click(r: &mut Realm, node: NodeId) -> DefaultAction {
    r.dispatch(UiEvent::ClickNode {
        node,
        modifiers: Modifiers::default(),
        detail: 1,
    })
}

fn text_of(r: &mut Realm, id: &str) -> String {
    r.eval(&format!("document.getElementById('{id}').textContent"))
        .unwrap()
}

#[test]
fn vanilla_tabs_accordion_menu() {
    let mut r = Realm::new(
        &fixture("tabs.html"),
        "https://example.test/tabs.html",
        Box::new(host()),
    );
    r.run_document();
    assert!(errors(&r).is_empty(), "{}", errors(&r));
    assert_eq!(text_of(&mut r, "status"), "loaded 3");
    let two = r.eval("document.querySelector('[data-tab=two]')").unwrap();
    assert!(!two.is_empty());
    let two_btn = r
        .document()
        .descendants(id(&r, "tabs"))
        .find(|n| r.document().attr(*n, "data-tab") == Some("two"))
        .unwrap();
    assert_eq!(click(&mut r, two_btn), DefaultAction::Focus(two_btn));
    assert_eq!(text_of(&mut r, "status"), "tab:two visible:40");
    assert_eq!(r.eval("[one.offsetHeight, two.offsetHeight, three.offsetHeight, document.querySelector('.tabs .active').textContent].join()").unwrap(), "0,40,0,Two");
    let head2 = r
        .document()
        .descendants(id(&r, "acc2"))
        .find(|n| r.document().is(*n, "h3"))
        .unwrap();
    click(&mut r, head2);
    assert_eq!(text_of(&mut r, "status"), "acc2 open 30");
    click(&mut r, head2);
    assert_eq!(text_of(&mut r, "status"), "acc2 closed 0");
    // Hover opens the menu through :hover and the mouseenter handler; a click on
    // the revealed link navigates.
    let menu_rect = r
        .eval("JSON.stringify(menu.getBoundingClientRect())")
        .unwrap();
    assert!(menu_rect.contains("\"width\":80"), "{menu_rect}");
    let (mx, my) = {
        let rect = r
            .eval("const q = menu.getBoundingClientRect(); [q.left + 5, q.top + 5].join()")
            .unwrap();
        let mut it = rect.split(',').map(|v| v.parse::<f64>().unwrap() as i32);
        (it.next().unwrap(), it.next().unwrap())
    };
    r.dispatch(UiEvent::PointerMove {
        x: mx,
        y: my,
        modifiers: Modifiers::default(),
    });
    assert_eq!(text_of(&mut r, "status"), "menu open block");
    assert_eq!(
        r.eval("getComputedStyle(document.getElementById('menu-list')).display")
            .unwrap(),
        "block"
    );
    let action = {
        let n = id(&r, "link-a");
        click(&mut r, n)
    };
    assert_eq!(
        action,
        DefaultAction::Navigate("https://example.test/a".into())
    );
    r.dispatch(UiEvent::PointerMove {
        x: 600,
        y: 700,
        modifiers: Modifiers::default(),
    });
    assert_eq!(text_of(&mut r, "status"), "menu closed");
    r.dispatch(UiEvent::Key {
        key: "Escape".into(),
        code: String::new(),
        modifiers: Modifiers::default(),
        repeat: false,
    });
    assert_eq!(text_of(&mut r, "status"), "escaped");
}

#[test]
fn jquery_page_runs_unmodified() {
    let mut r = Realm::new(
        &fixture("jquery.html"),
        "https://example.test/jq.html",
        Box::new(host()),
    );
    r.run_document();
    assert!(errors(&r).is_empty(), "{}", errors(&r));
    r.run_until_idle(100);
    assert!(errors(&r).is_empty(), "{}", errors(&r));
    assert_eq!(
        r.eval("typeof jQuery + ' ' + $.fn.jquery").unwrap(),
        "function 3.7.1"
    );
    assert_eq!(r.eval("window.boxHidden").unwrap(), "true none 100 50");
    assert_eq!(r.eval("window.boxShown").unwrap(), "true 100 50 0");
    assert_eq!(r.eval("$('li').length + ' ' + $('li.sel').attr('data-k') + ' ' + $('li.added').css('color') + ' ' + $('li.added')[0].style.marginLeft").unwrap(), "4 v rgb(255, 0, 0) 3px");
    assert_eq!(
        r.eval("window.each + ' ' + window.filtered").unwrap(),
        "0o1t2t3f two|three"
    );
    assert_eq!(r.eval("window.parentInfo").unwrap(), "list three 4 two");
    assert_eq!(r.eval("window.valInfo").unwrap(), "hello y q=hello&s=y");
    assert_eq!(r.eval("window.htmlInfo + ' ' + window.customGot + ' ' + $('#made').data('x') + ' ' + $('#made').parent().attr('id')").unwrap(), "true 3 5 wrap");
    assert_eq!(r.eval("window.dimInfo").unwrap(), "1280x800 800 0");
    assert_eq!(
        r.eval("$('#ajax').text() + ' ' + window.gotText").unwrap(),
        "1,2,3 plain text"
    );
    assert_eq!(
        r.eval("window.animDone + ' ' + window.boxAfterFade")
            .unwrap(),
        "true none"
    );
    // Click handlers bound through jQuery see the realm's trusted events.
    let li = r
        .document()
        .descendants(id(&r, "list"))
        .filter(|n| r.document().is(*n, "li"))
        .nth(1)
        .unwrap();
    click(&mut r, li);
    assert_eq!(text_of(&mut r, "out"), "clicked two true idx 1");
    click(&mut r, li);
    assert_eq!(text_of(&mut r, "out"), "clicked two false idx 1");
    // A prevented submit keeps the browser out of it.
    assert_eq!(
        {
            let n = id(&r, "go");
            click(&mut r, n)
        },
        DefaultAction::Prevented
    );
    assert_eq!(text_of(&mut r, "out"), "submitted q=changed&s=y");
}

#[test]
fn preact_counter_with_hooks_and_keyed_list() {
    let mut r = Realm::new(
        &fixture("preact.html"),
        "https://example.test/preact.html",
        Box::new(host()),
    );
    r.run_document();
    assert!(errors(&r).is_empty(), "{}", errors(&r));
    r.run_until_idle(50);
    assert!(errors(&r).is_empty(), "{}", errors(&r));
    assert_eq!(r.eval("document.querySelector('h1').textContent + '|' + document.getElementById('dec').disabled + '|' + document.querySelectorAll('#list li').length + '|' + window.effects.join(',') + '|' + document.title").unwrap(), "Count: 0|true|3|count:0,mounted INPUT|Count 0");
    let inc = id(&r, "inc");
    click(&mut r, inc);
    click(&mut r, inc);
    click(&mut r, inc);
    r.run_until_idle(50);
    assert_eq!(r.eval("document.querySelector('h1').textContent + '|' + document.getElementById('dec').disabled + '|' + document.getElementById('big').style.color + '|' + document.getElementById('big').style.fontSize + '|' + document.title + '|' + window.effects.length").unwrap(), "Count: 3|false|red|20px|Count 3|5");
    {
        let n = id(&r, "dec");
        click(&mut r, n)
    };
    r.run_until_idle(50);
    assert_eq!(
        r.eval("document.querySelector('h1').textContent + '|' + !!document.getElementById('big')")
            .unwrap(),
        "Count: 2|false"
    );
    // Keyed list: removing the middle item keeps the other DOM nodes.
    let first_before = r
        .eval("document.querySelector('#list li').dataset.id")
        .unwrap();
    let li_b = r
        .document()
        .descendants(id(&r, "list"))
        .find(|n| r.document().attr(*n, "data-id") == Some("2"))
        .unwrap();
    let li_a = r
        .document()
        .descendants(id(&r, "list"))
        .find(|n| r.document().attr(*n, "data-id") == Some("1"))
        .unwrap();
    click(&mut r, li_b);
    r.run_until_idle(50);
    assert_eq!(r.eval("[...document.querySelectorAll('#list li')].map(l => l.dataset.id + l.textContent).join()").unwrap(), "1a,3c");
    assert_eq!(first_before, "1");
    assert_eq!(
        r.document()
            .descendants(id(&r, "list"))
            .find(|n| r.document().attr(*n, "data-id") == Some("1")),
        Some(li_a)
    );
    {
        let n = id(&r, "add");
        click(&mut r, n)
    };
    r.run_until_idle(50);
    assert_eq!(
        r.eval("[...document.querySelectorAll('#list li')].map(l => l.textContent).join()")
            .unwrap(),
        "new2,a,c"
    );
    // Controlled input through typing.
    r.dispatch(UiEvent::Focus {
        node: Some(id(&r, "text")),
    });
    r.dispatch(UiEvent::TypeText { text: "hey".into() });
    r.run_until_idle(50);
    assert_eq!(r.eval("document.getElementById('echo').textContent + '|' + document.getElementById('text').value").unwrap(), "You typed: hey|hey");
    assert!(errors(&r).is_empty(), "{}", errors(&r));
}
