//! A scene as the viewer draws it.
//!
//! Every object is sent with its world matrix, so the browser keeps a flat
//! list and never walks a hierarchy: a joint turned in Kreate arrives as new
//! matrices for everything it carries. Meshes go separately, by identity, and
//! only when one changes — an arm being posed sends sixteen numbers per part,
//! not its triangles.
//!
//! An edit is taken in by what it changed, not by looking at everything: the
//! objects the session names and whatever hangs below them, since a parent's
//! move moves them all. Looking at everything cost 80 ms an edit in a
//! factory of 50,000 parts — a world transform, a matrix and a colour for
//! each — when one part had moved.
//!
//! Positions are in metres with Z up, as Kreate keeps them; the viewer sets
//! its camera up that way rather than turning the scene.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use kreate_math::DMat4;
use kreate_mesh::MeshData;
use kreate_scene::robot::RobotPart;
use kreate_scene::{Changes, DatablockKind, Document, MeshId, ObjectData, ObjectId};
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
    pub fn reset(&mut self, doc: &Document) {
        self.objects = doc
            .objects()
            .filter_map(|(id, _)| {
                let uid = doc.object_uid(id)?.raw();
                Some((uid, drawn(doc, id, uid)))
            })
            .collect();
        self.meshes.clear();
        let index = mesh_index(doc);
        for uid in referenced(&self.objects) {
            if let Some(mesh) = index.get(&uid).and_then(|id| doc.mesh(*id)) {
                self.meshes.insert(uid, mesh_json(mesh));
            }
        }
        self.robots = robots(doc);
    }

    /// Everything, for a viewer that has just opened.
    pub fn scene(&self) -> String {
        let mut out = String::from("{\"meshes\":");
        push_meshes(&mut out, self.meshes.iter());
        out.push_str(",\"objects\":");
        out.push_str(&serde_json::to_string(&self.objects.values().collect::<Vec<_>>()).unwrap());
        out.push_str(",\"robots\":");
        out.push_str(&self.robots.to_string());
        out.push('}');
        out
    }

    /// Takes in an edit and returns what the viewers need to show it, or
    /// `None` when nothing they draw changed.
    ///
    /// With `changes` — what a session patch names — only those objects and
    /// what hangs below them are looked at. Without, everything is, which is
    /// right but slow in a large scene.
    pub fn update(&mut self, doc: &Document, changes: Option<&Changes>) -> Option<String> {
        let index: HashMap<u64, ObjectId> = doc
            .objects()
            .filter_map(|(id, _)| Some((doc.object_uid(id)?.raw(), id)))
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

        // A material can colour any object, and the settings anything: those
        // are looked at whole. They are rare beside moves.
        let whole = changes.is_none_or(|c| {
            c.settings
                || c.changed
                    .iter()
                    .any(|(kind, _)| !matches!(kind, DatablockKind::Object | DatablockKind::Mesh))
        });
        let mut removed: BTreeSet<u64> = BTreeSet::new();
        let candidates: Vec<u64> = if whole {
            removed.extend(self.objects.keys().filter(|u| !index.contains_key(u)));
            index.keys().copied().collect()
        } else {
            let c = changes.unwrap();
            removed.extend(c.removed.iter().map(|u| u.raw()));
            let named: Vec<u64> = c
                .changed
                .iter()
                .filter(|(kind, _)| *kind == DatablockKind::Object)
                .map(|(_, uid)| uid.raw())
                // Objects showing an edited mesh: their slots may colour them.
                .chain(
                    self.objects
                        .values()
                        .filter(|d| d.mesh.is_some_and(|m| edited.contains(&m)))
                        .map(|d| d.uid),
                )
                .collect();
            below(doc, &index, &named)
        };

        let mut changed = Vec::new();
        let mut robots_touched = whole;
        for uid in candidates {
            let Some(&id) = index.get(&uid) else {
                removed.insert(uid);
                continue;
            };
            robots_touched |= doc.object(id).is_some_and(|o| o.robot.is_some());
            let now = drawn(doc, id, uid);
            if self.objects.get(&uid) != Some(&now) {
                self.objects.insert(uid, now.clone());
                changed.push(now);
            }
        }
        removed.retain(|uid| !index.contains_key(uid));
        for uid in &removed {
            if let Some(gone) = self.objects.remove(uid) {
                robots_touched |= gone.role.is_some();
            }
        }

        // Meshes: an edited one again, a newly shown one for the first time,
        // and one nothing shows any more let go.
        let mut meshes = BTreeMap::new();
        let wanted: BTreeSet<u64> = changed
            .iter()
            .filter_map(|d| d.mesh)
            .filter(|m| edited.contains(m) || !self.meshes.contains_key(m))
            .chain(
                edited
                    .iter()
                    .copied()
                    .filter(|m| self.meshes.contains_key(m)),
            )
            .collect();
        if !wanted.is_empty() {
            let index = mesh_index(doc);
            for uid in wanted {
                if let Some(mesh) = index.get(&uid).and_then(|id| doc.mesh(*id)) {
                    let encoded = mesh_json(mesh);
                    meshes.insert(uid, encoded.clone());
                    self.meshes.insert(uid, encoded);
                }
            }
        }
        if !removed.is_empty() || changed.iter().any(|d| d.mesh.is_some()) {
            let shown = referenced(&self.objects);
            self.meshes.retain(|uid, _| shown.contains(uid));
        }

        let mut robots_changed = false;
        if robots_touched {
            let now = robots(doc);
            if now != self.robots {
                self.robots = now;
                robots_changed = true;
            }
        }
        if changed.is_empty() && removed.is_empty() && meshes.is_empty() && !robots_changed {
            return None;
        }

        let mut out = String::from("{\"meshes\":");
        push_meshes(&mut out, meshes.iter());
        out.push_str(",\"objects\":");
        out.push_str(&serde_json::to_string(&changed).unwrap());
        out.push_str(",\"removed\":");
        out.push_str(&serde_json::to_string(&removed).unwrap());
        if robots_changed {
            out.push_str(",\"robots\":");
            out.push_str(&self.robots.to_string());
        }
        out.push('}');
        Some(out)
    }
}

