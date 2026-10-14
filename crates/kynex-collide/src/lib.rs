//! Where meshes run into each other: a robot's links into a fixture, into
//! another robot, or into itself.
//!
//! Exact on the triangles: two bodies are in contact when a triangle of one
//! passes through a triangle of the other, by more than [`PENETRATION`] —
//! so a part resting flush on a table is not, and one pushed a hair into it
//! is. Each mesh keeps a bounding volume hierarchy of its triangles in its
//! own space, made once however it moves; the bodies of a scene are found
//! by their boxes first.
//!
//! What it does not see: one closed mesh wholly inside another, with no
//! surfaces crossing. Along a path the surfaces cross on the way in, and a
//! path is sampled finely enough to catch it there.
//!
//! How near two bodies come ([`nearest`]) is exact on the triangles too:
//! the least distance between any two, found among the pairs the
//! hierarchy brings within reach.

use std::sync::Arc;

use kreate_math::{Aabb, Bvh, DAffine3, DVec3, Vec3};
use kreate_mesh::MeshData;

/// How far, in metres, one surface must pass through another to count.
pub const PENETRATION: f64 = 1e-5;

/// A mesh's triangles, in its own space, ready to be tested.
#[derive(Debug)]
pub struct Shape {
    positions: Vec<Vec3>,
    triangles: Vec<[u32; 3]>,
    bvh: Bvh,
}

impl Shape {
    pub fn of(mesh: &MeshData) -> Self {
        let positions = mesh.positions().to_vec();
        let triangles: Vec<[u32; 3]> = mesh.triangulate().indices.as_chunks::<3>().0.to_vec();
        Self::new(positions, triangles)
    }

    pub fn new(positions: Vec<Vec3>, triangles: Vec<[u32; 3]>) -> Self {
        let bvh = Bvh::build(&positions, &triangles);
        Self {
            positions,
            triangles,
            bvh,
        }
    }

    pub fn triangles(&self) -> usize {
        self.triangles.len()
    }

    fn triangle(&self, i: u32) -> [Vec3; 3] {
        let t = self.triangles[i as usize];
        [
            self.positions[t[0] as usize],
            self.positions[t[1] as usize],
            self.positions[t[2] as usize],
        ]
    }
}

/// A shape somewhere in the world.
#[derive(Clone, Debug)]
pub struct Body {
    pub uid: u64,
    pub shape: Arc<Shape>,
    pub world: DAffine3,
}

impl Body {
    /// Its box in the world: the corners of its own box, carried there.
    pub fn bounds(&self) -> Aabb {
        let local = self.shape.bvh.bounds();
        if local.is_empty() {
            return Aabb::EMPTY;
        }
        Aabb::from_points(
            local
                .corners()
                .map(|c| self.world.transform_point3(c.as_dvec3()).as_vec3()),
        )
    }
}

/// Where `a` and `b` meet, if they do: a point where they cross, in the
/// world — near it, for two large triangles.
pub fn contact(a: &Body, b: &Body) -> Option<DVec3> {
    // The one with fewer triangles is carried into the other's space and
    // looked up in its hierarchy, a triangle at a time.
    let (big, small) = if a.shape.triangles() >= b.shape.triangles() {
        (a, b)
    } else {
        (b, a)
    };
    if small.shape.triangles() == 0 || !big.bounds().intersects(&small.bounds()) {
        return None;
    }
    let into_big = big.world.inverse() * small.world;
    let mut near = Vec::new();
    for i in 0..small.shape.triangles() as u32 {
        let tri = small
            .shape
            .triangle(i)
            .map(|p| into_big.transform_point3(p.as_dvec3()));
        let reach = Aabb::from_points(tri.map(|p| p.as_vec3())).expanded(1e-6);
        big.shape.bvh.overlaps(&reach, &mut near);
        for &j in &near {
            let other = big.shape.triangle(j).map(|p| p.as_dvec3());
            if crossing(&tri, &other) {
                // Within both triangles' boxes: where they cross is there.
                let lo = |t: &[DVec3; 3]| t[0].min(t[1]).min(t[2]);
                let hi = |t: &[DVec3; 3]| t[0].max(t[1]).max(t[2]);
                let mid = (lo(&tri).max(lo(&other)) + hi(&tri).min(hi(&other))) / 2.0;
                return Some(big.world.transform_point3(mid));
            }
        }
    }
    None
}

