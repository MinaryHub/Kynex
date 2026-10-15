use kreate_math::{DAffine3, DVec3};

use super::*;
use crate::path::Waypoint;
use crate::post::Places;
use crate::tests::{find, load};

const LRMATE: &str = include_str!("../../../../apps/kynex/library/lrmate200id7l.urdf");

fn wp(kind: MoveKind, positions: [f64; 6], speed: f64, blend: f64) -> Waypoint {
    Waypoint {
        kind,
        positions: positions.to_vec(),
        speed,
        blend,
        via: None,
    }
}

/// The LR Mate, its uids: root, base, flange.
fn lrmate() -> (Document, Kinematics, u64, u64) {
    let doc = load(LRMATE);
    let (_, root) = find(&doc, "base_link");
    let (_, base) = find(&doc, "base");
    let (_, flange) = find(&doc, "tool0");
    let kin = Kinematics::of(&doc, root).unwrap();
    (doc, kin, base, flange)
}

use kreate_scene::Document;

/// The postures FANUC names: the tool down over the front, the elbow up;
/// the upper arm laid back, the wrist behind J1's axis; the forearm over
/// the head, the elbow down; the wrist flipped; J6 a turn round.
#[test]
fn a_posture_is_said_as_the_controller_says_it() {
    let (_, kin, ..) = lrmate();
    let (arm, _) = axes(&kin).unwrap();
    let h = std::f64::consts::FRAC_PI_2;
    for (q, want) in [
        ([0.0, 0.0, 0.0, 0.0, -h, 0.0], "'N U T, 0, 0, 0'"),
        ([0.0, -h, 0.0, 0.0, -h, 0.0], "'N U B, 0, 0, 0'"),
        (
            [0.0, 0.0, 120f64.to_radians(), 0.0, -h, 0.0],
            "'N D B, 0, 0, 0'",
        ),
        ([0.0, 0.0, 0.0, 0.0, h, 0.0], "'F U T, 0, 0, 0'"),
        (
            [0.0, 0.0, 0.0, 0.0, -h, 270f64.to_radians()],
            "'N U T, 0, 0, 1'",
        ),
        (
            [0.0, 0.0, 0.0, 0.0, -h, -200f64.to_radians()],
            "'N U T, 0, 0, -1'",
        ),
    ] {
        assert_eq!(config(&kin, &arm, &q).unwrap(), want, "{q:?}");
    }
}

