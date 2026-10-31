//! A block line's own pieces: the mock-up machine a block is worked in —
//! a cabinet that takes a block in, keeps it a while and gives it back,
//! standing for whatever the real machine would do —, the vision inspector
//! that looks at two put together, and what goes down
//! the line as a flow part looks: a block, a 6 cm cube coloured as its
//! product, and two of them put together side by side.
//!
//! Each stands on the floor at its middle, its front, where a block goes
//! in, towards +X. Neither has a role in the cell: its work is the process
//! flow's station's, shown in it (`flow`: half its height up, at most
//! 0.9 m — the inspector's table top).

use kreate_math::SceneRgb;
use kreate_scene::{Document, Material, MaterialId, Object, ObjectData, ObjectId};

use super::{block, post, Item, LookMeshes};

/// The templates this module makes.
pub const KEYS: &[&str] = &["mock_machine", "vision_inspector"];

/// The colours a mock-up machine's band comes in, the block it works on.
pub const BANDS: &[&str] = &["노랑", "빨강", "파랑", "회색"];

/// How big a block is, metres: a cube.
pub const BLOCK_SIZE: f64 = 0.06;

fn material(doc: &mut Document, name: &str, rgb: [f32; 3], metallic: f32) -> MaterialId {
    doc.add_material(Material {
        name: name.into(),
        base_color: SceneRgb::new(rgb[0], rgb[1], rgb[2]),
        metallic,
        ..Material::default()
    })
}

/// A block of product `product`'s colour: A yellow, B blue, E red, as
/// toy blocks are, bright; the others each product's own
/// (`flow::PRODUCT_COLOURS`).
pub(crate) fn block_colour(product: usize) -> [f32; 3] {
    match product {
        0 => [0.97, 0.80, 0.12],
        1 => [0.12, 0.40, 0.92],
        4 => [0.88, 0.16, 0.12],
        p => crate::cell::flow::PRODUCT_COLOURS[p % crate::cell::flow::PRODUCT_COLOURS.len()],
    }
}

/// A block of product `a`, or, `b` given, two side by side — `a`'s on the
/// left (+Y), `b`'s on the right —, put together: each a cube with two
/// studs on its top.
pub(crate) fn block_look(a: usize, b: Option<usize>) -> LookMeshes {
    let mut doc = Document::new();
    let s = BLOCK_SIZE;
    let cubes: Vec<(usize, f64)> = match b {
        None => vec![(a, 0.0)],
        Some(b) => vec![(a, s / 2.0), (b, -s / 2.0)],
    };
    for (product, y) in cubes {
        let paint = material(&mut doc, "block", block_colour(product), 0.0);
        // A hair short of touching, so that two read as two.
        block(&mut doc, "block", [s, s - 0.002, s], [0.0, y, s / 2.0], paint, None);
        for x in [-s / 4.0, s / 4.0] {
            post(&mut doc, "stud", s * 0.17, s * 0.15, [x, y, s * 1.075], paint, None);
        }
    }
    super::look_of(&doc)
}

/// Builds `item` into `doc`, if it is one of [`KEYS`].
pub(super) fn build(doc: &mut Document, item: &Item) {
    let root = doc.add_object(Object::new(item.template.clone(), ObjectData::Empty));
    let m = Some(root);
    match item.template.as_str() {
        "mock_machine" => mock_machine(doc, item.param("band").round() as usize, m),
        "vision_inspector" => vision_inspector(doc, m),
        _ => {}
    }
}

