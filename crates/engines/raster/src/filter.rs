//! Neighbourhood filters: blurs, sharpening, edges, emboss, median noise reduction,
//! vignette and pixelation. Kernels are fixed-point integers; smoothing works on
//! premultiplied colour so transparency never bleeds a dark fringe. Edges clamp.
use crate::adjust::luma;
use crate::blend::mix;
use crate::fmath::{self, isqrt};
use crate::{Canvas, IRect, Mask, Rgba};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Filter {
    BoxBlur {
        radius: u32,
    },
    GaussianBlur {
        radius: u32,
    },
    /// 0..=100: strength of a 3x3 Laplacian sharpen.
    Sharpen {
        amount: u32,
    },
    UnsharpMask {
        radius: u32,
        amount: u32,
        threshold: u8,
    },
    /// Sobel gradient magnitude per channel.
    EdgeDetect,
    Emboss,
    /// Noise reduction: the median of each channel over a square window.
    Median {
        radius: u32,
    },
    /// -100..=100: darken (positive) or lighten (negative) toward the corners.
    Vignette {
        amount: i32,
    },
    Pixelate {
        size: u32,
    },
}

pub const MAX_RADIUS: u32 = 100;
const ONE: i64 = 1 << 14;

impl Filter {
    /// How far a filter reads beyond the pixel it writes.
    pub fn reach(&self) -> u32 {
        match self {
            Self::BoxBlur { radius } | Self::GaussianBlur { radius } => (*radius).min(MAX_RADIUS),
            Self::UnsharpMask { radius, .. } => (*radius).min(MAX_RADIUS),
            Self::Median { radius } => (*radius).min(10),
            Self::Sharpen { .. } | Self::EdgeDetect | Self::Emboss => 1,
            Self::Vignette { .. } | Self::Pixelate { .. } => 0,
        }
    }
}

type Pre = [i64; 4];
fn pre(p: Rgba) -> Pre {
    let a = i64::from(p[3]);
    [
        i64::from(p[0]) * a,
        i64::from(p[1]) * a,
        i64::from(p[2]) * a,
        a * 255,
    ]
}
fn unpre(p: Pre) -> Rgba {
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

fn normalise(raw: Vec<f64>) -> Vec<i64> {
    let total: f64 = raw.iter().sum();
    let mut w: Vec<i64> = raw
        .iter()
        .map(|v| fmath::round(v / total * ONE as f64) as i64)
        .collect();
    let residue = ONE - w.iter().sum::<i64>();
    let mid = w.len() / 2;
    w[mid] += residue;
    w
}
fn box_kernel(radius: u32) -> Vec<i64> {
    normalise(vec![1.0; 2 * radius as usize + 1])
}
/// Gaussian with sigma = radius / 2, truncated at the radius.
fn gaussian_kernel(radius: u32) -> Vec<i64> {
    let sigma = (f64::from(radius) / 2.0).max(0.5);
    let r = radius as i64;
    normalise(
        (-r..=r)
            .map(|x| fmath::exp(-((x * x) as f64) / (2.0 * sigma * sigma)))
            .collect(),
    )
}

/// Separable convolution of a whole canvas with a symmetric kernel.
fn convolve(src: &Canvas, kernel: &[i64]) -> Canvas {
    let (w, h) = (src.width() as i32, src.height() as i32);
    let r = (kernel.len() / 2) as i32;
    let p: Vec<Pre> = src
        .pixels()
        .chunks(4)
        .map(|c| pre([c[0], c[1], c[2], c[3]]))
        .collect();
    let at = |x: i32, y: i32| (y.clamp(0, h - 1) * w + x.clamp(0, w - 1)) as usize;
    let mut mid = vec![[0i64; 4]; p.len()];
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0i64; 4];
            for (k, wt) in kernel.iter().enumerate() {
                let q = p[at(x + k as i32 - r, y)];
                for c in 0..4 {
                    acc[c] += q[c] * wt;
                }
            }
            mid[(y * w + x) as usize] = acc.map(|v| (v + ONE / 2).div_euclid(ONE));
        }
    }
    let mut out = Canvas::new(w as u32, h as u32);
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0i64; 4];
            for (k, wt) in kernel.iter().enumerate() {
                let q = mid[at(x, y + k as i32 - r)];
                for c in 0..4 {
                    acc[c] += q[c] * wt;
                }
            }
            out.set(x, y, unpre(acc.map(|v| (v + ONE / 2).div_euclid(ONE))));
        }
    }
    out
}

