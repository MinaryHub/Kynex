//! Robots carrying a process flow's parts, with no program written: the
//! same in the viewer and in a scenario's run.

use std::collections::BTreeMap;

use kreate_scene::Document;

use crate::cell::Cell;
use crate::pose::Posing;
use crate::run::Runner;

/// How many of a carrying program's moves would hit something, for robot
/// `root` in the scene given, but for the doors and the work places given
/// (where a gripper takes or puts a part): none for a scenario's run, the
/// viewer's collider in the viewer.
pub type Check<'a> = dyn FnMut(
        &Document,
        u64,
        &crate::program::Program,
        &std::collections::HashSet<u64>,
        &[kreate_math::DVec3],
    ) -> usize
    + 'a;

/// How long a part no robot could take waits before it is tried again,
/// seconds: working out a robot's reach is too slow for every step.
const RETRY: f64 = 2.0;

/// The robots carrying, and the part each has; and when each part no robot
/// could take was last tried.
#[derive(Debug, Default)]
pub struct Carry {
    carrying: BTreeMap<u64, u64>,
    tried: BTreeMap<u64, f64>,
    /// The way chosen for each robot between two places, by robot, the
    /// places to the centimetre, the scene's version and the pose it sets
    /// out from: worked out once —
    /// trying ways against the scene is slow —, then taken again.
    routes: BTreeMap<(u64, [i64; 6], u64, Vec<i64>), crate::program::Program>,
    /// The places each robot carrying goes between, by robot: their doors
    /// kept open until it is done and out, not only until the part is down.
    serving: BTreeMap<u64, [u64; 2]>,
    /// The robots that could not take each part where it goes, by part:
    /// once all have said so, it goes along its link without a robot, in
    /// the time a robot would take (see `Cell::robot_skip`).
    failed: BTreeMap<u64, std::collections::BTreeSet<u64>>,
}

impl Carry {
    /// Robots carrying a process flow's parts: each part waiting for one
    /// given to an idle robot with a grip point that reaches both where it
    /// is and where it goes, a program made for it there and then — above
    /// it, down, grip, up, over, down, let go, up — and run; the part
    /// handed over once it is put down. Returns whether anything changed.
    #[allow(clippy::too_many_arguments)]
    pub fn step(
        &mut self,
        scene: &Document,
        time: f64,
        cell: &mut Cell,
        posing: &Posing,
        runners: &mut Vec<Runner>,
        busy: &dyn Fn(u64) -> bool,
        check: &mut Check<'_>,
    ) -> bool {
        let mut changed = false;
        // Done, or stopped short.
        let done: Vec<(u64, u64, Option<String>)> = self
            .carrying
            .iter()
            .filter_map(|(root, part)| {
                let r = runners.iter().find(|r| r.root == *root)?;
                r.stopped().then(|| {
                    let why = (!r.finished()).then(|| {
                        format!(
                            "로봇이 부품을 나르다 멈췄습니다: {}",
                            r.to_json(time)["detail"].as_str().unwrap_or_default()
                        )
                    });
                    (*root, *part, why)
                })
            })
            .collect();
        for (root, part, why) in done {
            cell.robot_done(part, why);
            self.carrying.remove(&root);
            self.serving.remove(&root);
            runners.retain(|r| r.root != root);
            changed = true;
        }
        // Its program stopped from outside — the run stopped, the page
        // opened anew —: the robot free again, and its part not left with
        // it for ever (see `Cell::robot_dropped`).
        let gone: Vec<(u64, u64)> = self
            .carrying
            .iter()
            .filter(|(root, _)| !runners.iter().any(|r| r.root == **root))
            .map(|(root, part)| (*root, *part))
            .collect();
        for (root, part) in gone {
            cell.robot_dropped(part);
            self.carrying.remove(&root);
            self.serving.remove(&root);
            changed = true;
        }
        for ends in self.serving.values() {
            for object in ends {
                cell.hold_door(*object);
            }
        }
        let jobs = cell.robot_jobs(scene);
        if jobs.is_empty() {
            return changed;
        }
        let grippers = cell.grippers();
        self.tried.retain(|p, _| jobs.iter().any(|j| j.0 == *p));
        self.failed.retain(|p, _| jobs.iter().any(|j| j.0 == *p));
        for (part, at, to, ends, want) in jobs {
            if self
                .tried
                .get(&part)
                .is_some_and(|t| time - t < RETRY && time >= *t)
            {
                continue;
            }
            // Idle robots with a grip point, nearest first.
            let mut robots: Vec<(u64, u64, f64)> = kynex_model::robots(scene)
                .into_iter()
                .filter(|r| want.is_none_or(|w| w == r.root))
                .filter(|r| !self.carrying.contains_key(&r.root))
                .filter(|r| !busy(r.root))
                .filter(|r| !runners.iter().any(|x| x.root == r.root && !x.stopped()))
                .filter_map(|r| {
                    let kin = kynex_kin::Kinematics::of(scene, r.root)?;
                    let g = grippers.iter().copied().find(|g| kin.contains(*g))?;
                    let base = kin.world_of(&posing.positions(&kin, r.root), r.root)?;
                    Some((
                        r.root,
                        g,
                        base.translation.truncate().distance(at.truncate()),
                    ))
                })
                .collect();
            robots.sort_by(|a, b| a.2.total_cmp(&b.2));
            if robots.is_empty() {
                if self.carrying.is_empty() {
                    cell.flow_trouble(
                        "로봇으로 보내게 했지만 그리퍼가 있는 빈 로봇이 없습니다".into(),
                    );
                }
                continue;
            }
            let mut trouble = None;
            // Doors of the machines served open as a robot comes: no
            // obstacle to its moves.
            // The scene with the doors of machines served open, as they
            // are while a robot reaches in: moves are looked along in it.
            let opened = scene;
            let mut tried: Vec<u64> = Vec::new();
            for (root, gripper, _) in robots {
                tried.push(root);
                match self.transfer(scene, opened, posing, root, gripper, at, to, check) {
                    Ok(program) => match Runner::new(root, program, scene, time) {
                        Ok(runner) => {
                            runners.retain(|r| r.root != root);
                            runners.push(runner);
                            self.carrying.insert(root, part);
                            self.serving.insert(root, ends);
                            cell.robot_take(part, root);
                            trouble = None;
                            changed = true;
                            break;
                        }
                        Err(e) => trouble = Some(e),
                    },
                    Err(e) => trouble = Some(e),
                }
            }
            if let Some(why) = trouble {
                self.tried.insert(part, time);
                // Out of reach of the robots free now: of those busy too?
                // Then no robot can take it there: it still goes along the
                // link the user drew — the flow is theirs — in the time a
                // robot would take, the link marked out of reach.
                // Each robot is asked while it is free, as it stands
                // waiting: once every one with a grip point has said no.
                let failed = self.failed.entry(part).or_default();
                failed.extend(tried.iter().copied());
                let all = kynex_model::robots(scene).into_iter().all(|r| {
                    want.is_some_and(|w| w != r.root)
                        || failed.contains(&r.root)
                        || kynex_kin::Kinematics::of(scene, r.root)
                            .is_none_or(|kin| !grippers.iter().any(|g| kin.contains(*g)))
                });
                if all {
                    self.failed.remove(&part);
                    cell.robot_skip(part, why);
                    changed = true;
                } else if self.carrying.is_empty() {
                    cell.flow_trouble(why);
                }
            }
        }
        changed
    }

