//! A robot's program: poses taught in the viewer and the moves between
//! them, timed by kynex-kin's planner, and between the moves what the robot
//! does to the cell — grip, release, tell a machine to open or start, wait
//! for it to say it is done (see [`Action`]).
//!
//! A point may be taught in a machine's or a pallet's frame (see
//! [`Frame`]): moved with the equipment, the point moves with it.
//!
//! A point may stand on a pallet (see [`Point::pallet`]): taught over one
//! of its places, it is run over the place the pallet's count has reached,
//! and [`Action::Next`] moves the count on — one program for every place.
//!
//! Positions are kept by joint identity, so a program outlives the session
//! scene being rebuilt around it; a joint it does not name stands where the
//! robot stands.
//!
//! Saved, a program keeps names beside identities (see [`Program::to_saved`]):
//! a scene Kreate builds anew from the same robot description numbers its
//! objects anew, and the program finds its robot, joints and tool again by
//! name.

use std::collections::{BTreeMap, HashMap, HashSet};

use kreate_math::{DAffine3, DVec3};
use kreate_scene::Document;
use kynex_kin::path::{self, MoveKind, Plan, Problem, Waypoint};
use kynex_kin::urscript::{self, Export, Line, Move};
use kynex_kin::{Goal, Kinematics, Options, Tool};
use serde_json::{json, Value};

use crate::cell::{Command, Signal};

/// The programs file's layout, written into it. 2: the library's UR robots
/// turned at the base as ur_description has them (see [`migrate`]).
pub const FILE_VERSION: u64 = 2;

/// Brings a program saved in a file of layout `version` up to this one.
///
/// Before layout 2 the library's UR robots hung their arm from `base_link`
/// unturned, where ur_description (and the controller) turn it half a
/// turn: the same pose then had the base joint half a turn round. A
/// program for one of those robots — placed equipment, by its identity —
/// has its base joint turned so, to stand where it was taught.
pub fn migrate(saved: &mut Value, version: u64) {
    let library_robot = saved["robot"]
        .as_str()
        .and_then(|s| s.parse::<u64>().ok())
        .is_some_and(|u| (crate::library::EQUIPMENT..crate::cell::PARTS).contains(&u));
    if version >= 2 || !library_robot {
        return;
    }
    let turn = |joints: &mut Value| {
        for j in joints.as_array_mut().into_iter().flatten() {
            if j["name"] == "shoulder_pan_joint" {
                if let Some(q) = j["position"].as_f64() {
                    let q = q + std::f64::consts::PI;
                    j["position"] = json!(if q > std::f64::consts::TAU - 1e-3 {
                        q - std::f64::consts::TAU
                    } else {
                        q
                    });
                }
            }
        }
    };
    for list in ["steps", "points"] {
        for step in saved[list].as_array_mut().into_iter().flatten() {
            turn(&mut step["joints"]);
            turn(&mut step["via"]);
        }
    }
}

/// A new joint move's share of each joint's top speed.
pub const JOINT_SPEED: f64 = 0.5;
/// A new linear move's speed, metres a second.
pub const LINEAR_SPEED: f64 = 0.25;

/// The largest blend radius, metres.
pub const MAX_BLEND: f64 = 0.5;

/// A robot's first program's name: the one a run of the cell goes round.
pub const MAIN: &str = "메인";

/// How deep calls go: deeper, a program calls itself round.
const MAX_CALL_DEPTH: usize = 8;

/// The most times a call repeats.
pub const MAX_TIMES: u32 = 1000;

/// The most steps a program comes to with its calls replaced: repeats
/// within repeats would otherwise multiply past any memory.
const MAX_FLAT_STEPS: usize = 100_000;

/// The spacing of a drawn path, seconds.
const TRACE_STEP: f64 = 0.02;

/// The equipment a point is taught in: run, the tool goes where it was
/// taught as seen from the equipment, wherever the equipment stands now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    /// The equipment: a CNC machine or a pallet.
    pub object: u64,
    /// Where it stood in the world when the point was taught.
    pub taught_in: DAffine3,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Point {
    pub name: String,
    pub kind: MoveKind,
    /// See [`Waypoint::speed`].
    pub speed: f64,
    pub positions: BTreeMap<u64, f64>,
    /// The pallet the point is taught over, if it is: run, the tool goes
    /// as far from the place the pallet's count has reached as it was
    /// taught from the place nearest it, turned as taught.
    pub pallet: Option<u64>,
    /// The equipment it is taught in, if any.
    pub frame: Option<Frame>,
    /// See [`Waypoint::blend`]: metres. Only into another move; before an
    /// action the robot stops.
    pub blend: f64,
    /// For a circular move, the point its arc passes through: joint
    /// positions, as `positions`.
    pub via: Option<BTreeMap<u64, f64>>,
}

/// What the robot does between moves.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Close grip point `gripper` on the part at it.
    Grip { gripper: u64 },
    /// Open it, letting go of the part.
    Release { gripper: u64 },
    /// Tell CNC machine `machine` to do something.
    Command { machine: u64, command: Command },
    /// Wait until machine `machine` says `signal`.
    Wait { machine: u64, signal: Signal },
    /// Wait this many seconds.
    Delay { seconds: f64 },
    /// Move pallet `pallet`'s count on to its next place, from its last
    /// back to its first.
    Next { pallet: u64 },
    /// Send AMR `amr` to stop `stop`; a wait for it to say it has arrived
    /// holds the robot until it has.
    Drive { amr: u64, stop: u64 },
    /// Run the robot's program `routine`, `times` over, and come back.
    Call { routine: String, times: u32 },
}

