//! The factory around the cell, and the equipment a shop floor has besides
//! robots and machines: walls, columns, floor markings, an overhead crane,
//! lights and pipes; forklifts, racks, pallets, presses, compressors,
//! cabinets. None has a role in the cell — they are there to be seen, and
//! to be run into — so each is drawn as it is met on a real floor: painted
//! steel, wood, cardboard, the safety colours where they belong.
//!
//! Each stands on the floor at its middle, its front (if it has one)
//! facing −Y, as the rest of the library does. Floor markings lie just
//! under the floor, so that what stands on them does not touch them.

use kreate_math::{DQuat, DVec3, SceneRgb};
use kreate_scene::{Document, Material, MaterialId, Object, ObjectData, ObjectId};

use super::{bar, block, post, Item};

/// The templates this module makes.
pub const KEYS: &[&str] = &[
    "factory_wall",
    "factory_column",
    "floor_zone",
    "overhead_crane",
    "ceiling_light",
    "roll_door",
    "guard_rail",
    "pipe_rack",
    "forklift",
    "pallet_rack",
    "pallet_load",
    "drum_pallet",
    "workbench",
    "tool_cabinet",
    "press",
    "air_compressor",
    "control_panel",
    "dust_collector",
    "welding_machine",
];

/// What a floor zone is, by number: its floor and its lines.
pub const ZONES: &[&str] = &["통로", "작업 구역", "위험 구역", "보관 구역"];

/// Whether a wall has its band of windows.
pub const WINDOWS: &[&str] = &["없음", "창 띠"];

/// Whether a rack's shelves hold pallets.
pub const LOADS: &[&str] = &["비움", "적재"];

/// The colours of a shop floor.
struct Paint {
    concrete: MaterialId,
    panel: MaterialId,
    seam: MaterialId,
    glass: MaterialId,
    steel: MaterialId,
    structure: MaterialId,
    dark: MaterialId,
    black: MaterialId,
    yellow: MaterialId,
    orange: MaterialId,
    red: MaterialId,
    green: MaterialId,
    blue: MaterialId,
    white: MaterialId,
    light: MaterialId,
    wood: MaterialId,
    cardboard: MaterialId,
    wrap: MaterialId,
    rack: MaterialId,
    cabinet: MaterialId,
    machine: MaterialId,
    screen: MaterialId,
    epoxy: MaterialId,
    floor: MaterialId,
    danger: MaterialId,
    store: MaterialId,
}

impl Paint {
    fn new(doc: &mut Document) -> Self {
        let mut colour = |name: &str, rgb: [f32; 3], metallic: f32| {
            doc.add_material(Material {
                name: name.into(),
                base_color: SceneRgb::new(rgb[0], rgb[1], rgb[2]),
                metallic,
                ..Material::default()
            })
        };
        Self {
            concrete: colour("concrete", [0.55, 0.55, 0.53], 0.0),
            panel: colour("sandwich panel", [0.80, 0.82, 0.83], 0.1),
            seam: colour("panel seam", [0.66, 0.68, 0.70], 0.1),
            glass: colour("glass", [0.36, 0.50, 0.60], 0.3),
            steel: colour("steel", [0.70, 0.72, 0.75], 0.8),
            structure: colour("structural steel", [0.30, 0.38, 0.47], 0.3),
            dark: colour("dark grey", [0.18, 0.19, 0.21], 0.2),
            black: colour("black", [0.05, 0.05, 0.06], 0.0),
            yellow: colour("safety yellow", [0.96, 0.76, 0.06], 0.0),
            orange: colour("safety orange", [0.93, 0.42, 0.08], 0.0),
            red: colour("signal red", [0.75, 0.10, 0.10], 0.0),
            green: colour("signal green", [0.10, 0.60, 0.25], 0.0),
            blue: colour("blue", [0.12, 0.30, 0.65], 0.0),
            white: colour("white", [0.93, 0.93, 0.92], 0.0),
            light: colour("lamp", [1.0, 0.98, 0.90], 0.0),
            wood: colour("pallet wood", [0.66, 0.50, 0.32], 0.0),
            cardboard: colour("cardboard", [0.72, 0.56, 0.36], 0.0),
            wrap: colour("stretch wrap", [0.85, 0.87, 0.88], 0.0),
            rack: colour("rack upright", [0.10, 0.28, 0.58], 0.2),
            cabinet: colour("cabinet grey", [0.78, 0.79, 0.80], 0.1),
            machine: colour("machine blue", [0.18, 0.33, 0.52], 0.2),
            screen: colour("screen", [0.08, 0.22, 0.36], 0.4),
            epoxy: colour("epoxy floor", [0.20, 0.46, 0.33], 0.0),
            floor: colour("work floor", [0.46, 0.48, 0.49], 0.0),
            danger: colour("danger floor", [0.40, 0.36, 0.30], 0.0),
            store: colour("store floor", [0.30, 0.38, 0.48], 0.0),
        }
    }
}

/// A shape of any mesh, `centre`d, turned by `turn`.
fn shape(
    doc: &mut Document,
    name: &str,
    mesh: kreate_mesh::MeshData,
    centre: [f64; 3],
    turn: DQuat,
    colour: MaterialId,
    parent: Option<ObjectId>,
) -> ObjectId {
    let id = doc.add_mesh_object(name, mesh);
    super::paint(doc, id, colour);
    let o = doc.object_mut(id).unwrap();
    o.transform.translation = DVec3::from_array(centre);
    o.transform.rotation = turn;
    if parent.is_some() {
        doc.set_parent(id, parent).unwrap();
    }
    id
}

/// A box turned about Z by `angle`.
#[allow(clippy::too_many_arguments)]
fn turned_block(
    doc: &mut Document,
    name: &str,
    size: [f64; 3],
    centre: [f64; 3],
    angle: f64,
    colour: MaterialId,
    parent: Option<ObjectId>,
) -> ObjectId {
    let id = block(doc, name, size, centre, colour, parent);
    doc.object_mut(id).unwrap().transform.rotation = DQuat::from_rotation_z(angle);
    id
}

/// Yellow and black bands, `n` of them `step` high from `z`, round a box
/// `size` across centred at `(x, y)`: the guard painted on what a forklift
/// may hit.
#[allow(clippy::too_many_arguments)]
fn hazard_bands(
    doc: &mut Document,
    p: &Paint,
    size: [f64; 2],
    x: f64,
    y: f64,
    z: f64,
    step: f64,
    n: usize,
    parent: Option<ObjectId>,
) {
    for k in 0..n {
        block(
            doc,
            &format!("guard_{k}"),
            [size[0], size[1], step],
            [x, y, z + step * (k as f64 + 0.5)],
            if k % 2 == 0 { p.yellow } else { p.black },
            parent,
        );
    }
}

