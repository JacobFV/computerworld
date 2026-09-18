//! Device models, step control and mixed-signal simulation against closed-form answers.
//! Reference values are computed here with the host's `f64` functions, independently of
//! the simulator's own deterministic ones.
use cw_eda::netlist::NE555_SUBCKT;
use cw_eda::spice::{parse, SimResult, VT};

fn run(deck: &str) -> SimResult {
    parse(deck)
        .unwrap_or_else(|e| panic!("{e}"))
        .run()
        .unwrap_or_else(|e| panic!("{e}"))
}
fn near(got: f64, want: f64, rel: f64) -> bool {
    (got - want).abs() <= rel * want.abs()
}

// ---- Gummel–Poon BJT ---------------------------------------------------------------

#[test]
fn bjt_ohmic_resistances_add_their_drops_to_the_junction() {
    // 10 µA into the base: the intrinsic transistor carries BF·Ib, and the terminals
    // see the junction voltage plus Ib·RB and Ie·RE, and the collector loses Ic·RC.
    let r = run("rb re rc\nIB 0 b DC 10u\nVC c 0 5\nQ1 c b 0 qm\n.model qm npn(is=1e-15 bf=100 rb=100 re=2 rc=10)\n.op\n.end");
    let ib: f64 = 10e-6;
    let ic = 100.0 * ib;
    let vbe = VT * (ic / 1e-15 + 1.0).ln();
    let vb = vbe + ib * 100.0 + (ic + ib) * 2.0;
    let got = r.value_at("V(b)", 0.0).unwrap();
    assert!((got - vb).abs() < 1e-6, "V(b) {got} vs {vb}");
    let icol = -r.value_at("I(VC)", 0.0).unwrap();
    assert!(near(icol, ic, 1e-6), "Ic {icol} vs {ic}");
    // The internal collector node sits Ic·RC below the supply.
    let vci = r.value_at("V(Q1#collector)", 0.0);
    assert!(vci.is_none(), "internal nodes are not reported as signals");
}

/// Collector current of the intrinsic Gummel–Poon transistor, straight from the
/// equations (no resistances, no leakage), for the tests to compare with.
fn gp_ic(is: f64, br: f64, vaf: f64, var: f64, ikf: f64, vbe: f64, vbc: f64) -> (f64, f64) {
    let ibe = is * ((vbe / VT).exp() - 1.0);
    let ibc = is * ((vbc / VT).exp() - 1.0);
    let q1 = 1.0 / (1.0 - vbc / vaf - vbe / var);
    let q2 = ibe / ikf;
    let qb = q1 * (1.0 + (1.0 + 4.0 * q2).sqrt()) / 2.0;
    ((ibe - ibc) / qb - ibc / br, qb)
}

#[test]
fn bjt_high_injection_rolls_off_current_gain_by_qb() {
    for vbe in [0.6, 0.75, 0.85] {
        let r = run(&format!(
            "ikf\nVB b 0 {vbe}\nVC c 0 2\nQ1 c b 0 qm\n.model qm npn(is=1e-15 bf=100 ikf=10m)\n.op\n.end"
        ));
        let (ic, qb) = gp_ic(
            1e-15,
            1.0,
            f64::INFINITY,
            f64::INFINITY,
            10e-3,
            vbe,
            vbe - 2.0,
        );
        let got = -r.value_at("I(VC)", 0.0).unwrap();
        // GMIN across the base-collector junction adds 2 pA.
        assert!(
            (got - ic).abs() < 1e-7 * ic + 3e-12,
            "vbe {vbe}: {got} vs {ic}"
        );
        let ib = -r.value_at("I(VB)", 0.0).unwrap();
        let beta = got / ib;
        // GMIN on both junctions adds about a picoamp to the base current.
        assert!(
            near(beta, 100.0 / qb, 1e-5),
            "beta {beta} vs {}",
            100.0 / qb
        );
    }
}

#[test]
fn bjt_forward_and_reverse_early_voltages() {
    let (vbe, vce) = (0.7, 4.0);
    let r = run(&format!(
        "early\nVB b 0 {vbe}\nVC c 0 {vce}\nQ1 c b 0 qm\n.model qm npn(is=1e-15 bf=100 vaf=50 var=20)\n.op\n.end"
    ));
    let (ic, _) = gp_ic(1e-15, 1.0, 50.0, 20.0, f64::INFINITY, vbe, vbe - vce);
    let got = -r.value_at("I(VC)", 0.0).unwrap();
    assert!((got - ic).abs() < 1e-7 * ic + 5e-12, "{got} vs {ic}");
}

