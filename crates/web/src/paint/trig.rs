//! Integer trigonometry for paint. No `f32`, no `sin()`: a fixed-point Taylor series
//! over a reduced angle, so every platform rounds the same way. Angles are in
//! centidegrees (`9000` is a right angle); results are in 1/1024 units, the scene's
//! transform scale.

const SCALE: i64 = 1 << 20;
/// pi * 2^20, rounded.
const PI: i64 = 3_294_199;

/// `sin` of a centidegree angle in 1/1024 units, exact at the quadrant boundaries.
pub fn sin_1024(centi_deg: i32) -> i32 {
    let a = (centi_deg as i64).rem_euclid(36_000);
    // Reduce to the first quadrant with the right sign.
    let (a, sign) = if a <= 9000 {
        (a, 1)
    } else if a <= 18_000 {
        (18_000 - a, 1)
    } else if a <= 27_000 {
        (a - 18_000, -1)
    } else {
        (36_000 - a, -1)
    };
    let v = match a {
        0 => 0,
        9000 => 1024,
        _ => {
            // x in radians, 2^20 fixed point.
            let x = (a * PI + 9000) / 18_000;
            let mut term = x;
            let mut sum = x;
            for k in 1..7i64 {
                term = term * x / SCALE;
                term = term * x / SCALE;
                term = -term / ((2 * k) * (2 * k + 1));
                sum += term;
            }
            ((sum * 1024 + SCALE / 2) / SCALE).clamp(0, 1024)
        }
    };
    (v * sign) as i32
}

pub fn cos_1024(centi_deg: i32) -> i32 {
    sin_1024(centi_deg.wrapping_add(9000))
}

/// `tan` in 1/1024 units, saturating near the poles.
pub fn tan_1024(centi_deg: i32) -> i32 {
    let s = sin_1024(centi_deg) as i64;
    let c = cos_1024(centi_deg) as i64;
    if c == 0 {
        return if s >= 0 { i32::MAX / 2 } else { i32::MIN / 2 };
    }
    ((s * 1024) / c).clamp(i32::MIN as i64 / 2, i32::MAX as i64 / 2) as i32
}

/// Integer square root, floor.
pub fn isqrt(v: i128) -> i128 {
    if v <= 0 {
        return 0;
    }
    let mut x = v;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + v / x) / 2;
    }
    x
}

/// A quarter arc as a polyline: points on the circle of `radius` around `(cx, cy)`
/// from `start` to `start + 90` centidegrees (clockwise in screen coordinates),
/// including both ends. Segment count grows with the radius and is capped.
pub fn quarter_arc(cx: i32, cy: i32, radius: u32, start_centi_deg: i32) -> Vec<(i32, i32)> {
    let n = (radius / 3).clamp(1, 16) as i32;
    (0..=n)
        .map(|i| {
            let a = start_centi_deg + 9000 * i / n;
            let x = cx as i64 + (radius as i64 * cos_1024(a) as i64 + 512).div_euclid(1024);
            let y = cy as i64 + (radius as i64 * sin_1024(a) as i64 + 512).div_euclid(1024);
            (x as i32, y as i32)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_matches_known_values() {
        assert_eq!(sin_1024(0), 0);
        assert_eq!(sin_1024(9000), 1024);
        assert_eq!(sin_1024(18_000), 0);
        assert_eq!(sin_1024(27_000), -1024);
        assert_eq!(sin_1024(3000), 512);
        assert_eq!(cos_1024(6000), 512);
        // sin 45 = 0.7071 * 1024 = 724.08
        assert_eq!(sin_1024(4500), 724);
        assert_eq!(cos_1024(0), 1024);
        assert_eq!(sin_1024(-9000), -1024);
        assert_eq!(tan_1024(4500), 1024);
        assert_eq!(isqrt(144), 12);
        assert_eq!(isqrt(145), 12);
    }

    #[test]
    fn arc_ends_on_axes() {
        let pts = quarter_arc(10, 10, 10, 0);
        assert_eq!(pts.first(), Some(&(20, 10)));
        assert_eq!(pts.last(), Some(&(10, 20)));
    }
}
