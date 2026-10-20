//! A beverage plant's own equipment, for a PET bottling line: the stretch
//! blow moulder that makes bottles from preforms, the rinser–filler–capper
//! monoblock, the fill-level and cap inspector, the labeller, the shrink
//! wrapper, the palletiser and the stretch wrapper; the stainless process
//! tanks of the syrup room, and the narrow table-top conveyor bottles run
//! on. None has a role in the cell: the line's work is the process flow's
//! stations; these are what it looks like. And what goes down the line, as
//! a flow part looks: a tray of 24 — preforms, empty bottles, filled and
//! capped, labelled —, a shrink-wrapped pack, a wrapped pallet.
//!
//! Each stands on the floor at its middle, its length along X, the way
//! bottles go down the line (+X).

use kreate_math::SceneRgb;
use kreate_scene::{Document, Material, MaterialId, Object, ObjectData, ObjectId};

use super::{bar, block, post, Item, LookMeshes};

/// The templates this module makes.
pub const KEYS: &[&str] = &[
    "blow_molder",
    "filler_capper",
    "bottle_inspector",
    "labeler",
    "shrink_wrapper",
    "palletizer",
    "stretch_wrapper",
    "process_tank",
    "bottle_conveyor",
];

/// What goes down a bottling line, by number, as a flow part looks.
pub const STAGES: &[&str] = &[
    "프리폼 (24개)",
    "빈 병 (24병)",
    "충전·캡 병 (24병)",
    "라벨 병 (24병)",
    "수축 포장 팩",
    "음료 팔레트",
];

struct Paint {
    white: MaterialId,
    steel: MaterialId,
    dark: MaterialId,
    glass: MaterialId,
    blue: MaterialId,
    green: MaterialId,
    red: MaterialId,
    amber: MaterialId,
    belt: MaterialId,
    film: MaterialId,
    yellow: MaterialId,
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
            white: material(doc, "machine panel", [0.55, 0.64, 0.74], 0.15),
            steel: material(doc, "stainless", [0.76, 0.78, 0.80], 0.85),
            dark: material(doc, "dark grey", [0.20, 0.22, 0.25], 0.2),
            glass: material(doc, "guard glass", [0.55, 0.70, 0.78], 0.3),
            blue: material(doc, "machine blue", [0.10, 0.36, 0.68], 0.1),
            green: material(doc, "lamp green", [0.15, 0.75, 0.30], 0.0),
            red: material(doc, "heater red", [0.80, 0.22, 0.12], 0.0),
            amber: material(doc, "lamp amber", [0.98, 0.65, 0.10], 0.0),
            belt: material(doc, "chain belt", [0.30, 0.32, 0.34], 0.4),
            film: material(doc, "film roll", [0.85, 0.88, 0.90], 0.1),
            yellow: material(doc, "safety yellow", [0.96, 0.76, 0.06], 0.0),
        }
    }
}

/// Builds `item` into `doc`, if it is one of [`KEYS`].
pub(super) fn build(doc: &mut Document, item: &Item) {
    let p = Paint::new(doc);
    let root = doc.add_object(Object::new(item.template.clone(), ObjectData::Empty));
    let m = Some(root);
    match item.template.as_str() {
        "blow_molder" => blow_molder(doc, &p, m),
        "filler_capper" => filler_capper(doc, &p, m),
        "bottle_inspector" => bottle_inspector(doc, &p, m),
        "labeler" => labeler(doc, &p, m),
        "shrink_wrapper" => shrink_wrapper(doc, &p, item.param("length"), m),
        "palletizer" => palletizer(doc, &p, m),
        "stretch_wrapper" => stretch_wrapper(doc, &p, m),
        "process_tank" => process_tank(doc, &p, item.param("diameter"), item.param("height"), m),
        "bottle_conveyor" => {
            bottle_conveyor(doc, &p, item.param("length"), item.param("height"), m)
        }
        _ => {}
    }
}

/// A signal tower: green, amber and red lamps on a post, its foot at
/// `foot`.
fn tower(doc: &mut Document, p: &Paint, foot: [f64; 3], m: Option<ObjectId>) {
    let [x, y, z] = foot;
    post(doc, "tower post", 0.02, 0.3, [x, y, z + 0.15], p.dark, m);
    post(doc, "lamp green", 0.04, 0.08, [x, y, z + 0.34], p.green, m);
    post(doc, "lamp amber", 0.04, 0.08, [x, y, z + 0.42], p.amber, m);
    post(doc, "lamp red", 0.04, 0.08, [x, y, z + 0.50], p.red, m);
}

