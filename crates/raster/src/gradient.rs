//! The gradient ("blend") tool: a colour ramp laid along a dragged line, in the shapes
//! and repeat modes GIMP, Pinta and Pixelmator Pro offer, painted through the selection
//! with the tool's opacity and mode. Positions are evaluated at pixel centres with
//! correctly rounded arithmetic ([`fmath`]), so every target paints the same bytes.
use crate::blend::BlendMode;
use crate::draw::{apply_colors, BrushKind};
use crate::fmath;
use crate::mask::P16;
use crate::{Canvas, IRect, Mask, Rgba};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GradientShape {
    #[default]
    Linear,
    /// Mirrored about the start point (GIMP's Bi-linear, Pinta's Linear Reflected).
    BiLinear,
    Radial,
    /// Concentric squares (GIMP's Square).
    Square,
    /// Concentric diamonds (Pinta's Linear Diamond).
    Diamond,
    /// The angle from the dragged line, both ways round (GIMP's Conical symmetric).
    ConicalSymmetric,
    /// The angle all the way round (GIMP's Conical asymmetric, Pinta's Conical,
    /// Pixelmator's Angle).
    ConicalAsymmetric,
}
impl GradientShape {
    pub const ALL: [Self; 7] = [
        Self::Linear,
        Self::BiLinear,
        Self::Radial,
        Self::Square,
        Self::Diamond,
        Self::ConicalSymmetric,
        Self::ConicalAsymmetric,
    ];
    pub fn id(self) -> &'static str {
        match self {
            Self::Linear => "linear",
            Self::BiLinear => "bilinear",
            Self::Radial => "radial",
            Self::Square => "square",
            Self::Diamond => "diamond",
            Self::ConicalSymmetric => "conical-sym",
            Self::ConicalAsymmetric => "conical-asym",
        }
    }
    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.id() == id)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Repeat {
    /// The end colours extend past the line's ends.
    #[default]
    None,
    /// The ramp starts over at each length.
    Sawtooth,
    /// The ramp runs forward then back.
    Triangular,
    /// Nothing is painted past the end.
    Truncate,
}
impl Repeat {
    pub const ALL: [Self; 4] = [Self::None, Self::Sawtooth, Self::Triangular, Self::Truncate];
    pub fn id(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Sawtooth => "sawtooth",
            Self::Triangular => "triangular",
            Self::Truncate => "truncate",
        }
    }
    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.id() == id)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gradient {
    pub shape: GradientShape,
    pub repeat: Repeat,
    /// Colour at the start of the line, and at its end.
    pub from: Rgba,
    pub to: Rgba,
    pub reverse: bool,
    /// The dragged line, sub16.
    pub start: P16,
    pub end: P16,
    /// 0..=255.
    pub opacity: u8,
    pub blend: BlendMode,
}

