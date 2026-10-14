//! Robot kinematics: where a robot's links stand for a set of joint
//! positions (forward), and the joint positions that bring its tool to a
//! pose (inverse).
//!
//! Read from a Kreate [`Document`] as it is. Each object of the robot keeps
//! the fixed part of its placement — what stands between its parent and its
//! own transform, parenting and all — and a joint adds its turn or slide on
//! top, through Kreate's own [`Joint::place`]. So a pose computed here and
//! the same pose set in Kreate put every link in the same place.
//!
//! The inverse is numerical — damped least squares on the Jacobian — so it
//! takes any tree Kreate can hold: a robot on a track, a prismatic axis, a
//! joint that mimics another. Limits hold throughout; a joint pressed
//! against one stops pulling the rest of the solution.

use std::collections::{BTreeMap, HashMap, HashSet};

use kreate_math::{DAffine3, DQuat, DVec3};
use kreate_scene::robot::{Joint, JointKind, RobotPart};
use kreate_scene::{Document, ObjectId, Uid};

/// One joint position the solver sets: a joint that moves and follows no
/// other.
#[derive(Clone, Debug, PartialEq)]
pub struct Var {
    pub uid: u64,
    pub name: String,
    pub kind: JointKind,
    /// Radians, or metres for a prismatic joint.
    pub lower: f64,
    pub upper: f64,
    /// The fastest it moves, radians or metres a second: the description's,
    /// or [`DEFAULT_VELOCITY`] where it gives none.
    pub velocity: f64,
}

/// A joint's top speed when its description gives none, radians or metres
/// a second.
pub const DEFAULT_VELOCITY: f64 = 1.0;

impl Var {
    /// Whether limits hold it.
    pub fn limited(&self) -> bool {
        self.kind.limited() && self.lower <= self.upper
    }

    /// `position` held to the limits, where there are any.
    pub fn clamp(&self, position: f64) -> f64 {
        if self.limited() {
            position.clamp(self.lower, self.upper)
        } else {
            position
        }
    }
}

/// What sets a moving joint's position.
#[derive(Clone, Copy, Debug)]
enum Drive {
    Var(usize),
    /// `multiplier · var + offset`.
    Mimic {
        var: usize,
        multiplier: f64,
        offset: f64,
    },
    /// Follows a joint that is not a variable here: kept where it is.
    Held(f64),
}

#[derive(Clone, Debug)]
enum Motion {
    Fixed,
    Joint { joint: Joint, drive: Drive },
}

#[derive(Clone, Debug)]
struct Node {
    uid: u64,
    parent: Option<usize>,
    /// From the parent's world (or the world, for the root) to where the
    /// object's own transform starts: parenting, a bone, and for a fixed
    /// object its transform too.
    pre: DAffine3,
    motion: Motion,
}

/// A point fixed to one of the robot's objects, usually its last link: what
/// the inverse brings to the goal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tool {
    node: usize,
    /// From the object to the tool point, in the object's space.
    pub offset: DAffine3,
}

/// Where the tool is wanted, in the world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Goal {
    pub position: DVec3,
    /// `None` to reach the point in any orientation.
    pub rotation: Option<DQuat>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    pub max_iterations: usize,
    /// Metres.
    pub position_tolerance: f64,
    /// Radians.
    pub rotation_tolerance: f64,
    /// Metres a radian of orientation error counts as: how orientation is
    /// weighed against position when both cannot be met.
    pub rotation_weight: f64,
    /// Metres. More holds the arm steadier near a singularity and slows
    /// convergence away from one.
    pub damping: f64,
    /// The most any joint moves in one step, radians or metres.
    pub max_step: f64,
    /// Fresh starts tried after the seed fails, from random positions within
    /// the limits. None while dragging, where a jump to another configuration
    /// is worse than missing the goal.
    pub restarts: usize,
    /// Makes the restarts the same run to run.
    pub random_seed: u64,
    /// The first this many variables are held where the seed has them: a
    /// track or a positioner the controller moves its own way while the
    /// arm follows a line (see [`crate::post::axes`]).
    pub held: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            max_iterations: 150,
            position_tolerance: 1e-5,
            rotation_tolerance: 1e-4,
            rotation_weight: 0.3,
            damping: 0.02,
            max_step: 0.3,
            restarts: 0,
            random_seed: 0x9e37_79b9_7f4a_7c15,
            held: 0,
        }
    }
}

