//! Running a program: the robot goes through its steps by the simulation's
//! clock — moving, gripping, telling machines what to do, waiting for them
//! — while the machines run by the same clock (see `crate::cell`).
//!
//! A move is planned when it starts, from wherever the robot is then, so a
//! program can be run from any pose. A step the cell refuses — a cycle
//! started on an open door, a grip with nothing there — stops the run with
//! an alarm saying why, as a robot controller would.
//!
//! The clock can run faster than real time ([`Clock::speed`]): a 90 s
//! machining cycle need not be watched for 90 s.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use kreate_math::DVec3;
use kreate_scene::Document;
use kynex_kin::path::{self, MoveKind, Plan, Waypoint};
use kynex_kin::{Goal, Kinematics, Options, Tool};
use serde_json::{json, Value};

use crate::cell::{Cell, Signal};
use crate::pose::Posing;
use crate::program::{Action, Point, Program, Step};

/// How often a moving robot is shown at a new pose, in real time.
const FRAME: Duration = Duration::from_millis(33);

/// The simulation's clock: seconds, running at `speed` times real time.
#[derive(Debug)]
pub struct Clock {
    pub speed: f64,
    pub time: f64,
    last: Option<Instant>,
}

impl Default for Clock {
    fn default() -> Self {
        Self {
            speed: 1.0,
            time: 0.0,
            last: None,
        }
    }
}

impl Clock {
    /// Moves the clock on to real time `now`, and returns where it is.
    pub fn advance(&mut self, now: Instant) -> f64 {
        if let Some(last) = self.last {
            self.time += now.duration_since(last).as_secs_f64() * self.speed;
        }
        self.last = Some(now);
        self.time
    }

    pub fn to_json(&self) -> Value {
        json!({ "speed": self.speed, "time": self.time })
    }
}

/// Where a run is in its step.
#[derive(Debug)]
enum State {
    /// About to start the step.
    Begin,
    /// Through the moves `chain` (by step), one plan: blended where their
    /// points say.
    Moving {
        plan: Plan,
        from: f64,
        chain: Vec<usize>,
    },
    /// For the machine to say what the step waits for.
    Waiting,
    /// Until the clock reaches this: a gripper closing, a pause.
    Until(f64),
    Done,
    Alarm(String),
}

/// A program running on a robot.
#[derive(Debug)]
pub struct Runner {
    pub root: u64,
    program: Program,
    kin: Kinematics,
    tool: Tool,
    step: usize,
    state: State,
    /// When the current cycle began, how many have finished, and how long
    /// the last one took.
    cycle_from: f64,
    cycles: u32,
    last_cycle: Option<f64>,
    last_frame: Option<Instant>,
    /// Each pallet's count: the place its points go to, from 0.
    places: BTreeMap<u64, usize>,
}

impl Runner {
    /// Robot `root`'s `program`, about to run in `scene` from `time`.
    pub fn new(root: u64, program: Program, scene: &Document, time: f64) -> Result<Self, String> {
        if program.steps.is_empty() {
            return Err("프로그램에 단계가 없습니다".into());
        }
        let kin = Kinematics::of(scene, root).ok_or("no such robot")?;
        let tool = program.tool(&kin).ok_or("툴이 정해지지 않았습니다")?;
        Ok(Self {
            root,
            program,
            kin,
            tool,
            step: 0,
            state: State::Begin,
            cycle_from: time,
            cycles: 0,
            last_cycle: None,
            last_frame: None,
            places: BTreeMap::new(),
        })
    }

    /// Whether its program runs round and round.
    pub fn looping(&self) -> bool {
        self.program.close
    }

    /// The pallets its program goes through place by place.
    pub fn counted_pallets(&self) -> Vec<u64> {
        self.program.counted_pallets()
    }

    /// Whether it has stopped: at its end, or on an alarm.
    pub fn stopped(&self) -> bool {
        matches!(self.state, State::Done | State::Alarm(_))
    }