/// A wooden pallet, 1.2 × 1.0 m (EUR), its top at `z + 0.144`: three
/// stringers and the deck boards over them.
fn pallet(doc: &mut Document, p: &Paint, x: f64, y: f64, z: f64, parent: Option<ObjectId>) -> f64 {
    for (k, sy) in [-0.42, 0.0, 0.42].into_iter().enumerate() {
        block(
            doc,
            &format!("stringer_{k}"),
            [1.2, 0.14, 0.1],
            [x, y + sy, z + 0.05],
            p.wood,
            parent,
        );
    }
    for k in 0..5 {
        let sx = -0.5 + 0.25 * k as f64;
        block(
            doc,
            &format!("board_{k}"),
            [0.18, 1.0, 0.022],
            [x + sx, y, z + 0.111],
            p.wood,
            parent,
        );
    }
    block(
        doc,
        "deck",
        [1.2, 1.0, 0.022],
        [x, y, z + 0.133],
        p.wood,
        parent,
    );
    z + 0.144
}

/// Boxes on a pallet whose top is at `z`: `layers` of four, `box_h` high,
/// wrapped.
#[allow(clippy::too_many_arguments)]
fn load(
    doc: &mut Document,
    p: &Paint,
    x: f64,
    y: f64,
    z: f64,
    layers: usize,
    box_h: f64,
    parent: Option<ObjectId>,
) {
    for l in 0..layers {
        for (k, (bx, by)) in [(-0.29, -0.24), (0.29, -0.24), (-0.29, 0.24), (0.29, 0.24)]
            .into_iter()
            .enumerate()
        {
            block(
                doc,
                &format!("box_{l}_{k}"),
                [0.56, 0.46, box_h - 0.01],
                [x + bx, y + by, z + box_h * (l as f64 + 0.5)],
                p.cardboard,
                parent,
            );
        }
    }
    // The wrap round the lower layers, a band a little proud of the boxes.
    let wrapped = (box_h * layers as f64 * 0.6).max(0.1);
    block(
        doc,
        "wrap",
        [1.16, 0.96, wrapped],
        [x, y, z + wrapped / 2.0 + 0.02],
        p.wrap,
        parent,
    );
}

/// Builds `item` into `doc`, if it is one of [`KEYS`].
pub(super) fn build(doc: &mut Document, item: &Item) {
    let p = Paint::new(doc);
    let root = doc.add_object(Object::new(item.template.clone(), ObjectData::Empty));
    let m = Some(root);
    match item.template.as_str() {
        "factory_wall" => wall(doc, &p, item, m),
        "factory_column" => column(doc, &p, item, m),
        "floor_zone" => zone(doc, &p, item, m),
        "overhead_crane" => crane(doc, &p, item, m),
        "ceiling_light" => lights(doc, &p, item, m),
        "roll_door" => roll_door(doc, &p, item, m),
        "guard_rail" => guard_rail(doc, &p, item, m),
        "pipe_rack" => pipe_rack(doc, &p, item, m),
        "forklift" => forklift(doc, &p, item, m),
        "pallet_rack" => pallet_rack(doc, &p, item, m),
        "pallet_load" => {
            let top = pallet(doc, &p, 0.0, 0.0, 0.0, m);
            let layers = item.param("layers").round().clamp(1.0, 6.0) as usize;
            load(doc, &p, 0.0, 0.0, top, layers, 0.3, m);
        }
        "drum_pallet" => drums(doc, &p, m),
        "workbench" => workbench(doc, &p, item, m),
        "tool_cabinet" => tool_cabinet(doc, &p, m),
        "press" => press(doc, &p, item, m),
        "air_compressor" => compressor(doc, &p, m),
        "control_panel" => control_panel(doc, &p, item, m),
        "dust_collector" => dust_collector(doc, &p, m),
        "welding_machine" => welder(doc, &p, m),
        _ => {}
    }
}

// --- the building -------------------------------------------------------

/// A sandwich-panel wall along X on a concrete curb, its panels' seams
/// showing, with a band of windows if asked.
fn wall(doc: &mut Document, p: &Paint, item: &Item, m: Option<ObjectId>) {
    let (l, h) = (item.param("length"), item.param("height"));
    let curb = 0.4;
    block(
        doc,
        "curb",
        [l, 0.25, curb],
        [0.0, 0.0, curb / 2.0],
        p.concrete,
        m,
    );
    let tall = h - curb;
    block(
        doc,
        "panels",
        [l, 0.15, tall],
        [0.0, 0.0, curb + tall / 2.0],
        p.panel,
        m,
    );
    // A seam each metre, the panels' width.
    let seams = (l.floor() as usize).min(80);
    for k in 1..seams {
        let x = -l / 2.0 + k as f64 * (l / seams as f64);
        block(
            doc,
            &format!("seam_{k}"),
            [0.03, 0.16, tall],
            [x, 0.0, curb + tall / 2.0],
            p.seam,
            m,
        );
    }
    block(
        doc,
        "flashing",
        [l + 0.04, 0.2, 0.1],
        [0.0, 0.0, h - 0.05],
        p.steel,
        m,
    );
    if item.param("windows").round() as usize == 1 && h >= 3.0 {
        let (low, high) = (h * 0.55, (h * 0.55 + 1.2).min(h - 0.3));
        let n = ((l / 2.4).floor() as usize).max(1);
        let pitch = l / n as f64;
        for k in 0..n {
            let x = -l / 2.0 + pitch * (k as f64 + 0.5);
            block(
                doc,
                &format!("window_{k}"),
                [pitch - 0.4, 0.17, high - low],
                [x, 0.0, (low + high) / 2.0],
                p.glass,
                m,
            );
        }
        for (name, z) in [("sill", low - 0.03), ("head", high + 0.03)] {
            block(doc, name, [l, 0.2, 0.06], [0.0, 0.0, z], p.steel, m);
        }
    }
}

/// An H-section steel column on its base plate and footing, its foot
/// guarded in yellow and black.
fn column(doc: &mut Document, p: &Paint, item: &Item, m: Option<ObjectId>) {
    let (h, s) = (item.param("height"), item.param("size"));
    let t = (s * 0.06).clamp(0.01, 0.04);
    block(
        doc,
        "footing",
        [s + 0.4, s + 0.4, 0.15],
        [0.0, 0.0, 0.075],
        p.concrete,
        m,
    );
    block(
        doc,
        "base_plate",
        [s + 0.2, s + 0.2, 0.03],
        [0.0, 0.0, 0.165],
        p.steel,
        m,
    );
    let (z0, tall) = (0.18, h - 0.18);
    for (name, y) in [("flange_a", -(s - t) / 2.0), ("flange_b", (s - t) / 2.0)] {
        block(
            doc,
            name,
            [s, t, tall],
            [0.0, y, z0 + tall / 2.0],
            p.structure,
            m,
        );
    }
    block(
        doc,
        "web",
        [t, s - 2.0 * t, tall],
        [0.0, 0.0, z0 + tall / 2.0],
        p.structure,
        m,
    );
    block(
        doc,
        "cap_plate",
        [s + 0.05, s + 0.05, 0.03],
        [0.0, 0.0, h - 0.015],
        p.structure,
        m,
    );
    hazard_bands(doc, p, [s + 0.12, s + 0.12], 0.0, 0.0, z0, 0.15, 8, m);
}

