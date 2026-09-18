//! Healing and repair.
//!
//! [`heal`] is GIMP's Heal tool: the source's texture is copied, but the difference
//! between destination and source is carried in from the edge of the brush by solving
//! Laplace's equation over it, so the copy takes on the colour and shading of where it
//! lands. [`inpaint`] is a repair tool (Pixelmator Pro's Repair): the painted area is
//! rebuilt patch by patch from the best-matching texture around it, working in from
//! the edge (exemplar-based inpainting after Criminisi, Pérez and Toyama).
//!
//! Both use only integer arithmetic and IEEE basic operations in a fixed order.
use crate::{Canvas, IRect, Mask, Rgba};

/// Heal `area` of `layer`: where `cov` is non-zero the result is the source plus a
/// harmonic correction that matches the destination on the boundary, mixed in by the
/// coverage, `opacity` (0..=255) and the selection. Returns the area changed.
pub fn heal(
    layer: &mut Canvas,
    area: IRect,
    cov: &dyn Fn(i32, i32) -> u8,
    source: &dyn Fn(i32, i32) -> Rgba,
    opacity: u8,
    selection: Option<&Mask>,
) -> Option<IRect> {
    let area = area.clip(layer.width(), layer.height())?;
    // One pixel of boundary all round, where the canvas has it.
    let g = area.grow(1).clip(layer.width(), layer.height())?;
    let (gw, gh) = (g.w as usize, g.h as usize);
    let idx = |x: i32, y: i32| (y - g.y) as usize * gw + (x - g.x) as usize;
    let mut f = vec![[0f64; 4]; gw * gh];
    let mut inside = vec![false; gw * gh];
    let mut src = vec![[0u8; 4]; gw * gh];
    for y in g.y..g.bottom() {
        for x in g.x..g.right() {
            let i = idx(x, y);
            let (d, s) = (layer.get(x, y), source(x, y));
            src[i] = s;
            f[i] = std::array::from_fn(|c| f64::from(d[c]) - f64::from(s[c]));
            inside[i] = area.contains(x, y) && cov(x, y) > 0;
        }
    }
    // Start from the mean of the boundary, where the correction is known.
    let mut mean = [0f64; 4];
    let mut count = 0.0;
    for i in 0..gw * gh {
        if !inside[i] && neighbours(i, gw, gh).any(|j| inside[j]) {
            for c in 0..4 {
                mean[c] += f[i][c];
            }
            count += 1.0;
        }
    }
    if count == 0.0 {
        // The brush covers everything reachable: nothing to match, a plain clone.
        count = 1.0;
    }
    for (i, v) in f.iter_mut().enumerate() {
        if inside[i] {
            *v = mean.map(|m| m / count);
        }
    }
    // Successive over-relaxation, a fixed number of sweeps in scan order.
    let n = gw.max(gh);
    let omega = (2.0 / (1.0 + 3.0 / n as f64)).min(1.9);
    let sweeps = (2 * n).clamp(16, 400);
    for _ in 0..sweeps {
        for i in 0..gw * gh {
            if !inside[i] {
                continue;
            }
            let mut sum = [0f64; 4];
            let mut k = 0.0;
            for j in neighbours(i, gw, gh) {
                for c in 0..4 {
                    sum[c] += f[j][c];
                }
                k += 1.0;
            }
            for c in 0..4 {
                f[i][c] += omega * (sum[c] / k - f[i][c]);
            }
        }
    }
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            let i = idx(x, y);
            if !inside[i] {
                continue;
            }
            let healed: Rgba =
                std::array::from_fn(|c| crate::fmath::to_u8(f64::from(src[i][c]) + f[i][c]));
            let mut k = u32::from(cov(x, y)) * u32::from(opacity);
            k = crate::fmath::div255(k);
            if let Some(sel) = selection {
                k = crate::fmath::div255(k * u32::from(sel.get(x, y)));
            }
            let d = layer.get(x, y);
            layer.set(x, y, crate::blend::mix(d, healed, k));
        }
    }
    Some(area)
}

fn neighbours(i: usize, w: usize, h: usize) -> impl Iterator<Item = usize> {
    let (x, y) = (i % w, i / w);
    [
        (x > 0).then(|| i - 1),
        (x + 1 < w).then(|| i + 1),
        (y > 0).then(|| i - w),
        (y + 1 < h).then(|| i + w),
    ]
    .into_iter()
    .flatten()
}

/// Radius of the square patches the repair compares and copies.
const PATCH: i32 = 3;

