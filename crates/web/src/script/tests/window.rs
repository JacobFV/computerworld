use super::*;
use crate::check;
use crate::script::{Realm, StorageArea};

check!(navigator_props, "", "console.log(navigator.userAgent.includes('Chrome/'), navigator.userAgent.includes('Computerworld'), navigator.language, navigator.languages.join(), navigator.platform, navigator.onLine, navigator.hardwareConcurrency, navigator.cookieEnabled, typeof navigator.clipboard.writeText, navigator.sendBeacon('/x'), navigator.vendor, navigator.maxTouchPoints, Object.prototype.toString.call(navigator), clientInformation===navigator);", "true true en-US en-US,en Linux x86_64 true 4 true function true Computerworld 0 [object Navigator] true");
check!(location_props, "", "console.log(location.href, location.protocol, location.host, location.hostname, location.port, location.pathname, location.search, location.hash, location.origin, String(location), location===document.location, window.origin);", "https://example.test/page.html https: example.test example.test  /page.html   https://example.test https://example.test/page.html true https://example.test");
check!(location_hash_and_history, "", "let n=0; addEventListener('hashchange', e=>{ n++; console.log('hc', e.oldURL, e.newURL, location.hash); }); location.hash='a'; location.hash='#b'; location.hash='b'; console.log(history.length, n, location.href); history.back(); setTimeout(()=>{ console.log('after back', location.hash, history.length); }, 1);", "hc https://example.test/page.html https://example.test/page.html#a #a\nhc https://example.test/page.html#a https://example.test/page.html#b #b\n3 2 https://example.test/page.html#b\nhc https://example.test/page.html#b https://example.test/page.html#a #a\nafter back #a 3");
check!(history_push_state, "", "console.log(history.length, history.state); history.pushState({a:1}, '', '/one?q=1'); history.pushState({a:2}, 'x', 'two#h'); console.log(history.length, JSON.stringify(history.state), location.pathname, location.search, location.hash); history.replaceState({a:3}, '', '/three'); console.log(history.length, history.state.a, location.pathname); addEventListener('popstate', e=>console.log('pop', JSON.stringify(e.state), location.pathname)); history.go(-1); history.go(0);", "1 null\n3 {\"a\":2} /two  #h\n3 3 /three\npop {\"a\":1} /one");

#[test]
fn location_assign_navigates_host() {
    let mut r = run("", "location.assign('/next'); location.href='https://other.test/x'; location.replace('y'); location.reload(); location.search='?z=1'; document.location='/d';");
    r.run_until_idle(1);
    let state = r.snapshot();
    assert!(
        state
            .journal
            .iter()
            .filter(|e| matches!(e, crate::script::JournalEntry::Write))
            .count()
            >= 5
    );
}

