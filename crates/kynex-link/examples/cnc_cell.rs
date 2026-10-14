//! A CNC machine-tending cell, hosted as a Kreate session: a UR10e with a
//! double gripper on a pedestal, two CNC machines with automatic doors and
//! chucks, an infeed and an outfeed pallet, and a safety fence. Nothing
//! moves on its own: the cell is to be set up and programmed in the viewer.
//!
//! ```text
//! cargo run -p kynex-link --example cnc_cell -- [--port 7878] [--key KEY]
//! kynex view 127.0.0.1:7878 --key KEY
//! ```
//!
//! Sizes and times are the roadmap's industry-typical values
//! (docs/roadmap-vs-visual-components.md §4).

use std::path::Path;
use std::time::Duration;

use kreate_collab::Session;
use kreate_io::ir::{ImportScene, SourceMetadata};
use kreate_math::{DQuat, DVec3, SceneRgb};
use kreate_scene::robot::RobotPart;
use kreate_scene::{Document, Material, MaterialId, Object, ObjectData, ObjectId};

/// A UR10e: `ur_description`'s joint origins and limits, its links drawn
/// as plain shapes, and a double gripper on the flange: two parallel
/// grippers 140 mm apart, their grip points `grip_a` and `grip_b`.
const UR10E: &str = include_str!("../../../apps/kynex/library/ur10e_tending.urdf");

/// Where the robot stands, its joints: the tool down over the cell.
const HOME: [(&str, f64); 6] = [
    ("shoulder_pan_joint", 0.0),
    ("shoulder_lift_joint", -1.75),
    ("elbow_joint", 1.75),
    ("wrist_1_joint", -1.571),
    ("wrist_2_joint", -1.571),
    ("wrist_3_joint", 0.0),
];

/// The pedestal's height: the robot's base stands on it.
const PEDESTAL: f64 = 0.7;

struct Colours {
    machine: MaterialId,
    door: MaterialId,
    steel: MaterialId,
    table: MaterialId,
    pallet: MaterialId,
    dark: MaterialId,
    fence: MaterialId,
}

fn colour(doc: &mut Document, name: &str, rgb: [f32; 3], metallic: f32) -> MaterialId {
    doc.add_material(Material {
        name: name.into(),
        base_color: SceneRgb::new(rgb[0], rgb[1], rgb[2]),
        metallic,
        ..Material::default()
    })
}

/// A box `size` across, its centre at `centre`, in `colour`; under `parent`
/// if given (parenting keeps it where it is).
fn block(
    doc: &mut Document,
    name: &str,
    size: [f64; 3],
    centre: [f64; 3],
    colour: MaterialId,
    parent: Option<ObjectId>,
) -> ObjectId {
    let id = doc.add_mesh_object(name, kreate_mesh::primitives::cube(1.0));
    paint(doc, id, colour);
    let o = doc.object_mut(id).unwrap();
    o.transform.scale = DVec3::from_array(size);
    o.transform.translation = DVec3::from_array(centre);
    if parent.is_some() {
        doc.set_parent(id, parent).unwrap();
    }
    id
}

/// An upright cylinder, its centre at `centre`.
fn post(
    doc: &mut Document,
    name: &str,
    radius: f32,
    height: f32,
    centre: [f64; 3],
    colour: MaterialId,
    parent: Option<ObjectId>,
) -> ObjectId {
    let id = doc.add_mesh_object(name, kreate_mesh::primitives::cylinder(radius, height, 32));
    paint(doc, id, colour);
    doc.object_mut(id).unwrap().transform.translation = DVec3::from_array(centre);
    if parent.is_some() {
        doc.set_parent(id, parent).unwrap();
    }
    id
}

fn paint(doc: &mut Document, id: ObjectId, colour: MaterialId) {
    if let Some(mesh) = doc.object(id).and_then(|o| o.mesh()) {
        doc.set_mesh_slot(mesh, 0, Some(colour));
    }
}

