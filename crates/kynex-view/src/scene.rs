//! A scene as the viewer draws it.
//!
//! Every object is sent with its world matrix, so the browser keeps a flat
//! list and never walks a hierarchy: a joint turned in Kreate arrives as new
//! matrices for everything it carries. Meshes go separately, by identity, and
//! only when one changes — an arm being posed sends sixteen numbers per part,
//! not its triangles.
//!
//! Positions are in metres with Z up, as Kreate keeps them; the viewer sets
//! its camera up that way rather than turning the scene.

use std::collections::{BTreeMap, BTreeSet};

use kreate_math::DMat4;
use kreate_mesh::MeshData;
use kreate_scene::robot::RobotPart;
use kreate_scene::{Changes, DatablockKind, Document, MeshId, ObjectData};
use kynex_model::Model;
use serde::Serialize;
use serde_json::{json, Value};

use crate::base64;

/// One object, as drawn.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Drawn {
    pub uid: u64,
    pub name: String,
    pub parent: Option<u64>,
    /// empty, mesh, camera, light or armature.
    pub kind: &'static str,
    /// The mesh it shows, by identity.
    pub mesh: Option<u64>,
    /// World matrix, column major: what three.js's `Matrix4.fromArray` reads.
    pub matrix: [f64; 16],
    /// Its first material's base colour, linear; none for the default surface.
    pub color: Option<[f32; 3]>,
    pub visible: bool,
    /// link, joint, collision or rig.
    pub role: Option<&'static str>,
}

/// What the viewers have been sent, so the next edit can be sent as a
/// difference.
#[derive(Debug, Default)]
pub struct ViewState {
    objects: BTreeMap<u64, Drawn>,
    /// Each mesh's JSON, by identity: kept so a viewer that opens later gets
    /// the scene without anything being encoded again.
    meshes: BTreeMap<u64, String>,
    robots: Value,
}

impl ViewState {
    pub fn new() -> Self {
        Self {
            robots: Value::Array(Vec::new()),
            ..Default::default()
        }
    }

    /// Takes in a whole scene and forgets the old one.
    pub fn reset(&mut self, doc: &Document, model: &Model) {
        self.objects = drawn(doc);
        self.meshes.clear();
        let index = mesh_index(doc);
        for uid in referenced(&self.objects) {
            if let Some(mesh) = index.get(&uid).and_then(|id| doc.mesh(*id)) {
                self.meshes.insert(uid, mesh_json(mesh));
            }
        }
        self.robots = crate::json::robots(model);
    }

    /// Everything, for a viewer that has just opened.
    pub fn scene(&self) -> String {
        let mut out = String::from("{\"meshes\":{");
        for (i, (uid, mesh)) in self.meshes.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&format!("\"{uid}\":{mesh}"));
        }
        out.push_str("},\"objects\":");
        out.push_str(&serde_json::to_string(&self.objects.values().collect::<Vec<_>>()).unwrap());
        out.push_str(",\"robots\":");
        out.push_str(&self.robots.to_string());
        out.push('}');
        out
    }

    /// Takes in an edit and returns what the viewers need to show it, or
    /// `None` when nothing they draw changed. `changes` names the meshes
    /// whose geometry was edited; without it, only new meshes are sent.
    pub fn update(
        &mut self,
        doc: &Document,
        model: &Model,
        changes: Option<&Changes>,
    ) -> Option<String> {
        let now = drawn(doc);
        let changed: Vec<&Drawn> = now
            .iter()
            .filter(|(uid, d)| self.objects.get(uid) != Some(*d))
            .map(|(_, d)| d)
            .collect();
        let removed: Vec<u64> = self
            .objects
            .keys()
            .filter(|uid| !now.contains_key(uid))
            .copied()
            .collect();

        let edited: BTreeSet<u64> = changes
            .map(|c| {
                c.changed
                    .iter()
                    .filter(|(kind, _)| *kind == DatablockKind::Mesh)
                    .map(|(_, uid)| uid.raw())
                    .collect()
            })
            .unwrap_or_default();
        let wanted = referenced(&now);
        let mut meshes = BTreeMap::new();
        let mut index = None;
        for uid in &wanted {
            if edited.contains(uid) || !self.meshes.contains_key(uid) {
                let index = index.get_or_insert_with(|| mesh_index(doc));
                if let Some(mesh) = index.get(uid).and_then(|id| doc.mesh(*id)) {
                    let encoded = mesh_json(mesh);
                    meshes.insert(*uid, encoded.clone());
                    self.meshes.insert(*uid, encoded);
                }
            }
        }
        self.meshes.retain(|uid, _| wanted.contains(uid));

        let robots = crate::json::robots(model);
        let robots_changed = robots != self.robots;
        if changed.is_empty() && removed.is_empty() && meshes.is_empty() && !robots_changed {
            return None;
        }

        let mut out = String::from("{\"meshes\":{");
        for (i, (uid, mesh)) in meshes.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&format!("\"{uid}\":{mesh}"));
        }
        out.push_str("},\"objects\":");
        out.push_str(&serde_json::to_string(&changed).unwrap());
        out.push_str(",\"removed\":");
        out.push_str(&serde_json::to_string(&removed).unwrap());
        if robots_changed {
            out.push_str(",\"robots\":");
            out.push_str(&robots.to_string());
        }
        out.push('}');

        self.objects = now;
        self.robots = robots;
        Some(out)
    }
}

