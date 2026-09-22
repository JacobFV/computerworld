//! Semiconductor device equations, written after the Berkeley SPICE3 models they follow:
//! the junction diode, the Gummel–Poon bipolar transistor (the subset with high-level
//! injection, both Early voltages, leakage diodes, ohmic resistances, depletion and
//! transit-time charge) and the Shichman–Hodges level-1 MOSFET (body effect, channel-length
//! modulation, ohmic resistances, bulk junctions, Meyer gate capacitances plus overlaps).
//!
//! Every function works in the device's own polarity frame: a PNP's `vbe` is
//! `v(e) - v(b)` read the other way round, so one set of equations serves both types.
//! Arithmetic goes through `crate::num` so results are bit-identical on every host.
use crate::num::{exp, ln, pow};

/// Thermal voltage kT/q at 27 °C, as SPICE uses by default.
pub const VT: f64 = 0.025_852_017_444_120_28;
/// Conductance SPICE puts across every junction.
pub const GMIN: f64 = 1e-12;
/// Permittivity of silicon dioxide, F/m (3.9 ε0), as SPICE3 computes the oxide capacitance.
const EPS_OX: f64 = 3.9 * 8.854_214_871e-12;

#[derive(Clone, Debug, PartialEq)]
pub struct DiodeModel {
    pub is: f64,
    pub n: f64,
    pub rs: f64,
    pub bv: Option<f64>,
    pub ibv: f64,
    pub cjo: f64,
    pub vj: f64,
    pub m: f64,
    pub tt: f64,
    pub fc: f64,
}
impl Default for DiodeModel {
    fn default() -> Self {
        Self {
            is: 1e-14,
            n: 1.0,
            rs: 0.0,
            bv: None,
            ibv: 1e-3,
            cjo: 0.0,
            vj: 1.0,
            m: 0.5,
            tt: 0.0,
            fc: 0.5,
        }
    }
}

/// Gummel–Poon parameters (SPICE names). `None` for an Early voltage or knee current is
/// SPICE's "infinite" default.
#[derive(Clone, Debug, PartialEq)]
pub struct BjtModel {
    pub pnp: bool,
    pub is: f64,
    pub bf: f64,
    pub nf: f64,
    pub vaf: Option<f64>,
    pub ikf: Option<f64>,
    pub ise: f64,
    pub ne: f64,
    pub br: f64,
    pub nr: f64,
    pub var: Option<f64>,
    pub ikr: Option<f64>,
    pub isc: f64,
    pub nc: f64,
    pub rb: f64,
    pub re: f64,
    pub rc: f64,
    pub cje: f64,
    pub vje: f64,
    pub mje: f64,
    pub cjc: f64,
    pub vjc: f64,
    pub mjc: f64,
    pub tf: f64,
    pub tr: f64,
    pub fc: f64,
}
impl Default for BjtModel {
    fn default() -> Self {
        Self {
            pnp: false,
            is: 1e-16,
            bf: 100.0,
            nf: 1.0,
            vaf: None,
            ikf: None,
            ise: 0.0,
            ne: 1.5,
            br: 1.0,
            nr: 1.0,
            var: None,
            ikr: None,
            isc: 0.0,
            nc: 2.0,
            rb: 0.0,
            re: 0.0,
            rc: 0.0,
            cje: 0.0,
            vje: 0.75,
            mje: 0.33,
            cjc: 0.0,
            vjc: 0.75,
            mjc: 0.33,
            tf: 0.0,
            tr: 0.0,
            fc: 0.5,
        }
    }
}