/// Capacitance seen by an AC source: -Im(I)/ω.
fn cap_of(r: &SimResult, source: &str) -> f64 {
    let s = r.signal(source).unwrap();
    -s.im[0] / (2.0 * std::f64::consts::PI * r.x[0])
}

#[test]
fn bjt_depletion_capacitances_follow_the_junction_formula() {
    // Base-emitter reverse biased by 2 V, collector tied to the base.
    let r = run("cje\nV1 b 0 DC -2 AC 1\nQ1 b b 0 qm\n.model qm npn(is=1e-15 cje=2p vje=0.8 mje=0.4)\n.ac lin 1 1Meg 1Meg\n.end");
    let want = 2e-12 / (1.0f64 + 2.0 / 0.8).powf(0.4);
    let got = cap_of(&r, "I(V1)");
    assert!(near(got, want, 1e-9), "Cje {got} vs {want}");
    // The base-collector one, with the emitter tied to the base.
    let r = run("cjc\nV1 b 0 DC -3 AC 1\nQ1 0 b b qm\n.model qm npn(is=1e-15 cjc=1p vjc=0.6 mjc=0.33)\n.ac lin 1 1Meg 1Meg\n.end");
    let want = 1e-12 / (1.0f64 + 3.0 / 0.6).powf(0.33);
    let got = cap_of(&r, "I(V1)");
    assert!(near(got, want, 1e-9), "Cjc {got} vs {want}");
    // Forward biased beyond FC·VJ the capacitance continues linearly (SPICE's rule).
    let r = run("fc\nV1 b 0 DC 0.6 AC 1\nQ1 b b 0 qm\n.model qm npn(is=1e-30 cje=2p vje=0.8 mje=0.5 fc=0.5)\n.ac lin 1 1Meg 1Meg\n.end");
    let f1 = (1.0f64 - 0.5).powf(-1.5);
    let want = 2e-12 * f1 * (1.0 - 0.5 * 1.5 + 0.5 * 0.6 / 0.8);
    assert!(near(cap_of(&r, "I(V1)"), want, 1e-6));
}

#[test]
fn bjt_transit_times_store_charge_in_proportion_to_current() {
    // Forward-active: the base-emitter diffusion capacitance is TF·gm.
    let vbe = 0.65;
    let r = run(&format!(
        "tf\nV1 b 0 DC {vbe} AC 1\nVC c 0 5\nQ1 c b 0 qm\n.model qm npn(is=1e-15 bf=100 tf=0.5n)\n.ac lin 1 1Meg 1Meg\n.end"
    ));
    let gm = 1e-15 * (vbe / VT).exp() / VT;
    let got = cap_of(&r, "I(V1)");
    assert!(near(got, 0.5e-9 * gm, 1e-6), "{got} vs {}", 0.5e-9 * gm);
    // Reverse-active: the base-collector one is TR·gbc.
    let r = run(&format!(
        "tr\nV1 b 0 DC {vbe} AC 1\nVE e 0 5\nQ1 0 b e qm\n.model qm npn(is=1e-15 bf=100 br=2 tr=40n)\n.ac lin 1 1Meg 1Meg\n.end"
    ));
    let got = cap_of(&r, "I(V1)");
    assert!(near(got, 40e-9 * gm, 1e-6), "{got} vs {}", 40e-9 * gm);
}

#[test]
fn transistor_parameters_come_from_symbol_properties() {
    use cw_eda::geom::{Pt, Xf};
    use cw_eda::schematic::Schematic;
    let mut s = Schematic::new("q");
    let q = s
        .place("Device:Q_NPN_BCE", Pt::new(1000, 1000), Xf::IDENTITY)
        .unwrap();
    s.symbol_mut(q)
        .unwrap()
        .set_field("Sim.Params", "is=2f bf=150 rb=50 cje=3p tf=300p ikf=20m");
    s.annotate(true, false);
    let deck = cw_eda::netlist::spice_netlist(&s, "q", Some(".op")).unwrap();
    assert!(
        deck.text
            .contains(".model __Q1 NPN(is=2f bf=150 rb=50 cje=3p tf=300p ikf=20m)"),
        "{}",
        deck.text
    );
    // A model defined on the sheet and named in Sim.Name is used as written.
    s.symbol_mut(q).unwrap().set_field("Sim.Name", "Q2N3904");
    s.add_text(
        Pt::new(500, 3000),
        ".model Q2N3904 NPN(IS=6.734f BF=416.4 VAF=74.03 IKF=66.78m RB=10 CJE=4.493p TF=301.2p)",
    );
    let deck = cw_eda::netlist::spice_netlist(&s, "q", Some(".op")).unwrap();
    assert!(deck.text.contains("Q1 "), "{}", deck.text);
    assert!(deck.text.contains(" Q2N3904\n"), "{}", deck.text);
    assert!(!deck.text.contains("__Q1"), "{}", deck.text);
    parse(&deck.text).unwrap();
}