fn map3x3(src: &Canvas, f: impl Fn(&dyn Fn(i32, i32) -> Rgba) -> Rgba) -> Canvas {
    let (w, h) = (src.width() as i32, src.height() as i32);
    let mut out = Canvas::new(w as u32, h as u32);
    for y in 0..h {
        for x in 0..w {
            let at = |dx: i32, dy: i32| src.get_clamped(x + dx, y + dy);
            out.set(x, y, f(&at));
        }
    }
    out
}

/// Run `filter` over a whole canvas.
pub fn filtered(src: &Canvas, filter: &Filter) -> Canvas {
    match filter {
        Filter::BoxBlur { radius } => {
            let r = (*radius).min(MAX_RADIUS);
            if r == 0 {
                return src.clone();
            }
            convolve(src, &box_kernel(r))
        }
        Filter::GaussianBlur { radius } => {
            let r = (*radius).min(MAX_RADIUS);
            if r == 0 {
                return src.clone();
            }
            convolve(src, &gaussian_kernel(r))
        }
        Filter::Sharpen { amount } => {
            let a = i64::from((*amount).min(100));
            map3x3(src, |at| {
                let c = at(0, 0);
                let mut out = c;
                for k in 0..3 {
                    let n = i64::from(at(0, -1)[k])
                        + i64::from(at(0, 1)[k])
                        + i64::from(at(-1, 0)[k])
                        + i64::from(at(1, 0)[k]);
                    let v = i64::from(c[k]) * (100 + 4 * a) - a * n;
                    out[k] = ((v + 50).div_euclid(100)).clamp(0, 255) as u8;
                }
                out
            })
        }
        Filter::UnsharpMask {
            radius,
            amount,
            threshold,
        } => {
            let r = (*radius).clamp(1, MAX_RADIUS);
            let blurred = convolve(src, &gaussian_kernel(r));
            let amount = i64::from((*amount).min(500));
            let mut out = src.clone();
            for y in 0..src.height() as i32 {
                for x in 0..src.width() as i32 {
                    let (o, b) = (src.get(x, y), blurred.get(x, y));
                    let mut q = o;
                    for c in 0..3 {
                        let diff = i64::from(o[c]) - i64::from(b[c]);
                        if diff.unsigned_abs() >= u64::from(*threshold) {
                            q[c] = (i64::from(o[c]) + (diff * amount + 50).div_euclid(100))
                                .clamp(0, 255) as u8;
                        }
                    }
                    out.set(x, y, q);
                }
            }
            out
        }
        Filter::EdgeDetect => map3x3(src, |at| {
            let c = at(0, 0);
            let mut out = c;
            for k in 0..3 {
                let v = |dx, dy| i64::from(at(dx, dy)[k]);
                let gx = v(1, -1) + 2 * v(1, 0) + v(1, 1) - v(-1, -1) - 2 * v(-1, 0) - v(-1, 1);
                let gy = v(-1, 1) + 2 * v(0, 1) + v(1, 1) - v(-1, -1) - 2 * v(0, -1) - v(1, -1);
                out[k] = isqrt((gx * gx + gy * gy) as u64).min(255) as u8;
            }
            out
        }),
        Filter::Emboss => map3x3(src, |at| {
            let l = |dx, dy| i64::from(luma(at(dx, dy)));
            let e = 128 - l(-1, -1) - l(0, -1) - l(-1, 0) + l(1, 0) + l(0, 1) + l(1, 1);
            let v = e.clamp(0, 255) as u8;
            [v, v, v, at(0, 0)[3]]
        }),
        Filter::Median { radius } => {
            let r = (*radius).clamp(1, 10) as i32;
            let (w, h) = (src.width() as i32, src.height() as i32);
            let mut out = Canvas::new(w as u32, h as u32);
            let mut window: [Vec<u8>; 3] = Default::default();
            for y in 0..h {
                for x in 0..w {
                    for ch in &mut window {
                        ch.clear();
                    }
                    for dy in -r..=r {
                        for dx in -r..=r {
                            let p = src.get_clamped(x + dx, y + dy);
                            for c in 0..3 {
                                window[c].push(p[c]);
                            }
                        }
                    }
                    let mut q = src.get(x, y);
                    for c in 0..3 {
                        window[c].sort_unstable();
                        q[c] = window[c][window[c].len() / 2];
                    }
                    out.set(x, y, q);
                }
            }
            out
        }
        Filter::Vignette { amount } => {
            let a = i64::from((*amount).clamp(-100, 100));
            let (w, h) = (i64::from(src.width()), i64::from(src.height()));
            let mut out = src.clone();
            for y in 0..h {
                let ny = (2 * y + 1 - h) * 1024 / h;
                for x in 0..w {
                    let nx = (2 * x + 1 - w) * 1024 / w;
                    // 0 at the centre, 1024 in the corners, eased so the middle stays clean.
                    let t = (nx * nx + ny * ny) / 2048;
                    let weight = t * t / 1024;
                    let p = src.get(x as i32, y as i32);
                    let mut q = p;
                    for c in 0..3 {
                        let v = i64::from(p[c]);
                        q[c] = if a >= 0 {
                            v * (102_400 - a * weight) / 102_400
                        } else {
                            v + (255 - v) * (-a) * weight / 102_400
                        }
                        .clamp(0, 255) as u8;
                    }
                    out.set(x as i32, y as i32, q);
                }
            }
            out
        }
        Filter::Pixelate { size } => {
            let s = (*size).clamp(1, 256) as i32;
            let (w, h) = (src.width() as i32, src.height() as i32);
            let mut out = Canvas::new(w as u32, h as u32);
            for by in (0..h).step_by(s as usize) {
                for bx in (0..w).step_by(s as usize) {
                    let mut acc = [0i64; 4];
                    let mut n = 0;
                    for y in by..(by + s).min(h) {
                        for x in bx..(bx + s).min(w) {
                            let q = pre(src.get(x, y));
                            for c in 0..4 {
                                acc[c] += q[c];
                            }
                            n += 1;
                        }
                    }
                    let avg = unpre(acc.map(|v| (v + n / 2) / n));
                    for y in by..(by + s).min(h) {
                        for x in bx..(bx + s).min(w) {
                            out.set(x, y, avg);
                        }
                    }
                }
            }
            out
        }
    }
}