/// Level-1 MOSFET parameters (SPICE names).
#[derive(Clone, Debug, PartialEq)]
pub struct MosModel {
    pub pmos: bool,
    pub vto: f64,
    pub kp: f64,
    pub gamma: f64,
    pub phi: f64,
    pub lambda: f64,
    pub rd: f64,
    pub rs: f64,
    pub cbd: f64,
    pub cbs: f64,
    pub is: f64,
    pub pb: f64,
    pub mj: f64,
    pub fc: f64,
    pub cgso: f64,
    pub cgdo: f64,
    pub cgbo: f64,
    /// Oxide thickness; without it there is no Meyer gate capacitance, as in SPICE.
    pub tox: Option<f64>,
    pub ld: f64,
}
impl Default for MosModel {
    fn default() -> Self {
        Self {
            pmos: false,
            vto: 0.0,
            kp: 2e-5,
            gamma: 0.0,
            phi: 0.6,
            lambda: 0.0,
            rd: 0.0,
            rs: 0.0,
            cbd: 0.0,
            cbs: 0.0,
            is: 1e-14,
            pb: 0.8,
            mj: 0.5,
            fc: 0.5,
            cgso: 0.0,
            cgdo: 0.0,
            cgbo: 0.0,
            tox: None,
            ld: 0.0,
        }
    }
}
impl MosModel {
    /// Gate oxide capacitance per unit area, F/m².
    pub fn cox(&self) -> f64 {
        match self.tox {
            Some(t) if t > 0.0 => EPS_OX / t,
            _ => 0.0,
        }
    }
    /// Effective channel length.
    pub fn leff(&self, l: f64) -> f64 {
        (l - 2.0 * self.ld).max(l * 1e-3)
    }
}

/// SPICE3 `pnjlim`: keep a junction voltage from jumping so far up the exponential that
/// the next linearisation is useless.
pub fn pnjlim(vnew: f64, vold: f64, vt: f64, vcrit: f64) -> f64 {
    if vnew > vcrit && (vnew - vold).abs() > vt + vt {
        if vold > 0.0 {
            let arg = 1.0 + (vnew - vold) / vt;
            if arg > 0.0 {
                vold + vt * ln(arg)
            } else {
                vcrit
            }
        } else {
            vt * ln(vnew / vt)
        }
    } else {
        vnew
    }
}
pub fn vcrit(vt: f64, is: f64) -> f64 {
    vt * ln(vt / (std::f64::consts::SQRT_2 * is.max(1e-300)))
}

/// Ideal junction current and conductance, with SPICE's cubic reverse region.
fn junction(is: f64, vt: f64, v: f64) -> (f64, f64) {
    if v >= -3.0 * vt {
        let e = exp(v / vt);
        (is * (e - 1.0), is * e / vt)
    } else {
        let a = 3.0 * vt / (v * std::f64::consts::E);
        let a3 = a * a * a;
        (-is * (1.0 + a3), is * 3.0 * a3 / v)
    }
}

/// Diode current and conductance at junction voltage `vd`.
pub fn diode_iv(m: &DiodeModel, area: f64, vd: f64) -> (f64, f64) {
    let vt = VT * m.n;
    let (mut id, mut gd) = junction(m.is * area, vt, vd);
    if let Some(bv) = m.bv {
        if vd < -bv + 40.0 * vt {
            let e = exp(-(bv + vd) / vt);
            id -= m.ibv * area * e;
            gd += m.ibv * area * e / vt;
        }
    }
    (id + GMIN * vd, gd + GMIN)
}

/// Depletion charge and capacitance of a junction: `cj0 / (1 - v/vj)^m` below
/// `fc * vj`, SPICE's linear extension above it.
pub fn depletion(cj0: f64, vj: f64, m: f64, fc: f64, v: f64) -> (f64, f64) {
    if cj0 == 0.0 {
        return (0.0, 0.0);
    }
    let knee = fc * vj;
    if v < knee {
        let arg = 1.0 - v / vj;
        let s = pow(arg, -m);
        (cj0 * vj * (1.0 - arg * s) / (1.0 - m), cj0 * s)
    } else {
        // Charge at the knee, then the tangent-continued capacitance integrated.
        let arg = 1.0 - fc;
        let s = pow(arg, -m);
        let q0 = cj0 * vj * (1.0 - arg * s) / (1.0 - m);
        let f1 = pow(1.0 - fc, -(1.0 + m));
        let c0 = 1.0 - fc * (1.0 + m);
        let dq = cj0 * f1 * (c0 * (v - knee) + m / (2.0 * vj) * (v * v - knee * knee));
        (q0 + dq, cj0 * f1 * (c0 + m * v / vj))
    }
}

