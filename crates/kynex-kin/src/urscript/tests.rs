use kreate_math::{DAffine3, DVec3};

use super::*;
use crate::path::Waypoint;
use crate::post::identifier;
use crate::tests::{find, load, TRACK, UR5E};

/// The numbers in the first `call(...[...]` of `line`.
fn numbers(line: &str, call: &str) -> Vec<f64> {
    let start = line
        .find(call)
        .unwrap_or_else(|| panic!("no {call} in {line}"))
        + call.len();
    let open = line[start..].find('[').unwrap() + start + 1;
    let close = line[open..].find(']').unwrap() + open;
    line[open..close]
        .split(',')
        .map(|x| x.trim().parse().unwrap())
        .collect()
}

fn wp(kind: MoveKind, positions: [f64; 6], speed: f64) -> Waypoint {
    Waypoint {
        kind,
        positions: positions.to_vec(),
        speed,
        blend: 0.0,
        via: None,
    }
}

#[test]
fn a_ur_program_reads_as_movej_and_movel_in_the_base_frame() {
    let doc = load(UR5E);
    let (_, root) = find(&doc, "base_link");
    let (_, base) = find(&doc, "base");
    let (_, flange) = find(&doc, "tool0");
    let kin = Kinematics::of(&doc, root).unwrap();
    // A gripper 120 mm out from the flange.
    let tool = kin
        .tool(
            flange,
            DAffine3::from_translation(DVec3::new(0.0, 0.0, 0.12)),
        )
        .unwrap();
    let a = wp(MoveKind::Joint, [0.3, -1.2, 1.4, -1.6, -1.5, 0.2], 0.5);
    let b = wp(MoveKind::Linear, [0.5, -1.0, 1.2, -1.7, -1.5, 0.4], 0.25);
    let script = urscript(
        &kin,
        &tool,
        &Export {
            name: "pick place",
            moves: vec![
                Move {
                    name: "P1",
                    waypoint: &a,
                    before: Vec::new(),
                    place: None,
                },
                Move {
                    name: "P2",
                    waypoint: &b,
                    before: Vec::new(),
                    place: None,
                },
            ],
            close: false,
            base,
            flange,
            trailing: Vec::new(),
            markers: false,
            pallets: Vec::new(),
        },
    )
    .unwrap();
    let lines: Vec<&str> = script.lines().collect();
    assert!(lines.contains(&"def pick_place():"), "{script}");
    assert_eq!(lines.last(), Some(&"pick_place()"));
    assert!(!script.contains("while"));

    let tcp = numbers(
        lines.iter().find(|l| l.contains("set_tcp")).unwrap(),
        "set_tcp(p",
    );
    assert!((tcp[2] - 0.12).abs() < 1e-6 && tcp[0].abs() < 1e-9 && tcp[3].abs() < 1e-9);

    let movej = lines.iter().find(|l| l.contains("movej")).unwrap();
    for (got, want) in numbers(movej, "movej(").iter().zip(&a.positions) {
        assert!((got - want).abs() < 1e-6);
    }
    // Half of the slowest joint's 3.14 rad/s.
    assert!(movej.contains("v=1.5700"), "{movej}");

    // The line's end, where the controller's base frame has it: half a turn
    // from base_link, so x and y change sign.
    let movel = lines.iter().find(|l| l.contains("movel")).unwrap();
    let p = numbers(movel, "movel(p");
    let world = kin.pose(&b.positions, &tool).translation;
    assert!((p[0] + world.x).abs() < 1e-6, "{movel} vs {world}");
    assert!((p[1] + world.y).abs() < 1e-6);
    assert!((p[2] - world.z).abs() < 1e-6);
    assert!(movel.ends_with("a=1.2000, v=0.2500)"), "{movel}");
}

#[test]
fn a_closed_program_loops_back_to_its_first_point() {
    let doc = load(UR5E);
    let (_, root) = find(&doc, "base_link");
    let (_, base) = find(&doc, "base");
    let (_, flange) = find(&doc, "tool0");
    let kin = Kinematics::of(&doc, root).unwrap();
    let tool = kin.tool(flange, DAffine3::IDENTITY).unwrap();
    let a = wp(MoveKind::Linear, [0.3, -1.2, 1.4, -1.6, -1.5, 0.2], 0.1);
    let b = wp(MoveKind::Joint, [0.5, -1.0, 1.2, -1.7, -1.5, 0.4], 1.0);
    let script = urscript(
        &kin,
        &tool,
        &Export {
            name: "cycle",
            moves: vec![
                Move {
                    name: "P1",
                    waypoint: &a,
                    before: Vec::new(),
                    place: None,
                },
                Move {
                    name: "P2",
                    waypoint: &b,
                    before: Vec::new(),
                    place: None,
                },
            ],
            close: true,
            base,
            flange,
            trailing: Vec::new(),
            markers: false,
            pallets: Vec::new(),
        },
    )
    .unwrap();
    let body: Vec<&str> = script
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("move") || l.starts_with("while") || *l == "end")
        .collect();
    // To the start by the joints, then round: P2, and back to P1 as P1 is
    // taught, along a line.
    assert_eq!(body.len(), 6, "{script}");
    assert!(body[0].starts_with("movej"));
    assert_eq!(body[1], "while (True):");
    assert!(body[2].starts_with("movej"));
    assert!(body[3].starts_with("movel"));
    assert_eq!(body[4], "end");
}

