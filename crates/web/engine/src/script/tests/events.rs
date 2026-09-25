use super::*;
use crate::check;
use crate::dom::NodeId;
use crate::script::{DefaultAction, Modifiers, UiEvent};

fn id_of(r: &Realm, id: &str) -> NodeId {
    r.document().by_id(id)[0]
}

fn click_id(r: &mut Realm, id: &str) -> DefaultAction {
    let n = id_of(r, id);
    r.dispatch(UiEvent::ClickNode {
        node: n,
        modifiers: Modifiers::default(),
        detail: 1,
    })
}

check!(add_remove_dispatch, "<div id=d></div>", "const d=document.getElementById('d'); let n=0; const f=()=>n++; d.addEventListener('x', f); d.addEventListener('x', f); d.dispatchEvent(new Event('x')); d.removeEventListener('x', f); d.dispatchEvent(new Event('x')); console.log(n, d.dispatchEvent(new Event('y')), d.dispatchEvent(new Event('z', {cancelable:true})));", "1 true true");
check!(event_phases_and_order, "<div id=a><p id=b><span id=c>t</span></p></div>", "const log=[]; for (const id of ['a','b','c']) { const el=document.getElementById(id); el.addEventListener('e', ev=>log.push(id+'c'+ev.eventPhase), true); el.addEventListener('e', ev=>log.push(id+'b'+ev.eventPhase)); } window.addEventListener('e', ()=>log.push('w'), true); document.addEventListener('e', ()=>log.push('d')); document.getElementById('c').dispatchEvent(new Event('e', {bubbles:true})); console.log(log.join()); log.length=0; document.getElementById('c').dispatchEvent(new Event('e')); console.log(log.join());", "w,ac1,bc1,cc2,cb2,bb3,ab3,d\nw,ac1,bc1,cc2,cb2");
check!(stop_propagation, "<div id=a><p id=b></p></div>", "const a=document.getElementById('a'), b=document.getElementById('b'); const log=[]; b.addEventListener('e', ev=>{ log.push('b1'); ev.stopPropagation(); }); b.addEventListener('e', ()=>log.push('b2')); a.addEventListener('e', ()=>log.push('a')); b.dispatchEvent(new Event('e', {bubbles:true})); b.addEventListener('e', ev=>{ ev.stopImmediatePropagation(); log.push('b0'); }, true); b.dispatchEvent(new Event('e', {bubbles:true})); console.log(log.join(), new Event('e').cancelBubble);", "b1,b2,b0 false");
check!(prevent_default_and_return_value, "<div id=a></div>", "const a=document.getElementById('a'); a.addEventListener('e', e=>{ e.preventDefault(); }); const e=new Event('e', {cancelable:true}); console.log(a.dispatchEvent(e), e.defaultPrevented, e.returnValue); const e2=new Event('e'); a.dispatchEvent(e2); console.log(e2.defaultPrevented); a.onclick=()=>false; const e3=new Event('click', {cancelable:true}); console.log(a.dispatchEvent(e3), e3.defaultPrevented);", "false true false\nfalse\nfalse true");
check!(event_properties, "<div id=a><p id=b></p></div>", "const b=document.getElementById('b'); const e=new CustomEvent('c', {detail:{k:1}, bubbles:true, composed:true}); let inside; b.addEventListener('c', ev=>{ inside=[ev.target.id, ev.currentTarget.id, ev.eventPhase, ev.composedPath().length, ev.composedPath()[0]===b, ev.isTrusted]; }); b.dispatchEvent(e); console.log(inside.join(), e.type, e.detail.k, e.bubbles, e.cancelable, e.composed, e.target.id, e.currentTarget, e.eventPhase, typeof e.timeStamp, e instanceof Event, Object.prototype.toString.call(e), e.srcElement===b); try { new Event() } catch(err) { console.log(err.name) } try { b.dispatchEvent({}) } catch(err) { console.log(err.name) }", "b,b,2,6,true,false c 1 true false true b null 0 number true [object CustomEvent] true\nTypeError\nTypeError");
check!(listener_options, "<div id=a></div>", "const a=document.getElementById('a'); let n=0; a.addEventListener('e', ()=>n++, {once:true}); a.dispatchEvent(new Event('e')); a.dispatchEvent(new Event('e')); const c=new AbortController(); a.addEventListener('e', ()=>n+=10, {signal:c.signal}); a.dispatchEvent(new Event('e')); c.abort(); a.dispatchEvent(new Event('e')); a.addEventListener('e', e=>{ e.preventDefault(); n+=100; }, {passive:true}); const e=new Event('e', {cancelable:true}); a.dispatchEvent(e); console.log(n, e.defaultPrevented); a.addEventListener('h', {handleEvent(ev){ n+=1000; console.log(this===o, ev.type); }}); const o=a; a.dispatchEvent(new Event('h')); a.addEventListener('e', null); console.log(n);", "111 false\nfalse h\n1111");
check!(on_handler_attributes, "<div id=a onclick=\"console.log('inline', this.id, event.type, typeof event)\"><b id=b></b></div>", "const a=document.getElementById('a'); console.log(typeof a.onclick, a.onclick.length>=0, a.onclick===a.onclick); a.dispatchEvent(new Event('click', {bubbles:true})); a.onclick=null; a.dispatchEvent(new Event('click')); const order=[]; a.addEventListener('keydown', ()=>order.push(1)); a.onkeydown=()=>order.push(2); a.addEventListener('keydown', ()=>order.push(3)); a.onkeydown=()=>order.push('2b'); a.dispatchEvent(new Event('keydown')); console.log(order.join(), a.onkeydown !== null, document.getElementById('b').onclick, window.onload, typeof document.onreadystatechange);", "function true true\ninline a click object\n1,2b,3 true null null object");
check!(window_event_target, "", "let n=0; window.addEventListener('x', ()=>n++); window.dispatchEvent(new Event('x')); console.log(n, window instanceof EventTarget, window instanceof Window, self===window, top===window, parent===window, frames===window, window.length, typeof window.addEventListener, Object.prototype.toString.call(window)); window.onmessage=()=>n+=10; dispatchEvent(new Event('message')); console.log(n); document.body.onload=()=>{}; console.log(window.onload===document.body.onload);", "1 true true true true true true 0 function [object Window]\n11\ntrue");
check!(event_constructors, "", "const m=new MouseEvent('click', {clientX:5, clientY:6, button:2, ctrlKey:true, relatedTarget:document.body, detail:2}); console.log(m.clientX, m.pageX, m.x, m.button, m.buttons, m.ctrlKey, m.getModifierState('Control'), m.getModifierState('Shift'), m.relatedTarget===document.body, m.detail, m instanceof UIEvent, m.view===window); const p=new PointerEvent('pointerdown', {pointerId:3, pointerType:'pen'}); console.log(p.pointerId, p.pointerType, p.isPrimary, p instanceof MouseEvent); const k=new KeyboardEvent('keydown', {key:'a', code:'KeyA', repeat:true, shiftKey:true}); console.log(k.key, k.code, k.keyCode, k.which, k.repeat, k.shiftKey, k.location, new KeyboardEvent('keydown', {key:'Enter'}).keyCode, new KeyboardEvent('keypress', {key:'a'}).charCode); const i=new InputEvent('input', {data:'x', inputType:'insertText'}); console.log(i.data, i.inputType, i.isComposing); const f=new FocusEvent('focus', {relatedTarget:document.body}); console.log(f.relatedTarget.tagName); const w=new WheelEvent('wheel', {deltaY:3}); console.log(w.deltaY, w.deltaMode); const pr=new ProgressEvent('load', {loaded:1,total:2,lengthComputable:true}); console.log(pr.loaded, pr.total); console.log(new PopStateEvent('popstate', {state:{a:1}}).state.a, new HashChangeEvent('hashchange', {oldURL:'a',newURL:'b'}).newURL, new SubmitEvent('submit', {submitter:document.body}).submitter.tagName, new TransitionEvent('transitionend', {propertyName:'opacity'}).propertyName, new AnimationEvent('animationend', {animationName:'n'}).animationName, new ErrorEvent('error', {message:'m', lineno:3}).lineno, new PromiseRejectionEvent('unhandledrejection', {promise:Promise.resolve(), reason:'r'}).reason, new MessageEvent('message', {data:1, origin:'o'}).origin, new StorageEvent('storage', {key:'k'}).key, new ClipboardEvent('copy').type, new DragEvent('drag').dataTransfer, new TouchEvent('touchstart').touches.length, new FormDataEvent('formdata', {formData:new FormData()}).formData instanceof FormData); try { new PromiseRejectionEvent('x') } catch(e) { console.log(e.name) }", "5 5 5 2 0 true true false true 2 true false\n3 pen true true\na KeyA 65 65 true true 0 13 97\nx insertText false\nBODY\n3 0\n1 2\n1 b BODY opacity n 3 r o k copy null 0 true\nTypeError");
check!(uncaught_error_event, "", "window.addEventListener('error', e=>{ console.log('caught', e.message, e.error instanceof RangeError, e instanceof ErrorEvent); e.preventDefault(); }); setTimeout(()=>{ throw new RangeError('boom'); }, 0);", "caught RangeError: boom true true");
check!(unhandled_rejection_event, "", "window.addEventListener('unhandledrejection', e=>{ console.log('rej', e.reason, e.promise instanceof Promise); e.preventDefault(); }); Promise.reject('why');", "rej why true");

