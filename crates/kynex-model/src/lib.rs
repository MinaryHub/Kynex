//! KYNEX's view of a scene: bodies where they stand in the world, and robots
//! as kinematic trees of links and joints.
//!
//! Built from a Kreate [`Document`], read as it is — a joint's kind, axis and
//! limits, a link's inertia and a body's rigid-body settings are Kreate's own
//! records, so what a person sets in Kreate is what the simulation gets.
//!
//! The model is derived, never edited: when the document changes, build a new
//! one and [`Model::diff`] the two.

use std::collections::BTreeMap;

use kreate_math::{DQuat, DVec3};
use kreate_scene::physics::{BodyKind, CollisionShape};
use kreate_scene::robot::{JointKind, RobotPart};
use kreate_scene::{Document, ObjectData, ObjectId};

/// A placement in the world: metres, Z up, as Kreate keeps it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub translation: DVec3,
    pub rotation: DQuat,
    pub scale: DVec3,
}

/// What a body is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Empty,
    Mesh { verts: u32, faces: u32 },
    Camera,
    Light,
    Armature,
}

/// What a body is to a robot, if anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Link,
    Joint,
    Collision,
    Rig,
}

/// A body's rigid-body settings, as set in Kreate's Physics panel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rigid {
    /// `false` for a passive body: a floor, a fixture.
    pub dynamic: bool,
    pub shape: CollisionShape,
    /// Kilograms.
    pub mass: f64,
    pub friction: f64,
    pub bounciness: f64,
}

/// One object of the scene.
#[derive(Clone, Debug, PartialEq)]
pub struct Body {
    pub uid: u64,
    pub name: String,
    pub parent: Option<u64>,
    pub world: Pose,
    pub shape: Shape,
    pub visible: bool,
    pub role: Option<Role>,
    pub rigid: Option<Rigid>,
}

/// One joint of a robot, where it stands now.
#[derive(Clone, Debug, PartialEq)]
pub struct JointState {
    pub uid: u64,
    pub name: String,
    pub kind: JointKind,
    /// Radians, or metres for a prismatic joint.
    pub position: f64,
    pub lower: f64,
    pub upper: f64,
    pub effort: f64,
    pub velocity: f64,
    /// The joint's origin and axis, in the world.
    pub origin: DVec3,
    pub axis: DVec3,
    /// The links it joins, by identity.
    pub parent_link: Option<u64>,
    pub child_link: Option<u64>,
}

impl JointState {
    /// Whether the joint stands outside its limits.
    pub fn out_of_limits(&self) -> bool {
        self.kind.limited()
            && self.lower <= self.upper
            && (self.position < self.lower - 1e-9 || self.position > self.upper + 1e-9)
    }
}

/// A robot: a root link and the joints under it, in the order the tree
/// reaches them from the root.
#[derive(Clone, Debug, PartialEq)]
pub struct Robot {
    pub root: u64,
    pub name: String,
    pub joints: Vec<JointState>,
    /// The sum of its links' masses, where the description gives them.
    pub mass: f64,
    /// The link its tool hangs from, by identity: see [`TOOL_NAMES`].
    pub tool: Option<u64>,
}

impl Robot {
    /// Whose controller the robot is, as far as its description tells:
    /// `"ur"` for Universal Robots (`ur_description` names its first joint
    /// `shoulder_pan_joint`), `"abb"` for ABB (an IRB, by its name),
    /// `"doosan"` for Doosan Robotics (by its name, or a model's: M1013,
    /// H2017, A0912 and the like). `None` when it does not say.
    pub fn vendor(&self) -> Option<&'static str> {
        let name = self.name.to_lowercase();
        if self.joints.iter().any(|j| j.name == "shoulder_pan_joint") {
            Some("ur")
        } else if name.contains("irb") || name.contains("abb") {
            Some("abb")
        } else if name.contains("doosan") || doosan_model(&name) {
            Some("doosan")
        } else {
            None
        }
    }
}

