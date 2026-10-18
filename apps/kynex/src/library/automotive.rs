//! A car plant's own equipment, for its five shops: the body itself at
//! each stage — welded steel (BIW), painted, finished — the welding jig it
//! is framed in, the paint booth and the curing oven, the assembly line's
//! slat conveyor, and the light tunnel it is inspected in. None has a role
//! in the cell: the shops' work is the process flow's stations; these are
//! what it looks like.
//!
//! Each stands on the floor at its middle, its length along X, the way a
//! body travels down the line (+X); a body faces +X.

use kreate_math::{DQuat, SceneRgb};
use kreate_scene::{Document, Material, MaterialId, Object, ObjectData, ObjectId};

use super::{bar, block, post, Item};

/// The templates this module makes.
pub const KEYS: &[&str] = &[
    "car_body",
    "welding_jig",
    "paint_booth",
    "curing_oven",
    "assembly_line",
    "light_tunnel",
];

/// A body's stage, by number.
pub const STAGES: &[&str] = &["차체 (BIW)", "도장 차체", "완성차"];

/// A body's colour, by number.
pub const COLOURS: &[&str] = &["흰색", "빨강", "파랑", "은색", "검정"];

/// The colours of a car plant.
struct Paint {
    steel: MaterialId,
    dark: MaterialId,
    black: MaterialId,
    glass: MaterialId,
    yellow: MaterialId,
    white: MaterialId,
    light: MaterialId,
    panel: MaterialId,
    blue: MaterialId,
    red: MaterialId,
    grating: MaterialId,
    walkway: MaterialId,
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
            steel: material(doc, "steel", [0.70, 0.72, 0.75], 0.8),
            dark: material(doc, "dark grey", [0.20, 0.22, 0.25], 0.2),
            black: material(doc, "black", [0.05, 0.05, 0.06], 0.0),
            glass: material(doc, "glass", [0.25, 0.36, 0.45], 0.4),
            yellow: material(doc, "safety yellow", [0.96, 0.76, 0.06], 0.0),
            white: material(doc, "white", [0.93, 0.93, 0.92], 0.0),
            light: material(doc, "lamp", [1.0, 0.98, 0.92], 0.0),
            panel: material(doc, "booth panel", [0.88, 0.90, 0.91], 0.1),
            blue: material(doc, "machine blue", [0.18, 0.33, 0.52], 0.2),
            red: material(doc, "signal red", [0.75, 0.10, 0.10], 0.0),
            grating: material(doc, "floor grating", [0.30, 0.32, 0.34], 0.5),
            walkway: material(doc, "walkway", [0.20, 0.46, 0.33], 0.0),
        }
    }
}

/// A box tilted about Y by `angle`: a positive one takes its +X end down
/// and its +Z end forward (+X).
#[allow(clippy::too_many_arguments)]
fn tilted(
    doc: &mut Document,
    name: &str,
    size: [f64; 3],
    centre: [f64; 3],
    angle: f64,
    colour: MaterialId,
    parent: Option<ObjectId>,
) -> ObjectId {
    let id = block(doc, name, size, centre, colour, parent);
    doc.object_mut(id).unwrap().transform.rotation = DQuat::from_rotation_y(angle);
    id
}

/// Builds `item` into `doc`, if it is one of [`KEYS`].
pub(super) fn build(doc: &mut Document, item: &Item) {
    let p = Paint::new(doc);
    let root = doc.add_object(Object::new(item.template.clone(), ObjectData::Empty));
    let m = Some(root);
    match item.template.as_str() {
        "car_body" => {
            let stage = item.param("stage").round() as usize;
            let colour = item.param("colour").round() as usize;
            car_body(doc, &p, stage, colour, m);
        }
        "welding_jig" => welding_jig(doc, &p, m),
        "paint_booth" => paint_booth(doc, &p, item.param("length"), m),
        "curing_oven" => curing_oven(doc, &p, item.param("length"), m),
        "assembly_line" => assembly_line(doc, &p, item.param("length"), m),
        "light_tunnel" => light_tunnel(doc, &p, item.param("length"), m),
        _ => {}
    }
}