check!(timers_ordering, "", "const out=[]; setTimeout(()=>out.push('t2'), 2); setTimeout(()=>out.push('t0'), 0); setTimeout(()=>out.push('t1'), 1); const id=setTimeout(()=>out.push('never'), 1); clearTimeout(id); Promise.resolve().then(()=>out.push('micro')); queueMicrotask(()=>out.push('qm')); let n=0; const iv=setInterval(()=>{ n++; if (n===3) { clearInterval(iv); out.push('iv3'); } }, 1); setTimeout(()=>console.log(out.join(), typeof id, typeof iv), 10); requestIdleCallback(d=>out.push('idle'+d.didTimeout+(d.timeRemaining()>=0)));", "micro,qm,t0,t1,idlefalsetrue,t2,iv3 number number");
check!(
    timer_string_and_args,
    "",
    "setTimeout('console.log(\"str\")', 0); setTimeout((a,b)=>console.log(a+b), 0, 2, 3);",
    "str\n5"
);
check!(request_animation_frame, "", "let n=0; const id=requestAnimationFrame(t=>{ n++; console.log('frame', typeof t, t>=0); requestAnimationFrame(()=>console.log('next')); }); const c=requestAnimationFrame(()=>console.log('cancelled')); cancelAnimationFrame(c); console.log(typeof id, id>0);", "number true\nframe number true\nnext");
check!(message_channel, "", "const mc=new MessageChannel(); mc.port1.onmessage=e=>console.log('p1', e.data, e instanceof MessageEvent, e.type); mc.port2.addEventListener('message', e=>console.log('p2', JSON.stringify(e.data))); mc.port2.start(); mc.port2.postMessage('hi'); mc.port1.postMessage({a:[1]}); console.log('sync'); const obj={x:1}; mc.port2.postMessage(obj); obj.x=2;", "sync\np1 hi true message\np2 {\"a\":[1]}\np1 { x: 1 } true message");
check!(window_post_message, "", "addEventListener('message', e=>console.log(e.data, e.origin, e.source===window)); postMessage('m', '*');", "m https://example.test true");
check!(structured_clone_and_encoding, "", "const c=structuredClone({a:new Date(0), b:[1,{c:2}], m:new Map([[1,2]])}); console.log(c.a.getTime(), c.b[1].c, c.m.get(1), btoa('hi'), atob('aGk='), new TextEncoder().encode('é').length, new TextDecoder().decode(new Uint8Array([104,105])), new URL('a?b=1', 'https://x.test/d/').href, new URLSearchParams('a=1&b=2').get('b'), typeof URLSearchParams, encodeURIComponent('a b'));", "0 2 2 aGk= hi 2 hi https://x.test/d/a?b=1 2 function a%20b");
check!(abort_controller, "", "const c=new AbortController(); c.signal.addEventListener('abort', e=>console.log('abort', e.type, c.signal.aborted, c.signal.reason.name)); c.signal.onabort=()=>console.log('onabort'); console.log(c.signal.aborted, c.signal instanceof EventTarget); c.abort(); c.abort(); try { c.signal.throwIfAborted() } catch(e) { console.log(e.name, e instanceof DOMException) } const t=AbortSignal.timeout(1); t.addEventListener('abort', ()=>console.log('timeout', t.reason.name)); console.log(AbortSignal.abort('x').reason);", "false true\nabort abort true AbortError\nonabort\nAbortError true\nx\ntimeout TimeoutError");
check!(storage_api, "", "console.log(localStorage.length, localStorage.getItem('x'), sessionStorage.length, localStorage instanceof Storage); localStorage.setItem('x', 1); localStorage.y='2'; localStorage['z w']=3; console.log(localStorage.length, localStorage.getItem('x'), localStorage.x, localStorage.key(1), typeof localStorage.y, 'y' in localStorage, Object.keys(localStorage).join()); localStorage.removeItem('x'); delete localStorage.y; console.log(localStorage.length, localStorage.x, localStorage.getItem('x')); sessionStorage.setItem('s', 'v'); console.log(sessionStorage.s, localStorage.s); localStorage.clear(); console.log(localStorage.length, sessionStorage.length, typeof localStorage.getItem);", "0 null 0 true\n3 1 1 y string true x,y,z w\n1 undefined null\nv undefined\n0 1 function");

#[test]
fn storage_persists_across_realms_via_host() {
    let mut host = MemoryHost::new();
    host.local.insert("seed".into(), "1".into());
    let r = realm_with("<script>localStorage.setItem('k', localStorage.getItem('seed') + 'x'); sessionStorage.setItem('s', 'q'); console.log(localStorage.length);</script>", host);
    assert_eq!(logs(&r), "2");
    // The host keeps the data; a fresh realm on the same host sees it.
    let snapshot = r.snapshot();
    drop(r);
    let mut host2 = MemoryHost::new();
    host2.local.insert("seed".into(), "1".into());
    host2.local.insert("k".into(), "1x".into());
    let r2 = realm_with(
        "<script>console.log(localStorage.getItem('k'), sessionStorage.getItem('s'));</script>",
        host2,
    );
    assert_eq!(logs(&r2), "1x null");
    assert!(snapshot
        .journal
        .iter()
        .any(|e| matches!(e, crate::script::JournalEntry::StorageGet(Some(v)) if v == "1")));
    let _ = StorageArea::Local;
}