/// A small CNC machine, built facing −Y at the origin and then set at
/// `at`, turned `turn` about Z: a cabinet with a working space open at the
/// front behind a sliding door, and a chuck on the working space's floor.
/// Its parts are named after it: `{name}_door`, `{name}_chuck`.
fn cnc(doc: &mut Document, c: &Colours, name: &str, at: DVec3, turn: f64) -> ObjectId {
    let machine = doc.add_object(Object::new(name, ObjectData::Empty));
    let m = Some(machine);
    // 1.8 m wide, 1.6 m deep, 1.9 m tall; the front at y = 0.
    let (w, d) = (1.8, 1.6);
    let floor = 0.85;
    let roof = 1.55;
    let opening = 0.6;
    let side = (w - opening) / 2.0;
    block(
        doc,
        &format!("{name}_base"),
        [w, d, floor],
        [0.0, d / 2.0, floor / 2.0],
        c.machine,
        m,
    );
    block(
        doc,
        &format!("{name}_top"),
        [w, d, 1.9 - roof],
        [0.0, d / 2.0, (roof + 1.9) / 2.0],
        c.machine,
        m,
    );
    let mid = (floor + roof) / 2.0;
    let tall = roof - floor;
    block(
        doc,
        &format!("{name}_left"),
        [side, d, tall],
        [-(opening + side) / 2.0, d / 2.0, mid],
        c.machine,
        m,
    );
    block(
        doc,
        &format!("{name}_right"),
        [side, d, tall],
        [(opening + side) / 2.0, d / 2.0, mid],
        c.machine,
        m,
    );
    // The working space is 0.9 m deep; behind it, the spindle housing.
    block(
        doc,
        &format!("{name}_back"),
        [opening, d - 0.9, tall],
        [0.0, 0.9 + (d - 0.9) / 2.0, mid],
        c.machine,
        m,
    );
    block(
        doc,
        &format!("{name}_panel"),
        [0.35, 0.06, 0.5],
        [side / 2.0 + opening / 2.0 + 0.1, -0.03, 1.25],
        c.dark,
        m,
    );
    // The chuck, 0.35 m in from the front, its top 0.12 m above the floor.
    post(
        doc,
        &format!("{name}_chuck"),
        0.09,
        0.12,
        [0.0, 0.35, floor + 0.06],
        c.steel,
        m,
    );
    // The door, closed: it slides open along +X.
    block(
        doc,
        &format!("{name}_door"),
        [opening + 0.04, 0.03, tall + 0.04],
        [0.0, -0.02, mid],
        c.door,
        m,
    );
    let o = doc.object_mut(machine).unwrap();
    o.transform.translation = at;
    o.transform.rotation = DQuat::from_rotation_z(turn);
    machine
}

/// A table with a pallet plate on it, `name` the plate: 0.55 × 0.45 m, its
/// top 0.77 m up.
fn station(doc: &mut Document, c: &Colours, name: &str, at: [f64; 2]) -> ObjectId {
    block(
        doc,
        &format!("{name}_table"),
        [0.6, 0.5, 0.75],
        [at[0], at[1], 0.375],
        c.table,
        None,
    );
    block(
        doc,
        name,
        [0.55, 0.45, 0.02],
        [at[0], at[1], 0.76],
        c.pallet,
        None,
    )
}

