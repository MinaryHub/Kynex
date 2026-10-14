use kreate_math::{DAffine3, DVec3};

use super::*;
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
                },
                Move {
                    name: "P2",
                    waypoint: &b,
                },
            ],
            close: false,
            base,
            flange,
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
                },
                Move {
                    name: "P2",
                    waypoint: &b,
                },
            ],
            close: true,
            base,
            flange,
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
    };
    let err = urscript(
        &kin,
        &tool,
        &Export {
            name: "x",
            moves: vec![Move {
                name: "P1",
                waypoint: &w,
            }],
            close: false,
            base: root,
            flange: tip,
        },
    )
    .unwrap_err();
    assert!(err.contains("6"), "{err}");
}

#[test]
fn names_become_identifiers_and_comments_stay_ascii() {
    assert_eq!(function_name("pick place-1"), "pick_place_1");
    assert_eq!(function_name("1st"), "kynex_1st");
    assert_eq!(function_name("로봇"), "kynex___");
    assert_eq!(ascii("P1 집기"), "P1 ??");
}