// ---- Level-1 MOSFET ------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn mos_id(
    kp: f64,
    wl: f64,
    vto: f64,
    gamma: f64,
    phi: f64,
    lambda: f64,
    vgs: f64,
    vds: f64,
    vbs: f64,
) -> f64 {
    let vt = vto + gamma * ((phi - vbs).sqrt() - phi.sqrt());
    let vgst = vgs - vt;
    if vgst <= 0.0 {
        return 0.0;
    }
    let beta = kp * wl;
    if vds >= vgst {
        beta / 2.0 * vgst * vgst * (1.0 + lambda * vds)
    } else {
        beta * vds * (vgst - vds / 2.0) * (1.0 + lambda * vds)
    }
}

#[test]
fn mosfet_threshold_rises_with_source_bulk_reverse_bias() {
    for (vbs, vds) in [(0.0, 5.0), (-1.0, 5.0), (-3.0, 5.0), (-2.0, 0.5)] {
        let r = run(&format!(
            "body\nVD d 0 {vds}\nVG g 0 3\nVB b 0 {vbs}\nM1 d g 0 b nm W=10u L=2u\n.model nm nmos(vto=0.8 kp=100u gamma=0.5 phi=0.7 lambda=0.02)\n.op\n.end"
        ));
        let want = mos_id(100e-6, 5.0, 0.8, 0.5, 0.7, 0.02, 3.0, vds, vbs);
        let got = -r.value_at("I(VD)", 0.0).unwrap();
        // GMIN across drain-source and the reverse bulk junction add picoamps.
        assert!(
            (got - want).abs() < 1e-9 * want + 2e-11,
            "vbs {vbs} vds {vds}: {got} vs {want}"
        );
    }
    // PMOS mirrors it.
    let r = run("pbody\nVD d 0 -5\nVG g 0 -3\nVB b 0 2\nM1 d g 0 b pm W=10u L=2u\n.model pm pmos(vto=-0.8 kp=40u gamma=0.4 phi=0.65)\n.op\n.end");
    let want = mos_id(40e-6, 5.0, 0.8, 0.4, 0.65, 0.0, 3.0, 5.0, -2.0);
    let got = r.value_at("I(VD)", 0.0).unwrap();
    assert!((got - want).abs() < 1e-9 * want + 2e-11, "{got} vs {want}");
}