/// A stretch blow moulder, 6.5 × 3.4 m: preforms tipped from a hopper at
/// its −X end, heated in its oven and blown in the moulds of its wheel
/// behind glass; bottles out at its +X end, on air.
fn blow_molder(doc: &mut Document, p: &Paint, m: Option<ObjectId>) {
    block(doc, "base", [6.5, 3.4, 0.5], [0.0, 0.0, 0.25], p.dark, m);
    // The oven, its heaters glowing through a slot.
    block(doc, "oven", [3.0, 2.4, 2.2], [-1.6, 0.0, 1.6], p.white, m);
    block(
        doc,
        "heaters",
        [2.6, 0.05, 0.4],
        [-1.6, -1.22, 1.6],
        p.red,
        m,
    );
    // The blow wheel's cabinet, glass in front.
    block(
        doc,
        "wheel cabinet",
        [3.2, 3.2, 2.4],
        [1.6, 0.0, 1.7],
        p.white,
        m,
    );
    block(
        doc,
        "window",
        [2.6, 0.04, 1.2],
        [1.6, -1.62, 1.8],
        p.glass,
        m,
    );
    post(doc, "blow wheel", 1.1, 0.35, [1.6, 0.0, 1.3], p.steel, m);
    // The preform hopper and its elevator.
    block(doc, "hopper", [1.6, 1.6, 1.4], [-3.9, 1.0, 1.9], p.blue, m);
    block(
        doc,
        "elevator",
        [0.4, 0.4, 2.6],
        [-3.4, 0.2, 1.3],
        p.steel,
        m,
    );
    block(
        doc,
        "control panel",
        [0.8, 0.4, 1.8],
        [-0.2, -1.95, 0.9],
        p.white,
        m,
    );
    tower(doc, p, [2.9, -1.4, 2.9], m);
}

/// A rinser–filler–capper monoblock, 6 × 4.4 m: three wheels inside its
/// guard's frame — the rinser at −X, the filler's carousel and its bowl in
/// the middle, the capper and its cap chute at +X.
fn filler_capper(doc: &mut Document, p: &Paint, m: Option<ObjectId>) {
    let (l, w, h) = (6.0, 4.4, 2.6);
    block(doc, "base", [l, w, 0.9], [0.0, 0.0, 0.45], p.steel, m);
    post(doc, "rinser", 0.7, 0.4, [-2.0, 0.0, 1.1], p.steel, m);
    post(
        doc,
        "filler carousel",
        1.4,
        0.3,
        [0.0, 0.0, 1.05],
        p.steel,
        m,
    );
    post(doc, "filler bowl", 1.1, 0.5, [0.0, 0.0, 2.0], p.steel, m);
    post(doc, "bowl column", 0.15, 0.8, [0.0, 0.0, 1.5], p.steel, m);
    post(doc, "capper", 0.6, 0.6, [2.1, 0.0, 1.3], p.steel, m);
    // The caps: a hopper on top, a chute down to the capper.
    block(
        doc,
        "cap hopper",
        [0.8, 0.8, 0.8],
        [2.1, 1.5, 3.4],
        p.blue,
        m,
    );
    bar(
        doc,
        "cap chute",
        0.06,
        1.4,
        [2.1, 1.0, 2.6],
        false,
        p.steel,
        m,
    );
    // The guard's frame: corner posts and top beams; the glass between
    // is left out, to see the wheels.
    for (x, y) in [
        (-l / 2.0, -w / 2.0),
        (l / 2.0, -w / 2.0),
        (-l / 2.0, w / 2.0),
        (l / 2.0, w / 2.0),
    ] {
        block(
            doc,
            "guard post",
            [0.08, 0.08, h],
            [x, y, 0.9 + h / 2.0],
            p.steel,
            m,
        );
    }
    for y in [-w / 2.0, w / 2.0] {
        block(
            doc,
            "guard beam",
            [l, 0.08, 0.12],
            [0.0, y, 0.9 + h],
            p.steel,
            m,
        );
    }
    for x in [-l / 2.0, l / 2.0] {
        block(
            doc,
            "guard beam",
            [0.08, w, 0.12],
            [x, 0.0, 0.9 + h],
            p.steel,
            m,
        );
    }
    block(
        doc,
        "control panel",
        [0.9, 0.4, 1.8],
        [-l / 2.0 - 0.6, -1.6, 0.9],
        p.white,
        m,
    );
    tower(doc, p, [l / 2.0 - 0.2, -w / 2.0 + 0.2, 0.9 + h + 0.04], m);
}

