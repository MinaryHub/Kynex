use std::path::Path;

use kreate_io::ir::{ImportScene, SourceMetadata};
use kreate_math::DTransform;
use kreate_scene::robot::Mimic;
use kreate_scene::{Object, ObjectData};

use super::*;

/// A UR5e's kinematics: the joint origins of `ur_description`, and its
/// controller's `base` frame, no meshes.
pub(crate) const UR5E: &str = r#"<?xml version="1.0"?>
<robot name="ur5e">
  <link name="base_link"/>
  <joint name="base_link-base_fixed_joint" type="fixed">
    <parent link="base_link"/><child link="base"/>
    <origin rpy="0 0 3.141592654"/>
  </joint>
  <link name="base"/>
  <joint name="shoulder_pan_joint" type="revolute">
    <parent link="base_link"/><child link="shoulder_link"/>
    <origin xyz="0 0 0.1625"/><axis xyz="0 0 1"/>
    <limit lower="-6.283" upper="6.283" effort="150" velocity="3.14"/>
  </joint>
  <link name="shoulder_link"/>
  <joint name="shoulder_lift_joint" type="revolute">
    <parent link="shoulder_link"/><child link="upper_arm_link"/>
    <origin rpy="1.570796327 0 0"/><axis xyz="0 0 1"/>
    <limit lower="-6.283" upper="6.283" effort="150" velocity="3.14"/>
  </joint>
  <link name="upper_arm_link"/>
  <joint name="elbow_joint" type="revolute">
    <parent link="upper_arm_link"/><child link="forearm_link"/>
    <origin xyz="-0.425 0 0"/><axis xyz="0 0 1"/>
    <limit lower="-3.1416" upper="3.1416" effort="150" velocity="3.14"/>
  </joint>
  <link name="forearm_link"/>
  <joint name="wrist_1_joint" type="revolute">
    <parent link="forearm_link"/><child link="wrist_1_link"/>
    <origin xyz="-0.3922 0 0.1333"/><axis xyz="0 0 1"/>
    <limit lower="-6.283" upper="6.283" effort="28" velocity="6.28"/>
  </joint>
  <link name="wrist_1_link"/>
  <joint name="wrist_2_joint" type="revolute">
    <parent link="wrist_1_link"/><child link="wrist_2_link"/>
    <origin xyz="0 -0.0997 0" rpy="1.570796327 0 0"/><axis xyz="0 0 1"/>
    <limit lower="-6.283" upper="6.283" effort="28" velocity="6.28"/>
  </joint>
  <link name="wrist_2_link"/>
  <joint name="wrist_3_joint" type="revolute">
    <parent link="wrist_2_link"/><child link="wrist_3_link"/>
    <origin xyz="0 0.0996 0" rpy="1.570796327 3.141592654 3.141592654"/><axis xyz="0 0 1"/>
    <limit lower="-6.283" upper="6.283" effort="28" velocity="6.28"/>
  </joint>
  <link name="wrist_3_link"/>
  <joint name="wrist_3-tool0" type="fixed">
    <parent link="wrist_3_link"/><child link="tool0"/>
    <origin xyz="0 0 0.05"/>
  </joint>
  <link name="tool0"/>
</robot>
"#;

/// A robot on a linear track, as the demo session's: a prismatic axis and
/// four that turn, the last three in one plane.
pub(crate) const TRACK: &str = r#"<?xml version="1.0"?>
<robot name="track_arm">
  <link name="track"/>
  <joint name="carriage" type="prismatic">
    <parent link="track"/><child link="sled"/>
    <origin xyz="0.2 0 0.08"/><axis xyz="1 0 0"/>
    <limit lower="0" upper="1.2" effort="500" velocity="1"/>
  </joint>
  <link name="sled"/>
  <joint name="base_yaw" type="revolute">
    <parent link="sled"/><child link="turret"/>
    <origin xyz="0 0 0.06"/><axis xyz="0 0 1"/>
    <limit lower="-2.97" upper="2.97" effort="150" velocity="2"/>
  </joint>
  <link name="turret"/>
  <joint name="shoulder" type="revolute">
    <parent link="turret"/><child link="upper_arm"/>
    <origin xyz="0 0 0.2"/><axis xyz="0 1 0"/>
    <limit lower="-1.6" upper="1.6" effort="120" velocity="2"/>
  </joint>
  <link name="upper_arm"/>
  <joint name="elbow" type="revolute">
    <parent link="upper_arm"/><child link="forearm"/>
    <origin xyz="0 0 0.44"/><axis xyz="0 1 0"/>
    <limit lower="-2.2" upper="2.2" effort="60" velocity="2.5"/>
  </joint>
  <link name="forearm"/>
  <joint name="wrist" type="revolute">
    <parent link="forearm"/><child link="tool"/>
    <origin xyz="0 0 0.36"/><axis xyz="0 1 0"/>
    <limit lower="-1.9" upper="1.9" effort="20" velocity="3"/>
  </joint>
  <link name="tool"/>
