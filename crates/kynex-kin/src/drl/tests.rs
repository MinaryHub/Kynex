use kreate_math::{DAffine3, DMat3, DVec3};

use super::*;
use crate::path::Waypoint;
use crate::post::Places;
use crate::tests::{find, load, UR5E};

fn wp(kind: MoveKind, positions: [f64; 6], speed: f64, blend: f64) -> Waypoint {
    Waypoint {
        kind,
        positions: positions.to_vec(),
        speed,
        blend,
        via: None,
    }
}

#[test]
fn zyz_angles_give_the_turn_back() {
    use std::f64::consts::PI;
    let z = |a: f64| DMat3::from_rotation_z(a);
    let y = |b: f64| DMat3::from_rotation_y(b);
    for (a, b, c) in [
        (0.3, 0.7, -1.2),
        (-2.0, 2.5, 0.4),
        (1.0, 0.0, 0.5),
        (0.4, PI, -0.3),
        (0.0, PI, 0.0),
    ] {
        let r = z(a) * y(b) * z(c);
        let [a2, b2, c2] = zyz(r);
        let back = z(a2) * y(b2) * z(c2);
        assert!(
            (back - r).abs_diff_eq(DMat3::ZERO, 1e-9),
            "{a} {b} {c}: {a2} {b2} {c2}"
        );
        assert!((0.0..=PI).contains(&b2));
    }
    // The tool down, as Doosan writes it: B is 180.
    assert_eq!(
        posx(&DAffine3::from_rotation_y(PI)),
        "posx(0.000, 0.000, 0.000, 0.0000, 180.0000, 0.0000)"
    );
}

