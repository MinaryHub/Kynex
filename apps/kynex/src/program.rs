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
use kynex_kin::urscript::{self, Export, Move};
use kynex_kin::{Goal, Kinematics, Options, Tool};
use serde_json::{json, Value};

use crate::cell::{Command, Signal};

/// The programs file's layout, written into it.
pub const FILE_VERSION: u64 = 1;

/// A new joint move's share of each joint's top speed.
pub const JOINT_SPEED: f64 = 0.5;
/// A new linear move's speed, metres a second.
pub const LINEAR_SPEED: f64 = 0.25;

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
}

/// One line of a program: a move to a point, or an action.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Move(Point),
    Do(Action),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Program {
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
    /// A point at `positions`, after step `after` or at the end.
    pub fn teach(&mut self, kind: MoveKind, positions: BTreeMap<u64, f64>, after: Option<usize>) {
        self.taught += 1;
        let point = Point {
            name: format!("P{}", self.taught),
            kind,
            speed: match kind {
                MoveKind::Joint => JOINT_SPEED,
                MoveKind::Linear => LINEAR_SPEED,
            },
            positions,
            pallet: None,
            frame: None,
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
            tool: self.tool,
            offset: self.offset,
            ..Self::default()
        };
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
                    let mut positions = BTreeMap::new();
                    for j in s["joints"].as_array().into_iter().flatten() {
                        let Some(position) = j["position"].as_f64() else {
                            continue;
                        };
                        let name = text(&j["name"]).filter(|n| !n.is_empty());
                        if let Some(var) = pick(uid(&j["uid"]), name, &vars) {
                            positions.insert(var, position);
                        }
                    }
                    Step::Move(Point {
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
                    })
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
            tool: pick(uid(&saved["tool"]), text(&saved["tool_name"]), &parts),
            offset,
            close: saved["close"].as_bool().unwrap_or(false),
            taught: saved["taught"].as_u64().map_or(steps.len(), |t| t as usize),
            steps,
        };
        Some((root, program))
    }

    /// The program as URScript for a UR controller, and a file name for it.
    pub fn urscript(
        &self,
        root: u64,
        doc: &Document,
        kin: &Kinematics,
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
        // The actions, until their I/O is mapped, as comments where they
        // fall: someone finishing the program on the controller sees where
        // each goes.
        let name = |u: u64| names.get(&u).cloned().unwrap_or_else(|| u.to_string());
        let mut moves = Vec::new();
        let mut notes = Vec::new();
        let mut next = waypoints.iter();
        for step in &self.steps {
            match step {
                Step::Move(p) => moves.push(Move {
                    name: &p.name,
                    waypoint: next.next().expect("a waypoint per move"),
                    notes: std::mem::take(&mut notes),
                }),
                Step::Do(a) => notes.push(format!(
                    "KYNEX, not exported until its I/O is mapped: {}",
                    match a {
                        Action::Grip { gripper } => format!("close gripper {}", name(*gripper)),
                        Action::Release { gripper } => format!("open gripper {}", name(*gripper)),
                        Action::Command { machine, command } =>
                            format!("tell {} to {}", name(*machine), command.tag()),
                        Action::Wait { machine, signal } =>
                            format!("wait for {} to say {}", name(*machine), signal.tag()),
                        Action::Delay { seconds } => format!("sleep({seconds})"),
                        Action::Next { pallet } =>
                            format!("move pallet {} on to its next place", name(*pallet)),
                    }
                )),
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
                trailing: notes,
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
            .map(|(_, p)| Waypoint {
                kind: p.kind,
                positions: kin.with(&p.positions),
                speed: p.speed,
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
    }
}

pub fn kind_of(tag: &str) -> Option<MoveKind> {
    match tag {
        "joint" => Some(MoveKind::Joint),
        "linear" => Some(MoveKind::Linear),
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
                "직선 경로 {:.0}% 지점부터 도달 불가 (최대 {miss} 벗어남)",
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
