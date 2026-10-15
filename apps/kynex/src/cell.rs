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
    /// The chuck all the way open: a part can go in or come out.
    Unclamped,
    /// Not machining.
    Idle,
    /// Not machining, and a finished part in the chuck.
    Done,
    /// A conveyor's sensor at its end: a part has come to it.
    PartAtEnd,
    /// An AMR at rest where it was sent.
    Arrived,
}

impl Signal {
    pub const ALL: [Signal; 8] = [
        Signal::Arrived,
        Signal::DoorOpen,
        Signal::DoorClosed,
        Signal::Clamped,
        Signal::Unclamped,
        Signal::Idle,
        Signal::Done,
        Signal::PartAtEnd,
    ];

    pub fn tag(self) -> &'static str {
        match self {
            Signal::DoorOpen => "door_open",
            Signal::DoorClosed => "door_closed",
            Signal::Clamped => "clamped",
            Signal::Unclamped => "unclamped",
            Signal::Idle => "idle",
            Signal::Done => "done",
            Signal::PartAtEnd => "part_at_end",
            Signal::Arrived => "arrived",
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
    /// An injection moulding machine: as a CNC machine, an object with its
    /// door (guard) and its mould among its children (`…door…`,
    /// `…mould…`). It makes its parts itself: at the end of each cycle a
    /// moulded part stands in the mould, to be taken out.
    Molder,
    /// A belt conveyor: the belt, a box whose top it carries parts along,
    /// along its own X, to its end.
    Conveyor,
    /// An autonomous mobile robot: an object with its top plate below it
    /// (`…top…`), which parts ride on. It drives to stops.
    Amr,
    /// Where an AMR stops: its centre there, facing the stop's X.
    Stop,
    /// A positioner: a turntable, turned about its own vertical axis to an
    /// angle from where it was placed, its top plate (`…top…`) and the
    /// parts on it with it.
    Positioner,
    /// A machine's door, or any part of it that moves as its door does: it
    /// slides along, or swings about, an axis of its own as the machine
    /// opens it. A machine's part named `…door…` is its door without one.
    Door,
    /// A connection point on equipment of the user's own: where a robot
    /// stands, where a conveyor's belt starts or ends, or where this
    /// stands on another's mount.
    Connector,
}

impl Kind {
    pub const ALL: [Kind; 12] = [
        Kind::Amr,
        Kind::Stop,
        Kind::Cnc,
        Kind::Infeed,
        Kind::Outfeed,
        Kind::Gripper,
        Kind::Fence,
        Kind::Molder,
        Kind::Conveyor,
        Kind::Positioner,
        Kind::Door,
        Kind::Connector,
    ];

    pub fn tag(self) -> &'static str {
        match self {
            Kind::Cnc => "cnc",
            Kind::Infeed => "infeed",
            Kind::Outfeed => "outfeed",
            Kind::Gripper => "gripper",
            Kind::Fence => "fence",
            Kind::Molder => "molder",
            Kind::Conveyor => "conveyor",
            Kind::Amr => "amr",
            Kind::Stop => "amr_stop",
            Kind::Positioner => "positioner",
            Kind::Door => "door",
            Kind::Connector => "connector",
        }
    }

    pub fn of(tag: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.tag() == tag)
    }

    fn pallet(self) -> bool {
        matches!(self, Kind::Infeed | Kind::Outfeed)
    }

    /// A machine a robot tends: a door, a cycle, a part inside.
    pub fn machine(self) -> bool {
        matches!(self, Kind::Cnc | Kind::Molder)
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
    /// Conveyor: its belt's speed, metres a second (an AMR's too; a
    /// positioner's turn, radians a second); every how many seconds
    /// a raw part is put on at its start (0: none); and whether a part
    /// that reaches its end is taken away (1) or waits there (0).
    pub speed: f64,
    pub feed: f64,
    pub drain: f64,
    /// Door: whether it swings (1) about its axis rather than slides (0)
    /// along it; the axis, its own X, Y or Z (0, 1, 2); how far it slides,
    /// metres (0: aside by its own width, along its machine's X); how far
    /// it swings, radians.
    pub swing: f64,
    pub axis: u32,
    pub travel: f64,
    pub angle: f64,
    /// Door: the point its hinge passes through; connector: where it is.
    /// In the object's own frame.
    pub point: [f64; 3],
    /// Connector: what it takes (see [`PLUGS`]).
    pub plug: u32,
}

/// What a connector takes, by its number: a robot standing on it, a
/// conveyor's start (where parts come on), its end, or this standing on
/// another's mount.
pub const PLUGS: [&str; 4] = ["mount", "belt_in", "belt_out", "foot"];

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
            speed: 0.2,
            feed: 0.0,
            drain: 0.0,
            swing: 0.0,
            axis: 0,
            travel: 0.0,
            angle: std::f64::consts::FRAC_PI_2,
            point: [0.0; 3],
            plug: 0,
        }
    }
}

impl Props {
    /// The typical values for a component of kind `kind`.
    pub fn for_kind(kind: Kind) -> Self {
        match kind {
            // A moulding cycle of half a minute; a guard quicker than a
            // machine tool's door.
            Kind::Molder => Self {
                cycle: 30.0,
                door: 1.5,
                ..Self::default()
            },
            // A mobile base's usual pace indoors among people.
            Kind::Amr => Self {
                speed: 1.0,
                ..Self::default()
            },
            // A positioner turns some 90° a second: radians a second.
            Kind::Positioner => Self {
                speed: std::f64::consts::FRAC_PI_2,
                ..Self::default()
            },
            _ => Self::default(),
        }
    }

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
        self.speed = v["speed"]
            .as_f64()
            .map_or(self.speed, |x| x.clamp(0.01, 3.0));
        self.feed = time("feed", self.feed);
        self.drain = v["drain"]
            .as_f64()
            .map_or(self.drain, |x| if x > 0.5 { 1.0 } else { 0.0 });
        self.swing = v["swing"]
            .as_f64()
            .map_or(self.swing, |x| if x > 0.5 { 1.0 } else { 0.0 });
        self.axis = v["axis"]
            .as_f64()
            .map_or(self.axis, |x| x.clamp(0.0, 2.0) as u32);
        self.travel = v["travel"]
            .as_f64()
            .map_or(self.travel, |x| x.clamp(-10.0, 10.0));
        self.angle = v["angle"].as_f64().map_or(self.angle, |x| {
            x.clamp(-std::f64::consts::TAU, std::f64::consts::TAU)
        });
        if let Some([x, y, z]) = v["point"].as_array().map(Vec::as_slice) {
            if let (Some(x), Some(y), Some(z)) = (x.as_f64(), y.as_f64(), z.as_f64()) {
                self.point = [x, y, z].map(|c| c.clamp(-100.0, 100.0));
            }
        }
        self.plug = v["plug"]
            .as_f64()
            .map_or(self.plug, |x| x.clamp(0.0, (PLUGS.len() - 1) as f64) as u32);
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
            "speed": self.speed,
            "feed": self.feed,
            "drain": self.drain,
            "swing": self.swing,
            "axis": self.axis,
            "travel": self.travel,
            "angle": self.angle,
            "point": self.point,
            "plug": self.plug,
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
    /// The controller's digital I/O it is wired to, by what it does (see
    /// [`io_points`]): 0–7 the standard ones, 8–15 the configurable, 16
    /// and 17 the tool's.
    pub io: BTreeMap<String, u32>,
    /// Where on the PLC it is wired, by what it does (see [`io_points`]
    /// and `crate::plc`): a Modbus coil's number or an OPC UA node's id. A
    /// command is given as its address comes on; a state is written to
    /// its address.
    pub plc: BTreeMap<String, String>,
}

/// What a component of kind `kind` can be wired to: the robot's outputs
/// to it, and its outputs to the robot (the robot's inputs).
pub fn io_points(kind: Kind) -> (&'static [&'static str], &'static [&'static str]) {
    match kind {
        Kind::Gripper => (&["close"], &["closed"]),
        Kind::Cnc => (
            &["open", "close", "clamp", "unclamp", "start"],
            &[
                "door_open",
                "door_closed",
                "clamped",
                "unclamped",
                "idle",
                "done",
            ],
        ),
        Kind::Molder => (
            &["open", "close", "start"],
            &["door_open", "door_closed", "idle", "done"],
        ),
        Kind::Conveyor => (&[], &["part_at_end"]),
        _ => (&[], &[]),
    }
}

/// The highest I/O number: the tool's second output or input.
pub const LAST_IO: u32 = 17;

/// The longest PLC address KYNEX keeps: room for an OPC UA node id.
pub const LONGEST_ADDRESS: usize = 200;