/// Rebuild the pixels of `layer` under `hole` (any non-zero coverage) from the texture
/// around them. Returns the area changed.
pub fn inpaint(layer: &mut Canvas, hole: &Mask) -> Option<IRect> {
    let (w, h) = (layer.width() as i32, layer.height() as i32);
    let bounds = hole.bounds()?.clip(w as u32, h as u32)?;
    let mut unknown = vec![false; (w * h) as usize];
    let mut left = 0usize;
    for y in bounds.y..bounds.bottom() {
        for x in bounds.x..bounds.right() {
            if hole.get(x, y) > 0 {
                unknown[(y * w + x) as usize] = true;
                left += 1;
            }
        }
    }
    let known =
        |u: &[bool], x: i32, y: i32| x >= 0 && y >= 0 && x < w && y < h && !u[(y * w + x) as usize];
    // Candidate source patches: wholly known, around the hole.
    let margin = (bounds.w.max(bounds.h) as i32).clamp(12, 48);
    let search = bounds.grow(margin as u32);
    let mut candidates = vec![];
    let area = u64::from(search.w) * u64::from(search.h);
    let stride = if area > 40_000 { 2 } else { 1 };
    let mut y = search.y.max(PATCH);
    while y < search.bottom().min(h - PATCH) {
        let mut x = search.x.max(PATCH);
        while x < search.right().min(w - PATCH) {
            let whole = (-PATCH..=PATCH)
                .all(|dy| (-PATCH..=PATCH).all(|dx| known(&unknown, x + dx, y + dy)));
            if whole {
                candidates.push((x, y));
            }
            x += stride;
        }
        y += stride;
    }
    if candidates.is_empty() {
        // No intact texture nearby (the hole is nearly the whole image): diffuse the
        // colour in from whatever edge there is instead.
        return diffuse(layer, hole, bounds, &mut unknown);
    }
    while left > 0 {
        // The front pixel with the most known pixels in its patch goes first; ties in
        // scan order.
        let mut best: Option<(i32, i32, i32)> = None;
        for y in bounds.y..bounds.bottom() {
            for x in bounds.x..bounds.right() {
                if !unknown[(y * w + x) as usize] {
                    continue;
                }
                let front = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                    .iter()
                    .any(|(dx, dy)| known(&unknown, x + dx, y + dy));
                if !front {
                    continue;
                }
                let mut conf = 0;
                for dy in -PATCH..=PATCH {
                    for dx in -PATCH..=PATCH {
                        if known(&unknown, x + dx, y + dy) {
                            conf += 1;
                        }
                    }
                }
                if best.is_none_or(|b| conf > b.2) {
                    best = Some((x, y, conf));
                }
            }
        }
        let Some((tx, ty, _)) = best else {
            // Only unreachable pixels remain (off-canvas islands): diffuse them.
            return diffuse(layer, hole, bounds, &mut unknown).map(|r| r.union(&bounds));
        };
        // The source patch most like what is known of the target patch.
        let mut choice = candidates[0];
        let mut least = u64::MAX;
        for (sx, sy) in &candidates {
            let mut ssd = 0u64;
            'patch: for dy in -PATCH..=PATCH {
                for dx in -PATCH..=PATCH {
                    if !known(&unknown, tx + dx, ty + dy) {
                        continue;
                    }
                    let (a, b) = (layer.get(tx + dx, ty + dy), layer.get(sx + dx, sy + dy));
                    for c in 0..4 {
                        let d = i64::from(a[c]) - i64::from(b[c]);
                        ssd += (d * d) as u64;
                    }
                    if ssd >= least {
                        break 'patch;
                    }
                }
            }
            if ssd < least {
                least = ssd;
                choice = (*sx, *sy);
            }
        }
        for dy in -PATCH..=PATCH {
            for dx in -PATCH..=PATCH {
                let (x, y) = (tx + dx, ty + dy);
                if x < 0 || y < 0 || x >= w || y >= h || !unknown[(y * w + x) as usize] {
                    continue;
                }
                layer.set(x, y, layer.get(choice.0 + dx, choice.1 + dy));
                unknown[(y * w + x) as usize] = false;
                left -= 1;
            }
        }
    }
    Some(bounds)
}