/// How near an arm is to a singularity, where some way of moving the tool
/// takes no joint motion at all and, close by, a great deal (see
/// [`Kinematics::dexterity`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Dexterity {
    /// The least the tool moves for a unit of joint motion, the slowest
    /// way: metres a radian, a turn of the tool counted as
    /// [`TURN_LENGTH`] metres a radian. Zero at a singularity.
    pub least: f64,
    /// The joints that, in that way, move together and get the tool
    /// nowhere: those that would race near the singularity. By index into
    /// [`Kinematics::vars`], the most moved first.
    pub joints: Vec<usize>,
}

/// How a turn of the tool counts against its travel, metres a radian, in
/// [`Dexterity`]: a tenth of a metre, about the size of a wrist.
pub const TURN_LENGTH: f64 = 0.1;

/// What the inverse found: the best it reached, whether or not that is the
/// goal.
#[derive(Clone, Debug, PartialEq)]
pub struct Solution {
    /// One per [`Kinematics::vars`], within their limits.
    pub positions: Vec<f64>,
    /// Metres from the goal.
    pub position_error: f64,
    /// Radians from the goal's orientation; zero when it had none.
    pub rotation_error: f64,
    /// Within both tolerances.
    pub reached: bool,
    pub iterations: usize,
}

impl Solution {
    fn score(&self, opts: &Options) -> f64 {
        self.position_error + opts.rotation_weight * self.rotation_error
    }
}

/// A robot's kinematic tree, read from a document.
#[derive(Clone, Debug)]
pub struct Kinematics {
    pub root: u64,
    /// The positions the solver sets, in the order the tree reaches them.
    pub vars: Vec<Var>,
    /// Where they stood in the document read.
    pub current: Vec<f64>,
    /// Every object of the robot, parents before children.
    nodes: Vec<Node>,
    by_uid: HashMap<u64, usize>,
}

impl Kinematics {
    /// The robot whose root link is `root`, or `None` when there is no such
    /// object.
    pub fn of(doc: &Document, root: u64) -> Option<Self> {
        let mut id_of: HashMap<u64, ObjectId> = HashMap::new();
        // Each object's children in arena order, the order the document's
        // hierarchy lists siblings in, and kynex-model a robot's joints.
        let mut children: HashMap<ObjectId, Vec<ObjectId>> = HashMap::new();
        for (id, object) in doc.objects() {
            if let Some(uid) = doc.object_uid(id) {
                id_of.insert(uid.raw(), id);
            }
            if let Some(parent) = object.parent {
                children.entry(parent).or_default().push(id);
            }
        }
        let root_id = *id_of.get(&root)?;

        let mut nodes: Vec<Node> = Vec::new();
        let mut worlds: Vec<DAffine3> = Vec::new();
        let mut joints: Vec<(usize, Joint, f64, String)> = Vec::new();
        let mut stack: Vec<(ObjectId, Option<usize>)> = vec![(root_id, None)];
        while let Some((id, parent)) = stack.pop() {
            if nodes.len() > doc.object_count() {
                break;
            }
            let Some(object) = doc.object(id) else {
                continue;
            };
            let Some(uid) = doc.object_uid(id).map(|u| u.raw()) else {
                continue;
            };
            let world = doc.world_transform(id);
            let outer = parent.map_or(DAffine3::IDENTITY, |p| worlds[p].inverse());
            let at = nodes.len();
            let pre = match &object.robot {
                Some(RobotPart::Joint(joint)) if joint.kind.moves() => {
                    joints.push((
                        at,
                        joint.clone(),
                        joint.position_of(&object.transform),
                        object.name.clone(),
                    ));
                    // The joint's own transform comes back from its position.
                    outer * world * object.transform.to_affine().inverse()
                }
                _ => outer * world,
            };
            nodes.push(Node {
                uid,
                parent,
                pre,
                // A joint's is set below, once every joint's drive is known.
                motion: Motion::Fixed,
            });
            worlds.push(world);
            if let Some(list) = children.get(&id) {
                stack.extend(list.iter().rev().map(|c| (*c, Some(at))));
            }
        }

        let mut vars = Vec::new();
        let mut current = Vec::new();
        let mut var_of_name: HashMap<String, usize> = HashMap::new();
        for (at, joint, position, name) in &joints {
            if joint.mimic.is_none() {
                var_of_name.insert(name.clone(), vars.len());
                vars.push(Var {
                    uid: nodes[*at].uid,
                    name: name.clone(),
                    kind: joint.kind,
                    lower: joint.lower,
                    upper: joint.upper,
                    velocity: if joint.velocity > 0.0 {
                        joint.velocity
                    } else {
                        DEFAULT_VELOCITY
                    },
                });
                current.push(*position);
            }
        }
        let mut next = 0;
        for (at, joint, position, _) in joints {
            let drive = match &joint.mimic {
                None => {
                    next += 1;
                    Drive::Var(next - 1)
                }
                Some(m) => match var_of_name.get(&m.joint) {
                    Some(&var) => Drive::Mimic {
                        var,
                        multiplier: m.multiplier,
                        offset: m.offset,
                    },
                    None => Drive::Held(position),
                },
            };
            nodes[at].motion = Motion::Joint { joint, drive };
        }

        let by_uid = nodes.iter().enumerate().map(|(i, n)| (n.uid, i)).collect();
        Some(Self {
            root,
            vars,
            current,
            nodes,
            by_uid,
        })
    }

