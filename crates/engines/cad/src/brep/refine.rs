//! Refine (FreeCAD's `Refine = True`, OpenCascade's `ShapeUpgrade_UnifySameDomain`):
//! neighbouring faces on the same surface facing the same way become one face, and
//! edges that continue each other on one curve between the same two faces become one.
use super::geom::Curve;
use super::topo::{Coedge, Face, Solid, TOL};
use super::uv::face_uv;
use crate::math::TAU;

fn outward(s: &Solid, f: usize, p: crate::math::V3, sense: f64) -> crate::math::V3 {
    let (u, v) = s.faces[f].surface.project(p);
    s.faces[f].surface.normal(u, v) * sense
}

pub fn refine(s: &Solid) -> Solid {
    let mut out = merge_faces(s);
    merge_edges(&mut out);
    out.renumber();
    out
}

fn merge_faces(s: &Solid) -> Solid {
    let n = s.faces.len();
    let senses: Vec<f64> = (0..n).map(|f| face_uv(s, f).sense).collect();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    let uses = s.edge_uses();
    for (e, u) in uses.iter().enumerate() {
        if u.len() != 2 || s.edges[e].degenerate {
            continue;
        }
        let (fa, fb) = (u[0].0, u[1].0);
        if fa == fb {
            continue;
        }
        let (sa, sb) = (&s.faces[fa].surface, &s.faces[fb].surface);
        if !sa.same(sb, TOL * 10.0) {
            continue;
        }
        let p = s.edges[e].mid();
        if outward(s, fa, p, senses[fa]).dot(outward(s, fb, p, senses[fb])) <= 0.0 {
            continue;
        }
        let (ra, rb) = (find(&mut parent, fa), find(&mut parent, fb));
        if ra != rb {
            parent[ra.max(rb)] = ra.min(rb);
        }
    }
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut root_group: Vec<Option<usize>> = vec![None; n];
    for f in 0..n {
        let r = find(&mut parent, f);
        match root_group[r] {
            Some(g) => groups[g].push(f),
            None => {
                root_group[r] = Some(groups.len());
                groups.push(vec![f]);
            }
        }
    }
    let mut out = Solid {
        vertices: s.vertices.clone(),
        edges: s.edges.clone(),
        faces: Vec::new(),
    };
    for g in groups {
        if g.len() == 1 {
            out.faces.push(s.faces[g[0]].clone());
            continue;
        }
        match merged(s, &g, &mut out.edges) {
            Some(f) => out.faces.push(f),
            None => {
                for f in g {
                    out.faces.push(s.faces[f].clone());
                }
            }
        }
    }
    out
}

/// One face from a group of faces on the same surface, if its loops close.
fn merged(s: &Solid, group: &[usize], edges: &mut Vec<super::topo::Edge>) -> Option<Face> {
    // Coedges with their face; edges shared by two faces of the group cancel. Point
    // edges at poles are set aside and put back, summed, where the new loops pass.
    let mut owner: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
    let mut pole_span: std::collections::BTreeMap<usize, f64> = Default::default();
    for &f in group {
        for l in &s.faces[f].loops {
            for c in l {
                owner.entry(c.edge).or_default().push(f);
            }
        }
    }
    let mut rest: Vec<(Coedge, usize)> = Vec::new();
    for &f in group {
        for l in &s.faces[f].loops {
            for c in l {
                let e = &s.edges[c.edge];
                if e.degenerate {
                    *pole_span.entry(e.v0).or_insert(0.0) +=
                        (e.t1 - e.t0) * if c.rev { -1.0 } else { 1.0 };
                    continue;
                }
                let o = &owner[&c.edge];
                let internal = o.len() == 2 && o[0] != o[1];
                if !internal {
                    rest.push((*c, f));
                }
            }
        }
    }
    // Chain into loops.
    let mut used = vec![false; rest.len()];
    let mut loops: Vec<Vec<Coedge>> = Vec::new();
    for start in 0..rest.len() {
        if used[start] {
            continue;
        }
        let mut lp = vec![rest[start].0];
        used[start] = true;
        let first_v = s.co_ends(rest[start].0).0;
        let mut cur = start;
        for _ in 0..rest.len() {
            let end_v = s.co_ends(rest[cur].0).1;
            if end_v == first_v && lp.len() > 1
                || (end_v == first_v && s.edges[rest[cur].0.edge].closed())
            {
                break;
            }
            let cands: Vec<usize> = (0..rest.len())
                .filter(|&i| !used[i] && s.co_ends(rest[i].0).0 == end_v)
                .collect();
            let next = cands
                .iter()
                .copied()
                .find(|&i| rest[i].1 == rest[cur].1)
                .or(cands.first().copied());
            let nx = next?;
            used[nx] = true;
            lp.push(rest[nx].0);
            cur = nx;
        }
        if s.co_ends(*lp.last().unwrap()).1 != first_v {
            return None;
        }
        loops.push(lp);
    }
    // Put the pole edges back where a loop passes through their vertex.
    let mut new_edges: Vec<super::topo::Edge> = Vec::new();
    let mut placed: std::collections::BTreeSet<usize> = Default::default();
    for lp in &mut loops {
        let mut i = 0;
        while i < lp.len() {
            let v = s.co_ends(lp[i]).1;
            if let Some(&span) = pole_span.get(&v) {
                if span.abs() > 1e-12 && !placed.contains(&v) {
                    placed.insert(v);
                    new_edges.push(super::topo::Edge {
                        curve: Curve::Line {
                            o: s.vertices[v].p,
                            d: crate::math::V3::X,
                        },
                        t0: 0.0,
                        t1: span.abs(),
                        v0: v,
                        v1: v,
                        degenerate: true,
                    });
                    lp.insert(
                        i + 1,
                        Coedge {
                            edge: edges.len() + new_edges.len() - 1,
                            rev: span < 0.0,
                        },
                    );
                    i += 1;
                }
            }
            i += 1;
        }
    }
    if pole_span
        .iter()
        .any(|(v, sp)| sp.abs() > 1e-12 && !placed.contains(v))
    {
        return None;
    }
    let face = Face {
        surface: s.faces[group[0]].surface.clone(),
        loops,
    };
    let mut all_edges = edges.clone();
    all_edges.extend(new_edges.iter().cloned());
    let tmp = Solid {
        vertices: s.vertices.clone(),
        edges: all_edges,
        faces: vec![face.clone()],
    };
    let fu = face_uv(&tmp, 0);
    if !fu.valid {
        return None;
    }
    // The merged area must equal the parts' (a wrong chaining changes it).
    let area_of = |sol: &Solid, f: usize| -> f64 {
        let fu = face_uv(sol, f);
        fu.loops
            .iter()
            .map(|l| super::uv::loop_area(l))
            .sum::<f64>()
            .abs()
    };
    let parts: f64 = group.iter().map(|&f| area_of(s, f)).sum();
    let whole = area_of(&tmp, 0);
    if (parts - whole).abs() > 1e-6 * parts.max(1e-9) {
        return None;
    }
    edges.extend(new_edges);
    Some(face)
}