/// One line of a program: a move to a point, or an action.
#[derive(Clone, Debug, PartialEq)]
// A program is tens of steps: a point's size beside an action's costs
// nothing worth a box.
#[allow(clippy::large_enum_variant)]
pub enum Step {
    Move(Point),
    Do(Action),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Program {
    /// Its name among the robot's programs: how another calls it.
    pub name: String,
    pub steps: Vec<Step>,
    /// Ends with a move back to the first point: a cycle.
    pub close: bool,
    /// The tool the linear moves carry, and its point along it.
    pub tool: Option<u64>,
    pub offset: DVec3,
    /// Numbers the next point's name.
    taught: usize,
}

impl Program {
    /// An empty program `name`, its lines drawn with `tool`.
    pub fn named(name: &str, tool: Option<u64>, offset: DVec3) -> Self {
        Self {
            name: name.to_string(),
            tool,
            offset,
            ..Self::default()
        }
    }

    /// A point at `positions`, after step `after` or at the end.
    /// A circular move takes `via`, the point its arc passes through.
    pub fn teach(
        &mut self,
        kind: MoveKind,
        positions: BTreeMap<u64, f64>,
        via: Option<BTreeMap<u64, f64>>,
        after: Option<usize>,
    ) {
        self.taught += 1;
        let point = Point {
            name: format!("P{}", self.taught),
            kind,
            speed: match kind {
                MoveKind::Joint => JOINT_SPEED,
                MoveKind::Linear | MoveKind::Circular => LINEAR_SPEED,
            },
            positions,
            pallet: None,
            frame: None,
            blend: 0.0,
            via: via.filter(|_| kind == MoveKind::Circular),
        };
        self.insert(Step::Move(point), after);
    }

    /// `step`, after step `after` or at the end.
    pub fn insert(&mut self, step: Step, after: Option<usize>) {
        let at = after.map_or(self.steps.len(), |i| (i + 1).min(self.steps.len()));
        self.steps.insert(at, step);
    }

    /// The moves, each with its step's index.
    pub fn moves(&self) -> impl Iterator<Item = (usize, &Point)> {
        self.steps.iter().enumerate().filter_map(|(i, s)| match s {
            Step::Move(p) => Some((i, p)),
            Step::Do(_) => None,
        })
    }

    /// Copies steps `from` to `to` for CNC machine `machine`, after them:
    /// the same work at another machine. The machine the steps tell and
    /// wait for becomes `machine`; points in its frame are solved again to
    /// stand as far from `machine` as they stood from it, each from the one
    /// before so the arm keeps its posture; other points are copied as
    /// they are. Returns how many steps were added.
    pub fn copy_for(
        &mut self,
        from: usize,
        to: usize,
        machine: u64,
        doc: &Document,
        kin: &Kinematics,
    ) -> Result<usize, String> {
        if from > to || to >= self.steps.len() {
            return Err("복제할 구간이 맞지 않습니다".into());
        }
        let range = self.steps[from..=to].to_vec();
        // The machine the steps are for: the first they tell or wait for.
        let source = range
            .iter()
            .find_map(|s| match s {
                Step::Do(Action::Command { machine, .. } | Action::Wait { machine, .. }) => {
                    Some(*machine)
                }
                Step::Move(p) if p.pallet.is_none() => p.frame.map(|f| f.object),
                _ => None,
            })
            .ok_or("구간에 CNC 명령이나 CNC 기준 지점이 없습니다")?;
        if source == machine {
            return Err("같은 장비입니다".into());
        }
        let there = doc
            .object_by_uid(kreate_scene::Uid::from_raw(machine))
            .map(|id| doc.world_transform(id))
            .ok_or("no such machine")?;
        let tool = self.tool(kin).ok_or("툴이 정해지지 않았습니다")?;
        let mut seed: Option<Vec<f64>> = None;
        let mut copies = Vec::new();
        for step in range {
            copies.push(match step {
                Step::Do(Action::Command {
                    machine: m,
                    command,
                }) if m == source => Step::Do(Action::Command { machine, command }),
                Step::Do(Action::Wait { machine: m, signal }) if m == source => {
                    Step::Do(Action::Wait { machine, signal })
                }
                Step::Do(a) => Step::Do(a),
                Step::Move(mut p) => {
                    self.taught += 1;
                    p.name = format!("P{}", self.taught);
                    if let Some(frame) =
                        p.frame.filter(|f| f.object == source && p.pallet.is_none())
                    {
                        let q = kin.with(&p.positions);
                        let goal = there * frame.taught_in.inverse() * kin.pose(&q, &tool);
                        let (_, rotation, position) = goal.to_scale_rotation_translation();
                        let found = kin.solve(
                            &tool,
                            &Goal {
                                position,
                                rotation: Some(rotation),
                            },
                            seed.as_deref().unwrap_or(&q),
                            &Options {
                                restarts: 16,
                                ..Options::default()
                            },
                        );
                        if !found.reached {
                            return Err(format!("{}: 복제한 위치에 닿지 않습니다", p.name));
                        }
                        p.positions = kin
                            .vars
                            .iter()
                            .map(|v| v.uid)
                            .zip(found.positions.iter().copied())
                            .collect();
                        p.frame = Some(Frame {
                            object: machine,
                            taught_in: there,
                        });
                        seed = Some(found.positions);
                    }
                    Step::Move(p)
                }
            });
        }
        let added = copies.len();
        self.steps.splice(to + 1..to + 1, copies);
        Ok(added)
    }

