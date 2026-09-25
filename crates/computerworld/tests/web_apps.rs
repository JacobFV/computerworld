//! An application written as a web app, registered with the world like any
//! application: it opens in a desktop window whose frame is the platform's, is driven
//! by pointer and keyboard through the scene it paints, reaches the machine only
//! through the `cw` global (files, services, launching, named data), and survives a
//! snapshot as the state it declared.
use computerworld::{reference_world, World};
use cw_protocol::{ActionEnvelope, EnvironmentConfig};
use cw_sdk::{WebApplication, WebSource};
use serde_json::{json, Value};

const W: u32 = 1100;
const H: u32 = 720;

/// A counter in plain script, no framework: the host needs only a document.
const COUNTER: &str = r#"
const saved = cw.state.get();
const state = saved ?? { count: 0, status: "" };
if (saved === null) cw.state.set(state);
function change(patch) {
  Object.assign(state, patch);
  cw.state.set(Object.assign({}, state));
  render();
}
function render() {
  document.getElementById("root").innerHTML =
    '<h1 id="counter-title">Counter</h1>' +
    '<p id="counter-value" role="status">' + state.count + '</p>' +
    '<button id="counter:add">Add</button>' +
    '<button id="counter:save">Save</button>' +
    '<button id="counter:fetch">Fetch</button>' +
    '<button id="counter:notes">Notes</button>' +
    '<button id="counter:load">Load</button>' +
    '<input id="counter:name" aria-label="Name">' +
    '<p id="counter-status" role="status">' + state.status + '</p>';
}
document.addEventListener("click", (event) => {
  switch (event.target.id) {
    case "counter:add":
      change({ count: state.count + 1 });
      cw.emit("counter.added", { count: state.count });
      break;
    case "counter:save":
      cw.fs.mkdir("/Users/alice/Counter");
      cw.fs.writeFile("/Users/alice/Counter/count.txt", String(state.count)).then(
        () => change({ status: "saved" }),
        (error) => change({ status: "failed: " + error.message }),
      );
      break;
    case "counter:fetch":
      cw.fetch("http://calendar.internal/").then(
        (response) => change({ status: "fetched " + response.status }),
        (error) => change({ status: "offline: " + error.message }),
      );
      break;
    case "counter:notes":
      cw.launch("notes", "/Users/alice/Counter");
      break;
    case "counter:load":
      // Offline, fall back to the copy on disk: the read is asked for while the
      // failure is answered, and goes out with the next input.
      cw.fetch("http://nowhere.invalid/count").then(
        (response) => change({ status: "fetched " + response.status }),
        () =>
          cw.fs.readFile("/Users/alice/count.txt").then(
            (text) => change({ status: "loaded " + text }),
            (error) => change({ status: "unreadable: " + error.message }),
          ),
      );
      break;
  }
});
render();
"#;

fn counter() -> WebApplication {
    WebApplication {
        kind: "counter".into(),
        version: 1,
        titles: [("*".to_owned(), "Counter".to_owned())]
            .into_iter()
            .collect(),
        source: WebSource::Script {
            script: COUNTER.into(),
            style: "button { margin: 4px; } #counter-value { font-size: 24px; }".into(),
            react: false,
        },
    }
}

fn world() -> (World, String) {
    let mut d = reference_world();
    d.metadata["desktop_themes"] = json!({ "alice-mac": "virtual-macos-golden-gate" });
    for computer in &mut d.computers {
        if computer.id == "alice-mac" {
            computer
                .installed_apps
                .extend(["counter", "notes"].map(str::to_owned));
        }
    }
    let mut world = World::new(d, 42).unwrap();
    world.register_web_application(counter()).unwrap();
    let actor = world
        .environment(EnvironmentConfig::desktop("alice", "alice-mac"))
        .unwrap();
    (world, actor)
}

fn act(world: &mut World, actor: &str, family: &str, op: &str, payload: Value) {
    let result = world
        .step(
            actor,
            vec![ActionEnvelope::new(family, op, "alice-mac", payload)],
        )
        .unwrap();
    assert!(result.outcomes[0].success, "{:?}", result.outcomes[0]);
}

fn click(world: &mut World, actor: &str, target: &str) {
    let scene = world.scene(actor, W, H).unwrap();
    let suffix = format!(":content:{target}");
    let node = scene
        .nodes
        .iter()
        .find(|n| {
            n.interaction
                .as_deref()
                .is_some_and(|i| i.ends_with(&suffix))
        })
        .unwrap_or_else(|| panic!("missing interaction {target}"));
    let b = node.transform.bounds(node.bounds);
    act(
        world,
        actor,
        "pointer.v1",
        "click",
        json!({"x": b.x + (b.width / 2) as i32, "y": b.y + (b.height / 2) as i32, "width": W, "height": H}),
    );
}

fn windows(world: &World, actor: &str) -> Vec<Value> {
    let session = world.interfaces().session(actor).unwrap();
    let desktop = serde_json::to_value(&session.machines["alice-mac"].desktop).unwrap();
    desktop["windows"]
        .as_object()
        .unwrap()
        .values()
        .cloned()
        .collect()
}

fn counter_state(world: &World, actor: &str) -> Value {
    windows(world, actor)
        .into_iter()
        .find(|w| w["state"]["kind"] == "counter")
        .expect("a counter window")["state"]["state"]
        .clone()
}