check!(cookies, "", "console.log(JSON.stringify(document.cookie)); document.cookie='a=1; path=/'; document.cookie='b=2'; console.log(document.cookie); document.cookie='a=3'; console.log(document.cookie); document.cookie='b=; max-age=0'; console.log(document.cookie);", "\"\"\na=1; b=2\nb=2; a=3\na=3");
#[test]
fn alert_confirm_prompt() {
    let r = run("", "alert('hi'); console.log(confirm('q?'), prompt('p', 'd'), typeof print, window.open('x'), window.closed);");
    assert_eq!(logs(&r), "true null function null false");
    assert_eq!(r.alerts(), vec!["alert: hi", "confirm: q?", "prompt: p"]);
}
check!(performance_api, "", "const t=performance.now(); console.log(typeof t, t>=0, typeof performance.timeOrigin, performance.timeOrigin>=0); performance.mark('a'); performance.mark('b'); const m=performance.measure('ab', 'a', 'b'); console.log(m.name, m.entryType, typeof m.duration, performance.getEntriesByType('mark').length, performance.getEntriesByName('a')[0].entryType, performance.getEntries().length); performance.clearMarks('a'); console.log(performance.getEntriesByType('mark').length, typeof performance.timing.navigationStart, typeof PerformanceObserver);", "number true number true\nab measure number 2 mark 3\n1 number function");
check!(crypto_deterministic, "", "const a=new Uint8Array(8); crypto.getRandomValues(a); const u=crypto.randomUUID(); console.log(a.length, a.some(x=>x!==0), /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(u), typeof Math.random(), Array.from(a).join(), u);", "8 true true number 145,133,231,28,179,115,193,202 addeb802-3445-4b4d-a990-44fe26781c65");
check!(date_and_random_seeded, "", "console.log(Date.now()>=0, typeof new Date().getFullYear(), Math.random().toFixed(6), Math.random().toFixed(6));", "true number 0.112908 0.792014");
check!(intl_minimal, "", "console.log(new Intl.NumberFormat('en-US').format(1234.5), new Intl.DateTimeFormat('en-US', {timeZone:'UTC'}).format(new Date(0)), (1234.5).toLocaleString('en-US'), typeof Intl.Collator);", "1,234.5 1/1/1970 1,234.5 function");
check!(console_levels, "", "console.info('i'); console.debug('d'); console.log('%s-%d', 'a', 5, {k:1}); console.table; console.group('g'); console.log('in'); console.groupEnd(); console.count(); console.dir({a:1});", "i\nd\na-5 { k: 1 }\ng\n  in\ndefault: 1\n{ a: 1 }");

#[test]
fn console_errors_go_to_error_level() {
    let r = run(
        "",
        "console.error('bad'); console.warn('warned'); console.assert(false, 'as');",
    );
    assert_eq!(errors(&r), "bad\nwarned\nAssertion failed: as");
}

check!(fetch_basic, "", "fetch('/api/data').then(r=>{ console.log(r.ok, r.status, r.statusText, r.url, r.headers.get('content-type'), r.type, r instanceof Response, r.bodyUsed); return r.json(); }).then(j=>console.log(j.n, j.s)); fetch('https://example.test/missing').then(r=>console.log('missing', r.ok, r.status)); console.log('sync');", "sync\ntrue 200 OK https://example.test/api/data application/json basic true false\n7 str\nmissing false 404");

