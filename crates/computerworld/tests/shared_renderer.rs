//! Two machines on one world, drawn out of one renderer.
//!
//! A page may show a whole team at once — seven machines, one world, a session each — and
//! every one of them repaints whenever any of them is touched. The renderer's caches are
//! shared between them, but the frame each screen was last left at is not: a screen is
//! repainted incrementally against its own previous scene, never against whichever screen
//! happened to be drawn last. This pins that, because the saving is invisible (the wrong
//! frame is still the right pixels, just drawn from scratch) and the cost is not: diffing
//! one machine's desktop against another's damages nearly every pixel on it.

use computerworld::{reference_world, World};
use cw_protocol::{ActionEnvelope, EnvironmentConfig};

const W: u32 = 1280;
const H: u32 = 800;

/// The same pixels a renderer that had drawn nothing else would produce.
fn alone(session_of: fn(&mut World) -> String, steps: &[ActionEnvelope]) -> Vec<u8> {
    let mut world = World::new(reference_world(), 7).unwrap();
    let session = session_of(&mut world);
    for step in steps {
        world.step(&session, vec![step.clone()]).unwrap();
    }
    world.render(&session, W, H).unwrap().rgba
}

fn mac(world: &mut World) -> String {
    world
        .environment(EnvironmentConfig::desktop("alice", "alice-mac"))
        .unwrap()
}
fn windows(world: &mut World) -> String {
    world
        .environment(EnvironmentConfig::desktop("bob", "bob-windows"))
        .unwrap()
}

#[test]
fn machines_sharing_a_world_keep_their_own_frames() {
    let launch = ActionEnvelope::new(
        "application.v1",
        "launch",
        "alice-mac",
        serde_json::json!({ "kind": "code" }),
    );

    let mut world = World::new(reference_world(), 7).unwrap();
    let alice = mac(&mut world);
    let bob = windows(&mut world);

    // Draw them alternately, which is what a page full of machines does: each screen's
    // frame has to survive the other being drawn over the renderer in between.
    for _ in 0..2 {
        world.render(&alice, W, H).unwrap();
        world.render(&bob, W, H).unwrap();
    }
    world.step(&alice, vec![launch.clone()]).unwrap();
    world.render(&alice, W, H).unwrap();
    let after_the_others = world.render(&bob, W, H).unwrap();

    // Bob's machine was not touched, and neither the launch on Alice's nor the repaints
    // in between may leave a mark on it.
    assert_eq!(
        after_the_others.rgba,
        alone(windows, &[]),
        "the Windows screen differs from the same screen drawn on its own"
    );
    assert_eq!(
        world.render(&alice, W, H).unwrap().rgba,
        alone(mac, std::slice::from_ref(&launch)),
        "the Mac screen differs from the same screen drawn on its own"
    );
}

#[test]
fn a_repaint_with_nothing_to_show_paints_nothing() {
    let mut world = World::new(reference_world(), 7).unwrap();
    let alice = mac(&mut world);
    let bob = windows(&mut world);
    world.render(&alice, W, H).unwrap();
    world.render(&bob, W, H).unwrap();

    // A gesture on one machine, and then the repaint of every machine on the slide that a
    // page does after one — the second one here being a machine nothing happened to.
    world
        .step(
            &alice,
            vec![ActionEnvelope::new(
                "application.v1",
                "launch",
                "alice-mac",
                serde_json::json!({ "kind": "code" }),
            )],
        )
        .unwrap();
    world.render(&alice, W, H).unwrap();
    world.render(&bob, W, H).unwrap();

    // Both screens now show what the world says they show, so drawing either again is
    // work with no result. It costs nothing only while each machine is diffed against its
    // own last scene; against whichever machine was drawn before it, every repaint of
    // every machine damages the screen, and a slide of seven of them pays that seven
    // times for one click.
    for (machine, session) in [("the Mac", &alice), ("the Windows PC", &bob)] {
        let before = world.render_stats().unwrap().painted_pixels;
        world.render(session, W, H).unwrap();
        let painted = world.render_stats().unwrap().painted_pixels - before;
        assert_eq!(painted, 0, "{machine} repainted {painted} pixels for nothing");
    }
}
