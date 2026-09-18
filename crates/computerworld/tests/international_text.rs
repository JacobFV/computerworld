//! Text an actor types in Hebrew, Arabic, Thai, Devanagari, Chinese, Japanese, Korean
//! and emoji reaches the screen as real glyphs through a whole world: keyboard input,
//! a native application's scene, scene metrics and the renderer.
use computerworld::{reference_world, World};
use cw_protocol::{ActionEnvelope, EnvironmentConfig};
use cw_scene::text::{self, GlyphRef};
use cw_scene::{metrics, Primitive, Typeface};
use serde_json::{json, Value};

const W: u32 = 1100;
const H: u32 = 720;
const TYPED: &str = "שלום עולם مرحبا สวัสดี नमस्ते 你好 日本語 한국어 👋🏽";

fn world() -> (World, String) {
    let mut d = reference_world();
    d.metadata["desktop_themes"] = json!({ "alice-mac": "virtual-macos-golden-gate" });
    d.metadata["desktop_apps"] =
        json!([{"id":"notes","label":"Notes","kind":"native","url":"","icon":"notes"}]);
    for computer in &mut d.computers {
        if computer.id == "alice-mac" {
            computer.installed_apps.push("notes".into());
        }
    }
    let mut world = World::new(d, 42).unwrap();
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
                .is_some_and(|i| i == target || i.ends_with(&suffix))
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

#[test]
fn typed_international_text_renders_as_real_glyphs() {
    let (mut world, actor) = world();
    act(
        &mut world,
        &actor,
        "application.v1",
        "launch",
        json!({"kind":"notes","argument":"/Users/alice/Notes"}),
    );
    click(&mut world, &actor, "notes:new");
    act(
        &mut world,
        &actor,
        "keyboard.v1",
        "type",
        json!({ "text": TYPED }),
    );
    let scene = world.scene(&actor, W, H).unwrap();
    let (node, text, size, bold) = scene
        .nodes
        .iter()
        .find_map(|n| match &n.primitive {
            Primitive::UiText { text, size, .. } if text.contains("שלום") => {
                Some((n, text.clone(), *size, false))
            }
            Primitive::UiTextBold { text, size, .. } if text.contains("שלום") => {
                Some((n, text.clone(), *size, true))
            }
            _ => None,
        })
        .expect("the typed text is on screen as UI text");

    // Every character outside ASCII resolves to a real glyph of a fallback face.
    let lines = text::layout(scene.typeface, bold, &text, size, node.bounds.width);
    let glyphs: Vec<_> = lines.iter().flat_map(|l| l.glyphs.iter()).collect();
    for script in ["ש", "م", "ส", "न", "你", "日", "한", "👋"] {
        assert!(text.contains(script));
    }
    let shaped = glyphs.iter().filter(|g| g.face.is_some()).count();
    assert!(shaped >= 30, "only {shaped} shaped glyphs");
    assert!(
        glyphs
            .iter()
            .all(|g| g.face.is_none() || g.glyph != GlyphRef::Index(0)),
        "a fallback face drew .notdef"
    );
    // The scene's own wrapping is the renderer's.
    assert_eq!(
        lines.iter().map(|l| l.text.clone()).collect::<Vec<_>>(),
        metrics::wrap(scene.typeface, bold, &text, size, node.bounds.width)
    );
    assert_ne!(
        scene.typeface,
        Typeface::DejaVu,
        "macOS uses its platform face"
    );

    // Pixels are deterministic and survive a snapshot round trip.
    let frame = world.render(&actor, W, H).unwrap();
    assert_eq!(frame, world.render(&actor, W, H).unwrap());
    let snapshot = world.snapshot();
    let mut fork = world.fork(&snapshot).unwrap();
    assert_eq!(fork.render(&actor, W, H).unwrap(), frame);
    // The text's box is inked well beyond what a single run of boxes would be:
    // at least one dark pixel in most of its measured width.
    let b = node.transform.bounds(node.bounds);
    let measured = metrics::text_width(scene.typeface, bold, lines[0].text.trim_end(), size);
    let inked = (0..measured.min(b.width))
        .filter(|dx| {
            (0..b.height).any(|dy| {
                let (x, y) = (b.x as u32 + dx, b.y as u32 + dy);
                frame.pixel(x, y).is_some_and(|p| p[0] < 110)
            })
        })
        .count() as u32;
    assert!(
        inked * 2 > measured.min(b.width),
        "{inked} of {measured} columns inked"
    );
}
