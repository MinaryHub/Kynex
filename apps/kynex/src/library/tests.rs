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
    assert_eq!(
        robots[0].name, "UR10e + 이중 그리퍼 1",
        "named after the item"
    );
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
