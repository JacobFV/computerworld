//! Scrolling, text focus and the phone shells' platform surfaces, end to end. Every
//! step is a pointer or keyboard action aimed at what the scene shows, and every check
//! reads the published scene (its `scrolls`, its focus) or the machine back.
use computerworld::{reference_world, World};
use cw_protocol::{ActionEnvelope, EnvironmentConfig};
use cw_scene::{Scene, ScrollArea};
use serde_json::{json, Value};

const APPS: [&str; 7] = [
    "notes",
    "chat",
    "mail",
    "calculator",
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
            ("chat", "http://chat.internal/"),
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
    // Messages: an open conversation is not a focused composer until it is tapped.
    d.launch("chat", "");
    assert!(!d.keyboard_up(), "a conversation alone raises no keyboard");
    let window = d.desktop()["focused"].as_u64().unwrap().to_string();
    d.act("keyboard.v1", "type", json!({"text": "lost"}));
    assert_eq!(d.desktop()["windows"][&window]["state"]["draft"], "");
    d.tap("chat:compose");
    assert!(d.keyboard_up(), "the tapped composer takes text");
    let focus = d.scene().focus.unwrap();
    assert_eq!(focus.role, "textbox");
    assert!(focus
        .interaction
        .as_deref()
        .is_some_and(|i| i.ends_with(":content:chat:compose")));
    d.act("keyboard.v1", "type", json!({"text": "hi"}));
    assert_eq!(d.desktop()["windows"][&window]["state"]["draft"], "hi");
    // Mail's compose sheet focuses its To field at once.
    d.launch("mail", "");
    assert!(!d.keyboard_up());
    d.tap("mail:compose");
    assert!(d.keyboard_up());
    // On a desktop, an open conversation's composer has the focus without a tap.
    let mut desk = Desk::new("virtual-macos-golden-gate", None, (1280, 800));
    desk.launch("chat", "");
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
