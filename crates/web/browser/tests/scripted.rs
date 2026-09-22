//! Pages with JavaScript, driven through `BrowserState` over a fake transport: the
//! realm owns the DOM, every action is a DOM event first, timers run on the world
//! clock, and the whole thing snapshots and restores.
use cw_browser::BrowserState;
use cw_protocol::{HttpRequest, HttpResponse, Result};
use cw_scene::Scene;
use std::collections::BTreeMap;

const ORIGIN: &str = "https://app.test";
const W: u32 = 800;
const H: u32 = 600;

/// A site: path to `(content type, body)`; `POST /api/signup` and `/api/echo` answer
/// from the request. Every request is recorded.
struct Site {
    files: BTreeMap<String, (String, String)>,
    requests: Vec<HttpRequest>,
}

impl Site {
    fn new(files: &[(&str, &str, &str)]) -> Site {
        Site {
            files: files
                .iter()
                .map(|(p, t, b)| (p.to_string(), (t.to_string(), b.to_string())))
                .collect(),
            requests: Vec::new(),
        }
    }
    fn serve(&mut self, r: HttpRequest) -> Result<HttpResponse> {
        self.requests.push(r.clone());
        let url = url::Url::parse(&r.url).unwrap();
        if r.method == "POST" && url.path() == "/api/signup" {
            let body = String::from_utf8_lossy(&r.body).into_owned();
            let mut resp = HttpResponse::text(
                200,
                format!(
                    "{{\"ok\":true,\"got\":{}}}",
                    serde_json::to_string(&body).unwrap()
                ),
            );
            resp.headers
                .insert("content-type".into(), "application/json".into());
            return Ok(resp);
        }
        match self.files.get(url.path()) {
            Some((kind, body)) => {
                let mut resp = HttpResponse::text(200, body.clone());
                resp.headers.insert("content-type".into(), kind.clone());
                if url.path() == "/login" {
                    resp.headers
                        .insert("set-cookie".into(), "sid=secret; Path=/; HttpOnly".into());
                }
                Ok(resp)
            }
            None => Ok(HttpResponse::text(404, "not found")),
        }
    }
}

fn vendor(name: &str) -> String {
    let p = format!(
        "{}/../engine/tests/vendor/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{p}: {e}"))
}
fn fixture(name: &str) -> String {
    let p = format!(
        "{}/../engine/tests/script/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{p}: {e}"))
}

fn texts(scene: &Scene) -> Vec<String> {
    scene
        .nodes
        .iter()
        .filter_map(|n| serde_json::to_value(&n.primitive).ok())
        .filter_map(|v| v.get("text")?.as_str().map(str::to_owned))
        .collect()
}
fn shows(b: &BrowserState, needle: &str) -> bool {
    texts(&b.scene(W, H)).iter().any(|t| t.contains(needle))
}
fn console(b: &BrowserState) -> Vec<String> {
    b.console()
        .iter()
        .map(|e| format!("{}: {}", e.level, e.text))
        .collect()
}
fn browser() -> BrowserState {
    let mut b = BrowserState::default();
    b.set_entropy(42, "computer/test/browser");
    b.set_viewport(W, H);
    b
}
macro_rules! http {
    ($site:expr) => {
        &mut |r: HttpRequest| $site.serve(r)
    };
}

const TABS: &str = r#"<!DOCTYPE html><title>Tabs</title>
<style>.panel{display:none}.panel.on{display:block} details{margin:4px}</style>
<button id=t1 class=tab>One</button><button id=t2 class=tab>Two</button>
<div id=p1 class="panel on">First panel text</div><div id=p2 class=panel>Second panel text</div>
<h3 id=acc>Accordion</h3><div id=body style="display:none">Hidden accordion body</div>
<script>
  document.querySelectorAll('.tab').forEach(function (t, i) {
    t.addEventListener('click', function () {
      document.querySelectorAll('.panel').forEach(function (p, j) { p.classList.toggle('on', i === j); });
      console.log('tab', i + 1);
    });
  });
  document.getElementById('acc').onclick = function () { var b = document.getElementById('body'); b.style.display = b.style.display === 'none' ? 'block' : 'none'; };
</script>"#;

