//! Paths along a part's edges, from its mesh: for deburring, sealing or
//! welding along them, as Visual Components' OLP makes them from CAD.
//!
//! An edge is where two faces meet at a sharp angle (or a face has no
//! neighbour). Edges that meet end to end, two at a vertex, are one chain;
//! a chain ends where three or more meet, or at a loose end. Along a chain
//! the tool points into the edge, along the bisector of the faces on
//! either side, and runs along it.

use std::collections::{BTreeMap, HashMap};

use kreate_math::{DMat3, DQuat, DVec3};
use kreate_scene::{Document, ObjectData, Uid};

/// Faces meeting at more than this, radians, make an edge.
pub const SHARP: f64 = 30.0 * std::f64::consts::PI / 180.0;

/// How far a simplified chain may stray from the edge, metres.
pub const TOLERANCE: f64 = 0.0005;

/// Edges of a mesh joined end to end.
#[derive(Clone, Debug, PartialEq)]
pub struct Chain {
    pub points: Vec<DVec3>,
    /// At each point, the way out of the part: the faces' bisector there.
    pub normals: Vec<DVec3>,
    /// Whether it comes back to where it began (its last point is its first
    /// again).
    pub closed: bool,
}

impl Chain {
    pub fn length(&self) -> f64 {
        self.points.windows(2).map(|w| w[0].distance(w[1])).sum()
    }

    /// How far `p` is from the chain.
    pub fn distance(&self, p: DVec3) -> f64 {
        self.points
            .windows(2)
            .map(|w| segment_distance(p, w[0], w[1]))
            .fold(f64::INFINITY, f64::min)
    }
}

