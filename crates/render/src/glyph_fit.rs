//! Integer grid fitting for the fixed-pitch terminal face, for OCR legibility.
//!
//! The rasterizer is unhinted, so a thin horizontal stroke whose outline straddles a
//! pixel boundary is split across two rows at partial coverage and washes out: at 11 px
//! `-` is two rows of 45% grey, which OCR drops (`atlas-sync-31288` -> `atlas sync-31288`).
//! `~` spans under 2 px of outline below 18 px, so its wave survives downsampling as a
//! smear that reads as `"` or `#`. The dotted zero's centre dot is mid-grey and merges
//! into the bowl, leaving `0` and `8` separated only by antialiasing.
//!
//! Every step below is integer arithmetic over the rasterized coverage bytes, so a fitted
//! glyph is bit-identical on every target; nothing here depends on float rounding. The
//! proportional UI faces are deliberately left alone (see `assets/README.md`).
use fontdue::{Font, Metrics};

/// Coverage at or above which a pixel counts as ink when tracing a counter.
const INK: u8 = 128;

/// Glyphs drawn entirely from horizontal bars, and how many bars each has.
fn bar_count(c: char) -> Option<usize> {
    Some(match c {
        // hyphen-minus, low line, hyphen, non-breaking hyphen, en/em dash, minus, macron
        '-' | '_' | '\u{2010}' | '\u{2011}' | '\u{2013}' | '\u{2014}' | '\u{2212}' | '\u{00AF}' => {
            1
        }
        '=' => 2,
        _ => return None,
    })
}

/// Grid-fit one terminal glyph. `size` is the clamped pixel size already rasterized.
pub fn fit(
    font: &Font,
    c: char,
    size: u16,
    metrics: Metrics,
    alpha: Vec<u8>,
) -> (Metrics, Vec<u8>) {
    let (w, h) = (metrics.width, metrics.height);
    if w == 0 || h == 0 {
        return (metrics, alpha);
    }
    if let Some(bars) = bar_count(c) {
        return (metrics, snap_bars(w, h, &alpha, bars));
    }
    if c == '~' {
        return stretch(font, c, size, metrics, alpha);
    }
    (metrics, boost_dot(w, h, &alpha))
}

/// Collapse each bar onto one whole pixel row, conserving ink. Splitting a 1 px bar over
/// two rows halves its contrast without adding information; one opaque row keeps the same
/// ink at every downsampling scale and doubles it at native resolution.
fn snap_bars(w: usize, h: usize, alpha: &[u8], bars: usize) -> Vec<u8> {
    let ink: Vec<u32> = (0..h)
        .map(|y| {
            alpha[y * w..(y + 1) * w]
                .iter()
                .map(|v| u32::from(*v))
                .sum()
        })
        .collect();
    let mut order: Vec<usize> = (0..h).filter(|&y| ink[y] > 0).collect();
    order.sort_by_key(|&y| (std::cmp::Reverse(ink[y]), y));
    let mut rows: Vec<usize> = Vec::with_capacity(bars);
    for y in order {
        // An adjacent row is the same bar's antialiased fringe, never a second bar.
        if rows.iter().all(|&r| y.abs_diff(r) > 1) {
            rows.push(y);
            if rows.len() == bars {
                break;
            }
        }
    }
    if rows.is_empty() {
        return alpha.to_vec();
    }
    rows.sort_unstable();
    let mut out = vec![0u8; w * h];
    for y in 0..h {
        // Ties go to the upper row so the choice never depends on iteration order.
        let target = *rows.iter().min_by_key(|&&r| (y.abs_diff(r), r)).unwrap();
        for x in 0..w {
            let i = target * w + x;
            out[i] = out[i].saturating_add(alpha[y * w + x]);
        }
    }
    out
}