#[test]
fn uncaught_exception_logs_error() {
    let r = run("", "window.onerror = (m, f, l, c, e) => { console.log('handler', typeof m, e instanceof TypeError); }; setTimeout(() => { undefinedFn(); }, 0);");
    assert_eq!(logs(&r), "handler string false");
    assert!(
        errors(&r).contains("ReferenceError: undefinedFn is not defined"),
        "{}",
        errors(&r)
    );
}

#[test]
fn click_dispatch_sequence_and_link_navigation() {
    let mut r = run("<a id=a href='/next'>go</a><div id=d></div>", "const log=[]; for (const t of ['pointerdown','mousedown','pointerup','mouseup','click','focus','focusin']) document.getElementById('a').addEventListener(t, e=>log.push(t+':'+e.target.id+':'+e.isTrusted+':'+(e.button|0))); window.log=log;");
    let action = click_id(&mut r, "a");
    assert_eq!(
        action,
        DefaultAction::Navigate("https://example.test/next".into())
    );
    let out = r.eval("log.join()").unwrap();
    assert_eq!(out, "pointerdown:a:true:0,mousedown:a:true:0,focus:a:true:0,focusin:a:true:0,pointerup:a:true:0,mouseup:a:true:0,click:a:true:0");
    assert_eq!(r.focused(), Some(id_of(&r, "a")));
    // A prevented click keeps the browser from navigating.
    r.eval("document.getElementById('a').addEventListener('click', e => e.preventDefault())")
        .unwrap();
    assert_eq!(click_id(&mut r, "a"), DefaultAction::Prevented);
    // A click on plain content focuses nothing and reports None.
    assert_eq!(click_id(&mut r, "d"), DefaultAction::None);
    assert_eq!(r.focused(), None);
}

#[test]
fn click_by_coordinates_hit_tests() {
    let mut r = run("<div id=top style='height:50px'></div><button id=b style='display:block;width:100px;height:30px'>b</button>", "window.hits=[]; document.addEventListener('click', e=>hits.push(e.target.id+'@'+e.clientX+','+e.clientY+' off'+e.offsetX+','+e.offsetY));");
    let a = r.dispatch(UiEvent::Click {
        x: 20,
        y: 60,
        button: 0,
        modifiers: Modifiers::default(),
        detail: 1,
    });
    assert_eq!(a, DefaultAction::Focus(id_of(&r, "b")));
    r.dispatch(UiEvent::Click {
        x: 20,
        y: 10,
        button: 0,
        modifiers: Modifiers::default(),
        detail: 1,
    });
    assert_eq!(
        r.eval("hits.join()").unwrap(),
        "b@20,60 off12,2,top@20,10 off12,2"
    );
    assert_eq!(r.eval("document.elementFromPoint(20, 60).id + ' ' + document.elementFromPoint(20, 10).id + ' ' + document.elementFromPoint(-5, 0)").unwrap(), "b top null");
}

