//! Cover artwork for things that have none: albums, artists and playlists in a
//! synthetic catalogue. Every cover is a composition generated from its key (an album
//! id, an artist id, a playlist id), so the same album shows the same cover in the
//! native players, on spotify.com and on music.youtube.com, on every platform and in
//! every run, and two albums are told apart at a glance.
//!
//! A composition is a small scene description — a vertical gradient and a stack of
//! circles, rings and polygons in a 1000-unit square — rather than pixels, so a
//! native player draws it at any size with the renderer's own antialiased shapes, and
//! a site serves it rasterised here (`rasterize`) at the size its page asks for. All of
//! it is integer arithmetic: no float trigonometry, whose last bits vary by platform.

/// The side of the square compositions are laid out in.
pub const UNIT: i32 = 1000;

/// Straight-alpha colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);
impl Rgba {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self(r, g, b, 255)
    }
    fn hex(s: &str) -> Self {
        let b = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).unwrap_or(0);
        Self(b(1), b(3), b(5), 255)
    }
    /// `self` moved `pct` percent of the way to `other`, keeping `self`'s alpha.
    pub fn mix(self, other: Rgba, pct: u32) -> Rgba {
        let pct = pct.min(100);
        let c = |a: u8, b: u8| ((u32::from(a) * (100 - pct) + u32::from(b) * pct) / 100) as u8;
        Rgba(
            c(self.0, other.0),
            c(self.1, other.1),
            c(self.2, other.2),
            self.3,
        )
    }
    pub fn with_alpha(self, a: u8) -> Rgba {
        Rgba(self.0, self.1, self.2, a)
    }
    /// Perceived brightness, 0 to 255.
    pub fn luma(self) -> u32 {
        (u32::from(self.0) * 299 + u32::from(self.1) * 587 + u32::from(self.2) * 114) / 1000
    }
    const BLACK: Rgba = Rgba(0, 0, 0, 255);
    const WHITE: Rgba = Rgba(255, 255, 255, 255);
}

/// One shape of a composition, in `UNIT` coordinates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shape {
    Circle {
        cx: i32,
        cy: i32,
        r: i32,
        color: Rgba,
    },
    /// The band between `r - width` and `r`.
    Ring {
        cx: i32,
        cy: i32,
        r: i32,
        width: i32,
        color: Rgba,
    },
    /// A closed polygon, filled by the even-odd rule.
    Polygon {
        points: Vec<(i32, i32)>,
        color: Rgba,
    },
}

/// The family a composition belongs to. Chosen from the key, so it varies as much as
/// the colours do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Style {
    Sunset,
    Bauhaus,
    Waves,
    Orbit,
    Shards,
    Stripes,
    Halftone,
}
impl Style {
    pub const ALL: [Style; 7] = [
        Style::Sunset,
        Style::Bauhaus,
        Style::Waves,
        Style::Orbit,
        Style::Shards,
        Style::Stripes,
        Style::Halftone,
    ];
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Artwork {
    pub style: Style,
    /// Background gradient, top to bottom.
    pub top: Rgba,
    pub bottom: Rgba,
    pub shapes: Vec<Shape>,
    /// The colour a player washes its Now Playing screen or a page header with.
    pub tint: Rgba,
}

/// Palettes: two background colours, then three for shapes.
const PALETTES: [[&str; 5]; 16] = [
    ["#1d2b53", "#7e2553", "#ff004d", "#ffa300", "#ffec27"],
    ["#0f2027", "#2c5364", "#00c9a7", "#f9f871", "#ff8066"],
    ["#f4ecd8", "#eadcbc", "#d1495b", "#00798c", "#30638e"],
    ["#1a1a2e", "#16213e", "#e94560", "#f5c26b", "#3f72af"],
    ["#fff1e6", "#fad2e1", "#ff6b6b", "#4ecdc4", "#1a535c"],
    ["#10002b", "#3c096c", "#c77dff", "#e0aaff", "#ff9e00"],
    ["#003049", "#1d4e89", "#fcbf49", "#f77f00", "#d62828"],
    ["#2d3142", "#4f5d75", "#ef8354", "#bfc0c0", "#ffffff"],
    ["#0b132b", "#1c2541", "#5bc0be", "#6fffe9", "#f25f5c"],
    ["#264653", "#2a9d8f", "#e9c46a", "#f4a261", "#e76f51"],
    ["#f1faee", "#a8dadc", "#e63946", "#457b9d", "#1d3557"],
    ["#232946", "#121629", "#eebbc3", "#b8c1ec", "#ffd803"],
    ["#3d0c11", "#6a040f", "#ffba08", "#faa307", "#e85d04"],
    ["#081c15", "#1b4332", "#95d5b2", "#d8f3dc", "#ffd166"],
    ["#fef9ef", "#ffe8cc", "#227c9d", "#17c3b2", "#fe6d73"],
    ["#14080e", "#49475b", "#799496", "#acc196", "#e9eb9e"],
];

/// FNV-1a: stable across platforms and releases.
pub fn hash(key: &str) -> u64 {
    key.bytes().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    })
}