/// A car body at `stage` in `colour`, as meshes to draw a part of a process
/// flow with: one for each colour it has, in the body's own frame — its
/// middle on the floor, facing +X.
pub(crate) fn car_look(stage: usize, colour: usize) -> super::LookMeshes {
    let mut doc = Document::new();
    let p = Paint::new(&mut doc);
    car_body(&mut doc, &p, stage, colour, None);
    super::look_of(&doc)
}

/// A car body 4.6 m long, facing +X. Welded steel (stage 0) or painted
/// (1) it rides a skid, open where its glass and wheels will go; finished
/// (2) it stands on its wheels, glazed, its lights in.
fn car_body(doc: &mut Document, p: &Paint, stage: usize, colour: usize, m: Option<ObjectId>) {
    let rgb = match colour {
        0 => [0.92, 0.92, 0.90],
        1 => [0.70, 0.06, 0.08],
        2 => [0.10, 0.22, 0.52],
        3 => [0.66, 0.68, 0.70],
        _ => [0.06, 0.06, 0.07],
    };
    let body = if stage == 0 {
        p.steel
    } else {
        material(doc, "body paint", rgb, 0.45)
    };
    let finished = stage >= 2;
    // The skid a body rides through the shops, or the wheels it ends on.
    let zb = if finished {
        0.2
    } else {
        for (k, y) in [-0.5, 0.5].into_iter().enumerate() {
            block(
                doc,
                &format!("skid_rail_{k}"),
                [4.2, 0.08, 0.1],
                [0.0, y, 0.05],
                p.dark,
                m,
            );
        }
        for (k, x) in [-1.6, 0.0, 1.6].into_iter().enumerate() {
            block(
                doc,
                &format!("skid_cross_{k}"),
                [0.08, 1.2, 0.08],
                [x, 0.0, 0.14],
                p.dark,
                m,
            );
        }
        for (k, (x, y)) in [(-1.4, -0.5), (-1.4, 0.5), (1.4, -0.5), (1.4, 0.5)]
            .into_iter()
            .enumerate()
        {
            post(
                doc,
                &format!("skid_post_{k}"),
                0.04,
                0.16,
                [x, y, 0.26],
                p.dark,
                m,
            );
        }
        0.34
    };
    block(
        doc,
        "lower_body",
        [4.4, 1.78, 0.52],
        [0.0, 0.0, zb + 0.28],
        body,
        m,
    );
    block(
        doc,
        "front_bumper",
        [0.22, 1.74, 0.36],
        [2.29, 0.0, zb + 0.22],
        body,
        m,
    );
    block(
        doc,
        "rear_bumper",
        [0.2, 1.74, 0.36],
        [-2.28, 0.0, zb + 0.24],
        body,
        m,
    );
    tilted(
        doc,
        "hood",
        [1.35, 1.72, 0.08],
        [1.55, 0.0, zb + 0.6],
        0.09,
        body,
        m,
    );
    block(
        doc,
        "deck",
        [0.75, 1.72, 0.1],
        [-1.85, 0.0, zb + 0.6],
        body,
        m,
    );
    block(
        doc,
        "roof",
        [1.85, 1.46, 0.07],
        [-0.3, 0.0, zb + 1.3],
        body,
        m,
    );
    for side in [-1.0, 1.0] {
        let y = side * 0.74;
        tilted(
            doc,
            &format!("a_pillar_{side}"),
            [0.08, 0.07, 0.76],
            [0.79, y, zb + 0.97],
            -0.47,
            body,
            m,
        );
        block(
            doc,
            &format!("b_pillar_{side}"),
            [0.09, 0.07, 0.72],
            [-0.25, y, zb + 0.92],
            body,
            m,
        );
        tilted(
            doc,
            &format!("c_pillar_{side}"),
            [0.12, 0.07, 0.72],
            [-1.36, y, zb + 0.97],
            0.4,
            body,
            m,
        );
        // The doors' lines.
        for (k, x) in [0.55, -0.25, -1.05].into_iter().enumerate() {
            block(
                doc,
                &format!("door_line_{side}_{k}"),
                [0.012, 0.01, 0.48],
                [x, side * 0.895, zb + 0.3],
                p.dark,
                m,
            );
        }
        // Wheel arches: open on a body, tyres in on a car.
        for (k, x) in [1.38, -1.38].into_iter().enumerate() {
            if finished {
                bar(
                    doc,
                    &format!("tyre_{side}_{k}"),
                    0.33,
                    0.24,
                    [x, side * 0.78, 0.33],
                    false,
                    p.black,
                    m,
                );
                bar(
                    doc,
                    &format!("rim_{side}_{k}"),
                    0.2,
                    0.25,
                    [x, side * 0.78, 0.33],
                    false,
                    p.steel,
                    m,
                );
            } else {
                bar(
                    doc,
                    &format!("arch_{side}_{k}"),
                    0.36,
                    0.06,
                    [x, side * 0.87, zb + 0.15],
                    false,
                    p.black,
                    m,
                );
            }
        }
        if finished {
            block(
                doc,
                &format!("side_glass_{side}"),
                [1.75, 0.02, 0.38],
                [-0.25, side * 0.75, zb + 1.03],
                p.glass,
                m,
            );
            block(
                doc,
                &format!("mirror_{side}"),
                [0.12, 0.16, 0.1],
                [0.75, side * 0.98, zb + 0.75],
                body,
                m,
            );
            block(
                doc,
                &format!("headlight_{side}"),
                [0.04, 0.38, 0.12],
                [2.39, side * 0.6, zb + 0.43],
                p.light,
                m,
            );
            block(
                doc,
                &format!("taillight_{side}"),
                [0.04, 0.34, 0.12],
                [-2.38, side * 0.62, zb + 0.46],
                p.red,
                m,
            );
        }
    }
    if finished {
        tilted(
            doc,
            "windshield",
            [0.75, 1.44, 0.03],
            [0.79, 0.0, zb + 0.97],
            1.1,
            p.glass,
            m,
        );
        tilted(
            doc,
            "rear_glass",
            [0.71, 1.4, 0.03],
            [-1.36, 0.0, zb + 0.97],
            -1.17,
            p.glass,
            m,
        );
        block(
            doc,
            "grille",
            [0.03, 0.8, 0.18],
            [2.41, 0.0, zb + 0.35],
            p.black,
            m,
        );
    }
}