/// Join two edges meeting at a vertex that nothing else uses, when they lie on one
/// curve and separate the same two faces.
fn merge_edges(s: &mut Solid) {
    loop {
        let mut changed = false;
        let mut at: Vec<Vec<usize>> = vec![Vec::new(); s.vertices.len()];
        let uses = s.edge_uses();
        for (e, ed) in s.edges.iter().enumerate() {
            if uses[e].is_empty() {
                continue;
            }
            at[ed.v0].push(e);
            if ed.v1 != ed.v0 {
                at[ed.v1].push(e);
            }
        }
        for (v, here) in at.iter().enumerate() {
            if here.len() != 2 {
                continue;
            }
            let (e1, e2) = (here[0], here[1]);
            if e1 == e2 || s.edges[e1].degenerate || s.edges[e2].degenerate {
                continue;
            }
            if s.edges[e1].closed() || s.edges[e2].closed() {
                continue;
            }
            let (c1, c2) = (&s.edges[e1].curve, &s.edges[e2].curve);
            if !c1.same(c2, TOL * 10.0) {
                continue;
            }
            let mut f1: Vec<usize> = uses[e1].iter().map(|u| u.0).collect();
            let mut f2: Vec<usize> = uses[e2].iter().map(|u| u.0).collect();
            f1.sort_unstable();
            f2.sort_unstable();
            if f1 != f2 || f1.len() != 2 {
                continue;
            }
            // The far ends.
            let far = |e: usize| {
                let ed = &s.edges[e];
                if ed.v0 == v {
                    ed.v1
                } else {
                    ed.v0
                }
            };
            let (a, b) = (far(e1), far(e2));
            // New edge on e1's curve from a through v to b.
            let curve = s.edges[e1].curve.clone();
            let (pa, pv, pb) = (s.vertices[a].p, s.vertices[v].p, s.vertices[b].p);
            let (t0, t1, v0, v1) = match &curve {
                Curve::Line { .. } => {
                    let (ta, tb) = (curve.project(pa), curve.project(pb));
                    if ta <= tb {
                        (ta, tb, a, b)
                    } else {
                        (tb, ta, b, a)
                    }
                }
                Curve::Circle { .. } => {
                    // Keep e1's sense: its range extended across v into e2.
                    let ed1 = &s.edges[e1];
                    let tb = curve.project(pb);
                    let _ = (pa, pv);
                    if ed1.v1 == v {
                        let d = (tb - ed1.t1).rem_euclid(TAU);
                        (ed1.t0, ed1.t1 + d, a, b)
                    } else {
                        let d = (ed1.t0 - tb).rem_euclid(TAU);
                        (ed1.t0 - d, ed1.t1, b, a)
                    }
                }
                _ => continue,
            };
            if a == b {
                continue;
            }
            // Orientation of the new edge relative to e1: same where e1 ran v0→v1.
            let e1_forward_into_v = s.edges[e1].v1 == v;
            let new_forward_matches_e1 = if e1_forward_into_v { v0 == a } else { v0 == b };
            s.edges[e1].t0 = t0;
            s.edges[e1].t1 = t1;
            s.edges[e1].v0 = v0;
            s.edges[e1].v1 = v1;
            // Replace the coedge pair in every loop.
            for f in &mut s.faces {
                for l in &mut f.loops {
                    let n = l.len();
                    let Some(i1) = l.iter().position(|c| c.edge == e1) else {
                        continue;
                    };
                    let rev1 = l[i1].rev;
                    let new_rev = if new_forward_matches_e1 { rev1 } else { !rev1 };
                    l[i1] = Coedge {
                        edge: e1,
                        rev: new_rev,
                    };
                    if let Some(i2) = l.iter().position(|c| c.edge == e2) {
                        l.remove(i2);
                    }
                    let _ = n;
                }
            }
            changed = true;
            break;
        }
        if !changed {
            break;
        }
    }
}