/// SplitMix64, seeded by the key.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }
    /// Uniform in `lo..=hi`.
    fn range(&mut self, lo: i32, hi: i32) -> i32 {
        if hi <= lo {
            return lo;
        }
        lo + (self.next() % (hi - lo + 1) as u64) as i32
    }
    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[(self.next() % items.len() as u64) as usize]
    }
    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
}

/// Sine in 1/1024 units for whole degrees (Bhaskara I), identical on every target.
pub fn sin1024(degrees: i32) -> i32 {
    let d = degrees.rem_euclid(360);
    let (d, sign) = if d > 180 { (d - 180, -1) } else { (d, 1) };
    let k = i64::from(d) * i64::from(180 - d);
    sign * (4096 * k / (40500 - k)) as i32
}
pub fn cos1024(degrees: i32) -> i32 {
    sin1024(degrees + 90)
}
/// A point `r` from the centre at `degrees` clockwise from 3 o'clock.
fn polar(cx: i32, cy: i32, r: i32, degrees: i32) -> (i32, i32) {
    (
        cx + r * cos1024(degrees) / 1024,
        cy + r * sin1024(degrees) / 1024,
    )
}
/// A pie slice from `from` to `to` degrees, as a polygon.
fn wedge(cx: i32, cy: i32, r: i32, from: i32, to: i32) -> Vec<(i32, i32)> {
    let mut points = vec![(cx, cy)];
    let mut a = from;
    while a < to {
        points.push(polar(cx, cy, r, a));
        a += 10;
    }
    points.push(polar(cx, cy, r, to));
    points
}
fn rect(x: i32, y: i32, w: i32, h: i32) -> Vec<(i32, i32)> {
    vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h)]
}

