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
use kreate_math::{DAffine3, DQuat, DVec3, SceneRgb};
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
    /// A choice rather than a size: what each number names.
    pub choices: &'static [&'static str],
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
        choices: &[],
    }
}

/// The tools a robot can carry on its flange, by number (see [`tool_urdf`]).
pub const TOOLS: &[&str] = &[
    "없음",
    "2지 그리퍼",
    "이중 그리퍼",
    "진공 그리퍼",
    "디버링 스핀들",
    "용접 토치",
];

/// The tool on a robot's flange: none for a bare arm, the dual gripper for
/// one made for tending.
const fn tool(default: usize) -> Param {
    Param {
        key: "tool",
        label: "툴",
        default: default as f64,
        min: 0.0,
        max: (TOOLS.len() - 1) as f64,
        choices: TOOLS,
    }
}

/// Everything the library holds. Sizes start at the roadmap's
/// industry-typical values (docs/roadmap-vs-visual-components.md §4).
pub const TEMPLATES: &[Template] = &[
    Template {
        key: "ur10e_tending",
        name: "UR10e + 이중 그리퍼",
        group: "로봇",
        params: &[tool(2)],
    },
    Template {
        key: "ur5e",
        name: "UR5e",
        group: "로봇",
        params: &[tool(0)],
    },
    Template {
        key: "ur3e",
        name: "UR3e",
        group: "로봇",
        params: &[tool(0)],
    },
    Template {
        key: "ur16e",
        name: "UR16e",
        group: "로봇",
        params: &[tool(0)],
    },
    Template {
        key: "irb1600_tending",
        name: "ABB IRB 1600 + 이중 그리퍼",
        group: "로봇",
        params: &[tool(2)],
    },
    Template {
        key: "irb1600_track",
        name: "ABB IRB 1600 + 주행 트랙",
        group: "로봇",
        params: &[tool(2)],
    },
    Template {
        key: "m1013_tending",
        name: "Doosan M1013 + 이중 그리퍼",
        group: "로봇",
        params: &[tool(2)],
    },
    Template {
        key: "lrmate200id7l_tending",
        name: "FANUC LR Mate 200iD/7L + 이중 그리퍼",
        group: "로봇",
        params: &[tool(2)],
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
        key: "cnc_vmc",
        name: "CNC 머시닝센터 (수직형)",
        group: "공작기계",
        // A vertical machining centre of the 800 mm-travel class.
        params: &[
            p("width", "폭", 2.4, 1.5, 4.0),
            p("depth", "깊이", 2.3, 1.5, 4.0),
            p("height", "높이", 2.6, 2.0, 3.5),
            p("opening", "문 폭", 0.9, 0.5, 1.8),
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
        key: "positioner",
        name: "포지셔너 (턴테이블)",
        group: "주변 장치",
        // A two-station turntable: parts loaded on one side while the
        // robot works on the other.
        params: &[
            p("diameter", "지름", 1.2, 0.5, 2.5),
            p("height", "높이", 0.8, 0.4, 1.2),
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

/// Equipment of the user's own, saved from the scene: its objects, parents
/// first, each where it stands from the equipment's origin (on the floor,
/// under where it stood, turned as it was), with its shape and colour if it
/// has one, and its role in the cell if it had one.
#[derive(Clone, Debug, PartialEq)]
pub struct Custom {
    pub key: String,
    pub name: String,
    nodes: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
struct Node {
    name: String,
    parent: Option<usize>,
    /// From the equipment's origin.
    at: DAffine3,
    shape: Option<Shape>,
    role: Option<(Kind, Value)>,
}

#[derive(Clone, Debug, PartialEq)]
struct Shape {
    positions: Vec<[f32; 3]>,
    triangles: Vec<[u32; 3]>,
    colour: [f32; 3],
}

impl Custom {
    fn to_json(&self) -> Value {
        json!({
            "key": self.key,
            "name": self.name,
            "nodes": self.nodes.iter().map(|n| json!({
                "name": n.name,
                "parent": n.parent,
                "at": n.at.to_cols_array(),
                "shape": n.shape.as_ref().map(|s| json!({
                    "p": s.positions.iter().flatten().collect::<Vec<_>>(),
                    "t": s.triangles.iter().flatten().collect::<Vec<_>>(),
                    "c": s.colour,
                })),
                "role": n.role.as_ref().map(|(k, props)| json!({"kind": k.tag(), "props": props})),
            })).collect::<Vec<_>>(),
        })
    }

    fn of_json(v: &Value) -> Option<Self> {
        let nodes = v["nodes"]
            .as_array()?
            .iter()
            .map(|n| {
                let at: Vec<f64> = n["at"]
                    .as_array()?
                    .iter()
                    .filter_map(Value::as_f64)
                    .collect();
                let shape = match &n["shape"] {
                    Value::Null => None,
                    s => {
                        let floats: Vec<f32> = s["p"]
                            .as_array()?
                            .iter()
                            .filter_map(|x| Some(x.as_f64()? as f32))
                            .collect();
                        let ints: Vec<u32> = s["t"]
                            .as_array()?
                            .iter()
                            .filter_map(|x| Some(x.as_u64()? as u32))
                            .collect();
                        let c: Vec<f32> = s["c"]
                            .as_array()?
                            .iter()
                            .filter_map(|x| Some(x.as_f64()? as f32))
                            .collect();
                        Some(Shape {
                            positions: floats.as_chunks::<3>().0.to_vec(),
                            triangles: ints.as_chunks::<3>().0.to_vec(),
                            colour: [*c.first()?, *c.get(1)?, *c.get(2)?],
                        })
                    }
                };
                let role = match &n["role"] {
                    Value::Null => None,
                    r => Some((Kind::of(r["kind"].as_str()?)?, r["props"].clone())),
                };
                Some(Node {
                    name: n["name"].as_str()?.to_string(),
                    parent: n["parent"].as_u64().map(|p| p as usize),
                    at: DAffine3::from_cols_array(&at.try_into().ok()?),
                    shape,
                    role,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            key: v["key"].as_str()?.to_string(),
            name: v["name"].as_str()?.to_string(),
            nodes,
        })
    }

    fn build(&self) -> Built {
        let mut doc = Document::new();
        let mut ids: Vec<ObjectId> = Vec::with_capacity(self.nodes.len());
        let mut roles = Vec::new();
        let mut props = Vec::new();
        let mut colours: Vec<([f32; 3], MaterialId)> = Vec::new();
        for n in &self.nodes {
            let id = match &n.shape {
                Some(s) => {
                    let positions = s
                        .positions
                        .iter()
                        .map(|p| kreate_math::Vec3::from_array(*p))
                        .collect();
                    let faces: Vec<Vec<u32>> = s.triangles.iter().map(|t| t.to_vec()).collect();
                    let id = doc.add_mesh_object(
                        &n.name,
                        kreate_mesh::MeshData::from_faces(positions, &faces),
                    );
                    let colour = match colours.iter().find(|(c, _)| *c == s.colour) {
                        Some((_, m)) => *m,
                        None => {
                            let m = doc.add_material(Material {
                                name: format!("custom {}", colours.len()),
                                base_color: SceneRgb::new(s.colour[0], s.colour[1], s.colour[2]),
                                ..Material::default()
                            });
                            colours.push((s.colour, m));
                            m
                        }
                    };
                    paint(&mut doc, id, colour);
                    id
                }
                None => doc.add_object(Object::new(n.name.clone(), ObjectData::Empty)),
            };
            let (scale, rotation, translation) = n.at.to_scale_rotation_translation();
            let o = doc.object_mut(id).unwrap();
            o.transform.translation = translation;
            o.transform.rotation = rotation;
            o.transform.scale = scale;
            // Where it stands is kept: set_parent keeps the world transform.
            if let Some(p) = n.parent.and_then(|p| ids.get(p).copied()) {
                let _ = doc.set_parent(id, Some(p));
            }
            if let Some((kind, v)) = &n.role {
                roles.push((id, *kind));
                props.push((id, v.clone()));
            }
            ids.push(id);
        }
        let order = doc.hierarchy().into_iter().map(|(id, _)| id).collect();
        Built {
            doc,
            order,
            roles,
            root_link: None,
            props,
        }
    }
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

// --- connection points ------------------------------------------------------

/// What a connection point takes: a robot's foot stands on a mount; a
/// conveyor's end feeds another's start.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Plug {
    /// Where a robot stands: a pedestal's top, an AMR's.
    Mount,
    /// A robot's foot: its base.
    Foot,
    /// A conveyor's start, and its end.
    BeltIn,
    BeltOut,
}

impl Plug {
    fn fits(self, other: Plug) -> bool {
        matches!(
            (self, other),
            (Plug::Foot, Plug::Mount)
                | (Plug::Mount, Plug::Foot)
                | (Plug::BeltIn, Plug::BeltOut)
                | (Plug::BeltOut, Plug::BeltIn)
        )
    }
}

/// How far apart, across the floor, two connection points snap together,
/// metres.
pub const SNAP: f64 = 0.25;

impl Custom {
    /// Its connection points, from its origin: the connectors among its
    /// parts.
    fn plugs(&self) -> Vec<(Plug, DVec3)> {
        self.nodes
            .iter()
            .filter_map(|n| {
                let (Kind::Connector, v) = n.role.as_ref()? else {
                    return None;
                };
                let p: Vec<f64> = v["point"]
                    .as_array()
                    .map(|a| a.iter().filter_map(Value::as_f64).collect())
                    .unwrap_or_default();
                let point = match p.as_slice() {
                    [x, y, z] => DVec3::new(*x, *y, *z),
                    _ => DVec3::ZERO,
                };
                let plug = match crate::cell::PLUGS.get(v["plug"].as_u64().unwrap_or(0) as usize) {
                    Some(&"belt_in") => Plug::BeltIn,
                    Some(&"belt_out") => Plug::BeltOut,
                    Some(&"foot") => Plug::Foot,
                    _ => Plug::Mount,
                };
                Some((plug, n.at.transform_point3(point)))
            })
            .collect()
    }
}

impl Item {
    /// Its connection points, where it stands: what each takes, and where.
    /// The user's own equipment, `customs`, has those its connectors say.
    fn plugs(&self, customs: &[Custom]) -> Vec<(Plug, DVec3)> {
        let local: Vec<(Plug, DVec3)> = match self.template.as_str() {
            "pedestal" => vec![(Plug::Mount, DVec3::Z * self.param("height"))],
            "amr" => vec![(Plug::Mount, DVec3::Z * self.param("height"))],
            "conveyor" => {
                let (l, h) = (self.param("length"), self.param("height"));
                // Past each end by the frame's overhang: ends meet frame to
                // frame.
                vec![
                    (Plug::BeltIn, DVec3::new(-l / 2.0 - 0.05, 0.0, h)),
                    (Plug::BeltOut, DVec3::new(l / 2.0 + 0.05, 0.0, h)),
                ]
            }
            t if template(t).is_some_and(|t| t.group == "로봇") => {
                vec![(Plug::Foot, DVec3::ZERO)]
            }
            t => customs
                .iter()
                .find(|c| c.key == t)
                .map(Custom::plugs)
                .unwrap_or_default(),
        };
        let turn = DQuat::from_rotation_z(self.yaw);
        local
            .into_iter()
            .map(|(plug, p)| (plug, self.at + turn * p))
            .collect()
    }

    /// Moved so its connection point `mine` stands on `at`, `other`'s: a
    /// robot on its mount, as it was turned; a conveyor on in line with
    /// the other, flowing the same way, its belt as high.
    fn snapped_to(&self, mine: Plug, other: &Item, at: DVec3, customs: &[Custom]) -> (DVec3, f64) {
        let yaw = match mine {
            Plug::Foot => self.yaw,
            _ => other.yaw,
        };
        let mut moved = self.clone();
        moved.yaw = yaw;
        moved.at = DVec3::ZERO;
        let local = moved
            .plugs(customs)
            .into_iter()
            .find(|(p, _)| *p == mine)
            .map_or(DVec3::ZERO, |(_, p)| p);
        (at - local, yaw)
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
    /// The settings a role comes with, where it has its own: the user's
    /// equipment's.
    props: Vec<(ObjectId, Value)>,
}

/// What a request changed in the cell's roles: added with the equipment,
/// gone with it.
#[derive(Debug, Default, PartialEq)]
pub struct RoleChanges {
    pub added: Vec<(u64, String, Kind)>,
    pub removed: Vec<u64>,
    /// Objects of equipment made again that are other objects now: their
    /// roles go before the equipment's own are given again.
    pub stale: Vec<u64>,
    /// The settings roles added come with, where they have their own.
    pub props: Vec<(u64, Value)>,
}

/// The placed equipment.
#[derive(Default)]
pub struct Library {
    items: Vec<Item>,
    /// The user's own equipment, as saved from the scene.
    customs: Vec<Custom>,
    /// The `templates` event as last made: made again when the user's own
    /// equipment changes.
    thumbs: Option<String>,
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
        let mut library = Self {
            customs: saved
                .iter()
                .filter_map(|v| Custom::of_json(&v["custom"]))
                .collect(),
            ..Self::default()
        };
        for v in saved {
            let (Some(id), Some(key)) = (v["id"].as_u64(), v["template"].as_str()) else {
                continue;
            };
            if template(key).is_none() && !library.customs.iter().any(|c| c.key == key) {
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
        self.customs
            .iter()
            .map(|c| json!({ "custom": c.to_json() }))
            .chain(self.items.iter().map(|i| {
                json!({
                    "id": i.id,
                    "template": i.template,
                    "name": i.name,
                    "params": i.params,
                    "at": [i.at.x, i.at.y, i.at.z],
                    "yaw": i.yaw,
                })
            }))
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
            let named = template(key)
                .map(|t| t.name.to_string())
                .or_else(|| {
                    self.customs
                        .iter()
                        .find(|c| c.key == key)
                        .map(|c| c.name.clone())
                })
                .ok_or("no such template")?;
            self.next += 1;
            let n = self.items.iter().filter(|i| i.template == key).count() + 1;
            let item = Item {
                id: self.next,
                template: key.to_string(),
                name: format!("{named} {n}"),
                params: params(&req["params"]),
                at: point(&req["at"]).unwrap_or(DVec3::ZERO),
                yaw: req["yaw"].as_f64().unwrap_or(0.0),
            };
            let mut item = item;
            let snapped = if req["snap"] == true {
                self.snap(&mut item)
            } else {
                None
            };
            let built = self.build(&item)?;
            roles.added = role_uids(&item, &built);
            roles.props = role_props(&item, &built);
            self.built.insert(item.id, (item.params.clone(), built));
            self.items.push(item);
            self.items_changed = true;
            return Ok(json!({ "ok": true, "item": self.next, "snapped": snapped }));
        }
        // Equipment of the user's own taken out of the library, if none
        // placed is of it.
        if op == "delete_custom" {
            let key = req["key"].as_str().ok_or("no key")?;
            if self.items.iter().any(|i| i.template == key) {
                return Err("놓여 있는 장비가 있습니다: 먼저 지우세요".into());
            }
            let before = self.customs.len();
            self.customs.retain(|c| c.key != key);
            if self.customs.len() == before {
                return Err("no such equipment".into());
            }
            self.thumbs = None;
            self.items_changed = true;
            return Ok(json!({ "ok": true }));
        }
        let id = req["item"].as_u64().ok_or("no item")?;
        let at = self
            .items
            .iter()
            .position(|i| i.id == id)
            .ok_or("no such item")?;
        match op {
            "move" => {
                let mut item = self.items[at].clone();
                if let Some(p) = point(&req["at"]) {
                    item.at = p;
                }
                if let Some(yaw) = req["yaw"].as_f64() {
                    item.yaw = yaw;
                }
                let snapped = if req["snap"] == true {
                    self.snap(&mut item)
                } else {
                    None
                };
                self.items[at] = item;
                self.items_changed = true;
                return Ok(json!({ "ok": true, "snapped": snapped }));
            }
            "resize" => {
                let mut item = self.items[at].clone();
                let before = self.made(&item).map(|b| named_uids(&item, b));
                for (k, v) in params(&req["params"]) {
                    item.params.insert(k, v);
                }
                self.items[at] = item.clone();
                // Made again, an identity may name another object now: a
                // grip point where a tool's working point is. Its role goes.
                if let (Ok(before), Ok(after)) =
                    (before, self.made(&item).map(|b| named_uids(&item, b)))
                {
                    roles.stale.extend(
                        before
                            .into_iter()
                            .filter(|old| !after.contains(old))
                            .map(|(uid, _)| uid),
                    );
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

    /// Moves `item` onto the connection point of another it is nearest,
    /// across the floor, if one fits within [`SNAP`]; returns that one's
    /// name.
    fn snap(&self, item: &mut Item) -> Option<String> {
        let flat = |v: DVec3| DVec3::new(v.x, v.y, 0.0).length();
        let customs = &self.customs;
        let (mine, other, theirs, _) = item
            .plugs(customs)
            .into_iter()
            .flat_map(|(mine, here)| {
                self.items
                    .iter()
                    .filter(|o| o.id != item.id)
                    .flat_map(move |o| {
                        o.plugs(customs)
                            .into_iter()
                            .filter(move |(theirs, _)| mine.fits(*theirs))
                            .map(move |(_, there)| (mine, o, there, flat(there - here)))
                    })
            })
            .filter(|(.., gap)| *gap <= SNAP)
            .min_by(|a, b| a.3.total_cmp(&b.3))?;
        let (at, yaw) = item.snapped_to(mine, other, theirs, customs);
        item.at = at;
        item.yaw = yaw;
        Some(other.name.clone())
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
            let built = self.build(item)?;
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
    /// triangles, for the page to draw its thumbnail by — the library's
    /// made once, the user's own as they are.
    pub fn thumbnails(&mut self) -> &str {
        let customs = &self.customs;
        self.thumbs.get_or_insert_with(|| {
            let mut out = Self::builtin_thumbnails().clone();
            for c in customs {
                out.insert(c.key.clone(), triangles(&c.build().doc));
            }
            Value::Object(out).to_string()
        })
    }

    fn builtin_thumbnails() -> &'static serde_json::Map<String, Value> {
        static THUMBNAILS: std::sync::OnceLock<serde_json::Map<String, Value>> =
            std::sync::OnceLock::new();
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
            out
        })
    }

    /// Item `item` as made: from the library's template, or the user's.
    fn build(&self, item: &Item) -> Result<Built, String> {
        match self.customs.iter().find(|c| c.key == item.template) {
            Some(c) => Ok(c.build()),
            None => build(item),
        }
    }

    /// Saves object `object` of `doc` and everything below it as equipment
    /// of the user's own, named `name`, with the roles `roles` they have
    /// (by object, with their settings). Returns its template's key.
    pub fn save_custom(
        &mut self,
        doc: &Document,
        object: u64,
        name: &str,
        roles: &[(ObjectId, Kind, Value)],
    ) -> Result<String, String> {
        let root = doc
            .object_by_uid(Uid::from_raw(object))
            .ok_or("no such object")?;
        let name = name.trim();
        if name.is_empty() {
            return Err("이름을 정하세요".into());
        }
        let under: Vec<ObjectId> = doc
            .hierarchy()
            .into_iter()
            .map(|(id, _)| id)
            .filter(|id| *id == root || crate::cell::is_below(doc, *id, root))
            .collect();
        if under
            .iter()
            .any(|id| doc.object(*id).is_some_and(|o| o.robot.is_some()))
        {
            return Err("로봇이 들어 있는 물체는 내 장비로 저장할 수 없습니다".into());
        }
        // Its origin: on the floor under where it stands, turned as it is.
        let world = doc.world_transform(root);
        let x = world.matrix3.x_axis;
        let origin = DAffine3::from_rotation_translation(
            DQuat::from_rotation_z(x.y.atan2(x.x)),
            DVec3::new(world.translation.x, world.translation.y, 0.0),
        );
        let mut nodes = Vec::new();
        for id in &under {
            let o = doc.object(*id).ok_or("no such object")?;
            let shape = o.mesh().and_then(|m| doc.mesh(m)).map(|mesh| Shape {
                positions: mesh.positions().iter().map(|p| p.to_array()).collect(),
                triangles: mesh.triangulate().indices.as_chunks::<3>().0.to_vec(),
                colour: doc.material_at(*id, 0).map_or([0.7, 0.72, 0.75], |m| {
                    [m.base_color.r, m.base_color.g, m.base_color.b]
                }),
            });
            let parent = o.parent.and_then(|p| under.iter().position(|u| *u == p));
            let role = roles
                .iter()
                .find(|(r, ..)| r == id)
                .map(|(_, kind, props)| (*kind, props.clone()));
            nodes.push(Node {
                name: o.name.clone(),
                parent: if *id == root { None } else { parent },
                at: origin.inverse() * doc.world_transform(*id),
                shape,
                role,
            });
        }
        let n = self.customs.len() + 1;
        let key = (n..)
            .map(|k| format!("custom_{k}"))
            .find(|k| !self.customs.iter().any(|c| c.key == *k))
            .unwrap();
        self.customs.push(Custom {
            key: key.clone(),
            name: name.chars().take(40).collect(),
            nodes,
        });
        self.thumbs = None;
        self.items_changed = true;
        Ok(key)
    }

    /// The `library` event: what can be placed, and what is.
    pub fn to_json(&self) -> String {
        json!({
            "templates": TEMPLATES.iter().map(|t| json!({
                "key": t.key,
                "name": t.name,
                "group": t.group,
                "params": t.params.iter().map(|p| json!({
                    "key": p.key, "label": p.label, "default": p.default, "min": p.min, "max": p.max, "choices": p.choices,
                })).collect::<Vec<_>>(),
            })).chain(self.customs.iter().map(|c| json!({
                "key": c.key,
                "name": c.name,
                "group": "내 장비",
                "params": [],
                "custom": true,
            }))).collect::<Vec<_>>(),
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

/// The settings the roles `built` comes with carry, by the identities their
/// objects take in the scene.
fn role_props(item: &Item, built: &Built) -> Vec<(u64, Value)> {
    let uids = role_uids(item, built);
    built
        .props
        .iter()
        .filter_map(|(id, v)| {
            let i = built.roles.iter().position(|(r, _)| r == id)?;
            Some((uids.get(i)?.0, v.clone()))
        })
        .collect()
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

/// Each object of `item` as made, by the identity it has in the scene, with
/// its name.
fn named_uids(item: &Item, built: &Built) -> Vec<(u64, String)> {
    built
        .order
        .iter()
        .enumerate()
        .filter_map(|(k, id)| {
            Some((
                item.root_uid() + 1 + k as u64,
                built.doc.object(*id)?.name.clone(),
            ))
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

/// A UR's joints at rest, the tool down.
const UR_REST: &[(&str, f64)] = &[
    ("shoulder_lift_joint", -1.75),
    ("elbow_joint", 1.75),
    ("wrist_1_joint", -1.571),
    ("wrist_2_joint", -1.571),
];

/// Tool `n` of [`TOOLS`] as URDF, hung from the flange `tool0`, in plain
/// shapes at industry-typical sizes. A gripper's grip points are links
/// named `grip_a` (and `grip_b`), where a part is held; a process tool's
/// working point is a link named `tcp`, its Z the way it points.
fn tool_urdf(n: usize) -> String {
    let body = r#"<material name="tool_body"><color rgba="0.40 0.42 0.46 1"/></material>
  <material name="tool_jaw"><color rgba="0.88 0.55 0.15 1"/></material>
  <material name="tool_cup"><color rgba="0.15 0.16 0.18 1"/></material>"#;
    let shapes = match TOOLS.get(n).copied() {
        // A parallel two-finger gripper, some 100 mm long, 85 mm stroke.
        Some("2지 그리퍼") => {
            r#"
  <joint name="tool0-gripper" type="fixed"><parent link="tool0"/><child link="gripper"/></joint>
  <link name="gripper">
    <visual><origin xyz="0 0 0.045"/><geometry><box size="0.08 0.12 0.09"/></geometry><material name="tool_body"/></visual>
    <visual><origin xyz="0 0.035 0.115"/><geometry><box size="0.03 0.015 0.05"/></geometry><material name="tool_jaw"/></visual>
    <visual><origin xyz="0 -0.035 0.115"/><geometry><box size="0.03 0.015 0.05"/></geometry><material name="tool_jaw"/></visual>
  </link>
  <joint name="gripper-grip_a" type="fixed"><parent link="gripper"/><child link="grip_a"/><origin xyz="0 0 0.125"/></joint>
  <link name="grip_a"/>"#
        }
        // Two grippers side by side: a raw part in, a finished one out, in
        // one visit to the machine.
        Some("이중 그리퍼") => {
            r#"
  <joint name="tool0-gripper" type="fixed"><parent link="tool0"/><child link="gripper"/></joint>
  <link name="gripper">
    <visual><origin xyz="0 0 0.04"/><geometry><box size="0.09 0.24 0.08"/></geometry><material name="tool_body"/></visual>
    <visual><origin xyz="0 0.07 0.105"/><geometry><box size="0.07 0.07 0.05"/></geometry><material name="tool_body"/></visual>
    <visual><origin xyz="0 -0.07 0.105"/><geometry><box size="0.07 0.07 0.05"/></geometry><material name="tool_body"/></visual>
    <visual><origin xyz="0.045 0.07 0.165"/><geometry><box size="0.015 0.03 0.07"/></geometry><material name="tool_jaw"/></visual>
    <visual><origin xyz="-0.045 0.07 0.165"/><geometry><box size="0.015 0.03 0.07"/></geometry><material name="tool_jaw"/></visual>
    <visual><origin xyz="0.045 -0.07 0.165"/><geometry><box size="0.015 0.03 0.07"/></geometry><material name="tool_jaw"/></visual>
    <visual><origin xyz="-0.045 -0.07 0.165"/><geometry><box size="0.015 0.03 0.07"/></geometry><material name="tool_jaw"/></visual>
  </link>
  <joint name="gripper-grip_a" type="fixed"><parent link="gripper"/><child link="grip_a"/><origin xyz="0 0.07 0.17"/></joint>
  <link name="grip_a"/>
  <joint name="gripper-grip_b" type="fixed"><parent link="gripper"/><child link="grip_b"/><origin xyz="0 -0.07 0.17"/></joint>
  <link name="grip_b"/>"#
        }
        // A plate with four suction cups, for flat parts and boxes.
        Some("진공 그리퍼") => {
            r#"
  <joint name="tool0-gripper" type="fixed"><parent link="tool0"/><child link="gripper"/></joint>
  <link name="gripper">
    <visual><origin xyz="0 0 0.03"/><geometry><cylinder radius="0.03" length="0.06"/></geometry><material name="tool_body"/></visual>
    <visual><origin xyz="0 0 0.0675"/><geometry><box size="0.2 0.15 0.015"/></geometry><material name="tool_body"/></visual>
    <visual><origin xyz="0.07 0.05 0.09"/><geometry><cylinder radius="0.02" length="0.03"/></geometry><material name="tool_cup"/></visual>
    <visual><origin xyz="-0.07 0.05 0.09"/><geometry><cylinder radius="0.02" length="0.03"/></geometry><material name="tool_cup"/></visual>
    <visual><origin xyz="0.07 -0.05 0.09"/><geometry><cylinder radius="0.02" length="0.03"/></geometry><material name="tool_cup"/></visual>
    <visual><origin xyz="-0.07 -0.05 0.09"/><geometry><cylinder radius="0.02" length="0.03"/></geometry><material name="tool_cup"/></visual>
  </link>
  <joint name="gripper-grip_a" type="fixed"><parent link="gripper"/><child link="grip_a"/><origin xyz="0 0 0.105"/></joint>
  <link name="grip_a"/>"#
        }
        // A compliant deburring spindle on the flange's axis, its burr's
        // tip 250 mm out.
        Some("디버링 스핀들") => {
            r#"
  <joint name="tool0-spindle" type="fixed"><parent link="tool0"/><child link="spindle"/></joint>
  <link name="spindle">
    <visual><origin xyz="0 0 0.01"/><geometry><cylinder radius="0.045" length="0.02"/></geometry><material name="tool_body"/></visual>
    <visual><origin xyz="0 0 0.12"/><geometry><cylinder radius="0.035" length="0.2"/></geometry><material name="tool_body"/></visual>
    <visual><origin xyz="0 0 0.235"/><geometry><cylinder radius="0.004" length="0.03"/></geometry><material name="tool_jaw"/></visual>
  </link>
  <joint name="spindle-tcp" type="fixed"><parent link="spindle"/><child link="tcp"/><origin xyz="0 0 0.25"/></joint>
  <link name="tcp"/>"#
        }
        // An arc welding torch: a neck on the flange, the gooseneck bent
        // 22° off it, 15 mm wire stick-out past the nozzle.
        Some("용접 토치") => {
            r#"
  <joint name="tool0-torch" type="fixed"><parent link="tool0"/><child link="torch"/></joint>
  <link name="torch">
    <visual><origin xyz="0 0 0.06"/><geometry><cylinder radius="0.025" length="0.12"/></geometry><material name="tool_body"/></visual>
    <visual><origin xyz="0.0468 0 0.2359" rpy="0 0.384 0"/><geometry><cylinder radius="0.015" length="0.25"/></geometry><material name="tool_body"/></visual>
    <visual><origin xyz="0.0993 0 0.3657" rpy="0 0.384 0"/><geometry><cylinder radius="0.009" length="0.03"/></geometry><material name="tool_jaw"/></visual>
  </link>
  <joint name="torch-tcp" type="fixed"><parent link="torch"/><child link="tcp"/><origin xyz="0.1105 0 0.3935" rpy="0 0.384 0"/></joint>
  <link name="tcp"/>"#
        }
        _ => "",
    };
    if shapes.is_empty() {
        String::new()
    } else {
        format!("  {body}{shapes}\n")
    }
}

fn build(item: &Item) -> Result<Built, String> {
    let mut doc = Document::new();
    let mut roles = Vec::new();
    let mut root_link = None;
    match item.template.as_str() {
        "ur10e_tending"
        | "ur5e"
        | "ur3e"
        | "ur16e"
        | "irb1600_tending"
        | "irb1600_track"
        | "m1013_tending"
        | "lrmate200id7l_tending" => {
            // Each at rest, the tool down: the pose a tending robot waits in.
            let (urdf, rest): (&str, &[(&str, f64)]) = match item.template.as_str() {
                "ur5e" => (include_str!("../library/ur5e.urdf"), UR_REST),
                "ur3e" => (include_str!("../library/ur3e.urdf"), UR_REST),
                "ur16e" => (include_str!("../library/ur16e.urdf"), UR_REST),
                "irb1600_tending" => (
                    include_str!("../library/irb1600_tending.urdf"),
                    &[("joint_5", std::f64::consts::FRAC_PI_2)],
                ),
                "irb1600_track" => (
                    include_str!("../library/irb1600_track.urdf"),
                    &[("joint_5", std::f64::consts::FRAC_PI_2)],
                ),
                // Doosan's: the forearm level, the tool down.
                "m1013_tending" => (
                    include_str!("../library/m1013_tending.urdf"),
                    &[
                        ("joint_3", std::f64::consts::FRAC_PI_2),
                        ("joint_5", std::f64::consts::FRAC_PI_2),
                    ],
                ),
                // FANUC's: the forearm level, the faceplate down.
                "lrmate200id7l_tending" => (
                    include_str!("../library/lrmate200id7l.urdf"),
                    &[("joint_5", -std::f64::consts::FRAC_PI_2)],
                ),
                _ => (include_str!("../library/ur10e_tending.urdf"), UR_REST),
            };
            // Its tool on its flange, `tool0`.
            let tool = tool_urdf(item.param("tool").round() as usize);
            let urdf = urdf.replacen("</robot>", &format!("{tool}</robot>"), 1);
            let urdf = urdf.as_str();
            let none = |_: &Path| -> Result<ImportScene, String> {
                Ok(ImportScene::new(SourceMetadata::new("none")))
            };
            let robot = kreate_io::urdf::parse_urdf(urdf, Path::new("."), &none)
                .map_err(|e| e.to_string())?;
            kreate_io::build::build_into(&robot, &mut doc);
            let named = |doc: &Document, n: &str| {
                doc.objects().find(|(_, o)| o.name == n).map(|(id, _)| id)
            };
            // The robot's root: its track's, if it rides one.
            root_link = named(&doc, "track_base").or_else(|| named(&doc, "base_link"));
            for grip in ["grip_a", "grip_b"] {
                if let Some(id) = named(&doc, grip) {
                    roles.push((id, Kind::Gripper));
                }
            }
            for &(joint, q) in rest {
                if let Some(id) = named(&doc, joint) {
                    if let Some(RobotPart::Joint(j)) = doc.object(id).and_then(|o| o.robot.clone())
                    {
                        doc.object_mut(id).unwrap().transform = j.place(q);
                    }
                }
            }
        }
        "cnc_lathe" | "cnc_vmc" => {
            let c = Colours::new(&mut doc);
            let machine = cnc(&mut doc, &c, item, item.template == "cnc_vmc");
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
        "positioner" => {
            let c = Colours::new(&mut doc);
            let positioner = turntable(&mut doc, &c, item.param("diameter"), item.param("height"));
            roles.push((positioner, Kind::Positioner));
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
        props: Vec::new(),
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
fn cnc(doc: &mut Document, c: &Colours, item: &Item, vise: bool) -> ObjectId {
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
    if vise {
        // A machining centre: its table, and the vise on it the part is
        // clamped in (where a lathe has its chuck).
        let table = (opening - 0.1).min(0.9);
        block(
            doc,
            "table",
            [table, 0.45, 0.05],
            [0.0, 0.45, floor + 0.025],
            c.steel,
            m,
        );
        block(
            doc,
            "chuck",
            [0.25, 0.15, 0.1],
            [0.0, 0.45, floor + 0.1],
            c.dark,
            m,
        );
    } else {
        post(
            doc,
            "chuck",
            0.09,
            0.12,
            [0.0, 0.35, floor + 0.06],
            c.steel,
            m,
        );
    }
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

/// A two-station turntable `d` across, its plate `h` high: a round base,
/// the plate (`top`) turning on it, and a screen across the plate between
/// its two stations.
fn turntable(doc: &mut Document, c: &Colours, d: f64, h: f64) -> ObjectId {
    let table = doc.add_object(Object::new("positioner", ObjectData::Empty));
    let m = Some(table);
    post(
        doc,
        "base",
        d * 0.35,
        h - 0.03,
        [0.0, 0.0, (h - 0.03) / 2.0],
        c.dark,
        m,
    );
    post(doc, "top", d / 2.0, 0.03, [0.0, 0.0, h - 0.015], c.table, m);
    block(
        doc,
        "screen",
        [0.02, d * 0.9, 0.5],
        [0.0, 0.0, h + 0.25],
        c.accent,
        m,
    );
    table
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
    matches!(op, "place" | "move" | "resize" | "remove" | "delete_custom")
}

#[cfg(test)]
mod tests;