pub fn diode_charge(m: &DiodeModel, area: f64, vd: f64) -> (f64, f64) {
    let (id, gd) = diode_iv(m, area, vd);
    let (qj, cj) = depletion(m.cjo * area, m.vj, m.m, m.fc, vd);
    (m.tt * id + qj, m.tt * gd + cj)
}

/// One Gummel–Poon operating point: terminal currents of the intrinsic transistor and
/// their derivatives, plus the base-emitter and base-collector charges.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BjtPoint {
    /// Collector current (into the collector, out of the emitter).
    pub ic: f64,
    /// Base current (into the base, out of the emitter).
    pub ib: f64,
    /// dIc/dVbe, dIc/dVbc, dIb/dVbe, dIb/dVbc.
    pub gcbe: f64,
    pub gcbc: f64,
    pub gbbe: f64,
    pub gbbc: f64,
    /// Base-emitter charge (depletion + TF·Ibe/qb) and its derivatives.
    pub qbe: f64,
    pub cbe_be: f64,
    pub cbe_bc: f64,
    /// Base-collector charge (depletion + TR·Ibc).
    pub qbc: f64,
    pub cbc_bc: f64,
    /// Normalised base charge qb.
    pub qb: f64,
}

pub fn bjt_eval(m: &BjtModel, area: f64, vbe: f64, vbc: f64) -> BjtPoint {
    let is = m.is * area;
    let vtf = VT * m.nf;
    let vtr = VT * m.nr;
    let (ibe, gbe) = junction(is, vtf, vbe);
    let (ibc, gbc) = junction(is, vtr, vbc);
    let (ben, gben) = if m.ise > 0.0 {
        junction(m.ise * area, VT * m.ne, vbe)
    } else {
        (0.0, 0.0)
    };
    let (bcn, gbcn) = if m.isc > 0.0 {
        junction(m.isc * area, VT * m.nc, vbc)
    } else {
        (0.0, 0.0)
    };
    let inv = |v: Option<f64>| match v {
        Some(x) if x > 0.0 => 1.0 / x,
        _ => 0.0,
    };
    let (inv_vaf, inv_var) = (inv(m.vaf), inv(m.var));
    let (oik, oikr) = (inv(m.ikf.map(|x| x * area)), inv(m.ikr.map(|x| x * area)));
    // q1 models the Early effect; keep its denominator positive far outside the
    // operating region so a Newton excursion cannot flip the transistor inside out.
    let den = (1.0 - inv_vaf * vbc - inv_var * vbe).max(1e-3);
    let q1 = 1.0 / den;
    let clamped = 1.0 - inv_vaf * vbc - inv_var * vbe < 1e-3;
    let (dq1_be, dq1_bc) = if clamped {
        (0.0, 0.0)
    } else {
        (q1 * q1 * inv_var, q1 * q1 * inv_vaf)
    };
    let (qb, dqb_be, dqb_bc) = if oik == 0.0 && oikr == 0.0 {
        (q1, dq1_be, dq1_bc)
    } else {
        let q2 = oik * ibe + oikr * ibc;
        let arg = (1.0 + 4.0 * q2).max(0.0);
        let s = if arg > 0.0 { arg.sqrt() } else { 1.0 };
        let qb = q1 * (1.0 + s) / 2.0;
        (
            qb,
            dq1_be * (1.0 + s) / 2.0 + q1 * oik * gbe / s,
            dq1_bc * (1.0 + s) / 2.0 + q1 * oikr * gbc / s,
        )
    };
    let ict = (ibe - ibc) / qb;
    let ic = ict - ibc / m.br - bcn - GMIN * vbc;
    let ib = ibe / m.bf + ben + ibc / m.br + bcn + GMIN * vbe + GMIN * vbc;
    let gcbe = gbe / qb - ict / qb * dqb_be;
    let gcbc = -gbc / qb - ict / qb * dqb_bc - gbc / m.br - gbcn - GMIN;
    let gbbe = gbe / m.bf + gben + GMIN;
    let gbbc = gbc / m.br + gbcn + GMIN;
    // Charges.
    let (qje, cje) = depletion(m.cje * area, m.vje, m.mje, m.fc, vbe);
    let (qjc, cjc) = depletion(m.cjc * area, m.vjc, m.mjc, m.fc, vbc);
    let (qdf, cdf_be, cdf_bc) = if m.tf > 0.0 && vbe > 0.0 {
        let q = m.tf * ibe / qb;
        (
            q,
            m.tf * (gbe / qb - ibe / (qb * qb) * dqb_be),
            -m.tf * ibe / (qb * qb) * dqb_bc,
        )
    } else {
        (m.tf * ibe, m.tf * gbe, 0.0)
    };
    BjtPoint {
        ic,
        ib,
        gcbe,
        gcbc,
        gbbe,
        gbbc,
        qbe: qje + qdf,
        cbe_be: cje + cdf_be,
        cbe_bc: cdf_bc,
        qbc: qjc + m.tr * ibc,
        cbc_bc: cjc + m.tr * gbc,
        qb,
    }
}