#[test]
fn a_click_handler_changes_what_the_scene_shows() {
    let mut site = Site::new(&[("/", "text/html", TABS)]);
    let mut b = browser();
    b.navigate(&format!("{ORIGIN}/"), http!(site)).unwrap();
    assert!(b.document().unwrap().is_scripted());
    assert!(shows(&b, "First panel text") && !shows(&b, "Second panel text"));
    b.click("t2", http!(site)).unwrap();
    assert!(!shows(&b, "First panel text") && shows(&b, "Second panel text"));
    // The accordion header is a plain <h3> with a click handler: clicked by point.
    assert!(!shows(&b, "Hidden accordion body"));
    let scene = b.scene(W, H);
    let header = scene
        .nodes
        .iter()
        .find(|n| {
            serde_json::to_string(&n.primitive)
                .unwrap()
                .contains("Accordion")
        })
        .expect("header painted");
    b.click_at(header.bounds.x + 2, header.bounds.y + 2, W, H, http!(site))
        .unwrap();
    assert!(shows(&b, "Hidden accordion body"));
    // The page readers see only what is displayed.
    let text = b.document().unwrap().text();
    assert!(
        text.contains("Second panel text") && !text.contains("First panel text"),
        "{text}"
    );
    assert_eq!(console(&b), vec!["log: tab 2"]);
}

const COUNTER: &str = r#"<!DOCTYPE html><title>Counter</title><p id=n>0</p>
<script>var n = 0; setInterval(function () { n++; document.getElementById('n').textContent = String(n); document.title = 'Count ' + n; }, 1000);</script>"#;

#[test]
fn set_interval_advances_with_world_ticks_and_background_tabs_are_throttled() {
    let mut site = Site::new(&[
        ("/", "text/html", COUNTER),
        ("/fast", "text/html", &COUNTER.replace("1000", "100")),
    ]);
    let mut b = browser();
    b.set_clock(1_000_000);
    b.navigate(&format!("{ORIGIN}/"), http!(site)).unwrap();
    assert!(shows(&b, "0"));
    println!(
        "first interval due at {:?} (loaded at 1000000)",
        b.document()
            .unwrap()
            .scripted()
            .unwrap()
            .mirror()
            .next_timer
    );
    assert!(
        !b.refresh_pending(1_500_000),
        "nothing is due half a second in"
    );
    // The VM charges virtual time for the code it runs, so the first interval is due
    // a few milliseconds after the round second.
    assert!(
        b.refresh_pending(2_100_000),
        "the interval is due: the environment must tick"
    );
    b.tick(2_100_000, http!(site)).unwrap();
    assert_eq!(b.title().as_deref(), Some("Count 1"));
    // Three seconds in one tick: the interval fires at each of its due times.
    b.tick(5_100_000, http!(site)).unwrap();
    assert_eq!(b.title().as_deref(), Some("Count 4"));
    assert!(shows(&b, "4"));
    // A 100 ms interval in a background tab runs once per second, not ten times.
    b.new_tab();
    b.navigate(&format!("{ORIGIN}/fast"), http!(site)).unwrap();
    b.tick(6_100_000, http!(site)).unwrap();
    let fast_visible: u32 = b
        .title()
        .unwrap()
        .trim_start_matches("Count ")
        .parse()
        .unwrap();
    assert!(
        fast_visible >= 9,
        "visible: every due time fires ({fast_visible})"
    );
    b.switch_tab(0).unwrap();
    let before: u32 = b.tabs[1].history[0]
        .title()
        .trim_start_matches("Count ")
        .parse()
        .unwrap();
    for step in 1..=20u64 {
        b.tick(6_100_000 + step * 100_000, http!(site)).unwrap();
    }
    let after: u32 = b.tabs[1].history[0]
        .title()
        .trim_start_matches("Count ")
        .parse()
        .unwrap();
    assert_eq!(after - before, 2, "two seconds in the background: two runs");
}

const LIST: &str = r#"<!DOCTYPE html><title>List</title><ul id=items><li>loading</li></ul>
<script>
fetch('/api/items.json').then(function (r) { return r.json(); }).then(function (d) {
  document.getElementById('items').innerHTML = d.items.map(function (i) { return '<li class=item>' + i.name + '</li>'; }).join('');
  console.log('loaded', d.items.length);
});
</script>"#;

#[test]
fn a_fetch_driven_list_renders_json_from_the_transport() {
    let mut site = Site::new(&[
        ("/", "text/html", LIST),
        (
            "/api/items.json",
            "application/json",
            r#"{"items":[{"name":"Apples"},{"name":"Pears"}]}"#,
        ),
    ]);
    let mut b = browser();
    b.navigate(&format!("{ORIGIN}/"), http!(site)).unwrap();
    assert!(shows(&b, "Apples") && shows(&b, "Pears") && !shows(&b, "loading"));
    assert_eq!(console(&b), vec!["log: loaded 2"]);
    assert!(site
        .requests
        .iter()
        .any(|r| r.url.ends_with("/api/items.json")));
}