#[test]
fn mosfet_meyer_and_overlap_capacitances() {
    let (w, l, tox) = (10e-6, 2e-6, 20e-9);
    let cox = 3.9 * 8.854_214_871e-12 / tox * w * l;
    let model = ".model nm nmos(vto=0.8 kp=100u tox=20n cgso=0.2n cgdo=0.3n cgbo=0.1n)";
    // Saturation: two thirds of the oxide capacitance to the source.
    let r = run(&format!(
        "sat\nVG g 0 DC 3 AC 1\nVD d 0 5\nM1 d g 0 0 nm W=10u L=2u\n{model}\n.ac lin 1 1Meg 1Meg\n.end"
    ));
    let want = 2.0 / 3.0 * cox + 0.2e-9 * w + 0.3e-9 * w + 0.1e-9 * l;
    let got = cap_of(&r, "I(VG)");
    assert!(near(got, want, 1e-9), "saturation {got} vs {want}");
    // Accumulation (far below threshold): the whole oxide to the bulk.
    let r = run(&format!(
        "acc\nVG g 0 DC -5 AC 1\nVD d 0 5\nM1 d g 0 0 nm W=10u L=2u\n{model}\n.ac lin 1 1Meg 1Meg\n.end"
    ));
    let want = cox + 0.2e-9 * w + 0.3e-9 * w + 0.1e-9 * l;
    let got = cap_of(&r, "I(VG)");
    assert!(near(got, want, 1e-9), "cutoff {got} vs {want}");
    // Deep triode (vds = 0): half the oxide to each of source and drain.
    let r = run(&format!(
        "lin\nVG g 0 DC 4 AC 1\nVD d 0 0\nM1 d g 0 0 nm W=10u L=2u\n{model}\n.ac lin 1 1Meg 1Meg\n.end"
    ));
    let want = cox + 0.2e-9 * w + 0.3e-9 * w + 0.1e-9 * l;
    assert!(near(cap_of(&r, "I(VG)"), want, 1e-9));
    // The same gate capacitance charges through a resistor with time constant R·C.
    let r = run(&format!(
        "rc\nV1 in 0 PULSE(-6 -5 0 1p 1p 1 2)\nR1 in g 100k\nVD d 0 5\nM1 d g 0 0 nm W=10u L=2u\n{model}\n.tran 0.1n 2u\n.end"
    ));
    let c = cox + 0.2e-9 * w + 0.3e-9 * w + 0.1e-9 * l;
    let tau = 100e3 * c;
    for k in [0.5, 1.0, 2.0] {
        let v = r.value_at("V(g)", k * tau).unwrap();
        let want = -6.0 + (1.0 - (-k).exp());
        assert!((v - want).abs() < 2e-3, "t = {k} tau: {v} vs {want}");
    }
}

// ---- Truncation-error step control ------------------------------------------------------

fn rc_run(options: &str) -> SimResult {
    run(&format!(
        "lte\nV1 in 0 PULSE(0 1 0 1n 1n 1 2)\nR1 in out 1k\nC1 out 0 1u\n{options}\n.tran 1u 8m 0 1m\n.end"
    ))
}
fn rc_error(r: &SimResult) -> f64 {
    let s = r.signal("V(out)").unwrap();
    r.x.iter()
        .zip(&s.re)
        .map(|(t, v)| {
            let want = if *t <= 1e-9 {
                0.0
            } else {
                1.0 - (-(t - 0.5e-9) / 1e-3).exp()
            };
            (v - want).abs()
        })
        .fold(0.0, f64::max)
}

#[test]
fn truncation_error_chooses_the_steps() {
    // TMAX of 1 ms lets the step grow as the curve flattens: far fewer points than a
    // fixed 1 µs step, with the error within the tolerance.
    let r = rc_run("");
    let n = r.x.len();
    assert!(n < 400, "{n} points");
    let err = rc_error(&r);
    // SPICE3's criterion (TRTOL = 7 over RELTOL) keeps a 1 V step within about 2 %.
    assert!(err < 2e-2, "max error {err}");
    let steps: Vec<f64> = r.x.windows(2).map(|w| w[1] - w[0]).collect();
    let early = steps[steps.len() / 10];
    let late = steps[steps.len() - 2];
    assert!(late > 10.0 * early, "steps grow: {early} then {late}");
    assert!(
        steps.iter().all(|h| *h <= 1e-3 * (1.0 + 1e-12)),
        "TMAX holds"
    );
    // A tighter RELTOL buys accuracy with more points.
    let fine = rc_run(".options reltol=1e-5");
    assert!(fine.x.len() > n, "{} vs {n}", fine.x.len());
    let fine_err = rc_error(&fine);
    assert!(fine_err < err / 5.0, "{fine_err} vs {err}");
    // A looser TRTOL cannot take more steps than the twofold growth already allows.
    let loose = rc_run(".options trtol=50");
    assert!(loose.x.len() <= n, "{} vs {n}", loose.x.len());
}

#[test]
fn waveform_corners_are_landed_on_exactly() {
    let r = run("bp\nV1 in 0 PULSE(0 1 1m 10u 20u 1m 3m)\nR1 in out 1k\nC1 out 0 100n\n.tran 10u 7m 0 1m\n.end");
    for corner in [
        1e-3, 1.01e-3, 2.01e-3, 2.03e-3, 4e-3, 4.01e-3, 5.01e-3, 5.03e-3,
    ] {
        let hit = r.x.iter().any(|t| (t - corner).abs() <= corner * 1e-12);
        assert!(hit, "corner {corner} not a time point");
    }
    // After each corner the integration restarts small and grows again.
    let at = r.x.iter().position(|t| (t - 4e-3).abs() < 1e-12).unwrap();
    assert!(r.x[at + 1] - r.x[at] < 5e-6, "{}", r.x[at + 1] - r.x[at]);
}

