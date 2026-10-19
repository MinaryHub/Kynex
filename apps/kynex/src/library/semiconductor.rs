//! A semiconductor fab's equipment, and what it makes: in the front end the
//! vertical furnaces that oxidise and diffuse, the ion implanters, the CMP
//! polishers; the wafer probers that test each die; in the back end the
//! dicing saws, the die and wire bonders, the mould presses and the test
//! handlers. The deposition, etch, photo and wet tools, the stockers and
//! the overhead hoist's rail are the display fab's (`display`), the same in
//! kind. And the FOUP a lot of wafers goes round in, a wafer, a tray of
//! packaged chips and the overhead hoist's vehicle, as flow parts look.
//!
//! The tools are white with blue trim, as a clean room's are. Each stands on
//! the floor at its middle, its load ports at its front (−X), where an
//! overhead hoist sets a FOUP down.

use kreate_math::SceneRgb;
use kreate_scene::{Document, Material, MaterialId, Object, ObjectData, ObjectId};

use super::{bar, block, post, Item, LookMeshes};

/// The templates this module makes.
pub const KEYS: &[&str] = &[
    "vertical_furnace",
    "ion_implanter",
    "cmp_tool",
    "wafer_prober",
    "dicing_saw",
    "bonder",
    "mold_press",
    "test_handler",
];

/// What a bonder does, by number: only its name and its head differ.
pub const BONDS: &[&str] = &["다이 본딩", "와이어 본딩"];

struct Paint {
    white: MaterialId,
    trim: MaterialId,
    grey: MaterialId,
    dark: MaterialId,
    steel: MaterialId,
    glass: MaterialId,
    copper: MaterialId,
    green: MaterialId,
    red: MaterialId,
    amber: MaterialId,
    pad: MaterialId,
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
            copper: material(doc, "magnet coil", [0.72, 0.45, 0.20], 0.6),
            green: material(doc, "lamp green", [0.15, 0.75, 0.30], 0.0),
            red: material(doc, "lamp red", [0.85, 0.12, 0.10], 0.0),
            amber: material(doc, "lamp amber", [0.98, 0.65, 0.10], 0.0),
            pad: material(doc, "polishing pad", [0.85, 0.80, 0.55], 0.0),
        }
    }
}