const SIGNUP: &str = r#"<!DOCTYPE html><title>Sign up</title>
<form id=f action=/never method=post><input id=email name=email><button id=go>Join</button></form><p id=msg></p>
<script>
document.getElementById('f').addEventListener('submit', function (e) {
  e.preventDefault();
  var v = document.getElementById('email').value;
  var msg = document.getElementById('msg');
  if (v.indexOf('@') < 0) { msg.textContent = 'Enter a valid email'; return; }
  fetch('/api/signup', { method: 'POST', headers: { 'Content-Type': 'application/x-www-form-urlencoded' }, body: 'email=' + encodeURIComponent(v) })
    .then(function (r) { return r.json(); }).then(function (d) { msg.textContent = d.ok ? 'Welcome ' + v : 'Failed'; });
});
document.getElementById('email').addEventListener('input', function (e) { console.log('input', e.target.value); });
</script>"#;

#[test]
fn client_side_validation_prevents_the_submit_then_posts_with_fetch() {
    let mut site = Site::new(&[("/", "text/html", SIGNUP)]);
    let mut b = browser();
    b.navigate(&format!("{ORIGIN}/"), http!(site)).unwrap();
    b.fill("email", "nope").unwrap();
    b.click("go", http!(site)).unwrap();
    assert!(shows(&b, "Enter a valid email"));
    assert_eq!(
        b.url(),
        Some("https://app.test/"),
        "preventDefault: no navigation"
    );
    assert!(!site.requests.iter().any(|r| r.method == "POST"));
    // Typed keys reach `input` handlers one by one; the tab's fields mirror the value.
    b.fill("email", "").unwrap();
    b.text("a@b.c").unwrap();
    assert_eq!(b.tab().fields["email"], "a@b.c");
    assert_eq!(b.tab().focused.as_deref(), Some("email"));
    b.key("Enter", http!(site)).unwrap();
    assert!(shows(&b, "Welcome a@b.c"));
    let post = site
        .requests
        .iter()
        .find(|r| r.method == "POST")
        .expect("the fetch POST");
    assert_eq!(
        (
            post.url.as_str(),
            String::from_utf8_lossy(&post.body).as_ref()
        ),
        ("https://app.test/api/signup", "email=a%40b.c")
    );
    assert_eq!(b.url(), Some("https://app.test/"));
    assert!(
        console(&b).contains(&"log: input a@b.c".to_owned()),
        "{:?}",
        console(&b)
    );
}

const SPA: &str = r#"<!DOCTYPE html><title>SPA</title>
<nav><a id=home href="/">Home</a> <a id=about href="/about">About</a> <a id=ext href="/plain.html">Plain</a> <a id=blank href="/plain.html" target=_blank>New tab</a></nav><main id=view></main>
<script>
function render() { var p = location.pathname; document.getElementById('view').textContent = p === '/about' ? 'About view' : 'Home view'; document.title = p === '/about' ? 'About' : 'Home'; }
document.querySelectorAll('#home,#about').forEach(function (a) { a.addEventListener('click', function (e) { e.preventDefault(); history.pushState({ p: a.getAttribute('href') }, '', a.getAttribute('href')); render(); }); });
window.addEventListener('popstate', function (e) { console.log('popstate', JSON.stringify(e.state)); render(); });
window.addEventListener('pagehide', function () { console.log('pagehide'); });
render();
</script>"#;

#[test]
fn push_state_routing_back_forward_and_real_navigation() {
    let mut site = Site::new(&[
        ("/", "text/html", SPA),
        (
            "/plain.html",
            "text/html",
            "<title>Plain</title><p>No script here</p>",
        ),
    ]);
    let mut b = browser();
    b.navigate(&format!("{ORIGIN}/"), http!(site)).unwrap();
    let loads = site.requests.len();
    b.click("about", http!(site)).unwrap();
    assert_eq!(
        (b.url(), b.title().as_deref()),
        (Some("https://app.test/about"), Some("About"))
    );
    assert!(shows(&b, "About view"));
    assert_eq!(
        site.requests.len(),
        loads,
        "a router click makes no request"
    );
    assert_eq!(
        b.tab().history.len(),
        1,
        "same-document entries live in the realm"
    );
    b.back(http!(site)).unwrap();
    assert_eq!(
        (b.url(), b.title().as_deref()),
        (Some("https://app.test/"), Some("Home"))
    );
    assert!(shows(&b, "Home view"));
    b.forward(http!(site)).unwrap();
    assert!(shows(&b, "About view"));
    assert_eq!(
        console(&b),
        vec!["log: popstate null", "log: popstate {\"p\":\"/about\"}"]
    );
    // A link nobody prevents is a real navigation; the page sees pagehide.
    b.click("ext", http!(site)).unwrap();
    assert_eq!(b.title().as_deref(), Some("Plain"));
    assert!(!b.document().unwrap().is_scripted());
    assert_eq!(
        console(&b).last().map(String::as_str),
        Some("log: pagehide")
    );
    // Back across documents restores the SPA where it was (its journal replays).
    b.back(http!(site)).unwrap();
    assert_eq!(
        (b.url(), b.title().as_deref()),
        (Some("https://app.test/about"), Some("About"))
    );
    assert!(shows(&b, "About view"));
    b.click("blank", http!(site)).unwrap();
    assert_eq!(
        (b.tabs.len(), b.active, b.title().as_deref()),
        (2, 1, Some("Plain"))
    );
}

