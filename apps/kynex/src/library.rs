//! The library: equipment to place in a cell — robots, CNC machines,
//! injection moulding machines, pallet tables, conveyors, AMRs, pedestals,
//! fences — each made from a template for the sizes it is given, as Visual
//! Components' catalogue does. Each template comes with a thumbnail: the
//! piece at its usual size, as triangles, for the page to draw.
//!
//! Placed equipment is KYNEX's own, as the cell's parts are: it is added to
//! the scene KYNEX shows, under identities of its own (see [`EQUIPMENT`]),
//! and kept in the project file. The session's scene is not changed; a
//! cell can be laid out with no Kreate session at all (`kynex view
//! --empty`).
//!
//! Equipment arrives with its roles: a CNC machine is a CNC machine to the
//! cell, a pallet table's plate a pallet, a tending robot's grip points
//! grippers.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use kreate_io::ir::{ImportScene, SourceMetadata};
use kreate_math::{DQuat, DVec3, SceneRgb};
use kreate_scene::robot::RobotPart;
use kreate_scene::{
    Changes, DatablockKind, Document, Material, MaterialId, Object, ObjectData, ObjectId, Uid,
};
use serde_json::{json, Value};

use crate::cell::Kind;

/// The first identity of placed equipment. Kreate numbers a participant's
/// datablocks from `site << 48`; this site is never a participant, nor is
/// the cell's parts' (`0xFFFF`).
pub const EQUIPMENT: u64 = 0xFFFE << 48;

/// Identities each placed item has room for: its objects, meshes and
/// materials each count from its own start.
const PER_ITEM: u64 = 1 << 24;

/// A size a template is made for.
pub struct Param {
    pub key: &'static str,
    pub label: &'static str,
    pub default: f64,
    pub min: f64,
    pub max: f64,
}

/// A kind of equipment.
pub struct Template {
    pub key: &'static str,
    pub name: &'static str,
    pub group: &'static str,
    pub params: &'static [Param],
}

const fn p(key: &'static str, label: &'static str, default: f64, min: f64, max: f64) -> Param {
    Param {
        key,
        label,
        default,
        min,
        max,
    }
}

/// Everything the library holds. Sizes start at the roadmap's
/// industry-typical values (docs/roadmap-vs-visual-components.md §4).
pub const TEMPLATES: &[Template] = &[
    Template {
        key: "ur10e_tending",
        name: "UR10e + 이중 그리퍼",
        group: "로봇",
        params: &[],
    },
    Template {
        key: "ur5e",
        name: "UR5e",
        group: "로봇",
        params: &[],
    },
    Template {
        key: "cnc_lathe",
        name: "CNC 선반",
        group: "공작기계",
        params: &[
            p("width", "폭", 1.8, 1.0, 4.0),
            p("depth", "깊이", 1.6, 1.0, 4.0),
            p("height", "높이", 1.9, 1.2, 3.0),
            p("opening", "문 폭", 0.6, 0.3, 1.5),
        ],
    },
    Template {
        key: "injection_molder",
        name: "사출 성형기",
        group: "성형기",
        // A 150 t machine: some 5 m long, 1.5 m wide, 2 m to its hopper.
        params: &[
            p("length", "길이", 5.0, 3.0, 9.0),
            p("width", "폭", 1.5, 1.0, 2.5),
            p("height", "높이", 2.0, 1.5, 3.0),
        ],
    },
    Template {
        key: "infeed_station",
        name: "투입 팔레트 테이블",
        group: "주변 장치",
        params: &[
            p("height", "높이", 0.77, 0.3, 1.5),
            p("plate_w", "판 폭", 0.55, 0.2, 2.0),
            p("plate_d", "판 깊이", 0.45, 0.2, 2.0),
        ],
    },
    Template {
        key: "outfeed_station",
        name: "배출 팔레트 테이블",
        group: "주변 장치",
        params: &[
            p("height", "높이", 0.77, 0.3, 1.5),
            p("plate_w", "판 폭", 0.55, 0.2, 2.0),
            p("plate_d", "판 깊이", 0.45, 0.2, 2.0),
        ],
    },
    Template {
        key: "conveyor",
        name: "벨트 컨베이어",
        group: "물류",
        params: &[
            p("length", "길이", 2.0, 0.5, 12.0),
            p("width", "벨트 폭", 0.5, 0.2, 1.5),
            p("height", "높이", 0.8, 0.3, 1.5),
        ],
    },
    Template {
        key: "amr",
        name: "AMR (자율 이동 로봇)",
        group: "물류",
        // A 250 kg-class mobile base.
        params: &[
            p("length", "길이", 0.9, 0.5, 2.0),
            p("width", "폭", 0.6, 0.4, 1.5),
            p("height", "높이", 0.35, 0.2, 0.8),
        ],
    },
    Template {
        key: "amr_stop",
        name: "AMR 정지 위치",
        group: "물류",
        // The AMR's own footprint, to see where it will stand.
        params: &[
            p("length", "길이", 0.9, 0.5, 2.0),
            p("width", "폭", 0.6, 0.4, 1.5),
        ],
    },
    Template {
        key: "pedestal",
        name: "로봇 받침대",
        group: "주변 장치",
        params: &[
            p("height", "높이", 0.7, 0.1, 2.0),
            p("size", "폭", 0.4, 0.2, 1.5),
        ],
    },
    Template {
        key: "fence",
        name: "안전 펜스",
        group: "안전",
        params: &[
            p("length", "길이", 3.8, 1.0, 20.0),
            p("width", "폭", 3.8, 1.0, 20.0),
            p("height", "높이", 2.0, 1.0, 3.0),
        ],
    },
];

