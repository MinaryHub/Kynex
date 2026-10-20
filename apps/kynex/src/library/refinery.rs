//! An oil refinery's equipment: floating-roof storage tanks, distillation
//! columns, fired heaters, reactors, horizontal pressure vessels (a
//! desalter, a drum), a fluid catalytic cracker, air coolers, an LPG
//! sphere, a flare stack and the central control room. None has a role in
//! the cell: the refinery's work is the process flow's stations; these are
//! what it looks like. And a lot of oil going round the refinery, as a flow
//! part looks: an ISO tank container in its product's colour — crude, a
//! distilled cut, petrol, jet fuel, diesel, heavy fuel oil.
//!
//! Each stands on the ground at its middle; what has a length has it
//! along X.

use kreate_math::{DQuat, DVec3, SceneRgb};
use kreate_scene::{Document, Material, MaterialId, Object, ObjectData, ObjectId};

use super::{bar, block, paint, post, Item, LookMeshes};

/// The templates this module makes.
pub const KEYS: &[&str] = &[
    "storage_tank",
    "distillation_column",
    "fired_heater",
    "reactor",
    "horizontal_vessel",
    "fcc_unit",
    "air_cooler",
    "lpg_sphere",
    "flare_stack",
    "control_room",
];

struct Paint {
    white: MaterialId,
    steel: MaterialId,
    insulation: MaterialId,
    dark: MaterialId,
    brick: MaterialId,
    yellow: MaterialId,
    red: MaterialId,
    blue: MaterialId,
    glass: MaterialId,
    concrete: MaterialId,
}

fn material(doc: &mut Document, name: &str, rgb: [f32; 3], metallic: f32) -> MaterialId {
    doc.add_material(Material {
        name: name.into(),
        base_color: SceneRgb::new(rgb[0], rgb[1], rgb[2]),
        metallic,
        ..Material::default()
    })
}

impl Paint {
    fn new(doc: &mut Document) -> Self {
        Self {
            white: material(doc, "tank white", [0.90, 0.91, 0.90], 0.05),
            steel: material(doc, "column steel", [0.62, 0.65, 0.68], 0.6),
            insulation: material(doc, "insulation", [0.80, 0.81, 0.80], 0.5),
            dark: material(doc, "dark grey", [0.22, 0.24, 0.26], 0.2),
            brick: material(doc, "heater casing", [0.45, 0.30, 0.25], 0.2),
            yellow: material(doc, "platform yellow", [0.96, 0.76, 0.06], 0.0),
            red: material(doc, "flare red", [0.80, 0.15, 0.12], 0.0),
            blue: material(doc, "fan blue", [0.15, 0.35, 0.60], 0.1),
            glass: material(doc, "window", [0.40, 0.55, 0.65], 0.3),
            concrete: material(doc, "concrete", [0.62, 0.62, 0.60], 0.0),
        }
    }
}

/// A sphere of `radius` about `centre`.
fn sphere(
    doc: &mut Document,
    name: &str,
    radius: f64,
    centre: [f64; 3],
    colour: MaterialId,
    parent: Option<ObjectId>,
) -> ObjectId {
    let id = doc.add_mesh_object(
        name,
        kreate_mesh::primitives::uv_sphere(radius as f32, 32, 16),
    );
    paint(doc, id, colour);
    doc.object_mut(id).unwrap().transform.translation = DVec3::from_array(centre);
    if parent.is_some() {
        doc.set_parent(id, parent).unwrap();
    }
    id
}

/// Builds `item` into `doc`, if it is one of [`KEYS`].
pub(super) fn build(doc: &mut Document, item: &Item) {
    let p = Paint::new(doc);
    let root = doc.add_object(Object::new(item.template.clone(), ObjectData::Empty));
    let m = Some(root);
    match item.template.as_str() {
        "storage_tank" => storage_tank(doc, &p, item.param("diameter"), item.param("height"), m),
        "distillation_column" => column(
            doc,
            &p,
            item.param("diameter"),
            item.param("height"),
            true,
            m,
        ),
        "reactor" => column(
            doc,
            &p,
            item.param("diameter"),
            item.param("height"),
            false,
            m,
        ),
        "fired_heater" => fired_heater(
            doc,
            &p,
            [
                item.param("length"),
                item.param("width"),
                item.param("height"),
            ],
            m,
        ),
        "horizontal_vessel" => {
            horizontal_vessel(doc, &p, item.param("diameter"), item.param("length"), m)
        }
        "fcc_unit" => fcc_unit(doc, &p, m),
        "air_cooler" => air_cooler(
            doc,
            &p,
            [
                item.param("length"),
                item.param("width"),
                item.param("height"),
            ],
            m,
        ),
        "lpg_sphere" => lpg_sphere(doc, &p, item.param("diameter"), m),
        "flare_stack" => flare_stack(doc, &p, item.param("height"), m),
        "control_room" => control_room(
            doc,
            &p,
            [
                item.param("length"),
                item.param("width"),
                item.param("height"),
            ],
            m,
        ),
        _ => {}
    }
}