fn build() -> Document {
    let mut doc = Document::new();
    let c = Colours {
        machine: colour(&mut doc, "machine", [0.86, 0.87, 0.88], 0.0),
        door: colour(&mut doc, "door", [0.32, 0.40, 0.50], 0.2),
        steel: colour(&mut doc, "steel", [0.72, 0.74, 0.78], 0.8),
        table: colour(&mut doc, "table", [0.28, 0.30, 0.33], 0.0),
        pallet: colour(&mut doc, "pallet", [0.20, 0.45, 0.75], 0.0),
        dark: colour(&mut doc, "dark", [0.12, 0.13, 0.15], 0.0),
        fence: colour(&mut doc, "fence", [0.95, 0.75, 0.10], 0.0),
    };

    // The robot on its pedestal, at the origin.
    block(
        &mut doc,
        "pedestal",
        [0.4, 0.4, PEDESTAL],
        [0.0, 0.0, PEDESTAL / 2.0],
        c.dark,
        None,
    );
    let load = |_: &Path| -> Result<ImportScene, String> {
        Ok(ImportScene::new(SourceMetadata::new("none")))
    };
    let robot = kreate_io::urdf::parse_urdf(UR10E, Path::new("."), &load).expect("the UR10e reads");
    kreate_io::build::build_into(&robot, &mut doc);
    let find = |doc: &Document, name: &str| {
        doc.objects()
            .find(|(_, o)| o.name == name)
            .map(|(id, _)| id)
            .unwrap_or_else(|| panic!("no {name}"))
    };
    let base = find(&doc, "base_link");
    doc.object_mut(base).unwrap().transform.translation = DVec3::new(0.0, 0.0, PEDESTAL);
    for (name, position) in HOME {
        let id = find(&doc, name);
        let Some(RobotPart::Joint(j)) = doc.object(id).and_then(|o| o.robot.clone()) else {
            continue;
        };
        doc.object_mut(id).unwrap().transform = j.place(position);
    }

    // Two machines, their fronts 0.75 m from the robot: one ahead (+Y),
    // one to the right (+X), both facing it.
    cnc(&mut doc, &c, "cnc_1", DVec3::new(0.0, 0.75, 0.0), 0.0);
    cnc(
        &mut doc,
        &c,
        "cnc_2",
        DVec3::new(0.75, 0.0, 0.0),
        // Its −Y front turned to face −X: its body runs away along +X.
        -std::f64::consts::FRAC_PI_2,
    );

    // Raw parts in on the left, finished parts out behind.
    station(&mut doc, &c, "infeed", [-0.8, 0.05]);
    station(&mut doc, &c, "outfeed", [0.0, -0.8]);

    // A fence round the cell, 3.6 m square: posts and rails.
    let half = 1.9;
    for i in 0..4 {
        for j in 0..4 {
            let (x, y) = (
                -half + i as f64 * (2.0 * half / 3.0),
                -half + j as f64 * (2.0 * half / 3.0),
            );
            if i == 0 || i == 3 || j == 0 || j == 3 {
                post(
                    &mut doc,
                    &format!("fence_post_{i}_{j}"),
                    0.03,
                    2.0,
                    [x, y, 1.0],
                    c.fence,
                    None,
                );
            }
        }
    }
    for (k, z) in [0.15, 1.0, 1.95].into_iter().enumerate() {
        block(
            &mut doc,
            &format!("fence_rail_n{k}"),
            [2.0 * half, 0.03, 0.04],
            [0.0, half, z],
            c.fence,
            None,
        );
        block(
            &mut doc,
            &format!("fence_rail_s{k}"),
            [2.0 * half, 0.03, 0.04],
            [0.0, -half, z],
            c.fence,
            None,
        );
        block(
            &mut doc,
            &format!("fence_rail_e{k}"),
            [0.03, 2.0 * half, 0.04],
            [half, 0.0, z],
            c.fence,
            None,
        );
        block(
            &mut doc,
            &format!("fence_rail_w{k}"),
            [0.03, 2.0 * half, 0.04],
            [-half, 0.0, z],
            c.fence,
            None,
        );
    }
    doc
}

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
    let mut doc = build();
    let mut session = Session::host(port, "CNC cell", &key).expect("could not host the session");
    println!(
        "CNC 셀 세션: {} (키: {})",
        session.address,
        if key.is_empty() { "없음" } else { &key }
    );
    loop {
        for event in session.tick(&mut doc, None) {
            println!("{event:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cell_builds() {
        let doc = build();
        let names: Vec<String> = doc.objects().map(|(_, o)| o.name.clone()).collect();
        for want in [
            "cnc_1_door",
            "cnc_2_chuck",
            "infeed",
            "outfeed",
            "grip_a",
            "grip_b",
        ] {
            assert!(names.iter().any(|n| n == want), "no {want}");
        }
    }
}