/// Fill unknown pixels by repeatedly averaging their known neighbours.
fn diffuse(layer: &mut Canvas, hole: &Mask, bounds: IRect, unknown: &mut [bool]) -> Option<IRect> {
    let (w, h) = (layer.width() as i32, layer.height() as i32);
    let _ = hole;
    loop {
        let mut filled = vec![];
        for y in bounds.y..bounds.bottom() {
            for x in bounds.x..bounds.right() {
                if !unknown[(y * w + x) as usize] {
                    continue;
                }
                let mut sum = [0u32; 4];
                let mut n = 0;
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx >= 0 && ny >= 0 && nx < w && ny < h && !unknown[(ny * w + nx) as usize] {
                        let p = layer.get(nx, ny);
                        for c in 0..4 {
                            sum[c] += u32::from(p[c]);
                        }
                        n += 1;
                    }
                }
                if n > 0 {
                    filled.push((x, y, sum.map(|s| ((s + n / 2) / n) as u8)));
                }
            }
        }
        if filled.is_empty() {
            return Some(bounds);
        }
        for (x, y, p) in filled {
            layer.set(x, y, p);
            unknown[(y * w + x) as usize] = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn healing_keeps_the_texture_and_takes_the_surrounding_colour() {
        // Source: grey 100 with a bright cross; destination: flat 180.
        let mut layer = Canvas::filled(40, 20, [180, 180, 180, 255]);
        let mut source = Canvas::filled(40, 20, [100, 100, 100, 255]);
        for d in -2..=2 {
            source.set(10 + d, 10, [140, 100, 100, 255]);
            source.set(10, 10 + d, [140, 100, 100, 255]);
        }
        // Heal a disc of radius 6 at (30, 10) from (10, 10).
        let cov = |x: i32, y: i32| {
            if (x - 30) * (x - 30) + (y - 10) * (y - 10) <= 36 {
                255
            } else {
                0
            }
        };
        let src = |x: i32, y: i32| source.get(x - 20, y);
        heal(&mut layer, IRect::new(23, 3, 15, 15), &cov, &src, 255, None).unwrap();
        // Texture: the cross, shifted by the destination's brightness exactly.
        assert_eq!(layer.get(30, 10), [220, 180, 180, 255]);
        assert_eq!(layer.get(32, 10), [220, 180, 180, 255]);
        assert_eq!(layer.get(28, 12), [180, 180, 180, 255]);
        assert_eq!(layer.get(30, 4), [180, 180, 180, 255]);
        // Outside the brush nothing moved.
        assert_eq!(layer.get(10, 10), [180, 180, 180, 255]);
    }
    #[test]
    fn healing_blends_a_gradient_boundary_smoothly() {
        // Destination darkens left to right; source is flat. The healed disc must
        // follow the destination's ramp, not the source's flat colour.
        let mut layer = Canvas::new(30, 30);
        for y in 0..30 {
            for x in 0..30 {
                layer.set(x, y, [(200 - 4 * x) as u8, 50, 50, 255]);
            }
        }
        let before = layer.clone();
        let cov = |x: i32, y: i32| u8::from((x - 15) * (x - 15) + (y - 15) * (y - 15) <= 25) * 255;
        let src = |_: i32, _: i32| [0, 0, 0, 255];
        heal(&mut layer, IRect::new(9, 9, 13, 13), &cov, &src, 255, None).unwrap();
        for x in 11..=19 {
            let (got, want) = (layer.get(x, 15)[0], before.get(x, 15)[0]);
            assert!(got.abs_diff(want) <= 1, "x {x}: {got} vs {want}");
        }
    }
    #[test]
    fn repair_rebuilds_a_blemish_from_its_surroundings() {
        // Horizontal stripes, 3 pixels each, with a red blotch painted over them.
        let stripe = |y: i32| -> Rgba {
            if (y / 3) % 2 == 0 {
                [30, 60, 90, 255]
            } else {
                [200, 190, 180, 255]
            }
        };
        let mut layer = Canvas::new(48, 36);
        for y in 0..36 {
            for x in 0..48 {
                layer.set(x, y, stripe(y));
            }
        }
        let clean = layer.clone();
        let mut hole = Mask::empty(48, 36);
        for y in 14..21 {
            for x in 20..28 {
                layer.set(x, y, [255, 0, 0, 255]);
                hole.set(x, y, 255);
            }
        }
        let r = inpaint(&mut layer, &hole).unwrap();
        assert_eq!(r, IRect::new(20, 14, 8, 7));
        assert_eq!(layer, clean, "the stripes run straight through");
        // Flat colour fills flat.
        let mut flat = Canvas::filled(20, 20, [7, 8, 9, 255]);
        flat.set(10, 10, [255, 255, 255, 255]);
        let mut hole = Mask::empty(20, 20);
        hole.set(10, 10, 255);
        inpaint(&mut flat, &hole).unwrap();
        assert_eq!(flat.get(10, 10), [7, 8, 9, 255]);
        // Nothing intact to copy from: the colour diffuses in.
        let mut tiny = Canvas::filled(4, 4, [50, 50, 50, 255]);
        let hole = Mask::rect(4, 4, IRect::new(1, 1, 3, 3));
        inpaint(&mut tiny, &hole).unwrap();
        assert_eq!(tiny.get(3, 3), [50, 50, 50, 255]);
    }
}