/// How near `a` and `b` come, if nearer than `within` metres: the distance,
/// and the nearest points on each, in the world. Zero where they cross.
pub fn nearest(a: &Body, b: &Body, within: f64) -> Option<(f64, DVec3, DVec3)> {
    let (big, small, swapped) = if a.shape.triangles() >= b.shape.triangles() {
        (a, b, false)
    } else {
        (b, a, true)
    };
    if small.shape.triangles() == 0
        || !big
            .bounds()
            .expanded(within as f32)
            .intersects(&small.bounds())
    {
        return None;
    }
    // Looked up in the big one's own space, its reach grown by the most
    // that space is shrunk from the world's.
    let m = big.world.matrix3;
    let shrink = m
        .x_axis
        .length()
        .min(m.y_axis.length())
        .min(m.z_axis.length());
    if shrink <= 0.0 {
        return None;
    }
    let reach_local = (within / shrink) as f32;
    let into_big = big.world.inverse() * small.world;
    let mut best: Option<(f64, DVec3, DVec3)> = None;
    let mut near = Vec::new();
    for i in 0..small.shape.triangles() as u32 {
        let local = small.shape.triangle(i).map(|p| p.as_dvec3());
        let in_big = local.map(|p| into_big.transform_point3(p));
        let reach = Aabb::from_points(in_big.map(|p| p.as_vec3())).expanded(reach_local + 1e-6);
        big.shape.bvh.overlaps(&reach, &mut near);
        if near.is_empty() {
            continue;
        }
        let tri = local.map(|p| small.world.transform_point3(p));
        for &j in &near {
            let other = big
                .shape
                .triangle(j)
                .map(|p| big.world.transform_point3(p.as_dvec3()));
            let (d, p, q) = triangle_distance(&tri, &other);
            if d < within && best.is_none_or(|b| d < b.0) {
                best = Some((d, p, q));
            }
        }
    }
    // Found from the small one's side: its point first.
    best.map(|(d, on_small, on_big)| {
        if swapped {
            (d, on_small, on_big)
        } else {
            (d, on_big, on_small)
        }
    })
}

/// The least distance between two triangles, and the nearest points on
/// each: zero where they cross. Otherwise it is between a corner of one and
/// the other's face, or between an edge of each.
fn triangle_distance(a: &[DVec3; 3], b: &[DVec3; 3]) -> (f64, DVec3, DVec3) {
    if crossing(a, b) {
        let lo = |t: &[DVec3; 3]| t[0].min(t[1]).min(t[2]);
        let hi = |t: &[DVec3; 3]| t[0].max(t[1]).max(t[2]);
        let mid = (lo(a).max(lo(b)) + hi(a).min(hi(b))) / 2.0;
        return (0.0, mid, mid);
    }
    let mut best = (f64::INFINITY, DVec3::ZERO, DVec3::ZERO);
    let mut keep = |p: DVec3, q: DVec3| {
        let d = p.distance(q);
        if d < best.0 {
            best = (d, p, q);
        }
    };
    for p in a {
        keep(*p, closest_on_triangle(*p, b));
    }
    for q in b {
        keep(closest_on_triangle(*q, a), *q);
    }
    for i in 0..3 {
        for j in 0..3 {
            let (p, q) = closest_between_segments(a[i], a[(i + 1) % 3], b[j], b[(j + 1) % 3]);
            keep(p, q);
        }
    }
    best
}

/// The point of triangle `t` nearest `p` (Ericson, Real-Time Collision
/// Detection, 5.1.5).
fn closest_on_triangle(p: DVec3, t: &[DVec3; 3]) -> DVec3 {
    let [a, b, c] = *t;
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = va + vb + vc;
    if denom.abs() < 1e-300 {
        return a;
    }
    a + ab * (vb / denom) + ac * (vc / denom)
}

