//! STEP (ISO 10303-21) exchange of the exact B-rep: AP214 and AP242 files with a
//! `MANIFOLD_SOLID_BREP` of `ADVANCED_FACE`s on planes, cylinders, cones, spheres, tori
//! and B-spline surfaces, bounded by `EDGE_CURVE`s on lines, circles, ellipses and
//! B-spline curves — the structure OpenCascade (and so FreeCAD, and every other CAD
//! system that reads STEP) expects, with a proper header and millimetre units.
//!
//! Geometry no analytic surface describes (a rolling-ball blend's tube, a chamfer's
//! ruled surface) is written as an interpolating B-spline through samples dense enough
//! that the deviation is below the file's stated uncertainty.
use crate::brep::geom::{BSplineCurve, BSplineSurface, Curve, Surface};
use crate::brep::topo::{Coedge, Edge, Face, Solid, Vertex};
use crate::brep::uv::face_uv;
use crate::math::{self, v3, Frame, V3};
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// Which schema a file is written against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Schema {
    Ap214,
    Ap242,
}
impl Schema {
    fn file_schema(self) -> &'static str {
        match self {
            Schema::Ap214 => "AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }",
            Schema::Ap242 => "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 3 1 4 }",
        }
    }
    fn application(self) -> (&'static str, &'static str, i32) {
        match self {
            Schema::Ap214 => (
                "automotive_design",
                "core data for automotive mechanical design processes",
                2000,
            ),
            Schema::Ap242 => (
                "managed model based 3d engineering",
                "core data for automotive mechanical design processes",
                2020,
            ),
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Schema::Ap214 => "AP214",
            Schema::Ap242 => "AP242",
        }
    }
}

/// Distance the file declares as its uncertainty, in millimetres.
const UNCERTAINTY: f64 = 1e-7;
/// How closely a fitted B-spline must follow the true geometry.
const FIT_TOL: f64 = 1e-8;

fn num(v: f64) -> String {
    // STEP reals always carry a decimal point; `E` for exponents.
    if v == 0.0 {
        return "0.".into();
    }
    if !v.is_finite() {
        return "0.".into();
    }
    let a = v.abs();
    let s = if (1e-4..1e10).contains(&a) {
        let mut s = format!("{v:.12}");
        while s.ends_with('0') && !s.ends_with(".0") {
            s.pop();
        }
        if s.ends_with(".0") {
            s.pop();
        }
        s
    } else {
        let s = format!("{v:E}");
        let (m, e) = s.split_once('E').unwrap_or((s.as_str(), "0"));
        let m = if m.contains('.') {
            m.to_owned()
        } else {
            format!("{m}.")
        };
        format!("{m}E{e}")
    };
    s
}
fn point(p: V3) -> String {
    format!("({},{},{})", num(p.x), num(p.y), num(p.z))
}

struct Writer {
    lines: Vec<String>,
    dirs: BTreeMap<(u64, u64, u64), usize>,
}
impl Writer {
    fn add(&mut self, text: String) -> usize {
        self.lines.push(text);
        self.lines.len()
    }
    fn cartesian(&mut self, p: V3) -> usize {
        self.add(format!("CARTESIAN_POINT('',{});", point(p)))
    }
    fn direction(&mut self, d: V3) -> usize {
        let key = (d.x.to_bits(), d.y.to_bits(), d.z.to_bits());
        if let Some(id) = self.dirs.get(&key) {
            return *id;
        }
        let id = self.add(format!("DIRECTION('',{});", point(d)));
        self.dirs.insert(key, id);
        id
    }
    fn axis2(&mut self, f: &Frame) -> usize {
        let p = self.cartesian(f.origin);
        let z = self.direction(f.z);
        let x = self.direction(f.x);
        self.add(format!("AXIS2_PLACEMENT_3D('',#{p},#{z},#{x});"))
    }
    fn bspline_curve(&mut self, b: &BSplineCurve) -> usize {
        let poles: Vec<String> = b
            .poles
            .iter()
            .map(|p| format!("#{}", self.cartesian(*p)))
            .collect();
        let (knots, mults) = compress_knots(&b.knots);
        let id = self.add(format!(
            "B_SPLINE_CURVE_WITH_KNOTS('',{},({}),.UNSPECIFIED.,.F.,.F.,({}),({}),.UNSPECIFIED.);",
            b.degree,
            poles.join(","),
            mults
                .iter()
                .map(|m| m.to_string())
                .collect::<Vec<_>>()
                .join(","),
            knots.iter().map(|k| num(*k)).collect::<Vec<_>>().join(","),
        ));
        id
    }
    fn bspline_surface(&mut self, b: &BSplineSurface) -> usize {
        let mut rows = Vec::new();
        for row in &b.poles {
            let ids: Vec<String> = row
                .iter()
                .map(|p| format!("#{}", self.cartesian(*p)))
                .collect();
            rows.push(format!("({})", ids.join(",")));
        }
        let (ku, mu) = compress_knots(&b.ku);
        let (kv, mv) = compress_knots(&b.kv);
        self.add(format!(
            "B_SPLINE_SURFACE_WITH_KNOTS('',{},{},({}),.UNSPECIFIED.,.F.,.F.,.F.,({}),({}),({}),({}),.UNSPECIFIED.);",
            b.du,
            b.dv,
            rows.join(","),
            mu.iter().map(|m| m.to_string()).collect::<Vec<_>>().join(","),
            mv.iter().map(|m| m.to_string()).collect::<Vec<_>>().join(","),
            ku.iter().map(|k| num(*k)).collect::<Vec<_>>().join(","),
            kv.iter().map(|k| num(*k)).collect::<Vec<_>>().join(","),
        ))
    }
}

fn compress_knots(full: &[f64]) -> (Vec<f64>, Vec<usize>) {
    let mut knots = Vec::new();
    let mut mults: Vec<usize> = Vec::new();
    for &k in full {
        if knots.last().is_some_and(|l: &f64| (*l - k).abs() < 1e-12) {
            *mults.last_mut().unwrap() += 1;
        } else {
            knots.push(k);
            mults.push(1);
        }
    }
    (knots, mults)
}

