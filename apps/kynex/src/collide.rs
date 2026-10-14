//! Collisions in the scene: each robot's links against everything else,
//! and against its own links, now and along its program.
//!
//! What a robot collides with is its collision meshes where it has them
//! (`RobotPart::Collision`, hidden, as a URDF gives them), and otherwise
//! its visible meshes. Everything else is its visible meshes. Two links of
//! one robot that a single joint connects are not checked against each
//! other: they always meet at the joint.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use kreate_math::{Aabb, DVec3};
use kreate_scene::robot::RobotPart;
use kreate_scene::{Changes, DatablockKind, Document, Object, ObjectData, ObjectId};
use kynex_collide::{contact, Body, Obstacles, Shape};
use kynex_kin::path::Plan;
use kynex_kin::Kinematics;
use serde_json::{json, Value};

/// How finely a program's path is checked, seconds: at a linear move's
/// 250 mm/s, every 5 mm.
pub const PATH_STEP: f64 = 0.02;

/// Contact found: a robot's part, and what it runs into.
#[derive(Clone, Debug, PartialEq)]
pub struct Contact {
    pub robot: u64,
    pub part: u64,
    pub other: u64,
    pub at: DVec3,
}

/// Keeps each mesh's shape between checks: made once, until the mesh
/// changes.
#[derive(Default)]
pub struct Collider {
    shapes: HashMap<u64, Arc<Shape>>,
}

/// A robot's colliding parts at one pose, each with the joint it rides on.
type Posed = Vec<(Body, Option<u64>)>;

/// A robot's parts that collide, and which of them may meet.
struct Robot {
    root: u64,
    kin: Kinematics,
    /// Each part, by identity: its place in the robot's tree, its shape
    /// and the joint it rides on.
    parts: Vec<(u64, usize, Arc<Shape>, Option<u64>)>,
    /// Each moving joint's own carrier, the next in towards the root.
    up: HashMap<u64, Option<u64>>,
}

impl Robot {
    /// Whether two parts riding on `a` and `b` are checked against each
    /// other: not when one joint joins them, or none.
    fn may_meet(&self, a: Option<u64>, b: Option<u64>) -> bool {
        let up = |c: Option<u64>| c.map(|j| self.up.get(&j).copied().flatten());
        a != b && up(a) != Some(b) && up(b) != Some(a)
    }
}

impl Collider {
    /// Lets go of the shapes of meshes that changed.
    pub fn forget(&mut self, changes: &Changes) {
        for (kind, uid) in &changes.changed {
            if *kind == DatablockKind::Mesh {
                self.shapes.remove(&uid.raw());
            }
        }
    }

    pub fn clear(&mut self) {
        self.shapes.clear();
    }

    fn shape(&mut self, doc: &Document, object: &Object) -> Option<Arc<Shape>> {
        let ObjectData::Mesh(mesh) = object.data else {
            return None;
        };
        let uid = doc.mesh_uid(mesh)?.raw();
        if let Some(shape) = self.shapes.get(&uid) {
            return Some(shape.clone());
        }
        let shape = Arc::new(Shape::of(doc.mesh(mesh)?));
        if shape.triangles() == 0 {
            return None;
        }
        self.shapes.insert(uid, shape.clone());
        Some(shape)
    }

    /// The robots in `doc`, each with the parts it collides with. Found
    /// from their root links: a scene with none costs a look at each object
    /// and nothing more.
    fn robots(&mut self, doc: &Document) -> Vec<Robot> {
        let roots: Vec<u64> = doc
            .objects()
            .filter(|(id, o)| {
                matches!(o.robot, Some(RobotPart::Link { .. })) && doc.robot_root(*id) == Some(*id)
            })
            .filter_map(|(id, _)| Some(doc.object_uid(id)?.raw()))
            .collect();
        if roots.is_empty() {
            return Vec::new();
        }
        let ids: HashMap<u64, ObjectId> = doc
            .objects()
            .filter_map(|(id, _)| Some((doc.object_uid(id)?.raw(), id)))
            .collect();
        let mut robots = Vec::new();
        for root in roots {
            let Some(kin) = Kinematics::of(doc, root) else {
                continue;
            };
            let uids: Vec<u64> = kin.uids().collect();
            let carriers = kin.carriers();
            let object = |uid: u64| ids.get(&uid).and_then(|id| doc.object(*id));
            let has_collision = uids.iter().any(|u| {
                object(*u).is_some_and(|o| {
                    matches!(o.robot, Some(RobotPart::Collision))
                        && matches!(o.data, ObjectData::Mesh(_))
                })
            });
            let mut parts = Vec::new();
            for (at, (&uid, carrier)) in uids.iter().zip(&carriers).enumerate() {
                let Some(o) = object(uid) else { continue };
                let collision = matches!(o.robot, Some(RobotPart::Collision));
                let wanted = if has_collision {
                    collision
                } else {
                    o.visible && !collision
                };
                if !wanted {
                    continue;
                }
                if let Some(shape) = self.shape(doc, o) {
                    parts.push((uid, at, shape, *carrier));
                }
            }
            let up = kin.joints().map(|j| (j, kin.carrier_of_joint(j))).collect();
            robots.push(Robot {
                root,
                kin,
                parts,
                up,
            });
        }
        robots
    }