pub fn template(key: &str) -> Option<&'static Template> {
    TEMPLATES.iter().find(|t| t.key == key)
}

/// A placed piece of equipment.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: u64,
    pub template: String,
    /// Its name in the scene: the template's, numbered.
    pub name: String,
    pub params: BTreeMap<String, f64>,
    /// Where its origin stands, and its turn about Z, radians.
    pub at: DVec3,
    pub yaw: f64,
}

impl Item {
    fn param(&self, key: &str) -> f64 {
        let t = template(&self.template);
        let spec = t.and_then(|t| t.params.iter().find(|p| p.key == key));
        let v = self
            .params
            .get(key)
            .copied()
            .or(spec.map(|p| p.default))
            .unwrap_or(0.0);
        spec.map_or(v, |p| v.clamp(p.min, p.max))
    }

    fn root_uid(&self) -> u64 {
        EQUIPMENT + self.id * PER_ITEM
    }
}

/// An item made at the origin, in a document of its own: copied into the
/// scene each time it is shown.
struct Built {
    doc: Document,
    /// Its objects, parents first.
    order: Vec<ObjectId>,
    /// The objects that come with a role, and the robot's root link, which
    /// takes the item's name.
    roles: Vec<(ObjectId, Kind)>,
    root_link: Option<ObjectId>,
}

/// What a request changed in the cell's roles: added with the equipment,
/// gone with it.
#[derive(Debug, Default, PartialEq)]
pub struct RoleChanges {
    pub added: Vec<(u64, String, Kind)>,
    pub removed: Vec<u64>,
}

/// The placed equipment.
#[derive(Default)]
pub struct Library {
    items: Vec<Item>,
    next: u64,
    /// Each item as made, with the parameters it was made for.
    built: HashMap<u64, (BTreeMap<String, f64>, Built)>,
    /// Each item's object identities as last shown.
    shown: HashMap<u64, Vec<u64>>,
    removed: Vec<u64>,
    /// The items changed since they were last saved.
    items_changed: bool,
}

impl Library {
    pub fn from_saved(saved: &[Value]) -> Self {
        let mut library = Self::default();
        for v in saved {
            let (Some(id), Some(key)) = (v["id"].as_u64(), v["template"].as_str()) else {
                continue;
            };
            if template(key).is_none() {
                continue;
            }
            let at = match v["at"].as_array().map(Vec::as_slice) {
                Some([x, y, z]) => DVec3::new(
                    x.as_f64().unwrap_or(0.0),
                    y.as_f64().unwrap_or(0.0),
                    z.as_f64().unwrap_or(0.0),
                ),
                _ => DVec3::ZERO,
            };
            let params = v["params"]
                .as_object()
                .map(|m| {
                    m.iter()
                        .filter_map(|(k, v)| Some((k.clone(), v.as_f64()?)))
                        .collect()
                })
                .unwrap_or_default();
            library.items.push(Item {
                id,
                template: key.to_string(),
                name: v["name"].as_str().unwrap_or(key).to_string(),
                params,
                at,
                yaw: v["yaw"].as_f64().unwrap_or(0.0),
            });
            library.next = library.next.max(id);
        }
        library
    }

    /// Puts the equipment `saved` in place of this: a scenario taken back.
    /// What was shown goes from the viewers.
    pub fn restore(&mut self, saved: &[Value]) {
        let mut gone = std::mem::take(&mut self.removed);
        gone.extend(self.shown.values().flatten().copied());
        *self = Self::from_saved(saved);
        self.removed = gone;
        self.items_changed = true;
    }

    pub fn to_saved(&self) -> Vec<Value> {
        self.items
            .iter()
            .map(|i| {
                json!({
                    "id": i.id,
                    "template": i.template,
                    "name": i.name,
                    "params": i.params,
                    "at": [i.at.x, i.at.y, i.at.z],
                    "yaw": i.yaw,
                })
            })
            .collect()
    }

    pub fn take_items_changed(&mut self) -> bool {
        std::mem::take(&mut self.items_changed)
    }

    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// Takes one request from the page. Returns the answer, and the roles
    /// that came or went with the equipment.
    pub fn handle(&mut self, body: &str) -> (String, RoleChanges) {
        let mut roles = RoleChanges::default();
        let answer = match self.take(body, &mut roles) {
            Ok(v) => v,
            Err(e) => json!({ "ok": false, "error": e }),
        };
        (answer.to_string(), roles)
    }

