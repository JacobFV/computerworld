//! The simulator against closed-form answers. Every tolerance is stated beside the check
//! and is tighter than what an engineer reading a plot could see.
use cw_eda::spice::{parse, Analysis, VT};

fn run(deck: &str) -> cw_eda::spice::SimResult {
    parse(deck)
        .unwrap_or_else(|e| panic!("{e}"))
        .run()
        .unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn rc_charging_follows_one_minus_e_to_the_minus_t_over_tau() {
    // R = 1 kΩ, C = 1 µF: tau = 1 ms. A 1 ns edge at t = 0.
    let r =
        run("rc\nV1 in 0 PULSE(0 1 0 1n 1n 1 2)\nR1 in out 1k\nC1 out 0 1u\n.tran 10u 5m\n.end");
    for t in [0.5e-3, 1e-3, 2e-3, 3e-3, 5e-3] {
        let expected = 1.0 - (-(t - 0.5e-9) / 1e-3f64).exp();
        let got = r.value_at("V(out)", t).unwrap();
        // Trapezoidal integration at h = 10 µs = tau/100: error below 1e-5 V.
        assert!((got - expected).abs() < 1e-5, "t={t}: {got} vs {expected}");
    }
    // The time constant read off the curve, as the plot's cursors do: the 63.2 % point.
    let target = 1.0 - (-1.0f64).exp();
    let s = r.signal("V(out)").unwrap();
    let i = s.re.iter().position(|v| *v >= target).unwrap();
    let (x0, x1, y0, y1) = (r.x[i - 1], r.x[i], s.re[i - 1], s.re[i]);
    let tau = x0 + (target - y0) * (x1 - x0) / (y1 - y0);
    assert!(
        (tau - 1e-3).abs() < 1e-3 * 0.001,
        "tau {tau} not within 0.1% of 1 ms"
    );
}

#[test]
fn backward_euler_also_converges_on_the_rc_curve() {
    let r = run("rc be\nV1 in 0 PULSE(0 1 0 1n 1n 1 2)\nR1 in out 1k\nC1 out 0 1u\n.options method=gear\n.tran 1u 2m\n.end");
    let got = r.value_at("V(out)", 1e-3).unwrap();
    let expected = 1.0 - (-1.0f64).exp();
    // First-order method at h = tau/1000: within 2e-4 V.
    assert!((got - expected).abs() < 2e-4, "{got} vs {expected}");
}

#[test]
fn rl_current_rises_with_l_over_r() {
    // L = 10 mH, R = 100 Ω: tau = 100 µs, final current 10 mA.
    let r = run("rl\nV1 in 0 PULSE(0 1 0 1n 1n 1 2)\nR1 in a 100\nL1 a 0 10m\n.tran 1u 500u\n.end");
    let got = -r.value_at("I(V1)", 100e-6).unwrap();
    let expected = 0.01 * (1.0 - (-1.0f64).exp());
    assert!((got - expected).abs() < 1e-7, "{got} vs {expected}");
    let il = r.value_at("I(L1)", 300e-6).unwrap();
    assert!((il - 0.01 * (1.0 - (-3.0f64).exp())).abs() < 1e-7, "{il}");
}

#[test]
fn series_rlc_resonates_at_one_over_two_pi_root_lc() {
    // R = 10 Ω, L = 10 mH, C = 1 µF: f0 = 1591.549 Hz, where the whole source appears on R.
    let f0 = 1.0 / (2.0 * std::f64::consts::PI * (10e-3f64 * 1e-6).sqrt());
    let r = run(&format!(
        "rlc\nV1 in 0 AC 1\nL1 in a 10m\nC1 a b 1u\nR1 b 0 10\n.ac lin 2001 {} {}\n.end",
        f0 * 0.5,
        f0 * 1.5
    ));
    let s = r.signal("V(b)").unwrap();
    let (peak, _) = (0..r.x.len())
        .map(|i| (i, s.magnitude(i)))
        .fold((0, 0.0), |a, b| if b.1 > a.1 { b } else { a });
    // Sweep spacing is f0/2000; the peak lies within one step (0.05 %) of f0.
    assert!(
        (r.x[peak] - f0).abs() <= f0 / 2000.0 * 1.01,
        "{} vs {f0}",
        r.x[peak]
    );
    let at =
        r.x.iter()
            .position(|f| (f - f0).abs() < 1e-6)
            .expect("f0 is on the grid");
    assert!(
        (s.magnitude(at) - 1.0).abs() < 1e-9,
        "|V(b)| at f0 = {}",
        s.magnitude(at)
    );
    assert!(s.phase_deg(at).abs() < 1e-6);
}

#[test]
fn rc_low_pass_is_three_db_down_at_one_over_two_pi_rc() {
    // R = 1 kΩ, C = 159.155 nF: fc = 1000.0 Hz.
    let c = 1.0 / (2.0 * std::f64::consts::PI * 1000.0 * 1000.0);
    let r = run(&format!(
        "lowpass\nV1 in 0 DC 0 AC 1\nR1 in out 1k\nC1 out 0 {c:e}\n.ac dec 100 10 100k\n.end"
    ));
    let s = r.signal("V(out)").unwrap();
    // Exactly at 1 kHz (on the decade grid): -3.0103 dB and -45°.
    let at = r.x.iter().position(|f| (f - 1000.0).abs() < 1e-6).unwrap();
    assert!(
        (s.db(at) + 3.010_299_956_639_812).abs() < 1e-6,
        "{}",
        s.db(at)
    );
    assert!((s.phase_deg(at) + 45.0).abs() < 1e-6, "{}", s.phase_deg(at));
    // The -3 dB crossing found by interpolating between sweep points: within 0.5 %.
    let i = (0..r.x.len()).find(|i| s.db(*i) < -3.010_3).unwrap();
    let (f1, f2, d1, d2) = (r.x[i - 1], r.x[i], s.db(i - 1), s.db(i));
    let fc = f1 + (-3.010_3 - d1) * (f2 - f1) / (d2 - d1);
    assert!((fc - 1000.0).abs() < 5.0, "fc {fc}");
    // Two decades up: -40 dB (to within 0.01 dB of the exact -40.0004).
    let hi =
        r.x.iter()
            .position(|f| (f - 100_000.0).abs() < 1e-3)
            .unwrap();
    assert!((s.db(hi) + 40.000_434).abs() < 0.01, "{}", s.db(hi));
}

/// Diode in series with a resistor: solve Vd = n Vt ln(Id/Is + 1), Id = (V - Vd)/R.
fn diode_drop(v: f64, r: f64, is: f64, n: f64) -> f64 {
    let mut vd = 0.6;
    for _ in 0..200 {
        let id = (v - vd) / r;
        vd = n * VT * (id / is + 1.0).ln();
    }
    vd
}

#[test]
fn diode_forward_drop_matches_the_shockley_equation() {
    let r =
        run("diode\nV1 in 0 5\nR1 in a 1k\nD1 a 0 dmod\n.model dmod D(IS=1e-14 N=1)\n.op\n.end");
    let expected = diode_drop(5.0, 1000.0, 1e-14, 1.0);
    let got = r.value_at("V(a)", 0.0).unwrap();
    // Newton to 1e-6 relative: the forward drop within 10 µV.
    assert!((got - expected).abs() < 1e-5, "{got} vs {expected}");
    assert!(
        (0.65..0.75).contains(&got),
        "a silicon diode drops about 0.7 V: {got}"
    );
}

#[test]
fn diode_dc_sweep_traces_the_characteristic() {
    let r = run("sweep\nV1 in 0 0\nR1 in a 100\nD1 a 0 dmod\n.model dmod D(IS=1e-12 N=1.5)\n.dc V1 0 5 0.5\n.end");
    assert_eq!(r.x.len(), 11);
    let s = r.signal("V(a)").unwrap();
    for (i, v) in r.x.iter().enumerate().skip(2) {
        let expected = diode_drop(*v, 100.0, 1e-12, 1.5);
        assert!(
            (s.re[i] - expected).abs() < 1e-5,
            "at {v}: {} vs {expected}",
            s.re[i]
        );
    }
}

#[test]
fn zener_breaks_down_at_its_rated_voltage() {
    let r = run(
        "zener\nV1 in 0 12\nR1 in k 1k\nD1 0 k dz\n.model dz D(IS=1e-14 BV=5.1 IBV=1m)\n.op\n.end",
    );
    let v = r.value_at("V(k)", 0.0).unwrap();
    // About 6.9 mA flows; the reverse drop is BV plus a few thermal voltages.
    assert!((5.1..5.25).contains(&v), "zener voltage {v}");
}

#[test]
fn common_emitter_bias_point_matches_hand_analysis() {
    // VCC = 12 V, RB = 1 MΩ to the base, RC = 2 kΩ, BF = 100, IS = 1e-14.
    let r = run(
        "ce\nVCC vcc 0 12\nRB vcc b 1meg\nRC vcc c 2k\nQ1 c b 0 qn\n.model qn NPN(IS=1e-14 BF=100)\n.op\n.end",
    );
    // Ib = (12 - Vbe)/1M, Ic = BF Ib, Vbe = Vt ln(Ic/IS + 1): solve by iteration.
    let mut vbe: f64 = 0.6;
    for _ in 0..200 {
        let ic = 100.0 * (12.0 - vbe) / 1e6;
        vbe = VT * (ic / 1e-14 + 1.0).ln();
    }
    let ic = 100.0 * (12.0 - vbe) / 1e6;
    let vc = 12.0 - 2000.0 * ic;
    let got_vbe = r.value_at("V(b)", 0.0).unwrap();
    let got_vc = r.value_at("V(c)", 0.0).unwrap();
    // Forward active: reverse-junction terms are below 1e-14 A. Within 0.1 %.
    assert!((got_vbe - vbe).abs() < 1e-5, "Vbe {got_vbe} vs {vbe}");
    assert!((got_vc - vc).abs() < vc * 1e-3, "Vc {got_vc} vs {vc}");
    assert!(ic > 1.1e-3 && ic < 1.2e-3);
}

#[test]
fn pnp_mirrors_the_npn_bias() {
    let r = run(
        "pnp\nVEE vee 0 -12\nRB vee b 1meg\nRC vee c 2k\nQ1 c b 0 qp\n.model qp PNP(IS=1e-14 BF=100)\n.op\n.end",
    );
    let npn = run(
        "npn\nVCC vcc 0 12\nRB vcc b 1meg\nRC vcc c 2k\nQ1 c b 0 qn\n.model qn NPN(IS=1e-14 BF=100)\n.op\n.end",
    );
    let (vp, vn) = (
        r.value_at("V(c)", 0.0).unwrap(),
        npn.value_at("V(c)", 0.0).unwrap(),
    );
    assert!((vp + vn).abs() < 1e-9, "{vp} vs {vn}");
}

#[test]
fn early_voltage_raises_collector_current_with_vce() {
    let deck = |vce: f64| {
        format!("early\nVB b 0 0.65\nVC c 0 {vce}\nQ1 c b 0 qn\n.model qn NPN(IS=1e-14 BF=100 VAF=50)\n.op\n.end")
    };
    let i1 = -run(&deck(2.0)).value_at("I(VC)", 0.0).unwrap();
    let i2 = -run(&deck(12.0)).value_at("I(VC)", 0.0).unwrap();
    // Ic ∝ (1 - Vbc/VAF): the ratio is (1 + 11.35/50)/(1 + 1.35/50).
    let expected = (1.0 + 11.35 / 50.0) / (1.0 + 1.35 / 50.0);
    assert!(
        ((i2 / i1) - expected).abs() < 1e-6,
        "{} vs {expected}",
        i2 / i1
    );
}

#[test]
fn mosfet_follows_the_square_law() {
    // KP = 20 µA/V², W/L = 10: beta = 200 µA/V². VTO = 1 V.
    let base = "nmos\nVG g 0 3\nVD d 0 {vd}\nM1 d g 0 0 nm W=100u L=10u\n.model nm NMOS(VTO=1 KP=20u)\n.op\n.end";
    let sat = -run(&base.replace("{vd}", "5"))
        .value_at("I(VD)", 0.0)
        .unwrap();
    // Saturation: beta/2 (Vgs - Vt)^2 = 400 µA.
    assert!((sat - 4e-4).abs() < 1e-9, "{sat}");
    let lin = -run(&base.replace("{vd}", "0.5"))
        .value_at("I(VD)", 0.0)
        .unwrap();
    // Triode: beta ((Vgs - Vt) Vds - Vds^2/2) = 175 µA.
    assert!((lin - 1.75e-4).abs() < 1e-9, "{lin}");
    let pmos = run("pmos\nVS s 0 5\nVG g 0 2\nVD d 0 0\nM1 d g s s pm W=100u L=10u\n.model pm PMOS(VTO=-1 KP=20u)\n.op\n.end");
    let ip = pmos.value_at("I(VD)", 0.0).unwrap();
    assert!((ip - 4e-4).abs() < 1e-9, "PMOS drain current {ip}");
}

#[test]
fn inverting_op_amp_gain_is_minus_rf_over_rin() {
    // An op-amp macro-model: a VCVS with open-loop gain 1e6 inside a subcircuit.
    let r = run(
        "inverting\n.subckt opamp inp inn out\nE1 out 0 inp inn 1e6\n.ends\nV1 in 0 DC 0.1 AC 1\nR1 in inv 1k\nR2 inv out 10k\nX1 0 inv out opamp\n.op\n.end",
    );
    let a = 1e6;
    let ideal = -10.0;
    let expected = ideal / (1.0 + 11.0 / a);
    let got = r.value_at("V(out)", 0.0).unwrap() / 0.1;
    // Finite gain moves the answer by 11 ppm; the simulator must agree to 1e-9.
    assert!((got - expected).abs() < 1e-9, "{got} vs {expected}");
    let ac = parse(
        "inverting\n.subckt opamp inp inn out\nE1 out 0 inp inn 1e6\n.ends\nV1 in 0 DC 0 AC 1\nR1 in inv 1k\nR2 inv out 10k\nX1 0 inv out opamp\n.ac dec 1 100 1k\n.end",
    )
    .unwrap()
    .run()
    .unwrap();
    let s = ac.signal("V(out)").unwrap();
    assert!((s.db(0) - 20.0 * expected.abs().log10()).abs() < 1e-9);
    assert!((s.phase_deg(0).abs() - 180.0).abs() < 1e-9);
}

#[test]
fn sine_source_drives_the_expected_waveform() {
    let r = run("sine\nV1 a 0 SIN(1 2 1k)\nR1 a 0 1k\n.tran 10u 2m\n.end");
    // Every reported point is an accepted time point, where no interpolation is involved.
    let s = r.signal("V(a)").unwrap();
    assert!(r.x.len() > 200, "{} points", r.x.len());
    for (t, got) in r.x.iter().zip(&s.re) {
        let expected = 1.0 + 2.0 * (2.0 * std::f64::consts::PI * 1000.0 * t).sin();
        assert!((got - expected).abs() < 1e-9, "{t}: {got} vs {expected}");
    }
    // No step is longer than TSTEP (SPICE's default TMAX) and the run ends on TSTOP.
    assert!(r.x.windows(2).all(|w| w[1] - w[0] <= 10e-6 * (1.0 + 1e-9)));
    assert_eq!(*r.x.last().unwrap(), 2e-3);
}

#[test]
fn controlled_sources_f_g_h_stamp_correctly() {
    let r = run(
        "ctrl\nV1 a 0 2\nR1 a b 1k\nVS b 0 0\nF1 0 f VS 3\nRF f 0 1k\nG1 0 g a 0 1m\nRG g 0 1k\nH1 h 0 VS 500\n.op\n.end",
    );
    // I(VS) = 2 mA; F copies 3x into RF: 6 V. G: 1 mS * 2 V into 1k: 2 V. H: 500 * 2 mA = 1 V.
    assert!((r.value_at("V(f)", 0.0).unwrap() - 6.0).abs() < 1e-7);
    assert!((r.value_at("V(g)", 0.0).unwrap() - 2.0).abs() < 1e-7);
    assert!((r.value_at("V(h)", 0.0).unwrap() - 1.0).abs() < 1e-7);
}

#[test]
fn a_half_wave_rectifier_charges_its_reservoir() {
    let r = run(
        "rectifier\nV1 a 0 SIN(0 10 50)\nD1 a b dmod\nC1 b 0 100u\nR1 b 0 10k\n.model dmod D(IS=1e-14 CJO=10p TT=5n)\n.tran 100u 60m\n.end",
    );
    let peak = r
        .signal("V(b)")
        .unwrap()
        .re
        .iter()
        .cloned()
        .fold(0.0, f64::max);
    // The reservoir charges to the peak less one diode drop.
    assert!((9.0..9.5).contains(&peak), "peak {peak}");
}

#[test]
fn broken_decks_are_refused_with_a_reason() {
    for (deck, needle) in [
        ("t\nR1 a 0 0\n.op\n.end", "zero resistance"),
        ("t\nD1 a 0 missing\n.op\n.end", "unknown model"),
        ("t\nV1 a 0 1\nV2 a 0 2\n.op\n.end", ""),
        ("t\n.include foo.lib\n.end", "cannot be read"),
        (
            "t\nQ1 c b e qn\n.model qn NPN\nZ1 a b\n.end",
            "not supported",
        ),
    ] {
        match parse(deck).and_then(|c| c.run()) {
            Ok(_) => panic!("{deck} should fail"),
            Err(e) => assert!(e.contains(needle), "{e}"),
        }
    }
}

#[test]
fn analysis_cards_round_trip_through_the_parser() {
    for a in [
        Analysis::Op,
        Analysis::Tran {
            tstep: 1e-5,
            tstop: 5e-3,
            tstart: 0.0,
            tmax: None,
            uic: false,
        },
        Analysis::Ac {
            scale: "dec".into(),
            points: 10,
            fstart: 1.0,
            fstop: 1e6,
        },
        Analysis::Dc {
            source: "V1".into(),
            start: 0.0,
            stop: 5.0,
            step: 0.1,
        },
    ] {
        let deck = format!("t\nV1 a 0 1\nR1 a 0 1k\n{}\n.end", a.card());
        let parsed = parse(&deck).unwrap();
        let back = &parsed.analyses[0];
        match (back, &a) {
            (
                Analysis::Tran { tstep, tstop, .. },
                Analysis::Tran {
                    tstep: s, tstop: e, ..
                },
            ) => {
                assert!((tstep - s).abs() < s * 1e-9 && (tstop - e).abs() < e * 1e-9)
            }
            _ => assert_eq!(std::mem::discriminant(back), std::mem::discriminant(&a)),
        }
    }
}

#[test]
fn waveforms_are_bit_reproducible() {
    let deck = "rep\nV1 in 0 PULSE(0 5 0 1u 1u 1m 2m)\nR1 in a 1k\nD1 a b dmod\nC1 b 0 1u\nR2 b 0 10k\n.model dmod D(IS=1e-14)\n.tran 5u 4m\n.end";
    let a = run(deck);
    let b = run(deck);
    let bits = |r: &cw_eda::spice::SimResult| -> Vec<u64> {
        r.signals
            .iter()
            .flat_map(|s| s.re.iter().map(|v| v.to_bits()))
            .collect()
    };
    assert_eq!(bits(&a), bits(&b));
}