/// `named` and everything below them in the hierarchy, each once.
fn below(doc: &Document, index: &HashMap<u64, ObjectId>, named: &[u64]) -> Vec<u64> {
    let mut children: HashMap<ObjectId, Vec<ObjectId>> = HashMap::new();
    for (id, object) in doc.objects() {
        if let Some(parent) = object.parent {
            children.entry(parent).or_default().push(id);
        }
    }
    let mut seen = BTreeSet::new();
    let mut stack: Vec<u64> = named.to_vec();
    while let Some(uid) = stack.pop() {
        if !seen.insert(uid) {
            continue;
        }
        if let Some(kids) = index.get(&uid).and_then(|id| children.get(id)) {
            stack.extend(
                kids.iter()
                    .filter_map(|k| doc.object_uid(*k))
                    .map(|u| u.raw()),
            );
        }
    }
    seen.into_iter().collect()
}

fn drawn(doc: &Document, id: ObjectId, uid: u64) -> Drawn {
    let object = doc.object(id).expect("an object the document listed");
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
    }
}

fn robots(doc: &Document) -> Value {
    Value::Array(
        kynex_model::robots(doc)
            .iter()
            .map(crate::json::robot)
            .collect(),
    )
}

fn referenced(objects: &BTreeMap<u64, Drawn>) -> BTreeSet<u64> {
    objects.values().filter_map(|d| d.mesh).collect()
}

fn push_meshes<'a>(out: &mut String, meshes: impl Iterator<Item = (&'a u64, &'a String)>) {
    out.push('{');
    for (i, (uid, mesh)) in meshes.enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!("\"{uid}\":{mesh}"));
    }
    out.push('}');
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

    fn moved(uid: Uid) -> Changes {
        let mut c = Changes::default();
        c.changed.insert((DatablockKind::Object, uid));
        c
    }

    #[test]
    fn a_scene_is_sent_whole_then_by_the_edit() {
        let mut doc = Document::new();
        let cube = doc.add_mesh_object("Cube", kreate_mesh::primitives::cube(2.0));
        let mut view = ViewState::new();
        view.reset(&doc);
        let scene = parse(&view.scene());
        assert_eq!(scene["objects"].as_array().unwrap().len(), 1);
        assert_eq!(scene["meshes"].as_object().unwrap().len(), 1);

        // Moving it sends its matrix and not its mesh again.
        doc.object_mut(cube).unwrap().transform.translation.x = 3.0;
        let uid = doc.object_uid(cube).unwrap();
        let update = parse(&view.update(&doc, Some(&moved(uid))).unwrap());
        assert_eq!(update["meshes"].as_object().unwrap().len(), 0);
        assert_eq!(update["objects"][0]["matrix"][12], 3.0);

        // Editing its geometry sends the mesh.
        let mesh_uid = doc
            .mesh_uid(doc.object(cube).unwrap().mesh().unwrap())
            .unwrap();
        let mut changes = Changes::default();
        changes.changed.insert((DatablockKind::Mesh, mesh_uid));
        let update = parse(&view.update(&doc, Some(&changes)).unwrap());
        assert_eq!(update["meshes"].as_object().unwrap().len(), 1);

        // Nothing changed is nothing to send, looked at either way.
        assert!(view.update(&doc, None).is_none());
        assert!(view.update(&doc, Some(&moved(uid))).is_none());

        // Deleting it says so, and its mesh is let go.
        doc.remove_object(cube);
        let mut gone = Changes::default();
        gone.removed.insert(uid);
        let update = parse(&view.update(&doc, Some(&gone)).unwrap());
        assert_eq!(update["removed"][0], uid.raw());
        assert_eq!(parse(&view.scene())["meshes"].as_object().unwrap().len(), 0);
    }

    #[test]
    fn a_parent_moved_moves_what_hangs_below_it() {
        let mut doc = Document::new();
        let station = doc.add_object(kreate_scene::Object::new("Station", ObjectData::Empty));
        let part = doc.add_mesh_object("Part", kreate_mesh::primitives::cube(1.0));
        let deeper = doc.add_mesh_object("Bolt", kreate_mesh::primitives::cube(0.1));
        doc.set_parent(part, Some(station)).unwrap();
        doc.set_parent(deeper, Some(part)).unwrap();
        let other = doc.add_mesh_object("Elsewhere", kreate_mesh::primitives::cube(1.0));
        let mut view = ViewState::new();
        view.reset(&doc);

        // Only the station is named; both parts below it move with it.
        doc.object_mut(station).unwrap().transform.translation.y = 4.0;
        let uid = doc.object_uid(station).unwrap();
        let update = parse(&view.update(&doc, Some(&moved(uid))).unwrap());
        let names: BTreeSet<&str> = update["objects"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| o["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, BTreeSet::from(["Station", "Part", "Bolt"]));
        let _ = other;

        // And the incremental answer is the whole one.
        let mut fresh = ViewState::new();
        fresh.reset(&doc);
        assert_eq!(view.scene(), fresh.scene());
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
