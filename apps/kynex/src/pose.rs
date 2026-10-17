//! Robots posed in the viewer: a joint dragged, a tool sent to a point, a
//! program taught and played.
//!
//! The pose is KYNEX's own. Nothing goes to Kreate — the link is read-only —
//! so the viewer shows the session's scene with these robots set as asked,
//! until they are let go and take the session's pose again.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use kreate_math::{DAffine3, DQuat, DVec3};
use kreate_scene::{Changes, DatablockKind, Document, Uid};
use kynex_kin::path::MoveKind;
use kynex_kin::{Goal, Kinematics, Options};
use serde_json::{json, Value};

use crate::cell::Cell;
use crate::collide::{self, Collider};
use crate::program::{self, Action, Program, Step};

/// What the viewer has asked of the robots and not let go.
#[derive(Debug, Default)]
pub struct Posing {
    /// By robot root: joint positions, by joint.
    held: BTreeMap<u64, BTreeMap<u64, f64>>,
    /// By robot root: its programs, the first its main one.
    programs: BTreeMap<u64, Programs>,
    /// The programs changed since the viewers were last told.
    programs_changed: bool,
    /// Where the programs are kept between runs, if anywhere.
    file: Option<PathBuf>,
    /// Programs read from the file whose robot is not in the scene (yet):
    /// kept, and written back, as they were.
    pending: Vec<Value>,
    /// Each robot's programs as last saved, for a robot that has since left
    /// the scene: still written back.
    saved: BTreeMap<u64, Vec<Value>>,
    /// By robot root: the via point of a circular move being taught, until
    /// its end is.
    via: BTreeMap<u64, BTreeMap<u64, f64>>,
    /// Program steps copied, as a program is saved: pasted into this
    /// robot's program or another's of the same joints.
    clipboard: Option<Value>,
    /// How the cell is wired, for a program's export: set before it is
    /// asked for (see `crate::view`).
    pub cell_io: program::CellIo,
}

/// A robot's programs: its main one first, then the routines it calls, and
/// which of them the page is working on.
#[derive(Clone, Debug, Default)]
struct Programs {
    list: Vec<Program>,
    active: usize,
}

impl Programs {
    fn active(&self) -> Option<&Program> {
        self.list.get(self.active)
    }

    /// The program worked on: the main one, made, if there is none yet.
    fn active_mut(&mut self) -> &mut Program {
        if self.list.is_empty() {
            self.list
                .push(Program::named(program::MAIN, None, DVec3::ZERO));
        }
        self.active = self.active.min(self.list.len() - 1);
        &mut self.list[self.active]
    }

    /// `program` with its calls replaced by what they call, and where each
    /// step comes from (see [`Program::flatten`]).
    fn run(&self, program: &Program) -> Result<(Program, Vec<(String, usize)>), String> {
        program.flatten(&self.list)
    }
}

/// What the project file holds besides programs, as saved.
#[derive(Debug, Default)]
pub struct Project {
    pub cell: Vec<Value>,
    pub library: Vec<Value>,
    /// Saved variants of the cell, to compare (see `crate::scenario`).
    pub scenarios: Vec<Value>,
    /// Where the PLC is, if one is wired (see `crate::plc`).
    pub plc: Value,
    /// The site's drawing laid under the scene (see `crate::floor`).
    pub floor: Value,
}

impl Posing {
    /// Programs kept in the project file `file`: read now from `from` —
    /// the same file, or an older one to carry over — and written to `file`
    /// whenever they change. Also returns the rest of what the file held:
    /// the cell's roles and the placed equipment.
    pub fn with_file(from: &Path, file: &Path) -> Result<(Self, Project), String> {
        let (pending, project) = match std::fs::read_to_string(from) {
            Ok(text) => {
                let all: Value =
                    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", from.display()))?;
                let list = |key: &str| all[key].as_array().cloned().unwrap_or_default();
                // Programs from an older layout, and those in scenarios,
                // brought up to this one.
                let version = all["version"].as_u64().unwrap_or(1);
                let mut programs = list("programs");
                for p in &mut programs {
                    program::migrate(p, version);
                }
                let mut scenarios = list("scenarios");
                for s in &mut scenarios {
                    for p in s["programs"].as_array_mut().into_iter().flatten() {
                        program::migrate(p, version);
                    }
                }
                (
                    programs,
                    Project {
                        cell: list("cell"),
                        library: list("library"),
                        scenarios,
                        plc: all["plc"].clone(),
                        floor: all["floor"].clone(),
                    },
                )
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Vec::new(), Project::default()),
            Err(e) => return Err(format!("{}: {e}", from.display())),
        };
        let posing = Self {
            file: Some(file.to_path_buf()),
            pending,
            ..Self::default()
        };
        Ok((posing, project))
    }

    /// Saved from now on to `file` (a project saved under another name).
    pub fn set_file(&mut self, file: &Path) {
        self.file = Some(file.to_path_buf());
    }

    /// Programs as saved, kept in no file: waiting for their robots.
    pub fn from_saved(saved: Vec<Value>) -> Self {
        Self {
            pending: saved,
            ..Self::default()
        }
    }

    /// Puts `saved` in place of every program, and lets every robot go:
    /// a scenario taken back.
    pub fn restore(&mut self, saved: Vec<Value>) {
        self.programs.clear();
        self.held.clear();
        self.saved.clear();
        self.pending = saved;
        self.programs_changed = true;
    }

    /// Programs as saved before (an edit undone): the robots stay posed as
    /// they are, and each keeps the routine it was working on once its
    /// programs are adopted again (see [`Self::set_actives`]).
    pub fn undo_to(&mut self, saved: Vec<Value>) -> BTreeMap<u64, usize> {
        let held = std::mem::take(&mut self.held);
        let actives = self.programs.iter().map(|(r, p)| (*r, p.active)).collect();
        self.restore(saved);
        self.held = held;
        actives
    }

    /// The robots posed by hand, as undoing keeps them: by robot, each
    /// joint's position.
    pub fn held_json(&self) -> Value {
        json!(self
            .held
            .iter()
            .map(|(root, joints)| {
                let joints: BTreeMap<String, f64> =
                    joints.iter().map(|(j, q)| (j.to_string(), *q)).collect();
                (root.to_string(), joints)
            })
            .collect::<BTreeMap<_, _>>())
    }

    /// The robots posed as `v` (a [`Self::held_json`]) has them, the rest as
    /// placed. Returns every robot whose pose this changes.
    pub fn set_held(&mut self, v: &Value) -> Vec<u64> {
        let mut held: BTreeMap<u64, BTreeMap<u64, f64>> = BTreeMap::new();
        for (root, joints) in v.as_object().into_iter().flatten() {
            let Ok(root) = root.parse::<u64>() else {
                continue;
            };
            let joints = joints
                .as_object()
                .into_iter()
                .flatten()
                .filter_map(|(j, q)| Some((j.parse::<u64>().ok()?, q.as_f64()?)))
                .collect();
            held.insert(root, joints);
        }
        let mut roots: Vec<u64> = self.held.keys().chain(held.keys()).copied().collect();
        roots.sort_unstable();
        roots.dedup();
        roots.retain(|r| self.held.get(r) != held.get(r));
        self.held = held;
        roots
    }