/// A PLC map as saved: `{"open": "100", "done": "ns=2;s=Lathe.Done", …}`,
/// a coil's number as a number from before addresses were text.
fn plc_of(v: &Value) -> BTreeMap<String, String> {
    v.as_object()
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| {
                    let a = match v {
                        Value::Number(n) => n.to_string(),
                        Value::String(s) => s.trim().to_string(),
                        _ => return None,
                    };
                    (!a.is_empty() && a.len() <= LONGEST_ADDRESS).then(|| (k.clone(), a))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// An I/O map as saved: `{"close": 0, …}`.
fn io_of(v: &Value) -> BTreeMap<String, u32> {
    v.as_object()
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| {
                    Some((
                        k.clone(),
                        v.as_u64().filter(|n| *n <= LAST_IO as u64)? as u32,
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
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
    /// Whether the chuck is told to clamp (or has), rather than let go.
    pub clamped: bool,
    /// How far closed the chuck is: 0 open, 1 clamped. It closes and opens
    /// over the machine's chuck time.
    pub chuck: f64,
    /// Seconds into the cycle, while machining.
    pub machining: Option<f64>,
    /// Cycles finished since the cell started.
    pub cycles: u64,
}

impl Machine {
    pub fn shut(&self) -> bool {
        self.door <= 0.0 && !self.opening
    }

    /// Whether the chuck has closed on what is in it.
    pub fn gripping(&self) -> bool {
        self.clamped && self.chuck >= 1.0
    }

    /// Whether the chuck holds what is in it, however little: not open.
    pub fn holding(&self) -> bool {
        self.clamped || self.chuck > 0.0
    }
}

/// A conveyor's belt where it stands: the middle of its top, its axes
/// (along it, across it) and its size.
struct Belt {
    object: u64,
    props: Props,
    centre: DVec3,
    along: DVec3,
    across: DVec3,
    length: f64,
    width: f64,
}

impl Belt {
    /// How far along the belt from its middle part `p` stands, if it
    /// stands on it.
    fn along_of(&self, p: &Part) -> Option<f64> {
        let foot = p.world.translation - DVec3::Z * (p.size().1 / 2.0);
        let d = foot - self.centre;
        let (x, y) = (d.dot(self.along), d.dot(self.across));
        (x.abs() <= self.length / 2.0 + 1e-6 && y.abs() <= self.width / 2.0 && d.z.abs() < 0.01)
            .then_some(x)
    }

    /// The part waiting at its end, by index.
    fn at_end(&self, parts: &[Part]) -> Option<usize> {
        let end = self.length / 2.0 - RAW.0;
        parts
            .iter()
            .position(|p| p.held.is_none() && self.along_of(p).is_some_and(|x| x >= end - 0.003))
    }

    /// Its top's height under `at`, a part's centre `half` its height up,
    /// if `at` is over the belt and not far above it.
    fn top_under(&self, at: DVec3, half: f64) -> Option<f64> {
        let d = at - self.centre;
        let (x, y) = (d.dot(self.along), d.dot(self.across));
        let above = d.z - half;
        (x.abs() <= self.length / 2.0
            && y.abs() <= self.width / 2.0
            && (-0.01..0.1).contains(&above))
        .then_some(self.centre.z)
    }
}

/// An AMR's or a positioner's top plate: where it stands now, and the
/// plate's box from its middle.
struct Deck {
    object: u64,
    pose: DAffine3,
    lo: DVec3,
    hi: DVec3,
}

impl Deck {
    /// Whether part `p` stands on the plate.
    fn carries(&self, p: &Part) -> bool {
        let foot = self
            .pose
            .inverse()
            .transform_point3(p.world.translation - DVec3::Z * (p.size().1 / 2.0));
        foot.x >= self.lo.x
            && foot.x <= self.hi.x
            && foot.y >= self.lo.y
            && foot.y <= self.hi.y
            && (foot.z - self.hi.z).abs() < 0.01
    }

    /// The plate's height under `at`, a part's centre `half` its height
    /// up, if `at` is over the plate and not far above it.
    fn top_under(&self, at: DVec3, half: f64) -> Option<f64> {
        let l = self.pose.inverse().transform_point3(at);
        let above = l.z - half - self.hi.z;
        (l.x >= self.lo.x
            && l.x <= self.hi.x
            && l.y >= self.lo.y
            && l.y <= self.hi.y
            && (-0.01..0.1).contains(&above))
        .then(|| {
            self.pose
                .transform_point3(DVec3::new(0.0, 0.0, self.hi.z))
                .z
        })
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
    /// Each feeding conveyor's seconds since it last put a part on.
    fed: BTreeMap<u64, f64>,
    /// Whether a conveyor carried a part on its last step.
    conveying: bool,
    /// Parts taken away at a conveyor's end since last counted.
    drained: usize,
    /// Each AMR that has been sent anywhere: where it is, and the legs of
    /// its way still to go.
    rovers: BTreeMap<u64, Rover>,
    /// Each PLC command as last read, by address: a command is given as
    /// its address comes on.
    plc_last: BTreeMap<(u64, String), bool>,
    /// The signals shared with the PLC besides the machines', each one's
    /// value now, and each input's as last read (for its edges).
    signals: Vec<PlcSignal>,
    signal_values: BTreeMap<String, i64>,
    signal_last: BTreeMap<String, i64>,
}

/// A signal the cell shares with a PLC besides its machines' commands and
/// states: one the PLC writes and the cell reads (an input), or the other
/// way round, a bit or a number, at an address on the PLC. What it is tied
/// to in the cell, if anything, is its link; a signal tied to nothing is
/// the robot programs' to write or wait on.
#[derive(Clone, Debug, PartialEq)]
pub struct PlcSignal {
    pub name: String,
    pub input: bool,
    pub number: bool,
    pub address: String,
    pub link: Link,
}

/// What a PLC signal is tied to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    None,
    /// An input: robot `root`'s program starts as it comes on.
    Start(u64),
    /// An input: robot `root`'s program stops as it comes on.
    Stop(u64),
    /// An output: on while robot `root`'s program runs.
    Running(u64),
    /// An output: on once robot `root`'s program has run to its end.
    Done(u64),
    /// An output: how many cycles machine `object` has finished.
    Cycles(u64),
}

impl Link {
    fn tag(self) -> String {
        match self {
            Link::None => String::new(),
            Link::Start(u) => format!("start:{u}"),
            Link::Stop(u) => format!("stop:{u}"),
            Link::Running(u) => format!("running:{u}"),
            Link::Done(u) => format!("done:{u}"),
            Link::Cycles(u) => format!("cycles:{u}"),
        }
    }

    fn of(tag: &str) -> Option<Link> {
        if tag.is_empty() {
            return Some(Link::None);
        }
        let (what, uid) = tag.split_once(':')?;
        let uid: u64 = uid.parse().ok()?;
        Some(match what {
            "start" => Link::Start(uid),
            "stop" => Link::Stop(uid),
            "running" => Link::Running(uid),
            "done" => Link::Done(uid),
            "cycles" => Link::Cycles(uid),
            _ => return None,
        })
    }

    /// Whether it is tied to what the PLC reads, rather than writes.
    fn output(self) -> bool {
        matches!(self, Link::Running(_) | Link::Done(_) | Link::Cycles(_))
    }
}

/// How many signals a cell keeps, at most.
pub const MOST_SIGNALS: usize = 256;

impl PlcSignal {
    fn of(v: &Value) -> Result<Self, String> {
        let name: String = v["name"]
            .as_str()
            .unwrap_or("")
            .trim()
            .chars()
            .take(40)
            .collect();
        if name.is_empty() {
            return Err("신호 이름이 없습니다".into());
        }
        let address: String = v["address"].as_str().unwrap_or("").trim().to_string();
        if address.len() > LONGEST_ADDRESS {
            return Err(format!("{name}: 주소는 {LONGEST_ADDRESS}자까지입니다"));
        }
        let input = v["input"].as_bool().unwrap_or(false);
        let link = Link::of(v["link"].as_str().unwrap_or(""))
            .ok_or_else(|| format!("{name}: 연결을 알 수 없습니다"))?;
        if link != Link::None && link.output() == input {
            return Err(format!("{name}: 방향과 연결이 맞지 않습니다"));
        }
        Ok(Self {
            name,
            input,
            number: v["number"].as_bool().unwrap_or(false),
            address,
            link,
        })
    }

    fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "input": self.input,
            "number": self.number,
            "address": self.address,
            "link": self.link.tag(),
        })
    }

    fn kind(&self) -> crate::plc::Kind {
        if self.number {
            crate::plc::Kind::Number
        } else {
            crate::plc::Kind::Bit
        }
    }
}

/// What an exchange with a PLC asks of the robots, and what it writes.
#[derive(Debug, Default, PartialEq)]
pub struct Exchange {
    pub writes: Vec<(String, crate::plc::Kind, i64)>,
    pub errors: Vec<String>,
    /// Robots whose programs are to start, and to stop.
    pub start: Vec<u64>,
    pub stop: Vec<u64>,
}

/// An AMR on its way: where its centre is on the floor and which way it
/// faces, and what it still has to do.
#[derive(Clone, Debug, PartialEq)]
struct Rover {
    at: DVec3,
    yaw: f64,
    legs: Vec<Leg>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Leg {
    /// Turn on the spot to face this way.
    Turn(f64),
    /// Drive straight to here.
    Go(DVec3),
}

/// How fast an AMR turns on the spot, radians a second.
const AMR_TURN: f64 = 1.0;

/// Where a placement stands on the floor, and which way its X faces.
fn floor_pose(world: &DAffine3) -> (DVec3, f64) {
    let x = world.matrix3.x_axis;
    (world.translation, x.y.atan2(x.x))
}

/// The placement at `at` facing `yaw`, upright.
fn pose_of(at: DVec3, yaw: f64) -> DAffine3 {
    DAffine3::from_rotation_translation(kreate_math::DQuat::from_rotation_z(yaw), at)
}

/// From `from` to `to` the short way round, radians.
fn turn_between(from: f64, to: f64) -> f64 {
    (to - from + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
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
        let molder = |at: ObjectId| {
            part_named(doc, at, "door").is_some() && part_named(doc, at, "mould").is_some()
        };
        let kind = if machine(id)
            && !doc
                .objects()
                .any(|(c, o)| o.parent == Some(id) && machine(c))
        {
            Some(Kind::Cnc)
        } else if molder(id)
            && !doc
                .objects()
                .any(|(c, o)| o.parent == Some(id) && molder(c))
        {
            Some(Kind::Molder)
        } else if !link && object.mesh().is_some() && (name == "belt" || name.contains("conveyor"))
        {
            Some(Kind::Conveyor)
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
pub fn is_below(doc: &Document, id: ObjectId, parent: ObjectId) -> bool {
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
                    let mut props = Props::for_kind(kind);
                    props.update(&saved["props"]);
                    self.components.push(Component {
                        object,
                        name: name.to_string(),
                        kind,
                        props,
                        io: io_of(&saved["io"]),
                        plc: plc_of(&saved["plc"]),
                    });
                    self.changed = true;
                }
                // A role equipment came with: the settings saved win over
                // its defaults.
                (Some(object), Some(kind)) => {
                    if let Some(c) = self.components.iter_mut().find(|c| c.object == object) {
                        c.io = io_of(&saved["io"]);
                        c.plc = plc_of(&saved["plc"]);
                        c.kind = kind;
                        c.props.update(&saved["props"]);
                        self.changed = true;
                    }
                }
                _ => waiting.push(saved),
            }
        }
        self.pending = waiting;
        // One machine is one role: a role on a part of a machine, an AMR
        // or a stop that has one already (saved before auto roles, or the
        // library, knew better) is dropped.
        let whole = |k: Kind| {
            matches!(
                k,
                Kind::Cnc | Kind::Molder | Kind::Amr | Kind::Stop | Kind::Positioner
            )
        };
        let machines: Vec<ObjectId> = self
            .components
            .iter()
            .filter(|c| whole(c.kind))
            .filter_map(|c| id_of(doc, c.object))
            .collect();
        self.components.retain(|c| {
            !whole(c.kind)
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
                    "io": c.io,
                    "plc": c.plc,
                })
            })
            .chain(self.pending.iter().cloned())
            .collect()
    }

    /// Gives `object` the role `kind` unless it has one: the role equipment
    /// from the library comes with. A role changed by hand is kept.
    /// The roles of object `root` and everything below it in `doc`, each
    /// with its settings: what equipment saved from them comes with.
    pub fn roles_under(&self, doc: &Document, root: ObjectId) -> Vec<(ObjectId, Kind, Value)> {
        self.components
            .iter()
            .filter_map(|c| {
                let id = id_of(doc, c.object)?;
                (id == root || is_below(doc, id, root)).then(|| (id, c.kind, c.props.to_json()))
            })
            .collect()
    }

    /// Sets the settings `v` names on object `object`'s role.
    pub fn set_props(&mut self, object: u64, v: &Value) {
        if let Some(c) = self.components.iter_mut().find(|c| c.object == object) {
            c.props.update(v);
            self.changed = true;
            self.roles_changed = true;
        }
    }

    pub fn ensure(&mut self, object: u64, name: String, kind: Kind) {
        if self.components.iter().any(|c| c.object == object) {
            return;
        }
        self.components.push(Component {
            object,
            name,
            kind,
            props: Props::for_kind(kind),
            io: BTreeMap::new(),
            plc: BTreeMap::new(),
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
        // The PLC's signals are the project's, not the scenario's.
        let signals = std::mem::take(&mut self.signals);
        *self = Self::from_saved(saved);
        self.signals = signals;
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
        // And those carried away at a conveyor's end.
        taken + self.take_drained()
    }

    /// The same, as an operator changes whole pallets, for the pallets in
    /// `swapped` — those a program goes through place by place: an infeed
    /// is filled again once it has no raw part left, and an outfeed
    /// emptied once it has no free place. The other pallets are kept
    /// supplied place by place, as [`Cell::supply`] does.
    pub fn swap_pallets(&mut self, doc: &Document, swapped: &[u64]) {
        let pallets: Vec<Component> = self
            .components
            .iter()
            .filter(|c| c.kind.pallet())
            .cloned()
            .collect();
        for c in pallets {
            let slots = self.slots(doc, &c);
            let on = |p: &Part| {
                p.held.is_none()
                    && slots
                        .iter()
                        .any(|at| p.world.translation.distance(*at) < SNAP)
            };
            let swap = swapped.contains(&c.object);
            if c.kind == Kind::Infeed {
                let empty = !self.parts.iter().any(|p| !p.finished && on(p));
                if !swap || empty {
                    for at in slots.clone() {
                        let free = !self
                            .parts
                            .iter()
                            .any(|p| p.held.is_none() && p.world.translation.distance(at) < SNAP);
                        if free {
                            self.add_part(at, false);
                        }
                    }
                }
            } else {
                let full = slots.iter().all(|at| {
                    self.parts
                        .iter()
                        .any(|p| p.held.is_none() && p.world.translation.distance(*at) < SNAP)
                });
                if !swap || full {
                    let (gone, kept): (Vec<Part>, Vec<Part>) = std::mem::take(&mut self.parts)
                        .into_iter()
                        .partition(|p| p.finished && on(p));
                    self.parts = kept;
                    self.removed.extend(gone.iter().map(|p| p.uid));
                    self.changed |= !gone.is_empty();
                }
            }
        }
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

    /// The parts standing on component `c` in `doc`, by index: in its
    /// places (a pallet's, a chuck or mould), or on its belt.
    fn parts_on(&self, doc: &Document, c: &Component) -> Vec<usize> {
        let mut places = self.slots(doc, c);
        places.extend(self.seat(doc, c));
        let belt = (c.kind == Kind::Conveyor)
            .then(|| self.belts(doc).into_iter().find(|b| b.object == c.object))
            .flatten();
        self.parts
            .iter()
            .enumerate()
            .filter(|(_, p)| p.held.is_none())
            .filter(|(_, p)| {
                places
                    .iter()
                    .any(|at| p.world.translation.distance(*at) < SNAP)
                    || belt.as_ref().is_some_and(|b| b.along_of(p).is_some())
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Equipment moved or removed between scene `before` and `after`: the
    /// parts on what moved go with it, and those on what went go too.
    pub fn carry(&mut self, before: &Document, after: &Document) {
        let mut gone = Vec::new();
        for c in self.components.clone() {
            let (Some(was), now) = (id_of(before, c.object), id_of(after, c.object)) else {
                continue;
            };
            let on = self.parts_on(before, &c);
            if on.is_empty() {
                continue;
            }
            match now {
                Some(now) => {
                    let moved = after.world_transform(now) * before.world_transform(was).inverse();
                    if moved.abs_diff_eq(DAffine3::IDENTITY, 1e-9) {
                        continue;
                    }
                    for i in on {
                        // Moved, not turned over: the part stays upright.
                        let p = &mut self.parts[i];
                        let (_, turn, _) = moved.to_scale_rotation_translation();
                        let at = moved.transform_point3(p.world.translation);
                        p.world = DAffine3::from_rotation_translation(turn, at) * {
                            let (_, r, _) = p.world.to_scale_rotation_translation();
                            DAffine3::from_quat(r)
                        };
                    }
                    self.changed = true;
                }
                None => gone.extend(on),
            }
        }
        if !gone.is_empty() {
            gone.sort_unstable_by(|a, b| b.cmp(a));
            gone.dedup();
            for i in gone {
                let p = self.parts.remove(i);
                self.removed.push(p.uid);
            }
            self.changed = true;
        }
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

    /// What a program's export needs of the cell: each component's wiring,
    /// the pallets' places, the grip points and how long each takes.
    pub fn export(&self, doc: &Document) -> crate::program::CellIo {
        let mut out = crate::program::CellIo::default();
        for c in &self.components {
            for (key, n) in &c.io {
                out.io.insert((c.object, key.clone()), *n);
            }
            if c.kind.pallet() {
                out.pallets.insert(c.object, self.slots(doc, c));
            }
            if c.kind == Kind::Gripper {
                out.grippers.push(c.object);
                out.actuate.insert(c.object, c.props.actuate);
            }
            if c.kind == Kind::Positioner {
                out.turns.insert(c.object, c.props.speed);
            }
        }
        out
    }

    /// Every grip point, by identity.
    pub fn grippers(&self) -> Vec<u64> {
        self.components
            .iter()
            .filter(|c| c.kind == Kind::Gripper)
            .map(|c| c.object)
            .collect()
    }

    /// The places of pallet `pallet`, by its identity: none if it is not
    /// one.
    pub fn pallet_slots(&self, doc: &Document, pallet: u64) -> Vec<DVec3> {
        self.components
            .iter()
            .find(|c| c.object == pallet && c.kind.pallet())
            .map(|c| self.slots(doc, c))
            .unwrap_or_default()
    }

    /// Where a part's centre stands in a CNC machine's chuck, or on an
    /// injection moulding machine's mould, made.
    pub fn seat(&self, doc: &Document, c: &Component) -> Option<DVec3> {
        let holder = match c.kind {
            Kind::Cnc => "chuck",
            Kind::Molder => "mould",
            _ => return None,
        };
        let chuck = part_named(doc, id_of(doc, c.object)?, holder)?;
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
                // Its wiring stays while its kind does.
                let was = self.components.iter().find(|c| c.object == object).cloned();
                self.components.retain(|c| c.object != object);
                if let Some(tag) = req["kind"].as_str() {
                    let kind = Kind::of(tag).ok_or("unknown role")?;
                    // What the request does not name stays as it was.
                    let mut props = was
                        .as_ref()
                        .filter(|c| c.kind == kind)
                        .map_or(Props::for_kind(kind), |c| c.props);
                    props.update(&req["props"]);
                    self.components.push(Component {
                        object,
                        name: doc.object(id).map(|o| o.name.clone()).unwrap_or_default(),
                        kind,
                        props,
                        io: was
                            .as_ref()
                            .filter(|c| c.kind == kind)
                            .map(|c| c.io.clone())
                            .unwrap_or_default(),
                        plc: was
                            .filter(|c| c.kind == kind)
                            .map(|c| c.plc)
                            .unwrap_or_default(),
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
                        props: Props::for_kind(kind),
                        io: BTreeMap::new(),
                        plc: BTreeMap::new(),
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
            // A door's hinge or a connector picked in the scene: from the
            // world into the object's own frame.
            "point" => {
                let object = uid("object")?;
                let id = id_of(doc, object).ok_or("no such object")?;
                let at = match req["at"].as_array().map(Vec::as_slice) {
                    Some([x, y, z]) => DVec3::new(
                        x.as_f64().ok_or("no at")?,
                        y.as_f64().ok_or("no at")?,
                        z.as_f64().ok_or("no at")?,
                    ),
                    _ => return Err("no at".into()),
                };
                let c = self
                    .components
                    .iter_mut()
                    .find(|c| c.object == object && matches!(c.kind, Kind::Door | Kind::Connector))
                    .ok_or("문이나 연결점 역할이 아닙니다")?;
                c.props.point = doc
                    .world_transform(id)
                    .inverse()
                    .transform_point3(at)
                    .to_array();
                self.roles_changed = true;
                self.changed = true;
            }
            "drive" => self.drive(doc, uid("object")?, uid("stop")?)?,
            "turn" => {
                let angle = req["angle"].as_f64().ok_or("no angle")?;
                self.turn(doc, uid("object")?, angle)?;
            }
            "io" => {
                let object = uid("object")?;
                let key = req["key"].as_str().ok_or("no key")?;
                let c = self
                    .components
                    .iter_mut()
                    .find(|c| c.object == object)
                    .ok_or("셀 역할이 없습니다")?;
                let (outs, ins) = io_points(c.kind);
                if !outs.contains(&key) && !ins.contains(&key) {
                    return Err(format!("{key}: 이 역할에 없는 I/O입니다"));
                }
                match req["port"].as_u64() {
                    Some(n) if n <= LAST_IO as u64 => {
                        c.io.insert(key.to_string(), n as u32);
                    }
                    Some(_) => return Err(format!("I/O 번호는 0–{LAST_IO}입니다")),
                    None => {
                        c.io.remove(key);
                    }
                }
                self.roles_changed = true;
                self.changed = true;
            }
            // A command or a state wired to a PLC's address, or no more.
            "plc" => {
                let object = uid("object")?;
                let key = req["key"].as_str().ok_or("no key")?;
                let c = self
                    .components
                    .iter_mut()
                    .find(|c| c.object == object)
                    .ok_or("셀 역할이 없습니다")?;
                let (outs, ins) = io_points(c.kind);
                if !outs.contains(&key) && !ins.contains(&key) {
                    return Err(format!("{key}: 이 역할에 없는 신호입니다"));
                }
                // A coil's number, or a node's id; none, or empty, for none.
                let address = match &req["address"] {
                    Value::Number(n) => n.to_string(),
                    Value::String(s) => s.trim().to_string(),
                    _ => String::new(),
                };
                if address.len() > LONGEST_ADDRESS {
                    return Err(format!("주소는 {LONGEST_ADDRESS}자까지입니다"));
                }
                if address.is_empty() {
                    c.plc.remove(key);
                } else {
                    c.plc.insert(key.to_string(), address);
                }
                self.roles_changed = true;
                self.changed = true;
            }
            // The PLC signal table, whole.
            "signals" => self.set_signals(&req["signals"])?,
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
            .filter(|c| self.machines.get(&c.object).is_some_and(Machine::holding))
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
        let belts = self.belts(doc);
        let decks = self.decks(doc);
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
        } else if let Some(top) = belts
            .iter()
            .filter_map(|b| b.top_under(here, part.size().1 / 2.0))
            .next()
        {
            // Let go over a belt: it stands on it, upright, to be carried.
            let half = part.size().1 / 2.0;
            part.world = DAffine3::from_translation(DVec3::new(here.x, here.y, top + half));
        } else if let Some(top) = decks
            .iter()
            .filter_map(|d| d.top_under(here, part.size().1 / 2.0))
            .next()
        {
            // On an AMR's top plate: it rides along.
            let half = part.size().1 / 2.0;
            part.world = DAffine3::from_translation(DVec3::new(here.x, here.y, top + half));
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
            .find(|c| c.object == machine && c.kind.machine())
            .cloned()
            .ok_or("명령을 받는 장비(CNC, 사출기)가 아닙니다")?;
        let in_chuck = self.in_chuck(doc, &c);
        let m = self.machines.entry(machine).or_default();
        let busy = m.machining.is_some();
        if c.kind == Kind::Molder {
            // A moulding machine: a guard and its cycle; it makes the part.
            match command {
                Command::Clamp | Command::Unclamp => return Err("사출기에는 척이 없습니다".into()),
                Command::Open if busy => return Err("성형 중에는 가드를 열 수 없습니다".into()),
                Command::Open => m.opening = true,
                Command::Close => m.opening = false,
                Command::Start => {
                    if busy {
                        return Err("이미 성형 중입니다".into());
                    }
                    if !m.shut() {
                        return Err("가드를 닫아야 성형을 시작할 수 있습니다".into());
                    }
                    if in_chuck.is_some() {
                        return Err("금형에 성형품이 남아 있습니다. 먼저 꺼내세요".into());
                    }
                    m.machining = Some(0.0);
                }
            }
            self.changed = true;
            return Ok(());
        }
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
                if !m.gripping() {
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
        let Some(c) = self.components.iter().find(|c| c.object == machine) else {
            return false;
        };
        if matches!(c.kind, Kind::Amr | Kind::Positioner) {
            return signal == Signal::Arrived
                && self.rovers.get(&machine).is_none_or(|r| r.legs.is_empty());
        }
        if c.kind == Kind::Conveyor {
            return signal == Signal::PartAtEnd
                && self
                    .belts(doc)
                    .iter()
                    .find(|b| b.object == machine)
                    .is_some_and(|b| b.at_end(&self.parts).is_some());
        }
        if !c.kind.machine() {
            return false;
        }
        let m = self.machines.get(&machine).cloned().unwrap_or_default();
        match signal {
            Signal::DoorOpen => m.door >= 1.0 && m.opening,
            Signal::DoorClosed => m.shut(),
            Signal::Clamped => m.gripping(),
            Signal::Unclamped => !m.clamped && m.chuck <= 0.0,
            Signal::Idle => m.machining.is_none(),
            Signal::Done => m.machining.is_none() && self.in_chuck(doc, c) == Some(true),
            Signal::PartAtEnd | Signal::Arrived => false,
        }
    }

    /// The PLC addresses the cell reads, and what each holds: its machines'
    /// commands, and the signals the PLC writes.
    pub fn plc_reads(&self) -> Vec<(String, crate::plc::Kind)> {
        let mut out: Vec<(String, crate::plc::Kind)> = self
            .components
            .iter()
            .flat_map(|c| {
                let (outs, _) = io_points(c.kind);
                c.plc
                    .iter()
                    .filter(move |(k, _)| outs.contains(&k.as_str()))
                    .map(|(_, a)| (a.clone(), crate::plc::Kind::Bit))
            })
            .chain(
                self.signals
                    .iter()
                    .filter(|s| s.input && !s.address.is_empty())
                    .map(|s| (s.address.clone(), s.kind())),
            )
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// One exchange with a PLC: `read` what it holds, by address, and
    /// `robot(root)` whether a robot's program runs, and whether it has run
    /// to its end. A command whose address has come on since last read is
    /// given; an error a command meets (a door opened while machining) is
    /// said, with the machine's name. Returns what to write, the robots to
    /// start and stop, and what went wrong.
    pub fn plc_exchange(
        &mut self,
        doc: &Document,
        read: &BTreeMap<String, i64>,
        robot: &dyn Fn(u64) -> (bool, bool),
    ) -> Exchange {
        let mut out = Exchange::default();
        for c in self.components.clone() {
            let (outs, _) = io_points(c.kind);
            for (key, address) in &c.plc {
                if !outs.contains(&key.as_str()) {
                    continue;
                }
                let Some(&value) = read.get(address) else {
                    continue;
                };
                let on = value != 0;
                // Each machine its own: two on one address both take it.
                let was = self
                    .plc_last
                    .insert((c.object, address.clone()), on)
                    .unwrap_or(false);
                if on && !was {
                    if let Some(command) = Command::of(key) {
                        if let Err(e) = self.command(doc, c.object, command) {
                            out.errors.push(format!("{}: {e}", c.name));
                        }
                    }
                }
            }
        }
        // A state's address, as each machine on it says it: one written
        // by two machines is on only when both say so, and said to be
        // shared.
        let mut states: BTreeMap<String, Vec<(&str, bool)>> = BTreeMap::new();
        for c in &self.components {
            let (_, ins) = io_points(c.kind);
            for (key, address) in &c.plc {
                if !ins.contains(&key.as_str()) {
                    continue;
                }
                if let Some(signal) = Signal::of(key) {
                    states
                        .entry(address.clone())
                        .or_default()
                        .push((c.name.as_str(), self.signal(doc, c.object, signal)));
                }
            }
        }
        for (address, said) in states {
            if said.len() > 1 {
                let names: Vec<&str> = said.iter().map(|(n, _)| *n).collect();
                out.errors.push(format!(
                    "{address}: 상태 주소를 {}가 함께 씁니다",
                    names.join(", ")
                ));
            }
            let on = said.iter().all(|(_, on)| *on);
            out.writes
                .push((address, crate::plc::Kind::Bit, i64::from(on)));
        }
        // The signals: inputs taken in, a robot started or stopped as its
        // input comes on; outputs as what they are tied to says, or as the
        // programs set them.
        for s in self.signals.clone() {
            if s.address.is_empty() {
                continue;
            }
            if s.input {
                let Some(&value) = read.get(&s.address) else {
                    continue;
                };
                let was = self.signal_last.insert(s.name.clone(), value).unwrap_or(0);
                self.signal_values.insert(s.name.clone(), value);
                if value != 0 && was == 0 {
                    match s.link {
                        Link::Start(root) => out.start.push(root),
                        Link::Stop(root) => out.stop.push(root),
                        _ => {}
                    }
                }
            } else {
                let value = match s.link {
                    Link::Running(root) => i64::from(robot(root).0),
                    Link::Done(root) => i64::from(robot(root).1),
                    Link::Cycles(machine) => self
                        .machines
                        .get(&machine)
                        .map_or(0, |m| m.cycles.min(i64::MAX as u64) as i64),
                    _ => self.signal_values.get(&s.name).copied().unwrap_or(0),
                };
                self.signal_values.insert(s.name.clone(), value);
                out.writes.push((s.address.clone(), s.kind(), value));
            }
        }
        out
    }

    /// The signals shared with a PLC, as saved.
    pub fn signals_saved(&self) -> Value {
        Value::Array(self.signals.iter().map(PlcSignal::to_json).collect())
    }

    /// Takes on the signals `v` lists: all of them, or none if any is
    /// amiss (two of one name among them).
    pub fn set_signals(&mut self, v: &Value) -> Result<(), String> {
        let list = v.as_array().map(Vec::as_slice).unwrap_or_default();
        if list.len() > MOST_SIGNALS {
            return Err(format!("신호는 {MOST_SIGNALS}개까지입니다"));
        }
        let signals = list
            .iter()
            .map(PlcSignal::of)
            .collect::<Result<Vec<_>, _>>()?;
        for (i, s) in signals.iter().enumerate() {
            if signals[..i].iter().any(|t| t.name == s.name) {
                return Err(format!("{}: 같은 이름의 신호가 둘입니다", s.name));
            }
        }
        // Values of signals no longer there go with them.
        self.signal_values
            .retain(|name, _| signals.iter().any(|s| &s.name == name));
        self.signal_last
            .retain(|name, _| signals.iter().any(|s| &s.name == name));
        self.signals = signals;
        self.roles_changed = true;
        self.changed = true;
        Ok(())
    }

    /// Sets the signal named `name`, one the cell writes and nothing else
    /// is tied to, to `value`: a robot program's doing.
    pub fn set_signal(&mut self, name: &str, value: i64) -> Result<(), String> {
        let s = self
            .signals
            .iter()
            .find(|s| s.name == name)
            .ok_or_else(|| format!("{name}: PLC 신호가 없습니다"))?;
        if s.input || s.link != Link::None {
            return Err(format!("{name}: 프로그램이 쓰는 신호가 아닙니다"));
        }
        let value = if s.number {
            value
        } else {
            i64::from(value != 0)
        };
        if self.signal_values.insert(name.to_string(), value) != Some(value) {
            self.changed = true;
        }
        Ok(())
    }

    /// The value of the signal named `name`, if there is one.
    pub fn signal_value(&self, name: &str) -> Option<i64> {
        self.signals
            .iter()
            .any(|s| s.name == name)
            .then(|| self.signal_values.get(name).copied().unwrap_or(0))
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
        self.conveying
            || self.rovers.values().any(|r| !r.legs.is_empty())
            || self
                .components
                .iter()
                .any(|c| c.kind == Kind::Conveyor && c.props.feed > 0.0)
            || self.machines.values().any(|m| {
                m.machining.is_some()
                    || (m.opening && m.door < 1.0)
                    || (!m.opening && m.door > 0.0)
                    || m.chuck != if m.clamped { 1.0 } else { 0.0 }
            })
    }

    /// Moves the machines on by `dt` seconds. Returns whether anything moved
    /// or ran, and whether a state changed: a door reaching its end, a
    /// cycle ending.
    fn step(&mut self, doc: &Document, dt: f64) -> (bool, bool) {
        let mut moving = false;
        let mut finished = false;
        let mut done: Vec<DVec3> = Vec::new();
        let mut moulded: Vec<DVec3> = Vec::new();
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
            // The chuck closing or opening, over its own time.
            let target = if m.clamped { 1.0 } else { 0.0 };
            if m.chuck != target {
                moving = true;
                let pace = if c.props.chuck > 0.0 {
                    dt / c.props.chuck
                } else {
                    1.0
                };
                m.chuck = if m.clamped {
                    (m.chuck + pace).min(1.0)
                } else {
                    (m.chuck - pace).max(0.0)
                };
                finished |= m.chuck == target;
            }
            if let Some(t) = m.machining {
                moving = true;
                let t = t + dt;
                if t >= c.props.cycle {
                    m.machining = None;
                    m.cycles += 1;
                    finished = true;
                    if let Some(seat) = self.seat(doc, c) {
                        if c.kind == Kind::Molder {
                            moulded.push(seat);
                        } else {
                            done.push(seat);
                        }
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
        // A moulding cycle ended: the part it made, in the mould.
        for seat in moulded {
            self.add_part(seat + DVec3::Z * (FINISHED.1 - RAW.1) / 2.0, true);
        }
        let (carried, arrived) = self.convey(doc, dt);
        self.conveying = carried;
        let (driven, stopped) = self.drive_on(doc, dt);
        (moving || carried || driven, finished || arrived || stopped)
    }

    /// Where AMR `c` stands now: where it was sent, or else where it is
    /// placed in `doc`.
    fn rover(&self, doc: &Document, c: &Component) -> Option<Rover> {
        if let Some(r) = self.rovers.get(&c.object) {
            return Some(r.clone());
        }
        let (at, yaw) = floor_pose(&doc.world_transform(id_of(doc, c.object)?));
        Some(Rover {
            at,
            yaw,
            legs: Vec::new(),
        })
    }

    /// Sends AMR `amr` to stop `stop`: it turns to face it, drives there
    /// straight, and turns as the stop faces.
    pub fn drive(&mut self, doc: &Document, amr: u64, stop: u64) -> Result<(), String> {
        let c = self
            .components
            .iter()
            .find(|c| c.object == amr && c.kind == Kind::Amr)
            .cloned()
            .ok_or("AMR가 아닙니다")?;
        let s = self
            .components
            .iter()
            .find(|c| c.object == stop && c.kind == Kind::Stop)
            .ok_or("AMR 정지 위치가 아닙니다")?;
        let (to, facing) =
            floor_pose(&doc.world_transform(id_of(doc, s.object).ok_or("no such stop")?));
        let mut r = self.rover(doc, &c).ok_or("no such AMR")?;
        if !r.legs.is_empty() {
            return Err("AMR가 아직 주행 중입니다".into());
        }
        // The floor where it drives: its own height, not the stop's.
        let to = DVec3::new(to.x, to.y, r.at.z);
        let way = to - r.at;
        if way.truncate().length() > 1e-3 {
            r.legs.push(Leg::Turn(way.y.atan2(way.x)));
            r.legs.push(Leg::Go(to));
        }
        r.legs.push(Leg::Turn(facing));
        self.rovers.insert(amr, r);
        self.changed = true;
        Ok(())
    }

    /// Turns positioner `positioner` to `angle` radians from where it was
    /// placed, the short way round, and returns how long that takes, in
    /// seconds.
    pub fn turn(&mut self, doc: &Document, positioner: u64, angle: f64) -> Result<f64, String> {
        let c = self
            .components
            .iter()
            .find(|c| c.object == positioner && c.kind == Kind::Positioner)
            .cloned()
            .ok_or("포지셔너가 아닙니다")?;
        let id = id_of(doc, positioner).ok_or("no such positioner")?;
        let (_, placed) = floor_pose(&doc.world_transform(id));
        let mut r = self.rover(doc, &c).ok_or("no such positioner")?;
        if !r.legs.is_empty() {
            return Err("포지셔너가 아직 돌고 있습니다".into());
        }
        let to = placed + angle;
        let seconds = turn_between(r.yaw, to).abs() / c.props.speed.max(1e-6);
        r.legs.push(Leg::Turn(to));
        self.rovers.insert(positioner, r);
        self.changed = true;
        Ok(seconds)
    }

    /// How far positioner `c` stands turned now, radians from where it was
    /// placed.
    fn turned(&self, doc: &Document, c: &Component) -> Option<f64> {
        let (_, placed) = floor_pose(&doc.world_transform(id_of(doc, c.object)?));
        Some(turn_between(placed, self.rover(doc, c)?.yaw))
    }

    /// Moves each AMR on its way, and turns each positioner, by `dt`
    /// seconds, the parts on its top plate with it. Returns whether one
    /// moved, and whether one arrived.
    fn drive_on(&mut self, doc: &Document, dt: f64) -> (bool, bool) {
        let (mut moved, mut arrived) = (false, false);
        let amrs: Vec<Component> = self
            .components
            .iter()
            .filter(|c| {
                matches!(c.kind, Kind::Amr | Kind::Positioner)
                    && self
                        .rovers
                        .get(&c.object)
                        .is_some_and(|r| !r.legs.is_empty())
            })
            .cloned()
            .collect();
        for c in amrs {
            let deck = self.decks(doc).into_iter().find(|d| d.object == c.object);
            let r = self.rovers.get_mut(&c.object).expect("an AMR on its way");
            let before = pose_of(r.at, r.yaw);
            let mut left = dt;
            while left > 1e-12 {
                let Some(leg) = r.legs.first().copied() else {
                    break;
                };
                match leg {
                    Leg::Turn(to) => {
                        let rate = if c.kind == Kind::Positioner {
                            c.props.speed.max(1e-6)
                        } else {
                            AMR_TURN
                        };
                        let need = turn_between(r.yaw, to);
                        let can = rate * left;
                        if need.abs() <= can {
                            r.yaw = to;
                            left -= need.abs() / rate;
                            r.legs.remove(0);
                        } else {
                            r.yaw += can * need.signum();
                            left = 0.0;
                        }
                    }
                    Leg::Go(to) => {
                        let need = to.distance(r.at);
                        let can = c.props.speed * left;
                        if need <= can {
                            r.at = to;
                            left -= need / c.props.speed;
                            r.legs.remove(0);
                        } else {
                            r.at += (to - r.at) / need * can;
                            left = 0.0;
                        }
                    }
                }
            }
            moved = true;
            arrived |= r.legs.is_empty();
            let shift = pose_of(r.at, r.yaw) * before.inverse();
            // What stands on its top plate rides along.
            if let Some(deck) = deck {
                for p in &mut self.parts {
                    if p.held.is_none() && deck.carries(p) {
                        let (_, turn, _) = shift.to_scale_rotation_translation();
                        let at = shift.transform_point3(p.world.translation);
                        let (_, own, _) = p.world.to_scale_rotation_translation();
                        p.world = DAffine3::from_rotation_translation(turn * own, at);
                    }
                }
            }
        }
        (moved, arrived)
    }

    /// Each AMR's and positioner's top plate where it now is.
    fn decks(&self, doc: &Document) -> Vec<Deck> {
        self.components
            .iter()
            .filter(|c| matches!(c.kind, Kind::Amr | Kind::Positioner))
            .filter_map(|c| {
                let id = id_of(doc, c.object)?;
                let home = doc.world_transform(id);
                let (lo, hi) = bounds(doc, part_named(doc, id, "top")?)?;
                let (at, yaw) = floor_pose(&home);
                let r = self.rover(doc, c)?;
                // Where the plate is from the AMR's middle, as placed.
                let local = pose_of(at, yaw).inverse();
                let (a, b) = (local.transform_point3(lo), local.transform_point3(hi));
                Some(Deck {
                    object: c.object,
                    pose: pose_of(r.at, r.yaw),
                    lo: a.min(b),
                    hi: a.max(b),
                })
            })
            .collect()
    }

    /// Every conveyor's belt, where it is in `doc`.
    fn belts(&self, doc: &Document) -> Vec<Belt> {
        self.components
            .iter()
            .filter(|c| c.kind == Kind::Conveyor)
            .filter_map(|c| {
                let world = doc.world_transform(id_of(doc, c.object)?);
                let along = world.matrix3.x_axis;
                let across = world.matrix3.y_axis;
                Some(Belt {
                    object: c.object,
                    props: c.props,
                    centre: world.transform_point3(DVec3::new(0.0, 0.0, 0.5)),
                    along: along.normalize_or_zero(),
                    across: across.normalize_or_zero(),
                    length: along.length(),
                    width: across.length(),
                })
            })
            .collect()
    }

    /// Carries the parts on each belt on by `dt` seconds, each up to the
    /// part ahead of it or the belt's end; takes away those at the end of a
    /// belt that drains; and puts a raw part on at the start of one that
    /// feeds, when its time comes and there is room. Returns whether a part
    /// moved, and whether one came to an end or was put on.
    fn convey(&mut self, doc: &Document, dt: f64) -> (bool, bool) {
        let (mut moved, mut arrived) = (false, false);
        for belt in self.belts(doc) {
            let gap = 2.0 * RAW.0 + 0.005;
            let end = belt.length / 2.0 - RAW.0;
            // Front first: each stops behind the one ahead.
            let mut on: Vec<(usize, f64)> = self
                .parts
                .iter()
                .enumerate()
                .filter(|(_, p)| p.held.is_none())
                .filter_map(|(i, p)| belt.along_of(p).map(|x| (i, x)))
                .collect();
            on.sort_by(|a, b| b.1.total_cmp(&a.1));
            let mut limit = end;
            let mut gone = Vec::new();
            for (i, x) in on {
                let to = (x + belt.props.speed * dt).min(limit).max(x);
                if to > x + 1e-9 {
                    self.parts[i].world.translation += belt.along * (to - x);
                    moved = true;
                    if to >= end - 1e-9 {
                        arrived = true;
                    }
                }
                if belt.props.drain > 0.5 && to >= end - 1e-9 {
                    gone.push(i);
                    continue;
                }
                limit = to - gap;
            }
            if !gone.is_empty() {
                gone.sort_unstable_by(|a, b| b.cmp(a));
                for i in gone {
                    let p = self.parts.remove(i);
                    self.removed.push(p.uid);
                    self.drained += 1;
                }
                self.changed = true;
            }
            if belt.props.feed > 0.0 {
                let since = self.fed.entry(belt.object).or_insert(belt.props.feed);
                *since += dt;
                let start = -belt.length / 2.0 + RAW.0;
                let room = !self
                    .parts
                    .iter()
                    .filter(|p| p.held.is_none())
                    .filter_map(|p| belt.along_of(p))
                    .any(|x| x < start + gap);
                if *since >= belt.props.feed && room {
                    *since = 0.0;
                    let at = belt.centre + belt.along * start + DVec3::Z * RAW.1 / 2.0;
                    self.add_part(at, false);
                    arrived = true;
                }
            }
        }
        (moved, arrived)
    }

    /// Parts taken away at a belt's end since this was last asked.
    pub fn take_drained(&mut self) -> usize {
        std::mem::take(&mut self.drained)
    }

    /// What each CNC machine is doing, for the timeline: its identity, its
    /// name, and `cut` (machining), `door` (its door moving), `blocked`
    /// (a finished part waiting to be taken out), or `idle`.
    pub fn activities(&self, doc: &Document) -> Vec<(u64, String, &'static str)> {
        self.components
            .iter()
            .filter(|c| c.kind.machine())
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
            .chain(
                self.components
                    .iter()
                    .filter(|c| matches!(c.kind, Kind::Amr | Kind::Positioner))
                    .map(|c| {
                        let driving = self
                            .rovers
                            .get(&c.object)
                            .is_some_and(|r| !r.legs.is_empty());
                        let busy = if c.kind == Kind::Amr { "drive" } else { "turn" };
                        (
                            c.object,
                            c.name.clone(),
                            if driving { busy } else { "idle" },
                        )
                    }),
            )
            .collect()
    }

    /// Whether the scene shown needs the cell in it: parts to add, or a
    /// door to move.
    pub fn needs_scene(&self) -> bool {
        !self.parts.is_empty()
            || !self.rovers.is_empty()
            || self.machines.values().any(|m| m.door > 0.0)
    }

    /// Puts each AMR that has been sent anywhere where it now is, in
    /// `doc`, and returns its objects, all of which moved.
    fn place_rovers(&self, doc: &mut Document) -> Vec<u64> {
        let mut uids = Vec::new();
        for c in self
            .components
            .iter()
            .filter(|c| matches!(c.kind, Kind::Amr | Kind::Positioner))
        {
            let (Some(r), Some(id)) = (self.rovers.get(&c.object), id_of(doc, c.object)) else {
                continue;
            };
            let home = doc.world_transform(id);
            let (at, yaw) = floor_pose(&home);
            let now = pose_of(r.at, r.yaw) * pose_of(at, yaw).inverse() * home;
            let Some(o) = doc.object(id) else { continue };
            let above = home * o.transform.to_affine().inverse();
            let local = above.inverse() * now;
            let (scale, rotation, translation) = local.to_scale_rotation_translation();
            let o = doc.object_mut(id).unwrap();
            o.transform.translation = translation;
            o.transform.rotation = rotation;
            o.transform.scale = scale;
            uids.extend(
                doc.objects()
                    .filter(|(o, _)| *o == id || is_below(doc, *o, id))
                    .filter_map(|(o, _)| doc.object_uid(o).map(|u| u.raw())),
            );
        }
        uids
    }

    /// What the viewers need to look at again when the scene shown has
    /// nothing of the cell in it: parts that went, and doors back where
    /// the scene has them.
    pub fn untouched(&mut self, doc: &Document) -> Changes {
        let mut changes = self.take_removed();
        for uid in self.doors(doc).into_iter().map(|(uid, ..)| uid) {
            changes
                .changed
                .insert((DatablockKind::Object, Uid::from_raw(uid)));
        }
        changes
    }

    /// Machine `machine`'s door, by identity.
    pub fn door_of(&self, doc: &Document, machine: u64) -> Option<u64> {
        let id = id_of(doc, machine)?;
        let (door, _) = self.door_parts(doc, id).into_iter().next()?;
        doc.object_uid(door).map(|u| u.raw())
    }

    /// The parts of machine `machine` that move as its door does: those
    /// with the door role, with how they move; or else the part named for
    /// it, which slides aside.
    fn door_parts(&self, doc: &Document, machine: ObjectId) -> Vec<(ObjectId, Option<Props>)> {
        let marked: Vec<_> = self
            .components
            .iter()
            .filter(|c| c.kind == Kind::Door)
            .filter_map(|c| {
                let id = id_of(doc, c.object)?;
                is_below(doc, id, machine).then_some((id, Some(c.props)))
            })
            .collect();
        if !marked.is_empty() {
            return marked;
        }
        part_named(doc, machine, "door")
            .map(|id| (id, None))
            .into_iter()
            .collect()
    }

    /// Each machine's door parts: identity, how far open, the machine, and
    /// how the part moves if it was told.
    fn doors(&self, doc: &Document) -> Vec<(u64, f64, ObjectId, Option<Props>)> {
        let mut out = Vec::new();
        for c in self.components.iter().filter(|c| c.kind.machine()) {
            let Some(machine) = id_of(doc, c.object) else {
                continue;
            };
            let open = self.machines.get(&c.object).map_or(0.0, |m| m.door);
            for (door, props) in self.door_parts(doc, machine) {
                if let Some(uid) = doc.object_uid(door) {
                    out.push((uid.raw(), open, machine, props));
                }
            }
        }
        out
    }

    /// Moves each open door in `doc` as far as it is open: as its door role
    /// says, or else aside along its machine's X by its own width.
    fn open_doors(&self, doc: &mut Document) -> Vec<u64> {
        let mut uids = Vec::new();
        for (uid, open, machine, props) in self.doors(doc) {
            uids.push(uid);
            let Some(door) = id_of(doc, uid) else {
                continue;
            };
            if open <= 0.0 {
                continue;
            }
            let world = doc.world_transform(door);
            let own = |p: &Props| {
                let unit = [DVec3::X, DVec3::Y, DVec3::Z][p.axis.min(2) as usize];
                (world.matrix3 * unit).normalize_or_zero()
            };
            let moved = match props {
                Some(p) if p.swing > 0.5 => {
                    let pivot = world.transform_point3(DVec3::from_array(p.point));
                    DAffine3::from_translation(pivot)
                        * DAffine3::from_axis_angle(own(&p), p.angle * open)
                        * DAffine3::from_translation(-pivot)
                        * world
                }
                Some(p) if p.travel != 0.0 => {
                    DAffine3::from_translation(own(&p) * p.travel * open) * world
                }
                _ => {
                    let along = doc
                        .world_transform(machine)
                        .matrix3
                        .x_axis
                        .normalize_or_zero();
                    let Some((lo, hi)) = bounds(doc, door) else {
                        continue;
                    };
                    let width = (hi - lo).dot(along).abs();
                    DAffine3::from_translation(along * (width + 0.02) * open) * world
                }
            };
            // From the world into the door's own transform.
            let Some(o) = doc.object(door) else { continue };
            let above = world * o.transform.to_affine().inverse();
            let (scale, rotation, translation) =
                (above.inverse() * moved).to_scale_rotation_translation();
            let t = &mut doc.object_mut(door).unwrap().transform;
            t.translation = translation;
            t.rotation = rotation;
            t.scale = scale;
        }
        uids
    }

    /// Adds the parts to `doc`, the scene as shown, and returns what the
    /// viewers need to look at again. A held part goes where its grip point
    /// now is.
    pub fn apply(&mut self, doc: &mut Document) -> Changes {
        let mut changes = self.take_removed();
        for uid in self
            .open_doors(doc)
            .into_iter()
            .chain(self.place_rovers(doc))
        {
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
                    "io": c.io,
                    "plc": c.plc,
                    "io_points": {
                        "outputs": io_points(c.kind).0,
                        "inputs": io_points(c.kind).1,
                    },
                    "robot": robot,
                    "slots": self.slots(doc, c).into_iter().map(point).collect::<Vec<_>>(),
                    "seat": self.seat(doc, c).map(point),
                    "holding": self.parts.iter().any(|p| p.held.is_some_and(|h| h.0 == c.object)),
                    // A door's hinge, a connector: where it is in the world.
                    "point": matches!(c.kind, Kind::Door | Kind::Connector)
                        .then(|| id_of(doc, c.object))
                        .flatten()
                        .map(|id| point(doc.world_transform(id).transform_point3(DVec3::from_array(c.props.point)))),
                    // The machine a door moves with.
                    "machine_of": (c.kind == Kind::Door)
                        .then(|| id_of(doc, c.object))
                        .flatten()
                        .and_then(|id| {
                            self.components.iter().find(|m| {
                                m.kind.machine()
                                    && id_of(doc, m.object).is_some_and(|mid| is_below(doc, id, mid))
                            })
                        })
                        .map(|m| m.object.to_string()),
                    "positioner": (c.kind == Kind::Positioner).then(|| {
                        let r = self.rover(doc, c);
                        json!({
                            "turning": r.as_ref().is_some_and(|r| !r.legs.is_empty()),
                            "angle": self.turned(doc, c),
                        })
                    }),
                    "amr": (c.kind == Kind::Amr).then(|| {
                        let r = self.rover(doc, c);
                        json!({
                            "driving": r.as_ref().is_some_and(|r| !r.legs.is_empty()),
                            "at": r.as_ref().map(|r| [r.at.x, r.at.y]),
                        })
                    }),
                    "machine": c.kind.machine().then(|| {
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
                            "chuck": m.chuck,
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
            "signals": self.signals.iter().map(|s| {
                let mut v = s.to_json();
                v["value"] = json!(self.signal_values.get(&s.name).copied().unwrap_or(0));
                v
            }).collect::<Vec<_>>(),
            "parts": self.parts.iter().map(|p| json!({
                "uid": p.uid.to_string(),
                "finished": p.finished,
                "held": p.held.map(|h| h.0.to_string()),
                "at": [p.world.translation.x, p.world.translation.y, p.world.translation.z],
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
        "role"
            | "auto"
            | "fill"
            | "empty"
            | "clear_parts"
            | "grip"
            | "release"
            | "machine"
            | "drive"
            | "turn"
            | "io"
            | "point"
            | "plc"
            | "signals"
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
        // Not machined until the chuck has closed, over its 1 s.
        let a = act(&mut cell, &doc, "start");
        assert!(a["error"].as_str().unwrap().contains("척"), "{a}");
        assert!(cell.is_running(), "the chuck closing");
        cell.tick(&doc, 7.5, at(7.5));
        assert!(!cell.signal(&doc, machine, Signal::Clamped));
        cell.tick(&doc, 8.0, at(8.0));
        assert!(cell.signal(&doc, machine, Signal::Clamped));
        assert_eq!(act(&mut cell, &doc, "start")["ok"], true);
        assert_eq!(
            act(&mut cell, &doc, "open")["ok"],
            false,
            "not while machining"
        );
        assert_eq!(act(&mut cell, &doc, "unclamp")["ok"], false);

        // 90 s later the part in the chuck is finished.
        cell.tick(&doc, 51.0, at(51.0));
        assert!(cell.is_running());
        assert!(
            cell.tick(&doc, 98.5, at(98.5)),
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
        // Its cycles, as a PLC signal tied to them reads them.
        cell.set_signals(&json!([
            {"name": "cycles", "number": true, "address": "30", "link": format!("cycles:{machine}")},
        ]))
        .unwrap();
        let x = cell.plc_exchange(&doc, &BTreeMap::new(), &|_| (false, false));
        assert!(x
            .writes
            .contains(&("30".to_string(), crate::plc::Kind::Number, 1)));
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

    /// One piece of library equipment alone, its roles given.
    fn equipment(template: &str, props: Value) -> (Document, Cell, u64) {
        let mut library = crate::library::Library::default();
        library.handle(
            &json!({"op": "place", "template": template, "at": [0.0, 0.0, 0.0]}).to_string(),
        );
        let mut doc = Document::new();
        library.apply(&mut doc);
        let mut cell = Cell::default();
        let (uid, name, kind) = library.roles().remove(0);
        cell.ensure(uid, name, kind);
        let a = ask(
            &mut cell,
            &doc,
            json!({"op": "role", "object": uid.to_string(), "kind": kind.tag(), "props": props}),
        );
        assert_eq!(a["ok"], true, "{a}");
        (doc, cell, uid)
    }

    /// Runs the cell for `seconds`, a tenth of a second at a time.
    fn run_for(cell: &mut Cell, doc: &Document, from: f64, seconds: f64) -> f64 {
        let start = Instant::now();
        let mut t = from;
        while t < from + seconds {
            t += 0.1;
            cell.tick(doc, t, start + Duration::from_secs_f64(t));
        }
        t
    }

    #[test]
    fn a_moulding_machine_makes_a_part_each_cycle_and_waits_for_it_to_go() {
        let (doc, mut cell, m) = equipment("injection_molder", json!({}));
        assert_eq!(cell.components()[0].props.cycle, 30.0, "a moulding cycle");
        assert!(cell.command(&doc, m, Command::Clamp).is_err(), "no chuck");
        cell.command(&doc, m, Command::Start).unwrap();
        let t = run_for(&mut cell, &doc, 0.0, 31.0);
        assert!(cell.signal(&doc, m, Signal::Done));
        let made: Vec<&Part> = cell.parts().iter().filter(|p| p.finished).collect();
        assert_eq!(made.len(), 1);
        // The part on the mould, for the robot to take out.
        let seat = cell.seat(&doc, &cell.components()[0].clone()).unwrap();
        assert!(made[0].world.translation.distance(seat) < SNAP);
        // Not again while it is there; again once it has gone.
        let why = cell.command(&doc, m, Command::Start).unwrap_err();
        assert!(why.contains("성형품"), "{why}");
        ask(
            &mut cell,
            &doc,
            json!({"op": "empty", "object": m.to_string()}),
        );
        cell.command(&doc, m, Command::Start).unwrap();
        run_for(&mut cell, &doc, t, 31.0);
        assert_eq!(cell.parts().len(), 1);
    }

    #[test]
    fn a_conveyor_carries_parts_to_its_end_and_queues_them() {
        // 2 m at 0.5 m/s, a part every 2 s.
        let (doc, mut cell, belt) = equipment("conveyor", json!({"speed": 0.5, "feed": 2.0}));
        assert!(!cell.signal(&doc, belt, Signal::PartAtEnd));
        let t = run_for(&mut cell, &doc, 0.0, 12.0);
        assert!(cell.signal(&doc, belt, Signal::PartAtEnd));
        // Queued nose to tail from the end, none overlapping.
        let mut xs: Vec<f64> = cell.parts().iter().map(|p| p.world.translation.x).collect();
        xs.sort_by(f64::total_cmp);
        assert!(xs.len() >= 5, "{xs:?}");
        assert!((xs[xs.len() - 1] - (1.0 - RAW.0)).abs() < 1e-6, "{xs:?}");
        for pair in xs.windows(2) {
            assert!(pair[1] - pair[0] >= 2.0 * RAW.0, "{xs:?}");
        }
        // A part taken off the end makes room: the rest move up.
        ask(
            &mut cell,
            &doc,
            json!({"op": "role", "object": belt.to_string(), "kind": "conveyor",
                                    "props": {"speed": 0.5, "feed": 2.0, "drain": 1}}),
        );
        run_for(&mut cell, &doc, t, 6.0);
        assert!(cell.take_drained() >= 3);
    }

    #[test]
    fn parts_go_with_the_equipment_they_stand_on() {
        let mut library = crate::library::Library::default();
        library.handle(
            &json!({"op": "place", "template": "infeed_station", "at": [0.0, 0.0, 0.0]})
                .to_string(),
        );
        let mut before = Document::new();
        library.apply(&mut before);
        let mut cell = Cell::default();
        let (plate, name, kind) = library.roles().remove(0);
        cell.ensure(plate, name, kind);
        ask(
            &mut cell,
            &before,
            json!({"op": "fill", "object": plate.to_string()}),
        );
        let count = cell.parts().len();
        assert_eq!(count, 20);

        // Moved half a metre along X and turned a quarter: the parts are
        // in the plate's places where it now stands.
        library.handle(
            &json!({"op": "move", "item": 1, "at": [0.5, 0.0, 0.0], "yaw": std::f64::consts::FRAC_PI_2}).to_string(),
        );
        let mut after = before.clone();
        library.apply(&mut after);
        cell.carry(&before, &after);
        let c = cell.components()[0].clone();
        let slots = cell.slots(&after, &c);
        for p in cell.parts() {
            assert!(
                slots.iter().any(|s| s.distance(p.world.translation) < 1e-6),
                "{:?}",
                p.world.translation
            );
        }

        // Removed: they go too.
        library.handle(&json!({"op": "remove", "item": 1}).to_string());
        let mut gone = Document::new();
        library.apply(&mut gone);
        cell.carry(&after, &gone);
        assert!(cell.parts().is_empty());
        assert_eq!(cell.take_removed().removed.len(), count);
    }

    #[test]
    fn an_amr_drives_to_its_stop_with_what_is_on_it() {
        let mut library = crate::library::Library::default();
        library
            .handle(&json!({"op": "place", "template": "amr", "at": [0.0, 0.0, 0.0]}).to_string());
        // Two metres ahead, facing left.
        library.handle(
            &json!({"op": "place", "template": "amr_stop", "at": [2.0, 0.0, 0.0], "yaw": std::f64::consts::FRAC_PI_2})
                .to_string(),
        );
        let mut doc = Document::new();
        library.apply(&mut doc);
        let mut cell = Cell::default();
        let mut amr = 0;
        let mut stop = 0;
        for (uid, name, kind) in library.roles() {
            match kind {
                Kind::Amr => amr = uid,
                Kind::Stop => stop = uid,
                _ => {}
            }
            cell.ensure(uid, name, kind);
        }
        // Over its top plate a part let go stands on it.
        let top = cell.decks(&doc)[0].hi.z;
        let over =
            cell.decks(&doc)[0].top_under(DVec3::new(0.2, 0.1, top + 0.05), FINISHED.1 / 2.0);
        assert!(over.is_some_and(|z| (z - top).abs() < 1e-9));
        cell.add_part(DVec3::new(0.2, 0.1, top + FINISHED.1 / 2.0), true);

        assert!(
            cell.signal(&doc, amr, Signal::Arrived),
            "at rest where placed"
        );
        cell.drive(&doc, amr, stop).unwrap();
        assert!(!cell.signal(&doc, amr, Signal::Arrived));
        let t = run_for(&mut cell, &doc, 0.0, 1.0);
        assert!(!cell.signal(&doc, amr, Signal::Arrived), "on its way");
        // Straight ahead at 1 m/s, then a quarter turn at 1 rad/s: 3.6 s.
        run_for(&mut cell, &doc, t, 3.0);
        assert!(cell.signal(&doc, amr, Signal::Arrived));
        let r = cell.rovers[&amr].clone();
        assert!(
            r.at.truncate().distance(kreate_math::DVec2::new(2.0, 0.0)) < 1e-9,
            "{:?}",
            r.at
        );
        assert!((r.yaw - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
        // The part rode along: on the plate, turned with it.
        let p = cell.parts[0].world.translation;
        assert!(
            p.distance(DVec3::new(1.9, 0.2, top + FINISHED.1 / 2.0)) < 1e-6,
            "{p}"
        );
        assert!(cell.decks(&doc)[0].carries(&cell.parts[0]));
    }

    /// A positioner turns half round at 90° a second in 2 s, the part on its
    /// plate with it, and says so once there; the page sees its angle.
    #[test]
    fn a_positioner_turns_with_what_is_on_it() {
        let (doc, mut cell, table) = equipment("positioner", json!({}));
        let top = cell.decks(&doc)[0].hi.z;
        cell.add_part(DVec3::new(0.4, 0.0, top + FINISHED.1 / 2.0), true);
        assert!(cell.signal(&doc, table, Signal::Arrived), "at rest");
        let seconds = cell.turn(&doc, table, std::f64::consts::PI).unwrap();
        assert!((seconds - 2.0).abs() < 1e-9, "{seconds}");
        assert!(!cell.signal(&doc, table, Signal::Arrived));
        assert!(cell.turn(&doc, table, 0.0).is_err(), "still turning");
        let t = run_for(&mut cell, &doc, 0.0, 1.0);
        assert!(!cell.signal(&doc, table, Signal::Arrived), "half way");
        run_for(&mut cell, &doc, t, 1.2);
        assert!(cell.signal(&doc, table, Signal::Arrived));
        let p = cell.parts[0].world.translation;
        assert!(
            p.distance(DVec3::new(-0.4, 0.0, top + FINISHED.1 / 2.0)) < 1e-6,
            "{p}"
        );
        let shown: Value = serde_json::from_str(&cell.to_json(&doc)).unwrap();
        let angle = shown["components"][0]["positioner"]["angle"]
            .as_f64()
            .unwrap();
        assert!((angle.abs() - std::f64::consts::PI).abs() < 1e-9, "{angle}");
    }

    #[test]
    fn a_machine_is_wired_to_the_controllers_io_and_keeps_it() {
        let (doc, mut cell, m) = equipment("cnc_lathe", json!({}));
        let set = |cell: &mut Cell, key: &str, port: Value| {
            ask(
                cell,
                &doc,
                json!({"op": "io", "object": m.to_string(), "key": key, "port": port}),
            )
        };
        assert_eq!(set(&mut cell, "start", json!(4))["ok"], true);
        assert_eq!(set(&mut cell, "done", json!(9))["ok"], true);
        assert_eq!(
            set(&mut cell, "part_at_end", json!(1))["ok"],
            false,
            "not a CNC's"
        );
        assert_eq!(
            set(&mut cell, "open", json!(18))["ok"],
            false,
            "past the tool's"
        );
        // Its settings changed, its wiring stays.
        ask(
            &mut cell,
            &doc,
            json!({"op": "role", "object": m.to_string(), "kind": "cnc", "props": {"cycle": 60}}),
        );
        let io = cell.export(&doc);
        assert_eq!(io.io.get(&(m, "start".into())), Some(&4));
        assert_eq!(io.io.get(&(m, "done".into())), Some(&9));
        // Cleared, and saved and read back.
        assert_eq!(set(&mut cell, "done", Value::Null)["ok"], true);
        let mut again = Cell::from_saved(cell.to_saved());
        again.adopt(&doc);
        let io = again.export(&doc);
        assert_eq!(io.io.len(), 1);
        assert_eq!(io.io.get(&(m, "start".into())), Some(&4));
    }

    /// A lathe's door told how it moves: swung 90° about its own Z through
    /// a hinge picked on its corner, the hinge staying where it is; then
    /// lifted 0.4 m along its own Z instead. Kept in the project file.
    #[test]
    fn a_door_moves_as_its_role_says() {
        let (doc, mut cell, machine) = equipment("cnc_lathe", json!({"door": 1.0}));
        let door = part_named(&doc, id_of(&doc, machine).unwrap(), "door").unwrap();
        let door_uid = doc.object_uid(door).unwrap().raw();
        let before = doc.world_transform(door);
        let (lo, _) = bounds(&doc, door).unwrap();
        let a = ask(
            &mut cell,
            &doc,
            json!({"op": "role", "object": door_uid.to_string(), "kind": "door",
                   "props": {"swing": 1, "axis": 2, "angle": std::f64::consts::FRAC_PI_2}}),
        );
        assert_eq!(a["ok"], true, "{a}");
        let a = ask(
            &mut cell,
            &doc,
            json!({"op": "point", "object": door_uid.to_string(), "at": [lo.x, lo.y, lo.z]}),
        );
        assert_eq!(a["ok"], true, "{a}");
        assert_eq!(cell.door_of(&doc, machine), Some(door_uid));
        let open = |cell: &mut Cell| {
            ask(
                cell,
                &doc,
                json!({"op": "machine", "object": machine.to_string(), "action": "open"}),
            );
            run_for(cell, &doc, 0.0, 2.0);
            let mut shown = doc.clone();
            cell.apply(&mut shown);
            shown.world_transform(id_of(&shown, door_uid).unwrap())
        };
        let swung = open(&mut cell);
        let turn = DAffine3::from_translation(lo)
            * DAffine3::from_rotation_z(std::f64::consts::FRAC_PI_2)
            * DAffine3::from_translation(-lo);
        let want = turn * before;
        assert!(swung.abs_diff_eq(want, 1e-9), "{swung:?}\n{want:?}");
        // The hinge stays put.
        let hinge = before.inverse().transform_point3(lo);
        assert!((swung.transform_point3(hinge) - lo).length() < 1e-9);

        // Lifted instead: the same request keeps the hinge it does not name.
        ask(
            &mut cell,
            &doc,
            json!({"op": "role", "object": door_uid.to_string(), "kind": "door",
                   "props": {"swing": 0, "travel": 0.4}}),
        );
        let c = cell
            .components
            .iter()
            .find(|c| c.object == door_uid)
            .unwrap();
        assert!((DVec3::from_array(c.props.point) - hinge).length() < 1e-9);
        let lifted = open(&mut cell);
        let up = (before.matrix3 * DVec3::Z).normalize() * 0.4;
        assert!((lifted.translation - before.translation - up).length() < 1e-9);

        // Saved and read back.
        let mut again = Cell::from_saved(cell.to_saved());
        again.adopt(&doc);
        let c = again
            .components
            .iter()
            .find(|c| c.object == door_uid)
            .unwrap();
        assert_eq!(c.kind, Kind::Door);
        assert!((c.props.travel - 0.4).abs() < 1e-12);
    }
}