    /// Moves steps `from` to `to` one place earlier (`up`) or later, past
    /// the step beside them. Returns whether they moved: not past either
    /// end.
    pub fn shift(&mut self, from: usize, to: usize, up: bool) -> bool {
        if from > to || to >= self.steps.len() {
            return false;
        }
        if up && from > 0 {
            self.steps[from - 1..=to].rotate_left(1);
            true
        } else if !up && to + 1 < self.steps.len() {
            self.steps[from..=to + 1].rotate_right(1);
            true
        } else {
            false
        }
    }

    /// The pallets it goes through place by place: those it moves on.
    pub fn counted_pallets(&self) -> Vec<u64> {
        let mut out: Vec<u64> = self
            .steps
            .iter()
            .filter_map(|s| match s {
                Step::Do(Action::Next { pallet }) => Some(*pallet),
                _ => None,
            })
            .collect();
        out.dedup();
        out
    }

    /// The point step `i` moves to, if it is a move.
    pub fn point_mut(&mut self, i: usize) -> Option<&mut Point> {
        match self.steps.get_mut(i)? {
            Step::Move(p) => Some(p),
            Step::Do(_) => None,
        }
    }

    /// No points, the same tool.
    pub fn clear(&mut self) {
        *self = Self {
            name: std::mem::take(&mut self.name),
            tool: self.tool,
            offset: self.offset,
            ..Self::default()
        };
    }

    /// The program with every call in it replaced by the steps of the
    /// program it calls, as many times as it says, from among `all` (the
    /// robot's programs): what a run goes through, and what is exported.
    /// Also returns where each step comes from: the program's name and
    /// the step's own index in it.
    pub fn flatten(&self, all: &[Program]) -> Result<(Program, Vec<(String, usize)>), String> {
        fn spend(budget: &mut usize) -> Result<(), String> {
            if *budget == 0 {
                return Err(format!(
                    "서브루틴을 펼치면 {MAX_FLAT_STEPS}단계를 넘습니다: 반복 횟수를 줄이세요"
                ));
            }
            *budget -= 1;
            Ok(())
        }
        // Every step made and every call gone into is taken from one
        // budget: repeats of a routine with no steps of its own would
        // otherwise go round past any time, making nothing.
        fn into(
            program: &Program,
            all: &[Program],
            depth: usize,
            budget: &mut usize,
            steps: &mut Vec<Step>,
            origin: &mut Vec<(String, usize)>,
        ) -> Result<(), String> {
            if depth > MAX_CALL_DEPTH {
                return Err(format!(
                    "서브루틴 호출이 {MAX_CALL_DEPTH}단계보다 깊습니다: 스스로를 부르는지 보세요"
                ));
            }
            for (i, step) in program.steps.iter().enumerate() {
                match step {
                    Step::Do(Action::Call { routine, times }) => {
                        let called = all
                            .iter()
                            .find(|p| p.name == *routine)
                            .ok_or_else(|| format!("서브루틴 {routine}이(가) 없습니다"))?;
                        for _ in 0..*times {
                            spend(budget)?;
                            into(called, all, depth + 1, budget, steps, origin)?;
                        }
                    }
                    other => {
                        spend(budget)?;
                        steps.push(other.clone());
                        origin.push((program.name.clone(), i));
                    }
                }
            }
            Ok(())
        }
        let mut steps = Vec::new();
        let mut origin = Vec::new();
        let mut budget = MAX_FLAT_STEPS;
        into(self, all, 0, &mut budget, &mut steps, &mut origin)?;
        Ok((
            Program {
                steps,
                ..self.clone()
            },
            origin,
        ))
    }

    /// As saved to the programs file: identities and names both.
    pub fn to_saved(&self, root: u64, doc: &Document, kin: &Kinematics) -> Value {
        let names = names(doc);
        let name = |uid: u64| names.get(&uid).cloned().unwrap_or_default();
        let var_name: HashMap<u64, &str> =
            kin.vars.iter().map(|v| (v.uid, v.name.as_str())).collect();
        json!({
            "robot": root.to_string(),
            "robot_name": name(root),
            "tool": self.tool.map(|t| t.to_string()),
            "tool_name": self.tool.map(name),
            "offset": [self.offset.x, self.offset.y, self.offset.z],
            "close": self.close,
            "taught": self.taught,
            "routine": self.name,
            "steps": self.steps.iter().map(|s| match s {
                Step::Move(p) => json!({
                    "type": "move",
                    "name": p.name,
                    "kind": kind_tag(p.kind),
                    "speed": p.speed,
                    "joints": p.positions.iter().map(|(uid, q)| json!({
                        "uid": uid.to_string(),
                        "name": var_name.get(uid).copied().unwrap_or(""),
                        "position": q,
                    })).collect::<Vec<_>>(),
                    "pallet": p.pallet.map(|u| u.to_string()),
                    "pallet_name": p.pallet.map(name),
                    "frame": p.frame.map(|f| f.object.to_string()),
                    "frame_name": p.frame.map(|f| name(f.object)),
                    "frame_at": p.frame.map(|f| affine_json(&f.taught_in)),
                    "blend": p.blend,
                    "via": p.via.as_ref().map(|via| via.iter().map(|(uid, q)| json!({
                        "uid": uid.to_string(),
                        "name": var_name.get(uid).copied().unwrap_or(""),
                        "position": q,
                    })).collect::<Vec<_>>()),
                }),
                Step::Do(a) => action_json(a, &|u| name(u)),
            }).collect::<Vec<_>>(),
        })
    }