    /// Each robot back on the routine it was working on, if it still has it.
    pub fn set_actives(&mut self, actives: &BTreeMap<u64, usize>) {
        for (root, active) in actives {
            if let Some(p) = self.programs.get_mut(root) {
                p.active = (*active).min(p.list.len().saturating_sub(1));
            }
        }
    }

    /// Every robot's main program, as a run takes it: its calls replaced
    /// by what they call.
    pub fn mains(&self) -> Vec<(u64, Result<Program, String>)> {
        self.programs
            .iter()
            .filter_map(|(root, p)| {
                let main = p.list.first()?;
                Some((*root, p.run(main).map(|(flat, _)| flat)))
            })
            .collect()
    }

    /// Robot `root`'s program worked on, as a run takes it: its calls
    /// replaced by what they call, and where each step comes from.
    pub fn runnable(&self, root: u64) -> Result<(Program, Vec<(String, usize)>), String> {
        let p = self.programs.get(&root).ok_or("프로그램이 없습니다")?;
        p.run(p.active().ok_or("프로그램이 없습니다")?)
    }

    /// Every program as saved: those of robots in `doc`, and those kept for
    /// robots that are not.
    pub fn saved_programs(&mut self, doc: &Document) -> Vec<Value> {
        for (root, programs) in &self.programs {
            if let Some(kin) = Kinematics::of(doc, *root) {
                let all = programs
                    .list
                    .iter()
                    .map(|p| p.to_saved(*root, doc, &kin))
                    .collect();
                self.saved.insert(*root, all);
            }
        }
        self.saved
            .retain(|root, _| self.programs.contains_key(root));
        self.saved
            .values()
            .flatten()
            .chain(&self.pending)
            .cloned()
            .collect()
    }

    /// How many programs the file held that are waiting for their robot.
    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    /// Takes on the programs from the file whose robots are in `doc` now.
    pub fn adopt(&mut self, doc: &Document) {
        let mut waiting = Vec::new();
        // Programs made in this run win over the file's.
        let made: HashSet<u64> = self.programs.keys().copied().collect();
        for saved in std::mem::take(&mut self.pending) {
            match Program::from_saved(&saved, doc) {
                Some((root, program)) => {
                    if !made.contains(&root) {
                        // In the file's order: the main one first.
                        self.programs.entry(root).or_default().list.push(program);
                        self.programs_changed = true;
                    }
                }
                None => waiting.push(saved),
            }
        }
        self.pending = waiting;
    }

    /// Writes every program, and the rest of the project, to the project
    /// file, if there is one: whole, to a file beside it first, so a crash
    /// midway leaves the old one.
    pub fn save(&mut self, doc: &Document, rest: Project) -> Result<(), String> {
        if self.file.is_none() {
            return Ok(());
        }
        let programs = self.saved_programs(doc);
        let file = self.file.as_ref().expect("a project file");
        let text = serde_json::to_string_pretty(&json!({
            "version": program::FILE_VERSION,
            "programs": programs,
            "cell": rest.cell,
            "library": rest.library,
            "scenarios": rest.scenarios,
            "plc": rest.plc,
            "floor": rest.floor,
        }))
        .expect("programs serialise");
        let next = file.with_extension("json.new");
        std::fs::write(&next, text)
            .and_then(|_| std::fs::rename(&next, file))
            .map_err(|e| format!("{}: {e}", file.display()))
    }

    /// `doc` with the held robots posed, or `None` when none are and `doc`
    /// is the scene as it is.
    pub fn shown(&mut self, doc: &Document) -> Option<Document> {
        if self.held.is_empty() {
            return None;
        }
        let mut shown = doc.clone();
        // A robot no longer in the scene is let go of.
        self.held
            .retain(|root, positions| match Kinematics::of(&shown, *root) {
                Some(kin) => {
                    kin.apply(&mut shown, &kin.with(positions));
                    true
                }
                None => false,
            });
        Some(shown)
    }

    /// What the `pose` event says: the robots held here, the programs
    /// running, a list of them (see `crate::run`), and the simulation's clock.
    pub fn to_json(&self, running: Value, clock: Value) -> String {
        json!({
            "held": self.held.keys().map(u64::to_string).collect::<Vec<_>>(),
            "running": running,
            "clock": clock,
        })
        .to_string()
    }

    /// Robot `root`'s program worked on, if it has one.
    #[cfg(test)]
    pub fn program(&self, root: u64) -> Option<&Program> {
        self.programs.get(&root)?.active()
    }

    /// Where robot `root` stands as shown: held, or as the scene has it.
    pub fn positions(&self, kin: &Kinematics, root: u64) -> Vec<f64> {
        kin.with(&self.held.get(&root).cloned().unwrap_or_default())
    }

    /// Holds robot `root` at `q`, one position per [`Kinematics::vars`].
    pub fn hold_at(&mut self, kin: &Kinematics, root: u64, q: &[f64]) {
        self.held.insert(
            root,
            kin.vars
                .iter()
                .map(|v| v.uid)
                .zip(q.iter().copied())
                .collect(),
        );
    }

    /// Whether the programs changed since this was last asked.
    pub fn take_programs_changed(&mut self) -> bool {
        std::mem::take(&mut self.programs_changed)
    }

    /// Says the programs need telling again: the scene they are timed in
    /// changed.
    pub fn programs_changed(&mut self) {
        self.programs_changed = true;
    }

    /// What the `programs` event says: every robot's program, timed in
    /// `doc` and checked for collisions with the rest of it — the doors of
    /// machines the program has opened by each move out of the way (their
    /// doors as `cell` finds them).
    pub fn programs_json(&self, doc: &Document, collider: &mut Collider, cell: &Cell) -> String {
        let names = program::names(doc);
        let mut out = serde_json::Map::new();
        for (root, programs) in &self.programs {
            let Some(program) = programs.active() else {
                continue;
            };
            let Some(kin) = Kinematics::of(doc, *root) else {
                continue;
            };
            let plan = program.plan(&kin);
            let open: Vec<HashSet<u64>> = program
                .open_during_moves()
                .into_iter()
                .map(|machines| {
                    machines
                        .into_iter()
                        .filter_map(|m| cell.door_of(doc, m))
                        .collect()
                })
                .collect();
            let hits: Vec<Vec<String>> = match &plan {
                Some(plan) => {
                    let hits = collider.along(doc, *root, plan, &open);
                    // Near where it does not touch: but where a gripper works.
                    let work = program.work_places(&kin, plan);
                    let near =
                        collider.nearest_along(doc, *root, plan, &open, &work, collide::CLEARANCE);
                    hits.into_iter()
                        .zip(near)
                        .map(|(hit, near)| match (hit, near) {
                            (Some((t, c)), _) => vec![format!(
                                "충돌: {} ↔ {} ({:.2} s)",
                                collide::label(doc, c.part),
                                collide::label(doc, c.other),
                                t
                            )],
                            (None, Some(n)) => vec![format!(
                                "근접: {} ↔ {} {:.0} mm ({:.2} s)",
                                collide::label(doc, n.part),
                                collide::label(doc, n.other),
                                n.distance * 1000.0,
                                n.t
                            )],
                            (None, None) => Vec::new(),
                        })
                        .collect()
                }
                None => Vec::new(),
            };
            let mut json = program.to_json(&kin, plan.as_ref(), &hits, &names);
            json["routines"] = json!(programs.list.iter().map(|p| &p.name).collect::<Vec<_>>());
            json["active"] = json!(programs.active);
            out.insert(root.to_string(), json);
        }
        Value::Object(out).to_string()
    }

