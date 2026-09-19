//! Phones are driven the way phones are: iOS by swiping its home screen pages and its
//! home indicator, Android by its three navigation buttons. Every step here is a
//! `pointer.v1` event aimed at what the scene shows, and every check reads the state
//! the router really moved.
use computerworld::{reference_world, World};
use cw_protocol::{ActionEnvelope, EnvironmentConfig};
use serde_json::{json, Value};

/// Every native application, so a short iPhone has more than one home screen page.
const NATIVE: [&str; 13] = [
    "calendar",
    "mail",
    "messages",
    "docs",
    "notes",
    "contacts",
    "settings",
    "calculator",
    "clock",
    "photos",
    "music",
    "maps",
    "weather",
];

fn world(theme: &str) -> (World, String) {
    let mut definition = reference_world();
    definition.metadata["desktop_themes"] = json!({ "alice-mac": theme });
    definition.metadata["desktop_apps"] = Value::Array(
        NATIVE
            .iter()
            .map(|id| json!({"id":id,"label":id,"kind":"native","url":"","icon":id}))
            .collect(),
    );
    for computer in &mut definition.computers {
        if computer.id == "alice-mac" {
            computer
                .installed_apps
                .extend(NATIVE.iter().map(|id| id.to_string()));
        }
    }
    let mut world = World::new(definition, 11).unwrap();
    let actor = world
        .environment(EnvironmentConfig::desktop("alice", "alice-mac"))
        .unwrap();
    (world, actor)
}
fn step(world: &mut World, actor: &str, family: &str, op: &str, payload: Value) -> Value {
    let result = world
        .step(
            actor,
            vec![ActionEnvelope::new(family, op, "alice-mac", payload)],
        )
        .unwrap();
    assert!(result.outcomes[0].success, "{:?}", result.outcomes[0]);
    serde_json::to_value(&result.outcomes[0]).unwrap()
}
fn pointer(world: &mut World, actor: &str, op: &str, at: (i32, i32), size: (u32, u32)) -> Value {
    step(
        world,
        actor,
        "pointer.v1",
        op,
        json!({"x": at.0, "y": at.1, "width": size.0, "height": size.1}),
    )
}
fn swipe(
    world: &mut World,
    actor: &str,
    from: (i32, i32),
    to: (i32, i32),
    size: (u32, u32),
) -> Value {
    pointer(world, actor, "down", from, size);
    pointer(world, actor, "up", to, size)
}
fn tap(world: &mut World, actor: &str, at: (i32, i32), size: (u32, u32)) {
    pointer(world, actor, "down", at, size);
    pointer(world, actor, "up", at, size);
}
/// Centre of the control carrying `target`, wherever the shell painted it.
fn locate(world: &World, actor: &str, target: &str, size: (u32, u32)) -> Option<(i32, i32)> {
    let scene = world.scene(actor, size.0, size.1).unwrap();
    let node = scene
        .nodes
        .iter()
        .find(|n| n.interaction.as_deref() == Some(target))?;
    let b = node.transform.bounds(node.bounds);
    Some((b.x + b.width as i32 / 2, b.y + b.height as i32 / 2))
}
fn targets(world: &World, actor: &str, size: (u32, u32)) -> Vec<String> {
    world
        .scene(actor, size.0, size.1)
        .unwrap()
        .nodes
        .iter()
        .filter_map(|n| n.interaction.clone())
        .collect()
}
fn desktop(world: &World, actor: &str) -> Value {
    let session = world.interfaces().session(actor).unwrap();
    serde_json::to_value(&session.machines["alice-mac"].desktop).unwrap()
}
/// The page dot the scene marks selected.
fn current_dot(world: &World, actor: &str, size: (u32, u32)) -> Option<String> {
    world
        .scene(actor, size.0, size.1)
        .unwrap()
        .nodes
        .iter()
        .find(|n| {
            n.interaction
                .as_deref()
                .is_some_and(|i| i.starts_with("shell:home-page:"))
                && n.state.is_some_and(|s| s.selected == Some(true))
        })
        .and_then(|n| n.interaction.clone())
}