/// Double the tilde's vertical amplitude: rasterize at twice the size, then box-filter the
/// columns back to single width. The wave keeps the font's true outline and its advance,
/// but now spans four to eight rows instead of two, which survives a downsampling pass.
fn stretch(
    font: &Font,
    c: char,
    size: u16,
    metrics: Metrics,
    alpha: Vec<u8>,
) -> (Metrics, Vec<u8>) {
    let (big, src) = font.rasterize(c, f32::from(size) * 2.0);
    if big.width == 0 || big.height == 0 {
        return (metrics, alpha);
    }
    // Pad so each output column covers an even pair of double-size columns; without this
    // an odd xmin would shift the glyph half a pixel against the rest of the line.
    let pad = big.xmin.rem_euclid(2) as usize;
    let w = (big.width + pad).div_ceil(2);
    let mut sum = vec![0u32; w * big.height];
    for y in 0..big.height {
        for (x, v) in src[y * big.width..(y + 1) * big.width].iter().enumerate() {
            sum[y * w + (x + pad) / 2] += u32::from(*v);
        }
    }
    // Two source columns per output column; round to nearest so the stroke keeps its weight.
    let out: Vec<u8> = sum.iter().map(|s| s.div_ceil(2).min(255) as u8).collect();
    let fitted = Metrics {
        width: w,
        height: big.height,
        xmin: big.xmin.div_euclid(2),
        // Grow about the wave's centre line so the tilde stays where the reader expects.
        ymin: metrics.ymin + metrics.height as i32 / 2 - big.height as i32 / 2,
        // Layout is tabulated from the single-size advance; the taller glyph must not move it.
        ..metrics
    };
    (fitted, out)
}

/// Snap a lone interior mark - the dotted zero's dot - to full opacity. The mark is the
/// only thing telling `0` from `8` and `O`, and it rasterizes mid-grey at terminal sizes.
/// Restricted to a single mark of at most a third of the glyph box so that shading blocks
/// and multi-dot symbols keep their designed density.
fn boost_dot(w: usize, h: usize, alpha: &[u8]) -> Vec<u8> {
    let outside = outside_background(w, h, alpha);
    let mut seen = vec![false; w * h];
    let mut marks: Vec<(Vec<usize>, u8)> = Vec::new();
    for start in 0..w * h {
        if alpha[start] < INK || seen[start] {
            continue;
        }
        seen[start] = true;
        let mut stack = vec![start];
        let mut component = vec![start];
        let mut enclosed = true;
        let mut peak = 0u8;
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            if x == 0 || y == 0 || x + 1 == w || y + 1 == h {
                enclosed = false;
            }
            peak = peak.max(alpha[i]);
            for j in neighbours(w, h, i) {
                if alpha[j] >= INK {
                    if !seen[j] {
                        seen[j] = true;
                        component.push(j);
                        stack.push(j);
                    }
                } else if outside[j] {
                    enclosed = false;
                }
            }
        }
        if enclosed {
            marks.push((component, peak));
        }
    }
    let mut out = alpha.to_vec();
    if let [(mark, peak)] = &marks[..] {
        let (x0, x1) = span(mark.iter().map(|i| i % w));
        let (y0, y1) = span(mark.iter().map(|i| i / w));
        let dot = (x1 - x0 + 1) * 3 <= w && (y1 - y0 + 1) * 3 <= h;
        if dot && *peak > 0 && *peak < 255 {
            let peak = u32::from(*peak);
            for &i in mark {
                out[i] = ((u32::from(alpha[i]) * 255 + peak / 2) / peak).min(255) as u8;
            }
        }
    }
    out
}

fn span(values: impl Iterator<Item = usize> + Clone) -> (usize, usize) {
    (values.clone().min().unwrap_or(0), values.max().unwrap_or(0))
}

/// Background pixels reachable from the glyph box edge; the rest are counters.
fn outside_background(w: usize, h: usize, alpha: &[u8]) -> Vec<bool> {
    let mut outside = vec![false; w * h];
    let mut stack = Vec::new();
    let seed = |i: usize, outside: &mut Vec<bool>, stack: &mut Vec<usize>| {
        if alpha[i] < INK && !outside[i] {
            outside[i] = true;
            stack.push(i);
        }
    };
    for x in 0..w {
        seed(x, &mut outside, &mut stack);
        seed((h - 1) * w + x, &mut outside, &mut stack);
    }
    for y in 0..h {
        seed(y * w, &mut outside, &mut stack);
        seed(y * w + w - 1, &mut outside, &mut stack);
    }
    while let Some(i) = stack.pop() {
        for j in neighbours(w, h, i) {
            if alpha[j] < INK && !outside[j] {
                outside[j] = true;
                stack.push(j);
            }
        }
    }
    outside
}

fn neighbours(w: usize, h: usize, i: usize) -> impl Iterator<Item = usize> {
    let (x, y) = (i % w, i / w);
    [
        (x > 0).then(|| i - 1),
        (x + 1 < w).then_some(i + 1),
        (y > 0).then(|| i - w),
        (y + 1 < h).then_some(i + w),
    ]
    .into_iter()
    .flatten()
}
