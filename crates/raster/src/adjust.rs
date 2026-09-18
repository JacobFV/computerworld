//! Colour adjustments: per-pixel transforms applied to a layer through the selection.
//! Tone curves are built once as 256-entry tables; the colour-space ones (hue,
//! saturation, vibrance) use only correctly rounded arithmetic, so results are the same
//! bytes on every target.
use crate::blend::mix;
use crate::fmath::{powf, to_u8};
use crate::{Canvas, IRect, Mask, Rgba};
use serde::{Deserialize, Serialize};

/// Which channels a tone adjustment reads and writes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    /// All three colour channels alike.
    #[default]
    Value,
    Red,
    Green,
    Blue,
}
impl Channel {
    pub const ALL: [Channel; 4] = [Self::Value, Self::Red, Self::Green, Self::Blue];
    pub fn id(self) -> &'static str {
        match self {
            Self::Value => "value",
            Self::Red => "red",
            Self::Green => "green",
            Self::Blue => "blue",
        }
    }
    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.id() == id)
    }
    fn covers(self, c: usize) -> bool {
        match self {
            Self::Value => true,
            Self::Red => c == 0,
            Self::Green => c == 1,
            Self::Blue => c == 2,
        }
    }
}

/// Tonal range a colour balance acts on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tone {
    Shadows,
    #[default]
    Midtones,
    Highlights,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Adjustment {
    /// Both -100..=100. Brightness moves toward white or black without clipping;
    /// contrast scales about mid-grey (100 is a hard threshold, -100 flat grey).
    BrightnessContrast {
        brightness: i32,
        contrast: i32,
    },
    /// Hundredths of a stop, applied in linear light.
    Exposure {
        stops: i32,
    },
    Levels {
        channel: Channel,
        in_black: u8,
        in_white: u8,
        /// Hundredths: 100 is linear, above brightens midtones.
        gamma: u32,
        out_black: u8,
        out_white: u8,
    },
    /// A monotone cubic through the control points (input, output).
    Curves {
        channel: Channel,
        points: Vec<(u8, u8)>,
    },
    /// Hue in degrees (-180..=180); saturation and lightness -100..=100.
    HueSaturation {
        hue: i32,
        saturation: i32,
        lightness: i32,
    },
    /// Each -100..=100, acting on one tonal range.
    ColorBalance {
        tone: Tone,
        cyan_red: i32,
        magenta_green: i32,
        yellow_blue: i32,
    },
    /// Warm/cool and green/magenta, each -100..=100.
    Temperature {
        temperature: i32,
        tint: i32,
    },
    Invert,
    /// Rec. 709 luminosity.
    Grayscale,
    /// White where luminosity is within `low..=high`, black elsewhere.
    Threshold {
        low: u8,
        high: u8,
    },
    /// Levels per channel, 2..=255.
    Posterize {
        levels: u8,
    },
    /// -100..=100: saturation that spares already-saturated colour.
    Vibrance {
        amount: i32,
    },
    /// -100..=100 each: lift the dark tones, recover the bright ones.
    ShadowsHighlights {
        shadows: i32,
        highlights: i32,
    },
    /// 0..=100 of a warm brown monochrome.
    Sepia {
        amount: i32,
    },
    /// Stretch each channel to the full range, clipping 0.5% at each end.
    AutoLevels,
}

/// sRGB transfer, with our own `powf`.
fn to_linear(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        powf((v + 0.055) / 1.055, 2.4)
    }
}
fn from_linear(v: f64) -> f64 {
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * powf(v, 1.0 / 2.4) - 0.055
    }
}

fn clamp01(v: f64) -> f64 {
    v.clamp(0.0, 1.0)
}

/// Luminosity 0..=255 by Rec. 709 weights (54 + 183 + 19 = 256).
#[inline]
pub fn luma(p: Rgba) -> u8 {
    ((54 * u32::from(p[0]) + 183 * u32::from(p[1]) + 19 * u32::from(p[2]) + 128) >> 8) as u8
}