#[test]
fn dblclick_and_buttons() {
    let mut r = run("<div id=d style='height:40px'></div>", "window.ev=[]; for (const t of ['click','dblclick','auxclick','contextmenu']) document.addEventListener(t, e=>ev.push(t+e.detail+':'+e.button));");
    r.dispatch(UiEvent::Click {
        x: 5,
        y: 5,
        button: 0,
        modifiers: Modifiers::default(),
        detail: 2,
    });
    r.dispatch(UiEvent::Click {
        x: 5,
        y: 5,
        button: 1,
        modifiers: Modifiers::default(),
        detail: 1,
    });
    r.dispatch(UiEvent::Click {
        x: 5,
        y: 5,
        button: 2,
        modifiers: Modifiers::default(),
        detail: 1,
    });
    assert_eq!(
        r.eval("ev.join()").unwrap(),
        "click2:0,dblclick2:0,auxclick1:1,contextmenu0:2"
    );
}

#[test]
fn checkbox_and_radio_toggle_with_change_events() {
    let mut r = run("<input id=c type=checkbox><input id=r1 type=radio name=g><input id=r2 type=radio name=g checked><label id=l for=c>lab</label>", "window.ev=[]; for (const id of ['c','r1']) for (const t of ['click','input','change']) document.getElementById(id).addEventListener(t, e=>ev.push(id+':'+t+':'+e.target.checked));");
    assert_eq!(click_id(&mut r, "c"), DefaultAction::Toggle(id_of(&r, "c")));
    assert_eq!(
        r.eval("ev.join() + ' ' + c.checked").unwrap(),
        "c:click:true,c:input:true,c:change:true true"
    );
    assert_eq!(
        click_id(&mut r, "r1"),
        DefaultAction::Toggle(id_of(&r, "r1"))
    );
    assert_eq!(
        r.eval("[r1.checked, r2.checked, r1.matches(':checked')].join()")
            .unwrap(),
        "true,false,true"
    );
    // Prevented click reverts the toggle.
    r.eval("ev.length=0; c.addEventListener('click', e=>e.preventDefault());")
        .unwrap();
    assert_eq!(click_id(&mut r, "c"), DefaultAction::Prevented);
    assert_eq!(
        r.eval("ev.join() + ' ' + c.checked").unwrap(),
        "c:click:false true"
    );
    // Label forwards the click to its control.
    r.eval("ev.length=0; c.removeEventListener; c.checked=false;")
        .unwrap();
    let a = click_id(&mut r, "l");
    assert!(matches!(a, DefaultAction::Prevented), "{a:?}");
}

#[test]
fn form_submission_default_action_and_prevent() {
    let mut r = run("<form id=f action='/post' method=post enctype='multipart/form-data'><input name=a value=1><input name=b value=2 disabled><input id=s type=submit name=btn value=Go></form><form id=g><input name=q value=x type=hidden><input id=t type=text></form>", "window.subs=[]; document.getElementById('f').addEventListener('submit', e=>subs.push('f:'+(e.submitter&&e.submitter.id)+':'+e.isTrusted));");
    let f = id_of(&r, "f");
    let a = click_id(&mut r, "s");
    assert_eq!(
        a,
        DefaultAction::Submit {
            form: f,
            action: "https://example.test/post".into(),
            method: "post".into(),
            enctype: "multipart/form-data".into(),
            data: vec![("a".into(), "1".into()), ("btn".into(), "Go".into())]
        }
    );
    assert_eq!(r.eval("subs.join()").unwrap(), "f:s:true");
    r.eval("f.addEventListener('submit', e => e.preventDefault())")
        .unwrap();
    assert_eq!(click_id(&mut r, "s"), DefaultAction::Prevented);
    // Enter in the one text field of a form without a submit button submits it.
    r.dispatch(UiEvent::Focus {
        node: Some(id_of(&r, "t")),
    });
    let a = r.dispatch(UiEvent::Key {
        key: "Enter".into(),
        code: String::new(),
        modifiers: Modifiers::default(),
        repeat: false,
    });
    assert_eq!(
        a,
        DefaultAction::Submit {
            form: id_of(&r, "g"),
            action: "https://example.test/page.html".into(),
            method: "get".into(),
            enctype: "application/x-www-form-urlencoded".into(),
            data: vec![("q".into(), "x".into())]
        }
    );
}

#[test]
fn implicit_submission_clicks_the_default_button_without_moving_focus() {
    // HTML's implicit submission fires a click event at the default button; it is
    // not a pointer interaction, so the caret stays in the field and no blur runs
    // before the submit (Chromium behaves the same).
    let mut r = run(
        "<form id=f><input id=t type=text><button id=b type=button>no</button><button id=s>Go</button></form>",
        "window.log=[]; for (const t of ['pointerdown','mousedown','pointerup','mouseup','click','focus','blur']) for (const id of ['t','s']) document.getElementById(id).addEventListener(t, e=>log.push(t+':'+e.target.id+':'+e.detail)); document.getElementById('f').addEventListener('submit', e=>{ log.push('submit:'+(e.submitter&&e.submitter.id)); e.preventDefault(); });",
    );
    r.dispatch(UiEvent::Focus {
        node: Some(id_of(&r, "t")),
    });
    r.eval("log.length = 0").unwrap();
    assert_eq!(
        r.dispatch(UiEvent::Key {
            key: "Enter".into(),
            code: String::new(),
            modifiers: Modifiers::default(),
            repeat: false
        }),
        DefaultAction::Prevented
    );
    assert_eq!(r.eval("log.join()").unwrap(), "click:s:0,submit:s");
    assert_eq!(r.focused(), Some(id_of(&r, "t")));
    // A real click on the same button does move focus and fires pointer events.
    r.eval("log.length = 0").unwrap();
    click_id(&mut r, "s");
    assert_eq!(r.eval("log.join()").unwrap(), "pointerdown:s:1,mousedown:s:1,blur:t:0,focus:s:0,pointerup:s:1,mouseup:s:1,click:s:1,submit:s");
    assert_eq!(r.focused(), Some(id_of(&r, "s")));
    // A prevented click on the default button blocks the implicit submission.
    r.dispatch(UiEvent::Focus {
        node: Some(id_of(&r, "t")),
    });
    r.eval("log.length = 0; document.getElementById('s').addEventListener('click', e => e.preventDefault());").unwrap();
    assert_eq!(
        r.dispatch(UiEvent::Key {
            key: "Enter".into(),
            code: String::new(),
            modifiers: Modifiers::default(),
            repeat: false
        }),
        DefaultAction::Prevented
    );
    assert_eq!(r.eval("log.join()").unwrap(), "click:s:0");
}

