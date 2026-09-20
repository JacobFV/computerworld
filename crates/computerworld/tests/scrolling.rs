//! Scrolling, text focus and the phone shells' platform surfaces, end to end. Every
//! step is a pointer or keyboard action aimed at what the scene shows, and every check
//! reads the published scene (its `scrolls`, its focus) or the machine back.
use computerworld::{reference_world, World};
use cw_protocol::{ActionEnvelope, EnvironmentConfig};
use cw_scene::{Scene, ScrollArea};
use serde_json::{json, Value};

const APPS: [&str; 8] = [
    "notes",
    "messages",
    "mail",
    "calculator",
    "calendar",
    "settings",
    "spreadsheet",
    "weather",
];

struct Desk {
    world: World,
    actor: String,
    size: (u32, u32),
}

impl Desk {
    fn new(theme: &str, presentation: Option<&str>, size: (u32, u32)) -> Self {
        let mut d = reference_world();
        d.metadata["desktop_themes"] = json!({ "alice-mac": theme });
        let urls = [
            ("messages", "http://messages.internal/"),
            ("mail", "http://mail.internal/"),
            ("weather", "http://weather.com/"),
        ];
        d.metadata["desktop_apps"] = Value::Array(
            APPS.iter()
                .map(|id| {
                    let url = urls.iter().find(|(k, _)| k == id).map_or("", |(_, u)| u);
                    json!({"id":id,"label":id,"kind":"native","url":url,"icon":id})
                })
                .collect(),
        );
        if let Some(p) = presentation {
            d.metadata["device_presentations"] = json!({ "alice-mac": p });
        }
        for c in &mut d.computers {
            if c.id == "alice-mac" {
                c.installed_apps.extend(APPS.iter().map(|a| a.to_string()));
            }
        }
        let mut world = World::new(d, 17).unwrap();
        let mut config = EnvironmentConfig::desktop("alice", "alice-mac");
        config.observations.push("pixels.v1".into());
        let actor = world.environment(config).unwrap();
        Self { world, actor, size }
    }
    fn try_act(&mut self, family: &str, op: &str, payload: Value) -> Result<Value, String> {
        let r = self
            .world
            .step(
                &self.actor,
                vec![ActionEnvelope::new(family, op, "alice-mac", payload)],
            )
            .unwrap();
        let o = &r.outcomes[0];
        if o.success {
            Ok(o.value.clone())
        } else {
            Err(format!("{:?}", o.error))
        }
    }
    fn act(&mut self, family: &str, op: &str, payload: Value) -> Value {
        self.try_act(family, op, payload)
            .unwrap_or_else(|e| panic!("{family} {op}: {e}"))
    }
    fn scene(&self) -> Scene {
        self.world
            .scene(&self.actor, self.size.0, self.size.1)
            .unwrap()
    }
    fn launch(&mut self, kind: &str, argument: &str) {
        self.act(
            "application.v1",
            "launch",
            json!({"kind": kind, "argument": argument}),
        );
    }
    fn shell(&mut self, target: &str) -> Value {
        self.act("application.v1", "shell", json!({ "target": target }))
    }
    fn pointer(&mut self, op: &str, (x, y): (i32, i32)) -> Value {
        self.act(
            "pointer.v1",
            op,
            json!({"x": x, "y": y, "width": self.size.0, "height": self.size.1}),
        )
    }
    fn swipe(&mut self, from: (i32, i32), to: (i32, i32)) {
        self.pointer("down", from);
        self.pointer("up", to);
    }
    fn wheel(&mut self, at: (i32, i32), dy: i32, modifiers: &[&str]) -> bool {
        self.act(
            "pointer.v1",
            "wheel",
            json!({"x": at.0, "y": at.1, "width": self.size.0, "height": self.size.1,
                   "delta_y": dy, "modifiers": modifiers}),
        )["handled"]
            .as_bool()
            .unwrap()
    }
    /// Centre of the painted control whose target is or ends with `:content:<target>`.
    fn find(&self, target: &str) -> Option<(i32, i32)> {
        let scene = self.scene();
        let suffix = format!(":content:{target}");
        let n = scene.nodes.iter().find(|n| {
            n.interaction
                .as_deref()
                .is_some_and(|i| i == target || i.ends_with(&suffix))
        })?;
        let b = n.transform.bounds(n.bounds);
        let at = (b.x + b.width as i32 / 2, b.y + b.height as i32 / 2);
        (scene
            .hit_test(at.0, at.1)
            .and_then(|h| h.interaction.clone())
            == n.interaction)
            .then_some(at)
    }
    fn click(&mut self, target: &str) {
        let at = self
            .find(target)
            .unwrap_or_else(|| panic!("{target} is not painted and on top"));
        self.pointer("click", at);
    }
    fn tap(&mut self, target: &str) {
        let at = self
            .find(target)
            .unwrap_or_else(|| panic!("{target} is not painted and on top"));
        self.pointer("down", at);
        self.pointer("up", at);
    }
    /// The inner target (after `:content:`) of the first painted control starting with
    /// `prefix`.
    fn first_target(&self, prefix: &str) -> String {
        self.scene()
            .nodes
            .iter()
            .filter_map(|n| n.interaction.as_deref())
            .filter_map(|i| i.split_once(":content:").map(|(_, t)| t.to_owned()))
            .find(|t| t.starts_with(prefix))
            .unwrap_or_else(|| panic!("nothing painted starts with {prefix}"))
    }
    /// What one action changed, as its effect tags.
    fn changed(&mut self, family: &str, op: &str, payload: Value) -> Vec<String> {
        let r = self
            .world
            .step(
                &self.actor,
                vec![ActionEnvelope::new(family, op, "alice-mac", payload)],
            )
            .unwrap();
        let o = &r.outcomes[0];
        assert!(o.success, "{family} {op}: {:?}", o.error);
        o.effect.as_ref().unwrap().changed.clone()
    }
    fn area(&self, pane: &str) -> ScrollArea {
        self.scene()
            .scrolls
            .into_iter()
            .find(|a| a.target.ends_with(&format!(":content:pane:{pane}")))
            .unwrap_or_else(|| panic!("no pane {pane} is published"))
    }
    fn keyboard_up(&self) -> bool {
        let scene = self.scene();
        let painted = scene.nodes.iter().any(|n| {
            n.interaction
                .as_deref()
                .is_some_and(|i| i.starts_with("shell:type:"))
        });
        let published = scene.focus.as_ref().is_some_and(|f| f.keyboard.text_entry);
        assert_eq!(
            painted, published,
            "keyboard painted != text entry published"
        );
        painted
    }
    fn desktop(&self) -> Value {
        let session = self.world.interfaces().session(&self.actor).unwrap();
        serde_json::to_value(&session.machines["alice-mac"].desktop).unwrap()
    }
    /// Forty notes, more than any window shows at once.
    fn many_notes(&mut self) {
        self.act(
            "terminal.v1",
            "execute",
            json!({"command": "mkdir -p /Users/alice/Notes"}),
        );
        for i in 0..40 {
            self.act(
                "filesystem.v1",
                "write",
                json!({"path": format!("/Users/alice/Notes/note-{i:02}.txt"), "content": "x"}),
            );
        }
        self.launch("notes", "/Users/alice/Notes");
    }
}