/// A fill-level and cap inspector over the conveyor, 1.4 m along: an
/// arch with its camera and X-ray heads, a screen, and the reject bin
/// beside it.
fn bottle_inspector(doc: &mut Document, p: &Paint, m: Option<ObjectId>) {
    block(
        doc,
        "leg left",
        [1.4, 0.15, 1.9],
        [0.0, -0.45, 0.95],
        p.white,
        m,
    );
    block(
        doc,
        "leg right",
        [1.4, 0.15, 1.9],
        [0.0, 0.45, 0.95],
        p.white,
        m,
    );
    block(doc, "head", [1.4, 1.05, 0.5], [0.0, 0.0, 1.65], p.white, m);
    block(doc, "camera", [0.3, 0.3, 0.2], [0.0, 0.0, 1.32], p.dark, m);
    block(
        doc,
        "screen",
        [0.5, 0.05, 0.35],
        [0.0, -0.55, 1.5],
        p.dark,
        m,
    );
    block(
        doc,
        "reject bin",
        [0.6, 0.5, 0.7],
        [0.2, -0.9, 0.35],
        p.red,
        m,
    );
    tower(doc, p, [0.6, 0.0, 1.9], m);
}

/// A rotary labeller, 3 × 3 m: its turntable carries bottles past the
/// label stations, each a reel of labels and a glue drum.
fn labeler(doc: &mut Document, p: &Paint, m: Option<ObjectId>) {
    block(doc, "base", [3.0, 3.0, 0.9], [0.0, 0.0, 0.45], p.white, m);
    post(doc, "turntable", 1.0, 0.25, [0.0, 0.0, 1.03], p.steel, m);
    post(doc, "top", 0.9, 0.12, [0.0, 0.0, 1.7], p.steel, m);
    post(doc, "column", 0.12, 0.6, [0.0, 0.0, 1.4], p.steel, m);
    for (k, (x, y)) in [(1.15, -0.9), (-0.2, -1.25)].into_iter().enumerate() {
        let name = format!("label station {}", k + 1);
        block(doc, &name, [0.6, 0.5, 0.9], [x, y, 1.35], p.blue, m);
        post(doc, "label reel", 0.3, 0.12, [x, y, 1.86], p.film, m);
    }
    block(
        doc,
        "control panel",
        [0.7, 0.35, 1.7],
        [-1.9, -1.0, 0.85],
        p.white,
        m,
    );
    tower(doc, p, [1.3, 1.3, 0.9], m);
}

/// A shrink wrapper, `length` m: the infeed that groups bottles, the film
/// unit, and the heat tunnel the film shrinks in.
fn shrink_wrapper(doc: &mut Document, p: &Paint, length: f64, m: Option<ObjectId>) {
    let l = length.max(5.0);
    block(doc, "frame", [l, 1.6, 0.9], [0.0, 0.0, 0.45], p.white, m);
    block(
        doc,
        "infeed guard",
        [l * 0.35, 1.6, 1.0],
        [-l * 0.325, 0.0, 1.4],
        p.glass,
        m,
    );
    block(
        doc,
        "film unit",
        [1.2, 1.9, 1.6],
        [-l * 0.05, 0.0, 1.7],
        p.blue,
        m,
    );
    post(
        doc,
        "film roll",
        0.3,
        1.0,
        [-l * 0.05, -1.2, 1.0],
        p.film,
        m,
    );
    block(
        doc,
        "heat tunnel",
        [l * 0.4, 1.7, 1.2],
        [l * 0.28, 0.0, 1.5],
        p.steel,
        m,
    );
    block(
        doc,
        "tunnel mouth",
        [0.05, 1.2, 0.5],
        [l * 0.08 - 0.02, 0.0, 1.2],
        p.red,
        m,
    );
    tower(doc, p, [l * 0.45, 0.7, 2.1], m);
}

