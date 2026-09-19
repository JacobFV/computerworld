//! Toolbar icons in the style of KiCad's bitmap set, drawn as vectors on a 20 px grid.
use crate::desktop_scene::Painter;
use cw_scene::{Color, Rect};

const DARK: Color = Color::rgb(55, 60, 70);
const RED: Color = Color::rgb(200, 40, 40);
const GREEN: Color = Color::rgb(0, 150, 0);
const BLUE: Color = Color::rgb(30, 90, 190);
const AMBER: Color = Color::rgb(215, 140, 20);
const BODY: Color = Color::rgb(132, 0, 0);
const CREAM: Color = Color::rgb(255, 255, 194);

fn l(p: &mut Painter, x: i32, y: i32, pts: &[(i32, i32)], c: Color, w: u16) {
    p.line(pts.iter().map(|(a, b)| (x + a, y + b)).collect(), c, w);
}
fn f(p: &mut Painter, x: i32, y: i32, pts: &[(i32, i32)], c: Color) {
    p.path(pts.iter().map(|(a, b)| (x + a, y + b)).collect(), c);
}
fn b(p: &mut Painter, x: i32, y: i32, r: (i32, i32, u32, u32), c: Color) {
    p.box_(Rect::new(x + r.0, y + r.1, r.2, r.3), c, 1);
}

