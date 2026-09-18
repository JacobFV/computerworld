//! Arbitrary-precision signed integers for Python's `int`.
//!
//! Sign-magnitude with little-endian base-2^32 limbs. Values that fit an `i64`
//! normally live in `Value::Int`; this type carries everything larger.
use std::cmp::Ordering;

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct BigInt {
    neg: bool,
    mag: Vec<u32>,
}

fn trim(v: &mut Vec<u32>) {
    while v.last() == Some(&0) {
        v.pop();
    }
}
fn cmp_mag(a: &[u32], b: &[u32]) -> Ordering {
    if a.len() != b.len() {
        return a.len().cmp(&b.len());
    }
    for i in (0..a.len()).rev() {
        match a[i].cmp(&b[i]) {
            Ordering::Equal => continue,
            o => return o,
        }
    }
    Ordering::Equal
}
fn add_mag(a: &[u32], b: &[u32]) -> Vec<u32> {
    let (a, b) = if a.len() >= b.len() { (a, b) } else { (b, a) };
    let mut out = Vec::with_capacity(a.len() + 1);
    let mut carry = 0u64;
    for i in 0..a.len() {
        let s = a[i] as u64 + *b.get(i).unwrap_or(&0) as u64 + carry;
        out.push(s as u32);
        carry = s >> 32;
    }
    if carry != 0 {
        out.push(carry as u32);
    }
    out
}
/// a - b where |a| >= |b|.
fn sub_mag(a: &[u32], b: &[u32]) -> Vec<u32> {
    let mut out = Vec::with_capacity(a.len());
    let mut borrow = 0i64;
    for i in 0..a.len() {
        let mut d = a[i] as i64 - *b.get(i).unwrap_or(&0) as i64 - borrow;
        if d < 0 {
            d += 1 << 32;
            borrow = 1;
        } else {
            borrow = 0;
        }
        out.push(d as u32);
    }
    trim(&mut out);
    out
}
fn mul_mag(a: &[u32], b: &[u32]) -> Vec<u32> {
    if a.is_empty() || b.is_empty() {
        return vec![];
    }
    let mut out = vec![0u32; a.len() + b.len()];
    for (i, &x) in a.iter().enumerate() {
        if x == 0 {
            continue;
        }
        let mut carry = 0u64;
        for (j, &y) in b.iter().enumerate() {
            let t = x as u64 * y as u64 + out[i + j] as u64 + carry;
            out[i + j] = t as u32;
            carry = t >> 32;
        }
        let mut k = i + b.len();
        while carry != 0 {
            let t = out[k] as u64 + carry;
            out[k] = t as u32;
            carry = t >> 32;
            k += 1;
        }
    }
    trim(&mut out);
    out
}
fn divrem_small(a: &[u32], d: u32) -> (Vec<u32>, u32) {
    let mut out = vec![0u32; a.len()];
    let mut rem = 0u64;
    for i in (0..a.len()).rev() {
        let cur = (rem << 32) | a[i] as u64;
        out[i] = (cur / d as u64) as u32;
        rem = cur % d as u64;
    }
    trim(&mut out);
    (out, rem as u32)
}
fn shl_mag(a: &[u32], bits: usize) -> Vec<u32> {
    if a.is_empty() {
        return vec![];
    }
    let words = bits / 32;
    let b = bits % 32;
    let mut out = vec![0u32; words];
    if b == 0 {
        out.extend_from_slice(a);
    } else {
        let mut carry = 0u32;
        for &x in a {
            out.push((x << b) | carry);
            carry = x >> (32 - b);
        }
        if carry != 0 {
            out.push(carry);
        }
    }
    trim(&mut out);
    out
}
fn shr_mag(a: &[u32], bits: usize) -> Vec<u32> {
    let words = bits / 32;
    if words >= a.len() {
        return vec![];
    }
    let b = bits % 32;
    let mut out = Vec::with_capacity(a.len() - words);
    for i in words..a.len() {
        let lo = a[i] >> b;
        let hi = if b == 0 {
            0
        } else {
            a.get(i + 1).map(|v| v << (32 - b)).unwrap_or(0)
        };
        out.push(lo | hi);
    }
    trim(&mut out);
    out
}
/// Knuth algorithm D on magnitudes. Returns (quotient, remainder).
fn divrem_mag(a: &[u32], b: &[u32]) -> (Vec<u32>, Vec<u32>) {
    if cmp_mag(a, b) == Ordering::Less {
        return (vec![], a.to_vec());
    }
    if b.len() == 1 {
        let (q, r) = divrem_small(a, b[0]);
        return (q, if r == 0 { vec![] } else { vec![r] });
    }
    let s = b[b.len() - 1].leading_zeros() as usize;
    let b = shl_mag(b, s);
    let mut a = shl_mag(a, s);
    a.push(0);
    let n = b.len();
    let m = a.len() - n - 1;
    let mut q = vec![0u32; m + 1];
    let btop = b[n - 1] as u64;
    let bnext = b[n - 2] as u64;
    for j in (0..=m).rev() {
        let num = ((a[j + n] as u64) << 32) | a[j + n - 1] as u64;
        let mut qhat = num / btop;
        let mut rhat = num % btop;
        while qhat >= (1 << 32) || qhat * bnext > ((rhat << 32) | a[j + n - 2] as u64) {
            qhat -= 1;
            rhat += btop;
            if rhat >= (1 << 32) {
                break;
            }
        }
        // Multiply and subtract.
        let mut borrow = 0i64;
        let mut carry = 0u64;
        for i in 0..n {
            let p = qhat * b[i] as u64 + carry;
            carry = p >> 32;
            let t = a[i + j] as i64 - borrow - (p & 0xffff_ffff) as i64;
            a[i + j] = t as u32;
            borrow = if t < 0 { 1 } else { 0 };
        }
        let t = a[j + n] as i64 - borrow - carry as i64;
        a[j + n] = t as u32;
        if t < 0 {
            // Add back.
            qhat -= 1;
            let mut c = 0u64;
            for i in 0..n {
                let s2 = a[i + j] as u64 + b[i] as u64 + c;
                a[i + j] = s2 as u32;
                c = s2 >> 32;
            }
            a[j + n] = a[j + n].wrapping_add(c as u32);
        }
        q[j] = qhat as u32;
    }
    trim(&mut q);
    a.truncate(n);
    trim(&mut a);
    let r = shr_mag(&a, s);
    (q, r)
}