/// Whether `name` starts with a Doosan model's: M, H or A and four digits.
fn doosan_model(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() >= 5 && matches!(b[0], b'm' | b'h' | b'a') && b[1..5].iter().all(u8::is_ascii_digit)
}

/// Names a robot description gives the link a tool is fitted to, the most
/// telling first. A robot with none of them uses its link the most moving
/// joints out from the root, the first such.
pub const TOOL_NAMES: [&str; 6] = ["tcp", "tool0", "tool", "flange", "ee_link", "end_effector"];

/// A scene as KYNEX simulates it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Model {
    /// Every object, by identity.
    pub bodies: BTreeMap<u64, Body>,
    pub robots: Vec<Robot>,
}

/// What differs between two models, by identity.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Diff {
    pub added: Vec<u64>,
    pub changed: Vec<u64>,
    pub removed: Vec<u64>,
    /// Robots whose joints moved or whose structure changed, by root.
    pub robots: Vec<u64>,
}

impl Diff {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty()
            && self.changed.is_empty()
            && self.removed.is_empty()
            && self.robots.is_empty()
    }
}

impl Model {
    pub fn from_document(doc: &Document) -> Self {
        let uid = |id: ObjectId| doc.object_uid(id).map(|u| u.raw());
        let mut bodies = BTreeMap::new();
        for (id, object) in doc.objects() {
            let Some(own) = uid(id) else { continue };
            let (scale, rotation, translation) =
                doc.world_transform(id).to_scale_rotation_translation();
            let shape = match object.data {
                ObjectData::Empty => Shape::Empty,
                ObjectData::Mesh(mesh) => doc.mesh(mesh).map_or(Shape::Empty, |m| Shape::Mesh {
                    verts: m.num_verts(),
                    faces: m.num_faces(),
                }),
                ObjectData::Camera(_) => Shape::Camera,
                ObjectData::Light(_) => Shape::Light,
                ObjectData::Armature(_) => Shape::Armature,
            };
            let role = object.robot.as_ref().map(|part| match part {
                RobotPart::Link { .. } => Role::Link,
                RobotPart::Joint(_) => Role::Joint,
                RobotPart::Collision => Role::Collision,
                RobotPart::Rig { .. } => Role::Rig,
            });
            let rigid = object.physics.rigid.map(|r| Rigid {
                dynamic: r.kind == BodyKind::Active,
                shape: r.shape,
                mass: r.mass,
                friction: r.friction,
                bounciness: r.bounciness,
            });
            bodies.insert(
                own,
                Body {
                    uid: own,
                    name: object.name.clone(),
                    parent: object.parent.and_then(uid),
                    world: Pose {
                        translation,
                        rotation,
                        scale,
                    },
                    shape,
                    visible: object.visible,
                    role,
                    rigid,
                },
            );
        }
        Self {
            bodies,
            robots: robots(doc),
        }
    }

    pub fn robot(&self, root: u64) -> Option<&Robot> {
        self.robots.iter().find(|r| r.root == root)
    }

    /// What `newer` has that this does not, has differently, or lacks.
    pub fn diff(&self, newer: &Model) -> Diff {
        let mut diff = Diff::default();
        for (uid, body) in &newer.bodies {
            match self.bodies.get(uid) {
                None => diff.added.push(*uid),
                Some(old) if old != body => diff.changed.push(*uid),
                Some(_) => {}
            }
        }
        diff.removed = self
            .bodies
            .keys()
            .filter(|uid| !newer.bodies.contains_key(uid))
            .copied()
            .collect();
        diff.robots = newer
            .robots
            .iter()
            .filter(|r| self.robot(r.root) != Some(*r))
            .map(|r| r.root)
            .collect();
        diff
    }
}