/// The composition for `key`.
pub fn artwork(key: &str) -> Artwork {
    let mut rng = Rng(hash(key) ^ 0x05ee_dc0d_ea17_u64);
    let palette = PALETTES[(rng.next() % PALETTES.len() as u64) as usize].map(Rgba::hex);
    let style = Style::ALL[(rng.next() % Style::ALL.len() as u64) as usize];
    let [p0, p1, a, b, c] = palette;
    let inks = [a, b, c];
    let mut shapes = Vec::new();
    let (top, bottom) = match style {
        Style::Sunset => {
            let (top, bottom) = (p0, p1.mix(a, 25));
            let sky = |y: i32| top.mix(bottom, (y.clamp(0, UNIT) / 10) as u32);
            let (cx, cy, r) = (
                rng.range(380, 620),
                rng.range(470, 560),
                rng.range(250, 320),
            );
            shapes.push(Shape::Circle {
                cx,
                cy,
                r,
                color: b,
            });
            // The bands across the lower half of the sun, thickening towards the horizon.
            for i in 0..5 {
                let y = cy + 30 + i * 52;
                let h = 8 + i * 7;
                shapes.push(Shape::Polygon {
                    points: rect(cx - r - 10, y, 2 * r + 20, h),
                    color: sky(y + h / 2),
                });
            }
            for (ridge, (lo, hi), shade) in [(0, (640, 800), 0u32), (1, (780, 900), 35)] {
                let mut points = vec![(0, UNIT)];
                let steps = 6 + ridge;
                for i in 0..=steps {
                    points.push((i * UNIT / steps, rng.range(lo, hi)));
                }
                points.push((UNIT, UNIT));
                shapes.push(Shape::Polygon {
                    points,
                    color: c.mix(Rgba::BLACK, shade + 20),
                });
            }
            (top, bottom)
        }
        Style::Bauhaus => {
            let n = rng.pick(&[2, 3, 3]);
            let s = UNIT / n;
            for row in 0..n {
                for col in 0..n {
                    let (x, y) = (col * s, row * s);
                    let fill = rng.pick(&inks);
                    if rng.chance(45) {
                        shapes.push(Shape::Polygon {
                            points: rect(x, y, s, s),
                            color: rng.pick(&[p1, fill.mix(p0, 60)]),
                        });
                    }
                    let ink = rng.pick(&inks);
                    match rng.range(0, 5) {
                        0 => shapes.push(Shape::Circle {
                            cx: x + s / 2,
                            cy: y + s / 2,
                            r: s * 42 / 100,
                            color: ink,
                        }),
                        1 => {
                            let turn = rng.range(0, 3) * 90;
                            let (cx, cy) = match turn {
                                0 => (x, y),
                                90 => (x + s, y),
                                180 => (x + s, y + s),
                                _ => (x, y + s),
                            };
                            shapes.push(Shape::Polygon {
                                points: wedge(cx, cy, s, turn, turn + 90),
                                color: ink,
                            });
                        }
                        2 => {
                            let turn = rng.range(0, 3) * 90;
                            let (cx, cy) = polar(x + s / 2, y + s / 2, s / 2, turn + 180);
                            shapes.push(Shape::Polygon {
                                points: wedge(cx, cy, s / 2, turn - 90, turn + 90),
                                color: ink,
                            });
                        }
                        3 => shapes.push(Shape::Polygon {
                            points: if rng.chance(50) {
                                vec![(x, y), (x + s, y + s), (x, y + s)]
                            } else {
                                vec![(x + s, y), (x + s, y + s), (x, y + s)]
                            },
                            color: ink,
                        }),
                        4 => shapes.push(Shape::Ring {
                            cx: x + s / 2,
                            cy: y + s / 2,
                            r: s * 40 / 100,
                            width: s / 9,
                            color: ink,
                        }),
                        _ => {
                            let inset = s / 5;
                            shapes.push(Shape::Polygon {
                                points: rect(x + inset, y + inset, s - 2 * inset, s - 2 * inset),
                                color: ink,
                            })
                        }
                    }
                }
            }
            (p0, p0)
        }
        Style::Waves => {
            shapes.push(Shape::Circle {
                cx: rng.range(220, 780),
                cy: rng.range(180, 320),
                r: rng.range(80, 130),
                color: c.mix(Rgba::WHITE, 20),
            });
            let count = 4;
            for i in 0..count {
                let base = 440 + i * 150 + rng.range(-20, 20);
                let amp = rng.range(28, 70);
                let phase = rng.range(0, 359);
                let cycles = rng.range(1, 2);
                let mut points = vec![(0, UNIT)];
                for step in 0..=16 {
                    let x = step * UNIT / 16;
                    let angle = phase + x * cycles * 360 / UNIT;
                    points.push((x, base + amp * sin1024(angle) / 1024));
                }
                points.push((UNIT, UNIT));
                let ink = inks[(i as usize) % 3];
                shapes.push(Shape::Polygon {
                    points,
                    color: ink.mix(Rgba::BLACK, (i * 12) as u32),
                });
            }
            (p0, p1)
        }
        Style::Orbit => {
            let (cx, cy) = (rng.range(340, 660), rng.range(340, 660));
            let rings = rng.range(4, 6);
            for i in 0..rings {
                let r = 130 + i * 95;
                shapes.push(Shape::Ring {
                    cx,
                    cy,
                    r,
                    width: rng.range(6, 22),
                    color: inks[(i as usize) % 3].with_alpha(if i % 2 == 0 { 235 } else { 150 }),
                });
            }
            shapes.push(Shape::Circle {
                cx,
                cy,
                r: rng.range(55, 95),
                color: b,
            });
            for _ in 0..3 {
                let ring = rng.range(1, rings - 1);
                let (px, py) = polar(cx, cy, 130 + ring * 95 - 8, rng.range(0, 359));
                shapes.push(Shape::Circle {
                    cx: px,
                    cy: py,
                    r: rng.range(22, 48),
                    color: rng.pick(&inks),
                });
            }
            (p0, p1)
        }
        Style::Shards => {
            for _ in 0..rng.range(7, 10) {
                let (x, y) = (rng.range(-100, 1100), rng.range(-100, 1100));
                let size = rng.range(260, 560);
                let turn = rng.range(0, 359);
                // Roughly equilateral, so a shard reads as a facet rather than a splinter.
                let points = (0..3)
                    .map(|k| {
                        polar(
                            x,
                            y,
                            size * rng.range(70, 100) / 100,
                            turn + k * 120 + rng.range(-25, 25),
                        )
                    })
                    .collect();
                shapes.push(Shape::Polygon {
                    points,
                    color: rng.pick(&inks).with_alpha(rng.range(170, 235) as u8),
                });
            }
            if rng.chance(60) {
                shapes.push(Shape::Circle {
                    cx: rng.range(300, 700),
                    cy: rng.range(300, 700),
                    r: rng.range(90, 170),
                    color: p0.with_alpha(220),
                });
            }
            (p0, p1)
        }
        Style::Stripes => {
            let slope = rng.pick(&[1000, -1000, 600]);
            let mut offset = -UNIT - 400;
            let mut i = 0usize;
            while offset < 2 * UNIT + 400 {
                let w = rng.range(50, 150);
                if i.is_multiple_of(2) {
                    shapes.push(Shape::Polygon {
                        points: vec![
                            (offset, 0),
                            (offset + w, 0),
                            (offset + w - slope, UNIT),
                            (offset - slope, UNIT),
                        ],
                        color: inks[(i / 2) % 3],
                    });
                }
                offset += w;
                i += 1;
            }
            let (cx, cy) = (rng.range(350, 650), rng.range(350, 650));
            let r = rng.range(170, 260);
            shapes.push(Shape::Circle {
                cx,
                cy,
                r,
                color: p0,
            });
            shapes.push(Shape::Ring {
                cx,
                cy,
                r: r - 40,
                width: rng.range(14, 30),
                color: rng.pick(&inks),
            });
            (p0, p0.mix(p1, 50))
        }
        Style::Halftone => {
            let (fx, fy) = (rng.range(200, 800), rng.range(200, 800));
            shapes.push(Shape::Circle {
                cx: UNIT - fx,
                cy: UNIT - fy,
                r: rng.range(260, 360),
                color: a,
            });
            let n = 11;
            let pitch = UNIT / n;
            for row in 0..=n {
                for col in 0..=n {
                    let (x, y) = (col * pitch, row * pitch);
                    let (dx, dy) = (i64::from(x - fx), i64::from(y - fy));
                    let d = isqrt(dx * dx + dy * dy) as i32;
                    let r = (pitch * 48 / 100) - d * pitch / 1900;
                    if r >= 6 {
                        shapes.push(Shape::Circle {
                            cx: x,
                            cy: y,
                            r,
                            color: c.with_alpha(235),
                        });
                    }
                }
            }
            (p0, p1)
        }
    };
    let average = top.mix(bottom, 50);
    let tint = if average.luma() > 170 {
        a.mix(Rgba::BLACK, 15)
    } else {
        average.mix(a, 20)
    };
    Artwork {
        style,
        top,
        bottom,
        shapes,
        tint,
    }
}