    /// Moves the run on to `time` on the simulation's clock: as many steps
    /// as end by then. `scene` is the scene the cell is in; the robot's
    /// pose is held in `posing`. Returns whether the robot moved or the run
    /// changed, for the viewers.
    pub fn tick(
        &mut self,
        time: f64,
        now: Instant,
        posing: &mut Posing,
        cell: &mut Cell,
        scene: &Document,
    ) -> bool {
        let mut changed = false;
        // Steps that take no time follow one another within a tick; the
        // bound keeps a program of nothing but those from spinning.
        for _ in 0..256 {
            match &mut self.state {
                State::Done | State::Alarm(_) => break,
                State::Begin => {
                    changed = true;
                    if self.step >= self.program.steps.len() {
                        if self.program.close {
                            self.cycles += 1;
                            self.last_cycle = Some(time - self.cycle_from);
                            self.cycle_from = time;
                            self.step = 0;
                            continue;
                        }
                        self.state = State::Done;
                        break;
                    }
                    self.state = match self.begin(time, posing, cell, scene) {
                        Ok(state) => state,
                        Err(why) => State::Alarm(why),
                    };
                }
                State::Moving { plan, from, chain } => {
                    let t = time - *from;
                    let ended = t >= plan.duration;
                    // The step shown running: the move under way.
                    self.step = chain[plan.segment_at(t).min(chain.len() - 1)];
                    let due = self
                        .last_frame
                        .is_none_or(|f| now.duration_since(f) >= FRAME);
                    if ended || due {
                        posing.hold_at(&self.kin, self.root, &plan.at(t));
                        self.last_frame = Some(now);
                        changed = true;
                    }
                    if !ended {
                        break;
                    }
                    self.step = chain[chain.len() - 1];
                    self.next();
                }
                State::Waiting => {
                    let Step::Do(Action::Wait { machine, signal }) = self.program.steps[self.step]
                    else {
                        unreachable!("waiting on a wait step");
                    };
                    if !cell.signal(scene, machine, signal) {
                        break;
                    }
                    self.next();
                }
                State::Until(end) => {
                    if time < *end {
                        break;
                    }
                    self.next();
                }
            }
        }
        changed
    }

    fn next(&mut self) {
        self.step += 1;
        self.state = State::Begin;
    }

    /// Starts step `self.step`.
    fn begin(
        &mut self,
        time: f64,
        posing: &mut Posing,
        cell: &mut Cell,
        scene: &Document,
    ) -> Result<State, String> {
        let step = self.program.steps[self.step].clone();
        Ok(match step {
            Step::Move(_) => {
                // This move and those straight after it, up to the next
                // action: planned together, to blend from one to the next.
                let mut chain = vec![self.step];
                while let Some(Step::Move(_)) = self.program.steps.get(chain[chain.len() - 1] + 1) {
                    chain.push(chain[chain.len() - 1] + 1);
                }
                let mut waypoints = vec![Waypoint {
                    kind: MoveKind::Joint,
                    positions: posing.positions(&self.kin, self.root),
                    speed: 1.0,
                    blend: 0.0,
                }];
                for (k, i) in chain.iter().enumerate() {
                    let Step::Move(point) = &self.program.steps[*i] else {
                        unreachable!("a chain of moves");
                    };
                    waypoints.push(Waypoint {
                        kind: point.kind,
                        positions: self.target(point, cell, scene)?,
                        speed: point.speed,
                        // The last stops: an action comes next.
                        blend: if k + 1 < chain.len() {
                            point.blend
                        } else {
                            0.0
                        },
                    });
                }
                let plan = path::plan(&self.kin, &self.tool, &waypoints, false);
                State::Moving {
                    plan,
                    from: time,
                    chain,
                }
            }
            Step::Do(Action::Grip { gripper }) => {
                // Where the gripper is: the robot as it stands now.
                let posed = posing.shown(scene);
                cell.grip(posed.as_ref().unwrap_or(scene), gripper)?;
                State::Until(time + cell.actuate_time(gripper))
            }
            Step::Do(Action::Release { gripper }) => {
                let posed = posing.shown(scene);
                cell.release(posed.as_ref().unwrap_or(scene), gripper)?;
                State::Until(time + cell.actuate_time(gripper))
            }
            Step::Do(Action::Command { machine, command }) => {
                cell.command(scene, machine, command)?;
                self.step += 1;
                State::Begin
            }
            Step::Do(Action::Wait { .. }) => State::Waiting,
            Step::Do(Action::Delay { seconds }) => State::Until(time + seconds),
            Step::Do(Action::Next { pallet }) => {
                let count = cell.pallet_slots(scene, pallet).len();
                if count == 0 {
                    return Err("팔레트가 셀에 없습니다".into());
                }
                let at = self.places.entry(pallet).or_default();
                *at = (*at + 1) % count;
                self.step += 1;
                State::Begin
            }
        })
    }

    /// Where `point` is run, as joint positions: where it was taught, or
    /// — in a frame — moved as its equipment has moved since, and — on a
    /// pallet — over the place the pallet's count has reached, as far
    /// from it and turned as it was taught. The joints are solved from the
    /// taught ones, so the arm keeps the posture it was taught in.
    fn target(&self, point: &Point, cell: &Cell, scene: &Document) -> Result<Vec<f64>, String> {
        let taught = self.kin.with(&point.positions);
        let mut pose = self.kin.pose(&taught, &self.tool);
        // How the equipment has moved since: the identity if it has not.
        let mut moved = kreate_math::DAffine3::IDENTITY;
        if let Some(frame) = &point.frame {
            let id = scene
                .object_by_uid(kreate_scene::Uid::from_raw(frame.object))
                .ok_or_else(|| format!("{}의 기준 장비가 장면에 없습니다", point.name))?;
            moved = scene.world_transform(id) * frame.taught_in.inverse();
        }
        let shifted = !moved.abs_diff_eq(kreate_math::DAffine3::IDENTITY, 1e-9);
        pose = moved * pose;
        let mut goal = pose;
        if let Some(pallet) = point.pallet {
            goal.translation += self.place_shift(point, pallet, &taught, &moved, cell, scene)?;
        }
        if !shifted && goal.translation == pose.translation {
            return Ok(taught);
        }
        let (_, rotation, _) = goal.to_scale_rotation_translation();
        let found = self.kin.solve(
            &self.tool,
            &Goal {
                position: goal.translation,
                rotation: Some(rotation),
            },
            &taught,
            &Options {
                restarts: 8,
                ..Options::default()
            },
        );
        if !found.reached {
            return Err(match point.pallet {
                Some(_) => format!(
                    "{}: 팔레트 {}번 칸에 닿지 않습니다",
                    point.name,
                    self.places
                        .get(&point.pallet.unwrap_or_default())
                        .copied()
                        .unwrap_or(0)
                        + 1
                ),
                None => format!("{}: 옮겨진 장비 기준 위치에 닿지 않습니다", point.name),
            });
        }
        Ok(found.positions)
    }