/// A floating-roof storage tank, `diameter` × `height` m: its shell on a
/// ring wall, its roof down inside it, its stair wound up the side, the
/// wind girder round its top.
fn storage_tank(doc: &mut Document, p: &Paint, diameter: f64, height: f64, m: Option<ObjectId>) {
    let (r, h) = (diameter.max(4.0) / 2.0, height.max(3.0));
    post(
        doc,
        "ring wall",
        r + 0.6,
        0.6,
        [0.0, 0.0, 0.3],
        p.concrete,
        m,
    );
    post(doc, "shell", r, h, [0.0, 0.0, 0.6 + h / 2.0], p.white, m);
    post(
        doc,
        "floating roof",
        r - 0.3,
        0.4,
        [0.0, 0.0, 0.6 + h - 1.6],
        p.steel,
        m,
    );
    post(
        doc,
        "wind girder",
        r + 0.5,
        0.25,
        [0.0, 0.0, 0.6 + h - 0.4],
        p.steel,
        m,
    );
    // A stair round a quarter of it, as flights of steps up the side.
    let steps = 8;
    for k in 0..steps {
        let a = std::f64::consts::FRAC_PI_2 * k as f64 / steps as f64;
        let z = 0.6 + h * (k as f64 + 0.5) / steps as f64;
        let id = block(
            doc,
            "stair",
            [1.2, 0.9, 0.15],
            [(r + 0.7) * a.cos(), (r + 0.7) * a.sin(), z],
            p.yellow,
            m,
        );
        doc.object_mut(id).unwrap().transform.rotation = DQuat::from_rotation_z(a);
    }
}

/// A column: `diameter` × `height` m on a skirt — a distillation column with
/// its platforms every 8 m, its ladder and the overhead line from its top
/// (`trays`); or, without, a reactor's shorter vessel.
fn column(
    doc: &mut Document,
    p: &Paint,
    diameter: f64,
    height: f64,
    trays: bool,
    m: Option<ObjectId>,
) {
    let (r, h) = (diameter.max(0.8) / 2.0, height.max(3.0));
    let skirt = 3.0_f64.min(h * 0.2);
    post(
        doc,
        "foundation",
        r + 1.0,
        0.5,
        [0.0, 0.0, 0.25],
        p.concrete,
        m,
    );
    post(
        doc,
        "skirt",
        r * 0.95,
        skirt,
        [0.0, 0.0, 0.5 + skirt / 2.0],
        p.steel,
        m,
    );
    let shell = if trays { p.steel } else { p.insulation };
    post(
        doc,
        "shell",
        r,
        h - skirt,
        [0.0, 0.0, 0.5 + skirt + (h - skirt) / 2.0],
        shell,
        m,
    );
    sphere(doc, "head", r, [0.0, 0.0, 0.5 + h], shell, m);
    if trays {
        let mut z = 0.5 + skirt + 6.0;
        while z < h {
            post(doc, "platform", r + 1.0, 0.15, [0.0, 0.0, z], p.yellow, m);
            z += 8.0;
        }
        block(
            doc,
            "ladder",
            [0.6, 0.2, h - skirt],
            [0.0, -(r + 0.6), 0.5 + skirt + (h - skirt) / 2.0],
            p.yellow,
            m,
        );
        // The overhead vapour line, over to the side and down.
        bar(
            doc,
            "overhead line",
            0.35,
            r + 3.0,
            [(r + 3.0) / 2.0, 0.0, 0.5 + h + r * 0.6],
            true,
            p.insulation,
            m,
        );
        post(
            doc,
            "overhead down",
            0.35,
            h * 0.6,
            [r + 3.0, 0.0, 0.5 + h + r * 0.6 - h * 0.3],
            p.insulation,
            m,
        );
    } else {
        post(
            doc,
            "top nozzle",
            0.4,
            1.2,
            [0.0, 0.0, 0.5 + h + r + 0.4],
            p.steel,
            m,
        );
        post(
            doc,
            "platform",
            r + 1.0,
            0.15,
            [0.0, 0.0, 0.5 + h],
            p.yellow,
            m,
        );
    }
}