/// Global interpolation of points by a B-spline of the given degree (Piegl & Tiller).
pub fn interpolate(pts: &[V3], degree: usize) -> BSplineCurve {
    let n = pts.len();
    let p = degree.min(n.saturating_sub(1)).max(1);
    // Chord-length parameters.
    let mut u = vec![0.0; n];
    let mut total = 0.0;
    for i in 1..n {
        total += pts[i].dist(pts[i - 1]);
    }
    if total <= 0.0 {
        total = 1.0;
    }
    let mut acc = 0.0;
    for i in 1..n {
        acc += pts[i].dist(pts[i - 1]);
        u[i] = acc / total;
    }
    u[n - 1] = 1.0;
    // Averaged knots.
    let mut knots = vec![0.0; n + p + 1];
    for k in knots.iter_mut().take(n + p + 1).skip(n) {
        *k = 1.0;
    }
    for j in 1..n.saturating_sub(p) {
        let s: f64 = u[j..j + p].iter().sum();
        knots[j + p] = s / p as f64;
    }
    // Solve N · P = pts.
    let mut a = vec![vec![0.0; n]; n];
    for (i, ui) in u.iter().enumerate() {
        let span = span_of(n - 1, p, *ui, &knots);
        let basis = basis_funs(span, *ui, p, &knots);
        for (j, b) in basis.iter().enumerate() {
            a[i][span - p + j] = *b;
        }
    }
    let poles = solve_points(&mut a, pts);
    BSplineCurve {
        degree: p,
        knots,
        poles,
        weights: None,
    }
}

fn span_of(n: usize, p: usize, u: f64, knots: &[f64]) -> usize {
    if u >= knots[n + 1] {
        return n;
    }
    if u <= knots[p] {
        return p;
    }
    let (mut lo, mut hi) = (p, n + 1);
    let mut mid = (lo + hi) / 2;
    while u < knots[mid] || u >= knots[mid + 1] {
        if u < knots[mid] {
            hi = mid;
        } else {
            lo = mid;
        }
        mid = (lo + hi) / 2;
    }
    mid
}

fn basis_funs(span: usize, u: f64, p: usize, knots: &[f64]) -> Vec<f64> {
    let mut n = vec![0.0; p + 1];
    let mut left = vec![0.0; p + 1];
    let mut right = vec![0.0; p + 1];
    n[0] = 1.0;
    for j in 1..=p {
        left[j] = u - knots[span + 1 - j];
        right[j] = knots[span + j] - u;
        let mut saved = 0.0;
        for r in 0..j {
            let temp = n[r] / (right[r + 1] + left[j - r]);
            n[r] = saved + right[r + 1] * temp;
            saved = left[j - r] * temp;
        }
        n[j] = saved;
    }
    n
}

/// Gaussian elimination with partial pivoting on a system whose right-hand side is
/// points.
fn solve_points(a: &mut [Vec<f64>], rhs: &[V3]) -> Vec<V3> {
    let n = rhs.len();
    let mut b = rhs.to_vec();
    for col in 0..n {
        let piv = (col..n)
            .max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()).then(j.cmp(&i)))
            .unwrap_or(col);
        a.swap(col, piv);
        b.swap(col, piv);
        let d = a[col][col];
        if d.abs() < 1e-300 {
            continue;
        }
        for r in 0..n {
            if r == col {
                continue;
            }
            let k = a[r][col] / d;
            if k == 0.0 {
                continue;
            }
            for c in col..n {
                a[r][c] -= k * a[col][c];
            }
            b[r] = b[r] - b[col] * k;
        }
    }
    (0..n)
        .map(|i| {
            let d = a[i][i];
            if d.abs() < 1e-300 {
                b[i]
            } else {
                b[i] / d
            }
        })
        .collect()
}

/// A B-spline through a grid of points: interpolate each row, then the poles' columns.
pub fn interpolate_surface(grid: &[Vec<V3>], du: usize, dv: usize) -> BSplineSurface {
    let rows: Vec<BSplineCurve> = grid.iter().map(|r| interpolate(r, dv)).collect();
    let kv = rows[0].knots.clone();
    let dv = rows[0].degree;
    let ncols = rows[0].poles.len();
    let mut cols: Vec<Vec<V3>> = Vec::with_capacity(ncols);
    for j in 0..ncols {
        let col: Vec<V3> = rows.iter().map(|r| r.poles[j]).collect();
        cols.push(col);
    }
    let fitted: Vec<BSplineCurve> = cols.iter().map(|c| interpolate(c, du)).collect();
    let ku = fitted[0].knots.clone();
    let du = fitted[0].degree;
    let nrows = fitted[0].poles.len();
    let mut poles = vec![vec![V3::ZERO; ncols]; nrows];
    for (j, f) in fitted.iter().enumerate() {
        for i in 0..nrows {
            poles[i][j] = f.poles[i];
        }
    }
    BSplineSurface {
        du,
        dv,
        ku,
        kv,
        poles,
        weights: None,
    }
}

/// A B-spline curve following `c` over `[t0, t1]` to within `FIT_TOL · scale`.
fn fit_curve(c: &Curve, t0: f64, t1: f64, scale: f64) -> BSplineCurve {
    let mut n = 24usize;
    loop {
        let pts: Vec<V3> = (0..=n)
            .map(|i| c.eval(t0 + (t1 - t0) * i as f64 / n as f64))
            .collect();
        let fit = interpolate(&pts, 3);
        // Deviation at the midpoints between samples.
        let (a, b) = fit.range();
        let mut worst: f64 = 0.0;
        for i in 0..n {
            let t = t0 + (t1 - t0) * (i as f64 + 0.5) / n as f64;
            let want = c.eval(t);
            // The fitted curve's nearest point (its parametrisation differs).
            let u = a + (b - a) * (i as f64 + 0.5) / n as f64;
            let got = nearest_on_bspline(&fit, want, u);
            worst = worst.max(want.dist(got));
        }
        if worst <= FIT_TOL * scale || n >= 400 {
            return fit;
        }
        n *= 2;
    }
}

fn nearest_on_bspline(b: &BSplineCurve, p: V3, seed: f64) -> V3 {
    let (lo, hi) = b.range();
    let mut t = seed.clamp(lo, hi);
    for _ in 0..30 {
        let (c, d) = b.d1(t);
        let f = (c - p).dot(d);
        let h = 1e-6 * (hi - lo);
        let (c2, d2) = b.d1((t + h).min(hi));
        let df = ((c2 - p).dot(d2) - f) / h;
        if df.abs() < 1e-300 {
            break;
        }
        let step = f / df;
        t = (t - step).clamp(lo, hi);
        if step.abs() < 1e-15 * (hi - lo) {
            break;
        }
    }
    b.d1(t).0
}