#[test]
fn form_validation_blocks_submission() {
    let mut r = run("<form id=f><input id=i required><button id=b></button></form>", "window.inv=0; document.getElementById('i').addEventListener('invalid', ()=>inv++); document.getElementById('f').addEventListener('submit', ()=>{ window.submitted=true; });");
    assert_eq!(click_id(&mut r, "b"), DefaultAction::Prevented);
    assert_eq!(
        r.eval("inv + ' ' + window.submitted").unwrap(),
        "1 undefined"
    );
    r.dispatch(UiEvent::SetValue {
        node: id_of(&r, "i"),
        value: "ok".into(),
        commit: true,
    });
    assert!(matches!(
        click_id(&mut r, "b"),
        DefaultAction::Submit { .. }
    ));
}

#[test]
fn typing_fires_key_and_input_events_and_edits_value() {
    let mut r = run("<input id=i value='ab'><textarea id=t></textarea>", "window.ev=[]; const i=document.getElementById('i'); for (const t of ['keydown','keypress','beforeinput','input','keyup','change']) i.addEventListener(t, e=>ev.push(t+(e.key!==undefined?':'+e.key:'')+(e.data!==undefined?':'+e.data:'')+(e.inputType?':'+e.inputType:'')));");
    r.dispatch(UiEvent::Focus {
        node: Some(id_of(&r, "i")),
    });
    r.dispatch(UiEvent::Key {
        key: "c".into(),
        code: String::new(),
        modifiers: Modifiers::default(),
        repeat: false,
    });
    assert_eq!(
        r.eval("ev.join('|') + ' ' + i.value").unwrap(),
        "keydown:c|keypress:c|beforeinput:c:insertText|input:c:insertText|keyup:c abc"
    );
    r.eval("ev.length = 0").unwrap();
    r.dispatch(UiEvent::Key {
        key: "Backspace".into(),
        code: String::new(),
        modifiers: Modifiers::default(),
        repeat: false,
    });
    r.dispatch(UiEvent::TypeText { text: "xy".into() });
    assert_eq!(
        r.eval("i.value + ' ' + i.selectionStart").unwrap(),
        "abxy 4"
    );
    r.eval("ev.length = 0; i.setSelectionRange(0, 2);").unwrap();
    r.dispatch(UiEvent::Key {
        key: "Z".into(),
        code: String::new(),
        modifiers: Modifiers::default(),
        repeat: false,
    });
    assert_eq!(r.eval("i.value").unwrap(), "Zxy");
    // Prevented keydown suppresses the edit; prevented beforeinput too.
    r.eval("i.addEventListener('keydown', e => { if (e.key === 'q') e.preventDefault(); }); i.addEventListener('beforeinput', e => { if (e.data === 'w') e.preventDefault(); });").unwrap();
    assert_eq!(
        r.dispatch(UiEvent::Key {
            key: "q".into(),
            code: String::new(),
            modifiers: Modifiers::default(),
            repeat: false
        }),
        DefaultAction::Prevented
    );
    r.dispatch(UiEvent::Key {
        key: "w".into(),
        code: String::new(),
        modifiers: Modifiers::default(),
        repeat: false,
    });
    assert_eq!(r.eval("i.value").unwrap(), "Zxy");
    // Change fires on blur after an edit.
    r.eval("ev.length = 0").unwrap();
    r.dispatch(UiEvent::Focus {
        node: Some(id_of(&r, "t")),
    });
    assert_eq!(r.eval("ev.join()").unwrap(), "change");
    r.dispatch(UiEvent::Key {
        key: "Enter".into(),
        code: String::new(),
        modifiers: Modifiers::default(),
        repeat: false,
    });
    r.dispatch(UiEvent::TypeText { text: "k".into() });
    assert_eq!(r.eval("JSON.stringify(t.value)").unwrap(), "\"\\nk\"");
    // Ctrl+A selects all, then typing replaces.
    r.dispatch(UiEvent::Key {
        key: "a".into(),
        code: String::new(),
        modifiers: Modifiers {
            ctrl: true,
            ..Default::default()
        },
        repeat: false,
    });
    r.dispatch(UiEvent::TypeText { text: "R".into() });
    assert_eq!(r.eval("t.value").unwrap(), "R");
}

#[test]
fn tab_moves_focus_in_order() {
    let mut r = run("<input id=a><button id=b></button><div id=d tabindex=0></div><a id=l href=x></a><input id=h tabindex=-1><input id=z disabled>", "");
    let names = |r: &mut Realm| r.eval("document.activeElement.id").unwrap();
    r.dispatch(UiEvent::Key {
        key: "Tab".into(),
        code: String::new(),
        modifiers: Modifiers::default(),
        repeat: false,
    });
    assert_eq!(names(&mut r), "a");
    r.dispatch(UiEvent::Key {
        key: "Tab".into(),
        code: String::new(),
        modifiers: Modifiers::default(),
        repeat: false,
    });
    r.dispatch(UiEvent::Key {
        key: "Tab".into(),
        code: String::new(),
        modifiers: Modifiers::default(),
        repeat: false,
    });
    assert_eq!(names(&mut r), "d");
    r.dispatch(UiEvent::Key {
        key: "Tab".into(),
        code: String::new(),
        modifiers: Modifiers {
            shift: true,
            ..Default::default()
        },
        repeat: false,
    });
    assert_eq!(names(&mut r), "b");
    r.dispatch(UiEvent::Key {
        key: "Tab".into(),
        code: String::new(),
        modifiers: Modifiers::default(),
        repeat: false,
    });
    r.dispatch(UiEvent::Key {
        key: "Tab".into(),
        code: String::new(),
        modifiers: Modifiers::default(),
        repeat: false,
    });
    assert_eq!(names(&mut r), "l");
    r.dispatch(UiEvent::Key {
        key: "Tab".into(),
        code: String::new(),
        modifiers: Modifiers::default(),
        repeat: false,
    });
    assert_eq!(names(&mut r), "");
}

