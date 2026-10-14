//! The cell around the robots: what each object of the scene is to the
//! process — a CNC machine, a pallet, a gripper, a fence — and the parts
//! that go round it.
//!
//! The roles are KYNEX's own, kept in the project file beside the programs
//! and found again by identity or name, as programs are: the session's
//! scene is not changed. The parts are KYNEX's too: it adds them to the
//! scene it shows, under identities of its own (see [`PARTS`]), so a part
//! keeps its identity however often the scene is rebuilt.
//!
//! Sizes and times start at the roadmap's industry-typical values
//! (docs/roadmap-vs-visual-components.md §4).

use std::collections::{BTreeMap, HashSet};
use std::time::{Duration, Instant};

use kreate_math::{DAffine3, DVec3, SceneRgb};
use kreate_scene::{Changes, DatablockKind, Document, Material, Object, ObjectData, ObjectId, Uid};
use serde_json::{json, Value};

/// The first identity of what KYNEX adds to the scene — parts, their mesh
/// and material. Kreate numbers a participant's datablocks from
/// `site << 48`; the last site is never a participant.
pub const PARTS: u64 = 0xFFFF << 48;

/// A raw part: a steel bar Ø60 × 80 mm, standing on end.
pub const RAW: (f64, f64) = (0.03, 0.08);
/// A finished part: turned to Ø55 × 75 mm.
pub const FINISHED: (f64, f64) = (0.0275, 0.075);

/// How near a grip point must be to a part's centre to take it, metres.
pub const GRIP_REACH: f64 = 0.02;
/// How near a part let go must be to a free place to settle into it.
pub const SNAP: f64 = 0.03;

/// What a robot tells a CNC machine: its outputs to the machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Open,
    Close,
    Clamp,
    Unclamp,
    Start,
}

impl Command {
    pub const ALL: [Command; 5] = [
        Command::Open,
        Command::Close,
        Command::Clamp,
        Command::Unclamp,
        Command::Start,
    ];

    pub fn tag(self) -> &'static str {
        match self {
            Command::Open => "open",
            Command::Close => "close",
            Command::Clamp => "clamp",
            Command::Unclamp => "unclamp",
            Command::Start => "start",
        }
    }

    pub fn of(tag: &str) -> Option<Command> {
        Command::ALL.into_iter().find(|c| c.tag() == tag)
    }
}

/// What a CNC machine tells a robot: its outputs, the robot's inputs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    DoorOpen,
    DoorClosed,
    Clamped,
    /// Not machining.
    Idle,
    /// Not machining, and a finished part in the chuck.
    Done,
}

impl Signal {
    pub const ALL: [Signal; 5] = [
        Signal::DoorOpen,
        Signal::DoorClosed,
        Signal::Clamped,
        Signal::Idle,
        Signal::Done,
    ];

    pub fn tag(self) -> &'static str {
        match self {
            Signal::DoorOpen => "door_open",
            Signal::DoorClosed => "door_closed",
            Signal::Clamped => "clamped",
            Signal::Idle => "idle",
            Signal::Done => "done",
        }
    }

    pub fn of(tag: &str) -> Option<Signal> {
        Signal::ALL.into_iter().find(|s| s.tag() == tag)
    }
}

/// What an object is to the process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A CNC machine: an object whose children include its door and its
    /// chuck, by name (`…door…`, `…chuck…`).
    Cnc,
    /// Where raw parts come in: a plate, parts on it in a grid.
    Infeed,
    /// Where finished parts go: the same.
    Outfeed,
    /// A grip point on a robot: a link where a part is held, at its origin.
    Gripper,
    /// A safety fence: only marked.
    Fence,
}

impl Kind {
    pub const ALL: [Kind; 5] = [
        Kind::Cnc,
        Kind::Infeed,
        Kind::Outfeed,
        Kind::Gripper,
        Kind::Fence,
    ];

    pub fn tag(self) -> &'static str {
        match self {
            Kind::Cnc => "cnc",
            Kind::Infeed => "infeed",
            Kind::Outfeed => "outfeed",
            Kind::Gripper => "gripper",
            Kind::Fence => "fence",
        }
    }

    pub fn of(tag: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.tag() == tag)
    }

    fn pallet(self) -> bool {
        matches!(self, Kind::Infeed | Kind::Outfeed)
    }
}

/// A component's settings. One set for every kind; each kind reads its own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Props {
    /// CNC: machining, seconds.
    pub cycle: f64,
    /// CNC: the door opening, and again closing, seconds.
    pub door: f64,
    /// CNC: the chuck opening, and again closing, seconds.
    pub chuck: f64,
    /// Pallet: places across and along, and their spacing in metres.
    pub rows: u32,
    pub cols: u32,
    pub pitch: f64,
    /// Gripper: opening, and again closing, seconds.
    pub actuate: f64,
}

impl Default for Props {
    /// The roadmap's industry-typical values.
    fn default() -> Self {
        Self {
            cycle: 90.0,
            door: 3.0,
            chuck: 1.0,
            rows: 4,
            cols: 5,
            pitch: 0.1,
            actuate: 0.5,
        }
    }
}

impl Props {
    /// Takes the settings `v` names, held to sensible ranges.
    fn update(&mut self, v: &Value) {
        let time = |key: &str, now: f64| v[key].as_f64().map_or(now, |x| x.clamp(0.0, 3600.0));
        self.cycle = time("cycle", self.cycle);
        self.door = time("door", self.door);
        self.chuck = time("chuck", self.chuck);
        self.actuate = time("actuate", self.actuate);
        let count = |key: &str, now: u32| v[key].as_u64().map_or(now, |x| x.clamp(1, 50) as u32);
        self.rows = count("rows", self.rows);
        self.cols = count("cols", self.cols);
        self.pitch = v["pitch"]
            .as_f64()
            .map_or(self.pitch, |x| x.clamp(0.01, 2.0));
    }