/// A palletiser cell, 5 × 5 m: a gantry over the pallet stand, the layer
/// table it takes packs from, and a guard around.
fn palletizer(doc: &mut Document, p: &Paint, m: Option<ObjectId>) {
    let (s, h) = (5.0, 3.6);
    for (x, y) in [
        (-s / 2.0, -s / 2.0),
        (s / 2.0, -s / 2.0),
        (-s / 2.0, s / 2.0),
        (s / 2.0, s / 2.0),
    ] {
        block(doc, "column", [0.25, 0.25, h], [x, y, h / 2.0], p.yellow, m);
    }
    block(
        doc,
        "beam front",
        [s, 0.25, 0.3],
        [0.0, -s / 2.0, h],
        p.yellow,
        m,
    );
    block(
        doc,
        "beam back",
        [s, 0.25, 0.3],
        [0.0, s / 2.0, h],
        p.yellow,
        m,
    );
    block(
        doc,
        "bridge",
        [0.35, s, 0.3],
        [0.4, 0.0, h - 0.3],
        p.blue,
        m,
    );
    block(
        doc,
        "head",
        [0.6, 0.8, 1.2],
        [0.4, 0.3, h - 1.05],
        p.dark,
        m,
    );
    block(
        doc,
        "layer table",
        [1.6, 1.4, 0.9],
        [-1.4, 0.0, 0.45],
        p.steel,
        m,
    );
    block(
        doc,
        "pallet stand",
        [1.4, 1.2, 0.15],
        [1.2, 0.0, 0.075],
        p.belt,
        m,
    );
    // A guard on three sides, low, of mesh.
    block(
        doc,
        "guard back",
        [s, 0.04, 2.0],
        [0.0, s / 2.0 + 0.2, 1.0],
        p.glass,
        m,
    );
    block(
        doc,
        "guard side",
        [0.04, s, 2.0],
        [s / 2.0 + 0.2, 0.0, 1.0],
        p.glass,
        m,
    );
    tower(doc, p, [-s / 2.0, -s / 2.0, h + 0.15], m);
}

/// A stretch wrapper, 2.6 m: a turntable a pallet turns on, and the mast
/// the film carriage rides up and down.
fn stretch_wrapper(doc: &mut Document, p: &Paint, m: Option<ObjectId>) {
    post(doc, "turntable", 1.0, 0.12, [0.0, 0.0, 0.06], p.steel, m);
    block(doc, "mast", [0.3, 0.3, 3.0], [0.0, 1.35, 1.5], p.blue, m);
    block(doc, "carriage", [0.4, 0.4, 0.6], [0.0, 1.1, 1.4], p.dark, m);
    post(doc, "film roll", 0.12, 0.5, [0.0, 0.92, 1.4], p.film, m);
    block(
        doc,
        "control panel",
        [0.5, 0.3, 1.4],
        [0.9, 1.4, 0.7],
        p.white,
        m,
    );
}

/// A stainless process tank, `diameter` × `height` m on four legs: its
/// shell, its domed top and its manway, a ladder up its side.
fn process_tank(doc: &mut Document, p: &Paint, diameter: f64, height: f64, m: Option<ObjectId>) {
    let (r, h) = (diameter.max(0.5) / 2.0, height.max(1.0));
    let legs = 0.6;
    for (x, y) in [
        (r * 0.7, r * 0.7),
        (-r * 0.7, r * 0.7),
        (r * 0.7, -r * 0.7),
        (-r * 0.7, -r * 0.7),
    ] {
        post(doc, "leg", 0.05, legs, [x, y, legs / 2.0], p.steel, m);
    }
    post(doc, "shell", r, h, [0.0, 0.0, legs + h / 2.0], p.steel, m);
    post(
        doc,
        "top",
        r * 0.8,
        0.15,
        [0.0, 0.0, legs + h + 0.075],
        p.steel,
        m,
    );
    post(
        doc,
        "manway",
        0.25,
        0.15,
        [0.0, 0.0, legs + h + 0.22],
        p.steel,
        m,
    );
    post(doc, "outlet", 0.08, 0.3, [0.0, 0.0, legs - 0.1], p.steel, m);
    // A ladder up its −Y side.
    for x in [-0.2, 0.2] {
        block(
            doc,
            "ladder rail",
            [0.04, 0.04, h + legs],
            [x, -r - 0.15, (h + legs) / 2.0],
            p.yellow,
            m,
        );
    }
}

/// A table-top conveyor bottles run on, `length` m along X, its top
/// `height` m up: a narrow chain on legs, guide rails either side.
fn bottle_conveyor(doc: &mut Document, p: &Paint, length: f64, height: f64, m: Option<ObjectId>) {
    let (l, h) = (length.max(0.5), height.max(0.3));
    block(
        doc,
        "chain",
        [l, 0.3, 0.04],
        [0.0, 0.0, h - 0.02],
        p.belt,
        m,
    );
    block(
        doc,
        "frame",
        [l, 0.34, 0.12],
        [0.0, 0.0, h - 0.1],
        p.steel,
        m,
    );
    for y in [-0.2, 0.2] {
        bar(doc, "guide", 0.012, l, [0.0, y, h + 0.12], true, p.steel, m);
    }
    let legs = (l / 1.5).ceil() as usize + 1;
    for k in 0..legs {
        let x = -l / 2.0 + 0.1 + (l - 0.2) * k as f64 / (legs - 1) as f64;
        block(
            doc,
            "leg",
            [0.05, 0.3, h - 0.16],
            [x, 0.0, (h - 0.16) / 2.0],
            p.steel,
            m,
        );
    }
}