#[test]
fn adaptive_steps_are_bit_reproducible() {
    let deck = "rep\nV1 in 0 SIN(0 5 1k)\nR1 in a 100\nD1 a b dmod\nC1 b 0 10u\nR2 b 0 1k\n.model dmod D(IS=1e-14 CJO=5p TT=10n)\n.tran 10u 5m 0 100u\n.end";
    let (a, b) = (run(deck), run(deck));
    assert!(a.x.len() > 50, "{}", a.x.len());
    let bits = |r: &SimResult| -> Vec<u64> {
        r.x.iter()
            .chain(r.signals.iter().flat_map(|s| s.re.iter()))
            .map(|v| v.to_bits())
            .collect()
    };
    assert_eq!(bits(&a), bits(&b));
    assert_eq!(a.rejected, b.rejected);
}

// ---- Digital and mixed-signal ----------------------------------------------------------

const BRIDGES: &str = ".model adc adc_bridge(in_low=1.2 in_high=1.2)\n.model dac dac_bridge(out_low=0 out_high=3.3 t_rise=1n t_fall=1n)";

#[test]
fn logic_gates_follow_their_truth_tables() {
    let cases: [(&str, [u8; 4]); 6] = [
        ("d_and", [0, 0, 0, 1]),
        ("d_nand", [1, 1, 1, 0]),
        ("d_or", [0, 1, 1, 1]),
        ("d_nor", [1, 0, 0, 0]),
        ("d_xor", [0, 1, 1, 0]),
        ("d_xnor", [1, 0, 0, 1]),
    ];
    for (model, table) in cases {
        for (k, want) in table.iter().enumerate() {
            let (a, b) = ((k >> 1) & 1, k & 1);
            let r = run(&format!(
                "gate\nVA a 0 {}\nVB b 0 {}\nAin [a b] [da db] adc\nAg [da db] dy g\nAout [dy] [y] dac\nRL y 0 10k\n{BRIDGES}\n.model g {model}\n.op\n.end",
                a as f64 * 3.3,
                b as f64 * 3.3
            ));
            let y = r.value_at("V(y)", 0.0).unwrap();
            assert!(
                (y - *want as f64 * 3.3).abs() < 1e-6,
                "{model}({a},{b}) = {y}"
            );
        }
    }
    for (model, a, want) in [
        ("d_inverter", 0, 1),
        ("d_inverter", 1, 0),
        ("d_buffer", 1, 1),
    ] {
        let r = run(&format!(
            "one\nVA a 0 {}\nAin [a] [da] adc\nAg da dy g\nAout [dy] [y] dac\nRL y 0 10k\n{BRIDGES}\n.model g {model}\n.op\n.end",
            a as f64 * 3.3
        ));
        let y = r.value_at("V(y)", 0.0).unwrap();
        assert!((y - want as f64 * 3.3).abs() < 1e-6, "{model}({a}) = {y}");
    }
    // An inverted input (~) is read the other way.
    let r = run(&format!(
        "tilde\nVA a 0 3.3\nVB b 0 3.3\nAin [a b] [da db] adc\nAg [~da db] dy g\nAout [dy] [y] dac\nRL y 0 10k\n{BRIDGES}\n.model g d_and\n.op\n.end"
    ));
    assert!(r.value_at("V(y)", 0.0).unwrap().abs() < 1e-6);
}

#[test]
fn a_d_flip_flop_divides_its_clock_by_two() {
    // Q̄ fed back to D: Q toggles on every rising clock edge.
    let r = run(&format!(
        "div2\nVCLK clk 0 PULSE(0 3.3 1u 10n 10n 490n 1u)\nAin [clk] [dclk] adc\nAff dq dclk NULL NULL q nq ff\nAdq [nq] dq buf\nAout [q] [out] dac\nRL out 0 10k\n{BRIDGES}\n.model ff d_dff(clk_delay=5n ic=0)\n.model buf d_buffer(rise_delay=1n fall_delay=1n)\n.tran 10n 20u\n.end"
    ));
    let rises = r.crossings("V(out)", 1.65, true);
    assert!(rises.len() >= 8, "{rises:?}");
    for w in rises.windows(2) {
        assert!((w[1] - w[0] - 2e-6).abs() < 5e-9, "period {}", w[1] - w[0]);
    }
    // Each output edge follows a clock edge by the clock-to-Q delay (plus bridges).
    let clk_rises = r.crossings("V(clk)", 1.2, true);
    let first = rises[0];
    let before = clk_rises.iter().rev().find(|t| **t < first).unwrap();
    let lag = first - before;
    assert!(lag > 5e-9 && lag < 20e-9, "clock to Q {lag}");
    // The digital nets are reported too.
    assert!(r.signal("D(q)").is_some());
}

