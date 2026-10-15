use kreate_math::DVec3;
use serde_json::json;

use super::*;
use crate::collide::Collider;
use crate::library::Library;
use crate::program;
use crate::timeline::{Now, Timeline};

/// The M1 cell from the library: a tending UR10e on a 0.7 m pedestal, a
/// CNC lathe ahead of it, an infeed table on its left and an outfeed
/// behind. The machine's cycle is 10 s here.
struct Cell1 {
    doc: Document,
    cell: Cell,
    posing: Posing,
    library: Library,
    robot: u64,
    grip_a: u64,
    grip_b: u64,
    machine: u64,
    /// Every CNC machine, the first being `machine`.
    machines: Vec<u64>,
    infeed: u64,
    outfeed: u64,
}

/// An identity, as the viewer's JSON carries it: text.
fn uid_of(v: &serde_json::Value) -> Option<u64> {
    v.as_str()?.parse().ok()
}

fn ask_cell(cell: &mut Cell, doc: &Document, req: serde_json::Value) -> serde_json::Value {
    serde_json::from_str(&cell.handle(doc, &req.to_string())).unwrap()
}

fn ask_posing(p: &mut Posing, doc: &Document, req: serde_json::Value) -> serde_json::Value {
    serde_json::from_str(&p.handle(doc, &req.to_string()).0).unwrap()
}

fn cell1() -> Cell1 {
    cell_with(1, 10.0)
}

/// The M1 cell with `cncs` machines (one or two), each machining for
/// `cycle` seconds. The second stands on the robot's right, facing it.
fn cell_with(cncs: usize, cycle: f64) -> Cell1 {
    cell_for("ur10e_tending", 0.7, cncs, cycle)
}

/// [`cell_with`], its robot the library's `robot` on a pedestal `height`
/// metres high.
fn cell_for(robot: &str, height: f64, cncs: usize, cycle: f64) -> Cell1 {
    let mut library = Library::default();
    let mut placed = vec![
        ("pedestal", [0.0, 0.0, 0.0], 0.0),
        (robot, [0.0, 0.0, height], 0.0),
        ("cnc_lathe", [0.0, 0.75, 0.0], 0.0),
        ("infeed_station", [-0.8, 0.05, 0.0], 0.0),
        ("outfeed_station", [0.0, -0.8, 0.0], 0.0),
    ];
    if cncs > 1 {
        placed.push(("cnc_lathe", [0.75, 0.0, 0.0], -std::f64::consts::FRAC_PI_2));
    }
    for (template, at, yaw) in placed {
        library.handle(
            &json!({"op": "place", "template": template, "at": at, "yaw": yaw}).to_string(),
        );
    }
    if height != 0.7 {
        library
            .handle(&json!({"op": "resize", "item": 1, "params": {"height": height}}).to_string());
    }
    let mut doc = Document::new();
    library.apply(&mut doc);
    let mut cell = Cell::default();
    let mut machines = Vec::new();
    let (mut infeed, mut outfeed, mut grip_a, mut grip_b) = (0, 0, 0, 0);
    for (uid, name, kind) in library.roles() {
        match kind {
            crate::cell::Kind::Cnc => machines.push(uid),
            crate::cell::Kind::Infeed => infeed = uid,
            crate::cell::Kind::Outfeed => outfeed = uid,
            crate::cell::Kind::Gripper if name == "grip_a" => grip_a = uid,
            crate::cell::Kind::Gripper if name == "grip_b" => grip_b = uid,
            _ => {}
        }
        cell.ensure(uid, name, kind);
    }
    for m in &machines {
        ask_cell(
            &mut cell,
            &doc,
            json!({"op": "role", "object": m.to_string(), "kind": "cnc", "props": {"cycle": cycle}}),
        );
    }
    ask_cell(
        &mut cell,
        &doc,
        json!({"op": "fill", "object": infeed.to_string()}),
    );
    let robot = kynex_model::robots(&doc)[0].root;
    Cell1 {
        doc,
        cell,
        posing: Posing::default(),
        library,
        robot,
        grip_a,
        grip_b,
        machine: machines[0],
        machines,
        infeed,
        outfeed,
    }
}

impl Cell1 {
    /// Teaches a point with grip_a at `at`, pointing down.
    fn teach(&mut self, at: DVec3, kind: &str) {
        self.teach_with(at, kind, self.grip_a);
    }

    /// Teaches a point with grip point `tool` at `at`, pointing down.
    fn teach_with(&mut self, at: DVec3, kind: &str, tool: u64) {
        let (r, t) = (self.robot.to_string(), tool.to_string());
        // Into a joint move the arm first turns at its base to face the
        // point, as someone teaching it would: from there the solver keeps
        // the elbow up rather than folding the arm round the other way.
        if kind == "joint" {
            let kin = kynex_kin::Kinematics::of(&self.doc, self.robot).unwrap();
            let tool = kin.tool(tool, kreate_math::DAffine3::IDENTITY).unwrap();
            let q = self.posing.positions(&kin, self.robot);
            let base = kin.world_of(&q, self.robot).unwrap().translation;
            let now = kin.pose(&q, &tool).translation - base;
            let want = at - base;
            let turn = (want.y.atan2(want.x) - now.y.atan2(now.x) + std::f64::consts::PI)
                .rem_euclid(std::f64::consts::TAU)
                - std::f64::consts::PI;
            // The first joint: a UR's shoulder pan, Doosan's joint 1.
            let pan = 0;
            ask_posing(
                &mut self.posing,
                &self.doc,
                json!({"op": "jog", "robot": r, "joint": kin.vars[pan].uid.to_string(),
                       "position": q[pan] + turn}),
            );
        }
        let a = ask_posing(
            &mut self.posing,
            &self.doc,
            json!({"op": "ik", "robot": r, "tool": t, "position": [at.x, at.y, at.z],
                   "rotation": [1.0, 0.0, 0.0, 0.0], "restarts": 30}),
        );
        assert_eq!(a["reached"], true, "{at}: {a}");
        let a = ask_posing(
            &mut self.posing,
            &self.doc,
            json!({"op": "teach", "robot": r, "tool": t, "kind": kind}),
        );
        assert_eq!(a["ok"], true, "{a}");
    }

    fn act(&mut self, step: serde_json::Value) {
        let a = ask_posing(
            &mut self.posing,
            &self.doc,
            json!({"op": "add", "robot": self.robot.to_string(), "step": step}),
        );
        assert_eq!(a["ok"], true, "{a}");
    }

    fn slot(&self, pallet: u64, i: usize) -> DVec3 {
        let cell: serde_json::Value = serde_json::from_str(&self.cell.to_json(&self.doc)).unwrap();
        let c = cell["components"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| uid_of(&c["object"]) == Some(pallet))
            .unwrap();
        let p = &c["slots"][i];
        DVec3::new(
            p[0].as_f64().unwrap(),
            p[1].as_f64().unwrap(),
            p[2].as_f64().unwrap(),
        )
    }

    fn seat(&self) -> DVec3 {
        self.seat_of(self.machine)
    }

    fn seat_of(&self, machine: u64) -> DVec3 {
        let cell: serde_json::Value = serde_json::from_str(&self.cell.to_json(&self.doc)).unwrap();
        let c = cell["components"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| uid_of(&c["object"]) == Some(machine))
            .unwrap();
        let p = &c["seat"];
        DVec3::new(
            p[0].as_f64().unwrap(),
            p[1].as_f64().unwrap(),
            p[2].as_f64().unwrap(),
        )
    }

    /// Runs the program until it stops, at most `limit` simulated seconds.
    fn run(&mut self, limit: f64) -> (Runner, f64) {
        let (runner, t, _) = self.run_timed(limit);
        (runner, t)
    }

    /// The same, recording what the robot and the machine did.
    fn run_timed(&mut self, limit: f64) -> (Runner, f64, Timeline) {
        let program = self.posing.program(self.robot).unwrap().clone();
        let mut runner = Runner::new(self.robot, program, &self.doc, 0.0).unwrap();
        let start = Instant::now();
        let mut t = 0.0;
        let mut timeline = Timeline::new(0.0);
        while t < limit && !runner.stopped() {
            let now = start + Duration::from_secs_f64(t);
            runner.tick(t, now, &mut self.posing, &mut self.cell, &self.doc);
            self.cell.tick(&self.doc, t, now);
            let mut lanes = vec![Now {
                key: "robot".into(),
                name: "robot".into(),
                group: "robot",
                activity: runner.activity(),
            }];
            lanes.extend(
                self.cell
                    .activities(&self.doc)
                    .into_iter()
                    .map(|(uid, name, a)| Now {
                        key: uid.to_string(),
                        name,
                        group: "machine",
                        activity: a,
                    }),
            );
            timeline.sample(t, lanes);
            t += 0.02;
        }
        (runner, t, timeline)
    }
}

