//! A display fab's equipment, and what it makes: the cluster tools that
//! deposit and dry-etch, the coater/developer track and the exposure tool
//! of the photo bay, the wet benches that clean, etch and strip, the
//! cassette stockers between bays, the inspection gantries, and the rail the
//! overhead hoists run on; and the mother glass, the panels cut from it and
//! the modules built on them, as a flow part looks at each stage.
//!
//! The tools are white with blue trim, as a clean room's are. Each stands on
//! the floor at its middle, the glass going through it along +X; its
//! working height, where a station on a floor mark is placed in it, is 1 m.

use kreate_math::SceneRgb;
use kreate_scene::{Document, Material, MaterialId, Object, ObjectData, ObjectId};

use super::{bar, block, post, Item, LookMeshes};

/// The templates this module makes.
pub const KEYS: &[&str] = &[
    "cluster_tool",
    "track_tool",
    "exposure_tool",
    "wet_station",
    "stocker",
    "inspection_tool",
    "oht_rail",
    "crane_rail",
];

/// What a cluster tool does, by number: only its name differs.
pub const PROCESSES: &[&str] = &["CVD", "스퍼터 (PVD)", "건식 식각"];

/// The colours of a clean room.
struct Paint {
    white: MaterialId,
    trim: MaterialId,
    grey: MaterialId,
    dark: MaterialId,
    steel: MaterialId,
    glass: MaterialId,
    granite: MaterialId,
    liquid: MaterialId,
    yellow: MaterialId,
    green: MaterialId,
    red: MaterialId,
    amber: MaterialId,
    cassette: MaterialId,
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
            white: material(doc, "tool white", [0.92, 0.93, 0.94], 0.05),
            trim: material(doc, "tool trim", [0.16, 0.36, 0.66], 0.1),
            grey: material(doc, "tool grey", [0.70, 0.72, 0.74], 0.2),
            dark: material(doc, "dark grey", [0.20, 0.22, 0.25], 0.2),
            steel: material(doc, "stainless", [0.75, 0.77, 0.80], 0.8),
            glass: material(doc, "window", [0.40, 0.55, 0.65], 0.3),
            granite: material(doc, "granite", [0.16, 0.17, 0.18], 0.0),
            liquid: material(doc, "bath", [0.30, 0.55, 0.75], 0.2),
            yellow: material(doc, "rail yellow", [0.96, 0.76, 0.06], 0.0),
            green: material(doc, "lamp green", [0.15, 0.75, 0.30], 0.0),
            red: material(doc, "lamp red", [0.85, 0.12, 0.10], 0.0),
            amber: material(doc, "lamp amber", [0.98, 0.65, 0.10], 0.0),
            cassette: material(doc, "cassette", [0.30, 0.32, 0.36], 0.3),
        }
    }
}

/// Builds `item` into `doc`, if it is one of [`KEYS`].
pub(super) fn build(doc: &mut Document, item: &Item) {
    let p = Paint::new(doc);
    let root = doc.add_object(Object::new(item.template.clone(), ObjectData::Empty));
    let m = Some(root);
    match item.template.as_str() {
        "cluster_tool" => {
            let chambers = item.param("chambers").round().clamp(2.0, 6.0) as usize;
            cluster_tool(doc, &p, chambers, m);
        }
        "track_tool" => track_tool(doc, &p, item.param("length"), m),
        "exposure_tool" => exposure_tool(doc, &p, m),
        "wet_station" => wet_station(doc, &p, item.param("length"), m),
        "stocker" => stocker(doc, &p, item.param("length"), item.param("height"), m),
        "inspection_tool" => inspection_tool(doc, &p, m),
        "oht_rail" => oht_rail(
            doc,
            &p,
            item.param("length"),
            item.param("width"),
            item.param("height"),
            m,
        ),
        "crane_rail" => crane_rail(doc, &p, item.param("length"), m),
        _ => {}
    }
}