#[test]
fn a_program_reads_as_drl() {
    let doc = load(UR5E);
    let (_, root) = find(&doc, "base_link");
    let (_, base) = find(&doc, "base");
    let (_, flange) = find(&doc, "tool0");
    let kin = Kinematics::of(&doc, root).unwrap();
    let tool = kin
        .tool(
            flange,
            DAffine3::from_translation(DVec3::new(0.0, 0.0, 0.12)),
        )
        .unwrap();
    let a = wp(MoveKind::Joint, [0.3, -1.2, 1.4, -1.6, -1.5, 0.2], 0.5, 0.0);
    let b = wp(
        MoveKind::Linear,
        [0.5, -1.0, 1.2, -1.7, -1.5, 0.4],
        0.25,
        0.05,
    );
    let mut c = wp(
        MoveKind::Circular,
        [0.7, -1.1, 1.3, -1.7, -1.5, 0.4],
        0.1,
        0.0,
    );
    c.via = Some(vec![0.6, -1.05, 1.25, -1.7, -1.5, 0.4]);
    let d = wp(MoveKind::Joint, [0.5, -1.0, 1.2, -1.7, -1.5, 0.4], 0.5, 0.0);
    let pallet = Places {
        name: "pallet\nmovej(".into(),
        at: vec![DVec3::new(0.4, 0.0, 0.1), DVec3::new(0.5, 0.0, 0.1)],
    };
    let program = Doosan
        .write(
            &kin,
            &tool,
            &Export {
                name: "cell\nmovej(",
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
                        before: vec![Op::Set { port: 0, on: true }, Op::Sleep(0.5)],
                        place: None,
                    },
                    Move {
                        name: "P3",
                        waypoint: &c,
                        before: vec![Op::Pulse {
                            port: 16,
                            seconds: 0.2,
                        }],
                        place: None,
                    },
                    Move {
                        name: "P4\nmovej(",
                        waypoint: &d,
                        before: vec![Op::Wait { port: 3, on: false }],
                        place: Some(Place { pallet: 0, from: 1 }),
                    },
                ],
                close: true,
                base,
                flange,
                trailing: vec![Op::Next { pallet: 0 }],
                pallets: vec![pallet],
                markers: true,
            },
        )
        .unwrap();
    let lines: Vec<&str> = program.lines().collect();
    // Names say nothing past a comment.
    for l in &lines {
        assert!(!l.starts_with("movej(") || !l.contains("pallet"), "{l}");
    }
    assert!(lines.contains(&"# P4?movej("), "{program}");

    // The tool point to register, and chosen.
    assert!(program.contains("# TCP \"KYNEX\", to register on the teach pendant: posx(0.000, 0.000, 120.000, 0.0000, 0.0000, 0.0000)"), "{program}");
    assert!(lines.contains(&"set_tcp(\"KYNEX\")"));
    // The first point by its joints, in degrees, each joint at its share of
    // its own top speed.
    let degrees: Vec<String> = a
        .positions
        .iter()
        .map(|q| format!("{:.4}", q.to_degrees()))
        .collect();
    assert!(
        lines.contains(&format!("j1 = posj({})", degrees.join(", ")).as_str()),
        "{program}"
    );
    let tops: Vec<f64> = kin.vars.iter().map(|v| v.velocity.to_degrees()).collect();
    let share = format!("{:.2}", 0.5 * tops[0]);
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("movej(j1, vel=[") && l.contains(&share)),
        "{program}"
    );
    // A line in the base frame, its speed in mm/s and its blend a radius.
    let p2 = base_pose(&kin, base, &tool, &b.positions);
    let t = p2.translation * 1000.0;
    assert!(
        program.contains(&format!("p2 = posx({:.3}, {:.3}, {:.3}", t.x, t.y, t.z)),
        "{program}"
    );
    assert!(
        program.contains(
            "movel(p2, vel=[250.0, 57.30], acc=[1200.0, 114.59], radius=50.0, ref=DR_BASE)"
        ),
        "{program}"
    );
    // An arc through its via point.
    assert!(program.contains("movec(v3, p3, vel=[100.0"), "{program}");
    // A joint move to a pallet place: to the pose moved there, in the
    // posture taught.
    assert!(
        program.contains("movejx(kynex_shift(p4, kynex_places_0, kynex_place_0, 1), vel=["),
        "{program}"
    );
    assert!(program.contains("sol=get_solution_space(j4))"), "{program}");
    assert!(
        lines.contains(&"kynex_places_0 = [[400.000, 0.000, 100.000], [500.000, 0.000, 100.000]]")
    );
    assert!(lines
        .iter()
        .any(|l| l.trim() == "kynex_place_0 = (kynex_place_0 + 1) % 2"));

    // I/O, numbered as the controller numbers it.
    for want in [
        "set_digital_output(1, ON)",
        "wait(0.50)",
        "set_tool_digital_output(1, ON)",
        "wait(0.2)",
        "set_tool_digital_output(1, OFF)",
        "wait_digital_input(4, OFF)",
    ] {
        assert!(
            lines.iter().any(|l| l.trim() == want),
            "{want} in {program}"
        );
    }
    // Round in a loop, each move marked as it starts and is done.
    assert!(lines.contains(&"while True:"));
    assert!(lines.iter().any(|l| l.trim() == "tp_log(\"kynex mark 5\")"));
    assert!(lines
        .iter()
        .any(|l| l.trim().starts_with("tp_log(\"kynex done 5 %s %s\"")));
    // Every line in the loop is inside it.
    let start = lines.iter().position(|l| *l == "while True:").unwrap();
    assert!(lines[start + 1..]
        .iter()
        .all(|l| l.is_empty() || l.starts_with("    ")));
    assert_eq!(Doosan.extension(), "drl");
}

/// The tool pose at `q` in the base frame.
fn base_pose(kin: &Kinematics, base: u64, tool: &Tool, q: &[f64]) -> DAffine3 {
    kin.world_of(q, base).unwrap().inverse() * kin.pose(q, tool)
}

#[test]
fn only_a_six_axis_arm_is_written() {
    let doc = load(crate::tests::TRACK);
    let (_, root) = find(&doc, "track");
    let kin = Kinematics::of(&doc, root).unwrap();
    let tool = kin.tool(root, DAffine3::IDENTITY).unwrap();
    let a = Waypoint {
        kind: MoveKind::Joint,
        positions: vec![0.0; kin.vars.len()],
        speed: 0.5,
        blend: 0.0,
        via: None,
    };
    let err = drl(
        &kin,
        &tool,
        &Export {
            name: "x",
            moves: vec![Move {
                name: "P1",
                waypoint: &a,
                before: Vec::new(),
                place: None,
            }],
            close: false,
            base: root,
            flange: root,
            trailing: Vec::new(),
            pallets: Vec::new(),
            markers: false,
        },
    )
    .unwrap_err();
    assert!(err.contains('6'), "{err}");
}