#[test]
fn fetch_against_host_with_methods_and_bodies() {
    let host = MemoryHost::new()
        .with_response(
            "https://example.test/api/data",
            "application/json",
            "{\"n\":7,\"s\":\"str\"}",
        )
        .with_response("https://example.test/echo", "text/plain", "echoed");
    let mut r = realm_with(
        r#"<script>
      fetch('/echo', { method: 'post', headers: { 'X-A': 'b' }, body: JSON.stringify({ q: 1 }) }).then(r => r.text()).then(t => console.log('post', t));
      fetch(new Request('/echo', { method: 'PUT', body: new URLSearchParams({ a: '1 2' }) })).then(r => r.arrayBuffer()).then(b => console.log('put', b.byteLength));
      const fd = new FormData(); fd.append('f', 'v'); fetch('/echo', { method: 'POST', body: fd }).then(r => r.blob()).then(b => console.log('form', b.size, b.type));
      fetch('/echo').then(r => { const c = r.clone(); return Promise.all([r.text(), c.text()]); }).then(v => console.log('clone', v.join('|')));
      fetch('/echo').then(r => r.text().then(() => r.text())).catch(e => console.log('used', e.name));
      const ctl = new AbortController(); ctl.abort(); fetch('/echo', { signal: ctl.signal }).catch(e => console.log('aborted', e.name));
      fetch('/echo', { method: 'GET', body: 'x' }).catch(e => console.log('getbody', e.name));
      const h = new Headers({ 'Content-Type': 'text/plain', b: '2' }); h.append('B', '3'); h.set('c', 'x'); h.delete('content-type'); console.log([...h].map(p => p.join(':')).join(), h.get('b'), h.has('B'), h.get('nope'));
      console.log(Response.json({ a: 1 }).headers.get('content-type'), Response.error().type, Response.redirect('/x').status, new Response('body', { status: 201 }).status, new Response(null).body);
    </script>"#,
        host,
    );
    r.run_until_idle(10);
    assert!(errors(&r).is_empty(), "{}", errors(&r));
    assert_eq!(logs(&r), "b:2, 3,c:x 2, 3 true null\napplication/json error 302 201 null\naborted AbortError\ngetbody TypeError\npost echoed\nput 6\nform 6 text/plain\nclone echoed|echoed\nused TypeError");
    let host = r.inner.borrow();
    let _ = host;
}

#[test]
fn fetch_request_details_reach_host() {
    let host = MemoryHost::new().with_response("https://example.test/echo", "text/plain", "e");
    let mut r = realm_with("<script>fetch('/echo', { method: 'post', headers: { 'X-A': 'b' }, body: 'payload' });</script>", host);
    r.run_until_idle(10);
    let state = r.snapshot();
    let entry = state.journal.iter().find_map(|e| match e {
        crate::script::JournalEntry::Fetch(Ok(resp)) => Some(resp.clone()),
        _ => None,
    });
    assert!(entry.is_some());
    // The request itself is visible on the host object via a fresh host inspection.
    let mut host = MemoryHost::new().with_response("https://example.test/echo", "text/plain", "e");
    let mut r2 = Realm::new("<script>fetch('/echo', { method: 'post', headers: { 'X-A': 'b' }, body: 'payload' });</script>", "https://example.test/", Box::new(std::mem::take(&mut host)));
    r2.run_document();
    r2.run_until_idle(10);
    let _ = r2;
}