impl BigInt {
    pub fn zero() -> Self {
        Self::default()
    }
    pub fn from_i64(v: i64) -> Self {
        let neg = v < 0;
        let m = v.unsigned_abs();
        let mut mag = vec![m as u32, (m >> 32) as u32];
        trim(&mut mag);
        Self { neg, mag }
    }
    pub fn from_u64(m: u64) -> Self {
        let mut mag = vec![m as u32, (m >> 32) as u32];
        trim(&mut mag);
        Self { neg: false, mag }
    }
    pub fn from_i128(v: i128) -> Self {
        let neg = v < 0;
        let mut m = v.unsigned_abs();
        let mut mag = vec![];
        while m != 0 {
            mag.push(m as u32);
            m >>= 32;
        }
        Self { neg, mag }
    }
    fn make(neg: bool, mag: Vec<u32>) -> Self {
        let neg = neg && !mag.is_empty();
        Self { neg, mag }
    }
    pub fn is_zero(&self) -> bool {
        self.mag.is_empty()
    }
    pub fn is_negative(&self) -> bool {
        self.neg
    }
    pub fn to_i64(&self) -> Option<i64> {
        if self.mag.len() > 2 {
            return None;
        }
        let m = self.mag.first().copied().unwrap_or(0) as u64
            | (self.mag.get(1).copied().unwrap_or(0) as u64) << 32;
        if self.neg {
            if m <= 1u64 << 63 {
                Some((m as i64).wrapping_neg())
            } else {
                None
            }
        } else if m <= i64::MAX as u64 {
            Some(m as i64)
        } else {
            None
        }
    }
    pub fn to_u64_mod(&self) -> u64 {
        let m = self.mag.first().copied().unwrap_or(0) as u64
            | (self.mag.get(1).copied().unwrap_or(0) as u64) << 32;
        if self.neg {
            m.wrapping_neg()
        } else {
            m
        }
    }
    pub fn bit_length(&self) -> u64 {
        match self.mag.last() {
            None => 0,
            Some(top) => (self.mag.len() as u64 - 1) * 32 + (32 - top.leading_zeros() as u64),
        }
    }
    /// Correctly rounded (half-even) conversion; `None` on overflow.
    pub fn to_f64(&self) -> Option<f64> {
        let bits = self.bit_length();
        if bits <= 64 {
            let m = self.mag.first().copied().unwrap_or(0) as u64
                | (self.mag.get(1).copied().unwrap_or(0) as u64) << 32;
            // u64 -> f64 is correctly rounded in Rust.
            let v = m as f64;
            return Some(if self.neg { -v } else { v });
        }
        if bits > 1024 {
            return None;
        }
        // Keep 55 bits (53 + guard + round) and a sticky bit for the rest.
        let shift = bits - 55;
        let top = shr_mag(&self.mag, shift as usize);
        let mut m = top.first().copied().unwrap_or(0) as u64
            | (top.get(1).copied().unwrap_or(0) as u64) << 32;
        let sticky = {
            let words = (shift / 32) as usize;
            let rem = shift % 32;
            self.mag[..words].iter().any(|&w| w != 0)
                || (rem > 0 && self.mag[words] & ((1u32 << rem) - 1) != 0)
        };
        if sticky {
            m |= 1;
        }
        // m has 55 significant bits; round to 53 with half-even.
        let low = m & 3;
        let mut mant = m >> 2;
        if low > 2 || (low == 2 && (mant & 1) == 1) {
            mant += 1;
        }
        let mut exp = shift as i32 + 2;
        if mant == 1 << 53 {
            mant >>= 1;
            exp += 1;
        }
        let v = mant as f64 * 2f64.powi(exp);
        if v.is_infinite() {
            return None;
        }
        Some(if self.neg { -v } else { v })
    }
    /// Truncates toward zero. Caller guarantees `f` is finite.
    pub fn from_f64(f: f64) -> Self {
        let neg = f < 0.0;
        let f = f.abs().trunc();
        if f < 1.8446744073709552e19 {
            return Self::make(neg, Self::from_u64(f as u64).mag);
        }
        let bits = f.to_bits();
        let exp = ((bits >> 52) & 0x7ff) as i64 - 1075;
        let mant = (bits & ((1 << 52) - 1)) | (1 << 52);
        let m = Self::from_u64(mant);
        let mag = shl_mag(&m.mag, exp as usize);
        Self::make(neg, mag)
    }
    pub fn neg(&self) -> Self {
        Self::make(!self.neg, self.mag.clone())
    }
    pub fn abs(&self) -> Self {
        Self::make(false, self.mag.clone())
    }
    pub fn add(&self, o: &Self) -> Self {
        if self.neg == o.neg {
            return Self::make(self.neg, add_mag(&self.mag, &o.mag));
        }
        match cmp_mag(&self.mag, &o.mag) {
            Ordering::Equal => Self::zero(),
            Ordering::Greater => Self::make(self.neg, sub_mag(&self.mag, &o.mag)),
            Ordering::Less => Self::make(o.neg, sub_mag(&o.mag, &self.mag)),
        }
    }
    pub fn sub(&self, o: &Self) -> Self {
        self.add(&o.neg())
    }
    pub fn mul(&self, o: &Self) -> Self {
        Self::make(self.neg != o.neg, mul_mag(&self.mag, &o.mag))
    }
    /// Truncating division (quotient toward zero). Caller checks for zero divisor.
    pub fn divrem_trunc(&self, o: &Self) -> (Self, Self) {
        let (q, r) = divrem_mag(&self.mag, &o.mag);
        (Self::make(self.neg != o.neg, q), Self::make(self.neg, r))
    }
    /// Python floor division and modulo.
    pub fn divmod_floor(&self, o: &Self) -> (Self, Self) {
        let (q, r) = self.divrem_trunc(o);
        if !r.is_zero() && (r.neg != o.neg) {
            (q.sub(&Self::from_i64(1)), r.add(o))
        } else {
            (q, r)
        }
    }
    pub fn pow(&self, mut e: u64) -> Self {
        let mut base = self.clone();
        let mut acc = Self::from_i64(1);
        while e > 0 {
            if e & 1 == 1 {
                acc = acc.mul(&base);
            }
            e >>= 1;
            if e > 0 {
                base = base.mul(&base);
            }
        }
        acc
    }
    pub fn shl(&self, bits: u64) -> Self {
        Self::make(self.neg, shl_mag(&self.mag, bits as usize))
    }
    /// Arithmetic (floor) shift right.
    pub fn shr(&self, bits: u64) -> Self {
        if !self.neg {
            return Self::make(false, shr_mag(&self.mag, bits as usize));
        }
        // floor(-m / 2^k) = -ceil(m / 2^k)
        let q = shr_mag(&self.mag, bits as usize);
        let back = shl_mag(&q, bits as usize);
        let exact = cmp_mag(&back, &self.mag) == Ordering::Equal;
        let q = if exact { q } else { add_mag(&q, &[1]) };
        Self::make(true, q)
    }
    /// Two's complement limbs of width `n` words (n large enough to hold sign).
    fn twos(&self, n: usize) -> Vec<u32> {
        let mut v = self.mag.clone();
        v.resize(n, 0);
        if self.neg {
            for w in v.iter_mut() {
                *w = !*w;
            }
            let mut carry = 1u64;
            for w in v.iter_mut() {
                let s = *w as u64 + carry;
                *w = s as u32;
                carry = s >> 32;
                if carry == 0 {
                    break;
                }
            }
        }
        v
    }
    fn from_twos(mut v: Vec<u32>) -> Self {
        let neg = v.last().is_some_and(|w| w & 0x8000_0000 != 0);
        if neg {
            for w in v.iter_mut() {
                *w = !*w;
            }
            let mut carry = 1u64;
            for w in v.iter_mut() {
                let s = *w as u64 + carry;
                *w = s as u32;
                carry = s >> 32;
                if carry == 0 {
                    break;
                }
            }
        }
        trim(&mut v);
        Self::make(neg, v)
    }
    pub fn bitop(&self, o: &Self, op: char) -> Self {
        let n = self.mag.len().max(o.mag.len()) + 1;
        let a = self.twos(n);
        let b = o.twos(n);
        let v = a
            .iter()
            .zip(b.iter())
            .map(|(x, y)| match op {
                '&' => x & y,
                '|' => x | y,
                _ => x ^ y,
            })
            .collect();
        Self::from_twos(v)
    }
    pub fn cmp(&self, o: &Self) -> Ordering {
        match (self.neg, o.neg) {
            (false, true) => Ordering::Greater,
            (true, false) => Ordering::Less,
            (false, false) => cmp_mag(&self.mag, &o.mag),
            (true, true) => cmp_mag(&o.mag, &self.mag),
        }
    }
    pub fn is_odd(&self) -> bool {
        self.mag.first().is_some_and(|w| w & 1 == 1)
    }
    pub fn to_str_radix(&self, radix: u32) -> String {
        if self.mag.is_empty() {
            return "0".into();
        }
        let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
        let mut out = Vec::new();
        if radix == 10 {
            // Peel 9 decimal digits per division.
            let mut cur = self.mag.clone();
            while !cur.is_empty() {
                let (q, mut r) = divrem_small(&cur, 1_000_000_000);
                cur = q;
                for _ in 0..9 {
                    out.push(digits[(r % 10) as usize]);
                    r /= 10;
                    if cur.is_empty() && r == 0 {
                        break;
                    }
                }
            }
        } else {
            let mut cur = self.mag.clone();
            while !cur.is_empty() {
                let (q, r) = divrem_small(&cur, radix);
                cur = q;
                out.push(digits[r as usize]);
            }
        }
        while out.len() > 1 && out.last() == Some(&b'0') {
            out.pop();
        }
        if self.neg {
            out.push(b'-');
        }
        out.reverse();
        String::from_utf8(out).unwrap_or_default()
    }
    /// Parses digits (no sign, no prefix, no underscores) in `radix`.
    pub fn parse_digits(s: &str, radix: u32) -> Option<Self> {
        if s.is_empty() {
            return None;
        }
        let mut mag: Vec<u32> = vec![];
        for ch in s.chars() {
            let d = ch.to_digit(radix)?;
            // mag = mag * radix + d
            let mut carry = d as u64;
            for w in mag.iter_mut() {
                let t = *w as u64 * radix as u64 + carry;
                *w = t as u32;
                carry = t >> 32;
            }
            if carry != 0 {
                mag.push(carry as u32);
            }
        }
        trim(&mut mag);
        Some(Self::make(false, mag))
    }
    /// Integer square root of a non-negative value.
    pub fn isqrt(&self) -> Self {
        if self.is_zero() {
            return Self::zero();
        }
        // Newton iteration starting above the root.
        let bits = self.bit_length();
        let mut x = Self::from_i64(1).shl(bits.div_ceil(2));
        loop {
            let (q, _) = self.divrem_trunc(&x);
            let y = x.add(&q).shr(1);
            if y.cmp(&x) != Ordering::Less {
                return x;
            }
            x = y;
        }
    }
    /// Lowest 64 bits of the magnitude (for hashing).
    pub fn limbs(&self) -> &[u32] {
        &self.mag
    }
    pub fn to_bytes_le(&self, len: usize, signed: bool) -> Option<Vec<u8>> {
        if self.neg && !signed {
            return None;
        }
        let words = len.div_ceil(4) + 1;
        let tw = self.twos(words.max(self.mag.len() + 1));
        let mut bytes: Vec<u8> = tw.iter().flat_map(|w| w.to_le_bytes()).collect();
        // Check the value fits.
        let fill = if self.neg { 0xff } else { 0 };
        if bytes[len..].iter().any(|&b| b != fill) {
            return None;
        }
        if signed && len > 0 && ((bytes[len - 1] & 0x80 != 0) != self.neg) {
            return None;
        }
        if signed && len == 0 && !self.is_zero() {
            return None;
        }
        bytes.truncate(len);
        Some(bytes)
    }
    pub fn from_bytes_le(bytes: &[u8], signed: bool) -> Self {
        let mut words: Vec<u32> = bytes
            .chunks(4)
            .map(|c| {
                let mut b = [0u8; 4];
                b[..c.len()].copy_from_slice(c);
                u32::from_le_bytes(b)
            })
            .collect();
        let negative = signed && bytes.last().is_some_and(|b| b & 0x80 != 0);
        if negative {
            // Sign-extend the final partial word then decode two's complement.
            let pad = bytes.len() % 4;
            if pad != 0 {
                let last = words.len() - 1;
                words[last] |= !0u32 << (pad * 8);
            }
            words.push(u32::MAX);
            return Self::from_twos(words);
        }
        trim(&mut words);
        Self::make(false, words)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn p(s: &str) -> BigInt {
        let (neg, d) = s.strip_prefix('-').map(|d| (true, d)).unwrap_or((false, s));
        let v = BigInt::parse_digits(d, 10).unwrap();
        if neg {
            v.neg()
        } else {
            v
        }
    }
    #[test]
    fn arithmetic_round_trips() {
        let a = p("123456789012345678901234567890");
        let b = p("987654321098765432109876543210");
        assert_eq!(
            a.mul(&b).to_str_radix(10),
            "121932631137021795226185032733622923332237463801111263526900"
        );
        let (q, r) = b.divmod_floor(&a);
        assert_eq!(q.to_str_radix(10), "8");
        assert_eq!(r.to_str_radix(10), "9000000000900000000090");
        let (q, r) = p("-7").divmod_floor(&p("2"));
        assert_eq!((q.to_i64(), r.to_i64()), (Some(-4), Some(1)));
        assert_eq!(
            p("2").pow(100).to_str_radix(10),
            "1267650600228229401496703205376"
        );
        assert_eq!(p("-5").shr(1).to_i64(), Some(-3));
        assert_eq!(p("-6").bitop(&p("3"), '&').to_i64(), Some(2));
        assert_eq!(p("255").to_str_radix(16), "ff");
        assert_eq!(p("10").pow(30).isqrt().to_str_radix(10), "1000000000000000");
        assert_eq!(p("2").pow(1023).to_f64(), Some(2f64.powi(1023)));
    }
}