#[test]
fn a_digital_oscillator_runs_at_its_frequency() {
    let r = run(&format!(
        "osc\nA1 0 clk osc\nAout [clk] [out] dac\nRL out 0 10k\n{BRIDGES}\n.model osc d_osc(cntl_array=[-1 1] freq_array=[25k 25k] duty_cycle=0.25 init_phase=0)\n.tran 100n 400u\n.end"
    ));
    let rises = r.crossings("V(out)", 1.65, true);
    let falls = r.crossings("V(out)", 1.65, false);
    assert!(rises.len() >= 8);
    for w in rises.windows(2) {
        assert!((w[1] - w[0] - 40e-6).abs() < 1e-9, "period {}", w[1] - w[0]);
    }
    let high = falls.iter().find(|t| **t > rises[1]).unwrap() - rises[1];
    assert!((high - 10e-6).abs() < 1e-9, "high for {high}");
}

fn ne555(r1: &str, r2: &str, c: &str, extra: &str) -> SimResult {
    run(&format!(
        "astable\nVCC vcc 0 5\nX1 0 cap out vcc ctrl cap disch vcc kicad_ne555\nR1 vcc disch {r1}\nR2 disch cap {r2}\nC1 cap 0 {c}\nC2 ctrl 0 10n\nRL out 0 10k\n{NE555_SUBCKT}\n{extra}\n.end"
    ))
}

#[test]
fn a_555_astable_runs_at_1_44_over_r1_plus_2r2_times_c() {
    for (r1, r2, c, r1v, r2v, cv, stop) in [
        ("1k", "10k", "100n", 1e3, 10e3, 100e-9, "12m"),
        ("4.7k", "4.7k", "10n", 4.7e3, 4.7e3, 10e-9, "2m"),
    ] {
        let r = ne555(r1, r2, c, &format!(".tran 1u {stop}"));
        let want = 1.44 / ((r1v + 2.0 * r2v) * cv);
        let rises = r.crossings("V(out)", 2.5, true);
        assert!(rises.len() >= 5, "{r1} {r2} {c}: {rises:?}\n{:?}", r.log);
        // Skip the first cycle, which starts from the power-up state.
        let periods: Vec<f64> = rises[1..].windows(2).map(|w| w[1] - w[0]).collect();
        let period = periods.iter().sum::<f64>() / periods.len() as f64;
        let f = 1.0 / period;
        assert!(
            (f - want).abs() < 0.02 * want,
            "{r1} {r2} {c}: {f} Hz vs {want} Hz"
        );
        // Every cycle alike, and the timing capacitor swings between ⅓ and ⅔ VCC.
        for p in &periods {
            assert!((p - period).abs() < 1e-3 * period, "{periods:?}");
        }
        let cap = r.signal("V(cap)").unwrap();
        let start = r.x.iter().position(|t| *t > rises[1]).unwrap();
        let hi = cap.re[start..].iter().cloned().fold(f64::MIN, f64::max);
        let lo = cap.re[start..].iter().cloned().fold(f64::MAX, f64::min);
        assert!(
            (hi - 10.0 / 3.0).abs() < 0.05 && (lo - 5.0 / 3.0).abs() < 0.05,
            "{lo}..{hi}"
        );
    }
}

#[test]
fn the_555_reset_pin_holds_the_output_low() {
    let r = run(&format!(
        "reset\nVCC vcc 0 5\nVR rst 0 0\nX1 0 cap out rst ctrl cap disch vcc kicad_ne555\nR1 vcc disch 1k\nR2 disch cap 10k\nC1 cap 0 100n\nRL out 0 10k\n{NE555_SUBCKT}\n.tran 1u 3m\n.end"
    ));
    let out = r.signal("V(out)").unwrap();
    assert!(out.re.iter().all(|v| *v < 0.1), "held in reset");
}