const NAV: &str = r#"<!DOCTYPE html><title>Nav</title><button id=assign>assign</button><button id=open>open</button><button id=reload>reload</button><button id=hash>hash</button><p id=loads></p><p id=sec style="margin-top:2000px">Section</p>
<script>
var n = Number(sessionStorage.getItem('loads') || 0) + 1; sessionStorage.setItem('loads', String(n));
document.getElementById('loads').textContent = 'load ' + n;
document.getElementById('assign').onclick = function () { location.assign('/plain.html'); console.log('still running after assign'); };
document.getElementById('open').onclick = function () { console.log('handle', window.open('/plain.html')); };
document.getElementById('reload').onclick = function () { location.reload(); };
document.getElementById('hash').onclick = function () { location.hash = 'sec'; };
window.addEventListener('hashchange', function (e) { console.log('hashchange', e.newURL); });
document.write('<p>written by document.write</p>');
</script>"#;

#[test]
fn location_window_open_reload_hashchange_and_document_write() {
    let mut site = Site::new(&[
        ("/", "text/html", NAV),
        (
            "/plain.html",
            "text/html",
            "<title>Plain</title><p>plain</p>",
        ),
    ]);
    let mut b = browser();
    b.navigate(&format!("{ORIGIN}/"), http!(site)).unwrap();
    assert!(shows(&b, "written by document.write") && shows(&b, "load 1"));
    b.click("reload", http!(site)).unwrap();
    assert!(shows(&b, "load 2"), "reload keeps the tab's sessionStorage");
    assert_eq!(b.tab().history.len(), 1, "reload replaces the entry");
    b.click("hash", http!(site)).unwrap();
    assert_eq!(b.url(), Some("https://app.test/#sec"));
    assert!(
        b.tab().scroll_y > 1000,
        "the fragment scrolled into view: {}",
        b.tab().scroll_y
    );
    b.click("open", http!(site)).unwrap();
    assert_eq!(
        (b.tabs.len(), b.active, b.title().as_deref()),
        (2, 1, Some("Plain"))
    );
    b.switch_tab(0).unwrap();
    // The navigation happens after the handler finished, never inside it.
    b.click("assign", http!(site)).unwrap();
    assert_eq!(b.title().as_deref(), Some("Plain"));
    let log = console(&b);
    assert!(
        log.contains(&"log: hashchange https://app.test/#sec".to_owned()),
        "{log:?}"
    );
    assert!(
        log.contains(&"log: handle null".to_owned())
            && log.contains(&"log: still running after assign".to_owned()),
        "{log:?}"
    );
}

const NOTES: &str = r#"<!DOCTYPE html><title>Notes</title><input id=note><button id=save>Save</button><p id=saved></p>
<script>
document.getElementById('saved').textContent = 'saved: ' + (localStorage.getItem('note') || '(nothing)');
document.getElementById('save').onclick = function () { localStorage.setItem('note', document.getElementById('note').value); };
console.log('cookie', JSON.stringify(document.cookie));
document.cookie = 'theme=dark; path=/';
document.cookie = 'sid=stolen; path=/';
</script>"#;

#[test]
fn local_storage_survives_reload_and_http_only_cookies_stay_hidden() {
    let mut site = Site::new(&[
        ("/", "text/html", NOTES),
        (
            "/login",
            "text/html",
            "<title>In</title><a id=go href=/>go</a>",
        ),
    ]);
    let mut b = browser();
    b.navigate(&format!("{ORIGIN}/login"), http!(site)).unwrap();
    b.navigate(&format!("{ORIGIN}/"), http!(site)).unwrap();
    assert!(shows(&b, "saved: (nothing)"));
    b.fill("note", "buy milk").unwrap();
    b.click("save", http!(site)).unwrap();
    b.reload(http!(site)).unwrap();
    assert!(shows(&b, "saved: buy milk"));
    assert_eq!(
        b.storage_get("note").unwrap(),
        Some("buy milk"),
        "the same storage the browser API reads"
    );
    let log = console(&b);
    assert_eq!(
        log[0], "log: cookie \"\"",
        "HttpOnly cookies are invisible to script"
    );
    assert_eq!(log[1], "log: cookie \"theme=dark\"");
    let sent = site
        .requests
        .last()
        .unwrap()
        .headers
        .get("cookie")
        .cloned()
        .unwrap_or_default();
    assert!(
        sent.contains("sid=secret") && sent.contains("theme=dark"),
        "{sent}"
    );
}