    fn to_json(self) -> Value {
        json!({
            "cycle": self.cycle,
            "door": self.door,
            "chuck": self.chuck,
            "rows": self.rows,
            "cols": self.cols,
            "pitch": self.pitch,
            "actuate": self.actuate,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Component {
    pub object: u64,
    /// The object's name when the role was given: how it is found again.
    pub name: String,
    pub kind: Kind,
    pub props: Props,
}

/// A part going round the cell.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    /// Its identity in the scene shown: [`PARTS`] and on.
    pub uid: u64,
    /// Where it is, standing on end: its centre.
    pub world: DAffine3,
    pub finished: bool,
    /// The grip point holding it, and where it sits from there.
    pub held: Option<(u64, DAffine3)>,
}

impl Part {
    fn size(&self) -> (f64, f64) {
        if self.finished {
            FINISHED
        } else {
            RAW
        }
    }
}

/// A CNC machine as it runs: its door, its chuck and its cycle.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Machine {
    /// How far open the door is: 0 shut, 1 open.
    pub door: f64,
    /// Whether the door is opening (or open), rather than closing.
    pub opening: bool,
    pub clamped: bool,
    /// Seconds into the cycle, while machining.
    pub machining: Option<f64>,
}

impl Machine {
    pub fn shut(&self) -> bool {
        self.door <= 0.0 && !self.opening
    }
}

/// The cell: roles, and parts.
#[derive(Debug, Default)]
pub struct Cell {
    components: Vec<Component>,
    parts: Vec<Part>,
    next: u64,
    /// Parts taken away since the viewers were last told.
    removed: Vec<u64>,
    /// The roles changed since they were last saved.
    roles_changed: bool,
    /// Anything the viewers show changed since they were last told.
    changed: bool,
    /// Roles from the project file whose object is not in the scene (yet).
    pending: Vec<Value>,
    /// Each CNC machine's state, by its object.
    machines: BTreeMap<u64, Machine>,
    /// The simulation's clock when the cell was last moved on; and the real
    /// time it last said it changed while running.
    last_tick: Option<f64>,
    last_report: Option<Instant>,
}

/// How often a running cell tells the viewers how it is getting on.
const REPORT: Duration = Duration::from_millis(100);

/// An object's box in the world, from its mesh.
fn bounds(doc: &Document, id: ObjectId) -> Option<(DVec3, DVec3)> {
    let mesh = doc.mesh(doc.object(id)?.mesh()?)?;
    let world = doc.world_transform(id);
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = DVec3::splat(f64::NEG_INFINITY);
    for p in mesh.positions() {
        let w = world.transform_point3(p.as_dvec3());
        lo = lo.min(w);
        hi = hi.max(w);
    }
    (lo.x <= hi.x).then_some((lo, hi))
}

fn id_of(doc: &Document, uid: u64) -> Option<ObjectId> {
    doc.object_by_uid(Uid::from_raw(uid))
}

/// Roles read from names, for a scene built to the usual conventions:
///
/// - a CNC machine is an object with a door and a chuck among its children
///   (`…door…`, `…chuck…`);
/// - a pallet is named for its side, `infeed` or `outfeed` (`투입`, `배출`),
///   alone or with `pallet` (`팔레트`): not its table, `infeed_table`;
/// - a grip point is a robot's link named `grip…` with no link below it.
///
/// A fence is left to be marked by hand: its posts and rails are objects
/// of their own.
pub fn guess(doc: &Document) -> Vec<(u64, Kind)> {
    let pallet = |name: &str, side: &[&str]| {
        side.iter().any(|w| {
            name == *w || (name.contains(w) && (name.contains("pallet") || name.contains("팔레트")))
        })
    };
    use kreate_scene::robot::RobotPart;
    let lower = |id: ObjectId| {
        doc.object(id)
            .map(|o| o.name.to_lowercase())
            .unwrap_or_default()
    };
    let mut out = Vec::new();
    for (id, object) in doc.objects() {
        let Some(uid) = doc.object_uid(id).map(|u| u.raw()) else {
            continue;
        };
        let name = lower(id);
        let link = matches!(object.robot, Some(RobotPart::Link { .. }));
        // The machine itself, not a group holding it: the lowest object
        // with both below it.
        let machine = |at: ObjectId| {
            part_named(doc, at, "door").is_some() && part_named(doc, at, "chuck").is_some()
        };
        let kind = if machine(id)
            && !doc
                .objects()
                .any(|(c, o)| o.parent == Some(id) && machine(c))
        {
            Some(Kind::Cnc)
        } else if !link && object.mesh().is_some() && pallet(&name, &["infeed", "투입"]) {
            Some(Kind::Infeed)
        } else if !link && object.mesh().is_some() && pallet(&name, &["outfeed", "배출"]) {
            Some(Kind::Outfeed)
        } else if link && name.starts_with("grip") && !links_below(doc, id) {
            Some(Kind::Gripper)
        } else {
            None
        };
        if let Some(kind) = kind {
            out.push((uid, kind));
        }
    }
    out
}

/// Whether `id` hangs below `parent`, at any depth.
fn is_below(doc: &Document, id: ObjectId, parent: ObjectId) -> bool {
    let mut at = doc.object(id).and_then(|o| o.parent);
    for _ in 0..64 {
        match at {
            Some(p) if p == parent => return true,
            Some(p) => at = doc.object(p).and_then(|o| o.parent),
            None => return false,
        }
    }
    false
}

/// Whether any robot link hangs below `parent`, at any depth: between two
/// links there is always a joint.
fn links_below(doc: &Document, parent: ObjectId) -> bool {
    use kreate_scene::robot::RobotPart;
    doc.objects()
        .filter(|(_, o)| matches!(o.robot, Some(RobotPart::Link { .. })))
        .any(|(id, _)| is_below(doc, id, parent))
}

/// A child of `parent`, at any depth, whose name holds `word`.
fn part_named(doc: &Document, parent: ObjectId, word: &str) -> Option<ObjectId> {
    doc.objects()
        .filter(|(_, o)| o.name.to_lowercase().contains(word))
        .map(|(id, _)| id)
        .find(|id| is_below(doc, *id, parent))
}

impl Cell {
    pub fn from_saved(saved: Vec<Value>) -> Self {
        Self {
            pending: saved,
            ..Self::default()
        }
    }

    /// Takes on the roles from the project file whose objects are in `doc`:
    /// by identity while the name agrees, or else by name.
    pub fn adopt(&mut self, doc: &Document) {
        let mut waiting = Vec::new();
        for saved in std::mem::take(&mut self.pending) {
            let name = saved["name"].as_str().unwrap_or_default();
            let kind = saved["kind"].as_str().and_then(Kind::of);
            let by_uid = saved["object"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .filter(|u| {
                    id_of(doc, *u)
                        .and_then(|id| doc.object(id))
                        .is_some_and(|o| o.name == name)
                });
            let found = by_uid.or_else(|| {
                doc.objects()
                    .find(|(_, o)| o.name == name)
                    .and_then(|(id, _)| doc.object_uid(id))
                    .map(|u| u.raw())
            });
            match (found, kind) {
                (Some(object), Some(kind))
                    if !self.components.iter().any(|c| c.object == object) =>
                {
                    let mut props = Props::default();
                    props.update(&saved["props"]);
                    self.components.push(Component {
                        object,
                        name: name.to_string(),
                        kind,
                        props,
                    });
                    self.changed = true;
                }
                // A role equipment came with: the settings saved win over
                // its defaults.
                (Some(object), Some(kind)) => {
                    if let Some(c) = self.components.iter_mut().find(|c| c.object == object) {
                        c.kind = kind;
                        c.props.update(&saved["props"]);
                        self.changed = true;
                    }
                }
                _ => waiting.push(saved),
            }
        }
        self.pending = waiting;
        // One machine is one CNC role: a role on a part of a machine that
        // has one already (saved before auto roles knew better) is dropped.
        let machines: Vec<ObjectId> = self
            .components
            .iter()
            .filter(|c| c.kind == Kind::Cnc)
            .filter_map(|c| id_of(doc, c.object))
            .collect();
        self.components.retain(|c| {
            c.kind != Kind::Cnc
                || !id_of(doc, c.object)
                    .is_some_and(|id| machines.iter().any(|m| is_below(doc, id, *m)))
        });
    }

    /// The roles, as saved to the project file: with those still waiting
    /// for their objects.
    pub fn to_saved(&self) -> Vec<Value> {
        self.components
            .iter()
            .map(|c| {
                json!({
                    "object": c.object.to_string(),
                    "name": c.name,
                    "kind": c.kind.tag(),
                    "props": c.props.to_json(),
                })
            })
            .chain(self.pending.iter().cloned())
            .collect()
    }

    /// Gives `object` the role `kind` unless it has one: the role equipment
    /// from the library comes with. A role changed by hand is kept.
    pub fn ensure(&mut self, object: u64, name: String, kind: Kind) {
        if self.components.iter().any(|c| c.object == object) {
            return;
        }
        self.components.push(Component {
            object,
            name,
            kind,
            props: Props::default(),
        });
        self.roles_changed = true;
        self.changed = true;
    }

    /// Puts the roles `saved` in place of these, with no parts and every
    /// machine at rest: a scenario taken back.
    pub fn restore(&mut self, saved: Vec<Value>) {
        let mut removed = std::mem::take(&mut self.removed);
        removed.extend(self.parts.iter().map(|p| p.uid));
        let next = self.next;
        *self = Self::from_saved(saved);
        self.removed = removed;
        // Parts made from here on are new to the viewers.
        self.next = next;
        self.roles_changed = true;
        self.changed = true;
    }

    /// Keeps the cell supplied, as an operator would over a long run: every
    /// free place on an infeed pallet gets a raw part, and the finished
    /// parts set down on an outfeed pallet are taken away. Returns how many
    /// were taken away.
    pub fn supply(&mut self, doc: &Document) -> usize {
        self.stock(doc);
        let outfeeds: Vec<Component> = self
            .components
            .iter()
            .filter(|c| c.kind == Kind::Outfeed)
            .cloned()
            .collect();
        let mut taken = 0;
        for c in outfeeds {
            let slots = self.slots(doc, &c);
            let (gone, kept): (Vec<Part>, Vec<Part>) =
                std::mem::take(&mut self.parts).into_iter().partition(|p| {
                    p.finished
                        && p.held.is_none()
                        && slots
                            .iter()
                            .any(|at| p.world.translation.distance(*at) < SNAP)
                });
            self.parts = kept;
            taken += gone.len();
            self.removed.extend(gone.iter().map(|p| p.uid));
            self.changed |= !gone.is_empty();
        }
        taken
    }

    /// Puts a raw part on every free place of every infeed pallet: as a run
    /// starts, so that it has parts to take. Returns how many were put.
    pub fn stock(&mut self, doc: &Document) -> usize {
        let infeeds: Vec<Component> = self
            .components
            .iter()
            .filter(|c| c.kind == Kind::Infeed)
            .cloned()
            .collect();
        let mut put = 0;
        for c in infeeds {
            for at in self.slots(doc, &c) {
                let free = !self
                    .parts
                    .iter()
                    .any(|p| p.held.is_none() && p.world.translation.distance(at) < SNAP);
                if free {
                    self.add_part(at, false);
                    put += 1;
                }
            }
        }
        put
    }

    /// Takes away the roles of objects that are gone from the scene.
    pub fn forget(&mut self, objects: &[u64]) {
        let before = self.components.len();
        self.components.retain(|c| !objects.contains(&c.object));
        if self.components.len() != before {
            self.roles_changed = true;
            self.changed = true;
        }
    }

    /// Whether the roles changed since this was last asked: to be saved.
    pub fn take_roles_changed(&mut self) -> bool {
        std::mem::take(&mut self.roles_changed)
    }

    /// Whether what the viewers show changed since this was last asked.
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    #[cfg(test)]
    fn components(&self) -> &[Component] {
        &self.components
    }

    #[cfg(test)]
    fn parts(&self) -> &[Part] {
        &self.parts
    }

    /// The parts being held: left out of the collision check, as the
    /// gripper's own.
    pub fn held(&self) -> HashSet<u64> {
        self.parts
            .iter()
            .filter(|p| p.held.is_some())
            .map(|p| p.uid)
            .collect()
    }

    /// The places of a pallet, where a raw part's centre stands, row by row.
    pub fn slots(&self, doc: &Document, c: &Component) -> Vec<DVec3> {
        if !c.kind.pallet() {
            return Vec::new();
        }
        let Some(id) = id_of(doc, c.object) else {
            return Vec::new();
        };
        let Some((lo, hi)) = bounds(doc, id) else {
            return Vec::new();
        };
        // Along the plate's own axes, flat on its top.
        let world = doc.world_transform(id);
        let ux = world.matrix3.x_axis.normalize_or_zero();
        let uy = world.matrix3.y_axis.normalize_or_zero();
        let centre = DVec3::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0, hi.z + RAW.1 / 2.0);
        let p = c.props;
        let mut out = Vec::new();
        for r in 0..p.rows {
            for k in 0..p.cols {
                let x = (k as f64 - (p.cols - 1) as f64 / 2.0) * p.pitch;
                let y = (r as f64 - (p.rows - 1) as f64 / 2.0) * p.pitch;
                out.push(centre + ux * x + uy * y);
            }
        }
        out
    }

    /// Where a part's centre stands in a CNC machine's chuck.
    pub fn seat(&self, doc: &Document, c: &Component) -> Option<DVec3> {
        if c.kind != Kind::Cnc {
            return None;
        }
        let chuck = part_named(doc, id_of(doc, c.object)?, "chuck")?;
        let (lo, hi) = bounds(doc, chuck)?;
        Some(DVec3::new(
            (lo.x + hi.x) / 2.0,
            (lo.y + hi.y) / 2.0,
            hi.z + RAW.1 / 2.0,
        ))
    }

    /// Every place a part may stand, and whether one does.
    fn places(&self, doc: &Document) -> Vec<(DVec3, bool)> {
        let occupied = |at: DVec3| {
            self.parts
                .iter()
                .any(|p| p.held.is_none() && p.world.translation.distance(at) < SNAP)
        };
        self.components
            .iter()
            .flat_map(|c| {
                let mut v = self.slots(doc, c);
                v.extend(self.seat(doc, c));
                v
            })
            .map(|at| (at, occupied(at)))
            .collect()
    }

    fn add_part(&mut self, at: DVec3, finished: bool) {
        self.next += 1;
        self.parts.push(Part {
            uid: PARTS + self.next,
            world: DAffine3::from_translation(at),
            finished,
            held: None,
        });
        self.changed = true;
    }

    /// The world matrix of grip point `gripper` in `doc`.
    fn grip_point(doc: &Document, gripper: u64) -> Option<DAffine3> {
        Some(doc.world_transform(id_of(doc, gripper)?))
    }

    /// Takes one request from the page. `doc` is the scene as shown: the
    /// robots where they are posed. Returns the answer.
    pub fn handle(&mut self, doc: &Document, body: &str) -> String {
        match self.take(doc, body) {
            Ok(v) => v,
            Err(e) => json!({ "ok": false, "error": e }),
        }
        .to_string()
    }

    fn take(&mut self, doc: &Document, body: &str) -> Result<Value, String> {
        let req: Value = serde_json::from_str(body).map_err(|e| format!("not JSON: {e}"))?;
        let op = req["op"].as_str().ok_or("no op")?;
        let uid = |key: &str| {
            req[key]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .ok_or_else(|| format!("no {key}"))
        };
        match op {
            "role" => {
                let object = uid("object")?;
                let id = id_of(doc, object).ok_or("no such object")?;
                self.components.retain(|c| c.object != object);
                if let Some(tag) = req["kind"].as_str() {
                    let kind = Kind::of(tag).ok_or("unknown role")?;
                    let mut props = Props::default();
                    props.update(&req["props"]);
                    self.components.push(Component {
                        object,
                        name: doc.object(id).map(|o| o.name.clone()).unwrap_or_default(),
                        kind,
                        props,
                    });
                }
                self.roles_changed = true;
                self.changed = true;
            }
            "fill" => {
                let object = uid("object")?;
                let c = self
                    .components
                    .iter()
                    .find(|c| c.object == object && c.kind.pallet())
                    .cloned()
                    .ok_or("not a pallet")?;
                let finished = c.kind == Kind::Outfeed;
                let free: Vec<DVec3> =
                    self.slots(doc, &c)
                        .into_iter()
                        .filter(|at| {
                            !self.parts.iter().any(|p| {
                                p.held.is_none() && p.world.translation.distance(*at) < SNAP
                            })
                        })
                        .collect();
                for at in free {
                    self.add_part(at, finished);
                }
            }
            "empty" => {
                let object = uid("object")?;
                let c = self
                    .components
                    .iter()
                    .find(|c| c.object == object)
                    .cloned()
                    .ok_or("no such component")?;
                let mut places = self.slots(doc, &c);
                places.extend(self.seat(doc, &c));
                let (gone, kept): (Vec<Part>, Vec<Part>) =
                    std::mem::take(&mut self.parts).into_iter().partition(|p| {
                        p.held.is_none()
                            && places
                                .iter()
                                .any(|at| p.world.translation.distance(*at) < SNAP)
                    });
                self.parts = kept;
                self.removed.extend(gone.iter().map(|p| p.uid));
                self.changed = true;
            }
            "auto" => {
                let found = guess(doc);
                let mut added = 0;
                // A machine marked already, as equipment from the library
                // comes, is not marked again by a part of it.
                let machines: Vec<ObjectId> = self
                    .components
                    .iter()
                    .filter(|c| c.kind == Kind::Cnc)
                    .filter_map(|c| id_of(doc, c.object))
                    .collect();
                let inside = |object: u64| {
                    let mut at = id_of(doc, object);
                    for _ in 0..64 {
                        match at {
                            Some(id) if machines.contains(&id) => return true,
                            Some(id) => at = doc.object(id).and_then(|o| o.parent),
                            None => return false,
                        }
                    }
                    false
                };
                for (object, kind) in found {
                    if self.components.iter().any(|c| c.object == object)
                        || (kind == Kind::Cnc && inside(object))
                    {
                        continue;
                    }
                    let name = id_of(doc, object)
                        .and_then(|id| doc.object(id))
                        .map(|o| o.name.clone())
                        .unwrap_or_default();
                    self.components.push(Component {
                        object,
                        name,
                        kind,
                        props: Props::default(),
                    });
                    added += 1;
                }
                self.roles_changed |= added > 0;
                self.changed = true;
                return Ok(json!({ "ok": true, "added": added }));
            }
            "clear_parts" => {
                self.removed
                    .extend(std::mem::take(&mut self.parts).iter().map(|p| p.uid));
                self.changed = true;
            }
            "grip" => self.grip(doc, uid("gripper")?)?,
            "release" => self.release(doc, uid("gripper")?)?,
            "machine" => {
                let command = req["action"]
                    .as_str()
                    .and_then(Command::of)
                    .ok_or("unknown action")?;
                self.command(doc, uid("object")?, command)?;
            }
            other => return Err(format!("unknown op {other:?}")),
        }
        Ok(json!({ "ok": true }))
    }

    /// Closes grip point `gripper` on the part at it: within
    /// [`GRIP_REACH`], not held, and not clamped in a chuck.
    pub fn grip(&mut self, doc: &Document, gripper: u64) -> Result<(), String> {
        let clamped_seats: Vec<DVec3> = self
            .components
            .iter()
            .filter(|c| self.machines.get(&c.object).is_some_and(|m| m.clamped))
            .filter_map(|c| self.seat(doc, c))
            .collect();
        if self
            .parts
            .iter()
            .any(|p| p.held.is_some_and(|h| h.0 == gripper))
        {
            return Err("이미 부품을 잡고 있습니다".into());
        }
        let at = Self::grip_point(doc, gripper).ok_or("no such gripper")?;
        let near = self
            .parts
            .iter_mut()
            .filter(|p| p.held.is_none())
            .map(|p| (p.world.translation.distance(at.translation), p))
            .filter(|(d, _)| *d < GRIP_REACH)
            .min_by(|a, b| a.0.total_cmp(&b.0));
        let Some((_, part)) = near else {
            return Err("잡을 부품이 그리퍼 가까이에 없습니다".into());
        };
        let here = part.world.translation;
        if clamped_seats.iter().any(|s| s.distance(here) < SNAP) {
            return Err("척이 물려 있습니다. 척을 먼저 푸세요".into());
        }
        // Held as it stands, wherever the grip point turns.
        part.held = Some((gripper, at.inverse() * part.world));
        self.changed = true;
        Ok(())
    }

    /// Opens grip point `gripper`: its part settles into a free place or
    /// chuck within [`SNAP`], upright, or stays where it is.
    pub fn release(&mut self, doc: &Document, gripper: u64) -> Result<(), String> {
        let at = Self::grip_point(doc, gripper).ok_or("no such gripper")?;
        let places = self.places(doc);
        let Some(part) = self
            .parts
            .iter_mut()
            .find(|p| p.held.is_some_and(|h| h.0 == gripper))
        else {
            return Err("잡고 있는 부품이 없습니다".into());
        };
        let (_, grip) = part.held.take().unwrap();
        part.world = at * grip;
        let here = part.world.translation;
        if let Some((place, _)) = places
            .iter()
            .filter(|(p, taken)| !taken && p.distance(here) < SNAP)
            .min_by(|a, b| a.0.distance(here).total_cmp(&b.0.distance(here)))
        {
            let half = part.size().1 / 2.0;
            part.world = DAffine3::from_translation(*place + DVec3::Z * (half - RAW.1 / 2.0));
        }
        self.changed = true;
        Ok(())
    }

    /// How long grip point `gripper` takes to open or close, seconds.
    pub fn actuate_time(&self, gripper: u64) -> f64 {
        self.components
            .iter()
            .find(|c| c.object == gripper)
            .map_or(Props::default().actuate, |c| c.props.actuate)
    }

    /// What is in machine `c`'s chuck: `Some(finished)`, or `None`.
    fn in_chuck(&self, doc: &Document, c: &Component) -> Option<bool> {
        let seat = self.seat(doc, c)?;
        self.parts
            .iter()
            .find(|p| p.held.is_none() && p.world.translation.distance(seat) < SNAP)
            .map(|p| p.finished)
    }

    /// Tells CNC machine `machine` to do `command`, as a robot's output to
    /// it would. Refused, with why, where the machine would refuse.
    pub fn command(
        &mut self,
        doc: &Document,
        machine: u64,
        command: Command,
    ) -> Result<(), String> {
        let c = self
            .components
            .iter()
            .find(|c| c.object == machine && c.kind == Kind::Cnc)
            .cloned()
            .ok_or("CNC 장비가 아닙니다")?;
        let in_chuck = self.in_chuck(doc, &c);
        let m = self.machines.entry(machine).or_default();
        let busy = m.machining.is_some();
        match command {
            Command::Open if busy => return Err("가공 중에는 문을 열 수 없습니다".into()),
            Command::Open => m.opening = true,
            Command::Close => m.opening = false,
            Command::Clamp => m.clamped = true,
            Command::Unclamp if busy => return Err("가공 중에는 척을 풀 수 없습니다".into()),
            Command::Unclamp => m.clamped = false,
            Command::Start => {
                if busy {
                    return Err("이미 가공 중입니다".into());
                }
                if !m.shut() {
                    return Err("문을 닫아야 가공을 시작할 수 있습니다".into());
                }
                match in_chuck {
                    None => return Err("척에 소재가 없습니다".into()),
                    Some(true) => return Err("척에 있는 것은 이미 완성품입니다".into()),
                    Some(false) => {}
                }
                if !m.clamped {
                    return Err("척을 물려야 가공을 시작할 수 있습니다".into());
                }
                m.machining = Some(0.0);
            }
        }
        self.changed = true;
        Ok(())
    }

    /// Whether CNC machine `machine` says `signal` now, as its output to a
    /// robot would.
    pub fn signal(&self, doc: &Document, machine: u64, signal: Signal) -> bool {
        let Some(c) = self
            .components
            .iter()
            .find(|c| c.object == machine && c.kind == Kind::Cnc)
        else {
            return false;
        };
        let m = self.machines.get(&machine).cloned().unwrap_or_default();
        match signal {
            Signal::DoorOpen => m.door >= 1.0 && m.opening,
            Signal::DoorClosed => m.shut(),
            Signal::Clamped => m.clamped,
            Signal::Idle => m.machining.is_none(),
            Signal::Done => m.machining.is_none() && self.in_chuck(doc, c) == Some(true),
        }
    }

    /// Moves the machines on to `sim`, the simulation's clock in seconds:
    /// doors at their pace, cycles at theirs, a part done when its cycle
    /// is. `doc` is the scene the chucks are in. Returns whether the viewers
    /// have anything to look at again: at a change of state at once, and
    /// while running, every [`REPORT`] of real time `now`.
    pub fn tick(&mut self, doc: &Document, sim: f64, now: Instant) -> bool {
        let dt = self.last_tick.map_or(0.0, |t| (sim - t).max(0.0));
        self.last_tick = Some(sim);
        let (moving, finished) = self.step(doc, dt);
        let due = self
            .last_report
            .is_none_or(|t| now.duration_since(t) >= REPORT);
        let report = finished || (moving && due);
        if report {
            self.last_report = Some(now);
            self.changed = true;
        }
        report
    }

    /// Whether anything is moving or running: the loop then keeps time.
    pub fn is_running(&self) -> bool {
        self.machines.values().any(|m| {
            m.machining.is_some() || (m.opening && m.door < 1.0) || (!m.opening && m.door > 0.0)
        })
    }

    /// Moves the machines on by `dt` seconds. Returns whether anything moved
    /// or ran, and whether a state changed: a door reaching its end, a
    /// cycle ending.
    fn step(&mut self, doc: &Document, dt: f64) -> (bool, bool) {
        let mut moving = false;
        let mut finished = false;
        let mut done: Vec<DVec3> = Vec::new();
        for c in &self.components {
            let Some(m) = self.machines.get_mut(&c.object) else {
                continue;
            };
            let target = if m.opening { 1.0 } else { 0.0 };
            if m.door != target {
                moving = true;
                let pace = if c.props.door > 0.0 {
                    dt / c.props.door
                } else {
                    1.0
                };
                m.door = if m.opening {
                    (m.door + pace).min(1.0)
                } else {
                    (m.door - pace).max(0.0)
                };
                finished |= m.door == target;
            }
            if let Some(t) = m.machining {
                moving = true;
                let t = t + dt;
                if t >= c.props.cycle {
                    m.machining = None;
                    finished = true;
                    if let Some(seat) = self.seat(doc, c) {
                        done.push(seat);
                    }
                } else {
                    m.machining = Some(t);
                }
            }
        }
        // A raw part in a chuck whose cycle ended is a finished part.
        for seat in done {
            for p in &mut self.parts {
                if p.held.is_none() && !p.finished && p.world.translation.distance(seat) < SNAP {
                    p.finished = true;
                    // It stands on the same chuck, a little shorter.
                    p.world =
                        DAffine3::from_translation(seat + DVec3::Z * (FINISHED.1 - RAW.1) / 2.0);
                }
            }
        }
        (moving, finished)
    }

    /// What each CNC machine is doing, for the timeline: its identity, its
    /// name, and `cut` (machining), `door` (its door moving), `blocked`
    /// (a finished part waiting to be taken out), or `idle`.
    pub fn activities(&self, doc: &Document) -> Vec<(u64, String, &'static str)> {
        self.components
            .iter()
            .filter(|c| c.kind == Kind::Cnc)
            .map(|c| {
                let m = self.machines.get(&c.object).cloned().unwrap_or_default();
                let target = if m.opening { 1.0 } else { 0.0 };
                let activity = if m.machining.is_some() {
                    "cut"
                } else if m.door != target {
                    "door"
                } else if self.in_chuck(doc, c) == Some(true) {
                    "blocked"
                } else {
                    "idle"
                };
                (c.object, c.name.clone(), activity)
            })
            .collect()
    }

    /// Whether the scene shown needs the cell in it: parts to add, or a
    /// door to move.
    pub fn needs_scene(&self) -> bool {
        !self.parts.is_empty() || self.machines.values().any(|m| m.door > 0.0)
    }

    /// What the viewers need to look at again when the scene shown has
    /// nothing of the cell in it: parts that went, and doors back where
    /// the scene has them.
    pub fn untouched(&mut self, doc: &Document) -> Changes {
        let mut changes = self.take_removed();
        for uid in self.doors(doc).into_iter().map(|(uid, _, _)| uid) {
            changes
                .changed
                .insert((DatablockKind::Object, Uid::from_raw(uid)));
        }
        changes
    }

    /// Machine `machine`'s door, by identity.
    pub fn door_of(&self, doc: &Document, machine: u64) -> Option<u64> {
        let id = id_of(doc, machine)?;
        doc.object_uid(part_named(doc, id, "door")?)
            .map(|u| u.raw())
    }

    /// Each machine's door: its identity, how far open, and the machine's
    /// own X axis it slides along.
    fn doors(&self, doc: &Document) -> Vec<(u64, f64, ObjectId)> {
        self.components
            .iter()
            .filter(|c| c.kind == Kind::Cnc)
            .filter_map(|c| {
                let machine = id_of(doc, c.object)?;
                let door = part_named(doc, machine, "door")?;
                let open = self.machines.get(&c.object).map_or(0.0, |m| m.door);
                Some((doc.object_uid(door)?.raw(), open, machine))
            })
            .collect()
    }

    /// Slides each open door aside in `doc`, along its machine's X, by its
    /// own width.
    fn open_doors(&self, doc: &mut Document) -> Vec<u64> {
        let mut uids = Vec::new();
        for (uid, open, machine) in self.doors(doc) {
            uids.push(uid);
            let Some(door) = id_of(doc, uid) else {
                continue;
            };
            if open <= 0.0 {
                continue;
            }
            let along = doc
                .world_transform(machine)
                .matrix3
                .x_axis
                .normalize_or_zero();
            let Some((lo, hi)) = bounds(doc, door) else {
                continue;
            };
            let width = (hi - lo).dot(along).abs();
            let shift = along * (width + 0.02) * open;
            // From the world into the door's own transform.
            let world = doc.world_transform(door);
            let Some(o) = doc.object(door) else { continue };
            let above = world * o.transform.to_affine().inverse();
            let local = above.matrix3.inverse() * shift;
            doc.object_mut(door).unwrap().transform.translation += local;
        }
        uids
    }

    /// Adds the parts to `doc`, the scene as shown, and returns what the
    /// viewers need to look at again. A held part goes where its grip point
    /// now is.
    pub fn apply(&mut self, doc: &mut Document) -> Changes {
        let mut changes = self.take_removed();
        for uid in self.open_doors(doc) {
            changes
                .changed
                .insert((DatablockKind::Object, Uid::from_raw(uid)));
        }
        if self.parts.is_empty() {
            return changes;
        }
        for part in &mut self.parts {
            if let Some((gripper, grip)) = part.held {
                if let Some(at) = Self::grip_point(doc, gripper) {
                    part.world = at * grip;
                }
            }
        }
        // One mesh and one colour for each kind of part, under fixed
        // identities.
        let mut meshes = BTreeMap::new();
        for (k, finished) in [(1u64, false), (2, true)] {
            let (r, h) = if finished { FINISHED } else { RAW };
            let colour = if finished {
                SceneRgb::new(0.95, 0.72, 0.25)
            } else {
                SceneRgb::new(0.62, 0.64, 0.68)
            };
            let material = doc.restore_material(
                Material {
                    name: if finished {
                        "finished part"
                    } else {
                        "raw part"
                    }
                    .into(),
                    base_color: colour,
                    metallic: 0.7,
                    roughness: 0.35,
                    ..Material::default()
                },
                Uid::from_raw(PARTS + k),
            );
            let mesh = doc.restore_mesh(
                kreate_mesh::primitives::cylinder(r as f32, h as f32, 24),
                Uid::from_raw(PARTS + k),
            );
            doc.set_mesh_slot(mesh, 0, Some(material));
            meshes.insert(finished, mesh);
        }
        for part in &self.parts {
            let mut object = Object::new(
                if part.finished { "완성품" } else { "소재" },
                ObjectData::Mesh(meshes[&part.finished]),
            );
            let (_, rotation, translation) = part.world.to_scale_rotation_translation();
            object.transform.translation = translation;
            object.transform.rotation = rotation;
            doc.restore_object(object, Uid::from_raw(part.uid));
            changes
                .changed
                .insert((DatablockKind::Object, Uid::from_raw(part.uid)));
        }
        changes
    }

    /// The parts taken away since the viewers were last told, as changes.
    pub fn take_removed(&mut self) -> Changes {
        let mut changes = Changes::default();
        changes
            .removed
            .extend(self.removed.drain(..).map(Uid::from_raw));
        changes
    }

    /// The `cell` event: the roles, where their places are, and the parts.
    pub fn to_json(&self, doc: &Document) -> String {
        let point = |p: DVec3| json!([p.x, p.y, p.z]);
        let components: Vec<Value> = self
            .components
            .iter()
            .map(|c| {
                let robot = id_of(doc, c.object)
                    .and_then(|id| doc.robot_root(id))
                    .and_then(|r| doc.object_uid(r))
                    .map(|u| u.raw().to_string());
                json!({
                    "object": c.object.to_string(),
                    "name": c.name,
                    "kind": c.kind.tag(),
                    "props": c.props.to_json(),
                    "robot": robot,
                    "slots": self.slots(doc, c).into_iter().map(point).collect::<Vec<_>>(),
                    "seat": self.seat(doc, c).map(point),
                    "holding": self.parts.iter().any(|p| p.held.is_some_and(|h| h.0 == c.object)),
                    "machine": (c.kind == Kind::Cnc).then(|| {
                        let m = self.machines.get(&c.object).cloned().unwrap_or_default();
                        let part = self.seat(doc, c).and_then(|seat| {
                            self.parts
                                .iter()
                                .find(|p| p.held.is_none() && p.world.translation.distance(seat) < SNAP)
                                .map(|p| if p.finished { "finished" } else { "raw" })
                        });
                        json!({
                            "door": m.door,
                            "opening": m.opening,
                            "clamped": m.clamped,
                            "progress": m.machining.map(|t| (t / c.props.cycle.max(1e-9)).min(1.0)),
                            "left": m.machining.map(|t| (c.props.cycle - t).max(0.0)),
                            "part": part,
                        })
                    }),
                })
            })
            .collect();
        json!({
            "components": components,
            "parts": self.parts.iter().map(|p| json!({
                "uid": p.uid.to_string(),
                "finished": p.finished,
                "held": p.held.map(|h| h.0.to_string()),
            })).collect::<Vec<_>>(),
        })
        .to_string()
    }

    /// For tests: the part held by `gripper`.
    #[cfg(test)]
    fn holding(&self, gripper: u64) -> Option<&Part> {
        self.parts
            .iter()
            .find(|p| p.held.is_some_and(|h| h.0 == gripper))
    }
}

/// The requests [`Cell`] takes.
pub fn is_op(op: &str) -> bool {
    matches!(
        op,
        "role" | "auto" | "fill" | "empty" | "clear_parts" | "grip" | "release" | "machine"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plate 0.55 × 0.45 m, its top 0.77 m up, and a machine with a
    /// chuck; a grip point, as an empty, moved about by the test.
    fn scene() -> (Document, u64, u64, u64) {
        let mut doc = Document::new();
        let plate = doc.add_mesh_object("infeed", kreate_mesh::primitives::cube(1.0));
        {
            let o = doc.object_mut(plate).unwrap();
            o.transform.scale = DVec3::new(0.55, 0.45, 0.02);
            o.transform.translation = DVec3::new(-0.8, 0.0, 0.76);
        }
        let machine = doc.add_object(Object::new("cnc_1", ObjectData::Empty));
        let chuck = doc.add_mesh_object(
            "cnc_1_chuck",
            kreate_mesh::primitives::cylinder(0.09, 0.12, 16),
        );
        doc.object_mut(chuck).unwrap().transform.translation = DVec3::new(0.0, 1.1, 0.91);
        doc.set_parent(chuck, Some(machine)).unwrap();
        let grip = doc.add_object(Object::new("grip_a", ObjectData::Empty));
        let uid = |id| doc.object_uid(id).unwrap().raw();
        let (p, m, g) = (uid(plate), uid(machine), uid(grip));
        (doc, p, m, g)
    }

    fn ask(cell: &mut Cell, doc: &Document, req: Value) -> Value {
        serde_json::from_str(&cell.handle(doc, &req.to_string())).unwrap()
    }

    fn move_grip(doc: &mut Document, grip: u64, at: DVec3) {
        let id = id_of(doc, grip).unwrap();
        doc.object_mut(id).unwrap().transform.translation = at;
    }

    #[test]
    fn a_pallet_is_a_grid_on_its_top_and_fills() {
        let (doc, plate, _, _) = scene();
        let mut cell = Cell::default();
        let a = ask(
            &mut cell,
            &doc,
            json!({"op": "role", "object": plate.to_string(), "kind": "infeed"}),
        );
        assert_eq!(a["ok"], true, "{a}");
        let c = cell.components()[0].clone();
        assert_eq!(c.props, Props::default(), "the typical values");
        let slots = cell.slots(&doc, &c);
        assert_eq!(slots.len(), 20);
        // Centred on the plate, 100 mm apart, the parts standing on its top.
        let first = slots[0];
        assert!(
            (first - DVec3::new(-1.0, -0.15, 0.77 + 0.04)).length() < 1e-9,
            "{first}"
        );
        assert!((slots[1] - first - DVec3::new(0.1, 0.0, 0.0)).length() < 1e-9);
        ask(
            &mut cell,
            &doc,
            json!({"op": "fill", "object": plate.to_string()}),
        );
        assert_eq!(cell.parts().len(), 20);
        // Full: nothing more fits.
        ask(
            &mut cell,
            &doc,
            json!({"op": "fill", "object": plate.to_string()}),
        );
        assert_eq!(cell.parts().len(), 20);
        ask(
            &mut cell,
            &doc,
            json!({"op": "empty", "object": plate.to_string()}),
        );
        assert!(cell.parts().is_empty());
    }

    #[test]
    fn a_gripper_takes_a_part_carries_it_and_sets_it_in_the_chuck() {
        let (mut doc, plate, machine, grip) = scene();
        let mut cell = Cell::default();
        for (object, kind) in [(plate, "infeed"), (machine, "cnc"), (grip, "gripper")] {
            ask(
                &mut cell,
                &doc,
                json!({"op": "role", "object": object.to_string(), "kind": kind}),
            );
        }
        ask(
            &mut cell,
            &doc,
            json!({"op": "fill", "object": plate.to_string()}),
        );
        let pallet = cell.components()[0].clone();
        let first = cell.slots(&doc, &pallet)[0];

        // Too far: nothing to take.
        move_grip(&mut doc, grip, first + DVec3::Z * 0.1);
        let a = ask(
            &mut cell,
            &doc,
            json!({"op": "grip", "gripper": grip.to_string()}),
        );
        assert_eq!(a["ok"], false);
        // At the part: taken.
        move_grip(&mut doc, grip, first);
        let a = ask(
            &mut cell,
            &doc,
            json!({"op": "grip", "gripper": grip.to_string()}),
        );
        assert_eq!(a["ok"], true, "{a}");
        assert!(cell.held().len() == 1);

        // Carried: it follows the grip point in the scene shown.
        let seat = cell
            .seat(&doc, &cell.components()[1])
            .expect("the chuck's seat");
        // From the chuck's mesh, whose points are f32.
        assert!(
            (seat - DVec3::new(0.0, 1.1, 0.97 + 0.04)).length() < 1e-6,
            "{seat}"
        );
        move_grip(&mut doc, grip, seat + DVec3::new(0.01, 0.0, 0.005));
        let mut shown = doc.clone();
        let changes = cell.apply(&mut shown);
        let uid = cell.holding(grip).unwrap().uid;
        assert!(changes
            .changed
            .contains(&(DatablockKind::Object, Uid::from_raw(uid))));
        let id = shown
            .object_by_uid(Uid::from_raw(uid))
            .expect("the part is in the scene shown");
        let at = shown.world_transform(id).translation;
        assert!((at - (seat + DVec3::new(0.01, 0.0, 0.005))).length() < 1e-9);

        // Let go near the chuck: it settles into it.
        let a = ask(
            &mut cell,
            &doc,
            json!({"op": "release", "gripper": grip.to_string()}),
        );
        assert_eq!(a["ok"], true, "{a}");
        assert!(cell.held().is_empty());
        let part = cell.parts().iter().find(|p| p.uid == uid).unwrap();
        assert!((part.world.translation - seat).length() < 1e-9);
        // And the place it left on the pallet is free again.
        let places = cell.places(&doc);
        assert!(places
            .iter()
            .any(|(p, taken)| (*p - first).length() < 1e-9 && !taken));
    }

    #[test]
    fn roles_are_read_from_names() {
        let (mut doc, plate, machine, _) = scene();
        let door = doc.add_mesh_object("cnc_1_door", kreate_mesh::primitives::cube(0.5));
        let machine_id = id_of(&doc, machine).unwrap();
        doc.set_parent(door, Some(machine_id)).unwrap();
        // A group holding the machine is not a machine.
        let group = doc.add_object(Object::new("line_1", ObjectData::Empty));
        doc.set_parent(machine_id, Some(group)).unwrap();
        // The pallet's table is not a pallet.
        doc.add_mesh_object("infeed_table", kreate_mesh::primitives::cube(1.0));
        let found = guess(&doc);
        assert_eq!(found, vec![(plate, Kind::Infeed), (machine, Kind::Cnc)]);
    }

    #[test]
    fn a_machine_with_its_role_is_not_marked_again_by_a_part_of_it() {
        let mut doc = Document::new();
        let item = doc.add_object(Object::new("CNC 선반 1", ObjectData::Empty));
        let inner = doc.add_object(Object::new("machine", ObjectData::Empty));
        doc.set_parent(inner, Some(item)).unwrap();
        for part in ["door", "chuck"] {
            let id = doc.add_mesh_object(part, kreate_mesh::primitives::cube(0.2));
            doc.set_parent(id, Some(inner)).unwrap();
        }
        let item_uid = doc.object_uid(item).unwrap().raw();
        let mut cell = Cell::default();
        cell.ensure(item_uid, "CNC 선반 1".into(), Kind::Cnc);
        let a = ask(&mut cell, &doc, json!({"op": "auto"}));
        assert_eq!(a["added"], 0, "{a}");
        assert_eq!(cell.components().len(), 1);

        // A project saved with a role on both is read back with one.
        let inner_uid = doc.object_uid(inner).unwrap().raw();
        let saved = [(inner_uid, "machine"), (item_uid, "CNC 선반 1")]
            .map(|(uid, name)| json!({"object": uid.to_string(), "name": name, "kind": "cnc"}));
        let mut again = Cell::from_saved(saved.to_vec());
        again.adopt(&doc);
        assert_eq!(again.components().len(), 1);
        assert_eq!(again.components()[0].object, item_uid);
    }

    #[test]
    fn a_grip_point_is_the_last_link_not_the_gripper_holding_it() {
        const ARM: &str = r#"<?xml version="1.0"?>
<robot name="arm">
  <link name="base"/>
  <joint name="turn" type="revolute">
    <parent link="base"/><child link="arm"/><axis xyz="0 0 1"/>
    <limit lower="-3" upper="3" effort="1" velocity="1"/>
  </joint>
  <link name="arm"/>
  <joint name="arm-gripper" type="fixed"><parent link="arm"/><child link="gripper"/></joint>
  <link name="gripper"/>
  <joint name="gripper-grip_a" type="fixed"><parent link="gripper"/><child link="grip_a"/></joint>
  <link name="grip_a"/>
</robot>
"#;
        let mut doc = Document::new();
        let none = |_: &std::path::Path| -> Result<kreate_io::ir::ImportScene, String> {
            Ok(kreate_io::ir::ImportScene::new(
                kreate_io::ir::SourceMetadata::new("none"),
            ))
        };
        let robot = kreate_io::urdf::parse_urdf(ARM, std::path::Path::new("."), &none).unwrap();
        kreate_io::build::build_into(&robot, &mut doc);
        let grip_a = doc
            .objects()
            .find(|(_, o)| o.name == "grip_a")
            .and_then(|(id, _)| doc.object_uid(id))
            .unwrap()
            .raw();
        assert_eq!(guess(&doc), vec![(grip_a, Kind::Gripper)]);
    }

    #[test]
    fn a_machine_runs_its_cycle_only_shut_and_clamped_and_finishes_the_part() {
        let (mut doc, plate, machine, grip) = scene();
        let door = doc.add_mesh_object("cnc_1_door", kreate_mesh::primitives::cube(1.0));
        {
            let o = doc.object_mut(door).unwrap();
            o.transform.scale = DVec3::new(0.6, 0.03, 0.7);
            o.transform.translation = DVec3::new(0.0, 0.7, 1.2);
        }
        let m = id_of(&doc, machine).unwrap();
        doc.set_parent(door, Some(m)).unwrap();
        let door_uid = doc.object_uid(door).unwrap().raw();
        let mut cell = Cell::default();
        for (object, kind) in [(plate, "infeed"), (machine, "cnc"), (grip, "gripper")] {
            ask(
                &mut cell,
                &doc,
                json!({"op": "role", "object": object.to_string(), "kind": kind}),
            );
        }
        let act = |cell: &mut Cell, doc: &Document, action: &str| {
            ask(
                cell,
                doc,
                json!({"op": "machine", "object": machine.to_string(), "action": action}),
            )
        };
        let t0 = Instant::now();
        let at = |s: f64| t0 + Duration::from_secs_f64(s);
        cell.tick(&doc, 0.0, at(0.0));

        // The door opens over its 3 s, and slides aside by its width.
        assert_eq!(act(&mut cell, &doc, "open")["ok"], true);
        assert!(cell.tick(&doc, 1.5, at(1.5)), "moving: reported");
        assert!((cell.machines[&machine].door - 0.5).abs() < 1e-9);
        cell.tick(&doc, 3.5, at(3.5));
        assert_eq!(cell.machines[&machine].door, 1.0);
        let mut shown = doc.clone();
        let changes = cell.apply(&mut shown);
        assert!(changes
            .changed
            .contains(&(DatablockKind::Object, Uid::from_raw(door_uid))));
        let moved = shown
            .world_transform(id_of(&shown, door_uid).unwrap())
            .translation;
        assert!(
            (moved - DVec3::new(0.62, 0.7, 1.2)).length() < 1e-6,
            "{moved}"
        );

        // Not started while open, nor empty, nor unclamped.
        let a = act(&mut cell, &doc, "start");
        assert!(a["error"].as_str().unwrap().contains("문"), "{a}");
        // A raw part into the chuck.
        ask(
            &mut cell,
            &doc,
            json!({"op": "fill", "object": plate.to_string()}),
        );
        let first = cell.slots(&doc, &cell.components[0])[0];
        move_grip(&mut doc, grip, first);
        ask(
            &mut cell,
            &doc,
            json!({"op": "grip", "gripper": grip.to_string()}),
        );
        let seat = cell.seat(&doc, &cell.components[1]).unwrap();
        move_grip(&mut doc, grip, seat);
        ask(
            &mut cell,
            &doc,
            json!({"op": "release", "gripper": grip.to_string()}),
        );
        act(&mut cell, &doc, "close");
        cell.tick(&doc, 7.0, at(7.0));
        let a = act(&mut cell, &doc, "start");
        assert!(a["error"].as_str().unwrap().contains("척"), "{a}");
        act(&mut cell, &doc, "clamp");
        // Clamped: the part cannot be taken.
        let a = ask(
            &mut cell,
            &doc,
            json!({"op": "grip", "gripper": grip.to_string()}),
        );
        assert!(a["error"].as_str().unwrap().contains("척"), "{a}");
        assert_eq!(act(&mut cell, &doc, "start")["ok"], true);
        assert_eq!(
            act(&mut cell, &doc, "open")["ok"],
            false,
            "not while machining"
        );
        assert_eq!(act(&mut cell, &doc, "unclamp")["ok"], false);

        // 90 s later the part in the chuck is finished.
        cell.tick(&doc, 50.0, at(50.0));
        assert!(cell.is_running());
        assert!(
            cell.tick(&doc, 97.5, at(97.5)),
            "the cycle's end is reported"
        );
        assert!(!cell.is_running());
        let part = cell
            .parts()
            .iter()
            .find(|p| (p.world.translation - seat).length() < SNAP)
            .unwrap();
        assert!(part.finished);
        let json: Value = serde_json::from_str(&cell.to_json(&doc)).unwrap();
        assert_eq!(json["components"][1]["machine"]["part"], "finished");
    }

    #[test]
    fn roles_are_saved_and_found_again_by_name() {
        let (doc, plate, _, _) = scene();
        let mut cell = Cell::default();
        ask(
            &mut cell,
            &doc,
            json!({"op": "role", "object": plate.to_string(), "kind": "infeed", "props": {"rows": 2, "cols": 3}}),
        );
        assert!(cell.take_roles_changed());
        let saved = cell.to_saved();

        // A scene numbered anew.
        let mut other = Document::new();
        other.add_object(Object::new("floor", ObjectData::Empty));
        let plate2 = other.add_mesh_object("infeed", kreate_mesh::primitives::cube(1.0));
        let mut again = Cell::from_saved(saved);
        again.adopt(&other);
        let c = &again.components()[0];
        assert_eq!(c.object, other.object_uid(plate2).unwrap().raw());
        assert_eq!((c.props.rows, c.props.cols), (2, 3));
        assert_eq!(c.kind, Kind::Infeed);
    }

    #[test]
    fn the_viewer_is_told_what_left() {
        let (doc, plate, _, _) = scene();
        let mut cell = Cell::default();
        ask(
            &mut cell,
            &doc,
            json!({"op": "role", "object": plate.to_string(), "kind": "outfeed"}),
        );
        ask(
            &mut cell,
            &doc,
            json!({"op": "fill", "object": plate.to_string()}),
        );
        assert!(
            cell.parts().iter().all(|p| p.finished),
            "an outfeed fills with finished parts"
        );
        let mut shown = doc.clone();
        cell.apply(&mut shown);
        ask(&mut cell, &doc, json!({"op": "clear_parts"}));
        let changes = cell.apply(&mut doc.clone());
        assert_eq!(changes.removed.len(), 20);
    }
}