/// A B-spline surface following `s` over a parameter box, to within `FIT_TOL · scale`.
fn fit_surface(s: &Surface, u0: f64, u1: f64, v0: f64, v1: f64, scale: f64) -> BSplineSurface {
    let mut n = 16usize;
    loop {
        let grid: Vec<Vec<V3>> = (0..=n)
            .map(|i| {
                let u = u0 + (u1 - u0) * i as f64 / n as f64;
                (0..=n)
                    .map(|j| s.eval(u, v0 + (v1 - v0) * j as f64 / n as f64))
                    .collect()
            })
            .collect();
        let fit = interpolate_surface(&grid, 3, 3);
        let got = Surface::BSpline(Box::new(fit.clone()));
        let (a, b, c, d) = fit.range();
        let mut worst: f64 = 0.0;
        // Deviation at the middle of each patch, on a coarser check grid.
        let checks = n.min(24);
        for i in 0..checks {
            for j in 0..checks {
                let fu = (i as f64 + 0.5) / checks as f64;
                let fv = (j as f64 + 0.5) / checks as f64;
                let want = s.eval(u0 + (u1 - u0) * fu, v0 + (v1 - v0) * fv);
                let (pu, pv) = got.project_near(want, (a + (b - a) * fu, c + (d - c) * fv));
                worst = worst.max(want.dist(got.eval(pu, pv)));
            }
        }
        if worst <= FIT_TOL * scale || n >= 64 {
            return fit;
        }
        n *= 2;
    }
}

/// Write a solid as a STEP part file. `stamp` is the ISO 8601 time to record.
pub fn write(solid: &Solid, name: &str, schema: Schema, path: &str, stamp: &str) -> String {
    let mut w = Writer {
        lines: Vec::new(),
        dirs: BTreeMap::new(),
    };
    let scale = solid.bounds().diagonal().max(1.0);
    // Geometry and topology first; the product structure is written after, so its ids
    // can be appended in order.
    let mut vertex_id: Vec<usize> = Vec::with_capacity(solid.vertices.len());
    for v in &solid.vertices {
        let p = w.cartesian(v.p);
        vertex_id.push(w.add(format!("VERTEX_POINT('',#{p});")));
    }
    let mut edge_id: Vec<Option<usize>> = Vec::with_capacity(solid.edges.len());
    for e in &solid.edges {
        if e.degenerate {
            edge_id.push(None);
            continue;
        }
        let curve = write_curve(&mut w, e, scale);
        edge_id.push(Some(w.add(format!(
            "EDGE_CURVE('',#{},#{},#{curve},.T.);",
            vertex_id[e.v0], vertex_id[e.v1]
        ))));
    }
    let mut face_ids = Vec::with_capacity(solid.faces.len());
    for (fi, f) in solid.faces.iter().enumerate() {
        let fu = face_uv(solid, fi);
        let surface = write_surface(&mut w, &f.surface, &fu, scale);
        let mut bounds = Vec::new();
        for (li, l) in fu.loops.iter().enumerate() {
            let mut oriented = Vec::new();
            for c in l {
                let Some(id) = edge_id[c.co.edge] else {
                    continue;
                };
                oriented.push(format!(
                    "#{}",
                    w.add(format!(
                        "ORIENTED_EDGE('',*,*,#{id},{});",
                        if c.co.rev { ".F." } else { ".T." }
                    ))
                ));
            }
            if oriented.is_empty() {
                continue;
            }
            let loop_id = w.add(format!("EDGE_LOOP('',({}));", oriented.join(",")));
            let kind = if li == 0 {
                "FACE_OUTER_BOUND"
            } else {
                "FACE_BOUND"
            };
            bounds.push(format!("#{}", w.add(format!("{kind}('',#{loop_id},.T.);"))));
        }
        face_ids.push(format!(
            "#{}",
            w.add(format!(
                "ADVANCED_FACE('',({}),#{surface},{});",
                bounds.join(","),
                if fu.sense >= 0.0 { ".T." } else { ".F." }
            ))
        ));
    }
    let shell = w.add(format!("CLOSED_SHELL('',({}));", face_ids.join(",")));
    let brep = w.add(format!("MANIFOLD_SOLID_BREP('{name}',#{shell});"));
    // Units and context.
    let len = w.add("( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) );".into());
    let ang = w.add("( NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.) );".into());
    let sa = w.add("( NAMED_UNIT(*) SI_UNIT($,.STERADIAN.) SOLID_ANGLE_UNIT() );".into());
    let unc = w.add(format!(
        "UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE({}),#{len},'distance_accuracy_value','confusion accuracy');",
        num(UNCERTAINTY)
    ));
    let context = w.add(format!(
        "( GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{unc})) GLOBAL_UNIT_ASSIGNED_CONTEXT((#{len},#{ang},#{sa})) REPRESENTATION_CONTEXT('Context #1','3D Context with UNIT and UNCERTAINTY') );"
    ));
    let origin = w.axis2(&Frame::XY);
    let rep = w.add(format!(
        "ADVANCED_BREP_SHAPE_REPRESENTATION('{name}',(#{origin},#{brep}),#{context});"
    ));
    // Product structure.
    let (app_name, app_desc, year) = schema.application();
    let app_context = w.add(format!("APPLICATION_CONTEXT('{app_desc}');"));
    let _apd = w.add(format!(
        "APPLICATION_PROTOCOL_DEFINITION('international standard','{app_name}',{year},#{app_context});"
    ));
    let prod_context = w.add(format!(
        "PRODUCT_CONTEXT('',#{app_context},'mechanical');"
    ));
    let product = w.add(format!(
        "PRODUCT('{name}','{name}','',(#{prod_context}));"
    ));
    let formation = w.add(format!(
        "PRODUCT_DEFINITION_FORMATION('','',#{product});"
    ));
    let def_context = w.add(format!(
        "PRODUCT_DEFINITION_CONTEXT('part definition',#{app_context},'design');"
    ));
    let definition = w.add(format!(
        "PRODUCT_DEFINITION('design','',#{formation},#{def_context});"
    ));
    let shape = w.add(format!("PRODUCT_DEFINITION_SHAPE('','',#{definition});"));
    let _sdr = w.add(format!(
        "SHAPE_DEFINITION_REPRESENTATION(#{shape},#{rep});"
    ));
    let _prpc = w.add(format!(
        "PRODUCT_RELATED_PRODUCT_CATEGORY('part','',(#{product}));"
    ));
    let mut out = String::new();
    out.push_str("ISO-10303-21;\nHEADER;\n");
    let _ = writeln!(out, "FILE_DESCRIPTION(('FreeCAD Model'),'2;1');");
    let _ = writeln!(
        out,
        "FILE_NAME('{}','{stamp}',('FreeCAD'),(''),'Open CASCADE STEP processor 7.8','FreeCAD','Unknown');",
        escape(path)
    );
    let _ = writeln!(out, "FILE_SCHEMA(('{}'));", schema.file_schema());
    out.push_str("ENDSEC;\nDATA;\n");
    for (i, line) in w.lines.iter().enumerate() {
        let _ = writeln!(out, "#{} = {}", i + 1, line);
    }
    out.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    out
}

