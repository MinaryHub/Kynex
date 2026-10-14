//! Moves between taught poses, timed as a robot makes them.
//!
//! A program is a list of waypoints, each a set of joint positions and the
//! move that reaches it:
//!
//! - **Joint** (MoveJ): every joint at once, from one position to the next,
//!   all starting and stopping together. The tool's path is whatever curve
//!   that makes; it is the quickest move there is.
//! - **Linear** (MoveL): the tool along a straight line, its orientation
//!   turning evenly on the way. The joints are worked out along the line,
//!   so a line through a point the arm cannot reach, or past a singularity,
//!   shows up here and not on the shop floor. An arm with fewer than six
//!   joints cannot turn its tool every way, so for it the line holds the
//!   tool point only and the orientation goes where the joints take it.
//!
//! Each move speeds up, cruises and slows down (a trapezoid). Where a line
//! would need a joint faster than it goes, the move is slowed to what the
//! joint allows and the plan says so.
//!
//! A waypoint with a blend radius is passed without stopping: the next move
//! starts while the tool is still that close to the waypoint, and for as
//! long as both run, the joints move by the sum of both moves' changes. The
//! tool rounds the corner within about the radius, and the time the two
//! overlap is saved (as a controller's blend radius does).

use kreate_math::{DQuat, DVec3};

use crate::{Goal, Kinematics, Options, Tool};

/// How quickly a joint reaches its top speed: its acceleration is this many
/// times its top speed, a second. Three: full speed in a third of a second.
pub const JOINT_ACCELERATION: f64 = 3.0;

/// A linear move's acceleration, metres a second squared: a UR's default.
pub const LINEAR_ACCELERATION: f64 = 1.2;

/// How fast a linear move turns the tool, radians a second, and how quickly
/// it gets there.
pub const ANGULAR_SPEED: f64 = 1.0;
pub const ANGULAR_ACCELERATION: f64 = 2.0;

/// The spacing of a linear move's samples, seconds: the joints between two
/// are interpolated.
pub const SAMPLE: f64 = 0.02;

/// Past this much, radians or metres, a linear move that ends away from its
/// taught joints has the arm in another configuration: a jump, not drift.
pub const JUMP_TURN: f64 = 0.05;
pub const JUMP_SLIDE: f64 = 0.005;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveKind {
    Joint,
    Linear,
}

/// A taught pose and how to get to it.
#[derive(Clone, Debug, PartialEq)]
pub struct Waypoint {
    pub kind: MoveKind,
    /// One per [`Kinematics::vars`].
    pub positions: Vec<f64>,
    /// For a joint move, the share of each joint's top speed (0 to 1]; for a
    /// linear move, the tool's speed in metres a second.
    pub speed: f64,
    /// Metres: how close the tool comes to this waypoint before the next
    /// move takes over, without stopping. Zero stops at it.
    pub blend: f64,
}

/// What is wrong with a move. The move is still planned, as near as it
/// goes, so that it can be played and looked at.
#[derive(Clone, Debug, PartialEq)]
pub enum Problem {
    /// The line passes where the tool cannot be: how far along it (0 to 1)
    /// first, and the most it misses by, in metres and in radians.
    Unreachable {
        along: f64,
        position: f64,
        rotation: f64,
    },
    /// A joint would have to move faster than it goes: the move takes this
    /// many times as long as its speed asks.
    Slowed { joint: usize, factor: f64 },
    /// The line ends with the arm in another configuration than the one
    /// taught: at the end it would have to jump, by this much.
    Jump { joint: usize, by: f64 },
}

/// A speed-up, cruise and slow-down over a distance.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Profile {
    distance: f64,
    accel: f64,
    /// Time spent speeding up, and again slowing down.
    ramp: f64,
    duration: f64,
}

impl Profile {
    fn new(distance: f64, speed: f64, accel: f64) -> Self {
        let distance = distance.abs();
        if distance < 1e-12 || speed <= 0.0 || accel <= 0.0 {
            return Self {
                distance: 0.0,
                accel: 1.0,
                ramp: 0.0,
                duration: 0.0,
            };
        }
        let ramp = speed / accel;
        if distance < speed * ramp {
            // Never reaches full speed: a triangle.
            let ramp = (distance / accel).sqrt();
            Self {
                distance,
                accel,
                ramp,
                duration: 2.0 * ramp,
            }
        } else {
            Self {
                distance,
                accel,
                ramp,
                duration: distance / speed + ramp,
            }
        }
    }