    /// A saved program, and the robot it is for, found in `doc` by identity
    /// or else by name. `None` while that robot is not in the scene.
    pub fn from_saved(saved: &Value, doc: &Document) -> Option<(u64, Program)> {
        let text = |v: &Value| v.as_str().map(str::to_string);
        let uid = |v: &Value| v.as_str().and_then(|s| s.parse::<u64>().ok());
        // An identity counts only while its name still agrees: a scene built
        // anew numbers its objects anew, and the old number may now be a
        // floor, or another part of the same robot. Otherwise, by name.
        let pick = |id: Option<u64>, name: Option<String>, candidates: &[(u64, &str)]| {
            let agrees = |u: u64| {
                candidates
                    .iter()
                    .any(|(c, n)| *c == u && name.as_deref().is_none_or(|want| want == *n))
            };
            id.filter(|u| agrees(*u)).or_else(|| {
                let name = name.as_deref()?;
                candidates.iter().find(|(_, n)| *n == name).map(|(c, _)| *c)
            })
        };
        let robots = kynex_model::robots(doc);
        let roots: Vec<(u64, &str)> = robots.iter().map(|r| (r.root, r.name.as_str())).collect();
        let root = pick(uid(&saved["robot"]), text(&saved["robot_name"]), &roots)?;
        let kin = Kinematics::of(doc, root)?;
        let names = names(doc);
        let parts: Vec<(u64, &str)> = names
            .iter()
            .filter(|(u, _)| kin.contains(**u))
            .map(|(u, n)| (*u, n.as_str()))
            .collect();
        let vars: Vec<(u64, &str)> = kin.vars.iter().map(|v| (v.uid, v.name.as_str())).collect();
        // Machines and grip points anywhere in the scene, by name too.
        let everything: Vec<(u64, &str)> = names.iter().map(|(u, n)| (*u, n.as_str())).collect();
        let object = |v: &Value, key: &str| {
            pick(uid(&v[key]), text(&v[&format!("{key}_name")]), &everything)
        };
        let mut steps = Vec::new();
        // "points" is how a program was kept before it had actions.
        let saved_steps = saved["steps"].as_array().or(saved["points"].as_array())?;
        for s in saved_steps {
            let step = match s["type"].as_str().unwrap_or("move") {
                "move" => {
                    let joints = |list: &Value| {
                        let mut positions = BTreeMap::new();
                        for j in list.as_array().into_iter().flatten() {
                            let Some(position) = j["position"].as_f64() else {
                                continue;
                            };
                            let name = text(&j["name"]).filter(|n| !n.is_empty());
                            if let Some(var) = pick(uid(&j["uid"]), name, &vars) {
                                positions.insert(var, position);
                            }
                        }
                        positions
                    };
                    let positions = joints(&s["joints"]);
                    let mut point = Point {
                        name: text(&s["name"]).unwrap_or_else(|| format!("P{}", steps.len() + 1)),
                        kind: kind_of(s["kind"].as_str().unwrap_or("joint"))?,
                        speed: s["speed"].as_f64().unwrap_or(JOINT_SPEED),
                        positions,
                        pallet: if s["pallet"].is_string() {
                            object(s, "pallet")
                        } else {
                            None
                        },
                        frame: if s["frame"].is_string() {
                            object(s, "frame")
                                .zip(affine_of(&s["frame_at"]))
                                .map(|(object, taught_in)| Frame { object, taught_in })
                        } else {
                            None
                        },
                        blend: s["blend"].as_f64().unwrap_or(0.0).clamp(0.0, MAX_BLEND),
                        via: s["via"].is_array().then(|| joints(&s["via"])),
                    };
                    // Saved before points had frames: a pallet point is in
                    // its pallet's frame, where the pallet stands now.
                    if let (Some(pallet), None) = (point.pallet, point.frame) {
                        point.frame =
                            doc.object_by_uid(kreate_scene::Uid::from_raw(pallet))
                                .map(|id| Frame {
                                    object: pallet,
                                    taught_in: doc.world_transform(id),
                                });
                    }
                    Step::Move(point)
                }
                "grip" => Step::Do(Action::Grip {
                    gripper: object(s, "gripper")?,
                }),
                "release" => Step::Do(Action::Release {
                    gripper: object(s, "gripper")?,
                }),
                "command" => Step::Do(Action::Command {
                    machine: object(s, "machine")?,
                    command: Command::of(s["command"].as_str()?)?,
                }),
                "wait" => Step::Do(Action::Wait {
                    machine: object(s, "machine")?,
                    signal: Signal::of(s["signal"].as_str()?)?,
                }),
                "delay" => Step::Do(Action::Delay {
                    seconds: s["seconds"].as_f64().unwrap_or(1.0),
                }),
                "next" => Step::Do(Action::Next {
                    pallet: object(s, "pallet")?,
                }),
                "drive" => Step::Do(Action::Drive {
                    amr: object(s, "amr")?,
                    stop: object(s, "stop")?,
                }),
                "call" => Step::Do(Action::Call {
                    routine: text(&s["routine"])?,
                    times: s["times"].as_u64().unwrap_or(1).clamp(1, MAX_TIMES as u64) as u32,
                }),
                _ => continue,
            };
            steps.push(step);
        }
        let offset = match saved["offset"].as_array().map(Vec::as_slice) {
            Some([x, y, z]) => DVec3::new(
                x.as_f64().unwrap_or(0.0),
                y.as_f64().unwrap_or(0.0),
                z.as_f64().unwrap_or(0.0),
            ),
            _ => DVec3::ZERO,
        };
        let program = Program {
            name: text(&saved["routine"]).unwrap_or_else(|| MAIN.to_string()),
            tool: pick(uid(&saved["tool"]), text(&saved["tool_name"]), &parts),
            offset,
            close: saved["close"].as_bool().unwrap_or(false),
            taught: saved["taught"].as_u64().map_or(steps.len(), |t| t as usize),
            steps,
        };
        Some((root, program))
    }

