//! Geometry: crop, resample, quarter turns, flips and free rotation. Resampling works on
//! premultiplied colour so a transparent neighbour never bleeds its nominal colour into
//! an edge, and filter weights are fixed-point integers normalised to sum exactly to one.
use crate::fmath;
use crate::{Canvas, IRect};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Resample {
    Nearest,
    Bilinear,
    #[default]
    Bicubic,
}
impl Resample {
    pub const ALL: [Resample; 3] = [Self::Nearest, Self::Bilinear, Self::Bicubic];
    pub fn id(self) -> &'static str {
        match self {
            Self::Nearest => "nearest",
            Self::Bilinear => "bilinear",
            Self::Bicubic => "bicubic",
        }
    }
    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.id() == id)
    }
    fn support(self) -> f64 {
        match self {
            Self::Nearest => 0.5,
            Self::Bilinear => 1.0,
            Self::Bicubic => 2.0,
        }
    }
    fn weight(self, x: f64) -> f64 {
        let x = if x < 0.0 { -x } else { x };
        match self {
            Self::Nearest => {
                if x < 0.5 {
                    1.0
                } else {
                    0.0
                }
            }
            Self::Bilinear => (1.0 - x).max(0.0),
            // Catmull-Rom (a = -0.5), the cubic most editors call "bicubic".
            Self::Bicubic => {
                let a = -0.5;
                if x < 1.0 {
                    ((a + 2.0) * x - (a + 3.0)) * x * x + 1.0
                } else if x < 2.0 {
                    (((x - 5.0) * x + 8.0) * x - 4.0) * a
                } else {
                    0.0
                }
            }
        }
    }
}

const ONE: i64 = 1 << 14;

/// Per output sample: first source index and fixed-point weights summing to `ONE`.
fn kernel(src: u32, dst: u32, filter: Resample) -> Vec<(usize, Vec<i64>)> {
    let scale = f64::from(src) / f64::from(dst);
    let fscale = scale.max(1.0);
    let support = filter.support() * fscale;
    (0..dst)
        .map(|i| {
            let center = (f64::from(i) + 0.5) * scale;
            let lo = ((center - support + 0.5).floor().max(0.0)) as usize;
            let hi = ((center + support + 0.5).floor().min(f64::from(src))) as usize;
            let hi = hi.max(lo + 1).min(src as usize);
            let lo = lo.min(hi - 1);
            let raw: Vec<f64> = (lo..hi)
                .map(|j| filter.weight((j as f64 + 0.5 - center) / fscale))
                .collect();
            let total: f64 = raw.iter().sum();
            let mut w: Vec<i64> = if total.abs() < 1e-12 {
                // Degenerate: take the nearest sample outright.
                let mut w = vec![0; raw.len()];
                let k = ((center.floor() as usize).clamp(lo, hi - 1)) - lo;
                w[k] = ONE;
                w
            } else {
                raw.iter()
                    .map(|v| fmath::round(v / total * ONE as f64) as i64)
                    .collect()
            };
            // Put any rounding residue on the heaviest tap, so weights sum to exactly one.
            let residue = ONE - w.iter().sum::<i64>();
            if residue != 0 {
                let (k, _) = w
                    .iter()
                    .enumerate()
                    .max_by_key(|(k, v)| (**v, std::cmp::Reverse(*k)))
                    .unwrap();
                w[k] += residue;
            }
            (lo, w)
        })
        .collect()
}

/// Premultiplied working pixel: `[r·a, g·a, b·a, a·255]`.
type Pre = [i64; 4];
fn premultiply(c: &Canvas) -> Vec<Pre> {
    c.pixels()
        .chunks(4)
        .map(|p| {
            let a = i64::from(p[3]);
            [
                i64::from(p[0]) * a,
                i64::from(p[1]) * a,
                i64::from(p[2]) * a,
                a * 255,
            ]
        })
        .collect()
}
fn unpremultiply(p: Pre) -> [u8; 4] {
    let a255 = p[3].clamp(0, 255 * 255);
    let a = (a255 + 127) / 255;
    if a == 0 {
        return [0, 0, 0, 0];
    }
    let mut out = [0u8; 4];
    for c in 0..3 {
        out[c] = ((p[c].max(0) * 255 + a255 / 2) / a255).clamp(0, 255) as u8;
    }
    out[3] = a as u8;
    out
}