impl Gradient {
    /// Position along the ramp at the centre of pixel `(x, y)`, after the repeat mode;
    /// `None` where a truncated gradient paints nothing, or the line has no length.
    pub fn position(&self, x: i32, y: i32) -> Option<f64> {
        let (ax, ay) = (self.start.0 as f64 / 16.0, self.start.1 as f64 / 16.0);
        let (dx, dy) = (
            (self.end.0 - self.start.0) as f64 / 16.0,
            (self.end.1 - self.start.1) as f64 / 16.0,
        );
        let len2 = dx * dx + dy * dy;
        if len2 == 0.0 {
            return None;
        }
        let (px, py) = (f64::from(x) + 0.5 - ax, f64::from(y) + 0.5 - ay);
        // Along the line and across it, in lengths of the line.
        let u = (px * dx + py * dy) / len2;
        let v = (px * dy - py * dx) / len2;
        let t = match self.shape {
            GradientShape::Linear => u,
            GradientShape::BiLinear => u.abs(),
            GradientShape::Radial => (u * u + v * v).sqrt(),
            GradientShape::Square => u.abs().max(v.abs()),
            GradientShape::Diamond => u.abs() + v.abs(),
            GradientShape::ConicalSymmetric => {
                fmath::atan2((px * dy - py * dx).abs(), px * dx + py * dy) / fmath::PI
            }
            GradientShape::ConicalAsymmetric => {
                let a = fmath::atan2(-(px * dy - py * dx), px * dx + py * dy) / (2.0 * fmath::PI);
                if a < 0.0 {
                    a + 1.0
                } else {
                    a
                }
            }
        };
        let t = match self.repeat {
            Repeat::None => t.clamp(0.0, 1.0),
            Repeat::Truncate => {
                if !(0.0..=1.0).contains(&t) {
                    return None;
                }
                t
            }
            Repeat::Sawtooth => t - t.floor(),
            Repeat::Triangular => {
                let s = t - 2.0 * (t / 2.0).floor();
                if s > 1.0 {
                    2.0 - s
                } else {
                    s
                }
            }
        };
        Some(if self.reverse { 1.0 - t } else { t })
    }
    /// The ramp's colour at `t` (0..=1): each channel mixed straight.
    pub fn color(&self, t: f64) -> Rgba {
        std::array::from_fn(|c| {
            let (a, b) = (f64::from(self.from[c]), f64::from(self.to[c]));
            fmath::to_u8(a + (b - a) * t)
        })
    }
    /// Paint onto `layer` within `selection`; the area painted, if any.
    pub fn apply(&self, layer: &mut Canvas, selection: Option<&Mask>) -> Option<IRect> {
        if self.start == self.end {
            return None;
        }
        let area = match selection {
            Some(sel) => sel.bounds()?,
            None => layer.bounds(),
        };
        apply_colors(
            layer,
            None,
            area,
            selection,
            BrushKind::Paint,
            self.opacity,
            self.blend,
            |x, y| match self.position(x, y) {
                Some(t) => (255, self.color(t)),
                None => (0, [0; 4]),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BLACK, WHITE};
    fn ramp(shape: GradientShape, repeat: Repeat, start: (i64, i64), end: (i64, i64)) -> Gradient {
        Gradient {
            shape,
            repeat,
            from: BLACK,
            to: WHITE,
            reverse: false,
            start: (start.0 * 16, start.1 * 16),
            end: (end.0 * 16, end.1 * 16),
            opacity: 255,
            blend: BlendMode::Normal,
        }
    }
    #[test]
    fn a_linear_ramp_runs_from_colour_to_colour_exactly() {
        let mut c = Canvas::filled(256, 4, [9, 9, 9, 255]);
        ramp(GradientShape::Linear, Repeat::None, (0, 0), (256, 0))
            .apply(&mut c, None)
            .unwrap();
        // Pixel x's centre is (x + 1/2) / 256 of the way: 255 (x + 1/2) / 256.
        assert_eq!(c.get(0, 0), [0, 0, 0, 255]);
        assert_eq!(c.get(128, 3), [128, 128, 128, 255]);
        assert_eq!(c.get(255, 1), [255, 255, 255, 255]);
        for x in 0..255 {
            assert!(c.get(x, 0)[0] <= c.get(x + 1, 0)[0]);
        }
        // Reversed and seen through a selection.
        let mut c = Canvas::filled(256, 1, [9, 9, 9, 255]);
        let sel = Mask::rect(256, 1, IRect::new(0, 0, 100, 1));
        let mut g = ramp(GradientShape::Linear, Repeat::None, (0, 0), (256, 0));
        g.reverse = true;
        g.apply(&mut c, Some(&sel)).unwrap();
        assert_eq!(c.get(0, 0), [255, 255, 255, 255]);
        assert_eq!(c.get(100, 0), [9, 9, 9, 255], "outside the selection");
    }
    #[test]
    fn shapes_and_repeats_place_the_ramp() {
        let at = |g: &Gradient, x, y| g.position(x, y).map(|t| g.color(t)[0]);
        // Radial from the centre of a 100-pixel line: 0 at the centre, 255 beyond.
        let r = ramp(GradientShape::Radial, Repeat::None, (50, 50), (150, 50));
        assert_eq!(at(&r, 50, 50), Some(2)); // centre of the pixel is ~0.7 px out
        assert_eq!(at(&r, 50, 99), Some(126));
        assert_eq!(at(&r, 0, 0), Some(179)); // 70.0 px out of 100
        assert_eq!(at(&r, 50, 160), Some(255));
        // Bi-linear mirrors about the start.
        let b = ramp(GradientShape::BiLinear, Repeat::None, (50, 0), (60, 0));
        assert_eq!(at(&b, 44, 0), at(&b, 55, 0));
        // Triangular goes back down after the end; truncate stops.
        let tri = ramp(GradientShape::Linear, Repeat::Triangular, (0, 0), (10, 0));
        assert_eq!(at(&tri, 4, 0), at(&tri, 15, 0));
        let saw = ramp(GradientShape::Linear, Repeat::Sawtooth, (0, 0), (10, 0));
        assert_eq!(at(&saw, 4, 0), at(&saw, 14, 0));
        let cut = ramp(GradientShape::Linear, Repeat::Truncate, (0, 0), (10, 0));
        assert_eq!(at(&cut, 12, 0), None);
        assert_eq!(at(&cut, 9, 0), Some(242));
        // Square and diamond: equal along both axes at equal distance.
        let sq = ramp(GradientShape::Square, Repeat::None, (50, 50), (70, 50));
        assert_eq!(at(&sq, 59, 49), at(&sq, 49, 59));
        let di = ramp(GradientShape::Diamond, Repeat::None, (50, 50), (70, 50));
        assert_eq!(at(&di, 55, 54), at(&di, 54, 55));
        // Conical: half-way round is the end colour (symmetric) or the middle
        // (asymmetric); a quarter turn is the middle or a quarter.
        let cs = ramp(
            GradientShape::ConicalSymmetric,
            Repeat::None,
            (50, 50),
            (60, 50),
        );
        assert_eq!(at(&cs, 20, 49), Some(254));
        let ca = ramp(
            GradientShape::ConicalAsymmetric,
            Repeat::None,
            (50, 50),
            (60, 50),
        );
        // Clockwise on screen: a quarter turn is below the start, three quarters above.
        let quarter = at(&ca, 49, 80).unwrap();
        let three = at(&ca, 49, 20).unwrap();
        assert!((60..=68).contains(&quarter), "{quarter}");
        assert!((188..=196).contains(&three), "{three}");
        // A click without a drag paints nothing.
        let mut c = Canvas::new(4, 4);
        assert!(ramp(GradientShape::Linear, Repeat::None, (1, 1), (1, 1))
            .apply(&mut c, None)
            .is_none());
    }
    #[test]
    fn foreground_to_transparent_fades_over_what_is_there() {
        let mut c = Canvas::filled(100, 1, WHITE);
        let mut g = ramp(GradientShape::Linear, Repeat::None, (0, 0), (100, 0));
        g.from = [255, 0, 0, 255];
        g.to = [255, 0, 0, 0];
        g.apply(&mut c, None).unwrap();
        assert_eq!(c.get(0, 0), [255, 1, 1, 255]);
        assert_eq!(c.get(99, 0), [255, 254, 254, 255]);
        let mid = c.get(50, 0);
        assert!(mid[1] > 120 && mid[1] < 135, "{mid:?}");
    }
}
