//! The sample media the reference world seeds each machine's movies folder with. They
//! are drawn and synthesised here, deterministically, rather than recorded: a leader
//! countdown, broadcast colour bars, a sunset over the sea, a beep track and a short
//! piece of music. `tests/samples.rs` checks the files under `worlds/company-2026/samples`
//! are exactly these.
use crate::{apng, wav};
use cw_raster::{fmath, Canvas, Rgba};

pub const WIDTH: u32 = 320;
pub const HEIGHT: u32 = 180;
pub const FPS: u16 = 12;
pub const RATE: u32 = 11_025;

fn rect(c: &mut Canvas, x: i32, y: i32, w: i32, h: i32, color: Rgba) {
    for yy in y.max(0)..(y + h).min(c.height() as i32) {
        for xx in x.max(0)..(x + w).min(c.width() as i32) {
            c.set(xx, yy, color);
        }
    }
}

/// A seven-segment digit `d` with its top-left at (x, y), `s` pixels per segment unit.
fn digit(c: &mut Canvas, d: u8, x: i32, y: i32, s: i32, color: Rgba) {
    // Segments a..g as on a display: top, top-right, bottom-right, bottom, bottom-left,
    // top-left, middle.
    const LIT: [u8; 10] = [
        0b0111111, 0b0000110, 0b1011011, 0b1001111, 0b1100110, 0b1101101, 0b1111101, 0b0000111,
        0b1111111, 0b1101111,
    ];
    let on = LIT[usize::from(d % 10)];
    let (w, t) = (6 * s, s);
    let segs = [
        (x + t, y, w, t),
        (x + w + t, y + t, t, w),
        (x + w + t, y + w + 2 * t, t, w),
        (x + t, y + 2 * w + 2 * t, w, t),
        (x, y + w + 2 * t, t, w),
        (x, y + t, t, w),
        (x + t, y + w + t, w, t),
    ];
    for (i, (sx, sy, sw, sh)) in segs.into_iter().enumerate() {
        if on & (1 << i) != 0 {
            rect(c, sx, sy, sw, sh, color);
        }
    }
}

/// A film-leader countdown: 3, 2, 1, a sweep going round each second.
pub fn countdown() -> Vec<Canvas> {
    let (cx, cy, r) = (160i64, 90i64, 70i64);
    (0..3 * i32::from(FPS))
        .map(|f| {
            let mut c = Canvas::filled(WIDTH, HEIGHT, [38, 42, 48, 255]);
            let second = f / i32::from(FPS);
            let within = f % i32::from(FPS);
            // Sweep angle, clockwise from twelve o'clock.
            let angle = f64::from(within + 1) / f64::from(FPS) * 2.0 * fmath::PI;
            let ca = fmath::cos(angle);
            for y in 0..HEIGHT as i64 {
                for x in 0..WIDTH as i64 {
                    let (dx, dy) = (x - cx, y - cy);
                    let d2 = dx * dx + dy * dy;
                    if d2 > r * r {
                        continue;
                    }
                    // A pixel's clockwise angle from twelve o'clock is below the hand's
                    // when its cosine compares the right way for its half of the dial.
                    let cos_p = if d2 == 0 {
                        1.0
                    } else {
                        -(dy as f64) / (d2 as f64).sqrt()
                    };
                    let swept = if dx >= 0 {
                        angle > fmath::PI || cos_p > ca
                    } else {
                        angle > fmath::PI && cos_p < ca
                    };
                    let ring = d2 >= (r - 4) * (r - 4);
                    let color = if ring {
                        [230, 230, 230, 255]
                    } else if swept {
                        [96, 104, 116, 255]
                    } else {
                        [58, 64, 72, 255]
                    };
                    c.set(x as i32, y as i32, color);
                }
            }
            // Crosshair.
            rect(&mut c, 0, 89, WIDTH as i32, 2, [140, 140, 140, 255]);
            rect(&mut c, 159, 0, 2, HEIGHT as i32, [140, 140, 140, 255]);
            digit(&mut c, (3 - second) as u8, 136, 52, 6, [255, 255, 255, 255]);
            c
        })
        .collect()
}