    /// The program as URScript for a UR controller, and a file name for it.
    /// With `markers`, the moves are numbered on an output register, to be
    /// timed (see [`urscript::Export::markers`]).
    ///
    /// `cell` says how the cell is wired: an action whose gripper or
    /// machine is wired goes as script — an output set, an input waited
    /// for — and one that is not, as a comment where it falls, for whoever
    /// finishes the program on the controller. Points on a pallet go to the
    /// place the script counts to, as the run does.
    pub fn urscript(
        &self,
        root: u64,
        doc: &Document,
        kin: &Kinematics,
        markers: bool,
        cell: &CellIo,
    ) -> Result<(String, String), String> {
        let tool = self.tool(kin).ok_or("툴이 정해지지 않았습니다")?;
        let names = names(doc);
        // The controller's frames, as ur_description names them.
        let named = |want: &[&str]| {
            want.iter().find_map(|w| {
                names
                    .iter()
                    .find(|(u, n)| n.eq_ignore_ascii_case(w) && kin.contains(**u))
                    .map(|(u, _)| *u)
            })
        };
        let base = named(&["base"]).unwrap_or(root);
        let flange = named(&["tool0", "flange"])
            .or(self.tool)
            .ok_or("no flange")?;
        let waypoints = self.waypoints(kin);
        let robot = names.get(&root).cloned().unwrap_or_else(|| "robot".into());
        let name = |u: u64| names.get(&u).cloned().unwrap_or_else(|| u.to_string());
        let any = waypoints
            .first()
            .map(|w| w.positions.clone())
            .unwrap_or_default();
        let base_at = kin.world_of(&any, base).ok_or("no base frame")?;

        // The pallets the program counts through: each its places, in the
        // base frame, and a count from 0.
        let mut pallets: Vec<u64> = Vec::new();
        for step in &self.steps {
            let pallet = match step {
                Step::Move(p) => p.pallet,
                Step::Do(Action::Next { pallet }) => Some(*pallet),
                _ => None,
            };
            if let Some(pallet) = pallet.filter(|p| cell.pallets.contains_key(p)) {
                if !pallets.contains(&pallet) {
                    pallets.push(pallet);
                }
            }
        }
        let mut globals = Vec::new();
        for (k, pallet) in pallets.iter().enumerate() {
            let places: Vec<String> = cell.pallets[pallet]
                .iter()
                .map(|at| {
                    let p = base_at.inverse().transform_point3(*at);
                    format!("p[{:.6}, {:.6}, {:.6}, 0, 0, 0]", p.x, p.y, p.z)
                })
                .collect();
            globals.push(format!(
                "# The places of {}",
                urscript::comment(&name(*pallet))
            ));
            globals.push(format!("kynex_places_{k} = [{}]", places.join(", ")));
            globals.push(format!("kynex_place_{k} = 0"));
        }
        // How far the place a point's pallet has counted to is from the
        // place it was taught over (see `crate::run`).
        let offset = |p: &Point| -> Option<String> {
            let pallet = p.pallet?;
            let k = pallets.iter().position(|q| *q == pallet)?;
            let slots = &cell.pallets[&pallet];
            let q = kin.with(&p.positions);
            let points: Vec<DVec3> = std::iter::once(kin.pose(&q, &tool).translation)
                .chain(
                    cell.grippers
                        .iter()
                        .filter_map(|g| kin.tool(*g, DAffine3::IDENTITY))
                        .map(|g| kin.pose(&q, &g).translation),
                )
                .collect();
            let flat = |v: DVec3| DVec3::new(v.x, v.y, 0.0).length();
            let gap = |i: usize| {
                points
                    .iter()
                    .map(|a| flat(slots[i] - *a))
                    .fold(f64::INFINITY, f64::min)
            };
            let from = (0..slots.len()).min_by(|a, b| gap(*a).total_cmp(&gap(*b)))?;
            Some(format!(
                "pose_sub(kynex_places_{k}[kynex_place_{k}], kynex_places_{k}[{from}])"
            ))
        };

        let mut moves = Vec::new();
        let mut lines = Vec::new();
        let mut next = waypoints.iter();
        for step in &self.steps {
            match step {
                Step::Move(p) => moves.push(Move {
                    name: &p.name,
                    waypoint: next.next().expect("a waypoint per move"),
                    notes: std::mem::take(&mut lines),
                    offset: offset(p),
                }),
                Step::Do(a) => lines.push(action_line(a, cell, &pallets, &name)),
            }
        }
        let script = urscript::urscript(
            kin,
            &tool,
            &Export {
                name: &robot,
                moves,
                close: self.close,
                base,
                flange,
                trailing: lines,
                globals,
                markers,
            },
        )?;
        let file: String = robot
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        Ok((script, format!("{file}.script")))
    }