/// Resample to `width` x `height`.
pub fn resize(src: &Canvas, width: u32, height: u32, filter: Resample) -> Canvas {
    let (width, height) = (
        width.clamp(1, crate::MAX_SIDE),
        height.clamp(1, crate::MAX_SIDE),
    );
    let (sw, sh) = (src.width(), src.height());
    if filter == Resample::Nearest {
        let mut out = Canvas::new(width, height);
        for y in 0..height {
            let sy = ((2 * u64::from(y) + 1) * u64::from(sh) / (2 * u64::from(height))) as i32;
            for x in 0..width {
                let sx = ((2 * u64::from(x) + 1) * u64::from(sw) / (2 * u64::from(width))) as i32;
                out.set(x as i32, y as i32, src.get(sx, sy));
            }
        }
        return out;
    }
    let pre = premultiply(src);
    // Horizontal pass into an intermediate of width x sh.
    let hk = kernel(sw, width, filter);
    let mut mid = vec![[0i64; 4]; width as usize * sh as usize];
    for y in 0..sh as usize {
        for (x, (lo, w)) in hk.iter().enumerate() {
            let mut acc = [0i64; 4];
            for (k, weight) in w.iter().enumerate() {
                let p = pre[y * sw as usize + lo + k];
                for c in 0..4 {
                    acc[c] += p[c] * weight;
                }
            }
            mid[y * width as usize + x] = acc.map(|v| (v + ONE / 2).div_euclid(ONE));
        }
    }
    let vk = kernel(sh, height, filter);
    let mut out = Canvas::new(width, height);
    for (y, (lo, w)) in vk.iter().enumerate() {
        for x in 0..width as usize {
            let mut acc = [0i64; 4];
            for (k, weight) in w.iter().enumerate() {
                let p = mid[(lo + k) * width as usize + x];
                for c in 0..4 {
                    acc[c] += p[c] * weight;
                }
            }
            let p = acc.map(|v| (v + ONE / 2).div_euclid(ONE));
            out.set(x as i32, y as i32, unpremultiply(p));
        }
    }
    out
}

/// `r` of the canvas; parts outside it become transparent.
pub fn crop(src: &Canvas, r: IRect) -> Canvas {
    let mut out = Canvas::new(r.w.max(1), r.h.max(1));
    if let Some(inside) = src.region(r) {
        out.put(
            (r.x.max(0) - r.x).max(0),
            (r.y.max(0) - r.y).max(0),
            &inside,
        );
    }
    out
}

/// Clockwise quarter turns.
pub fn rotate_quarter(src: &Canvas, turns: u32) -> Canvas {
    let (w, h) = (src.width() as i32, src.height() as i32);
    match turns % 4 {
        0 => src.clone(),
        2 => {
            let mut out = Canvas::new(w as u32, h as u32);
            for y in 0..h {
                for x in 0..w {
                    out.set(w - 1 - x, h - 1 - y, src.get(x, y));
                }
            }
            out
        }
        t => {
            let mut out = Canvas::new(h as u32, w as u32);
            for y in 0..h {
                for x in 0..w {
                    if t == 1 {
                        out.set(h - 1 - y, x, src.get(x, y));
                    } else {
                        out.set(y, w - 1 - x, src.get(x, y));
                    }
                }
            }
            out
        }
    }
}

pub fn flip(src: &Canvas, horizontal: bool) -> Canvas {
    let (w, h) = (src.width() as i32, src.height() as i32);
    let mut out = Canvas::new(w as u32, h as u32);
    for y in 0..h {
        for x in 0..w {
            let (sx, sy) = if horizontal {
                (w - 1 - x, y)
            } else {
                (x, h - 1 - y)
            };
            out.set(x, y, src.get(sx, sy));
        }
    }
    out
}

const FIX: i64 = 1 << 16;