/// A body-framing jig: its base, the locating pins and the clamps all
/// round, ready for a body on its top (0.7 m up) and the robots about it.
fn welding_jig(doc: &mut Document, p: &Paint, m: Option<ObjectId>) {
    block(doc, "base", [5.4, 2.6, 0.62], [0.0, 0.0, 0.31], p.blue, m);
    block(
        doc,
        "top_plate",
        [5.2, 2.4, 0.08],
        [0.0, 0.0, 0.66],
        p.steel,
        m,
    );
    for (k, x) in [-2.2, -1.1, 0.0, 1.1, 2.2].into_iter().enumerate() {
        for side in [-1.0, 1.0] {
            let y = side * 1.25;
            block(
                doc,
                &format!("clamp_post_{k}_{side}"),
                [0.14, 0.14, 0.7],
                [x, y, 1.05],
                p.dark,
                m,
            );
            block(
                doc,
                &format!("clamp_{k}_{side}"),
                [0.24, 0.2, 0.12],
                [x, y - side * 0.1, 1.42],
                p.yellow,
                m,
            );
            block(
                doc,
                &format!("clamp_arm_{k}_{side}"),
                [0.06, 0.3, 0.05],
                [x, y - side * 0.3, 1.36],
                p.steel,
                m,
            );
        }
    }
    for (k, x) in [-1.6, 1.6].into_iter().enumerate() {
        post(
            doc,
            &format!("locating_pin_{k}"),
            0.04,
            0.12,
            [x, 0.0, 0.76],
            p.steel,
            m,
        );
    }
    block(
        doc,
        "cable_tray",
        [5.0, 0.2, 0.08],
        [0.0, -1.45, 0.7],
        p.dark,
        m,
    );
    block(
        doc,
        "valve_box",
        [0.5, 0.3, 0.4],
        [-2.6, -1.5, 0.2],
        p.dark,
        m,
    );
}