#[test]
fn the_wheel_scrolls_a_list_to_rows_that_were_out_of_view() {
    for theme in [
        "virtual-macos-golden-gate",
        "virtual-windows-11",
        "virtual-ubuntu-24",
    ] {
        let mut d = Desk::new(theme, None, (1280, 800));
        d.many_notes();
        let list = d.area("list");
        assert_eq!(list.offset, 0, "{theme}");
        assert!(
            list.extent > list.bounds.height,
            "{theme}: the list overflows"
        );
        assert!(d.find("notes:open:note-39.txt").is_none(), "{theme}");
        let inside = (list.bounds.x + 40, list.bounds.y + 40);
        // Towards the user moves the content up; far enough reaches the last row.
        assert!(d.wheel(inside, 120, &[]), "{theme}");
        assert_eq!(d.area("list").offset, 120, "{theme}");
        assert!(d.wheel(inside, 100_000, &[]), "{theme}");
        let end = d.area("list");
        assert_eq!(end.offset, end.max_offset(), "{theme}: clamped to the end");
        assert!(d.find("notes:open:note-39.txt").is_some(), "{theme}");
        assert!(d.find("notes:open:note-00.txt").is_none(), "{theme}");
        // At the end the list cannot move further, so the wheel is not handled.
        assert!(!d.wheel(inside, 120, &[]), "{theme}");
        // Opening a note is a click on the row that scrolled into view.
        d.click("notes:open:note-39.txt");
        assert_eq!(
            d.desktop()["windows"]["0"]["state"]["open"],
            json!("note-39.txt"),
            "{theme}"
        );
        // Scroll position is window state: it survives a snapshot round trip.
        let snapshot = d.world.snapshot();
        d.wheel(inside, -100_000, &[]);
        assert_eq!(d.area("list").offset, 0);
        d.world.restore(&snapshot).unwrap();
        assert_eq!(d.area("list").offset, end.max_offset(), "{theme}");
    }
}

#[test]
fn a_scroll_bar_thumb_drags_and_its_track_jumps() {
    let mut d = Desk::new("virtual-macos-golden-gate", None, (1280, 800));
    d.many_notes();
    let list = d.area("list");
    let bar = d
        .scene()
        .nodes
        .into_iter()
        .find(|n| {
            n.interaction
                .as_deref()
                .is_some_and(|i| i.contains(":content:pane:list:"))
        })
        .expect("an overflowing list paints a scroll bar");
    assert_eq!(bar.semantic.as_ref().unwrap().role, "scrollbar");
    let b = bar.transform.bounds(bar.bounds);
    let x = b.x + b.width as i32 / 2;
    // Grab the thumb at its top and pull it all the way down.
    d.pointer("down", (x, b.y + 4));
    d.pointer("move", (x, b.y + b.height as i32 / 2));
    assert!(d.area("list").offset > 0, "the list follows the thumb");
    d.pointer("up", (x, b.y + b.height as i32 + 200));
    assert_eq!(d.area("list").offset, list.max_offset());
    // A click on the track, near its top, jumps back up.
    d.pointer("click", (x, b.y + 2));
    assert_eq!(d.area("list").offset, 0);
}