    /// Whether robot `root` (none: any robot with a grip point) can carry
    /// a part from `at` to `to`: or why not. Worked out as a carry would
    /// be, against nothing in the way — its reach, not its path.
    pub fn reaches(
        &mut self,
        scene: &Document,
        posing: &Posing,
        grippers: &[u64],
        root: Option<u64>,
        at: kreate_math::DVec3,
        to: kreate_math::DVec3,
    ) -> Result<(), String> {
        let robots: Vec<(u64, u64)> = kynex_model::robots(scene)
            .into_iter()
            .filter(|r| root.is_none_or(|w| w == r.root))
            .filter_map(|r| {
                let kin = kynex_kin::Kinematics::of(scene, r.root)?;
                let g = grippers.iter().copied().find(|g| kin.contains(*g))?;
                Some((r.root, g))
            })
            .collect();
        if robots.is_empty() {
            return Err(match root {
                Some(_) => "이 로봇에 그리퍼(잡는 점)가 없습니다".into(),
                None => "그리퍼가 있는 로봇이 없습니다".into(),
            });
        }
        let mut why = String::new();
        for (r, g) in robots {
            match self.transfer(scene, scene, posing, r, g, at, to, &mut |_, _, _, _, _| 0) {
                Ok(_) => return Ok(()),
                Err(e) => why = e,
            }
        }
        Err(why)
    }

