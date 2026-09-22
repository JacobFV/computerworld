//! Milestone 4: unmodified production builds of the major front-end frameworks
//! (React 18/17, Vue 3, Svelte 4, styled-components, emotion) run on the script
//! layer. Each fixture under `crates/web/tests/script/` loads its vendored bundle
//! from `crates/web/tests/vendor/` (see `tools/fetch-vendor.sh`), runs to idle and is
//! driven through `Realm::dispatch`.

use super::*;
use crate::dom::NodeId;
use crate::script::{DefaultAction, Modifiers, UiEvent};

const BASE: &str = "https://example.test/";

fn manifest(rel: &str) -> String {
    let p = format!("{}/tests/{rel}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{p}: {e}"))
}

/// A host serving every file of `tests/vendor/` (recursively) under `/vendor/`.
fn host() -> MemoryHost {
    fn add(dir: &std::path::Path, prefix: &str, mut h: MemoryHost) -> MemoryHost {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        entries.sort();
        for p in entries {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            if p.is_dir() {
                h = add(&p, &format!("{prefix}{name}/"), h);
            } else if let Ok(body) = std::fs::read_to_string(&p) {
                let ty = if name.ends_with(".css") {
                    "text/css"
                } else if name.ends_with(".json") {
                    "application/json"
                } else {
                    "text/javascript"
                };
                h = h.with_response(&format!("{BASE}vendor/{prefix}{name}"), ty, &body);
            }
        }
        h
    }
    let dir = format!("{}/tests/vendor", env!("CARGO_MANIFEST_DIR"));
    add(std::path::Path::new(&dir), "", MemoryHost::new())
}

/// Loads a fixture page, runs it to idle and reports the load time.
fn load(name: &str) -> (Realm, std::time::Duration) {
    let html = manifest(&format!("script/{name}"));
    let h = host();
    let t = std::time::Instant::now();
    let mut r = Realm::new(&html, &format!("{BASE}{name}"), Box::new(h));
    r.run_document();
    r.run_until_idle(50);
    let dt = t.elapsed();
    eprintln!("load {name}: {:.1} ms", dt.as_secs_f64() * 1000.0);
    (r, dt)
}

/// The connected element with `id` (frameworks that mount over a template leave
/// detached nodes with the same id behind).
fn id(r: &Realm, id: &str) -> NodeId {
    let d = r.document();
    d.by_id(id)
        .iter()
        .copied()
        .find(|n| d.ancestors(*n).any(|a| a == crate::dom::Document::ROOT))
        .unwrap_or_else(|| panic!("no connected #{id}"))
}

fn click(r: &mut Realm, el: &str) -> DefaultAction {
    let node = id(r, el);
    let a = r.dispatch(UiEvent::ClickNode {
        node,
        modifiers: Modifiers::default(),
        detail: 1,
    });
    r.run_until_idle(20);
    a
}

/// Clicks a node a framework rendered without an id.
fn click_node(r: &mut Realm, node: NodeId) -> DefaultAction {
    let a = r.dispatch(UiEvent::ClickNode {
        node,
        modifiers: Modifiers::default(),
        detail: 1,
    });
    r.run_until_idle(20);
    a
}

/// The first descendant of `#parent` carrying `class`.
fn by_class(r: &Realm, parent: &str, class: &str) -> NodeId {
    let p = id(r, parent);
    let d = r.document();
    d.descendants(p)
        .find(|n| {
            d.attr(*n, "class")
                .map(|c| c.split_ascii_whitespace().any(|x| x == class))
                .unwrap_or(false)
        })
        .unwrap_or_else(|| panic!("no .{class} in #{parent}"))
}

fn focus(r: &mut Realm, el: &str) {
    let node = id(r, el);
    r.dispatch(UiEvent::Focus { node: Some(node) });
}

fn type_text(r: &mut Realm, el: &str, text: &str) {
    focus(r, el);
    r.dispatch(UiEvent::TypeText { text: text.into() });
    r.run_until_idle(20);
}

fn key(r: &mut Realm, k: &str) {
    r.dispatch(UiEvent::Key {
        key: k.into(),
        code: String::new(),
        modifiers: Modifiers::default(),
        repeat: false,
    });
    r.run_until_idle(20);
}

fn set_value(r: &mut Realm, el: &str, value: &str) {
    let node = id(r, el);
    r.dispatch(UiEvent::SetValue {
        node,
        value: value.into(),
        commit: true,
    });
    r.run_until_idle(20);
}

fn hover(r: &mut Realm, el: &str) {
    let rect = r.eval(&format!("(() => {{ const q = document.getElementById('{el}').getBoundingClientRect(); return [q.left + q.width / 2, q.top + q.height / 2].join(); }})()")).unwrap();
    let mut it = rect.split(',').map(|v| v.parse::<f64>().unwrap() as i32);
    let (x, y) = (it.next().unwrap(), it.next().unwrap());
    r.dispatch(UiEvent::PointerMove {
        x,
        y,
        modifiers: Modifiers::default(),
    });
    r.run_until_idle(20);
}

fn js(r: &mut Realm, src: &str) -> String {
    r.eval(src).unwrap_or_else(|e| panic!("{src}: {e}"))
}

fn text(r: &mut Realm, el: &str) -> String {
    js(r, &format!("document.getElementById('{el}').textContent"))
}

fn computed(r: &mut Realm, el: &str, prop: &str) -> String {
    js(
        r,
        &format!("getComputedStyle(document.getElementById('{el}')).getPropertyValue('{prop}')"),
    )
}

/// The colour the paint pass fills `#el`'s background with, as `r,g,b,a`, so a test
/// can say the painted scene really changed and not just the computed style.
fn painted_fill(r: &mut Realm, el: &str) -> String {
    let node = id(r, el);
    let i = r.layout();
    let scene = crate::paint::paint(
        &i.doc,
        &i.styles,
        i.tree.as_ref().unwrap(),
        i.viewport,
        &crate::paint::PaintContext::default(),
    );
    let fill = scene
        .nodes
        .iter()
        .find(|n| (n.id >> 28) as u32 == node.0 && (n.id & 0xFFFF) as u32 == 1)
        .map(|n| match n.primitive {
            cw_scene::Primitive::Box { fill, .. }
            | cw_scene::Primitive::RoundedBox { fill, .. } => fill,
            _ => cw_scene::Color(0, 0, 0, 0),
        });
    match fill {
        Some(cw_scene::Color(r, g, b, a)) => format!("{r},{g},{b},{a}"),
        None => "none".into(),
    }
}