#[test]
fn a_phone_scrolls_under_the_finger_and_ios_collapses_its_large_title() {
    let mut d = Desk::new("virtual-ios-18", None, (390, 844));
    d.many_notes();
    let main = d.area("main");
    assert_eq!(main.title.as_deref(), Some("Notes"));
    assert!(!main.title_collapsed());
    // The title is shown once: large in the content, not again in the bar.
    let bar_titles = |d: &Desk| {
        d.scene()
            .nodes
            .iter()
            .filter(|n| n.painted_text() == Some("Notes"))
            .filter(|n| n.transform.bounds(n.bounds).y < 96)
            .count()
    };
    assert_eq!(
        bar_titles(&d),
        0,
        "no inline title over an expanded large title"
    );
    // A finger dragged up the list scrolls it and opens nothing.
    d.swipe((200, 700), (200, 400));
    let scrolled = d.area("main");
    assert_eq!(scrolled.offset, 300);
    assert!(scrolled.title_collapsed());
    assert_eq!(bar_titles(&d), 1, "the collapsed title moves into the bar");
    assert_eq!(d.desktop()["windows"]["0"]["state"]["open"], Value::Null);
    // A short drag is a tap: it opens the row under it.
    let row = d.find("notes:open:note-20.txt").expect("row 20 on screen");
    d.pointer("down", row);
    d.pointer("up", (row.0 + 3, row.1 + 3));
    assert_eq!(
        d.desktop()["windows"]["0"]["state"]["open"],
        json!("note-20.txt")
    );
    // Shell gestures still start where they always did: from the home indicator.
    d.swipe((195, 840), (195, 700));
    assert_eq!(d.desktop()["focused"], Value::Null, "the swipe went home");
}

#[test]
fn an_android_app_scrolls_under_the_finger_but_the_shade_still_pulls_down() {
    let mut d = Desk::new("virtual-android-12", None, (412, 915));
    d.many_notes();
    d.swipe((200, 700), (200, 500));
    assert_eq!(d.area("main").offset, 200);
    d.swipe((200, 500), (200, 700));
    assert_eq!(d.area("main").offset, 0);
    d.swipe((200, 10), (200, 300));
    assert_eq!(d.desktop()["panel"], "notifications");
}

#[test]
fn the_browser_page_scrolls_with_the_wheel() {
    let mut d = Desk::new("virtual-macos-golden-gate", None, (1280, 800));
    d.launch("browser", "");
    d.act(
        "browser.v1",
        "navigate",
        json!({"url": "http://guide.example/"}),
    );
    let page = d.area("page");
    assert!(
        page.extent > page.bounds.height,
        "the guide is longer than a screen"
    );
    let at = (page.bounds.x + 200, page.bounds.y + 200);
    assert!(d.wheel(at, 240, &[]));
    let session = d.world.interfaces().session(&d.actor).unwrap();
    assert_eq!(session.machines["alice-mac"].browser.tab().scroll_y, 240);
    assert_eq!(d.area("page").offset, 240);
}

#[test]
fn a_spreadsheet_scrolls_rows_columns_and_zooms() {
    let mut d = Desk::new("virtual-windows-11", None, (1280, 800));
    d.launch("spreadsheet", "");
    // Launched on nothing it shows its file sheet; start a blank workbook.
    d.click("sheet:new:excel");
    let state = |d: &Desk| d.desktop()["windows"]["0"]["state"].clone();
    // Launched on nothing, the spreadsheet opens a new workbook or its file sheet.
    let grid = d.scene().nodes.into_iter().find(|n| {
        n.interaction
            .as_deref()
            .is_some_and(|i| i.contains(":content:sheet:grid:"))
    });
    let Some(grid) = grid else {
        panic!("no grid painted: {}", state(&d));
    };
    let b = grid.transform.bounds(grid.bounds);
    let at = (b.x + 100, b.y + 100);
    let scroll = |d: &Desk| state(d)["scroll"].clone();
    assert!(d.wheel(at, 120, &[]));
    assert_eq!(scroll(&d), json!([3, 0]));
    assert!(d.wheel(at, 120, &["Shift"]));
    assert_eq!(scroll(&d), json!([3, 1]));
    let zoom = state(&d)["zoom"].as_u64().unwrap();
    assert!(d.wheel(at, -120, &["Ctrl"]));
    assert_eq!(state(&d)["zoom"].as_u64().unwrap(), zoom + 10);
}