    /// A program for robot `root` to carry a part from `at` to `to` with
    /// grip point `gripper`, pointing down: or why it cannot reach.
    #[allow(clippy::too_many_arguments)]
    fn transfer(
        &mut self,
        scene: &Document,
        opened: &Document,
        posing: &Posing,
        root: u64,
        gripper: u64,
        at: kreate_math::DVec3,
        to: kreate_math::DVec3,
        check: &mut Check<'_>,
    ) -> Result<crate::program::Program, String> {
        // The same way as last time, between the same two places.
        let cm = |v: kreate_math::DVec3| {
            [
                (v.x * 100.0).round() as i64,
                (v.y * 100.0).round() as i64,
                (v.z * 100.0).round() as i64,
            ]
        };
        let [a, b, c] = cm(at);
        let [d, e, f] = cm(to);
        // And from where it stands now, to the degree.
        let standing: Vec<i64> = kynex_kin::Kinematics::of(scene, root)
            .map(|kin| {
                posing
                    .positions(&kin, root)
                    .iter()
                    .map(|q| q.to_degrees().round() as i64)
                    .collect()
            })
            .unwrap_or_default();
        let key = (root, [a, b, c, d, e, f], scene.version(), standing);
        if let Some(program) = self.routes.get(&key) {
            return Ok(program.clone());
        }
        let found = self.find_route(scene, opened, posing, root, gripper, at, to, check);
        if let Ok(program) = &found {
            if self.routes.len() > 256 {
                self.routes.clear();
            }
            self.routes.insert(key, program.clone());
        }
        found
    }