/// A marked area of floor: its floor and its border, lying just under the
/// floor. An aisle is green with a dashed middle; a danger zone is
/// bordered yellow and black.
fn zone(doc: &mut Document, p: &Paint, item: &Item, m: Option<ObjectId>) {
    let (l, w, line) = (
        item.param("length"),
        item.param("width"),
        item.param("line"),
    );
    let kind = item.param("kind").round() as usize;
    let fill = match kind {
        0 => p.epoxy,
        1 => p.floor,
        2 => p.danger,
        _ => p.store,
    };
    let lines = if matches!(kind, 1 | 3) {
        p.white
    } else {
        p.yellow
    };
    // Under the floor: the fill lowest, the lines over it.
    block(doc, "floor", [l, w, 0.002], [0.0, 0.0, -0.0015], fill, m);
    let z = -0.0004;
    let edges = [
        ([l, line], [0.0, -(w - line) / 2.0], true),
        ([l, line], [0.0, (w - line) / 2.0], true),
        ([line, w - 2.0 * line], [-(l - line) / 2.0, 0.0], false),
        ([line, w - 2.0 * line], [(l - line) / 2.0, 0.0], false),
    ];
    for (k, (size, at, along_x)) in edges.into_iter().enumerate() {
        if kind == 2 {
            // Yellow and black, 0.3 m each.
            let run = if along_x { size[0] } else { size[1] };
            let n = ((run / 0.3).round() as usize).clamp(1, 400);
            let step = run / n as f64;
            for s in 0..n {
                let off = -run / 2.0 + step * (s as f64 + 0.5);
                let (cx, cy, sz) = if along_x {
                    (at[0] + off, at[1], [step, size[1], 0.002])
                } else {
                    (at[0], at[1] + off, [size[0], step, 0.002])
                };
                block(
                    doc,
                    &format!("edge_{k}_{s}"),
                    sz,
                    [cx, cy, z],
                    if s % 2 == 0 { p.yellow } else { p.black },
                    m,
                );
            }
        } else {
            block(
                doc,
                &format!("edge_{k}"),
                [size[0], size[1], 0.002],
                [at[0], at[1], z],
                lines,
                m,
            );
        }
    }
    if kind == 0 {
        // The aisle's middle, dashed: 0.6 m on, 0.6 off.
        let n = ((l / 1.2).floor() as usize).clamp(0, 200);
        for s in 0..n {
            let x = -l / 2.0 + 1.2 * (s as f64 + 0.5);
            block(
                doc,
                &format!("dash_{s}"),
                [0.6, line * 0.8, 0.002],
                [x, 0.0, z],
                p.white,
                m,
            );
        }
    }
}

/// A double-girder overhead crane on its runway: columns, runway beams,
/// the bridge across them, the trolley and its hook hanging.
fn crane(doc: &mut Document, p: &Paint, item: &Item, m: Option<ObjectId>) {
    let (span, l, h) = (
        item.param("span"),
        item.param("length"),
        item.param("height"),
    );
    let bays = ((l / 6.0).ceil() as usize).max(1);
    for side in [-1.0, 1.0] {
        let x = side * span / 2.0;
        for k in 0..=bays {
            let y = -l / 2.0 + l * k as f64 / bays as f64;
            block(
                doc,
                &format!("runway_column_{side}_{k}"),
                [0.3, 0.3, h],
                [x, y, h / 2.0],
                p.structure,
                m,
            );
        }
        block(
            doc,
            &format!("runway_{side}"),
            [0.3, l + 0.3, 0.5],
            [x, 0.0, h + 0.25],
            p.structure,
            m,
        );
        block(
            doc,
            &format!("rail_{side}"),
            [0.06, l + 0.3, 0.06],
            [x, 0.0, h + 0.53],
            p.steel,
            m,
        );
        block(
            doc,
            &format!("end_truck_{side}"),
            [0.5, 2.0, 0.45],
            [x, 0.0, h + 0.785],
            p.yellow,
            m,
        );
    }
    let deck = h + 1.01;
    for (k, y) in [-0.4, 0.4].into_iter().enumerate() {
        block(
            doc,
            &format!("girder_{k}"),
            [span + 0.3, 0.35, 0.8],
            [0.0, y, deck + 0.4],
            p.yellow,
            m,
        );
    }
    // Black and yellow at the girders' ends: the buffers.
    for side in [-1.0, 1.0] {
        block(
            doc,
            &format!("buffer_{side}"),
            [0.12, 1.15, 0.3],
            [side * (span / 2.0 + 0.21), 0.0, deck + 0.4],
            p.black,
            m,
        );
    }
    let tx = span * 0.2;
    block(
        doc,
        "trolley",
        [1.2, 1.4, 0.5],
        [tx, 0.0, deck + 1.05],
        p.yellow,
        m,
    );
    bar(
        doc,
        "hoist_drum",
        0.22,
        0.9,
        [tx, 0.0, deck + 0.95],
        false,
        p.dark,
        m,
    );
    let hook = (h - 2.5).max(1.5);
    for (k, y) in [-0.08, 0.08].into_iter().enumerate() {
        let top = deck + 0.7;
        post(
            doc,
            &format!("rope_{k}"),
            0.012,
            top - hook,
            [tx, y, (top + hook) / 2.0],
            p.dark,
            m,
        );
    }
    block(
        doc,
        "hook_block",
        [0.35, 0.22, 0.45],
        [tx, 0.0, hook - 0.2],
        p.yellow,
        m,
    );
    shape(
        doc,
        "hook",
        kreate_mesh::primitives::torus(0.12, 0.035, 24, 10),
        [tx, 0.0, hook - 0.55],
        DQuat::from_rotation_x(std::f64::consts::FRAC_PI_2),
        p.dark,
        m,
    );
}