#[test]
fn the_soft_keyboard_appears_only_for_a_focused_text_field() {
    let mut d = Desk::new("virtual-ios-18", None, (390, 844));
    // Apps with no field at all never raise it, and typing reaches nothing.
    for kind in ["settings", "calculator"] {
        d.launch(kind, "");
        assert!(!d.keyboard_up(), "{kind}");
    }
    let calc = d.desktop()["focused"].as_u64().unwrap().to_string();
    let before = d.desktop()["windows"][&calc]["state"].clone();
    d.act("keyboard.v1", "type", json!({"text": "12"}));
    assert_eq!(d.desktop()["windows"][&calc]["state"], before);
    // Messages starts on its conversation list; an open conversation is not a focused
    // composer until it is tapped.
    d.launch("messages", "");
    assert!(!d.keyboard_up(), "the conversation list raises no keyboard");
    let conversation = d.first_target("messages:open:");
    d.tap(&conversation);
    assert!(!d.keyboard_up(), "a conversation alone raises no keyboard");
    let window = d.desktop()["focused"].as_u64().unwrap().to_string();
    d.act("keyboard.v1", "type", json!({"text": "lost"}));
    assert_eq!(d.desktop()["windows"][&window]["state"]["draft"], "");
    d.tap("messages:compose");
    assert!(d.keyboard_up(), "the tapped composer takes text");
    let focus = d.scene().focus.unwrap();
    assert_eq!(focus.role, "textbox");
    assert!(focus
        .interaction
        .as_deref()
        .is_some_and(|i| i.ends_with(":content:messages:compose")));
    d.act("keyboard.v1", "type", json!({"text": "hi"}));
    assert_eq!(d.desktop()["windows"][&window]["state"]["draft"], "hi");
    // Mail's compose sheet focuses its To field at once.
    d.launch("mail", "");
    assert!(!d.keyboard_up());
    d.tap("mail:compose");
    assert!(d.keyboard_up());
    // On a desktop, an open conversation's composer has the focus without a tap.
    let mut desk = Desk::new("virtual-macos-golden-gate", None, (1280, 800));
    desk.launch("messages", "");
    assert!(
        desk.scene().focus.unwrap().keyboard.text_entry,
        "a desktop composer is focused with its conversation"
    );
}

#[test]
fn a_battery_is_shown_only_where_the_machine_has_one() {
    let labels = |d: &Desk| -> Vec<String> {
        d.scene()
            .nodes
            .iter()
            .filter_map(|n| n.semantic.as_ref().map(|s| s.label.clone()))
            .collect()
    };
    let symbols = |d: &Desk| -> Vec<String> {
        d.scene()
            .nodes
            .iter()
            .filter_map(|n| match &n.primitive {
                cw_scene::Primitive::Symbol { asset, .. } => Some(asset.clone()),
                _ => None,
            })
            .collect()
    };
    // A Mac: the menu bar's battery item exists on a laptop only.
    let mac = Desk::new("virtual-macos-golden-gate", Some("desktop"), (1280, 800));
    assert!(!labels(&mac).iter().any(|l| l.starts_with("Battery")));
    let book = Desk::new("virtual-macos-golden-gate", Some("laptop"), (1280, 800));
    assert!(labels(&book).iter().any(|l| l == "Battery"));
    // No presentation stated: a desktop computer, with no battery to show.
    let plain = Desk::new("virtual-windows-11", None, (1280, 800));
    assert!(!symbols(&plain).iter().any(|s| s == "symbol/battery"));
    assert!(labels(&plain).iter().any(|l| l == "Network and volume"));
    let mut laptop = Desk::new("virtual-windows-11", Some("laptop"), (1280, 800));
    assert!(symbols(&laptop).iter().any(|s| s == "symbol/battery"));
    laptop.shell("shell:panel:quick");
    assert!(labels(&laptop)
        .iter()
        .any(|l| l == "Power and battery settings"));
    // GNOME: a laptop's top bar has a battery and its menu a battery pill; a desktop
    // has the power glyph and no pill.
    let mut ubuntu = Desk::new("virtual-ubuntu-24", Some("laptop"), (1280, 800));
    assert!(symbols(&ubuntu).iter().any(|s| s == "symbol/battery"));
    ubuntu.shell("shell:panel:quick");
    assert!(labels(&ubuntu)
        .iter()
        .any(|l| l == "Power settings, fully charged"));
    let mut tower = Desk::new("virtual-ubuntu-24", Some("desktop"), (1280, 800));
    tower.shell("shell:panel:quick");
    assert!(!symbols(&tower).iter().any(|s| s == "symbol/battery"));
    // Phones always run on one.
    let phone = Desk::new("virtual-android-12", None, (412, 915));
    assert!(symbols(&phone)
        .iter()
        .any(|s| s.starts_with("symbol/battery")));
}

#[test]
fn pixel_launcher_pages_swipe_and_answer_their_dots() {
    let mut d = Desk::new("virtual-android-12", None, (412, 915));
    let dots = |d: &Desk| -> Vec<String> {
        d.scene()
            .nodes
            .iter()
            .filter_map(|n| n.interaction.clone())
            .filter(|i| i.starts_with("shell:home-page:"))
            .collect()
    };
    let pages = dots(&d).len();
    assert!(pages >= 2, "more apps than the first page holds: {pages}");
    let page = |d: &Desk| d.desktop()["home_page"].as_u64().unwrap_or(0);
    let launchers = |d: &Desk| -> Vec<String> {
        d.scene()
            .nodes
            .iter()
            .filter_map(|n| n.interaction.clone())
            .filter(|i| i.starts_with("shell:launch:"))
            .collect()
    };
    let first = launchers(&d);
    d.swipe((350, 450), (60, 450));
    assert_eq!(page(&d), 1);
    let second = launchers(&d);
    assert_ne!(first, second, "another page shows other apps");
    // The dot for the first page goes back to it; a swipe right does too.
    d.click("shell:home-page:0");
    assert_eq!(page(&d), 0);
    d.swipe((350, 450), (60, 450));
    d.swipe((60, 450), (350, 450));
    assert_eq!(page(&d), 0);
    // Before the first page there is nothing to go to.
    d.swipe((60, 450), (350, 450));
    assert_eq!(page(&d), 0);
    assert_eq!(d.desktop()["panel"], Value::Null);
}