    pub fn tool(&self, kin: &Kinematics) -> Option<Tool> {
        kin.tool(self.tool?, DAffine3::from_translation(self.offset))
    }

    /// The moves as the planner takes them, in order.
    pub fn waypoints(&self, kin: &Kinematics) -> Vec<Waypoint> {
        self.moves()
            .map(|(i, p)| Waypoint {
                kind: p.kind,
                positions: kin.with(&p.positions),
                via: p.via.as_ref().map(|v| kin.with(v)),
                speed: p.speed,
                // A blend into the next move only: an action stops the robot.
                blend: match self.steps.get(i + 1) {
                    Some(Step::Move(_)) => p.blend,
                    _ => 0.0,
                },
            })
            .collect()
    }

    /// Move by move in `plan`'s order, where a gripper works at either end:
    /// at a point a grip or a release follows, the grip point there; at one
    /// it departs from after a grip or a release, the same.
    pub fn work_places(&self, kin: &Kinematics, plan: &Plan) -> Vec<Vec<DVec3>> {
        let moves: Vec<usize> = self.moves().map(|(i, _)| i).collect();
        let waypoints = self.waypoints(kin);
        let at = |m: usize| -> Option<DVec3> {
            let i = *moves.get(m)?;
            let gripper = match self.steps.get(i + 1)? {
                Step::Do(Action::Grip { gripper } | Action::Release { gripper }) => *gripper,
                _ => return None,
            };
            let tool = kin.tool(gripper, DAffine3::IDENTITY)?;
            Some(kin.pose(&waypoints[m].positions, &tool).translation)
        };
        plan.segments
            .iter()
            .map(|seg| {
                let to = seg.to;
                let from = if to == 0 { moves.len() - 1 } else { to - 1 };
                at(to).into_iter().chain(at(from)).collect()
            })
            .collect()
    }

    /// Move by move in the plan's order, the machines this program has
    /// opened by then: their doors are out of the way. A closed program's
    /// move back to its start comes last, with the machines open at its end.
    pub fn open_during_moves(&self) -> Vec<HashSet<u64>> {
        let mut open = HashSet::new();
        let mut out = Vec::new();
        for (i, step) in self.steps.iter().enumerate() {
            match step {
                Step::Do(Action::Command {
                    machine,
                    command: Command::Open,
                }) => {
                    open.insert(*machine);
                }
                Step::Do(Action::Command {
                    machine,
                    command: Command::Close,
                }) => {
                    open.remove(machine);
                }
                // The plan's moves are into its second point on: the first
                // is where it starts.
                Step::Move(_) if self.moves().next().map(|m| m.0) != Some(i) => {
                    out.push(open.clone());
                }
                _ => {}
            }
        }
        if self.close {
            out.push(open);
        }
        out
    }

    /// The program timed, when it has a tool to time its lines by.
    pub fn plan(&self, kin: &Kinematics) -> Option<Plan> {
        let tool = self.tool(kin)?;
        Some(path::plan(kin, &tool, &self.waypoints(kin), self.close))
    }