/// High-bay lamps hanging in a row along X from a cable tray.
fn lights(doc: &mut Document, p: &Paint, item: &Item, m: Option<ObjectId>) {
    let n = item.param("count").round().clamp(1.0, 30.0) as usize;
    let (pitch, h) = (item.param("spacing"), item.param("height"));
    let run = pitch * (n as f64 - 1.0);
    block(
        doc,
        "cable_tray",
        [run + 0.6, 0.3, 0.08],
        [0.0, 0.0, h],
        p.steel,
        m,
    );
    for k in 0..n {
        let x = -run / 2.0 + pitch * k as f64;
        let lamp = h - 0.8;
        post(
            doc,
            &format!("hanger_{k}"),
            0.008,
            0.7,
            [x, 0.0, h - 0.4],
            p.dark,
            m,
        );
        post(
            doc,
            &format!("driver_{k}"),
            0.09,
            0.12,
            [x, 0.0, lamp + 0.06],
            p.dark,
            m,
        );
        shape(
            doc,
            &format!("reflector_{k}"),
            kreate_mesh::primitives::cone(0.28, 0.25, 32),
            [x, 0.0, lamp - 0.1],
            DQuat::IDENTITY,
            p.cabinet,
            m,
        );
        post(
            doc,
            &format!("lens_{k}"),
            0.27,
            0.02,
            [x, 0.0, lamp - 0.23],
            p.light,
            m,
        );
    }
}

/// A rolling shutter door: its frame, its coil's housing, the slatted
/// curtain, and the guards and bollards at its sides.
fn roll_door(doc: &mut Document, p: &Paint, item: &Item, m: Option<ObjectId>) {
    let (w, h) = (item.param("width"), item.param("height"));
    for side in [-1.0, 1.0] {
        let x = side * (w / 2.0 + 0.125);
        block(
            doc,
            &format!("jamb_{side}"),
            [0.25, 0.3, h],
            [x, 0.0, h / 2.0],
            p.structure,
            m,
        );
        hazard_bands(doc, p, [0.29, 0.34], x, 0.0, 0.0, 0.2, 6, m);
        post(
            doc,
            &format!("bollard_{side}"),
            0.08,
            1.1,
            [x, -0.7, 0.55],
            p.yellow,
            m,
        );
        post(
            doc,
            &format!("bollard_band_{side}"),
            0.082,
            0.08,
            [x, -0.7, 0.85],
            p.black,
            m,
        );
    }
    block(
        doc,
        "coil_housing",
        [w + 0.5, 0.55, 0.6],
        [0.0, 0.05, h + 0.3],
        p.panel,
        m,
    );
    block(doc, "curtain", [w, 0.05, h], [0.0, 0.0, h / 2.0], p.seam, m);
    let slats = ((h / 0.25).floor() as usize).min(60);
    for k in 1..slats {
        block(
            doc,
            &format!("slat_{k}"),
            [w, 0.07, 0.025],
            [0.0, 0.0, k as f64 * h / slats as f64],
            p.steel,
            m,
        );
    }
    block(
        doc,
        "bottom_bar",
        [w, 0.08, 0.08],
        [0.0, 0.0, 0.04],
        p.black,
        m,
    );
}

/// A guard rail along X: yellow posts, two rails, base plates.
fn guard_rail(doc: &mut Document, p: &Paint, item: &Item, m: Option<ObjectId>) {
    let l = item.param("length");
    let n = ((l / 1.5).ceil() as usize).max(1);
    for k in 0..=n {
        let x = -l / 2.0 + l * k as f64 / n as f64;
        block(
            doc,
            &format!("plate_{k}"),
            [0.22, 0.22, 0.012],
            [x, 0.0, 0.006],
            p.dark,
            m,
        );
        block(
            doc,
            &format!("post_{k}"),
            [0.1, 0.1, 1.0],
            [x, 0.0, 0.512],
            p.yellow,
            m,
        );
        block(
            doc,
            &format!("cap_{k}"),
            [0.11, 0.11, 0.03],
            [x, 0.0, 1.027],
            p.black,
            m,
        );
    }
    for (k, z) in [0.4, 0.9].into_iter().enumerate() {
        block(
            doc,
            &format!("rail_{k}"),
            [l, 0.06, 0.15],
            [0.0, -0.08, z],
            p.yellow,
            m,
        );
    }
}

/// Pipes overhead along X on T-shaped supports, in the colours that say
/// what they carry: water, gas, air, fire water.
fn pipe_rack(doc: &mut Document, p: &Paint, item: &Item, m: Option<ObjectId>) {
    let (l, h) = (item.param("length"), item.param("height"));
    let n = ((l / 4.0).ceil() as usize).max(1);
    for k in 0..=n {
        let x = -l / 2.0 + l * k as f64 / n as f64;
        block(
            doc,
            &format!("support_{k}"),
            [0.15, 0.15, h],
            [x, 0.0, h / 2.0],
            p.structure,
            m,
        );
        block(
            doc,
            &format!("arm_{k}"),
            [0.12, 1.4, 0.12],
            [x, 0.0, h - 0.06],
            p.structure,
            m,
        );
    }
    let pipes = [
        (-0.5, 0.10, p.green),
        (-0.2, 0.07, p.yellow),
        (0.12, 0.08, p.blue),
        (0.45, 0.12, p.red),
    ];
    for (k, (y, r, colour)) in pipes.into_iter().enumerate() {
        bar(
            doc,
            &format!("pipe_{k}"),
            r,
            l + 0.4,
            [0.0, y, h + r],
            true,
            colour,
            m,
        );
    }
}

// --- equipment ------------------------------------------------------------