#[test]
fn pixel_recents_walks_its_carousel_selects_text_screenshots_and_clears_all() {
    let mut d = Desk::new("virtual-android-12", None, (412, 915));
    for kind in ["settings", "weather", "calculator"] {
        d.launch(kind, "");
    }
    d.click("shell:overview");
    assert_eq!(d.desktop()["panel"], "overview");
    for chip in ["shell:recents:screenshot", "shell:recents:select"] {
        assert!(d.find(chip).is_some(), "{chip}");
    }
    // Screenshot captures the application, and Recents stays open over it.
    d.click("shell:recents:screenshot");
    assert_eq!(d.desktop()["panel"], "overview");
    let pictures = d.act(
        "filesystem.v1",
        "list",
        json!({"path": d.desktop()["home"].as_str().map(|h| format!("{h}/Pictures")).unwrap_or_default()}),
    );
    assert!(pictures
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p.as_str().unwrap().starts_with("screen-")));
    // Swipe right: the older card is centred; Select on it and Copy its text.
    d.swipe((100, 400), (350, 400));
    d.click("shell:recents:select");
    assert!(d.find("shell:recents:copy").is_some());
    d.click("shell:recents:copy");
    let clip = d.desktop()["clipboard_text"].as_str().unwrap().to_owned();
    assert!(clip.contains("Portland"), "Weather's card text: {clip}");
    // Past the oldest card is Clear all.
    d.swipe((100, 400), (350, 400));
    d.swipe((100, 400), (350, 400));
    assert!(d.find("shell:recents:clear").is_some());
    d.click("shell:recents:clear");
    assert_eq!(d.desktop()["windows"], json!({}));
    assert_eq!(d.desktop()["panel"], Value::Null);
}

/// A finger scrolls the list it holds on every move, not only when it lets go, and the
/// release carries it on as far as the platform's deceleration takes the finger's last
/// speed. The content follows the finger exactly while it is down.
#[test]
fn a_phone_list_follows_the_finger_and_flings_when_it_lifts() {
    use cw_applications::desktop_scene::scroll::fling_distance;
    for (theme, size, android) in [
        ("virtual-ios-18", (390u32, 844u32), false),
        ("virtual-android-12", (412, 915), true),
    ] {
        let mut d = Desk::new(theme, None, size);
        d.many_notes();
        assert_eq!(d.area("main").offset, 0, "{theme}");
        d.pointer("down", (200, 700));
        // Ten pixels a frame: past the slop the list moves under the finger at every
        // sample, by the whole distance the finger has travelled.
        for step in 1..=10 {
            d.pointer("move", (200, 700 - step * 10));
            let moved = if step * 10 > 12 { step * 10 } else { 0 };
            assert_eq!(d.area("main").offset, moved, "{theme} at move {step}");
        }
        // Lifting where it last was: 10 px in one 60 Hz frame of finger travel.
        d.pointer("up", (200, 600));
        let fling = fling_distance(android, 10 * 1_000_000 / 16_667);
        assert!(fling > 0, "{theme}: a flicked list keeps going");
        assert_eq!(d.area("main").offset, 100 + fling, "{theme}");
        // The two platforms decelerate differently, so they land in different places.
        if android {
            assert!(
                fling < fling_distance(false, 10 * 1_000_000 / 16_667),
                "Android's spline carries a list less far than iOS's curve"
            );
        }
        // A finger that rests before lifting does not fling: the list stops where it is.
        let resting = d.area("main").offset;
        d.pointer("down", (200, 700));
        d.pointer("move", (200, 660));
        d.world.runtime_mut().advance(100_000).unwrap();
        d.pointer("up", (200, 660));
        assert_eq!(d.area("main").offset, resting + 40, "{theme}: no fling");
    }
}

/// Pulled past an end, the content still follows the finger, but less and less, and
/// springs back when it lifts. The offset itself never leaves its range.
#[test]
fn a_list_pulled_past_its_top_rubber_bands_and_springs_back() {
    let mut d = Desk::new("virtual-ios-18", None, (390, 844));
    d.many_notes();
    let row = |d: &Desk| {
        let target = d.first_target("notes:open:");
        d.find(&target).map(|(_, y)| y)
    };
    let rest = row(&d).expect("a row on screen");
    d.pointer("down", (200, 300));
    d.pointer("move", (200, 400));
    let pulled = row(&d).expect("the row is still there");
    assert!(
        pulled > rest && pulled < rest + 100,
        "the content follows the finger past the top, with resistance: {rest} -> {pulled}"
    );
    assert_eq!(d.area("main").offset, 0, "the offset stays at the top");
    let stretch = d.desktop()["windows"]["0"]["scroll"]["stretch"].clone();
    assert_eq!(stretch[0], "main");
    d.pointer("up", (200, 400));
    assert_eq!(
        d.desktop()["windows"]["0"]["scroll"]["stretch"],
        Value::Null
    );
    assert_eq!(row(&d), Some(rest), "it springs back on release");
}