/// What goes down the line at `stage` (see [`STAGES`]), as meshes to draw a
/// flow part with: a tray of 4 × 6 — preforms, empty bottles, filled and
/// capped, labelled —, a shrink-wrapped pack of them, or a wrapped pallet
/// of packs; in its own frame, its middle on the floor, facing +X.
pub(crate) fn beverage_look(stage: usize) -> LookMeshes {
    let mut doc = Document::new();
    let tray = material(&mut doc, "tray", [0.15, 0.35, 0.70], 0.0);
    let pet = material(&mut doc, "pet", [0.70, 0.85, 0.92], 0.2);
    let drink = material(&mut doc, "drink", [0.98, 0.55, 0.10], 0.1);
    let cap = material(&mut doc, "cap", [0.92, 0.92, 0.90], 0.0);
    let label = material(&mut doc, "label", [0.10, 0.50, 0.25], 0.0);
    let wood = material(&mut doc, "pallet wood", [0.70, 0.55, 0.35], 0.0);
    let film = material(&mut doc, "stretch film", [0.88, 0.90, 0.92], 0.2);
    let pitch = 0.066;
    let bottles = |f: &mut dyn FnMut(f64, f64)| {
        for i in 0..6 {
            for j in 0..4 {
                f((i as f64 - 2.5) * pitch, (j as f64 - 1.5) * pitch);
            }
        }
    };
    match stage.min(STAGES.len() - 1) {
        // A tray of preforms: small test tubes of PET.
        0 => {
            block(
                &mut doc,
                "tray",
                [0.42, 0.29, 0.04],
                [0.0, 0.0, 0.02],
                tray,
                None,
            );
            bottles(&mut |x, y| {
                post(&mut doc, "preform", 0.013, 0.1, [x, y, 0.09], pet, None);
            });
        }
        // A tray of bottles: empty, filled and capped, or labelled too.
        1..=3 => {
            block(
                &mut doc,
                "tray",
                [0.42, 0.29, 0.04],
                [0.0, 0.0, 0.02],
                tray,
                None,
            );
            let body = if stage == 1 { pet } else { drink };
            bottles(&mut |x, y| {
                post(&mut doc, "bottle", 0.03, 0.2, [x, y, 0.14], body, None);
                post(&mut doc, "neck", 0.014, 0.04, [x, y, 0.26], pet, None);
                if stage >= 2 {
                    post(&mut doc, "cap", 0.016, 0.018, [x, y, 0.289], cap, None);
                }
                if stage >= 3 {
                    post(&mut doc, "label", 0.031, 0.07, [x, y, 0.15], label, None);
                }
            });
        }
        // A pack: the labelled bottles under shrink film.
        4 => {
            block(
                &mut doc,
                "pack",
                [0.40, 0.27, 0.22],
                [0.0, 0.0, 0.13],
                drink,
                None,
            );
            block(
                &mut doc,
                "label band",
                [0.405, 0.275, 0.07],
                [0.0, 0.0, 0.13],
                label,
                None,
            );
            block(
                &mut doc,
                "film top",
                [0.40, 0.27, 0.06],
                [0.0, 0.0, 0.27],
                film,
                None,
            );
        }
        // A pallet: five layers of packs, wrapped.
        _ => {
            block(
                &mut doc,
                "pallet",
                [1.2, 1.0, 0.15],
                [0.0, 0.0, 0.075],
                wood,
                None,
            );
            block(
                &mut doc,
                "load",
                [1.18, 0.98, 1.3],
                [0.0, 0.0, 0.8],
                drink,
                None,
            );
            for k in 0..5 {
                let z = 0.28 + 0.26 * k as f64;
                block(
                    &mut doc,
                    "label band",
                    [1.19, 0.99, 0.07],
                    [0.0, 0.0, z],
                    label,
                    None,
                );
            }
            block(
                &mut doc,
                "film",
                [1.2, 1.0, 0.1],
                [0.0, 0.0, 1.5],
                film,
                None,
            );
        }
    }
    super::look_of(&doc)
}