/// Apply `filter` to `layer` within `selection`, returning the area changed.
pub fn apply(layer: &mut Canvas, filter: &Filter, selection: Option<&Mask>) -> Option<IRect> {
    let area = match selection {
        Some(sel) => sel.bounds()?,
        None => layer.bounds(),
    };
    // Position-dependent filters see the whole image; the rest only a margin around it.
    let source = match filter {
        Filter::Vignette { .. } | Filter::Pixelate { .. } => layer.bounds(),
        _ => area
            .grow(filter.reach())
            .clip(layer.width(), layer.height())?,
    };
    let input = layer.region(source)?;
    let output = filtered(&input, filter);
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            let q = output.get(x - source.x, y - source.y);
            let out = match selection {
                Some(sel) => mix(layer.get(x, y), q, u32::from(sel.get(x, y))),
                None => q,
            };
            layer.set(x, y, out);
        }
    }
    Some(area)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BLACK, WHITE};
    fn dot() -> Canvas {
        let mut c = Canvas::filled(5, 5, BLACK);
        c.set(2, 2, WHITE);
        c
    }
    #[test]
    fn blurs_spread_a_dot_and_conserve_flat_colour() {
        let b = filtered(&dot(), &Filter::BoxBlur { radius: 1 });
        // 255 / 9 = 28.3 over the 3x3 neighbourhood.
        assert_eq!(b.get(2, 2), [28, 28, 28, 255]);
        assert_eq!(b.get(1, 1), [28, 28, 28, 255]);
        assert_eq!(b.get(0, 0), BLACK);
        let g = filtered(&dot(), &Filter::GaussianBlur { radius: 2 });
        assert!(g.get(2, 2)[0] > g.get(1, 2)[0] && g.get(1, 2)[0] > g.get(0, 2)[0]);
        assert_eq!(g.get(2, 1), g.get(1, 2), "the kernel is symmetric");
        let flat = Canvas::filled(6, 6, [90, 30, 200, 255]);
        for f in [
            Filter::BoxBlur { radius: 3 },
            Filter::GaussianBlur { radius: 4 },
            Filter::Sharpen { amount: 80 },
            Filter::Median { radius: 2 },
            Filter::UnsharpMask {
                radius: 2,
                amount: 150,
                threshold: 0,
            },
            Filter::Pixelate { size: 4 },
        ] {
            assert_eq!(filtered(&flat, &f), flat, "{f:?} changed a flat image");
        }
    }
    #[test]
    fn edges_emboss_sharpen_and_median_are_exact() {
        let e = filtered(&dot(), &Filter::EdgeDetect);
        assert_eq!(e.get(2, 2), BLACK, "no gradient at the dot's centre");
        // Directly left of the dot: gx = 2*255, gy = 0.
        assert_eq!(e.get(1, 2), [255, 255, 255, 255]);
        assert_eq!(e.get(0, 0), BLACK);
        let m = filtered(&dot(), &Filter::Emboss);
        assert_eq!(m.get(0, 0), [128, 128, 128, 255]);
        // Above-left of the dot sees it at +1,+1: 128 + 255 clipped.
        assert_eq!(m.get(1, 1), [255, 255, 255, 255]);
        assert_eq!(m.get(3, 3), [0, 0, 0, 255]);
        let s = filtered(&dot(), &Filter::Sharpen { amount: 25 });
        assert_eq!(s.get(2, 2), WHITE);
        // 0*200 - 25*255 = -6375 / 100 clipped to 0.
        assert_eq!(s.get(1, 2), BLACK);
        let n = filtered(&dot(), &Filter::Median { radius: 1 });
        assert_eq!(n.get(2, 2), BLACK, "a lone speck is removed");
    }
    #[test]
    fn vignette_darkens_corners_and_spares_the_centre() {
        let c = Canvas::filled(21, 21, [200, 200, 200, 255]);
        let v = filtered(&c, &Filter::Vignette { amount: 100 });
        assert_eq!(v.get(10, 10), [200, 200, 200, 255]);
        assert_eq!(v.get(0, 0), [35, 35, 35, 255]);
        let l = filtered(&c, &Filter::Vignette { amount: -100 });
        assert!(l.get(0, 0)[0] > 240);
        let p = {
            let mut p = Canvas::new(4, 1);
            p.set(0, 0, [0, 0, 0, 255]);
            p.set(1, 0, [100, 100, 100, 255]);
            p
        };
        let px = filtered(&p, &Filter::Pixelate { size: 2 });
        assert_eq!(px.get(0, 0), [50, 50, 50, 255]);
        assert_eq!(px.get(1, 0), px.get(0, 0));
    }
    #[test]
    fn a_selection_confines_the_filter() {
        let mut c = dot();
        let sel = Mask::rect(5, 5, IRect::new(0, 0, 2, 5));
        apply(&mut c, &Filter::BoxBlur { radius: 1 }, Some(&sel));
        assert_eq!(c.get(1, 2), [28, 28, 28, 255], "inside, next to the dot");
        assert_eq!(c.get(2, 2), WHITE, "outside the selection");
        assert_eq!(c.get(3, 2), BLACK);
    }
}