</robot>
"#;

pub(crate) fn load(urdf: &str) -> Document {
    let mut doc = Document::new();
    let none = |_: &Path| -> Result<ImportScene, String> {
        Ok(ImportScene::new(SourceMetadata::new("none")))
    };
    let scene = kreate_io::urdf::parse_urdf(urdf, Path::new("."), &none).expect("the URDF reads");
    kreate_io::build::build_into(&scene, &mut doc);
    doc
}

pub(crate) fn find(doc: &Document, name: &str) -> (ObjectId, u64) {
    let id = doc
        .objects()
        .find(|(_, o)| o.name == name)
        .unwrap_or_else(|| panic!("no {name}"))
        .0;
    (id, doc.object_uid(id).unwrap().raw())
}

/// Positions within the limits, the same each run.
pub(crate) fn random_q(kin: &Kinematics, rng: &mut u64) -> Vec<f64> {
    kin.vars
        .iter()
        .map(|v| {
            let u = next_unit(rng);
            // Not the full ±2π of a UR's joints: the same poses again.
            let (lo, hi) = (v.lower.max(-3.0), v.upper.min(3.0));
            lo + u * (hi - lo)
        })
        .collect()
}

/// The angle between two orientations, exactly: glam's `angle_between`
/// goes through an approximate arccosine, off by more than the tolerance.
fn turn(a: DQuat, b: DQuat) -> f64 {
    let mut d = (a * b.inverse()).normalize();
    if d.w < 0.0 {
        d = -d;
    }
    d.to_axis_angle().1
}

fn gap(a: &DAffine3, b: &DAffine3) -> f64 {
    let m = a.matrix3 - b.matrix3;
    (a.translation - b.translation)
        .abs()
        .max_element()
        .max(m.x_axis.abs().max_element())
        .max(m.y_axis.abs().max_element())
        .max(m.z_axis.abs().max_element())
}

#[test]
fn forward_puts_every_object_where_kreate_does() {
    let mut doc = load(UR5E);
    // On a stand, turned and raised: what is above the root counts too.
    let mut stand = Object::new("stand", ObjectData::Empty);
    stand.transform = DTransform {
        translation: DVec3::new(1.5, -0.4, 0.8),
        rotation: DQuat::from_rotation_z(0.7) * DQuat::from_rotation_x(0.2),
        ..DTransform::IDENTITY
    };
    let stand = doc.add_object(stand);
    let (base, root) = find(&doc, "base_link");
    doc.set_parent(base, Some(stand)).unwrap();

    let kin = Kinematics::of(&doc, root).unwrap();
    assert_eq!(kin.vars.len(), 6);
    assert_eq!(kin.vars[0].name, "shoulder_pan_joint");
    let mut rng = 7;
    for _ in 0..10 {
        let q = random_q(&kin, &mut rng);
        let changed = kin.apply(&mut doc, &q);
        assert_eq!(changed.len(), 6);
        let world = kin.forward(&q);
        for (i, node) in kin.nodes.iter().enumerate() {
            let id = doc.object_by_uid(Uid::from_raw(node.uid)).unwrap();
            let d = gap(&world[i], &doc.world_transform(id));
            assert!(d < 1e-9, "object {i} is {d} off");
        }
        // And read back, the document says the positions it was given.
        let again = Kinematics::of(&doc, root).unwrap();
        for (a, b) in again.current.iter().zip(&q) {
            assert!((a - b).abs() < 1e-9, "{a} vs {b}");
        }
    }
}

#[test]
fn the_jacobian_is_how_the_tool_moves() {
    let doc = load(TRACK);
    let (_, root) = find(&doc, "track");
    let (_, tip) = find(&doc, "tool");
    let kin = Kinematics::of(&doc, root).unwrap();
    let tool = kin
        .tool(tip, DAffine3::from_translation(DVec3::new(0.0, 0.0, 0.1)))
        .unwrap();
    let mut rng = 11;
    let q = random_q(&kin, &mut rng);
    let world = kin.forward(&q);
    let pose = world[tool.node] * tool.offset;
    let cols = kin.jacobian(&world, &tool, pose.translation);
    let h = 1e-6;
    for (i, col) in cols.iter().enumerate() {
        let mut ahead = q.clone();
        ahead[i] += h;
        let mut back = q.clone();
        back[i] -= h;
        let (a, b) = (kin.pose(&ahead, &tool), kin.pose(&back, &tool));
        let v = (a.translation - b.translation) / (2.0 * h);
        let (_, ra, _) = a.to_scale_rotation_translation();
        let (_, rb, _) = b.to_scale_rotation_translation();
        let (axis, angle) = (ra * rb.inverse()).to_axis_angle();
        let w = axis * angle / (2.0 * h);
        let want = [v.x, v.y, v.z, w.x, w.y, w.z];
        for r in 0..6 {
            assert!(
                (col[r] - want[r]).abs() < 1e-5,
                "{} row {r}: {} vs {}",
                kin.vars[i].name,
                col[r],
                want[r]
            );
        }
    }
}