    fn take(&mut self, body: &str, roles: &mut RoleChanges) -> Result<Value, String> {
        let req: Value = serde_json::from_str(body).map_err(|e| format!("not JSON: {e}"))?;
        let op = req["op"].as_str().ok_or("no op")?;
        let point = |v: &Value| match v.as_array().map(Vec::as_slice) {
            Some([x, y, z]) => Some(DVec3::new(x.as_f64()?, y.as_f64()?, z.as_f64()?)),
            _ => None,
        };
        let params = |v: &Value| -> BTreeMap<String, f64> {
            v.as_object()
                .map(|m| {
                    m.iter()
                        .filter_map(|(k, v)| Some((k.clone(), v.as_f64()?)))
                        .collect()
                })
                .unwrap_or_default()
        };
        if op == "place" {
            let key = req["template"].as_str().ok_or("no template")?;
            let t = template(key).ok_or("no such template")?;
            self.next += 1;
            let n = self.items.iter().filter(|i| i.template == key).count() + 1;
            let item = Item {
                id: self.next,
                template: key.to_string(),
                name: format!("{} {n}", t.name),
                params: params(&req["params"]),
                at: point(&req["at"]).unwrap_or(DVec3::ZERO),
                yaw: req["yaw"].as_f64().unwrap_or(0.0),
            };
            let built = build(&item)?;
            roles.added = role_uids(&item, &built);
            self.built.insert(item.id, (item.params.clone(), built));
            self.items.push(item);
            self.items_changed = true;
            return Ok(json!({ "ok": true, "item": self.next }));
        }
        let id = req["item"].as_u64().ok_or("no item")?;
        let at = self
            .items
            .iter()
            .position(|i| i.id == id)
            .ok_or("no such item")?;
        match op {
            "move" => {
                let item = &mut self.items[at];
                if let Some(p) = point(&req["at"]) {
                    item.at = p;
                }
                if let Some(yaw) = req["yaw"].as_f64() {
                    item.yaw = yaw;
                }
            }
            "resize" => {
                let item = &mut self.items[at];
                for (k, v) in params(&req["params"]) {
                    item.params.insert(k, v);
                }
            }
            "remove" => {
                let item = self.items.remove(at);
                self.built.remove(&item.id);
                if let Some(gone) = self.shown.remove(&item.id) {
                    roles.removed.extend(gone.iter().copied());
                    self.removed.extend(gone);
                }
            }
            other => return Err(format!("unknown op {other:?}")),
        }
        self.items_changed = true;
        Ok(json!({ "ok": true }))
    }

    /// Every item's roles, by the identities its objects have in the scene:
    /// for roles that are to be there whatever was saved.
    pub fn roles(&mut self) -> Vec<(u64, String, Kind)> {
        let mut out = Vec::new();
        for item in self.items.clone() {
            if let Ok(built) = self.made(&item) {
                out.extend(role_uids(&item, built));
            }
        }
        out
    }

    /// The item as made for its parameters now: made again if they changed.
    fn made(&mut self, item: &Item) -> Result<&Built, String> {
        let fresh = self
            .built
            .get(&item.id)
            .is_some_and(|(params, _)| *params == item.params);
        if !fresh {
            let built = build(item)?;
            self.built.insert(item.id, (item.params.clone(), built));
        }
        Ok(&self.built[&item.id].1)
    }

    /// Adds every item to `doc`, the scene as shown, and returns what the
    /// viewers need to look at again.
    pub fn apply(&mut self, doc: &mut Document) -> Changes {
        let mut changes = Changes::default();
        changes
            .removed
            .extend(self.removed.drain(..).map(Uid::from_raw));
        for item in self.items.clone() {
            let Ok(built) = self.made(&item) else {
                continue;
            };
            let uids = graft(&item, built, doc);
            if let Some(before) = self.shown.get(&item.id) {
                // Made smaller: what it no longer has goes.
                changes.removed.extend(
                    before
                        .iter()
                        .filter(|u| !uids.contains(u))
                        .map(|u| Uid::from_raw(*u)),
                );
            }
            changes.changed.extend(
                uids.iter()
                    .map(|u| (DatablockKind::Object, Uid::from_raw(*u))),
            );
            self.shown.insert(item.id, uids);
        }
        // Gone and back again, as equipment taken back from a scenario is:
        // only changed.
        let kept: Vec<Uid> = changes.changed.iter().map(|(_, u)| *u).collect();
        changes.removed.retain(|u| !kept.contains(u));
        changes
    }

