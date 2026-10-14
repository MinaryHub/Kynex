//! Where an arm reaches: the working range a datasheet draws, worked out
//! from the arm's description.
//!
//! The arm's first joint turns it about the base frame's Z; what it reaches
//! is then the same at every turn of that joint, a section in (distance
//! from the axis, height) swept round it. The section is found by moving
//! the shoulder and the elbow over their whole ranges, with the wrist bent
//! a few ways, and marking where the tool link (the flange) gets to on a
//! grid; then, height by height, from the nearest it comes to the axis to
//! the farthest. The arm running into itself or the floor is not looked at,
//! as a datasheet's range does not.

use kreate_math::DVec3;

use crate::{Kinematics, Tool};

/// The grid the section is marked on, metres.
pub const CELL: f64 = 0.02;

/// Steps over the shoulder's and the elbow's range.
const STEPS: usize = 90;

/// Where an arm reaches, in its base frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Reach {
    /// The section, (distance from the axis, height) in metres, round its
    /// edge: out along its far side from the bottom up, then back down its
    /// near side.
    pub section: Vec<[f64; 2]>,
    /// The first joint's range, radians: the section swept from the one to
    /// the other. A full turn or more is all the way round.
    pub turn: (f64, f64),
    /// The farthest it reaches from the axis, metres.
    pub farthest: f64,
}

/// Where the arm reaches with `tool`, its base frame the object `base`.
/// `None` for an arm of fewer than three joints, or whose first does not
/// turn it about the base frame's Z.
pub fn reach(kin: &Kinematics, tool: &Tool, base: u64) -> Option<Reach> {
    let n = kin.vars.len();
    if n < 3 || kin.vars[0].kind.slides() {
        return None;
    }
    let mut q = vec![0.0; n];
    for (i, v) in kin.vars.iter().enumerate() {
        q[i] = v.clamp(0.0);
    }
    let base_at = kin.world_of(&q, base)?;
    let in_base = |q: &[f64]| (base_at.inverse() * kin.pose(q, tool)).translation;
    // The first joint turns the arm about Z: a turn of it moves the tool
    // round Z at the same height and distance.
    let p0 = in_base(&q);
    let mut turned = q.clone();
    turned[0] = kin.vars[0].clamp(q[0] + 0.5);
    let p1 = in_base(&turned);
    let flat = |p: DVec3| (p.x * p.x + p.y * p.y).sqrt();
    if (p0.z - p1.z).abs() > 1e-6 || (flat(p0) - flat(p1)).abs() > 1e-6 {
        return None;
    }

    // A joint's range, at most a turn: past that the arm only comes round
    // to where it was.
    let range = |i: usize| {
        let v = &kin.vars[i];
        let (lo, hi) = if v.limited() {
            (v.lower, v.upper)
        } else {
            (-std::f64::consts::PI, std::f64::consts::PI)
        };
        let lo = lo.max(-std::f64::consts::PI);
        let hi = hi.min(lo + std::f64::consts::TAU);
        (lo, hi)
    };
    let sweep = |i: usize| {
        let (lo, hi) = range(i);
        (0..=STEPS).map(move |k| lo + (hi - lo) * k as f64 / STEPS as f64)
    };
    // The wrist bent a few ways, in the arm's plane and across it.
    let bends = [
        -std::f64::consts::FRAC_PI_2,
        0.0,
        std::f64::consts::FRAC_PI_2,
    ];
    let wrist: Vec<(usize, f64)> = (3..n.min(5)).flat_map(|i| bends.map(|b| (i, b))).collect();

    let mut marked: std::collections::BTreeMap<i64, (i64, i64)> = Default::default();
    let mut mark = |p: DVec3| {
        let (r, z) = ((flat(p) / CELL).round() as i64, (p.z / CELL).round() as i64);
        let e = marked.entry(z).or_insert((r, r));
        e.0 = e.0.min(r);
        e.1 = e.1.max(r);
    };
    let mut poses = vec![q.clone()];
    if n > 3 {
        poses.clear();
        // Every pair of the wrist's bends, the others straight.
        for (a, &(i, x)) in wrist.iter().enumerate() {
            for &(j, y) in &wrist[a..] {
                if i == j && x != y {
                    continue;
                }
                let mut w = q.clone();
                w[i] = kin.vars[i].clamp(x);
                w[j] = kin.vars[j].clamp(y);
                poses.push(w);
            }
        }
    }
    let mut farthest: f64 = 0.0;
    for wrist in &poses {
        for a in sweep(1) {
            for b in sweep(2) {
                let mut at = wrist.clone();
                at[1] = a;
                at[2] = b;
                let p = in_base(&at);
                farthest = farthest.max(flat(p));
                mark(p);
            }
        }
    }

    // Up the far side, down the near.
    let rows: Vec<(f64, f64, f64)> = marked
        .iter()
        .map(|(z, (near, far))| {
            let z = *z as f64 * CELL;
            (
                (*near as f64 * CELL - CELL / 2.0).max(0.0),
                *far as f64 * CELL + CELL / 2.0,
                z,
            )
        })
        .collect();
    let mut section: Vec<[f64; 2]> = rows.iter().map(|(_, far, z)| [*far, *z]).collect();
    section.extend(rows.iter().rev().map(|(near, _, z)| [*near, *z]));
    let first = &kin.vars[0];
    let turn = if first.limited() {
        (first.lower, first.upper)
    } else {
        (-std::f64::consts::PI, std::f64::consts::PI)
    };
    Some(Reach {
        section,
        turn,
        farthest,
    })
}

#[cfg(test)]
mod tests;
