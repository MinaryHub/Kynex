//! A Kreate session without Kreate: hosts one on this machine with a robot
//! on a linear track and keeps it moving, so the link and the viewer can be
//! tried — and shown — with nothing else running. Beside it stands a UR5e
//! that does not move, to be programmed in the viewer.
//!
//! ```text
//! cargo run -p kynex-link --example demo_session -- [--port 7878] [--key KEY]
//! kynex view 127.0.0.1:7878 --key KEY
//! ```
//!
//! It is a participant like any Kreate: what it moves reaches every viewer
//! through the same relay and patches a person's edits would.

use std::path::Path;
use std::time::{Duration, Instant};

use kreate_collab::Session;
use kreate_io::ir::{ImportScene, SourceMetadata};
use kreate_math::DVec3;
use kreate_scene::robot::RobotPart;
use kreate_scene::{Document, ObjectId};

const ROBOT: &str = r#"<?xml version="1.0"?>
<robot name="track_arm">
  <material name="rail"><color rgba="0.32 0.35 0.40 1"/></material>
  <material name="orange"><color rgba="1.0 0.45 0.12 1"/></material>
  <material name="dark"><color rgba="0.16 0.17 0.19 1"/></material>
  <material name="steel"><color rgba="0.72 0.74 0.78 1"/></material>

  <link name="track">
    <inertial><mass value="40"/><inertia ixx="1" ixy="0" ixz="0" iyy="1" iyz="0" izz="1"/></inertial>
    <visual><origin xyz="0.8 0 0.04"/><geometry><box size="2.0 0.3 0.08"/></geometry><material name="rail"/></visual>
  </link>
  <joint name="carriage" type="prismatic">
    <parent link="track"/><child link="sled"/>
    <origin xyz="0.2 0 0.08"/><axis xyz="1 0 0"/>
    <limit lower="0" upper="1.2" effort="500" velocity="1"/>
  </joint>
  <link name="sled">
    <inertial><mass value="8"/><inertia ixx="0.1" ixy="0" ixz="0" iyy="0.1" iyz="0" izz="0.1"/></inertial>
    <visual><origin xyz="0 0 0.03"/><geometry><box size="0.32 0.34 0.06"/></geometry><material name="dark"/></visual>
  </link>
  <joint name="base_yaw" type="revolute">
    <parent link="sled"/><child link="turret"/>
    <origin xyz="0 0 0.06"/><axis xyz="0 0 1"/>
    <limit lower="-2.97" upper="2.97" effort="150" velocity="2"/>
  </joint>
  <link name="turret">
    <inertial><mass value="6"/><inertia ixx="0.05" ixy="0" ixz="0" iyy="0.05" iyz="0" izz="0.05"/></inertial>
    <visual><origin xyz="0 0 0.1"/><geometry><cylinder radius="0.11" length="0.2"/></geometry><material name="orange"/></visual>
  </link>
  <joint name="shoulder" type="revolute">
    <parent link="turret"/><child link="upper_arm"/>
    <origin xyz="0 0 0.2"/><axis xyz="0 1 0"/>
    <limit lower="-1.6" upper="1.6" effort="120" velocity="2"/>
  </joint>
  <link name="upper_arm">
    <inertial><mass value="4"/><inertia ixx="0.04" ixy="0" ixz="0" iyy="0.04" iyz="0" izz="0.01"/></inertial>
    <visual><origin xyz="0 0 0.22"/><geometry><box size="0.1 0.12 0.48"/></geometry><material name="orange"/></visual>
  </link>
  <joint name="elbow" type="revolute">
    <parent link="upper_arm"/><child link="forearm"/>
    <origin xyz="0 0 0.44"/><axis xyz="0 1 0"/>
    <limit lower="-2.2" upper="2.2" effort="60" velocity="2.5"/>
  </joint>
  <link name="forearm">
    <inertial><mass value="2.5"/><inertia ixx="0.02" ixy="0" ixz="0" iyy="0.02" iyz="0" izz="0.005"/></inertial>
    <visual><origin xyz="0 0 0.18"/><geometry><box size="0.08 0.09 0.38"/></geometry><material name="orange"/></visual>
  </link>
  <joint name="wrist" type="revolute">
    <parent link="forearm"/><child link="tool"/>
    <origin xyz="0 0 0.36"/><axis xyz="0 1 0"/>
    <limit lower="-1.9" upper="1.9" effort="20" velocity="3"/>
  </joint>
  <link name="tool">
    <inertial><mass value="0.8"/><inertia ixx="0.002" ixy="0" ixz="0" iyy="0.002" iyz="0" izz="0.001"/></inertial>
    <visual><origin xyz="0 0 0.05"/><geometry><cylinder radius="0.035" length="0.1"/></geometry><material name="steel"/></visual>
  </link>
</robot>
"#;