#[test]
fn hover_events_and_hover_state() {
    let mut r = run("<div id=a style='height:20px'><span id=b>x</span></div><div id=c style='height:20px'>c</div>", "window.ev=[]; for (const id of ['a','b','c']) for (const t of ['mouseover','mouseout','mouseenter','mouseleave','mousemove','pointerenter']) document.getElementById(id).addEventListener(t, e=>ev.push(id+':'+t+(e.relatedTarget?'>'+e.relatedTarget.id:'')));");
    r.dispatch(UiEvent::PointerMove {
        x: 12,
        y: 12,
        modifiers: Modifiers::default(),
    });
    let first = r.eval("ev.join()").unwrap();
    assert!(first.starts_with("b:mouseover,a:mouseover,a:pointerenter,a:mouseenter,b:pointerenter,b:mouseenter,b:mousemove,a:mousemove"), "{first}");
    assert_eq!(r.hovered(), Some(id_of(&r, "b")));
    assert_eq!(
        r.eval("[a.matches(':hover'), b.matches(':hover'), c.matches(':hover')].join()")
            .unwrap(),
        "true,true,false"
    );
    r.eval("ev.length=0").unwrap();
    r.dispatch(UiEvent::PointerMove {
        x: 12,
        y: 38,
        modifiers: Modifiers::default(),
    });
    let second = r.eval("ev.join()").unwrap();
    assert!(second.starts_with("b:mouseout>c,a:mouseout>c,b:mouseleave>c,a:mouseleave>c,c:mouseover>b,c:pointerenter>b,c:mouseenter>b,c:mousemove"), "{second}");
    r.eval("ev.length=0").unwrap();
    r.dispatch(UiEvent::PointerMove {
        x: 13,
        y: 39,
        modifiers: Modifiers::default(),
    });
    assert_eq!(r.eval("ev.join()").unwrap(), "c:mousemove");
}

#[test]
fn scroll_and_wheel_events() {
    let mut r = run("<div id=box style='height:50px;overflow:auto'><div style='height:500px'></div></div><div style='height:3000px'></div>", "window.ev=[]; document.getElementById('box').addEventListener('scroll', ()=>ev.push('box'+box.scrollTop)); document.addEventListener('scroll', ()=>ev.push('doc'+window.scrollY)); window.addEventListener('scroll', ()=>ev.push('win')); document.addEventListener('wheel', e=>{ if (e.deltaY > 900) e.preventDefault(); });");
    r.dispatch(UiEvent::Scroll {
        node: Some(id_of(&r, "box")),
        x: 0,
        y: 30,
    });
    r.dispatch(UiEvent::Scroll {
        node: None,
        x: 0,
        y: 100,
    });
    r.dispatch(UiEvent::Scroll {
        node: None,
        x: 0,
        y: 100,
    });
    assert_eq!(r.eval("ev.join()").unwrap(), "box30,doc100,win");
    r.dispatch(UiEvent::Scroll {
        node: None,
        x: 0,
        y: 0,
    });
    r.eval("ev.length=0").unwrap();
    r.dispatch(UiEvent::Wheel {
        x: 20,
        y: 20,
        delta_x: 0,
        delta_y: 20,
        modifiers: Modifiers::default(),
    });
    assert_eq!(
        r.eval("ev.join() + ' ' + box.scrollTop").unwrap(),
        "box50 50"
    );
    assert_eq!(
        r.dispatch(UiEvent::Wheel {
            x: 20,
            y: 20,
            delta_x: 0,
            delta_y: 1000,
            modifiers: Modifiers::default()
        }),
        DefaultAction::Prevented
    );
    r.dispatch(UiEvent::Wheel {
        x: 5,
        y: 200,
        delta_x: 0,
        delta_y: 50,
        modifiers: Modifiers::default(),
    });
    assert_eq!(r.eval("window.scrollY").unwrap(), "50");
    // Clamped to the scrollable range.
    r.dispatch(UiEvent::Scroll {
        node: Some(id_of(&r, "box")),
        x: 0,
        y: 9999,
    });
    // 500px of content in a 50px box: no horizontal bar is needed, so the whole
    // 50px is the scrollport and 450px of content is below it.
    assert_eq!(
        r.eval("box.scrollTop + ',' + (box.scrollHeight - box.clientHeight)")
            .unwrap(),
        "450,450"
    );
}

#[test]
fn resize_hashchange_popstate_visibility_unload() {
    let mut r = run("<div id=t></div>", "window.ev=[]; addEventListener('resize', ()=>ev.push('resize'+innerWidth)); addEventListener('hashchange', e=>ev.push('hash:'+location.hash+':'+e.oldURL.slice(-9))); addEventListener('popstate', e=>ev.push('pop:'+JSON.stringify(e.state))); document.addEventListener('visibilitychange', ()=>ev.push('vis:'+document.visibilityState)); addEventListener('pageshow', e=>ev.push('show')); addEventListener('beforeunload', e=>{ ev.push('bu'); e.returnValue='stay'; }); addEventListener('unload', ()=>ev.push('unload')); const mq=matchMedia('(max-width: 600px)'); mq.addEventListener('change', e=>ev.push('mq:'+e.matches));");
    r.dispatch(UiEvent::Resize {
        width: 500,
        height: 400,
    });
    r.dispatch(UiEvent::HashChange { hash: "t".into() });
    r.eval("history.pushState({p:1}, '', '/next'); history.pushState({p:2}, '', '/next2');")
        .unwrap();
    r.dispatch(UiEvent::HistoryGo { delta: -1 });
    assert_eq!(r.url(), "https://example.test/next");
    r.dispatch(UiEvent::Visibility { hidden: true });
    r.dispatch(UiEvent::PageShow);
    assert_eq!(
        r.dispatch(UiEvent::Unload),
        DefaultAction::ConfirmUnload("stay".into())
    );
    assert_eq!(r.eval("ev.join()").unwrap(), "show,mq:true,resize500,pop:null,hash:#t:page.html,pop:{\"p\":1},vis:hidden,show,bu,vis:hidden,unload");
}