    /// What the viewer shows of it: its steps, each move's time and
    /// problems, the time of its moves and the tool's path.
    ///
    /// `plan` is [`Program::plan`]'s; `more` adds problems of the plan's
    /// own moves, in its order: the collisions found along them. `names`
    /// names the machines and grip points the actions are for.
    pub fn to_json(
        &self,
        kin: &Kinematics,
        plan: Option<&Plan>,
        more: &[Vec<String>],
        names: &HashMap<u64, String>,
    ) -> Value {
        let tool = self.tool(kin);
        // The move into each point, by the point, with its index in the plan.
        let mut into: BTreeMap<usize, (usize, &path::Segment)> = BTreeMap::new();
        let mut back = None;
        if let Some(plan) = plan {
            for (i, seg) in plan.segments.iter().enumerate() {
                if seg.to == 0 {
                    back = Some((i, seg));
                } else {
                    into.insert(seg.to, (i, seg));
                }
            }
        }
        let move_json = |(i, seg): (usize, &path::Segment)| {
            let problems: Vec<String> = seg
                .problems
                .iter()
                .map(|p| describe(kin, p))
                .chain(more.get(i).into_iter().flatten().cloned())
                .collect();
            json!({
                "duration": seg.duration,
                "problems": problems,
            })
        };
        let waypoints = self.waypoints(kin);
        let mut nth = 0;
        let name = |u: u64| names.get(&u).cloned().unwrap_or_default();
        let steps: Vec<Value> = self
            .steps
            .iter()
            .map(|s| match s {
                Step::Move(p) => {
                    // The plan numbers the moves among themselves.
                    let i = nth;
                    nth += 1;
                    let at = tool.as_ref().map(|t| {
                        let p = kin.pose(&waypoints[i].positions, t).translation;
                        [p.x, p.y, p.z]
                    });
                    json!({
                        "type": "move",
                        "name": p.name,
                        "kind": kind_tag(p.kind),
                        "speed": p.speed,
                        "at": at,
                        "move": into.get(&i).map(|s| move_json(*s)),
                        "pallet": p.pallet.map(|u| u.to_string()),
                        "pallet_name": p.pallet.map(name),
                        "frame": p.frame.map(|f| f.object.to_string()),
                        "frame_name": p.frame.map(|f| name(f.object)),
                        "blend": p.blend,
                        "via_at": match (&p.via, &tool) {
                            (Some(via), Some(t)) => {
                                let v = kin.pose(&kin.with(via), t).translation;
                                Some([v.x, v.y, v.z])
                            }
                            _ => None,
                        },
                    })
                }
                Step::Do(a) => action_json(a, &name),
            })
            .collect();
        let trace: Vec<f64> = match (&plan, &tool) {
            (Some(plan), Some(tool)) if plan.duration > 0.0 => {
                path::trace(kin, tool, plan, TRACE_STEP)
                    .into_iter()
                    .flat_map(|p| [p.x, p.y, p.z])
                    .collect()
            }
            _ => Vec::new(),
        };
        json!({
            "steps": steps,
            "close": self.close,
            "back": back.map(move_json),
            "cycle": plan.as_ref().map(|p| p.duration),
            "trace": trace,
        })
    }
}

/// What a program's export needs of the cell (see [`Program::urscript`]).
#[derive(Clone, Debug, Default)]
pub struct CellIo {
    /// The digital I/O each component is wired to, by its identity and
    /// what it does (see `crate::cell::io_points`).
    pub io: HashMap<(u64, String), u32>,
    /// Each pallet's places, in the world.
    pub pallets: HashMap<u64, Vec<DVec3>>,
    /// The grip points, and how long each takes to open or close.
    pub grippers: Vec<u64>,
    pub actuate: HashMap<u64, f64>,
}

impl CellIo {
    fn port(&self, object: u64, key: &str) -> Option<u32> {
        self.io.get(&(object, key.to_string())).copied()
    }
}

/// How long a command to a machine holds its output, seconds: a pulse the
/// machine sees.
const PULSE: f64 = 0.2;

/// URScript setting digital output `n` (0–7 standard, 8–15 configurable,
/// 16–17 the tool's).
fn set_out(n: u32, on: bool) -> String {
    let on = if on { "True" } else { "False" };
    match n {
        0..=7 => format!("set_standard_digital_out({n}, {on})"),
        8..=15 => format!("set_configurable_digital_out({}, {on})", n - 8),
        _ => format!("set_tool_digital_out({}, {on})", n - 16),
    }
}

/// URScript reading digital input `n`, numbered as [`set_out`] numbers.
fn get_in(n: u32) -> String {
    match n {
        0..=7 => format!("get_standard_digital_in({n})"),
        8..=15 => format!("get_configurable_digital_in({})", n - 8),
        _ => format!("get_tool_digital_in({})", n - 16),
    }
}

/// URScript waiting until `condition` holds.
fn wait_until(condition: &str) -> String {
    format!("while (not {condition}):\n  sync()\nend")
}

/// Action `a` as script where the cell is wired for it, or as a comment.
fn action_line(a: &Action, cell: &CellIo, pallets: &[u64], name: &dyn Fn(u64) -> String) -> Line {
    let note = |what: String| {
        Line::Note(format!(
            "KYNEX, not exported until its I/O is mapped: {what}"
        ))
    };
    match a {
        Action::Grip { gripper } | Action::Release { gripper } => {
            let close = matches!(a, Action::Grip { .. });
            let what = if close { "close" } else { "open" };
            let Some(out) = cell.port(*gripper, "close") else {
                return note(format!("{what} gripper {}", name(*gripper)));
            };
            // Closed: its sensor says so, or else its time is let pass.
            let settle = match cell.port(*gripper, "closed") {
                Some(n) if close => wait_until(&get_in(n)),
                Some(n) => wait_until(&format!("not {}", get_in(n))),
                None => format!(
                    "sleep({:.2})",
                    cell.actuate.get(gripper).copied().unwrap_or(0.5)
                ),
            };
            Line::Code(format!("{}\n{settle}", set_out(out, close)))
        }
        Action::Command { machine, command } => match cell.port(*machine, command.tag()) {
            Some(n) => Line::Code(format!(
                "{}\nsleep({PULSE})\n{}",
                set_out(n, true),
                set_out(n, false)
            )),
            None => note(format!("tell {} to {}", name(*machine), command.tag())),
        },
        Action::Wait { machine, signal } => match cell.port(*machine, signal.tag()) {
            Some(n) => Line::Code(wait_until(&get_in(n))),
            None => note(format!(
                "wait for {} to say {}",
                name(*machine),
                signal.tag()
            )),
        },
        Action::Delay { seconds } => Line::Code(format!("sleep({seconds})")),
        Action::Next { pallet } => match pallets.iter().position(|p| p == pallet) {
            Some(k) => Line::Code(format!(
                "kynex_place_{k} = (kynex_place_{k} + 1) % {}",
                cell.pallets[pallet].len()
            )),
            None => note(format!(
                "move pallet {} on to its next place",
                name(*pallet)
            )),
        },
        Action::Drive { amr, stop } => note(format!("send AMR {} to {}", name(*amr), name(*stop))),
        // Calls are gone by the time a program is exported (see `flatten`).
        Action::Call { routine, times } => note(format!("call {routine} {times} times")),
    }
}

/// The requests [`crate::pose::Posing`] hands to a robot's program.
pub fn is_op(op: &str) -> bool {
    matches!(
        op,
        "program"
            | "teach"
            | "add"
            | "retouch"
            | "edit"
            | "delete"
            | "close"
            | "clear"
            | "goto"
            | "urscript"
            | "copy"
            | "shift"
            | "routine_new"
            | "routine_select"
            | "routine_rename"
            | "routine_delete"
    )
}

/// An action, as saved and as the viewer reads it: its machine or grip
/// point by identity and by name.
fn action_json(a: &Action, name: &dyn Fn(u64) -> String) -> Value {
    match a {
        Action::Grip { gripper } | Action::Release { gripper } => json!({
            "type": if matches!(a, Action::Grip { .. }) { "grip" } else { "release" },
            "gripper": gripper.to_string(),
            "gripper_name": name(*gripper),
        }),
        Action::Command { machine, command } => json!({
            "type": "command",
            "machine": machine.to_string(),
            "machine_name": name(*machine),
            "command": command.tag(),
        }),
        Action::Wait { machine, signal } => json!({
            "type": "wait",
            "machine": machine.to_string(),
            "machine_name": name(*machine),
            "signal": signal.tag(),
        }),
        Action::Delay { seconds } => json!({ "type": "delay", "seconds": seconds }),
        Action::Next { pallet } => json!({
            "type": "next",
            "pallet": pallet.to_string(),
            "pallet_name": name(*pallet),
        }),
        Action::Call { routine, times } => json!({
            "type": "call",
            "routine": routine,
            "times": times,
        }),
        Action::Drive { amr, stop } => json!({
            "type": "drive",
            "amr": amr.to_string(),
            "amr_name": name(*amr),
            "stop": stop.to_string(),
            "stop_name": name(*stop),
        }),
    }
}

/// An action from the page: the step `v` describes.
pub fn action_of(v: &Value) -> Result<Action, String> {
    let uid = |key: &str| {
        v[key]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or_else(|| format!("no {key}"))
    };
    Ok(match v["type"].as_str().ok_or("no type")? {
        "grip" => Action::Grip {
            gripper: uid("gripper")?,
        },
        "release" => Action::Release {
            gripper: uid("gripper")?,
        },
        "command" => Action::Command {
            machine: uid("machine")?,
            command: v["command"]
                .as_str()
                .and_then(Command::of)
                .ok_or("unknown command")?,
        },
        "wait" => Action::Wait {
            machine: uid("machine")?,
            signal: v["signal"]
                .as_str()
                .and_then(Signal::of)
                .ok_or("unknown signal")?,
        },
        "delay" => Action::Delay {
            seconds: v["seconds"].as_f64().unwrap_or(1.0).clamp(0.0, 3600.0),
        },
        "next" => Action::Next {
            pallet: uid("pallet")?,
        },
        "drive" => Action::Drive {
            amr: uid("amr")?,
            stop: uid("stop")?,
        },
        "call" => Action::Call {
            routine: v["routine"]
                .as_str()
                .filter(|r| !r.trim().is_empty())
                .ok_or("no routine")?
                .trim()
                .to_string(),
            times: v["times"].as_u64().unwrap_or(1).clamp(1, MAX_TIMES as u64) as u32,
        },
        other => return Err(format!("unknown step {other:?}")),
    })
}

/// A placement as saved: its matrix, column by column (the axes, then where
/// it is), so that it reads back exactly.
fn affine_json(a: &DAffine3) -> Value {
    json!(a.to_cols_array())
}

fn affine_of(v: &Value) -> Option<DAffine3> {
    let list = v.as_array()?;
    let cols: Vec<f64> = list.iter().filter_map(Value::as_f64).collect();
    let cols: [f64; 12] = cols.try_into().ok()?;
    Some(DAffine3::from_cols_array(&cols))
}

/// Every object's name, by identity.
pub fn names(doc: &Document) -> HashMap<u64, String> {
    doc.objects()
        .filter_map(|(id, o)| Some((doc.object_uid(id)?.raw(), o.name.clone())))
        .collect()
}

pub fn kind_tag(kind: MoveKind) -> &'static str {
    match kind {
        MoveKind::Joint => "joint",
        MoveKind::Linear => "linear",
        MoveKind::Circular => "circular",
    }
}