    /// The `templates` event: each template at its usual size, as coloured
    /// triangles, for the page to draw its thumbnail by. Made once.
    pub fn thumbnails() -> &'static str {
        static THUMBNAILS: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        THUMBNAILS.get_or_init(|| {
            let mut out = serde_json::Map::new();
            for t in TEMPLATES {
                let item = Item {
                    id: 0,
                    template: t.key.to_string(),
                    name: t.name.to_string(),
                    params: BTreeMap::new(),
                    at: DVec3::ZERO,
                    yaw: 0.0,
                };
                if let Ok(built) = build(&item) {
                    out.insert(t.key.to_string(), triangles(&built.doc));
                }
            }
            Value::Object(out).to_string()
        })
    }

    /// The `library` event: what can be placed, and what is.
    pub fn to_json(&self) -> String {
        json!({
            "templates": TEMPLATES.iter().map(|t| json!({
                "key": t.key,
                "name": t.name,
                "group": t.group,
                "params": t.params.iter().map(|p| json!({
                    "key": p.key, "label": p.label, "default": p.default, "min": p.min, "max": p.max,
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "items": self.items.iter().map(|i| json!({
                "id": i.id,
                "template": i.template,
                "name": i.name,
                "root": i.root_uid().to_string(),
                "params": i.params,
                "at": [i.at.x, i.at.y, i.at.z],
                "yaw": i.yaw,
            })).collect::<Vec<_>>(),
        })
        .to_string()
    }
}

/// Every mesh of `doc` where it stands, as `[{"c": [r, g, b], "p": [x, y,
/// z, …]}]`: a colour, and three corners a triangle, to the millimetre.
fn triangles(doc: &Document) -> Value {
    let mut parts = Vec::new();
    for (id, object) in doc.objects() {
        let Some(mesh) = object.mesh().and_then(|m| doc.mesh(m)) else {
            continue;
        };
        let world = doc.world_transform(id);
        let colour = doc.material_at(id, 0).map_or([0.7, 0.72, 0.75], |m| {
            [m.base_color.r, m.base_color.g, m.base_color.b]
        });
        let at: Vec<DVec3> = mesh
            .positions()
            .iter()
            .map(|p| world.transform_point3(p.as_dvec3()))
            .collect();
        let corners: Vec<f64> = mesh
            .triangulate()
            .indices
            .iter()
            .flat_map(|i| {
                let p = at[*i as usize];
                [p.x, p.y, p.z].map(|v| (v * 1000.0).round() / 1000.0)
            })
            .collect();
        parts.push(json!({ "c": colour, "p": corners }));
    }
    Value::Array(parts)
}

/// The roles `built` comes with, by the identities its objects take in the
/// scene.
fn role_uids(item: &Item, built: &Built) -> Vec<(u64, String, Kind)> {
    built
        .roles
        .iter()
        .filter_map(|(id, kind)| {
            // A machine, an AMR, a stop is the item as a whole: its root,
            // under its name.
            if matches!(kind, Kind::Cnc | Kind::Molder | Kind::Amr | Kind::Stop) {
                return Some((item.root_uid(), item.name.clone(), *kind));
            }
            let k = built.order.iter().position(|o| o == id)? as u64;
            let name = built.doc.object(*id)?.name.clone();
            Some((item.root_uid() + 1 + k, name, *kind))
        })
        .collect()
}

/// Copies `built` into `doc` as `item`, under its own identities: the
/// item's root (an empty, where it stands), then every object of it.
fn graft(item: &Item, built: &Built, doc: &mut Document) -> Vec<u64> {
    let base = item.root_uid();
    let mut root = Object::new(item.name.clone(), ObjectData::Empty);
    root.transform.translation = item.at;
    root.transform.rotation = DQuat::from_rotation_z(item.yaw);
    let root_id = doc.restore_object(root, Uid::from_raw(base));
    let mut uids = vec![base];

    let mut materials: HashMap<MaterialId, MaterialId> = HashMap::new();
    let mut meshes = HashMap::new();
    let mut ids: HashMap<ObjectId, ObjectId> = HashMap::new();
    let mut next_mesh = base;
    let mut next_material = base;
    for (k, &src) in built.order.iter().enumerate() {
        let Some(o) = built.doc.object(src) else {
            continue;
        };
        let data = match o.data {
            ObjectData::Mesh(m) => {
                let Some(mesh) = built.doc.mesh(m) else {
                    continue;
                };
                let id = *meshes.entry(m).or_insert_with(|| {
                    next_mesh += 1;
                    let id = doc.restore_mesh(mesh.clone(), Uid::from_raw(next_mesh));
                    for (slot, material) in built.doc.mesh_slots(m).iter().enumerate() {
                        let copied = material.map(|mat| {
                            *materials.entry(mat).or_insert_with(|| {
                                next_material += 1;
                                let m = built.doc.material(mat).cloned().unwrap_or_default();
                                doc.restore_material(m, Uid::from_raw(next_material))
                            })
                        });
                        doc.set_mesh_slot(id, slot, copied);
                    }
                    id
                });
                ObjectData::Mesh(id)
            }
            _ => ObjectData::Empty,
        };
        let name = if Some(src) == built.root_link {
            item.name.clone()
        } else {
            o.name.clone()
        };
        let mut copy = Object::new(name, data);
        copy.transform = o.transform;
        copy.visible = o.visible;
        copy.robot = o.robot.clone();
        match o.parent.and_then(|p| ids.get(&p)) {
            Some(parent) => {
                copy.parent = Some(*parent);
                copy.parent_inverse = o.parent_inverse;
            }
            // Its top: under the item's root, where the root stands.
            None => copy.parent = Some(root_id),
        }
        let uid = base + 1 + k as u64;
        ids.insert(src, doc.restore_object(copy, Uid::from_raw(uid)));
        uids.push(uid);
    }
    uids
}

// --- making equipment -------------------------------------------------------

fn build(item: &Item) -> Result<Built, String> {
    let mut doc = Document::new();
    let mut roles = Vec::new();
    let mut root_link = None;
    match item.template.as_str() {
        "ur10e_tending" | "ur5e" => {
            let urdf = if item.template == "ur5e" {
                include_str!("../library/ur5e.urdf")
            } else {
                include_str!("../library/ur10e_tending.urdf")
            };
            let none = |_: &Path| -> Result<ImportScene, String> {
                Ok(ImportScene::new(SourceMetadata::new("none")))
            };
            let robot = kreate_io::urdf::parse_urdf(urdf, Path::new("."), &none)
                .map_err(|e| e.to_string())?;
            kreate_io::build::build_into(&robot, &mut doc);
            let named = |doc: &Document, n: &str| {
                doc.objects().find(|(_, o)| o.name == n).map(|(id, _)| id)
            };
            root_link = named(&doc, "base_link");
            for grip in ["grip_a", "grip_b"] {
                if let Some(id) = named(&doc, grip) {
                    roles.push((id, Kind::Gripper));
                }
            }
            // At rest, the tool down: the pose a tending robot waits in.
            for (joint, q) in [
                ("shoulder_lift_joint", -1.75),
                ("elbow_joint", 1.75),
                ("wrist_1_joint", -1.571),
                ("wrist_2_joint", -1.571),
            ] {
                if let Some(id) = named(&doc, joint) {
                    if let Some(RobotPart::Joint(j)) = doc.object(id).and_then(|o| o.robot.clone())
                    {
                        doc.object_mut(id).unwrap().transform = j.place(q);
                    }
                }
            }
        }
        "cnc_lathe" => {
            let c = Colours::new(&mut doc);
            let machine = cnc(&mut doc, &c, item);
            roles.push((machine, Kind::Cnc));
        }
        "infeed_station" | "outfeed_station" => {
            let c = Colours::new(&mut doc);
            let (h, w, d) = (
                item.param("height"),
                item.param("plate_w"),
                item.param("plate_d"),
            );
            block(
                &mut doc,
                "table",
                [w + 0.05, d + 0.05, h - 0.02],
                [0.0, 0.0, (h - 0.02) / 2.0],
                c.table,
                None,
            );
            let plate = block(
                &mut doc,
                "pallet",
                [w, d, 0.02],
                [0.0, 0.0, h - 0.01],
                c.pallet,
                None,
            );
            let kind = if item.template == "infeed_station" {
                Kind::Infeed
            } else {
                Kind::Outfeed
            };
            roles.push((plate, kind));
        }
        "conveyor" => {
            let c = Colours::new(&mut doc);
            let belt = conveyor(
                &mut doc,
                &c,
                item.param("length"),
                item.param("width"),
                item.param("height"),
            );
            roles.push((belt, Kind::Conveyor));
        }
        "injection_molder" => {
            let c = Colours::new(&mut doc);
            let machine = molder(
                &mut doc,
                &c,
                item.param("length"),
                item.param("width"),
                item.param("height"),
            );
            roles.push((machine, Kind::Molder));
        }
        "amr" => {
            let c = Colours::new(&mut doc);
            let amr = amr(
                &mut doc,
                &c,
                item.param("length"),
                item.param("width"),
                item.param("height"),
            );
            roles.push((amr, Kind::Amr));
        }
        "amr_stop" => {
            let c = Colours::new(&mut doc);
            let stop = amr_stop(&mut doc, &c, item.param("length"), item.param("width"));
            roles.push((stop, Kind::Stop));
        }
        "pedestal" => {
            let c = Colours::new(&mut doc);
            let (h, s) = (item.param("height"), item.param("size"));
            block(
                &mut doc,
                "pedestal",
                [s, s, h],
                [0.0, 0.0, h / 2.0],
                c.dark,
                None,
            );
        }
        "fence" => {
            let c = Colours::new(&mut doc);
            let (l, w, h) = (
                item.param("length"),
                item.param("width"),
                item.param("height"),
            );
            fence(&mut doc, &c, l, w, h);
        }
        other => return Err(format!("no template {other:?}")),
    }
    let order = doc.hierarchy().into_iter().map(|(id, _)| id).collect();
    Ok(Built {
        doc,
        order,
        roles,
        root_link,
    })
}

struct Colours {
    machine: MaterialId,
    door: MaterialId,
    steel: MaterialId,
    table: MaterialId,
    pallet: MaterialId,
    dark: MaterialId,
    fence: MaterialId,
    belt: MaterialId,
    body: MaterialId,
    accent: MaterialId,
}

impl Colours {
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
            machine: colour("machine", [0.86, 0.87, 0.88], 0.0),
            door: colour("door", [0.32, 0.40, 0.50], 0.2),
            steel: colour("steel", [0.72, 0.74, 0.78], 0.8),
            table: colour("table", [0.28, 0.30, 0.33], 0.0),
            pallet: colour("pallet", [0.20, 0.45, 0.75], 0.0),
            dark: colour("dark", [0.12, 0.13, 0.15], 0.0),
            fence: colour("fence", [0.95, 0.75, 0.10], 0.0),
            belt: colour("belt", [0.06, 0.06, 0.07], 0.0),
            body: colour("body", [0.93, 0.93, 0.91], 0.0),
            accent: colour("accent", [0.95, 0.45, 0.12], 0.0),
        }
    }
}

/// A box `size` across, its centre at `centre`, under `parent` if given.
fn block(
    doc: &mut Document,
    name: &str,
    size: [f64; 3],
    centre: [f64; 3],
    colour: MaterialId,
    parent: Option<ObjectId>,
) -> ObjectId {
    let id = doc.add_mesh_object(name, kreate_mesh::primitives::cube(1.0));
    paint(doc, id, colour);
    let o = doc.object_mut(id).unwrap();
    o.transform.scale = DVec3::from_array(size);
    o.transform.translation = DVec3::from_array(centre);
    if parent.is_some() {
        doc.set_parent(id, parent).unwrap();
    }
    id
}

/// An upright cylinder, its centre at `centre`.
fn post(
    doc: &mut Document,
    name: &str,
    radius: f64,
    height: f64,
    centre: [f64; 3],
    colour: MaterialId,
    parent: Option<ObjectId>,
) -> ObjectId {
    let id = doc.add_mesh_object(
        name,
        kreate_mesh::primitives::cylinder(radius as f32, height as f32, 32),
    );
    paint(doc, id, colour);
    doc.object_mut(id).unwrap().transform.translation = DVec3::from_array(centre);
    if parent.is_some() {
        doc.set_parent(id, parent).unwrap();
    }
    id
}

/// A cylinder lying along X (`along_x`) or else Y, its centre at `centre`.
#[allow(clippy::too_many_arguments)]
fn bar(
    doc: &mut Document,
    name: &str,
    radius: f64,
    length: f64,
    centre: [f64; 3],
    along_x: bool,
    colour: MaterialId,
    parent: Option<ObjectId>,
) -> ObjectId {
    let id = post(doc, name, radius, length, centre, colour, parent);
    doc.object_mut(id).unwrap().transform.rotation = if along_x {
        DQuat::from_rotation_y(std::f64::consts::FRAC_PI_2)
    } else {
        DQuat::from_rotation_x(std::f64::consts::FRAC_PI_2)
    };
    id
}

fn paint(doc: &mut Document, id: ObjectId, colour: MaterialId) {
    if let Some(mesh) = doc.object(id).and_then(|o| o.mesh()) {
        doc.set_mesh_slot(mesh, 0, Some(colour));
    }
}

/// A small CNC machine, its front at y = 0 facing −Y, centred on x: a
/// cabinet with a working space open at the front behind a sliding door,
/// and a chuck on the working space's floor. The machine is the returned
/// empty; its door and chuck are named so (see `crate::cell`).
fn cnc(doc: &mut Document, c: &Colours, item: &Item) -> ObjectId {
    let machine = doc.add_object(Object::new("machine", ObjectData::Empty));
    let m = Some(machine);
    let (w, d, h) = (
        item.param("width"),
        item.param("depth"),
        item.param("height"),
    );
    let opening = item.param("opening").min(w - 0.2);
    let floor = 0.85;
    let roof = (h - 0.35).max(floor + 0.4);
    let side = (w - opening) / 2.0;
    let mid = (floor + roof) / 2.0;
    let tall = roof - floor;
    let space = (d - 0.7).max(0.4);
    block(
        doc,
        "base",
        [w, d, floor],
        [0.0, d / 2.0, floor / 2.0],
        c.machine,
        m,
    );
    block(
        doc,
        "top",
        [w, d, h - roof],
        [0.0, d / 2.0, (roof + h) / 2.0],
        c.machine,
        m,
    );
    block(
        doc,
        "left",
        [side, d, tall],
        [-(opening + side) / 2.0, d / 2.0, mid],
        c.machine,
        m,
    );
    block(
        doc,
        "right",
        [side, d, tall],
        [(opening + side) / 2.0, d / 2.0, mid],
        c.machine,
        m,
    );
    block(
        doc,
        "back",
        [opening, d - space, tall],
        [0.0, space + (d - space) / 2.0, mid],
        c.machine,
        m,
    );
    block(
        doc,
        "panel",
        [0.35, 0.06, 0.5],
        [
            opening / 2.0 + side / 2.0,
            -0.03,
            (roof + floor) / 2.0 + 0.1,
        ],
        c.dark,
        m,
    );
    post(
        doc,
        "chuck",
        0.09,
        0.12,
        [0.0, 0.35, floor + 0.06],
        c.steel,
        m,
    );
    block(
        doc,
        "door",
        [opening + 0.04, 0.03, tall + 0.04],
        [0.0, -0.02, mid],
        c.door,
        m,
    );
    machine
}

/// A belt conveyor `l` long along X, centred on the origin, its belt `w`
/// wide with its top at `h`: side frames, legs at most 1.5 m apart, and a
/// drum at each end. Returns the belt, which carries parts towards +X.
fn conveyor(doc: &mut Document, c: &Colours, l: f64, w: f64, h: f64) -> ObjectId {
    let belt = block(
        doc,
        "belt",
        [l, w, 0.02],
        [0.0, 0.0, h - 0.01],
        c.belt,
        None,
    );
    for (side, y) in [("left", -1.0), ("right", 1.0)] {
        let y = y * (w / 2.0 + 0.02);
        block(
            doc,
            &format!("frame_{side}"),
            [l + 0.1, 0.04, 0.14],
            [0.0, y, h - 0.06],
            c.steel,
            None,
        );
        let legs = ((l / 1.5).ceil() as usize).max(1);
        for i in 0..=legs {
            let x = -l / 2.0 + 0.05 + (l - 0.1) * i as f64 / legs as f64;
            block(
                doc,
                &format!("leg_{side}{i}"),
                [0.05, 0.05, h - 0.12],
                [x, y, (h - 0.12) / 2.0],
                c.steel,
                None,
            );
        }
    }
    for (end, x) in [("in", -1.0), ("out", 1.0)] {
        bar(
            doc,
            &format!("drum_{end}"),
            0.05,
            w + 0.02,
            [x * l / 2.0, 0.0, h - 0.05],
            false,
            c.dark,
            None,
        );
    }
    belt
}

/// An injection moulding machine `l` long along X, its operator's side at
/// y = 0 facing −Y as a CNC machine's front: a bed, the clamping unit on
/// the left (platens, tie bars, the mould behind its guard), the injection
/// unit on the right (barrel and hopper), and the control panel. The
/// machine is the returned empty; its guard is its `door` and its mould
/// `mould` (see `crate::cell`), the moulded part standing on top of it.
fn molder(doc: &mut Document, c: &Colours, l: f64, w: f64, h: f64) -> ObjectId {
    let machine = doc.add_object(Object::new("machine", ObjectData::Empty));
    let m = Some(machine);
    let bed = 0.9;
    let (x0, y0) = (-l / 2.0, w / 2.0);
    block(doc, "bed", [l, w, bed], [0.0, y0, bed / 2.0], c.body, m);
    // The clamp: a fixed and a moving platen, the mould between.
    let clamp = l * 0.45;
    let tall = (h - bed - 0.35).max(0.6);
    let top = bed + tall;
    for (name, x) in [
        ("platen_end", 0.1),
        ("platen_moving", 0.45),
        ("platen_fixed", clamp - 0.1),
    ] {
        block(
            doc,
            name,
            [0.15, w * 0.75, tall],
            [x0 + x, y0, bed + tall / 2.0],
            c.door,
            m,
        );
    }
    block(
        doc,
        "mould",
        [clamp - 0.75, w * 0.5, tall * 0.5],
        [x0 + (0.55 + clamp - 0.2) / 2.0, y0, bed + tall * 0.25],
        c.steel,
        m,
    );
    for (k, (dy, dz)) in [(-1.0, 0.15), (1.0, 0.15), (-1.0, 0.85), (1.0, 0.85)]
        .iter()
        .enumerate()
    {
        bar(
            doc,
            &format!("tie_bar{k}"),
            0.04,
            clamp - 0.1,
            [x0 + clamp / 2.0, y0 + dy * w * 0.3, bed + tall * dz],
            true,
            c.steel,
            m,
        );
    }
    // The guard over the clamp, on the operator's side: the door the robot
    // reaches in by.
    block(
        doc,
        "door",
        [clamp, 0.03, tall + 0.2],
        [x0 + clamp / 2.0, -0.02, bed + (tall + 0.2) / 2.0],
        c.accent,
        m,
    );
    // The injection unit: the barrel into the fixed platen, the hopper on it.
    let unit = l - clamp - 0.2;
    bar(
        doc,
        "barrel",
        0.09,
        unit,
        [x0 + clamp + unit / 2.0, y0, bed + tall / 2.0],
        true,
        c.steel,
        m,
    );
    block(
        doc,
        "injection_unit",
        [unit * 0.45, w * 0.55, tall * 0.55],
        [x0 + clamp + unit * 0.7, y0, bed + tall * 0.5],
        c.door,
        m,
    );
    let hopper = (h - top).max(0.3);
    post(
        doc,
        "hopper",
        0.18,
        hopper,
        [x0 + clamp + unit * 0.45, y0, top + hopper / 2.0 - 0.05],
        c.body,
        m,
    );
    block(
        doc,
        "panel",
        [0.5, 0.06, 0.6],
        [x0 + clamp + 0.4, -0.03, bed + 0.5],
        c.dark,
        m,
    );
    machine
}

/// An autonomous mobile robot `l` × `w` × `h`, centred on the origin: a
/// body with a top plate to carry a load, a bumper round its foot, drive
/// wheels at its middle and a laser scanner at its front.
fn amr(doc: &mut Document, c: &Colours, l: f64, w: f64, h: f64) -> ObjectId {
    let amr = doc.add_object(Object::new("amr", ObjectData::Empty));
    let m = Some(amr);
    let clearance = 0.04;
    block(
        doc,
        "body",
        [l, w, h - clearance - 0.02],
        [0.0, 0.0, clearance + (h - clearance - 0.02) / 2.0],
        c.body,
        m,
    );
    block(
        doc,
        "top",
        [l - 0.02, w - 0.02, 0.02],
        [0.0, 0.0, h - 0.01],
        c.dark,
        m,
    );
    block(
        doc,
        "bumper",
        [l + 0.02, w + 0.02, 0.06],
        [0.0, 0.0, clearance + 0.05],
        c.dark,
        m,
    );
    block(
        doc,
        "stripe",
        [l + 0.004, w + 0.004, 0.025],
        [0.0, 0.0, h * 0.7],
        c.accent,
        m,
    );
    for (side, y) in [("left", -1.0), ("right", 1.0)] {
        bar(
            doc,
            &format!("wheel_{side}"),
            0.08,
            0.05,
            [0.0, y * (w / 2.0 - 0.03), 0.08],
            false,
            c.belt,
            m,
        );
    }
    post(
        doc,
        "scanner",
        0.04,
        0.06,
        [l / 2.0 - 0.06, 0.0, h + 0.03],
        c.dark,
        m,
    );
    amr
}

/// Where an AMR stops: a frame on the floor its size, and an arrow along
/// +X, the way the AMR faces there. Returns the stop, an empty at its
/// middle.
fn amr_stop(doc: &mut Document, c: &Colours, l: f64, w: f64) -> ObjectId {
    let stop = doc.add_object(Object::new("stop", ObjectData::Empty));
    let s = Some(stop);
    let t = 0.03;
    for (name, size, at) in [
        ("edge_front", [l, t, 0.004], [0.0, -w / 2.0, 0.002]),
        ("edge_back", [l, t, 0.004], [0.0, w / 2.0, 0.002]),
        ("edge_left", [t, w, 0.004], [-l / 2.0, 0.0, 0.002]),
        ("edge_right", [t, w, 0.004], [l / 2.0, 0.0, 0.002]),
        ("arrow", [l * 0.5, 0.05, 0.005], [0.0, 0.0, 0.003]),
        ("arrow_head", [0.08, 0.16, 0.005], [l * 0.25, 0.0, 0.003]),
    ] {
        block(doc, name, size, at, c.fence, s);
    }
    stop
}

/// A fence `l` × `w` round the origin: posts at most 1.3 m apart, and three
/// rails.
fn fence(doc: &mut Document, c: &Colours, l: f64, w: f64, h: f64) {
    let (hx, hy) = (l / 2.0, w / 2.0);
    let along = |n: f64| ((n / 1.3).ceil() as usize).max(1);
    let (nx, ny) = (along(l), along(w));
    for i in 0..=nx {
        let x = -hx + l * i as f64 / nx as f64;
        post(
            doc,
            &format!("post_s{i}"),
            0.03,
            h,
            [x, -hy, h / 2.0],
            c.fence,
            None,
        );
        post(
            doc,
            &format!("post_n{i}"),
            0.03,
            h,
            [x, hy, h / 2.0],
            c.fence,
            None,
        );
    }
    for j in 1..ny {
        let y = -hy + w * j as f64 / ny as f64;
        post(
            doc,
            &format!("post_w{j}"),
            0.03,
            h,
            [-hx, y, h / 2.0],
            c.fence,
            None,
        );
        post(
            doc,
            &format!("post_e{j}"),
            0.03,
            h,
            [hx, y, h / 2.0],
            c.fence,
            None,
        );
    }
    for (k, z) in [0.15, h / 2.0, h - 0.05].into_iter().enumerate() {
        block(
            doc,
            &format!("rail_n{k}"),
            [l, 0.03, 0.04],
            [0.0, hy, z],
            c.fence,
            None,
        );
        block(
            doc,
            &format!("rail_s{k}"),
            [l, 0.03, 0.04],
            [0.0, -hy, z],
            c.fence,
            None,
        );
        block(
            doc,
            &format!("rail_e{k}"),
            [0.03, w, 0.04],
            [hx, 0.0, z],
            c.fence,
            None,
        );
        block(
            doc,
            &format!("rail_w{k}"),
            [0.03, w, 0.04],
            [-hx, 0.0, z],
            c.fence,
            None,
        );
    }
}

/// The requests [`Library`] takes.
pub fn is_op(op: &str) -> bool {
    matches!(op, "place" | "move" | "resize" | "remove")
}

#[cfg(test)]
mod tests;