    /// The share of the distance covered at `t`, the profile stretched to
    /// take `over` seconds instead.
    fn fraction(&self, t: f64, over: f64) -> f64 {
        if self.duration <= 0.0 || over <= 0.0 {
            return 1.0;
        }
        let t = (t * self.duration / over).clamp(0.0, self.duration);
        let (a, r, total) = (self.accel, self.ramp, self.duration);
        let cruise = a * r;
        let s = if t < r {
            0.5 * a * t * t
        } else if t < total - r {
            0.5 * a * r * r + cruise * (t - r)
        } else {
            let left = total - t;
            self.distance - 0.5 * a * left * left
        };
        (s / self.distance).clamp(0.0, 1.0)
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Path {
    Joint {
        from: Vec<f64>,
        to: Vec<f64>,
        profile: Profile,
    },
    /// Joint positions at even times over the move, first and last
    /// included.
    Linear { samples: Vec<Vec<f64>> },
}

/// One move of a plan.
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub kind: MoveKind,
    /// The waypoint it reaches, by index.
    pub to: usize,
    /// Seconds from the start of the plan.
    pub start: f64,
    pub duration: f64,
    pub problems: Vec<Problem>,
    path: Path,
}

impl Segment {
    /// The joint positions it starts from.
    fn origin(&self) -> &[f64] {
        match &self.path {
            Path::Joint { from, .. } => from,
            Path::Linear { samples } => &samples[0],
        }
    }

    fn at(&self, t: f64) -> Vec<f64> {
        match &self.path {
            Path::Joint { from, to, profile } => {
                let k = profile.fraction(t, self.duration);
                from.iter().zip(to).map(|(a, b)| a + (b - a) * k).collect()
            }
            Path::Linear { samples } => {
                let last = samples.len() - 1;
                if last == 0 || self.duration <= 0.0 {
                    return samples[last].clone();
                }
                let x = (t / self.duration).clamp(0.0, 1.0) * last as f64;
                let i = (x.floor() as usize).min(last - 1);
                let k = x - i as f64;
                samples[i]
                    .iter()
                    .zip(&samples[i + 1])
                    .map(|(a, b)| a + (b - a) * k)
                    .collect()
            }
        }
    }
}

/// A program, timed: where every joint is at any moment of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    /// Where it starts: the first waypoint.
    pub start: Vec<f64>,
    pub segments: Vec<Segment>,
    /// Seconds, start to end: the cycle time.
    pub duration: f64,
}

impl Plan {
    /// The joint positions `t` seconds in, held to the plan's ends: where
    /// it starts, moved on by every move started by then — two at once
    /// where they blend.
    pub fn at(&self, t: f64) -> Vec<f64> {
        let t = t.clamp(0.0, self.duration);
        let mut q = self.start.clone();
        for seg in &self.segments {
            if seg.start > t {
                break;
            }
            let now = seg.at((t - seg.start).min(seg.duration));
            for ((q, a), o) in q.iter_mut().zip(&now).zip(seg.origin()) {
                *q += a - o;
            }
        }
        q
    }

    /// The move under way `t` seconds in, by its index: the latest begun.
    pub fn segment_at(&self, t: f64) -> usize {
        self.segments
            .iter()
            .rposition(|s| s.start <= t)
            .unwrap_or(0)
    }
}

/// Times a program. With `close`, the plan ends with a move back to the
/// first waypoint, made as the first waypoint says: a cycle that can run
/// again.
pub fn plan(kin: &Kinematics, tool: &Tool, waypoints: &[Waypoint], close: bool) -> Plan {
    let start = waypoints
        .first()
        .map_or_else(|| kin.current.clone(), |w| w.positions.clone());
    let mut moves: Vec<(usize, usize)> = (1..waypoints.len()).map(|i| (i - 1, i)).collect();
    if close && waypoints.len() >= 2 {
        moves.push((waypoints.len() - 1, 0));
    }
    let mut segments: Vec<Segment> = Vec::new();
    let mut at = 0.0;
    for (from, to) in moves {
        let (a, b) = (&waypoints[from], &waypoints[to]);
        let mut seg = match b.kind {
            MoveKind::Joint => joint_move(kin, &a.positions, &b.positions, b.speed),
            MoveKind::Linear => linear_move(kin, tool, &a.positions, &b.positions, b.speed),
        };
        // Through the waypoint between, if it blends: this move starts
        // before the last one ends.
        if let Some(last) = segments.last() {
            at -= overlap(kin, tool, last, &seg, a.blend);
        }
        seg.to = to;
        seg.start = at;
        at += seg.duration;
        segments.push(seg);
    }
    let duration = segments
        .iter()
        .map(|s| s.start + s.duration)
        .fold(0.0, f64::max);
    Plan {
        start,
        segments,
        duration,
    }
}

