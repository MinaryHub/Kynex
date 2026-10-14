use kreate_math::DAffine3;

use super::*;
use crate::tests::{find, load, UR5E};

/// One prismatic axis along X, slow: 0.1 m/s.
const SLIDE: &str = r#"<?xml version="1.0"?>
<robot name="slide">
  <link name="rail"/>
  <joint name="carriage" type="prismatic">
    <parent link="rail"/><child link="tool0"/>
    <origin xyz="0 0 0.5"/><axis xyz="1 0 0"/>
    <limit lower="-2" upper="2" effort="100" velocity="0.1"/>
  </joint>
  <link name="tool0"/>
</robot>
"#;

/// One joint turning about Z, the tool 1 m out from it.
const SWING: &str = r#"<?xml version="1.0"?>
<robot name="swing">
  <link name="base"/>
  <joint name="turn" type="revolute">
    <parent link="base"/><child link="arm"/>
    <origin xyz="0 0 0.5"/><axis xyz="0 0 1"/>
    <limit lower="-3" upper="3" effort="100" velocity="2"/>
  </joint>
  <link name="arm"/>
  <joint name="arm-tool0" type="fixed">
    <parent link="arm"/><child link="tool0"/>
    <origin xyz="1 0 0"/>
  </joint>
  <link name="tool0"/>
</robot>
"#;

fn robot(urdf: &str, root: &str) -> (Kinematics, Tool) {
    robot_to(urdf, root, "tool0")
}

fn robot_to(urdf: &str, root: &str, tip: &str) -> (Kinematics, Tool) {
    let doc = load(urdf);
    let (_, root) = find(&doc, root);
    let (_, tip) = find(&doc, tip);
    let kin = Kinematics::of(&doc, root).unwrap();
    let tool = kin.tool(tip, DAffine3::IDENTITY).unwrap();
    (kin, tool)
}

fn wp(kind: MoveKind, positions: &[f64], speed: f64) -> Waypoint {
    Waypoint {
        kind,
        positions: positions.to_vec(),
        speed,
    }
}

#[test]
fn a_profile_speeds_up_cruises_and_slows_down() {
    // 1 m at 0.5 m/s, 1 m/s²: half a second up, 1.5 s cruising, half down.
    let p = Profile::new(1.0, 0.5, 1.0);
    assert!((p.duration - 2.5).abs() < 1e-12);
    assert_eq!(p.fraction(0.0, p.duration), 0.0);
    assert!((p.fraction(p.duration, p.duration) - 1.0).abs() < 1e-12);
    assert!(
        (p.fraction(1.25, p.duration) - 0.5).abs() < 1e-12,
        "symmetric"
    );
    // Too short to reach full speed: a triangle, √(d/a) each way.
    let short = Profile::new(0.01, 0.5, 1.0);
    assert!((short.duration - 0.2).abs() < 1e-12);
    // Stretched to twice as long, it is half as far along at the same time.
    let mut last = 0.0;
    for k in 0..=100 {
        let f = p.fraction(k as f64 * 0.05, 5.0);
        assert!(f >= last - 1e-15, "never goes back");
        last = f;
    }
    assert!((last - 1.0).abs() < 1e-12);
}

#[test]
fn a_joint_move_takes_every_joint_there_together_within_its_speed() {
    let (kin, tool) = robot(UR5E, "base_link");
    let a = [0.0, -1.2, 1.2, -1.5, -1.5, 0.0];
    let b = [1.0, -0.8, 0.6, -1.0, -1.2, 2.0];
    let plan = plan(
        &kin,
        &tool,
        &[wp(MoveKind::Joint, &a, 1.0), wp(MoveKind::Joint, &b, 0.5)],
        false,
    );
    // wrist_3 turns furthest, 2 rad, at half of 6.28 rad/s.
    let top = kin.vars[5].velocity;
    let alone = Profile::new(2.0, 0.5 * top, JOINT_ACCELERATION * top);
    assert!(
        (plan.duration - alone.duration).abs() < 1e-9,
        "{}",
        plan.duration
    );
    assert_eq!(plan.at(0.0), a.to_vec());
    assert_eq!(plan.at(plan.duration), b.to_vec());
    let dt = 1e-3;
    let mut t = 0.0;
    while t + dt <= plan.duration {
        let (x, y) = (plan.at(t), plan.at(t + dt));
        for (i, var) in kin.vars.iter().enumerate() {
            let v = (y[i] - x[i]).abs() / dt;
            assert!(v <= 0.5 * var.velocity * 1.001, "{} at {v}", var.name);
        }
        // All arrive together: the same share of the way at any moment.
        let share = |i: usize| (x[i] - a[i]) / (b[i] - a[i]);
        for i in 1..6 {
            assert!((share(i) - share(0)).abs() < 1e-9);
        }
        t += dt;
    }
}