fn painted(world: &World, actor: &str, text: &str) -> bool {
    world
        .scene(actor, W, H)
        .unwrap()
        .nodes
        .iter()
        .any(|n| n.painted_text() == Some(text))
}

#[test]
fn a_registered_web_app_runs_in_a_desktop_window() {
    let (mut world, actor) = world();
    act(
        &mut world,
        &actor,
        "application.v1",
        "launch",
        json!({"kind": "counter"}),
    );
    assert_eq!(
        counter_state(&world, &actor),
        json!({"count": 0, "status": ""})
    );
    // The frame is the platform's and titles the window; the content is the document.
    assert!(painted(&world, &actor, "Counter"));
    click(&mut world, &actor, "counter:add");
    click(&mut world, &actor, "counter:add");
    assert_eq!(counter_state(&world, &actor)["count"], 2);
    assert!(painted(&world, &actor, "2"));
    // Named data went to the world's event log.
    let added: Vec<_> = world
        .trajectory()
        .into_iter()
        .filter(|e| e.kind == "counter.added")
        .map(|e| e.data)
        .collect();
    assert_eq!(added, vec![json!({"count": 1}), json!({"count": 2})]);
    // Files go through the machine, with the user's permissions.
    click(&mut world, &actor, "counter:save");
    assert_eq!(counter_state(&world, &actor)["status"], "saved");
    let bytes = world
        .runtime()
        .read_file("alice-mac", "/Users/alice/Counter/count.txt")
        .unwrap();
    assert_eq!(bytes, b"2");
    // Services are reached through the machine's network.
    click(&mut world, &actor, "counter:fetch");
    let status = counter_state(&world, &actor)["status"].clone();
    assert!(status.as_str().unwrap().starts_with("fetched "), "{status}");
    // Typing goes to the focused field.
    click(&mut world, &actor, "counter:name");
    act(
        &mut world,
        &actor,
        "keyboard.v1",
        "type",
        json!({"text": "ada"}),
    );
    let observation = world.observe(&actor).unwrap();
    let focus = serde_json::to_value(&observation).unwrap().to_string();
    assert!(focus.contains("counter:name"), "the field has the focus");
    // Launching opens another application's window.
    click(&mut world, &actor, "counter:notes");
    assert!(windows(&world, &actor)
        .iter()
        .any(|w| w["state"]["app"] == "notes"));
}

#[test]
fn a_web_app_window_survives_a_snapshot_and_a_fork() {
    let (mut world, actor) = world();
    act(
        &mut world,
        &actor,
        "application.v1",
        "launch",
        json!({"kind": "counter"}),
    );
    click(&mut world, &actor, "counter:add");
    let snapshot = world.snapshot();
    let mut fork = world.fork(&snapshot).unwrap();
    assert_eq!(fork.state_hash().unwrap(), world.state_hash().unwrap());
    assert_eq!(counter_state(&fork, &actor)["count"], 1);
    // The restored window shows what the live one does, and the two go their own ways.
    assert_eq!(
        fork.scene(&actor, W, H).unwrap().nodes,
        world.scene(&actor, W, H).unwrap().nodes
    );
    click(&mut fork, &actor, "counter:add");
    assert_eq!(counter_state(&fork, &actor)["count"], 2);
    assert_eq!(counter_state(&world, &actor)["count"], 1);
    // An exported snapshot imports into a world that registered the same application.
    let exported = world.export_snapshot().unwrap();
    let (mut other, _) = self::world();
    other.import_snapshot(&exported).unwrap();
    assert_eq!(other.state_hash().unwrap(), world.state_hash().unwrap());
    assert!(painted(&other, &actor, "1"));
}

/// A window saved while a request it made is still outstanding keeps it through the
/// world's snapshot: the restored window, like the live one, gets the machine's answer.
#[test]
fn a_request_outstanding_at_a_snapshot_is_answered_after_the_restore() {
    let (mut world, actor) = world();
    act(
        &mut world,
        &actor,
        "filesystem.v1",
        "write",
        json!({"path": "/Users/alice/count.txt", "content": "41"}),
    );
    act(
        &mut world,
        &actor,
        "application.v1",
        "launch",
        json!({"kind": "counter"}),
    );
    // The service is unreachable, so the counter asks for its file instead; that
    // read is outstanding when the step ends.
    click(&mut world, &actor, "counter:load");
    assert_eq!(counter_state(&world, &actor)["status"], "");
    let window = windows(&world, &actor)
        .into_iter()
        .find(|w| w["state"]["kind"] == "counter")
        .unwrap();
    assert!(
        window["state"]["inflight"].is_object(),
        "the outstanding read is saved"
    );
    let snapshot = world.snapshot();
    let mut fork = world.fork(&snapshot).unwrap();
    assert_eq!(fork.state_hash().unwrap(), world.state_hash().unwrap());
    let exported = world.export_snapshot().unwrap();
    let (mut imported, _) = self::world();
    imported.import_snapshot(&exported).unwrap();
    for w in [&mut world, &mut fork, &mut imported] {
        click(w, &actor, "counter:add");
        assert_eq!(counter_state(w, &actor)["status"], "loaded 41");
        assert_eq!(counter_state(w, &actor)["count"], 1);
    }
    assert_eq!(fork.state_hash().unwrap(), world.state_hash().unwrap());
    assert_eq!(imported.state_hash().unwrap(), world.state_hash().unwrap());
}