#[test]
fn only_a_six_joint_arm_has_urscript() {
    let doc = load(TRACK);
    let (_, root) = find(&doc, "track");
    let (_, tip) = find(&doc, "tool");
    let kin = Kinematics::of(&doc, root).unwrap();
    let tool = kin.tool(tip, DAffine3::IDENTITY).unwrap();
    let w = Waypoint {
        kind: MoveKind::Joint,
        positions: kin.current.clone(),
        speed: 0.5,
        blend: 0.0,
        via: None,
    };
    let err = urscript(
        &kin,
        &tool,
        &Export {
            name: "x",
            moves: vec![Move {
                name: "P1",
                waypoint: &w,
                before: Vec::new(),
                place: None,
            }],
            close: false,
            base: root,
            flange: tip,
            trailing: Vec::new(),
            markers: false,
            pallets: Vec::new(),
        },
    )
    .unwrap_err();
    assert!(err.contains("6"), "{err}");
}

#[test]
fn names_become_identifiers_and_comments_stay_ascii() {
    assert_eq!(identifier("pick place-1"), "pick_place_1");
    assert_eq!(identifier("1st"), "kynex_1st");
    assert_eq!(identifier("로봇"), "kynex___");
    assert_eq!(comment("P1 집기"), "P1 ??");
}

#[test]
fn what_cannot_be_said_yet_is_left_as_comments_in_its_place() {
    let doc = load(UR5E);
    let (_, root) = find(&doc, "base_link");
    let (_, base) = find(&doc, "base");
    let (_, flange) = find(&doc, "tool0");
    let kin = Kinematics::of(&doc, root).unwrap();
    let tool = kin.tool(flange, DAffine3::IDENTITY).unwrap();
    let a = wp(MoveKind::Joint, [0.3, -1.2, 1.4, -1.6, -1.5, 0.2], 0.5);
    let b = wp(MoveKind::Joint, [0.5, -1.0, 1.2, -1.7, -1.5, 0.4], 0.5);
    let script = urscript(
        &kin,
        &tool,
        &Export {
            name: "notes",
            moves: vec![
                Move {
                    name: "P1",
                    waypoint: &a,
                    before: Vec::new(),
                    place: None,
                },
                Move {
                    name: "P2",
                    waypoint: &b,
                    before: vec!["close gripper grip_a".into()],
                    place: None,
                },
            ],
            close: false,
            base,
            flange,
            trailing: vec!["wait for machine".into()],
            markers: false,
            pallets: Vec::new(),
        },
    )
    .unwrap();
    let lines: Vec<&str> = script.lines().map(str::trim).collect();
    let note = lines
        .iter()
        .position(|l| *l == "# close gripper grip_a")
        .unwrap();
    assert_eq!(lines[note + 1], "# P2", "before the move it comes before");
    let last = lines
        .iter()
        .position(|l| *l == "# wait for machine")
        .unwrap();
    assert!(
        lines[..last]
            .iter()
            .filter(|l| l.starts_with("movej"))
            .count()
            == 2
    );
}

#[test]
fn what_the_program_does_between_moves_reads_as_urscript() {
    let doc = load(UR5E);
    let (_, root) = find(&doc, "base_link");
    let (_, base) = find(&doc, "base");
    let (_, flange) = find(&doc, "tool0");
    let kin = Kinematics::of(&doc, root).unwrap();
    let tool = kin.tool(flange, DAffine3::IDENTITY).unwrap();
    let a = wp(MoveKind::Joint, [0.3, -1.2, 1.4, -1.6, -1.5, 0.2], 0.5);
    let script = urscript(
        &kin,
        &tool,
        &Export {
            name: "ops",
            moves: vec![Move {
                name: "P1",
                waypoint: &a,
                before: Vec::new(),
                place: None,
            }],
            close: false,
            base,
            flange,
            trailing: vec![
                Op::Set { port: 9, on: true },
                Op::Pulse {
                    port: 16,
                    seconds: 0.2,
                },
                Op::Wait { port: 3, on: false },
                Op::Sleep(1.0),
                Op::Note("hand\nover".into()),
            ],
            pallets: Vec::new(),
            markers: false,
        },
    )
    .unwrap();
    let body: Vec<&str> = script.lines().map(str::trim).collect();
    for line in [
        "set_configurable_digital_out(1, True)",
        "set_tool_digital_out(0, True)",
        "sleep(0.2)",
        "set_tool_digital_out(0, False)",
        "while (get_standard_digital_in(3)):",
        "sync()",
        "sleep(1.00)",
        "# hand?over",
    ] {
        assert!(body.contains(&line), "{line} in\n{script}");
    }
}
