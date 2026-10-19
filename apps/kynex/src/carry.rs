//! Robots carrying a process flow's parts, with no program written: the
//! same in the viewer and in a scenario's run.

use std::collections::BTreeMap;

use kreate_scene::Document;

use crate::cell::Cell;
use crate::pose::Posing;
use crate::run::Runner;

/// How long a part no robot could take waits before it is tried again,
/// seconds: working out a robot's reach is too slow for every step.
const RETRY: f64 = 2.0;

/// The robots carrying, and the part each has; and when each part no robot
/// could take was last tried.
#[derive(Debug, Default)]
pub struct Carry {
    carrying: BTreeMap<u64, u64>,
    tried: BTreeMap<u64, f64>,
}

impl Carry {
    /// Robots carrying a process flow's parts: each part waiting for one
    /// given to an idle robot with a grip point that reaches both where it
    /// is and where it goes, a program made for it there and then — above
    /// it, down, grip, up, over, down, let go, up — and run; the part
    /// handed over once it is put down. Returns whether anything changed.
    pub fn step(
        &mut self,
        scene: &Document,
        time: f64,
        cell: &mut Cell,
        posing: &Posing,
        runners: &mut Vec<Runner>,
        busy: &dyn Fn(u64) -> bool,
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
            runners.retain(|r| r.root != root);
            changed = true;
        }
        let jobs = cell.robot_jobs(scene);
        if jobs.is_empty() {
            return changed;
        }
        let grippers = cell.grippers();
        self.tried.retain(|p, _| jobs.iter().any(|j| j.0 == *p));
        for (part, at, to) in jobs {
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
            for (root, gripper, _) in robots {
                match self.transfer(scene, posing, root, gripper, at, to) {
                    Ok(program) => match Runner::new(root, program, scene, time) {
                        Ok(runner) => {
                            runners.retain(|r| r.root != root);
                            runners.push(runner);
                            self.carrying.insert(root, part);
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
                cell.flow_trouble(why);
            }
        }
        changed
    }

    /// A program for robot `root` to carry a part from `at` to `to` with
    /// grip point `gripper`, pointing down: or why it cannot reach.
    fn transfer(
        &self,
        scene: &Document,
        posing: &Posing,
        root: u64,
        gripper: u64,
        at: kreate_math::DVec3,
        to: kreate_math::DVec3,
    ) -> Result<crate::program::Program, String> {
        use crate::program::{Action, Point, Program, Step};
        use kreate_math::{DAffine3, DVec3};
        use kynex_kin::path::MoveKind;
        let kin = kynex_kin::Kinematics::of(scene, root).ok_or("no such robot")?;
        let tool = kin
            .tool(gripper, DAffine3::IDENTITY)
            .ok_or("no such grip point")?;
        let up = DVec3::Z * 0.15;
        let mut seed = posing.positions(&kin, root);
        // The grip point turned as it stands now — a robot waits with its
        // tool down — whatever way it is mounted on the tool.
        let (_, down, _) = kin.pose(&seed, &tool).to_scale_rotation_translation();
        let mut point = |name: &str, p: DVec3, linear: bool| -> Result<Point, String> {
            let found = kin.solve(
                &tool,
                &kynex_kin::Goal {
                    position: p,
                    rotation: Some(down),
                },
                &seed,
                &kynex_kin::Options {
                    restarts: 24,
                    ..kynex_kin::Options::default()
                },
            );
            if !found.reached {
                return Err(format!(
                    "{}: 로봇이 닿지 않는 곳입니다 ({:.2}, {:.2}, {:.2} m)",
                    crate::collide::label(scene, root),
                    p.x,
                    p.y,
                    p.z
                ));
            }
            seed = found.positions.clone();
            Ok(Point {
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
                    .zip(found.positions)
                    .collect(),
                pallet: None,
                frame: None,
                blend: 0.0,
                via: None,
                work: false,
            })
        };
        let mut program = Program::named("흐름 운반", Some(gripper), DVec3::ZERO);
        program.steps = vec![
            Step::Move(point("집기 위", at + up, false)?),
            Step::Move(point("집기", at, true)?),
            Step::Do(Action::Grip { gripper }),
            Step::Move(point("집은 뒤 위", at + up, true)?),
            Step::Move(point("놓기 위", to + up, false)?),
            Step::Move(point("놓기", to, true)?),
            Step::Do(Action::Release { gripper }),
            Step::Move(point("놓은 뒤 위", to + up, true)?),
        ];
        Ok(program)
    }
}