/// The M1 program: a raw part from the infeed into the machine, the
/// machine run, the finished part out to the outfeed.
fn tending(c: &mut Cell1) {
    let (m, g) = (c.machine.to_string(), c.grip_a.to_string());
    let up = DVec3::Z * 0.15;
    let raw = c.slot(c.infeed, 0);
    let seat = c.seat();
    let out = c.slot(c.outfeed, 0);
    // Above the raw part, down to it, grip, up.
    c.teach(raw + up, "joint");
    c.teach(raw, "linear");
    c.act(json!({"type": "grip", "gripper": g}));
    c.teach(raw + up, "linear");
    // The door open, in over the chuck, down, let go, back out.
    c.act(json!({"type": "command", "machine": m, "command": "open"}));
    c.act(json!({"type": "wait", "machine": m, "signal": "door_open"}));
    c.teach(seat + up, "joint");
    c.teach(seat, "linear");
    c.act(json!({"type": "release", "gripper": g}));
    c.teach(seat + up, "linear");
    c.teach(seat + DVec3::new(0.0, -0.6, 0.15), "linear");
    // Clamp, shut, machine, and wait for it.
    c.act(json!({"type": "command", "machine": m, "command": "clamp"}));
    c.act(json!({"type": "command", "machine": m, "command": "close"}));
    c.act(json!({"type": "wait", "machine": m, "signal": "door_closed"}));
    c.act(json!({"type": "command", "machine": m, "command": "start"}));
    c.act(json!({"type": "wait", "machine": m, "signal": "done"}));
    // Open, take the finished part, put it out.
    c.act(json!({"type": "command", "machine": m, "command": "open"}));
    c.act(json!({"type": "wait", "machine": m, "signal": "door_open"}));
    c.act(json!({"type": "command", "machine": m, "command": "unclamp"}));
    c.teach(seat + up, "linear");
    // A finished part stands lower: it is shorter.
    c.teach(
        seat + DVec3::Z * ((crate::cell::FINISHED.1 - crate::cell::RAW.1) / 2.0),
        "linear",
    );
    c.act(json!({"type": "grip", "gripper": g}));
    c.teach(seat + up, "linear");
    c.teach(out + up, "joint");
    c.teach(out, "linear");
    c.act(json!({"type": "release", "gripper": g}));
    c.teach(out + up, "linear");
}

#[test]
fn a_machine_tending_program_loads_waits_for_the_cycle_and_unloads() {
    let mut c = cell1();
    tending(&mut c);
    let (runner, t) = c.run(120.0);
    let state = runner.to_json(t);
    assert_eq!(state["state"], "done", "{state}");
    // The part went the whole way: finished, on the outfeed.
    let cell: serde_json::Value = serde_json::from_str(&c.cell.to_json(&c.doc)).unwrap();
    let parts = cell["parts"].as_array().unwrap();
    assert_eq!(parts.iter().filter(|p| p["finished"] == true).count(), 1);
    assert!(parts.iter().all(|p| p["held"].is_null()));
    // The 10 s cycle, two 3 s doors each way and the moves: about 30 s.
    assert!(t > 25.0 && t < 60.0, "{t} s");
    // Nothing held in a gripper ran into anything along the way: the
    // check sees the robot where it stopped, clear of the cell.
    let shown = c.posing.shown(&c.doc).unwrap();
    let contacts = Collider::default().contacts(&shown, &c.cell.held());
    assert!(contacts.is_empty(), "{contacts:?}");
}

#[test]
fn a_refused_step_stops_the_run_with_why() {
    let mut c = cell1();
    let m = c.machine.to_string();
    // Start the machine, shut, with nothing in its chuck.
    c.teach(c.seat() + DVec3::new(0.0, -0.6, 0.15), "joint");
    c.act(json!({"type": "command", "machine": m, "command": "start"}));
    let (runner, t) = c.run(10.0);
    let state = runner.to_json(t);
    assert_eq!(state["state"], "alarm");
    assert!(
        state["detail"].as_str().unwrap().contains("소재"),
        "{state}"
    );
}

#[test]
fn a_cycle_runs_again_and_is_timed() {
    let mut c = cell1();
    let seat = c.seat();
    c.teach(seat + DVec3::new(0.0, -0.6, 0.15), "joint");
    c.act(json!({"type": "delay", "seconds": 2.0}));
    c.teach(seat + DVec3::new(0.2, -0.6, 0.15), "joint");
    ask_posing(
        &mut c.posing,
        &c.doc,
        json!({"op": "close", "robot": c.robot.to_string(), "close": true}),
    );
    let (runner, t) = c.run(20.0);
    let state = runner.to_json(t);
    assert!(state["cycles"].as_u64().unwrap() >= 2, "{state}");
    let cycle = state["last_cycle"].as_f64().unwrap();
    // Two short moves and the 2 s pause.
    assert!(cycle > 2.0 && cycle < 8.0, "{cycle}");
}

#[test]
fn the_clock_runs_at_its_speed() {
    let mut clock = Clock::default();
    let t0 = Instant::now();
    clock.advance(t0);
    clock.speed = 10.0;
    assert!((clock.advance(t0 + Duration::from_secs(2)) - 20.0).abs() < 1e-9);
}

#[test]
fn a_door_the_program_has_opened_is_not_in_its_way() {
    let mut c = cell1();
    tending(&mut c);
    let programs: serde_json::Value = serde_json::from_str(&c.posing.programs_json(
        &c.doc,
        &mut Collider::default(),
        &c.cell,
    ))
    .unwrap();
    let steps = programs[c.robot.to_string()]["steps"].as_array().unwrap();
    let problems: Vec<&str> = steps
        .iter()
        .flat_map(|s| s["move"]["problems"].as_array().into_iter().flatten())
        .filter_map(|p| p.as_str())
        .collect();
    assert!(problems.iter().all(|p| !p.contains("door")), "{problems:?}");
    // Without the opening, the way in runs into the door.
    let mut shut = c.posing.program(c.robot).unwrap().clone();
    shut.steps.retain(|s| {
        !matches!(
            s,
            Step::Do(Action::Command {
                command: crate::cell::Command::Open,
                ..
            })
        )
    });
    assert!(shut.open_during_moves().iter().all(|o| o.is_empty()));
}

#[test]
fn the_timeline_shows_the_machine_cutting_and_the_robot_waiting_for_it() {
    let mut c = cell1();
    tending(&mut c);
    let (_, t, timeline) = c.run_timed(120.0);
    let j = timeline.to_json();
    let lanes = j["lanes"].as_array().unwrap();
    let machine = lanes.iter().find(|l| l["group"] == "machine").unwrap();
    let robot = lanes.iter().find(|l| l["group"] == "robot").unwrap();
    // The 10 s cycle, cut once.
    let cut = machine["totals"]["cut"].as_f64().unwrap();
    assert!((cut - 10.0).abs() < 0.1, "{cut}");
    assert_eq!(j["parts"], 1);
    // The robot waited out the cycle, and the doors.
    let waited = robot["totals"]["wait"].as_f64().unwrap();
    assert!(waited > 10.0 && waited < 20.0, "{waited}");
    // The shares add up to the run.
    let total: f64 = robot["shares"]
        .as_object()
        .unwrap()
        .values()
        .map(|v| v.as_f64().unwrap())
        .sum();
    assert!((total - 1.0).abs() < 1e-6, "{total}");
    assert!((j["elapsed"].as_f64().unwrap() - (t - 0.02)).abs() < 1e-6);
}

/// Loads a raw part from the infeed into `machine` and starts it.
fn load(c: &mut Cell1, machine: u64) {
    let (m, g) = (machine.to_string(), c.grip_a.to_string());
    let up = DVec3::Z * 0.15;
    let raw = c.slot(c.infeed, 0);
    let seat = c.seat_of(machine);
    // Back out towards the robot.
    let back = DVec3::new(-seat.x, -seat.y, 0.0).normalize() * 0.6 + up;
    c.teach(raw + up, "joint");
    c.teach(raw, "linear");
    c.act(json!({"type": "grip", "gripper": g}));
    c.teach(raw + up, "linear");
    c.act(json!({"type": "command", "machine": m, "command": "open"}));
    c.act(json!({"type": "wait", "machine": m, "signal": "door_open"}));
    c.teach(seat + up, "joint");
    c.teach(seat, "linear");
    c.act(json!({"type": "release", "gripper": g}));
    c.teach(seat + up, "linear");
    c.teach(seat + back, "linear");
    c.act(json!({"type": "command", "machine": m, "command": "clamp"}));
    c.act(json!({"type": "command", "machine": m, "command": "close"}));
    c.act(json!({"type": "wait", "machine": m, "signal": "door_closed"}));
    c.act(json!({"type": "command", "machine": m, "command": "start"}));
}

/// Waits for `machine`, takes its finished part out to the outfeed.
fn unload(c: &mut Cell1, machine: u64) {
    let (m, g) = (machine.to_string(), c.grip_a.to_string());
    let up = DVec3::Z * 0.15;
    let seat = c.seat_of(machine);
    let back = DVec3::new(-seat.x, -seat.y, 0.0).normalize() * 0.6 + up;
    let out = c.slot(c.outfeed, 0);
    c.act(json!({"type": "wait", "machine": m, "signal": "done"}));
    c.act(json!({"type": "command", "machine": m, "command": "open"}));
    c.act(json!({"type": "wait", "machine": m, "signal": "door_open"}));
    c.act(json!({"type": "command", "machine": m, "command": "unclamp"}));
    c.teach(seat + up, "joint");
    c.teach(
        seat + DVec3::Z * ((crate::cell::FINISHED.1 - crate::cell::RAW.1) / 2.0),
        "linear",
    );
    c.act(json!({"type": "grip", "gripper": g}));
    c.teach(seat + up, "linear");
    c.teach(seat + back, "linear");
    c.teach(out + up, "joint");
    c.teach(out, "linear");
    c.act(json!({"type": "release", "gripper": g}));
    c.teach(out + up, "linear");
}

