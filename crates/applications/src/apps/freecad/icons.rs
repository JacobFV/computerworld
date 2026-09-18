//! Toolbar and tree icons, drawn as small vector pictograms in FreeCAD's colour language
//! (yellow Part Design solids, red sketches and constraints, blue construction). They
//! are original drawings, not FreeCAD's artwork.
use crate::desktop_scene::shared::Align;
use crate::desktop_scene::Painter;
use cw_cad::sketch::ConstraintType as T;
use cw_scene::{Color, Rect};

const YELLOW: Color = Color::rgb(252, 233, 79);
const YELLOW_DARK: Color = Color::rgb(196, 160, 0);
const RED: Color = Color::rgb(204, 0, 0);
const BLUE: Color = Color::rgb(52, 101, 164);
const LIGHT_BLUE: Color = Color::rgb(114, 159, 207);
const GRAY: Color = Color::rgb(85, 87, 83);
const WHITE: Color = Color::rgb(255, 255, 255);
const CONSTRUCTION: Color = Color::rgb(59, 91, 219);

struct Pen<'a> {
    p: &'a mut Painter,
    x: i32,
    y: i32,
    s: f64,
    fade: bool,
}
impl Pen<'_> {
    fn at(&self, x: f64, y: f64) -> (i32, i32) {
        (
            self.x + (x * self.s).round() as i32,
            self.y + (y * self.s).round() as i32,
        )
    }
    fn c(&self, c: Color) -> Color {
        if self.fade {
            Color(c.0, c.1, c.2, c.3 / 3)
        } else {
            c
        }
    }
    fn poly(&mut self, pts: &[(f64, f64)], fill: Color, edge: Option<Color>) {
        let v: Vec<(i32, i32)> = pts.iter().map(|(x, y)| self.at(*x, *y)).collect();
        let f = self.c(fill);
        self.p.path(v.clone(), f);
        if let Some(e) = edge {
            let mut v = v;
            v.push(v[0]);
            let e = self.c(e);
            self.p.line(v, e, 1);
        }
    }
    fn line(&mut self, pts: &[(f64, f64)], color: Color, w: u16) {
        let v: Vec<(i32, i32)> = pts.iter().map(|(x, y)| self.at(*x, *y)).collect();
        let c = self.c(color);
        let w = ((f64::from(w) * self.s).round() as u16).max(1);
        self.p.line(v, c, w);
    }
    fn dot(&mut self, x: f64, y: f64, r: f64, color: Color) {
        let (cx, cy) = self.at(x, y);
        let r = ((r * self.s).round() as u32).max(1);
        let c = self.c(color);
        self.p.circle(cx, cy, r, c);
    }
    fn ring(&mut self, x: f64, y: f64, r: f64, color: Color) {
        let (cx, cy) = self.at(x, y);
        let r = ((r * self.s).round() as u32).max(2);
        let c = self.c(color);
        self.p.ring(cx, cy, r, 1, c);
    }
    #[allow(clippy::too_many_arguments)]
    fn arc(&mut self, cx: f64, cy: f64, r: f64, from: i32, to: i32, color: Color, w: u16) {
        let (x, y) = self.at(cx, cy);
        let pts = crate::desktop_scene::shared::arc_points(
            x,
            y,
            (r * self.s).round() as i32,
            from,
            to,
            10,
        );
        let c = self.c(color);
        self.p
            .line(pts, c, ((f64::from(w) * self.s).round() as u16).max(1));
    }
    fn text(&mut self, t: &str, color: Color) {
        let size = (self.s * 16.0) as u32;
        let c = self.c(color);
        self.p.label(
            self.x,
            self.y + (size as i32 - (size as i32 * 9 / 10)) / 2 - 1,
            size,
            t,
            ((size * 7 / 10).max(8)) as u16,
            c,
            true,
            Align::Center,
        );
    }
    /// An isometric box: front, right and top faces.
    fn block(&mut self, fill: Color, edge: Color) {
        let front = [(2.0, 6.0), (10.0, 6.0), (10.0, 14.0), (2.0, 14.0)];
        let top = [(2.0, 6.0), (6.0, 2.0), (14.0, 2.0), (10.0, 6.0)];
        let right = [(10.0, 6.0), (14.0, 2.0), (14.0, 10.0), (10.0, 14.0)];
        self.poly(&front, fill, Some(edge));
        self.poly(&top, lighten(fill), Some(edge));
        self.poly(&right, darken(fill), Some(edge));
    }
}
fn lighten(c: Color) -> Color {
    Color(
        c.0.saturating_add((255 - c.0) / 2),
        c.1.saturating_add((255 - c.1) / 2),
        c.2.saturating_add((255 - c.2) / 2),
        c.3,
    )
}
fn darken(c: Color) -> Color {
    Color(
        (u32::from(c.0) * 3 / 4) as u8,
        (u32::from(c.1) * 3 / 4) as u8,
        (u32::from(c.2) * 3 / 4) as u8,
        c.3,
    )
}

