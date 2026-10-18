use kreate_math::DAffine3;
use kynex_kin::Kinematics;

use super::*;

fn ask(library: &mut Library, req: Value) -> (Value, RoleChanges) {
    let (answer, roles) = library.handle(&req.to_string());
    (serde_json::from_str(&answer).unwrap(), roles)
}

fn shown(library: &mut Library) -> (Document, Changes) {
    let mut doc = Document::new();
    let changes = library.apply(&mut doc);
    (doc, changes)
}

fn named(doc: &Document, name: &str) -> ObjectId {
    doc.objects()
        .find(|(_, o)| o.name == name)
        .map(|(id, _)| id)
        .unwrap_or_else(|| panic!("no {name}"))
}

#[test]
fn a_cnc_machine_comes_with_its_role_and_stands_where_placed() {
    let mut library = Library::default();
    let (a, roles) = ask(
        &mut library,
        json!({"op": "place", "template": "cnc_lathe", "at": [2.0, 0.0, 0.0], "yaw": std::f64::consts::FRAC_PI_2}),
    );
    assert_eq!(a["ok"], true, "{a}");
    assert_eq!(roles.added.len(), 1);
    let (uid, name, kind) = roles.added[0].clone();
    assert_eq!((name.as_str(), kind), ("CNC 선반 1", Kind::Cnc));
    assert_eq!(uid, EQUIPMENT + PER_ITEM);

    let (doc, changes) = shown(&mut library);
    assert!(changes
        .changed
        .contains(&(DatablockKind::Object, Uid::from_raw(uid))));
    // Turned a quarter round, the machine's front (−Y) faces +X, so its
    // chuck, 0.35 m in from the front, lies 0.35 m along −X from x = 2.
    let chuck = doc.world_transform(named(&doc, "chuck")).translation;
    assert!(
        (chuck - DVec3::new(2.0 - 0.35, 0.0, 0.91)).length() < 1e-9,
        "{chuck}"
    );
    // Its parts hang under the item's root, under its identity.
    let root = doc.object_by_uid(Uid::from_raw(uid)).unwrap();
    assert_eq!(doc.object(root).unwrap().name, "CNC 선반 1");
}

#[test]
fn a_robot_from_the_library_is_a_robot() {
    let mut library = Library::default();
    let (_, roles) = ask(
        &mut library,
        json!({"op": "place", "template": "ur10e_tending", "at": [0.0, 0.0, 0.7]}),
    );
    // Its two grip points.
    let grips: Vec<_> = roles
        .added
        .iter()
        .filter(|r| r.2 == Kind::Gripper)
        .map(|r| r.1.clone())
        .collect();
    assert_eq!(grips, vec!["grip_a", "grip_b"]);
    let (doc, _) = shown(&mut library);
    let robots = kynex_model::robots(&doc);
    assert_eq!(robots.len(), 1);
    assert_eq!(robots[0].name, "UR10e 1", "named after the item");
    let kin = Kinematics::of(&doc, robots[0].root).unwrap();
    assert_eq!(kin.vars.len(), 6);
    // Its base link stands where the item was placed, 0.7 m up.
    let base_link = kin.uids().next().unwrap();
    let base = kin.world_of(&kin.current, base_link).unwrap().translation;
    assert!((base.z - 0.7).abs() < 1e-9, "{base}");
}

#[test]
fn moving_and_resizing_keep_identities_and_say_what_went() {
    let mut library = Library::default();
    ask(&mut library, json!({"op": "place", "template": "fence"}));
    let (doc, _) = shown(&mut library);
    let before = doc.object_count();
    // Moved: the same objects, elsewhere.
    ask(
        &mut library,
        json!({"op": "move", "item": 1, "at": [1.0, 2.0, 0.0]}),
    );
    let (moved, changes) = shown(&mut library);
    assert_eq!(moved.object_count(), before);
    assert!(changes.removed.is_empty());
    // Made smaller: fewer posts, and those that went are said to have gone.
    ask(
        &mut library,
        json!({"op": "resize", "item": 1, "params": {"length": 1.2, "width": 1.2}}),
    );
    let (small, changes) = shown(&mut library);
    assert!(small.object_count() < before);
    assert_eq!(changes.removed.len(), before - small.object_count());
    // Taken away: everything it had goes.
    let (_, roles) = ask(&mut library, json!({"op": "remove", "item": 1}));
    assert!(roles.added.is_empty());
    let (empty, changes) = shown(&mut library);
    assert_eq!(empty.object_count(), 0);
    assert_eq!(changes.removed.len(), small.object_count());
}

#[test]
fn placed_equipment_is_saved_and_comes_back_the_same() {
    let mut library = Library::default();
    ask(
        &mut library,
        json!({"op": "place", "template": "infeed_station", "at": [-0.8, 0.0, 0.0], "params": {"height": 0.9}}),
    );
    ask(&mut library, json!({"op": "place", "template": "pedestal"}));
    assert!(library.take_items_changed());
    let (doc, _) = shown(&mut library);
    let mut again = Library::from_saved(&library.to_saved());
    let (doc2, _) = shown(&mut again);
    let uids = |d: &Document| {
        let mut v: Vec<u64> = d
            .objects()
            .filter_map(|(id, _)| d.object_uid(id))
            .map(|u| u.raw())
            .collect();
        v.sort();
        v
    };
    assert_eq!(uids(&doc), uids(&doc2));
    let plate = doc2.world_transform(named(&doc2, "pallet")).translation;
    assert!(
        (plate - DVec3::new(-0.8, 0.0, 0.89)).length() < 1e-9,
        "{plate}"
    );
    // The next one placed is numbered on.
    let (a, _) = ask(&mut again, json!({"op": "place", "template": "pedestal"}));
    assert_eq!(a["item"], 3);
}

#[test]
fn an_unknown_template_or_item_is_refused() {
    let mut library = Library::default();
    let (a, _) = ask(
        &mut library,
        json!({"op": "place", "template": "teleporter"}),
    );
    assert_eq!(a["ok"], false);
    let (a, _) = ask(&mut library, json!({"op": "move", "item": 9}));
    assert_eq!(a["ok"], false);
}