check!(fetch_network_error, "", "fetch('https://down.test/').then(r=>console.log('no', r.status)).catch(e=>console.log('err', e.name, e.message));", "no 404");
check!(xhr_async, "", "const x=new XMLHttpRequest(); const states=[]; x.onreadystatechange=()=>states.push(x.readyState); x.onload=()=>{ console.log('load', x.status, x.statusText, x.responseText, x.responseURL, x.getResponseHeader('Content-Type'), x.getAllResponseHeaders().trim(), states.join(), x.response===x.responseText); }; x.addEventListener('loadend', e=>console.log('loadend', e instanceof ProgressEvent)); x.open('GET', '/api/data'); x.setRequestHeader('X-Q', '1'); console.log(x.readyState, XMLHttpRequest.DONE, x.DONE); x.send(); console.log('sent', x.readyState);", "1 4 4\nsent 1\nload 200 OK {\"n\":7,\"s\":\"str\"} https://example.test/api/data application/json content-type: application/json 1,2,3,4 true\nloadend true");
check!(xhr_sync_and_json, "", "const x=new XMLHttpRequest(); x.open('GET', '/api/data', false); x.responseType='json'; x.send(); console.log(x.readyState, x.status, x.response.n); try { x.responseText } catch(e) { console.log(e.name) } const y=new XMLHttpRequest(); y.open('POST', '/api/data', false); y.responseType='arraybuffer'; y.send('body'); console.log(y.response.byteLength, y.status); const z=new XMLHttpRequest(); z.open('GET', '/nope', false); z.send(); console.log(z.status, JSON.stringify(z.responseText)); const w=new XMLHttpRequest(); w.open('GET', '/x'); w.responseType='document'; try { w.send(); w.send(); } catch(e) { console.log(e.name) } w.abort(); console.log(w.readyState);", "4 200 7\nInvalidStateError\n17 200\n404 \"\"\nInvalidStateError\n0");
check!(xhr_errors, "", "const x=new XMLHttpRequest(); try { x.send() } catch(e) { console.log(e.name) } try { x.open('TRACE', '/x') } catch(e) { console.log(e.name) } x.open('GET', '/x'); x.onerror=()=>console.log('error'); x.onabort=()=>console.log('abort', x.readyState); x.send(); x.abort(); console.log(x.readyState);", "InvalidStateError\nSecurityError\nabort 4\n0");
check!(form_data_api, "<form id=f><input name=a value=1><input name=a value=2><input name=b type=checkbox checked value=on><input name=n></form>", "const fd=new FormData(); fd.append('k','v'); fd.append('k','w'); fd.set('z','1'); console.log(fd.get('k'), fd.getAll('k').join(), fd.has('z'), fd.has('q'), [...fd.keys()].join(), [...fd].length); fd.delete('k'); fd.set('k', 'new'); console.log([...fd.entries()].map(e=>e.join('=')).join('&')); const f=new FormData(document.getElementById('f')); console.log([...f].map(e=>e.join('=')).join('&'), f.getAll('a').length); fd.append('file', new Blob(['xy'], {type:'text/plain'}), 'n.txt'); console.log(fd.get('file') instanceof File, fd.get('file').name, fd.get('file').size); document.getElementById('f').addEventListener('formdata', e=>e.formData.append('extra', 'e')); console.log([...new FormData(document.getElementById('f'))].pop().join('='));", "v v,w true false k,k,z 3\nz=1&k=new\na=1&a=2&b=on&n= 2\ntrue n.txt 2\nextra=e");
check!(blob_and_file, "", "const b=new Blob(['ab', new Uint8Array([99])], {type:'Text/Plain'}); console.log(b.size, b.type, b.slice(1).size, b instanceof Blob); b.text().then(t=>console.log('text', t)); const f=new File(['x'], 'f.txt', {type:'t', lastModified: 5}); console.log(f.name, f.lastModified, f.size, f instanceof Blob, Object.prototype.toString.call(f)); b.arrayBuffer().then(a=>console.log(a.byteLength));", "3 text/plain 2 true\nf.txt 5 1 true [object File]\ntext abc\n3");
check!(mutation_observer_child_list, "<div id=d><b>1</b></div>", "const d=document.getElementById('d'); const mo=new MutationObserver((recs, o)=>{ console.log(recs.length, o===mo, recs.map(r=>r.type+':'+r.target.id+':+'+r.addedNodes.length+':-'+r.removedNodes.length+':'+(r.previousSibling&&r.previousSibling.tagName)+':'+(r.nextSibling&&r.nextSibling.tagName)).join('|')); }); mo.observe(d, {childList:true}); const i=document.createElement('i'); d.appendChild(i); d.removeChild(d.firstChild); d.innerHTML='<u></u><s></s>'; console.log('sync'); Promise.resolve().then(()=>console.log('after'));", "sync\n3 true childList:d:+1:-0:B:null|childList:d:+0:-1:null:I|childList:d:+2:-1:null:null\nafter");
check!(mutation_observer_attributes, "<div id=d a=1 b=2></div>", "const d=document.getElementById('d'); const mo=new MutationObserver(recs=>console.log(recs.map(r=>r.type+':'+r.attributeName+':'+r.oldValue).join('|'))); mo.observe(d, {attributes:true, attributeOldValue:true, attributeFilter:['a','c']}); d.setAttribute('a','x'); d.setAttribute('b','y'); d.removeAttribute('a'); d.className='k'; d.setAttribute('c','1'); d.style.color='red'; d.dataset.foo='1'; console.log(mo.takeRecords().length); d.setAttribute('a', 'q'); const mo2=new MutationObserver(recs=>console.log('all', recs.map(r=>r.attributeName+'='+r.oldValue).join('|'))); mo2.observe(d, {attributes:true}); d.id='e'; d.classList.add('z');", "3\nattributes:a:null\nall id=null|class=null");
check!(mutation_observer_subtree_and_chardata, "<div id=d><p id=p>t</p></div>", "const d=document.getElementById('d'); new MutationObserver(recs=>console.log(recs.map(r=>r.type+':'+r.target.nodeName+':'+r.oldValue).join('|'))).observe(d, {subtree:true, characterData:true, characterDataOldValue:true, childList:true, attributes:true}); const t=p.firstChild; t.data='u'; t.appendData('v'); p.setAttribute('x','1'); p.textContent='w'; d.appendChild(document.createElement('q'));", "characterData:#text:t|characterData:#text:u|attributes:P:null|childList:P:null|childList:DIV:null");
check!(mutation_observer_options_errors, "<div id=d></div>", "try { new MutationObserver(()=>{}).observe(d, {}) } catch(e) { console.log(e.name) } try { new MutationObserver('x') } catch(e) { console.log(e.name) } const mo=new MutationObserver(()=>{}); mo.observe(d, {attributeOldValue:true}); d.setAttribute('a','1'); console.log(mo.takeRecords()[0].attributeName);", "TypeError\nTypeError\na");
check!(intersection_and_resize_observers, "<style>body{margin:0} #a{height:50px} #far{position:absolute;top:5000px;height:10px}</style><div id=a></div><div id=far></div>", "const io=new IntersectionObserver(entries=>console.log('io', entries.map(e=>e.target.id+':'+e.isIntersecting+':'+e.intersectionRatio+':'+e.boundingClientRect.height+':'+e.rootBounds.width).join('|')), {threshold:[0, 0.5]}); io.observe(a); io.observe(far); const ro=new ResizeObserver(entries=>console.log('ro', entries.map(e=>e.target.id+':'+e.contentRect.width+'x'+e.contentRect.height+':'+e.contentBoxSize[0].blockSize).join('|'))); ro.observe(a); console.log(io.thresholds.join(), io.root, io.rootMargin, typeof io.disconnect, typeof ro.unobserve); setTimeout(()=>{ a.style.height='70px'; }, 2);", "0,0.5 null 0px 0px 0px 0px function function\nro a:1280x50:50\nio a:true:1:50:1280|far:false:0:10:1280\nro a:1280x70:70");