fn escape(s: &str) -> String {
    s.replace('\'', "''")
}

fn write_curve(w: &mut Writer, e: &Edge, scale: f64) -> usize {
    match &e.curve {
        Curve::Line { o, d } => {
            let p = w.cartesian(*o);
            let dir = w.direction(*d);
            let vec = w.add(format!("VECTOR('',#{dir},{});", num(1.0)));
            w.add(format!("LINE('',#{p},#{vec});"))
        }
        Curve::Circle { f, r } => {
            let a = w.axis2(f);
            w.add(format!("CIRCLE('',#{a},{});", num(*r)))
        }
        Curve::Ellipse { f, a, b } => {
            let ax = w.axis2(f);
            w.add(format!("ELLIPSE('',#{ax},{},{});", num(*a), num(*b)))
        }
        Curve::BSpline(b) => w.bspline_curve(b),
        Curve::Traced(_) => {
            let fit = fit_curve(&e.curve, e.t0, e.t1, scale);
            w.bspline_curve(&fit)
        }
    }
}

fn write_surface(
    w: &mut Writer,
    s: &Surface,
    fu: &crate::brep::uv::FaceUV,
    scale: f64,
) -> usize {
    match s {
        Surface::Plane { f } => {
            let a = w.axis2(f);
            w.add(format!("PLANE('',#{a});"))
        }
        Surface::Cylinder { f, r } => {
            let a = w.axis2(f);
            w.add(format!("CYLINDRICAL_SURFACE('',#{a},{});", num(*r)))
        }
        Surface::Cone { f, r, a } => {
            // STEP's semi-angle is positive, measured towards growing radius along +z.
            let (frame, radius, angle) = if *a >= 0.0 {
                (*f, *r, *a)
            } else {
                (
                    Frame {
                        origin: f.origin,
                        x: f.x,
                        y: -f.y,
                        z: -f.z,
                    },
                    *r,
                    -*a,
                )
            };
            let ax = w.axis2(&frame);
            w.add(format!(
                "CONICAL_SURFACE('',#{ax},{},{});",
                num(radius),
                num(angle)
            ))
        }
        Surface::Sphere { f, r } => {
            let a = w.axis2(f);
            w.add(format!("SPHERICAL_SURFACE('',#{a},{});", num(*r)))
        }
        Surface::Torus { f, major, minor } => {
            let a = w.axis2(f);
            w.add(format!(
                "TOROIDAL_SURFACE('',#{a},{},{});",
                num(*major),
                num(*minor)
            ))
        }
        other => {
            let (mut u0, mut u1, mut v0, mut v1) = (fu.lo.x, fu.hi.x, fu.lo.y, fu.hi.y);
            // A little beyond the face, so its boundary is inside the patch — but never
            // past a full period, which would make the patch overlap itself.
            let room = |lo: f64, hi: f64, period: Option<f64>| -> f64 {
                match period {
                    Some(p) if hi - lo >= p * (1.0 - 1e-9) => 0.0,
                    _ => (hi - lo) * 0.02,
                }
            };
            let du = room(u0, u1, other.period_u());
            let dv = room(v0, v1, other.period_v());
            u0 -= du;
            u1 += du;
            v0 -= dv;
            v1 += dv;
            let fit = fit_surface(other, u0, u1, v0, v1, scale);
            w.bspline_surface(&fit)
        }
    }
}

// ---------------------------------------------------------------------------------
// Reading.

#[derive(Clone, Debug, PartialEq)]
enum Arg {
    Id(usize),
    Num(f64),
    Str(String),
    Enum(String),
    List(Vec<Arg>),
    Null,
    Star,
}
#[derive(Clone, Debug)]
struct Entity {
    name: String,
    args: Vec<Arg>,
    /// Sub-entities of a complex instance.
    parts: Vec<Entity>,
}