/// Bilinear sample at a 16.16 fixed-point position, transparent outside.
fn sample(src: &Canvas, pre: &[Pre], fx: i64, fy: i64, clamp: bool) -> Pre {
    // Pixel centres sit at .5; shift so integer parts index the top-left tap.
    let (fx, fy) = (fx - FIX / 2, fy - FIX / 2);
    let (x0, y0) = (fx.div_euclid(FIX), fy.div_euclid(FIX));
    let (tx, ty) = (fx.rem_euclid(FIX) >> 8, fy.rem_euclid(FIX) >> 8); // 0..=255
    let (w, h) = (i64::from(src.width()), i64::from(src.height()));
    let tap = |x: i64, y: i64| -> Pre {
        let (x, y) = if clamp {
            (x.clamp(0, w - 1), y.clamp(0, h - 1))
        } else {
            (x, y)
        };
        if x < 0 || y < 0 || x >= w || y >= h {
            [0; 4]
        } else {
            pre[(y * w + x) as usize]
        }
    };
    let (a, b, c, d) = (
        tap(x0, y0),
        tap(x0 + 1, y0),
        tap(x0, y0 + 1),
        tap(x0 + 1, y0 + 1),
    );
    let mut out = [0i64; 4];
    for k in 0..4 {
        let top = a[k] * (256 - tx) + b[k] * tx;
        let bottom = c[k] * (256 - tx) + d[k] * tx;
        out[k] = (top * (256 - ty) + bottom * ty + 32768) >> 16;
    }
    out
}

/// Inverse-map every output pixel through `m` (2x2, 16.16) about the two centres.
fn warp(src: &Canvas, width: u32, height: u32, m: [i64; 4], clamp: bool) -> Canvas {
    let pre = premultiply(src);
    let mut out = Canvas::new(width, height);
    let (ocx, ocy) = (i64::from(width) * FIX / 2, i64::from(height) * FIX / 2);
    let (scx, scy) = (
        i64::from(src.width()) * FIX / 2,
        i64::from(src.height()) * FIX / 2,
    );
    for y in 0..height as i64 {
        let py = y * FIX + FIX / 2 - ocy;
        for x in 0..width as i64 {
            let px = x * FIX + FIX / 2 - ocx;
            let sx = ((m[0] * px + m[1] * py) >> 16) + scx;
            let sy = ((m[2] * px + m[3] * py) >> 16) + scy;
            out.set(
                x as i32,
                y as i32,
                unpremultiply(sample(src, &pre, sx, sy, clamp)),
            );
        }
    }
    out
}

fn angle(centidegrees: i32) -> (f64, f64) {
    let rad = f64::from(centidegrees) / 100.0 * fmath::PI / 180.0;
    (fmath::cos(rad), fmath::sin(rad))
}
fn fix(v: f64) -> i64 {
    fmath::round(v * FIX as f64) as i64
}

/// Rotate clockwise by `centidegrees` hundredths of a degree. `expand` grows the canvas
/// to hold the whole rotated image; otherwise corners are clipped. Uncovered pixels
/// are transparent.
pub fn rotate(src: &Canvas, centidegrees: i32, expand: bool) -> Canvas {
    if centidegrees.rem_euclid(9000) == 0 {
        let turns = (centidegrees.rem_euclid(36000) / 9000) as u32;
        let turned = rotate_quarter(src, turns);
        return if expand {
            turned
        } else {
            let (w, h) = (src.width(), src.height());
            let r = IRect::new(
                (turned.width() as i32 - w as i32) / 2,
                (turned.height() as i32 - h as i32) / 2,
                w,
                h,
            );
            crop(&turned, r)
        };
    }
    let (c, s) = angle(centidegrees);
    let (w, h) = (f64::from(src.width()), f64::from(src.height()));
    let (width, height) = if expand {
        let (ac, as_) = (c.abs(), s.abs());
        (
            (w * ac + h * as_ - 1e-9).ceil().max(1.0) as u32,
            (w * as_ + h * ac - 1e-9).ceil().max(1.0) as u32,
        )
    } else {
        (src.width(), src.height())
    };
    // Screen y points down, so a clockwise turn is the usual matrix; invert it.
    warp(src, width, height, [fix(c), fix(s), fix(-s), fix(c)], false)
}