/// A UR5e: `ur_description`'s joint origins and limits, its links drawn as
/// plain shapes. `base` is the controller's base frame, `tool0` its flange.
const UR5E: &str = r#"<?xml version="1.0"?>
<robot name="ur5e">
  <material name="ur_blue"><color rgba="0.35 0.62 0.86 1"/></material>
  <material name="ur_grey"><color rgba="0.78 0.80 0.82 1"/></material>
  <material name="ur_dark"><color rgba="0.20 0.21 0.23 1"/></material>
  <link name="base_link">
    <visual><origin xyz="0 0 0.045"/><geometry><cylinder radius="0.075" length="0.09"/></geometry><material name="ur_dark"/></visual>
  </link>
  <joint name="base_link-base_fixed_joint" type="fixed">
    <parent link="base_link"/><child link="base"/><origin rpy="0 0 3.141592654"/>
  </joint>
  <link name="base"/>
  <joint name="shoulder_pan_joint" type="revolute">
    <parent link="base_link"/><child link="shoulder_link"/>
    <origin xyz="0 0 0.1625"/><axis xyz="0 0 1"/>
    <limit lower="-6.283" upper="6.283" effort="150" velocity="3.14"/>
  </joint>
  <link name="shoulder_link">
    <visual><origin xyz="0 0 -0.04"/><geometry><cylinder radius="0.062" length="0.13"/></geometry><material name="ur_blue"/></visual>
  </link>
  <joint name="shoulder_lift_joint" type="revolute">
    <parent link="shoulder_link"/><child link="upper_arm_link"/>
    <origin rpy="1.570796327 0 0"/><axis xyz="0 0 1"/>
    <limit lower="-6.283" upper="6.283" effort="150" velocity="3.14"/>
  </joint>
  <link name="upper_arm_link">
    <visual><origin xyz="-0.2125 0 0.135" rpy="0 1.570796327 0"/><geometry><cylinder radius="0.048" length="0.425"/></geometry><material name="ur_grey"/></visual>
    <visual><origin xyz="0 0 0.07"/><geometry><cylinder radius="0.062" length="0.14"/></geometry><material name="ur_blue"/></visual>
  </link>
  <joint name="elbow_joint" type="revolute">
    <parent link="upper_arm_link"/><child link="forearm_link"/>
    <origin xyz="-0.425 0 0"/><axis xyz="0 0 1"/>
    <limit lower="-3.1416" upper="3.1416" effort="150" velocity="3.14"/>
  </joint>
  <link name="forearm_link">
    <visual><origin xyz="-0.196 0 0.02" rpy="0 1.570796327 0"/><geometry><cylinder radius="0.038" length="0.392"/></geometry><material name="ur_grey"/></visual>
    <visual><origin xyz="0 0 0.06"/><geometry><cylinder radius="0.05" length="0.12"/></geometry><material name="ur_blue"/></visual>
  </link>
  <joint name="wrist_1_joint" type="revolute">
    <parent link="forearm_link"/><child link="wrist_1_link"/>
    <origin xyz="-0.3922 0 0.1333"/><axis xyz="0 0 1"/>
    <limit lower="-6.283" upper="6.283" effort="28" velocity="6.28"/>
  </joint>
  <link name="wrist_1_link">
    <visual><geometry><cylinder radius="0.04" length="0.11"/></geometry><material name="ur_blue"/></visual>
  </link>
  <joint name="wrist_2_joint" type="revolute">
    <parent link="wrist_1_link"/><child link="wrist_2_link"/>
    <origin xyz="0 -0.0997 0" rpy="1.570796327 0 0"/><axis xyz="0 0 1"/>
    <limit lower="-6.283" upper="6.283" effort="28" velocity="6.28"/>
  </joint>
  <link name="wrist_2_link">
    <visual><geometry><cylinder radius="0.04" length="0.11"/></geometry><material name="ur_blue"/></visual>
  </link>
  <joint name="wrist_3_joint" type="revolute">
    <parent link="wrist_2_link"/><child link="wrist_3_link"/>
    <origin xyz="0 0.0996 0" rpy="1.570796327 3.141592654 3.141592654"/><axis xyz="0 0 1"/>
    <limit lower="-6.283" upper="6.283" effort="28" velocity="6.28"/>
  </joint>
  <link name="wrist_3_link">
    <visual><origin xyz="0 0 -0.02"/><geometry><cylinder radius="0.038" length="0.04"/></geometry><material name="ur_dark"/></visual>
  </link>
  <joint name="wrist_3-flange" type="fixed">
    <parent link="wrist_3_link"/><child link="tool0"/>
  </joint>
  <link name="tool0">
    <visual><origin xyz="0 0 0.04"/><geometry><box size="0.03 0.08 0.08"/></geometry><material name="ur_dark"/></visual>
  </link>
</robot>
"#;