/// A fired heater: its firebox (`size` m: length, width, height), the
/// convection section over it and the stack over that.
fn fired_heater(doc: &mut Document, p: &Paint, size: [f64; 3], m: Option<ObjectId>) {
    let [l, w, h] = [size[0].max(4.0), size[1].max(3.0), size[2].max(5.0)];
    block(
        doc,
        "firebox",
        [l, w, h * 0.55],
        [0.0, 0.0, 1.5 + h * 0.275],
        p.brick,
        m,
    );
    // It stands on legs, for its burners underneath.
    for x in [-l / 2.0 + 0.5, l / 2.0 - 0.5] {
        for y in [-w / 2.0 + 0.5, w / 2.0 - 0.5] {
            block(doc, "leg", [0.5, 0.5, 1.5], [x, y, 0.75], p.dark, m);
        }
    }
    block(
        doc,
        "convection",
        [l * 0.8, w * 0.6, h * 0.25],
        [0.0, 0.0, 1.5 + h * 0.55 + h * 0.125],
        p.steel,
        m,
    );
    post(
        doc,
        "stack",
        w * 0.15,
        h * 0.6,
        [0.0, 0.0, 1.5 + h * 0.8 + h * 0.3],
        p.dark,
        m,
    );
    block(
        doc,
        "platform",
        [l + 1.0, w + 1.0, 0.15],
        [0.0, 0.0, 1.5 + h * 0.55],
        p.yellow,
        m,
    );
}

/// A horizontal pressure vessel, `diameter` × `length` m along X, on two
/// saddles: a desalter, a drum.
fn horizontal_vessel(
    doc: &mut Document,
    p: &Paint,
    diameter: f64,
    length: f64,
    m: Option<ObjectId>,
) {
    let (r, l) = (diameter.max(0.5) / 2.0, length.max(2.0));
    let z = 1.0 + r;
    bar(doc, "shell", r, l, [0.0, 0.0, z], true, p.insulation, m);
    for x in [-l / 2.0, l / 2.0] {
        sphere(doc, "head", r, [x, 0.0, z], p.insulation, m);
    }
    for x in [-l * 0.3, l * 0.3] {
        block(
            doc,
            "saddle",
            [0.6, r * 1.6, 1.0 + r * 0.5],
            [x, 0.0, (1.0 + r * 0.5) / 2.0],
            p.concrete,
            m,
        );
    }
    post(
        doc,
        "nozzle",
        0.25,
        0.8,
        [0.0, 0.0, z + r + 0.3],
        p.steel,
        m,
    );
}

/// A fluid catalytic cracker, 26 × 16 m: the reactor and the taller
/// regenerator side by side, the riser between them, the main
/// fractionator beside, and the structure they stand in.
fn fcc_unit(doc: &mut Document, p: &Paint, m: Option<ObjectId>) {
    // The structure: four columns and floors.
    let (sx, sy) = (14.0, 12.0);
    for x in [-sx / 2.0, sx / 2.0] {
        for y in [-sy / 2.0, sy / 2.0] {
            block(
                doc,
                "steel column",
                [0.6, 0.6, 30.0],
                [x - 4.0, y, 15.0],
                p.dark,
                m,
            );
        }
    }
    for z in [10.0, 20.0, 30.0] {
        block(
            doc,
            "floor",
            [sx + 0.6, sy + 0.6, 0.3],
            [-4.0, 0.0, z],
            p.yellow,
            m,
        );
    }
    // The regenerator, broad and tall; the reactor over the structure.
    post(
        doc,
        "regenerator",
        5.0,
        28.0,
        [-8.0, 0.0, 14.0],
        p.insulation,
        m,
    );
    sphere(
        doc,
        "regenerator head",
        5.0,
        [-8.0, 0.0, 28.0],
        p.insulation,
        m,
    );
    post(doc, "reactor", 3.0, 14.0, [0.0, 0.0, 37.0], p.insulation, m);
    sphere(doc, "reactor head", 3.0, [0.0, 0.0, 44.0], p.insulation, m);
    post(doc, "riser", 0.8, 32.0, [-3.0, 0.0, 16.0], p.steel, m);
    // The main fractionator.
    post(doc, "fractionator", 3.0, 40.0, [9.0, 0.0, 20.0], p.steel, m);
    sphere(doc, "fractionator head", 3.0, [9.0, 0.0, 40.0], p.steel, m);
    post(doc, "flue stack", 1.0, 20.0, [-8.0, 0.0, 43.0], p.dark, m);
}

/// An air cooler, `size` m (length, width, height to its fans): a bank of
/// finned tubes over a row of fans, on legs.
fn air_cooler(doc: &mut Document, p: &Paint, size: [f64; 3], m: Option<ObjectId>) {
    let [l, w, h] = [size[0].max(3.0), size[1].max(2.0), size[2].max(3.0)];
    let bays = (l / 4.0).round().max(1.0) as usize;
    for k in 0..=bays {
        let x = -l / 2.0 + l * k as f64 / bays as f64;
        for y in [-w / 2.0, w / 2.0] {
            block(doc, "leg", [0.4, 0.4, h], [x, y, h / 2.0], p.dark, m);
        }
    }
    block(
        doc,
        "tube bank",
        [l, w, 1.0],
        [0.0, 0.0, h + 0.5],
        p.steel,
        m,
    );
    for k in 0..bays {
        let x = -l / 2.0 + l * (k as f64 + 0.5) / bays as f64;
        post(
            doc,
            "fan",
            (w / 2.0).min(2.0) * 0.8,
            0.3,
            [x, 0.0, h - 0.3],
            p.blue,
            m,
        );
    }
}