/// Straighten as a phone photo editor does: rotate by `centidegrees` and zoom just
/// enough that no empty corner shows, keeping the frame the same size.
pub fn straighten(src: &Canvas, centidegrees: i32) -> Canvas {
    if centidegrees == 0 {
        return src.clone();
    }
    let (c, s) = angle(centidegrees);
    let (w, h) = (f64::from(src.width()), f64::from(src.height()));
    let (ac, as_) = (c.abs(), s.abs());
    let zoom = ((w * ac + h * as_) / w).max((w * as_ + h * ac) / h);
    warp(
        src,
        src.width(),
        src.height(),
        [fix(c / zoom), fix(s / zoom), fix(-s / zoom), fix(c / zoom)],
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BLACK, TRANSPARENT, WHITE};
    fn gradient(w: u32, h: u32) -> Canvas {
        let mut c = Canvas::new(w, h);
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                c.set(x, y, [(x * 20) as u8, (y * 20) as u8, 100, 255]);
            }
        }
        c
    }
    #[test]
    fn quarter_turns_and_flips_are_exact_permutations() {
        let c = gradient(3, 2);
        let r = rotate_quarter(&c, 1);
        assert_eq!((r.width(), r.height()), (2, 3));
        // The bottom-left corner moves to the top-left under a clockwise turn.
        assert_eq!(r.get(0, 0), c.get(0, 1));
        assert_eq!(r.get(1, 0), c.get(0, 0));
        assert_eq!(rotate_quarter(&rotate_quarter(&c, 1), 3), c);
        assert_eq!(rotate_quarter(&c, 2).get(0, 0), c.get(2, 1));
        assert_eq!(flip(&c, true).get(0, 0), c.get(2, 0));
        assert_eq!(flip(&c, false).get(0, 0), c.get(0, 1));
        assert_eq!(flip(&flip(&c, true), true), c);
        assert_eq!(rotate(&c, 9000, true), r);
    }
    #[test]
    fn resampling_keeps_flat_colour_and_scales_exactly() {
        let flat = Canvas::filled(7, 5, [40, 90, 200, 255]);
        for filter in Resample::ALL {
            let big = resize(&flat, 13, 11, filter);
            assert_eq!((big.width(), big.height()), (13, 11));
            assert!(
                big.pixels().chunks(4).all(|p| p == [40, 90, 200, 255]),
                "{filter:?} changed a flat colour"
            );
            let small = resize(&flat, 2, 2, filter);
            assert!(small.pixels().chunks(4).all(|p| p == [40, 90, 200, 255]));
        }
        // Nearest doubling repeats each pixel.
        let c = gradient(2, 2);
        let d = resize(&c, 4, 4, Resample::Nearest);
        assert_eq!(d.get(1, 1), c.get(0, 0));
        assert_eq!(d.get(2, 1), c.get(1, 0));
        // Bilinear halving averages each 2x2 block.
        let mut checker = Canvas::new(8, 8);
        for y in 0..8 {
            for x in 0..8 {
                checker.set(x, y, if (x + y) % 2 == 0 { BLACK } else { WHITE });
            }
        }
        // Away from the edges, halving a checkerboard gives an even grey.
        let half = resize(&checker, 4, 4, Resample::Bilinear);
        assert!(half.get(1, 1)[0].abs_diff(128) <= 1, "{:?}", half.get(1, 1));
        // Transparent neighbours do not darken an edge: premultiplied resampling.
        let mut edge = Canvas::new(2, 1);
        edge.set(0, 0, [255, 0, 0, 255]);
        let wide = resize(&edge, 4, 1, Resample::Bilinear);
        assert_eq!(&wide.get(1, 0)[..3], &[255, 0, 0]);
        assert!(wide.get(2, 0)[3] < 255 && wide.get(2, 0)[3] > 0);
    }
    #[test]
    fn crop_and_free_rotation_behave() {
        let c = gradient(4, 4);
        let k = crop(&c, IRect::new(1, 1, 2, 2));
        assert_eq!(k.get(0, 0), c.get(1, 1));
        let out = crop(&c, IRect::new(-1, -1, 2, 2));
        assert_eq!(out.get(0, 0), TRANSPARENT);
        assert_eq!(out.get(1, 1), c.get(0, 0));
        // A half turn by free rotation equals the exact one.
        assert_eq!(rotate(&c, 18000, false), rotate_quarter(&c, 2));
        let square = Canvas::filled(20, 20, WHITE);
        let turned = rotate(&square, 4500, true);
        assert_eq!((turned.width(), turned.height()), (29, 29));
        assert_eq!(turned.get(14, 14), WHITE);
        assert_eq!(turned.get(0, 0), TRANSPARENT, "the corners are uncovered");
        // Straightening zooms so no corner is empty.
        let s = straighten(&square, 1000);
        assert_eq!((s.width(), s.height()), (20, 20));
        assert!(s.pixels().chunks(4).all(|p| p[3] == 255));
    }
}