/// The UR5e's pose as it stands: elbow up, the tool pointing down.
const UR_HOME: [(&str, f64); 6] = [
    ("shoulder_pan_joint", 0.0),
    ("shoulder_lift_joint", -1.9),
    ("elbow_joint", 1.9),
    ("wrist_1_joint", -1.571),
    ("wrist_2_joint", -1.571),
    ("wrist_3_joint", 0.0),
];

fn main() {
    let mut port = kreate_collab::DEFAULT_PORT;
    let mut key = String::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" => port = args.next().and_then(|p| p.parse().ok()).unwrap_or(port),
            "--key" => key = args.next().unwrap_or_default(),
            other => {
                eprintln!("unknown argument {other:?}");
                std::process::exit(2);
            }
        }
    }

    let mut doc = Document::new();
    let load = |_: &Path| -> Result<ImportScene, String> {
        Ok(ImportScene::new(SourceMetadata::new("none")))
    };
    let scene =
        kreate_io::urdf::parse_urdf(ROBOT, Path::new("."), &load).expect("the demo robot reads");
    kreate_io::build::build_into(&scene, &mut doc);
    let ur = kreate_io::urdf::parse_urdf(UR5E, Path::new("."), &load).expect("the demo UR5e reads");
    kreate_io::build::build_into(&ur, &mut doc);
    // Beside the pallet, at its home pose.
    let ur_base = doc
        .objects()
        .find(|(_, o)| o.name == "base_link")
        .map(|(id, _)| id)
        .expect("the UR5e's base");
    doc.object_mut(ur_base).unwrap().transform.translation = DVec3::new(0.95, -0.75, 0.0);
    for (name, position) in UR_HOME {
        let id = doc
            .objects()
            .find(|(_, o)| o.name == name)
            .map(|(id, _)| id)
            .expect("a UR5e joint");
        let Some(RobotPart::Joint(j)) = doc.object(id).and_then(|o| o.robot.clone()) else {
            continue;
        };
        doc.object_mut(id).unwrap().transform = j.place(position);
    }
    for (name, size, at) in [
        (
            "conveyor",
            DVec3::new(0.5, 1.6, 0.7),
            DVec3::new(0.9, 1.3, 0.35),
        ),
        (
            "pallet",
            DVec3::new(0.6, 0.6, 0.12),
            DVec3::new(0.2, -0.75, 0.06),
        ),
        (
            "part",
            DVec3::new(0.12, 0.12, 0.12),
            DVec3::new(0.9, 1.9, 0.76),
        ),
    ] {
        let id = doc.add_mesh_object(name, kreate_mesh::primitives::cube(1.0));
        let o = doc.object_mut(id).unwrap();
        o.transform.scale = size;
        o.transform.translation = at;
    }

    let joint = |doc: &Document, name: &str| -> ObjectId {
        doc.objects()
            .find(|(_, o)| o.name == name && matches!(o.robot, Some(RobotPart::Joint(_))))
            .map(|(id, _)| id)
            .unwrap_or_else(|| panic!("no joint {name}"))
    };
    let joints: Vec<(ObjectId, &str)> = ["carriage", "base_yaw", "shoulder", "elbow", "wrist"]
        .into_iter()
        .map(|n| (joint(&doc, n), n))
        .collect();
    let part = doc.objects().find(|(_, o)| o.name == "part").unwrap().0;

    let mut session = Session::host(port, "Demo", &key).expect("could not host the session");
    println!(
        "데모 세션: {} (키: {})",
        session.address,
        if key.is_empty() { "없음" } else { &key }
    );
    println!(
        "다른 창에서: kynex view 127.0.0.1:{port}{}",
        if key.is_empty() {
            String::new()
        } else {
            format!(" --key {key}")
        }
    );

    let start = Instant::now();
    loop {
        let t = start.elapsed().as_secs_f64();
        for (id, name) in &joints {
            let position = match *name {
                "carriage" => 0.6 + 0.55 * (t * 0.35).sin(),
                "base_yaw" => 1.1 * (t * 0.5).sin(),
                "shoulder" => 0.5 + 0.55 * (t * 0.8).sin(),
                // Past its upper limit for a moment each cycle, so the
                // viewer's warning has something to show.
                "elbow" => 1.15 + 1.15 * (t * 0.8 + 1.0).sin(),
                _ => 0.6 * (t * 1.3).sin(),
            };
            let Some(RobotPart::Joint(j)) = doc.object(*id).and_then(|o| o.robot.clone()) else {
                continue;
            };
            doc.object_mut(*id).unwrap().transform = j.place(position);
        }
        // A part riding the conveyor.
        let y = 2.0 - (t * 0.25) % 1.4;
        doc.object_mut(part).unwrap().transform.translation.y = y;

        for event in session.tick(&mut doc, None) {
            println!("{event:?}");
        }
        std::thread::sleep(Duration::from_millis(33));
    }
}