/// A tool's signal tower, `h` up at (x, y): green, amber, red.
fn tower(doc: &mut Document, p: &Paint, x: f64, y: f64, h: f64, m: Option<ObjectId>) {
    post(doc, "tower_post", 0.02, 0.25, [x, y, h + 0.125], p.dark, m);
    for (k, c) in [p.green, p.amber, p.red].into_iter().enumerate() {
        post(
            doc,
            &format!("tower_{k}"),
            0.045,
            0.08,
            [x, y, h + 0.3 + 0.085 * k as f64],
            c,
            m,
        );
    }
}

/// A cluster tool: the glass comes in at its front (−X) through the load
/// ports and the equipment front end, then a load lock, into the transfer
/// chamber in the middle, its process chambers all round, their pumps
/// below and the gas boxes behind.
fn cluster_tool(doc: &mut Document, p: &Paint, chambers: usize, m: Option<ObjectId>) {
    // The front end, its load ports on the aisle.
    block(doc, "efem", [1.6, 4.0, 2.4], [-4.6, 0.0, 1.2], p.white, m);
    block(
        doc,
        "efem_window",
        [0.02, 3.2, 1.0],
        [-5.41, 0.0, 1.6],
        p.glass,
        m,
    );
    for (k, y) in [-1.2, 0.0, 1.2].into_iter().enumerate() {
        block(
            doc,
            &format!("load_port_{k}"),
            [0.6, 1.0, 1.0],
            [-5.7, y, 0.5],
            p.grey,
            m,
        );
        block(
            doc,
            &format!("cassette_{k}"),
            [0.5, 0.9, 0.5],
            [-5.7, y, 1.25],
            p.cassette,
            m,
        );
    }
    block(
        doc,
        "load_lock",
        [1.4, 2.4, 1.4],
        [-3.1, 0.0, 1.0],
        p.grey,
        m,
    );
    // The transfer chamber: an octagon, its robot inside under the lid.
    let id = doc.add_mesh_object(
        "transfer_chamber",
        kreate_mesh::primitives::cylinder(1.8, 1.2, 8),
    );
    super::paint(doc, id, p.grey);
    {
        let o = doc.object_mut(id).unwrap();
        o.transform.translation = kreate_math::DVec3::new(0.0, 0.0, 1.0);
    }
    if m.is_some() {
        doc.set_parent(id, m).unwrap();
    }
    post(doc, "transfer_lid", 1.7, 0.08, [0.0, 0.0, 1.64], p.trim, m);
    // The process chambers round it, behind and to the sides (the front is
    // the load lock's).
    for k in 0..chambers {
        let angle = (-135.0 + 270.0 * (k as f64 + 0.5) / chambers as f64).to_radians();
        let at = [3.0 * angle.cos(), 3.0 * angle.sin(), 1.2];
        let chamber =
            doc.add_mesh_object(format!("chamber_{k}"), kreate_mesh::primitives::cube(1.0));
        super::paint(doc, chamber, p.white);
        {
            let o = doc.object_mut(chamber).unwrap();
            o.transform.translation = kreate_math::DVec3::from_array(at);
            o.transform.scale = kreate_math::DVec3::new(2.2, 2.0, 1.6);
            o.transform.rotation = kreate_math::DQuat::from_rotation_z(angle);
        }
        if m.is_some() {
            doc.set_parent(chamber, m).unwrap();
        }
        post(
            doc,
            &format!("chamber_lid_{k}"),
            0.7,
            0.25,
            [at[0], at[1], 2.12],
            p.trim,
            m,
        );
        post(
            doc,
            &format!("pump_{k}"),
            0.25,
            0.4,
            [at[0], at[1], 0.2],
            p.dark,
            m,
        );
    }
    block(doc, "gas_box", [1.2, 3.0, 2.2], [4.9, 0.0, 1.1], p.grey, m);
    for k in 0..4 {
        bar(
            doc,
            &format!("gas_line_{k}"),
            0.03,
            2.0,
            [4.2, -0.9 + 0.6 * k as f64, 2.0],
            true,
            p.steel,
            m,
        );
    }
    tower(doc, p, -4.0, 1.7, 2.4, m);
}