/// A spray booth `l` long: white walls with a band of windows, the plenum
/// above blowing down, exhaust stacks, the floor grating, rails for the
/// skids and lights along the walls; open at both ends.
fn paint_booth(doc: &mut Document, p: &Paint, l: f64, m: Option<ObjectId>) {
    let (w, h) = (6.0, 4.5);
    for side in [-1.0, 1.0] {
        let y = side * w / 2.0;
        block(
            doc,
            &format!("wall_low_{side}"),
            [l, 0.15, 1.6],
            [0.0, y, 0.8],
            p.panel,
            m,
        );
        block(
            doc,
            &format!("window_{side}"),
            [l, 0.12, 1.2],
            [0.0, y, 2.2],
            p.glass,
            m,
        );
        block(
            doc,
            &format!("wall_high_{side}"),
            [l, 0.15, h - 2.8],
            [0.0, y, 2.8 + (h - 2.8) / 2.0],
            p.panel,
            m,
        );
        let posts = ((l / 2.0).ceil() as usize).max(1);
        for k in 0..=posts {
            let x = -l / 2.0 + l * k as f64 / posts as f64;
            block(
                doc,
                &format!("mullion_{side}_{k}"),
                [0.08, 0.17, 1.2],
                [x, y, 2.2],
                p.dark,
                m,
            );
        }
        block(
            doc,
            &format!("lamp_strip_{side}"),
            [l - 0.5, 0.05, 0.12],
            [0.0, y - side * 0.12, 3.6],
            p.light,
            m,
        );
    }
    block(doc, "ceiling", [l, w, 0.15], [0.0, 0.0, h], p.panel, m);
    block(
        doc,
        "plenum",
        [l, w * 0.8, 0.9],
        [0.0, 0.0, h + 0.52],
        p.panel,
        m,
    );
    let stacks = ((l / 6.0).ceil() as usize).max(1);
    for k in 0..stacks {
        let x = -l / 2.0 + l * (k as f64 + 0.5) / stacks as f64;
        post(
            doc,
            &format!("exhaust_{k}"),
            0.4,
            2.2,
            [x, 1.4, h + 2.0],
            p.steel,
            m,
        );
    }
    block(
        doc,
        "grating",
        [l, w - 0.3, 0.05],
        [0.0, 0.0, 0.025],
        p.grating,
        m,
    );
    for (k, y) in [-0.5, 0.5].into_iter().enumerate() {
        block(
            doc,
            &format!("rail_{k}"),
            [l, 0.1, 0.12],
            [0.0, y, 0.11],
            p.steel,
            m,
        );
    }
    for side in [-1.0, 1.0] {
        block(
            doc,
            &format!("portal_{side}"),
            [0.2, w + 0.3, 0.4],
            [side * l / 2.0, 0.0, h - 0.2],
            p.yellow,
            m,
        );
    }
}