fn isqrt(n: i64) -> i64 {
    if n <= 0 {
        return 0;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

/// Whether the point (in UNIT coordinates scaled by `SCALE`) lies in the polygon.
fn inside(points: &[(i64, i64)], x: i64, y: i64) -> bool {
    let mut inside = false;
    let n = points.len();
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (xi, yi) = points[i];
        let (xj, yj) = points[j];
        if (yi > y) != (yj > y) {
            // x < xi + (y - yi) * (xj - xi) / (yj - yi), without division.
            let lhs = (x - xi) * (yj - yi);
            let rhs = (y - yi) * (xj - xi);
            if (yj > yi && lhs < rhs) || (yj < yi && lhs > rhs) {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

fn over(dst: [u32; 3], c: Rgba) -> [u32; 3] {
    let a = u32::from(c.3);
    [
        (dst[0] * (255 - a) + u32::from(c.0) * a) / 255,
        (dst[1] * (255 - a) + u32::from(c.1) * a) / 255,
        (dst[2] * (255 - a) + u32::from(c.2) * a) / 255,
    ]
}

/// The composition as `size`×`size` straight-alpha RGBA, corners rounded to `radius`
/// pixels (half the size for a round avatar). Four samples a pixel, so edges are
/// smooth at the size it was asked for.
pub fn rasterize(art: &Artwork, size: u32, radius: u32) -> Vec<u8> {
    let size = size.clamp(1, 1024);
    const S: i64 = 2;
    let n = i64::from(size) * S;
    // Sample positions in UNIT * 2 space (the centre of each sub-pixel).
    let at = |i: i64| (2 * i + 1) * i64::from(UNIT) / n;
    struct Prepared {
        bounds: (i64, i64, i64, i64),
        kind: u8,
        cx: i64,
        cy: i64,
        r2: i64,
        inner2: i64,
        points: Vec<(i64, i64)>,
        color: Rgba,
    }
    let prepared: Vec<Prepared> = art
        .shapes
        .iter()
        .map(|s| match s {
            Shape::Circle { cx, cy, r, color } => {
                let (cx, cy, r) = (i64::from(*cx) * 2, i64::from(*cy) * 2, i64::from(*r) * 2);
                Prepared {
                    bounds: (cx - r, cy - r, cx + r, cy + r),
                    kind: 0,
                    cx,
                    cy,
                    r2: r * r,
                    inner2: -1,
                    points: vec![],
                    color: *color,
                }
            }
            Shape::Ring {
                cx,
                cy,
                r,
                width,
                color,
            } => {
                let (cx, cy, r) = (i64::from(*cx) * 2, i64::from(*cy) * 2, i64::from(*r) * 2);
                let inner = (r - i64::from(*width) * 2).max(0);
                Prepared {
                    bounds: (cx - r, cy - r, cx + r, cy + r),
                    kind: 0,
                    cx,
                    cy,
                    r2: r * r,
                    inner2: inner * inner,
                    points: vec![],
                    color: *color,
                }
            }
            Shape::Polygon { points, color } => {
                let points: Vec<(i64, i64)> = points
                    .iter()
                    .map(|(x, y)| (i64::from(*x) * 2, i64::from(*y) * 2))
                    .collect();
                let bounds = points.iter().fold(
                    (i64::MAX, i64::MAX, i64::MIN, i64::MIN),
                    |(a, b, c, d), (x, y)| (a.min(*x), b.min(*y), c.max(*x), d.max(*y)),
                );
                Prepared {
                    bounds,
                    kind: 1,
                    cx: 0,
                    cy: 0,
                    r2: 0,
                    inner2: 0,
                    points,
                    color: *color,
                }
            }
        })
        .collect();
    let mut out = vec![0u8; (size * size * 4) as usize];
    let radius = i64::from(radius.min(size / 2));
    for py in 0..i64::from(size) {
        for px in 0..i64::from(size) {
            let mut sum = [0u32; 3];
            for sy in 0..S {
                let y = at(py * S + sy);
                let t = (y / 2).clamp(0, i64::from(UNIT)) as u32;
                let bg = art.top.mix(art.bottom, t / 10);
                for sx in 0..S {
                    let x = at(px * S + sx);
                    // Samples are in doubled UNIT coordinates, where the shapes were prepared.
                    let (x2, y2) = (x, y);
                    let mut c = [u32::from(bg.0), u32::from(bg.1), u32::from(bg.2)];
                    for s in &prepared {
                        let (l, t, r, b) = s.bounds;
                        if x2 < l || x2 > r || y2 < t || y2 > b {
                            continue;
                        }
                        let hit = if s.kind == 0 {
                            let (dx, dy) = (x2 - s.cx, y2 - s.cy);
                            let d2 = dx * dx + dy * dy;
                            d2 <= s.r2 && d2 >= s.inner2.max(0)
                        } else {
                            inside(&s.points, x2, y2)
                        };
                        if hit {
                            c = over(c, s.color);
                        }
                    }
                    for k in 0..3 {
                        sum[k] += c[k];
                    }
                }
            }
            let i = ((py * i64::from(size) + px) * 4) as usize;
            let samples = (S * S) as u32;
            out[i] = (sum[0] / samples) as u8;
            out[i + 1] = (sum[1] / samples) as u8;
            out[i + 2] = (sum[2] / samples) as u8;
            out[i + 3] = corner_alpha(px, py, i64::from(size), radius);
        }
    }
    out
}

/// Coverage of a pixel by a square of side `size` with corners of `radius`.
fn corner_alpha(px: i64, py: i64, size: i64, radius: i64) -> u8 {
    if radius == 0 {
        return 255;
    }
    let near = |v: i64| v < radius || v >= size - radius;
    if !(near(px) && near(py)) {
        return 255;
    }
    // Four-by-four samples against the corner circle, in eighths of a pixel.
    let r = radius * 8;
    let mut hits = 0u32;
    for sy in [1, 3, 5, 7] {
        for sx in [1, 3, 5, 7] {
            let x = px * 8 + sx;
            let y = py * 8 + sy;
            let cx = if x < r { r } else { size * 8 - r };
            let cy = if y < r { r } else { size * 8 - r };
            let (dx, dy) = (x - cx, y - cy);
            let outside_x = (x < r && x < cx) || (x > size * 8 - r && x > cx);
            let outside_y = (y < r && y < cy) || (y > size * 8 - r && y > cy);
            if !(outside_x && outside_y) || dx * dx + dy * dy <= r * r {
                hits += 1;
            }
        }
    }
    (hits * 255 / 16) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_key_always_draws_the_same_cover() {
        for key in ["cold-start", "fan-out", "puget", "ship-it", "harbor-lights"] {
            assert_eq!(artwork(key), artwork(key));
            assert_eq!(
                rasterize(&artwork(key), 32, 4),
                rasterize(&artwork(key), 32, 4)
            );
        }
    }

    #[test]
    fn keys_are_told_apart_and_every_style_is_reachable() {
        let keys: Vec<String> = (0..200).map(|i| format!("album-{i}")).collect();
        let arts: Vec<Artwork> = keys.iter().map(|k| artwork(k)).collect();
        for style in Style::ALL {
            assert!(arts.iter().any(|a| a.style == style), "{style:?}");
        }
        // Neighbouring ids do not collapse to one picture.
        let pictures: std::collections::BTreeSet<Vec<u8>> =
            arts.iter().take(40).map(|a| rasterize(a, 12, 0)).collect();
        assert_eq!(pictures.len(), 40);
    }

    #[test]
    fn rasterizing_honours_size_and_rounds_the_corners() {
        let art = artwork("cold-start");
        let square = rasterize(&art, 20, 0);
        assert_eq!(square.len(), 20 * 20 * 4);
        assert!(square.chunks(4).all(|p| p[3] == 255));
        let round = rasterize(&art, 20, 10);
        // A corner of a round avatar is transparent; its centre is not.
        assert_eq!(round[3], 0);
        assert_eq!(round[(10 * 20 + 10) * 4 + 3], 255);
        // Every shape stays inside the square it was composed for, near enough.
        for shape in &art.shapes {
            if let Shape::Circle { r, .. } = shape {
                assert!(*r > 0);
            }
        }
    }

    #[test]
    fn integer_trigonometry_is_close_to_the_real_thing() {
        assert_eq!(sin1024(0), 0);
        assert_eq!(sin1024(90), 1024);
        assert_eq!(sin1024(270), -1024);
        assert_eq!(cos1024(0), 1024);
        assert!((sin1024(30) - 512).abs() < 4);
        assert_eq!(isqrt(144), 12);
        assert!(inside(&[(0, 0), (10, 0), (10, 10), (0, 10)], 5, 5));
        assert!(!inside(&[(0, 0), (10, 0), (10, 10), (0, 10)], 15, 5));
    }
}