/// Draw icon `id` in a `size`×`size` square at (`x`, `y`).
pub fn draw(p: &mut Painter, id: &str, x: i32, y: i32, size: u32, enabled: bool) {
    let mut g = Pen {
        p,
        x,
        y,
        s: f64::from(size) / 16.0,
        fade: !enabled,
    };
    let std_cube = |g: &mut Pen, hi: &str| {
        let front = [(2.0, 6.0), (10.0, 6.0), (10.0, 14.0), (2.0, 14.0)];
        let top = [(2.0, 6.0), (6.0, 2.0), (14.0, 2.0), (10.0, 6.0)];
        let right = [(10.0, 6.0), (14.0, 2.0), (14.0, 10.0), (10.0, 14.0)];
        let face = |name: &str| {
            if hi == name || hi == "all" {
                LIGHT_BLUE
            } else {
                WHITE
            }
        };
        let back = |name: &str| if hi == name { BLUE } else { WHITE };
        g.poly(
            &front,
            if hi == "rear" {
                back("rear")
            } else {
                face("front")
            },
            Some(GRAY),
        );
        g.poly(
            &top,
            if hi == "bottom" {
                back("bottom")
            } else {
                face("top")
            },
            Some(GRAY),
        );
        g.poly(
            &right,
            if hi == "left" {
                back("left")
            } else {
                face("right")
            },
            Some(GRAY),
        );
    };
    match id {
        "Std_New" | "Document" => {
            g.poly(
                &[
                    (3.0, 1.0),
                    (10.0, 1.0),
                    (13.0, 4.0),
                    (13.0, 15.0),
                    (3.0, 15.0),
                ],
                WHITE,
                Some(GRAY),
            );
            g.line(&[(10.0, 1.0), (10.0, 4.0), (13.0, 4.0)], GRAY, 1);
            if id == "Document" {
                g.line(&[(5.0, 8.0), (11.0, 8.0)], LIGHT_BLUE, 1);
                g.line(&[(5.0, 11.0), (11.0, 11.0)], LIGHT_BLUE, 1);
            }
        }
        "Std_Open" => {
            g.poly(
                &[
                    (1.0, 4.0),
                    (6.0, 4.0),
                    (7.0, 5.0),
                    (14.0, 5.0),
                    (14.0, 14.0),
                    (1.0, 14.0),
                ],
                Color::rgb(233, 185, 110),
                Some(Color::rgb(143, 89, 2)),
            );
            g.poly(
                &[(3.0, 8.0), (15.0, 8.0), (13.0, 14.0), (1.0, 14.0)],
                Color::rgb(252, 175, 62),
                Some(Color::rgb(143, 89, 2)),
            );
        }
        "Std_Save" => {
            g.poly(
                &[
                    (2.0, 2.0),
                    (13.0, 2.0),
                    (15.0, 4.0),
                    (15.0, 15.0),
                    (2.0, 15.0),
                ],
                LIGHT_BLUE,
                Some(BLUE),
            );
            g.poly(
                &[(5.0, 2.0), (12.0, 2.0), (12.0, 6.0), (5.0, 6.0)],
                WHITE,
                None,
            );
            g.poly(
                &[(4.0, 9.0), (13.0, 9.0), (13.0, 15.0), (4.0, 15.0)],
                WHITE,
                Some(BLUE),
            );
        }
        "Std_SaveAs" => draw_inner(&mut g, "Std_Save"),
        "Std_Undo" | "Std_Redo" => {
            let flip = id == "Std_Redo";
            let fx = |x: f64| if flip { 16.0 - x } else { x };
            g.arc(
                8.0,
                9.0,
                5.0,
                if flip { 300 } else { 60 },
                if flip { 420 } else { 180 },
                BLUE,
                2,
            );
            let (ax, ay) = (fx(3.0), 7.0);
            g.poly(
                &[(ax, ay - 4.0), (ax, ay + 2.0), (fx(8.0), ay - 1.0)],
                BLUE,
                None,
            );
        }
        "Std_Delete" => {
            g.line(&[(3.0, 3.0), (13.0, 13.0)], RED, 2);
            g.line(&[(13.0, 3.0), (3.0, 13.0)], RED, 2);
        }
        "Std_Refresh" => {
            g.arc(8.0, 8.0, 5.0, 30, 320, Color::rgb(78, 154, 6), 2);
            g.poly(
                &[(11.0, 1.0), (14.0, 5.0), (9.0, 5.0)],
                Color::rgb(78, 154, 6),
                None,
            );
        }
        "Std_ViewFitAll" | "Std_ViewFitSelection" => {
            g.poly(
                &[(2.0, 2.0), (11.0, 2.0), (11.0, 11.0), (2.0, 11.0)],
                lighten(LIGHT_BLUE),
                Some(GRAY),
            );
            g.ring(9.0, 9.0, 4.0, Color::rgb(46, 52, 54));
            g.line(&[(12.0, 12.0), (15.0, 15.0)], Color::rgb(46, 52, 54), 2);
        }
        "Std_ViewIsometric" => std_cube(&mut g, "all"),
        "Std_ViewFront" => std_cube(&mut g, "front"),
        "Std_ViewTop" => std_cube(&mut g, "top"),
        "Std_ViewRight" => std_cube(&mut g, "right"),
        "Std_ViewRear" => std_cube(&mut g, "rear"),
        "Std_ViewBottom" => std_cube(&mut g, "bottom"),
        "Std_ViewLeft" => std_cube(&mut g, "left"),
        "Std_OrthographicCamera" => {
            g.poly(
                &[(3.0, 4.0), (13.0, 4.0), (13.0, 12.0), (3.0, 12.0)],
                WHITE,
                Some(GRAY),
            );
            g.poly(
                &[(5.0, 6.0), (11.0, 6.0), (11.0, 10.0), (5.0, 10.0)],
                LIGHT_BLUE,
                Some(GRAY),
            );
        }
        "Std_PerspectiveCamera" => {
            g.poly(
                &[(2.0, 3.0), (14.0, 5.0), (14.0, 11.0), (2.0, 13.0)],
                WHITE,
                Some(GRAY),
            );
            g.poly(
                &[(5.0, 6.0), (11.0, 7.0), (11.0, 9.0), (5.0, 10.0)],
                LIGHT_BLUE,
                Some(GRAY),
            );
        }
        "Std_ToggleVisibility" => {
            g.poly(
                &[(1.0, 8.0), (8.0, 3.0), (15.0, 8.0), (8.0, 13.0)],
                WHITE,
                Some(GRAY),
            );
            g.dot(8.0, 8.0, 3.0, BLUE);
        }
        "Std_SelBoundingBox" => {
            g.poly(
                &[(3.0, 3.0), (13.0, 3.0), (13.0, 13.0), (3.0, 13.0)],
                Color::TRANSPARENT,
                Some(GRAY),
            );
            g.block(YELLOW, YELLOW_DARK);
        }
        "Std_AxisCross" => {
            g.line(&[(8.0, 9.0), (15.0, 13.0)], RED, 1);
            g.line(&[(8.0, 9.0), (1.0, 13.0)], Color::rgb(78, 154, 6), 1);
            g.line(&[(8.0, 9.0), (8.0, 1.0)], BLUE, 1);
        }
        "Std_ReportView" => {
            g.poly(
                &[(1.0, 3.0), (15.0, 3.0), (15.0, 13.0), (1.0, 13.0)],
                WHITE,
                Some(GRAY),
            );
            g.line(&[(3.0, 6.0), (12.0, 6.0)], GRAY, 1);
            g.line(&[(3.0, 9.0), (10.0, 9.0)], RED, 1);
        }
        "Std_Measure" => {
            g.poly(
                &[(1.0, 10.0), (10.0, 1.0), (15.0, 6.0), (6.0, 15.0)],
                YELLOW,
                Some(YELLOW_DARK),
            );
            for k in 0..4 {
                let t = 3.0 + f64::from(k) * 2.2;
                g.line(&[(t, 12.0 - t + 3.0), (t + 1.5, 13.5 - t + 3.0)], GRAY, 1);
            }
        }
        "Std_About" | "FreeCAD" => {
            g.poly(
                &[(1.0, 1.0), (15.0, 1.0), (15.0, 15.0), (1.0, 15.0)],
                Color::rgb(70, 110, 170),
                None,
            );
            g.poly(
                &[
                    (4.0, 3.0),
                    (12.0, 3.0),
                    (12.0, 5.5),
                    (6.8, 5.5),
                    (6.8, 7.5),
                    (11.0, 7.5),
                    (11.0, 10.0),
                    (6.8, 10.0),
                    (6.8, 13.0),
                    (4.0, 13.0),
                ],
                WHITE,
                None,
            );
            g.dot(12.0, 12.0, 1.8, RED);
        }
        "PartDesign_Body" | "PartDesign::Body" => g.block(LIGHT_BLUE, BLUE),
        "PartDesign_Workbench" => g.block(YELLOW, YELLOW_DARK),
        "Sketcher_Workbench"
        | "PartDesign_NewSketch"
        | "Sketcher::SketchObject"
        | "Sketcher_EditSketch" => {
            g.poly(
                &[(1.0, 13.0), (5.0, 3.0), (15.0, 3.0), (11.0, 13.0)],
                WHITE,
                Some(GRAY),
            );
            g.line(
                &[
                    (4.0, 10.0),
                    (7.0, 6.0),
                    (11.0, 6.0),
                    (10.0, 10.0),
                    (4.0, 10.0),
                ],
                RED,
                1,
            );
            g.dot(7.0, 6.0, 1.0, RED);
        }
        "PartDesign_Pad" | "PartDesign::Pad" => {
            g.block(YELLOW, YELLOW_DARK);
            g.line(&[(6.0, 11.0), (6.0, 3.0)], RED, 2);
            g.poly(&[(3.5, 5.0), (6.0, 1.0), (8.5, 5.0)], RED, None);
        }
        "PartDesign_Pocket" | "PartDesign::Pocket" => {
            g.block(YELLOW, YELLOW_DARK);
            g.poly(
                &[(4.5, 4.5), (8.0, 2.5), (11.5, 2.5), (8.0, 4.5)],
                Color::rgb(80, 60, 10),
                None,
            );
            g.line(&[(8.0, 1.0), (8.0, 9.0)], RED, 2);
        }
        "PartDesign_Hole" | "PartDesign::Hole" => {
            g.block(YELLOW, YELLOW_DARK);
            g.dot(6.0, 10.0, 2.5, Color::rgb(80, 60, 10));
            g.dot(8.0, 4.0, 1.4, Color::rgb(80, 60, 10));
        }
        "PartDesign_Revolution" | "PartDesign::Revolution" => {
            g.poly(
                &[(3.0, 4.0), (13.0, 4.0), (13.0, 12.0), (3.0, 12.0)],
                YELLOW,
                Some(YELLOW_DARK),
            );
            g.poly(
                &[(3.0, 4.0), (13.0, 4.0), (11.0, 6.0), (5.0, 6.0)],
                lighten(YELLOW),
                None,
            );
            g.arc(8.0, 8.0, 7.0, 200, 340, RED, 2);
        }
        "PartDesign_Groove" | "PartDesign::Groove" => {
            g.poly(
                &[(3.0, 3.0), (13.0, 3.0), (13.0, 13.0), (3.0, 13.0)],
                YELLOW,
                Some(YELLOW_DARK),
            );
            g.poly(
                &[(3.0, 7.0), (13.0, 7.0), (13.0, 9.0), (3.0, 9.0)],
                darken(YELLOW_DARK),
                None,
            );
            g.arc(8.0, 8.0, 7.0, 200, 340, RED, 2);
        }
        "PartDesign_Fillet" | "PartDesign::Fillet" => {
            g.poly(
                &[
                    (2.0, 14.0),
                    (2.0, 6.0),
                    (4.0, 3.5),
                    (7.0, 2.0),
                    (14.0, 2.0),
                    (14.0, 14.0),
                ],
                YELLOW,
                Some(YELLOW_DARK),
            );
            g.arc(7.0, 7.0, 5.0, 270, 360, RED, 2);
        }
        "PartDesign_Chamfer" | "PartDesign::Chamfer" => {
            g.poly(
                &[
                    (2.0, 14.0),
                    (2.0, 7.0),
                    (7.0, 2.0),
                    (14.0, 2.0),
                    (14.0, 14.0),
                ],
                YELLOW,
                Some(YELLOW_DARK),
            );
            g.line(&[(2.0, 7.0), (7.0, 2.0)], RED, 2);
        }
        "PartDesign_Mirrored" | "PartDesign::Mirrored" => {
            g.poly(
                &[(1.0, 5.0), (6.0, 3.0), (6.0, 13.0), (1.0, 11.0)],
                YELLOW,
                Some(YELLOW_DARK),
            );
            g.poly(
                &[(15.0, 5.0), (10.0, 3.0), (10.0, 13.0), (15.0, 11.0)],
                lighten(YELLOW),
                Some(YELLOW_DARK),
            );
            g.line(&[(8.0, 1.0), (8.0, 3.5)], RED, 1);
            g.line(&[(8.0, 6.0), (8.0, 9.5)], RED, 1);
            g.line(&[(8.0, 12.0), (8.0, 15.0)], RED, 1);
        }
        "PartDesign_LinearPattern" | "PartDesign::LinearPattern" => {
            for k in 0..3 {
                let o = f64::from(k) * 5.0;
                g.poly(
                    &[
                        (o + 1.0, 6.0),
                        (o + 5.0, 6.0),
                        (o + 5.0, 10.0),
                        (o + 1.0, 10.0),
                    ],
                    if k == 0 { YELLOW } else { lighten(YELLOW) },
                    Some(YELLOW_DARK),
                );
            }
            g.line(&[(1.0, 13.0), (15.0, 13.0)], RED, 1);
        }
        "PartDesign_PolarPattern" | "PartDesign::PolarPattern" => {
            g.ring(8.0, 8.0, 5.0, RED);
            for (cx, cy) in [(8.0, 3.0), (12.5, 10.5), (3.5, 10.5)] {
                g.poly(
                    &[
                        (cx - 2.0, cy - 2.0),
                        (cx + 2.0, cy - 2.0),
                        (cx + 2.0, cy + 2.0),
                        (cx - 2.0, cy + 2.0),
                    ],
                    YELLOW,
                    Some(YELLOW_DARK),
                );
            }
        }
        "PartDesign_MoveTip" => {
            g.block(YELLOW, YELLOW_DARK);
            g.poly(
                &[
                    (9.0, 9.0),
                    (15.0, 12.0),
                    (12.0, 12.5),
                    (13.5, 15.0),
                    (12.0, 15.5),
                    (10.8, 13.2),
                    (9.0, 15.0),
                ],
                Color::rgb(78, 154, 6),
                None,
            );
        }
        "Part::Feature" => {
            // A solid brick, as FreeCAD's Part shapes are drawn.
            g.poly(
                &[(2.0, 5.0), (8.0, 2.0), (14.0, 5.0), (8.0, 8.0)],
                Color::rgb(203, 214, 226),
                Some(GRAY),
            );
            g.poly(
                &[(2.0, 5.0), (8.0, 8.0), (8.0, 14.0), (2.0, 11.0)],
                Color::rgb(160, 176, 192),
                Some(GRAY),
            );
            g.poly(
                &[(14.0, 5.0), (8.0, 8.0), (8.0, 14.0), (14.0, 11.0)],
                Color::rgb(132, 151, 171),
                Some(GRAY),
            );
        }
        "Mesh::Feature" => {
            g.poly(
                &[(2.0, 13.0), (8.0, 2.0), (14.0, 13.0)],
                Color::rgb(186, 189, 182),
                Some(GRAY),
            );
            g.line(&[(5.0, 7.5), (11.0, 7.5), (8.0, 13.0), (5.0, 7.5)], GRAY, 1);
        }
        "App::Origin" => {
            g.line(&[(8.0, 9.0), (15.0, 13.0)], RED, 1);
            g.line(&[(8.0, 9.0), (1.0, 13.0)], Color::rgb(78, 154, 6), 1);
            g.line(&[(8.0, 9.0), (8.0, 1.0)], BLUE, 1);
        }
        "App::Plane" => g.poly(
            &[(1.0, 12.0), (5.0, 4.0), (15.0, 4.0), (11.0, 12.0)],
            Color(114, 159, 207, 140),
            Some(BLUE),
        ),
        "App::Line" => g.line(&[(2.0, 14.0), (14.0, 2.0)], BLUE, 2),
        "Mouse" => {
            g.poly(
                &[(4.0, 3.0), (12.0, 3.0), (12.0, 13.0), (4.0, 13.0)],
                WHITE,
                Some(GRAY),
            );
            g.line(&[(8.0, 3.0), (8.0, 7.0)], GRAY, 1);
        }
        "Sketcher_LeaveSketch" => {
            draw_inner(&mut g, "Sketcher::SketchObject");
            g.poly(
                &[
                    (9.0, 10.0),
                    (13.0, 10.0),
                    (13.0, 8.0),
                    (16.0, 11.5),
                    (13.0, 15.0),
                    (13.0, 13.0),
                    (9.0, 13.0),
                ],
                Color::rgb(78, 154, 6),
                None,
            );
        }
        "Sketcher_ViewSketch" => {
            g.poly(
                &[(1.0, 3.0), (15.0, 3.0), (15.0, 13.0), (1.0, 13.0)],
                WHITE,
                Some(GRAY),
            );
            g.line(&[(4.0, 10.0), (7.0, 6.0), (12.0, 6.0)], RED, 1);
        }
        "Sketcher_CreatePoint" => g.dot(8.0, 8.0, 2.5, RED),
        "Sketcher_CreateLine" => {
            g.line(&[(3.0, 13.0), (13.0, 3.0)], Color::rgb(46, 52, 54), 1);
            g.dot(3.0, 13.0, 1.5, RED);
            g.dot(13.0, 3.0, 1.5, RED);
        }
        "Sketcher_CreatePolyline" => {
            g.line(
                &[(2.0, 13.0), (6.0, 4.0), (10.0, 12.0), (14.0, 3.0)],
                Color::rgb(46, 52, 54),
                1,
            );
            for (x, y) in [(2.0, 13.0), (6.0, 4.0), (10.0, 12.0), (14.0, 3.0)] {
                g.dot(x, y, 1.2, RED);
            }
        }
        "Sketcher_CreateArc" | "Sketcher_Create3PointArc" => {
            g.arc(8.0, 11.0, 6.0, 270, 450, Color::rgb(46, 52, 54), 1);
            g.dot(2.0, 11.0, 1.3, RED);
            g.dot(14.0, 11.0, 1.3, RED);
            if id == "Sketcher_CreateArc" {
                g.dot(8.0, 11.0, 1.3, RED);
            } else {
                g.dot(8.0, 5.0, 1.3, RED);
            }
        }
        "Sketcher_CreateCircle" | "Sketcher_Create3PointCircle" => {
            g.ring(8.0, 8.0, 6.0, Color::rgb(46, 52, 54));
            if id == "Sketcher_CreateCircle" {
                g.dot(8.0, 8.0, 1.3, RED);
            } else {
                for (x, y) in [(2.0, 8.0), (14.0, 8.0), (8.0, 2.0)] {
                    g.dot(x, y, 1.3, RED);
                }
            }
        }
        "Sketcher_CreateRectangle" => {
            g.poly(
                &[(2.0, 4.0), (14.0, 4.0), (14.0, 12.0), (2.0, 12.0)],
                Color::TRANSPARENT,
                Some(Color::rgb(46, 52, 54)),
            );
            for (x, y) in [(2.0, 4.0), (14.0, 4.0), (14.0, 12.0), (2.0, 12.0)] {
                g.dot(x, y, 1.2, RED);
            }
        }
        "Sketcher_CreateSlot" => {
            g.arc(5.0, 8.0, 4.0, 180, 360, Color::rgb(46, 52, 54), 1);
            g.arc(11.0, 8.0, 4.0, 0, 180, Color::rgb(46, 52, 54), 1);
            g.line(&[(5.0, 4.0), (11.0, 4.0)], Color::rgb(46, 52, 54), 1);
            g.line(&[(5.0, 12.0), (11.0, 12.0)], Color::rgb(46, 52, 54), 1);
        }
        "Sketcher_CreateFillet" => {
            g.line(&[(2.0, 14.0), (2.0, 8.0)], Color::rgb(46, 52, 54), 1);
            g.arc(8.0, 8.0, 6.0, 270, 360, RED, 1);
            g.line(&[(8.0, 2.0), (14.0, 2.0)], Color::rgb(46, 52, 54), 1);
        }
        "Sketcher_Trimming" => {
            g.line(&[(1.0, 8.0), (6.0, 8.0)], Color::rgb(46, 52, 54), 1);
            g.line(&[(10.0, 8.0), (15.0, 8.0)], Color::rgb(46, 52, 54), 1);
            g.line(&[(6.0, 8.0), (10.0, 8.0)], Color(204, 0, 0, 120), 1);
            g.line(&[(6.0, 3.0), (6.0, 13.0)], GRAY, 1);
            g.line(&[(10.0, 3.0), (10.0, 13.0)], GRAY, 1);
        }
        "Sketcher_Extend" => {
            g.line(&[(1.0, 8.0), (9.0, 8.0)], Color::rgb(46, 52, 54), 1);
            g.line(&[(9.0, 8.0), (13.0, 8.0)], RED, 1);
            g.line(&[(14.0, 2.0), (14.0, 14.0)], GRAY, 1);
        }
        "Sketcher_ToggleConstruction" => {
            for k in 0..4 {
                let t = f64::from(k) * 3.5;
                g.line(&[(2.0 + t, 14.0 - t), (4.0 + t, 12.0 - t)], CONSTRUCTION, 1);
            }
        }
        "Sketcher_ConstrainLock" => {
            g.poly(
                &[(3.0, 7.0), (13.0, 7.0), (13.0, 15.0), (3.0, 15.0)],
                RED,
                None,
            );
            g.arc(8.0, 7.0, 3.5, 270, 450, RED, 2);
        }
        "Sketcher_SelectConflictingConstraints" => {
            g.poly(
                &[(8.0, 1.0), (15.0, 14.0), (1.0, 14.0)],
                Color::rgb(252, 175, 62),
                Some(Color::rgb(206, 92, 0)),
            );
            g.text("!", Color::rgb(46, 52, 54));
        }
        "Sketcher_ToggleDrivingConstraint" => {
            let r = Rect::new(g.x, g.y, (16.0 * g.s) as u32, (16.0 * g.s) as u32);
            let fade = g.fade;
            badge(g.p, "⇄", r, fade);
        }
        other => {
            if let Some(kind) = constraint_kind(other) {
                let r = Rect::new(g.x, g.y, (16.0 * g.s) as u32, (16.0 * g.s) as u32);
                let fade = g.fade;
                badge(g.p, glyph(kind), r, fade);
            } else {
                g.poly(
                    &[(2.0, 2.0), (14.0, 2.0), (14.0, 14.0), (2.0, 14.0)],
                    WHITE,
                    Some(GRAY),
                );
            }
        }
    }
}