const SPIN: &str = r#"<!DOCTYPE html><title>Spin</title><button id=spin>Spin</button><button id=ok>OK</button><p id=out>idle</p>
<script>
document.getElementById('spin').onclick = function () { while (true) {} };
document.getElementById('ok').onclick = function () { document.getElementById('out').textContent = 'still alive'; };
</script>"#;

#[test]
fn an_infinite_loop_is_interrupted_and_the_page_stays_usable() {
    let mut site = Site::new(&[("/", "text/html", SPIN)]);
    let mut b = browser();
    b.navigate(&format!("{ORIGIN}/"), http!(site)).unwrap();
    let started = std::time::Instant::now();
    b.click("spin", http!(site)).unwrap();
    let took = started.elapsed();
    let log = console(&b);
    assert!(
        log.iter()
            .any(|l| l.starts_with("error:") && l.contains("step limit")),
        "{log:?}"
    );
    b.click("ok", http!(site)).unwrap();
    assert!(shows(&b, "still alive"));
    println!("interrupting a script that never yields took {took:?}");
}

const MENU: &str = r#"<!DOCTYPE html><title>Menu</title>
<style>body{margin:0;font:16px/20px monospace} #menu{cursor:pointer} #menu .items{display:none} #menu:hover .items{display:block} #list{height:120px;overflow:auto;border:1px solid #888} .row{height:40px}</style>
<nav id=menu>File<div class=items>Open recent</div></nav>
<p id=over>menu closed</p>
<div id=list><div class=row>row 1</div><div class=row>row 2</div><div class=row>row 3</div><div class=row>row 4</div><div class=row>row 5</div><div class=row>row 6</div></div>
<p id=depth>depth 0</p>
<script>
var menu = document.getElementById('menu');
menu.addEventListener('mouseenter', function () { document.getElementById('over').textContent = 'menu entered'; });
menu.addEventListener('mouseleave', function () { document.getElementById('over').textContent = 'menu left'; });
document.getElementById('list').addEventListener('scroll', function (e) { document.getElementById('depth').textContent = 'depth ' + e.target.scrollTop; });
</script>"#;

#[test]
fn hovering_and_scrolling_are_dom_events_too() {
    let mut site = Site::new(&[("/", "text/html", MENU)]);
    let mut b = browser();
    b.navigate(&format!("{ORIGIN}/"), http!(site)).unwrap();
    assert!(!shows(&b, "Open recent") && shows(&b, "menu closed"));
    let scene = b.scene(W, H);
    let file = scene
        .nodes
        .iter()
        .find(|n| {
            serde_json::to_string(&n.primitive)
                .unwrap()
                .contains("File")
        })
        .expect("the menu label");
    let (mx, my) = (file.bounds.x + 2, file.bounds.y + 2);
    // Hover: `:hover` opens the submenu, `mouseenter` runs, and the cursor comes from
    // the computed style.
    let cursor = b.hover_at_with(mx, my, W, H, http!(site));
    assert_eq!(cursor, Some("pointer"));
    assert!(
        shows(&b, "Open recent") && shows(&b, "menu entered"),
        "{:?}",
        texts(&b.scene(W, H))
    );
    // Moving away closes it: `mouseleave` runs and `:hover` no longer matches.
    b.hover_at_with(mx, my + 400, W, H, http!(site));
    assert!(
        !shows(&b, "Open recent") && shows(&b, "menu left"),
        "{:?}",
        texts(&b.scene(W, H))
    );
    // Scrolling a scroll container fires `scroll` on it and moves the paint.
    assert!(b.scroll_pane_with("list", 80, false, http!(site)));
    assert!(shows(&b, "depth 80"), "{:?}", texts(&b.scene(W, H)));
    assert!(
        !b.scroll_pane_with("list", 80, false, http!(site)),
        "scrolling where it already is moves nothing"
    );
}

const RESPONSIVE: &str = r#"<!DOCTYPE html><title>Responsive</title><p id=w></p><p id=m></p><p id=v>visible</p><p id=f>frames 0</p>
<script>
var mq = matchMedia('(max-width: 600px)');
function show() { document.getElementById('w').textContent = 'w=' + window.innerWidth + 'x' + window.innerHeight; document.getElementById('m').textContent = mq.matches ? 'narrow' : 'wide'; }
show();
window.addEventListener('resize', show);
document.addEventListener('visibilitychange', function () { document.getElementById('v').textContent = document.hidden ? 'hidden' : 'visible'; });
var frames = 0;
function step() { frames++; document.getElementById('f').textContent = 'frames ' + frames; if (frames < 30) requestAnimationFrame(step); }
requestAnimationFrame(step);
</script>"#;