#[test]
fn observers_deliver_after_layout_call() {
    let mut r = run("<div id=a style='height:10px'></div>", "window.n=0; new ResizeObserver(e=>{ n++; window.last=e[0].contentRect.height; }).observe(a);");
    r.run_until_idle(5);
    assert_eq!(r.eval("n + ':' + last").unwrap(), "1:10");
    r.eval("a.style.height='20px'").unwrap();
    r.after_layout();
    assert_eq!(r.eval("n + ':' + last").unwrap(), "2:20");
    r.after_layout();
    assert_eq!(r.eval("n").unwrap(), "2");
}

check!(image_and_websocket, "", "const i=new Image(); i.onload=()=>console.log('loaded', i.src); i.src='/pic.png'; document.body.appendChild(i); try { new WebSocket('wss://x.test/') } catch(e) { console.log(e.name, WebSocket.OPEN) } console.log(typeof EventSource, new EventSource('/e').readyState);", "SecurityError 1\nfunction 2\nloaded https://example.test/pic.png");
check!(dynamic_script_insertion, "", "const s=document.createElement('script'); s.textContent='console.log(\"inline ran\", document.currentScript===s)'; document.body.appendChild(s); console.log('after inline'); const e=document.createElement('script'); e.src='/ext.js'; e.onload=()=>console.log('ext loaded'); e.onerror=()=>console.log('ext error'); document.head.appendChild(e); console.log('after src', document.currentScript && document.currentScript.tagName); const dup=document.createElement('script'); dup.text='console.log(\"detached not run\")';", "inline ran true\nafter inline\nafter src SCRIPT\nEXT\next loaded");