fn drawn(doc: &Document) -> BTreeMap<u64, Drawn> {
    let mut out = BTreeMap::new();
    for (id, object) in doc.objects() {
        let Some(uid) = doc.object_uid(id).map(|u| u.raw()) else {
            continue;
        };
        let (kind, mesh) = match object.data {
            ObjectData::Empty => ("empty", None),
            ObjectData::Mesh(m) => ("mesh", doc.mesh_uid(m).map(|u| u.raw())),
            ObjectData::Camera(_) => ("camera", None),
            ObjectData::Light(_) => ("light", None),
            ObjectData::Armature(_) => ("armature", None),
        };
        let color = doc.material_at(id, 0).map(|m| {
            let c = m.base_color;
            [c.r, c.g, c.b]
        });
        let role = object.robot.as_ref().map(|part| match part {
            RobotPart::Link { .. } => "link",
            RobotPart::Joint(_) => "joint",
            RobotPart::Collision => "collision",
            RobotPart::Rig { .. } => "rig",
        });
        out.insert(
            uid,
            Drawn {
                uid,
                name: object.name.clone(),
                parent: object
                    .parent
                    .and_then(|p| doc.object_uid(p))
                    .map(|u| u.raw()),
                kind,
                mesh,
                matrix: DMat4::from(doc.world_transform(id)).to_cols_array(),
                color,
                visible: object.visible,
                role,
            },
        );
    }
    out
}

fn referenced(objects: &BTreeMap<u64, Drawn>) -> BTreeSet<u64> {
    objects.values().filter_map(|d| d.mesh).collect()
}

/// Every mesh by identity, made once: `Document::mesh_by_uid` looks through
/// all the meshes, which for each of a factory's meshes is a factory's worth
/// of looking.
fn mesh_index(doc: &Document) -> BTreeMap<u64, MeshId> {
    doc.meshes()
        .filter_map(|(id, _)| doc.mesh_uid(id).map(|u| (u.raw(), id)))
        .collect()
}

/// A mesh's triangles: positions as little-endian f32, indices as u32, both
/// base64.
fn mesh_json(mesh: &MeshData) -> String {
    let positions: Vec<u8> = mesh
        .positions()
        .iter()
        .flat_map(|p| [p.x, p.y, p.z])
        .flat_map(f32::to_le_bytes)
        .collect();
    let indices: Vec<u8> = mesh
        .triangulate()
        .indices
        .iter()
        .flat_map(|i| i.to_le_bytes())
        .collect();
    json!({
        "positions": base64::encode(&positions),
        "indices": base64::encode(&indices),
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kreate_scene::Uid;

    fn parse(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn a_scene_is_sent_whole_then_by_the_edit() {
        let mut doc = Document::new();
        let cube = doc.add_mesh_object("Cube", kreate_mesh::primitives::cube(2.0));
        let mut view = ViewState::new();
        view.reset(&doc, &Model::from_document(&doc));
        let scene = parse(&view.scene());
        assert_eq!(scene["objects"].as_array().unwrap().len(), 1);
        assert_eq!(scene["meshes"].as_object().unwrap().len(), 1);

        // Moving it sends its matrix and not its mesh again.
        doc.object_mut(cube).unwrap().transform.translation.x = 3.0;
        let update = parse(
            &view
                .update(&doc, &Model::from_document(&doc), Some(&Changes::default()))
                .unwrap(),
        );
        assert_eq!(update["meshes"].as_object().unwrap().len(), 0);
        assert_eq!(update["objects"][0]["matrix"][12], 3.0);

        // Editing its geometry sends the mesh.
        let mesh_uid = doc
            .mesh_uid(doc.object(cube).unwrap().mesh().unwrap())
            .unwrap();
        let mut changes = Changes::default();
        changes.changed.insert((DatablockKind::Mesh, mesh_uid));
        let update = parse(
            &view
                .update(&doc, &Model::from_document(&doc), Some(&changes))
                .unwrap(),
        );
        assert_eq!(update["meshes"].as_object().unwrap().len(), 1);

        // Nothing changed is nothing to send.
        assert!(view
            .update(&doc, &Model::from_document(&doc), None)
            .is_none());

        // Deleting it says so, and its mesh is let go.
        let uid = doc.object_uid(cube).unwrap();
        doc.remove_object(cube);
        let update = parse(
            &view
                .update(&doc, &Model::from_document(&doc), None)
                .unwrap(),
        );
        assert_eq!(update["removed"][0], uid.raw());
        assert_eq!(parse(&view.scene())["meshes"].as_object().unwrap().len(), 0);
        let _ = Uid::NONE;
    }

    #[test]
    fn a_cube_is_twelve_triangles_of_eight_points() {
        let json = parse(&mesh_json(&kreate_mesh::primitives::cube(1.0)));
        // 8 points × 12 bytes, 36 indices × 4 bytes, as base64.
        assert_eq!(
            json["positions"].as_str().unwrap().len(),
            96usize.div_ceil(3) * 4
        );
        assert_eq!(
            json["indices"].as_str().unwrap().len(),
            144usize.div_ceil(3) * 4
        );
    }
}