#[test]
fn ios_home_screen_pages_swipe_to_today_view_and_the_app_library() {
    let (mut world, actor) = world("virtual-ios-18");
    // A shorter iPhone: the whole roster needs two pages here.
    let size = (390, 600);
    let (left, right) = ((340, 300), (50, 300));
    assert_eq!(
        current_dot(&world, &actor, size).as_deref(),
        Some("shell:home-page:0")
    );
    let dots: Vec<_> = targets(&world, &actor, size)
        .into_iter()
        .filter(|t| t.starts_with("shell:home-page:"))
        .collect();
    assert_eq!(dots, ["shell:home-page:0", "shell:home-page:1"]);
    let first = targets(&world, &actor, size);
    // Right to left: the next page, a change the action result reports.
    let result = swipe(&mut world, &actor, left, right, size);
    assert!(result["effect"]["changed"]
        .as_array()
        .unwrap()
        .contains(&json!("focus")));
    assert_eq!(desktop(&world, &actor)["home_page"], 1);
    assert_eq!(
        current_dot(&world, &actor, size).as_deref(),
        Some("shell:home-page:1")
    );
    let second = targets(&world, &actor, size);
    let icons = |t: &[String]| -> Vec<String> {
        t.iter()
            .filter(|t| t.starts_with("shell:launch:"))
            .cloned()
            .collect()
    };
    assert_ne!(icons(&first), icons(&second));
    // Past the last page lies the App Library; swiping back returns to the last page.
    swipe(&mut world, &actor, left, right, size);
    assert_eq!(desktop(&world, &actor)["launcher_open"], true);
    swipe(&mut world, &actor, right, left, size);
    let d = desktop(&world, &actor);
    assert_eq!(
        (d["launcher_open"].clone(), d["home_page"].clone()),
        (json!(false), json!(1))
    );
    // Tapping a dot goes to its page.
    let dot = locate(&world, &actor, "shell:home-page:0", size).unwrap();
    tap(&mut world, &actor, dot, size);
    assert_eq!(desktop(&world, &actor)["home_page"], 0);
    // Left to right from the first page is Today View; right to left comes back.
    swipe(&mut world, &actor, right, left, size);
    assert_eq!(desktop(&world, &actor)["panel"], "calendar");
    assert!(targets(&world, &actor, size)
        .iter()
        .any(|t| t == "shell:month:next"));
    swipe(&mut world, &actor, left, right, size);
    let d = desktop(&world, &actor);
    assert_eq!(
        (d["panel"].clone(), d["home_page"].clone()),
        (Value::Null, json!(0))
    );
    // An application opened from the second page returns to it; the home gesture from
    // the home screen itself goes back to the first page.
    swipe(&mut world, &actor, left, right, size);
    let icon = icons(&targets(&world, &actor, size))[0].clone();
    let at = locate(&world, &actor, &icon, size).unwrap();
    tap(&mut world, &actor, at, size);
    assert!(desktop(&world, &actor)["focused"].is_u64());
    swipe(&mut world, &actor, (195, 590), (195, 480), size);
    let d = desktop(&world, &actor);
    assert!(d["focused"].is_null());
    assert_eq!(d["home_page"], 1);
    step(
        &mut world,
        &actor,
        "application.v1",
        "shell",
        json!({"target":"shell:gesture:home"}),
    );
    assert_eq!(desktop(&world, &actor)["home_page"], 0);
}

#[test]
fn the_home_indicator_is_swiped_not_tapped() {
    let (mut world, actor) = world("virtual-ios-18");
    let size = (390, 844);
    step(
        &mut world,
        &actor,
        "application.v1",
        "launch",
        json!({"kind":"notes"}),
    );
    let all = targets(&world, &actor, size);
    // No application screen paints a way "Home" or to the App Library; the indicator is
    // the only way out, and it is a gesture affordance.
    assert!(!all
        .iter()
        .any(|t| t == "shell:home" || t == "shell:launcher"));
    let bar = locate(&world, &actor, "shell:gesture:home", size).expect("home indicator");
    assert!(bar.1 > 800);
    // A tap does nothing, as it does on an iPhone.
    tap(&mut world, &actor, bar, size);
    step(
        &mut world,
        &actor,
        "pointer.v1",
        "click",
        json!({"x": bar.0, "y": bar.1, "width": size.0, "height": size.1}),
    );
    assert!(desktop(&world, &actor)["focused"].is_u64());
    // A long swipe up from it is the App Switcher; a short one goes home.
    swipe(&mut world, &actor, bar, (bar.0, 300), size);
    assert_eq!(desktop(&world, &actor)["panel"], "overview");
    let card = locate(&world, &actor, "window:0:focus", size).unwrap();
    tap(&mut world, &actor, card, size);
    assert_eq!(desktop(&world, &actor)["focused"], 0);
    swipe(&mut world, &actor, bar, (bar.0, bar.1 - 150), size);
    let d = desktop(&world, &actor);
    assert!(d["focused"].is_null());
    assert_eq!(
        d["windows"].as_object().unwrap().len(),
        1,
        "going home keeps the app"
    );
    // The status bar pull-downs are gestures too: tapped, nothing; pulled, the panel.
    let status = locate(&world, &actor, "shell:gesture:control-center", size).unwrap();
    tap(&mut world, &actor, status, size);
    assert!(desktop(&world, &actor)["panel"].is_null());
    swipe(&mut world, &actor, status, (status.0, 400), size);
    assert_eq!(desktop(&world, &actor)["panel"], "quick");
}