    /// The way for robot `root` to carry a part from `at` to `to`: see
    /// [`Carry::transfer`].
    #[allow(clippy::too_many_arguments)]
    fn find_route(
        &self,
        scene: &Document,
        opened: &Document,
        posing: &Posing,
        root: u64,
        gripper: u64,
        at: kreate_math::DVec3,
        to: kreate_math::DVec3,
        check: &mut Check<'_>,
    ) -> Result<crate::program::Program, String> {
        use crate::program::{Action, Point, Program, Step};
        use kreate_math::{DAffine3, DQuat, DVec3};
        use kynex_kin::path::MoveKind;
        let kin = kynex_kin::Kinematics::of(scene, root).ok_or("no such robot")?;
        let tool = kin
            .tool(gripper, DAffine3::IDENTITY)
            .ok_or("no such grip point")?;
        let stand = posing.positions(&kin, root);
        // The grip point turned as it stands now — a robot waits with its
        // tool down — whatever way it is mounted on the tool.
        let (_, down, _) = kin.pose(&stand, &tool).to_scale_rotation_translation();
        let base = kin
            .world_of(&stand, root)
            .map_or(DVec3::ZERO, |b| b.translation);
        let solve = |p: DVec3, rotation: DQuat, from: &[f64]| -> Option<Vec<f64>> {
            let found = kin.solve(
                &tool,
                &kynex_kin::Goal {
                    position: p,
                    rotation: Some(rotation),
                },
                from,
                &kynex_kin::Options {
                    restarts: 24,
                    ..kynex_kin::Options::default()
                },
            );
            found.reached.then_some(found.positions)
        };
        // The ways to each end: from above, the tool down; and from in
        // front, level, the tool pointing at it from the robot's side —
        // a chuck deep in a machine, through its door.
        let ways = |p: DVec3| -> Vec<(DVec3, DQuat)> {
            let mut ways = vec![(DVec3::Z * 0.15, down)];
            let level = (p - base).truncate();
            if level.length() > 1e-6 {
                let toward = level.normalize().extend(0.0);
                let turn = DQuat::from_rotation_arc(-DVec3::Z, toward);
                // Lined up from outside: far enough back to be clear of a
                // machine's front, then straight in.
                ways.push((-toward * 0.5, turn * down));
                ways.push((-toward * 0.15, turn * down));
            }
            ways
        };
        // Poses to start the search from: as it stands, and with its elbow
        // the other way (a six-axis arm's second and third joints), so the
        // arm may come over rather than under what is in the way.
        let mut seeds = vec![stand.clone()];
        if stand.len() >= 4 {
            let mut flipped = stand.clone();
            flipped[1] += stand[2];
            flipped[2] = -stand[2];
            flipped[3] += stand[2];
            seeds.push(flipped);
        }
        // And, for a six-axis arm, the base turned toward the part each
        // way, the shoulder and the elbow at a few bends, the tool kept
        // down: the arm over the top of a belt or a machine's front rather
        // than through it.
        if stand.len() == 6 {
            let turn = kin.world_of(&stand, root).map_or(DQuat::IDENTITY, |b| {
                DQuat::from_mat3(&b.matrix3).normalize()
            });
            let local = turn.inverse() * (at - base);
            let pan = local.y.atan2(local.x);
            for pan in [pan, pan + std::f64::consts::PI] {
                for (lift, bend) in [(-1.2, 1.2), (-1.9, 1.9), (-0.7, 0.9), (-2.4, 2.2)] {
                    let wrist = -(lift + bend) - std::f64::consts::FRAC_PI_2;
                    seeds.push(vec![
                        pan,
                        lift,
                        bend,
                        wrist,
                        -std::f64::consts::FRAC_PI_2,
                        stand[5],
                    ]);
                }
            }
        }
        // An end reached a way, near and at, from a seed: or not.
        let end = |p: DVec3, way: (DVec3, DQuat), from: &[f64]| -> Option<(Vec<f64>, Vec<f64>)> {
            let near = solve(p + way.0, way.1, from)?;
            let at = solve(p, way.1, &near)?;
            Some((near, at))
        };
        let point = |name: &str, q: &[f64], linear: bool| Point {
            name: name.to_string(),
            kind: if linear {
                MoveKind::Linear
            } else {
                MoveKind::Joint
            },
            speed: if linear {
                crate::program::LINEAR_SPEED
            } else {
                crate::program::JOINT_SPEED
            },
            positions: kin
                .vars
                .iter()
                .map(|v| v.uid)
                .zip(q.iter().copied())
                .collect(),
            pallet: None,
            frame: None,
            blend: 0.0,
            via: None,
            work: false,
        };
        // Where it waits, and goes by between the two ends: as it stands,
        // if that touches nothing — the doors of machines shut, and open,
        // as they may be while it waits —; else, for a six-axis arm, drawn
        // in over its base, or upright.
        let rest = {
            let mut clear = |q: &[f64]| {
                let mut p = Program::named("대기", Some(gripper), DVec3::ZERO);
                // A move however small: two same poses make no move to look
                // along.
                let mut nudged = q.to_vec();
                if let Some(last) = nudged.last_mut() {
                    *last += 0.01;
                }
                p.steps = vec![
                    Step::Move(point("대기", q, false)),
                    Step::Move(point("대기", &nudged, false)),
                ];
                check(scene, root, &p, &std::collections::HashSet::new(), &[]) == 0
                    && check(opened, root, &p, &std::collections::HashSet::new(), &[]) == 0
            };
            let mut ways = vec![stand.clone()];
            if stand.len() == 6 {
                let h = std::f64::consts::FRAC_PI_2;
                ways.push(vec![stand[0], -h * 1.35, h * 1.25, -h * 1.4, -h, stand[5]]);
                ways.push(vec![stand[0], -h, 0.0, -h, -h, stand[5]]);
            }
            ways.iter()
                .find(|q| clear(q))
                .cloned()
                .unwrap_or(stand.clone())
        };
        let program = |pick: &(Vec<f64>, Vec<f64>), put: &(Vec<f64>, Vec<f64>), via: bool| {
            let mut p = Program::named("흐름 운반", Some(gripper), DVec3::ZERO);
            let mut steps = vec![
                // From where it stands: the first move is looked along too.
                Step::Move(point("출발", &stand, false)),
                Step::Move(point("집기 앞", &pick.0, false)),
                Step::Move(point("집기", &pick.1, true)),
                Step::Do(Action::Grip { gripper }),
                Step::Move(point("집은 뒤 물러남", &pick.0, true)),
            ];
            // By way of where it waits: around what is between, not
            // through it.
            if via {
                steps.push(Step::Move(point("비켜 감", &rest, false)));
            }
            steps.extend([
                Step::Move(point("놓기 앞", &put.0, false)),
                Step::Move(point("놓기", &put.1, true)),
                Step::Do(Action::Release { gripper }),
                Step::Move(point("놓은 뒤 물러남", &put.0, true)),
            ]);
            // Back where it waits, always: not left in a machine whose door
            // then shuts on it.
            steps.push(Step::Move(point("대기 자세로", &rest, false)));
            p.steps = steps;
            p
        };
        // Every way to carry it, each checked for what it would hit on the
        // way: the first that hits nothing, else the one that hits least.
        let mut best: Option<(usize, Program)> = None;
        let mut looked = 0;
        'search: for seed in &seeds {
            for &pw in &ways(at) {
                let Some(pick) = end(at, pw, seed) else {
                    continue;
                };
                for &tw in &ways(to) {
                    let Some(put) = end(to, tw, &pick.1) else {
                        continue;
                    };
                    for via in [false, true] {
                        // Each way is slow to look along: a few dozen at most.
                        if looked >= 24 {
                            break 'search;
                        }
                        looked += 1;
                        let candidate = program(&pick, &put, via);
                        let hits = check(
                            opened,
                            root,
                            &candidate,
                            &std::collections::HashSet::new(),
                            &[at, to],
                        );
                        if best.as_ref().is_none_or(|(h, _)| hits < *h) {
                            best = Some((hits, candidate));
                        }
                        if hits == 0 {
                            break 'search;
                        }
                    }
                }
            }
        }
        if let Some((_, program)) = best {
            return Ok(program);
        }
        // Out of reach: of the end it cannot get to.
        let far = if ways(at).into_iter().any(|w| end(at, w, &rest).is_some()) {
            to
        } else {
            at
        };
        Err(format!(
            "{}: 로봇이 닿지 않는 곳입니다 ({:.2}, {:.2}, {:.2} m)",
            crate::collide::label(scene, root),
            far.x,
            far.y,
            far.z
        ))
    }
}