#[test]
fn resizing_visibility_and_animation_frames_follow_the_browser() {
    let mut site = Site::new(&[("/", "text/html", RESPONSIVE)]);
    let mut b = browser();
    b.navigate(&format!("{ORIGIN}/"), http!(site)).unwrap();
    let at = |b: &BrowserState, w: u32, h: u32, needle: &str| {
        texts(&b.scene(w, h)).iter().any(|t| t.contains(needle))
    };
    assert!(at(&b, W, H, "w=800x600") && at(&b, W, H, "wide"));
    // The environment announces the content viewport before it acts: `resize` fires
    // and `matchMedia` re-evaluates.
    b.set_viewport(500, 400);
    assert!(
        at(&b, 500, 400, "w=500x400"),
        "{:?}",
        texts(&b.scene(500, 400))
    );
    assert!(at(&b, 500, 400, "narrow"));
    // Zoom changes the CSS viewport the page is laid out for, so it resizes too.
    b.step_zoom("in").unwrap();
    assert!(
        at(&b, 500, 400, "w=435x348"),
        "{:?}",
        texts(&b.scene(500, 400))
    );
    b.step_zoom("reset").unwrap();
    // A hidden tab sees `visibilitychange`; the tab on show sees it come back.
    b.new_tab();
    assert!(b.tabs[0].history[0]
        .web()
        .unwrap()
        .text()
        .contains("hidden"));
    b.switch_tab(0).unwrap();
    assert!(at(&b, 500, 400, "visible"));
    // `requestAnimationFrame`: the browser says the page wants another frame, and a
    // tick on the world clock delivers them, one per 16 ms.
    let frames = |b: &BrowserState| {
        texts(&b.scene(500, 400))
            .iter()
            .find_map(|t| t.strip_prefix("frames ").map(|n| n.parse::<u32>().unwrap()))
            .unwrap()
    };
    assert!(
        b.refresh_pending(b.clock),
        "a page mid-animation always wants the next frame"
    );
    let before = frames(&b);
    b.tick(b.clock + 100_000, http!(site)).unwrap();
    let after = frames(&b);
    assert!(
        (5..=8).contains(&(after - before)),
        "100 ms of world time is six or so frames: {before} -> {after}"
    );
    for _ in 0..10 {
        b.tick(b.clock + 100_000, http!(site)).unwrap();
    }
    assert_eq!(frames(&b), 30, "the animation stopped asking");
    assert!(
        !b.refresh_pending(b.clock + 10_000_000),
        "a settled page wants nothing"
    );
}

fn library_site() -> Site {
    let jq = vendor("jquery-3.7.1.min.js");
    let preact = vendor("preact-10.19.3.umd.js");
    let hooks = vendor("preact-hooks-10.19.3.umd.js");
    let jq_page = fixture("jquery.html");
    let preact_page = fixture("preact.html");
    Site::new(&[
        ("/jq.html", "text/html", &jq_page),
        ("/preact.html", "text/html", &preact_page),
        ("/vendor/jquery-3.7.1.min.js", "text/javascript", &jq),
        ("/vendor/preact-10.19.3.umd.js", "text/javascript", &preact),
        (
            "/vendor/preact-hooks-10.19.3.umd.js",
            "text/javascript",
            &hooks,
        ),
        ("/api/items", "application/json", "{\"items\":[1,2,3]}"),
        ("/api/text", "text/plain", "plain text"),
    ])
}

#[test]
fn the_jquery_fixture_runs_and_is_driven_by_clicks() {
    let mut site = library_site();
    let mut b = browser();
    b.navigate(&format!("{ORIGIN}/jq.html"), http!(site))
        .unwrap();
    assert!(console(&b).is_empty(), "{:?}", console(&b));
    assert!(shows(&b, "ready 3.7.1 3"), "{:?}", texts(&b.scene(W, H)));
    assert!(
        shows(&b, "1,2,3"),
        "the $.ajax JSON arrived through the transport"
    );
    assert!(shows(&b, "four"));
    // The 50 ms fadeOut runs on the world clock.
    assert!(b.refresh_pending(1_000_000));
    for t in 1..=10u64 {
        b.tick(t * 20_000, http!(site)).unwrap();
    }
    assert!(!shows(&b, "Box"), "faded out");
    let li = b.document().unwrap().with_document(|d| {
        d.descendants(cw_web::dom::Document::ROOT)
            .filter(|n| d.is(*n, "li"))
            .nth(2)
            .unwrap()
    });
    let li = b.document().unwrap().id_of(li);
    b.click(&li, http!(site)).unwrap();
    assert!(
        shows(&b, "clicked three true idx 2"),
        "{:?}",
        texts(&b.scene(W, H))
    );
    b.click("go", http!(site)).unwrap();
    assert!(
        shows(&b, "submitted q=changed&s=y"),
        "{:?}",
        texts(&b.scene(W, H))
    );
    assert_eq!(
        b.url(),
        Some("https://app.test/jq.html"),
        "jQuery's submit handler prevented the navigation"
    );
}