/// Everything the page or a framework wrote to the console at any level.
fn all_logs(r: &Realm) -> String {
    r.logs()
        .iter()
        .map(|l| format!("[{:?}] {}", l.level, l.text))
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_clean(r: &Realm) {
    assert!(
        errors(r).is_empty(),
        "console errors/warnings:\n{}",
        errors(r)
    );
}

/// Development aid: `M4_PAGE=/path/page.html [M4_EVAL='js'] cargo test scratch_page -- --ignored --nocapture`
/// loads an arbitrary page against the vendor host and prints its console.
#[test]
#[ignore]
fn scratch_page() {
    let Ok(path) = std::env::var("M4_PAGE") else {
        return;
    };
    let html = std::fs::read_to_string(&path).unwrap();
    let t = std::time::Instant::now();
    let mut r = Realm::new(&html, &format!("{BASE}scratch.html"), Box::new(host()));
    r.run_document();
    r.run_until_idle(
        std::env::var("M4_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(50),
    );
    eprintln!("load: {:.1} ms", t.elapsed().as_secs_f64() * 1000.0);
    // M4_STEPS: a file of `click ID`, `type ID TEXT`, `key KEY`, `set ID VALUE`, `hover ID`,
    // `idle MS`, `eval JS` lines.
    if let Ok(steps) = std::env::var("M4_STEPS") {
        for line in std::fs::read_to_string(steps).unwrap().lines() {
            let mut it = line.splitn(2, ' ');
            let (cmd, rest) = (it.next().unwrap_or(""), it.next().unwrap_or(""));
            let mut args = rest.splitn(2, ' ');
            let (a, b) = (args.next().unwrap_or(""), args.next().unwrap_or(""));
            match cmd {
                "click" => eprintln!("click {a}: {:?}", click(&mut r, a)),
                "type" => type_text(&mut r, a, b),
                "key" => key(&mut r, a),
                "set" => set_value(&mut r, a, b),
                "hover" => hover(&mut r, a),
                "idle" => {
                    r.run_until_idle(a.parse().unwrap());
                }
                "eval" => eprintln!("eval> {:?}", r.eval(rest)),
                _ => {}
            }
        }
    }
    eprintln!("{}", all_logs(&r));
    eprintln!("BODY: {}", body_html(&r));
}

// ------------------------------------------------------------------ React 18

#[test]
fn react18_create_root_hooks_events_and_concurrent_features() {
    let (mut r, _) = load("react18.html");
    r.run_until_idle(200);
    assert_clean(&r);
    // render() is asynchronous under createRoot: nothing was committed synchronously.
    assert_eq!(js(&mut r, "String(window.syncAfterRender)"), "0");
    // Commit order at mount: layout effects run inside the commit with the DOM in
    // place; passive effects run in a later scheduler task (MessageChannel), after
    // the commit's microtasks, and before the next animation frame here.
    // (Where the animation frame falls relative to the passive-effect task is up to
    // the event loop; both come after the commit.)
    assert_eq!(
        js(&mut r, "trace.filter((t) => t !== 'T-raf0').join()"),
        "layout:0:Count: 0,T-layout:0,T-micro,effect:0,T-passive:0"
    );
    assert_eq!(
        js(
            &mut r,
            "String(trace.indexOf('T-raf0') > trace.indexOf('T-micro'))"
        ),
        "true"
    );
    assert_eq!(js(&mut r, "document.title"), "n=0");
    assert_eq!(js(&mut r, "typeof setImmediate"), "undefined");
    // useId: label and input share the generated id.
    assert_eq!(
        js(
            &mut r,
            "lbl.htmlFor + '|' + document.querySelector('[data-useid]').id"
        ),
        ":r0:|:r0:"
    );

    // useState / useMemo / useCallback / effects with cleanup.
    js(&mut r, "trace.length = 0");
    click(&mut r, "inc");
    assert_eq!(text(&mut r, "count"), "Count: 1");
    assert_eq!(text(&mut r, "derived"), "double=2 total=10 theme=light");
    assert_eq!(
        js(&mut r, "trace.join()"),
        "layout-cleanup:0,layout:1:Count: 1,cleanup:0,effect:1"
    );
    // Three updates in one handler are batched into one render.
    let before: i32 = text(&mut r, "renders").parse().unwrap();
    click(&mut r, "batch");
    assert_eq!(text(&mut r, "count"), "Count: 3");
    assert_eq!(text(&mut r, "derived"), "double=6 total=11 theme=light");
    assert_eq!(text(&mut r, "renders").parse::<i32>().unwrap(), before + 1);
    // useReducer and useContext.
    click(&mut r, "add5");
    click(&mut r, "theme");
    assert_eq!(text(&mut r, "derived"), "double=6 total=16 theme=dark");

    // Keyed list: reordering moves the same DOM nodes.
    let lis = |r: &Realm| -> Vec<(String, NodeId)> {
        let d = r.document();
        d.descendants(id(r, "list"))
            .filter(|n| d.is(*n, "li"))
            .map(|n| (d.attr(n, "data-k").unwrap().to_owned(), n))
            .collect()
    };
    let first = lis(&r);
    click(&mut r, "reverse");
    let reversed = lis(&r);
    assert_eq!(
        reversed.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
        ["c", "b", "a"]
    );
    assert_eq!(reversed.iter().rev().cloned().collect::<Vec<_>>(), first);
    click(&mut r, "prepend");
    click(&mut r, "drop");
    let after = lis(&r);
    assert_eq!(
        after.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
        ["z", "c", "a"]
    );
    assert_eq!(after[1], first[2]);
    assert_eq!(after[2], first[0]);

    // Controlled inputs: React's value tracker wraps the prototype's `value`
    // accessor; the handler upper-cases, so the DOM follows state, not keystrokes.
    assert_eq!(js(&mut r, "const d = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value'); typeof d.get + typeof d.set + Object.getOwnPropertyDescriptor(document.getElementById('name'), 'value').configurable"), "functionfunctiontrue");
    js(&mut r, "trace.length = 0");
    type_text(&mut r, "name", "hello");
    assert_eq!(text(&mut r, "form-out"), "HELLO|false|red|n:|one|hello");
    assert_eq!(js(&mut r, "document.getElementById('name').value"), "HELLO");
    key(&mut r, "Backspace");
    assert_eq!(js(&mut r, "document.getElementById('name').value"), "HELL");
    assert_eq!(
        click(&mut r, "agree"),
        DefaultAction::Toggle(id(&r, "agree"))
    );
    set_value(&mut r, "color", "blue");
    type_text(&mut r, "notes", "xyz");
    click(&mut r, "r2");
    // A controlled input whose handler ignores the change snaps back.
    type_text(&mut r, "locked", "zzz");
    assert_eq!(
        text(&mut r, "form-out"),
        "HELL|true|blue|n:xyz|two|helloBackspace"
    );
    assert_eq!(js(&mut r, "[agree.checked, color.value, color.selectedIndex, notes.value, r1.checked, r2.checked, locked.value].join()"), "true,blue,2,n:xyz,false,true,fixed");
    // Enter submits implicitly; the button submits; both are prevented by React's handler.
    focus(&mut r, "name");
    key(&mut r, "Enter");
    assert_eq!(click(&mut r, "submit"), DefaultAction::Prevented);
    assert_eq!(
        js(&mut r, "trace.join()"),
        "focus:name,blur:name,focus:name,submit:HELL,blur:name,submit:HELL"
    );

    // Delegated events: capture, bubble, stopPropagation, native event, mouseenter/leave.
    click(&mut r, "inner-stop");
    assert_eq!(text(&mut r, "seq"), "cS");
    click(&mut r, "inner-go");
    assert_eq!(text(&mut r, "seq"), "cScGn8MO");
    hover(&mut r, "hover");
    hover(&mut r, "count");
    assert_eq!(text(&mut r, "seq"), "cScGn8MOEL");

    // Portals render elsewhere in the DOM but bubble through the React tree.
    click(&mut r, "open");
    assert_eq!(
        js(
            &mut r,
            "document.getElementById('portal-target').innerHTML + bubbled.textContent"
        ),
        "<div id=\"dialog\" role=\"dialog\"><button id=\"in-portal\">inside</button></div>1"
    );
    click(&mut r, "in-portal");
    assert_eq!(text(&mut r, "bubbled"), "2");
    click(&mut r, "open");
    assert_eq!(
        js(
            &mut r,
            "document.getElementById('portal-target').innerHTML + bubbled.textContent"
        ),
        "3"
    );

    // Suspense + lazy: the fallback first, the content once the timer resolved and
    // React's fallback throttle (500 ms) elapsed on the world clock.
    r.run_until_idle(1000);
    assert_eq!(js(&mut r, "!!document.getElementById('loading') + ' ' + document.getElementById('lazy').textContent"), "false lazy loaded");

    // Refs and forwardRef.
    click(&mut r, "use-ref");
    // (An inline callback ref is detached and re-attached on every re-render.)
    assert_eq!(text(&mut r, "ref-info"), "INPUT:start:true:EM,null,EM");
    assert_eq!(r.focused(), Some(id(&r, "fancy")));

    // className, style objects (unitless numbers get px, custom properties pass
    // through), dangerouslySetInnerHTML and SVG through createElementNS.
    assert_eq!(js(&mut r, "const cs = getComputedStyle(box); [cs.backgroundColor, cs.marginLeft, cs.opacity, cs.borderTopWidth, cs.zIndex, cs.getPropertyValue('--accent')].join('|')"), "rgb(128, 0, 0)|4px|0.5|2px|3|blue");
    click(&mut r, "toggle-style");
    assert_eq!(js(&mut r, "const cs2 = getComputedStyle(box); [cs2.backgroundColor, cs2.marginLeft, box.className].join('|')"), "rgb(0, 128, 0)|12px|box on");
    assert_eq!(js(&mut r, "[circle.getAttribute('r'), circle.namespaceURI, circle instanceof SVGElement, svg.getAttribute('viewBox'), document.getElementById('use').getAttributeNS('http://www.w3.org/1999/xlink', 'href'), circle.getAttribute('class'), circle.getAttribute('stroke-width')].join('|')"), "15|http://www.w3.org/2000/svg|true|0 0 40 40|#circle|dot|2");
    assert_eq!(
        js(&mut r, "raw.innerHTML"),
        "<b id=\"bold\">bold</b> &amp; <i>it</i>"
    );

    // useTransition (a pending render was seen), useDeferredValue, useSyncExternalStore, flushSync.
    click(&mut r, "transition");
    r.run_until_idle(100);
    assert_eq!(text(&mut r, "conc"), "ab/ab/ab/0");
    assert!(text(&mut r, "pending-seen").contains('P'));
    js(&mut r, "store.set(7)");
    r.run_until_idle(50);
    assert_eq!(text(&mut r, "conc"), "ab/ab/ab/7");
    click(&mut r, "flush");
    r.run_until_idle(50);
    assert_eq!(js(&mut r, "window.flushed"), "sync/ab/ab/7");
    assert_eq!(text(&mut r, "conc"), "sync/ab/sync/7");

    // A discrete event flushes render, layout and passive effects synchronously,
    // before the handler's microtasks; timers and frames follow.
    js(&mut r, "trace.length = 0");
    click(&mut r, "timing");
    r.run_until_idle(50);
    assert_eq!(
        js(
            &mut r,
            "trace.slice(0, 3).join() + '|' + trace.slice(3).sort().join()"
        ),
        "T-layout:1,T-passive:1,T-micro|T-raf,T-timeout".replace("T-micro", "T-microtask")
    );
    assert_clean(&r);

    // Error boundary: the fallback renders; production React reports the caught
    // error once through console.error, as it does in Chromium.
    js(&mut r, "trace.length = 0");
    click(&mut r, "bomb");
    assert_eq!(
        js(
            &mut r,
            "document.getElementById('fallback').textContent + ' ' + trace.join()"
        ),
        "Error: boom caught:boom:string"
    );
    let errs = errors(&r);
    assert!(errs.starts_with("Error: boom"), "{errs}");
    assert_eq!(errs.matches("Error: boom").count(), 1, "{errs}");
}

#[test]
fn react17_legacy_render_class_components_and_forms() {
    let (mut r, _) = load("react17.html");
    assert_clean(&r);
    // ReactDOM.render commits synchronously.
    assert_eq!(
        js(
            &mut r,
            "syncChildren + ' ' + trace.join() + ' ' + React.version"
        ),
        "1 mount,rendered,items:1 17.0.2"
    );
    r.run_until_idle(250);
    assert!(
        text(&mut r, "ticks").starts_with("ticks: "),
        "{}",
        text(&mut r, "ticks")
    );
    assert!(js(&mut r, "trace.join()").contains("tick1,tick2"));
    type_text(&mut r, "draft", "milk");
    assert_eq!(js(&mut r, "add.disabled + draft.value"), "falsemilk");
    key(&mut r, "Enter");
    assert_eq!(js(&mut r, "[...document.querySelectorAll('#items li')].map(l => l.textContent).join() + '|' + draft.value + '|' + left.textContent + '|' + (document.activeElement === draft) + add.disabled"), "one,milk||2 left|truetrue");
    click(&mut r, "item-2");
    assert_eq!(js(&mut r, "document.getElementById('item-2').className + getComputedStyle(document.getElementById('item-2')).textDecorationLine + left.textContent"), "doneline-through1 left");
    click(&mut r, "toggle-clock");
    assert_eq!(
        js(
            &mut r,
            "!!document.getElementById('ticks') + ' ' + trace[trace.length - 1]"
        ),
        "false unmount"
    );
    assert_clean(&r);
}

#[test]
fn react18_hydrate_root_adopts_server_markup() {
    let (mut r, _) = load("react18-hydrate.html");
    r.run_until_idle(200);
    assert_clean(&r);
    // No mismatch: nothing recoverable reported, the server's nodes were adopted
    // (not recreated) and the only change is the mount effect's attribute.
    assert_eq!(js(&mut r, "recoverable.length + '|' + [serverNodes.h1 === greeting, serverNodes.inc === inc, serverNodes.li === document.querySelector('#results li'), serverNodes.aside === details, serverNodes.input === document.querySelector('[data-role=filter]')].join()"), "0|true,true,true,true,true");
    assert_eq!(js(&mut r, "String(document.getElementById('root').innerHTML === serverHTML.replace('data-mounted=\"no\"', 'data-mounted=\"yes\"'))"), "true");
    // The hydrated tree is interactive.
    click(&mut r, "inc");
    assert_eq!(js(&mut r, "inc.textContent + '|' + parity.textContent + '|' + parity.style.color + '|' + (serverNodes.inc === inc) + '|' + inc.childNodes.length"), "Clicked 1 times|odd|red|true|5");
    type_text(&mut r, ":R3:", "an");
    assert_eq!(js(&mut r, "[...document.querySelectorAll('#results li')].map(l => l.textContent).join() + '|' + summary.textContent + '|' + agree.checked"), "banana|1 of 3 shown|true");
    assert_clean(&r);
}

// -------------------------------------------------------------------- Vue 3.4

#[test]
fn vue3_global_build_compiles_templates_and_drives_the_dom() {
    let (mut r, _) = load("vue3.html");
    assert_clean(&r);
    // The global build compiles the in-DOM template at runtime, which needs the
    // `Function` constructor; the mount is synchronous and the lifecycle ran.
    assert_eq!(
        js(&mut r, "Vue.version + '|' + typeof Vue.compile"),
        "3.4.38|function"
    );
    assert_eq!(js(&mut r, "[title.textContent, document.title, trace.join(), document.getElementById('form-out').textContent].join(' ~ ')"), "Todos (1 left) ~ Todos 1 ~ item-mounted:1,item-mounted:2,rc-mounted,app-mounted ~ false|red|a||number:1|");
    // `v-if`/`v-else-if`/`v-else`, `v-show` (display, not removal), `:class`/`:style`
    // objects, keyed `v-for` with a scoped slot, and `v-html`.
    assert_eq!(js(&mut r, "[!!document.getElementById('empty'), some.textContent, !!document.getElementById('many')].join()"), "false,A few things,false");
    assert_eq!(js(&mut r, "[getComputedStyle(shown).display, shown.className, shown.style.color, shown.style.fontSize].join('|')"), "none|box|red|14px");
    assert_eq!(js(&mut r, "[...document.querySelectorAll('#todos li')].map(l => l.id + ':' + l.className + ':' + l.dataset.index + ':' + l.textContent).join('|')"), "todo-1::0:writex|todo-2:done:1:testdonex");
    assert_eq!(js(&mut r, "[document.getElementById('html').innerHTML, document.getElementById('vb').textContent, document.getElementById('themed').textContent].join('|')"), "<b id=\"vb\">raw</b>|raw|dark:fallback");
    // Reactivity through a `<form>`: `v-model.trim`, the computed `remaining`, the
    // watcher's log and the post-flush watcher's trace entry.
    type_text(&mut r, "draft", "milk");
    assert_eq!(
        js(&mut r, "[draft.value, add.disabled, vm.draft].join()"),
        "milk,false,milk"
    );
    key(&mut r, "Enter");
    assert_eq!(
        js(
            &mut r,
            "[...document.querySelectorAll('#todos li')].map(l => l.id).join()"
        ),
        "todo-1,todo-2,todo-3"
    );
    assert_eq!(js(&mut r, "[title.textContent, document.title, document.getElementById('watch-log').textContent, trace.slice(-2).join()].join(' ~ ')"), "Todos (2 left) ~ Todos 2 ~ 1>2 ~ len:3,item-mounted:3");
    assert_eq!(
        js(
            &mut r,
            "[draft.value, add.disabled, !!document.getElementById('many')].join()"
        ),
        ",true,true"
    );

    // A keyed `v-for` reordered in place keeps the same DOM nodes.
    let lis = |r: &Realm| -> Vec<(String, NodeId)> {
        let d = r.document();
        d.descendants(id(r, "todos"))
            .filter(|n| d.is(*n, "li"))
            .map(|n| (d.attr(n, "id").unwrap().to_owned(), n))
            .collect()
    };
    let before = lis(&r);
    click(&mut r, "reverse");
    let after = lis(&r);
    assert_eq!(
        after.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
        ["todo-3", "todo-2", "todo-1"]
    );
    assert_eq!(after.iter().rev().cloned().collect::<Vec<_>>(), before);
    assert_eq!(
        js(
            &mut r,
            "[...document.querySelectorAll('#todos li')].map(l => l.dataset.index).join()"
        ),
        "0,1,2"
    );

    // Every `v-model` flavour, driven through the realm's own events.
    assert_eq!(
        click(&mut r, "agree"),
        DefaultAction::Toggle(id(&r, "agree"))
    );
    assert_eq!(
        js(
            &mut r,
            "[getComputedStyle(shown).display, shown.className].join('|')"
        ),
        "block|box active"
    );
    set_value(&mut r, "color", "blue");
    set_value(&mut r, "notes", "hello");
    set_value(&mut r, "age", "42");
    click(&mut r, "r-b");
    click(&mut r, "multi-a");
    click(&mut r, "multi-b");
    assert_eq!(text(&mut r, "form-out"), "true|blue|b|hello|number:42|a+b");
    assert_eq!(js(&mut r, "[shown.style.color, agree.checked, color.value, notes.value, age.value, vm.multi.join('+')].join('|')"), "blue|true|blue|hello|42|a+b");

    // `<teleport>` moves the node under `#modals` but keeps it in the app's tree.
    click(&mut r, "open-modal");
    assert_eq!(
        js(&mut r, "document.getElementById('modals').innerHTML"),
        "<div id=\"modal\">Teleported: Todos</div>"
    );
    click(&mut r, "open-modal");
    assert_eq!(
        js(&mut r, "document.getElementById('modals').innerHTML"),
        "<!---->"
    );

    // `nextTick`: the DOM is stale inside the handler and current after awaiting it.
    // Updating an injected ref re-renders the injecting component.
    click(&mut r, "tick");
    assert_eq!(js(&mut r, "[window.beforeTick, window.afterTick, document.getElementById('themed').textContent].join(' ~ ')"), "14px ~ 16px ~ light:fallback");

    // `<transition>`: Vue adds the enter/leave classes, waits for `transitionend` on
    // the world clock (300 ms, from the stylesheet) and then removes the node.
    js(&mut r, "window.tevents = []; document.addEventListener('transitionend', (e) => tevents.push(e.target.id + ':' + e.propertyName + ':' + e.elapsedTime), true);");
    click(&mut r, "toggle-fade");
    assert_eq!(
        js(&mut r, "document.getElementById('fading').className"),
        "fade-leave-from fade-leave-active"
    );
    // The next frame swaps `-from` for `-to`, which is what starts the transition.
    r.run_until_idle(50);
    assert_eq!(js(&mut r, "[!!document.getElementById('fading'), document.getElementById('fading').className].join(' ~ ')"), "true ~ fade-leave-active fade-leave-to");
    assert_eq!(js(&mut r, "tevents.join()"), "");
    r.run_until_idle(500);
    assert_eq!(
        js(
            &mut r,
            "[!!document.getElementById('fading'), trace.slice(-1)[0], tevents.join()].join(' ~ ')"
        ),
        "false ~ after-leave ~ fading:opacity:0.3"
    );
    click(&mut r, "toggle-fade");
    r.run_until_idle(50);
    assert_eq!(
        js(&mut r, "document.getElementById('fading').className"),
        "fade-enter-active fade-enter-to"
    );
    r.run_until_idle(500);
    assert_eq!(js(&mut r, "[document.getElementById('fading').className, trace.slice(-1)[0], tevents.length].join(' ~ ')"), " ~ after-enter ~ 2");

    // A render-function component with the composition API: props with defaults,
    // a computed, and `onUpdated`.
    click(&mut r, "rc");
    assert_eq!(js(&mut r, "[document.getElementById('rc').textContent, document.getElementById('rc').className, trace.slice(-1)[0]].join(' ~ ')"), "2 / 4 ~  ~ rc-updated:2");
    click(&mut r, "rc");
    assert_eq!(js(&mut r, "[document.getElementById('rc').textContent, document.getElementById('rc').className].join(' ~ ')"), "4 / 8 ~ big");

    // Component events: `$emit` through `@toggle`, and `@click.stop` on the remove
    // button, which must not also toggle the row.
    let toggle = by_class(&r, "todo-1", "text");
    click_node(&mut r, toggle);
    assert_eq!(
        js(
            &mut r,
            "[document.getElementById('todo-1').className, title.textContent].join('|')"
        ),
        "done|Todos (1 left)"
    );
    let rm = by_class(&r, "todo-2", "rm");
    click_node(&mut r, rm);
    assert_eq!(js(&mut r, "[[...document.querySelectorAll('#todos li')].map(l => l.id).join('|'), trace.slice(-1)[0], document.getElementById('todo-3').className].join(' ~ ')"), "todo-3|todo-1 ~ item-unmounted:2 ~ ");
    assert_eq!(
        js(&mut r, "document.getElementById('watch-log').textContent"),
        "1>2,2>1"
    );
    assert_clean(&r);
}

// ------------------------------------------------------- CSS-in-JS at runtime

/// `(sheets, rules, style text length)`: styled-components and emotion both run in
/// "speedy" mode, inserting rules through `CSSStyleSheet.insertRule` into a `<style>`
/// element they never write text into.
fn sheet_state(r: &mut Realm) -> String {
    js(r, "[...document.styleSheets].map((s) => s.ownerNode.tagName + ':' + s.cssRules.length + ':' + s.ownerNode.textContent.length).join(',')")
}

#[test]
fn styled_components_6_inserts_rules_at_runtime_and_the_cascade_follows() {
    let (mut r, _) = load("styled-components.html");
    assert_clean(&r);
    // One `<style data-styled=active>`, empty of text, holding every rule: the
    // component classes, `createGlobalStyle`'s rules and the `@keyframes` block.
    assert_eq!(sheet_state(&mut r), "STYLE:8:0");
    assert_eq!(js(&mut r, "document.styleSheets[0].ownerNode.getAttribute('data-styled') + '|' + document.styleSheets[0].ownerNode.getAttribute('data-styled-version')"), "active|6.1.13");
    assert_eq!(js(&mut r, "[...document.styleSheets[0].cssRules].filter((x) => x.cssText.startsWith('@keyframes')).map((x) => x.cssText.slice(0, 22)).join()"), "@keyframes dHAcpB { fr");
    // `createGlobalStyle` reaches the document, and the generated classes compute the
    // same values Chromium computes for this page.
    assert_eq!(
        computed(&mut r, "box", "background-color"),
        "rgb(128, 0, 0)"
    );
    assert_eq!(js(&mut r, "const c = getComputedStyle(box); [box.className, c.color, c.animationName, c.animationDuration, c.width, c.height].join('|')"), "sc-blHHSb jXlNeX|rgb(255, 255, 255)|dHAcpB|2s|120px|40px");
    assert_eq!(js(&mut r, "getComputedStyle(document.body).backgroundColor + '|' + getComputedStyle(document.documentElement).boxSizing"), "rgb(255, 255, 240)|border-box");
    assert_eq!(painted_fill(&mut r, "box"), "128,0,0,255");

    // A prop change generates a new class and a new rule; the component's static
    // `sc-` id is stable, the hashed half is not.
    click(&mut r, "toggle-box");
    assert_eq!(sheet_state(&mut r), "STYLE:9:0");
    assert_eq!(js(&mut r, "box.className + '|' + getComputedStyle(box).backgroundColor + '|' + getComputedStyle(box).animationName"), "sc-blHHSb hBYtQz|rgb(0, 128, 0)|dHAcpB");
    assert_eq!(painted_fill(&mut r, "box"), "0,128,0,255");

    // Clicking swaps the button's generated class; clicking back reuses the first one
    // (the hash is deterministic), so no rule is added the second time.
    let first = js(&mut r, "document.getElementById('color-button').className");
    click(&mut r, "color-button");
    assert_eq!(sheet_state(&mut r), "STYLE:10:0");
    assert_eq!(js(&mut r, "const b = document.getElementById('color-button'); const c = getComputedStyle(b); [b.className, c.backgroundColor, c.boxShadow, b.textContent].join('|')"), "sc-gtLWhw izKDIt|rgb(200, 0, 120)|rgba(0, 0, 0, 0.4) 0px 2px 0px 0px|click me 1");
    assert_eq!(painted_fill(&mut r, "color-button"), "200,0,120,255");
    click(&mut r, "color-button");
    assert_eq!(sheet_state(&mut r), "STYLE:10:0");
    assert_eq!(js(&mut r, "document.getElementById('color-button').className + '|' + getComputedStyle(document.getElementById('color-button')).boxShadow"), format!("{first}|none"));

    // `ThemeProvider`/`useTheme`, `styled(Component)` and `.attrs`.
    assert_eq!(js(&mut r, "const t = getComputedStyle(document.getElementById('themed')); [t.color, t.borderLeftColor, t.borderLeftWidth, document.getElementById('theme-readout').textContent].join('|')"), "rgb(20, 20, 20)|rgb(0, 96, 200)|4px|rgb(20, 20, 20)/rgb(0, 96, 200)");
    assert_eq!(js(&mut r, "const e = getComputedStyle(document.getElementById('ext-label')); [e.fontWeight, e.textTransform].join('|')"), "700|uppercase");
    assert_eq!(js(&mut r, "const i = document.getElementById('attrs-input'); [i.type, i.getAttribute('data-attrs'), i.placeholder, getComputedStyle(i).borderTopWidth].join('|')"), "text|yes|via attrs|1px");
    assert_clean(&r);
}

#[test]
fn emotion_11_inserts_rules_at_runtime_and_the_cascade_follows() {
    let (mut r, _) = load("emotion.html");
    assert_clean(&r);
    // Two sheets: `@emotion/css`'s own and the one `injectGlobal`/`<Global>` write to.
    assert_eq!(sheet_state(&mut r), "STYLE:9:0,STYLE:1:0");
    assert_eq!(
        js(
            &mut r,
            "[...document.styleSheets].map((s) => s.ownerNode.getAttribute('data-emotion')).join()"
        ),
        "css,css-global"
    );
    assert_eq!(js(&mut r, "const c = getComputedStyle(box); [box.className, c.backgroundColor, c.color, c.width, c.height].join('|')"), "css-6xtlcj|rgb(128, 0, 0)|rgb(255, 255, 255)|120px|40px");
    assert_eq!(js(&mut r, "const s = getComputedStyle(document.getElementById('spinner')); [document.getElementById('spinner').className, s.animationName, s.animationDuration, s.borderTopColor, s.borderRadius].join('|')"), "css-5epojb|animation-w6tfot|1.2s|rgba(0, 0, 0, 0)|50%");
    assert_eq!(js(&mut r, "getComputedStyle(document.body).backgroundColor + '|' + getComputedStyle(document.documentElement).boxSizing"), "rgb(255, 255, 240)|border-box");
    assert_eq!(painted_fill(&mut r, "box"), "128,0,0,255");

    click(&mut r, "toggle-box");
    assert_eq!(
        js(
            &mut r,
            "box.className + '|' + getComputedStyle(box).backgroundColor"
        ),
        "css-1h4ce1a|rgb(0, 128, 0)"
    );
    assert_eq!(painted_fill(&mut r, "box"), "0,128,0,255");

    let first = js(&mut r, "document.getElementById('color-button').className");
    click(&mut r, "color-button");
    assert_eq!(js(&mut r, "const b = document.getElementById('color-button'); const c = getComputedStyle(b); [b.className, c.backgroundColor, c.boxShadow, b.textContent].join('|')"), "css-zvrajx|rgb(200, 0, 120)|rgba(0, 0, 0, 0.4) 0px 2px 0px 0px|click me 1");
    assert_eq!(painted_fill(&mut r, "color-button"), "200,0,120,255");
    click(&mut r, "color-button");
    assert_eq!(js(&mut r, "document.getElementById('color-button').className + '|' + getComputedStyle(document.getElementById('color-button')).boxShadow"), format!("{first}|none"));
    // Five more rules than at load, and still no text in either `<style>`.
    assert_eq!(sheet_state(&mut r), "STYLE:14:0,STYLE:1:0");

    assert_eq!(js(&mut r, "const t = getComputedStyle(document.getElementById('themed')); [document.getElementById('themed').className, t.color, t.borderLeftColor, t.borderLeftWidth, document.getElementById('theme-readout').textContent].join('|')"), "css-tmcble|rgb(20, 20, 20)|rgb(0, 96, 200)|4px|#141414/#0060c8");
    assert_clean(&r);
}

// -------------------------------------------------------------------- Svelte 4

/// The ids of the elements matching `sel`, in order.
fn ids(r: &mut Realm, sel: &str) -> String {
    js(
        r,
        &format!("[...document.querySelectorAll('{sel}')].map((n) => n.id).join('|')"),
    )
}

#[test]
fn svelte4_compiled_components_run_stores_events_and_transitions() {
    // `crates/web/tools/build-svelte.mjs` compiles `tests/script/svelte/*.svelte` with
    // the pinned 4.2.19 compiler and bundles them with the Svelte runtime; the page
    // loads only that build.
    let (mut r, _) = load("svelte4.html");
    assert_clean(&r);
    assert_eq!(js(&mut r, "[title.textContent, document.getElementById('sv-some').textContent, document.getElementById('sv-shared-count').textContent].join(' ~ ')".replace("title.textContent", "document.getElementById('sv-title').textContent").as_str()), "Todos (2 left) ~ A few things left ~ shared count: 0 (doubled 0)");
    assert_eq!(ids(&mut r, "#sv-list li"), "sv-item-1|sv-item-2|sv-item-3");
    // Lifecycle order: `beforeUpdate` before the first mount, `onMount` child first.
    assert_eq!(
        js(&mut r, "trace.join()"),
        "remaining:2,todos:before-update,counter:mount,todos:mount,todos:after-update"
    );
    assert_eq!(js(&mut r, "[document.getElementById('sv-item-1-check').checked, document.getElementById('sv-item-2-check').checked, document.getElementById('sv-item-2').className].join()"), "false,true,done");

    // `bind:value` and a reactive statement: the button follows the bound value.
    type_text(&mut r, "sv-new-input", "Buy milk");
    assert_eq!(js(&mut r, "[document.getElementById('sv-new-input').value, document.getElementById('sv-add').disabled].join()"), "Buy milk,false");
    // `on:submit|preventDefault` keeps the browser out of it.
    assert_eq!(click(&mut r, "sv-add"), DefaultAction::Prevented);
    assert_eq!(
        ids(&mut r, "#sv-list li"),
        "sv-item-1|sv-item-2|sv-item-3|sv-item-4"
    );
    assert_eq!(js(&mut r, "[document.getElementById('sv-title').textContent, document.getElementById('sv-item-4-text').textContent, document.getElementById('sv-new-input').value, document.getElementById('sv-add').disabled, trace.slice(-4).join()].join(' ~ ')"), "Todos (3 left) ~ Buy milk ~  ~ true ~ add:Buy milk,remaining:3,todos:before-update,todos:after-update");
    // `on:keydown` with a key guard.
    type_text(&mut r, "sv-new-input", "temp");
    key(&mut r, "Escape");
    assert_eq!(
        js(
            &mut r,
            "[document.getElementById('sv-new-input').value, trace.includes('escape')].join(' ~ ')"
        ),
        " ~ true"
    );

    // `bind:checked` on the rows and on the compact switch.
    click(&mut r, "sv-item-2-check");
    click(&mut r, "sv-item-1-check");
    assert_eq!(js(&mut r, "[document.getElementById('sv-item-1-check').checked, document.getElementById('sv-item-1').className, document.getElementById('sv-item-2').className, document.getElementById('sv-title').textContent].join(' ~ ')"), "true ~ done ~  ~ Todos (3 left)");
    click(&mut r, "sv-compact");
    assert_eq!(js(&mut r, "[document.getElementById('sv-compact').checked, document.getElementById('sv-list').className].join(' ~ ')"), "true ~ compact");

    // The keyed `{#each}` filters and reorders; a reorder keeps the DOM nodes.
    click(&mut r, "sv-filter-active");
    assert_eq!(ids(&mut r, "#sv-list li"), "sv-item-2|sv-item-3|sv-item-4");
    click(&mut r, "sv-filter-done");
    assert_eq!(ids(&mut r, "#sv-list li"), "sv-item-1");
    click(&mut r, "sv-filter-all");
    let before: Vec<NodeId> = {
        let d = r.document();
        d.descendants(id(&r, "sv-list"))
            .filter(|n| d.is(*n, "li"))
            .collect()
    };
    click(&mut r, "sv-reverse");
    let after: Vec<NodeId> = {
        let d = r.document();
        d.descendants(id(&r, "sv-list"))
            .filter(|n| d.is(*n, "li"))
            .collect()
    };
    assert_eq!(
        ids(&mut r, "#sv-list li"),
        "sv-item-4|sv-item-3|sv-item-2|sv-item-1"
    );
    assert_eq!(after.iter().rev().copied().collect::<Vec<_>>(), before);

    // The shared `writable` store, its `derived`, `tick()` and the component event.
    for _ in 0..5 {
        click(&mut r, "sv-inc");
    }
    assert_eq!(js(&mut r, "[document.getElementById('sv-counter-value').textContent, document.getElementById('sv-counter-double').textContent, document.getElementById('sv-shared-count').textContent, document.getElementById('sv-last-milestone').textContent].join(' ~ ')"), "10 ~ (x2 = 20) ~ shared count: 10 (doubled 20) ~ last milestone: 10");
    // `tick()` resolves after the DOM caught up, and `createEventDispatcher` reached
    // the parent's `on:milestone`.
    assert_eq!(
        js(&mut r, "trace.filter((t) => t.startsWith('tick:')).join()"),
        "tick:0->2,tick:2->4,tick:4->6,tick:6->8,tick:8->10"
    );
    assert_eq!(
        js(
            &mut r,
            "trace.filter((t) => t.includes('milestone')).join()"
        ),
        "todos:milestone:10,counter:milestone:10"
    );
    click(&mut r, "sv-dec");
    assert_eq!(js(&mut r, "[document.getElementById('sv-counter-value').textContent, document.getElementById('sv-shared-count').textContent, document.getElementById('sv-last-milestone').textContent].join(' ~ ')"), "8 ~ shared count: 8 (doubled 16) ~ last milestone: 10");
    // The default slot the parent filled.
    assert_eq!(text(&mut r, "sv-counter-slot"), "extra widget info");

    click(&mut r, "sv-item-4-remove");
    assert_eq!(ids(&mut r, "#sv-list li"), "sv-item-3|sv-item-2|sv-item-1");
    assert_eq!(
        js(&mut r, "trace.slice(-4).join()"),
        "remove:4,remaining:2,todos:before-update,todos:after-update"
    );

    // `transition:fade` writes a runtime `@keyframes` rule through `insertRule` into a
    // `<style>` it adds, drives it from `requestAnimationFrame`, and takes the sheet
    // away when the transition ends.
    assert_eq!(js(&mut r, "document.styleSheets.length"), "0");
    click(&mut r, "sv-note-toggle");
    let rule = js(&mut r, "document.styleSheets[0].cssRules[0].cssText.replace(/__svelte_-?\\d+_/, 'K').replace(/\\s+/g, ' ')");
    assert!(
        rule.starts_with("@keyframes K0 { 0% { opacity: 0; }")
            && rule.ends_with("100% { opacity: 1; } }"),
        "{rule}"
    );
    assert_eq!(js(&mut r, "[!!document.getElementById('sv-note'), document.styleSheets.length, document.styleSheets[0].cssRules.length, document.styleSheets[0].ownerNode.tagName, document.getElementById('sv-note').style.animation.replace(/__svelte_-?\\d+_/, 'K')].join(' ~ ')"), "true ~ 1 ~ 1 ~ STYLE ~ K0 200ms linear 0ms 1 both");
    r.run_until_idle(300);
    assert_eq!(js(&mut r, "[!!document.getElementById('sv-note'), getComputedStyle(document.getElementById('sv-note')).opacity, document.styleSheets.length, document.getElementById('sv-note').style.animation].join(' ~ ')"), "true ~ 1 ~ 0 ~ ");
    // The outro fades out and then removes the node, on the world clock.
    click(&mut r, "sv-note-toggle");
    assert_eq!(
        js(
            &mut r,
            "[!!document.getElementById('sv-note'), document.styleSheets.length].join(' ~ ')"
        ),
        "true ~ 1"
    );
    r.run_until_idle(400);
    assert_eq!(
        js(
            &mut r,
            "[!!document.getElementById('sv-note'), document.styleSheets.length].join(' ~ ')"
        ),
        "false ~ 0"
    );
    assert_clean(&r);
}

// ------------------------------------------------------------------- The sweep

/// Globals a bundle might look for and this engine does not have. A page that reads
/// one of these has taken a feature-detection branch, so the sweep records it.
const PROBES: &[&str] = &[
    // Schedulers and host hooks.
    "setImmediate",
    "requestIdleCallback",
    "MessageChannel",
    "queueMicrotask",
    "MSApp",
    "chrome",
    "trustedTypes",
    "WebKitMutationObserver",
    "webkitRequestAnimationFrame",
    "mozRequestAnimationFrame",
    "msRequestAnimationFrame",
    "ActiveXObject",
    "msCrypto",
    // Non-browser hosts the UMD wrappers sniff for.
    "process",
    "global",
    "Deno",
    "Bun",
    "define",
    "exports",
    "module",
    "require",
    "__webpack_require__",
    // Devtools and test hooks.
    "__REACT_DEVTOOLS_GLOBAL_HOOK__",
    "__VUE_DEVTOOLS_GLOBAL_HOOK__",
    "__VUE_PROD_DEVTOOLS__",
    "__VUE_OPTIONS_API__",
    "__VUE_PROD_HYDRATION_MISMATCH_DETAILS__",
    "__SVELTE_DEVTOOLS_GLOBAL_HOOK__",
    "__svelte",
    "IS_REACT_ACT_ENVIRONMENT",
    "jest",
    "jasmine",
    "SC_DISABLE_SPEEDY",
    "SC_ATTR",
    "REACT_APP_SC_ATTR",
    // Platform APIs a minimal engine might not carry.
    "IntersectionObserver",
    "ResizeObserver",
    "ReportingObserver",
    "scheduler",
    "reportError",
    "WeakRef",
    "FinalizationRegistry",
    "structuredClone",
    "AbortController",
    "CSS",
    "HTMLIFrameElement",
    "ShadowRoot",
    "customElements",
];

/// Installs a recording getter for each probe the realm does not already have, then
/// loads the page. Anything the bundles then read is a feature detection that came up
/// empty. (The getter makes `name in window` true where it was false; every bundle
/// here uses `typeof`, which still answers `undefined`.)
fn probe_page(name: &str) -> (Realm, Vec<String>) {
    let html = manifest(&format!("script/{name}"));
    let probe = format!(
        "<script>window.__probed = [];\nfor (const n of {}) {{\n  if (n in window) continue;\n  Object.defineProperty(window, n, {{ configurable: true, get() {{ if (!window.__probed.includes(n)) window.__probed.push(n); return undefined; }} }});\n}}</script>",
        serde_json::to_string(PROBES).unwrap()
    );
    let marker = "<head>";
    assert!(
        html.contains(marker),
        "{name}: no <head> to inject the probe into"
    );
    let html = html.replacen(marker, &format!("{marker}\n{probe}"), 1);
    let mut r = Realm::new(&html, &format!("{BASE}{name}"), Box::new(host()));
    r.run_document();
    r.run_until_idle(300);
    let probed = js(&mut r, "__probed.join(',')");
    let probed = if probed.is_empty() {
        Vec::new()
    } else {
        probed.split(',').map(str::to_owned).collect()
    };
    (r, probed)
}

/// Every bundle, loaded once with the console captured at every level and with a
/// recorder on the globals this engine does not provide. Nothing may be logged, and
/// the feature detections that come up empty are pinned here so a change to the
/// engine's surface shows up as a diff rather than as silently different behaviour.
#[test]
fn every_bundle_loads_with_a_clean_console_and_a_known_set_of_feature_misses() {
    // Every miss below is a branch the bundle took because this engine has no such
    // global: the UMD wrappers falling through `exports`/`module`/`define` to the
    // browser global, React's devtools hook and `setImmediate`/`MSApp` scheduler
    // fallbacks, styled-components' `process.env`/`SC_DISABLE_SPEEDY` switches (so it
    // stays in speedy mode), and Svelte's `__svelte` devtools namespace. None of them
    // changed what the page rendered.
    let expected: &[(&str, &str)] = &[
        ("jquery.html", "module,define"),
        ("preact.html", "exports,define"),
        ("react18.html", "exports,define,setImmediate,MSApp,__REACT_DEVTOOLS_GLOBAL_HOOK__"),
        ("react17.html", "exports,define,MSApp,__REACT_DEVTOOLS_GLOBAL_HOOK__"),
        ("react18-hydrate.html", "exports,define,setImmediate,MSApp,__REACT_DEVTOOLS_GLOBAL_HOOK__"),
        ("vue3.html", ""),
        ("svelte4.html", "__svelte"),
        ("styled-components.html", "exports,define,setImmediate,MSApp,__REACT_DEVTOOLS_GLOBAL_HOOK__,process,SC_DISABLE_SPEEDY"),
        ("emotion.html", "exports,define,setImmediate,MSApp,__REACT_DEVTOOLS_GLOBAL_HOOK__"),
    ];
    let mut report = String::new();
    let mut misses: Vec<(String, String)> = Vec::new();
    for (name, _) in expected {
        let (r, probed) = probe_page(name);
        let logs = all_logs(&r);
        report.push_str(&format!("{name}: probed [{}]\n", probed.join(",")));
        assert!(
            logs.is_empty(),
            "{name} wrote to the console at load:\n{logs}"
        );
        misses.push(((*name).to_owned(), probed.join(",")));
    }
    eprintln!("{report}");
    let got: Vec<(&str, &str)> = misses
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    assert_eq!(
        got,
        expected.to_vec(),
        "the feature detections that come up empty changed"
    );
}