/// A curing oven `l` long: an insulated tunnel, its air seals at the ends,
/// burner boxes and their stacks on the roof, the ducts along its sides.
fn curing_oven(doc: &mut Document, p: &Paint, l: f64, m: Option<ObjectId>) {
    let (w, h) = (4.4, 3.8);
    for side in [-1.0, 1.0] {
        block(
            doc,
            &format!("wall_{side}"),
            [l, 0.3, h],
            [0.0, side * w / 2.0, h / 2.0],
            p.steel,
            m,
        );
        bar(
            doc,
            &format!("duct_{side}"),
            0.22,
            l,
            [0.0, side * (w / 2.0 + 0.35), h - 0.6],
            true,
            p.steel,
            m,
        );
        // The air seal's housing over each end, the opening below it.
        block(
            doc,
            &format!("seal_{side}"),
            [0.6, w + 0.6, 1.4],
            [side * l / 2.0, 0.0, h - 0.7],
            p.dark,
            m,
        );
        block(
            doc,
            &format!("seal_stripe_{side}"),
            [0.62, w + 0.62, 0.12],
            [side * l / 2.0, 0.0, h - 1.46],
            p.yellow,
            m,
        );
    }
    block(
        doc,
        "roof",
        [l, w + 0.3, 0.3],
        [0.0, 0.0, h + 0.15],
        p.steel,
        m,
    );
    let burners = ((l / 5.0).ceil() as usize).max(1);
    for k in 0..burners {
        let x = -l / 2.0 + l * (k as f64 + 0.5) / burners as f64;
        block(
            doc,
            &format!("burner_{k}"),
            [1.4, 1.6, 1.1],
            [x, 0.0, h + 0.85],
            p.dark,
            m,
        );
        post(
            doc,
            &format!("stack_{k}"),
            0.18,
            1.6,
            [x + 0.4, 0.4, h + 2.2],
            p.steel,
            m,
        );
        block(
            doc,
            &format!("burner_panel_{k}"),
            [0.4, 0.04, 0.3],
            [x, -0.82, h + 0.9],
            p.red,
            m,
        );
    }
    block(doc, "floor", [l, w, 0.1], [0.0, 0.0, 0.05], p.dark, m);
}

/// An assembly line's slat conveyor `l` long, the bodies riding it on
/// their wheels: the slats, its yellow edges, walkways each side, and
/// tool balancers on their rail overhead.
fn assembly_line(doc: &mut Document, p: &Paint, l: f64, m: Option<ObjectId>) {
    let w = 2.8;
    block(doc, "conveyor", [l, w, 0.08], [0.0, 0.0, 0.04], p.dark, m);
    let slats = ((l / 0.6).floor() as usize).min(200);
    for k in 0..slats {
        let x = -l / 2.0 + 0.3 + 0.6 * k as f64;
        block(
            doc,
            &format!("slat_{k}"),
            [0.04, w - 0.1, 0.085],
            [x, 0.0, 0.0425],
            p.grating,
            m,
        );
    }
    for side in [-1.0, 1.0] {
        block(
            doc,
            &format!("edge_{side}"),
            [l, 0.12, 0.09],
            [0.0, side * (w / 2.0 + 0.06), 0.045],
            p.yellow,
            m,
        );
        block(
            doc,
            &format!("walkway_{side}"),
            [l, 1.4, 0.03],
            [0.0, side * (w / 2.0 + 0.82), 0.015],
            p.walkway,
            m,
        );
    }
    let bays = ((l / 4.0).ceil() as usize).max(1);
    for k in 0..=bays {
        let x = -l / 2.0 + l * k as f64 / bays as f64;
        for side in [-1.0, 1.0] {
            block(
                doc,
                &format!("post_{k}_{side}"),
                [0.15, 0.15, 3.6],
                [x, side * (w / 2.0 + 1.7), 1.8],
                p.white,
                m,
            );
        }
        block(
            doc,
            &format!("beam_{k}"),
            [0.15, w + 3.55, 0.15],
            [x, 0.0, 3.6],
            p.white,
            m,
        );
    }
    for side in [-1.0, 1.0] {
        let y = side * (w / 2.0 + 0.5);
        block(
            doc,
            &format!("balancer_rail_{side}"),
            [l, 0.1, 0.12],
            [0.0, y, 3.45],
            p.steel,
            m,
        );
        for k in 0..bays {
            let x = -l / 2.0 + l * (k as f64 + 0.5) / bays as f64;
            block(
                doc,
                &format!("balancer_{side}_{k}"),
                [0.2, 0.2, 0.25],
                [x, y, 3.2],
                p.yellow,
                m,
            );
            post(
                doc,
                &format!("tool_cord_{side}_{k}"),
                0.01,
                1.2,
                [x, y, 2.5],
                p.black,
                m,
            );
            block(
                doc,
                &format!("tool_{side}_{k}"),
                [0.1, 0.08, 0.25],
                [x, y, 1.8],
                p.dark,
                m,
            );
        }
    }
}