/// A screenshot is of the screen the actor is looking at: the phone's own size and
/// orientation, or whatever size it last addressed the machine at.
#[test]
fn a_screenshot_is_taken_at_the_machines_own_screen_size() {
    let size = |d: &mut Desk| -> (u32, u32) {
        let home = d.desktop()["home"].as_str().unwrap().to_owned();
        let files = d.act(
            "filesystem.v1",
            "list",
            json!({ "path": format!("{home}/Pictures") }),
        );
        let name = files
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|f| f.as_str())
            .rfind(|f| f.starts_with("screen-"))
            .expect("a screenshot")
            .to_owned();
        let png = d.act(
            "filesystem.v1",
            "read",
            json!({ "path": format!("{home}/Pictures/{name}") }),
        );
        let bytes: Vec<u8> = png["bytes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b.as_u64().unwrap() as u8)
            .collect();
        // PNG: IHDR width and height are big-endian at bytes 16..24.
        let word = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
        (word(16), word(20))
    };
    // A phone nothing has touched yet is portrait, at its own screen size.
    let mut phone = Desk::new("virtual-ios-18", None, (390, 844));
    phone.shell("shell:screenshot");
    assert_eq!(size(&mut phone), (390, 844));
    // Held landscape — the size its actions state — the capture turns with it.
    phone.act(
        "pointer.v1",
        "move",
        json!({"x": 10, "y": 10, "width": 844, "height": 390}),
    );
    phone.shell("shell:screenshot");
    assert_eq!(size(&mut phone), (844, 390));
    // Android's Recents captures the application at the phone's size too.
    let mut pixel = Desk::new("virtual-android-12", None, (412, 915));
    pixel.launch("settings", "");
    pixel.click("shell:overview");
    pixel.click("shell:recents:screenshot");
    assert_eq!(size(&mut pixel), (412, 915));
    // A desktop captures the viewport the session is really using.
    let mut mac = Desk::new("virtual-macos-golden-gate", None, (1440, 900));
    mac.pointer("move", (10, 10));
    mac.shell("shell:screenshot");
    assert_eq!(size(&mut mac), (1440, 900));
}

/// The reference world says what each of its computers is, so the shells show the
/// battery the machine really has.
#[test]
fn the_reference_worlds_machines_report_their_own_form_factor() {
    let has_battery = |machine: &str, user: &str| {
        // The shells are opt-in (the live site world turns them on the same way); what
        // each machine *is* comes from the world itself.
        let mut d = reference_world();
        d.metadata["desktop_themes"] = json!({
            "alice-mac": "virtual-macos-golden-gate",
            "bob-windows": "virtual-windows-11",
            "carol-ubuntu": "virtual-ubuntu-24",
        });
        let mut world = World::new(d, 5).unwrap();
        let actor = world
            .environment(EnvironmentConfig::desktop(user, machine))
            .unwrap();
        world
            .scene(&actor, 1280, 800)
            .unwrap()
            .nodes
            .iter()
            .any(|n| match &n.primitive {
                cw_scene::Primitive::Symbol { asset, .. } => asset.starts_with("symbol/battery"),
                _ => false,
            })
    };
    // Carol's machine is a laptop; the two desktop computers have no battery at all.
    assert!(has_battery("carol-ubuntu", "carol"), "the laptop has one");
    assert!(!has_battery("alice-mac", "alice"), "a desktop Mac has none");
    assert!(!has_battery("bob-windows", "bob"), "a desktop PC has none");
    assert_eq!(
        reference_world().metadata["device_presentations"]["carol-ubuntu"],
        "laptop"
    );
}

