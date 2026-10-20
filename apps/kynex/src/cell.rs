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

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{Duration, Instant};

use kreate_math::{DAffine3, DVec3, SceneRgb};
use kreate_scene::{Changes, DatablockKind, Document, Material, Object, ObjectData, ObjectId, Uid};
use serde_json::{json, Value};

/// The first identity of what KYNEX adds to the scene — parts, their mesh
/// and material. Kreate numbers a participant's datablocks from
/// `site << 48`; the last site is never a participant.
pub const PARTS: u64 = 0xFFFF << 48;

/// What a flow part can look like, by number: a part as it always was; a
/// car body at its stage — welded steel, painted, finished; or a display at
/// its — the mother glass, a panel cut from it, the module built on that.
pub const LOOKS: &[&str] = &[
    "부품",
    "차체 (BIW)",
    "도장 차체",
    "완성차",
    "유리 기판",
    "LCD 패널",
    "LCD 모듈",
    "유리 카세트",
    "FOUP",
    "웨이퍼",
    "반도체 패키지",
    "음료 프리폼",
    "빈 병",
    "충전·캡 병",
    "라벨 병",
    "수축 포장 팩",
    "음료 팔레트",
    "원유 로트",
    "증류 유분 로트",
    "휘발유 로트",
    "항공유 로트",
    "경유 로트",
    "중질유 로트",
];

/// How far apart parts of each look stand in line at a station, and the
/// lanes they stand in in a store, metres. A part of the first look is set
/// out on its station's table instead.
const LOOK_PITCH: [(f64, f64); 23] = [
    (0.0, 0.0),
    (5.2, 2.6),
    (5.2, 2.6),
    (5.2, 2.6),
    (3.0, 2.6),
    (1.0, 0.7),
    (1.0, 0.7),
    (3.0, 2.8),
    (0.6, 0.6),
    (0.4, 0.4),
    (0.4, 0.25),
    (0.45, 0.32),
    (0.45, 0.32),
    (0.45, 0.32),
    (0.45, 0.32),
    (0.43, 0.3),
    (1.3, 1.1),
    (6.6, 3.0),
    (6.6, 3.0),
    (6.6, 3.0),
    (6.6, 3.0),
    (6.6, 3.0),
    (6.6, 3.0),
];

/// How far apart parts of each look are stacked one over another in a store,
/// metres, and how many to a stack — sheets and panels twenty high, cassettes
/// five on a stocker's shelves, FOUPs four, wafers twenty-five, trays ten;
/// none for a car body, which stands in lanes.
const LOOK_STACK: [(f64, usize); 23] = [
    (0.0, 1),
    (0.0, 1),
    (0.0, 1),
    (0.0, 1),
    (0.25, 20),
    (0.08, 20),
    (0.08, 20),
    (1.2, 5),
    (0.45, 4),
    (0.01, 25),
    (0.02, 10),
    (0.0, 1),
    (0.0, 1),
    (0.0, 1),
    (0.0, 1),
    (0.27, 4),
    (0.0, 1),
    (2.5, 3),
    (2.5, 3),
    (2.5, 3),
    (2.5, 3),
    (2.5, 3),
    (2.5, 3),
];

/// How high a sheet is lifted to go between a tool and a stocker, metres:
/// it goes overhead, as on an overhead hoist's rail, not across the floor.
const LIFT: f64 = 4.5;

/// A part of a look drawn in pieces, one for each colour of it, under the
/// part: always all of them (empty if it has fewer colours), so that a part
/// changing its look leaves none behind.
const LOOK_PIECES: u64 = 8;

/// The identity of piece `k` of the look part `uid` is drawn in.
fn look_piece(uid: u64, k: u64) -> u64 {
    PARTS + (1 << 40) + (uid - PARTS) * LOOK_PIECES + k
}

/// How many colours a painted car body comes in, one after another.
const CAR_COLOURS: usize = 5;

/// Which of [`look_meshes`] part `p` is drawn with: a car body's stage and
/// colour — steel for a welded one, else one of the colours in turn — or
/// a display's stage; a cassette on a stocker's crane, with the crane; a
/// semiconductor fab's FOUP, wafer or tray of chips; a FOUP on an overhead
/// hoist, with the hoist's vehicle.
fn look_key(p: &Part) -> usize {
    let carried = p.rail.is_some() && matches!(p.flow, Some(flow::Place::Moving { .. }));
    match p.look {
        7 if carried => return CRANE_LOOK,
        8 if carried => return HOIST_LOOK,
        look @ 8..=10 => return CRANE_LOOK + 1 + (look as usize - 8),
        // A beverage line's: a tray, a pack, a pallet.
        look @ 11..=16 => return HOIST_LOOK + 1 + (look as usize - 11),
        // A refinery's: a lot of oil in its tank container.
        look @ 17..=22 => return HOIST_LOOK + 7 + (look as usize - 17),
        _ => {}
    }
    match p.look as usize {
        look @ 1..=3 => {
            let colour = if look == 1 {
                0
            } else {
                ((p.uid - PARTS) % CAR_COLOURS as u64) as usize
            };
            (look - 1) * CAR_COLOURS + colour
        }
        look => 3 * CAR_COLOURS + (look - 4).min(3),
    }
}

/// The meshes each look is drawn with, at each stage and colour: made once.
fn look_meshes() -> &'static Vec<crate::library::LookMeshes> {
    static MADE: std::sync::OnceLock<Vec<crate::library::LookMeshes>> = std::sync::OnceLock::new();
    MADE.get_or_init(|| {
        (1..=3)
            .flat_map(|look| (0..CAR_COLOURS).map(move |c| crate::library::car_look(look - 1, c)))
            .chain((0..4).map(crate::library::display_look))
            .chain(std::iter::once(
                [crate::library::display_look(3), crane_look().clone()].concat(),
            ))
            .chain((0..3).map(crate::library::semi_look))
            .chain(std::iter::once(
                [crate::library::semi_look(0), hoist_look().clone()].concat(),
            ))
            .chain((0..6).map(crate::library::beverage_look))
            .chain((0..6).map(crate::library::lot_look))
            .collect()
    })
}

/// How small a glass robot's sheet is drawn while it carries none: out of
/// sight, not gone, so that it is there to show again.
pub(crate) const NO_GLASS: f64 = 1e-4;

/// The look of a cassette on a stocker's crane: the cassette's pieces,
/// then the crane's (see [`crane_look`]).
const CRANE_LOOK: usize = 3 * CAR_COLOURS + 4;

/// The look of a FOUP on an overhead hoist: the FOUP's pieces, then the
/// hoist vehicle's (see [`hoist_look`]), after a FOUP, a wafer and a tray.
const HOIST_LOOK: usize = CRANE_LOOK + 4;

/// How high a FOUP rides under its hoist, its middle, metres; the
/// vehicle's gripper on its top, 0.34 m higher, its rail over that (an
/// overhead rail 5.35 m up).
pub(crate) const HOIST_RIDE: f64 = 4.3;
const FOUP_HIGH: f64 = 0.34;

/// An overhead hoist's vehicle: drawn on its rail over the FOUP it lifts,
/// its gripper on the FOUP's top as it goes along.
fn hoist_look() -> &'static crate::library::LookMeshes {
    static MADE: std::sync::OnceLock<crate::library::LookMeshes> = std::sync::OnceLock::new();
    MADE.get_or_init(|| crate::library::semi_look(3))
}

/// A stocker's crane, its pieces drawn at the rail under the cassette it
/// carries, not under the cassette itself.
fn crane_look() -> &'static crate::library::LookMeshes {
    static MADE: std::sync::OnceLock<crate::library::LookMeshes> = std::sync::OnceLock::new();
    MADE.get_or_init(|| crate::library::display_look(4))
}

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
    /// An axis (a stopper, a lifter, a slide, a turn) to its far end.
    Extend,
    /// Back to where it was placed.
    Retract,
}

impl Command {
    pub const ALL: [Command; 7] = [
        Command::Open,
        Command::Close,
        Command::Clamp,
        Command::Unclamp,
        Command::Start,
        Command::Extend,
        Command::Retract,
    ];