/// A program as an LS file: its header counting its lines, the tool set
/// from the faceplate, joint-form positions with J3 from level, I/O on the
/// controller's own, a pallet's places in position registers and a move on
/// it offset from the place it was taught over, in Cartesian form at the
/// pose the arm reaches; markers; and a name that cannot break out of its
/// remark.
#[test]
fn a_program_reads_as_an_ls_file() {
    let (_, kin, base, flange) = lrmate();
    let tool = kin
        .tool(
            flange,
            DAffine3::from_translation(DVec3::new(0.0, 0.0, 0.15)),
        )
        .unwrap();
    let h = std::f64::consts::FRAC_PI_2;
    let a = wp(MoveKind::Joint, [0.2, 0.3, 0.1, 0.0, -h, 0.0], 0.5, 0.0);
    let b = wp(
        MoveKind::Linear,
        [0.3, 0.4, 0.2, 0.1, -1.4, 0.2],
        0.25,
        0.02,
    );
    let mut c = wp(
        MoveKind::Circular,
        [0.5, 0.4, 0.2, 0.1, -1.4, 0.2],
        0.1,
        0.0,
    );
    c.via = Some(vec![0.4, 0.42, 0.2, 0.1, -1.4, 0.2]);
    let d = wp(MoveKind::Linear, [0.3, 0.4, 0.2, 0.1, -1.4, 0.2], 0.25, 0.0);
    let pallet = Places {
        name: "pallet".into(),
        at: vec![
            DVec3::new(0.4, 0.0, -0.2),
            DVec3::new(0.5, 0.0, -0.2),
            DVec3::new(0.6, 0.0, -0.2),
        ],
    };
    let ls = tp(
        &kin,
        &tool,
        &Export {
            name: "main;\nJ P[1] 100% FINE",
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
                    name: "P4",
                    waypoint: &d,
                    before: vec![Op::Wait { port: 3, on: true }],
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
    let lines: Vec<&str> = ls.lines().collect();
    assert!(
        lines[0].starts_with("/PROG  KYNEX_MAIN__J_P_1__100__FINE"),
        "{}",
        lines[0]
    );
    assert_eq!(lines.last(), Some(&"/END"));
    // Its count of numbered lines, as the header says.
    let mn = lines.iter().position(|l| *l == "/MN").unwrap();
    let pos = lines.iter().position(|l| *l == "/POS").unwrap();
    let numbered = lines[mn + 1..pos]
        .iter()
        .filter(|l| !l.starts_with("    :"))
        .count();
    assert!(ls.contains(&format!("LINE_COUNT\t= {numbered};")), "{ls}");
    // Numbered from 1, each line's instruction ended.
    assert!(lines[mn + 1].starts_with("   1:"));
    let main_lines = &lines[mn + 1..pos];
    assert!(
        main_lines
            .iter()
            .filter(|l| !l.starts_with("    :C") && !l.ends_with("    "))
            .all(|l| l.ends_with(';')),
        "{ls}"
    );
    // The name only in a remark, on one line.
    assert!(main_lines.iter().all(
        |l| !l.contains("J P[1] 100% FINE") || (l.contains('!') && l.matches(';').count() == 1)
    ));

    // The tool: 150 mm out along the faceplate's Z.
    assert!(ls.contains("PR[49,3]=150.000 ;"), "{ls}");
    assert!(ls.contains("UTOOL[1]=PR[49] ;"));
    // I/O.
    assert!(ls.contains("  DO[1]=ON ;"));
    assert!(ls.contains("  WAIT   0.50(sec) ;"));
    assert!(ls.contains("  PULSE RO[1],0.2sec ;"));
    assert!(ls.contains("  WAIT DI[4]=ON ;"));
    // Moves.
    assert!(ls.contains("J P[1] 50% FINE    ;"), "{ls}");
    assert!(ls.contains("L P[2] 250mm/sec CNT20    ;"));
    assert!(
        ls.contains("C P[5]    \n    :  P[3] 100mm/sec FINE    ;"),
        "{ls}"
    );
    assert!(ls.contains("L P[4] 250mm/sec FINE Offset,PR[50]    ;"));
    // The pallet: its places from PR[51], counted in R[81].
    assert!(ls.contains("PR[52,1]=500.000 ;"));
    assert!(ls.contains("PR[53,3]=-200.000 ;"));
    assert!(ls.contains("R[90]=R[81]+51 ;"));
    assert!(ls.contains("PR[50]=PR[R[90]]-PR[52] ;"));
    assert!(ls.contains("IF R[81]<3,JMP LBL[102] ;"), "{ls}");
    // Round again.
    assert!(ls.contains("  LBL[1] ;") && ls.contains("  JMP LBL[1] ;"));
    // Markers.
    assert!(ls.contains("R[98]=1 ;") && ls.contains("R[99]=1 ;"));

    // P[1] in joint form: J3 from level, the description's J3 less its J2.
    let p1 = &ls[ls.find("P[1]{").unwrap()..];
    let j = |name: &str| -> f64 {
        let at = p1.find(&format!("{name}=")).unwrap() + name.len() + 1;
        p1[at..].split_whitespace().next().unwrap().parse().unwrap()
    };
    assert!((j("J2") - 0.3f64.to_degrees()).abs() < 1e-3);
    assert!((j("J3") - (0.1f64 - 0.3).to_degrees()).abs() < 1e-3);
    assert!((j("J5") + 90.0).abs() < 1e-3);

    // P[4] in Cartesian form: where the tool is, in the base frame.
    let p4 = &ls[ls.find("P[4]{").unwrap()..];
    let v = |name: &str| -> f64 {
        let at = p4.find(&format!("{name} =")).unwrap() + name.len() + 2;
        p4[at..].split_whitespace().next().unwrap().parse().unwrap()
    };
    let world_base = kin.world_of(&d.positions, base).unwrap();
    let pose = world_base.inverse() * kin.pose(&d.positions, &tool);
    let t = pose.translation * 1000.0;
    for (name, want) in [("X", t.x), ("Y", t.y), ("Z", t.z)] {
        assert!((v(name) - want).abs() < 1e-3, "{name}");
    }
    let (w, p, r) = (v("W"), v("P"), v("R"));
    let back = DQuat::from_euler(
        EulerRot::ZYX,
        r.to_radians(),
        p.to_radians(),
        w.to_radians(),
    );
    let (_, turn, _) = pose.to_scale_rotation_translation();
    assert!(back.angle_between(turn) < 1e-4, "{w} {p} {r}");
    assert!(p4.contains("CONFIG : 'N U T, 0, 0, 0'"), "{p4}");

    // No free simulator runs it: the whole file is kept, and an export
    // that writes it otherwise fails here. When a change is meant, write
    // it again with KYNEX_BLESS=1.
    let golden = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/fanuc/golden.ls");
    if std::env::var_os("KYNEX_BLESS").is_some() {
        std::fs::write(&golden, &ls).unwrap();
    }
    let kept = std::fs::read_to_string(&golden)
        .unwrap()
        .replace("\r\n", "\n");
    if ls != kept {
        let at = ls.lines().zip(kept.lines()).position(|(a, b)| a != b);
        panic!("golden.ls differs at line {at:?}:\n{ls}");
    }
}

/// A robot on a track: not written, and said why.
#[test]
fn an_axis_outside_the_arm_is_refused() {
    let doc = load(include_str!(
        "../../../../apps/kynex/library/irb1600_track.urdf"
    ));
    let (_, root) = find(&doc, "track_base");
    let (_, base) = find(&doc, "base");
    let (_, flange) = find(&doc, "tool0");
    let kin = Kinematics::of(&doc, root).unwrap();
    let tool = kin.tool(flange, DAffine3::IDENTITY).unwrap();
    let a = Waypoint {
        kind: MoveKind::Joint,
        positions: vec![0.0; kin.vars.len()],
        speed: 0.5,
        blend: 0.0,
        via: None,
    };
    let error = tp(
        &kin,
        &tool,
        &Export {
            name: "track",
            moves: vec![Move {
                name: "P1",
                waypoint: &a,
                before: Vec::new(),
                place: None,
            }],
            close: false,
            base,
            flange,
            trailing: Vec::new(),
            pallets: Vec::new(),
            markers: false,
        },
    )
    .unwrap_err();
    assert!(error.contains("외부축"), "{error}");
}