/// iOS Mail's Mailboxes screen and Gmail's navigation drawer: on a phone every mailbox
/// is reachable, and Back goes back the way each platform's does.
#[test]
fn a_phone_reaches_every_mailbox_and_comes_back() {
    // iOS: the list is a mailbox, its navigation bar goes up to Mailboxes.
    let mut d = Desk::new("virtual-ios-18", None, (390, 844));
    d.launch("mail", "");
    let mail = |d: &Desk| d.desktop()["windows"]["0"]["state"].clone();
    let showing_mailboxes = |d: &Desk| mail(d)["mailboxes"] == json!(true);
    assert_eq!(mail(&d)["folder"], "inbox");
    assert!(d.find("mail:folder:sent").is_none(), "no folder list yet");
    d.tap("mail:mailboxes");
    assert!(showing_mailboxes(&d), "the Mailboxes screen");
    d.tap("mail:folder:sent");
    assert_eq!(mail(&d)["folder"], "sent");
    assert!(!showing_mailboxes(&d), "picking a mailbox shows its list");
    // Opening a message from the inbox and coming back with the bar's chevron.
    d.tap("mail:mailboxes");
    d.tap("mail:folder:inbox");
    let message = d.first_target("mail:open:");
    d.tap(&message);
    assert!(mail(&d)["selected"].is_string());
    d.tap("mail:back");
    assert_eq!(mail(&d)["selected"], Value::Null);
    // Android: the drawer, and the system Back button closes it.
    let mut a = Desk::new("virtual-android-12", None, (412, 915));
    a.launch("mail", "");
    let gmail = |a: &Desk| a.desktop()["windows"]["0"]["state"].clone();
    let drawer = |a: &Desk| gmail(a)["mailboxes"] == json!(true);
    a.tap("mail:mailboxes");
    assert!(drawer(&a), "the navigation drawer");
    a.shell("shell:mobile-back");
    assert!(!drawer(&a), "Back closes the drawer");
    a.tap("mail:mailboxes");
    a.tap("mail:folder:archive");
    assert_eq!(gmail(&a)["folder"], "archive");
    assert!(!drawer(&a), "picking a mailbox closes the drawer");
    // Compose is reachable on both phones.
    assert!(a.find("mail:compose").is_some(), "Gmail's compose button");
    assert!(d.find("mail:compose").is_some(), "Mail's compose button");
    // And the list refreshes the way a phone refreshes one: pulled down and released.
    let requests = |d: &Desk| {
        d.world
            .trajectory()
            .iter()
            .filter(|e| e.kind == "http.submitted")
            .count()
    };
    let before = requests(&d);
    d.pointer("down", (200, 300));
    d.pointer("move", (200, 420));
    d.pointer("up", (200, 420));
    assert!(
        requests(&d) > before,
        "pulling the list down and letting go asks the service again"
    );
    // A short pull is not a refresh.
    let before = requests(&d);
    d.pointer("down", (200, 300));
    d.pointer("move", (200, 330));
    d.pointer("up", (200, 330));
    assert_eq!(requests(&d), before, "a short pull refreshes nothing");
}

/// Messages opens on its conversation list, as both phones' messaging apps do, and a
/// conversation comes back to it.
#[test]
fn a_phone_messages_starts_on_its_conversation_list() {
    for (theme, size) in [
        ("virtual-ios-18", (390u32, 844u32)),
        ("virtual-android-12", (412, 915)),
    ] {
        let mut d = Desk::new(theme, None, size);
        d.launch("messages", "");
        let chat = |d: &Desk| d.desktop()["windows"]["0"]["state"].clone();
        let listing = |d: &Desk| chat(d)["listing"] == json!(true);
        assert!(listing(&d), "{theme}: the list is the root screen");
        assert!(
            d.find("messages:compose").is_none(),
            "{theme}: no composer yet"
        );
        let conversation = d.first_target("messages:open:");
        d.tap(&conversation);
        assert!(!listing(&d), "{theme}: the conversation is open");
        assert!(
            d.find("messages:compose").is_some(),
            "{theme}: the conversation"
        );
        // Back: the navigation bar's chevron on iOS, the system button on Android.
        if theme == "virtual-ios-18" {
            d.tap("messages:back");
        } else {
            d.shell("shell:mobile-back");
        }
        assert!(listing(&d), "{theme}: back to the list");
        assert!(d.find(&conversation).is_some(), "{theme}: the list again");
    }
}

/// A plain-text editor scrolls with the wheel independently of its caret, and a click
/// still lands on the character under the pointer however far it has scrolled.
#[test]
fn the_text_editor_scrolls_with_the_wheel_and_clicks_where_it_shows() {
    let mut d = Desk::new("virtual-macos-golden-gate", None, (1280, 800));
    // Two hundred lines of nine characters each (eight and a newline).
    let text: String = (0..200).map(|i| format!("line {i:03}\n")).collect();
    d.act(
        "filesystem.v1",
        "write",
        json!({"path": "/Users/alice/long.txt", "content": text}),
    );
    d.launch("editor", "/Users/alice/long.txt");
    let editor = |d: &Desk| d.desktop()["windows"]["0"]["state"].clone();
    let caret = |d: &Desk| editor(d)["cursor"].as_u64().unwrap() as usize;
    let first_row = |d: &Desk| -> usize {
        d.first_target("editor-text")
            .split(':')
            .nth(1)
            .and_then(|f| f.parse().ok())
            .unwrap()
    };
    let pane = d.area("text");
    assert!(pane.extent > pane.bounds.height, "200 lines overflow");
    let rows = (pane.bounds.height / 18) as usize;
    // 200 lines and the empty row after the last newline, of 18 px each.
    assert_eq!(pane.extent, 201 * 18);
    // Opened, the view shows the caret, which the editor leaves at the end of the file.
    let opened = first_row(&d);
    assert_eq!(opened, 201 - rows, "the last row is in view");
    let at = (pane.bounds.x + 100, pane.bounds.y + 60);
    // Six rows of 18 px a notch, and the caret stays exactly where it was.
    let end = caret(&d);
    assert!(d.wheel(at, -18 * 6, &[]));
    assert_eq!(first_row(&d), opened - 6);
    assert_eq!(caret(&d), end, "the caret did not move with the view");
    assert!(d.wheel(at, -18 * 20, &[]));
    let showing = first_row(&d);
    assert_eq!(showing, opened - 26);
    // A click lands on the character under it: the third row down, column two.
    let text_region = d
        .scene()
        .nodes
        .into_iter()
        .find(|n| {
            n.interaction
                .as_deref()
                .is_some_and(|i| i.contains(":content:editor-text:"))
        })
        .expect("the document text");
    let b = text_region.transform.bounds(text_region.bounds);
    d.pointer("click", (b.x + 2 * 8 + 1, b.y + 3 * 18 + 4));
    let clicked = (showing + 3) * 9 + 2;
    assert_eq!(caret(&d), clicked);
    assert_eq!(first_row(&d), showing, "a click does not move the view");
    // Scrolled away from the caret, typing brings it back into view — no further than
    // it must, so the caret lands on the view's last row.
    assert!(d.wheel(at, -1_000_000, &[]));
    assert_eq!(first_row(&d), 0);
    d.act("keyboard.v1", "type", json!({"text": "!"}));
    assert_eq!(caret(&d), clicked + 1);
    // The caret's row is the last the view shows: it came back no further than it had to.
    assert_eq!(first_row(&d) + rows, showing + 3 + 1);
    // The pane has a real scroll bar, and dragging it scrolls the view, not the caret.
    let bar = d
        .scene()
        .nodes
        .into_iter()
        .find(|n| {
            n.interaction
                .as_deref()
                .is_some_and(|i| i.contains(":content:pane:text:"))
        })
        .expect("an overflowing document paints a scroll bar");
    let b = bar.transform.bounds(bar.bounds);
    d.pointer(
        "click",
        (b.x + b.width as i32 / 2, b.y + b.height as i32 - 4),
    );
    assert_eq!(first_row(&d), 201 - rows, "the thumb jumped to the end");
    assert_eq!(caret(&d), clicked + 1, "the caret stayed where it was");
}