fn draw_inner(g: &mut Pen<'_>, id: &str) {
    let (x, y, s, fade) = (g.x, g.y, (g.s * 16.0) as u32, g.fade);
    draw(g.p, id, x, y, s, !fade);
}

fn constraint_kind(id: &str) -> Option<T> {
    Some(match id {
        "Sketcher_ConstrainCoincident" => T::Coincident,
        "Sketcher_ConstrainPointOnObject" => T::PointOnObject,
        "Sketcher_ConstrainHorizontal" => T::Horizontal,
        "Sketcher_ConstrainVertical" => T::Vertical,
        "Sketcher_ConstrainParallel" => T::Parallel,
        "Sketcher_ConstrainPerpendicular" => T::Perpendicular,
        "Sketcher_ConstrainTangent" => T::Tangent,
        "Sketcher_ConstrainEqual" => T::Equal,
        "Sketcher_ConstrainSymmetric" => T::Symmetric,
        "Sketcher_ConstrainBlock" => T::Block,
        "Sketcher_ConstrainDistanceX" => T::DistanceX,
        "Sketcher_ConstrainDistanceY" => T::DistanceY,
        "Sketcher_ConstrainDistance" => T::Distance,
        "Sketcher_ConstrainRadius" => T::Radius,
        "Sketcher_ConstrainDiameter" => T::Diameter,
        "Sketcher_ConstrainAngle" => T::Angle,
        _ => return None,
    })
}