/// Level-1 drain current in the forward frame (`vds >= 0`): (id, gm, gds, gmbs, von,
/// vdsat).
pub fn mos_forward(
    m: &MosModel,
    w: f64,
    l: f64,
    vgs: f64,
    vds: f64,
    vbs: f64,
) -> (f64, f64, f64, f64, f64, f64) {
    let beta = m.kp * w / m.leff(l);
    let vto = if m.pmos { -m.vto } else { m.vto };
    let phi = m.phi.max(0.1);
    let sphi = phi.sqrt();
    // SPICE3's square root of the surface potential, linearised for forward body bias.
    let sarg = if vbs <= 0.0 {
        (phi - vbs).sqrt()
    } else {
        (sphi - vbs / (2.0 * sphi)).max(0.0)
    };
    let von = vto + m.gamma * (sarg - sphi);
    let vgst = vgs - von;
    let vdsat = vgst.max(0.0);
    if vgst <= 0.0 {
        return (0.0, 0.0, 0.0, 0.0, von, vdsat);
    }
    // dVon/dVbs = -gamma / (2 sarg): the body transconductance is gm times that.
    let arg = if sarg > 0.0 {
        m.gamma / (2.0 * sarg)
    } else {
        0.0
    };
    let betap = beta * (1.0 + m.lambda * vds);
    if vgst <= vds {
        let id = betap * vgst * vgst / 2.0;
        let gm = betap * vgst;
        let gds = m.lambda * beta * vgst * vgst / 2.0;
        (id, gm, gds, gm * arg, von, vdsat)
    } else {
        let id = betap * vds * (vgst - vds / 2.0);
        let gm = betap * vds;
        let gds = betap * (vgst - vds) + m.lambda * beta * vds * (vgst - vds / 2.0);
        (id, gm, gds, gm * arg, von, vdsat)
    }
}