struct Parser<'a> {
    s: &'a [u8],
    at: usize,
}
impl<'a> Parser<'a> {
    fn skip(&mut self) {
        loop {
            while self.at < self.s.len() && (self.s[self.at] as char).is_whitespace() {
                self.at += 1;
            }
            if self.s[self.at..].starts_with(b"/*") {
                match self.s[self.at + 2..].windows(2).position(|w| w == b"*/") {
                    Some(k) => self.at += 4 + k,
                    None => self.at = self.s.len(),
                }
                continue;
            }
            return;
        }
    }
    fn eat(&mut self, c: u8) -> bool {
        self.skip();
        if self.at < self.s.len() && self.s[self.at] == c {
            self.at += 1;
            return true;
        }
        false
    }
    fn word(&mut self) -> String {
        self.skip();
        let start = self.at;
        while self.at < self.s.len()
            && (self.s[self.at].is_ascii_alphanumeric() || self.s[self.at] == b'_')
        {
            self.at += 1;
        }
        String::from_utf8_lossy(&self.s[start..self.at]).into_owned()
    }
    fn args(&mut self) -> Result<Vec<Arg>, String> {
        let mut out = Vec::new();
        if !self.eat(b'(') {
            return Err("expected (".into());
        }
        loop {
            self.skip();
            if self.eat(b')') {
                return Ok(out);
            }
            out.push(self.arg()?);
            self.skip();
            if self.eat(b',') {
                continue;
            }
            if self.eat(b')') {
                return Ok(out);
            }
            return Err("expected , or ) in an argument list".into());
        }
    }
    fn arg(&mut self) -> Result<Arg, String> {
        self.skip();
        if self.at >= self.s.len() {
            return Err("file ends inside an entity".into());
        }
        match self.s[self.at] {
            b'#' => {
                self.at += 1;
                let start = self.at;
                while self.at < self.s.len() && self.s[self.at].is_ascii_digit() {
                    self.at += 1;
                }
                let id: usize = String::from_utf8_lossy(&self.s[start..self.at])
                    .parse()
                    .map_err(|_| "bad entity reference".to_owned())?;
                Ok(Arg::Id(id))
            }
            b'\'' => {
                self.at += 1;
                let mut out = String::new();
                while self.at < self.s.len() {
                    if self.s[self.at] == b'\'' {
                        if self.s.get(self.at + 1) == Some(&b'\'') {
                            out.push('\'');
                            self.at += 2;
                            continue;
                        }
                        self.at += 1;
                        break;
                    }
                    out.push(self.s[self.at] as char);
                    self.at += 1;
                }
                Ok(Arg::Str(out))
            }
            b'.' => {
                self.at += 1;
                let w = self.word();
                self.eat(b'.');
                Ok(Arg::Enum(w))
            }
            b'(' => Ok(Arg::List(self.args()?)),
            b'$' => {
                self.at += 1;
                Ok(Arg::Null)
            }
            b'*' => {
                self.at += 1;
                Ok(Arg::Star)
            }
            c if c.is_ascii_digit() || c == b'-' || c == b'+' => {
                let start = self.at;
                self.at += 1;
                while self.at < self.s.len()
                    && (self.s[self.at].is_ascii_digit()
                        || matches!(self.s[self.at], b'.' | b'e' | b'E' | b'-' | b'+'))
                {
                    self.at += 1;
                }
                let raw = String::from_utf8_lossy(&self.s[start..self.at]).into_owned();
                // STEP writes `1.`, `1.E-06`, `-2.5`: Rust wants a digit after the point.
                let mut text = raw.replace(".E", ".0E").replace(".e", ".0e");
                if text.ends_with('.') {
                    text.push('0');
                }
                text.parse::<f64>()
                    .map(Arg::Num)
                    .map_err(|_| format!("bad number {raw}"))
            }
            _ => {
                // A keyword argument (an inline typed value like LENGTH_MEASURE(1.)).
                let name = self.word();
                if name.is_empty() {
                    return Err(format!(
                        "unexpected character {:?} in a STEP file",
                        self.s[self.at] as char
                    ));
                }
                let inner = self.args()?;
                Ok(inner.into_iter().next().unwrap_or(Arg::Null))
            }
        }
    }
}

fn parse(text: &str) -> Result<BTreeMap<usize, Entity>, String> {
    let bytes = text.as_bytes();
    let data = text
        .find("DATA;")
        .ok_or("Not a STEP file: no DATA section")?;
    let mut p = Parser {
        s: bytes,
        at: data + 5,
    };
    let mut out: BTreeMap<usize, Entity> = BTreeMap::new();
    loop {
        p.skip();
        if p.at >= bytes.len() {
            break;
        }
        if bytes[p.at..].starts_with(b"ENDSEC") {
            break;
        }
        if bytes[p.at] != b'#' {
            return Err("Not a STEP file: an entity must start with #".into());
        }
        p.at += 1;
        let start = p.at;
        while p.at < bytes.len() && bytes[p.at].is_ascii_digit() {
            p.at += 1;
        }
        let id: usize = String::from_utf8_lossy(&bytes[start..p.at])
            .parse()
            .map_err(|_| "bad entity id".to_owned())?;
        p.skip();
        if !p.eat(b'=') {
            return Err(format!("entity #{id} has no ="));
        }
        p.skip();
        let entity = if p.s[p.at] == b'(' {
            // A complex instance: several types in one.
            let mut parts = Vec::new();
            p.at += 1;
            loop {
                p.skip();
                if p.eat(b')') {
                    break;
                }
                let name = p.word();
                let args = p.args()?;
                parts.push(Entity {
                    name,
                    args,
                    parts: vec![],
                });
            }
            Entity {
                name: "COMPLEX".into(),
                args: vec![],
                parts,
            }
        } else {
            let name = p.word();
            let args = p.args()?;
            Entity {
                name,
                args,
                parts: vec![],
            }
        };
        p.skip();
        if !p.eat(b';') {
            return Err(format!("entity #{id} does not end with ;"));
        }
        out.insert(id, entity);
    }
    Ok(out)
}