/// A counterbalance forklift, its forks to +X: the chassis on its wheels,
/// the counterweight behind, the seat under the overhead guard, the mast
/// and the forks at the height asked.
fn forklift(doc: &mut Document, p: &Paint, item: &Item, m: Option<ObjectId>) {
    let lift = item.param("fork_height");
    block(
        doc,
        "chassis",
        [1.6, 1.0, 0.55],
        [-0.1, 0.0, 0.43],
        p.orange,
        m,
    );
    block(
        doc,
        "counterweight",
        [0.45, 1.06, 0.8],
        [-1.1, 0.0, 0.55],
        p.dark,
        m,
    );
    for (k, (x, r)) in [(0.45, 0.3), (-0.7, 0.24)].into_iter().enumerate() {
        for side in [-1.0, 1.0] {
            bar(
                doc,
                &format!("tyre_{k}_{side}"),
                r,
                0.22,
                [x, side * 0.45, r],
                false,
                p.black,
                m,
            );
            bar(
                doc,
                &format!("hub_{k}_{side}"),
                r * 0.5,
                0.23,
                [x, side * 0.45, r],
                false,
                p.steel,
                m,
            );
        }
    }
    block(
        doc,
        "seat",
        [0.45, 0.5, 0.1],
        [-0.45, 0.0, 0.76],
        p.black,
        m,
    );
    block(
        doc,
        "backrest",
        [0.08, 0.5, 0.45],
        [-0.68, 0.0, 1.0],
        p.black,
        m,
    );
    post(
        doc,
        "steering_column",
        0.03,
        0.45,
        [0.2, 0.0, 0.92],
        p.dark,
        m,
    );
    post(
        doc,
        "steering_wheel",
        0.16,
        0.03,
        [0.15, 0.0, 1.15],
        p.black,
        m,
    );
    // The overhead guard: four posts, a frame and its slats.
    for (x, y) in [(-0.7, -0.45), (-0.7, 0.45), (0.5, -0.45), (0.5, 0.45)] {
        block(
            doc,
            &format!("guard_post_{x}_{y}"),
            [0.07, 0.07, 1.45],
            [x, y, 1.42],
            p.dark,
            m,
        );
    }
    for k in 0..4 {
        let y = -0.45 + 0.3 * k as f64;
        block(
            doc,
            &format!("guard_slat_{k}"),
            [1.27, 0.06, 0.05],
            [-0.1, y, 2.15],
            p.dark,
            m,
        );
    }
    for (k, x) in [-0.7, 0.5].into_iter().enumerate() {
        block(
            doc,
            &format!("guard_bar_{k}"),
            [0.07, 0.97, 0.06],
            [x, 0.0, 2.15],
            p.dark,
            m,
        );
    }
    post(doc, "beacon", 0.05, 0.1, [-0.6, 0.35, 2.23], p.orange, m);
    // The mast, and the carriage with its forks.
    for side in [-1.0, 1.0] {
        block(
            doc,
            &format!("mast_{side}"),
            [0.1, 0.12, 2.3],
            [0.8, side * 0.32, 1.3],
            p.dark,
            m,
        );
    }
    for (k, z) in [0.4, 1.4, 2.4].into_iter().enumerate() {
        block(
            doc,
            &format!("mast_tie_{k}"),
            [0.08, 0.76, 0.08],
            [0.8, 0.0, z],
            p.dark,
            m,
        );
    }
    let z = lift.clamp(0.0, 1.9) + 0.06;
    block(
        doc,
        "carriage",
        [0.06, 0.95, 0.5],
        [0.9, 0.0, z + 0.28],
        p.dark,
        m,
    );
    for side in [-1.0, 1.0] {
        block(
            doc,
            &format!("fork_shank_{side}"),
            [0.05, 0.12, 0.5],
            [0.95, side * 0.28, z + 0.25],
            p.steel,
            m,
        );
        block(
            doc,
            &format!("fork_{side}"),
            [1.1, 0.12, 0.045],
            [1.5, side * 0.28, z],
            p.steel,
            m,
        );
    }
}

/// A selective pallet rack: blue uprights, orange beams, two pallets a
/// bay a level, loaded if asked.
fn pallet_rack(doc: &mut Document, p: &Paint, item: &Item, m: Option<ObjectId>) {
    let bays = item.param("bays").round().clamp(1.0, 12.0) as usize;
    let levels = item.param("levels").round().clamp(1.0, 8.0) as usize;
    let loaded = item.param("load").round() as usize == 1;
    let (bay, depth, step) = (2.8, 1.1, 1.6);
    let height = step * levels as f64 + 0.2;
    let x0 = -(bay * bays as f64) / 2.0;
    for k in 0..=bays {
        let x = x0 + bay * k as f64;
        for y in [-depth / 2.0, depth / 2.0] {
            block(
                doc,
                &format!("upright_{k}_{y}"),
                [0.09, 0.07, height],
                [x, y, height / 2.0],
                p.rack,
                m,
            );
            block(
                doc,
                &format!("foot_{k}_{y}"),
                [0.16, 0.14, 0.01],
                [x, y, 0.005],
                p.dark,
                m,
            );
        }
        // The frame's bracing: ties across, a diagonal between.
        let ties = ((height / 1.0).floor() as usize).max(2);
        for t in 0..ties {
            let z = 0.15 + (height - 0.3) * t as f64 / (ties - 1) as f64;
            block(
                doc,
                &format!("tie_{k}_{t}"),
                [0.04, depth, 0.04],
                [x, 0.0, z],
                p.rack,
                m,
            );
        }
        hazard_bands(
            doc,
            p,
            [0.13, depth + 0.11],
            x,
            0.0,
            0.0,
            0.1,
            if k == 0 || k == bays { 4 } else { 0 },
            m,
        );
    }
    for b in 0..bays {
        let cx = x0 + bay * (b as f64 + 0.5);
        for lev in 0..levels {
            let floor = if lev == 0 {
                0.0
            } else {
                let z = step * lev as f64;
                for y in [-depth / 2.0, depth / 2.0] {
                    block(
                        doc,
                        &format!("beam_{b}_{lev}_{y}"),
                        [bay - 0.09, 0.05, 0.12],
                        [cx, y, z - 0.06],
                        p.orange,
                        m,
                    );
                }
                block(
                    doc,
                    &format!("label_{b}_{lev}"),
                    [0.18, 0.01, 0.07],
                    [cx, -depth / 2.0 - 0.03, z - 0.06],
                    p.white,
                    m,
                );
                z
            };
            if loaded {
                for (k, px) in [-0.65, 0.65].into_iter().enumerate() {
                    let top = pallet(doc, p, cx + px, 0.0, floor, m);
                    // Not every pallet is as full: two to four layers.
                    let layers = 2 + (b + lev * 3 + k) % 3;
                    let box_h = ((step - 0.35) / layers as f64).min(0.35);
                    load(doc, p, cx + px, 0.0, top, layers, box_h, m);
                }
            }
        }
    }
}

/// Four steel drums on a pallet, two blue and two red, each with its
/// rims.
fn drums(doc: &mut Document, p: &Paint, m: Option<ObjectId>) {
    let top = pallet(doc, p, 0.0, 0.0, 0.0, m);
    for (k, (x, y)) in [(-0.3, -0.25), (0.3, -0.25), (-0.3, 0.25), (0.3, 0.25)]
        .into_iter()
        .enumerate()
    {
        let colour = if k % 3 == 0 { p.blue } else { p.red };
        post(
            doc,
            &format!("drum_{k}"),
            0.28,
            0.88,
            [x, y, top + 0.44],
            colour,
            m,
        );
        for z in [0.3, 0.6] {
            post(
                doc,
                &format!("rim_{k}_{z}"),
                0.29,
                0.025,
                [x, y, top + z],
                colour,
                m,
            );
        }
        post(
            doc,
            &format!("bung_{k}"),
            0.03,
            0.02,
            [x + 0.15, y, top + 0.89],
            p.steel,
            m,
        );
    }
}