    /// The tool point `offset` from object `uid`, if it is part of this
    /// robot.
    pub fn tool(&self, uid: u64, offset: DAffine3) -> Option<Tool> {
        Some(Tool {
            node: *self.by_uid.get(&uid)?,
            offset,
        })
    }

    /// Every object of the robot, by identity, in the order
    /// [`Kinematics::forward`] gives their matrices.
    pub fn uids(&self) -> impl Iterator<Item = u64> + '_ {
        self.nodes.iter().map(|n| n.uid)
    }

    /// For each object, in [`Kinematics::uids`] order, the moving joint it
    /// rides on: the nearest at or above it, by identity. `None` for what
    /// is fixed to the root. Objects with the same carrier move as one.
    pub fn carriers(&self) -> Vec<Option<u64>> {
        let mut out: Vec<Option<u64>> = Vec::with_capacity(self.nodes.len());
        for node in &self.nodes {
            out.push(match node.motion {
                Motion::Joint { .. } => Some(node.uid),
                Motion::Fixed => node.parent.and_then(|p| out[p]),
            });
        }
        out
    }

    /// The moving joint that carries joint `joint` in turn: the next one in
    /// towards the root, `None` when it hangs from the root's rigid part.
    pub fn carrier_of_joint(&self, joint: u64) -> Option<u64> {
        let at = *self.by_uid.get(&joint)?;
        let parent = self.nodes[at].parent?;
        self.carriers()[parent]
    }

    /// Every joint [`Kinematics::apply`] sets, mimics too, by identity.
    pub fn joints(&self) -> impl Iterator<Item = u64> + '_ {
        self.nodes
            .iter()
            .filter(|n| matches!(n.motion, Motion::Joint { .. }))
            .map(|n| n.uid)
    }

    /// Whether `uid` is one of this robot's objects.
    pub fn contains(&self, uid: u64) -> bool {
        self.by_uid.contains_key(&uid)
    }

    /// [`Kinematics::current`] with some positions replaced, by joint
    /// identity, and held to the limits.
    pub fn with(&self, positions: &BTreeMap<u64, f64>) -> Vec<f64> {
        self.vars
            .iter()
            .zip(&self.current)
            .map(|(var, now)| var.clamp(positions.get(&var.uid).copied().unwrap_or(*now)))
            .collect()
    }

    fn value(drive: Drive, q: &[f64]) -> f64 {
        match drive {
            Drive::Var(i) => q[i],
            Drive::Mimic {
                var,
                multiplier,
                offset,
            } => multiplier * q[var] + offset,
            Drive::Held(v) => v,
        }
    }

    /// Every object's world matrix at positions `q`, in the tree's order.
    pub fn forward(&self, q: &[f64]) -> Vec<DAffine3> {
        let mut world: Vec<DAffine3> = Vec::with_capacity(self.nodes.len());
        for node in &self.nodes {
            let base = node.parent.map_or(node.pre, |p| world[p] * node.pre);
            world.push(match &node.motion {
                Motion::Fixed => base,
                Motion::Joint { joint, drive } => {
                    base * joint.place(Self::value(*drive, q)).to_affine()
                }
            });
        }
        world
    }

    /// Object `uid`'s world matrix at positions `q`.
    pub fn world_of(&self, q: &[f64], uid: u64) -> Option<DAffine3> {
        let at = *self.by_uid.get(&uid)?;
        Some(self.forward(q)[at])
    }

    /// The tool's world pose at positions `q`.
    pub fn pose(&self, q: &[f64], tool: &Tool) -> DAffine3 {
        self.forward(q)[tool.node] * tool.offset
    }

    /// How near the arm at `q` is to a singularity, for the tool `tool`:
    /// the smallest singular value of its Jacobian and the joints of that
    /// direction. The arm's alone, where it rides a track (see
    /// [`post::axes`]): a controller moves the track its own way. `None`
    /// for an arm of other than six joints.
    pub fn dexterity(&self, q: &[f64], tool: &Tool) -> Option<Dexterity> {
        let (arm, _) = post::axes(self).ok()?;
        let n = arm.len();
        let world = self.forward(q);
        let p = (world[tool.node] * tool.offset).translation;
        let all = self.jacobian(&world, tool, p);
        let mut cols: Vec<[f64; 6]> = arm.iter().map(|i| all[*i]).collect();
        for col in &mut cols {
            for x in &mut col[3..] {
                *x *= TURN_LENGTH;
            }
        }
        // Jᵀ J: its least eigenvalue is the least singular value squared,
        // and its eigenvector the joint motion that moves the tool least.
        let mut a = vec![vec![0.0; n]; n];
        for (i, row) in a.iter_mut().enumerate() {
            for (j, x) in row.iter_mut().enumerate() {
                *x = (0..6).map(|r| cols[i][r] * cols[j][r]).sum();
            }
        }
        let (values, vectors) = symmetric_eigen(a);
        let k = (0..n).min_by(|x, y| values[*x].total_cmp(&values[*y]))?;
        let v: Vec<f64> = (0..n).map(|i| vectors[i][k]).collect();
        let most = v.iter().fold(0.0f64, |m, x| m.max(x.abs()));
        let mut joints: Vec<usize> = (0..n).filter(|i| v[*i].abs() >= 0.4 * most).collect();
        joints.sort_by(|x, y| v[*y].abs().total_cmp(&v[*x].abs()));
        let joints = joints.into_iter().map(|j| arm[j]).collect();
        Some(Dexterity {
            least: values[k].max(0.0).sqrt(),
            joints,
        })
    }

    /// How the tool point `p` moves and turns as each variable does: per
    /// variable, linear velocity then angular, in the world.
    fn jacobian(&self, world: &[DAffine3], tool: &Tool, p: DVec3) -> Vec<[f64; 6]> {
        let mut cols = vec![[0.0; 6]; self.vars.len()];
        let mut at = Some(tool.node);
        while let Some(k) = at {
            let node = &self.nodes[k];
            at = node.parent;
            let Motion::Joint { joint, drive } = &node.motion else {
                continue;
            };
            let (var, c) = match *drive {
                Drive::Var(i) => (i, 1.0),
                Drive::Mimic {
                    var, multiplier, ..
                } => (var, multiplier),
                Drive::Held(_) => continue,
            };
            let base = node.parent.map_or(node.pre, |p| world[p] * node.pre);
            let dir = base.matrix3 * (joint.rest.rotation * DVec3::Z);
            let (v, w) = if joint.kind.slides() {
                (dir, DVec3::ZERO)
            } else {
                let axis = dir.normalize_or_zero();
                let origin = base.transform_point3(joint.rest.translation);
                (axis.cross(p - origin), axis)
            };
            let col = &mut cols[var];
            for (i, x) in [v.x, v.y, v.z, w.x, w.y, w.z].into_iter().enumerate() {
                col[i] += c * x;
            }
        }
        cols
    }

    /// Joint positions that bring `tool` to `goal`, starting from `seed`.
    pub fn solve(&self, tool: &Tool, goal: &Goal, seed: &[f64], opts: &Options) -> Solution {
        let mut best = self.descend(tool, goal, seed, opts);
        let mut rng = opts.random_seed | 1;
        for _ in 0..opts.restarts {
            if best.reached {
                break;
            }
            let start: Vec<f64> = self
                .vars
                .iter()
                .zip(seed)
                .enumerate()
                .map(|(i, (var, s))| {
                    let u = next_unit(&mut rng);
                    if i < opts.held {
                        *s
                    } else if var.limited() {
                        var.lower + u * (var.upper - var.lower)
                    } else if var.kind.slides() {
                        s + (u - 0.5)
                    } else {
                        (u * 2.0 - 1.0) * std::f64::consts::PI
                    }
                })
                .collect();
            let tried = self.descend(tool, goal, &start, opts);
            if tried.reached || tried.score(opts) < best.score(opts) {
                best = tried;
            }
        }
        best
    }

    fn descend(&self, tool: &Tool, goal: &Goal, seed: &[f64], opts: &Options) -> Solution {
        let n = self.vars.len();
        let mut q: Vec<f64> = self
            .vars
            .iter()
            .enumerate()
            .map(|(i, var)| var.clamp(seed.get(i).copied().unwrap_or(self.current[i])))
            .collect();
        let rows = if goal.rotation.is_some() { 6 } else { 3 };
        let w = opts.rotation_weight;
        let mut best: Option<Solution> = None;
        for iteration in 0..=opts.max_iterations {
            let world = self.forward(&q);
            let pose = world[tool.node] * tool.offset;
            let (dp, dr) = error(&pose, goal);
            let found = Solution {
                positions: q.clone(),
                position_error: dp.length(),
                rotation_error: dr.length(),
                reached: dp.length() <= opts.position_tolerance
                    && dr.length() <= opts.rotation_tolerance,
                iterations: iteration,
            };
            if best
                .as_ref()
                .is_none_or(|b| found.score(opts) < b.score(opts))
            {
                best = Some(found);
            }
            if best.as_ref().unwrap().reached || iteration == opts.max_iterations || n == 0 {
                break;
            }

            let e = [dp.x, dp.y, dp.z, w * dr.x, w * dr.y, w * dr.z];
            let mut cols = self.jacobian(&world, tool, pose.translation);
            for col in &mut cols {
                for x in &mut col[3..] {
                    *x *= w;
                }
            }
            // Held variables stay where they are.
            for col in cols.iter_mut().take(opts.held) {
                *col = [0.0; 6];
            }
            // A joint at a limit that the error would push further is left
            // out: kept in, it soaks up the step and the rest barely move.
            for (i, col) in cols.iter_mut().enumerate() {
                let var = &self.vars[i];
                if !var.limited() {
                    continue;
                }
                let pull: f64 = (0..rows).map(|r| col[r] * e[r]).sum();
                let at_upper = q[i] >= var.upper - 1e-12 && pull > 0.0;
                let at_lower = q[i] <= var.lower + 1e-12 && pull < 0.0;
                if at_upper || at_lower {
                    *col = [0.0; 6];
                }
            }
            // (J Jᵀ + λ² I) y = e, then Δq = Jᵀ y.
            let mut a = [[0.0; 6]; 6];
            for (r, row) in a.iter_mut().enumerate().take(rows) {
                for (c, x) in row.iter_mut().enumerate().take(rows) {
                    *x = cols.iter().map(|col| col[r] * col[c]).sum();
                }
                row[r] += opts.damping * opts.damping;
            }
            let Some(y) = solve_spd(a, e, rows) else {
                break;
            };
            let mut dq: Vec<f64> = cols
                .iter()
                .map(|col| (0..rows).map(|r| col[r] * y[r]).sum())
                .collect();
            let biggest = dq.iter().fold(0.0f64, |m, x| m.max(x.abs()));
            if biggest > opts.max_step {
                let k = opts.max_step / biggest;
                dq.iter_mut().for_each(|x| *x *= k);
            }
            for (i, var) in self.vars.iter().enumerate() {
                q[i] = var.clamp(q[i] + dq[i]);
            }
        }
        best.expect("at least one iteration ran")
    }

    /// Sets every joint object of the robot in `doc` to positions `q`, and
    /// returns those whose transform changed.
    pub fn apply(&self, doc: &mut Document, q: &[f64]) -> Vec<Uid> {
        let wanted: HashSet<u64> = self.joints().collect();
        let ids: HashMap<u64, ObjectId> = doc
            .objects()
            .filter_map(|(id, _)| {
                let uid = doc.object_uid(id)?.raw();
                wanted.contains(&uid).then_some((uid, id))
            })
            .collect();
        let mut changed = Vec::new();
        for node in &self.nodes {
            let Motion::Joint { joint, drive } = &node.motion else {
                continue;
            };
            let Some(object) = ids.get(&node.uid).and_then(|id| doc.object_mut(*id)) else {
                continue;
            };
            let placed = joint.place(Self::value(*drive, q));
            if object.transform != placed {
                object.transform = placed;
                changed.push(Uid::from_raw(node.uid));
            }
        }
        changed
    }
}