/// A coater/developer track `l` long: white cabinets with windows on the
/// spin cups and the bake plates inside, a glass conveyor through them.
fn track_tool(doc: &mut Document, p: &Paint, l: f64, m: Option<ObjectId>) {
    let (w, h) = (3.4, 2.6);
    block(doc, "cabinet", [l, w, h], [0.0, 0.0, h / 2.0], p.white, m);
    block(
        doc,
        "plinth",
        [l + 0.02, w + 0.02, 0.15],
        [0.0, 0.0, 0.075],
        p.trim,
        m,
    );
    let bays = ((l / 2.0).floor() as usize).max(1);
    for k in 0..bays {
        let x = -l / 2.0 + l * (k as f64 + 0.5) / bays as f64;
        for side in [-1.0, 1.0] {
            block(
                doc,
                &format!("window_{k}_{side}"),
                [l / bays as f64 - 0.4, 0.02, 0.9],
                [x, side * (w / 2.0 + 0.005), 1.5],
                p.glass,
                m,
            );
        }
        post(
            doc,
            &format!("exhaust_{k}"),
            0.15,
            0.6,
            [x, 0.8, h + 0.3],
            p.steel,
            m,
        );
    }
    block(doc, "duct", [l, 0.4, 0.3], [0.0, 0.8, h + 0.75], p.steel, m);
    tower(doc, p, -l / 2.0 + 0.3, -w / 2.0 + 0.2, h, m);
}

/// An exposure tool: its environment chamber on a granite base, the lens
/// column rising from its roof, the mask library beside it.
fn exposure_tool(doc: &mut Document, p: &Paint, m: Option<ObjectId>) {
    block(
        doc,
        "granite_base",
        [6.4, 4.4, 0.5],
        [0.0, 0.0, 0.25],
        p.granite,
        m,
    );
    block(doc, "chamber", [6.0, 4.0, 2.8], [0.0, 0.0, 1.9], p.white, m);
    block(doc, "trim", [6.02, 4.02, 0.12], [0.0, 0.0, 3.0], p.trim, m);
    post(doc, "lens_column", 0.8, 1.4, [0.4, 0.0, 4.0], p.grey, m);
    post(doc, "lamp_house", 0.5, 0.6, [0.4, 0.0, 5.0], p.dark, m);
    block(
        doc,
        "mask_library",
        [1.2, 1.6, 2.2],
        [-3.6, 1.2, 1.1],
        p.grey,
        m,
    );
    block(
        doc,
        "window",
        [3.0, 0.02, 0.8],
        [0.0, -2.01, 2.0],
        p.glass,
        m,
    );
    block(doc, "chiller", [1.0, 1.0, 1.4], [3.6, 1.4, 0.7], p.grey, m);
    tower(doc, p, -2.8, -1.8, 3.3, m);
}