/// A workbench: a steel frame, a wooden top, a drawer unit, a pegboard
/// with tools on it, a vise and a lamp.
fn workbench(doc: &mut Document, p: &Paint, item: &Item, m: Option<ObjectId>) {
    let l = item.param("length");
    let (d, top) = (0.75, 0.9);
    for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        block(
            doc,
            &format!("leg_{x}_{y}"),
            [0.05, 0.05, top - 0.05],
            [
                x * (l / 2.0 - 0.05),
                y * (d / 2.0 - 0.05),
                (top - 0.05) / 2.0,
            ],
            p.dark,
            m,
        );
    }
    block(doc, "top", [l, d, 0.05], [0.0, 0.0, top - 0.025], p.wood, m);
    block(
        doc,
        "shelf",
        [l - 0.1, d - 0.1, 0.02],
        [0.0, 0.0, 0.18],
        p.dark,
        m,
    );
    let dw = 0.5;
    let dx = l / 2.0 - dw / 2.0 - 0.06;
    block(
        doc,
        "drawers",
        [dw, d - 0.1, 0.6],
        [dx, 0.0, top - 0.35],
        p.blue,
        m,
    );
    for k in 0..4 {
        let z = top - 0.12 - 0.15 * k as f64;
        block(
            doc,
            &format!("drawer_pull_{k}"),
            [0.25, 0.02, 0.02],
            [dx, -(d - 0.1) / 2.0 - 0.01, z],
            p.steel,
            m,
        );
    }
    block(
        doc,
        "pegboard",
        [l, 0.02, 0.8],
        [0.0, d / 2.0 - 0.01, top + 0.45],
        p.cabinet,
        m,
    );
    for k in 0..((l / 0.2).floor() as usize).min(14) {
        let x = -l / 2.0 + 0.15 + 0.2 * k as f64;
        let tall = 0.12 + 0.06 * (k % 3) as f64;
        block(
            doc,
            &format!("tool_{k}"),
            [0.04, 0.03, tall],
            [x, d / 2.0 - 0.035, top + 0.6],
            if k % 4 == 0 { p.red } else { p.dark },
            m,
        );
    }
    let vx = -l / 2.0 + 0.2;
    block(
        doc,
        "vise_body",
        [0.2, 0.15, 0.12],
        [vx, -d / 2.0 + 0.1, top + 0.06],
        p.machine,
        m,
    );
    block(
        doc,
        "vise_jaw",
        [0.18, 0.04, 0.08],
        [vx, -d / 2.0 + 0.0, top + 0.08],
        p.steel,
        m,
    );
    post(
        doc,
        "lamp_arm",
        0.012,
        0.6,
        [l / 2.0 - 0.15, d / 2.0 - 0.1, top + 0.3],
        p.dark,
        m,
    );
    block(
        doc,
        "lamp_head",
        [0.3, 0.1, 0.05],
        [l / 2.0 - 0.3, d / 2.0 - 0.2, top + 0.6],
        p.dark,
        m,
    );
}

/// A red roller cabinet with its top chest: drawers, pulls, casters.
fn tool_cabinet(doc: &mut Document, p: &Paint, m: Option<ObjectId>) {
    let (w, d) = (0.7, 0.48);
    block(doc, "cabinet", [w, d, 0.85], [0.0, 0.0, 0.53], p.red, m);
    block(
        doc,
        "chest",
        [w, d - 0.05, 0.4],
        [0.0, 0.02, 1.16],
        p.red,
        m,
    );
    block(
        doc,
        "lid",
        [w + 0.01, d - 0.04, 0.03],
        [0.0, 0.02, 1.375],
        p.dark,
        m,
    );
    for (k, z) in [0.22, 0.36, 0.5, 0.64, 0.8, 1.02, 1.14, 1.26]
        .into_iter()
        .enumerate()
    {
        let y = if z > 1.0 {
            -d / 2.0 + 0.02
        } else {
            -d / 2.0 - 0.005
        };
        block(
            doc,
            &format!("seam_{k}"),
            [w - 0.04, 0.01, 0.008],
            [0.0, y, z - 0.06],
            p.dark,
            m,
        );
        block(
            doc,
            &format!("pull_{k}"),
            [w - 0.16, 0.02, 0.015],
            [0.0, y - 0.01, z - 0.03],
            p.steel,
            m,
        );
    }
    for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        bar(
            doc,
            &format!("caster_{x}_{y}"),
            0.05,
            0.04,
            [x * (w / 2.0 - 0.07), y * (d / 2.0 - 0.07), 0.05],
            true,
            p.black,
            m,
        );
    }
    bar(
        doc,
        "handle",
        0.015,
        w - 0.1,
        [0.0, d / 2.0 + 0.05, 0.85],
        true,
        p.steel,
        m,
    );
}

/// A straight-side press: bed, columns, crown, slide over the bolster,
/// the flywheel at the side, light curtains and a two-hand station.
fn press(doc: &mut Document, p: &Paint, item: &Item, m: Option<ObjectId>) {
    let (h, w) = (item.param("height"), item.param("width"));
    let d = 1.4;
    let bed = 0.9;
    let crown = 0.8;
    block(doc, "bed", [w, d, bed], [0.0, 0.0, bed / 2.0], p.machine, m);
    block(
        doc,
        "bolster",
        [w - 0.5, d - 0.3, 0.1],
        [0.0, 0.0, bed + 0.05],
        p.steel,
        m,
    );
    let col = 0.35;
    let tall = h - bed - crown;
    for side in [-1.0, 1.0] {
        block(
            doc,
            &format!("column_{side}"),
            [col, d - 0.1, tall],
            [side * (w / 2.0 - col / 2.0), 0.0, bed + tall / 2.0],
            p.machine,
            m,
        );
    }
    block(
        doc,
        "crown",
        [w + 0.1, d + 0.05, crown],
        [0.0, 0.0, h - crown / 2.0],
        p.machine,
        m,
    );
    let slide = bed + 0.1 + tall * 0.45;
    block(
        doc,
        "slide",
        [w - 2.0 * col - 0.05, d - 0.4, 0.45],
        [0.0, 0.0, slide + 0.225],
        p.machine,
        m,
    );
    block(
        doc,
        "die",
        [w - 2.0 * col - 0.25, d - 0.6, 0.2],
        [0.0, 0.0, bed + 0.2],
        p.dark,
        m,
    );
    // Yellow and black along the slide's front edge.
    let n = 8;
    let sw = (w - 2.0 * col - 0.05) / n as f64;
    for k in 0..n {
        block(
            doc,
            &format!("slide_stripe_{k}"),
            [sw, 0.01, 0.08],
            [
                -(w - 2.0 * col - 0.05) / 2.0 + sw * (k as f64 + 0.5),
                -(d - 0.4) / 2.0 - 0.005,
                slide + 0.04,
            ],
            if k % 2 == 0 { p.yellow } else { p.black },
            m,
        );
    }
    bar(
        doc,
        "flywheel",
        0.5,
        0.2,
        [w / 2.0 + 0.15, 0.1, h - crown / 2.0],
        true,
        p.dark,
        m,
    );
    block(
        doc,
        "motor",
        [0.4, 0.5, 0.4],
        [0.0, d / 2.0 + 0.25, h + 0.2 - crown],
        p.dark,
        m,
    );
    for side in [-1.0, 1.0] {
        block(
            doc,
            &format!("light_curtain_{side}"),
            [0.05, 0.05, 1.2],
            [side * (w / 2.0 - col - 0.05), -d / 2.0 - 0.05, bed + 0.6],
            p.yellow,
            m,
        );
    }
    block(
        doc,
        "two_hand_stand",
        [0.08, 0.08, 1.0],
        [0.0, -d / 2.0 - 0.7, 0.5],
        p.dark,
        m,
    );
    block(
        doc,
        "two_hand_box",
        [0.6, 0.2, 0.15],
        [0.0, -d / 2.0 - 0.7, 1.05],
        p.yellow,
        m,
    );
    for (k, x) in [-0.2, 0.2].into_iter().enumerate() {
        post(
            doc,
            &format!("palm_button_{k}"),
            0.045,
            0.04,
            [x, -d / 2.0 - 0.7, 1.14],
            p.green,
            m,
        );
    }
    post(
        doc,
        "e_stop",
        0.04,
        0.05,
        [0.0, -d / 2.0 - 0.7, 1.15],
        p.red,
        m,
    );
}