/// From the tool's pose to the goal: the move, and the turn as a rotation
/// vector, both in the world.
fn error(pose: &DAffine3, goal: &Goal) -> (DVec3, DVec3) {
    let (_, rotation, translation) = pose.to_scale_rotation_translation();
    let dp = goal.position - translation;
    let dr = match goal.rotation {
        None => DVec3::ZERO,
        Some(want) => {
            let mut turn = (want * rotation.inverse()).normalize();
            // The short way round.
            if turn.w < 0.0 {
                turn = -turn;
            }
            let (axis, angle) = turn.to_axis_angle();
            axis * angle
        }
    };
    (dp, dr)
}

/// `a y = b` for a symmetric positive definite `a` of size `n`, by
/// Cholesky.
/// The eigenvalues of symmetric `a`, and its eigenvectors as columns, by
/// Jacobi's rotations.
fn symmetric_eigen(mut a: Vec<Vec<f64>>) -> (Vec<f64>, Vec<Vec<f64>>) {
    let n = a.len();
    let mut v: Vec<Vec<f64>> = (0..n)
        .map(|i| (0..n).map(|j| if i == j { 1.0 } else { 0.0 }).collect())
        .collect();
    for _ in 0..100 {
        let off: f64 = (0..n)
            .flat_map(|i| (0..n).filter(move |j| *j != i).map(move |j| (i, j)))
            .map(|(i, j)| a[i][j] * a[i][j])
            .sum();
        if off < 1e-22 {
            break;
        }
        for p in 0..n {
            for q in p + 1..n {
                if a[p][q].abs() < 1e-300 {
                    continue;
                }
                let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
                let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
                let t = if theta == 0.0 { 1.0 } else { t };
                let c = 1.0 / (t * t + 1.0).sqrt();
                let s = t * c;
                for row in a.iter_mut() {
                    let (akp, akq) = (row[p], row[q]);
                    row[p] = c * akp - s * akq;
                    row[q] = s * akp + c * akq;
                }
                let (rp, rq) = (a[p].clone(), a[q].clone());
                for ((x, apk), aqk) in a[p].iter_mut().zip(&rp).zip(&rq) {
                    *x = c * apk - s * aqk;
                }
                for ((x, apk), aqk) in a[q].iter_mut().zip(&rp).zip(&rq) {
                    *x = s * apk + c * aqk;
                }
                for row in v.iter_mut() {
                    let (vp, vq) = (row[p], row[q]);
                    row[p] = c * vp - s * vq;
                    row[q] = s * vp + c * vq;
                }
            }
        }
    }
    ((0..n).map(|i| a[i][i]).collect(), v)
}