pub fn kind_of(tag: &str) -> Option<MoveKind> {
    match tag {
        "joint" => Some(MoveKind::Joint),
        "linear" => Some(MoveKind::Linear),
        "circular" => Some(MoveKind::Circular),
        _ => None,
    }
}

/// A move's problem, as the viewer says it.
fn describe(kin: &Kinematics, problem: &Problem) -> String {
    let name = |i: usize| kin.vars.get(i).map_or("?", |v| v.name.as_str());
    match problem {
        Problem::Unreachable {
            along,
            position,
            rotation,
        } => {
            let mut miss = format!("{:.1} mm", position * 1000.0);
            if *rotation > 1e-3 {
                miss.push_str(&format!(", {:.1}°", rotation.to_degrees()));
            }
            format!(
                "경로 {:.0}% 지점부터 도달 불가 (최대 {miss} 벗어남)",
                along * 100.0
            )
        }
        Problem::Slowed { joint, factor } => {
            format!("{} 관절 속도 한계로 {:.1}배 느려짐", name(*joint), factor)
        }
        Problem::Jump { joint, by } => {
            let var = kin.vars.get(*joint);
            let amount = if var.is_some_and(|v| v.kind.slides()) {
                format!("{:.1} mm", by * 1000.0)
            } else {
                format!("{:.1}°", by.to_degrees())
            };
            format!("끝에서 자세 구성이 바뀜 ({} {amount} 점프)", name(*joint))
        }
    }
}