#[test]
fn the_preact_fixture_is_driven_by_clicks_and_typing() {
    let mut site = library_site();
    let mut b = browser();
    b.navigate(&format!("{ORIGIN}/preact.html"), http!(site))
        .unwrap();
    assert!(console(&b).is_empty(), "{:?}", console(&b));
    assert!(
        shows(&b, "Count: ") && b.title().as_deref() == Some("Count 0"),
        "{:?} {:?}",
        b.title(),
        texts(&b.scene(W, H))
    );
    for _ in 0..3 {
        b.click("inc", http!(site)).unwrap();
        b.tick(b.clock + 20_000, http!(site)).unwrap();
    }
    assert_eq!(
        b.title().as_deref(),
        Some("Count 3"),
        "useEffect ran after each render"
    );
    assert!(shows(&b, "big!"));
    // A controlled input: every key goes through onInput and state.
    b.click("text", http!(site)).unwrap();
    b.text("hey").unwrap();
    b.tick(b.clock + 20_000, http!(site)).unwrap();
    assert!(
        shows(&b, "You typed: hey") || shows(&b, "hey"),
        "{:?}",
        texts(&b.scene(W, H))
    );
    assert_eq!(b.tab().fields["text"], "hey");
    let first = b.document().unwrap().with_document(|d| {
        d.descendants(cw_web::dom::Document::ROOT)
            .find(|n| d.is(*n, "li"))
            .unwrap()
    });
    let first = b.document().unwrap().id_of(first);
    b.click(&first, http!(site)).unwrap();
    b.tick(b.clock + 20_000, http!(site)).unwrap();
    let items = b.document().unwrap().with_document(|d| {
        d.descendants(cw_web::dom::Document::ROOT)
            .filter(|n| d.is(*n, "li"))
            .count()
    });
    assert_eq!(items, 2, "clicking an item removed it");
}

const LIVE: &str = r#"<!DOCTYPE html><title>Live</title><button id=inc>+</button><p id=n>n=0</p><p id=t>t=0</p><p id=r></p><canvas id=c width=40 height=20></canvas>
<script>
var n = 0, t = 0;
document.getElementById('inc').onclick = function () { n++; document.getElementById('n').textContent = 'n=' + n; console.log('click', n, Math.floor(Math.random() * 1000)); };
setInterval(function () { t++; document.getElementById('t').textContent = 't=' + t; }, 500);
document.getElementById('r').textContent = 'r=' + Math.floor(Math.random() * 1e6) + ' at ' + Date.now();
var g = document.getElementById('c').getContext('2d'); g.fillStyle = '#ff0000'; g.fillRect(0, 0, 40, 20);
</script>"#;

/// Clicks and ticks; returns the scene digests and the console after every step.
fn drive(b: &mut BrowserState, site: &mut Site, from: u64, steps: u64) -> (Vec<u64>, Vec<String>) {
    let mut scenes = Vec::new();
    for i in 0..steps {
        b.click("inc", http!(site)).unwrap();
        b.tick(from + (i + 1) * 300_000, http!(site)).unwrap();
        scenes.push(cw_scene::digest(&b.scene(W, H)));
    }
    (scenes, console(b))
}

#[test]
fn a_scripted_page_snapshots_restores_and_continues_identically() {
    let mut site = Site::new(&[("/", "text/html", LIVE)]);
    let mut b = browser();
    b.set_clock(10_000_000);
    b.navigate(&format!("{ORIGIN}/"), http!(site)).unwrap();
    drive(&mut b, &mut site, 10_000_000, 5);
    // The canvas the script filled is painted from the realm's raster.
    let scene = b.scene(W, H);
    let red = scene.nodes.iter().any(|n| matches!(&n.primitive, cw_scene::Primitive::Image { width: 40, height: 20, rgba } if rgba[..4] == [255, 0, 0, 255]));
    assert!(red, "the canvas paints its pixels");
    // Two kinds of snapshot: the in-memory clone the environment takes, and JSON.
    let cloned = b.clone();
    let json = serde_json::to_string(&b).unwrap();
    let (scenes_live, console_live) = drive(&mut b, &mut site, 11_500_000, 6);
    let mut from_clone = cloned;
    let (scenes_clone, console_clone) = drive(&mut from_clone, &mut site, 11_500_000, 6);
    let mut from_json: BrowserState = serde_json::from_str(&json).unwrap();
    from_json.set_entropy(42, "computer/test/browser");
    let (scenes_json, console_json) = drive(&mut from_json, &mut site, 11_500_000, 6);
    assert_eq!(scenes_live, scenes_clone);
    assert_eq!(console_live, console_clone);
    assert_eq!(scenes_live, scenes_json);
    assert_eq!(console_live, console_json);
    assert_eq!(console_live.len(), 11);
    assert_eq!(
        serde_json::to_string(&b).unwrap(),
        serde_json::to_string(&from_json).unwrap(),
        "the states agree too"
    );
}