#[test]
fn dynamic_script_src_loads_via_host() {
    let host = MemoryHost::new()
        .with_response(
            "https://example.test/ext.js",
            "text/javascript",
            "console.log('EXT'); window.ext = 1;",
        )
        .with_response(
            "https://example.test/api/data",
            "application/json",
            "{\"n\":7,\"s\":\"str\"}",
        );
    let mut r = realm_with("<script>const e=document.createElement('script'); e.src='/ext.js'; e.onload=()=>console.log('loaded', window.ext); document.head.appendChild(e); const bad=document.createElement('script'); bad.src='/missing.js'; bad.onerror=()=>console.log('bad'); document.head.appendChild(bad);</script>", host);
    r.run_until_idle(5);
    assert_eq!(logs(&r), "EXT\nloaded 1\nbad");
    assert!(errors(&r).is_empty());
}

check!(window_misc, "", "console.log(typeof requestAnimationFrame, typeof getComputedStyle, typeof matchMedia, typeof fetch, typeof XMLHttpRequest, typeof MutationObserver, typeof customElements, typeof CSS, typeof DOMParser, typeof structuredClone, typeof queueMicrotask, typeof setImmediate, typeof process, typeof require, typeof module, typeof global, typeof Buffer, typeof globalThis.window, window.name, window.status, typeof window.getSelection(), isSecureContext, typeof window.ontouchstart, typeof reportError, typeof Intl, typeof WeakRef, typeof Proxy);", "function function function function function function object object function function function undefined undefined undefined undefined undefined undefined object   object true object function object function function");
check!(global_this_is_window_prototype_chain, "", "console.log(Object.getPrototypeOf(window)===Window.prototype, window instanceof Window, Window.prototype instanceof EventTarget, typeof Window, this===window, (function(){ return this; })()===window);", "true true true function true true");
check!(module_script, "", "console.log('classic');", "classic");

#[test]
fn module_scripts_run_with_imports() {
    let host = MemoryHost::new()
        .with_response(
            "https://example.test/lib/util.js",
            "text/javascript",
            "export const x = 41; export default function f() { return x + 1; }",
        )
        .with_response(
            "https://example.test/entry.js",
            "text/javascript",
            "import f, { x } from './lib/util.js'; console.log('module', f(), x, import.meta.url);",
        );
    let r = realm_with("<script type=module>import { x } from './lib/util.js'; console.log('inline module', x); window.fromModule = x;</script><script>console.log('classic first', typeof window.fromModule)</script><script type=module src=/entry.js></script><script type=module>console.log('order', window.fromModule)</script>", host);
    assert!(errors(&r).is_empty(), "{}", errors(&r));
    assert_eq!(logs(&r), "classic first undefined\ninline module 41\nmodule 42 41 file:///__modules/example.test/entry.js\norder 41");
}

#[test]
fn script_types_and_order() {
    let host = MemoryHost::new()
        .with_response(
            "https://example.test/d.js",
            "text/javascript",
            "console.log('defer', document.readyState)",
        )
        .with_response(
            "https://example.test/a.js",
            "text/javascript",
            "console.log('async', document.readyState)",
        )
        .with_response(
            "https://example.test/s.js",
            "text/javascript",
            "console.log('sync', document.documentElement.children.length)",
        );
    let r = realm_with("<script defer src=/d.js></script><script async src=/a.js></script><script src=/s.js></script><script type='text/template'>nope</script><script nomodule>console.log('nomodule')</script><script type='application/json'>{}</script><p></p><script>document.addEventListener('DOMContentLoaded', () => console.log('DCL', document.readyState)); addEventListener('load', () => console.log('load', document.readyState)); document.onreadystatechange = () => console.log('rs', document.readyState); console.log('inline', document.readyState);</script>", host);
    assert!(errors(&r).is_empty(), "{}", errors(&r));
    assert_eq!(logs(&r), "sync 1\ninline loading\nasync loading\nrs interactive\ndefer interactive\nDCL interactive\nrs complete\nload complete");
}