pub fn save(p: &mut Painter, x: i32, y: i32) {
    b(p, x, y, (2, 2, 16, 16), BLUE);
    b(p, x, y, (5, 3, 10, 6), Color::WHITE);
    b(p, x, y, (5, 12, 10, 6), Color::rgb(200, 210, 230));
}
pub fn new_project(p: &mut Painter, x: i32, y: i32) {
    l(
        p,
        x,
        y,
        &[(4, 2), (12, 2), (16, 6), (16, 18), (4, 18), (4, 2)],
        DARK,
        1,
    );
    l(p, x, y, &[(12, 2), (12, 6), (16, 6)], DARK, 1);
    b(p, x, y, (12, 11, 7, 2), GREEN);
    b(p, x, y, (14, 9, 2, 7), GREEN);
}
pub fn open(p: &mut Painter, x: i32, y: i32) {
    f(
        p,
        x,
        y,
        &[(1, 5), (7, 5), (9, 7), (17, 7), (17, 17), (1, 17)],
        AMBER,
    );
    f(
        p,
        x,
        y,
        &[(3, 10), (19, 10), (17, 17), (1, 17)],
        Color::rgb(245, 190, 70),
    );
}
pub fn folder(p: &mut Painter, x: i32, y: i32) {
    f(
        p,
        x,
        y,
        &[(1, 5), (7, 5), (9, 7), (18, 7), (18, 17), (1, 17)],
        Color::rgb(90, 140, 210),
    );
}
pub fn refresh(p: &mut Painter, x: i32, y: i32) {
    p.ring(x + 10, y + 10, 7, 2, GREEN);
    f(p, x, y, &[(12, 0), (18, 4), (12, 7)], GREEN);
    b(p, x, y, (4, 9, 3, 3), Color::rgb(240, 240, 240));
}
pub fn undo(p: &mut Painter, x: i32, y: i32) {
    l(
        p,
        x,
        y,
        &[(4, 8), (13, 8), (17, 12), (13, 16), (8, 16)],
        BLUE,
        2,
    );
    f(p, x, y, &[(1, 8), (6, 4), (6, 12)], BLUE);
}
pub fn redo(p: &mut Painter, x: i32, y: i32) {
    l(
        p,
        x,
        y,
        &[(16, 8), (7, 8), (3, 12), (7, 16), (12, 16)],
        BLUE,
        2,
    );
    f(p, x, y, &[(19, 8), (14, 4), (14, 12)], BLUE);
}
fn lens(p: &mut Painter, x: i32, y: i32) {
    p.ring(x + 8, y + 8, 6, 2, DARK);
    l(p, x, y, &[(12, 12), (18, 18)], DARK, 3);
}
pub fn zoom_in(p: &mut Painter, x: i32, y: i32) {
    lens(p, x, y);
    l(p, x, y, &[(5, 8), (11, 8)], DARK, 2);
    l(p, x, y, &[(8, 5), (8, 11)], DARK, 2);
}
pub fn zoom_out(p: &mut Painter, x: i32, y: i32) {
    lens(p, x, y);
    l(p, x, y, &[(5, 8), (11, 8)], DARK, 2);
}
pub fn zoom_fit(p: &mut Painter, x: i32, y: i32) {
    lens(p, x, y);
    l(p, x, y, &[(1, 5), (1, 1), (5, 1)], RED, 1);
    l(p, x, y, &[(15, 1), (19, 1), (19, 5)], RED, 1);
}
pub fn zoom_objects(p: &mut Painter, x: i32, y: i32) {
    lens(p, x, y);
    b(p, x, y, (5, 5, 6, 6), Color::rgb(0, 150, 0));
}
pub fn rotate(p: &mut Painter, x: i32, y: i32) {
    l(
        p,
        x,
        y,
        &[
            (15, 5),
            (10, 2),
            (4, 5),
            (2, 11),
            (5, 16),
            (11, 18),
            (16, 15),
        ],
        DARK,
        2,
    );
    f(p, x, y, &[(17, 1), (18, 8), (11, 6)], DARK);
}
pub fn mirror_v(p: &mut Painter, x: i32, y: i32) {
    l(p, x, y, &[(1, 10), (19, 10)], RED, 1);
    f(p, x, y, &[(10, 2), (4, 8), (16, 8)], DARK);
    l(p, x, y, &[(10, 18), (4, 12), (16, 12), (10, 18)], DARK, 1);
}
pub fn mirror_h(p: &mut Painter, x: i32, y: i32) {
    l(p, x, y, &[(10, 1), (10, 19)], RED, 1);
    f(p, x, y, &[(2, 10), (8, 4), (8, 16)], DARK);
    l(p, x, y, &[(18, 10), (12, 4), (12, 16), (18, 10)], DARK, 1);
}
pub fn flip(p: &mut Painter, x: i32, y: i32) {
    b(p, x, y, (2, 4, 16, 5), RED);
    b(p, x, y, (2, 11, 16, 5), BLUE);
    l(p, x, y, &[(10, 1), (10, 19)], DARK, 1);
}
pub fn delete(p: &mut Painter, x: i32, y: i32) {
    l(p, x, y, &[(4, 5), (16, 5)], DARK, 2);
    l(p, x, y, &[(5, 5), (6, 18), (14, 18), (15, 5)], DARK, 1);
    l(p, x, y, &[(8, 3), (12, 3)], DARK, 2);
}
pub fn properties(p: &mut Painter, x: i32, y: i32) {
    b(p, x, y, (3, 2, 14, 16), Color::WHITE);
    l(
        p,
        x,
        y,
        &[(3, 2), (17, 2), (17, 18), (3, 18), (3, 2)],
        DARK,
        1,
    );
    for yy in [6, 10, 14] {
        l(p, x, y, &[(6, yy), (14, yy)], BLUE, 1);
    }
}
pub fn annotate(p: &mut Painter, x: i32, y: i32) {
    b(p, x, y, (7, 2, 6, 14), CREAM);
    l(
        p,
        x,
        y,
        &[(7, 2), (13, 2), (13, 16), (7, 16), (7, 2)],
        BODY,
        1,
    );
    l(p, x, y, &[(1, 18), (5, 13)], BLUE, 2);
    l(p, x, y, &[(15, 5), (19, 5)], BLUE, 2);
    l(p, x, y, &[(15, 9), (19, 9)], BLUE, 2);
}
pub fn erc(p: &mut Painter, x: i32, y: i32) {
    p.circle(x + 9, y + 11, 6, Color::rgb(90, 70, 40));
    l(p, x, y, &[(3, 7), (15, 15)], Color::rgb(90, 70, 40), 1);
    l(p, x, y, &[(3, 15), (15, 7)], Color::rgb(90, 70, 40), 1);
    l(p, x, y, &[(11, 12), (14, 16), (19, 6)], GREEN, 2);
}
pub fn netlist(p: &mut Painter, x: i32, y: i32) {
    for (yy, c) in [(3, GREEN), (9, GREEN), (15, GREEN)] {
        l(p, x, y, &[(2, yy + 2), (18, yy + 2)], c, 1);
        p.circle(x + 4, y + yy + 2, 2, DARK);
        p.circle(x + 16, y + yy + 2, 2, DARK);
    }
}
pub fn bom(p: &mut Painter, x: i32, y: i32) {
    b(p, x, y, (3, 1, 14, 18), Color::WHITE);
    l(
        p,
        x,
        y,
        &[(3, 1), (17, 1), (17, 19), (3, 19), (3, 1)],
        DARK,
        1,
    );
    for yy in [5, 9, 13] {
        b(p, x, y, (5, yy, 2, 2), DARK);
        l(p, x, y, &[(9, yy + 1), (15, yy + 1)], DARK, 1);
    }
}
pub fn schematic(p: &mut Painter, x: i32, y: i32) {
    b(p, x, y, (1, 2, 18, 16), CREAM);
    l(
        p,
        x,
        y,
        &[(1, 2), (19, 2), (19, 18), (1, 18), (1, 2)],
        BODY,
        1,
    );
    l(
        p,
        x,
        y,
        &[
            (3, 10),
            (6, 10),
            (7, 7),
            (9, 13),
            (11, 7),
            (13, 13),
            (14, 10),
            (17, 10),
        ],
        GREEN,
        1,
    );
}
pub fn board(p: &mut Painter, x: i32, y: i32) {
    b(p, x, y, (1, 2, 18, 16), Color::rgb(30, 110, 60));
    l(
        p,
        x,
        y,
        &[(4, 6), (10, 6), (14, 10), (14, 15)],
        Color::rgb(220, 180, 60),
        2,
    );
    p.circle(x + 4, y + 6, 2, Color::rgb(230, 230, 230));
    p.circle(x + 14, y + 15, 2, Color::rgb(230, 230, 230));
}
pub fn simulator(p: &mut Painter, x: i32, y: i32) {
    b(p, x, y, (1, 2, 18, 16), Color::rgb(20, 30, 40));
    l(
        p,
        x,
        y,
        &[
            (2, 13),
            (5, 13),
            (7, 5),
            (10, 15),
            (12, 7),
            (15, 10),
            (18, 10),
        ],
        Color::rgb(80, 230, 90),
        1,
    );
}
pub fn update(p: &mut Painter, x: i32, y: i32) {
    board(p, x, y);
    f(p, x, y, &[(12, 1), (19, 5), (12, 9)], GREEN);
}
pub fn select(p: &mut Painter, x: i32, y: i32) {
    f(
        p,
        x,
        y,
        &[
            (4, 1),
            (4, 16),
            (8, 12),
            (11, 19),
            (13, 18),
            (10, 11),
            (15, 11),
        ],
        DARK,
    );
}
pub fn wire(p: &mut Painter, x: i32, y: i32) {
    l(p, x, y, &[(2, 16), (10, 16), (10, 4), (18, 4)], GREEN, 2);
}
pub fn add_symbol(p: &mut Painter, x: i32, y: i32) {
    f(p, x, y, &[(3, 2), (17, 10), (3, 18)], CREAM);
    l(p, x, y, &[(3, 2), (17, 10), (3, 18), (3, 2)], BODY, 1);
    l(p, x, y, &[(0, 6), (3, 6)], BODY, 1);
    l(p, x, y, &[(0, 14), (3, 14)], BODY, 1);
}
pub fn add_power(p: &mut Painter, x: i32, y: i32) {
    l(p, x, y, &[(10, 2), (10, 12)], BODY, 2);
    l(p, x, y, &[(4, 12), (16, 12)], BODY, 2);
    l(p, x, y, &[(6, 15), (14, 15)], BODY, 2);
    l(p, x, y, &[(8, 18), (12, 18)], BODY, 2);
}
pub fn label(p: &mut Painter, x: i32, y: i32) {
    l(p, x, y, &[(2, 16), (18, 16)], GREEN, 2);
    l(p, x, y, &[(4, 13), (8, 3), (12, 13)], DARK, 2);
    l(p, x, y, &[(5, 10), (11, 10)], DARK, 1);
}
pub fn global_label(p: &mut Painter, x: i32, y: i32) {
    l(
        p,
        x,
        y,
        &[(1, 10), (5, 5), (18, 5), (18, 15), (5, 15), (1, 10)],
        BODY,
        1,
    );
    l(p, x, y, &[(8, 8), (15, 8)], DARK, 1);
    l(p, x, y, &[(8, 12), (15, 12)], DARK, 1);
}
pub fn no_connect(p: &mut Painter, x: i32, y: i32) {
    l(p, x, y, &[(4, 4), (16, 16)], BLUE, 2);
    l(p, x, y, &[(16, 4), (4, 16)], BLUE, 2);
}
pub fn junction(p: &mut Painter, x: i32, y: i32) {
    l(p, x, y, &[(2, 10), (18, 10)], GREEN, 2);
    l(p, x, y, &[(10, 10), (10, 18)], GREEN, 2);
    p.circle(x + 10, y + 10, 3, GREEN);
}
pub fn probe(p: &mut Painter, x: i32, y: i32) {
    l(p, x, y, &[(3, 17), (9, 11)], DARK, 1);
    f(p, x, y, &[(8, 9), (16, 1), (19, 4), (11, 12)], RED);
}
pub fn route(p: &mut Painter, x: i32, y: i32) {
    l(
        p,
        x,
        y,
        &[(2, 16), (8, 16), (16, 8), (16, 2)],
        Color::rgb(200, 52, 52),
        3,
    );
}
pub fn via(p: &mut Painter, x: i32, y: i32) {
    p.circle(x + 10, y + 10, 7, Color::rgb(200, 200, 200));
    p.circle(x + 10, y + 10, 3, DARK);
}
pub fn zone(p: &mut Painter, x: i32, y: i32) {
    f(
        p,
        x,
        y,
        &[(2, 4), (18, 2), (17, 18), (3, 16)],
        Color::rgb(200, 52, 52),
    );
    for d in [6, 11, 16] {
        l(
            p,
            x,
            y,
            &[(3 + d / 2, 3), (3, d + 1)],
            Color::rgb(240, 160, 160),
            1,
        );
    }
}
pub fn rect(p: &mut Painter, x: i32, y: i32) {
    l(
        p,
        x,
        y,
        &[(3, 4), (17, 4), (17, 16), (3, 16), (3, 4)],
        Color::rgb(160, 160, 0),
        2,
    );
}
pub fn line(p: &mut Painter, x: i32, y: i32) {
    l(p, x, y, &[(3, 17), (17, 3)], Color::rgb(160, 160, 0), 2);
}
pub fn drc(p: &mut Painter, x: i32, y: i32) {
    board(p, x, y);
    l(
        p,
        x,
        y,
        &[(9, 12), (12, 16), (19, 5)],
        Color::rgb(120, 255, 120),
        2,
    );
}
pub fn fill(p: &mut Painter, x: i32, y: i32) {
    f(
        p,
        x,
        y,
        &[(2, 4), (18, 2), (17, 18), (3, 16)],
        Color::rgb(77, 127, 196),
    );
    l(p, x, y, &[(6, 10), (14, 10)], Color::WHITE, 2);
    l(p, x, y, &[(10, 6), (10, 14)], Color::WHITE, 2);
}
pub fn plot(p: &mut Painter, x: i32, y: i32) {
    b(p, x, y, (3, 1, 14, 18), Color::WHITE);
    l(
        p,
        x,
        y,
        &[(3, 1), (17, 1), (17, 19), (3, 19), (3, 1)],
        DARK,
        1,
    );
    b(p, x, y, (6, 5, 8, 5), Color::rgb(200, 52, 52));
    b(p, x, y, (6, 12, 8, 4), Color::rgb(77, 127, 196));
}
pub fn settings(p: &mut Painter, x: i32, y: i32) {
    p.ring(x + 10, y + 10, 6, 3, DARK);
    for (a, bq, c, d) in [
        (10, 1, 10, 4),
        (10, 16, 10, 19),
        (1, 10, 4, 10),
        (16, 10, 19, 10),
    ] {
        l(p, x, y, &[(a, bq), (c, d)], DARK, 3);
    }
}
pub fn run(p: &mut Painter, x: i32, y: i32) {
    f(p, x, y, &[(5, 2), (17, 10), (5, 18)], GREEN);
}
pub fn add_signals(p: &mut Painter, x: i32, y: i32) {
    l(
        p,
        x,
        y,
        &[(1, 14), (4, 14), (6, 6), (9, 16), (11, 10), (13, 10)],
        BLUE,
        1,
    );
    b(p, x, y, (13, 5, 6, 2), GREEN);
    b(p, x, y, (15, 3, 2, 6), GREEN);
}
pub fn grid(p: &mut Painter, x: i32, y: i32) {
    for gx in [3, 8, 13, 18] {
        for gy in [3, 8, 13, 18] {
            b(p, x, y, (gx - 1, gy - 1, 2, 2), DARK);
        }
    }
}
pub fn units(p: &mut Painter, x: i32, y: i32) {
    l(p, x, y, &[(2, 12), (18, 12)], DARK, 1);
    for t in [2, 6, 10, 14, 18] {
        l(
            p,
            x,
            y,
            &[(t, 12), (t, if t % 8 == 2 { 6 } else { 9 })],
            DARK,
            1,
        );
    }
}
pub fn ratsnest(p: &mut Painter, x: i32, y: i32) {
    l(p, x, y, &[(3, 16), (17, 4)], Color::rgb(120, 120, 120), 1);
    l(p, x, y, &[(3, 4), (12, 16)], Color::rgb(120, 120, 120), 1);
    p.circle(x + 3, y + 16, 2, RED);
    p.circle(x + 17, y + 4, 2, RED);
    p.circle(x + 3, y + 4, 2, RED);
    p.circle(x + 12, y + 16, 2, RED);
}
pub fn posture(p: &mut Painter, x: i32, y: i32) {
    l(p, x, y, &[(2, 17), (9, 10), (18, 10)], DARK, 2);
    l(p, x, y, &[(2, 7), (11, 7), (18, 0)], FAINT_ICON, 1);
}
const FAINT_ICON: Color = Color::rgb(150, 150, 150);
pub fn cursor(p: &mut Painter, x: i32, y: i32) {
    l(p, x, y, &[(10, 1), (10, 19)], DARK, 1);
    l(
        p,
        x,
        y,
        &[(1, 14), (5, 12), (9, 8), (13, 6), (19, 5)],
        BLUE,
        2,
    );
}
pub fn bus(p: &mut Painter, x: i32, y: i32) {
    l(p, x, y, &[(2, 16), (10, 16), (10, 4), (18, 4)], BLUE, 4);
}
pub fn bus_entry(p: &mut Painter, x: i32, y: i32) {
    l(p, x, y, &[(3, 2), (3, 18)], BLUE, 4);
    l(p, x, y, &[(3, 8), (10, 15), (18, 15)], GREEN, 2);
}
pub fn hier_label(p: &mut Painter, x: i32, y: i32) {
    l(
        p,
        x,
        y,
        &[(2, 10), (7, 5), (18, 5), (18, 15), (7, 15), (2, 10)],
        AMBER,
        2,
    );
}
pub fn sheet(p: &mut Painter, x: i32, y: i32) {
    b(p, x, y, (3, 3, 14, 14), CREAM);
    l(
        p,
        x,
        y,
        &[(3, 3), (17, 3), (17, 17), (3, 17), (3, 3)],
        Color::rgb(132, 0, 132),
        2,
    );
    l(p, x, y, &[(6, 8), (14, 8)], DARK, 1);
}
pub fn sheet_pin(p: &mut Painter, x: i32, y: i32) {
    l(
        p,
        x,
        y,
        &[(8, 2), (18, 2), (18, 18), (8, 18)],
        Color::rgb(132, 0, 132),
        2,
    );
    f(p, x, y, &[(2, 7), (8, 10), (2, 13)], AMBER);
}
pub fn leave_sheet(p: &mut Painter, x: i32, y: i32) {
    l(p, x, y, &[(10, 17), (10, 4)], DARK, 2);
    l(p, x, y, &[(5, 9), (10, 4), (15, 9)], DARK, 2);
}
pub fn symbol_editor(p: &mut Painter, x: i32, y: i32) {
    add_symbol(p, x, y);
    l(p, x, y, &[(12, 18), (19, 11)], BLUE, 2);
}
pub fn footprint_editor(p: &mut Painter, x: i32, y: i32) {
    b(p, x, y, (3, 5, 5, 10), Color::rgb(200, 52, 52));
    b(p, x, y, (12, 5, 5, 10), Color::rgb(200, 52, 52));
    l(p, x, y, &[(12, 18), (19, 11)], BLUE, 2);
}
pub fn viewer3d(p: &mut Painter, x: i32, y: i32) {
    f(p, x, y, &[(2, 8), (10, 4), (18, 8), (10, 12)], GREEN);
    f(
        p,
        x,
        y,
        &[(2, 8), (10, 12), (10, 16), (2, 12)],
        Color::rgb(0, 100, 0),
    );
    f(
        p,
        x,
        y,
        &[(10, 12), (18, 8), (18, 12), (10, 16)],
        Color::rgb(0, 120, 0),
    );
}
pub fn pad(p: &mut Painter, x: i32, y: i32) {
    b(p, x, y, (4, 4, 12, 12), Color::rgb(200, 52, 52));
    p.circle(x + 10, y + 10, 3, Color::rgb(40, 40, 40));
}
pub fn pin(p: &mut Painter, x: i32, y: i32) {
    l(p, x, y, &[(1, 10), (13, 10)], BODY, 2);
    p.ring(x + 15, y + 10, 3, 1, BODY);
}
pub fn circle(p: &mut Painter, x: i32, y: i32) {
    p.ring(x + 10, y + 10, 7, 2, BODY);
}
pub fn text(p: &mut Painter, x: i32, y: i32) {
    l(p, x, y, &[(4, 4), (16, 4)], DARK, 2);
    l(p, x, y, &[(10, 4), (10, 17)], DARK, 2);
}
pub fn orbit(p: &mut Painter, x: i32, y: i32) {
    p.ring(x + 10, y + 10, 7, 1, DARK);
    f(p, x, y, &[(14, 1), (19, 4), (14, 7)], BLUE);
}
pub fn pan(p: &mut Painter, x: i32, y: i32) {
    l(p, x, y, &[(10, 2), (10, 18)], DARK, 2);
    l(p, x, y, &[(2, 10), (18, 10)], DARK, 2);
    f(p, x, y, &[(10, 0), (13, 4), (7, 4)], DARK);
    f(p, x, y, &[(10, 20), (13, 16), (7, 16)], DARK);
}
pub fn keepout(p: &mut Painter, x: i32, y: i32) {
    l(
        p,
        x,
        y,
        &[(3, 3), (17, 3), (17, 17), (3, 17), (3, 3)],
        RED,
        2,
    );
    l(p, x, y, &[(3, 17), (17, 3)], RED, 1);
    l(p, x, y, &[(3, 10), (10, 3)], RED, 1);
    l(p, x, y, &[(10, 17), (17, 10)], RED, 1);
}