/// Enter fires keypress (charCode 13) before its default action, and preventing
/// that keypress keeps the form from being submitted implicitly.
#[test]
fn enter_keypress_can_prevent_implicit_submission() {
    let mut r = run("<form id=f><input id=tag><button>Publish</button></form>", "window.ev=[]; f.addEventListener('submit', e=>{ e.preventDefault(); ev.push('submit'); }); tag.addEventListener('keypress', e=>{ ev.push('keypress:'+e.key+':'+e.charCode+':'+e.keyCode); if (tag.value==='ci') e.preventDefault(); }); tag.focus();");
    r.eval("tag.value='ci'").unwrap();
    r.dispatch(UiEvent::Key {
        key: "Enter".into(),
        code: String::new(),
        modifiers: Modifiers::default(),
        repeat: false,
    });
    r.eval("tag.value='x'").unwrap();
    r.dispatch(UiEvent::Key {
        key: "Enter".into(),
        code: String::new(),
        modifiers: Modifiers::default(),
        repeat: false,
    });
    assert_eq!(
        r.eval("ev.join()").unwrap(),
        "keypress:Enter:13:13,keypress:Enter:13:13,submit"
    );
}

/// A fragment navigation (a `#/active` link, `location.hash`, `location.replace`)
/// fires popstate and then hashchange, as browsers do: React Router's HashRouter
/// (TodoMVC's React example) re-renders on popstate alone.
#[test]
fn fragment_navigation_fires_popstate_then_hashchange() {
    let mut r = run("<a id=l href='#/active'>Active</a>", "window.ev=[]; addEventListener('popstate', e=>ev.push('pop:'+e.state+':'+location.hash)); addEventListener('hashchange', e=>ev.push('hash:'+location.hash));");
    click_id(&mut r, "l");
    r.eval("location.hash='#/completed'; location.replace('#/');")
        .unwrap();
    assert_eq!(
        r.eval("ev.join() + ' ' + history.length").unwrap(),
        "pop:null:#/active,hash:#/active,pop:null:#/completed,hash:#/completed,pop:null:#/,hash:#/ 3"
    );
}

#[test]
fn set_value_event_and_select_change() {
    let mut r = run("<select id=s><option>a</option><option>b</option></select><input id=i>", "window.ev=[]; for (const id of ['s','i']) for (const t of ['input','change']) document.getElementById(id).addEventListener(t, e=>ev.push(id+':'+t+':'+e.target.value));");
    r.dispatch(UiEvent::SetValue {
        node: id_of(&r, "s"),
        value: "b".into(),
        commit: true,
    });
    r.dispatch(UiEvent::SetValue {
        node: id_of(&r, "i"),
        value: "typed".into(),
        commit: false,
    });
    assert_eq!(
        r.eval("ev.join() + ' ' + s.selectedIndex").unwrap(),
        "s:input:b,s:change:b,i:input:typed 1"
    );
    let opt = r
        .document()
        .descendants(id_of(&r, "s"))
        .find(|n| r.document().is(*n, "option"))
        .unwrap();
    assert_eq!(
        r.dispatch(UiEvent::ClickNode {
            node: opt,
            modifiers: Modifiers::default(),
            detail: 1
        }),
        DefaultAction::Toggle(id_of(&r, "s"))
    );
    assert_eq!(r.eval("s.value").unwrap(), "a");
}

/// A focused, closed select takes keys as Chromium on Linux does: the arrows
/// step over disabled options, PageUp/PageDown move three, Home/End jump, and
/// printable keys select by label (repeating a key cycles; a longer buffer is a
/// prefix, reset after a second). Each change fires input then change between
/// keydown and keyup; a prevented keydown changes nothing.
#[test]
fn keys_change_a_closed_select() {
    let mut r = run(
        "<select id=s><option>apple</option><option disabled>avocado</option><option>banana</option><option>blueberry</option><option>cherry</option><option>date</option></select>",
        "window.ev=[]; for (const t of ['keydown','input','change','keyup']) s.addEventListener(t, e=>ev.push(t));",
    );
    r.dispatch(UiEvent::Focus {
        node: Some(id_of(&r, "s")),
    });
    let press = |r: &mut Realm, key: &str| {
        r.dispatch(UiEvent::Key {
            key: key.into(),
            code: String::new(),
            modifiers: Modifiers::default(),
            repeat: false,
        });
        r.eval("s.value").unwrap()
    };
    assert_eq!(press(&mut r, "ArrowDown"), "banana");
    assert_eq!(r.eval("ev.join()").unwrap(), "keydown,input,change,keyup");
    r.eval("ev.length = 0").unwrap();
    assert_eq!(press(&mut r, "ArrowUp"), "apple");
    assert_eq!(press(&mut r, "ArrowUp"), "apple");
    assert_eq!(
        r.eval("ev.join()").unwrap(),
        "keydown,input,change,keyup,keydown,keyup"
    );
    assert_eq!(press(&mut r, "End"), "date");
    assert_eq!(press(&mut r, "Home"), "apple");
    assert_eq!(press(&mut r, "PageDown"), "blueberry");
    assert_eq!(press(&mut r, "PageUp"), "apple");
    assert_eq!(press(&mut r, "ArrowRight"), "banana");
    assert_eq!(press(&mut r, "ArrowLeft"), "apple");
    assert_eq!(press(&mut r, "b"), "banana");
    assert_eq!(press(&mut r, "b"), "blueberry");
    assert_eq!(press(&mut r, "b"), "banana");
    r.eval("setTimeout(() => {}, 1100)").unwrap();
    r.run_until_idle(1200);
    // The buffer resets but Blink keeps the repeated character, so this cycles on;
    // "ba" is then a prefix searched from the selected option on.
    assert_eq!(press(&mut r, "b"), "blueberry");
    assert_eq!(press(&mut r, "a"), "banana");
    r.eval("setTimeout(() => {}, 1100)").unwrap();
    r.run_until_idle(1200);
    assert_eq!(press(&mut r, " "), "banana");
    r.eval("s.addEventListener('keydown', e => e.preventDefault())")
        .unwrap();
    assert_eq!(press(&mut r, "ArrowDown"), "banana");
}

#[test]
fn summary_toggles_details() {
    let mut r = run("<details id=d><summary id=s>sum</summary>body</details>", "window.n=0; document.getElementById('d').addEventListener('toggle', e=>{ n++; window.state=e.newState; });");
    assert_eq!(click_id(&mut r, "s"), DefaultAction::Toggle(id_of(&r, "d")));
    r.run_until_idle(10);
    assert_eq!(
        r.eval("d.open + ' ' + n + ' ' + state").unwrap(),
        "true 1 open"
    );
}