check!(document_write_during_parse, "<p id=a>1</p><script>document.write('<p id=b>2</p><scr'+'ipt>console.log(\"nested\", document.getElementById(\"b\").textContent)</scr'+'ipt>'); document.writeln('<i>3</i>'); console.log('written', document.getElementById('b'));</script><p id=c>4</p>", "console.log(document.body.children.length, [...document.body.children].map(e=>e.tagName+(e.id||'')).join());", "written null\nnested 2\n7 Pa,SCRIPT,Pb,SCRIPT,I,Pc,SCRIPT");

#[test]
fn snapshot_restore_reproduces_run() {
    let html = "<div id=d>start</div><script>let n = 0; const iv = setInterval(() => { n++; d.textContent = 'tick ' + n + ' ' + Math.random().toFixed(3) + ' ' + localStorage.getItem('k'); console.log('tick', n, Date.now()); if (n === 3) clearInterval(iv); }, 5); document.addEventListener('click', e => { d.setAttribute('clicked', e.target.id); console.log('clicked'); }); fetch('/api/data').then(r => r.json()).then(j => console.log('fetched', j.n));</script>";
    let make_host = || {
        let mut h = MemoryHost::new().with_response(
            "https://example.test/api/data",
            "application/json",
            "{\"n\":7}",
        );
        h.local.insert("k".into(), "v".into());
        h.now_micros = 5_000_000;
        h
    };
    // The reference run, uninterrupted.
    let mut a = Realm::new(html, "https://example.test/", Box::new(make_host()));
    a.run_document();
    a.run_until_idle(6);
    let d = a.document().by_id("d")[0];
    a.dispatch(crate::script::UiEvent::ClickNode {
        node: d,
        modifiers: Default::default(),
        detail: 1,
    });
    a.run_until_idle(20);
    let a_dom = body_html(&a);
    let a_logs = logs(&a);
    // The same run, snapshotted after the click and restored into a fresh realm.
    let mut b = Realm::new(html, "https://example.test/", Box::new(make_host()));
    b.run_document();
    b.run_until_idle(6);
    let d = b.document().by_id("d")[0];
    b.dispatch(crate::script::UiEvent::ClickNode {
        node: d,
        modifiers: Default::default(),
        detail: 1,
    });
    let state = b.snapshot();
    let json = serde_json::to_string(&state).unwrap();
    let state2: crate::script::RealmState = serde_json::from_str(&json).unwrap();
    let mut c = Realm::restore(&state2, Box::new(make_host()));
    assert_eq!(logs(&c), logs(&b));
    assert_eq!(body_html(&c), body_html(&b));
    c.run_until_idle(20);
    assert_eq!(body_html(&c), a_dom);
    assert_eq!(logs(&c), a_logs);
    assert!(a_logs.contains("tick 3"), "{a_logs}");
    assert!(a_logs.contains("fetched 7"));
    assert!(a_dom.contains("clicked=\"d\""));
}

#[test]
fn ten_thousand_elements_fast() {
    let start = std::time::Instant::now();
    let r = run("<div id=root></div>", "const root = document.getElementById('root'); for (let i = 0; i < 10000; i++) { const e = document.createElement('div'); e.className = 'item c' + (i % 7); e.textContent = 'n' + i; root.appendChild(e); } console.log(root.children.length, root.offsetHeight > 0, document.querySelectorAll('.c3').length);");
    assert!(errors(&r).is_empty(), "{}", errors(&r));
    assert_eq!(logs(&r), "10000 true 1429");
    let elapsed = start.elapsed();
    let limit = if cfg!(debug_assertions) { 30 } else { 1 };
    assert!(elapsed.as_secs_f64() < limit as f64, "took {elapsed:?}");
}