/// A vision inspector, 0.9 × 0.8 m and 1.6 m high: a table, its top 0.8 m
/// up, under an arch carrying a camera looking down through a ring light,
/// and a screen showing what it sees.
fn vision_inspector(doc: &mut Document, m: Option<ObjectId>) {
    let panel = material(doc, "machine panel", [0.86, 0.88, 0.90], 0.1);
    let dark = material(doc, "dark grey", [0.20, 0.22, 0.25], 0.2);
    let steel = material(doc, "stainless", [0.76, 0.78, 0.80], 0.85);
    let light = material(doc, "ring light", [0.92, 0.96, 1.0], 0.0);
    let screen = material(doc, "screen", [0.10, 0.45, 0.30], 0.0);
    block(doc, "table", [0.9, 0.8, 0.77], [0.0, 0.0, 0.385], panel, m);
    block(doc, "table top", [0.9, 0.8, 0.03], [0.0, 0.0, 0.785], steel, m);
    for y in [-0.36, 0.36] {
        block(doc, "arch post", [0.06, 0.06, 0.8], [-0.2, y, 1.2], dark, m);
    }
    block(doc, "arch beam", [0.08, 0.78, 0.08], [-0.2, 0.0, 1.56], dark, m);
    block(doc, "camera arm", [0.24, 0.06, 0.05], [-0.08, 0.0, 1.5], dark, m);
    block(doc, "camera", [0.08, 0.08, 0.12], [0.0, 0.0, 1.42], dark, m);
    post(doc, "ring light", 0.07, 0.02, [0.0, 0.0, 1.35], light, m);
    block(doc, "monitor", [0.03, 0.34, 0.22], [-0.2, 0.0, 1.3], dark, m);
    block(doc, "monitor screen", [0.005, 0.3, 0.18], [-0.18, 0.0, 1.3], screen, m);
}

/// A mock-up machine, 1.4 × 1.1 × 1.7 m: a cabinet on a plinth, a band of
/// its colour round it, a window onto its chamber, the hatch a block goes
/// in at on its front (+X) at 0.9 m, a control panel beside it and a signal
/// tower on its roof.
fn mock_machine(doc: &mut Document, band: usize, m: Option<ObjectId>) {
    let rgb = match band.min(BANDS.len() - 1) {
        0 => block_colour(0),
        1 => block_colour(4),
        2 => block_colour(1),
        _ => [0.55, 0.58, 0.62],
    };
    let panel = material(doc, "machine panel", [0.86, 0.88, 0.90], 0.1);
    let dark = material(doc, "dark grey", [0.20, 0.22, 0.25], 0.2);
    let colour = material(doc, "machine band", rgb, 0.0);
    let glass = material(doc, "window", [0.45, 0.62, 0.72], 0.3);
    let green = material(doc, "lamp green", [0.15, 0.75, 0.30], 0.0);
    let amber = material(doc, "lamp amber", [0.98, 0.65, 0.10], 0.0);
    let red = material(doc, "lamp red", [0.85, 0.20, 0.15], 0.0);
    let (l, w, h) = (1.1, 1.4, 1.7);
    block(doc, "plinth", [l, w, 0.1], [0.0, 0.0, 0.05], dark, m);
    block(doc, "cabinet", [l, w, h - 0.1], [0.0, 0.0, 0.1 + (h - 0.1) / 2.0], panel, m);
    block(doc, "band", [l + 0.01, w + 0.01, 0.12], [0.0, 0.0, h - 0.2], colour, m);
    // The front: the window over the hatch, the panel beside them.
    let front = l / 2.0;
    block(doc, "window", [0.02, 0.7, 0.4], [front + 0.005, -0.15, 1.3], glass, m);
    block(doc, "hatch frame", [0.03, 0.4, 0.3], [front + 0.01, -0.15, 0.9], dark, m);
    block(doc, "hatch", [0.02, 0.32, 0.22], [front + 0.02, -0.15, 0.9], colour, m);
    block(doc, "ledge", [0.2, 0.4, 0.03], [front + 0.1, -0.15, 0.735], dark, m);
    block(doc, "control panel", [0.06, 0.3, 0.4], [front + 0.03, 0.47, 1.2], dark, m);
    block(doc, "screen", [0.01, 0.22, 0.14], [front + 0.065, 0.47, 1.28], glass, m);
    // The signal tower, at the back corner of the roof.
    let (x, y, z) = (-l / 2.0 + 0.15, w / 2.0 - 0.15, h);
    post(doc, "tower post", 0.02, 0.2, [x, y, z + 0.1], dark, m);
    post(doc, "lamp green", 0.04, 0.07, [x, y, z + 0.235], green, m);
    post(doc, "lamp amber", 0.04, 0.07, [x, y, z + 0.305], amber, m);
    post(doc, "lamp red", 0.04, 0.07, [x, y, z + 0.375], red, m);
}