/// An LPG sphere, `diameter` m, on its legs, a stair up to its top.
fn lpg_sphere(doc: &mut Document, p: &Paint, diameter: f64, m: Option<ObjectId>) {
    let r = diameter.max(3.0) / 2.0;
    let z = r + 2.0;
    sphere(doc, "sphere", r, [0.0, 0.0, z], p.white, m);
    for k in 0..8 {
        let a = std::f64::consts::TAU * k as f64 / 8.0;
        post(
            doc,
            "leg",
            0.35,
            z,
            [r * 0.95 * a.cos(), r * 0.95 * a.sin(), z / 2.0],
            p.steel,
            m,
        );
    }
    post(
        doc,
        "top platform",
        1.5,
        0.2,
        [0.0, 0.0, z + r + 0.1],
        p.yellow,
        m,
    );
}

/// A flare stack, `height` m: a tall thin stack on a base, banded red and
/// white near the top, its tip.
fn flare_stack(doc: &mut Document, p: &Paint, height: f64, m: Option<ObjectId>) {
    let h = height.max(10.0);
    post(doc, "base", 3.0, 1.0, [0.0, 0.0, 0.5], p.concrete, m);
    post(doc, "stack", 0.9, h, [0.0, 0.0, 1.0 + h / 2.0], p.steel, m);
    for k in 0..4 {
        let z = 1.0 + h - 2.0 - 4.0 * k as f64;
        let colour = if k % 2 == 0 { p.red } else { p.white };
        post(doc, "band", 0.95, 2.0, [0.0, 0.0, z], colour, m);
    }
    post(doc, "tip", 1.2, 1.5, [0.0, 0.0, 1.0 + h + 0.75], p.dark, m);
}

/// The central control room, `size` m: a low blast-proof building, its
/// windows in a band.
fn control_room(doc: &mut Document, p: &Paint, size: [f64; 3], m: Option<ObjectId>) {
    let [l, w, h] = [size[0].max(5.0), size[1].max(4.0), size[2].max(3.0)];
    block(
        doc,
        "building",
        [l, w, h],
        [0.0, 0.0, h / 2.0],
        p.concrete,
        m,
    );
    block(
        doc,
        "windows",
        [l * 0.8, 0.05, h * 0.25],
        [0.0, -w / 2.0 - 0.03, h * 0.6],
        p.glass,
        m,
    );
    block(
        doc,
        "roof",
        [l + 0.4, w + 0.4, 0.3],
        [0.0, 0.0, h + 0.15],
        p.dark,
        m,
    );
}

/// A lot of oil at `stage` — crude, a distilled cut, petrol, jet fuel,
/// diesel, heavy fuel oil —, as meshes to draw a flow part
/// with: an ISO tank container — its frame, and its tank in its product's
/// colour —, in its own frame, its middle on the ground, along X.
pub(crate) fn lot_look(stage: usize) -> LookMeshes {
    let mut doc = Document::new();
    let frame = material(&mut doc, "container frame", [0.25, 0.27, 0.30], 0.3);
    let colours = [
        [0.08, 0.08, 0.09],
        [0.55, 0.45, 0.35],
        [0.95, 0.80, 0.15],
        [0.55, 0.75, 0.90],
        [0.85, 0.55, 0.15],
        [0.30, 0.20, 0.15],
    ];
    let tank = material(&mut doc, "tank", colours[stage.min(colours.len() - 1)], 0.4);
    let (l, w, h) = (6.0, 2.4, 2.4);
    for x in [-l / 2.0 + 0.1, l / 2.0 - 0.1] {
        for y in [-w / 2.0 + 0.1, w / 2.0 - 0.1] {
            block(
                &mut doc,
                "post",
                [0.2, 0.2, h],
                [x, y, h / 2.0],
                frame,
                None,
            );
        }
    }
    for z in [0.1, h - 0.1] {
        block(
            &mut doc,
            "rail",
            [l, 0.2, 0.2],
            [0.0, -w / 2.0 + 0.1, z],
            frame,
            None,
        );
        block(
            &mut doc,
            "rail",
            [l, 0.2, 0.2],
            [0.0, w / 2.0 - 0.1, z],
            frame,
            None,
        );
    }
    bar(
        &mut doc,
        "tank",
        1.1,
        l - 0.6,
        [0.0, 0.0, h / 2.0],
        true,
        tank,
        None,
    );
    super::look_of(&doc)
}