/// SPICE3 `DEVqmeyer`: the halves of the Meyer gate-source, gate-drain and gate-bulk
/// capacitances (SPICE sums this point's and the previous point's halves).
pub fn meyer(vgs: f64, vgd: f64, von: f64, vdsat: f64, phi: f64, cox: f64) -> (f64, f64, f64) {
    let vgst = vgs - von;
    if vgst <= -phi {
        (0.0, 0.0, cox / 2.0)
    } else if vgst <= -phi / 2.0 {
        (0.0, 0.0, -vgst * cox / (2.0 * phi))
    } else if vgst <= 0.0 {
        (
            vgst * cox / (1.5 * phi) + cox / 3.0,
            0.0,
            -vgst * cox / (2.0 * phi),
        )
    } else {
        let vds = vgs - vgd;
        if vdsat <= vds {
            (cox / 3.0, 0.0, 0.0)
        } else {
            let vddif = 2.0 * vdsat - vds;
            let vddif1 = vdsat - vds;
            let vddif2 = vddif * vddif;
            (
                cox * (1.0 - vddif1 * vddif1 / vddif2) / 3.0,
                cox * (1.0 - vdsat * vdsat / vddif2) / 3.0,
                0.0,
            )
        }
    }
}

/// Junction current of a MOSFET's bulk diode.
pub fn bulk_diode(is: f64, v: f64) -> (f64, f64) {
    let (i, g) = junction(is, VT, v);
    (i + GMIN * v, g + GMIN)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gummel_poon_reduces_to_ebers_moll_without_knees() {
        let m = BjtModel {
            is: 1e-15,
            bf: 100.0,
            ..Default::default()
        };
        let p = bjt_eval(&m, 1.0, 0.65, -2.0);
        let ibe = 1e-15 * ((0.65 / VT).exp() - 1.0);
        // The reverse junction and GMIN add picoamps; nothing else differs.
        assert!((p.ic - ibe).abs() < 3e-12, "{} vs {ibe}", p.ic);
        assert!((p.ib - ibe / 100.0).abs() < 3e-12);
    }
    #[test]
    fn derivatives_match_finite_differences() {
        let m = BjtModel {
            is: 1e-15,
            bf: 150.0,
            vaf: Some(60.0),
            var: Some(20.0),
            ikf: Some(10e-3),
            ikr: Some(1e-3),
            ise: 1e-14,
            isc: 1e-13,
            tf: 300e-12,
            ..Default::default()
        };
        let (vbe, vbc) = (0.72, -1.5);
        let p = bjt_eval(&m, 1.0, vbe, vbc);
        let d = 1e-7;
        let pe = bjt_eval(&m, 1.0, vbe + d, vbc);
        let pc = bjt_eval(&m, 1.0, vbe, vbc + d);
        let near = |a: f64, b: f64| (a - b).abs() <= 1e-4 * a.abs().max(b.abs()) + 1e-15;
        assert!(near((pe.ic - p.ic) / d, p.gcbe), "gcbe");
        assert!(near((pc.ic - p.ic) / d, p.gcbc), "gcbc");
        assert!(near((pe.ib - p.ib) / d, p.gbbe), "gbbe");
        assert!(near((pc.ib - p.ib) / d, p.gbbc), "gbbc");
        assert!(near((pe.qbe - p.qbe) / d, p.cbe_be), "cbe_be");
        assert!(near((pc.qbe - p.qbe) / d, p.cbe_bc), "cbe_bc");
        let mo = MosModel {
            vto: 0.8,
            kp: 1e-4,
            gamma: 0.5,
            lambda: 0.02,
            ..Default::default()
        };
        for (vgs, vds) in [(2.0, 0.3), (2.0, 3.0)] {
            let vbs = -1.0;
            let (id, gm, gds, gmbs, _, _) = mos_forward(&mo, 10e-6, 2e-6, vgs, vds, vbs);
            let f = |a: f64, b: f64, c: f64| mos_forward(&mo, 10e-6, 2e-6, a, b, c).0;
            assert!(near((f(vgs + d, vds, vbs) - id) / d, gm));
            assert!(near((f(vgs, vds + d, vbs) - id) / d, gds));
            assert!(near((f(vgs, vds, vbs + d) - id) / d, gmbs));
        }
    }
}