/// A wet bench `l` long: its frame and tanks of chemicals and rinse water,
/// the rollers carrying the glass through, the exhaust hood above, the
/// chemical cabinet at its end.
fn wet_station(doc: &mut Document, p: &Paint, l: f64, m: Option<ObjectId>) {
    let w = 3.2;
    block(doc, "frame", [l, w, 0.9], [0.0, 0.0, 0.45], p.white, m);
    let tanks = ((l / 2.2).floor() as usize).max(1);
    for k in 0..tanks {
        let x = -l / 2.0 + l * (k as f64 + 0.5) / tanks as f64;
        block(
            doc,
            &format!("tank_{k}"),
            [l / tanks as f64 - 0.25, w - 0.4, 0.06],
            [x, 0.0, 0.93],
            p.liquid,
            m,
        );
    }
    let rollers = ((l / 0.5).floor() as usize).min(60);
    for k in 0..rollers {
        let x = -l / 2.0 + 0.25 + 0.5 * k as f64;
        bar(
            doc,
            &format!("roller_{k}"),
            0.03,
            w - 0.3,
            [x, 0.0, 1.0],
            false,
            p.steel,
            m,
        );
    }
    for side in [-1.0, 1.0] {
        block(
            doc,
            &format!("splash_{side}"),
            [l, 0.04, 0.6],
            [0.0, side * w / 2.0, 1.25],
            p.glass,
            m,
        );
        for k in 0..=((l / 3.0).ceil() as usize) {
            let x = -l / 2.0 + l * k as f64 / ((l / 3.0).ceil()).max(1.0);
            block(
                doc,
                &format!("hood_post_{side}_{k}"),
                [0.08, 0.08, 1.4],
                [x, side * (w / 2.0 - 0.05), 1.6],
                p.steel,
                m,
            );
        }
    }
    block(doc, "hood", [l, w, 0.5], [0.0, 0.0, 2.55], p.white, m);
    post(doc, "exhaust", 0.3, 1.2, [0.0, 0.6, 3.4], p.steel, m);
    block(
        doc,
        "chemical_cabinet",
        [1.0, w, 2.0],
        [l / 2.0 + 0.6, 0.0, 1.0],
        p.grey,
        m,
    );
    tower(doc, p, -l / 2.0 + 0.2, -w / 2.0 + 0.2, 2.8, m);
}

/// A cassette stocker `l` long and `h` high: its frame, a window down one
/// side on the shelves of glass cassettes, the crane's rail along it, and
/// its ports at the ends.
fn stocker(doc: &mut Document, p: &Paint, l: f64, h: f64, m: Option<ObjectId>) {
    let w = 3.6;
    for side in [-1.0, 1.0] {
        block(
            doc,
            &format!("wall_{side}"),
            [l, 0.1, h],
            [0.0, side * w / 2.0, h / 2.0],
            if side < 0.0 { p.glass } else { p.white },
            m,
        );
    }
    block(doc, "roof", [l, w, 0.12], [0.0, 0.0, h], p.white, m);
    for k in 0..=((l / 2.5).ceil() as usize) {
        let x = -l / 2.0 + l * k as f64 / (l / 2.5).ceil().max(1.0);
        block(
            doc,
            &format!("frame_{k}"),
            [0.12, w, 0.12],
            [x, 0.0, h - 0.06],
            p.trim,
            m,
        );
        block(
            doc,
            &format!("column_{k}"),
            [0.12, 0.14, h],
            [x, -w / 2.0, h / 2.0],
            p.trim,
            m,
        );
    }
    // Shelves of cassettes along the far side, a lane for the crane between.
    let levels = ((h / 0.8).floor() as usize).max(1);
    let bays = ((l / 1.2).floor() as usize).max(1);
    for lv in 0..levels {
        let z = 0.3 + 0.8 * lv as f64;
        block(
            doc,
            &format!("shelf_{lv}"),
            [l - 0.3, 1.1, 0.04],
            [0.0, 1.1, z],
            p.steel,
            m,
        );
        for b in 0..bays {
            if (lv * 7 + b * 3) % 5 == 0 {
                continue;
            }
            let x = -l / 2.0 + 0.6 + 1.2 * b as f64;
            block(
                doc,
                &format!("cassette_{lv}_{b}"),
                [0.9, 0.9, 0.5],
                [x, 1.1, z + 0.27],
                p.cassette,
                m,
            );
        }
    }
    block(
        doc,
        "crane_rail",
        [l, 0.12, 0.12],
        [0.0, -0.4, 0.06],
        p.yellow,
        m,
    );
    block(
        doc,
        "crane_mast",
        [0.3, 0.3, h - 0.3],
        [-l / 4.0, -0.4, (h - 0.3) / 2.0],
        p.yellow,
        m,
    );
    block(
        doc,
        "crane_fork",
        [0.9, 0.8, 0.1],
        [-l / 4.0, 0.1, 1.5],
        p.grey,
        m,
    );
    for side in [-1.0, 1.0] {
        block(
            doc,
            &format!("port_{side}"),
            [0.8, 1.2, 1.0],
            [side * (l / 2.0 + 0.4), -0.8, 0.5],
            p.grey,
            m,
        );
    }
}