fn rgb_to_hsl(p: Rgba) -> (f64, f64, f64) {
    let (r, g, b) = (
        f64::from(p[0]) / 255.0,
        f64::from(p[1]) / 255.0,
        f64::from(p[2]) / 255.0,
    );
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if max == min {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h * 60.0, s, l)
}
fn hsl_to_rgb(h: f64, s: f64, l: f64, a: u8) -> Rgba {
    if s <= 0.0 {
        let v = to_u8(l * 255.0);
        return [v, v, v, a];
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let hue = |mut t: f64| {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    let h = h / 360.0;
    [
        to_u8(hue(h + 1.0 / 3.0) * 255.0),
        to_u8(hue(h) * 255.0),
        to_u8(hue(h - 1.0 / 3.0) * 255.0),
        a,
    ]
}

/// Monotone cubic (Fritsch-Carlson) through `points`, as a 256-entry table.
pub fn curve_table(points: &[(u8, u8)]) -> [u8; 256] {
    let mut pts: Vec<(f64, f64)> = vec![];
    let mut sorted = points.to_vec();
    sorted.sort_by_key(|p| p.0);
    sorted.dedup_by_key(|p| p.0);
    for (x, y) in &sorted {
        pts.push((f64::from(*x), f64::from(*y)));
    }
    let mut table = [0u8; 256];
    if pts.is_empty() {
        for (i, v) in table.iter_mut().enumerate() {
            *v = i as u8;
        }
        return table;
    }
    if pts.len() == 1 {
        return [to_u8(pts[0].1); 256];
    }
    let n = pts.len();
    let slopes: Vec<f64> = (0..n - 1)
        .map(|i| (pts[i + 1].1 - pts[i].1) / (pts[i + 1].0 - pts[i].0))
        .collect();
    let mut m = vec![0.0; n];
    m[0] = slopes[0];
    m[n - 1] = slopes[n - 2];
    for i in 1..n - 1 {
        m[i] = if slopes[i - 1] * slopes[i] <= 0.0 {
            0.0
        } else {
            (slopes[i - 1] + slopes[i]) / 2.0
        };
    }
    for i in 0..n - 1 {
        if slopes[i] == 0.0 {
            m[i] = 0.0;
            m[i + 1] = 0.0;
            continue;
        }
        let (a, b) = (m[i] / slopes[i], m[i + 1] / slopes[i]);
        let s = a * a + b * b;
        if s > 9.0 {
            let t = 3.0 / fmath_sqrt(s);
            m[i] = t * a * slopes[i];
            m[i + 1] = t * b * slopes[i];
        }
    }
    for (i, v) in table.iter_mut().enumerate() {
        let x = i as f64;
        *v = if x <= pts[0].0 {
            to_u8(pts[0].1)
        } else if x >= pts[n - 1].0 {
            to_u8(pts[n - 1].1)
        } else {
            let k = (0..n - 1).find(|k| x <= pts[k + 1].0).unwrap_or(n - 2);
            let (x0, y0, x1, y1) = (pts[k].0, pts[k].1, pts[k + 1].0, pts[k + 1].1);
            let h = x1 - x0;
            let t = (x - x0) / h;
            let (t2, t3) = (t * t, t * t * t);
            to_u8(
                (2.0 * t3 - 3.0 * t2 + 1.0) * y0
                    + (t3 - 2.0 * t2 + t) * h * m[k]
                    + (-2.0 * t3 + 3.0 * t2) * y1
                    + (t3 - t2) * h * m[k + 1],
            )
        };
    }
    table
}
/// IEEE square root is correctly rounded on every target.
fn fmath_sqrt(v: f64) -> f64 {
    v.sqrt()
}

/// Per-channel histograms: red, green, blue, luminosity. Transparent pixels are skipped.
pub fn histogram(c: &Canvas) -> [[u32; 256]; 4] {
    let mut h = [[0u32; 256]; 4];
    for p in c.pixels().chunks(4) {
        if p[3] == 0 {
            continue;
        }
        let px = [p[0], p[1], p[2], p[3]];
        h[0][p[0] as usize] += 1;
        h[1][p[1] as usize] += 1;
        h[2][p[2] as usize] += 1;
        h[3][luma(px) as usize] += 1;
    }
    h
}

#[allow(clippy::large_enum_variant)]
enum Prepared {
    /// One table per colour channel.
    Tables([[u8; 256]; 3]),
    Pixel(Box<dyn Fn(Rgba) -> Rgba>),
}
impl Prepared {
    fn apply(&self, p: Rgba) -> Rgba {
        match self {
            Self::Tables(t) => [
                t[0][p[0] as usize],
                t[1][p[1] as usize],
                t[2][p[2] as usize],
                p[3],
            ],
            Self::Pixel(f) => f(p),
        }
    }
}

fn tables(channel: Channel, f: impl Fn(f64) -> f64) -> Prepared {
    let table: [u8; 256] = std::array::from_fn(|i| to_u8(f(i as f64 / 255.0) * 255.0));
    let identity: [u8; 256] = std::array::from_fn(|i| i as u8);
    Prepared::Tables(std::array::from_fn(|c| {
        if channel.covers(c) {
            table
        } else {
            identity
        }
    }))
}

impl Adjustment {
    /// Build the per-pixel function. `source` is what `AutoLevels` measures.
    fn prepare(&self, source: &Canvas, area: IRect) -> Prepared {
        match self {
            Self::BrightnessContrast {
                brightness,
                contrast,
            } => {
                let b = f64::from((*brightness).clamp(-100, 100)) / 100.0;
                let c = (*contrast).clamp(-100, 100);
                let factor = if c >= 0 {
                    100.0 / f64::from(100 - c.min(99))
                } else {
                    f64::from(100 + c) / 100.0
                };
                tables(Channel::Value, |v| {
                    let v = if b >= 0.0 {
                        v + (1.0 - v) * b
                    } else {
                        v * (1.0 + b)
                    };
                    (v - 0.5) * factor + 0.5
                })
            }
            Self::Exposure { stops } => {
                let gain = powf(2.0, f64::from((*stops).clamp(-1000, 1000)) / 100.0);
                tables(Channel::Value, |v| {
                    from_linear(clamp01(to_linear(v) * gain))
                })
            }
            Self::Levels {
                channel,
                in_black,
                in_white,
                gamma,
                out_black,
                out_white,
            } => {
                let (ib, iw) = (
                    f64::from(*in_black),
                    f64::from((*in_white).max(in_black.saturating_add(1))),
                );
                let g = f64::from((*gamma).clamp(10, 1000)) / 100.0;
                let (ob, ow) = (f64::from(*out_black), f64::from(*out_white));
                tables(*channel, |v| {
                    let t = clamp01((v * 255.0 - ib) / (iw - ib));
                    (ob + powf(t, 1.0 / g) * (ow - ob)) / 255.0
                })
            }
            Self::Curves { channel, points } => {
                let table = curve_table(points);
                let identity: [u8; 256] = std::array::from_fn(|i| i as u8);
                Prepared::Tables(std::array::from_fn(|c| {
                    if channel.covers(c) {
                        table
                    } else {
                        identity
                    }
                }))
            }
            Self::HueSaturation {
                hue,
                saturation,
                lightness,
            } => {
                let (h, s, l) = (
                    f64::from((*hue).clamp(-180, 180)),
                    f64::from((*saturation).clamp(-100, 100)) / 100.0,
                    f64::from((*lightness).clamp(-100, 100)) / 100.0,
                );
                Prepared::Pixel(Box::new(move |p| {
                    let (ph, ps, pl) = rgb_to_hsl(p);
                    let mut hh = ph + h;
                    if hh < 0.0 {
                        hh += 360.0;
                    }
                    if hh >= 360.0 {
                        hh -= 360.0;
                    }
                    let out = hsl_to_rgb(hh, clamp01(ps * (1.0 + s)), pl, p[3]);
                    if l == 0.0 {
                        return out;
                    }
                    let target = if l > 0.0 { 255.0 } else { 0.0 };
                    let k = l.abs();
                    [
                        to_u8(f64::from(out[0]) + (target - f64::from(out[0])) * k),
                        to_u8(f64::from(out[1]) + (target - f64::from(out[1])) * k),
                        to_u8(f64::from(out[2]) + (target - f64::from(out[2])) * k),
                        p[3],
                    ]
                }))
            }
            Self::ColorBalance {
                tone,
                cyan_red,
                magenta_green,
                yellow_blue,
            } => {
                let amounts = [*cyan_red, *magenta_green, *yellow_blue]
                    .map(|v| f64::from(v.clamp(-100, 100)) / 100.0);
                let tone = *tone;
                Prepared::Pixel(Box::new(move |p| {
                    let l = f64::from(luma(p)) / 255.0;
                    // GIMP's range weights: each range fades out over a quarter of the scale.
                    let w = match tone {
                        Tone::Shadows => clamp01((0.333 - l) / 0.25 + 0.5) * 0.7,
                        Tone::Midtones => {
                            clamp01((l - 0.333) / 0.25 + 0.5)
                                * clamp01((l + 0.333 - 1.0) / -0.25 + 0.5)
                                * 0.7
                        }
                        Tone::Highlights => clamp01((l - 0.667) / 0.25 + 0.5) * 0.7,
                    };
                    let mut out = p;
                    for c in 0..3 {
                        out[c] = to_u8(f64::from(p[c]) + amounts[c] * w * 255.0 * 0.5);
                    }
                    out
                }))
            }
            Self::Temperature { temperature, tint } => {
                let t = f64::from((*temperature).clamp(-100, 100)) / 100.0;
                let g = f64::from((*tint).clamp(-100, 100)) / 100.0;
                let gains = [1.0 + 0.25 * t, 1.0 - 0.2 * g, 1.0 - 0.25 * t];
                Prepared::Tables(std::array::from_fn(|c| {
                    std::array::from_fn(|i| to_u8(i as f64 * gains[c]))
                }))
            }
            Self::Invert => Prepared::Tables([std::array::from_fn(|i| 255 - i as u8); 3]),
            Self::Grayscale => Prepared::Pixel(Box::new(|p| {
                let y = luma(p);
                [y, y, y, p[3]]
            })),
            Self::Threshold { low, high } => {
                let (low, high) = (*low, *high);
                Prepared::Pixel(Box::new(move |p| {
                    let y = luma(p);
                    let v = if y >= low && y <= high { 255 } else { 0 };
                    [v, v, v, p[3]]
                }))
            }
            Self::Posterize { levels } => {
                let n = u32::from((*levels).max(2)) - 1;
                Prepared::Tables(
                    [std::array::from_fn(|i| {
                        let q = (i as u32 * n + 127) / 255;
                        ((q * 255 + n / 2) / n) as u8
                    }); 3],
                )
            }
            Self::Vibrance { amount } => {
                let a = f64::from((*amount).clamp(-100, 100)) / 100.0;
                Prepared::Pixel(Box::new(move |p| {
                    let (h, s, l) = rgb_to_hsl(p);
                    let s = clamp01(s * (1.0 + a * (1.0 - s)));
                    hsl_to_rgb(h, s, l, p[3])
                }))
            }
            Self::ShadowsHighlights {
                shadows,
                highlights,
            } => {
                let s = f64::from((*shadows).clamp(-100, 100)) / 100.0;
                let h = f64::from((*highlights).clamp(-100, 100)) / 100.0;
                tables(Channel::Value, |v| {
                    v + s * 1.2 * v * (1.0 - v) * (1.0 - v) + h * 1.2 * v * v * (1.0 - v)
                })
            }
            Self::Sepia { amount } => {
                let k = u32::try_from((*amount).clamp(0, 100)).unwrap_or(0) * 255 / 100;
                Prepared::Pixel(Box::new(move |p| {
                    let (r, g, b) = (u32::from(p[0]), u32::from(p[1]), u32::from(p[2]));
                    let sepia = [
                        ((393 * r + 769 * g + 189 * b) / 1000).min(255) as u8,
                        ((349 * r + 686 * g + 168 * b) / 1000).min(255) as u8,
                        ((272 * r + 534 * g + 131 * b) / 1000).min(255) as u8,
                        p[3],
                    ];
                    mix(p, sepia, k)
                }))
            }
            Self::AutoLevels => {
                let region = source.region(area).unwrap_or_else(|| source.clone());
                let h = histogram(&region);
                let total: u32 = h[0].iter().sum();
                let clip = total / 200;
                let mut t = [[0u8; 256]; 3];
                for c in 0..3 {
                    let mut acc = 0;
                    let lo = (0..256)
                        .find(|i| {
                            acc += h[c][*i];
                            acc > clip
                        })
                        .unwrap_or(0) as u32;
                    acc = 0;
                    let hi = (0..256)
                        .rev()
                        .find(|i| {
                            acc += h[c][*i];
                            acc > clip
                        })
                        .unwrap_or(255) as u32;
                    for (i, v) in t[c].iter_mut().enumerate() {
                        *v = if hi <= lo {
                            i as u8
                        } else {
                            (((i as u32).clamp(lo, hi) - lo) * 255 / (hi - lo)) as u8
                        };
                    }
                }
                Prepared::Tables(t)
            }
        }
    }
}

/// Apply `adj` to `layer`, restricted to `selection`. Returns the area it changed.
pub fn apply(layer: &mut Canvas, adj: &Adjustment, selection: Option<&Mask>) -> Option<IRect> {
    let area = match selection {
        Some(sel) => sel.bounds()?,
        None => layer.bounds(),
    };
    let prepared = adj.prepare(layer, area);
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            let p = layer.get(x, y);
            let q = prepared.apply(p);
            let out = match selection {
                Some(sel) => mix(p, q, u32::from(sel.get(x, y))),
                None => q,
            };
            layer.set(x, y, out);
        }
    }
    Some(area)
}