/// A piston compressor on its receiver tank: the tank, the pump and motor
/// on it, the belt guard, the gauge.
fn compressor(doc: &mut Document, p: &Paint, m: Option<ObjectId>) {
    for (k, x) in [-0.55, 0.55].into_iter().enumerate() {
        block(
            doc,
            &format!("foot_{k}"),
            [0.08, 0.6, 0.2],
            [x, 0.0, 0.1],
            p.dark,
            m,
        );
    }
    bar(doc, "tank", 0.35, 1.6, [0.0, 0.0, 0.55], true, p.red, m);
    for side in [-1.0, 1.0] {
        shape(
            doc,
            &format!("tank_end_{side}"),
            kreate_mesh::primitives::uv_sphere(0.35, 24, 12),
            [side * 0.8, 0.0, 0.55],
            DQuat::IDENTITY,
            p.red,
            m,
        );
    }
    block(
        doc,
        "base_plate",
        [1.0, 0.55, 0.03],
        [0.0, 0.0, 0.915],
        p.dark,
        m,
    );
    block(doc, "pump", [0.35, 0.3, 0.35], [-0.2, 0.0, 1.1], p.dark, m);
    for k in 0..4 {
        block(
            doc,
            &format!("fin_{k}"),
            [0.4, 0.34, 0.015],
            [-0.2, 0.0, 1.0 + 0.06 * k as f64],
            p.steel,
            m,
        );
    }
    bar(doc, "motor", 0.14, 0.35, [0.3, 0.05, 1.07], true, p.blue, m);
    block(
        doc,
        "belt_guard",
        [0.6, 0.08, 0.35],
        [0.05, -0.2, 1.1],
        p.orange,
        m,
    );
    bar(
        doc,
        "gauge",
        0.05,
        0.03,
        [0.6, -0.3, 0.9],
        false,
        p.white,
        m,
    );
}

/// An electrical cabinet: its doors, an operator screen, buttons, a
/// warning label and a stack light on top.
fn control_panel(doc: &mut Document, p: &Paint, item: &Item, m: Option<ObjectId>) {
    let (w, h) = (item.param("width"), item.param("height"));
    let d = 0.5;
    block(doc, "plinth", [w, d, 0.1], [0.0, 0.0, 0.05], p.dark, m);
    block(
        doc,
        "cabinet",
        [w, d, h - 0.1],
        [0.0, 0.0, 0.1 + (h - 0.1) / 2.0],
        p.cabinet,
        m,
    );
    let doors = if w > 0.9 { 2 } else { 1 };
    for k in 1..doors {
        block(
            doc,
            &format!("door_seam_{k}"),
            [0.006, 0.01, h - 0.2],
            [
                -w / 2.0 + w * k as f64 / doors as f64,
                -d / 2.0 - 0.003,
                h / 2.0 + 0.05,
            ],
            p.dark,
            m,
        );
    }
    for k in 0..doors {
        let x = -w / 2.0 + w * (k as f64 + 1.0) / doors as f64 - 0.06;
        block(
            doc,
            &format!("handle_{k}"),
            [0.025, 0.04, 0.18],
            [x, -d / 2.0 - 0.02, h * 0.55],
            p.dark,
            m,
        );
    }
    let sx = -w / 2.0 + w / (2.0 * doors as f64);
    block(
        doc,
        "screen_bezel",
        [0.36, 0.02, 0.28],
        [sx, -d / 2.0 - 0.01, h * 0.72],
        p.dark,
        m,
    );
    block(
        doc,
        "screen",
        [0.31, 0.022, 0.22],
        [sx, -d / 2.0 - 0.012, h * 0.72],
        p.screen,
        m,
    );
    for (k, colour) in [p.green, p.red, p.yellow, p.white].into_iter().enumerate() {
        bar(
            doc,
            &format!("button_{k}"),
            0.022,
            0.03,
            [sx - 0.12 + 0.08 * k as f64, -d / 2.0 - 0.015, h * 0.55],
            false,
            colour,
            m,
        );
    }
    block(
        doc,
        "warning_label",
        [0.12, 0.012, 0.1],
        [sx, -d / 2.0 - 0.006, h * 0.4],
        p.yellow,
        m,
    );
    // The stack light: a post with red, yellow and green on it.
    let lx = w / 2.0 - 0.1;
    post(doc, "stack_post", 0.015, 0.2, [lx, 0.0, h + 0.1], p.dark, m);
    for (k, colour) in [p.green, p.yellow, p.red].into_iter().enumerate() {
        post(
            doc,
            &format!("stack_{k}"),
            0.04,
            0.07,
            [lx, 0.0, h + 0.235 + 0.072 * k as f64],
            colour,
            m,
        );
    }
}

