//! What the demos are built with: a plant laid out item by item, its
//! roles asked of the cell as the page asks them, its scenarios as changes
//! to its cell, and a check that no two pieces of its equipment run into
//! each other.

use kreate_scene::Document;
use serde_json::{json, Value};

use crate::cell::Cell;
use crate::library::Library;

/// The plant being laid out: the library, and each item placed by its name.
pub(super) struct Plant {
    pub(super) library: Library,
    pub(super) items: Vec<(String, u64)>,
}

impl Plant {
    /// Places `template` named `name` at `at`, turned `yaw`, with `params`,
    /// costing `price` (만원).
    pub(super) fn place(
        &mut self,
        template: &str,
        name: &str,
        at: [f64; 3],
        yaw: f64,
        params: Value,
        price: f64,
    ) {
        let (reply, _) = self.library.handle(
            &json!({"op": "place", "template": template, "at": at, "yaw": yaw, "params": params})
                .to_string(),
        );
        let reply: Value = serde_json::from_str(&reply).unwrap();
        assert_eq!(reply["ok"], true, "{template}: {reply}");
        let id = reply["item"].as_u64().unwrap();
        let (reply, _) = self
            .library
            .handle(&json!({"op": "rename", "item": id, "name": name}).to_string());
        assert!(reply.contains("\"ok\":true"), "{name}: {reply}");
        if price > 0.0 {
            self.library
                .handle(&json!({"op": "price", "item": id, "price": price}).to_string());
        }
        self.items.push((name.to_string(), id));
    }
}

pub(super) fn ask(cell: &mut Cell, doc: &Document, req: Value) {
    let a: Value = serde_json::from_str(&cell.handle(doc, &req.to_string())).unwrap();
    assert_eq!(a["ok"], true, "{req}: {a}");
}

/// Each flow piece named first stands for the equipment named second:
/// picking the equipment shows the piece's role.
pub(super) fn hosts(plant: &Plant, cell: &mut Cell, doc: &Document, roles: &[(u64, String, crate::cell::Kind)], pairs: &[(&str, &str)]) {
    for (piece, equipment) in pairs {
        let id = plant
            .items
            .iter()
            .find(|(n, _)| n == equipment)
            .unwrap_or_else(|| panic!("no item {equipment}"))
            .1;
        let host = crate::library::EQUIPMENT + id * crate::library::PER_ITEM;
        ask(
            cell,
            doc,
            json!({"op": "flow_host", "object": object(roles, piece), "host": host.to_string()}),
        );
    }
}

/// A role's object, by its item's name.
pub(super) fn object(roles: &[(u64, String, crate::cell::Kind)], name: &str) -> String {
    roles
        .iter()
        .find(|r| r.1 == name)
        .unwrap_or_else(|| panic!("no role {name}"))
        .0
        .to_string()
}

/// The cell `cell` with the props of role `name` changed by `props`.
pub(super) fn changed(cell: &[Value], changes: &[(&str, Value)]) -> Vec<Value> {
    let mut cell = cell.to_vec();
    for (name, props) in changes {
        let r = cell
            .iter_mut()
            .find(|r| r["name"] == *name)
            .unwrap_or_else(|| panic!("no role {name}"));
        for (k, v) in props.as_object().unwrap() {
            r["props"][k] = v.clone();
        }
    }
    cell
}

/// Each piece's box in the world, of an item as built.
pub(super) type Boxes = Vec<([f64; 3], [f64; 3])>;

/// Each item's pieces' boxes, with its name and template.
pub(super) fn boxes(plant: &mut Plant) -> Vec<(String, String, Boxes)> {
    let mut doc = Document::new();
    plant.library.apply(&mut doc);
    let items: Vec<(String, String, u64)> = plant
        .library
        .items()
        .iter()
        .map(|i| (i.name.clone(), i.template.clone(), i.root_uid()))
        .collect();
    items
        .into_iter()
        .filter_map(|(name, template, root)| {
            let root = doc.object_by_uid(kreate_scene::Uid::from_raw(root))?;
            let mut pieces = Vec::new();
            for (o, x) in doc.objects() {
                if o != root && !crate::cell::is_below(&doc, o, root) {
                    continue;
                }
                let Some(mesh) = x.mesh().and_then(|m| doc.mesh(m)) else {
                    continue;
                };
                let world = doc.world_transform(o);
                let mut lo = [f64::INFINITY; 3];
                let mut hi = [f64::NEG_INFINITY; 3];
                for p in mesh.positions() {
                    let w = world.transform_point3(p.as_dvec3());
                    for k in 0..3 {
                        lo[k] = lo[k].min(w[k]);
                        hi[k] = hi[k].max(w[k]);
                    }
                }
                if lo[0] <= hi[0] {
                    pieces.push((lo, hi));
                }
            }
            Some((name, template, pieces))
        })
        .collect()
}

/// The pairs of the plant's equipment that run into each other: a piece of
/// one and a piece of the other overlapping by more than 2 cm every way.
/// Floor markings lie under everything; a flow's station, buffer, source
/// or sink stands where its machine does, carrying its work; the
/// building's walls, columns, doors and lights meet each other — none of
/// these counts.
pub(super) fn clashes(plant: &mut Plant) -> Vec<String> {
    let skip = |t: &str| {
        matches!(
            t,
            "floor_zone"
                | "flow_station"
                | "flow_buffer"
                | "flow_source"
                | "flow_sink"
                | "amr_stop"
        )
    };
    let building = |t: &str| {
        matches!(
            t,
            "factory_wall" | "factory_column" | "roll_door" | "ceiling_light"
        )
    };
    let meet = |a: &([f64; 3], [f64; 3]), b: &([f64; 3], [f64; 3])| {
        (0..3).all(|k| a.1[k].min(b.1[k]) - a.0[k].max(b.0[k]) > 0.02)
    };
    let all = boxes(plant);
    let mut out = Vec::new();
    for (i, a) in all.iter().enumerate() {
        for b in &all[i + 1..] {
            if skip(&a.1) || skip(&b.1) || (building(&a.1) && building(&b.1)) {
                continue;
            }
            if a.2.iter().any(|x| b.2.iter().any(|y| meet(x, y))) {
                out.push(format!("{} ↔ {}", a.0, b.0));
            }
        }
    }
    out
}