/// The cell as a scenario: its equipment, roles and programs.
fn scenario_of(c: &mut Cell1, name: &str) -> serde_json::Value {
    crate::scenario::snapshot(
        name,
        c.posing.saved_programs(&c.doc),
        c.cell.to_saved(),
        c.library.to_saved(),
    )
}

#[test]
fn a_second_machine_nearly_doubles_the_parts_an_hour() {
    // Machining for a minute: the robot has time to tend a second machine.
    let mut one = cell_with(1, 60.0);
    let m = one.machine;
    load(&mut one, m);
    unload(&mut one, m);
    let one = scenario_of(&mut one, "CNC 1대");

    let mut two = cell_with(2, 60.0);
    let (m1, m2) = (two.machines[0], two.machines[1]);
    load(&mut two, m1);
    load(&mut two, m2);
    unload(&mut two, m1);
    unload(&mut two, m2);
    let two = scenario_of(&mut two, "CNC 2대");

    let started = std::time::Instant::now();
    let session = Document::new();
    let a = crate::scenario::simulate(&session, &one, 900.0);
    let b = crate::scenario::simulate(&session, &two, 900.0);
    let took = started.elapsed();
    assert!(a["alarm"].is_null(), "{a}");
    assert!(b["alarm"].is_null(), "{b}");
    assert_eq!(
        (a["machines"].as_u64(), b["machines"].as_u64()),
        (Some(1), Some(2))
    );
    let (ra, rb) = (
        a["per_hour"].as_f64().unwrap(),
        b["per_hour"].as_f64().unwrap(),
    );
    // One machine: a minute's cut and some 25 s of tending each part.
    assert!(ra > 30.0 && ra < 50.0, "{a}");
    assert!(rb > ra * 1.5, "{ra} against {rb}: {b}");
    // The one machine is idle while it is tended; the robot mostly waits.
    let u = a["machine_lanes"][0]["utilization"].as_f64().unwrap();
    assert!(u > 0.55 && u < 0.85, "{a}");
    let wait = |s: &serde_json::Value| s["robot_lanes"][0]["wait"].as_f64().unwrap();
    assert!(wait(&a) > wait(&b), "{a}\n{b}");
    println!("{a}\n{b}\n{took:?}");
}

/// Binds the move at step `index` to `pallet`.
fn on_pallet(c: &mut Cell1, index: usize, pallet: u64) {
    let a = ask_posing(
        &mut c.posing,
        &c.doc,
        json!({"op": "edit", "robot": c.robot.to_string(), "index": index, "pallet": pallet.to_string()}),
    );
    assert_eq!(a["ok"], true, "{a}");
}

#[test]
fn points_on_a_pallet_go_from_place_to_place() {
    let mut c = cell1();
    let g = c.grip_a.to_string();
    let up = DVec3::Z * 0.15;
    // Taught over the first place of each pallet: down, grip or let go, up,
    // and the count on.
    let raw = c.slot(c.infeed, 0);
    let out = c.slot(c.outfeed, 0);
    c.teach(raw + up, "joint");
    c.teach(raw, "linear");
    c.act(json!({"type": "grip", "gripper": g}));
    c.teach(raw + up, "linear");
    c.teach(out + up, "joint");
    c.teach(out, "linear");
    c.act(json!({"type": "release", "gripper": g}));
    c.teach(out + up, "linear");
    c.act(json!({"type": "next", "pallet": c.infeed.to_string()}));
    c.act(json!({"type": "next", "pallet": c.outfeed.to_string()}));
    let (infeed, outfeed) = (c.infeed, c.outfeed);
    for i in [0, 1, 3] {
        on_pallet(&mut c, i, infeed);
    }
    for i in [4, 5, 7] {
        on_pallet(&mut c, i, outfeed);
    }
    ask_posing(
        &mut c.posing,
        &c.doc,
        json!({"op": "close", "robot": c.robot.to_string(), "close": true}),
    );
    // Saved and read back, it keeps its pallets.
    let saved = c.posing.saved_programs(&c.doc);
    let (_, again) = crate::program::Program::from_saved(&saved[0], &c.doc).unwrap();
    assert_eq!(&again, c.posing.program(c.robot).unwrap());

    let (runner, t) = c.run(40.0);
    let state = runner.to_json(t);
    assert_ne!(state["state"], "alarm", "{state}");
    let cycles = state["cycles"].as_u64().unwrap() as usize;
    assert!(cycles >= 3, "{state}");
    // Raw parts taken from the first places of the infeed, in order, and
    // set down on the first places of the outfeed: none on another.
    let cell: serde_json::Value = serde_json::from_str(&c.cell.to_json(&c.doc)).unwrap();
    let parts = cell["parts"].as_array().unwrap();
    let at = |p: &serde_json::Value| {
        DVec3::new(
            p["at"][0].as_f64().unwrap(),
            p["at"][1].as_f64().unwrap(),
            p["at"][2].as_f64().unwrap(),
        )
    };
    let near = |place: DVec3| {
        parts
            .iter()
            .any(|p| p["held"].is_null() && (at(p) - place).truncate().length() < 0.01)
    };
    for i in 0..cycles {
        assert!(!near(c.slot(c.infeed, i)), "infeed {i} still full");
        assert!(near(c.slot(c.outfeed, i)), "outfeed {i} empty");
    }
    assert!(near(c.slot(c.infeed, cycles + 1)));
    assert!(!near(c.slot(c.outfeed, cycles + 1)));
}

/// Moves the first CNC machine to `at`, turned `yaw`, and builds the scene
/// again: as when it is dragged in the viewer.
fn move_machine(c: &mut Cell1, at: [f64; 3], yaw: f64) {
    let item = c
        .library
        .items()
        .iter()
        .find(|i| i.template == "cnc_lathe")
        .unwrap()
        .id;
    c.library
        .handle(&json!({"op": "move", "item": item, "at": at, "yaw": yaw}).to_string());
    let mut doc = Document::new();
    c.library.apply(&mut doc);
    c.doc = doc;
}

#[test]
fn points_in_a_machines_frame_follow_it_when_it_moves() {
    // The moves into and out of the machine, in its frame.
    let into_machine = [6, 7, 9, 10, 19, 20, 22];
    let mut framed = cell1();
    tending(&mut framed);
    let m = framed.machine;
    for i in into_machine {
        let a = ask_posing(
            &mut framed.posing,
            &framed.doc,
            json!({"op": "edit", "robot": framed.robot.to_string(), "index": i, "frame": m.to_string()}),
        );
        assert_eq!(a["ok"], true, "{a}");
    }
    // Saved and read back, it keeps its frames.
    let saved = framed.posing.saved_programs(&framed.doc);
    let (_, again) = crate::program::Program::from_saved(&saved[0], &framed.doc).unwrap();
    assert_eq!(&again, framed.posing.program(framed.robot).unwrap());
    let mut plain = cell1();
    tending(&mut plain);

    // The machine 10 cm to the side and turned a little.
    for c in [&mut framed, &mut plain] {
        move_machine(c, [0.1, 0.75, 0.0], 0.1);
    }
    let (runner, t) = framed.run(120.0);
    let state = runner.to_json(t);
    assert_eq!(state["state"], "done", "{state}");
    let cell: serde_json::Value = serde_json::from_str(&framed.cell.to_json(&framed.doc)).unwrap();
    let parts = cell["parts"].as_array().unwrap();
    assert_eq!(parts.iter().filter(|p| p["finished"] == true).count(), 1);

    // Taught in the world, the program misses the chuck it moved.
    let (runner, t) = plain.run(120.0);
    let state = runner.to_json(t);
    assert_eq!(state["state"], "alarm", "{state}");
}

fn edit_step(c: &mut Cell1, index: usize, field: &str, uid: u64) {
    let a = ask_posing(
        &mut c.posing,
        &c.doc,
        json!({"op": "edit", "robot": c.robot.to_string(), "index": index, field: uid.to_string()}),
    );
    assert_eq!(a["ok"], true, "{a}");
}