/// How long move `a` and the move after it, `b`, run at once through the
/// waypoint between them blended with radius `radius`: while the tool is
/// still within the radius of it on `a`'s way in, and not yet out of it on
/// `b`'s way out. Never more than half of either, so that no three moves
/// run at once.
fn overlap(kin: &Kinematics, tool: &Tool, a: &Segment, b: &Segment, radius: f64) -> f64 {
    if radius <= 0.0 || a.duration <= 0.0 || b.duration <= 0.0 {
        return 0.0;
    }
    let corner = kin.pose(b.origin(), tool).translation;
    let near = |q: &[f64]| kin.pose(q, tool).translation.distance(corner) <= radius;
    // In from the end of `a`, out from the start of `b`, by the sampled time.
    let within = |seg: &Segment, from_end: bool| {
        let n = 60;
        let dt = seg.duration / n as f64;
        let mut k = 0;
        while k < n {
            let t = if from_end {
                seg.duration - dt * (k + 1) as f64
            } else {
                dt * (k + 1) as f64
            };
            if !near(&seg.at(t)) {
                break;
            }
            k += 1;
        }
        dt * k as f64
    };
    within(a, true)
        .min(within(b, false))
        .min(a.duration / 2.0)
        .min(b.duration / 2.0)
}

fn joint_move(kin: &Kinematics, from: &[f64], to: &[f64], speed: f64) -> Segment {
    let speed = speed.clamp(0.01, 1.0);
    // Every joint's own quickest move; the slowest sets the pace, and the
    // rest follow its shape scaled down, so all arrive together.
    let profile = kin
        .vars
        .iter()
        .zip(from.iter().zip(to))
        .map(|(var, (a, b))| {
            Profile::new(
                b - a,
                speed * var.velocity,
                JOINT_ACCELERATION * var.velocity,
            )
        })
        .max_by(|x, y| x.duration.total_cmp(&y.duration))
        .unwrap_or(Profile::new(0.0, 1.0, 1.0));
    Segment {
        kind: MoveKind::Joint,
        to: 0,
        start: 0.0,
        duration: profile.duration,
        problems: Vec::new(),
        path: Path::Joint {
            from: from.to_vec(),
            to: to.to_vec(),
            profile,
        },
    }
}