/// An inspection light tunnel `l` long: arches of light bars all round a
/// body, on a dark floor with yellow edges, to show every flaw in its
/// paint.
fn light_tunnel(doc: &mut Document, p: &Paint, l: f64, m: Option<ObjectId>) {
    let (w, h) = (4.6, 3.2);
    block(
        doc,
        "floor",
        [l + 1.0, w, 0.06],
        [0.0, 0.0, 0.03],
        p.black,
        m,
    );
    for side in [-1.0, 1.0] {
        block(
            doc,
            &format!("edge_{side}"),
            [l + 1.0, 0.1, 0.065],
            [0.0, side * (w / 2.0 - 0.05), 0.033],
            p.yellow,
            m,
        );
    }
    let arches = ((l / 0.8).floor() as usize).max(2);
    for k in 0..=arches {
        let x = -l / 2.0 + l * k as f64 / arches as f64;
        for side in [-1.0, 1.0] {
            let y = side * w / 2.0;
            block(
                doc,
                &format!("leg_{k}_{side}"),
                [0.12, 0.12, h],
                [x, y, h / 2.0],
                p.dark,
                m,
            );
            block(
                doc,
                &format!("leg_light_{k}_{side}"),
                [0.06, 0.03, h - 0.4],
                [x, y - side * 0.075, h / 2.0],
                p.light,
                m,
            );
        }
        block(
            doc,
            &format!("top_{k}"),
            [0.12, w + 0.12, 0.12],
            [x, 0.0, h],
            p.dark,
            m,
        );
        block(
            doc,
            &format!("top_light_{k}"),
            [0.06, w - 0.3, 0.03],
            [x, 0.0, h - 0.075],
            p.light,
            m,
        );
    }
    block(
        doc,
        "monitor",
        [0.05, 0.9, 0.6],
        [l / 2.0 + 0.8, -w / 2.0, 1.6],
        p.dark,
        m,
    );
    block(
        doc,
        "monitor_screen",
        [0.055, 0.8, 0.5],
        [l / 2.0 + 0.8, -w / 2.0, 1.6],
        p.blue,
        m,
    );
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::*;
    use crate::library::Library;

    #[test]
    fn every_piece_of_the_car_plant_is_made_and_stands_on_the_floor() {
        let mut cases: Vec<(&str, Value)> = KEYS.iter().map(|k| (*k, json!({}))).collect();
        for stage in 0..STAGES.len() {
            for colour in 0..COLOURS.len() {
                cases.push(("car_body", json!({"stage": stage, "colour": colour})));
            }
        }
        for (key, params) in cases {
            let mut library = Library::default();
            let req =
                json!({"op": "place", "template": key, "at": [1.0, 2.0, 0.0], "params": params});
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
            assert!(shapes > 8, "{key} {params}: drawn in some detail");
            assert!(
                lowest > -1e-6 && lowest < 0.05,
                "{key} {params}: lowest at {lowest}"
            );
        }
    }
}