    /// Takes one request from the page, `doc` being the session's scene.
    /// Returns the answer, and what the viewers need to look at again.
    pub fn handle(&mut self, doc: &Document, body: &str) -> (String, Changes) {
        let mut touched = Changes::default();
        let answer = match self.take(doc, body, &mut touched) {
            Ok(answer) => answer,
            Err(e) => json!({ "ok": false, "error": e }),
        };
        (answer.to_string(), touched)
    }

    fn take(&mut self, doc: &Document, body: &str, touched: &mut Changes) -> Result<Value, String> {
        let req: Value = serde_json::from_str(body).map_err(|e| format!("not JSON: {e}"))?;
        let op = req["op"].as_str().ok_or("no op")?;
        if op == "release" && req["robot"].is_null() {
            for root in std::mem::take(&mut self.held).into_keys() {
                mark(doc, root, touched);
            }
            return Ok(json!({ "ok": true }));
        }
        let root = uid(&req["robot"]).ok_or("no robot")?;
        // From the pose shown, not the session's: a second drag goes on from
        // where the first left the arm.
        let kin = Kinematics::of(doc, root).ok_or("no such robot")?;
        let held = self.held.get(&root).cloned().unwrap_or_default();
        let now = kin.with(&held);
        if program::is_op(op) {
            return self.program_op(op, &req, doc, &kin, root, &now, touched);
        }
        match op {
            "release" => {
                self.held.remove(&root);
                mark(doc, root, touched);
                Ok(json!({ "ok": true }))
            }
            "jog" => {
                let joint = uid(&req["joint"]).ok_or("no joint")?;
                let position = req["position"].as_f64().ok_or("no position")?;
                let var = kin
                    .vars
                    .iter()
                    .find(|v| v.uid == joint)
                    .ok_or("not a joint that moves on its own")?;
                let position = var.clamp(position);
                self.hold(&kin, root, &now).insert(joint, position);
                mark(doc, root, touched);
                Ok(json!({ "ok": true, "position": position }))
            }
            "ik" => {
                let tool_uid = uid(&req["tool"]).ok_or("no tool")?;
                let offset = vec3(&req["offset"]).unwrap_or(DVec3::ZERO);
                let tool = kin
                    .tool(tool_uid, DAffine3::from_translation(offset))
                    .ok_or("the tool is not part of the robot")?;
                let goal = Goal {
                    position: vec3(&req["position"]).ok_or("no position")?,
                    rotation: quat(&req["rotation"]),
                };
                let opts = Options {
                    restarts: req["restarts"].as_u64().unwrap_or(0).min(64) as usize,
                    ..Options::default()
                };
                let found = kin.solve(&tool, &goal, &now, &opts);
                let positions = self.hold(&kin, root, &now);
                for (var, q) in kin.vars.iter().zip(&found.positions) {
                    positions.insert(var.uid, *q);
                }
                mark(doc, root, touched);
                Ok(json!({
                    "ok": true,
                    "reached": found.reached,
                    "position_error": found.position_error,
                    "rotation_error": found.rotation_error,
                    "iterations": found.iterations,
                }))
            }
            // Where the arm's flange reaches: a section in its base frame,
            // to be swept round the first joint (see `kynex_kin::reach`).
            "reach" => {
                let flange = kynex_model::robots(doc)
                    .into_iter()
                    .find(|r| r.root == root)
                    .and_then(|r| r.tool)
                    .and_then(|t| kin.tool(t, DAffine3::IDENTITY))
                    .ok_or("no flange")?;
                let names = program::names(doc);
                let base = names
                    .iter()
                    .find(|(u, n)| n.eq_ignore_ascii_case("base") && kin.contains(**u))
                    .map_or(root, |(u, _)| *u);
                let reach = kynex_kin::reach::reach(&kin, &flange, base)
                    .ok_or("이 로봇은 도달 범위를 그릴 수 없습니다")?;
                let at = kin.world_of(&now, base).ok_or("no base frame")?;
                Ok(json!({
                    "ok": true,
                    "section": reach.section,
                    "turn": [reach.turn.0, reach.turn.1],
                    "farthest": reach.farthest,
                    "base": kreate_math::DMat4::from(at).to_cols_array(),
                }))
            }
            other => Err(format!("unknown op {other:?}")),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn program_op(
        &mut self,
        op: &str,
        req: &Value,
        doc: &Document,
        kin: &Kinematics,
        root: u64,
        now: &[f64],
        touched: &mut Changes,
    ) -> Result<Value, String> {
        let here: BTreeMap<u64, f64> = kin
            .vars
            .iter()
            .map(|v| v.uid)
            .zip(now.iter().copied())
            .collect();
        let programs = self.programs.entry(root).or_default();
        programs.active_mut();
        // The robot's programs: a new one, another worked on, renamed,
        // removed.
        let given = req["name"]
            .as_str()
            .map(str::trim)
            .filter(|n| !n.is_empty());
        match op {
            "routine_new" => {
                let n = programs.list.len();
                let name = given
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("서브루틴 {n}"));
                if programs.list.iter().any(|p| p.name == name) {
                    return Err(format!("{name}: 이미 있는 이름입니다"));
                }
                let like = programs.list[programs.active].clone();
                let name: String = name.chars().take(32).collect();
                programs
                    .list
                    .push(Program::named(&name, like.tool, like.offset));
                programs.active = programs.list.len() - 1;
                self.programs_changed = true;
                return Ok(json!({ "ok": true, "active": programs.active }));
            }
            "routine_select" => {
                let i = req["index"].as_u64().ok_or("no index")? as usize;
                if i >= programs.list.len() {
                    return Err(format!("no program {i}"));
                }
                programs.active = i;
                self.programs_changed = true;
                return Ok(json!({ "ok": true }));
            }
            "routine_rename" => {
                let name: String = given.ok_or("no name")?.chars().take(32).collect();
                let old = programs.list[programs.active].name.clone();
                if name == old {
                    return Ok(json!({ "ok": true }));
                }
                if programs.list.iter().any(|p| p.name == name) {
                    return Err(format!("{name}: 이미 있는 이름입니다"));
                }
                // Its calls follow it.
                for p in &mut programs.list {
                    for step in &mut p.steps {
                        if let Step::Do(program::Action::Call { routine, .. }) = step {
                            if *routine == old {
                                routine.clone_from(&name);
                            }
                        }
                    }
                }
                programs.list[programs.active].name = name;
                self.programs_changed = true;
                return Ok(json!({ "ok": true }));
            }
            "routine_delete" => {
                if programs.active == 0 {
                    return Err("메인 프로그램은 지울 수 없습니다".into());
                }
                programs.list.remove(programs.active);
                programs.active = 0;
                self.programs_changed = true;
                return Ok(json!({ "ok": true }));
            }
            _ => {}
        }
        let program = programs.active_mut();
        // The tool its lines are drawn with, as the page has it now.
        if let Some(tool) = uid(&req["tool"]) {
            if !kin.contains(tool) {
                return Err("the tool is not part of the robot".into());
            }
            let offset = vec3(&req["offset"]).unwrap_or(DVec3::ZERO);
            if program.tool != Some(tool) || program.offset != offset {
                program.tool = Some(tool);
                program.offset = offset;
                self.programs_changed = true;
            }
        }
        // An edge of a part, picked near a point on it: shown, or followed
        // with the tool (see `crate::edges`).
        if op == "edge" || op == "edge_path" {
            let object = uid(&req["object"]).ok_or("no object")?;
            let at = vec3(&req["point"]).ok_or("no point")?;
            let chain = crate::edges::picked(doc, object, at)?;
            if op == "edge" {
                let points: Vec<[f64; 3]> = chain.points.iter().map(|p| p.to_array()).collect();
                return Ok(json!({
                    "ok": true,
                    "points": points,
                    "closed": chain.closed,
                    "length": chain.length(),
                }));
            }
            let program = programs.active_mut();
            let tool = program.tool(kin).ok_or("툴이 정해지지 않았습니다")?;
            // Deburring's usual pace, and room to come in and go out.
            let speed = req["speed"].as_f64().unwrap_or(0.05).clamp(0.001, 1.0);
            let clear = req["approach"].as_f64().unwrap_or(0.05).clamp(0.0, 0.5);
            let along = crate::edges::poses(&chain, crate::edges::TOLERANCE);
            let (first, last) = (along[0], along[along.len() - 1]);
            let back = |(p, turn): (DVec3, DQuat)| (p - turn * DVec3::Z * clear, turn);
            // Each from the one before, so the arm keeps its posture; into
            // the first by any way that reaches it.
            let mut seed = now.to_vec();
            let mut missed = 0;
            let mut solved = Vec::new();
            for (k, (p, turn)) in std::iter::once(back(first))
                .chain(along.iter().copied())
                .chain(std::iter::once(back(last)))
                .enumerate()
            {
                let opts = Options {
                    restarts: if k == 0 { 30 } else { 0 },
                    ..Options::default()
                };
                let found = kin.solve(
                    &tool,
                    &Goal {
                        position: p,
                        rotation: Some(turn),
                    },
                    &seed,
                    &opts,
                );
                missed += usize::from(!found.reached);
                seed = found.positions.clone();
                solved.push(found.positions);
            }
            let mut after = req["after"].as_u64().map(|i| i as usize);
            let n = solved.len();
            for (k, q) in solved.iter().enumerate() {
                let (kind, pace) = match k {
                    0 => (MoveKind::Joint, program::JOINT_SPEED),
                    _ if k == n - 1 => (MoveKind::Linear, program::LINEAR_SPEED),
                    _ => (MoveKind::Linear, speed),
                };
                let positions = kin
                    .vars
                    .iter()
                    .map(|v| v.uid)
                    .zip(q.iter().copied())
                    .collect();
                program.teach(kind, positions, None, after);
                let i = after.map_or(program.steps.len() - 1, |a| {
                    (a + 1).min(program.steps.len() - 1)
                });
                if let Some(point) = program.point_mut(i) {
                    point.speed = pace;
                    // Into the edge, along it and off it, the tool works.
                    point.work = k > 0;
                }
                after = Some(i);
            }
            self.programs_changed = true;
            return Ok(json!({
                "ok": true,
                "added": n,
                "missed": missed,
                "length": chain.length(),
            }));
        }
        // Exported as it runs: its calls replaced by what they call, for
        // the controller of the vendor asked for (Universal Robots unless
        // another is).
        if op == "urscript" || op == "export" {
            let writer: &dyn kynex_kin::post::Writer = match req["vendor"].as_str() {
                None | Some("ur") => &kynex_kin::urscript::Ur,
                Some("abb") => &kynex_kin::rapid::Abb,
                Some("doosan") => &kynex_kin::drl::Doosan,
                Some("fanuc") => &kynex_kin::fanuc::Fanuc,
                Some(other) => return Err(format!("no writer for {other}")),
            };
            let program = programs.active().expect("a program worked on");
            let (flat, _) = programs.run(program)?;
            let (script, file) = flat.export(root, doc, kin, false, &self.cell_io, writer)?;
            return Ok(json!({ "ok": true, "script": script, "file": file }));
        }
        let program = programs.active_mut();
        let count = program.steps.len();
        let index = || -> Result<usize, String> {
            let i = req["index"].as_u64().ok_or("no index")? as usize;
            (i < count)
                .then_some(i)
                .ok_or_else(|| format!("no step {i}"))
        };
        let not_a_move = || "이동 단계가 아닙니다".to_string();
        match op {
            "program" => return Ok(json!({ "ok": true })),
            "teach" => {
                let kind = program::kind_of(req["kind"].as_str().unwrap_or("joint"))
                    .ok_or("kind is joint, linear or circular")?;
                let after = req["after"].as_u64().map(|i| i as usize);
                // A circular move is taught twice: its via point, then its end.
                let via = match kind {
                    MoveKind::Circular => match self.via.remove(&root) {
                        Some(via) => Some(via),
                        None => {
                            self.via.insert(root, here);
                            return Ok(json!({ "ok": true, "via": true }));
                        }
                    },
                    _ => None,
                };
                program.teach(kind, here, via, after);
            }
            "add" => {
                let action = program::action_of(&req["step"])?;
                let after = req["after"].as_u64().map(|i| i as usize);
                // A block comes with its end, to put steps into.
                let block = matches!(action, Action::If(_) | Action::While(_));
                program.insert(Step::Do(action), after);
                if block {
                    let at = after.map_or(program.steps.len() - 1, |i| {
                        (i + 1).min(program.steps.len() - 1)
                    });
                    program.insert(Step::Do(Action::End), Some(at));
                }
            }
            "retouch" if req["via"] == true => {
                let point = program.point_mut(index()?).ok_or_else(not_a_move)?;
                if point.kind != MoveKind::Circular {
                    return Err("원호 이동이 아닙니다".into());
                }
                point.via = Some(here);
            }
            "retouch" => {
                let point = program.point_mut(index()?).ok_or_else(not_a_move)?;
                point.positions = here;
                // Taught again: in its equipment as that stands now.
                if let Some(frame) = &mut point.frame {
                    if let Some(id) = doc.object_by_uid(Uid::from_raw(frame.object)) {
                        frame.taught_in = doc.world_transform(id);
                    }
                }
            }
            "edit" if req["step"].is_object() => {
                let i = index()?;
                let Step::Do(action) = &mut program.steps[i] else {
                    return Err("동작 단계가 아닙니다".into());
                };
                *action = program::action_of(&req["step"])?;
            }
            // Several moves at once, `index` to `to`: their kind, speed and
            // blend; the other steps among them as they are.
            "edit" if req["to"].is_u64() => {
                let (from, to) = (index()?, range_end(req, count)?);
                if req["kind"] == "circular" {
                    return Err("원호 이동은 한 단계씩 바꾸세요".into());
                }
                let mut each = req.clone();
                for key in ["name", "frame", "pallet"] {
                    each.as_object_mut().map(|o| o.remove(key));
                }
                each.as_object_mut().map(|o| o.remove("to"));
                // Joint moves' speed is a fraction, the others' m/s: one
                // kind at a time, if `only` says.
                let only = match req["only"].as_str() {
                    Some(k) => Some(program::kind_of(k).ok_or("only is joint or linear")?),
                    None => None,
                };
                each.as_object_mut().map(|o| o.remove("only"));
                let moves: Vec<usize> = (from..=to)
                    .filter(|i| match &program.steps[*i] {
                        Step::Move(p) => only.is_none_or(|k| {
                            k == p.kind || (k == MoveKind::Linear && p.kind == MoveKind::Circular)
                        }),
                        Step::Do(_) => false,
                    })
                    .collect();
                if moves.is_empty() {
                    return Err(not_a_move());
                }
                for i in &moves {
                    let point = program.point_mut(*i).expect("a move");
                    edit_point(point, &each, doc)?;
                }
                self.programs_changed = true;
                return Ok(json!({ "ok": true, "edited": moves.len() }));
            }
            // Steps kept to paste, here or into another robot's program.
            "steps_copy" => {
                let (from, to) = (index()?, range_end(req, count)?);
                let mut slice = program.clone();
                slice.steps = program.steps[from..=to].to_vec();
                self.clipboard = Some(slice.to_saved(root, doc, kin));
                return Ok(json!({ "ok": true, "copied": to - from + 1 }));
            }
            "steps_paste" => {
                let mut saved = self.clipboard.clone().ok_or("복사한 단계가 없습니다")?;
                // For this robot: its joints found by name.
                saved["robot"] = json!(root.to_string());
                saved["robot_name"] = Value::Null;
                // Another robot of as many joints: its joints in the same
                // order (names in one scene differ, `shoulder.001`). Both
                // lists go by identity, as a robot's parts are numbered.
                let mut mine: Vec<(u64, &str)> =
                    kin.vars.iter().map(|v| (v.uid, v.name.as_str())).collect();
                mine.sort_by_key(|(u, _)| *u);
                for step in saved["steps"].as_array_mut().into_iter().flatten() {
                    for key in ["joints", "via"] {
                        let Some(list) = step[key].as_array_mut() else {
                            continue;
                        };
                        if list.len() != mine.len() {
                            continue;
                        }
                        for (j, (uid, name)) in list.iter_mut().zip(&mine) {
                            j["uid"] = json!(uid.to_string());
                            j["name"] = json!(name);
                        }
                    }
                }
                let (_, pasted) =
                    Program::from_saved(&saved, doc).ok_or("복사한 단계를 붙여 넣지 못했습니다")?;
                let whole = pasted.steps.iter().all(|s| match s {
                    Step::Move(p) => {
                        p.positions.len() == kin.vars.len()
                            && p.via.as_ref().is_none_or(|v| v.len() == kin.vars.len())
                    }
                    Step::Do(_) => true,
                });
                if !whole {
                    return Err("관절이 다른 로봇에서 복사한 지점이라 붙여 넣을 수 없습니다".into());
                }
                let n = pasted.steps.len();
                let mut at = req["after"].as_u64().map_or(program.steps.len(), |i| {
                    (i as usize + 1).min(program.steps.len())
                });
                for step in pasted.steps {
                    program.steps.insert(at, step);
                    at += 1;
                }
                self.programs_changed = true;
                return Ok(json!({ "ok": true, "pasted": n }));
            }
            // Steps `index` to `to` put before step `before` (as numbered
            // now): dragged in the list.
            "steps_move" => {
                let (from, to) = (index()?, range_end(req, count)?);
                let before = req["before"].as_u64().ok_or("no before")? as usize;
                if !program.move_range(from, to, before) {
                    return Err("그 자리로는 옮길 수 없습니다".into());
                }
            }
            "edit" => {
                let point = program.point_mut(index()?).ok_or_else(not_a_move)?;
                edit_point(point, req, doc)?;
            }
            "delete" => {
                program.steps.remove(index()?);
            }
            "close" => {
                program.close = req["close"].as_bool().ok_or("close is true or false")?;
            }
            "clear" => program.clear(),
            "goto" => {
                let point = program.point_mut(index()?).ok_or_else(not_a_move)?;
                let q = match (&point.via, req["via"] == true) {
                    (Some(via), true) => kin.with(via),
                    _ => kin.with(&point.positions),
                };
                self.held
                    .insert(root, kin.vars.iter().map(|v| v.uid).zip(q).collect());
                mark_joints(kin.joints(), touched);
                return Ok(json!({ "ok": true }));
            }
            "shift" => {
                let at = |key: &str| {
                    req[key]
                        .as_u64()
                        .map(|i| i as usize)
                        .ok_or(format!("no {key}"))
                };
                let up = req["up"].as_bool().ok_or("up is true or false")?;
                if !program.shift(at("from")?, at("to")?, up) {
                    return Err("더 옮길 수 없습니다".into());
                }
            }
            "copy" => {
                let at = |key: &str| {
                    req[key]
                        .as_u64()
                        .map(|i| i as usize)
                        .ok_or(format!("no {key}"))
                };
                let machine = uid(&req["machine"]).ok_or("no machine")?;
                let added = program.copy_for(at("from")?, at("to")?, machine, doc, kin)?;
                self.programs_changed = true;
                return Ok(json!({ "ok": true, "added": added }));
            }
            _ => unreachable!("program::is_op lists {op}"),
        }
        self.programs_changed = true;
        Ok(json!({ "ok": true }))
    }

    /// The robot's held positions, every variable of it, starting from `now`.
    fn hold(&mut self, kin: &Kinematics, root: u64, now: &[f64]) -> &mut BTreeMap<u64, f64> {
        self.held
            .entry(root)
            .or_insert_with(|| kin.vars.iter().zip(now).map(|(v, q)| (v.uid, *q)).collect())
    }
}

/// A move's kind, speed, name, blend and the equipment it is taught in,
/// as request `req` gives them.
fn edit_point(point: &mut program::Point, req: &Value, doc: &Document) -> Result<(), String> {
    if let Some(kind) = req["kind"].as_str() {
        let kind = program::kind_of(kind).ok_or("kind is joint or linear")?;
        if kind != point.kind {
            if kind == MoveKind::Circular && point.via.is_none() {
                return Err("원호 이동은 + MoveC로 경유점과 함께 티칭하세요".into());
            }
            if point.kind == MoveKind::Joint || kind == MoveKind::Joint {
                point.speed = match kind {
                    MoveKind::Joint => program::JOINT_SPEED,
                    _ => program::LINEAR_SPEED,
                };
            }
            point.kind = kind;
            if kind != MoveKind::Circular {
                point.via = None;
            }
        }
    }
    if let Some(speed) = req["speed"].as_f64() {
        point.speed = match point.kind {
            MoveKind::Joint => speed.clamp(0.01, 1.0),
            MoveKind::Linear | MoveKind::Circular => speed.clamp(0.001, 5.0),
        };
    }
    if let Some(name) = req["name"].as_str() {
        let name = name.trim();
        if !name.is_empty() {
            point.name = name.chars().take(32).collect();
        }
    }
    if let Some(blend) = req["blend"].as_f64() {
        point.blend = blend.clamp(0.0, program::MAX_BLEND);
    }
    // The equipment it is taught in, or none (`null`, ""): where
    // that stands now is where the point was taught in it.
    let frame_of = |object: u64| {
        let id = doc.object_by_uid(Uid::from_raw(object))?;
        Some(program::Frame {
            object,
            taught_in: doc.world_transform(id),
        })
    };
    if let Some(frame) = req.get("frame") {
        point.frame = match uid(frame) {
            Some(object) => Some(frame_of(object).ok_or("no such equipment")?),
            None => None,
        };
        if point.frame.is_none_or(|f| Some(f.object) != point.pallet) {
            point.pallet = None;
        }
    }
    // The pallet it stands on, or none: a pallet is its frame.
    if let Some(pallet) = req.get("pallet") {
        point.pallet = uid(pallet);
        point.frame = match point.pallet {
            Some(object) => Some(frame_of(object).ok_or("no such pallet")?),
            None => None,
        };
    }
    Ok(())
}

/// The last step of a range from `index` to `to` in `req` (`index` alone
/// if no `to`): not before its first, within the `count` steps there are.
fn range_end(req: &Value, count: usize) -> Result<usize, String> {
    let from = req["index"].as_u64().ok_or("no index")? as usize;
    let to = req["to"].as_u64().map_or(from, |t| t as usize);
    if to < from || to >= count {
        return Err(format!("no steps {from}–{to}"));
    }
    Ok(to)
}

/// Every joint of the robot, for the viewers to look at again: what a
/// joint carries follows from it.
/// Marks robot `root`'s joints for the viewers to look at again: it moved.
pub fn mark(doc: &Document, root: u64, touched: &mut Changes) {
    if let Some(kin) = Kinematics::of(doc, root) {
        mark_joints(kin.joints(), touched);
    }
}

fn mark_joints(joints: impl Iterator<Item = u64>, touched: &mut Changes) {
    touched
        .changed
        .extend(joints.map(|j| (DatablockKind::Object, Uid::from_raw(j))));
}

/// An identity, sent as text (see kynex-view's scene module).
fn uid(v: &Value) -> Option<u64> {
    v.as_str()?.parse().ok()
}

fn vec3(v: &Value) -> Option<DVec3> {
    let a = v.as_array()?;
    match a.as_slice() {
        [x, y, z] => Some(DVec3::new(x.as_f64()?, y.as_f64()?, z.as_f64()?)),
        _ => None,
    }
}

fn quat(v: &Value) -> Option<DQuat> {
    let a = v.as_array()?;
    match a.as_slice() {
        [x, y, z, w] => {
            let q = DQuat::from_xyzw(x.as_f64()?, y.as_f64()?, z.as_f64()?, w.as_f64()?);
            (q.length() > 1e-9).then(|| q.normalize())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kreate_math::DTransform;
    use kreate_scene::robot::RobotPart;
    use kreate_scene::robot::{Joint, JointKind};
    use kreate_scene::{Object, ObjectData};

    /// A base, a shoulder turning about Z 1 m up, and an arm reaching 1 m
    /// along X from it.
    fn arm() -> (Document, u64, u64, u64) {
        arm_in(Document::new())
    }

    /// The arm, added to what `doc` already holds.
    fn arm_in(mut doc: Document) -> (Document, u64, u64, u64) {
        let mut base = Object::new("base", ObjectData::Empty);
        base.robot = Some(RobotPart::Link { inertial: None });
        let base = doc.add_object(base);
        let rest = DTransform::from_translation(DVec3::new(0.0, 0.0, 1.0));
        let mut joint = Object::new("shoulder", ObjectData::Empty);
        joint.transform = rest;
        joint.robot = Some(RobotPart::Joint(Joint {
            kind: JointKind::Revolute,
            rest,
            lower: -1.0,
            upper: 1.0,
            ..Joint::default()
        }));
        let joint = doc.add_object(joint);
        doc.set_parent(joint, Some(base)).unwrap();
        // Placed in the world: parenting keeps it where it is.
        let mut tip = Object::new("tool0", ObjectData::Empty);
        tip.transform.translation = DVec3::new(1.0, 0.0, 1.0);
        tip.robot = Some(RobotPart::Link { inertial: None });
        let tip = doc.add_object(tip);
        doc.set_parent(tip, Some(joint)).unwrap();
        let raw = |id| doc.object_uid(id).unwrap().raw();
        let (b, j, t) = (raw(base), raw(joint), raw(tip));
        (doc, b, j, t)
    }

    fn ask(p: &mut Posing, doc: &Document, req: Value) -> (Value, Changes) {
        let (answer, touched) = p.handle(doc, &req.to_string());
        (serde_json::from_str(&answer).unwrap(), touched)
    }

    #[test]
    fn a_jog_poses_the_robot_shown_and_not_the_session() {
        let (doc, base, joint, tip) = arm();
        let mut p = Posing::default();
        assert!(p.shown(&doc).is_none(), "nothing held, nothing to copy");
        let (answer, touched) = ask(
            &mut p,
            &doc,
            json!({"op": "jog", "robot": base.to_string(), "joint": joint.to_string(), "position": 5.0}),
        );
        assert_eq!(answer["position"], 1.0, "held to its limit");
        assert!(touched
            .changed
            .contains(&(DatablockKind::Object, Uid::from_raw(joint))));
        let shown = p.shown(&doc).unwrap();
        let at = |d: &Document| {
            d.world_transform(d.object_by_uid(Uid::from_raw(tip)).unwrap())
                .translation
        };
        assert!((at(&shown) - DVec3::new(1f64.cos(), 1f64.sin(), 1.0)).length() < 1e-9);
        assert!(
            (at(&doc) - DVec3::new(1.0, 0.0, 1.0)).length() < 1e-9,
            "the session's scene is as it was"
        );
        assert_eq!(
            p.to_json(Value::Null, Value::Null),
            json!({"held": [base.to_string()], "running": null, "clock": null}).to_string()
        );

        ask(
            &mut p,
            &doc,
            json!({"op": "release", "robot": base.to_string()}),
        );
        assert!(p.shown(&doc).is_none());
    }

    #[test]
    fn ik_sends_the_tool_to_a_point_it_can_reach() {
        let (doc, base, _, tip) = arm();
        let mut p = Posing::default();
        let goal = DVec3::new(0.6f64.cos(), 0.6f64.sin(), 1.0);
        let (answer, _) = ask(
            &mut p,
            &doc,
            json!({"op": "ik", "robot": base.to_string(), "tool": tip.to_string(),
                   "position": [goal.x, goal.y, goal.z], "rotation": null}),
        );
        assert_eq!(answer["reached"], true, "{answer}");
        let shown = p.shown(&doc).unwrap();
        let at = shown
            .world_transform(shown.object_by_uid(Uid::from_raw(tip)).unwrap())
            .translation;
        assert!((at - goal).length() < 1e-4);

        // Past the limit: as near as it goes, and said so.
        let (answer, _) = ask(
            &mut p,
            &doc,
            json!({"op": "ik", "robot": base.to_string(), "tool": tip.to_string(),
                   "position": [-1.0, 0.0, 1.0]}),
        );
        assert_eq!(answer["reached"], false);
        assert!(answer["position_error"].as_f64().unwrap() > 0.5);
    }

    /// Points taught at three angles; two copied and pasted after the
    /// third, and into a second arm of the same joints; one dragged to
    /// the end; the speed of several set at once.
    #[test]
    fn steps_are_copied_pasted_moved_and_edited_together() {
        let (doc, base, joint, _) = arm();
        let (doc, base2, joint2, _) = arm_in(doc);
        let mut p = Posing::default();
        let robot = base.to_string();
        for angle in [0.1, 0.2, 0.3] {
            ask(
                &mut p,
                &doc,
                json!({"op": "jog", "robot": robot, "joint": joint.to_string(), "position": angle}),
            );
            let (a, _) = ask(&mut p, &doc, json!({"op": "teach", "robot": robot}));
            assert_eq!(a["ok"], true, "{a}");
        }
        let angles = |p: &Posing, root: u64, joint: u64| -> Vec<f64> {
            p.program(root)
                .unwrap()
                .moves()
                .map(|(_, pt)| (pt.positions[&joint] * 10.0).round() / 10.0)
                .collect()
        };
        let (c, _) = ask(
            &mut p,
            &doc,
            json!({"op": "steps_copy", "robot": robot, "index": 0, "to": 1}),
        );
        assert_eq!(c["copied"], 2, "{c}");
        let (v, _) = ask(
            &mut p,
            &doc,
            json!({"op": "steps_paste", "robot": robot, "after": 2}),
        );
        assert_eq!(v["pasted"], 2, "{v}");
        assert_eq!(angles(&p, base, joint), vec![0.1, 0.2, 0.3, 0.1, 0.2]);

        // Into the other arm, its joint found by name.
        let (v, _) = ask(
            &mut p,
            &doc,
            json!({"op": "steps_paste", "robot": base2.to_string()}),
        );
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(angles(&p, base2, joint2), vec![0.1, 0.2]);

        // The first dragged to the end; not into itself.
        let (m, _) = ask(
            &mut p,
            &doc,
            json!({"op": "steps_move", "robot": robot, "index": 0, "to": 0, "before": 5}),
        );
        assert_eq!(m["ok"], true, "{m}");
        assert_eq!(angles(&p, base, joint), vec![0.2, 0.3, 0.1, 0.2, 0.1]);
        let (bad, _) = ask(
            &mut p,
            &doc,
            json!({"op": "steps_move", "robot": robot, "index": 1, "to": 2, "before": 2}),
        );
        assert_eq!(bad["ok"], false);

        let (e, _) = ask(
            &mut p,
            &doc,
            json!({"op": "edit", "robot": robot, "index": 1, "to": 3, "speed": 0.25}),
        );
        assert_eq!(e["edited"], 3, "{e}");
        let speeds: Vec<f64> = p
            .program(base)
            .unwrap()
            .moves()
            .map(|(_, pt)| pt.speed)
            .collect();
        assert_eq!(speeds[1..4], [0.25, 0.25, 0.25]);
        assert_ne!(speeds[0], 0.25);
        let (circ, _) = ask(
            &mut p,
            &doc,
            json!({"op": "edit", "robot": robot, "index": 0, "to": 1, "kind": "circular"}),
        );
        assert_eq!(circ["ok"], false);
    }

    #[test]
    fn a_bad_request_is_answered_not_dropped() {
        let (doc, base, _, _) = arm();
        let mut p = Posing::default();
        for req in [
            json!({"op": "jog"}),
            json!({"op": "fly", "robot": base.to_string()}),
            json!({"op": "jog", "robot": "999", "joint": "1", "position": 0.0}),
            json!({"op": "ik", "robot": base.to_string(), "tool": "999", "position": [0, 0, 0]}),
        ] {
            let (answer, touched) = ask(&mut p, &doc, req);
            assert_eq!(answer["ok"], false, "{answer}");
            assert!(touched.changed.is_empty());
        }
        let (answer, _) = p.handle(&doc, "not json");
        assert!(answer.contains("not JSON"));
    }

    #[test]
    fn a_program_is_taught_and_timed() {
        let (doc, base, joint, tip) = arm();
        let (b, j, t) = (base.to_string(), joint.to_string(), tip.to_string());
        let mut p = Posing::default();
        let teach = |p: &mut Posing, at: f64, kind: &str| {
            ask(
                p,
                &doc,
                json!({"op": "jog", "robot": b, "joint": j, "position": at}),
            );
            let (answer, _) = ask(
                p,
                &doc,
                json!({"op": "teach", "robot": b, "tool": t, "offset": [0, 0, 0], "kind": kind}),
            );
            assert_eq!(answer["ok"], true, "{answer}");
        };
        teach(&mut p, -0.5, "joint");
        teach(&mut p, 0.5, "joint");
        teach(&mut p, 0.2, "linear");
        assert!(p.take_programs_changed());
        assert!(!p.take_programs_changed(), "told once");

        let programs: Value = serde_json::from_str(&p.programs_json(
            &doc,
            &mut Collider::default(),
            &Cell::default(),
        ))
        .unwrap();
        let program = &programs[&b];
        let points = program["steps"].as_array().unwrap();
        assert_eq!(points.len(), 3);
        assert_eq!(points[0]["name"], "P1");
        assert!(
            points[0]["move"].is_null(),
            "the first point is where it starts"
        );
        // A turn of 1 rad at half of the joint's default 1 rad/s.
        let first = points[1]["move"]["duration"].as_f64().unwrap();
        assert!(first > 2.0 && first < 2.5, "{first}");
        // The line from 0.5 to 0.2 rad cuts inside the tool's circle.
        assert!(points[2]["move"]["problems"][0]
            .as_str()
            .unwrap()
            .contains("도달 불가"));
        assert!(!program["trace"].as_array().unwrap().is_empty());
        let cycle = program["cycle"].as_f64().unwrap();

        // Edits: the third point becomes a joint move, the second goes.
        ask(
            &mut p,
            &doc,
            json!({"op": "edit", "robot": b, "index": 2, "kind": "joint", "speed": 1.0}),
        );
        ask(
            &mut p,
            &doc,
            json!({"op": "delete", "robot": b, "index": 1}),
        );
        ask(
            &mut p,
            &doc,
            json!({"op": "close", "robot": b, "close": true}),
        );
        let programs: Value = serde_json::from_str(&p.programs_json(
            &doc,
            &mut Collider::default(),
            &Cell::default(),
        ))
        .unwrap();
        let program = &programs[&b];
        assert_eq!(program["steps"].as_array().unwrap().len(), 2);
        assert_eq!(program["steps"][1]["name"], "P3");
        assert_eq!(program["steps"][1]["kind"], "joint");
        assert!(!program["back"].is_null(), "closed: a move back to P1");
        assert!(program["cycle"].as_f64().unwrap() < cycle);

        // Going to a point poses the robot there.
        ask(&mut p, &doc, json!({"op": "goto", "robot": b, "index": 1}));
        let shown = p.shown(&doc).unwrap();
        let kin = Kinematics::of(&shown, base).unwrap();
        assert!((kin.current[0] - 0.2).abs() < 1e-9);
    }

    #[test]
    fn actions_go_between_the_moves() {
        let (doc, base, _, tip) = arm();
        let mut p = Posing::default();
        let (b, t) = (base.to_string(), tip.to_string());
        ask(&mut p, &doc, json!({"op": "teach", "robot": b, "tool": t}));
        ask(&mut p, &doc, json!({"op": "teach", "robot": b, "tool": t}));
        // Between the two: grip, then wait two seconds.
        let (a, _) = ask(
            &mut p,
            &doc,
            json!({"op": "add", "robot": b, "after": 0, "step": {"type": "grip", "gripper": t}}),
        );
        assert_eq!(a["ok"], true, "{a}");
        ask(
            &mut p,
            &doc,
            json!({"op": "add", "robot": b, "after": 1, "step": {"type": "delay", "seconds": 2.0}}),
        );
        let programs: Value = serde_json::from_str(&p.programs_json(
            &doc,
            &mut Collider::default(),
            &Cell::default(),
        ))
        .unwrap();
        let steps = programs[&b]["steps"].as_array().unwrap();
        let types: Vec<&str> = steps.iter().map(|s| s["type"].as_str().unwrap()).collect();
        assert_eq!(types, ["move", "grip", "delay", "move"]);
        assert_eq!(steps[1]["gripper_name"], "tool0");
        // The second move is the plan's first move, timed as before.
        assert!(steps[3]["move"]["duration"].is_number());
        // An action is edited as a whole; a move's tools are not for it.
        let (a, _) = ask(
            &mut p,
            &doc,
            json!({"op": "edit", "robot": b, "index": 2, "step": {"type": "delay", "seconds": 5.0}}),
        );
        assert_eq!(a["ok"], true, "{a}");
        for op in ["goto", "retouch"] {
            let (a, _) = ask(&mut p, &doc, json!({"op": op, "robot": b, "index": 1}));
            assert_eq!(a["ok"], false, "{op} on an action");
        }
        let (a, _) = ask(&mut p, &doc, json!({"op": "edit", "robot": b, "index": 9}));
        assert_eq!(a["ok"], false, "no tenth step");
        let (a, _) = ask(
            &mut p,
            &doc,
            json!({"op": "add", "robot": b, "step": {"type": "wait", "machine": "1", "signal": "nonsense"}}),
        );
        assert_eq!(a["ok"], false);
    }

    /// A file of its own in the temporary directory, gone when dropped.
    struct TempFile(PathBuf);

    impl TempFile {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "kynex-{name}-{}-{:?}.json",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_file(&path);
            Self(path)
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
            let _ = std::fs::remove_file(self.0.with_extension("json.new"));
        }
    }

    fn teach_two(p: &mut Posing, doc: &Document, base: u64, joint: u64, tip: u64) {
        let (b, j, t) = (base.to_string(), joint.to_string(), tip.to_string());
        for (at, kind) in [(-0.4, "joint"), (0.6, "joint")] {
            ask(
                p,
                doc,
                json!({"op": "jog", "robot": b, "joint": j, "position": at}),
            );
            ask(
                p,
                doc,
                json!({"op": "teach", "robot": b, "tool": t, "offset": [0, 0, 0.05], "kind": kind}),
            );
        }
        ask(p, doc, json!({"op": "close", "robot": b, "close": true}));
    }

    #[test]
    fn programs_are_kept_in_their_file_between_runs() {
        let file = TempFile::new("programs");
        let (doc, base, joint, tip) = arm();
        let mut p = Posing::with_file(&file.0, &file.0).unwrap().0;
        assert_eq!(p.pending(), 0, "no file yet");
        teach_two(&mut p, &doc, base, joint, tip);
        assert!(p.take_programs_changed());
        p.save(&doc, Project::default()).unwrap();
        let before: Value = serde_json::from_str(&p.programs_json(
            &doc,
            &mut Collider::default(),
            &Cell::default(),
        ))
        .unwrap();

        // The next run reads it, and takes it on once its robot is there.
        let mut again = Posing::with_file(&file.0, &file.0).unwrap().0;
        assert_eq!(again.pending(), 1);
        again.adopt(&Document::new());
        assert_eq!(again.pending(), 1, "no robot, still waiting");
        again.adopt(&doc);
        assert_eq!(again.pending(), 0);
        let after: Value = serde_json::from_str(&again.programs_json(
            &doc,
            &mut Collider::default(),
            &Cell::default(),
        ))
        .unwrap();
        assert_eq!(before, after);

        // Its next point is numbered on from the file's.
        ask(
            &mut again,
            &doc,
            json!({"op": "teach", "robot": base.to_string(), "tool": tip.to_string()}),
        );
        let now: Value = serde_json::from_str(&again.programs_json(
            &doc,
            &mut Collider::default(),
            &Cell::default(),
        ))
        .unwrap();
        assert_eq!(now[base.to_string()]["steps"][2]["name"], "P3");
    }

    #[test]
    fn a_saved_program_finds_its_robot_by_name_in_a_scene_built_anew() {
        let file = TempFile::new("renumbered");
        let (doc, base, joint, tip) = arm();
        let mut p = Posing::with_file(&file.0, &file.0).unwrap().0;
        teach_two(&mut p, &doc, base, joint, tip);
        p.save(&doc, Project::default()).unwrap();

        // The same robot, every object numbered differently.
        let mut other = Document::new();
        other.add_object(Object::new("Floor", ObjectData::Empty));
        other.add_object(Object::new("Fence", ObjectData::Empty));
        let (other, base2, joint2, _) = arm_in(other);
        assert_ne!(base, base2);
        let mut again = Posing::with_file(&file.0, &file.0).unwrap().0;
        again.adopt(&other);
        let programs: Value = serde_json::from_str(&again.programs_json(
            &other,
            &mut Collider::default(),
            &Cell::default(),
        ))
        .unwrap();
        let program = &programs[base2.to_string()];
        assert_eq!(program["steps"].as_array().unwrap().len(), 2);
        assert_eq!(program["close"], true);
        // Joint and tool found by name: P2 is at 0.6 rad, the tool 50 mm out.
        ask(
            &mut again,
            &other,
            json!({"op": "goto", "robot": base2.to_string(), "index": 1}),
        );
        let shown = again.shown(&other).unwrap();
        let kin = Kinematics::of(&shown, base2).unwrap();
        assert_eq!(kin.vars[0].uid, joint2);
        assert!((kin.current[0] - 0.6).abs() < 1e-12);
        assert!(
            !program["trace"].as_array().unwrap().is_empty(),
            "the tool was found"
        );
    }

    #[test]
    fn urscript_is_refused_for_an_arm_that_is_not_a_ur() {
        let (doc, base, joint, tip) = arm();
        let mut p = Posing::default();
        teach_two(&mut p, &doc, base, joint, tip);
        let (answer, _) = ask(
            &mut p,
            &doc,
            json!({"op": "urscript", "robot": base.to_string()}),
        );
        assert_eq!(answer["ok"], false);
        assert!(answer["error"].as_str().unwrap().contains("6"));
    }

    #[test]
    fn a_program_is_exported_for_the_vendor_asked_for() {
        let (doc, base, joint, tip) = arm();
        let mut p = Posing::default();
        teach_two(&mut p, &doc, base, joint, tip);
        for vendor in ["abb", "kuka"] {
            let (answer, _) = ask(
                &mut p,
                &doc,
                json!({"op": "export", "vendor": vendor, "robot": base.to_string()}),
            );
            // Refused: the arm is no six-axis one, and KUKA has no writer.
            assert_eq!(answer["ok"], false);
            let error = answer["error"].as_str().unwrap();
            assert!(
                error.contains(if vendor == "abb" { "6" } else { "kuka" }),
                "{error}"
            );
        }
    }
}