    pub fn tag(self) -> &'static str {
        match self {
            Command::Open => "open",
            Command::Close => "close",
            Command::Clamp => "clamp",
            Command::Unclamp => "unclamp",
            Command::Start => "start",
            Command::Extend => "extend",
            Command::Retract => "retract",
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
    /// A sensor's beam broken by a part.
    Detected,
    /// An axis at its far end, or back where it was placed.
    Extended,
    Retracted,
}

impl Signal {
    pub const ALL: [Signal; 11] = [
        Signal::Detected,
        Signal::Extended,
        Signal::Retracted,
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
            Signal::Detected => "detected",
            Signal::Extended => "extended",
            Signal::Retracted => "retracted",
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
    /// A photo sensor: a beam from the object's origin along its own axis
    /// (`axis`), `travel` long, broken by a part across it.
    Sensor,
    /// An axis of its own: the object, with what is under it and the parts
    /// standing on it, slides along (or turns about) an axis of its own as
    /// a door does, to its far end when told to extend and back when told
    /// to retract, over `door` seconds — a stopper, a lifter, a shuttle, a
    /// rotary table. Raised across a belt, it holds the parts behind it.
    Actuator,
    /// Where parts come into a process flow: one every `feed` seconds, as
    /// long as its way out (`capacity`) has room (see `flow`).
    Source,
    /// A step of a process flow: `takes` parts in (more than one, an
    /// assembly), `cycle` seconds, `makes` parts out (more than one, a
    /// disassembly); `capacity` parts may wait for it.
    Station,
    /// Parts kept in turn, `capacity` of them, on their way.
    Buffer,
    /// Where parts leave a process flow, counted, their lead time taken.
    Sink,
    /// A person: walks (`speed`) to the stations that need one and works
    /// there, and carries parts by hand (see `fleet`).
    Operator,
}

impl Kind {
    pub const ALL: [Kind; 19] = [
        Kind::Operator,
        Kind::Source,
        Kind::Station,
        Kind::Buffer,
        Kind::Sink,
        Kind::Sensor,
        Kind::Actuator,
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
            Kind::Sensor => "sensor",
            Kind::Actuator => "actuator",
            Kind::Source => "source",
            Kind::Station => "station",
            Kind::Buffer => "buffer",
            Kind::Sink => "sink",
            Kind::Operator => "operator",
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

    /// A part of a process flow: parts go from one to the next.
    pub fn flow(self) -> bool {
        matches!(
            self,
            Kind::Source | Kind::Station | Kind::Buffer | Kind::Sink | Kind::Conveyor
        )
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
    /// Process flow: how a part is sent on: straight there (0), by AMR
    /// (1), by an operator (2, see `fleet`) or by a robot (3, run by the
    /// view: `Cell::robot_jobs`); and whether a station needs
    /// an operator at it to work (1).
    pub carrier: u32,
    pub crew: u32,
    /// Process flow: how a station's cycle and a source's feed vary about
    /// their mean: fixed (0), uniform (1) or triangular (2) within
    /// `spread` either side, normal (3) with `spread` its standard
    /// deviation, exponential (4) about the mean alone (see `flow::draw`).
    pub dist: u32,
    pub spread: f64,
    /// A station's breakdowns: the mean time it works between them and the
    /// mean time a repair takes, seconds (0: it does not break down); and
    /// the share of what it makes that is scrap.
    pub mtbf: f64,
    pub mttr: f64,
    pub scrap: f64,
    /// A station's change-over: seconds before a part of another product
    /// than the last it worked on.
    pub setup: f64,
    /// What a source's or a station's parts look like once it has made
    /// them, by number (see [`LOOKS`]): a part, or a car body at a stage
    /// of the car plant — to see a car go down its line as it is built.
    pub look: u32,
    /// A buffer's order: first in first out (0), or last in first out (1).
    pub policy: u32,
    /// A source's limit on its parts in the flow at once — a kanban: none
    /// more until one leaves (0: no limit); and how any flow component
    /// picks among the next with room: in turn (0), the one with the
    /// fewest waiting (1), or at random (2).
    pub kanban: u32,
    pub rule: u32,
    /// An AMR's battery: minutes it drives on a full charge (0: no
    /// battery to mind), and minutes to charge it from empty; a stop's
    /// being a charger (1).
    pub battery: f64,
    pub charge: f64,
    pub charger: u32,
    /// A buffer that is a stocker (1): it sends each part on to the next
    /// step of its recipe (see `Cell::recipes`), whichever tool that is,
    /// and on as its links say once the recipe is done.
    pub dispatch: u32,
    /// A stocker whose crane carries its parts (1): it runs on a rail
    /// along the stocker's length, past the tools' load ports, and a part
    /// sent from or to the stocker goes across onto the rail, along it, and
    /// across off it, under it the crane. Or (2) whose parts overhead
    /// hoists carry: lifted to the hoist's rail overhead, along it, and let
    /// down onto the load port, under the hoist's vehicle.
    pub rail: u32,
    /// An operator's shift, hours of the day on the flow's clock (from
    /// its start at hour 0): from, to; the same, all day.
    pub shift_from: f64,
    pub shift_to: f64,
    /// An operator's wage, won an hour.
    pub wage: f64,
    /// Process flow: how many parts may wait (a source's and a buffer's,
    /// how many it holds); how many a station takes and makes a cycle.
    pub capacity: u32,
    pub takes: u32,
    pub makes: u32,
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
            carrier: 0,
            crew: 0,
            dist: 0,
            spread: 0.0,
            mtbf: 0.0,
            mttr: 0.0,
            scrap: 0.0,
            setup: 0.0,
            look: 0,
            policy: 0,
            kanban: 0,
            rule: 0,
            battery: 0.0,
            charge: 60.0,
            charger: 0,
            dispatch: 0,
            rail: 0,
            shift_from: 0.0,
            shift_to: 0.0,
            wage: 0.0,
            capacity: 1,
            takes: 1,
            makes: 1,
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
            // A beam half a metre along the sensor's X.
            Kind::Sensor => Self {
                travel: 0.5,
                ..Self::default()
            },
            // A stroke of 100 mm up its own Z in a second.
            Kind::Actuator => Self {
                door: 1.0,
                axis: 2,
                travel: 0.1,
                ..Self::default()
            },
            // A part a minute in; a minute's work; ten kept; parts carried
            // on at a walk.
            Kind::Source => Self {
                feed: 60.0,
                speed: 1.0,
                ..Self::default()
            },
            Kind::Station => Self {
                cycle: 60.0,
                speed: 1.0,
                ..Self::default()
            },
            Kind::Buffer => Self {
                capacity: 10,
                speed: 1.0,
                ..Self::default()
            },
            // A walk on a shop floor, a little over 1 m/s.
            Kind::Operator => Self {
                speed: 1.2,
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
        let few = |key: &str, now: u32, most: u32| {
            v[key]
                .as_f64()
                .map_or(now, |x| x.clamp(1.0, most as f64) as u32)
        };
        self.capacity = few("capacity", self.capacity, 1000);
        self.carrier = v["carrier"]
            .as_f64()
            .map_or(self.carrier, |x| x.round().clamp(0.0, 3.0) as u32);
        self.crew = v["crew"]
            .as_f64()
            .map_or(self.crew, |x| if x > 0.5 { 1 } else { 0 });
        self.takes = few("takes", self.takes, 100);
        self.makes = few("makes", self.makes, 20);
        self.dist = v["dist"]
            .as_f64()
            .map_or(self.dist, |x| x.round().clamp(0.0, 4.0) as u32);
        self.spread = v["spread"]
            .as_f64()
            .map_or(self.spread, |x| x.clamp(0.0, 36000.0));
        let hours = |key: &str, now: f64| v[key].as_f64().map_or(now, |x| x.clamp(0.0, 1.0e7));
        self.mtbf = hours("mtbf", self.mtbf);
        self.mttr = hours("mttr", self.mttr);
        self.scrap = v["scrap"]
            .as_f64()
            .map_or(self.scrap, |x| x.clamp(0.0, 1.0));
        self.wage = v["wage"]
            .as_f64()
            .map_or(self.wage, |x| x.clamp(0.0, 1.0e8));
        self.shift_from = v["shift_from"]
            .as_f64()
            .map_or(self.shift_from, |x| x.clamp(0.0, 24.0));
        self.shift_to = v["shift_to"]
            .as_f64()
            .map_or(self.shift_to, |x| x.clamp(0.0, 24.0));
        self.battery = v["battery"]
            .as_f64()
            .map_or(self.battery, |x| x.clamp(0.0, 100000.0));
        self.charge = v["charge"]
            .as_f64()
            .map_or(self.charge, |x| x.clamp(0.1, 100000.0));
        self.charger = v["charger"]
            .as_f64()
            .map_or(self.charger, |x| if x > 0.5 { 1 } else { 0 });
        self.dispatch = v["dispatch"]
            .as_f64()
            .map_or(self.dispatch, |x| if x > 0.5 { 1 } else { 0 });
        self.rail = v["rail"]
            .as_f64()
            .map_or(self.rail, |x| x.round().clamp(0.0, 2.0) as u32);
        self.kanban = v["kanban"]
            .as_f64()
            .map_or(self.kanban, |x| x.clamp(0.0, 100000.0) as u32);
        self.rule = v["rule"]
            .as_f64()
            .map_or(self.rule, |x| x.round().clamp(0.0, 2.0) as u32);
        self.policy = v["policy"]
            .as_f64()
            .map_or(self.policy, |x| if x > 0.5 { 1 } else { 0 });
        self.setup = v["setup"]
            .as_f64()
            .map_or(self.setup, |x| x.clamp(0.0, 36000.0));
        self.look = v["look"].as_f64().map_or(self.look, |x| {
            x.round().clamp(0.0, (LOOKS.len() - 1) as f64) as u32
        });
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
            "capacity": self.capacity,
            "carrier": self.carrier,
            "crew": self.crew,
            "dist": self.dist,
            "spread": self.spread,
            "mtbf": self.mtbf,
            "mttr": self.mttr,
            "scrap": self.scrap,
            "setup": self.setup,
            "look": self.look,
            "policy": self.policy,
            "kanban": self.kanban,
            "battery": self.battery,
            "shift_from": self.shift_from,
            "wage": self.wage,
            "shift_to": self.shift_to,
            "charge": self.charge,
            "charger": self.charger,
            "dispatch": self.dispatch,
            "rail": self.rail,
            "rule": self.rule,
            "takes": self.takes,
            "makes": self.makes,
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
    /// The real machine it follows, if it does (see `crate::mtconnect`):
    /// its door, its cutting and its count of parts are then that
    /// machine's, and it takes no commands.
    pub mirror: Option<crate::mtconnect::Source>,
}

/// A mirror's source as saved (see `crate::mtconnect::Source`).
fn mirror_of(v: &Value) -> Option<crate::mtconnect::Source> {
    crate::mtconnect::Source::of(v)
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
        Kind::Sensor => (&[], &["detected"]),
        Kind::Actuator => (&["extend", "retract"], &["extended", "retracted"]),
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
    /// When it came into the cell, on the simulation's clock.
    pub born: f64,
    /// Where it is in a process flow, if it is in one (see `flow`).
    pub flow: Option<flow::Place>,
    /// Which product it is, 0 for A on (see `flow::PRODUCTS`).
    pub product: u8,
    /// The source it came in at, and the order it was made for (0: none).
    pub origin: u64,
    pub order: u32,
    /// The step of its source's recipe it goes to next.
    pub step: u32,
    /// What it looks like (see [`LOOKS`]): as the last flow piece that
    /// made it has it.
    pub look: u32,
    /// On its way by a stocker's crane: where it goes onto the rail, where
    /// off it, and the rail's way (see `Props::rail`).
    pub rail: Option<[DVec3; 3]>,
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
    /// Whether the door was left part open by hand (`door_to`): it stays
    /// so until the machine is told to open or close it.
    pub held: bool,
    /// Whether the chuck is told to clamp (or has), rather than let go.
    pub clamped: bool,
    /// How far closed the chuck is: 0 open, 1 clamped. It closes and opens
    /// over the machine's chuck time.
    pub chuck: f64,
    /// Seconds into the cycle, while machining.
    pub machining: Option<f64>,
    /// Cycles finished since the cell started.
    pub cycles: u64,
    /// Following a real machine: when it began cutting, on the
    /// simulation's clock; its part count when first read; and how its
    /// agent is getting on, for the page.
    pub cutting_since: Option<f64>,
    pub parts_at_start: Option<i64>,
    pub mirror_state: Option<Value>,
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
#[derive(Clone)]
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
    /// Each AMR following a real one: how its feed is getting on.
    follows: BTreeMap<u64, Value>,
    /// The process flow: where each of its parts goes next, how each
    /// stands, whether it runs (see `flow`).
    flow_next: BTreeMap<u64, Vec<u64>>,
    /// Each source's recipe: the stations its parts go through, in order —
    /// a tool more than once if the recipe says (a pattern repeated, as a
    /// display's masks are) — sent to each in turn by a stocker.
    recipes: BTreeMap<u64, Vec<u64>>,
    stations: BTreeMap<u64, flow::Station>,
    flow_on: bool,
    flow_stats: flow::Stats,
    /// What last kept the flow from going on as it should, for the page.
    flow_problem: Option<String>,
    /// The flow's random numbers: drawn in turn from its seed, the same
    /// run after run for one seed.
    rng: u64,
    /// Products: each source's mix, by product; each station's time for
    /// each (0: its own); the products each link of the flow takes (none:
    /// all); and the calendar and warm-up (see `flow`).
    mix: BTreeMap<u64, Vec<f64>>,
    /// Each source's orders, and how far each has got (see `flow`).
    orders: BTreeMap<u64, Vec<flow::Order>>,
    cycles: BTreeMap<u64, Vec<f64>>,
    routes: BTreeMap<(u64, u64), Vec<u8>>,
    flow_cfg: flow::Settings,
    /// Room on each belt of the flow for one more, this step; the tops of
    /// each rack's shelves, lowest first.
    belt_room: BTreeMap<u64, u32>,
    shelf_tops: BTreeMap<u64, Vec<DVec3>>,
    /// Where the flow's parts are set out on each component, and its
    /// shelves, as found for the scene at a version with these
    /// components: found again only when either changes.
    docks_seen: Option<flow::Docks>,
    warmed: bool,
    just_warmed: bool,
    flow_start: Option<f64>,
    /// The lanes between stops, both ways; the stop each AMR holds; the
    /// parts AMRs are to carry (see `fleet`).
    lanes: BTreeMap<u64, Vec<u64>>,
    holds: BTreeMap<u64, u64>,
    /// Each AMR's charge, 0 to 1 (full if not here); those charging until
    /// nearly full; and the stop each is waiting to drive to, held by
    /// another.
    energy: BTreeMap<u64, f64>,
    /// The stations each operator may work at (none: all).
    skills: BTreeMap<u64, Vec<u64>>,
    /// What the equipment placed cost, ten thousand won (the library's).
    investment: f64,
    charging: std::collections::BTreeSet<u64>,
    blocked: BTreeMap<u64, u64>,
    jobs: Vec<fleet::Job>,
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
    /// How far it has walked, metres: a person's stride is drawn from it.
    walked: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Leg {
    /// Turn on the spot to face this way.
    Turn(f64),
    /// Drive straight to stop `0`, once no other AMR holds it (see
    /// `fleet`).
    Via(u64),
    /// Walk straight here: a person, who goes round others.
    Walk(DVec3),
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

/// The longest step the cell is worked out over at once, seconds of the
/// simulation's clock: however fast it runs, it is stepped no coarser.
const MOST_STEP: f64 = 0.2;

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

thread_local! {
    /// Objects by identity, as last found, and the parts found by name
    /// below them. `Document` finds an object by looking at every one, and
    /// a running cell asks every step: what was found is kept, checked on
    /// each use (still there, still that one), and looked for again only
    /// when wrong — the scene changed — so no answer is ever stale.
    static BY_UID: RefCell<HashMap<u64, ObjectId>> = RefCell::new(HashMap::new());
    static NAMED: RefCell<HashMap<(ObjectId, &'static str), ObjectId>> =
        RefCell::new(HashMap::new());
}

fn id_of(doc: &Document, uid: u64) -> Option<ObjectId> {
    let is = |id: ObjectId| doc.object_uid(id).map(|u| u.raw()) == Some(uid);
    if let Some(id) = BY_UID
        .with(|m| m.borrow().get(&uid).copied())
        .filter(|id| is(*id))
    {
        return Some(id);
    }
    // Wrong or never found: every object's identity again, in one look —
    // the first of any two with one identity, as `Document` finds it.
    BY_UID.with(|m| {
        let mut m = m.borrow_mut();
        m.clear();
        for (id, _) in doc.objects() {
            if let Some(u) = doc.object_uid(id) {
                m.entry(u.raw()).or_insert(id);
            }
        }
        m.get(&uid).copied()
    })
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

/// A child of `parent`, at any depth, whose name holds `word` (lower case,
/// ASCII), in any case. Asked every step of a running cell: what is below
/// is looked at first, and no name is copied.
fn part_named(doc: &Document, parent: ObjectId, word: &'static str) -> Option<ObjectId> {
    let named = |id: ObjectId| {
        doc.object(id).is_some_and(|o| {
            o.name
                .as_bytes()
                .windows(word.len())
                .any(|w| w.eq_ignore_ascii_case(word.as_bytes()))
        })
    };
    let kept = NAMED.with(|m| m.borrow().get(&(parent, word)).copied());
    if let Some(id) = kept.filter(|id| named(*id) && is_below(doc, *id, parent)) {
        return Some(id);
    }
    let found = doc
        .objects()
        .map(|(id, _)| id)
        .filter(|id| is_below(doc, *id, parent))
        .find(|id| named(*id))?;
    NAMED.with(|m| {
        let mut m = m.borrow_mut();
        // Kept small: a scene's machines and AMRs, not every scene seen.
        if m.len() > 4096 {
            m.clear();
        }
        m.insert((parent, word), found);
    });
    Some(found)
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
            if saved["flow_settings"].is_object() {
                self.flow_cfg = flow::Settings::of(&saved["flow_settings"]);
                continue;
            }
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
            if let (Some(object), Some(next)) = (found, saved["next"].as_array()) {
                let next: Vec<u64> = next
                    .iter()
                    .filter_map(|n| n.as_str().and_then(|s| s.parse().ok()))
                    .collect();
                self.flow_next.insert(object, next);
            }
            if let (Some(object), Some(steps)) = (found, saved["recipe"].as_array()) {
                let steps: Vec<u64> = steps
                    .iter()
                    .filter_map(|n| n.as_str().and_then(|s| s.parse().ok()))
                    .collect();
                self.recipes.insert(object, steps);
            }
            if let Some(object) = found {
                let floats = |v: &Value| -> Option<Vec<f64>> {
                    Some(v.as_array()?.iter().filter_map(Value::as_f64).collect())
                };
                if let Some(skills) = saved["skills"].as_array() {
                    let skills = skills
                        .iter()
                        .filter_map(|x| x.as_str().and_then(|s| s.parse::<u64>().ok()))
                        .collect();
                    self.skills.insert(object, skills);
                }
                if let Some(orders) = saved["orders"].as_array() {
                    self.orders
                        .insert(object, orders.iter().filter_map(flow::Order::of).collect());
                }
                if let Some(mix) = floats(&saved["mix"]) {
                    self.mix.insert(object, mix);
                }
                if let Some(cycles) = floats(&saved["cycles"]) {
                    self.cycles.insert(object, cycles);
                }
                if let Some(routes) = saved["routes"].as_object() {
                    for (to, p) in routes {
                        let (Ok(to), Some(p)) = (to.parse::<u64>(), p.as_array()) else {
                            continue;
                        };
                        let p = p
                            .iter()
                            .filter_map(|x| x.as_u64().map(|x| x as u8))
                            .collect();
                        self.routes.insert((object, to), p);
                    }
                }
            }
            if let (Some(object), Some(lanes)) = (found, saved["lanes"].as_array()) {
                let lanes: Vec<u64> = lanes
                    .iter()
                    .filter_map(|n| n.as_str().and_then(|s| s.parse().ok()))
                    .collect();
                self.lanes.insert(object, lanes);
            }
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
                        mirror: mirror_of(&saved["mirror"]),
                    });
                    self.changed = true;
                }
                // A role equipment came with: the settings saved win over
                // its defaults.
                (Some(object), Some(kind)) => {
                    if let Some(c) = self.components.iter_mut().find(|c| c.object == object) {
                        c.io = io_of(&saved["io"]);
                        c.plc = plc_of(&saved["plc"]);
                        c.mirror = mirror_of(&saved["mirror"]);
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
                    "mirror": c.mirror.as_ref().map(crate::mtconnect::Source::to_json),
                    "next": self.flow_next.get(&c.object).map(|n| {
                        n.iter().map(u64::to_string).collect::<Vec<_>>()
                    }),
                    "recipe": self.recipes.get(&c.object).map(|n| {
                        n.iter().map(u64::to_string).collect::<Vec<_>>()
                    }),
                    "lanes": self.lanes.get(&c.object).map(|n| {
                        n.iter().map(u64::to_string).collect::<Vec<_>>()
                    }),
                    "mix": self.mix.get(&c.object),
                    "skills": self.skills.get(&c.object).map(|n| n.iter().map(u64::to_string).collect::<Vec<_>>()),
                    "orders": self.orders.get(&c.object).map(|o| o.iter().map(flow::Order::to_json).collect::<Vec<_>>()),
                    "cycles": self.cycles.get(&c.object),
                    "routes": self
                        .routes
                        .iter()
                        .filter(|((from, _), _)| *from == c.object)
                        .map(|((_, to), p)| (to.to_string(), json!(p)))
                        .collect::<serde_json::Map<_, _>>(),
                })
            })
            // The flow's calendar and warm-up, with the roles.
            .chain((!self.flow_cfg.is_default()).then(|| json!({ "flow_settings": self.flow_cfg.to_json() })))
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
    /// The machines' doors as they stand, as undoing keeps them: by
    /// machine, how far open, whether opening, whether left so by hand.
    pub fn doors_json(&self) -> Value {
        json!(self
            .machines
            .iter()
            .filter(|(_, m)| m.door > 0.0 || m.opening || m.held)
            .map(|(object, m)| (object.to_string(), json!([m.door, m.opening, m.held])))
            .collect::<BTreeMap<_, _>>())
    }

    /// The doors as `v` (a [`Self::doors_json`]) has them, the others shut.
    pub fn set_doors(&mut self, v: &Value) {
        for (object, m) in self.machines.iter_mut() {
            let (door, opening, held) = match &v[object.to_string()] {
                Value::Array(a) => (
                    a.first()
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0)
                        .clamp(0.0, 1.0),
                    a.get(1).and_then(Value::as_bool).unwrap_or(false),
                    a.get(2).and_then(Value::as_bool).unwrap_or(false),
                ),
                _ => (0.0, false, false),
            };
            if m.machining.is_none() {
                (m.door, m.opening, m.held) = (door, opening, held);
            }
        }
        self.changed = true;
    }

    /// Object `object`'s role under its new name (its equipment renamed).
    pub fn rename(&mut self, object: u64, name: &str) {
        if let Some(c) = self.components.iter_mut().find(|c| c.object == object) {
            c.name = name.to_string();
            self.changed = true;
            self.roles_changed = true;
        }
    }

    pub fn set_props(&mut self, object: u64, v: &Value) {
        if let Some(c) = self.components.iter_mut().find(|c| c.object == object) {
            c.props.update(v);
            self.changed = true;
            self.roles_changed = true;
        }
    }

    /// Object `to` (a copy of `from`) takes `from`'s settings: its cycle,
    /// its door's times, its grid. Not its wiring, nor the real machine it
    /// follows: those are one machine's own.
    pub fn copy_props(&mut self, from: u64, to: u64) {
        let Some((props, kind)) = self
            .components
            .iter()
            .find(|c| c.object == from)
            .map(|c| (c.props, c.kind))
        else {
            return;
        };
        if let Some(c) = self
            .components
            .iter_mut()
            .find(|c| c.object == to && c.kind == kind)
        {
            c.props = props;
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
            mirror: None,
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
            born: self.last_tick.unwrap_or(0.0),
            flow: None,
            product: 0,
            origin: 0,
            order: 0,
            step: 0,
            look: 0,
            rail: None,
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
                // A part of a machine — its door, a panel — is not a machine
                // of its own: the machine above it is.
                if let Some(kind) = req["kind"].as_str().and_then(Kind::of) {
                    if kind.machine() {
                        let mut at = doc.object(id).and_then(|o| o.parent);
                        for _ in 0..64 {
                            let Some(up) = at else { break };
                            let above = doc.object_uid(up).map(|u| u.raw());
                            if let Some(m) = self
                                .components
                                .iter()
                                .find(|c| Some(c.object) == above && c.kind.machine())
                            {
                                return Err(format!(
                                    "이 부품은 이미 장비 \"{}\"의 일부입니다. 장비 전체를 고르세요",
                                    m.name
                                ));
                            }
                            at = doc.object(up).and_then(|o| o.parent);
                        }
                    }
                }
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
                        mirror: was
                            .as_ref()
                            .filter(|c| c.kind == kind)
                            .and_then(|c| c.mirror.clone()),
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
                        mirror: None,
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
            // A machine to follow a real one over MTConnect, or no more.
            "mirror" => {
                let object = uid("object")?;
                let c = self
                    .components
                    .iter_mut()
                    .find(|c| c.object == object && (c.kind.machine() || c.kind == Kind::Amr))
                    .ok_or("CNC, 사출기, AMR이 아닙니다")?;
                let source = mirror_of(&req);
                // A machine follows its agent; an AMR, its fleet's messages.
                let mqtt = matches!(source, Some(crate::mtconnect::Source::Mqtt { .. }));
                if source.is_some() && mqtt != (c.kind == Kind::Amr) {
                    return Err(if mqtt {
                        "MQTT로 따라가는 것은 AMR입니다".to_string()
                    } else {
                        "AMR은 MQTT(VDA 5050)로 따라갑니다".to_string()
                    });
                }
                c.mirror = source;
                self.rovers.remove(&object);
                self.follows.remove(&object);
                if let Some(m) = c.mirror.as_ref() {
                    m.check()?;
                }
                // Its state its own again, or the real one's from now.
                self.machines.remove(&object);
                self.roles_changed = true;
                self.changed = true;
            }
            // The PLC signal table, whole.
            "signals" => self.set_signals(&req["signals"])?,
            // The process flow: started, stopped, reset; where a part of it
            // sends its parts.
            "flow" => {
                if let Some(seed) = req["seed"].as_u64() {
                    self.seed_flow(seed);
                }
                self.flow(req["action"].as_str().unwrap_or(""))?
            }
            "amr_lanes" => {
                let next = req["next"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|n| n.as_str().and_then(|s| s.parse::<u64>().ok()))
                    .collect();
                self.set_lanes(uid("object")?, next, req["oneway"] == true)?;
            }
            "flow_products" => {
                let object = uid("object")?;
                let floats = |key: &str| -> Vec<f64> {
                    req[key]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|x| x.as_f64().unwrap_or(0.0).clamp(0.0, 1.0e6))
                        .take(flow::PRODUCTS.len())
                        .collect()
                };
                if req["mix"].is_array() {
                    self.mix.insert(object, floats("mix"));
                }
                if req["cycles"].is_array() {
                    self.cycles.insert(object, floats("cycles"));
                }
                if let Some(to) = req["to"].as_str().and_then(|s| s.parse::<u64>().ok()) {
                    let p: Vec<u8> = req["products"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|x| x.as_u64())
                        .filter(|x| (*x as usize) < flow::PRODUCTS.len())
                        .map(|x| x as u8)
                        .collect();
                    if p.is_empty() {
                        self.routes.remove(&(object, to));
                    } else {
                        self.routes.insert((object, to), p);
                    }
                }
                self.roles_changed = true;
                self.changed = true;
            }
            "flow_orders" => {
                let object = uid("object")?;
                let orders = flow::Order::parse(req["csv"].as_str().unwrap_or(""))?;
                if orders.is_empty() {
                    self.orders.remove(&object);
                } else {
                    self.orders.insert(object, orders);
                }
                self.roles_changed = true;
                self.changed = true;
            }
            "operator_skills" => {
                let object = uid("object")?;
                let stations: Vec<u64> = req["stations"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|x| x.as_str().and_then(|s| s.parse::<u64>().ok()))
                    .collect();
                if stations.is_empty() {
                    self.skills.remove(&object);
                } else {
                    self.skills.insert(object, stations);
                }
                self.roles_changed = true;
                self.changed = true;
            }
            "flow_settings" => {
                self.flow_cfg = flow::Settings::of(&req);
                self.roles_changed = true;
                self.changed = true;
            }
            // A source's recipe: `steps` (stations), all of it `repeat`
            // times — a display's five masks, its glass back to the same
            // tools each time.
            "flow_recipe" => {
                let object = uid("object")?;
                let steps: Vec<u64> = req["steps"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|n| n.as_str().and_then(|s| s.parse::<u64>().ok()))
                    .collect();
                let repeat = req["repeat"].as_u64().unwrap_or(1).clamp(1, 100) as usize;
                self.set_recipe(object, steps.repeat(repeat))?;
            }
            "flow_next" => {
                let next = req["next"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|n| n.as_str().and_then(|s| s.parse::<u64>().ok()))
                    .collect();
                self.set_flow_next(uid("object")?, next)?;
            }
            "grip" => self.grip(doc, uid("gripper")?)?,
            "release" => self.release(doc, uid("gripper")?)?,
            // A machine's door dragged to `open` (0 shut, 1 open), as it is
            // left: not while it machines, nor while it follows a real one.
            "door_to" => {
                let object = uid("object")?;
                let c = self
                    .components
                    .iter()
                    .find(|c| c.object == object && c.kind.machine())
                    .ok_or("no such machine")?;
                if c.mirror.is_some() {
                    return Err("실제 장비를 따라가는 동안에는 문을 움직일 수 없습니다".into());
                }
                let open = req["open"]
                    .as_f64()
                    .filter(|v| v.is_finite())
                    .ok_or("open is a number")?;
                let m = self.machines.entry(object).or_default();
                if m.machining.is_some() {
                    return Err("가공 중에는 문을 움직일 수 없습니다".into());
                }
                m.door = open.clamp(0.0, 1.0);
                m.opening = m.door >= 0.5;
                m.held = m.door > 0.0 && m.door < 1.0;
                self.changed = true;
            }
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
        // A robot carrying the flow's part takes that part, not whichever
        // is nearest: the others waiting beside it close up as it is
        // taken, and one of them may have come to stand where it was.
        let mut robot = Vec::new();
        let mut up = id_of(doc, gripper)
            .and_then(|id| doc.object(id))
            .and_then(|o| o.parent);
        for _ in 0..64 {
            let Some(id) = up else { break };
            if let Some(u) = doc.object_uid(id) {
                robot.push(u.raw());
            }
            up = doc.object(id).and_then(|o| o.parent);
        }
        let riding = self.parts.iter().position(|p| {
            p.held.is_none()
                && matches!(p.flow, Some(flow::Place::Riding { amr, .. }) if robot.contains(&amr))
                && p.world.translation.distance(at.translation) < GRIP_REACH
        });
        let near = match riding {
            Some(k) => {
                let p = &mut self.parts[k];
                Some((p.world.translation.distance(at.translation), p))
            }
            None => self
                .parts
                .iter_mut()
                .filter(|p| p.held.is_none())
                .map(|p| (p.world.translation.distance(at.translation), p))
                .filter(|(d, _)| *d < GRIP_REACH)
                .min_by(|a, b| a.0.total_cmp(&b.0)),
        };
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
        let axes = self.axis_boxes(doc);
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
        } else if let Some(top) = axes
            .iter()
            .filter(|(_, lo, hi)| {
                let above = here.z - part.size().1 / 2.0 - hi.z;
                (lo.x..=hi.x).contains(&here.x)
                    && (lo.y..=hi.y).contains(&here.y)
                    && (-0.01..0.1).contains(&above)
            })
            .map(|(_, _, hi)| hi.z)
            .next()
        {
            // On an axis's top — a lifter's table, a shuttle: it rides along.
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
            .find(|c| c.object == machine && (c.kind.machine() || c.kind == Kind::Actuator))
            .cloned()
            .ok_or("명령을 받는 장비(CNC, 사출기, 동작축)가 아닙니다")?;
        if c.kind == Kind::Actuator {
            let m = self.machines.entry(machine).or_default();
            m.held = false;
            m.opening = match command {
                Command::Extend => true,
                Command::Retract => false,
                _ => return Err("동작축은 전진·후진 명령만 받습니다".into()),
            };
            self.changed = true;
            return Ok(());
        }
        if matches!(command, Command::Extend | Command::Retract) {
            return Err("전진·후진은 동작축의 명령입니다".into());
        }
        if c.mirror.is_some() {
            return Err("실제 장비(MTConnect)를 따라가는 중이라 명령을 받지 않습니다".into());
        }
        let in_chuck = self.in_chuck(doc, &c);
        let m = self.machines.entry(machine).or_default();
        let busy = m.machining.is_some();
        // Told to open or close: a door left part open by hand goes on.
        if matches!(command, Command::Open | Command::Close) && !busy {
            m.held = false;
        }
        if c.kind == Kind::Molder {
            // A moulding machine: a guard and its cycle; it makes the part.
            match command {
                Command::Clamp | Command::Unclamp => return Err("사출기에는 척이 없습니다".into()),
                Command::Extend | Command::Retract => unreachable!("refused above"),
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
            Command::Extend | Command::Retract => unreachable!("refused above"),
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
        if matches!(c.kind, Kind::Amr | Kind::Positioner | Kind::Operator) {
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
        if c.kind == Kind::Sensor {
            return signal == Signal::Detected && self.detects(doc, c);
        }
        if c.kind == Kind::Actuator {
            let m = self.machines.get(&machine).cloned().unwrap_or_default();
            return match signal {
                Signal::Extended => m.opening && m.door >= 1.0,
                Signal::Retracted => !m.opening && m.door <= 0.0,
                _ => false,
            };
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
            Signal::PartAtEnd
            | Signal::Arrived
            | Signal::Detected
            | Signal::Extended
            | Signal::Retracted => false,
        }
    }

    /// Sensor `c`'s beam where it is: from, the way it points, how long.
    fn beam(doc: &Document, c: &Component) -> Option<(DVec3, DVec3, f64)> {
        let world = doc.world_transform(id_of(doc, c.object)?);
        let unit = [DVec3::X, DVec3::Y, DVec3::Z][c.props.axis.min(2) as usize];
        let way = (world.matrix3 * unit).normalize_or_zero();
        let length = if c.props.travel.abs() > 1e-6 {
            c.props.travel
        } else {
            0.5
        };
        // A beam set the other way points back along its axis.
        Some((world.translation, way * length.signum(), length.abs()))
    }

    /// Whether a part breaks sensor `c`'s beam: a part held or not, the
    /// beam passing within its round and its height.
    fn detects(&self, doc: &Document, c: &Component) -> bool {
        let Some((from, way, length)) = Self::beam(doc, c) else {
            return false;
        };
        self.parts.iter().any(|p| {
            let at = p.world.translation;
            let along = (at - from).dot(way).clamp(0.0, length);
            let d = at - (from + way * along);
            let (r, h) = p.size();
            DVec3::new(d.x, d.y, 0.0).length() <= r && d.z.abs() <= h / 2.0
        })
    }

    /// Axis `c` as far out as `out` (0 where placed, 1 at its far end):
    /// how the world moves with it from where it was placed.
    fn axis_shift(doc: &Document, c: &Component, out: f64) -> Option<DAffine3> {
        let id = id_of(doc, c.object)?;
        Some(match door_motion(doc, id, id, Some(&c.props))? {
            DoorMotion::Swing { pivot, axis, angle } => {
                DAffine3::from_translation(pivot)
                    * DAffine3::from_axis_angle(axis, angle * out)
                    * DAffine3::from_translation(-pivot)
            }
            DoorMotion::Slide(by) => DAffine3::from_translation(by * out),
        })
    }

    /// Each axis's box in the world as it now stands: the object's own.
    fn axis_boxes(&self, doc: &Document) -> Vec<(u64, DVec3, DVec3)> {
        self.components
            .iter()
            .filter(|c| c.kind == Kind::Actuator)
            .filter_map(|c| {
                let out = self.machines.get(&c.object).map_or(0.0, |m| m.door);
                let shift = Self::axis_shift(doc, c, out)?;
                let (lo, hi) = bounds(doc, id_of(doc, c.object)?)?;
                let mut a = DVec3::splat(f64::INFINITY);
                let mut b = DVec3::splat(f64::NEG_INFINITY);
                for k in 0..8 {
                    let corner = DVec3::new(
                        if k & 1 == 0 { lo.x } else { hi.x },
                        if k & 2 == 0 { lo.y } else { hi.y },
                        if k & 4 == 0 { lo.z } else { hi.z },
                    );
                    let w = shift.transform_point3(corner);
                    a = a.min(w);
                    b = b.max(w);
                }
                Some((c.object, a, b))
            })
            .collect()
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

    /// Machine `machine`, following a umati server it trusts no
    /// certificate of yet, trusts `thumbprint` from now on (and so it is
    /// saved). Returns its source as it is then, if it changed.
    pub fn trust_mirror(
        &mut self,
        machine: u64,
        thumbprint: &str,
    ) -> Option<crate::mtconnect::Source> {
        let c = self.components.iter_mut().find(|c| c.object == machine)?;
        let Some(crate::mtconnect::Source::Umati { settings, .. }) = c.mirror.as_mut() else {
            return None;
        };
        if !settings.trust.is_empty() {
            return None;
        }
        settings.trust = thumbprint.to_string();
        self.roles_changed = true;
        c.mirror.clone()
    }

    /// The machines that follow real ones, and where.
    pub fn mirror_sources(&self) -> Vec<(u64, crate::mtconnect::Source)> {
        self.components
            .iter()
            .filter_map(|c| Some((c.object, c.mirror.clone()?)))
            .collect()
    }

    /// Machine `machine` as the real one it follows was last read, `seen`
    /// (none while its agent does not answer), at `time` on the
    /// simulation's clock: its door open or shut, cutting or not, and the
    /// parts it has made since it was first read counted as its cycles.
    /// `state` is how its agent is getting on, for the page.
    pub fn mirror(
        &mut self,
        machine: u64,
        seen: Option<&crate::mtconnect::Observation>,
        time: f64,
        state: Value,
    ) {
        let m = self.machines.entry(machine).or_default();
        if m.mirror_state.as_ref() != Some(&state) {
            m.mirror_state = Some(state);
            self.changed = true;
        }
        let Some(seen) = seen else { return };
        let before = (m.door, m.opening, m.machining.is_some(), m.cycles);
        match seen.door.as_deref() {
            Some("OPEN") | Some("UNLATCHED") => {
                m.door = 1.0;
                m.opening = true;
            }
            Some("CLOSED") => {
                m.door = 0.0;
                m.opening = false;
            }
            _ => {}
        }
        if seen.execution.as_deref() == Some("ACTIVE") {
            let since = *m.cutting_since.get_or_insert(time);
            m.machining = Some((time - since).max(0.0));
        } else {
            m.cutting_since = None;
            m.machining = None;
        }
        if let Some(count) = seen.part_count {
            let start = *m.parts_at_start.get_or_insert(count);
            // A count reset at the machine starts the count again.
            if count < start {
                m.parts_at_start = Some(count);
            } else {
                m.cycles = (count - start) as u64;
            }
        }
        if before != (m.door, m.opening, m.machining.is_some(), m.cycles) {
            self.changed = true;
        }
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
        // In steps of at most `MOST_STEP`: run fast, the clock goes on
        // seconds between two looks, and the cell worked out over them at
        // once would jump — an AMR past a stop, a part past its station.
        let from = self.last_tick.unwrap_or(sim).min(sim);
        let steps = ((sim - from) / MOST_STEP).ceil().max(1.0) as usize;
        let (mut moving, mut finished) = (false, false);
        for k in 1..=steps {
            let t = if k == steps {
                sim
            } else {
                from + (sim - from) * k as f64 / steps as f64
            };
            let dt = self.last_tick.map_or(0.0, |last| (t - last).max(0.0));
            self.last_tick = Some(t);
            let (m, f) = self.step(doc, dt);
            moving |= m;
            finished |= f;
        }
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
        self.flow_on
            || self.conveying
            || self.rovers.values().any(|r| !r.legs.is_empty())
            || self
                .components
                .iter()
                .any(|c| c.kind == Kind::Conveyor && c.props.feed > 0.0)
            || self.machines.values().any(|m| {
                m.machining.is_some()
                    || (!m.held && m.opening && m.door < 1.0)
                    || (!m.held && !m.opening && m.door > 0.0)
                    || m.chuck != if m.clamped { 1.0 } else { 0.0 }
            })
    }

    /// Moves the machines on by `dt` seconds. Returns whether anything moved
    /// or ran, and whether a state changed: a door reaching its end, a
    /// cycle ending.
    fn step(&mut self, doc: &Document, dt: f64) -> (bool, bool) {
        // Each axis as it stood, and its box: what stands on it goes with it.
        let axes_before: Vec<(u64, f64)> = self
            .components
            .iter()
            .filter(|c| c.kind == Kind::Actuator)
            .map(|c| {
                (
                    c.object,
                    self.machines.get(&c.object).map_or(0.0, |m| m.door),
                )
            })
            .collect();
        let boxes_before = self.axis_boxes(doc);
        let mut moving = false;
        let mut finished = false;
        let mut done: Vec<DVec3> = Vec::new();
        let mut moulded: Vec<DVec3> = Vec::new();
        for c in &self.components {
            if c.mirror.is_some() {
                continue;
            }
            let Some(m) = self.machines.get_mut(&c.object) else {
                continue;
            };
            let target = if m.opening { 1.0 } else { 0.0 };
            if m.door != target && !m.held {
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
        // The parts standing on an axis that moved, moved with it.
        for (object, before) in axes_before {
            let Some(c) = self.components.iter().find(|c| c.object == object).cloned() else {
                continue;
            };
            let after = self.machines.get(&object).map_or(0.0, |m| m.door);
            if after == before {
                continue;
            }
            let (Some(from), Some(to)) = (
                Self::axis_shift(doc, &c, before),
                Self::axis_shift(doc, &c, after),
            ) else {
                continue;
            };
            let Some(&(_, lo, hi)) = boxes_before.iter().find(|b| b.0 == object) else {
                continue;
            };
            let shift = to * from.inverse();
            let (_, turn, _) = shift.to_scale_rotation_translation();
            for p in &mut self.parts {
                if p.held.is_some() {
                    continue;
                }
                let foot = p.world.translation - DVec3::Z * (p.size().1 / 2.0);
                let on = foot.x >= lo.x
                    && foot.x <= hi.x
                    && foot.y >= lo.y
                    && foot.y <= hi.y
                    && (foot.z - hi.z).abs() < 0.01;
                if on {
                    let at = shift.transform_point3(p.world.translation);
                    let (_, own, _) = p.world.to_scale_rotation_translation();
                    p.world = DAffine3::from_rotation_translation(turn * own, at);
                }
            }
        }
        let (flowed, flow_changed) = self.flow_step(doc, dt);
        moving |= flowed;
        finished |= flow_changed;
        let (carried, arrived) = self.convey(doc, dt);
        self.conveying = carried;
        let (driven, stopped) = self.drive_on(doc, dt);
        let recharged = self.fleet_care(doc, dt);
        (
            moving || carried || driven,
            finished || arrived || stopped || recharged,
        )
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
            walked: 0.0,
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
        if c.mirror.is_some() {
            return Err("실제 AMR을 따라가는 중이라 보내지 않습니다".into());
        }
        let mut r = self.rover(doc, &c).ok_or("no such AMR")?;
        if !r.legs.is_empty() {
            return Err("AMR가 아직 주행 중입니다".into());
        }
        // Along the lanes, if there are any; straight there if not.
        let _ = (to, facing);
        r.legs = self.route(doc, r.at, stop)?;
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
                matches!(c.kind, Kind::Amr | Kind::Positioner | Kind::Operator)
                    && self
                        .rovers
                        .get(&c.object)
                        .is_some_and(|r| !r.legs.is_empty())
            })
            .cloned()
            .collect();
        let places = self.stop_places(doc);
        // Each deck is where its own AMR is, which only its own turn below
        // moves: worked out once, not once an AMR.
        let decks = if amrs.is_empty() {
            Vec::new()
        } else {
            self.decks(doc)
        };
        for c in amrs {
            let deck = decks.iter().find(|d| d.object == c.object).cloned();
            let r = self.rovers.get_mut(&c.object).expect("an AMR on its way");
            let before = pose_of(r.at, r.yaw);
            let mut left = dt;
            while left > 1e-12 {
                let Some(leg) = r.legs.first().copied() else {
                    break;
                };
                match leg {
                    Leg::Turn(to) => {
                        let rate = match c.kind {
                            Kind::Positioner => c.props.speed.max(1e-6),
                            // A person turns round in a second.
                            Kind::Operator => 3.0,
                            _ => AMR_TURN,
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
                    Leg::Walk(to) => {
                        let to = DVec3::new(to.x, to.y, r.at.z);
                        let need = to.distance(r.at);
                        let can = c.props.speed.max(0.01) * left;
                        if need <= can {
                            r.at = to;
                            r.walked += need;
                            left -= need / c.props.speed.max(0.01);
                            r.legs.remove(0);
                        } else {
                            r.at += (to - r.at) / need * can;
                            r.walked += can;
                            left = 0.0;
                        }
                    }
                    Leg::Via(stop) => {
                        // Held by another: waits where it is.
                        if self.holds.get(&stop).is_some_and(|a| *a != c.object) {
                            self.blocked.insert(c.object, stop);
                            left = 0.0;
                            continue;
                        }
                        self.blocked.remove(&c.object);
                        // Driving uses its battery.
                        if c.props.battery > 0.0 {
                            let e = self.energy.entry(c.object).or_insert(1.0);
                            *e = (*e - left / (c.props.battery * 60.0)).max(0.0);
                        }
                        self.holds.insert(stop, c.object);
                        let Some(&(p, _)) = places.get(&stop) else {
                            r.legs.remove(0);
                            continue;
                        };
                        let to = DVec3::new(p.x, p.y, r.at.z);
                        let need = to.distance(r.at);
                        let can = c.props.speed * left;
                        if need <= can {
                            r.at = to;
                            left -= need / c.props.speed;
                            r.legs.remove(0);
                            // There: what it held on the way let go of.
                            let me = c.object;
                            self.holds.retain(|s, a| *a != me || *s == stop);
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

    /// AMR `amr` where its real one says it is (`seen`, metres and radians
    /// on the floor, none while nothing has been said), the parts on its top
    /// plate with it; `state` is how its feed is getting on, for the page.
    pub fn follow_amr(
        &mut self,
        doc: &Document,
        amr: u64,
        seen: Option<&crate::mqtt::Rover>,
        state: Value,
    ) {
        if self.follows.get(&amr) != Some(&state) {
            self.follows.insert(amr, state);
            self.changed = true;
        }
        let Some(seen) = seen else { return };
        let Some(c) = self.components.iter().find(|c| c.object == amr).cloned() else {
            return;
        };
        let Some(r) = self.rover(doc, &c) else { return };
        let to = DVec3::new(seen.x, seen.y, r.at.z);
        if to.distance(r.at) < 1e-6 && (seen.theta - r.yaw).abs() < 1e-6 {
            return;
        }
        let deck = self.decks(doc).into_iter().find(|d| d.object == amr);
        let shift = pose_of(to, seen.theta) * pose_of(r.at, r.yaw).inverse();
        if let Some(deck) = deck {
            let (_, turn, _) = shift.to_scale_rotation_translation();
            for p in &mut self.parts {
                if p.held.is_none() && deck.carries(p) {
                    let at = shift.transform_point3(p.world.translation);
                    let (_, own, _) = p.world.to_scale_rotation_translation();
                    p.world = DAffine3::from_rotation_translation(turn * own, at);
                }
            }
        }
        self.rovers.insert(
            amr,
            Rover {
                at: to,
                yaw: seen.theta,
                legs: Vec::new(),
                walked: 0.0,
            },
        );
        self.changed = true;
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
        let boxes = self.axis_boxes(doc);
        for belt in self.belts(doc) {
            let gap = 2.0 * RAW.0 + 0.005;
            let end = belt.length / 2.0 - RAW.0;
            // An axis standing up across the belt, where parts ride — a
            // stopper raised — and how far along the belt it begins.
            let stops: Vec<f64> = boxes
                .iter()
                .filter_map(|&(_, lo, hi)| {
                    let corners = (0..8).map(|k| {
                        DVec3::new(
                            if k & 1 == 0 { lo.x } else { hi.x },
                            if k & 2 == 0 { lo.y } else { hi.y },
                            if k & 4 == 0 { lo.z } else { hi.z },
                        ) - belt.centre
                    });
                    let (mut a0, mut a1, mut c0, mut c1) = (
                        f64::INFINITY,
                        f64::NEG_INFINITY,
                        f64::INFINITY,
                        f64::NEG_INFINITY,
                    );
                    for d in corners {
                        a0 = a0.min(d.dot(belt.along));
                        a1 = a1.max(d.dot(belt.along));
                        c0 = c0.min(d.dot(belt.across));
                        c1 = c1.max(d.dot(belt.across));
                    }
                    let across = c1 >= -belt.width / 2.0 && c0 <= belt.width / 2.0;
                    let up = hi.z > belt.centre.z + 0.002 && lo.z < belt.centre.z + RAW.1;
                    let on = a1 >= -belt.length / 2.0 && a0 <= belt.length / 2.0;
                    (across && up && on).then_some(a0)
                })
                .collect();
            // Front first: each stops behind the one ahead.
            let mut on: Vec<(usize, f64)> = self
                .parts
                .iter()
                .enumerate()
                .filter(|(_, p)| {
                    p.held.is_none() && matches!(p.flow, None | Some(flow::Place::Belt(_)))
                })
                .filter_map(|(i, p)| belt.along_of(p).map(|x| (i, x)))
                .collect();
            on.sort_by(|a, b| b.1.total_cmp(&a.1));
            let mut limit = end;
            let mut gone = Vec::new();
            for (i, x) in on {
                // Held behind a stop ahead of it.
                let held = stops
                    .iter()
                    .filter(|a0| **a0 >= x + RAW.0 - 0.002)
                    .map(|a0| a0 - RAW.0)
                    .fold(limit, f64::min);
                let to = (x + belt.props.speed * dt).min(held).max(x);
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
            .chain(self.flow_activities())
            .chain(self.operator_activities())
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
                        let charging = !driving && self.charging.contains(&c.object);
                        (
                            c.object,
                            c.name.clone(),
                            if driving {
                                busy
                            } else if charging {
                                "charge"
                            } else {
                                "idle"
                            },
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
            .filter(|c| matches!(c.kind, Kind::Amr | Kind::Positioner | Kind::Operator))
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
            if c.kind == Kind::Operator {
                self.pose_person(doc, id, c.object, r);
            }
            uids.extend(
                doc.objects()
                    .filter(|(o, _)| *o == id || is_below(doc, *o, id))
                    .filter_map(|(o, _)| doc.object_uid(o).map(|u| u.raw())),
            );
        }
        uids
    }

    /// The robots that tend a station while it works — a glass transfer
    /// robot taking each sheet from its cassette into the tool and back —
    /// moving as they would, in `doc`, the scene as shown: the base turning
    /// between the cassette and the tool, the arm reaching and drawing back,
    /// a sheet every few seconds. Only a robot standing within `ARM_REACH`
    /// of a station at work. Returns the objects moved.
    fn move_arms(&self, doc: &mut Document) -> Vec<u64> {
        const ARM_REACH: f64 = 6.0;
        // One sheet in and one out every six seconds of the clock.
        const SHEET: f64 = 6.0;
        let working: Vec<DVec3> = self
            .components
            .iter()
            .filter(|c| c.kind == Kind::Station)
            .filter(|c| {
                self.stations
                    .get(&c.object)
                    .is_some_and(|s| s.working.is_some())
            })
            .filter_map(|c| {
                self.docks_seen
                    .as_ref()
                    .and_then(|d| d.docks.get(&c.object))
                    .map(|d| d.0)
            })
            .collect();
        if working.is_empty() {
            return Vec::new();
        }
        let roots: Vec<(u64, DAffine3)> = doc
            .objects()
            .filter(|(id, o)| {
                matches!(o.robot, Some(kreate_scene::robot::RobotPart::Link { .. }))
                    && doc.robot_root(*id) == Some(*id)
            })
            .filter_map(|(id, _)| Some((doc.object_uid(id)?.raw(), doc.world_transform(id))))
            .collect();
        let now = self.last_tick.unwrap_or(0.0);
        let mut moved = Vec::new();
        for (k, (root, place)) in roots.into_iter().enumerate() {
            let at = place.translation;
            let Some(port) = working
                .iter()
                .filter(|w| w.truncate().distance(at.truncate()) < ARM_REACH)
                .min_by(|a, b| {
                    a.truncate()
                        .distance(at.truncate())
                        .total_cmp(&b.truncate().distance(at.truncate()))
                })
            else {
                continue;
            };
            let Some(kin) = kynex_kin::Kinematics::of(doc, root) else {
                continue;
            };
            // A glass robot (a lifting column, its second joint sliding):
            // it turns to the cassette on the load port and reaches in
            // with its fork as far as the port is, draws back, turns to
            // the tool — the way it was placed facing — and reaches in.
            if kin.vars.len() >= 5
                && matches!(kin.vars[1].kind, kreate_scene::robot::JointKind::Prismatic)
            {
                use std::f64::consts::{PI, TAU};
                // Its arms, 1.2 and 1.0 m: folded by `f`, the wrist
                // 2.2 cos(f/2) out; the fork's middle 1.2 m on from it, its
                // tips 2.1 m, the sheet's edge 2.3 m. Folded all the way it
                // turns within 2.6 m, clear of the tools.
                let fold_for = |wrist: f64| 2.0 * (wrist / 2.2).clamp(-1.0, 1.0).acos();
                const FOLDED: f64 = 2.9;
                let local = place.inverse().transform_point3(*port);
                let cassette = local.y.atan2(local.x);
                // The tool straight ahead, reached by the shorter way round:
                // its front 3.4 m away, the sheet set at its face.
                let tool = cassette + (-cassette + PI).rem_euclid(TAU) - PI;
                let u = (now / SHEET + k as f64 * 0.37).fract();
                let v = (u * 2.0).fract();
                let (from, to, fold) = if u < 0.5 {
                    // The wrist and its joint (0.2 m round) kept short of the
                    // cassette's edge, 1.35 m from its middle: only the hand
                    // and the fork go over it.
                    (tool, cassette, fold_for(local.truncate().length() - 1.6))
                } else {
                    (cassette, tool, fold_for(3.2 - 2.3))
                };
                let ease = |x: f64| (1.0 - (PI * x.clamp(0.0, 1.0)).cos()) / 2.0;
                let turn = from + (to - from) * ease(v / 0.25);
                let reach = if v < 0.25 {
                    0.0
                } else {
                    (PI * (v - 0.25) / 0.75).sin()
                };
                // A sheet taken from the cassette at the far end of the
                // reach, and given into the tool at the far end of the next.
                let carrying = (u < 0.5) == (v >= 0.625);
                let f = FOLDED + (fold - FOLDED) * reach;
                let mut q = kin.current.clone();
                q[0] = (turn + PI).rem_euclid(TAU) - PI;
                // The hand and fork just clear of a cassette's top on a port
                // 0.7 m up (1.8 m).
                q[1] = 0.12 + 0.03 * (TAU * v).sin();
                q[2] = -f / 2.0;
                q[3] = f;
                q[4] = -f / 2.0;
                for (v, var) in q.iter_mut().zip(&kin.vars).skip(1) {
                    *v = v.clamp(var.lower, var.upper);
                }
                let sheet = doc
                    .objects()
                    .find(|(id, o)| {
                        o.name == "glass"
                            && doc
                                .robot_root(*id)
                                .is_some_and(|r| doc.object_uid(r).is_some_and(|u| u.raw() == root))
                    })
                    .map(|(id, _)| id);
                if let Some(id) = sheet {
                    let scale = DVec3::splat(if carrying { 1.0 } else { NO_GLASS });
                    let o = doc.object_mut(id).unwrap();
                    if o.transform.scale != scale {
                        o.transform.scale = scale;
                        if let Some(uid) = doc.object_uid(id) {
                            moved.push(uid.raw());
                        }
                    }
                }
                moved.extend(kin.apply(doc, &q).into_iter().map(|u| u.raw()));
                continue;
            }
            let phase = std::f64::consts::TAU * now / SHEET + k as f64;
            let mut q = kin.current.clone();
            for (j, amp) in [0.9, 0.25, -0.35, 0.0, 0.3].into_iter().enumerate() {
                if let Some(v) = q.get_mut(j) {
                    *v += amp * phase.sin() * if j == 0 { 1.0 } else { (2.0 * phase).cos() };
                }
            }
            moved.extend(kin.apply(doc, &q).into_iter().map(|u| u.raw()));
        }
        moved
    }

    /// Person `who`'s limbs as they move, in `doc`, the scene as shown
    /// (made anew each time, so from the pose the person was built in):
    /// walking, the legs swing from the hips, the leg behind bent at the
    /// knee, and the arms swing against them — not while carrying a part,
    /// held before them; at work at a station, the hands go to and fro.
    /// The limbs are found by name, as the library makes a person
    /// (`library::interior::person`); one made otherwise is left as it is.
    fn pose_person(&self, doc: &mut Document, root: ObjectId, who: u64, r: &Rover) {
        // One stride (two steps) every 1.4 m: a walk at 1.2 m/s.
        const STRIDE: f64 = 1.4;
        let walking = r.legs.iter().any(|l| matches!(l, Leg::Walk(_)));
        let carrying = self
            .parts
            .iter()
            .any(|p| matches!(p.flow, Some(flow::Place::Riding { amr, .. }) if amr == who));
        let working = self
            .stations
            .values()
            .any(|s| s.crew == Some(who) && s.working.is_some());
        if !walking && !working {
            return;
        }
        let phase = r.walked / STRIDE * std::f64::consts::TAU;
        let swing = if walking { 0.45 * phase.sin() } else { 0.0 };
        let now = self.last_tick.unwrap_or(0.0);
        // About an axis along Y through `pivot`, in the person's own frame.
        let turn = |doc: &mut Document, name: &'static str, pivot: DVec3, angle: f64| {
            if angle.abs() < 1e-6 {
                return;
            }
            let Some(id) = part_named(doc, root, name) else {
                return;
            };
            let about = DAffine3::from_translation(pivot)
                * DAffine3::from_rotation_y(angle)
                * DAffine3::from_translation(-pivot);
            let t = &mut doc.object_mut(id).unwrap().transform;
            let (scale, rotation, translation) =
                (about * t.to_affine()).to_scale_rotation_translation();
            t.translation = translation;
            t.rotation = rotation;
            t.scale = scale;
        };
        type Limbs = [&'static str; 4];
        let sides: [(f64, f64, Limbs, Limbs); 2] = [
            (
                -1.0,
                1.0,
                ["thigh_right", "knee_right", "shin_right", "shoe_right"],
                [
                    "upper_arm_right",
                    "elbow_right",
                    "forearm_right",
                    "hand_right",
                ],
            ),
            (
                1.0,
                -1.0,
                ["thigh_left", "knee_left", "shin_left", "shoe_left"],
                ["upper_arm_left", "elbow_left", "forearm_left", "hand_left"],
            ),
        ];
        for (y, sign, leg, arm) in sides {
            // A positive turn about Y takes a hanging limb backwards.
            let back = sign * swing;
            // The knee bends only as the leg goes back, more as it goes.
            let knee = 0.9 * (back / 0.45).max(0.0) * 0.6;
            for name in &leg[2..] {
                turn(doc, name, DVec3::new(0.0, 0.1 * y, 0.51), knee);
            }
            for name in leg {
                turn(doc, name, DVec3::new(0.0, 0.1 * y, 0.94), back);
            }
            if walking && !carrying {
                for name in arm {
                    turn(doc, name, DVec3::new(0.0, 0.215 * y, 1.43), -0.7 * back);
                }
            }
            if working && !walking {
                // Hands at work, in turn, about the elbow.
                let to_and_fro = 0.25 * (now * 7.0 + if y < 0.0 { 0.0 } else { 2.0 }).sin();
                for name in &arm[2..] {
                    turn(doc, name, DVec3::new(0.0, 0.215 * y, 1.13), to_and_fro);
                }
            }
        }
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
        // An axis moves as a door of its own does.
        for c in self.components.iter().filter(|c| c.kind == Kind::Actuator) {
            let Some(id) = id_of(doc, c.object) else {
                continue;
            };
            let out_by = self.machines.get(&c.object).map_or(0.0, |m| m.door);
            out.push((c.object, out_by, id, Some(c.props)));
        }
        out
    }

    /// How each door of machine `machine` moves, shut as `doc` has it: for
    /// the page, to drag it along its way (`door_to`).
    fn door_motions(&self, doc: &Document, machine: ObjectId) -> Vec<Value> {
        self.door_parts(doc, machine)
            .into_iter()
            .filter_map(|(door, props)| {
                let uid = doc.object_uid(door)?.raw().to_string();
                Some(match door_motion(doc, door, machine, props.as_ref())? {
                    DoorMotion::Slide(by) => json!({ "uid": uid, "slide": by.to_array() }),
                    DoorMotion::Swing { pivot, axis, angle } => json!({
                        "uid": uid,
                        "pivot": pivot.to_array(),
                        "axis": axis.to_array(),
                        "angle": angle,
                    }),
                })
            })
            .collect()
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
            let moved = match door_motion(doc, door, machine, props.as_ref()) {
                Some(DoorMotion::Swing { pivot, axis, angle }) => {
                    DAffine3::from_translation(pivot)
                        * DAffine3::from_axis_angle(axis, angle * open)
                        * DAffine3::from_translation(-pivot)
                        * world
                }
                Some(DoorMotion::Slide(by)) => DAffine3::from_translation(by * open) * world,
                None => continue,
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
            .chain(self.move_arms(doc))
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
        let kinds: std::collections::BTreeSet<(bool, u8)> =
            self.parts.iter().map(|p| (p.finished, p.product)).collect();
        for (finished, product) in kinds {
            // Product A as parts always were; the others each a colour.
            let k = if product == 0 {
                1 + u64::from(finished)
            } else {
                10 + 2 * u64::from(product) + u64::from(finished)
            };
            let (r, h) = if finished { FINISHED } else { RAW };
            let colour = match (product, finished) {
                (0, true) => SceneRgb::new(0.95, 0.72, 0.25),
                (0, false) => SceneRgb::new(0.62, 0.64, 0.68),
                (p, f) => {
                    let [r, g, b] =
                        flow::PRODUCT_COLOURS[(p as usize) % flow::PRODUCT_COLOURS.len()];
                    let light = if f { 1.0 } else { 0.6 };
                    SceneRgb::new(
                        r * light + (1.0 - light) * 0.7,
                        g * light + (1.0 - light) * 0.7,
                        b * light + (1.0 - light) * 0.7,
                    )
                }
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
            meshes.insert((finished, product), mesh);
        }
        // Parts of a look — car bodies, displays — each look's meshes, then
        // each part's pieces under it.
        let mut car_meshes: BTreeMap<usize, Vec<(kreate_scene::MeshId, kreate_scene::MaterialId)>> =
            BTreeMap::new();
        for part in self.parts.iter().filter(|p| p.look > 0) {
            let key = look_key(part);
            if car_meshes.contains_key(&key) {
                continue;
            }
            let made = look_meshes()[key]
                .iter()
                .enumerate()
                .map(|(g, (rgb, mesh))| {
                    let uid = Uid::from_raw(PARTS + 1000 + (key as u64) * LOOK_PIECES + g as u64);
                    let material = doc.restore_material(
                        Material {
                            name: "car body".into(),
                            base_color: SceneRgb::new(rgb[0], rgb[1], rgb[2]),
                            ..Material::default()
                        },
                        uid,
                    );
                    let mesh = doc.restore_mesh(mesh.clone(), uid);
                    doc.set_mesh_slot(mesh, 0, Some(material));
                    (mesh, material)
                })
                .collect();
            car_meshes.insert(key, made);
        }
        for part in &self.parts {
            if part.look > 0 {
                // On the floor under its centre, as a body stands.
                let (_, rotation, translation) = part.world.to_scale_rotation_translation();
                let mut root = Object::new(LOOKS[part.look as usize], ObjectData::Empty);
                root.transform.translation = translation - DVec3::Z * part.size().1 / 2.0;
                root.transform.rotation = rotation;
                let root = doc.restore_object(root, Uid::from_raw(part.uid));
                changes
                    .changed
                    .insert((DatablockKind::Object, Uid::from_raw(part.uid)));
                let key = look_key(part);
                let pieces = &car_meshes[&key];
                // The crane's pieces, where it is on its rail: under the
                // cassette as it goes along, beside it as it is put across.
                let crane = match part.rail {
                    // A hoist on its rail, over the FOUP it lifts: as high
                    // as the FOUP's top as it rides.
                    Some([a, ..]) if key == HOIST_LOOK => {
                        let floor = translation - DVec3::Z * part.size().1 / 2.0;
                        let on =
                            DVec3::new(floor.x, floor.y, a.z - part.size().1 / 2.0 + FOUP_HIGH);
                        Some((
                            pieces.len() - hoist_look().len(),
                            rotation.inverse() * (on - floor),
                        ))
                    }
                    Some([a, b, along]) if key == CRANE_LOOK => {
                        let floor = translation - DVec3::Z * part.size().1 / 2.0;
                        let (lo, hi) = (
                            a.dot(along).min(b.dot(along)),
                            a.dot(along).max(b.dot(along)),
                        );
                        let s = floor.dot(along).clamp(lo, hi);
                        let on = a + along * (s - a.dot(along));
                        let on = DVec3::new(on.x, on.y, 0.0);
                        Some((
                            pieces.len() - crane_look().len(),
                            rotation.inverse() * (on - floor),
                        ))
                    }
                    _ => None,
                };
                for k in 0..LOOK_PIECES {
                    let data = pieces
                        .get(k as usize)
                        .map_or(ObjectData::Empty, |(mesh, _)| ObjectData::Mesh(*mesh));
                    let mut piece = Object::new("piece", data);
                    piece.parent = Some(root);
                    if let Some((first, offset)) = crane {
                        if k as usize >= first {
                            piece.transform.translation = offset;
                        }
                    }
                    let uid = Uid::from_raw(look_piece(part.uid, k));
                    doc.restore_object(piece, uid);
                    changes.changed.insert((DatablockKind::Object, uid));
                }
                continue;
            }
            let mut object = Object::new(
                if part.finished { "완성품" } else { "소재" },
                ObjectData::Mesh(meshes[&(part.finished, part.product)]),
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
        for uid in std::mem::take(&mut self.removed) {
            changes.removed.insert(Uid::from_raw(uid));
            // The pieces of the car body it may have been drawn as, with it,
            // whether or not it was: what this cell drew is not all it knows
            // (a scenario taken back, an edit undone, starts it afresh), and
            // a piece the viewers do not have is let go of as nothing.
            if (PARTS..PARTS + (1 << 40)).contains(&uid) {
                changes
                    .removed
                    .extend((0..LOOK_PIECES).map(|k| Uid::from_raw(look_piece(uid, k))));
            }
        }
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
                    "mirror": c.mirror.as_ref().map(crate::mtconnect::Source::to_json),
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
                    "flow": c.kind.flow().then(|| self.flow_json(doc, c)),
                    "skills": (c.kind == Kind::Operator).then(|| {
                        self.skills.get(&c.object).map(|n| n.iter().map(u64::to_string).collect::<Vec<_>>()).unwrap_or_default()
                    }),
                    "lanes": (c.kind == Kind::Stop).then(|| {
                        self.lanes.get(&c.object).map(|n| n.iter().map(u64::to_string).collect::<Vec<_>>()).unwrap_or_default()
                    }),
                    "stop_at": (c.kind == Kind::Stop)
                        .then(|| id_of(doc, c.object))
                        .flatten()
                        .map(|id| point(doc.world_transform(id).translation)),
                    "sensor": (c.kind == Kind::Sensor).then(|| {
                        let beam = Self::beam(doc, c);
                        json!({
                            "from": beam.map(|(f, ..)| point(f)),
                            "to": beam.map(|(f, w, l)| point(f + w * l)),
                            "detected": self.detects(doc, c),
                        })
                    }),
                    "actuator": (c.kind == Kind::Actuator).then(|| {
                        let m = self.machines.get(&c.object).cloned().unwrap_or_default();
                        json!({ "out": m.door, "extending": m.opening })
                    }),
                    "amr": (c.kind == Kind::Amr).then(|| {
                        let r = self.rover(doc, c);
                        json!({
                            "driving": r.as_ref().is_some_and(|r| !r.legs.is_empty()),
                            "at": r.as_ref().map(|r| [r.at.x, r.at.y]),
                            "yaw": r.as_ref().map(|r| r.yaw),
                            "battery": (c.props.battery > 0.0).then(|| self.energy.get(&c.object).copied().unwrap_or(1.0)),
                            "charging": self.charging.contains(&c.object),
                            "follow": self.follows.get(&c.object),
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
                            "doors": id_of(doc, c.object).map(|id| self.door_motions(doc, id)),
                            "opening": m.opening,
                            "clamped": m.clamped,
                            "cycles": m.cycles,
                            "mirror": m.mirror_state,
                            "chuck": m.chuck,
                            "progress": m.machining.map(|t| (t / c.props.cycle.max(1e-9)).min(1.0)),
                            "left": m.machining.map(|t| (c.props.cycle - t).max(0.0)),
                            "part": part,
                        })
                    }),
                })
            })
            .collect();
        // What a component has not, left out: a long flow's state is
        // shorter by half.
        let components: Vec<Value> = components
            .into_iter()
            .map(|mut v| {
                if let Some(o) = v.as_object_mut() {
                    o.retain(|_, x| !x.is_null());
                }
                v
            })
            .collect();
        json!({
            "flow": self.flow_summary(),
            "components": components,
            "signals": self.signals.iter().map(|s| {
                let mut v = s.to_json();
                v["value"] = json!(self.signal_values.get(&s.name).copied().unwrap_or(0));
                v
            }).collect::<Vec<_>>(),
            // The cell's own parts; a flow's are drawn in the scene.
            "parts": self.parts.iter().filter(|p| p.flow.is_none()).map(|p| json!({
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
/// How a door moves from shut to open, in the world: along a way, or round
/// a hinge by an angle.
enum DoorMotion {
    Slide(DVec3),
    Swing {
        pivot: DVec3,
        axis: DVec3,
        angle: f64,
    },
}

/// How door part `door` of machine `machine` moves, shut as `doc` has it:
/// as its door role says, or else aside along its machine's X by its own
/// width.
fn door_motion(
    doc: &Document,
    door: ObjectId,
    machine: ObjectId,
    props: Option<&Props>,
) -> Option<DoorMotion> {
    let world = doc.world_transform(door);
    let own = |p: &Props| {
        let unit = [DVec3::X, DVec3::Y, DVec3::Z][p.axis.min(2) as usize];
        (world.matrix3 * unit).normalize_or_zero()
    };
    Some(match props {
        Some(p) if p.swing > 0.5 => DoorMotion::Swing {
            pivot: world.transform_point3(DVec3::from_array(p.point)),
            axis: own(p),
            angle: p.angle,
        },
        Some(p) if p.travel != 0.0 => DoorMotion::Slide(own(p) * p.travel),
        _ => {
            let along = doc
                .world_transform(machine)
                .matrix3
                .x_axis
                .normalize_or_zero();
            let (lo, hi) = bounds(doc, door)?;
            let width = (hi - lo).dot(along).abs();
            DoorMotion::Slide(along * (width + 0.02))
        }
    })
}

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
            | "door_to"
            | "drive"
            | "turn"
            | "io"
            | "point"
            | "plc"
            | "signals"
            | "mirror"
            | "flow"
            | "flow_next"
            | "flow_recipe"
            | "amr_lanes"
            | "flow_products"
            | "flow_orders"
            | "operator_skills"
            | "flow_settings"
    )
}

mod fleet;
mod flow;
#[cfg(test)]
pub use flow::SEED;

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

    /// A machine's part — its chuck — is not made a machine of its own; a
    /// part of nothing that is one, or another role on the part, still is.
    #[test]
    fn a_machine_s_part_is_not_a_machine_of_its_own() {
        let (doc, plate, machine, _) = scene();
        let chuck = doc
            .objects()
            .find(|(_, o)| o.name == "cnc_1_chuck")
            .and_then(|(id, _)| doc.object_uid(id))
            .unwrap()
            .raw();
        let mut cell = Cell::default();
        let role = |object: u64, kind: &str| json!({"op": "role", "object": object.to_string(), "kind": kind});
        assert_eq!(ask(&mut cell, &doc, role(machine, "cnc"))["ok"], true);
        let refused = ask(&mut cell, &doc, role(chuck, "cnc"));
        assert_eq!(refused["ok"], false, "{refused}");
        assert!(
            refused["error"].as_str().unwrap().contains("cnc_1"),
            "{refused}"
        );
        assert_eq!(cell.components().len(), 1);
        assert_eq!(ask(&mut cell, &doc, role(plate, "cnc"))["ok"], true);
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

    /// A door dragged part open stays as left, its way told to the page; a
    /// cycle will not start with it so; told to open, it goes on.
    #[test]
    fn a_door_is_dragged_open_and_stays_as_left() {
        let (mut doc, _, machine, _) = scene();
        let door = doc.add_mesh_object("cnc_1_door", kreate_mesh::primitives::cube(1.0));
        {
            let o = doc.object_mut(door).unwrap();
            o.transform.scale = DVec3::new(0.6, 0.03, 0.7);
            o.transform.translation = DVec3::new(0.0, 0.7, 1.2);
        }
        let m = id_of(&doc, machine).unwrap();
        doc.set_parent(door, Some(m)).unwrap();
        let mut cell = Cell::default();
        ask(
            &mut cell,
            &doc,
            json!({"op": "role", "object": machine.to_string(), "kind": "cnc"}),
        );
        let t0 = Instant::now();
        let at = |s: f64| t0 + Duration::from_secs_f64(s);
        cell.tick(&doc, 0.0, at(0.0));

        let a = ask(
            &mut cell,
            &doc,
            json!({"op": "door_to", "object": machine.to_string(), "open": 0.4}),
        );
        assert_eq!(a["ok"], true, "{a}");
        cell.tick(&doc, 2.0, at(2.0));
        assert_eq!(cell.machines[&machine].door, 0.4, "held where it was left");
        assert!(!cell.is_running());
        // Its way, for the page: aside along the machine's X by its width.
        let json: Value = serde_json::from_str(&cell.to_json(&doc)).unwrap();
        let c = json["components"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["kind"] == "cnc")
            .unwrap();
        let way = &c["machine"]["doors"][0]["slide"];
        assert!((way[0].as_f64().unwrap() - 0.62).abs() < 1e-9, "{way}");
        let start = ask(
            &mut cell,
            &doc,
            json!({"op": "machine", "object": machine.to_string(), "action": "start"}),
        );
        assert_eq!(start["ok"], false, "not part open");

        ask(
            &mut cell,
            &doc,
            json!({"op": "machine", "object": machine.to_string(), "action": "open"}),
        );
        cell.tick(&doc, 2.9, at(2.9));
        assert!(cell.machines[&machine].door > 0.4, "going on");
        cell.tick(&doc, 9.0, at(9.0));
        assert_eq!(cell.machines[&machine].door, 1.0);
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
        assert_eq!(parts_in(&changes.removed), 20);
    }

    /// How many of `removed` are parts, not the pieces of a car body.
    fn parts_in(removed: &std::collections::BTreeSet<Uid>) -> usize {
        removed
            .iter()
            .filter(|u| u.raw() >= PARTS && u.raw() < PARTS + (1 << 40))
            .count()
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
        assert_eq!(parts_in(&cell.take_removed().removed), count);
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

    /// A lathe following a real one: its door as the real one's, cutting
    /// while it is ACTIVE (its own cycle not run), its cycles the parts made
    /// since first read (a count reset starting again); no commands taken;
    /// following kept in the project file, and let go.
    #[test]
    fn a_machine_follows_a_real_one() {
        use crate::mtconnect::Observation;
        let (doc, mut cell, machine) = equipment("cnc_lathe", json!({}));
        let a = ask(
            &mut cell,
            &doc,
            json!({"op": "mirror", "object": machine.to_string(),
                   "agent": "http://10.0.0.5:5000", "device": "Mazak"}),
        );
        assert_eq!(a["ok"], true, "{a}");
        assert_eq!(cell.mirror_sources().len(), 1);
        let seen = |door: &str, execution: &str, parts: i64| Observation {
            device: "Mazak".into(),
            door: Some(door.into()),
            execution: Some(execution.into()),
            part_count: Some(parts),
            ..Observation::default()
        };
        cell.mirror(machine, Some(&seen("OPEN", "READY", 41)), 0.0, json!({}));
        assert!(cell.signal(&doc, machine, Signal::DoorOpen));
        cell.mirror(machine, Some(&seen("CLOSED", "ACTIVE", 41)), 1.0, json!({}));
        assert!(cell.signal(&doc, machine, Signal::DoorClosed));
        assert!(!cell.signal(&doc, machine, Signal::Idle), "cutting");
        // Its own 90 s cycle not run: it cuts as long as the real one does.
        let t0 = Instant::now();
        cell.tick(&doc, 200.0, t0);
        cell.tick(&doc, 400.0, t0 + Duration::from_secs(1));
        assert!(!cell.signal(&doc, machine, Signal::Idle));
        cell.mirror(machine, Some(&seen("CLOSED", "READY", 43)), 2.0, json!({}));
        assert!(cell.signal(&doc, machine, Signal::Idle));
        assert_eq!(cell.machines[&machine].cycles, 2);
        cell.mirror(machine, Some(&seen("CLOSED", "READY", 0)), 3.0, json!({}));
        cell.mirror(machine, Some(&seen("CLOSED", "READY", 1)), 4.0, json!({}));
        assert_eq!(
            cell.machines[&machine].cycles, 1,
            "counted again after a reset"
        );
        // Its agent silent: as it was last read.
        cell.mirror(machine, None, 5.0, json!({"error": "timed out"}));
        assert!(cell.signal(&doc, machine, Signal::Idle));
        assert!(cell.command(&doc, machine, Command::Open).is_err());

        let mut again = Cell::from_saved(cell.to_saved());
        again.adopt(&doc);
        assert_eq!(again.mirror_sources(), cell.mirror_sources());
        let a = ask(
            &mut cell,
            &doc,
            json!({"op": "mirror", "object": machine.to_string(), "agent": ""}),
        );
        assert_eq!(a["ok"], true);
        assert!(cell.mirror_sources().is_empty());
        assert!(cell.command(&doc, machine, Command::Open).is_ok());
        let a = ask(
            &mut cell,
            &doc,
            json!({"op": "mirror", "object": machine.to_string(), "agent": "https://x"}),
        );
        assert_eq!(a["ok"], false, "only http");
    }
}

#[cfg(test)]
mod axis_tests {
    use super::*;

    fn ask(cell: &mut Cell, doc: &Document, req: Value) -> Value {
        serde_json::from_str(&cell.handle(doc, &req.to_string())).unwrap()
    }

    fn run_for(cell: &mut Cell, doc: &Document, from: f64, seconds: f64) -> f64 {
        let start = Instant::now();
        let mut t = from;
        while t < from + seconds {
            t += 0.05;
            cell.tick(doc, t, start + Duration::from_secs_f64(t));
        }
        t
    }

    /// A conveyor 2 m long along X at the origin, its belt's top 0.8 m up,
    /// and the equipment `more` names placed beside it: the scene, and the
    /// cell with each item's role, its settings the template's.
    fn line(more: &[(&str, [f64; 2], f64, Value)]) -> (Document, Cell, Vec<u64>) {
        let mut library = crate::library::Library::default();
        library.handle(
            &json!({"op": "place", "template": "conveyor", "at": [0.0, 0.0, 0.0]}).to_string(),
        );
        for (template, at, yaw, _) in more {
            library.handle(
                &json!({"op": "place", "template": template, "at": [at[0], at[1], 0.0], "yaw": yaw})
                    .to_string(),
            );
        }
        let mut doc = Document::new();
        library.apply(&mut doc);
        let mut cell = Cell::default();
        let roles = library.roles();
        let mut uids = Vec::new();
        for (k, (uid, name, kind)) in roles.into_iter().enumerate() {
            cell.ensure(uid, name, kind);
            let props = if k == 0 {
                json!({"speed": 0.5})
            } else {
                more[k - 1].3.clone()
            };
            let a = ask(
                &mut cell,
                &doc,
                json!({"op": "role", "object": uid.to_string(), "kind": kind.tag(), "props": props}),
            );
            assert_eq!(a["ok"], true, "{a}");
            uids.push(uid);
        }
        (doc, cell, uids)
    }

    #[test]
    fn an_amr_following_its_fleet_s_messages_carries_what_is_on_it() {
        let mut library = crate::library::Library::default();
        library
            .handle(&json!({"op": "place", "template": "amr", "at": [0.0, 0.0, 0.0]}).to_string());
        library.handle(
            &json!({"op": "place", "template": "amr_stop", "at": [2.0, 0.0, 0.0]}).to_string(),
        );
        let mut doc = Document::new();
        library.apply(&mut doc);
        let mut cell = Cell::default();
        let (mut amr, mut stop) = (0, 0);
        for (uid, name, kind) in library.roles() {
            match kind {
                Kind::Amr => amr = uid,
                Kind::Stop => stop = uid,
                _ => {}
            }
            cell.ensure(uid, name, kind);
        }
        // An AMR follows MQTT; a machine's agent is not for it.
        let a = ask(
            &mut cell,
            &doc,
            json!({"op": "mirror", "object": amr.to_string(), "agent": "http://10.0.0.5:5000"}),
        );
        assert_eq!(a["ok"], false, "{a}");
        let a = ask(
            &mut cell,
            &doc,
            json!({"op": "mirror", "object": amr.to_string(), "kind": "mqtt",
                   "broker": "10.0.0.5", "topic": "uagv/v2/Acme/amr-01/state", "password": "pw"}),
        );
        assert_eq!(a["ok"], true, "{a}");
        let saved = cell.to_saved();
        assert!(
            !saved.iter().any(|v| v.to_string().contains("pw")),
            "no password kept"
        );
        let top = cell.decks(&doc)[0].hi.z;
        cell.add_part(DVec3::new(0.2, 0.0, top + FINISHED.1 / 2.0), true);
        assert!(
            cell.drive(&doc, amr, stop).is_err(),
            "not sent while following"
        );
        // Its fleet says: 1 m along X, 2 m along Y, turned a quarter.
        let seen = crate::mqtt::Rover {
            x: 1.0,
            y: 2.0,
            theta: std::f64::consts::FRAC_PI_2,
            driving: Some(false),
            battery: None,
            errors: Vec::new(),
        };
        cell.follow_amr(&doc, amr, Some(&seen), json!({"connected": true}));
        let r = cell.rovers[&amr].clone();
        assert!((r.at.x - 1.0).abs() < 1e-9 && (r.at.y - 2.0).abs() < 1e-9);
        // The part 0.2 m ahead of its middle, ahead still: now along +Y.
        let p = cell.parts()[0].world.translation;
        assert!((p.x - 1.0).abs() < 1e-9 && (p.y - 2.2).abs() < 1e-9, "{p}");
        let v: Value = serde_json::from_str(&cell.to_json(&doc)).unwrap();
        let c = v["components"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["kind"] == "amr")
            .unwrap()
            .clone();
        assert_eq!(c["amr"]["follow"]["connected"], true);
        assert_eq!(c["mirror"]["kind"], "mqtt");
    }

    #[test]
    fn a_stopper_holds_parts_on_the_belt_and_a_sensor_sees_them() {
        // A stopper across the belt 0.3 m past its middle; a sensor's beam
        // across it at 0.0, looking along +Y.
        let (doc, mut cell, uids) = line(&[
            (
                "stopper",
                [0.3, 0.0],
                0.0,
                json!({"axis": 2, "travel": 0.04, "door": 0.3}),
            ),
            (
                "photo_sensor",
                [0.0, -0.4],
                std::f64::consts::FRAC_PI_2,
                json!({"axis": 0, "travel": 0.8}),
            ),
        ]);
        let (stopper, sensor) = (uids[1], uids[2]);
        // A part at the belt's start.
        cell.add_part(DVec3::new(-0.9, 0.0, 0.8 + RAW.1 / 2.0), false);
        assert!(cell.signal(&doc, stopper, Signal::Retracted));
        cell.command(&doc, stopper, Command::Extend).unwrap();
        assert!(cell.command(&doc, stopper, Command::Start).is_err());
        let mut t = run_for(&mut cell, &doc, 0.0, 0.5);
        assert!(cell.signal(&doc, stopper, Signal::Extended));
        // On the way past the sensor.
        let mut seen = false;
        while t < 6.0 {
            t = run_for(&mut cell, &doc, t, 0.05);
            seen |= cell.signal(&doc, sensor, Signal::Detected);
        }
        assert!(seen, "the part crossed the beam");
        assert!(
            !cell.signal(&doc, sensor, Signal::Detected),
            "and is past it"
        );
        // Held against the stopper, its pin 10 mm thick at 0.3.
        let x = cell.parts()[0].world.translation.x;
        assert!((x - (0.3 - 0.01 - RAW.0)).abs() < 0.01, "{x}");
        // Lowered, the part goes on to the end.
        cell.command(&doc, stopper, Command::Retract).unwrap();
        run_for(&mut cell, &doc, t, 4.0);
        let x = cell.parts()[0].world.translation.x;
        assert!((x - (1.0 - RAW.0)).abs() < 1e-6, "{x}");
        assert!(cell.signal(&doc, stopper, Signal::Retracted));
    }

    #[test]
    fn a_lifter_lifts_what_stands_on_it_and_a_turntable_turns_it() {
        let (doc, mut cell, uids) = line(&[
            (
                "lifter",
                [3.0, 0.0],
                0.0,
                json!({"axis": 2, "travel": 0.3, "door": 1.0}),
            ),
            (
                "rotary_axis",
                [5.0, 0.0],
                0.0,
                json!({"swing": 1, "axis": 2, "angle": std::f64::consts::PI, "door": 1.0}),
            ),
        ]);
        let (lifter, table) = (uids[1], uids[2]);
        cell.add_part(DVec3::new(3.1, 0.0, 0.8 + RAW.1 / 2.0), false);
        cell.add_part(DVec3::new(5.2, 0.0, 0.8 + RAW.1 / 2.0), false);
        cell.command(&doc, lifter, Command::Extend).unwrap();
        cell.command(&doc, table, Command::Extend).unwrap();
        run_for(&mut cell, &doc, 0.0, 1.5);
        let lifted = cell.parts()[0].world.translation;
        assert!(
            (lifted - DVec3::new(3.1, 0.0, 1.1 + RAW.1 / 2.0)).length() < 1e-6,
            "{lifted}"
        );
        // Half a turn about the table's middle.
        let turned = cell.parts()[1].world.translation;
        assert!(
            (turned - DVec3::new(4.8, 0.0, 0.8 + RAW.1 / 2.0)).length() < 1e-6,
            "{turned}"
        );
        // Shown where they are: the lifter's table up.
        let mut shown = doc.clone();
        cell.apply(&mut shown);
        let id = id_of(&shown, lifter).unwrap();
        let (_, hi) = bounds(&shown, id).unwrap();
        assert!((hi.z - 1.1).abs() < 1e-6, "{hi}");
    }
}