/// The ABB IRB 1600-10/1.45 as the controller has it: its joints, limits and
/// top speeds as the product specification gives them, and its flange
/// (`tool0`) where the controller puts it — at the calibration position
/// [[815, 0, 1186.5], [0.707107, 0, 0.707107, 0]], 150 + 600 + 65 mm out and
/// 486.5 + 700 mm up, pointing forward — and moving the controller's way.
#[test]
fn the_abb_robot_moves_as_its_controller_has_it() {
    let mut library = Library::default();
    let (_, roles) = ask(
        &mut library,
        json!({"op": "place", "template": "irb1600_tending", "at": [1.0, 2.0, 0.5], "yaw": 0.7}),
    );
    assert_eq!(
        roles.added.iter().filter(|r| r.2 == Kind::Gripper).count(),
        2
    );
    let (doc, _) = shown(&mut library);
    let robots = kynex_model::robots(&doc);
    let kin = Kinematics::of(&doc, robots[0].root).unwrap();
    let names: Vec<&str> = kin.vars.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(
        names,
        ["joint_1", "joint_2", "joint_3", "joint_4", "joint_5", "joint_6"]
    );
    for (var, (lower, upper, speed)) in kin.vars.iter().zip([
        (-180.0, 180.0, 150.0),
        (-63.0, 136.0, 160.0),
        (-235.0, 55.0, 170.0),
        (-200.0, 200.0, 320.0),
        (-115.0, 115.0, 400.0),
        (-400.0, 400.0, 460.0),
    ]) {
        let deg = |r: f64| r.to_degrees();
        assert!((deg(var.lower) - lower).abs() < 0.01, "{}", var.name);
        assert!((deg(var.upper) - upper).abs() < 0.01, "{}", var.name);
        assert!((deg(var.velocity) - speed).abs() < 0.01, "{}", var.name);
    }

    let uid = |n: &str| doc.object_uid(named(&doc, n)).unwrap().raw();
    let flange = kin.tool(uid("tool0"), DAffine3::IDENTITY).unwrap();
    // The flange in the base frame, millimetres, at joint positions in degrees.
    let at = |degrees: [f64; 6]| {
        let q: Vec<f64> = degrees.iter().map(|d| d.to_radians()).collect();
        let base = kin.world_of(&q, uid("base")).unwrap();
        let pose = base.inverse() * kin.pose(&q, &flange);
        (pose.translation * 1000.0, DQuat::from_affine3(&pose))
    };
    let near = |a: DVec3, b: [f64; 3]| (a - DVec3::from(b)).length() < 0.01;

    let (home, turned) = at([0.0; 6]);
    assert!(near(home, [815.0, 0.0, 1186.5]), "{home}");
    let want = DQuat::from_xyzw(
        0.0,
        std::f64::consts::FRAC_1_SQRT_2,
        0.0,
        std::f64::consts::FRAC_1_SQRT_2,
    );
    assert!(turned.dot(want).abs() > 1.0 - 1e-6, "{turned}");
    // Axis 2 forward: the upper arm level, the forearm hanging down.
    let (p, _) = at([0.0, 90.0, 0.0, 0.0, 0.0, 0.0]);
    assert!(near(p, [850.0, 0.0, -178.5]), "{p}");
    // Axis 3 back: the forearm upright.
    let (p, _) = at([0.0, 0.0, -90.0, 0.0, 0.0, 0.0]);
    assert!(near(p, [150.0, 0.0, 1851.5]), "{p}");
    // Axis 1 to the left: the arm swings round to +Y.
    let (p, _) = at([90.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    assert!(near(p, [0.0, 815.0, 1186.5]), "{p}");
    // Axis 5 down: the flange faces the floor, as the robot waits.
    let (_, turned) = at([0.0, 0.0, 0.0, 0.0, 90.0, 0.0]);
    assert!((turned * DVec3::Z - DVec3::NEG_Z).length() < 1e-6);
    let rest: Vec<f64> = kin.current.iter().map(|q| q.to_degrees()).collect();
    assert!((rest[4] - 90.0).abs() < 0.01, "{rest:?}");
}

/// A program for the ABB robot, as RAPID: the calibration position written
/// as the controller writes it, and a point over a table, the tool down,
/// reached and written by its joints.
#[test]
fn the_abb_robot_s_program_reads_as_its_controller_would_write_it() {
    use kynex_kin::path::{MoveKind, Waypoint};
    use kynex_kin::post::{Export, Move, Writer};

    let mut library = Library::default();
    ask(
        &mut library,
        json!({"op": "place", "template": "irb1600_tending", "at": [0.0, 0.0, 0.0]}),
    );
    let (doc, _) = shown(&mut library);
    let kin = Kinematics::of(&doc, kynex_model::robots(&doc)[0].root).unwrap();
    let uid = |n: &str| doc.object_uid(named(&doc, n)).unwrap().raw();
    let flange = kin.tool(uid("tool0"), DAffine3::IDENTITY).unwrap();

    // Over a table 0.8 m out and 0.3 m to the side, the flange facing down.
    let goal = kynex_kin::Goal {
        position: DVec3::new(0.8, 0.3, 0.6),
        rotation: Some(DQuat::from_rotation_y(std::f64::consts::PI)),
    };
    let solved = kin.solve(&flange, &goal, &kin.current, &Default::default());
    assert!(solved.reached, "{solved:?}");

    let moves = [
        vec![0.0; 6],
        solved.positions.clone(),
        solved.positions.clone(),
    ];
    let waypoints: Vec<Waypoint> = moves
        .iter()
        .zip([MoveKind::Linear, MoveKind::Joint, MoveKind::Linear])
        .map(|(q, kind)| Waypoint {
            kind,
            positions: q.clone(),
            speed: 0.25,
            blend: 0.0,
            via: None,
        })
        .collect();
    let module = kynex_kin::rapid::Abb
        .write(
            &kin,
            &flange,
            &Export {
                name: "irb",
                moves: waypoints
                    .iter()
                    .enumerate()
                    .map(|(k, w)| Move {
                        name: ["home", "table", "again"][k],
                        waypoint: w,
                        before: Vec::new(),
                        place: None,
                    })
                    .collect(),
                close: false,
                base: uid("base"),
                flange: uid("tool0"),
                trailing: Vec::new(),
                pallets: Vec::new(),
                markers: false,
            },
        )
        .unwrap();
    // The tool is the flange itself.
    assert!(
        module.contains(
            "kynex_tool := [TRUE,[[0.00,0.00,0.00],[1.000000,0.000000,0.000000,0.000000]]"
        ),
        "{module}"
    );
    assert!(
        module.contains(
            "CONST robtarget p1 := [[815.00,0.00,1186.50],[0.707107,0.000000,0.707107,0.000000]"
        ),
        "{module}"
    );
    // The flange down, as ABB writes it: [0,0,1,0], to the solver's
    // tolerance.
    let p3 = module
        .lines()
        .find_map(|l| l.trim().strip_prefix("CONST robtarget p3 := [["))
        .unwrap();
    let numbers: Vec<f64> = p3
        .split(['[', ']', ','])
        .filter_map(|x| x.parse().ok())
        .take(7)
        .collect();
    for (got, want) in numbers
        .iter()
        .zip([800.0, 300.0, 600.0, 0.0, 0.0, 1.0, 0.0])
    {
        assert!((got - want).abs() < 1e-3, "{p3}");
    }
    let degrees: Vec<String> = solved
        .positions
        .iter()
        .map(|q| format!("{:.4}", q.to_degrees()))
        .collect();
    assert!(
        module.contains(&format!("CONST jointtarget j2 := [[{}]", degrees.join(","))),
        "{module}"
    );
}

/// Each robot of the library says whose controller it is, so a program is
/// offered in that controller's language only.
#[test]
fn the_library_s_robots_say_whose_they_are() {
    for (template, vendor) in [
        ("ur10e_tending", Some("ur")),
        ("ur5e", Some("ur")),
        ("irb1600_tending", Some("abb")),
        ("m1013_tending", Some("doosan")),
    ] {
        let mut library = Library::default();
        ask(&mut library, json!({"op": "place", "template": template}));
        let (doc, _) = shown(&mut library);
        assert_eq!(kynex_model::robots(&doc)[0].vendor(), vendor, "{template}");
    }
}

/// The Doosan M1013 as Doosan's description has it: its joints, limits and
/// top speeds, upright at zero with its flange 1452.5 mm up and 34.5 mm to
/// the side, and at rest with the forearm level and the tool down.
#[test]
fn the_doosan_robot_moves_as_its_description_has_it() {
    let mut library = Library::default();
    ask(
        &mut library,
        json!({"op": "place", "template": "m1013_tending", "at": [1.0, 2.0, 0.5], "yaw": 0.7}),
    );
    let (doc, _) = shown(&mut library);
    let kin = Kinematics::of(&doc, kynex_model::robots(&doc)[0].root).unwrap();
    for (var, (limit, speed)) in kin.vars.iter().zip([
        (360.0, 120.0),
        (360.0, 120.0),
        (160.0, 180.0),
        (360.0, 225.0),
        (360.0, 225.0),
        (360.0, 225.0),
    ]) {
        assert!(
            (var.upper.to_degrees() - limit).abs() < 0.01,
            "{}",
            var.name
        );
        assert!(
            (var.lower.to_degrees() + limit).abs() < 0.01,
            "{}",
            var.name
        );
        assert!(
            (var.velocity.to_degrees() - speed).abs() < 0.01,
            "{}",
            var.name
        );
    }
    let uid = |n: &str| doc.object_uid(named(&doc, n)).unwrap().raw();
    let flange = kin.tool(uid("tool0"), DAffine3::IDENTITY).unwrap();
    let at = |degrees: [f64; 6]| {
        let q: Vec<f64> = degrees.iter().map(|d| d.to_radians()).collect();
        let base = kin.world_of(&q, uid("base")).unwrap();
        let pose = base.inverse() * kin.pose(&q, &flange);
        (pose.translation * 1000.0, pose.matrix3 * DVec3::Z)
    };
    let near = |a: DVec3, b: [f64; 3]| (a - DVec3::from(b)).length() < 0.01;
    let (p, z) = at([0.0; 6]);
    assert!(
        near(p, [0.0, 34.5, 1452.5]) && (z - DVec3::Z).length() < 1e-6,
        "{p}"
    );
    let (p, z) = at([0.0, 0.0, 90.0, 0.0, 90.0, 0.0]);
    assert!(near(p, [559.0, 34.5, 651.5]), "{p}");
    assert!((z - DVec3::NEG_Z).length() < 1e-6, "{z}");
    let rest: Vec<f64> = kin.current.iter().map(|q| q.to_degrees()).collect();
    assert_eq!(
        rest.iter().map(|d| d.round()).collect::<Vec<_>>(),
        [0.0, 0.0, 90.0, 0.0, 90.0, 0.0]
    );
}

/// Where the IRB 1600's flange reaches, as the page asks for it: at the
/// most 150, 700, 600 and 65 mm laid end to end (its 1.45 m to the wrist,
/// and the flange), drawn from its base frame.
#[test]
fn the_abb_robot_s_reach_is_its_arm_laid_out() {
    let mut library = Library::default();
    ask(
        &mut library,
        json!({"op": "place", "template": "irb1600_tending", "at": [1.0, 0.0, 0.5]}),
    );
    let (doc, _) = shown(&mut library);
    let root = kynex_model::robots(&doc)[0].root;
    let mut posing = crate::pose::Posing::default();
    let (answer, _) = posing.handle(
        &doc,
        &json!({"op": "reach", "robot": root.to_string()}).to_string(),
    );
    let answer: Value = serde_json::from_str(&answer).unwrap();
    assert_eq!(answer["ok"], true, "{answer}");
    let farthest = answer["farthest"].as_f64().unwrap();
    assert!((farthest - 1.515).abs() < 0.005, "{farthest}");
    // Its base frame where the robot stands.
    let base = answer["base"].as_array().unwrap();
    assert!((base[12].as_f64().unwrap() - 1.0).abs() < 1e-6);
    assert!((base[14].as_f64().unwrap() - 0.5).abs() < 1e-6);
    assert!(answer["section"].as_array().unwrap().len() > 50);
}

/// The IRB 1600 on its track: the track its first axis, a line along the
/// track planned with the track going evenly from end to end as the
/// controller moves it, and its RAPID giving the track as external axis a,
/// its targets in the frame the track stands in. No other controller's
/// language has the track.
#[test]
fn the_abb_robot_on_a_track_moves_the_track_as_its_controller_does() {
    use kynex_kin::path::{plan, MoveKind, Waypoint};
    use kynex_kin::post::{Export, Move, Writer};

    let mut library = Library::default();
    ask(
        &mut library,
        json!({"op": "place", "template": "irb1600_track", "at": [0.0, 0.0, 0.0]}),
    );
    let (doc, _) = shown(&mut library);
    let robot = &kynex_model::robots(&doc)[0];
    assert_eq!(robot.vendor(), Some("abb"));
    let kin = Kinematics::of(&doc, robot.root).unwrap();
    assert_eq!(kin.vars.len(), 7);
    let track = &kin.vars[0];
    assert!(
        track.kind.slides() && track.lower == 0.0 && track.upper == 3.0 && track.velocity == 1.6
    );
    let uid = |n: &str| doc.object_uid(named(&doc, n)).unwrap().raw();
    let flange = kin.tool(uid("tool0"), DAffine3::IDENTITY).unwrap();

    // The tool down 0.9 m above the floor, ahead of the carriage, with the
    // carriage at 0.5 m; then the same a metre on, the carriage at 1.5 m.
    let down = Some(DQuat::from_rotation_y(std::f64::consts::PI));
    let at = |x: f64, track: f64| {
        let mut seed = kin.current.clone();
        seed[0] = track;
        let goal = kynex_kin::Goal {
            position: DVec3::new(x, 0.0, 0.9),
            rotation: down,
        };
        let opts = kynex_kin::Options {
            held: 1,
            ..Default::default()
        };
        let found = kin.solve(&flange, &goal, &seed, &opts);
        assert!(found.reached, "{found:?}");
        assert!((found.positions[0] - track).abs() < 1e-12);
        found.positions
    };
    let (a, b) = (at(1.3, 0.5), at(2.3, 1.5));
    let waypoints = [
        Waypoint {
            kind: MoveKind::Joint,
            positions: a.clone(),
            speed: 0.5,
            blend: 0.0,
            via: None,
        },
        Waypoint {
            kind: MoveKind::Linear,
            positions: b.clone(),
            speed: 0.25,
            blend: 0.0,
            via: None,
        },
    ];
    let planned = plan(&kin, &flange, &waypoints, false);
    let line = &planned.segments[0];
    assert!(line.problems.is_empty(), "{:?}", line.problems);
    // Half way along, the carriage is half way, the tool on the line.
    let middle = planned.at(line.start + line.duration / 2.0);
    assert!((middle[0] - 1.0).abs() < 0.02, "{middle:?}");
    let tool_at = kin.pose(&middle, &flange).translation;
    assert!(
        (tool_at - DVec3::new(1.8, 0.0, 0.9)).length() < 0.002,
        "{tool_at}"
    );

    let export = Export {
        name: "track",
        moves: waypoints
            .iter()
            .enumerate()
            .map(|(k, w)| Move {
                name: ["P1", "P2"][k],
                waypoint: w,
                before: Vec::new(),
                place: None,
            })
            .collect(),
        close: false,
        base: uid("base"),
        flange: uid("tool0"),
        trailing: Vec::new(),
        pallets: Vec::new(),
        markers: false,
    };
    let module = kynex_kin::rapid::Abb.write(&kin, &flange, &export).unwrap();
    assert!(module.contains("! External axis a: track (mm)"), "{module}");
    assert!(
        module.contains("],[500.00,9E9,9E9,9E9,9E9,9E9]];"),
        "{module}"
    );
    // In the track's frame: 2.3 m along it, whatever the carriage's place.
    assert!(
        module.contains("CONST robtarget p2 := [[2300.00,0.00,900.00],[0.000000,0.000000,1.000000,0.000000],[0,0,0,0],[1500.00,9E9,9E9,9E9,9E9,9E9]];"),
        "{module}"
    );
    for writer in [
        &kynex_kin::urscript::Ur as &dyn Writer,
        &kynex_kin::drl::Doosan,
    ] {
        let err = writer.write(&kin, &flange, &export).unwrap_err();
        assert!(err.contains("RAPID"), "{err}");
    }
}

/// Connection points: a robot let go near a pedestal stands on its top; a
/// conveyor let go near another's end runs on from it, in line; one let go
/// far off stays where it was let go.
#[test]
fn equipment_snaps_to_what_it_connects_to() {
    let mut library = Library::default();
    ask(
        &mut library,
        json!({"op": "place", "template": "pedestal", "at": [1.0, 1.0, 0.0]}),
    );
    let (a, _) = ask(
        &mut library,
        json!({"op": "place", "template": "ur10e_tending", "at": [1.1, 0.9, 0.0], "snap": true}),
    );
    assert_eq!(a["snapped"], "로봇 받침대 1", "{a}");
    let robot = library.items.last().unwrap();
    assert!(
        (robot.at - DVec3::new(1.0, 1.0, 0.7)).length() < 1e-9,
        "{}",
        robot.at
    );

    let (a, _) = ask(
        &mut library,
        json!({"op": "place", "template": "ur5e", "at": [1.6, 1.0, 0.0], "snap": true}),
    );
    assert!(a["snapped"].is_null(), "too far: {a}");

    ask(
        &mut library,
        json!({"op": "place", "template": "conveyor", "at": [0.0, -2.0, 0.0]}),
    );
    // Its start some 0.15 m past the first's end, a little turned.
    let (a, _) = ask(
        &mut library,
        json!({"op": "place", "template": "conveyor", "at": [2.2, -1.95, 0.0], "yaw": 0.2, "snap": true}),
    );
    assert_eq!(a["snapped"], "벨트 컨베이어 1", "{a}");
    let second = library.items.last().unwrap();
    assert!(
        (second.at - DVec3::new(2.1, -2.0, 0.0)).length() < 1e-9,
        "{}",
        second.at
    );
    assert!(second.yaw.abs() < 1e-12);

    // Moved off and back, it snaps again; moved without, it does not.
    let id = second.id;
    let (a, _) = ask(
        &mut library,
        json!({"op": "move", "item": id, "at": [2.15, -1.9, 0.0], "snap": false}),
    );
    assert!(a["snapped"].is_null());
    let (a, _) = ask(
        &mut library,
        json!({"op": "move", "item": id, "at": [2.15, -1.9, 0.0], "snap": true}),
    );
    assert_eq!(a["snapped"], "벨트 컨베이어 1", "{a}");
}

/// The UR3e and UR16e as Universal Robots describes them: six joints, their
/// top speeds (180 and 360°/s; 120 and 180°/s), the flange as far out as
/// their links reach, and each a UR.
#[test]
fn the_other_urs_are_as_universal_robots_describes_them() {
    for (template, speeds, reach) in [
        (
            "ur3e",
            [180.0, 180.0, 180.0, 360.0, 360.0, 360.0],
            (0.24355 + 0.2132, 0.13105),
        ),
        (
            "ur16e",
            [120.0, 120.0, 180.0, 180.0, 180.0, 180.0],
            (0.4784 + 0.36, 0.17415),
        ),
        ("ur5e", [180.0; 6], (0.425 + 0.3922, 0.1333)),
    ] {
        let mut library = Library::default();
        ask(&mut library, json!({"op": "place", "template": template}));
        let (doc, _) = shown(&mut library);
        let robot = &kynex_model::robots(&doc)[0];
        assert_eq!(robot.vendor(), Some("ur"), "{template}");
        let kin = Kinematics::of(&doc, robot.root).unwrap();
        for (var, speed) in kin.vars.iter().zip(speeds) {
            assert!(
                (var.velocity.to_degrees() - speed).abs() < 0.01,
                "{template} {}",
                var.name
            );
        }
        // Stretched out level, the wrist's first joint as far from the
        // shoulder's axis as the upper arm and forearm reach, and d4 to
        // the side.
        let uid = |n: &str| doc.object_uid(named(&doc, n)).unwrap().raw();
        let q = [0.0; 6];
        let shoulder = kin.world_of(&q, uid("shoulder_link")).unwrap().translation;
        let wrist = kin.world_of(&q, uid("wrist_1_link")).unwrap().translation;
        let out = DVec3::new(wrist.x - shoulder.x, wrist.y - shoulder.y, 0.0).length();
        assert!(
            (out - f64::hypot(reach.0, reach.1)).abs() < 1e-6,
            "{template}: {out}"
        );
    }
}

/// The fab's glass robot: a column, a lifting carriage and two arms that
/// fold its fork back to turn within 2.6 m — the arms 1.2 and 1.0 m, the
/// fork's middle 1.2 m on — and its sheet out of sight until it carries one.
#[test]
fn the_glass_robot_folds_to_turn_and_carries_its_sheet_hidden() {
    let mut library = Library::default();
    ask(&mut library, json!({"op": "place", "template": "glass_robot"}));
    let (doc, _) = shown(&mut library);
    let robot = &kynex_model::robots(&doc)[0];
    let kin = Kinematics::of(&doc, robot.root).unwrap();
    assert_eq!(kin.vars.len(), 5);
    assert!(matches!(kin.vars[1].kind, kreate_scene::robot::JointKind::Prismatic));
    let uid = |n: &str| doc.object_uid(named(&doc, n)).unwrap().raw();
    let out = |q: &[f64], n: &str| {
        let p = kin.world_of(q, uid(n)).unwrap().translation;
        DVec3::new(p.x, p.y, 0.0).length()
    };
    // Stretched out: 1.2 + 1.0 to the wrist, 1.2 more to the fork's middle.
    assert!((out(&[0.0; 5], "tool0") - 3.4).abs() < 1e-6);
    // Folded as it waits, the fork's middle within reach of its column.
    assert!(out(&kin.current, "tool0") < 1.6, "{}", out(&kin.current, "tool0"));
    let sheet = doc.object(named(&doc, "glass")).unwrap();
    assert!(sheet.transform.scale.max_element() < 1e-3);
}

/// A vertical machining centre: a CNC machine, its vise where its part sits.
#[test]
fn a_machining_centre_holds_its_part_in_its_vise() {
    let mut library = Library::default();
    let (_, roles) = ask(&mut library, json!({"op": "place", "template": "cnc_vmc"}));
    assert_eq!(roles.added.len(), 1);
    assert_eq!(roles.added[0].2, Kind::Cnc);
    let (doc, _) = shown(&mut library);
    let vise = doc.world_transform(named(&doc, "chuck")).translation;
    assert!(
        (vise - DVec3::new(0.0, 0.45, 0.95)).length() < 1e-9,
        "{vise}"
    );
}

/// Equipment of the user's own: a lathe as placed and set up (its cycle
/// 45 s) saved under a name of its own, placed again elsewhere with the
/// same shape and role and settings, kept in the project file, and shown
/// among the templates.
#[test]
fn equipment_saved_from_the_scene_places_again_with_its_role() {
    let mut library = Library::default();
    ask(
        &mut library,
        json!({"op": "place", "template": "cnc_lathe", "at": [1.0, 2.0, 0.0], "yaw": 0.5}),
    );
    let (doc, _) = shown(&mut library);
    let mut cell = crate::cell::Cell::default();
    for (uid, name, kind) in library.roles() {
        cell.ensure(uid, name, kind);
    }
    let machine = library.items[0].root_uid();
    cell.set_props(machine, &json!({"cycle": 45.0}));
    let root = doc.object_by_uid(Uid::from_raw(machine)).unwrap();
    let roles = cell.roles_under(&doc, root);
    assert_eq!(roles.len(), 1);
    let key = library
        .save_custom(&doc, machine, "내 선반", &roles)
        .unwrap();

    // Placed again, a metre on, not turned.
    let (a, added) = ask(
        &mut library,
        json!({"op": "place", "template": key, "at": [4.0, 0.0, 0.0]}),
    );
    assert_eq!(a["ok"], true, "{a}");
    assert_eq!(added.added.len(), 1);
    assert_eq!(added.added[0].1, "내 선반 1");
    assert_eq!(added.added[0].2, Kind::Cnc);
    assert_eq!(added.props.len(), 1);
    assert_eq!(added.props[0].1["cycle"], 45.0);
    let (doc, _) = shown(&mut library);
    let chucks: Vec<DVec3> = doc
        .objects()
        .filter(|(_, o)| o.name == "chuck")
        .map(|(id, _)| doc.world_transform(id).translation)
        .collect();
    assert_eq!(chucks.len(), 2);
    // Where the first stands from its origin, turned back by its 0.5.
    let first = DQuat::from_rotation_z(-0.5) * (chucks[0] - DVec3::new(1.0, 2.0, 0.0));
    let second = chucks[1] - DVec3::new(4.0, 0.0, 0.0);
    assert!((first - second).length() < 1e-6, "{first} {second}");

    // Kept, and read back.
    let saved = library.to_saved();
    let again = Library::from_saved(&saved);
    assert_eq!(again.items.len(), 2);
    assert_eq!(again.customs.len(), 1);
    let mut again = again;
    assert!(again.thumbnails().contains(&key));
    let listed: Value = serde_json::from_str(&again.to_json()).unwrap();
    assert!(listed["templates"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["key"] == key && t["group"] == "내 장비"));
    // Not taken out while one of it stands.
    let (a, _) = ask(&mut again, json!({"op": "delete_custom", "key": key}));
    assert_eq!(a["ok"], false);
}

/// Equipment of the user's own with a connector: a stand whose top is
/// marked where a robot stands. Placed turned, a robot let go near it
/// stands on that point.
#[test]
fn a_connector_on_the_user_s_equipment_takes_a_robot() {
    let mut doc = Document::new();
    let stand = doc.add_mesh_object("stand", kreate_mesh::primitives::cube(1.0));
    {
        let o = doc.object_mut(stand).unwrap();
        o.transform.scale = DVec3::new(0.5, 0.5, 0.9);
        o.transform.translation = DVec3::new(2.0, 0.0, 0.45);
    }
    let uid = doc.object_uid(stand).unwrap().raw();
    // Its top's centre, a little off to its side, in its own frame.
    let top = DVec3::new(0.2, 0.0, 0.5);
    let roles = vec![(
        stand,
        Kind::Connector,
        json!({"plug": 0, "point": [top.x, top.y, top.z]}),
    )];
    let mut library = Library::default();
    let key = library.save_custom(&doc, uid, "받침", &roles).unwrap();

    ask(
        &mut library,
        json!({"op": "place", "template": key, "at": [5.0, 0.0, 0.0], "yaw": std::f64::consts::FRAC_PI_2}),
    );
    // The point: 0.2 × 0.5 m out along the stand's X, turned to Y; 0.9 m up.
    let point = DVec3::new(5.0, 0.1, 0.9);
    let (a, _) = ask(
        &mut library,
        json!({"op": "place", "template": "ur5e", "at": [5.1, 0.2, 0.0], "snap": true}),
    );
    assert_eq!(a["snapped"], "받침 1", "{a}");
    let robot = library.items.last().unwrap();
    assert!((robot.at - point).length() < 1e-9, "{}", robot.at);
}

/// Each tool of the catalogue on a UR5e's flange: grippers come with their
/// grip points as grippers, process tools with their working point as the
/// robot's tool — the torch's bent 22° off the flange's axis, its wire's
/// tip 295 mm out along it from the neck.
#[test]
fn a_robot_carries_the_tool_it_is_given() {
    for (n, grips, tcp) in [
        (0, 0, "tool0"),
        (1, 1, "tool0"),
        (2, 2, "tool0"),
        (3, 1, "tool0"),
        (4, 0, "tcp"),
        (5, 0, "tcp"),
    ] {
        let mut library = Library::default();
        let (a, roles) = ask(
            &mut library,
            json!({"op": "place", "template": "ur5e", "params": {"tool": n}}),
        );
        assert_eq!(a["ok"], true, "{a}");
        let grippers = roles.added.iter().filter(|r| r.2 == Kind::Gripper).count();
        assert_eq!(grippers, grips, "{}", TOOLS[n]);
        let (doc, _) = shown(&mut library);
        let robot = &kynex_model::robots(&doc)[0];
        let tool = doc
            .object_by_uid(Uid::from_raw(robot.tool.unwrap()))
            .unwrap();
        assert_eq!(doc.object(tool).unwrap().name, tcp, "{}", TOOLS[n]);
        if TOOLS[n] == "용접 토치" {
            let flange = doc.world_transform(named(&doc, "tool0"));
            let tip = doc.world_transform(tool);
            let (fz, tz) = (flange.matrix3.z_axis, tip.matrix3.z_axis);
            assert!((fz.angle_between(tz).to_degrees() - 22.0).abs() < 0.05);
            let neck = flange.transform_point3(DVec3::new(0.0, 0.0, 0.12));
            assert!(((tip.translation - neck).length() - 0.295).abs() < 1e-3);
        }
    }
    // Changed on a robot placed: made again with the other.
    let mut library = Library::default();
    ask(
        &mut library,
        json!({"op": "place", "template": "ur10e_tending"}),
    );
    assert_eq!(library.roles().len(), 2);
    ask(
        &mut library,
        json!({"op": "resize", "item": 1, "params": {"tool": 1}}),
    );
    assert_eq!(library.roles().len(), 1);
}

/// A tool changed, the grip point it had is no longer there: its identity,
/// now another object's, is said to have lost its role.
#[test]
fn a_tool_changed_says_which_roles_went_stale() {
    let mut library = Library::default();
    ask(
        &mut library,
        json!({"op": "place", "template": "ur10e_tending"}),
    );
    let grips: Vec<u64> = library.roles().iter().map(|r| r.0).collect();
    let (_, roles) = ask(
        &mut library,
        json!({"op": "resize", "item": 1, "params": {"tool": 4}}),
    );
    for g in &grips {
        assert!(roles.stale.contains(g), "{g} {:?}", roles.stale);
    }
    // Sized again with nothing changed: nothing stale.
    let (_, roles) = ask(
        &mut library,
        json!({"op": "resize", "item": 1, "params": {"tool": 4}}),
    );
    assert!(roles.stale.is_empty());
}

/// The FANUC LR Mate 200iD/7L as ROS-Industrial describes it: FANUC's, its
/// top speeds the datasheet's, and with its joints at zero its faceplate
/// 550 mm out and 475 mm up from the world frame (at J2's height), facing
/// out along X.
#[test]
fn the_fanuc_robot_is_as_its_description_has_it() {
    let mut library = Library::default();
    ask(
        &mut library,
        json!({"op": "place", "template": "lrmate200id7l_tending"}),
    );
    let (doc, _) = shown(&mut library);
    let robot = &kynex_model::robots(&doc)[0];
    assert_eq!(robot.vendor(), Some("fanuc"));
    let kin = Kinematics::of(&doc, robot.root).unwrap();
    for (var, speed) in kin
        .vars
        .iter()
        .zip([370.0, 310.0, 410.0, 550.0, 545.0, 1000.0])
    {
        assert!(
            (var.velocity.to_degrees() - speed).abs() < 0.01,
            "{}",
            var.name
        );
    }
    let uid = |n: &str| doc.object_uid(named(&doc, n)).unwrap().raw();
    let q = [0.0; 6];
    let base = kin.world_of(&q, uid("base")).unwrap();
    let face = base.inverse() * kin.world_of(&q, uid("tool0")).unwrap();
    assert!(
        (face.translation - DVec3::new(0.55, 0.0, 0.475)).length() < 1e-6,
        "{}",
        face.translation
    );
    assert!((face.matrix3.z_axis - DVec3::X).length() < 1e-6);
}

/// Two items copied at once beside themselves: same template, size and
/// turn, new names, their roles paired with the originals'; moved
/// together; removed together; a name freed is used again.
#[test]
fn items_are_copied_moved_and_removed_together() {
    let mut library = Library::default();
    let (a, _) = ask(
        &mut library,
        json!({"op": "place", "template": "cnc_lathe", "at": [1.0, 0.0, 0.0], "yaw": 0.5}),
    );
    let first = a["item"].as_u64().unwrap();
    ask(
        &mut library,
        json!({"op": "resize", "item": first, "params": {"width": 2.2}}),
    );
    let (b, _) = ask(
        &mut library,
        json!({"op": "place", "template": "fence", "at": [3.0, 0.0, 0.0]}),
    );
    let second = b["item"].as_u64().unwrap();

    let (copied, roles) = ask(
        &mut library,
        json!({"op": "duplicate", "items": [first, second], "offset": [0.0, 2.0, 0.0]}),
    );
    assert_eq!(copied["ok"], true, "{copied}");
    let made: Vec<u64> = copied["made"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["item"].as_u64().unwrap())
        .collect();
    assert_eq!(made.len(), 2);
    let copy = library
        .items()
        .iter()
        .find(|i| i.id == made[0])
        .unwrap()
        .clone();
    let original = library
        .items()
        .iter()
        .find(|i| i.id == first)
        .unwrap()
        .clone();
    assert_eq!(copy.name, "CNC 선반 2");
    assert_eq!(
        (copy.template.as_str(), copy.yaw, copy.params.clone()),
        (
            original.template.as_str(),
            original.yaw,
            original.params.clone()
        )
    );
    assert!((copy.at - DVec3::new(1.0, 2.0, 0.0)).length() < 1e-12);
    // The lathe's role, paired: original's root to the copy's.
    assert!(
        roles
            .copied
            .contains(&(original.root_uid(), copy.root_uid())),
        "{:?}",
        roles.copied
    );
    assert!(roles
        .added
        .iter()
        .any(|(uid, _, _)| *uid == copy.root_uid()));

    let (moved, _) = ask(
        &mut library,
        json!({"op": "move_many", "moves": [
            {"item": made[0], "at": [5.0, 5.0, 0.0], "yaw": 1.0},
            {"item": made[1], "at": [6.0, 5.0, 0.0]},
        ]}),
    );
    assert_eq!(moved["ok"], true, "{moved}");
    let at = |l: &Library, id: u64| l.items().iter().find(|i| i.id == id).unwrap().at;
    assert!((at(&library, made[1]) - DVec3::new(6.0, 5.0, 0.0)).length() < 1e-12);
    // One unknown: none moved.
    let (bad, _) = ask(
        &mut library,
        json!({"op": "move_many", "moves": [
            {"item": made[0], "at": [9.0, 9.0, 0.0]}, {"item": 999, "at": [0.0, 0.0, 0.0]},
        ]}),
    );
    assert_eq!(bad["ok"], false);
    assert!((at(&library, made[0]) - DVec3::new(5.0, 5.0, 0.0)).length() < 1e-12);

    let (removed, roles) = ask(
        &mut library,
        json!({"op": "remove", "items": [first, made[1]]}),
    );
    assert_eq!(removed["ok"], true, "{removed}");
    assert_eq!(library.items().len(), 2);
    let _ = roles;
    // "CNC 선반 1" is free again: the next lathe takes it, not a second "2".
    let (c, _) = ask(
        &mut library,
        json!({"op": "place", "template": "cnc_lathe", "at": [0.0, 0.0, 0.0]}),
    );
    let lathe = library
        .items()
        .iter()
        .find(|i| i.id == c["item"].as_u64().unwrap())
        .unwrap();
    assert_eq!(lathe.name, "CNC 선반 1");
    let (twice, _) = ask(
        &mut library,
        json!({"op": "duplicate", "items": [second, second]}),
    );
    assert_eq!(twice["ok"], false);
}

/// A robot hung upside down: its root turned half round about X, saved
/// and read back so; tilted, it snaps onto nothing; a tilt not a number
/// is refused.
#[test]
fn equipment_is_tilted_about_x_and_y() {
    let mut library = Library::default();
    let (a, _) = ask(
        &mut library,
        json!({"op": "place", "template": "ur5e", "at": [0.0, 0.0, 2.5]}),
    );
    let id = a["item"].as_u64().unwrap();
    let (moved, _) = ask(
        &mut library,
        json!({"op": "move", "item": id, "roll": std::f64::consts::PI, "snap": true}),
    );
    assert_eq!(moved["ok"], true, "{moved}");
    assert_eq!(moved["snapped"], Value::Null);
    let (doc, _) = shown(&mut library);
    let root = library.items()[0].root_uid();
    let root = doc.object_by_uid(Uid::from_raw(root)).unwrap();
    let up = doc.world_transform(root).transform_vector3(DVec3::Z);
    assert!((up - DVec3::NEG_Z).length() < 1e-9, "{up}");

    let saved = library.to_saved();
    let back = Library::from_saved(&saved);
    assert!((back.items()[0].roll.abs() - std::f64::consts::PI).abs() < 1e-12);

    let (bad, _) = ask(
        &mut library,
        json!({"op": "move", "item": id, "pitch": "steil"}),
    );
    assert_eq!(bad["ok"], false);
    let (upright, _) = ask(&mut library, json!({"op": "move", "item": id, "roll": 0.0}));
    assert_eq!(upright["ok"], true);
    assert!(library.items()[0].upright());
}

/// A robot hung upside down still reaches: its base points down, and a
/// point 10 cm below its tool, in the world, is solved for.
#[test]
fn a_robot_hung_upside_down_reaches_below_it() {
    let mut library = Library::default();
    let (a, _) = ask(
        &mut library,
        json!({"op": "place", "template": "ur5e", "at": [0.0, 0.0, 2.5]}),
    );
    let id = a["item"].as_u64().unwrap();
    ask(
        &mut library,
        json!({"op": "move", "item": id, "roll": std::f64::consts::PI}),
    );
    let (doc, _) = shown(&mut library);
    let robots = kynex_model::robots(&doc);
    let kin = Kinematics::of(&doc, robots[0].root).unwrap();
    // Its tool end: what lies farthest from its base.
    let base = DVec3::new(0.0, 0.0, 2.5);
    let tip = kin
        .uids()
        .max_by(|a, b| {
            let d = |u: u64| (kin.world_of(&kin.current, u).unwrap().translation - base).length();
            d(*a).total_cmp(&d(*b))
        })
        .unwrap();
    let tool = kin.tool(tip, DAffine3::IDENTITY).unwrap();
    let now = kin.world_of(&kin.current, tip).unwrap();
    // Hung under 2.5 m, its tool is below the base.
    assert!(now.translation.z < 2.5, "{}", now.translation);
    let goal = kynex_kin::Goal {
        position: now.translation - DVec3::Z * 0.1,
        rotation: None,
    };
    let found = kin.solve(&tool, &goal, &kin.current, &kynex_kin::Options::default());
    assert!(found.reached, "{found:?}");
    let got = kin.world_of(&found.positions, tip).unwrap().translation;
    assert!((got - goal.position).length() < 1e-3, "{got}");
}

/// Copies placed one by one (an array, a mirrored copy); an item locked
/// stays put until let go; an item renamed, its role with it, and not to
/// a name another has.
#[test]
fn copies_are_placed_items_locked_and_renamed() {
    let mut library = Library::default();
    let (a, _) = ask(
        &mut library,
        json!({"op": "place", "template": "cnc_lathe", "at": [1.0, 0.0, 0.0], "yaw": 0.3}),
    );
    let lathe = a["item"].as_u64().unwrap();
    let (c, _) = ask(
        &mut library,
        json!({"op": "duplicate", "copies": [
            {"item": lathe, "at": [3.0, 0.0, 0.0]},
            {"item": lathe, "at": [-1.0, 0.0, 0.0], "yaw": 2.8},
        ]}),
    );
    assert_eq!(c["made"].as_array().unwrap().len(), 2, "{c}");
    let yaws: Vec<f64> = library.items().iter().map(|i| i.yaw).collect();
    assert_eq!(yaws, vec![0.3, 0.3, 2.8]);
    let (far, _) = ask(
        &mut library,
        json!({"op": "duplicate", "copies": [{"item": lathe, "at": [1e9, 0.0, 0.0]}]}),
    );
    assert_eq!(far["ok"], false);

    let (l, _) = ask(
        &mut library,
        json!({"op": "lock", "items": [lathe], "locked": true}),
    );
    assert_eq!(l["ok"], true);
    for req in [
        json!({"op": "move", "item": lathe, "at": [0.0, 0.0, 0.0]}),
        json!({"op": "resize", "item": lathe, "params": {"width": 2.0}}),
        json!({"op": "remove", "item": lathe}),
        json!({"op": "remove", "items": [lathe]}),
        json!({"op": "move_many", "moves": [{"item": lathe, "at": [0.0, 0.0, 0.0]}]}),
    ] {
        let (refused, _) = ask(&mut library, req.clone());
        assert_eq!(refused["ok"], false, "{req}");
    }
    assert!(
        Library::from_saved(&library.to_saved()).items()[0].locked,
        "kept locked"
    );
    ask(
        &mut library,
        json!({"op": "lock", "items": [lathe], "locked": false}),
    );
    let (moved, _) = ask(
        &mut library,
        json!({"op": "move", "item": lathe, "at": [0.0, 0.0, 0.0]}),
    );
    assert_eq!(moved["ok"], true);

    let (r, roles) = ask(
        &mut library,
        json!({"op": "rename", "item": lathe, "name": "OP10 선반"}),
    );
    assert_eq!(r["ok"], true, "{r}");
    let root = library.items()[0].root_uid();
    assert!(
        roles.renamed.contains(&(root, "OP10 선반".to_string())),
        "{:?}",
        roles.renamed
    );
    let (doc, _) = shown(&mut library);
    assert_eq!(
        doc.object(doc.object_by_uid(Uid::from_raw(root)).unwrap())
            .unwrap()
            .name,
        "OP10 선반"
    );
    let (taken, _) = ask(
        &mut library,
        json!({"op": "rename", "item": lathe, "name": "CNC 선반 2"}),
    );
    assert_eq!(taken["ok"], false);
    let (empty, _) = ask(
        &mut library,
        json!({"op": "rename", "item": lathe, "name": "  "}),
    );
    assert_eq!(empty["ok"], false);
}