#[test]
fn a_linear_move_keeps_the_tool_on_the_line() {
    let (kin, tool) = robot(UR5E, "base_link");
    let a = vec![0.3, -1.2, 1.4, -1.6, -1.5, 0.2];
    // The same tool orientation 300 mm along Y and 100 mm down.
    let (_, rotation, p0) = kin.pose(&a, &tool).to_scale_rotation_translation();
    let p1 = p0 + DVec3::new(0.0, 0.3, -0.1);
    let b = kin
        .solve(
            &tool,
            &Goal {
                position: p1,
                rotation: Some(rotation),
            },
            &a,
            &Options::default(),
        )
        .positions;
    let plan = plan(
        &kin,
        &tool,
        &[wp(MoveKind::Joint, &a, 1.0), wp(MoveKind::Linear, &b, 0.25)],
        false,
    );
    let seg = &plan.segments[0];
    assert!(seg.problems.is_empty(), "{:?}", seg.problems);
    // From where the solved end really is, which is within the solver's
    // tolerance of `p1`.
    let d = kin.pose(&b, &tool).translation.distance(p0);
    let want = Profile::new(d, 0.25, LINEAR_ACCELERATION).duration;
    assert!(
        (plan.duration - want).abs() < 1e-9,
        "{} vs {want}",
        plan.duration
    );
    let line = (p1 - p0).normalize();
    for k in 0..=200 {
        let q = plan.at(plan.duration * k as f64 / 200.0);
        let (_, r, p) = kin.pose(&q, &tool).to_scale_rotation_translation();
        let off = (p - p0) - line * (p - p0).dot(line);
        assert!(
            off.length() < 1e-4,
            "{} mm off the line",
            off.length() * 1e3
        );
        assert!(turn(r, rotation) < 1e-3);
    }
}

#[test]
fn a_line_the_tool_cannot_follow_is_reported() {
    // The tool swings on a circle: the straight chord between two of its
    // points cuts inside it.
    let (kin, tool) = robot(SWING, "base");
    let plan = plan(
        &kin,
        &tool,
        &[
            wp(MoveKind::Joint, &[-0.5], 1.0),
            wp(MoveKind::Linear, &[0.5], 0.25),
        ],
        false,
    );
    let problems = &plan.segments[0].problems;
    let Some(Problem::Unreachable {
        along, position, ..
    }) = problems.first()
    else {
        panic!("{problems:?}");
    };
    assert!(*along < 0.2, "from early on: {along}");
    // At the middle the chord is 1 − cos 0.5 inside the circle.
    assert!(*position > 0.1, "{position}");
    // It still ends where it was taught.
    assert_eq!(plan.at(plan.duration), vec![0.5]);
}

#[test]
fn a_line_faster_than_a_joint_goes_is_slowed_to_it() {
    let (kin, tool) = robot(SLIDE, "rail");
    let plan = plan(
        &kin,
        &tool,
        &[
            wp(MoveKind::Joint, &[0.0], 1.0),
            wp(MoveKind::Linear, &[1.0], 0.5),
        ],
        false,
    );
    let seg = &plan.segments[0];
    let Some(Problem::Slowed { joint: 0, factor }) = seg.problems.first() else {
        panic!("{:?}", seg.problems);
    };
    assert!(*factor > 4.0, "{factor}");
    // Now within the joint's 0.1 m/s.
    let dt = 0.01;
    let mut t = 0.0;
    while t + dt <= plan.duration {
        let v = (plan.at(t + dt)[0] - plan.at(t)[0]).abs() / dt;
        assert!(v <= 0.1 * 1.01, "{v} m/s at {t}");
        t += dt;
    }
}

#[test]
fn an_arm_with_fewer_than_six_joints_holds_the_line_not_the_orientation() {
    // The demo's arm on its track, five joints: its tool's orientation
    // changes as it moves along a line, and that is no problem.
    let (kin, tool) = robot_to(crate::tests::TRACK, "track", "tool");
    let a = vec![0.3, 0.4, 0.3, 0.8, -0.4];
    let (_, _, p0) = kin.pose(&a, &tool).to_scale_rotation_translation();
    let p1 = p0 + DVec3::new(0.0, -0.2, -0.15);
    let b = kin
        .solve(
            &tool,
            &Goal {
                position: p1,
                rotation: None,
            },
            &a,
            &Options::default(),
        )
        .positions;
    let plan = plan(
        &kin,
        &tool,
        &[wp(MoveKind::Joint, &a, 1.0), wp(MoveKind::Linear, &b, 0.25)],
        false,
    );
    assert!(
        plan.segments[0].problems.is_empty(),
        "{:?}",
        plan.segments[0].problems
    );
    let line = (p1 - p0).normalize();
    for k in 0..=50 {
        let p = kin
            .pose(&plan.at(plan.duration * k as f64 / 50.0), &tool)
            .translation;
        let off = (p - p0) - line * (p - p0).dot(line);
        assert!(off.length() < 1e-4);
    }
}

#[test]
fn a_closed_program_comes_back_to_where_it_began() {
    let (kin, tool) = robot(SWING, "base");
    let points = [
        wp(MoveKind::Joint, &[0.0], 1.0),
        wp(MoveKind::Joint, &[1.0], 1.0),
        wp(MoveKind::Joint, &[-1.0], 1.0),
    ];
    let open = plan(&kin, &tool, &points, false);
    let closed = plan(&kin, &tool, &points, true);
    assert_eq!(open.segments.len(), 2);
    assert_eq!(closed.segments.len(), 3);
    assert_eq!(closed.segments[2].to, 0);
    assert!(closed.duration > open.duration);
    assert_eq!(closed.at(closed.duration), vec![0.0]);
    assert_eq!(open.at(open.segments[0].duration), vec![1.0]);
    // A trace follows the arc, a metre out.
    for p in trace(&kin, &tool, &open, 0.05) {
        assert!((DVec3::new(p.x, p.y, 0.0).length() - 1.0).abs() < 1e-9);
    }
}

#[test]
fn one_waypoint_or_none_is_no_move() {
    let (kin, tool) = robot(SWING, "base");
    let one = plan(&kin, &tool, &[wp(MoveKind::Joint, &[0.4], 1.0)], true);
    assert_eq!(one.duration, 0.0);
    assert_eq!(one.at(3.0), vec![0.4]);
    let none = plan(&kin, &tool, &[], true);
    assert_eq!(none.at(0.0), kin.current);
}