    /// What may be run into within `region`, where `doc` has it: every
    /// visible mesh that is no robot's, and the robots' colliding parts.
    /// Only those whose boxes reach the region: a factory's far stations
    /// are not looked at past their boxes.
    fn obstacles(
        &mut self,
        doc: &Document,
        robots: &[Robot],
        region: Aabb,
        ignore: &HashSet<u64>,
    ) -> Obstacles {
        let mut in_robot: HashSet<u64> = HashSet::new();
        let mut colliding: HashSet<u64> = HashSet::new();
        for robot in robots {
            in_robot.extend(robot.kin.uids());
            colliding.extend(robot.parts.iter().map(|p| p.0));
        }
        let mut bodies = Vec::new();
        if region.is_empty() {
            return Obstacles::new(bodies);
        }
        for (id, object) in doc.objects() {
            if !matches!(object.data, ObjectData::Mesh(_)) {
                continue;
            }
            let Some(uid) = doc.object_uid(id).map(|u| u.raw()) else {
                continue;
            };
            let wanted = if in_robot.contains(&uid) {
                colliding.contains(&uid)
            } else {
                object.visible
            };
            if !wanted || ignore.contains(&uid) {
                continue;
            }
            let Some(shape) = self.shape(doc, object) else {
                continue;
            };
            let body = Body {
                uid,
                shape,
                world: doc.world_transform(id),
            };
            if body.bounds().intersects(&region) {
                bodies.push(body);
            }
        }
        Obstacles::new(bodies)
    }

    /// Every contact of a robot's part in `doc` as it stands, but with what
    /// `ignore` names: parts a gripper holds, which move as its own.
    pub fn contacts(&mut self, doc: &Document, ignore: &HashSet<u64>) -> Vec<Contact> {
        let robots = self.robots(doc);
        if robots.is_empty() {
            return Vec::new();
        }
        let posed: Vec<Posed> = robots.iter().map(|r| r.bodies(&r.kin.current)).collect();
        let region = posed
            .iter()
            .flatten()
            .fold(Aabb::EMPTY, |b, (part, _)| b.union(part.bounds()));
        let obstacles = self.obstacles(doc, &robots, region, ignore);
        let mut out = Vec::new();
        for (robot, parts) in robots.iter().zip(&posed) {
            let own: HashSet<u64> = robot.kin.uids().collect();
            for (part, _) in parts {
                for (other, at) in obstacles.contacts(part, |u| own.contains(&u)) {
                    out.push(Contact {
                        robot: robot.root,
                        part: part.uid,
                        other,
                        at,
                    });
                }
            }
            out.extend(robot.own_contacts(parts, usize::MAX));
        }
        out
    }