fn solve_spd(mut a: [[f64; 6]; 6], b: [f64; 6], n: usize) -> Option<[f64; 6]> {
    for j in 0..n {
        let d = a[j][j] - a[j][..j].iter().map(|x| x * x).sum::<f64>();
        if d <= 0.0 || !d.is_finite() {
            return None;
        }
        let d = d.sqrt();
        a[j][j] = d;
        let row = a[j];
        for r in a.iter_mut().take(n).skip(j + 1) {
            let s = r[j]
                - r[..j]
                    .iter()
                    .zip(&row[..j])
                    .map(|(x, y)| x * y)
                    .sum::<f64>();
            r[j] = s / d;
        }
    }
    let mut y = [0.0; 6];
    for i in 0..n {
        let mut s = b[i];
        for k in 0..i {
            s -= a[i][k] * y[k];
        }
        y[i] = s / a[i][i];
    }
    for i in (0..n).rev() {
        let mut s = y[i];
        for k in i + 1..n {
            s -= a[k][i] * y[k];
        }
        y[i] = s / a[i][i];
    }
    Some(y)
}

/// In [0, 1), from xorshift64.
fn next_unit(state: &mut u64) -> f64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    (*state >> 11) as f64 / (1u64 << 53) as f64
}

pub mod drl;
pub mod path;
pub mod post;
pub mod rapid;
pub mod reach;
pub mod urscript;

#[cfg(test)]
mod tests;
