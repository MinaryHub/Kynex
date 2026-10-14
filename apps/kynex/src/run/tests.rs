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
    let mut library = Library::default();
    let mut placed = vec![
        ("pedestal", [0.0, 0.0, 0.0], 0.0),
        ("ur10e_tending", [0.0, 0.0, 0.7], 0.0),
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
    let mut doc = Document::new();
    library.apply(&mut doc);
    let mut cell = Cell::default();
    let mut machines = Vec::new();
    let (mut infeed, mut outfeed, mut grip_a) = (0, 0, 0);
    for (uid, name, kind) in library.roles() {
        match kind {
            crate::cell::Kind::Cnc => machines.push(uid),
            crate::cell::Kind::Infeed => infeed = uid,
            crate::cell::Kind::Outfeed => outfeed = uid,
            crate::cell::Kind::Gripper if name == "grip_a" => grip_a = uid,
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
        machine: machines[0],
        machines,
        infeed,
        outfeed,
    }
}

impl Cell1 {
    /// Teaches a point with grip_a at `at`, pointing down.
    fn teach(&mut self, at: DVec3, kind: &str) {
        let (r, t) = (self.robot.to_string(), self.grip_a.to_string());
        // Into a joint move the arm first turns at its base to face the
        // point, as someone teaching it would: from there the solver keeps
        // the elbow up rather than folding the arm round the other way.
        if kind == "joint" {
            let kin = kynex_kin::Kinematics::of(&self.doc, self.robot).unwrap();
            let tool = kin
                .tool(self.grip_a, kreate_math::DAffine3::IDENTITY)
                .unwrap();
            let q = self.posing.positions(&kin, self.robot);
            let base = kin.world_of(&q, self.robot).unwrap().translation;
            let now = kin.pose(&q, &tool).translation - base;
            let want = at - base;
            let turn = (want.y.atan2(want.x) - now.y.atan2(now.x) + std::f64::consts::PI)
                .rem_euclid(std::f64::consts::TAU)
                - std::f64::consts::PI;
            let pan = kin
                .vars
                .iter()
                .position(|v| v.name == "shoulder_pan_joint")
                .unwrap();
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
                .find(|(_, n)| *n == "base")
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