#[test]
fn space_and_enter_activate_buttons() {
    let mut r = run(
        "<button id=b>b</button>",
        "window.n=0; b.addEventListener('click', ()=>n++);",
    );
    r.dispatch(UiEvent::Focus {
        node: Some(id_of(&r, "b")),
    });
    r.dispatch(UiEvent::Key {
        key: "Enter".into(),
        code: String::new(),
        modifiers: Modifiers::default(),
        repeat: false,
    });
    r.dispatch(UiEvent::Key {
        key: " ".into(),
        code: String::new(),
        modifiers: Modifiers::default(),
        repeat: false,
    });
    assert_eq!(r.eval("n").unwrap(), "2");
}

#[test]
fn pointer_halves_and_active_state() {
    let mut r = run("<div id=d style='height:30px'>x</div>", "window.ev=[]; d.addEventListener('mousedown', ()=>ev.push('down:'+d.matches(':active'))); d.addEventListener('mouseup', ()=>ev.push('up:'+d.matches(':active'))); d.addEventListener('click', ()=>ev.push('click'));");
    r.dispatch(UiEvent::PointerDown {
        x: 20,
        y: 20,
        button: 0,
        modifiers: Modifiers::default(),
    });
    r.dispatch(UiEvent::PointerUp {
        x: 20,
        y: 20,
        button: 0,
        modifiers: Modifiers::default(),
    });
    assert_eq!(r.eval("ev.join()").unwrap(), "down:true,up:false,click");
}

#[test]
fn error_in_task_reaches_console() {
    let r = run("", "setTimeout(()=>{ null.x; }, 0);");
    assert!(
        errors(&r).contains("Cannot read properties of null (reading 'x')"),
        "{}",
        errors(&r)
    );
}

// ------------------------------------------------- CSS transitions and animations

/// A realm whose head holds `css`, for the transition and animation tests.
fn styled(css: &str, body: &str, script: &str) -> Realm {
    let html = format!("<!DOCTYPE html><html><head><style>{css}</style></head><body>{body}<script>{script}</script></body></html>");
    let mut r = Realm::new(
        &html,
        "https://example.test/page.html",
        Box::new(crate::script::MemoryHost::new()),
    );
    r.run_document();
    // The load's own style flush: an element transitions only from a value a previous
    // flush saw, so without this the first change after load would start nothing.
    r.run_until_idle(0);
    r
}

#[test]
fn css_transition_fires_run_start_and_end_on_the_world_clock() {
    let mut r = styled(
        "#t { opacity: 1; color: rgb(0,0,0); transition: opacity 300ms ease 50ms, color 1s; } #t.off { opacity: 0.25; color: rgb(0,128,0); }",
        "<div id=t>t</div>",
        "window.log=[]; for (const e of ['transitionrun','transitionstart','transitionend','transitioncancel']) document.getElementById('t').addEventListener(e, ev => log.push(Math.round(ev.timeStamp) + ' ' + ev.type + ':' + ev.propertyName + ':' + ev.elapsedTime + ':' + ev.bubbles)); window.start = 0;",
    );
    // Nothing transitions while no property changed.
    r.run_until_idle(500);
    assert_eq!(r.eval("log.join('|')").unwrap(), "");
    r.eval("start = performance.now(); t.classList.add('off')")
        .unwrap();
    r.run_until_idle(10);
    // `transitionrun` is synchronous with the style change; a delayed
    // `transitionstart` waits (`color` has none, `opacity` has 50 ms of it).
    assert_eq!(r.eval("log.length").unwrap(), "3");
    r.run_until_idle(2000);
    let log = r
        .eval("log.map(l => l.replace(/^\\d+ /, '')).join('|')")
        .unwrap();
    assert_eq!(log, "transitionrun:opacity:0:true|transitionrun:color:0:true|transitionstart:color:0:true|transitionstart:opacity:0:true|transitionend:opacity:0.3:true|transitionend:color:1:true");
    // The times are the declared delay and delay + duration, on the world clock.
    let times = r
        .eval("log.map(l => Math.round((+l.split(' ')[0] - start) / 10) * 10).join()")
        .unwrap();
    assert_eq!(times, "0,0,0,50,350,1000");
    assert_eq!(r.eval("getComputedStyle(t).opacity").unwrap(), "0.25");
    assert!(errors(&r).is_empty(), "{}", errors(&r));
}

#[test]
fn css_transition_needs_a_previous_value_a_duration_and_a_real_change() {
    let mut r = styled(
        "#a { transition: opacity 100ms; } #b { transition: opacity 0s; } #b.off, #a.off { opacity: 0; } .fresh { opacity: 0; transition: opacity 100ms; }",
        "<div id=a>a</div><div id=b>b</div>",
        "window.log=[]; document.addEventListener('transitionrun', e => log.push(e.target.id + ':' + e.propertyName), true);",
    );
    r.run_until_idle(50);
    // An element that appears mid-flight transitions nothing: it has no previous value.
    r.eval("const n = document.createElement('div'); n.id = 'c'; n.className = 'fresh'; document.body.appendChild(n); getComputedStyle(n).opacity").unwrap();
    r.run_until_idle(300);
    assert_eq!(r.eval("log.join()").unwrap(), "");
    // A zero duration transitions nothing either; writing the value it already has is
    // not a change.
    r.eval("b.classList.add('off'); a.style.opacity = '1';")
        .unwrap();
    r.run_until_idle(300);
    assert_eq!(r.eval("log.join()").unwrap(), "");
    r.eval("a.style.opacity = '0.5'").unwrap();
    r.run_until_idle(300);
    assert_eq!(r.eval("log.join()").unwrap(), "a:opacity");
}

#[test]
fn a_second_change_cancels_the_transition_in_flight() {
    let mut r = styled(
        "#t { opacity: 1; transition: opacity 400ms; }",
        "<div id=t>t</div>",
        "window.log=[]; for (const e of ['transitionend','transitioncancel']) t.addEventListener(e, ev => log.push(ev.type + ':' + Math.round(ev.elapsedTime * 20) / 20));",
    );
    r.eval("t.style.opacity = '0'; setTimeout(() => { t.style.opacity = '0.5'; }, 100);")
        .unwrap();
    r.run_until_idle(1000);
    assert_eq!(
        r.eval("log.join()").unwrap(),
        "transitioncancel:0.1,transitionend:0.4"
    );
}

