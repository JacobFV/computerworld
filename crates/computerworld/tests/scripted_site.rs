//! A static-site directory whose pages carry JavaScript, driven end to end through
//! the agent API: the browser runs the page in a script realm, every agent action is
//! a DOM event first, the `setInterval` counter advances with the world clock, a
//! `fetch` POST reaches the service, the SPA router moves without a request, the
//! console reaches the `browser.v1` observation, and `localStorage` survives a
//! reload. The whole world snapshots and restores mid-interaction.
use computerworld::{reference_world, World};
use cw_protocol::{ActionEnvelope, EnvironmentConfig, ServiceDefinition};
use serde_json::{json, Value};

const MACHINE: &str = "alice-mac";
const ACTOR: &str = "alice";
const HOME: &str = "http://ledger.example:8091/";
/// One second of world time, in the clock's microseconds.
const S: u64 = 1_000_000;

const INDEX: &str = r#"<!DOCTYPE html><html><head><title>Ledger</title><link rel=stylesheet href=/site.css></head>
<body>
<h1>Ledger</h1>
<nav><a id=home href="/">Home</a> <a id=about href="/about">About</a></nav>
<p id=view></p>
<button id=tab-list class=tab>Entries</button> <button id=tab-add class=tab>Add</button>
<section id=panel-list class="panel on"><ul id=entries><li>loading</li></ul></section>
<section id=panel-add class=panel>
  <form id=note action=/api/records/note method=post>
    <label for=who>Name</label> <input id=who name=who> <button id=save>Save</button>
  </form>
  <p id=msg></p>
</section>
<p id=clock>ticks 0</p>
<p id=visits></p>
<script src=/app.js></script>
</body></html>"#;

const APP_JS: &str = r#"
function render() {
  var about = location.pathname === '/about';
  document.getElementById('view').textContent = about ? 'About this ledger' : 'Recent entries';
  document.title = about ? 'About' : 'Ledger';
}
document.querySelectorAll('nav a').forEach(function (a) {
  a.addEventListener('click', function (e) {
    e.preventDefault();
    history.pushState({ path: a.getAttribute('href') }, '', a.getAttribute('href'));
    render();
  });
});
window.addEventListener('popstate', render);
render();

document.querySelectorAll('.tab').forEach(function (t) {
  t.addEventListener('click', function () {
    document.querySelectorAll('.panel').forEach(function (p) { p.classList.remove('on'); });
    document.getElementById(t.id.replace('tab-', 'panel-')).classList.add('on');
  });
});

function load() {
  return fetch('/api/records').then(function (r) { return r.json(); }).then(function (d) {
    var names = Object.keys(d);
    document.getElementById('entries').innerHTML = names.length
      ? names.map(function (k) { return '<li>' + k + ': ' + d[k] + '</li>'; }).join('')
      : '<li>no entries yet</li>';
    console.log('entries', names.length);
  });
}
load();

document.getElementById('note').addEventListener('submit', function (e) {
  e.preventDefault();
  var who = document.getElementById('who').value;
  if (!who) { document.getElementById('msg').textContent = 'a name is required'; return; }
  fetch('/api/records/note', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(who) })
    .then(function (r) { return r.json(); })
    .then(function (v) { document.getElementById('msg').textContent = 'saved ' + v; return load(); });
});

var visits = Number(localStorage.getItem('visits') || 0) + 1;
localStorage.setItem('visits', String(visits));
document.getElementById('visits').textContent = 'visits ' + visits;

var n = 0;
setInterval(function () { n++; document.getElementById('clock').textContent = 'ticks ' + n; }, 1000);
"#;

const SITE_CSS: &str = "body{font-family:sans-serif;margin:16px} .panel{display:none} .panel.on{display:block} nav a{margin-right:8px}";

fn definition() -> cw_protocol::WorldDefinition {
    let mut definition = reference_world();
    definition.services.push(ServiceDefinition {
        id: "ledger-site".into(),
        kind: "static-site".into(),
        node: "app-server".into(),
        domains: vec!["ledger.example".into()],
        port: 8091,
        tls: false,
        initial_state: json!({"files":{
            "/index.html": INDEX,
            "/app.js": APP_JS,
            "/site.css": SITE_CSS,
        }}),
    });
    definition
}

fn world() -> (World, String) {
    let mut world = World::new(definition(), 42).unwrap();
    let session = world
        .environment(EnvironmentConfig {
            actor: ACTOR.into(),
            machines: vec![MACHINE.into()],
            actions: vec!["browser.v1".into(), "keyboard.v1".into()],
            observations: vec!["semantic.v1".into(), "browser.v1".into()],
            action_budget: 1 << 20,
        })
        .unwrap();
    (world, session)
}