#[test]
fn restore_cost_of_a_thousand_journaled_events() {
    let mut site = Site::new(&[("/", "text/html", LIVE)]);
    let mut b = browser();
    b.navigate(&format!("{ORIGIN}/"), http!(site)).unwrap();
    let started = std::time::Instant::now();
    for _ in 0..1000 {
        b.click("inc", http!(site)).unwrap();
    }
    let live = started.elapsed();
    let state = b.script_state().unwrap();
    let started = std::time::Instant::now();
    let snapshot = b.clone();
    let clone_cost = started.elapsed();
    let json = serde_json::to_string(&b).unwrap();
    // Move the live copy on, so the snapshot has to rebuild its own realm.
    b.click("inc", http!(site)).unwrap();
    let started = std::time::Instant::now();
    assert!(shows(&snapshot, "n=1000"));
    let restore = started.elapsed();
    println!(
        "1000 clicks live: {live:?}; journal: {} inputs, {} host answers, {} bytes of JSON for the whole browser; snapshot (clone): {clone_cost:?}; restore by replay + first paint: {restore:?}",
        state.inputs.len(),
        state.journal.len(),
        json.len()
    );
    assert!(shows(&b, "n=1001"));
    // The compaction boundary: a navigation builds a new realm, so no journal ever
    // spans one and the replay cost of a page is bounded by that page's own life.
    assert_eq!(
        b.document().unwrap().scripted().unwrap().journal_len(),
        4009
    );
    b.navigate(&format!("{ORIGIN}/?again"), http!(site))
        .unwrap();
    let fresh = b.document().unwrap().scripted().unwrap().journal_len();
    assert!(
        fresh <= 8,
        "a navigation starts a fresh journal, not a continued one: {fresh}"
    );
    let started = std::time::Instant::now();
    assert!(shows(&b, "n=0"));
    println!(
        "after a navigation the journal is {fresh} inputs and the page paints in {:?}",
        started.elapsed()
    );
}

const STATIC: &str = r#"<!DOCTYPE html><html><head><title>Static</title><link rel=stylesheet href=/site.css><style>h1{color:#123456} .box{float:right;width:120px;border:2px solid red;padding:4px}</style></head>
<body><h1>Static page</h1><div class=box>Floated <b>box</b></div><p>Some <a href=/x>link</a> text and <em>emphasis</em> that wraps across a few lines so the inline layout has work to do in this paragraph.</p>
<form><label for=q>Query</label> <input id=q name=q value=hello> <input type=checkbox checked> <select><option>a<option selected>b</select> <button>Go</button></form>
<table border=1><tr><td>1<td>2</table><img src=/pic.rgba width=10 height=10 alt=pic></body></html>"#;

#[test]
fn static_html_paints_identically_through_both_paths() {
    let files = [
        ("/", "text/html", STATIC),
        (
            "/site.css",
            "text/css",
            "body{font-family:serif;margin:20px} p{line-height:1.6}",
        ),
    ];
    let mut plain = browser();
    let mut site = Site::new(&files);
    plain.navigate(&format!("{ORIGIN}/"), http!(site)).unwrap();
    assert!(
        !plain.document().unwrap().is_scripted(),
        "no script: the cheaper path"
    );
    let mut scripted = browser();
    scripted.always_script = true;
    let mut site = Site::new(&files);
    scripted
        .navigate(&format!("{ORIGIN}/"), http!(site))
        .unwrap();
    assert!(scripted.document().unwrap().is_scripted());
    for (w, h) in [(W, H), (500, 700)] {
        plain.set_viewport(w, h);
        scripted.set_viewport(w, h);
        let (a, b) = (plain.scene(w, h), scripted.scene(w, h));
        assert_eq!(a.nodes.len(), b.nodes.len());
        for (x, y) in a.nodes.iter().zip(&b.nodes) {
            assert_eq!(x, y);
        }
        assert_eq!(cw_scene::digest(&a), cw_scene::digest(&b), "{w}x{h}");
    }
    assert_eq!(
        plain.current_page().unwrap().into_owned(),
        scripted.current_page().unwrap().into_owned()
    );
    assert_eq!(plain.tab().fields, scripted.tab().fields);
}