/// A cartridge dust collector: its housing on legs, the hopper and bin
/// below, the fan and motor on top, the duct coming in at the side.
fn dust_collector(doc: &mut Document, p: &Paint, m: Option<ObjectId>) {
    let r: f64 = 0.6;
    for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        block(
            doc,
            &format!("leg_{x}_{y}"),
            [0.08, 0.08, 1.4],
            [x * 0.45, y * 0.45, 0.7],
            p.structure,
            m,
        );
    }
    shape(
        doc,
        "hopper",
        kreate_mesh::primitives::cone(r as f32, 0.7, 32),
        [0.0, 0.0, 1.05],
        DQuat::from_rotation_x(std::f64::consts::PI),
        p.cabinet,
        m,
    );
    post(doc, "bin", 0.3, 0.45, [0.0, 0.0, 0.25], p.dark, m);
    post(doc, "housing", r, 2.2, [0.0, 0.0, 2.5], p.cabinet, m);
    for (k, z) in [1.6, 2.5, 3.4].into_iter().enumerate() {
        post(
            doc,
            &format!("band_{k}"),
            r + 0.015,
            0.05,
            [0.0, 0.0, z],
            p.steel,
            m,
        );
    }
    block(doc, "fan", [0.7, 0.5, 0.4], [0.0, 0.0, 3.8], p.machine, m);
    bar(
        doc,
        "fan_motor",
        0.16,
        0.4,
        [0.0, 0.0, 4.15],
        true,
        p.dark,
        m,
    );
    bar(
        doc,
        "outlet",
        0.12,
        0.5,
        [0.0, 0.45, 3.8],
        false,
        p.steel,
        m,
    );
    bar(
        doc,
        "inlet_duct",
        0.18,
        1.2,
        [-1.15, 0.0, 2.0],
        true,
        p.steel,
        m,
    );
    block(
        doc,
        "access_door",
        [0.4, 0.02, 0.6],
        [0.0, -r - 0.005, 2.4],
        p.seam,
        m,
    );
}

/// A welding power source on its trolley with a gas cylinder chained on,
/// and its cable coiled.
fn welder(doc: &mut Document, p: &Paint, m: Option<ObjectId>) {
    block(doc, "trolley", [0.8, 0.5, 0.05], [0.0, 0.0, 0.2], p.dark, m);
    for (k, (x, r)) in [(-0.3, 0.12), (0.3, 0.08)].into_iter().enumerate() {
        for side in [-1.0, 1.0] {
            bar(
                doc,
                &format!("wheel_{k}_{side}"),
                r,
                0.05,
                [x, side * 0.27, r],
                false,
                p.black,
                m,
            );
        }
    }
    block(
        doc,
        "power_source",
        [0.55, 0.32, 0.45],
        [0.1, -0.05, 0.45],
        p.blue,
        m,
    );
    block(
        doc,
        "front_panel",
        [0.02, 0.28, 0.3],
        [0.385, -0.05, 0.47],
        p.dark,
        m,
    );
    bar(
        doc,
        "knob",
        0.03,
        0.03,
        [0.4, -0.05, 0.52],
        true,
        p.white,
        m,
    );
    block(
        doc,
        "wire_feeder",
        [0.45, 0.25, 0.25],
        [0.1, -0.05, 0.8],
        p.blue,
        m,
    );
    post(doc, "cylinder", 0.11, 1.3, [-0.28, 0.12, 0.88], p.green, m);
    shape(
        doc,
        "cylinder_shoulder",
        kreate_mesh::primitives::uv_sphere(0.11, 16, 8),
        [-0.28, 0.12, 1.53],
        DQuat::IDENTITY,
        p.green,
        m,
    );
    post(doc, "valve", 0.03, 0.12, [-0.28, 0.12, 1.67], p.steel, m);
    block(
        doc,
        "chain_bar",
        [0.04, 0.35, 0.04],
        [-0.17, 0.12, 1.1],
        p.steel,
        m,
    );
    shape(
        doc,
        "cable_coil",
        kreate_mesh::primitives::torus(0.18, 0.025, 24, 8),
        [0.1, -0.05, 0.96],
        DQuat::IDENTITY,
        p.black,
        m,
    );
    turned_block(
        doc,
        "torch",
        [0.3, 0.04, 0.04],
        [0.25, 0.15, 0.95],
        0.6,
        p.dark,
        m,
    );
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use serde_json::{json, Value};

    use super::*;
    use crate::library::Library;

    fn place(library: &mut Library, template: &str, at: [f64; 3], params: Value) {
        let req = json!({"op": "place", "template": template, "at": at, "params": params});
        let (answer, _) = library.handle(&req.to_string());
        let answer: Value = serde_json::from_str(&answer).unwrap();
        assert_eq!(answer["ok"], true, "{template}: {answer}");
    }

    /// Each object's lowest point in the world, from its mesh.
    fn lowest(doc: &Document, id: ObjectId) -> Option<f64> {
        let mesh = doc.mesh(doc.object(id)?.mesh()?)?;
        let world = doc.world_transform(id);
        mesh.positions()
            .iter()
            .map(|p| world.transform_point3(p.as_dvec3()).z)
            .reduce(f64::min)
    }

    #[test]
    fn every_piece_of_the_factory_is_made_and_stands_on_the_floor() {
        for key in KEYS {
            assert!(crate::library::template(key).is_some(), "{key} listed");
            let mut library = Library::default();
            place(&mut library, key, [3.0, -2.0, 0.0], json!({}));
            let mut doc = Document::new();
            library.apply(&mut doc);
            let low: Vec<f64> = doc
                .objects()
                .filter_map(|(id, _)| lowest(&doc, id))
                .collect();
            assert!(low.len() > 3, "{key}: drawn in some detail");
            let floor = low.iter().copied().fold(f64::INFINITY, f64::min);
            // On the floor; a floor marking just under it; lamps hung
            // from the ceiling, under it.
            if *key == "ceiling_light" {
                assert!(floor > 1.0, "{key}: hung, lowest at {floor}");
            } else {
                assert!(floor > -0.005 && floor < 0.2, "{key}: lowest at {floor}");
            }
        }
        // Their choices and sizes at their ends make them too.
        let mut library = Library::default();
        for kind in 0..ZONES.len() {
            place(
                &mut library,
                "floor_zone",
                [0.0, 4.0 * kind as f64, 0.0],
                json!({"kind": kind, "length": 100.0, "width": 0.5}),
            );
        }
        place(
            &mut library,
            "pallet_rack",
            [0.0, -10.0, 0.0],
            json!({"bays": 12, "levels": 8, "load": 1}),
        );
        place(
            &mut library,
            "factory_wall",
            [0.0, 20.0, 0.0],
            json!({"length": 1.0, "height": 2.5}),
        );
        let mut doc = Document::new();
        library.apply(&mut doc);
    }

    #[test]
    fn a_robot_standing_on_a_floor_marking_does_not_touch_it() {
        let mut library = Library::default();
        place(
            &mut library,
            "floor_zone",
            [0.0, 0.0, 0.0],
            json!({"kind": 2}),
        );
        place(&mut library, "ur10e_tending", [0.0, 0.0, 0.0], json!({}));
        let mut doc = Document::new();
        library.apply(&mut doc);
        let contacts = crate::collide::Collider::default().contacts(&doc, &HashSet::new());
        assert!(contacts.is_empty(), "{contacts:?}");
    }
}