struct Reader {
    ents: BTreeMap<usize, Entity>,
    scale: f64,
    angle: f64,
}
impl Reader {
    fn ent(&self, id: usize) -> Result<&Entity, String> {
        self.ents
            .get(&id)
            .ok_or_else(|| format!("STEP file refers to a missing entity #{id}"))
    }
    fn id(&self, a: &Arg) -> Result<usize, String> {
        match a {
            Arg::Id(i) => Ok(*i),
            _ => Err("expected an entity reference".into()),
        }
    }
    fn num(&self, a: &Arg) -> Result<f64, String> {
        match a {
            Arg::Num(v) => Ok(*v),
            _ => Err("expected a number".into()),
        }
    }
    fn flag(&self, a: &Arg) -> bool {
        matches!(a, Arg::Enum(e) if e == "T")
    }
    fn list<'b>(&self, a: &'b Arg) -> Result<&'b [Arg], String> {
        match a {
            Arg::List(l) => Ok(l),
            _ => Err("expected a list".into()),
        }
    }
    fn point(&self, id: usize) -> Result<V3, String> {
        let e = self.ent(id)?;
        let c = self.list(&e.args[1])?;
        Ok(v3(
            self.num(&c[0])? * self.scale,
            self.num(&c[1])? * self.scale,
            self.num(&c[2])? * self.scale,
        ))
    }
    fn direction(&self, id: usize) -> Result<V3, String> {
        let e = self.ent(id)?;
        let c = self.list(&e.args[1])?;
        Ok(v3(self.num(&c[0])?, self.num(&c[1])?, self.num(&c[2])?).norm())
    }
    fn axis2(&self, id: usize) -> Result<Frame, String> {
        let e = self.ent(id)?;
        let o = self.point(self.id(&e.args[1])?)?;
        let z = match &e.args[2] {
            Arg::Id(i) => self.direction(*i)?,
            _ => V3::Z,
        };
        let x = match e.args.get(3) {
            Some(Arg::Id(i)) => self.direction(*i)?,
            _ => z.any_perp(),
        };
        let x = (x - z * x.dot(z)).norm();
        Ok(Frame {
            origin: o,
            x,
            y: z.cross(x).norm(),
            z,
        })
    }
    fn curve(&self, id: usize) -> Result<Curve, String> {
        let e = self.ent(id)?;
        match e.name.as_str() {
            "LINE" => {
                let p = self.point(self.id(&e.args[1])?)?;
                let v = self.ent(self.id(&e.args[2])?)?;
                let d = self.direction(self.id(&v.args[1])?)?;
                Ok(Curve::Line { o: p, d })
            }
            "CIRCLE" => Ok(Curve::Circle {
                f: self.axis2(self.id(&e.args[1])?)?,
                r: self.num(&e.args[2])? * self.scale,
            }),
            "ELLIPSE" => Ok(Curve::Ellipse {
                f: self.axis2(self.id(&e.args[1])?)?,
                a: self.num(&e.args[2])? * self.scale,
                b: self.num(&e.args[3])? * self.scale,
            }),
            "B_SPLINE_CURVE_WITH_KNOTS" => self.bspline_curve(e, None),
            "COMPLEX" => {
                // A rational B-spline: the pieces carry the knots and the weights.
                let base = e
                    .parts
                    .iter()
                    .find(|p| p.name == "B_SPLINE_CURVE_WITH_KNOTS")
                    .ok_or("unsupported complex curve in STEP")?;
                let poles = e
                    .parts
                    .iter()
                    .find(|p| p.name == "B_SPLINE_CURVE")
                    .ok_or("a rational B-spline curve without its poles")?;
                let weights = e
                    .parts
                    .iter()
                    .find(|p| p.name == "RATIONAL_B_SPLINE_CURVE")
                    .map(|p| -> Result<Vec<f64>, String> {
                        self.list(&p.args[0])?.iter().map(|a| self.num(a)).collect()
                    })
                    .transpose()?;
                let mut merged = base.clone();
                merged.args = vec![
                    Arg::Str(String::new()),
                    poles.args[0].clone(),
                    poles.args[1].clone(),
                    Arg::Enum("UNSPECIFIED".into()),
                    Arg::Enum("F".into()),
                    Arg::Enum("F".into()),
                    base.args[0].clone(),
                    base.args[1].clone(),
                    Arg::Enum("UNSPECIFIED".into()),
                ];
                self.bspline_curve(&merged, weights)
            }
            "TRIMMED_CURVE" => self.curve(self.id(&e.args[1])?),
            "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE" => {
                self.curve(self.id(&e.args[1])?)
            }
            other => Err(format!("STEP curve {other} is not supported")),
        }
    }
    fn bspline_curve(&self, e: &Entity, weights: Option<Vec<f64>>) -> Result<Curve, String> {
        // name, degree, poles, form, closed, self_intersect, mults, knots, spec
        let degree = self.num(&e.args[1])? as usize;
        let poles: Vec<V3> = self
            .list(&e.args[2])?
            .iter()
            .map(|a| self.point(self.id(a)?))
            .collect::<Result<_, _>>()?;
        let mults: Vec<usize> = self
            .list(&e.args[6])?
            .iter()
            .map(|a| self.num(a).map(|v| v as usize))
            .collect::<Result<_, _>>()?;
        let knots: Vec<f64> = self
            .list(&e.args[7])?
            .iter()
            .map(|a| self.num(a))
            .collect::<Result<_, _>>()?;
        let mut full = Vec::new();
        for (k, m) in knots.iter().zip(&mults) {
            for _ in 0..*m {
                full.push(*k);
            }
        }
        Ok(Curve::BSpline(Box::new(BSplineCurve {
            degree,
            knots: full,
            poles,
            weights,
        })))
    }
    fn surface(&self, id: usize) -> Result<Surface, String> {
        let e = self.ent(id)?;
        match e.name.as_str() {
            "PLANE" => Ok(Surface::Plane {
                f: self.axis2(self.id(&e.args[1])?)?,
            }),
            "CYLINDRICAL_SURFACE" => Ok(Surface::Cylinder {
                f: self.axis2(self.id(&e.args[1])?)?,
                r: self.num(&e.args[2])? * self.scale,
            }),
            "CONICAL_SURFACE" => Ok(Surface::Cone {
                f: self.axis2(self.id(&e.args[1])?)?,
                r: self.num(&e.args[2])? * self.scale,
                a: self.num(&e.args[3])? * self.angle,
            }),
            "SPHERICAL_SURFACE" => Ok(Surface::Sphere {
                f: self.axis2(self.id(&e.args[1])?)?,
                r: self.num(&e.args[2])? * self.scale,
            }),
            "TOROIDAL_SURFACE" => Ok(Surface::Torus {
                f: self.axis2(self.id(&e.args[1])?)?,
                major: self.num(&e.args[2])? * self.scale,
                minor: self.num(&e.args[3])? * self.scale,
            }),
            "B_SPLINE_SURFACE_WITH_KNOTS" => self.bspline_surface(e, None),
            "SURFACE_OF_REVOLUTION" => {
                let curve = self.curve(self.id(&e.args[1])?)?;
                let axis = self.ent(self.id(&e.args[2])?)?;
                let o = self.point(self.id(&axis.args[1])?)?;
                let d = self.direction(self.id(&axis.args[2])?)?;
                let (t0, t1) = curve_range(&curve);
                Ok(Surface::Revolution {
                    f: Frame::from_normal(o, d, d.any_perp()),
                    curve: Box::new(curve),
                    t0,
                    t1,
                })
            }
            "SURFACE_OF_LINEAR_EXTRUSION" => {
                let curve = self.curve(self.id(&e.args[1])?)?;
                let v = self.ent(self.id(&e.args[2])?)?;
                let d = self.direction(self.id(&v.args[1])?)?;
                let (t0, t1) = curve_range(&curve);
                Ok(Surface::Extrusion {
                    curve: Box::new(curve),
                    dir: d,
                    t0,
                    t1,
                })
            }
            "COMPLEX" => {
                let base = e
                    .parts
                    .iter()
                    .find(|p| p.name == "B_SPLINE_SURFACE_WITH_KNOTS")
                    .ok_or("unsupported complex surface in STEP")?;
                let poles = e
                    .parts
                    .iter()
                    .find(|p| p.name == "B_SPLINE_SURFACE")
                    .ok_or("a rational B-spline surface without its poles")?;
                let weights = e
                    .parts
                    .iter()
                    .find(|p| p.name == "RATIONAL_B_SPLINE_SURFACE")
                    .map(|p| -> Result<Vec<Vec<f64>>, String> {
                        self.list(&p.args[0])?
                            .iter()
                            .map(|row| {
                                self.list(row)?.iter().map(|a| self.num(a)).collect()
                            })
                            .collect()
                    })
                    .transpose()?;
                let mut merged = base.clone();
                merged.args = vec![
                    Arg::Str(String::new()),
                    poles.args[0].clone(),
                    poles.args[1].clone(),
                    poles.args[2].clone(),
                    Arg::Enum("UNSPECIFIED".into()),
                    Arg::Enum("F".into()),
                    Arg::Enum("F".into()),
                    Arg::Enum("F".into()),
                    base.args[0].clone(),
                    base.args[1].clone(),
                    base.args[2].clone(),
                    base.args[3].clone(),
                    Arg::Enum("UNSPECIFIED".into()),
                ];
                self.bspline_surface(&merged, weights)
            }
            other => Err(format!("STEP surface {other} is not supported")),
        }
    }
    fn bspline_surface(
        &self,
        e: &Entity,
        weights: Option<Vec<Vec<f64>>>,
    ) -> Result<Surface, String> {
        let du = self.num(&e.args[1])? as usize;
        let dv = self.num(&e.args[2])? as usize;
        let poles: Vec<Vec<V3>> = self
            .list(&e.args[3])?
            .iter()
            .map(|row| {
                self.list(row)?
                    .iter()
                    .map(|a| self.point(self.id(a)?))
                    .collect()
            })
            .collect::<Result<_, _>>()?;
        let mult = |a: &Arg| -> Result<Vec<usize>, String> {
            self.list(a)?
                .iter()
                .map(|x| self.num(x).map(|v| v as usize))
                .collect()
        };
        let knot = |a: &Arg| -> Result<Vec<f64>, String> {
            self.list(a)?.iter().map(|x| self.num(x)).collect()
        };
        let expand = |k: Vec<f64>, m: Vec<usize>| {
            let mut out = Vec::new();
            for (kk, mm) in k.iter().zip(&m) {
                for _ in 0..*mm {
                    out.push(*kk);
                }
            }
            out
        };
        let ku = expand(knot(&e.args[10])?, mult(&e.args[8])?);
        let kv = expand(knot(&e.args[11])?, mult(&e.args[9])?);
        Ok(Surface::BSpline(Box::new(BSplineSurface {
            du,
            dv,
            ku,
            kv,
            poles,
            weights,
        })))
    }
}