/// Every scroll says so in the action's effect, whether it moved a platform pane or an
/// application's own view.
#[test]
fn a_scroll_reports_itself_in_the_actions_effect() {
    let mut d = Desk::new("virtual-macos-golden-gate", None, (1280, 800));
    d.many_notes();
    let list = d.area("list");
    let at = (list.bounds.x + 40, list.bounds.y + 40);
    let wheel = |d: &mut Desk, dy: i32| {
        d.changed(
            "pointer.v1",
            "wheel",
            json!({"x": at.0, "y": at.1, "width": 1280, "height": 800, "delta_y": dy}),
        )
    };
    let changed = wheel(&mut d, 120);
    assert!(changed.contains(&"scroll".to_owned()), "{changed:?}");
    assert!(
        !changed.contains(&"content".to_owned()),
        "only the view moved"
    );
    // At the end there is nothing to move, and the effect says nothing changed.
    wheel(&mut d, 1_000_000);
    assert!(wheel(&mut d, 120).is_empty(), "nothing left to scroll");
    // An application's own use of the wheel reports a scroll too.
    let mut sheet = Desk::new("virtual-windows-11", None, (1280, 800));
    sheet.launch("spreadsheet", "");
    sheet.click("sheet:new:excel");
    let grid = sheet
        .scene()
        .nodes
        .into_iter()
        .find(|n| {
            n.interaction
                .as_deref()
                .is_some_and(|i| i.contains(":content:sheet:grid:"))
        })
        .expect("a grid");
    let b = grid.transform.bounds(grid.bounds);
    let changed = sheet.changed(
        "pointer.v1",
        "wheel",
        json!({"x": b.x + 100, "y": b.y + 100, "width": 1280, "height": 800, "delta_y": 120}),
    );
    assert!(changed.contains(&"scroll".to_owned()), "{changed:?}");
    // State that is not a window's still reports itself: the clipboard, notifications
    // and the shell's own surfaces each have a tag.
    let mut mac = Desk::new("virtual-macos-golden-gate", None, (1280, 800));
    mac.launch("files", "/Users/alice");
    let changed = mac.changed(
        "application.v1",
        "shell",
        json!({"target": "shell:screenshot"}),
    );
    assert!(changed.contains(&"notifications".to_owned()), "{changed:?}");
    let changed = mac.changed(
        "application.v1",
        "shell",
        json!({"target": "shell:toggle:wifi"}),
    );
    assert!(changed.contains(&"settings".to_owned()), "{changed:?}");
}

/// Screens a phone can get to, it can get back from, and controls a desktop has are
/// not simply dropped on a narrow screen.
#[test]
fn a_phone_has_no_dead_ends() {
    let mut d = Desk::new("virtual-ios-18", None, (390, 844));
    // Numbers: the document list is reachable from a workbook and leads back to it.
    d.launch("spreadsheet", "");
    d.tap("sheet:new:numbers");
    let sheet = |d: &Desk| d.desktop()["windows"]["0"]["state"].clone();
    assert_eq!(sheet(&d)["browsing"], json!(false));
    d.tap("sheet:open");
    assert_eq!(sheet(&d)["browsing"], json!(true), "the document list");
    assert!(
        d.find("sheet:closelist").is_some(),
        "the list has a way back to the open workbook"
    );
    d.tap("sheet:closelist");
    assert_eq!(sheet(&d)["browsing"], json!(false), "back in the workbook");
    // Calendar: Reload is a symbol where there is no room for the word, not nothing.
    d.launch("calendar", "");
    assert!(d.find("cal:reload").is_some(), "a phone can still reload");
}