#[test]
fn a_ur5e_reaches_every_pose_it_can_take() {
    let doc = load(UR5E);
    let (_, root) = find(&doc, "base_link");
    let (_, tip) = find(&doc, "tool0");
    let kin = Kinematics::of(&doc, root).unwrap();
    let tool = kin.tool(tip, DAffine3::IDENTITY).unwrap();
    let opts = Options {
        restarts: 20,
        max_iterations: 200,
        ..Options::default()
    };
    let mut rng = 3;
    for _ in 0..25 {
        let truth = random_q(&kin, &mut rng);
        let (_, rotation, position) = kin.pose(&truth, &tool).to_scale_rotation_translation();
        let goal = Goal {
            position,
            rotation: Some(rotation),
        };
        let found = kin.solve(&tool, &goal, &kin.current, &opts);
        assert!(found.reached, "missed by {found:?}");
        // What it found is a pose that is there, not just a small error.
        let (_, r, p) = kin
            .pose(&found.positions, &tool)
            .to_scale_rotation_translation();
        assert!((p - position).length() < 1e-5);
        assert!(turn(r, rotation) < 1e-4, "{found:?}");
    }
}

#[test]
fn a_small_move_from_where_it_stands_stays_in_its_configuration() {
    let doc = load(UR5E);
    let (_, root) = find(&doc, "base_link");
    let (_, tip) = find(&doc, "tool0");
    let kin = Kinematics::of(&doc, root).unwrap();
    let tool = kin.tool(tip, DAffine3::IDENTITY).unwrap();
    let start = vec![0.3, -1.2, 1.4, -1.6, -1.5, 0.2];
    let (_, rotation, position) = kin.pose(&start, &tool).to_scale_rotation_translation();
    // Five millimetres along X, as a drag would ask, with no restarts.
    let goal = Goal {
        position: position + DVec3::new(0.005, 0.0, 0.0),
        rotation: Some(rotation),
    };
    let found = kin.solve(&tool, &goal, &start, &Options::default());
    assert!(found.reached, "{found:?}");
    for (a, b) in found.positions.iter().zip(&start) {
        assert!((a - b).abs() < 0.05, "jumped from {b} to {a}");
    }
}

#[test]
fn out_of_reach_it_stretches_toward_the_goal_within_its_limits() {
    let doc = load(TRACK);
    let (_, root) = find(&doc, "track");
    let (_, tip) = find(&doc, "tool");
    let kin = Kinematics::of(&doc, root).unwrap();
    let tool = kin.tool(tip, DAffine3::IDENTITY).unwrap();
    let goal = Goal {
        position: DVec3::new(6.0, 0.0, 0.5),
        rotation: None,
    };
    let found = kin.solve(&tool, &goal, &kin.current, &Options::default());
    assert!(!found.reached);
    for (v, q) in kin.vars.iter().zip(&found.positions) {
        assert!(
            *q >= v.lower - 1e-12 && *q <= v.upper + 1e-12,
            "{} at {q}",
            v.name
        );
    }
    // The carriage runs to its end and the arm reaches out: about 6 m less
    // the track's 1.4 m and the arm's 0.8 m.
    assert!((found.positions[0] - 1.2).abs() < 1e-9, "{found:?}");
    assert!(found.position_error < 6.0 - 1.4 - 0.75, "{found:?}");
}

#[test]
fn a_point_alone_is_reached_by_an_arm_with_fewer_than_six_joints() {
    let doc = load(TRACK);
    let (_, root) = find(&doc, "track");
    let (_, tip) = find(&doc, "tool");
    let kin = Kinematics::of(&doc, root).unwrap();
    let tool = kin.tool(tip, DAffine3::IDENTITY).unwrap();
    let mut rng = 5;
    for _ in 0..10 {
        let truth = random_q(&kin, &mut rng);
        let goal = Goal {
            position: kin.pose(&truth, &tool).translation,
            rotation: None,
        };
        let found = kin.solve(
            &tool,
            &goal,
            &kin.current,
            &Options {
                restarts: 10,
                ..Options::default()
            },
        );
        assert!(found.reached, "{found:?}");
        assert_eq!(found.rotation_error, 0.0);
    }
}

