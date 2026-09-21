//! Two machines on one world, drawn out of one renderer.
//!
//! A page may show a whole team at once — seven machines, one world, a session each — and
//! every one of them repaints whenever any of them is touched. The renderer's caches are
//! shared between them, but the frame each screen was last left at is not: a screen is
//! repainted incrementally against its own previous scene, never against whichever screen
//! happened to be drawn last. This pins that, because the saving is invisible (the wrong
//! frame is still the right pixels, just drawn from scratch) and the cost is not: diffing
//! one machine's desktop against another's damages nearly every pixel on it.
//!
//! What every one of them keeps is bounded, too. A session is never removed from a world,
//! so a world that hands out sessions all day would hold a frame for every session it had
//! ever drawn; past the budget the screens drawn longest ago are forgotten, and a screen
//! forgotten is only a screen drawn again from nothing.

use computerworld::{reference_world, World, RETAINED_SURFACE_BYTES};
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
        assert_eq!(
            painted, 0,
            "{machine} repainted {painted} pixels for nothing"
        );
    }
}

/// What one of these screens costs to hold: the frame is four bytes a pixel.
const FRAME: usize = W as usize * H as usize * 4;
/// The crowd below draws at a size of its own, which is what makes the bytes a world is
/// holding say *which* screens are still in it: Alice's is then the only frame of its size,
/// and a total that is a whole number of the crowd's is hers gone.
const CROWD_W: u32 = 1200;
const CROWD_H: u32 = 900;
const CROWD_FRAME: usize = CROWD_W as usize * CROWD_H as usize * 4;
/// Machines to open the crowd of throwaway sessions on. Alice's is left out of it, so that
/// nothing the crowd does can reach the screen these tests compare.
const OTHERS: [&str; 2] = ["bob-windows", "carol-ubuntu"];

/// Open `count` sessions that exist only to be drawn and left: nothing ever removes a
/// session from a world, so every one of them would otherwise keep its screen for as long
/// as the world lives.
fn crowd(world: &mut World, count: usize) -> Vec<String> {
    (0..count)
        .map(|i| {
            world
                .environment(EnvironmentConfig::desktop(
                    format!("extra-{i}"),
                    OTHERS[i % OTHERS.len()],
                ))
                .unwrap()
        })
        .collect()
}

/// Draw each of them once, as a page that showed them and moved on would.
fn draw_all(world: &mut World, sessions: &[String]) {
    for session in sessions {
        world.render(session, CROWD_W, CROWD_H).unwrap();
    }
}

#[test]
fn a_forgotten_screen_comes_back_whole() {
    let launch = ActionEnvelope::new(
        "application.v1",
        "launch",
        "alice-mac",
        serde_json::json!({ "kind": "code" }),
    );

    let mut world = World::new(reference_world(), 7).unwrap();
    let alice = mac(&mut world);
    world.render(&alice, W, H).unwrap();
    world.step(&alice, vec![launch.clone()]).unwrap();
    world.render(&alice, W, H).unwrap();

    // The fewest other screens that leave no room for Alice's, which was drawn longest ago
    // and so is the one the world forgets. They all fit once hers is gone.
    let no_room_left = (RETAINED_SURFACE_BYTES - FRAME) / CROWD_FRAME + 1;
    let others = crowd(&mut world, no_room_left);
    draw_all(&mut world, &others);
    assert_eq!(
        world.retained_surface_bytes(),
        others.len() * CROWD_FRAME,
        "what the world kept is not exactly the crowd, so the wrong screen was forgotten"
    );

    // Her machine is drawn again from nothing, and has to arrive at the same pixels as a
    // world that had drawn nothing else. It starts from nothing because the scene went out
    // with the frame: a scene kept on its own would leave the next repaint measuring its
    // damage against a screen this world no longer has.
    assert_eq!(
        world.render(&alice, W, H).unwrap().rgba,
        alone(mac, std::slice::from_ref(&launch)),
        "the Mac screen came back from being forgotten differing from the same screen alone"
    );
}

#[test]
fn a_world_stops_holding_screens_it_has_stopped_drawing() {
    let mut world = World::new(reference_world(), 7).unwrap();

    // Four more screens than fit, drawn once each and never again — a world that has been
    // handing out sessions all day. Unbounded, this keeps every one of them.
    let sessions = crowd(&mut world, RETAINED_SURFACE_BYTES / CROWD_FRAME + 4);
    let unbounded = sessions.len() * CROWD_FRAME;
    assert!(
        unbounded > RETAINED_SURFACE_BYTES,
        "{} screens come to {unbounded} bytes, which is inside the budget to begin with",
        sessions.len()
    );
    draw_all(&mut world, &sessions);
    let retained = world.retained_surface_bytes();
    assert!(
        retained <= RETAINED_SURFACE_BYTES,
        "{} screens left {retained} bytes retained, over the {RETAINED_SURFACE_BYTES} budget",
        sessions.len()
    );

    // Recency is use, not arrival. The session opened first is long forgotten; drawing it
    // again makes it the most recently drawn of all of them, so the newcomer after it
    // costs some other screen and this one is still there to be repainted for nothing.
    let oldest = sessions[0].clone();
    world.render(&oldest, CROWD_W, CROWD_H).unwrap();
    let newcomer = crowd(&mut world, 1);
    draw_all(&mut world, &newcomer);
    let before = world.render_stats().unwrap().painted_pixels;
    world.render(&oldest, CROWD_W, CROWD_H).unwrap();
    let painted = world.render_stats().unwrap().painted_pixels - before;
    assert_eq!(
        painted, 0,
        "the session drawn again was forgotten anyway, and repainted {painted} pixels"
    );
    assert!(
        world.retained_surface_bytes() <= RETAINED_SURFACE_BYTES,
        "the budget stopped holding once a session was drawn a second time"
    );
}