    /// The first contact in each move of robot `root`'s plan, the rest of
    /// the scene standing as `doc` has it: seconds into the move, and the
    /// contact.
    ///
    /// `open` gives, move by move, what is out of the way during it: the
    /// doors of machines the program has opened by then. A move past the
    /// list has nothing out of the way.
    pub fn along(
        &mut self,
        doc: &Document,
        root: u64,
        plan: &Plan,
        open: &[HashSet<u64>],
    ) -> Vec<Option<(f64, Contact)>> {
        let robots = self.robots(doc);
        let Some(robot) = robots.iter().find(|r| r.root == root) else {
            return vec![None; plan.segments.len()];
        };
        // Every sample of every move, posed: the space they sweep is all the
        // rest of the scene that needs looking at.
        let samples: Vec<Vec<(f64, Posed)>> = plan
            .segments
            .iter()
            .map(|seg| {
                let steps = ((seg.duration / PATH_STEP).ceil() as usize).max(1);
                (0..=steps)
                    .map(|k| {
                        let t = seg.duration * k as f64 / steps as f64;
                        (t, robot.bodies(&plan.at(seg.start + t)))
                    })
                    .collect()
            })
            .collect();
        let region = samples
            .iter()
            .flatten()
            .flat_map(|(_, parts)| parts)
            .fold(Aabb::EMPTY, |b, (part, _)| b.union(part.bounds()));
        let own: HashSet<u64> = robot.kin.uids().collect();
        let obstacles = self.obstacles(doc, &robots, region, &HashSet::new());
        let none = HashSet::new();
        samples
            .iter()
            .enumerate()
            .map(|(i, seg)| {
                let away = open.get(i).unwrap_or(&none);
                seg.iter().find_map(|(t, parts)| {
                    let hit = parts.iter().find_map(|(part, _)| {
                        let (other, at) = obstacles
                            .contacts(part, |u| own.contains(&u) || away.contains(&u))
                            .into_iter()
                            .next()?;
                        Some(Contact {
                            robot: root,
                            part: part.uid,
                            other,
                            at,
                        })
                    });
                    hit.or_else(|| robot.own_contacts(parts, 1).into_iter().next())
                        .map(|c| (*t, c))
                })
            })
            .collect()
    }
}

impl Robot {
    /// Its colliding parts at positions `q`, each with the joint it rides on.
    fn bodies(&self, q: &[f64]) -> Posed {
        let worlds = self.kin.forward(q);
        self.parts
            .iter()
            .map(|(uid, at, shape, carrier)| {
                (
                    Body {
                        uid: *uid,
                        shape: shape.clone(),
                        world: worlds[*at],
                    },
                    *carrier,
                )
            })
            .collect()
    }

    /// Its parts in contact with each other, up to `most` of them.
    fn own_contacts(&self, parts: &[(Body, Option<u64>)], most: usize) -> Vec<Contact> {
        let mut out = Vec::new();
        for (i, (a, ca)) in parts.iter().enumerate() {
            for (b, cb) in &parts[i + 1..] {
                if out.len() >= most {
                    return out;
                }
                if self.may_meet(*ca, *cb) {
                    if let Some(at) = contact(a, b) {
                        out.push(Contact {
                            robot: self.root,
                            part: a.uid,
                            other: b.uid,
                            at,
                        });
                    }
                }
            }
        }
        out
    }
}

/// What the viewer calls an object: the link it belongs to, for a robot's
/// part, or its own name.
pub fn label(doc: &Document, uid: u64) -> String {
    let Some(id) = doc.object_by_uid(kreate_scene::Uid::from_raw(uid)) else {
        return String::new();
    };
    let own = doc.object(id).map(|o| o.name.clone()).unwrap_or_default();
    let mut at = Some(id);
    for _ in 0..=doc.object_count() {
        let Some(here) = at else { break };
        let Some(object) = doc.object(here) else {
            break;
        };
        if matches!(object.robot, Some(RobotPart::Link { .. })) {
            return object.name.clone();
        }
        at = object.parent;
    }
    own
}

