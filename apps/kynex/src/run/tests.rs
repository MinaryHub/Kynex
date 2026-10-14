use kreate_math::DVec3;
use serde_json::json;

use super::*;
use crate::collide::Collider;
use crate::library::Library;

/// The M1 cell from the library: a tending UR10e on a 0.7 m pedestal, a
/// CNC lathe ahead of it, an infeed table on its left and an outfeed
/// behind. The machine's cycle is 10 s here.
struct Cell1 {
    doc: Document,
    cell: Cell,
    posing: Posing,
    robot: u64,
    grip_a: u64,
    machine: u64,
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
    let mut library = Library::default();
    for (template, at) in [
        ("pedestal", [0.0, 0.0, 0.0]),
        ("ur10e_tending", [0.0, 0.0, 0.7]),
        ("cnc_lathe", [0.0, 0.75, 0.0]),
        ("infeed_station", [-0.8, 0.05, 0.0]),
        ("outfeed_station", [0.0, -0.8, 0.0]),
    ] {
        library.handle(&json!({"op": "place", "template": template, "at": at}).to_string());
    }
    let mut doc = Document::new();
    library.apply(&mut doc);
    let mut cell = Cell::default();
    let mut machine = 0;
    let (mut infeed, mut outfeed, mut grip_a) = (0, 0, 0);
    for (uid, name, kind) in library.roles() {
        match kind {
            crate::cell::Kind::Cnc => machine = uid,
            crate::cell::Kind::Infeed => infeed = uid,
            crate::cell::Kind::Outfeed => outfeed = uid,
            crate::cell::Kind::Gripper if name == "grip_a" => grip_a = uid,
            _ => {}
        }
        cell.ensure(uid, name, kind);
    }
    ask_cell(
        &mut cell,
        &doc,
        json!({"op": "role", "object": machine.to_string(), "kind": "cnc", "props": {"cycle": 10.0}}),
    );
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
        robot,
        grip_a,
        machine,
        infeed,
        outfeed,
    }
}

impl Cell1 {
    /// Teaches a point with grip_a at `at`, pointing down.
    fn teach(&mut self, at: DVec3, kind: &str) {
        let (r, t) = (self.robot.to_string(), self.grip_a.to_string());
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
        let cell: serde_json::Value = serde_json::from_str(&self.cell.to_json(&self.doc)).unwrap();
        let c = cell["components"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| uid_of(&c["object"]) == Some(self.machine))
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
        let program = self.posing.program(self.robot).unwrap().clone();
        let mut runner = Runner::new(self.robot, program, &self.doc, 0.0).unwrap();
        let start = Instant::now();
        let mut t = 0.0;
        while t < limit && !runner.stopped() {
            let now = start + Duration::from_secs_f64(t);
            runner.tick(t, now, &mut self.posing, &mut self.cell, &self.doc);
            self.cell.tick(&self.doc, t, now);
            t += 0.02;
        }
        (runner, t)
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