/// Every robot in `doc`: each link with no link above it is a root.
pub fn robots(doc: &Document) -> Vec<Robot> {
    let uid = |id: ObjectId| doc.object_uid(id).map(|u| u.raw());
    let mut children: BTreeMap<ObjectId, Vec<ObjectId>> = BTreeMap::new();
    for (id, object) in doc.objects() {
        if let Some(parent) = object.parent {
            children.entry(parent).or_default().push(id);
        }
    }
    // Document order, so the same scene always lists its children alike.
    let order: Vec<ObjectId> = doc.hierarchy().into_iter().map(|(id, _)| id).collect();
    let rank: BTreeMap<ObjectId, usize> =
        order.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    for list in children.values_mut() {
        list.sort_by_key(|id| rank.get(id).copied().unwrap_or(usize::MAX));
    }

    let mut out = Vec::new();
    for &root in &order {
        let Some(object) = doc.object(root) else {
            continue;
        };
        if !matches!(object.robot, Some(RobotPart::Link { .. }))
            || doc.robot_root(root) != Some(root)
        {
            continue;
        }
        let Some(root_uid) = uid(root) else { continue };
        let mut robot = Robot {
            root: root_uid,
            name: object.name.clone(),
            joints: Vec::new(),
            mass: 0.0,
            tool: None,
        };
        // The tool so far: by name, then by how many moving joints out.
        let mut tool: Option<(usize, u32, u64)> = None;
        // Depth first from the root, so a joint comes after the one it hangs from.
        let mut stack = vec![(root, 0u32)];
        let mut seen = 0usize;
        while let Some((at, depth)) = stack.pop() {
            seen += 1;
            if seen > doc.object_count() {
                break;
            }
            let Some(o) = doc.object(at) else { continue };
            let mut below = depth;
            match &o.robot {
                Some(RobotPart::Link { inertial }) => {
                    robot.mass += inertial.as_ref().map_or(0.0, |i| i.mass);
                    let lower = o.name.to_lowercase();
                    let named = TOOL_NAMES
                        .iter()
                        .position(|n| lower == *n)
                        .unwrap_or(TOOL_NAMES.len());
                    if let Some(own) = uid(at) {
                        let better =
                            tool.is_none_or(|(n, d, _)| named < n || (named == n && depth > d));
                        if better {
                            tool = Some((named, depth, own));
                        }
                    }
                }
                Some(RobotPart::Joint(joint)) => {
                    if joint.kind.moves() && joint.mimic.is_none() {
                        below += 1;
                    }
                    let world = doc.world_transform(at);
                    let child_link = children.get(&at).and_then(|list| {
                        list.iter()
                            .copied()
                            .find(|c| {
                                matches!(
                                    doc.object(*c).and_then(|c| c.robot.as_ref()),
                                    Some(RobotPart::Link { .. })
                                )
                            })
                            .and_then(uid)
                    });
                    robot.joints.push(JointState {
                        uid: uid(at).unwrap_or(0),
                        name: o.name.clone(),
                        kind: joint.kind,
                        position: joint.position_of(&o.transform),
                        lower: joint.lower,
                        upper: joint.upper,
                        effort: joint.effort,
                        velocity: joint.velocity,
                        origin: world.translation,
                        axis: world.matrix3.mul_vec3(DVec3::Z).normalize_or_zero(),
                        parent_link: o.parent.and_then(uid),
                        child_link,
                    });
                }
                _ => {}
            }
            if let Some(list) = children.get(&at) {
                stack.extend(list.iter().rev().map(|c| (*c, below)));
            }
        }
        robot.tool = tool.map(|(_, _, uid)| uid);
        out.push(robot);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use kreate_math::DTransform;
    use kreate_scene::robot::{Inertial, Joint};
    use kreate_scene::Object;

    /// A base, one revolute joint 1 m up turning about world Z, and an arm.
    pub(crate) fn arm(doc: &mut Document) -> (ObjectId, ObjectId, ObjectId) {
        let mut base = Object::new("base", ObjectData::Empty);
        base.robot = Some(RobotPart::Link {
            inertial: Some(Inertial {
                origin: DTransform::IDENTITY,
                mass: 4.0,
                inertia: [0.1, 0.0, 0.0, 0.1, 0.0, 0.1],
            }),
        });
        let base = doc.add_object(base);
        let rest = DTransform {
            translation: DVec3::new(0.0, 0.0, 1.0),
            ..DTransform::IDENTITY
        };
        let mut joint = Object::new("shoulder", ObjectData::Empty);
        joint.transform = rest;
        joint.robot = Some(RobotPart::Joint(Joint {
            kind: JointKind::Revolute,
            rest,
            lower: -1.0,
            upper: 1.0,
            effort: 50.0,
            velocity: 2.0,
            mimic: None,
        }));
        let joint = doc.add_object(joint);
        doc.set_parent(joint, Some(base)).unwrap();
        let mut link = Object::new("upper_arm", ObjectData::Empty);
        link.transform.translation = DVec3::new(0.0, 0.0, 1.0);
        link.robot = Some(RobotPart::Link {
            inertial: Some(Inertial {
                origin: DTransform::IDENTITY,
                mass: 2.0,
                inertia: [0.1, 0.0, 0.0, 0.1, 0.0, 0.1],
            }),
        });
        let link = doc.add_object(link);
        doc.set_parent(link, Some(joint)).unwrap();
        (base, joint, link)
    }

    #[test]
    fn a_robot_reads_as_its_tree() {
        let mut doc = Document::new();
        let (_, joint, link) = arm(&mut doc);
        doc.add_mesh_object("Table", kreate_mesh::primitives::cube(1.0));
        let model = Model::from_document(&doc);
        assert_eq!(model.bodies.len(), 4);
        assert_eq!(model.robots.len(), 1);
        let robot = &model.robots[0];
        assert_eq!(robot.name, "base");
        assert_eq!(robot.mass, 6.0);
        let j = &robot.joints[0];
        assert_eq!(j.name, "shoulder");
        assert_eq!(j.kind, JointKind::Revolute);
        assert!(j.position.abs() < 1e-12);
        assert!((j.origin - DVec3::new(0.0, 0.0, 1.0)).length() < 1e-9);
        assert!((j.axis - DVec3::Z).length() < 1e-9);
        assert_eq!(j.child_link, doc.object_uid(link).map(|u| u.raw()));
        assert_eq!(robot.tool, j.child_link, "the link past the last joint");
        let _ = joint;
    }

    #[test]
    fn turning_a_joint_moves_its_link_and_shows_in_the_diff() {
        let mut doc = Document::new();
        let (_, joint, link) = arm(&mut doc);
        let before = Model::from_document(&doc);

        let RobotPart::Joint(j) = doc.object(joint).unwrap().robot.clone().unwrap() else {
            unreachable!()
        };
        doc.object_mut(joint).unwrap().transform = j.place(0.5);
        let after = Model::from_document(&doc);

        let diff = before.diff(&after);
        let link_uid = doc.object_uid(link).unwrap().raw();
        assert!(
            diff.changed.contains(&link_uid),
            "the child link turned too"
        );
        assert_eq!(diff.robots, vec![after.robots[0].root]);
        assert!((after.robots[0].joints[0].position - 0.5).abs() < 1e-9);
        assert!(!after.robots[0].joints[0].out_of_limits());
    }

    #[test]
    fn a_link_named_for_a_tool_is_the_tool_wherever_it_is() {
        let mut doc = Document::new();
        let (base, _, _) = arm(&mut doc);
        let mut flange = Object::new("Flange", ObjectData::Empty);
        flange.robot = Some(RobotPart::Link { inertial: None });
        let flange = doc.add_object(flange);
        doc.set_parent(flange, Some(base)).unwrap();
        let model = Model::from_document(&doc);
        assert_eq!(
            model.robots[0].tool,
            doc.object_uid(flange).map(|u| u.raw())
        );
    }

    #[test]
    fn nothing_changed_is_an_empty_diff() {
        let mut doc = Document::new();
        arm(&mut doc);
        let a = Model::from_document(&doc);
        let b = Model::from_document(&doc);
        assert!(a.diff(&b).is_empty());
    }
}