/// SMPTE-style 75% colour bars with a marker crossing the bottom band.
pub fn color_bars() -> Vec<Canvas> {
    const TOP: [Rgba; 7] = [
        [191, 191, 191, 255],
        [191, 191, 0, 255],
        [0, 191, 191, 255],
        [0, 191, 0, 255],
        [191, 0, 191, 255],
        [191, 0, 0, 255],
        [0, 0, 191, 255],
    ];
    const MID: [Rgba; 7] = [
        [0, 0, 191, 255],
        [19, 19, 19, 255],
        [191, 0, 191, 255],
        [19, 19, 19, 255],
        [0, 191, 191, 255],
        [19, 19, 19, 255],
        [191, 191, 191, 255],
    ];
    let bar = |i: usize| (WIDTH as usize * i / 7) as i32;
    (0..2 * i32::from(FPS))
        .map(|f| {
            let mut c = Canvas::filled(WIDTH, HEIGHT, [19, 19, 19, 255]);
            for i in 0..7 {
                let w = bar(i + 1) - bar(i);
                rect(&mut c, bar(i), 0, w, 120, TOP[i]);
                rect(&mut c, bar(i), 120, w, 15, MID[i]);
            }
            rect(&mut c, 0, 135, 57, 45, [0, 33, 76, 255]);
            rect(&mut c, 57, 135, 57, 45, [255, 255, 255, 255]);
            rect(&mut c, 114, 135, 57, 45, [50, 0, 106, 255]);
            let x = 180 + f * 5;
            rect(&mut c, x, 145, 20, 25, [235, 235, 235, 255]);
            c
        })
        .collect()
}

/// A sun setting into the sea, the water's light rippling.
pub fn sunset() -> Vec<Canvas> {
    let horizon = 112i32;
    (0..3 * i32::from(FPS))
        .map(|f| {
            let mut c = Canvas::new(WIDTH, HEIGHT);
            for y in 0..horizon {
                // Orange at the horizon to violet overhead.
                let t = y * 255 / horizon;
                let px = [
                    (120 + t * 130 / 255) as u8,
                    (40 + t * 100 / 255) as u8,
                    (110 - t * 70 / 255) as u8,
                    255,
                ];
                rect(&mut c, 0, y, WIDTH as i32, 1, px);
            }
            let (sx, sy, r) = (200, 60 + f * 2, 22);
            for y in (sy - r).max(0)..(sy + r).min(horizon) {
                for x in sx - r..sx + r {
                    if (x - sx) * (x - sx) + (y - sy) * (y - sy) <= r * r {
                        c.set(x, y, [255, 214, 120, 255]);
                    }
                }
            }
            for y in horizon..HEIGHT as i32 {
                let depth = y - horizon;
                let base = [
                    (30 + depth / 2) as u8,
                    (40 + depth / 3) as u8,
                    (90 - depth / 3) as u8,
                    255,
                ];
                rect(&mut c, 0, y, WIDTH as i32, 1, base);
                // Glints under the sun, moving with the frame.
                if (y + f) % 4 == 0 {
                    let w = 50 - depth / 2;
                    rect(
                        &mut c,
                        sx - w / 2 + (y * 7 + f * 3) % 9 - 4,
                        y,
                        w,
                        1,
                        [255, 190, 110, 255],
                    );
                }
            }
            c
        })
        .collect()
}

fn tone(freq: f64, t: f64) -> f64 {
    fmath::sin(2.0 * fmath::PI * freq * t)
}

/// A beep at the start of each second, the last one higher: a countdown's sound.
pub fn beeps() -> Vec<i16> {
    (0..3 * RATE)
        .map(|i| {
            let second = i / RATE;
            let at = i % RATE;
            if at >= RATE * 15 / 100 {
                return 0;
            }
            let t = f64::from(at) / f64::from(RATE);
            let freq = if second == 2 { 1760.0 } else { 880.0 };
            (tone(freq, t) * 12_000.0) as i16
        })
        .collect()
}

/// Four seconds of arpeggiated chords: C, A minor, F, G.
pub fn music() -> Vec<i16> {
    const CHORDS: [[f64; 3]; 4] = [
        [261.63, 329.63, 392.00],
        [220.00, 261.63, 329.63],
        [174.61, 220.00, 261.63],
        [196.00, 246.94, 293.66],
    ];
    let note = RATE / 4;
    (0..4 * RATE)
        .map(|i| {
            let beat = (i / note) as usize;
            let chord = CHORDS[beat / 4 % 4];
            let freq = chord[beat % 3];
            let at = i % note;
            let t = f64::from(at) / f64::from(RATE);
            // A plucked envelope: quick attack, linear decay.
            let env = if at < 80 {
                f64::from(at) / 80.0
            } else {
                1.0 - f64::from(at - 80) / f64::from(note)
            };
            let bass = tone(chord[0] / 2.0, f64::from(i) / f64::from(RATE)) * 0.35;
            ((tone(freq, t) * env + bass) * 9000.0) as i16
        })
        .collect()
}

/// Every sample file, by the name it is seeded under.
pub fn files() -> Result<Vec<(&'static str, Vec<u8>)>, String> {
    Ok(vec![
        ("Countdown.apng", apng::encode(&countdown(), FPS)?),
        ("Color Bars.apng", apng::encode(&color_bars(), FPS)?),
        ("Sunset.apng", apng::encode(&sunset(), FPS)?),
        (
            "Countdown Beeps.wav",
            wav::encode(&wav::Pcm {
                rate: RATE,
                samples: beeps(),
            }),
        ),
        (
            "Music Bed.wav",
            wav::encode(&wav::Pcm {
                rate: RATE,
                samples: music(),
            }),
        ),
    ])
}