/// The nearest points of segments `p1`–`q1` and `p2`–`q2` (Ericson,
/// 5.1.9).
fn closest_between_segments(p1: DVec3, q1: DVec3, p2: DVec3, q2: DVec3) -> (DVec3, DVec3) {
    let (d1, d2, r) = (q1 - p1, q2 - p2, p1 - p2);
    let (a, e, f) = (d1.length_squared(), d2.length_squared(), d2.dot(r));
    const TINY: f64 = 1e-18;
    let (s, t) = if a <= TINY && e <= TINY {
        (0.0, 0.0)
    } else if a <= TINY {
        (0.0, (f / e).clamp(0.0, 1.0))
    } else {
        let c = d1.dot(r);
        if e <= TINY {
            ((-c / a).clamp(0.0, 1.0), 0.0)
        } else {
            let b = d1.dot(d2);
            let denom = a * e - b * b;
            let mut s = if denom > TINY {
                ((b * f - c * e) / denom).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let mut t = (b * s + f) / e;
            if t < 0.0 {
                t = 0.0;
                s = (-c / a).clamp(0.0, 1.0);
            } else if t > 1.0 {
                t = 1.0;
                s = ((b - c) / a).clamp(0.0, 1.0);
            }
            (s, t)
        }
    };
    (p1 + d1 * s, p2 + d2 * t)
}

/// Whether two triangles pass through each other by more than
/// [`PENETRATION`], by the separating axis test: they do unless some axis
/// keeps their shadows apart.
///
/// Two triangles in one plane are faces lying on each other. Facing apart,
/// they are two bodies resting flush, which is not a contact. Facing the
/// same way and overlapping, the bodies behind them share the space behind:
/// two equal blocks pushed into each other from the side meet nowhere else,
/// their tops and bottoms in the same planes.
fn crossing(a: &[DVec3; 3], b: &[DVec3; 3]) -> bool {
    let ea = [a[1] - a[0], a[2] - a[1], a[0] - a[2]];
    let eb = [b[1] - b[0], b[2] - b[1], b[0] - b[2]];
    let na = ea[0].cross(ea[1]);
    let nb = eb[0].cross(eb[1]);
    let parallel =
        na.cross(nb).length_squared() <= 1e-12 * na.length_squared() * nb.length_squared();
    let unit = na.normalize_or_zero();
    let coplanar = parallel && b.iter().all(|p| (*p - a[0]).dot(unit).abs() <= PENETRATION);
    let axes: Vec<DVec3> = if coplanar {
        if na.dot(nb) < 0.0 {
            return false;
        }
        // In the plane, the edges' own normals part them.
        ea.iter().chain(&eb).map(|e| na.cross(*e)).collect()
    } else {
        let mut axes = vec![na, nb];
        for x in &ea {
            for y in &eb {
                axes.push(x.cross(*y));
            }
        }
        axes
    };
    for axis in axes {
        let len = axis.length();
        if len < 1e-12 {
            continue;
        }
        let axis = axis / len;
        let span = |t: &[DVec3; 3]| {
            let d = t.map(|p| p.dot(axis));
            (d[0].min(d[1]).min(d[2]), d[0].max(d[1]).max(d[2]))
        };
        let (a0, a1) = span(a);
        let (b0, b1) = span(b);
        if a1 < b0 + PENETRATION || b1 < a0 + PENETRATION {
            return false;
        }
    }
    true
}

/// Bodies that stand still while something moves among them, found by
/// their boxes first.
pub struct Obstacles {
    bodies: Vec<Body>,
    /// A hierarchy over the bodies' boxes: each box is a "triangle" from
    /// its least corner to its greatest and back, whose bounds are the box.
    boxes: Bvh,
}

impl Obstacles {
    pub fn new(bodies: Vec<Body>) -> Self {
        let mut corners = Vec::with_capacity(bodies.len() * 2);
        let mut boxes = Vec::with_capacity(bodies.len());
        for (i, body) in bodies.iter().enumerate() {
            let b = body.bounds();
            let (lo, hi) = if b.is_empty() {
                // Never met: a box nowhere.
                (Vec3::splat(f32::MAX), Vec3::splat(f32::MAX))
            } else {
                (b.min, b.max)
            };
            corners.push(lo);
            corners.push(hi);
            let i = i as u32 * 2;
            boxes.push([i, i + 1, i]);
        }
        Self {
            boxes: Bvh::build(&corners, &boxes),
            bodies,
        }
    }

    pub fn len(&self) -> usize {
        self.bodies.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bodies.is_empty()
    }

    /// The obstacle `body` comes nearest, if nearer than `within`, but
    /// those `skip` names: its identity, the distance, and the nearest
    /// points on `body` and on it.
    pub fn nearest(
        &self,
        body: &Body,
        within: f64,
        skip: impl Fn(u64) -> bool,
    ) -> Option<(u64, f64, DVec3, DVec3)> {
        let mut near = Vec::new();
        self.boxes
            .overlaps(&body.bounds().expanded(within as f32), &mut near);
        near.iter()
            .map(|&i| &self.bodies[i as usize])
            .filter(|other| !skip(other.uid))
            .filter_map(|other| nearest(body, other, within).map(|(d, p, q)| (other.uid, d, p, q)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// Every obstacle `body` is in contact with, but those `skip` names:
    /// each by identity, with where they meet.
    pub fn contacts(&self, body: &Body, skip: impl Fn(u64) -> bool) -> Vec<(u64, DVec3)> {
        let mut near = Vec::new();
        self.boxes.overlaps(&body.bounds(), &mut near);
        near.sort_unstable();
        near.iter()
            .map(|&i| &self.bodies[i as usize])
            .filter(|other| !skip(other.uid))
            .filter_map(|other| contact(body, other).map(|at| (other.uid, at)))
            .collect()
    }
}

#[cfg(test)]
mod tests;
