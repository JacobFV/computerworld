//! Solids as closed triangle meshes. Each triangle records which analytic surface it
//! approximates, so a mesh still knows its faces are planes and cylinders: picking,
//! measuring and fillets work on faces and edges, not on triangles.
use crate::math::{v3, Frame, Xform, V3};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::hash::BuildHasherDefault;

/// A hasher with fixed keys, so hash maps behave identically on every run and target.
pub type Fixed = BuildHasherDefault<std::collections::hash_map::DefaultHasher>;
pub type FixedMap<K, V> = HashMap<K, V, Fixed>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Surface {
    Plane {
        origin: V3,
        normal: V3,
    },
    Cylinder {
        origin: V3,
        axis: V3,
        radius: f64,
    },
    /// Swept by a line that is neither parallel nor perpendicular to the axis.
    Cone {
        origin: V3,
        axis: V3,
    },
    /// Swept by an arc about an axis: a sphere, torus or part of one.
    Revolved {
        origin: V3,
        axis: V3,
        center: V3,
        radius: f64,
    },
    /// Triangles with no surface behind them, as imported from STL or OBJ.
    Facets,
}
impl Surface {
    pub fn name(&self) -> &'static str {
        match self {
            Surface::Plane { .. } => "Plane",
            Surface::Cylinder { .. } => "Cylinder",
            Surface::Cone { .. } => "Cone",
            Surface::Revolved { .. } => "SurfaceOfRevolution",
            Surface::Facets => "Mesh",
        }
    }
    pub fn transformed(&self, x: &Xform) -> Surface {
        match self {
            Surface::Plane { origin, normal } => Surface::Plane {
                origin: x.point(*origin),
                normal: x.dir(*normal).norm(),
            },
            Surface::Cylinder {
                origin,
                axis,
                radius,
            } => Surface::Cylinder {
                origin: x.point(*origin),
                axis: x.dir(*axis).norm(),
                radius: *radius,
            },
            Surface::Cone { origin, axis } => Surface::Cone {
                origin: x.point(*origin),
                axis: x.dir(*axis).norm(),
            },
            Surface::Revolved {
                origin,
                axis,
                center,
                radius,
            } => Surface::Revolved {
                origin: x.point(*origin),
                axis: x.dir(*axis).norm(),
                center: x.point(*center),
                radius: *radius,
            },
            Surface::Facets => Surface::Facets,
        }
    }
    /// Whether two surfaces are the same geometric surface (so faces on them merge).
    pub fn same(&self, other: &Surface) -> bool {
        const LIN: f64 = 1e-6;
        const ANG: f64 = 1e-9;
        let on_line = |o1: V3, a1: V3, o2: V3, a2: V3| {
            a1.cross(a2).len() < ANG * 10.0 && {
                let d = o2 - o1;
                (d - a1 * d.dot(a1)).len() < LIN
            }
        };
        match (self, other) {
            (
                Surface::Plane { origin, normal },
                Surface::Plane {
                    origin: o2,
                    normal: n2,
                },
            ) => {
                (*normal - *n2).len() < ANG * 10.0
                    && (o2.dot(*normal) - origin.dot(*normal)).abs() < LIN
            }
            (
                Surface::Cylinder {
                    origin,
                    axis,
                    radius,
                },
                Surface::Cylinder {
                    origin: o2,
                    axis: a2,
                    radius: r2,
                },
            ) => (radius - r2).abs() < LIN && on_line(*origin, *axis, *o2, *a2),
            _ => false,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Mesh {
    pub verts: Vec<V3>,
    pub tris: Vec<[u32; 3]>,
    /// Index into `surfaces` for every triangle.
    pub tri_surface: Vec<u32>,
    pub surfaces: Vec<Surface>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub min: V3,
    pub max: V3,
}
impl Bounds {
    pub fn size(&self) -> V3 {
        self.max - self.min
    }
    pub fn center(&self) -> V3 {
        (self.min + self.max) * 0.5
    }
    pub fn diagonal(&self) -> f64 {
        self.size().len()
    }
    pub fn union(&self, o: &Bounds) -> Bounds {
        Bounds {
            min: self.min.min(o.min),
            max: self.max.max(o.max),
        }
    }
}

impl Mesh {
    pub fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }
    pub fn tri(&self, i: usize) -> [V3; 3] {
        let [a, b, c] = self.tris[i];
        [
            self.verts[a as usize],
            self.verts[b as usize],
            self.verts[c as usize],
        ]
    }
    pub fn tri_normal(&self, i: usize) -> V3 {
        let [a, b, c] = self.tri(i);
        (b - a).cross(c - a).norm()
    }
    /// Signed volume by the divergence theorem; positive for an outward-facing solid.
    pub fn volume(&self) -> f64 {
        (0..self.tris.len())
            .map(|i| {
                let [a, b, c] = self.tri(i);
                a.dot(b.cross(c))
            })
            .sum::<f64>()
            / 6.0
    }
    pub fn area(&self) -> f64 {
        (0..self.tris.len()).map(|i| self.tri_area(i)).sum()
    }
    pub fn tri_area(&self, i: usize) -> f64 {
        let [a, b, c] = self.tri(i);
        (b - a).cross(c - a).len() / 2.0
    }
    /// Centre of mass of the enclosed volume at uniform density.
    pub fn center_of_mass(&self) -> Option<V3> {
        let mut total = 0.0;
        let mut acc = V3::ZERO;
        for i in 0..self.tris.len() {
            let [a, b, c] = self.tri(i);
            let v = a.dot(b.cross(c)) / 6.0;
            total += v;
            acc += (a + b + c) * (v / 4.0);
        }
        (total.abs() > 1e-15).then(|| acc / total)
    }
    pub fn bounds(&self) -> Option<Bounds> {
        let mut it = self.tris.iter().flatten().map(|i| self.verts[*i as usize]);
        let first = it.next()?;
        let (min, max) = it.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p)));
        Some(Bounds { min, max })
    }
    /// Directed edges without exactly one opposite partner. Zero means the mesh is
    /// closed, manifold and consistently oriented: watertight.
    pub fn open_edges(&self) -> usize {
        let mut count: FixedMap<(u32, u32), i32> = FixedMap::default();
        for t in &self.tris {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                *count.entry((a, b)).or_insert(0) += 1;
            }
        }
        let mut bad = 0;
        for (&(a, b), &n) in &count {
            let back = count.get(&(b, a)).copied().unwrap_or(0);
            if n != 1 || back != 1 {
                bad += 1;
            }
        }
        bad
    }
    pub fn is_watertight(&self) -> bool {
        !self.tris.is_empty() && self.open_edges() == 0
    }
    pub fn transformed(&self, x: &Xform) -> Mesh {
        let flip = x.det() < 0.0;
        Mesh {
            verts: self.verts.iter().map(|p| x.point(*p)).collect(),
            tris: self
                .tris
                .iter()
                .map(|t| if flip { [t[0], t[2], t[1]] } else { *t })
                .collect(),
            tri_surface: self.tri_surface.clone(),
            surfaces: self.surfaces.iter().map(|s| s.transformed(x)).collect(),
        }
    }
    /// Turn inside out.
    pub fn flipped(&self) -> Mesh {
        let mut m = self.clone();
        for t in &mut m.tris {
            t.swap(1, 2);
        }
        for s in &mut m.surfaces {
            if let Surface::Plane { normal, .. } = s {
                *normal = -*normal;
            }
        }
        m
    }
    /// Append another mesh's triangles (no boolean: the caller knows they are disjoint).
    pub fn append(&mut self, other: &Mesh) {
        let vbase = self.verts.len() as u32;
        let sbase = self.surfaces.len() as u32;
        self.verts.extend_from_slice(&other.verts);
        self.tris.extend(
            other
                .tris
                .iter()
                .map(|t| [t[0] + vbase, t[1] + vbase, t[2] + vbase]),
        );
        self.tri_surface
            .extend(other.tri_surface.iter().map(|s| s + sbase));
        self.surfaces.extend(other.surfaces.iter().cloned());
    }
    /// Merge identical surfaces and drop unused ones and unused vertices, keeping first
    /// appearances in order so numbering stays stable.
    pub fn compact(&mut self) {
        let mut remap_s: Vec<u32> = Vec::with_capacity(self.surfaces.len());
        let mut surfaces: Vec<Surface> = Vec::new();
        let used: Vec<bool> = {
            let mut u = vec![false; self.surfaces.len()];
            for s in &self.tri_surface {
                u[*s as usize] = true;
            }
            u
        };
        for (i, s) in self.surfaces.iter().enumerate() {
            if !used[i] {
                remap_s.push(u32::MAX);
                continue;
            }
            match surfaces.iter().position(|t| t.same(s)) {
                Some(at) => remap_s.push(at as u32),
                None => {
                    remap_s.push(surfaces.len() as u32);
                    surfaces.push(s.clone());
                }
            }
        }
        for s in &mut self.tri_surface {
            *s = remap_s[*s as usize];
        }
        self.surfaces = surfaces;
        let mut remap_v = vec![u32::MAX; self.verts.len()];
        let mut verts = Vec::new();
        for t in &mut self.tris {
            for i in t.iter_mut() {
                if remap_v[*i as usize] == u32::MAX {
                    remap_v[*i as usize] = verts.len() as u32;
                    verts.push(self.verts[*i as usize]);
                }
                *i = remap_v[*i as usize];
            }
        }
        self.verts = verts;
    }
    /// A mesh of one axis-aligned box, for tests and primitives.
    pub fn cuboid(min: V3, max: V3) -> Mesh {
        let p = |x: bool, y: bool, z: bool| {
            v3(
                if x { max.x } else { min.x },
                if y { max.y } else { min.y },
                if z { max.z } else { min.z },
            )
        };
        let verts = vec![
            p(false, false, false),
            p(true, false, false),
            p(true, true, false),
            p(false, true, false),
            p(false, false, true),
            p(true, false, true),
            p(true, true, true),
            p(false, true, true),
        ];
        let quads: [([u32; 4], V3, V3); 6] = [
            ([0, 3, 2, 1], -V3::Z, min),
            ([4, 5, 6, 7], V3::Z, max),
            ([0, 1, 5, 4], -V3::Y, min),
            ([2, 3, 7, 6], V3::Y, max),
            ([0, 4, 7, 3], -V3::X, min),
            ([1, 2, 6, 5], V3::X, max),
        ];
        let mut m = Mesh {
            verts,
            ..Mesh::default()
        };
        for (s, (q, n, o)) in quads.into_iter().enumerate() {
            m.surfaces.push(Surface::Plane {
                origin: o,
                normal: n,
            });
            m.tris.push([q[0], q[1], q[2]]);
            m.tris.push([q[0], q[2], q[3]]);
            m.tri_surface.push(s as u32);
            m.tri_surface.push(s as u32);
        }
        m
    }
    /// Vertex positions keyed exactly, for callers that need to find shared vertices.
    pub fn vertex_index(&self) -> BTreeMap<(u64, u64, u64), u32> {
        self.verts
            .iter()
            .enumerate()
            .map(|(i, p)| ((p.x.to_bits(), p.y.to_bits(), p.z.to_bits()), i as u32))
            .collect()
    }
    /// Place this mesh with a frame (local coordinates → world).
    pub fn placed(&self, f: &Frame) -> Mesh {
        let x = Xform {
            m: [
                [f.x.x, f.y.x, f.z.x],
                [f.x.y, f.y.y, f.z.y],
                [f.x.z, f.y.z, f.z.z],
            ],
            t: f.origin,
        };
        self.transformed(&x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_box_measures_as_a_box() {
        let m = Mesh::cuboid(v3(0.0, 0.0, 0.0), v3(2.0, 3.0, 4.0));
        assert!(m.is_watertight());
        assert!((m.volume() - 24.0).abs() < 1e-12);
        assert!((m.area() - 2.0 * (6.0 + 8.0 + 12.0)).abs() < 1e-12);
        assert!((m.center_of_mass().unwrap() - v3(1.0, 1.5, 2.0)).len() < 1e-12);
        let mirrored = m.transformed(&Xform::mirror(V3::ZERO, V3::X));
        assert!(
            (mirrored.volume() - 24.0).abs() < 1e-12,
            "a mirror must keep it outward"
        );
        assert!(mirrored.is_watertight());
    }
}
