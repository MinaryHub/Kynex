//! A robot's program: poses taught in the viewer and the moves between
//! them, timed by kynex-kin's planner.
//!
//! Positions are kept by joint identity, so a program outlives the session
//! scene being rebuilt around it; a joint it does not name stands where the
//! robot stands.
//!
//! Saved, a program keeps names beside identities (see [`Program::to_saved`]):
//! a scene Kreate builds anew from the same robot description numbers its
//! objects anew, and the program finds its robot, joints and tool again by
//! name.

use std::collections::{BTreeMap, HashMap};

use kreate_math::{DAffine3, DVec3};
use kreate_scene::Document;
use kynex_kin::path::{self, MoveKind, Plan, Problem, Waypoint};
use kynex_kin::urscript::{self, Export, Move};
use kynex_kin::{Kinematics, Tool};
use serde_json::{json, Value};

/// The programs file's layout, written into it.
pub const FILE_VERSION: u64 = 1;

/// A new joint move's share of each joint's top speed.
pub const JOINT_SPEED: f64 = 0.5;
/// A new linear move's speed, metres a second.
pub const LINEAR_SPEED: f64 = 0.25;

/// The spacing of a drawn path, seconds.
const TRACE_STEP: f64 = 0.02;

#[derive(Clone, Debug, PartialEq)]
pub struct Point {
    pub name: String,
    pub kind: MoveKind,
    /// See [`Waypoint::speed`].
    pub speed: f64,
    pub positions: BTreeMap<u64, f64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Program {
    pub points: Vec<Point>,
    /// Ends with a move back to the first point: a cycle.
    pub close: bool,
    /// The tool the linear moves carry, and its point along it.
    pub tool: Option<u64>,
    pub offset: DVec3,
    /// Numbers the next point's name.
    taught: usize,
}

impl Program {
    /// A point at `positions`, after `after` or at the end.
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
        };
        let at = after.map_or(self.points.len(), |i| (i + 1).min(self.points.len()));
        self.points.insert(at, point);
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
            "points": self.points.iter().map(|p| json!({
                "name": p.name,
                "kind": kind_tag(p.kind),
                "speed": p.speed,
                "joints": p.positions.iter().map(|(uid, q)| json!({
                    "uid": uid.to_string(),
                    "name": var_name.get(uid).copied().unwrap_or(""),
                    "position": q,
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
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
        let mut points = Vec::new();
        for p in saved["points"].as_array()? {
            let mut positions = BTreeMap::new();
            for j in p["joints"].as_array().into_iter().flatten() {
                let Some(position) = j["position"].as_f64() else {
                    continue;
                };
                let name = text(&j["name"]).filter(|n| !n.is_empty());
                if let Some(var) = pick(uid(&j["uid"]), name, &vars) {
                    positions.insert(var, position);
                }
            }
            points.push(Point {
                name: text(&p["name"]).unwrap_or_else(|| format!("P{}", points.len() + 1)),
                kind: kind_of(p["kind"].as_str().unwrap_or("joint"))?,
                speed: p["speed"].as_f64().unwrap_or(JOINT_SPEED),
                positions,
            });
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
            taught: saved["taught"]
                .as_u64()
                .map_or(points.len(), |t| t as usize)
                .max(points.len()),
            points,
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
        let script = urscript::urscript(
            kin,
            &tool,
            &Export {
                name: &robot,
                moves: self
                    .points
                    .iter()
                    .zip(&waypoints)
                    .map(|(p, w)| Move {
                        name: &p.name,
                        waypoint: w,
                    })
                    .collect(),
                close: self.close,
                base,
                flange,
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

    pub fn waypoints(&self, kin: &Kinematics) -> Vec<Waypoint> {
        self.points
            .iter()
            .map(|p| Waypoint {
                kind: p.kind,
                positions: kin.with(&p.positions),
                speed: p.speed,
            })
            .collect()
    }

    /// The program timed, when it has a tool to time its lines by.
    pub fn plan(&self, kin: &Kinematics) -> Option<Plan> {
        let tool = self.tool(kin)?;
        Some(path::plan(kin, &tool, &self.waypoints(kin), self.close))
    }

    /// What the viewer shows of it: its points, each move's time and
    /// problems, the cycle time and the tool's path.
    ///
    /// `plan` is [`Program::plan`]'s; `more` adds problems of the plan's
    /// own moves, in its order: the collisions found along them.
    pub fn to_json(&self, kin: &Kinematics, plan: Option<&Plan>, more: &[Vec<String>]) -> Value {
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
        let points: Vec<Value> = self
            .points
            .iter()
            .zip(&waypoints)
            .enumerate()
            .map(|(i, (p, w))| {
                let at = tool.as_ref().map(|t| {
                    let p = kin.pose(&w.positions, t).translation;
                    [p.x, p.y, p.z]
                });
                json!({
                    "name": p.name,
                    "kind": kind_tag(p.kind),
                    "speed": p.speed,
                    "at": at,
                    "move": into.get(&i).map(|s| move_json(*s)),
                })
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
            "points": points,
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
            | "retouch"
            | "edit"
            | "delete"
            | "close"
            | "clear"
            | "goto"
            | "play"
            | "stop"
            | "urscript"
    )
}

/// Every object's name, by identity.
fn names(doc: &Document) -> HashMap<u64, String> {
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
