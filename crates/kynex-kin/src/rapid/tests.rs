use kreate_math::{DAffine3, DVec3};

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

/// The numbers in the `n`th bracket of `text` from `start`.
fn numbers_after(text: &str, start: &str) -> Vec<f64> {
    let i = text
        .find(start)
        .unwrap_or_else(|| panic!("no {start} in {text}"))
        + start.len();
    let open = text[i..].find('[').unwrap() + i;
    let open = open + text[open..].find(|c: char| c != '[').unwrap();
    let close = text[open..].find(']').unwrap() + open;
    text[open..close]
        .split(',')
        .map(|x| x.trim().parse().unwrap())
        .collect()
}

#[test]
fn a_program_reads_as_a_rapid_module() {
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
    let d = wp(
        MoveKind::Linear,
        [0.5, -1.0, 1.2, -1.7, -1.5, 0.4],
        0.25,
        0.0,
    );
    let pallet = Places {
        name: "pallet\nMoveJ".into(),
        at: vec![
            DVec3::new(0.4, 0.0, 0.1),
            DVec3::new(0.5, 0.0, 0.1),
            DVec3::new(0.6, 0.0, 0.1),
        ],
    };
    let module = rapid(
        &kin,
        &tool,
        &Export {
            name: "a very long program name for an ABB controller",
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
                        port: 2,
                        seconds: 0.2,
                    }],
                    place: None,
                },
                Move {
                    name: "P4\nMoveJ",
                    waypoint: &d,
                    before: vec![Op::Wait { port: 3, on: true }],
                    place: Some(Place { pallet: 0, from: 1 }),
                },
            ],
            close: false,
            base,
            flange,
            trailing: vec![Op::Next { pallet: 0 }, Op::Note("done".into())],
            pallets: vec![pallet],
            markers: true,
        },
    )
    .unwrap();
    let lines: Vec<&str> = module.lines().map(str::trim).collect();

    // A module, its name a RAPID name of at most 32 characters.
    let name = lines[0].strip_prefix("MODULE ").unwrap();
    assert!(name.len() <= 32 && name.starts_with("KYNEX_"), "{name}");
    assert_eq!(lines.last(), Some(&"ENDMODULE"));
    assert_eq!(lines.iter().filter(|l| **l == "PROC main()").count(), 1);
    assert_eq!(lines.iter().filter(|l| **l == "ENDPROC").count(), 1);
    // Every statement ends with a semicolon; comments say nothing more.
    for l in &lines {
        let structural = l.is_empty()
            || l.starts_with('!')
            || l.starts_with("MODULE ")
            || ["ENDMODULE", "PROC main()", "ENDPROC"].contains(l);
        assert!(structural || l.ends_with(';'), "{l}");
    }
    assert!(!lines
        .iter()
        .any(|l| l.starts_with("MoveJ") && l.contains("pallet")));
    assert!(lines.contains(&"! P4?MoveJ"), "{module}");
    assert!(lines.contains(&"! The places of pallet?MoveJ"), "{module}");

    // The first point by its joints, exactly, in degrees.
    let j1 = numbers_after(&module, "CONST jointtarget j1 :=");
    for (got, want) in j1.iter().zip(a.positions.iter()) {
        assert!((got - want.to_degrees()).abs() < 1e-3, "{j1:?}");
    }
    assert!(lines.contains(&"VelSet 50, 5000;"));
    assert!(lines.contains(&"MoveAbsJ j1, vmax, fine, kynex_tool;"));

    // A line: its pose in the base frame, in millimetres; its speed and
    // blend as data of their own.
    let p2 = numbers_after(&module, "CONST robtarget p2 :=");
    let want = (kin.world_of(&b.positions, base).unwrap().inverse()
        * kin.pose(&b.positions, &tool))
    .translation
        * 1000.0;
    assert!(
        (DVec3::new(p2[0], p2[1], p2[2]) - want).length() < 0.01,
        "{p2:?} {want}"
    );
    assert!(lines.contains(&"CONST speeddata kynex_v250 := [250,500,5000,1000];"));
    assert!(lines.contains(&"CONST zonedata kynex_z50 := [FALSE,50,75,75,7.5,75,7.5];"));
    assert!(lines.contains(&"MoveL p2, kynex_v250, kynex_z50, kynex_tool \\WObj:=wobj0;"));
    // An arc through its via point.
    assert!(lines.contains(&"MoveC v3, p3, kynex_v100, fine, kynex_tool \\WObj:=wobj0;"));
    // A pallet point moved to the place counted to, from the second.
    assert!(
        lines.contains(&"MoveL Offs(p4, kynex_places_0{kynex_place_0 + 1}.x - kynex_places_0{2}.x, kynex_places_0{kynex_place_0 + 1}.y - kynex_places_0{2}.y, kynex_places_0{kynex_place_0 + 1}.z - kynex_places_0{2}.z), kynex_v250, fine, kynex_tool \\WObj:=wobj0;"),
        "{module}"
    );
    assert!(lines.contains(&"CONST pos kynex_places_0{3} := [[400.00,0.00,100.00],[500.00,0.00,100.00],[600.00,0.00,100.00]];"));
    assert!(lines.contains(&"kynex_place_0 := (kynex_place_0 + 1) MOD 3;"));

    // I/O by signal, those used listed at the top.
    assert!(lines.contains(&"!   DO_KYNEX_0: digital output"));
    assert!(lines.contains(&"!   DO_KYNEX_2: digital output"));
    assert!(lines.contains(&"!   DI_KYNEX_3: digital input"));
    assert!(lines.contains(&"SetDO DO_KYNEX_0, 1;"));
    assert!(lines.contains(&"WaitTime 0.50;"));
    assert!(lines.contains(&"PulseDO \\PLength:=0.2, DO_KYNEX_2;"));
    assert!(lines.contains(&"WaitDI DI_KYNEX_3, 1;"));

    // Each move numbered as it starts and as it is done, and one past the
    // last at the end.
    for k in 1..=4 {
        assert!(lines.contains(&format!("kynex_mark := {k};").as_str()));
        assert!(lines.contains(&format!("kynex_done := {k};").as_str()));
    }
    assert!(lines.contains(&"kynex_mark := 5;"));
}