fn act(world: &mut World, session: &str, op: &str, payload: Value) -> Value {
    let result = world
        .step(
            session,
            vec![ActionEnvelope::new("browser.v1", op, MACHINE, payload)],
        )
        .unwrap();
    assert!(result.outcomes[0].success, "{op}: {:?}", result.outcomes[0]);
    result.outcomes[0].value.clone()
}
/// Lets the world clock run, then takes an empty step: what the environment does
/// between an agent's actions, which is where page timers get their turn.
fn wait(world: &mut World, session: &str, micros: u64) {
    world.runtime_mut().advance(micros).unwrap();
    world.step(session, vec![]).unwrap();
}
fn browser(world: &World, session: &str) -> Value {
    world.observe(session).unwrap().channels["browser.v1"][MACHINE].clone()
}
fn page(world: &World, session: &str) -> Value {
    world.observe(session).unwrap().channels["semantic.v1"][MACHINE].clone()
}
fn texts(page: &Value) -> Vec<String> {
    fn walk(elements: &[Value], out: &mut Vec<String>) {
        for e in elements {
            for key in ["text", "label"] {
                if let Some(s) = e[key].as_str() {
                    out.push(s.to_owned());
                }
            }
            if let Some(children) = e["children"].as_array() {
                walk(children, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(page["elements"].as_array().unwrap(), &mut out);
    out
}
fn shows(world: &World, session: &str, needle: &str) -> bool {
    texts(&page(world, session))
        .iter()
        .any(|t| t.contains(needle))
}
fn console(world: &World, session: &str) -> Vec<String> {
    browser(world, session)["console"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|e| {
                    format!(
                        "{}: {}",
                        e["level"].as_str().unwrap_or(""),
                        e["text"].as_str().unwrap_or("")
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_scripted_static_site_runs_end_to_end_through_the_agent_api() {
    let (mut world, session) = world();
    act(&mut world, &session, "navigate", json!({"url": HOME}));

    // The page ran: its `fetch` filled the list, `localStorage` counted the visit,
    // the SPA router drew the home view, and the hidden panel is not in the reading.
    assert_eq!(browser(&world, &session)["page"]["title"], "Ledger");
    assert!(
        shows(&world, &session, "no entries yet"),
        "{:?}",
        texts(&page(&world, &session))
    );
    assert!(shows(&world, &session, "Recent entries"));
    assert!(shows(&world, &session, "visits 1"));
    assert!(
        !shows(&world, &session, "loading"),
        "the fetch replaced the placeholder"
    );
    assert!(
        !shows(&world, &session, "Name"),
        "the Add panel is display:none"
    );
    assert_eq!(console(&world, &session), vec!["log: entries 0"]);

    // A click on a tab is a DOM event; the panel it shows is what the reader sees.
    act(&mut world, &session, "click", json!({"id":"tab-add"}));
    assert!(
        shows(&world, &session, "Name"),
        "{:?}",
        texts(&page(&world, &session))
    );
    assert!(!shows(&world, &session, "no entries yet"));

    // Typing goes to the focused control and fires `input`; the submit handler calls
    // `preventDefault` and posts with `fetch`, so the tab never navigates.
    act(&mut world, &session, "click", json!({"id":"who"}));
    let typed = world
        .step(
            &session,
            vec![ActionEnvelope::new(
                "keyboard.v1",
                "type",
                MACHINE,
                json!({"text":"rent"}),
            )],
        )
        .unwrap();
    assert!(typed.outcomes[0].success, "{:?}", typed.outcomes[0]);
    assert_eq!(browser(&world, &session)["fields"]["who"], "rent");
    act(&mut world, &session, "click", json!({"id":"save"}));
    assert_eq!(
        browser(&world, &session)["url"],
        HOME,
        "preventDefault: no navigation"
    );
    assert!(
        shows(&world, &session, "saved rent"),
        "{:?}",
        texts(&page(&world, &session))
    );
    act(&mut world, &session, "click", json!({"id":"tab-list"}));
    assert!(
        shows(&world, &session, "note: rent"),
        "the POST reached the service"
    );
    assert_eq!(
        console(&world, &session),
        vec!["log: entries 0", "log: entries 1"]
    );

    // The `setInterval` counter advances with the world clock, not with actions.
    assert!(shows(&world, &session, "ticks 0"));
    wait(&mut world, &session, 3 * S);
    assert!(
        shows(&world, &session, "ticks 3"),
        "{:?}",
        texts(&page(&world, &session))
    );

    // The router pushes a same-document entry: no request, and `back` fires popstate.
    act(&mut world, &session, "click", json!({"id":"about"}));
    assert_eq!(
        browser(&world, &session)["url"],
        "http://ledger.example:8091/about"
    );
    assert!(shows(&world, &session, "About this ledger"));
    act(&mut world, &session, "back", json!({}));
    assert_eq!(browser(&world, &session)["url"], HOME);
    assert!(shows(&world, &session, "Recent entries"));

    // A snapshot taken mid-interaction restores the realm by replaying its journal:
    // the counter, the list and storage all come back, and time keeps running.
    let snapshot = world.export_snapshot().unwrap();
    wait(&mut world, &session, 2 * S);
    assert!(shows(&world, &session, "ticks 5"));
    let mut restored = World::new(definition(), 42).unwrap();
    restored.import_snapshot(&snapshot).unwrap();
    assert!(
        shows(&restored, &session, "ticks 3"),
        "{:?}",
        texts(&page(&restored, &session))
    );
    assert!(shows(&restored, &session, "note: rent"));
    wait(&mut restored, &session, 2 * S);
    assert!(
        shows(&restored, &session, "ticks 5"),
        "{:?}",
        texts(&page(&restored, &session))
    );
    assert_eq!(
        page(&restored, &session),
        page(&world, &session),
        "the restored world shows what the live one does"
    );

    // A reload re-runs the page against the storage it left behind.
    act(&mut world, &session, "reload", json!({}));
    assert!(
        shows(&world, &session, "visits 2"),
        "{:?}",
        texts(&page(&world, &session))
    );
    assert!(
        shows(&world, &session, "note: rent"),
        "the list comes back from the service"
    );
}