fn segment_distance(p: DVec3, a: DVec3, b: DVec3) -> f64 {
    let ab = b - a;
    let t = if ab.length_squared() > 0.0 {
        ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.distance(a + ab * t)
}

/// The chains of edges of the mesh of `positions` and `triangles`, in the
/// same frame as the positions.
pub fn chains(positions: &[DVec3], triangles: &[[u32; 3]]) -> Vec<Chain> {
    // One vertex where several stand at the same place (a mesh splits them
    // at its seams).
    let mut welded: HashMap<[i64; 3], u32> = HashMap::new();
    let mut points: Vec<DVec3> = Vec::new();
    let mut index = Vec::with_capacity(positions.len());
    for p in positions {
        let key = (*p * 1e6).round().to_array().map(|v| v as i64);
        let k = *welded.entry(key).or_insert_with(|| {
            points.push(*p);
            (points.len() - 1) as u32
        });
        index.push(k);
    }
    // Each edge, and the faces on it.
    let mut normals = Vec::new();
    let mut faces: BTreeMap<(u32, u32), Vec<usize>> = BTreeMap::new();
    for t in triangles {
        let [a, b, c] = t.map(|i| index[i as usize]);
        if a == b || b == c || a == c {
            continue;
        }
        let n = (points[b as usize] - points[a as usize])
            .cross(points[c as usize] - points[a as usize])
            .normalize_or_zero();
        if n == DVec3::ZERO {
            continue;
        }
        let f = normals.len();
        normals.push(n);
        for (u, v) in [(a, b), (b, c), (c, a)] {
            faces.entry((u.min(v), u.max(v))).or_default().push(f);
        }
    }
    // The sharp ones, with the way out of the part along each.
    let mut sharp: BTreeMap<(u32, u32), DVec3> = BTreeMap::new();
    for (edge, on) in &faces {
        let out = match on.as_slice() {
            [f] => normals[*f],
            [f, g] if normals[*f].angle_between(normals[*g]) > SHARP => {
                (normals[*f] + normals[*g]).normalize_or_zero()
            }
            [_, _] => continue,
            more => more
                .iter()
                .map(|f| normals[*f])
                .sum::<DVec3>()
                .normalize_or_zero(),
        };
        sharp.insert(*edge, out);
    }
    let mut at: HashMap<u32, Vec<(u32, u32)>> = HashMap::new();
    for edge in sharp.keys() {
        at.entry(edge.0).or_default().push(*edge);
        at.entry(edge.1).or_default().push(*edge);
    }

    // Walk each chain from an end, then what is left: loops.
    let mut used: std::collections::HashSet<(u32, u32)> = Default::default();
    let mut out = Vec::new();
    let walk = |start: u32, first: (u32, u32), used: &mut std::collections::HashSet<(u32, u32)>| {
        let mut vertices = vec![start];
        let mut outs: Vec<DVec3> = Vec::new();
        let mut edge = first;
        let mut here = start;
        loop {
            used.insert(edge);
            outs.push(sharp[&edge]);
            let next = if edge.0 == here { edge.1 } else { edge.0 };
            vertices.push(next);
            here = next;
            let around = &at[&here];
            if around.len() != 2 || here == start {
                break;
            }
            match around.iter().find(|e| !used.contains(*e)) {
                Some(e) => edge = *e,
                None => break,
            }
        }
        let closed = vertices.len() > 2 && vertices.first() == vertices.last();
        // At each point, the edges' ways out on either side of it.
        let n = vertices.len();
        let normals: Vec<DVec3> = (0..n)
            .map(|i| {
                let before = if i > 0 {
                    Some(outs[i - 1])
                } else if closed {
                    outs.last().copied()
                } else {
                    None
                };
                let after = if i < n - 1 {
                    Some(outs[i])
                } else if closed {
                    outs.first().copied()
                } else {
                    None
                };
                (before.unwrap_or(DVec3::ZERO) + after.unwrap_or(DVec3::ZERO)).normalize_or_zero()
            })
            .collect();
        Chain {
            points: vertices.iter().map(|v| points[*v as usize]).collect(),
            normals,
            closed,
        }
    };
    let ends: Vec<u32> = {
        let mut v: Vec<u32> = at
            .iter()
            .filter(|(_, e)| e.len() != 2)
            .map(|(v, _)| *v)
            .collect();
        v.sort();
        v
    };
    for start in ends {
        for edge in at[&start].clone() {
            if !used.contains(&edge) {
                out.push(walk(start, edge, &mut used));
            }
        }
    }
    for edge in sharp.keys().copied().collect::<Vec<_>>() {
        if !used.contains(&edge) {
            out.push(walk(edge.0, edge, &mut used));
        }
    }
    out
}

/// How far from an edge a point picked on a part may be, metres, for the
/// edge to be the one meant.
pub const NEAR: f64 = 0.03;

/// The edge of object `object` in `doc` nearest `at`, a point picked on
/// it, in the world.
pub fn picked(doc: &Document, object: u64, at: DVec3) -> Result<Chain, String> {
    let id = doc
        .object_by_uid(Uid::from_raw(object))
        .ok_or("no such object")?;
    // Its own shape, or — an object of parts — theirs.
    let mut all = Vec::new();
    for (part, o) in doc.objects() {
        if part != id && !crate::cell::is_below(doc, part, id) {
            continue;
        }
        let ObjectData::Mesh(mesh) = o.data else {
            continue;
        };
        let Some(mesh) = doc.mesh(mesh) else { continue };
        let world = doc.world_transform(part);
        let positions: Vec<DVec3> = mesh
            .positions()
            .iter()
            .map(|p| world.transform_point3(p.as_dvec3()))
            .collect();
        let triangles: Vec<[u32; 3]> = mesh.triangulate().indices.as_chunks::<3>().0.to_vec();
        all.extend(chains(&positions, &triangles));
    }
    if all.is_empty() {
        return Err("형상이 있는 물체가 아닙니다".into());
    }
    match nearest(&all, at) {
        Some((chain, gap)) if gap <= NEAR => Ok(chain.clone()),
        _ => Err("가까운 모서리가 없습니다: 모서리 가까이를 누르세요".into()),
    }
}

/// The chain nearest `p`, and how far it is.
pub fn nearest(chains: &[Chain], p: DVec3) -> Option<(&Chain, f64)> {
    chains
        .iter()
        .map(|c| (c, c.distance(p)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// The points of `chain` the tool goes through, as few as keep it within
/// `tolerance` of the edge, each with the tool's turn there: its Z into the
/// edge along the faces' bisector, its X along the way it goes.
pub fn poses(chain: &Chain, tolerance: f64) -> Vec<(DVec3, DQuat)> {
    let n = chain.points.len();
    if n < 2 {
        return Vec::new();
    }
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;
    simplify(&chain.points, 0, n - 1, tolerance, &mut keep);
    // Corners are kept too, where the way out turns: the tool turns there.
    for (i, w) in chain.normals.windows(3).enumerate() {
        if w[1].angle_between(w[0]) > 0.05 || w[1].angle_between(w[2]) > 0.05 {
            keep[i + 1] = true;
        }
    }
    let kept: Vec<usize> = (0..n).filter(|i| keep[*i]).collect();
    kept.iter()
        .enumerate()
        .map(|(k, &i)| {
            let p = chain.points[i];
            let prev = if k > 0 { chain.points[kept[k - 1]] } else { p };
            let next = if k + 1 < kept.len() {
                chain.points[kept[k + 1]]
            } else {
                p
            };
            let along = if k + 1 < kept.len() {
                next - p
            } else {
                p - prev
            };
            let z = -chain.normals[i];
            let x = (along - z * along.dot(z)).normalize_or_zero();
            let x = if x == DVec3::ZERO {
                z.any_orthonormal_vector()
            } else {
                x
            };
            let y = z.cross(x);
            (p, DQuat::from_mat3(&DMat3::from_cols(x, y, z)))
        })
        .collect()
}

/// Marks the points of `points[a..=b]` to keep so that a line through
/// those kept strays no more than `tolerance` from any (Douglas–Peucker).
fn simplify(points: &[DVec3], a: usize, b: usize, tolerance: f64, keep: &mut [bool]) {
    if b <= a + 1 {
        return;
    }
    let (far, at) = (a + 1..b)
        .map(|i| (segment_distance(points[i], points[a], points[b]), i))
        .fold((0.0, a), |m, x| if x.0 > m.0 { x } else { m });
    if far > tolerance {
        keep[at] = true;
        simplify(points, a, at, tolerance, keep);
        simplify(points, at, b, tolerance, keep);
    }
}

#[cfg(test)]
mod tests;