/// An inspection gantry: a granite stage the glass lies on, the bridge
/// across it carrying the cameras, the operator's console beside it.
fn inspection_tool(doc: &mut Document, p: &Paint, m: Option<ObjectId>) {
    block(doc, "base", [4.0, 3.4, 0.6], [0.0, 0.0, 0.3], p.white, m);
    block(
        doc,
        "stage",
        [3.6, 3.0, 0.3],
        [0.0, 0.0, 0.75],
        p.granite,
        m,
    );
    for side in [-1.0, 1.0] {
        block(
            doc,
            &format!("upright_{side}"),
            [0.35, 0.3, 1.2],
            [0.4, side * 1.6, 1.5],
            p.trim,
            m,
        );
    }
    block(doc, "bridge", [0.4, 3.5, 0.35], [0.4, 0.0, 2.2], p.white, m);
    for k in 0..5 {
        let y = -1.2 + 0.6 * k as f64;
        block(
            doc,
            &format!("camera_{k}"),
            [0.14, 0.14, 0.3],
            [0.2, y, 1.9],
            p.dark,
            m,
        );
    }
    block(
        doc,
        "console",
        [0.8, 0.6, 1.1],
        [-2.4, -1.2, 0.55],
        p.grey,
        m,
    );
    block(
        doc,
        "screen",
        [0.05, 0.5, 0.35],
        [-2.3, -1.2, 1.35],
        p.trim,
        m,
    );
    tower(doc, p, -1.8, 1.5, 0.6, m);
}

/// The rail the overhead hoist vehicles run on: a loop `l` by `w`, `h` up,
/// hung from the ceiling, with vehicles on it carrying cassettes.
fn oht_rail(doc: &mut Document, p: &Paint, l: f64, w: f64, h: f64, m: Option<ObjectId>) {
    for side in [-1.0, 1.0] {
        block(
            doc,
            &format!("rail_x_{side}"),
            [l, 0.12, 0.1],
            [0.0, side * w / 2.0, h],
            p.steel,
            m,
        );
        block(
            doc,
            &format!("rail_y_{side}"),
            [0.12, w, 0.1],
            [side * l / 2.0, 0.0, h],
            p.steel,
            m,
        );
        let hangers = ((l / 3.0).ceil() as usize).max(1);
        for k in 0..=hangers {
            let x = -l / 2.0 + l * k as f64 / hangers as f64;
            post(
                doc,
                &format!("hanger_{side}_{k}"),
                0.02,
                1.5,
                [x, side * w / 2.0, h + 0.8],
                p.dark,
                m,
            );
        }
    }
    let vehicles = ((l / 8.0).ceil() as usize).max(2);
    for k in 0..vehicles {
        let x = -l / 2.0 + l * (k as f64 + 0.5) / vehicles as f64;
        let y = if k % 2 == 0 { -w / 2.0 } else { w / 2.0 };
        block(
            doc,
            &format!("vehicle_{k}"),
            [0.8, 0.5, 0.45],
            [x, y, h - 0.3],
            p.yellow,
            m,
        );
        block(
            doc,
            &format!("load_{k}"),
            [0.6, 0.6, 0.4],
            [x, y, h - 0.8],
            p.cassette,
            m,
        );
    }
}