#[test]
fn css_animation_fires_start_iteration_and_end() {
    let mut r = styled(
        "@keyframes spin { from { opacity: 1 } to { opacity: 0 } } #a.go { animation: spin 200ms linear 20ms 2; } #b.go { animation: spin 100ms infinite; }",
        "<div id=a>a</div><div id=b>b</div>",
        "window.log=[]; for (const e of ['animationstart','animationiteration','animationend','animationcancel']) document.addEventListener(e, ev => log.push(ev.target.id + ':' + ev.type + ':' + ev.animationName + ':' + Math.round(ev.elapsedTime * 100) / 100), true);",
    );
    r.run_until_idle(50);
    r.eval("a.classList.add('go'); b.classList.add('go');")
        .unwrap();
    r.run_until_idle(2000);
    // `infinite` starts and never ends; the finite one iterates once, then ends.
    assert_eq!(r.eval("log.join('|')").unwrap(), "b:animationstart:spin:0|a:animationstart:spin:0|a:animationiteration:spin:0.2|a:animationend:spin:0.4");
    // A runtime `@keyframes` inserted through the CSSOM animates the same way.
    r.eval("const s = document.createElement('style'); document.head.appendChild(s); s.sheet.insertRule('@keyframes fade { from { opacity: 1 } to { opacity: 0 } }', 0); s.sheet.insertRule('#a.fade { animation: fade 50ms linear both }', 1); log.length = 0; a.className = 'fade';").unwrap();
    r.run_until_idle(500);
    // (`spin` had already ended, so dropping its name cancels nothing.)
    assert_eq!(
        r.eval("log.join('|')").unwrap(),
        "a:animationstart:fade:0|a:animationend:fade:0.05"
    );
    // Taking the name off an animation still running does cancel it.
    r.eval("log.length = 0; b.className = ''").unwrap();
    r.run_until_idle(50);
    assert_eq!(
        r.eval("log.map(l => l.split(':').slice(0, 3).join(':')).join('|')")
            .unwrap(),
        "b:animationcancel:spin"
    );
    assert!(errors(&r).is_empty(), "{}", errors(&r));
}
check!(inline_handler_added_during_dispatch_runs_in_the_same_event, "<div id=a><p id=b></p></div>", "const a=document.getElementById('a'), b=document.getElementById('b'); window.seen=[]; b.addEventListener('click', ()=>{ seen.push('b'); a.setAttribute('onclick', 'seen.push(\"a inline\")'); }); b.dispatchEvent(new MouseEvent('click', {bubbles:true})); b.dispatchEvent(new MouseEvent('click', {bubbles:true})); console.log(seen.join());", "b,a inline,b,a inline");
check!(inline_handler_attribute_removed_stops_running, "<div id=a onclick=\"window.n=(window.n||0)+1\"><p id=b></p></div>", "const a=document.getElementById('a'), b=document.getElementById('b'); b.dispatchEvent(new Event('click', {bubbles:true})); a.removeAttribute('onclick'); b.dispatchEvent(new Event('click', {bubbles:true})); console.log(window.n);", "1");

/// Enter in a text field submits its form through the default button, or with no
/// submit button only when one field in the form blocks implicit submission: two
/// text fields and no button do not submit (Conduit React's article editor, whose
/// tag field adds a tag on Enter).
#[test]
fn enter_submits_a_form_without_a_button_only_with_one_text_field() {
    let mut r = run(
        "<form id=a><input id=a1><input id=a2></form><form id=b><input id=b1><input type=checkbox><input type=hidden></form>",
        "window.subs=[]; for (const f of document.forms) f.addEventListener('submit', e => { e.preventDefault(); subs.push(f.id); });",
    );
    for id in ["a1", "b1"] {
        r.dispatch(UiEvent::Focus {
            node: Some(id_of(&r, id)),
        });
        r.dispatch(UiEvent::Key {
            key: "Enter".into(),
            code: String::new(),
            modifiers: Modifiers::default(),
            repeat: false,
        });
    }
    assert_eq!(r.eval("subs.join()").unwrap(), "b");
}

/// A text field fires `change` on blur (or Enter) only when its value differs from
/// the one it had at focus or at the last change; a value script sets while no
/// edit is pending becomes that baseline. Each case is what Chromium does (Conduit
/// Vue clears its tag field after adding the tag, and must not add an empty one).
#[test]
fn change_compares_against_the_value_at_focus() {
    let cases: &[(&str, &str, &str)] = &[
        ("type-then-reset", "a", "i.value = 'x'"),
        ("type-then-other", "a", "i.value = 'q'"),
        ("script-only", "", "i.value = 'q'"),
        ("type-then-empty", "a", "i.value = ''"),
    ];
    let want = ["", "change:q", "", "change:"];
    for ((name, typed, script), want) in cases.iter().zip(want) {
        let mut r = run(
            "<input id=i value=x><button id=o>o</button>",
            "window.log=[]; i.addEventListener('change', () => log.push('change:' + i.value));",
        );
        r.dispatch(UiEvent::Focus {
            node: Some(id_of(&r, "i")),
        });
        if !typed.is_empty() {
            r.dispatch(UiEvent::TypeText {
                text: (*typed).into(),
            });
        }
        r.eval(script).unwrap();
        r.dispatch(UiEvent::Focus {
            node: Some(id_of(&r, "o")),
        });
        assert_eq!(r.eval("log.join()").unwrap(), want, "{name}");
    }
    // Set by script before typing, typed, set back: no change.
    let mut r = run(
        "<input id=i value=x><button id=o>o</button>",
        "window.log=[]; i.addEventListener('change', () => log.push('change:' + i.value));",
    );
    r.dispatch(UiEvent::Focus {
        node: Some(id_of(&r, "i")),
    });
    r.eval("i.value = 'q'").unwrap();
    r.dispatch(UiEvent::TypeText { text: "a".into() });
    r.eval("i.value = 'q'").unwrap();
    // Enter commits what was typed; the blur after more typing commits again.
    r.dispatch(UiEvent::TypeText { text: "b".into() });
    r.dispatch(UiEvent::Key {
        key: "Enter".into(),
        code: String::new(),
        modifiers: Modifiers::default(),
        repeat: false,
    });
    r.dispatch(UiEvent::TypeText { text: "c".into() });
    r.dispatch(UiEvent::Focus {
        node: Some(id_of(&r, "o")),
    });
    assert_eq!(r.eval("log.join()").unwrap(), "change:qb,change:qbc");
}