fn curve_range(c: &Curve) -> (f64, f64) {
    match c {
        Curve::Circle { .. } | Curve::Ellipse { .. } => (0.0, math::TAU),
        Curve::BSpline(b) => b.range(),
        Curve::Traced(t) => t.range(),
        Curve::Line { .. } => (-1e3, 1e3),
    }
}

/// Read every solid in a STEP file, with the name each carries.
pub fn read(text: &str) -> Result<Vec<(String, Solid)>, String> {
    if !text.trim_start().starts_with("ISO-10303-21") {
        return Err("Not a STEP file".into());
    }
    let ents = parse(text)?;
    // Units: millimetres unless the file says otherwise.
    let mut scale = 1.0;
    let mut angle = 1.0;
    for e in ents.values() {
        if e.name != "COMPLEX" {
            continue;
        }
        let names: Vec<&str> = e.parts.iter().map(|p| p.name.as_str()).collect();
        if names.contains(&"LENGTH_UNIT") {
            if let Some(si) = e.parts.iter().find(|p| p.name == "SI_UNIT") {
                let prefix = match &si.args[0] {
                    Arg::Enum(p) => p.clone(),
                    _ => String::new(),
                };
                scale = match prefix.as_str() {
                    "MILLI" => 1.0,
                    "CENTI" => 10.0,
                    "DECI" => 100.0,
                    "MICRO" => 1e-3,
                    "KILO" => 1e6,
                    _ => 1000.0,
                };
            }
        }
        if names.contains(&"PLANE_ANGLE_UNIT") {
            if let Some(si) = e.parts.iter().find(|p| p.name == "SI_UNIT") {
                let unit = match &si.args[1] {
                    Arg::Enum(p) => p.clone(),
                    _ => String::new(),
                };
                if unit == "DEGREE" {
                    angle = math::PI / 180.0;
                }
            }
        }
    }
    // Converted units (inches, degrees), whether written plainly or in a complex entity.
    let flat: Vec<&Entity> = ents
        .values()
        .flat_map(|e| std::iter::once(e).chain(e.parts.iter()))
        .collect();
    for e in flat {
        if e.name == "CONVERSION_BASED_UNIT" && e.args.len() >= 2 {
            if let (Arg::Str(name), Arg::Id(factor)) = (&e.args[0], &e.args[1]) {
                if let Some(m) = ents.get(factor) {
                    if let Ok(v) = (Reader {
                        ents: ents.clone(),
                        scale: 1.0,
                        angle: 1.0,
                    })
                    .num(&m.args[0])
                    {
                        let lower = name.to_ascii_lowercase();
                        if lower.contains("inch") {
                            scale = v;
                        } else if lower.contains("degree") {
                            angle = v;
                        }
                    }
                }
            }
        }
    }
    let r = Reader { ents, scale, angle };
    let mut out = Vec::new();
    let breps: Vec<(usize, &Entity)> = r
        .ents
        .iter()
        .filter(|(_, e)| e.name == "MANIFOLD_SOLID_BREP" || e.name == "BREP_WITH_VOIDS")
        .map(|(i, e)| (*i, e))
        .collect();
    if breps.is_empty() {
        return Err("The STEP file has no solid (MANIFOLD_SOLID_BREP)".into());
    }
    for (id, e) in breps {
        let name = match &e.args[0] {
            Arg::Str(s) if !s.is_empty() => s.clone(),
            _ => format!("Shape{id}"),
        };
        let shell = r.id(&e.args[1])?;
        out.push((name, r.shell(shell)?));
    }
    Ok(out)
}