/// The floor rail a stocker's crane runs on, `l` long, past the tools'
/// load ports: two rails 1.6 m apart on sleepers, buffers at the ends, the
/// lane it sweeps marked on the floor either side.
fn crane_rail(doc: &mut Document, p: &Paint, l: f64, m: Option<ObjectId>) {
    for side in [-1.0, 1.0] {
        block(
            doc,
            &format!("rail_{side}"),
            [l, 0.08, 0.06],
            [0.0, side * 0.8, 0.03],
            p.steel,
            m,
        );
        block(
            doc,
            &format!("lane_{side}"),
            [l, 0.08, 0.004],
            [0.0, side * 1.3, 0.002],
            p.yellow,
            m,
        );
        block(
            doc,
            &format!("stop_{side}"),
            [0.3, 2.2, 0.4],
            [side * (l / 2.0 - 0.15), 0.0, 0.2],
            p.yellow,
            m,
        );
    }
    let sleepers = ((l / 1.5).floor() as usize).max(1);
    for k in 0..=sleepers {
        let x = -l / 2.0 + l * k as f64 / sleepers as f64;
        block(
            doc,
            &format!("sleeper_{k}"),
            [0.15, 2.0, 0.02],
            [x, 0.0, 0.01],
            p.dark,
            m,
        );
    }
}