/// Apply to a copy, whole canvas: for previews that must not touch the document.
pub fn applied(src: &Canvas, adj: &Adjustment) -> Canvas {
    let mut out = src.clone();
    apply(&mut out, adj, None);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn one(p: Rgba, adj: Adjustment) -> Rgba {
        let mut c = Canvas::filled(1, 1, p);
        apply(&mut c, &adj, None);
        c.get(0, 0)
    }
    #[test]
    fn simple_point_operations_are_exact() {
        let p = [200, 100, 50, 255];
        assert_eq!(one(p, Adjustment::Invert), [55, 155, 205, 255]);
        // (54*200 + 183*100 + 19*50 + 128) >> 8 = 30178 >> 8 = 117.
        assert_eq!(one(p, Adjustment::Grayscale), [117, 117, 117, 255]);
        assert_eq!(
            one(
                p,
                Adjustment::Threshold {
                    low: 117,
                    high: 255
                }
            ),
            [255, 255, 255, 255]
        );
        assert_eq!(
            one(
                p,
                Adjustment::Threshold {
                    low: 118,
                    high: 255
                }
            ),
            [0, 0, 0, 255]
        );
        // Four levels: 0, 85, 170, 255.
        assert_eq!(
            one(p, Adjustment::Posterize { levels: 4 }),
            [170, 85, 85, 255]
        );
        // Brightness +50: v + (1 - v)/2; 200 -> 227.4999 (in binary) -> 227.
        assert_eq!(
            one(
                p,
                Adjustment::BrightnessContrast {
                    brightness: 50,
                    contrast: 0
                }
            ),
            [227, 178, 153, 255]
        );
        // Contrast +50 doubles distance from mid-grey: 200 -> 272.5 clipped.
        assert_eq!(
            one(
                p,
                Adjustment::BrightnessContrast {
                    brightness: 0,
                    contrast: 50
                }
            ),
            [255, 73, 0, 255]
        );
        // Alpha is never touched.
        assert_eq!(one([10, 20, 30, 40], Adjustment::Invert)[3], 40);
    }
    #[test]
    fn tone_curves_levels_and_exposure() {
        let identity = Adjustment::Curves {
            channel: Channel::Value,
            points: vec![(0, 0), (255, 255)],
        };
        for v in [0u8, 1, 77, 128, 254, 255] {
            assert_eq!(one([v, v, v, 255], identity.clone()), [v, v, v, 255]);
        }
        let t = curve_table(&[(0, 0), (128, 192), (255, 255)]);
        assert_eq!(t[128], 192);
        assert!(t.windows(2).all(|w| w[0] <= w[1]), "monotone");
        // Levels: stretch 50..200 to the full range on red only.
        let levels = Adjustment::Levels {
            channel: Channel::Red,
            in_black: 50,
            in_white: 200,
            gamma: 100,
            out_black: 0,
            out_white: 255,
        };
        assert_eq!(one([125, 125, 125, 255], levels), [128, 125, 125, 255]);
        // One stop brighter in linear light: sRGB 128 (0.2159 linear) -> 0.4317 -> 175.55.
        assert_eq!(
            one([128, 128, 128, 255], Adjustment::Exposure { stops: 100 }),
            [176, 176, 176, 255]
        );
        assert_eq!(
            one([128, 0, 255, 255], Adjustment::Exposure { stops: 0 }),
            [128, 0, 255, 255]
        );
    }
    #[test]
    fn colour_space_adjustments() {
        // A 120 degree hue turn takes red to green.
        assert_eq!(
            one(
                [255, 0, 0, 255],
                Adjustment::HueSaturation {
                    hue: 120,
                    saturation: 0,
                    lightness: 0
                }
            ),
            [0, 255, 0, 255]
        );
        // Full desaturation leaves HSL lightness: (200 + 50) / 2 = 125.
        assert_eq!(
            one(
                [200, 100, 50, 255],
                Adjustment::HueSaturation {
                    hue: 0,
                    saturation: -100,
                    lightness: 0
                }
            ),
            [125, 125, 125, 255]
        );
        // Warm raises red and lowers blue.
        let warm = one(
            [100, 100, 100, 255],
            Adjustment::Temperature {
                temperature: 40,
                tint: 0,
            },
        );
        assert_eq!(warm, [110, 100, 90, 255]);
        // Vibrance boosts a dull colour more than a vivid one.
        let dull = one([140, 120, 120, 255], Adjustment::Vibrance { amount: 100 });
        assert!(dull[0] - dull[1] > 20);
        let vivid = one([255, 0, 0, 255], Adjustment::Vibrance { amount: 100 });
        assert_eq!(vivid, [255, 0, 0, 255]);
        let sepia = one([100, 100, 100, 255], Adjustment::Sepia { amount: 100 });
        assert_eq!(sepia, [135, 120, 93, 255]);
        let lifted = one(
            [40, 40, 40, 255],
            Adjustment::ShadowsHighlights {
                shadows: 100,
                highlights: 0,
            },
        );
        assert!(lifted[0] > 60, "{lifted:?}");
        let balanced = one(
            [128, 128, 128, 255],
            Adjustment::ColorBalance {
                tone: Tone::Midtones,
                cyan_red: 50,
                magenta_green: 0,
                yellow_blue: -50,
            },
        );
        assert!(balanced[0] > 128 && balanced[2] < 128 && balanced[1] == 128);
    }
    #[test]
    fn auto_levels_stretch_and_selection_limits() {
        let mut c = Canvas::new(10, 1);
        for x in 0..10 {
            c.set(x, 0, [(50 + x * 10) as u8, 100, 100, 255]);
        }
        apply(&mut c, &Adjustment::AutoLevels, None);
        assert_eq!(c.get(0, 0)[0], 0);
        assert_eq!(c.get(9, 0)[0], 255);
        let mut c = Canvas::filled(4, 1, [0, 0, 0, 255]);
        let sel = Mask::rect(4, 1, IRect::new(2, 0, 2, 1));
        apply(&mut c, &Adjustment::Invert, Some(&sel));
        assert_eq!(c.get(1, 0), [0, 0, 0, 255]);
        assert_eq!(c.get(2, 0), [255, 255, 255, 255]);
        let h = histogram(&c);
        assert_eq!(h[3][255], 2);
    }
}