#[test]
fn android_navigation_buttons_go_back_home_and_to_recents() {
    let (mut world, actor) = world("virtual-android-15");
    let size = (412, 892);
    let bar = 892 - 24;
    let (back, home, recents) = ((103, bar), (206, bar), (309, bar));
    step(
        &mut world,
        &actor,
        "application.v1",
        "launch",
        json!({"kind":"files"}),
    );
    // The file manager's active tab, found whether or not it is in front.
    let tab = |w: &World| -> Value {
        let d = desktop(w, &actor);
        let window = d["windows"]
            .as_object()
            .unwrap()
            .values()
            .find(|w| w["state"]["type"] == "files")
            .expect("a file manager window")
            .clone();
        let active = window["state"]["active"].as_u64().unwrap() as usize;
        window["state"]["tabs"][active].clone()
    };
    let path = |w: &World| tab(w)["path"].as_str().unwrap().to_owned();
    // Into a folder, then Back: the file manager's own back stack comes first.
    let start = path(&world);
    let entries = tab(&world)["entries"].as_array().unwrap().clone();
    let folder = entries
        .iter()
        .position(|e| e.as_str().is_some_and(|n| n.ends_with('/')))
        .expect("a folder at home");
    let row = world
        .scene(&actor, size.0, size.1)
        .unwrap()
        .nodes
        .iter()
        .find(|n| {
            n.interaction
                .as_deref()
                .is_some_and(|i| i.ends_with(&format!(":content:open:{folder}")))
        })
        .map(|n| {
            let b = n.transform.bounds(n.bounds);
            (b.x + b.width as i32 / 2, b.y + b.height as i32 / 2)
        })
        .unwrap();
    tap(&mut world, &actor, row, size);
    assert_ne!(path(&world), start);
    tap(&mut world, &actor, back, size);
    assert_eq!(path(&world), start);
    // Back at the root of the application leaves it for the home screen.
    for _ in 0..8 {
        if path(&world).trim_end_matches('/').is_empty() {
            break;
        }
        tap(&mut world, &actor, back, size);
        assert!(desktop(&world, &actor)["focused"].is_u64());
    }
    assert!(path(&world).trim_end_matches('/').is_empty());
    tap(&mut world, &actor, back, size);
    let d = desktop(&world, &actor);
    assert!(d["focused"].is_null());
    assert_eq!(d["windows"].as_object().unwrap().len(), 1);
    // Recents opens the overview; Home from there goes home.
    tap(&mut world, &actor, recents, size);
    assert_eq!(desktop(&world, &actor)["panel"], "overview");
    let card = locate(&world, &actor, "window:0:focus", size).unwrap();
    tap(&mut world, &actor, card, size);
    assert_eq!(desktop(&world, &actor)["focused"], 0);
    tap(&mut world, &actor, home, size);
    assert!(desktop(&world, &actor)["focused"].is_null());
    // Back closes a panel before anything else.
    step(
        &mut world,
        &actor,
        "application.v1",
        "shell",
        json!({"target":"shell:gesture:notifications"}),
    );
    assert_eq!(desktop(&world, &actor)["panel"], "notifications");
    tap(&mut world, &actor, back, size);
    assert!(desktop(&world, &actor)["panel"].is_null());
    // The status bar opens nothing when tapped, and the shade when pulled.
    tap(&mut world, &actor, (200, 12), size);
    assert!(desktop(&world, &actor)["panel"].is_null());
    swipe(&mut world, &actor, (200, 12), (200, 300), size);
    assert_eq!(desktop(&world, &actor)["panel"], "notifications");
    // A card swiped up in the overview closes its application.
    tap(&mut world, &actor, recents, size);
    assert_eq!(desktop(&world, &actor)["panel"], "overview");
    let card = locate(&world, &actor, "window:0:focus", size).unwrap();
    swipe(&mut world, &actor, card, (card.0, card.1 - 250), size);
    assert!(desktop(&world, &actor)["windows"]
        .as_object()
        .unwrap()
        .is_empty());
}