fn linear_move(kin: &Kinematics, tool: &Tool, from: &[f64], to: &[f64], speed: f64) -> Segment {
    let (_, r0, p0) = kin.pose(from, tool).to_scale_rotation_translation();
    let (_, mut r1, p1) = kin.pose(to, tool).to_scale_rotation_translation();
    // The short way round.
    if r0.dot(r1) < 0.0 {
        r1 = -r1;
    }
    // Fewer than six joints cannot hold every orientation on the way.
    let orient = kin.vars.len() >= 6;
    let angle = if orient { turn(r0, r1) } else { 0.0 };
    let along = Profile::new(p1.distance(p0), speed.max(1e-3), LINEAR_ACCELERATION);
    let turning = Profile::new(angle, ANGULAR_SPEED, ANGULAR_ACCELERATION);
    let profile = if along.duration >= turning.duration {
        along
    } else {
        turning
    };

    let opts = Options {
        max_iterations: 60,
        ..Options::default()
    };
    let mut problems = Vec::new();
    let mut duration = profile.duration;
    let mut samples;
    // Once as asked, and once more slowed if a joint could not keep up.
    let mut pass = 0;
    loop {
        let n = ((duration / SAMPLE).ceil() as usize).max(1);
        samples = Vec::with_capacity(n + 1);
        samples.push(from.to_vec());
        let mut unreachable: Option<Problem> = None;
        let mut last_s = 0.0;
        for k in 1..=n {
            let s = profile.fraction(duration * k as f64 / n as f64, duration);
            let goal = Goal {
                position: p0.lerp(p1, s),
                rotation: orient.then(|| r0.slerp(r1, s)),
            };
            // From the last sample, stepped on as the joints would step
            // between the two ends. The solver's correction is the smallest
            // there is, which never moves the arm in a way that leaves the
            // tool where it is; so in those ways — the freedom an arm has
            // past what the line asks — it follows the step, and arrives in
            // the configuration taught rather than wherever it drifted.
            let seed: Vec<f64> = samples
                .last()
                .unwrap()
                .iter()
                .zip(from.iter().zip(to))
                .map(|(q, (a, b))| q + (b - a) * (s - last_s))
                .collect();
            last_s = s;
            let found = kin.solve(tool, &goal, &seed, &opts);
            if !found.reached {
                let (first, position, rotation) = match unreachable {
                    Some(Problem::Unreachable {
                        along,
                        position,
                        rotation,
                    }) => (along, position, rotation),
                    _ => (s, 0.0, 0.0),
                };
                unreachable = Some(Problem::Unreachable {
                    along: first,
                    position: position.max(found.position_error),
                    rotation: rotation.max(found.rotation_error),
                });
            }
            samples.push(found.positions);
        }
        if pass == 0 {
            problems.extend(unreachable);
        }
        let dt = duration / n as f64;
        let (joint, ratio) = fastest(kin, &samples, dt);
        if pass == 0 && ratio > 1.0 + 1e-6 {
            problems.push(Problem::Slowed {
                joint,
                factor: ratio,
            });
            duration *= ratio * 1.02;
            pass += 1;
            continue;
        }
        break;
    }

    // The line ends where the program says, whatever the solver found. A
    // small difference is numerical drift, spread over the move so that
    // nothing snaps; past JUMP the arm has come round to another
    // configuration, and the plan says so.
    let end = samples.last().unwrap().clone();
    let (worst, by) = end
        .iter()
        .zip(to)
        .map(|(a, b)| (a - b).abs())
        .enumerate()
        .max_by(|x, y| x.1.total_cmp(&y.1))
        .unwrap_or((0, 0.0));
    let slides = kin.vars.get(worst).is_some_and(|v| v.kind.slides());
    if by > if slides { JUMP_SLIDE } else { JUMP_TURN } {
        problems.push(Problem::Jump { joint: worst, by });
        *samples.last_mut().unwrap() = to.to_vec();
    } else {
        let last = samples.len() - 1;
        for (k, sample) in samples.iter_mut().enumerate().skip(1) {
            let share = k as f64 / last as f64;
            for (q, (e, t)) in sample.iter_mut().zip(end.iter().zip(to)) {
                *q += (t - e) * share;
            }
        }
    }

    Segment {
        kind: MoveKind::Linear,
        to: 0,
        start: 0.0,
        duration,
        problems,
        path: Path::Linear { samples },
    }
}

/// The joint that moves fastest for its top speed between samples `dt`
/// apart, and how many times its top speed that is.
fn fastest(kin: &Kinematics, samples: &[Vec<f64>], dt: f64) -> (usize, f64) {
    let mut worst = (0, 0.0);
    for pair in samples.windows(2) {
        for (i, var) in kin.vars.iter().enumerate() {
            let ratio = (pair[1][i] - pair[0][i]).abs() / dt / var.velocity;
            if ratio > worst.1 {
                worst = (i, ratio);
            }
        }
    }
    worst
}

/// The angle between two orientations.
fn turn(a: DQuat, b: DQuat) -> f64 {
    let mut d = (b * a.inverse()).normalize();
    if d.w < 0.0 {
        d = -d;
    }
    d.to_axis_angle().1
}

/// The tool point's path over a plan, sampled: for drawing it.
pub fn trace(kin: &Kinematics, tool: &Tool, plan: &Plan, step: f64) -> Vec<DVec3> {
    let n = ((plan.duration / step.max(1e-3)).ceil() as usize).clamp(1, 20_000);
    (0..=n)
        .map(|k| {
            let q = plan.at(plan.duration * k as f64 / n as f64);
            kin.pose(&q, tool).translation
        })
        .collect()
}

#[cfg(test)]
mod tests;