    /// How far the place pallet `pallet`'s count has reached is from the
    /// place `point` was taught over: `taught` its joints, `moved` how its
    /// frame has moved since.
    fn place_shift(
        &self,
        point: &Point,
        pallet: u64,
        taught: &[f64],
        moved: &kreate_math::DAffine3,
        cell: &Cell,
        scene: &Document,
    ) -> Result<DVec3, String> {
        let slots = cell.pallet_slots(scene, pallet);
        if slots.is_empty() {
            return Err(format!("{}의 기준 팔레트가 셀에 없습니다", point.name));
        }
        let pose = *moved * self.kin.pose(taught, &self.tool);
        // The place it was taught over: the one nearest, seen from above,
        // the tool point or a grip point of this robot's — on a double
        // gripper, the grip point holding the part stands off the tool's.
        let points: Vec<DVec3> = std::iter::once(pose.translation)
            .chain(
                cell.grippers()
                    .into_iter()
                    .filter_map(|g| self.kin.tool(g, kreate_math::DAffine3::IDENTITY))
                    .map(|g| moved.transform_point3(self.kin.pose(taught, &g).translation)),
            )
            .collect();
        let flat = |v: DVec3| DVec3::new(v.x, v.y, 0.0).length();
        let gap = |i: usize| {
            points
                .iter()
                .map(|p| flat(slots[i] - *p))
                .fold(f64::INFINITY, f64::min)
        };
        let from = (0..slots.len())
            .min_by(|a, b| gap(*a).total_cmp(&gap(*b)))
            .expect("a place");
        let to = self.places.get(&pallet).copied().unwrap_or(0) % slots.len();
        Ok(slots[to] - slots[from])
    }

    /// What the robot is doing, for the timeline: `move`, `wait` (for a
    /// machine), `grip` (a gripper opening or closing), `pause`, `alarm`,
    /// or `idle` once the run has ended.
    pub fn activity(&self) -> &'static str {
        match &self.state {
            State::Begin => "move",
            State::Moving { .. } => "move",
            State::Waiting => "wait",
            State::Until(_) => match self.program.steps.get(self.step) {
                Some(Step::Do(Action::Delay { .. })) => "pause",
                _ => "grip",
            },
            State::Done => "idle",
            State::Alarm(_) => "alarm",
        }
    }

    /// What the `pose` event says of the run.
    pub fn to_json(&self, time: f64) -> Value {
        let (state, detail) = match &self.state {
            State::Begin => ("running", String::new()),
            State::Moving { plan, from, .. } => (
                "moving",
                format!(
                    "{:.1} / {:.1} s",
                    (time - from).min(plan.duration),
                    plan.duration
                ),
            ),
            State::Waiting => ("waiting", self.waiting_for()),
            State::Until(end) => ("pausing", format!("{:.1} s", (end - time).max(0.0))),
            State::Done => ("done", String::new()),
            State::Alarm(why) => ("alarm", why.clone()),
        };
        json!({
            "robot": self.root.to_string(),
            "step": self.step,
            "state": state,
            "detail": detail,
            "cycles": self.cycles,
            "last_cycle": self.last_cycle,
            "places": self.places.iter().map(|(p, i)| (p.to_string(), json!(i + 1))).collect::<serde_json::Map<_, _>>(),
            "cycle_time": time - self.cycle_from,
        })
    }

    fn waiting_for(&self) -> String {
        match self.program.steps.get(self.step) {
            Some(Step::Do(Action::Wait { signal, .. })) => signal_label(*signal).to_string(),
            _ => String::new(),
        }
    }
}

/// A signal, as the viewer names it.
pub fn signal_label(signal: Signal) -> &'static str {
    match signal {
        Signal::DoorOpen => "문 열림",
        Signal::DoorClosed => "문 닫힘",
        Signal::Clamped => "척 물림",
        Signal::Idle => "가공 대기",
        Signal::Done => "가공 완료",
    }
}

#[cfg(test)]
mod tests;