/// A display at its stage, as a flow part looks: the mother glass (0) with
/// its six panels laid out on it, a panel cut from it (1), a module (2) —
/// the panel on its backlight, its driver films and board along one edge —
/// or a cassette of sheets (3), as a lot goes between stocker and tools;
/// or (4) the stocker's crane that carries a cassette along its rail —
/// alone, from the floor at the rail's middle, its way along x: a carriage
/// on the rails, the table the cassette rides on at a load port's height
/// and two masts joined at the top.
pub(crate) fn display_look(stage: usize) -> LookMeshes {
    let mut doc = Document::new();
    let glass = material(&mut doc, "glass", [0.62, 0.78, 0.84], 0.3);
    let cell = material(&mut doc, "active area", [0.07, 0.08, 0.10], 0.2);
    let frame = material(&mut doc, "backlight", [0.72, 0.74, 0.77], 0.7);
    let film = material(&mut doc, "driver film", [0.90, 0.55, 0.15], 0.0);
    let board = material(&mut doc, "board", [0.10, 0.45, 0.25], 0.0);
    match stage {
        4 => {
            let body = material(&mut doc, "crane", [0.95, 0.55, 0.10], 0.1);
            let dark = material(&mut doc, "crane carriage", [0.20, 0.22, 0.25], 0.3);
            block(
                &mut doc,
                "carriage",
                [3.4, 2.0, 0.35],
                [0.0, 0.0, 0.25],
                dark,
                None,
            );
            block(
                &mut doc,
                "table",
                [2.4, 2.0, 0.08],
                [0.0, 0.0, 0.66],
                body,
                None,
            );
            block(
                &mut doc,
                "lift",
                [1.0, 1.0, 0.24],
                [0.0, 0.0, 0.5],
                dark,
                None,
            );
            for side in [-1.0, 1.0] {
                block(
                    &mut doc,
                    &format!("mast_{side}"),
                    [0.35, 0.5, 4.0],
                    [side * 1.75, 0.0, 2.4],
                    body,
                    None,
                );
            }
            block(
                &mut doc,
                "beam",
                [3.85, 0.5, 0.3],
                [0.0, 0.0, 4.4],
                body,
                None,
            );
            block(
                &mut doc,
                "drive",
                [0.6, 0.9, 0.6],
                [-1.75, 0.0, 0.75],
                dark,
                None,
            );
        }
        0 => {
            block(
                &mut doc,
                "glass",
                [2.5, 2.2, 0.012],
                [0.0, 0.0, 0.006],
                glass,
                None,
            );
            for i in 0..3 {
                for j in 0..2 {
                    let (x, y) = (-0.82 + 0.82 * i as f64, -0.52 + 1.04 * j as f64);
                    block(
                        &mut doc,
                        &format!("cell_{i}_{j}"),
                        [0.74, 0.96, 0.004],
                        [x, y, 0.014],
                        cell,
                        None,
                    );
                }
            }
        }
        1 => {
            block(
                &mut doc,
                "glass",
                [0.78, 0.48, 0.012],
                [0.0, 0.0, 0.006],
                glass,
                None,
            );
            block(
                &mut doc,
                "active",
                [0.72, 0.42, 0.004],
                [0.0, 0.0, 0.014],
                cell,
                None,
            );
        }
        3 => {
            // A cassette: its frame — posts, base and top — and the sheets
            // racked in it, one over another.
            let steel = material(&mut doc, "cassette frame", [0.30, 0.32, 0.36], 0.4);
            block(
                &mut doc,
                "base",
                [2.7, 2.4, 0.06],
                [0.0, 0.0, 0.03],
                steel,
                None,
            );
            block(
                &mut doc,
                "top",
                [2.7, 2.4, 0.04],
                [0.0, 0.0, 1.0],
                steel,
                None,
            );
            let posts = [
                (-1.3, -1.15),
                (1.3, -1.15),
                (-1.3, 1.15),
                (1.3, 1.15),
                (0.0, 1.15),
                (0.0, -1.15),
            ];
            for (k, (x, y)) in posts.into_iter().enumerate() {
                block(
                    &mut doc,
                    &format!("post_{k}"),
                    [0.06, 0.06, 1.0],
                    [x, y, 0.5],
                    steel,
                    None,
                );
            }
            for k in 0..10 {
                let z = 0.12 + 0.085 * k as f64;
                block(
                    &mut doc,
                    &format!("sheet_{k}"),
                    [2.5, 2.2, 0.01],
                    [0.0, 0.0, z],
                    glass,
                    None,
                );
            }
        }
        _ => {
            block(
                &mut doc,
                "backlight",
                [0.80, 0.50, 0.035],
                [0.0, 0.0, 0.0175],
                frame,
                None,
            );
            block(
                &mut doc,
                "glass",
                [0.78, 0.48, 0.012],
                [0.0, 0.0, 0.041],
                glass,
                None,
            );
            block(
                &mut doc,
                "active",
                [0.72, 0.42, 0.004],
                [0.0, 0.0, 0.049],
                cell,
                None,
            );
            for k in 0..4 {
                let x = -0.27 + 0.18 * k as f64;
                block(
                    &mut doc,
                    &format!("film_{k}"),
                    [0.08, 0.06, 0.004],
                    [x, -0.27, 0.036],
                    film,
                    None,
                );
            }
            block(
                &mut doc,
                "board",
                [0.72, 0.06, 0.012],
                [0.0, -0.33, 0.03],
                board,
                None,
            );
        }
    }
    super::look_of(&doc)
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::*;
    use crate::library::Library;

    #[test]
    fn every_piece_of_the_fab_is_made_and_stands_on_the_floor() {
        for key in KEYS {
            let mut library = Library::default();
            let req = json!({"op": "place", "template": key, "at": [1.0, 2.0, 0.0]});
            let (answer, _) = library.handle(&req.to_string());
            let answer: Value = serde_json::from_str(&answer).unwrap();
            assert_eq!(answer["ok"], true, "{key}: {answer}");
            let mut doc = Document::new();
            library.apply(&mut doc);
            let mut lowest = f64::INFINITY;
            let mut shapes = 0;
            for (id, o) in doc.objects() {
                let Some(mesh) = o.mesh().and_then(|m| doc.mesh(m)) else {
                    continue;
                };
                shapes += 1;
                let world = doc.world_transform(id);
                for p in mesh.positions() {
                    lowest = lowest.min(world.transform_point3(p.as_dvec3()).z);
                }
            }
            assert!(shapes > 8, "{key}: drawn in some detail");
            if *key == "oht_rail" {
                assert!(lowest > 2.0, "{key}: hung overhead, lowest at {lowest}");
            } else {
                assert!(lowest > -1e-6 && lowest < 0.05, "{key}: lowest at {lowest}");
            }
        }
    }

    #[test]
    fn the_glass_the_panel_and_the_module_each_have_their_look() {
        for stage in 0..5 {
            let look = display_look(stage);
            assert!(look.len() >= 2, "stage {stage}: more than one colour");
            assert!(look.iter().all(|(_, m)| m.num_faces() > 0));
        }
    }
}