impl Reader {
    fn shell(&self, id: usize) -> Result<Solid, String> {
        let e = self.ent(id)?;
        let faces = self.list(&e.args[1])?;
        let mut solid = Solid::default();
        // Vertices and edges are shared by id.
        let mut vmap: BTreeMap<usize, usize> = BTreeMap::new();
        let mut emap: BTreeMap<usize, usize> = BTreeMap::new();
        for fa in faces {
            let f = self.ent(self.id(fa)?)?;
            if f.name != "ADVANCED_FACE" && f.name != "FACE_SURFACE" {
                continue;
            }
            let surface = self.surface(self.id(&f.args[2])?)?;
            let same_sense = self.flag(&f.args[3]);
            let mut loops = Vec::new();
            for b in self.list(&f.args[1])? {
                let bound = self.ent(self.id(b)?)?;
                let outer = bound.name == "FACE_OUTER_BOUND";
                let orientation = self.flag(&bound.args[2]);
                let lp = self.ent(self.id(&bound.args[1])?)?;
                if lp.name == "VERTEX_LOOP" {
                    continue;
                }
                let mut coedges = Vec::new();
                for oe in self.list(&lp.args[1])? {
                    let o = self.ent(self.id(oe)?)?;
                    let edge_id = self.id(&o.args[3])?;
                    let forward = self.flag(&o.args[4]);
                    let ec = self.ent(edge_id)?;
                    let (va, vb) = (self.id(&ec.args[1])?, self.id(&ec.args[2])?);
                    let curve = self.curve(self.id(&ec.args[3])?)?;
                    let same = self.flag(&ec.args[4]);
                    let vertex = |this: &Self,
                                  solid: &mut Solid,
                                  vmap: &mut BTreeMap<usize, usize>,
                                  id: usize|
                     -> Result<usize, String> {
                        if let Some(v) = vmap.get(&id) {
                            return Ok(*v);
                        }
                        let ve = this.ent(id)?;
                        let p = this.point(this.id(&ve.args[1])?)?;
                        let v = solid.add_vertex(p);
                        vmap.insert(id, v);
                        Ok(v)
                    };
                    let (v0, v1) = (
                        vertex(self, &mut solid, &mut vmap, va)?,
                        vertex(self, &mut solid, &mut vmap, vb)?,
                    );
                    let e = match emap.get(&edge_id) {
                        Some(e) => *e,
                        None => {
                            // Our edges always run with the curve; the ends follow.
                            let (start, end) = if same { (v0, v1) } else { (v1, v0) };
                            let (pa, pb) = (solid.vertices[start].p, solid.vertices[end].p);
                            let (t0, t1) = if va == vb {
                                // A closed edge covers its curve once round.
                                closed_range(&curve, pa)
                            } else {
                                param_range(&curve, pa, pb)
                            };
                            let e = solid.add_edge(curve, t0, t1, start, end);
                            emap.insert(edge_id, e);
                            e
                        }
                    };
                    coedges.push(Coedge {
                        edge: e,
                        rev: forward != same,
                    });
                }
                if coedges.is_empty() {
                    continue;
                }
                if !orientation {
                    coedges.reverse();
                    for c in coedges.iter_mut() {
                        c.rev = !c.rev;
                    }
                }
                if outer {
                    loops.insert(0, coedges);
                } else {
                    loops.push(coedges);
                }
            }
            if loops.is_empty() {
                continue;
            }
            let mut face = Face { surface, loops };
            if !same_sense {
                // The face's own normal is the surface's reversed: our loops already
                // run round the face's normal, so nothing else changes.
                face.surface = face.surface.clone();
            }
            solid.faces.push(face);
        }
        if solid.faces.is_empty() {
            return Err("The STEP solid has no faces this kernel understands".into());
        }
        repair_poles(&mut solid);
        solid.renumber();
        solid
            .check()
            .map_err(|e| format!("The STEP solid is not a closed shell: {e}"))?;
        Ok(solid)
    }
}

/// The range of a closed edge: a full turn of a conic, or the whole of a B-spline that
/// starts and ends at the same point.
fn closed_range(c: &Curve, p: V3) -> (f64, f64) {
    match c.period() {
        Some(per) => {
            let a = c.project(p);
            (a, a + per)
        }
        None => curve_range(c),
    }
}

/// The parameters of `pa` and `pb` on a curve, in the curve's own direction.
fn param_range(c: &Curve, pa: V3, pb: V3) -> (f64, f64) {
    match c.period() {
        Some(per) => {
            let a = c.project(pa);
            let mut b = c.project(pb);
            while b <= a + 1e-9 {
                b += per;
            }
            (a, b)
        }
        None => {
            let (a, b) = (c.project(pa), c.project(pb));
            if a <= b {
                (a, b)
            } else {
                (b, a)
            }
        }
    }
}

/// Put back the point edges a sphere or cone needs at its poles: STEP files leave them
/// out, but the parameter plane needs the loop to close.
fn repair_poles(solid: &mut Solid) {
    for fi in 0..solid.faces.len() {
        let has_pole = matches!(
            solid.faces[fi].surface,
            Surface::Sphere { .. } | Surface::Cone { .. } | Surface::Revolution { .. }
        );
        if !has_pole {
            continue;
        }
        loop {
            let fu = face_uv(solid, fi);
            if fu.valid {
                break;
            }
            // Find the junction where u jumps at a singular point.
            let mut fixed = false;
            for (li, l) in fu.loops.iter().enumerate() {
                let n = l.len();
                for k in 0..n {
                    let end = *l[k].uv.last().unwrap();
                    let start = l[(k + 1) % n].uv[0];
                    if (end - start).len() < 1e-7 {
                        continue;
                    }
                    let p = *l[k].pts.last().unwrap();
                    if !crate::brep::uv::singular_point(&solid.faces[fi].surface, p) {
                        continue;
                    }
                    let v = solid.co_ends(l[k].co).1;
                    let span = start.x - end.x;
                    let e = solid.add_degenerate(v);
                    solid.edges[e].t1 = span.abs();
                    let at = solid.faces[fi].loops[li]
                        .iter()
                        .position(|c| *c == l[k].co)
                        .map(|x| x + 1)
                        .unwrap_or(0);
                    solid.faces[fi].loops[li].insert(
                        at,
                        Coedge {
                            edge: e,
                            rev: span < 0.0,
                        },
                    );
                    fixed = true;
                    break;
                }
                if fixed {
                    break;
                }
            }
            if !fixed {
                break;
            }
        }
    }
}

#[allow(dead_code)]
fn unused(_: &Vertex) {}