#[test]
fn work_taught_at_one_machine_is_copied_for_another() {
    let mut c = cell_with(2, 10.0);
    let (m1, m2) = (c.machines[0], c.machines[1]);
    let (infeed, outfeed) = (c.infeed, c.outfeed);
    // At the first machine only: load, unload, and the pallets on.
    load(&mut c, m1);
    unload(&mut c, m1);
    c.act(json!({"type": "next", "pallet": infeed.to_string()}));
    c.act(json!({"type": "next", "pallet": outfeed.to_string()}));
    for i in [0, 1, 3] {
        edit_step(&mut c, i, "pallet", infeed);
    }
    for i in [24, 25, 27] {
        edit_step(&mut c, i, "pallet", outfeed);
    }
    for i in [6, 7, 9, 10, 19, 20, 22, 23] {
        edit_step(&mut c, i, "frame", m1);
    }
    let steps = c.posing.program(c.robot).unwrap().steps.len();
    let a = ask_posing(
        &mut c.posing,
        &c.doc,
        json!({"op": "copy", "robot": c.robot.to_string(), "from": 0, "to": steps - 1,
               "machine": m2.to_string()}),
    );
    assert_eq!(a["ok"], true, "{a}");
    assert_eq!(a["added"], steps);
    let program = c.posing.program(c.robot).unwrap();
    assert_eq!(program.steps.len(), steps * 2);
    // The copy tells and waits for the second machine, in its frame.
    for step in &program.steps[steps..] {
        match step {
            Step::Do(Action::Command { machine, .. } | Action::Wait { machine, .. }) => {
                assert_eq!(*machine, m2)
            }
            Step::Move(p) => assert!(p.frame.is_none_or(|f| f.object != m1)),
            _ => {}
        }
    }

    let (runner, t, timeline) = c.run_timed(150.0);
    let state = runner.to_json(t);
    assert_eq!(state["state"], "done", "{state}");
    // Both machines cut once, and two parts are out on the outfeed.
    let j = timeline.to_json();
    let cut: Vec<f64> = j["lanes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|l| l["group"] == "machine")
        .map(|l| l["totals"]["cut"].as_f64().unwrap_or(0.0))
        .collect();
    assert_eq!(cut.len(), 2);
    assert!(cut.iter().all(|c| (c - 10.0).abs() < 0.1), "{cut:?}");
    let cell: serde_json::Value = serde_json::from_str(&c.cell.to_json(&c.doc)).unwrap();
    let parts = cell["parts"].as_array().unwrap();
    assert_eq!(parts.iter().filter(|p| p["finished"] == true).count(), 2);
}

#[test]
fn steps_move_up_and_down_past_their_neighbours() {
    let delay = |s: f64| Step::Do(Action::Delay { seconds: s });
    let mut p = crate::program::Program::default();
    p.steps = (0..5).map(|i| delay(i as f64)).collect();
    let order = |p: &crate::program::Program| -> Vec<f64> {
        p.steps
            .iter()
            .map(|s| match s {
                Step::Do(Action::Delay { seconds }) => *seconds,
                _ => -1.0,
            })
            .collect()
    };
    // Steps 1 and 2 up, past step 0; then down twice.
    assert!(p.shift(1, 2, true));
    assert_eq!(order(&p), [1.0, 2.0, 0.0, 3.0, 4.0]);
    assert!(p.shift(0, 1, false));
    assert!(p.shift(1, 2, false));
    assert_eq!(order(&p), [0.0, 3.0, 1.0, 2.0, 4.0]);
    // Not past either end.
    assert!(!p.shift(0, 0, true));
    assert!(!p.shift(4, 4, false));
}

#[test]
fn blended_points_shorten_the_cycle_and_the_parts_still_go_round() {
    let mut plain = cell1();
    tending(&mut plain);
    let (runner, slow) = plain.run(120.0);
    assert_eq!(runner.to_json(slow)["state"], "done");

    let mut blended = cell1();
    tending(&mut blended);
    // Through the points between moves: over the chuck on the way out,
    // and on to the outfeed.
    for i in [9, 22, 23] {
        let a = ask_posing(
            &mut blended.posing,
            &blended.doc,
            json!({"op": "edit", "robot": blended.robot.to_string(), "index": i, "blend": 0.05}),
        );
        assert_eq!(a["ok"], true, "{a}");
    }
    let (runner, fast) = blended.run(120.0);
    let state = runner.to_json(fast);
    assert_eq!(state["state"], "done", "{state}");
    assert!(fast < slow - 0.2, "{fast} s against {slow} s");
    let cell: serde_json::Value =
        serde_json::from_str(&blended.cell.to_json(&blended.doc)).unwrap();
    let parts = cell["parts"].as_array().unwrap();
    assert_eq!(parts.iter().filter(|p| p["finished"] == true).count(), 1);
    assert!(parts.iter().all(|p| p["held"].is_null()));
}

#[test]
fn a_circular_move_is_taught_twice_saved_exported_and_run() {
    let mut c = cell1();
    let seat = c.seat();
    let start = seat + DVec3::new(0.0, -0.6, 0.15);
    c.teach(start, "joint");
    // Round a quarter of a circle about the start's left: via, then end.
    let (r, t) = (c.robot.to_string(), c.grip_a.to_string());
    let mut taught = |at: DVec3| {
        let a = ask_posing(
            &mut c.posing,
            &c.doc,
            json!({"op": "ik", "robot": r, "tool": t, "position": [at.x, at.y, at.z],
                   "rotation": [1.0, 0.0, 0.0, 0.0], "restarts": 30}),
        );
        assert_eq!(a["reached"], true, "{a}");
        ask_posing(
            &mut c.posing,
            &c.doc,
            json!({"op": "teach", "robot": r, "tool": t, "kind": "circular"}),
        )
    };
    let centre = start + DVec3::new(-0.15, 0.0, 0.0);
    let s = std::f64::consts::FRAC_1_SQRT_2 * 0.15;
    let first = taught(centre + DVec3::new(s, -s, 0.0));
    assert_eq!(first["via"], true, "the via point first: {first}");
    let end = centre + DVec3::new(0.0, -0.15, 0.0);
    let second = taught(end);
    assert_eq!(second["ok"], true, "{second}");
    let program = c.posing.program(c.robot).unwrap().clone();
    assert_eq!(program.steps.len(), 2);
    let Step::Move(p) = &program.steps[1] else {
        panic!("a move")
    };
    assert_eq!(p.kind, MoveKind::Circular);
    assert!(p.via.is_some());

    // Saved and read back, exported, and run.
    let saved = c.posing.saved_programs(&c.doc);
    let (_, again) = crate::program::Program::from_saved(&saved[0], &c.doc).unwrap();
    assert_eq!(again, program);
    let script = ask_posing(
        &mut c.posing,
        &c.doc,
        json!({"op": "urscript", "robot": c.robot.to_string()}),
    );
    assert!(
        script["script"].as_str().unwrap().contains("movec("),
        "{script}"
    );
    let (runner, t) = c.run(20.0);
    assert_eq!(runner.to_json(t)["state"], "done");
    let kin = kynex_kin::Kinematics::of(&c.doc, c.robot).unwrap();
    let tool = kin.tool(c.grip_a, kreate_math::DAffine3::IDENTITY).unwrap();
    let q = c.posing.positions(&kin, c.robot);
    assert!(kin.pose(&q, &tool).translation.distance(end) < 1e-3);
}

/// Writes the M1 tending program for URSim: the URScript, its moves
/// numbered on output register 0, and how long KYNEX plans each move. Run
/// by `tools/ursim/check.py`, not with the tests.
#[test]
#[ignore = "writes target/ursim for tools/ursim/check.py"]
fn ursim_program() {
    let mut c = cell1();
    tending(&mut c);
    // Blends where the robot would pass straight through.
    for i in [9, 22, 23] {
        ask_posing(
            &mut c.posing,
            &c.doc,
            json!({"op": "edit", "robot": c.robot.to_string(), "index": i, "blend": 0.05}),
        );
    }
    let program = c.posing.program(c.robot).unwrap().clone();
    let kin = kynex_kin::Kinematics::of(&c.doc, c.robot).unwrap();
    let (script, _) = program
        .urscript(
            c.robot,
            &c.doc,
            &kin,
            true,
            &crate::program::CellIo::default(),
        )
        .unwrap();
    let plan = program.plan(&kin).unwrap();
    let names: Vec<&str> = program.moves().map(|(_, p)| p.name.as_str()).collect();
    let moves: Vec<serde_json::Value> = plan
        .segments
        .iter()
        .map(|s| {
            json!({
                "name": names[s.to],
                "kind": crate::program::kind_tag(s.kind),
                "start": s.start,
                "duration": s.duration,
            })
        })
        .collect();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ursim");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("m1.script"), script).unwrap();
    std::fs::write(
        dir.join("m1.json"),
        serde_json::to_string_pretty(&json!({
            "program": "M1 machine tending, UR10e",
            "moves": moves,
            "duration": plan.duration,
        }))
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn a_program_for_a_library_robot_saved_before_layout_2_is_turned_at_the_base() {
    let mut c = cell1();
    tending(&mut c);
    let now = c.posing.saved_programs(&c.doc).remove(0);
    let pan = |v: &serde_json::Value| -> Vec<f64> {
        v["steps"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|s| s["joints"].as_array().cloned().unwrap_or_default())
            .filter(|j| j["name"] == "shoulder_pan_joint")
            .map(|j| j["position"].as_f64().unwrap())
            .collect()
    };
    // As layout 1 had it: the base joint half a turn the other way.
    let mut old = now.clone();
    for s in old["steps"].as_array_mut().unwrap() {
        for j in s["joints"].as_array_mut().into_iter().flatten() {
            if j["name"] == "shoulder_pan_joint" {
                let q = j["position"].as_f64().unwrap() - std::f64::consts::PI;
                j["position"] = json!(q);
            }
        }
    }
    let mut kept = old.clone();
    crate::program::migrate(&mut kept, 2);
    assert_eq!(kept, old, "a file of this layout is as it is");
    crate::program::migrate(&mut old, 1);
    for (a, b) in pan(&old).iter().zip(pan(&now)) {
        let d = (a - b).rem_euclid(std::f64::consts::TAU);
        assert!(
            d < 1e-9 || std::f64::consts::TAU - d < 1e-9,
            "{a} against {b}"
        );
        assert!(a.abs() < std::f64::consts::TAU, "{a}");
    }
}

/// Writes the M1 tending program wired for URSim: its gripper on digital
/// output 0 and the machine's commands on 2–6, its pallet points counted
/// in the script, twice round — the second time at the pallets' next
/// places — and where the tool should be at the end of each move. Run by
/// `tools/ursim/check.py --io`, not with the tests.
#[test]
#[ignore = "writes target/ursim for tools/ursim/check.py --io"]
fn ursim_wired_program() {
    let mut c = cell1();
    tending(&mut c);
    let (infeed, outfeed) = (c.infeed, c.outfeed);
    for i in [0, 1, 3] {
        edit_step(&mut c, i, "pallet", infeed);
    }
    for i in [23, 24, 26] {
        edit_step(&mut c, i, "pallet", outfeed);
    }
    c.act(json!({"type": "next", "pallet": infeed.to_string()}));
    c.act(json!({"type": "next", "pallet": outfeed.to_string()}));
    // Twice round, one after the other.
    let once = c.posing.program(c.robot).unwrap().steps.clone();
    let program = {
        let mut p = c.posing.program(c.robot).unwrap().clone();
        p.steps.extend(once.clone());
        p
    };
    // The gripper and the machine wired; its signals left for a machine
    // URSim does not have.
    let mut io = c.cell.export(&c.doc);
    io.io.insert((c.grip_a, "close".into()), 0);
    for (k, command) in ["open", "close", "clamp", "unclamp", "start"]
        .iter()
        .enumerate()
    {
        io.io.insert((c.machine, command.to_string()), 2 + k as u32);
    }
    let kin = kynex_kin::Kinematics::of(&c.doc, c.robot).unwrap();
    let (script, _) = program.urscript(c.robot, &c.doc, &kin, true, &io).unwrap();
    assert!(
        script.contains("set_standard_digital_out(0, True)"),
        "{script}"
    );
    assert!(
        script.contains("kynex_place_0 = (kynex_place_0 + 1) % 20"),
        "{script}"
    );

    // Where the tool ends each move, in the controller's base frame: a
    // pallet point at the place counted to, as a run takes it.
    let tool = program.tool(&kin).unwrap();
    let base = kin
        .world_of(
            &kin.current,
            program::names(&c.doc)
                .iter()
                .find(|(u, n)| *n == "base" && kin.contains(**u))
                .map(|(u, _)| *u)
                .unwrap(),
        )
        .unwrap();
    let mut placed: std::collections::HashMap<u64, usize> = Default::default();
    let mut ends = Vec::new();
    for step in &program.steps {
        match step {
            Step::Move(p) => {
                let mut at = kin.pose(&kin.with(&p.positions), &tool).translation;
                if let Some(pallet) = p.pallet {
                    let slots = &io.pallets[&pallet];
                    let from = (0..slots.len())
                        .min_by(|a, b| {
                            let d = |i: usize| (slots[i] - at).truncate().length();
                            d(*a).total_cmp(&d(*b))
                        })
                        .unwrap();
                    at += slots[placed.get(&pallet).copied().unwrap_or(0)] - slots[from];
                }
                let b = base.inverse().transform_point3(at);
                ends.push(json!([b.x, b.y, b.z]));
            }
            Step::Do(Action::Next { pallet }) => *placed.entry(*pallet).or_default() += 1,
            _ => {}
        }
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ursim");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("m1_io.script"), script).unwrap();
    std::fs::write(
        dir.join("m1_io.json"),
        serde_json::to_string_pretty(&json!({
            "program": "M1 machine tending, UR10e, wired, twice round",
            "ends": ends,
            "gripper_output": 0,
        }))
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn a_name_cannot_put_script_into_an_export() {
    let mut c = cell1();
    tending(&mut c);
    let infeed = c.infeed;
    for i in [0, 1, 3] {
        edit_step(&mut c, i, "pallet", infeed);
    }
    c.act(json!({"type": "next", "pallet": infeed.to_string()}));
    // The pallet renamed, as a session's participant could: a line break
    // and a move of its own.
    let id = c
        .doc
        .object_by_uid(kreate_scene::Uid::from_raw(infeed))
        .unwrap();
    c.doc.object_mut(id).unwrap().name = "pallet\nmovej([0,0,0,0,0,0])\n#".into();
    let program = c.posing.program(c.robot).unwrap().clone();
    let kin = kynex_kin::Kinematics::of(&c.doc, c.robot).unwrap();
    let io = c.cell.export(&c.doc);
    let (script, _) = program.urscript(c.robot, &c.doc, &kin, false, &io).unwrap();
    assert!(
        !script
            .lines()
            .any(|l| l.trim_start().starts_with("movej([0,0,0,0,0,0])")),
        "{script}"
    );
    assert!(script.contains("# The places of pallet?movej"), "{script}");
}

#[test]
fn a_routine_is_called_times_over_renamed_and_saved() {
    let mut c = cell1();
    let r = c.robot.to_string();
    let ask = |c: &mut Cell1, req: serde_json::Value| {
        let mut req = req;
        req["robot"] = json!(r);
        ask_posing(&mut c.posing, &c.doc, req)
    };
    // The main program: a point, then a call; the routine: a pause and a
    // point of its own.
    let seat = c.seat();
    c.teach(seat + DVec3::new(0.0, -0.6, 0.15), "joint");
    let a = ask(&mut c, json!({"op": "routine_new", "name": "쉬기"}));
    assert_eq!(a["active"], 1, "{a}");
    c.act(json!({"type": "delay", "seconds": 1.0}));
    c.teach(seat + DVec3::new(0.2, -0.6, 0.15), "joint");
    assert_eq!(
        ask(&mut c, json!({"op": "routine_new", "name": "쉬기"}))["ok"],
        false,
        "names are its own"
    );
    ask(&mut c, json!({"op": "routine_select", "index": 0}));
    c.act(json!({"type": "call", "routine": "쉬기", "times": 2}));

    // Run: the routine twice over, in a program of its own steps.
    let (flat, origin) = c.posing.runnable(c.robot).unwrap();
    assert_eq!(flat.steps.len(), 1 + 2 * 2);
    assert_eq!(origin[3], ("쉬기".to_string(), 0));
    let mut runner = Runner::new(c.robot, flat, &c.doc, 0.0).unwrap();
    runner.origin = origin;
    let start = Instant::now();
    let mut t = 0.0;
    let mut seen = false;
    while t < 30.0 && !runner.stopped() {
        runner.tick(
            t,
            start + Duration::from_secs_f64(t),
            &mut c.posing,
            &mut c.cell,
            &c.doc,
        );
        seen |= runner.to_json(t)["routine"] == "쉬기";
        t += 0.02;
    }
    assert_eq!(runner.to_json(t)["state"], "done");
    assert!(seen, "the run says when it is in the routine");
    // Two pauses of a second: two seconds and more.
    assert!(t > 2.0, "{t}");

    // Renamed, its call follows; the main one stays.
    ask(&mut c, json!({"op": "routine_select", "index": 1}));
    assert_eq!(
        ask(&mut c, json!({"op": "routine_rename", "name": "대기"}))["ok"],
        true
    );
    ask(&mut c, json!({"op": "routine_select", "index": 0}));
    assert_eq!(ask(&mut c, json!({"op": "routine_delete"}))["ok"], false);
    let main = c.posing.program(c.robot).unwrap();
    assert!(
        matches!(&main.steps[1], Step::Do(Action::Call { routine, times: 2 }) if routine == "대기")
    );

    // Saved and read back: both, in order, by name.
    let saved = c.posing.saved_programs(&c.doc);
    assert_eq!(saved.len(), 2);
    let mut again = Posing::from_saved(saved);
    again.adopt(&c.doc);
    let (flat, _) = again.runnable(c.robot).unwrap();
    assert_eq!(flat.steps.len(), 5);

    // A routine that calls itself goes nowhere.
    ask(&mut c, json!({"op": "routine_select", "index": 1}));
    c.act(json!({"type": "call", "routine": "대기", "times": 1}));
    let why = c.posing.runnable(c.robot).unwrap_err();
    assert!(why.contains("스스로"), "{why}");
}

#[test]
fn repeats_within_repeats_are_held_to_a_size() {
    use crate::program::Program;
    // Each routine calls the next a thousand times: a thousand to the
    // sixth steps, were they all made.
    let mut all: Vec<Program> = (0..6)
        .map(|k| {
            let mut p = Program::named(&format!("r{k}"), None, DVec3::ZERO);
            p.steps.push(if k < 5 {
                Step::Do(Action::Call {
                    routine: format!("r{}", k + 1),
                    times: 1000,
                })
            } else {
                Step::Do(Action::Delay { seconds: 1.0 })
            });
            p
        })
        .collect();
    all[0].name = "main".into();
    all[0].steps = vec![Step::Do(Action::Call {
        routine: "r1".into(),
        times: 1000,
    })];
    let why = all[0].flatten(&all).unwrap_err();
    assert!(why.contains("100000"), "{why}");
}

#[test]
fn repeats_of_empty_routines_are_held_to_a_size_too() {
    use crate::program::Program;
    // Routines with no steps of their own, calling each other a thousand
    // times over: nothing made, but a thousand to the sixth calls.
    let mut all: Vec<Program> = (0..6)
        .map(|k| {
            let mut p = Program::named(&format!("r{k}"), None, DVec3::ZERO);
            if k < 5 {
                p.steps.push(Step::Do(Action::Call {
                    routine: format!("r{}", k + 1),
                    times: 1000,
                }));
            }
            p
        })
        .collect();
    all[0].name = "main".into();
    all[0].steps = vec![Step::Do(Action::Call {
        routine: "r1".into(),
        times: 1000,
    })];
    let started = std::time::Instant::now();
    let why = all[0].flatten(&all).unwrap_err();
    assert!(why.contains("100000"), "{why}");
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
}

/// The step last added to the robot's program worked on.
fn last_step(c: &Cell1) -> usize {
    c.posing.program(c.robot).unwrap().steps.len() - 1
}

/// Takes the next raw part from the infeed with grip_a: over it, down,
/// grip, up, and the infeed's count on. Each point is on the pallet.
fn pick_raw(c: &mut Cell1) {
    let (a, infeed) = (c.grip_a, c.infeed);
    let up = DVec3::Z * 0.15;
    let raw = c.slot(infeed, 0);
    c.teach_with(raw + up, "joint", a);
    edit_step(c, last_step(c), "pallet", infeed);
    c.teach_with(raw, "linear", a);
    edit_step(c, last_step(c), "pallet", infeed);
    c.act(json!({"type": "grip", "gripper": a.to_string()}));
    c.teach_with(raw + up, "linear", a);
    edit_step(c, last_step(c), "pallet", infeed);
    c.act(json!({"type": "next", "pallet": infeed.to_string()}));
}

/// Machine `machine`'s seat, a point above it, and one backed out of its
/// door towards the robot, far enough for a double gripper to turn clear
/// of the machine's front.
fn at_machine(c: &Cell1, machine: u64) -> (DVec3, DVec3, DVec3) {
    let seat = c.seat_of(machine);
    let up = DVec3::Z * 0.15;
    let back = DVec3::new(-seat.x, -seat.y, 0.0).normalize() * 0.8 + up;
    (seat, seat + up, seat + back)
}

/// Clamps, shuts and starts `machine` from outside its door.
fn shut_and_start(c: &mut Cell1, machine: u64) {
    let m = machine.to_string();
    c.act(json!({"type": "command", "machine": m, "command": "clamp"}));
    c.act(json!({"type": "command", "machine": m, "command": "close"}));
    c.act(json!({"type": "wait", "machine": m, "signal": "door_closed"}));
    c.act(json!({"type": "command", "machine": m, "command": "start"}));
}

/// Loads an empty `machine` with the raw part on grip_a and starts it.
fn load_first(c: &mut Cell1, machine: u64) {
    let (a, m) = (c.grip_a, machine.to_string());
    let (seat, over, back) = at_machine(c, machine);
    c.act(json!({"type": "command", "machine": m, "command": "open"}));
    c.act(json!({"type": "wait", "machine": m, "signal": "door_open"}));
    // Before the door by its joints, then in along a line: an arm swung
    // round inside sweeps the machine's walls.
    c.teach_with(back, "joint", a);
    c.teach_with(over, "linear", a);
    c.teach_with(seat, "linear", a);
    c.act(json!({"type": "release", "gripper": a.to_string()}));
    c.teach_with(over, "linear", a);
    c.teach_with(back, "linear", a);
    shut_and_start(c, machine);
}

/// The double gripper's swap at `machine`, a raw part on grip_a: the
/// finished part out with grip_b, the raw one in with grip_a, the machine
/// started; the finished part to the outfeed's next place, and the next
/// raw part from the infeed. On its way to the machine it passes `via`,
/// if given: the side the arm swings round by.
fn swap(c: &mut Cell1, machine: u64, via: Option<DVec3>) {
    let (a, b, m) = (c.grip_a, c.grip_b, machine.to_string());
    let (seat, over, back) = at_machine(c, machine);
    let finished = seat + DVec3::Z * ((crate::cell::FINISHED.1 - crate::cell::RAW.1) / 2.0);
    c.act(json!({"type": "wait", "machine": m, "signal": "done"}));
    c.act(json!({"type": "command", "machine": m, "command": "open"}));
    c.act(json!({"type": "wait", "machine": m, "signal": "door_open"}));
    c.act(json!({"type": "command", "machine": m, "command": "unclamp"}));
    // Out with grip_b: above the place before the door, down to it, then
    // in along a line...
    if let Some(via) = via {
        c.teach_with(via, "joint", b);
    }
    c.teach_with(back + DVec3::Z * 0.25, "joint", b);
    c.teach_with(back, "linear", b);
    c.teach_with(over, "linear", b);
    c.teach_with(finished, "linear", b);
    c.act(json!({"type": "grip", "gripper": b.to_string()}));
    c.teach_with(over, "linear", b);
    // ...in with grip_a.
    c.teach_with(over, "linear", a);
    c.teach_with(seat, "linear", a);
    c.act(json!({"type": "release", "gripper": a.to_string()}));
    c.teach_with(over, "linear", a);
    c.teach_with(back, "linear", a);
    shut_and_start(c, machine);
    // The finished part out to the outfeed.
    let (outfeed, up) = (c.outfeed, DVec3::Z * 0.15);
    let out = c.slot(outfeed, 0);
    c.teach_with(out + up, "joint", b);
    edit_step(c, last_step(c), "pallet", outfeed);
    c.teach_with(out, "linear", b);
    edit_step(c, last_step(c), "pallet", outfeed);
    c.act(json!({"type": "release", "gripper": b.to_string()}));
    c.teach_with(out + up, "linear", b);
    edit_step(c, last_step(c), "pallet", outfeed);
    c.act(json!({"type": "next", "pallet": outfeed.to_string()}));
    pick_raw(c);
}

/// Stage 1's cell (docs/roadmap-vs-visual-components.md §5): two CNC
/// machines of 90 s, a double gripper. The main program loads both, then
/// calls the swap at each, `times` over.
fn stage_one(times: u32) -> Cell1 {
    stage_one_with("ur10e_tending", 0.7, times)
}

/// [`stage_one`], its robot the library's `robot` on a pedestal `height`
/// metres high.
fn stage_one_with(robot: &str, height: f64, times: u32) -> Cell1 {
    let mut c = cell_for(robot, height, 2, 90.0);
    let (m1, m2) = (c.machines[0], c.machines[1]);
    pick_raw(&mut c);
    load_first(&mut c, m1);
    pick_raw(&mut c);
    load_first(&mut c, m2);
    pick_raw(&mut c);
    let r = c.robot.to_string();
    let a = ask_posing(
        &mut c.posing,
        &c.doc,
        json!({"op": "routine_new", "robot": r, "name": "교체"}),
    );
    assert_eq!(a["ok"], true, "{a}");
    swap(&mut c, m1, None);
    // From the infeed on the left to the machine on the right, round by the
    // outfeed's side: not across the first machine's front.
    swap(&mut c, m2, Some(DVec3::new(0.35, -0.45, 1.35)));
    ask_posing(
        &mut c.posing,
        &c.doc,
        json!({"op": "routine_select", "robot": r, "index": 0}),
    );
    c.act(json!({"type": "call", "routine": "교체", "times": times}));
    c
}

#[test]
fn stage_ones_cell_runs_half_an_hour_with_both_machines_busy() {
    let mut c = stage_one(1000);
    // Its paths, the main program's and the swap's, run into nothing and
    // come near nothing.
    for index in [1, 0] {
        ask_posing(
            &mut c.posing,
            &c.doc,
            json!({"op": "routine_select", "robot": c.robot.to_string(), "index": index}),
        );
        let programs: serde_json::Value = serde_json::from_str(&c.posing.programs_json(
            &c.doc,
            &mut Collider::default(),
            &c.cell,
        ))
        .unwrap();
        let problems: Vec<&serde_json::Value> = programs[c.robot.to_string()]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|s| s["move"]["problems"].as_array().into_iter().flatten())
            .collect();
        assert!(problems.is_empty(), "{problems:?}");
    }
    // Half an hour, as a scenario: the main program round, the cell kept
    // supplied.
    let saved = crate::scenario::snapshot(
        "1단계 셀",
        c.posing.saved_programs(&c.doc),
        c.cell.to_saved(),
        c.library.to_saved(),
    );
    let started = std::time::Instant::now();
    let r = crate::scenario::simulate(&Document::new(), &saved, 1800.0);
    println!("{r}\n{:?}", started.elapsed());
    assert!(r["alarm"].is_null(), "{r}");
    assert_eq!(r["machines"], 2);
    // Two machines of 90 s, each idle only while it is swapped.
    for m in r["machine_lanes"].as_array().unwrap() {
        let u = m["utilization"].as_f64().unwrap();
        assert!(u > 0.7, "{r}");
    }
    let per_hour = r["per_hour"].as_f64().unwrap();
    assert!(per_hour > 55.0 && per_hour < 80.0, "{r}");
}

/// Writes stage 1's cell for URSim: its main program with the swap twice
/// over, wired (the grippers on digital outputs 0 and 1, the first
/// machine's commands on 2–6 and the second's on the configurable 0–4),
/// its moves numbered, how long KYNEX plans each, and where the tool should
/// be at the end of each. Run by `tools/ursim/check.py --name stage1`, and
/// with `--io`, not with the tests.
#[test]
#[ignore = "writes target/ursim for tools/ursim/check.py --name stage1"]
fn ursim_stage_one() {
    let c = stage_one(2);
    stage_one_files(&c, &kynex_kin::urscript::Ur, "ursim", 1);
    // The same cell for an ABB controller.
    let (flat, _) = c.posing.runnable(c.robot).unwrap();
    let kin = kynex_kin::Kinematics::of(&c.doc, c.robot).unwrap();
    let (module, _) = flat
        .export(
            c.robot,
            &c.doc,
            &kin,
            true,
            &stage_one_io(&c),
            &kynex_kin::rapid::Abb,
        )
        .unwrap();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ursim");
    std::fs::write(dir.join("stage1.mod"), module).unwrap();
}

#[test]
#[ignore = "writes target/doosan for tools/doosan/check.py --name stage1"]
fn doosan_stage_one() {
    // Its arm shorter than a UR10e's to a tool held down, the M1013 stands
    // higher, over the machines' seats.
    let c = stage_one_with("m1013_tending", 1.2, 2);
    stage_one_files(&c, &kynex_kin::drl::Doosan, "doosan", 1);
}

/// Stage 1's cell wired: grip_a closes on output 0, grip_b on 1; the first
/// machine's commands on 2–6, the second's on 8–12; no inputs, so waits go
/// as notes and the program runs on a controller with nothing attached.
fn stage_one_io(c: &Cell1) -> crate::program::CellIo {
    let mut io = c.cell.export(&c.doc);
    io.io.insert((c.grip_a, "close".into()), 0);
    io.io.insert((c.grip_b, "close".into()), 1);
    for (k, command) in ["open", "close", "clamp", "unclamp", "start"]
        .iter()
        .enumerate()
    {
        io.io
            .insert((c.machines[0], command.to_string()), 2 + k as u32);
        io.io
            .insert((c.machines[1], command.to_string()), 8 + k as u32);
    }
    io
}

/// Stage 1's program for the controller `writer` writes for, with its
/// markers, in `target/<dir>/stage1.<extension>`, and beside it what KYNEX
/// plans: each move's start and length, and where the tool ends each move
/// in the controller's base frame (`stage1.json`, for the checks in
/// `tools/`). The gripper closes on output `gripper_output`.
fn stage_one_files(
    c: &Cell1,
    writer: &dyn kynex_kin::post::Writer,
    dir: &str,
    gripper_output: u32,
) {
    let (flat, _) = c.posing.runnable(c.robot).unwrap();
    let io = stage_one_io(c);
    let kin = kynex_kin::Kinematics::of(&c.doc, c.robot).unwrap();
    let (script, _) = flat
        .export(c.robot, &c.doc, &kin, true, &io, writer)
        .unwrap();
    let plan = flat.plan(&kin).unwrap();
    let names: Vec<&str> = flat.moves().map(|(_, p)| p.name.as_str()).collect();
    let moves: Vec<serde_json::Value> = plan
        .segments
        .iter()
        .map(|s| {
            json!({
                "name": names[s.to],
                "kind": crate::program::kind_tag(s.kind),
                "start": s.start,
                "duration": s.duration,
            })
        })
        .collect();

    // Where the tool ends each move, in the controller's base frame: a
    // pallet point at the place counted to, from the place it was taught
    // over (nearest the tool point or a grip point, as the export finds it).
    let tool = flat.tool(&kin).unwrap();
    let names_of = program::names(&c.doc);
    let base_uid = names_of
        .iter()
        .find(|(u, n)| *n == "base" && kin.contains(**u))
        .map(|(u, _)| *u)
        .unwrap();
    let base = kin.world_of(&kin.current, base_uid).unwrap();
    let mut placed: std::collections::HashMap<u64, usize> = Default::default();
    let mut ends = Vec::new();
    for step in &flat.steps {
        match step {
            Step::Move(p) => {
                let q = kin.with(&p.positions);
                let mut at = kin.pose(&q, &tool).translation;
                if let Some(pallet) = p.pallet {
                    let slots = &io.pallets[&pallet];
                    let points: Vec<DVec3> = std::iter::once(at)
                        .chain(io.grippers.iter().map(|g| {
                            let t = kin.tool(*g, kreate_math::DAffine3::IDENTITY).unwrap();
                            kin.pose(&q, &t).translation
                        }))
                        .collect();
                    let gap = |i: usize| {
                        points
                            .iter()
                            .map(|a| (slots[i] - *a).truncate().length())
                            .fold(f64::INFINITY, f64::min)
                    };
                    let from = (0..slots.len())
                        .min_by(|a, b| gap(*a).total_cmp(&gap(*b)))
                        .unwrap();
                    at += slots[placed.get(&pallet).copied().unwrap_or(0)] - slots[from];
                }
                let b = base.inverse().transform_point3(at);
                ends.push(json!([b.x, b.y, b.z]));
            }
            Step::Do(Action::Next { pallet }) => *placed.entry(*pallet).or_default() += 1,
            _ => {}
        }
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join(dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("stage1.{}", writer.extension())), script).unwrap();
    std::fs::write(
        dir.join("stage1.json"),
        serde_json::to_string_pretty(&json!({
            "program": "Stage 1's cell: two CNC machines, a double gripper, the swap twice over",
            "moves": moves,
            "duration": plan.duration,
            "ends": ends,
            "gripper_output": gripper_output,
        }))
        .unwrap(),
    )
    .unwrap();
}

/// The programs exported as they were when the export was taken apart
/// into a vendor's writer over a program any vendor reads
/// (`kynex_kin::post`): the M1 program timed, the M1 program wired, and
/// stage 1's cell; stage 1's cell for ABB, as RAPID; and stage 1's cell on
/// a Doosan M1013, as DRL. Kept in `golden/`; an export that writes them
/// otherwise fails here. When a change to them is meant, write them again
/// with the ignored `ursim_*` and `doosan_*` tests and copy them from
/// `target/ursim` and `target/doosan`.
#[test]
fn the_exports_are_as_they_were() {
    ursim_program();
    ursim_wired_program();
    ursim_stage_one();
    doosan_stage_one();
    let target = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    for (name, golden) in [
        ("ursim/m1.script", include_str!("golden/m1.script")),
        ("ursim/m1_io.script", include_str!("golden/m1_io.script")),
        ("ursim/stage1.script", include_str!("golden/stage1.script")),
        ("ursim/stage1.mod", include_str!("golden/stage1.mod")),
        ("doosan/stage1.drl", include_str!("golden/stage1.drl")),
    ] {
        let made = std::fs::read_to_string(target.join(name)).unwrap();
        let (made, golden) = (made.replace("\r\n", "\n"), golden.replace("\r\n", "\n"));
        if made != golden {
            let at = made
                .lines()
                .zip(golden.lines())
                .position(|(a, b)| a != b)
                .unwrap_or(made.lines().count().min(golden.lines().count()));
            panic!(
                "{name}: line {} differs\n  made:   {:?}\n  golden: {:?}",
                at + 1,
                made.lines().nth(at),
                golden.lines().nth(at)
            );
        }
    }
}

/// A positioner turned by the robot's program: the run waits the 2 s it
/// takes to turn half round, it ends turned, and the program exports as
/// `MoveExtJ` for ABB, its positioner external axis a, and as a note for a
/// controller with no axis to turn it.
#[test]
fn a_program_turns_a_positioner_and_waits_for_it() {
    let mut c = cell_for("irb1600_tending", 0.7, 1, 10.0);
    c.library.handle(
        &json!({"op": "place", "template": "positioner", "at": [0.0, -1.3, 0.0]}).to_string(),
    );
    c.doc = Document::new();
    c.library.apply(&mut c.doc);
    let (table, name, kind) = c
        .library
        .roles()
        .into_iter()
        .find(|r| r.2 == crate::cell::Kind::Positioner)
        .unwrap();
    c.cell.ensure(table, name, kind);
    let raw = c.slot(c.infeed, 0) + DVec3::Z * 0.15;
    c.teach(raw, "joint");
    c.act(json!({"type": "turn", "positioner": table.to_string(), "degrees": 180.0}));
    c.teach(raw + DVec3::Z * 0.1, "linear");

    let (runner, t) = c.run(30.0);
    assert!(runner.stopped());
    let (flat, _) = c.posing.runnable(c.robot).unwrap();
    let kin = kynex_kin::Kinematics::of(&c.doc, c.robot).unwrap();
    let moving = flat.plan(&kin).unwrap().duration;
    assert!(t > moving + 2.0 - 0.05, "{t} {moving}");
    let shown: serde_json::Value = serde_json::from_str(&c.cell.to_json(&c.doc)).unwrap();
    let angle = shown["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["kind"] == "positioner")
        .unwrap()["positioner"]["angle"]
        .as_f64()
        .unwrap();
    assert!((angle.abs() - std::f64::consts::PI).abs() < 1e-6, "{angle}");

    let io = c.cell.export(&c.doc);
    let (module, _) = flat
        .export(c.robot, &c.doc, &kin, false, &io, &kynex_kin::rapid::Abb)
        .unwrap();
    assert!(module.contains("! External axis a: positioner"), "{module}");
    assert!(
        module.contains("CONST speeddata kynex_rot90 := [100,500,5000,90];"),
        "{module}"
    );
    assert!(
        module.contains("MoveExtJ [[9E9,9E9,9E9,9E9,9E9,9E9],[180.0000,9E9,9E9,9E9,9E9,9E9]], kynex_rot90, fine;"),
        "{module}"
    );
    let (script, _) = flat
        .export(c.robot, &c.doc, &kin, false, &io, &kynex_kin::urscript::Ur)
        .unwrap();
    assert!(script.contains("# KYNEX: turn positioner"), "{script}");
}

/// A path along an edge of a part, from its mesh: the outfeed plate's near
/// top edge, picked by a point beside it, followed with grip_a pointing
/// into it at 45° from in front and above — in from a clear point, along
/// it at 50 mm/s, and out again.
#[test]
fn a_path_follows_an_edge_picked_on_a_part() {
    let mut c = cell1();
    let (r, t) = (c.robot.to_string(), c.grip_a.to_string());
    let plate = c.outfeed.to_string();
    let near = DVec3::new(0.0, -0.57, 0.785);
    let shown = ask_posing(
        &mut c.posing,
        &c.doc,
        json!({"op": "edge", "robot": r, "tool": t, "object": plate, "point": near.to_array()}),
    );
    assert_eq!(shown["ok"], true, "{shown}");
    assert!(
        (shown["length"].as_f64().unwrap() - 0.55).abs() < 1e-6,
        "{shown}"
    );
    let a = ask_posing(
        &mut c.posing,
        &c.doc,
        json!({"op": "edge_path", "robot": r, "tool": t, "object": plate,
               "point": near.to_array(), "speed": 0.05, "approach": 0.05}),
    );
    assert_eq!(a["ok"], true, "{a}");
    assert_eq!(a["added"], 4, "{a}");
    assert_eq!(a["missed"], 0, "{a}");

    let program = c.posing.program(c.robot).unwrap().clone();
    let kin = kynex_kin::Kinematics::of(&c.doc, c.robot).unwrap();
    let plan = program.plan(&kin).unwrap();
    for s in &plan.segments {
        assert!(s.problems.is_empty(), "{:?}", s.problems);
    }
    // Along the edge, 0.55 m at 50 mm/s.
    let edge = &plan.segments[1];
    assert!((edge.duration - 11.0).abs() < 0.5, "{}", edge.duration);
    // Half way, the tool on the edge, pointing into it.
    let tool = program.tool(&kin).unwrap();
    let q = plan.at(edge.start + edge.duration / 2.0);
    let pose = kin.pose(&q, &tool);
    let edge_y = pose.translation.y;
    assert!(pose.translation.x.abs() < 0.01, "{}", pose.translation);
    let into = pose.matrix3 * DVec3::Z;
    assert!(
        (into - DVec3::new(0.0, -1.0, -1.0).normalize()).length() < 0.01,
        "{into} at y {edge_y}"
    );
}

/// M2 (docs/roadmap-vs-visual-components.md §10): a machine tending and
/// deburring cell laid out from the catalogue alone — stage 1's cell, and
/// beside it an ABB IRB 1600 with a deburring spindle on a pedestal, the
/// near top edge of a part on a stand followed with the spindle — every
/// path free of contact, the tending program written for Universal Robots
/// and the deburring one for ABB. Writes the project to `target/m2` for
/// the viewer to open and record (`kynex view --empty --project
/// target/m2/kynex-project.json`).
#[test]
fn m2_a_tending_and_deburring_cell_from_the_catalogue() {
    let mut c = stage_one(2);
    let place = |library: &mut Library, req: serde_json::Value| {
        let (a, _) = library.handle(&req.to_string());
        let a: serde_json::Value = serde_json::from_str(&a).unwrap();
        assert_eq!(a["ok"], true, "{a}");
        a["item"].as_u64().unwrap()
    };
    place(
        &mut c.library,
        json!({"op": "place", "template": "pedestal", "at": [-3.2, 0.0, 0.0]}),
    );
    let (a, _) = c.library.handle(
        &json!({"op": "place", "template": "irb1600_tending", "at": [-3.15, 0.05, 0.0],
                "params": {"tool": 4}, "snap": true})
        .to_string(),
    );
    assert!(a.contains("로봇 받침대"), "snapped onto its pedestal: {a}");
    // The part: a 0.5 m block on a stand, 0.6 m up.
    let part = place(
        &mut c.library,
        json!({"op": "place", "template": "pedestal", "at": [-2.0, 0.0, 0.0],
               "params": {"height": 0.6, "size": 0.5}}),
    );
    let mut doc = Document::new();
    c.library.apply(&mut doc);
    for (uid, name, kind) in c.library.roles() {
        c.cell.ensure(uid, name, kind);
    }
    c.doc = doc;
    let listed: serde_json::Value = serde_json::from_str(&c.library.to_json()).unwrap();
    let part_root = listed["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == part)
        .unwrap()["root"]
        .as_str()
        .unwrap()
        .to_string();
    let abb = kynex_model::robots(&c.doc)
        .into_iter()
        .find(|r| r.vendor() == Some("abb"))
        .unwrap()
        .root;
    let tcp = c
        .doc
        .objects()
        .find(|(id, o)| o.name == "tcp" && c.doc.robot_root(*id).is_some())
        .and_then(|(id, _)| c.doc.object_uid(id))
        .unwrap()
        .raw();

    // The near top edge, deburred at 30 mm/s.
    let a = ask_posing(
        &mut c.posing,
        &c.doc,
        json!({"op": "edge_path", "robot": abb.to_string(), "tool": tcp.to_string(),
               "object": part_root, "point": [-2.26, 0.0, 0.61], "speed": 0.03, "approach": 0.05}),
    );
    assert_eq!(a["ok"], true, "{a}");
    assert_eq!(a["missed"], 0, "{a}");
    assert!((a["length"].as_f64().unwrap() - 0.5).abs() < 1e-6, "{a}");

    // Every path of both robots touches nothing and comes near nothing but
    // where its tool works.
    let programs: serde_json::Value = serde_json::from_str(&c.posing.programs_json(
        &c.doc,
        &mut Collider::default(),
        &c.cell,
    ))
    .unwrap();
    for robot in [c.robot, abb] {
        let problems: Vec<&serde_json::Value> = programs[robot.to_string()]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|s| s["move"]["problems"].as_array().into_iter().flatten())
            .collect();
        assert!(problems.is_empty(), "{robot}: {problems:?}");
    }

    // Two vendors' programs.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/m2");
    std::fs::create_dir_all(&dir).unwrap();
    let io = stage_one_io(&c);
    for (robot, writer, file) in [
        (
            c.robot,
            &kynex_kin::urscript::Ur as &dyn kynex_kin::post::Writer,
            "tending.script",
        ),
        (abb, &kynex_kin::rapid::Abb, "deburring.mod"),
    ] {
        let (flat, _) = c.posing.runnable(robot).unwrap();
        let kin = kynex_kin::Kinematics::of(&c.doc, robot).unwrap();
        let (script, _) = flat
            .export(robot, &c.doc, &kin, false, &io, writer)
            .unwrap();
        std::fs::write(dir.join(file), &script).unwrap();
        let plan = flat.plan(&kin).unwrap();
        let cycle = plan.segments.last().map_or(0.0, |s| s.start + s.duration);
        println!(
            "{file}: {} lines, moves {:.1} s",
            script.lines().count(),
            cycle
        );
    }
    let deburr = std::fs::read_to_string(dir.join("deburring.mod")).unwrap();
    assert!(
        deburr.contains("MoveL") && deburr.contains("kynex_v30"),
        "{deburr}"
    );

    // The project, for the viewer.
    let project = json!({
        "version": crate::program::FILE_VERSION,
        "programs": c.posing.saved_programs(&c.doc),
        "cell": c.cell.to_saved(),
        "library": c.library.to_saved(),
        "scenarios": [],
    });
    std::fs::write(
        dir.join("kynex-project.json"),
        serde_json::to_string_pretty(&project).unwrap(),
    )
    .unwrap();
}