#[test]
fn a_mimic_joint_follows_its_leader() {
    let mut doc = load(TRACK);
    let (wrist, _) = find(&doc, "wrist");
    if let Some(RobotPart::Joint(j)) = &mut doc.object_mut(wrist).unwrap().robot {
        j.mimic = Some(Mimic {
            joint: "elbow".into(),
            multiplier: -0.5,
            offset: 0.1,
        });
    }
    let (_, root) = find(&doc, "track");
    let kin = Kinematics::of(&doc, root).unwrap();
    assert_eq!(kin.vars.len(), 4, "the wrist is no longer free");
    let q = vec![0.3, 0.2, 0.4, 1.0];
    kin.apply(&mut doc, &q);
    let RobotPart::Joint(j) = doc.object(wrist).unwrap().robot.clone().unwrap() else {
        unreachable!()
    };
    let at = j.position_of(&doc.object(wrist).unwrap().transform);
    assert!((at - (-0.5 * 1.0 + 0.1)).abs() < 1e-9, "{at}");

    // The Jacobian counts it too.
    let (_, tip) = find(&doc, "tool");
    let tool = kin
        .tool(tip, DAffine3::from_translation(DVec3::new(0.0, 0.0, 0.2)))
        .unwrap();
    let world = kin.forward(&q);
    let pose = world[tool.node] * tool.offset;
    let cols = kin.jacobian(&world, &tool, pose.translation);
    let h = 1e-6;
    let mut ahead = q.clone();
    ahead[3] += h;
    let mut back = q.clone();
    back[3] -= h;
    let v = (kin.pose(&ahead, &tool).translation - kin.pose(&back, &tool).translation) / (2.0 * h);
    assert!((DVec3::new(cols[3][0], cols[3][1], cols[3][2]) - v).length() < 1e-5);
}

#[test]
fn with_takes_positions_by_joint_and_holds_them_to_the_limits() {
    let doc = load(TRACK);
    let (_, root) = find(&doc, "track");
    let kin = Kinematics::of(&doc, root).unwrap();
    let carriage = kin.vars[0].uid;
    let elbow = kin.vars[3].uid;
    let q = kin.with(&BTreeMap::from([
        (carriage, 5.0),
        (elbow, -0.3),
        (12345, 1.0),
    ]));
    assert_eq!(q[0], 1.2);
    assert_eq!(q[3], -0.3);
    assert_eq!(q[1], kin.current[1]);
}

#[test]
fn spd_solve() {
    let mut a = [[0.0; 6]; 6];
    a[0] = [4.0, 1.0, 0.0, 0.0, 0.0, 0.0];
    a[1] = [1.0, 3.0, 0.5, 0.0, 0.0, 0.0];
    a[2] = [0.0, 0.5, 2.0, 0.0, 0.0, 0.0];
    let y = solve_spd(a, [1.0, 2.0, 3.0, 0.0, 0.0, 0.0], 3).unwrap();
    for (r, want) in [1.0, 2.0, 3.0].into_iter().enumerate() {
        let got: f64 = (0..3).map(|c| a[r][c] * y[c]).sum();
        assert!((got - want).abs() < 1e-12);
    }
}

#[test]
fn a_symmetric_matrix_comes_apart_into_its_eigenvalues() {
    let a = vec![
        vec![4.0, 1.0, 0.5],
        vec![1.0, 3.0, -0.2],
        vec![0.5, -0.2, 1.0],
    ];
    let (values, vectors) = crate::symmetric_eigen(a.clone());
    for k in 0..3 {
        // A v = λ v, for each column v.
        for i in 0..3 {
            let av: f64 = (0..3).map(|j| a[i][j] * vectors[j][k]).sum();
            assert!((av - values[k] * vectors[i][k]).abs() < 1e-9, "{values:?}");
        }
    }
    let trace: f64 = values.iter().sum();
    assert!((trace - 8.0).abs() < 1e-9);
}

#[test]
fn an_arm_at_rest_is_dexterous_and_one_with_its_wrist_straight_is_not() {
    let doc = load(UR5E);
    let (_, root) = find(&doc, "base_link");
    let (_, tip) = find(&doc, "tool0");
    let kin = Kinematics::of(&doc, root).unwrap();
    let tool = kin.tool(tip, DAffine3::IDENTITY).unwrap();
    let rest = kin
        .dexterity(&[0.0, -1.2, 1.4, -1.6, -1.5, 0.0], &tool)
        .unwrap();
    assert!(rest.least > 5.0 * crate::path::SINGULAR, "{rest:?}");
    let straight = kin
        .dexterity(&[0.0, -1.2, 1.4, -1.6, 0.0, 0.0], &tool)
        .unwrap();
    assert!(straight.least < 1e-6, "{straight:?}");
}