fn glyph(kind: T) -> &'static str {
    match kind {
        T::Coincident => "•",
        T::PointOnObject => "⊙",
        T::Horizontal => "H",
        T::Vertical => "V",
        T::Parallel => "∥",
        T::Perpendicular => "⊥",
        T::Tangent => "T",
        T::Equal => "=",
        T::Symmetric => "⋈",
        T::Block => "B",
        T::DistanceX => "↔",
        T::DistanceY => "↕",
        T::Distance => "⤢",
        T::Radius => "R",
        T::Diameter => "⌀",
        T::Angle => "∠",
    }
}

fn badge(p: &mut Painter, text: &str, r: Rect, fade: bool) {
    let red = if fade { Color(204, 0, 0, 80) } else { RED };
    p.box_(
        Rect::new(
            r.x + 1,
            r.y + 1,
            r.width.saturating_sub(2),
            r.height.saturating_sub(2),
        ),
        red,
        3,
    );
    let size = (r.height * 7 / 10).max(8) as u16;
    p.label(
        r.x,
        r.y + (r.height as i32 - i32::from(size) * 3 / 2) / 2 + 1,
        r.width,
        text,
        size,
        Color::WHITE,
        true,
        Align::Center,
    );
}

/// The small red badge the constraint list shows beside each constraint.
pub fn constraint_badge(p: &mut Painter, kind: T, x: i32, y: i32) {
    badge(p, glyph(kind), Rect::new(x, y, 16, 16), false);
}