#[test]
fn a_closed_program_goes_round_in_a_loop() {
    let doc = load(UR5E);
    let (_, root) = find(&doc, "base_link");
    let (_, base) = find(&doc, "base");
    let (_, flange) = find(&doc, "tool0");
    let kin = Kinematics::of(&doc, root).unwrap();
    let tool = kin.tool(flange, DAffine3::IDENTITY).unwrap();
    let a = wp(MoveKind::Joint, [0.3, -1.2, 1.4, -1.6, -1.5, 0.2], 1.0, 0.0);
    let b = wp(MoveKind::Joint, [0.5, -1.0, 1.2, -1.7, -1.5, 0.4], 1.0, 0.0);
    let module = Abb
        .write(
            &kin,
            &tool,
            &Export {
                name: "loop",
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
                pallets: Vec::new(),
                markers: false,
            },
        )
        .unwrap();
    let lines: Vec<&str> = module.lines().map(str::trim).collect();
    let start = lines.iter().position(|l| *l == "WHILE TRUE DO").unwrap();
    let end = lines.iter().position(|l| *l == "ENDWHILE").unwrap();
    let inside = &lines[start..end];
    // Round: to the second point, and back to the first.
    assert!(inside.contains(&"MoveAbsJ j2, vmax, fine, kynex_tool;"));
    assert!(inside.contains(&"MoveAbsJ j1, vmax, fine, kynex_tool;"));
    assert_eq!(Abb.extension(), "mod");
}

/// Whatever the first move is — a line, or a joint move to a pallet place —
/// every target a move goes to is declared, in a program that goes round.
#[test]
fn every_target_moved_to_is_declared() {
    let doc = load(UR5E);
    let (_, root) = find(&doc, "base_link");
    let (_, base) = find(&doc, "base");
    let (_, flange) = find(&doc, "tool0");
    let kin = Kinematics::of(&doc, root).unwrap();
    let tool = kin.tool(flange, DAffine3::IDENTITY).unwrap();
    let a = [0.3, -1.2, 1.4, -1.6, -1.5, 0.2];
    let b = [0.5, -1.0, 1.2, -1.7, -1.5, 0.4];
    let pallet = Places {
        name: "pallet".into(),
        at: vec![DVec3::new(0.4, 0.0, 0.1), DVec3::new(0.5, 0.0, 0.1)],
    };
    for (kind, place) in [
        (MoveKind::Linear, None),
        (MoveKind::Joint, Some(Place { pallet: 0, from: 0 })),
    ] {
        let first = wp(kind, a, 0.25, 0.0);
        let second = wp(MoveKind::Linear, b, 0.25, 0.0);
        let module = rapid(
            &kin,
            &tool,
            &Export {
                name: "round",
                moves: vec![
                    Move {
                        name: "P1",
                        waypoint: &first,
                        before: Vec::new(),
                        place,
                    },
                    Move {
                        name: "P2",
                        waypoint: &second,
                        before: Vec::new(),
                        place: None,
                    },
                ],
                close: true,
                base,
                flange,
                trailing: vec![Op::Next { pallet: 0 }],
                pallets: vec![pallet.clone()],
                markers: false,
            },
        )
        .unwrap();
        for line in module.lines().map(str::trim) {
            let Some(rest) = ["MoveAbsJ ", "MoveJ ", "MoveL ", "MoveC "]
                .iter()
                .find_map(|m| line.strip_prefix(m))
            else {
                continue;
            };
            let rest = rest.strip_prefix("Offs(").unwrap_or(rest);
            let target = rest.split([',', ' ']).next().unwrap();
            let kind = if target.starts_with('j') {
                "jointtarget"
            } else {
                "robtarget"
            };
            assert!(
                module.contains(&format!("CONST {kind} {target} :=")),
                "{kind:?} {target} undeclared in\n{module}"
            );
        }
        assert!(
            !module.contains("-0.00,") && !module.contains("[-0.00"),
            "{module}"
        );
    }
}