/// Builds `item` into `doc`, if it is one of [`KEYS`].
pub(super) fn build(doc: &mut Document, item: &Item) {
    let p = Paint::new(doc);
    let root = doc.add_object(Object::new(item.template.clone(), ObjectData::Empty));
    let m = Some(root);
    match item.template.as_str() {
        "vertical_furnace" => vertical_furnace(doc, &p, m),
        "ion_implanter" => ion_implanter(doc, &p, m),
        "cmp_tool" => cmp_tool(doc, &p, m),
        "wafer_prober" => wafer_prober(doc, &p, m),
        "dicing_saw" => dicing_saw(doc, &p, m),
        "bonder" => bonder(doc, &p, item.param("bond").round() as usize == 1, m),
        "mold_press" => mold_press(doc, &p, m),
        "test_handler" => test_handler(doc, &p, m),
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

/// The front end every front-end tool has at −X: its two load ports, a
/// FOUP's height up, before a cabinet `d` wide where a robot takes each
/// wafer in, `x` its front.
fn front_end(doc: &mut Document, p: &Paint, x: f64, d: f64, m: Option<ObjectId>) {
    block(doc, "efem", [1.0, d, 2.3], [x + 0.5, 0.0, 1.15], p.white, m);
    block(
        doc,
        "efem_window",
        [0.02, d * 0.7, 0.8],
        [x - 0.01, 0.0, 1.6],
        p.glass,
        m,
    );
    for (k, y) in [-d / 4.0, d / 4.0].into_iter().enumerate() {
        block(
            doc,
            &format!("load_port_{k}"),
            [0.5, 0.5, 0.9],
            [x - 0.25, y, 0.45],
            p.grey,
            m,
        );
    }
}

/// A vertical furnace (oxidation, diffusion, LPCVD): wafers stood a hundred
/// at a time in a boat and raised into a quartz tube in the tall cabinet;
/// its gas cabinet and exhaust at the back.
fn vertical_furnace(doc: &mut Document, p: &Paint, m: Option<ObjectId>) {
    front_end(doc, p, -2.6, 2.4, m);
    block(
        doc,
        "loading_area",
        [1.4, 2.4, 2.2],
        [-0.9, 0.0, 1.1],
        p.white,
        m,
    );
    block(doc, "furnace", [1.8, 2.4, 4.4], [0.7, 0.0, 2.2], p.white, m);
    block(
        doc,
        "furnace_band",
        [1.82, 2.42, 0.25],
        [0.7, 0.0, 3.6],
        p.trim,
        m,
    );
    // The heater's hot zone, seen through its window: the tube and boat.
    block(
        doc,
        "furnace_window",
        [0.02, 0.6, 1.4],
        [-0.21, 0.0, 3.0],
        p.glass,
        m,
    );
    post(doc, "tube", 0.3, 1.3, [-0.1, 0.0, 3.0], p.amber, m);
    block(
        doc,
        "gas_cabinet",
        [0.8, 1.6, 2.4],
        [2.0, 0.6, 1.2],
        p.grey,
        m,
    );
    post(doc, "exhaust", 0.15, 1.2, [2.0, -0.6, 3.0], p.steel, m);
    tower(doc, p, -0.3, 1.0, 2.2, m);
}

/// An ion implanter: an ion source in its high-voltage cage at the back
/// (+X), the analysing magnet bending the beam, the beamline and the end
/// station where the wafers are, its front end at −X.
fn ion_implanter(doc: &mut Document, p: &Paint, m: Option<ObjectId>) {
    front_end(doc, p, -5.0, 2.4, m);
    block(
        doc,
        "end_station",
        [2.0, 2.4, 2.2],
        [-3.0, 0.0, 1.1],
        p.white,
        m,
    );
    bar(
        doc,
        "beamline",
        0.18,
        2.4,
        [-0.8, 0.0, 1.3],
        true,
        p.steel,
        m,
    );
    for (k, x) in [-1.6, -0.6].into_iter().enumerate() {
        block(
            doc,
            &format!("beamline_stand_{k}"),
            [0.3, 0.5, 1.2],
            [x, 0.0, 0.6],
            p.grey,
            m,
        );
    }
    block(doc, "magnet", [1.6, 1.6, 1.4], [1.2, 0.0, 0.9], p.dark, m);
    for side in [-1.0, 1.0] {
        block(
            doc,
            &format!("coil_{side}"),
            [1.2, 0.12, 1.0],
            [1.2, side * 0.86, 0.9],
            p.copper,
            m,
        );
    }
    block(
        doc,
        "source_cage",
        [2.2, 2.2, 2.4],
        [3.1, 0.0, 1.2],
        p.grey,
        m,
    );
    block(
        doc,
        "source_window",
        [0.02, 1.4, 1.0],
        [1.99, 0.0, 1.5],
        p.glass,
        m,
    );
    block(
        doc,
        "control_rack",
        [0.8, 0.8, 2.0],
        [-3.0, 1.7, 1.0],
        p.white,
        m,
    );
    tower(doc, p, -3.6, 1.0, 2.2, m);
}

/// A CMP polisher: three platens with their pads, a head over each and the
/// slurry arm beside it, under a cover with windows; the cleaner and the
/// front end at −X.
fn cmp_tool(doc: &mut Document, p: &Paint, m: Option<ObjectId>) {
    front_end(doc, p, -4.4, 3.0, m);
    block(
        doc,
        "cleaner",
        [1.6, 3.0, 2.0],
        [-2.6, 0.0, 1.0],
        p.white,
        m,
    );
    block(
        doc,
        "polisher_base",
        [4.0, 3.0, 1.0],
        [0.0, 0.0, 0.5],
        p.white,
        m,
    );
    for (k, (x, y)) in [(-0.9, -0.7), (0.9, -0.7), (0.0, 0.7)]
        .into_iter()
        .enumerate()
    {
        post(
            doc,
            &format!("platen_{k}"),
            0.55,
            0.06,
            [x, y, 1.03],
            p.pad,
            m,
        );
        post(
            doc,
            &format!("head_{k}"),
            0.18,
            0.5,
            [x + 0.2, y, 1.3],
            p.dark,
            m,
        );
        bar(
            doc,
            &format!("slurry_arm_{k}"),
            0.03,
            0.6,
            [x - 0.3, y + 0.45, 1.2],
            true,
            p.steel,
            m,
        );
    }
    block(
        doc,
        "cover_back",
        [4.0, 0.05, 1.2],
        [0.0, 1.5, 1.6],
        p.glass,
        m,
    );
    block(
        doc,
        "cover_top",
        [4.0, 3.0, 0.06],
        [0.0, 0.0, 2.2],
        p.white,
        m,
    );
    for (k, x) in [-1.98, 1.98].into_iter().enumerate() {
        block(
            doc,
            &format!("cover_post_{k}"),
            [0.06, 3.0, 1.2],
            [x, 0.0, 1.6],
            p.trim,
            m,
        );
    }
    tower(doc, p, -1.9, 1.3, 2.2, m);
}

/// A wafer prober and its tester: the prober, a wafer under the probe
/// card, the test head swung over it, the tester's mainframe behind.
fn wafer_prober(doc: &mut Document, p: &Paint, m: Option<ObjectId>) {
    block(
        doc,
        "load_port",
        [0.5, 0.5, 0.9],
        [-1.15, -0.4, 0.45],
        p.grey,
        m,
    );
    block(doc, "prober", [1.8, 1.8, 1.3], [0.0, 0.0, 0.65], p.white, m);
    block(
        doc,
        "test_head",
        [1.3, 1.3, 0.45],
        [0.0, 0.0, 1.55],
        p.dark,
        m,
    );
    block(
        doc,
        "manipulator",
        [0.4, 0.6, 1.6],
        [1.1, 0.0, 0.8],
        p.grey,
        m,
    );
    block(doc, "tester", [1.2, 1.8, 1.9], [2.0, 0.0, 0.95], p.white, m);
    block(
        doc,
        "tester_band",
        [1.22, 1.82, 0.15],
        [2.0, 0.0, 1.6],
        p.trim,
        m,
    );
    block(
        doc,
        "screen",
        [0.03, 0.5, 0.35],
        [-0.92, 0.5, 1.15],
        p.dark,
        m,
    );
    tower(doc, p, -0.7, 0.7, 1.3, m);
}

/// A dicing saw: its cabinet, the cutting chamber behind a window, the
/// spindle in it, a screen on its front.
fn dicing_saw(doc: &mut Document, p: &Paint, m: Option<ObjectId>) {
    block(doc, "cabinet", [1.8, 1.4, 1.0], [0.0, 0.0, 0.5], p.white, m);
    block(
        doc,
        "chamber",
        [1.2, 1.4, 0.7],
        [0.3, 0.0, 1.35],
        p.white,
        m,
    );
    block(
        doc,
        "window",
        [0.02, 1.0, 0.45],
        [-0.31, 0.0, 1.35],
        p.glass,
        m,
    );
    post(doc, "spindle", 0.08, 0.3, [0.3, 0.0, 1.3], p.steel, m);
    block(
        doc,
        "cassette_elevator",
        [0.5, 0.6, 0.5],
        [-0.65, -0.3, 1.25],
        p.grey,
        m,
    );
    block(
        doc,
        "screen",
        [0.03, 0.4, 0.3],
        [-0.92, 0.35, 1.2],
        p.dark,
        m,
    );
    block(doc, "band", [1.82, 1.42, 0.08], [0.0, 0.0, 0.96], p.trim, m);
    tower(doc, p, 0.8, 0.6, 1.7, m);
}

/// A die or wire bonder: its base, the gantry carrying the bond head over
/// the work under its cover, the magazines at either end.
fn bonder(doc: &mut Document, p: &Paint, wire: bool, m: Option<ObjectId>) {
    block(doc, "base", [1.6, 1.2, 0.95], [0.0, 0.0, 0.475], p.white, m);
    block(doc, "band", [1.62, 1.22, 0.08], [0.0, 0.0, 0.92], p.trim, m);
    block(
        doc,
        "work_table",
        [1.0, 0.5, 0.05],
        [0.0, 0.0, 1.0],
        p.steel,
        m,
    );
    for (k, x) in [-0.45, 0.45].into_iter().enumerate() {
        block(
            doc,
            &format!("gantry_post_{k}"),
            [0.08, 0.1, 0.5],
            [x, 0.3, 1.2],
            p.dark,
            m,
        );
    }
    block(doc, "gantry", [1.0, 0.12, 0.1], [0.0, 0.3, 1.45], p.dark, m);
    if wire {
        // The wire spool over the head, its capillary down to the work.
        post(doc, "spool", 0.06, 0.05, [0.0, 0.18, 1.55], p.copper, m);
        post(doc, "capillary", 0.01, 0.35, [0.0, 0.12, 1.25], p.steel, m);
    } else {
        block(
            doc,
            "bond_head",
            [0.15, 0.15, 0.3],
            [0.0, 0.18, 1.3],
            p.grey,
            m,
        );
        block(
            doc,
            "wafer_table",
            [0.4, 0.4, 0.04],
            [0.5, -0.25, 1.02],
            p.dark,
            m,
        );
    }
    block(doc, "cover", [1.0, 0.7, 0.03], [0.0, 0.0, 1.62], p.glass, m);
    for (k, x) in [-0.95, 0.95].into_iter().enumerate() {
        block(
            doc,
            &format!("magazine_{k}"),
            [0.3, 0.35, 0.5],
            [x, 0.0, 1.2],
            p.grey,
            m,
        );
    }
    tower(doc, p, 0.7, -0.5, 1.0, m);
}

/// A mould press: the lower and upper platens on four tie bars, the
/// moulding compound loader at its side, its control cabinet.
fn mold_press(doc: &mut Document, p: &Paint, m: Option<ObjectId>) {
    block(doc, "base", [1.8, 1.6, 0.8], [0.0, 0.0, 0.4], p.white, m);
    block(
        doc,
        "lower_platen",
        [1.2, 1.0, 0.2],
        [0.0, 0.0, 0.9],
        p.steel,
        m,
    );
    for (k, (x, y)) in [(-0.5, -0.4), (0.5, -0.4), (-0.5, 0.4), (0.5, 0.4)]
        .into_iter()
        .enumerate()
    {
        post(
            doc,
            &format!("tie_bar_{k}"),
            0.05,
            1.0,
            [x, y, 1.3],
            p.steel,
            m,
        );
    }
    block(
        doc,
        "upper_platen",
        [1.2, 1.0, 0.25],
        [0.0, 0.0, 1.55],
        p.steel,
        m,
    );
    block(
        doc,
        "cylinder",
        [0.6, 0.6, 0.4],
        [0.0, 0.0, 1.88],
        p.trim,
        m,
    );
    block(doc, "loader", [0.8, 1.2, 1.2], [-1.3, 0.0, 0.6], p.white, m);
    block(doc, "cabinet", [0.6, 1.6, 1.8], [1.25, 0.0, 0.9], p.grey, m);
    tower(doc, p, 1.25, 0.6, 1.8, m);
}

/// A test handler and its tester: the handler taking chips from their trays
/// into the test sockets under its window, the tester's head docked on it,
/// the tray stackers at its front.
fn test_handler(doc: &mut Document, p: &Paint, m: Option<ObjectId>) {
    block(doc, "handler", [2.0, 1.6, 1.4], [0.0, 0.0, 0.7], p.white, m);
    block(
        doc,
        "window",
        [0.02, 1.0, 0.5],
        [-1.01, 0.0, 1.05],
        p.glass,
        m,
    );
    for (k, y) in [-0.5, 0.0, 0.5].into_iter().enumerate() {
        block(
            doc,
            &format!("tray_stack_{k}"),
            [0.4, 0.35, 0.3],
            [-1.25, y, 0.95],
            p.dark,
            m,
        );
    }
    block(
        doc,
        "shelf",
        [0.5, 1.6, 0.05],
        [-1.25, 0.0, 0.78],
        p.grey,
        m,
    );
    block(
        doc,
        "test_head",
        [1.0, 1.0, 0.5],
        [0.4, 0.0, 1.65],
        p.dark,
        m,
    );
    block(doc, "tester", [1.2, 1.6, 1.9], [1.7, 0.0, 0.95], p.white, m);
    block(
        doc,
        "tester_band",
        [1.22, 1.62, 0.15],
        [1.7, 0.0, 1.6],
        p.trim,
        m,
    );
    tower(doc, p, -0.6, 0.65, 1.4, m);
}

/// A semiconductor fab's flow part at its stage: a FOUP (0) — its shell, its
/// door at the front, the flange on top an overhead hoist takes it by,
/// 0.34 m high —; a wafer (1), its dies on it; a tray of packaged chips
/// (2); or (3) the overhead hoist's vehicle alone, from its gripper at the
/// bottom, where a FOUP's top is, up to its wheels on the rail.
pub(crate) fn semi_look(stage: usize) -> LookMeshes {
    let mut doc = Document::new();
    match stage {
        0 => {
            let shell = material(&mut doc, "foup shell", [0.80, 0.82, 0.86], 0.1);
            let door = material(&mut doc, "foup door", [0.45, 0.55, 0.70], 0.1);
            let flange = material(&mut doc, "foup flange", [0.25, 0.27, 0.30], 0.2);
            block(
                &mut doc,
                "shell",
                [0.39, 0.33, 0.29],
                [0.0, 0.0, 0.145],
                shell,
                None,
            );
            block(
                &mut doc,
                "door",
                [0.02, 0.30, 0.25],
                [-0.2, 0.0, 0.145],
                door,
                None,
            );
            block(
                &mut doc,
                "neck",
                [0.08, 0.08, 0.03],
                [0.0, 0.0, 0.305],
                flange,
                None,
            );
            block(
                &mut doc,
                "flange",
                [0.25, 0.20, 0.02],
                [0.0, 0.0, 0.33],
                flange,
                None,
            );
            for side in [-1.0, 1.0] {
                block(
                    &mut doc,
                    &format!("handle_{side}"),
                    [0.2, 0.02, 0.06],
                    [0.0, side * 0.175, 0.17],
                    flange,
                    None,
                );
            }
        }
        1 => {
            let silicon = material(&mut doc, "silicon", [0.42, 0.44, 0.52], 0.6);
            let die = material(&mut doc, "die", [0.30, 0.25, 0.45], 0.4);
            post(
                &mut doc,
                "wafer",
                0.15,
                0.003,
                [0.0, 0.0, 0.0015],
                silicon,
                None,
            );
            for i in -3..=3 {
                for j in -3..=3 {
                    let (x, y) = (i as f64 * 0.036, j as f64 * 0.036);
                    if x.hypot(y) > 0.125 {
                        continue;
                    }
                    block(
                        &mut doc,
                        &format!("die_{i}_{j}"),
                        [0.03, 0.03, 0.001],
                        [x, y, 0.0035],
                        die,
                        None,
                    );
                }
            }
        }
        2 => {
            let tray = material(&mut doc, "tray", [0.10, 0.12, 0.14], 0.0);
            let chip = material(&mut doc, "chip", [0.03, 0.03, 0.03], 0.2);
            let mark = material(&mut doc, "chip mark", [0.85, 0.85, 0.80], 0.0);
            block(
                &mut doc,
                "tray",
                [0.32, 0.14, 0.01],
                [0.0, 0.0, 0.005],
                tray,
                None,
            );
            for i in 0..6 {
                for j in 0..3 {
                    let (x, y) = (-0.125 + 0.05 * i as f64, -0.04 + 0.04 * j as f64);
                    block(
                        &mut doc,
                        &format!("chip_{i}_{j}"),
                        [0.03, 0.03, 0.008],
                        [x, y, 0.014],
                        chip,
                        None,
                    );
                    block(
                        &mut doc,
                        &format!("mark_{i}_{j}"),
                        [0.01, 0.01, 0.001],
                        [x - 0.007, y - 0.007, 0.0185],
                        mark,
                        None,
                    );
                }
            }
        }
        _ => {
            let body = material(&mut doc, "hoist body", [0.88, 0.89, 0.90], 0.1);
            let dark = material(&mut doc, "hoist gripper", [0.20, 0.22, 0.25], 0.3);
            block(
                &mut doc,
                "gripper",
                [0.32, 0.26, 0.05],
                [0.0, 0.0, 0.025],
                dark,
                None,
            );
            block(
                &mut doc,
                "body",
                [0.75, 0.50, 0.45],
                [0.0, 0.0, 0.3],
                body,
                None,
            );
            block(
                &mut doc,
                "wheels",
                [0.5, 0.25, 0.15],
                [0.0, 0.0, 0.6],
                dark,
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
            assert!(lowest > -1e-6 && lowest < 0.05, "{key}: lowest at {lowest}");
        }
    }

    #[test]
    fn the_foup_the_wafer_the_tray_and_the_hoist_each_have_their_look() {
        for stage in 0..4 {
            let look = semi_look(stage);
            assert!(look.len() >= 2, "stage {stage}: more than one colour");
            assert!(look.len() <= 4, "stage {stage}: room left for the hoist's");
            assert!(look.iter().all(|(_, m)| m.num_faces() > 0));
        }
    }
}