/// The `contacts` event: every contact, with names to show.
pub fn contacts_json(doc: &Document, contacts: &[Contact]) -> String {
    Value::Array(
        contacts
            .iter()
            .map(|c| {
                json!({
                    "robot": c.robot.to_string(),
                    "part": c.part.to_string(),
                    "other": c.other.to_string(),
                    "part_name": label(doc, c.part),
                    "other_name": label(doc, c.other),
                    "at": [c.at.x, c.at.y, c.at.z],
                })
            })
            .collect(),
    )
    .to_string()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use kreate_io::ir::{ImportScene, SourceMetadata};
    use kreate_math::DAffine3;
    use kynex_kin::path::{self, MoveKind, Waypoint};

    use super::*;

    /// A base, and an arm a metre long turning about Z half a metre up. The
    /// base's block reaches up past the joint, into the arm's root.
    const SWING: &str = r#"<?xml version="1.0"?>
<robot name="swing">
  <link name="base">
    <visual><origin xyz="0 0 0.3"/><geometry><box size="0.3 0.3 0.6"/></geometry></visual>
  </link>
  <joint name="turn" type="revolute">
    <parent link="base"/><child link="arm"/>
    <origin xyz="0 0 0.5"/><axis xyz="0 0 1"/>
    <limit lower="-3.2" upper="3.2" effort="10" velocity="1"/>
  </joint>
  <link name="arm">
    <visual><origin xyz="0.5 0 0"/><geometry><box size="1 0.1 0.1"/></geometry></visual>
  </link>
  <joint name="arm-tool0" type="fixed">
    <parent link="arm"/><child link="tool0"/><origin xyz="1 0 0"/>
  </joint>
  <link name="tool0"/>
</robot>
"#;

    /// The arm, and a block in its way a quarter turn round.
    fn scene() -> (Document, u64, u64, u64) {
        let mut doc = Document::new();
        let none = |_: &Path| -> Result<ImportScene, String> {
            Ok(ImportScene::new(SourceMetadata::new("none")))
        };
        let robot = kreate_io::urdf::parse_urdf(SWING, Path::new("."), &none).unwrap();
        kreate_io::build::build_into(&robot, &mut doc);
        let block = doc.add_mesh_object("block", kreate_mesh::primitives::cube(0.2));
        doc.object_mut(block).unwrap().transform.translation = DVec3::new(0.0, 1.0, 0.5);
        let uid = |name: &str| {
            let id = doc.objects().find(|(_, o)| o.name == name).unwrap().0;
            doc.object_uid(id).unwrap().raw()
        };
        let (root, joint, tip) = (uid("base"), uid("turn"), uid("tool0"));
        (doc, root, joint, tip)
    }

    fn turned(doc: &Document, root: u64, angle: f64) -> Document {
        let mut doc = doc.clone();
        let kin = Kinematics::of(&doc, root).unwrap();
        kin.apply(&mut doc, &[angle]);
        doc
    }

    #[test]
    fn a_link_in_a_block_is_a_contact_and_its_own_joint_is_not() {
        let (doc, root, _, _) = scene();
        let mut collider = Collider::default();
        // The base's block and the arm overlap at the joint: never reported.
        assert!(
            collider.contacts(&doc, &HashSet::new()).is_empty(),
            "{:?}",
            collider.contacts(&doc, &HashSet::new())
        );
        let into = turned(&doc, root, std::f64::consts::FRAC_PI_2);
        let found = collider.contacts(&into, &HashSet::new());
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].robot, root);
        assert_eq!(label(&into, found[0].part), "arm");
        assert_eq!(label(&into, found[0].other), "block");
        assert!((found[0].at - DVec3::new(0.0, 0.95, 0.5)).length() < 0.15);
        let json: Value = serde_json::from_str(&contacts_json(&into, &found)).unwrap();
        assert_eq!(json[0]["part_name"], "arm");
        assert_eq!(json[0]["other_name"], "block");
    }

    #[test]
    fn a_program_through_the_block_is_caught_where_it_gets_there() {
        let (doc, root, _, tip) = scene();
        let kin = Kinematics::of(&doc, root).unwrap();
        let tool = kin.tool(tip, DAffine3::IDENTITY).unwrap();
        let w = |q: f64| Waypoint {
            kind: MoveKind::Joint,
            positions: vec![q],
            speed: 1.0,
            blend: 0.0,
        };
        // Most of a half turn: through the block. Then a little back, clear
        // of it.
        let plan = path::plan(&kin, &tool, &[w(0.0), w(3.0), w(2.0)], false);
        let hits = Collider::default().along(&doc, root, &plan, &[]);
        assert_eq!(hits.len(), 2);
        let (t, c) = hits[0].as_ref().expect("the first move hits the block");
        assert_eq!(label(&doc, c.other), "block");
        // The arm's end reaches the block's near side a little short of a
        // quarter turn.
        let angle = plan.at(plan.segments[0].start + t)[0];
        assert!(angle > 1.3 && angle < 1.5, "{angle} rad");
        assert!(hits[1].is_none(), "{:?}", hits[1]);
    }
}
