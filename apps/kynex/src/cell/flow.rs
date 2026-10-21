//! A process flow, as Visual Components' process modelling has one: parts
//! come in at sources, wait and are worked on at stations, wait their turn
//! in buffers, and leave at sinks — each going on to the next its
//! component names (`Cell::flow_next`), the first of them with room,
//! taking turns, carried there at the sender's speed.
//!
//! A station takes `takes` parts a cycle (more than one: an assembly, the
//! first of them going on as the product) and makes `makes` (more than
//! one: a disassembly); it holds what it made until there is somewhere to
//! send it, and starts nothing meanwhile — blocked. With nothing to take
//! it is starved, idle. A sink counts what reaches it and how long each
//! part took from its source: its lead time.
//!
//! The flow runs on the simulation's clock while it is started; stopped,
//! it holds as it is; reset, its parts are gone.

use kreate_math::DQuat;

use super::*;

/// Where a part of a process flow is.
#[derive(Clone, Debug, PartialEq)]
pub enum Place {
    /// At component `0` of the flow: waiting, worked on, or done.
    At(u64),
    /// On its way to `to`, from `from`, between those times.
    Moving {
        from: DVec3,
        to: u64,
        start: f64,
        end: f64,
    },
    /// At `at`, for an AMR to take it to `to` (see `fleet`).
    Waiting { at: u64, to: u64 },
    /// On AMR `amr`'s top plate, to `to`.
    Riding { amr: u64, to: u64 },
    /// On conveyor `0`'s belt, carried by it, to go on from its end.
    Belt(u64),
}

/// A component of the flow as it stands.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Station {
    /// Parts waiting to be worked on, in turn.
    pub queue: Vec<u64>,
    /// Being worked on, and when it is done.
    pub working: Option<(f64, Vec<u64>)>,
    /// How long the work in hand takes, all told: how far it has got is
    /// what is left of it against this (its end put off while it rests or
    /// is down, as what is left is not).
    pub length: f64,
    /// Done, waiting to go on (a buffer's and a source's parts too).
    pub out: Vec<u64>,
    /// A source's seconds since it last put a part in.
    pub fed: f64,
    /// Cycles done.
    pub made: u64,
    /// The next of its targets to try first.
    pub turn: usize,
    /// The operator it has called, who works at it (see `fleet`).
    pub crew: Option<u64>,
    /// A source's wait for its next part, drawn as each comes.
    pub wait: f64,
    /// Seconds of work left before it next breaks down; broken down until
    /// then.
    pub fail_in: Option<f64>,
    pub down_until: Option<f64>,
    /// Since the flow started: seconds at work, broken down, and the parts
    /// made good and scrapped — its OEE.
    pub busy: f64,
    pub down: f64,
    pub good: u64,
    pub bad: u64,
    /// The product it last worked on, for its change-over; seconds rested.
    pub last: Option<u8>,
    pub rested: f64,
}

/// What the sinks have seen since the flow started.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    pub from: Option<f64>,
    pub scrapped: u64,
    /// Each product's parts out and their lead times' sum.
    pub by_product: [(u64, f64); 6],
    /// Seconds from the start, parts in the flow and parts out so far,
    /// sampled as it runs.
    pub series: Vec<[f64; 3]>,
    pub sampled: f64,
    pub sunk: u64,
    pub lead_sum: f64,
    pub lead_min: Option<f64>,
    pub lead_max: f64,
}

/// Room between parts set out on a component, metres.
const PITCH: f64 = 2.0 * RAW.0 + 0.01;

/// The seed a flow's random numbers start from unless one is given.
pub const SEED: u64 = 1;

/// The products a flow can carry, by number.
pub const PRODUCTS: [&str; 6] = ["A", "B", "C", "D", "E", "F"];

/// Each product's colour after A's (A keeps the parts' own).
pub const PRODUCT_COLOURS: [[f32; 3]; 6] = [
    [0.62, 0.64, 0.68],
    [0.25, 0.55, 0.95],
    [0.25, 0.78, 0.45],
    [0.62, 0.40, 0.90],
    [0.90, 0.35, 0.35],
    [0.20, 0.75, 0.75],
];

/// An order: so many parts of a product, released at a time into the
/// flow, wanted done by its due time (0: whenever); seconds from the
/// flow's start. And how far it has got.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Order {
    pub at: f64,
    pub product: u8,
    pub qty: u32,
    pub due: f64,
    pub released: u32,
    pub done: u32,
    pub finished: Option<f64>,
}

impl Order {
    pub fn to_json(&self) -> Value {
        json!([self.at, self.product, self.qty, self.due])
    }

    pub fn of(v: &Value) -> Option<Self> {
        let a = v.as_array()?;
        Some(Self {
            at: a.first()?.as_f64()?.max(0.0),
            product: (a.get(1)?.as_u64()? as usize).min(PRODUCTS.len() - 1) as u8,
            qty: a.get(2)?.as_u64()?.clamp(1, 100000) as u32,
            due: a.get(3).and_then(Value::as_f64).unwrap_or(0.0).max(0.0),
            ..Self::default()
        })
    }

    /// Orders written as CSV: a line each, `release (minutes), product
    /// (A–F or 1–6), quantity, due (minutes, may be empty)`; a heading
    /// line, if there is one, passed over.
    pub fn parse(text: &str) -> Result<Vec<Order>, String> {
        let mut out = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let cells: Vec<&str> = line.split([',', '\t', ';']).map(str::trim).collect();
            if cells.iter().all(|c| c.is_empty()) {
                continue;
            }
            let at = cells[0].parse::<f64>();
            if at.is_err() && n == 0 {
                continue; // a heading
            }
            let bad = || {
                format!(
                    "주문표 {}번째 줄: 시각(분), 제품, 수량, 납기(분)이어야 합니다",
                    n + 1
                )
            };
            let at = at.map_err(|_| bad())?;
            let product = cells.get(1).ok_or_else(bad)?;
            let product = PRODUCTS
                .iter()
                .position(|p| p.eq_ignore_ascii_case(product))
                .or_else(|| product.parse::<usize>().ok().and_then(|k| k.checked_sub(1)))
                .filter(|k| *k < PRODUCTS.len())
                .ok_or_else(bad)? as u8;
            let qty = cells
                .get(2)
                .and_then(|q| q.parse::<u32>().ok())
                .filter(|q| *q > 0)
                .ok_or_else(bad)?;
            let due = match cells.get(3) {
                Some(d) if !d.is_empty() => d.parse::<f64>().map_err(|_| bad())?,
                _ => 0.0,
            };
            if out.len() >= 10000 {
                return Err("주문은 10000줄까지입니다".into());
            }
            out.push(Order {
                at: at.max(0.0) * 60.0,
                product,
                qty: qty.min(100000),
                due: due.max(0.0) * 60.0,
                ..Order::default()
            });
        }
        out.sort_by(|a, b| a.at.total_cmp(&b.at));
        Ok(out)
    }
}

/// Where the flow sets parts out, found for a scene at `version` with the
/// flow components `objects`.
#[derive(Debug)]
pub struct Docks {
    pub(super) version: u64,
    objects: Vec<u64>,
    pub(super) docks: BTreeMap<u64, (DVec3, DVec3, DVec3)>,
    shelves: BTreeMap<u64, Vec<DVec3>>,
}

/// The flow's calendar and warm-up: work and rest in turn (minutes of
/// each; no rest, no calendar), and the time at the start left out of its
/// figures, seconds.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Settings {
    pub work: f64,
    pub rest: f64,
    pub warmup: f64,
    /// For the cost: hours a year the line runs, years its equipment is
    /// written off over, and what a part brings in above its material
    /// (won); 0, the usual.
    pub hours: f64,
    pub years: f64,
    pub margin: f64,
}

impl Settings {
    pub fn of(v: &Value) -> Self {
        let n = |k: &str| v[k].as_f64().unwrap_or(0.0).clamp(0.0, 1.0e6);
        Self {
            work: n("work"),
            rest: n("rest"),
            warmup: n("warmup"),
            hours: n("hours"),
            years: n("years"),
            margin: n("margin"),
        }
    }

    pub fn to_json(self) -> Value {
        json!({
            "work": self.work,
            "rest": self.rest,
            "warmup": self.warmup,
            "hours": self.hours,
            "years": self.years,
            "margin": self.margin,
        })
    }

    /// Hours a year, years: as set, or 4000 and 5.
    pub fn horizon(self) -> (f64, f64) {
        (
            if self.hours > 0.0 { self.hours } else { 4000.0 },
            if self.years > 0.0 { self.years } else { 5.0 },
        )
    }

    pub fn is_default(self) -> bool {
        self == Self::default()
    }

    /// Whether `t` seconds into the flow falls in a rest.
    pub fn resting(self, t: f64) -> bool {
        if self.rest <= 0.0 || self.work <= 0.0 {
            return false;
        }
        let period = (self.work + self.rest) * 60.0;
        t.rem_euclid(period) >= self.work * 60.0
    }
}

impl Cell {
    /// The next random number of the flow, uniform in (0, 1): SplitMix64,
    /// the same sequence for one seed.
    pub(super) fn uniform(&mut self) -> f64 {
        self.rng = self.rng.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        ((z >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    }

    /// A time about `mean`, as `dist` and `spread` say (see `Props::dist`);
    /// never below zero.
    pub(super) fn draw(&mut self, mean: f64, dist: u32, spread: f64) -> f64 {
        let x = match dist {
            1 => mean + spread * (2.0 * self.uniform() - 1.0),
            2 => mean + spread * (self.uniform() - self.uniform()),
            3 => {
                // Box–Muller.
                let (a, b) = (self.uniform(), self.uniform());
                mean + spread * (-2.0 * a.ln()).sqrt() * (std::f64::consts::TAU * b).cos()
            }
            4 => -mean * self.uniform().ln(),
            _ => mean,
        };
        x.max(0.0)
    }

    /// Starts the flow's random numbers again from `seed`.
    pub fn seed_flow(&mut self, seed: u64) {
        self.rng = seed;
    }
}

impl Cell {
    /// Starts the flow, stops it where it is, or takes its parts away.
    pub fn flow(&mut self, action: &str) -> Result<(), String> {
        let now = self.last_tick.unwrap_or(0.0);
        match action {
            "start" => {
                if !self.components.iter().any(|c| c.kind == Kind::Source) {
                    return Err(
                        "이 프로젝트에는 공정 흐름이 없습니다. 라이브러리 '공정 흐름'에서 \
                                소재 공급·공정 스테이션·배출을 놓고 다음 공정으로 이으세요"
                            .into(),
                    );
                }
                self.flow_on = true;
                self.flow_stats.from.get_or_insert(now);
                self.flow_start.get_or_insert(now);
            }
            "stop" => self.flow_on = false,
            "reset" => {
                self.flow_on = false;
                let gone: Vec<u64> = self
                    .parts
                    .iter()
                    .filter(|p| p.flow.is_some())
                    .map(|p| p.uid)
                    .collect();
                self.parts.retain(|p| p.flow.is_none());
                self.removed.extend(gone);
                self.stations.clear();
                self.jobs.clear();
                self.station_doors.clear();
                self.station_door_wanted.clear();
                self.rng = SEED;
                self.warmed = false;
                for orders in self.orders.values_mut() {
                    for o in orders {
                        o.released = 0;
                        o.done = 0;
                        o.finished = None;
                    }
                }
                self.flow_start = None;
                self.flow_problem = None;
                self.flow_stats = Stats::default();
            }
            other => return Err(format!("unknown flow action {other:?}")),
        }
        self.changed = true;
        Ok(())
    }

    /// Whether the flow runs.
    pub fn flow_running(&self) -> bool {
        self.flow_on
    }

    /// Seconds since the flow first started, rests and all: its calendar's
    /// clock.
    pub(super) fn flow_clock(&self) -> f64 {
        let now = self.last_tick.unwrap_or(0.0);
        self.flow_start.map_or(0.0, |f| (now - f).max(0.0))
    }

    /// Whether the warm-up has ended since this was last asked: the figures
    /// begin again from here.
    pub fn take_warmed(&mut self) -> bool {
        std::mem::take(&mut self.just_warmed)
    }

    /// A product for a part source `object` puts in, drawn by its mix
    /// (A alone if it has none).
    fn product_of(&mut self, object: u64) -> u8 {
        let mix = self.mix.get(&object).cloned().unwrap_or_default();
        let total: f64 = mix.iter().sum();
        if total <= 0.0 {
            return 0;
        }
        let mut x = self.uniform() * total;
        for (k, w) in mix.iter().enumerate() {
            if x < *w {
                return k as u8;
            }
            x -= w;
        }
        (mix.len() - 1) as u8
    }

    /// Station `c`'s mean time for product `product`: its own for it, or
    /// else its one time.
    fn cycle_of(&self, c: &Component, product: u8) -> f64 {
        self.cycles
            .get(&c.object)
            .and_then(|t| t.get(product as usize))
            .copied()
            .filter(|t| *t > 0.0)
            .unwrap_or(c.props.cycle)
    }

    /// Whether the cell has a process flow to run: a source.
    pub fn has_flow(&self) -> bool {
        self.components.iter().any(|c| c.kind == Kind::Source)
    }

    /// Sets where the parts of component `object` go next.
    pub fn set_flow_next(&mut self, object: u64, next: Vec<u64>) -> Result<(), String> {
        let c = self
            .components
            .iter()
            .find(|c| c.object == object && c.kind.flow())
            .ok_or("공정 흐름(공급·공정·버퍼·배출)의 역할이 아닙니다")?;
        if c.kind == Kind::Sink && !next.is_empty() {
            return Err("배출에서는 부품이 나가지 않습니다".into());
        }
        for n in &next {
            let ok = self
                .components
                .iter()
                .any(|t| t.object == *n && t.kind.flow() && t.kind != Kind::Source);
            if !ok || *n == object {
                return Err("다음 공정은 다른 공정·버퍼·배출이어야 합니다".into());
            }
        }
        self.flow_next.insert(object, next);
        self.roles_changed = true;
        self.changed = true;
        Ok(())
    }

    /// Source `object`'s recipe: the stations its parts go through, in
    /// order, sent to each by a stocker (a buffer that dispatches). None
    /// clears it.
    pub fn set_recipe(&mut self, object: u64, steps: Vec<u64>) -> Result<(), String> {
        if !self
            .components
            .iter()
            .any(|c| c.object == object && c.kind == Kind::Source)
        {
            return Err("레시피는 소재 공급에 정합니다".into());
        }
        if steps.iter().any(|n| {
            !self
                .components
                .iter()
                .any(|c| c.object == *n && c.kind == Kind::Station)
        }) {
            return Err("레시피의 단계는 공정 스테이션이어야 합니다".into());
        }
        if steps.is_empty() {
            self.recipes.remove(&object);
        } else {
            self.recipes.insert(object, steps);
        }
        self.roles_changed = true;
        self.changed = true;
        Ok(())
    }

    /// Where part `uid` goes from stocker `c` by its recipe: the station of
    /// its next step, if `c` dispatches and the recipe is not done.
    fn recipe_step(&self, c: &Component, uid: u64) -> Option<u64> {
        if c.kind != Kind::Buffer || c.props.dispatch == 0 {
            return None;
        }
        let p = &self.parts[self.part_index(uid)?];
        self.recipes.get(&p.origin)?.get(p.step as usize).copied()
    }

    /// Where on component `c` parts are set out: the middle of the top of
    /// it and all below it, and its own X and Y along the floor.
    pub(super) fn dock(doc: &Document, c: &Component) -> Option<(DVec3, DVec3, DVec3)> {
        let id = id_of(doc, c.object)?;
        let world = doc.world_transform(id);
        let along = world.matrix3.x_axis.normalize_or_zero();
        let across = world.matrix3.y_axis.normalize_or_zero();
        let all = doc
            .objects()
            .filter(|(o, _)| *o == id || is_below(doc, *o, id))
            .filter_map(|(o, _)| bounds(doc, o))
            .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)));
        let top = match all {
            Some((lo, hi)) => DVec3::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0, hi.z),
            None => world.translation,
        };
        // A machine a part goes into — a lathe's chuck, a press's mould —:
        // the part there, not on its roof.
        let held = ["chuck", "mould"]
            .into_iter()
            .find_map(|w| part_named(doc, id, w))
            .and_then(|h| bounds(doc, h))
            .map(|(lo, hi)| DVec3::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0, hi.z));
        Some((held.unwrap_or(top), along, across))
    }

    /// Each component's dock and each rack's shelf tops, for the scene at
    /// its version: the objects below each flow component found in one
    /// pass over the scene, not one for each.
    fn docks_of(
        &mut self,
        doc: &Document,
        flow: &[Component],
    ) -> (
        BTreeMap<u64, (DVec3, DVec3, DVec3)>,
        BTreeMap<u64, Vec<DVec3>>,
    ) {
        let key: Vec<u64> = flow.iter().map(|c| c.object).collect();
        if let Some(seen) = &self.docks_seen {
            if seen.version == doc.version() && seen.objects == key {
                return (seen.docks.clone(), seen.shelves.clone());
            }
        }
        // Each flow component by its scene object.
        let ids: BTreeMap<ObjectId, (u64, bool)> = flow
            .iter()
            .filter_map(|c| Some((id_of(doc, c.object)?, (c.object, c.kind == Kind::Buffer))))
            .collect();
        let mut all: BTreeMap<u64, (DVec3, DVec3)> = BTreeMap::new();
        let mut shelves: BTreeMap<u64, Vec<DVec3>> = BTreeMap::new();
        for (o, x) in doc.objects() {
            if ids.is_empty() {
                break;
            }
            let shelf = x.name.starts_with("shelf");
            let mut b = None;
            // It and each object above it that is a flow component's.
            let mut at = Some(o);
            for depth in 0..64 {
                let Some(a) = at else { break };
                if let Some(&(object, buffer)) = ids.get(&a) {
                    let Some(bb) = b.get_or_insert_with(|| bounds(doc, o)) else {
                        break;
                    };
                    all.entry(object)
                        .and_modify(|e| *e = (e.0.min(bb.0), e.1.max(bb.1)))
                        .or_insert(*bb);
                    if shelf && buffer && depth > 0 {
                        let (lo, hi) = *bb;
                        shelves.entry(object).or_default().push(DVec3::new(
                            (lo.x + hi.x) / 2.0,
                            (lo.y + hi.y) / 2.0,
                            hi.z,
                        ));
                    }
                }
                at = doc.object(a).and_then(|x| x.parent);
            }
        }
        for tops in shelves.values_mut() {
            tops.sort_by(|a, b| a.z.total_cmp(&b.z));
        }
        let docks: BTreeMap<u64, (DVec3, DVec3, DVec3)> = ids
            .iter()
            .map(|(id, (object, _))| {
                let world = doc.world_transform(*id);
                let along = world.matrix3.x_axis.normalize_or_zero();
                let across = world.matrix3.y_axis.normalize_or_zero();
                let top = match all.get(object) {
                    Some((lo, hi)) => DVec3::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0, hi.z),
                    None => world.translation,
                };
                // Into a machine's chuck or mould, as `dock` has it.
                let held = ["chuck", "mould"]
                    .into_iter()
                    .find_map(|w| part_named(doc, *id, w))
                    .and_then(|h| bounds(doc, h))
                    .map(|(lo, hi)| DVec3::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0, hi.z));
                (*object, (held.unwrap_or(top), along, across))
            })
            .collect();
        self.docks_seen = Some(Docks {
            version: doc.version(),
            objects: key,
            docks: docks.clone(),
            shelves: shelves.clone(),
        });
        (docks, shelves)
    }

    /// Parts on their way to each component: moving, waiting for or riding
    /// on a carrier.
    fn coming(&self) -> BTreeMap<u64, u32> {
        let mut coming = BTreeMap::new();
        for p in &self.parts {
            if let Some(
                Place::Moving { to, .. } | Place::Waiting { to, .. } | Place::Riding { to, .. },
            ) = &p.flow
            {
                *coming.entry(*to).or_insert(0) += 1;
            }
        }
        coming
    }

    /// Whether `object` is a machine with a door made a station.
    fn door_station(&self, object: u64) -> bool {
        self.door_stations
            .as_ref()
            .is_some_and(|(_, _, d)| d.contains(&object))
    }

    /// How many parts station `t` may hold at once, as set: for a machine
    /// with a door, one unless set; never fewer than it takes for a cycle;
    /// none, if it has no such limit (only as many as may wait).
    fn holds_of(&self, t: &Component) -> Option<u32> {
        if t.kind != Kind::Station {
            return None;
        }
        match (t.props.holds, self.door_station(t.object)) {
            (0, false) => None,
            (0, true) => Some(t.props.takes.max(1)),
            (n, _) => Some(n.max(t.props.takes)),
        }
    }

    /// Room for more at component `t`: what may still come to it, with
    /// `coming` on their way to each.
    fn room(&self, t: &Component, coming: &BTreeMap<u64, u32>) -> u32 {
        if t.kind == Kind::Sink {
            return u32::MAX;
        }
        let coming = coming.get(&t.object).copied().unwrap_or(0) as usize;
        let empty = Station::default();
        let s = self.stations.get(&t.object).unwrap_or(&empty);
        // A machine — a CNC, a molder, made a station — takes one part at a
        // time into its chuck or mould: none more while one is waiting, at
        // work or done there.
        let machine = self.door_station(t.object);
        let holds = self.holds_of(t);
        if t.kind == Kind::Station {
            if let Some(holds) = holds {
                // And those done, waiting there for the robot to take out.
                let taking = self
                    .jobs
                    .iter()
                    .filter(|j| j.from == t.object && !j.gripped)
                    .count();
                let inside = s.queue.len()
                    + s.working.iter().map(|(_, p)| p.len()).sum::<usize>()
                    + s.out.len()
                    + taking;
                let left = holds.saturating_sub((inside + coming) as u32);
                if machine {
                    return left;
                }
                return left.min(t.props.capacity.saturating_sub((s.queue.len() + coming) as u32));
            }
        }
        let held = match t.kind {
            Kind::Station => s.queue.len(),
            Kind::Buffer => s.queue.len() + s.out.len(),
            // One at a time onto a belt, its start clear.
            Kind::Conveyor => {
                return self
                    .belt_room
                    .get(&t.object)
                    .copied()
                    .unwrap_or(0)
                    .saturating_sub(coming as u32)
            }
            _ => return 0,
        };
        t.props.capacity.saturating_sub((held + coming) as u32)
    }

    /// Where part `uid` is in the list: kept in the order they were made,
    /// so found by halves; else looked for one by one.
    pub(super) fn part_index(&self, uid: u64) -> Option<usize> {
        match self.parts.binary_search_by_key(&uid, |p| p.uid) {
            Ok(i) => Some(i),
            Err(_) => self.parts.iter().position(|p| p.uid == uid),
        }
    }

    /// Moves the flow on by `dt` seconds. Returns whether it runs, and
    /// whether anything happened: a part in, worked, moved on or out.
    pub(super) fn flow_step(&mut self, doc: &Document, dt: f64) -> (bool, bool) {
        if !self.flow_on {
            return (false, false);
        }
        let now = self.last_tick.unwrap_or(0.0);
        let mut happened = false;
        // A station's door — a machine's, made a station — opens while a
        // robot comes to load it or take from it, and shuts once it has.
        let robot_at: Vec<u64> = self
            .jobs
            .iter()
            .filter(|j| j.by == 3 && j.amr.is_some())
            .flat_map(|j| [j.from, j.to])
            .collect();
        let all: Vec<u64> = self
            .components
            .iter()
            .filter(|c| c.kind == Kind::Station)
            .map(|c| c.object)
            .collect();
        let fresh = self
            .door_stations
            .as_ref()
            .is_some_and(|(_, s, _)| *s == all);
        if !fresh {
            let with_door = all
                .iter()
                .copied()
                .filter(|o| id_of(doc, *o).is_some_and(|id| !self.door_parts(doc, id).is_empty()))
                .collect();
            self.door_stations = Some((doc.version(), all, with_door));
        }
        let stations: Vec<(u64, bool)> = self
            .door_stations
            .as_ref()
            .map(|(_, _, d)| d.iter().map(|o| (*o, robot_at.contains(o))).collect())
            .unwrap_or_default();
        for (object, coming) in stations {
            if coming {
                self.station_door_wanted.insert(object, now);
            }
            // Open while one comes, and a little after it has gone — its arm
            // clear of the door's way —: shut between two robots one after
            // the other only to open again would flicker.
            let open = self
                .station_door_wanted
                .get(&object)
                .is_some_and(|t| now - t < 2.5);
            let was = self.station_doors.get(&object).copied().unwrap_or(0.0);
            // Over less than a second: open before the robot reaches in.
            let pace = dt / 0.8;
            let now_open = if open {
                (was + pace).min(1.0)
            } else {
                (was - pace).max(0.0)
            };
            if now_open != was {
                self.station_doors.insert(object, now_open);
                happened = true;
            }
        }
        // Warmed up: what came before is left out of its figures.
        if !self.warmed && self.flow_clock() >= self.flow_cfg.warmup {
            self.warmed = true;
            if self.flow_cfg.warmup > 0.0 {
                self.just_warmed = true;
                self.flow_stats = Stats {
                    from: Some(now),
                    ..Stats::default()
                };
                for s in self.stations.values_mut() {
                    s.busy = 0.0;
                    s.down = 0.0;
                    s.rested = 0.0;
                    s.good = 0;
                    s.bad = 0;
                }
                happened = true;
            }
        }
        // At rest: nothing comes in, nothing is worked on; what is on its
        // way gets there; the clock of each cycle waits.
        if self.flow_cfg.resting(self.flow_clock()) {
            for s in self.stations.values_mut() {
                s.rested += dt;
                if let Some((end, _)) = s.working.as_mut() {
                    *end += dt;
                }
                if let Some(until) = s.down_until.as_mut() {
                    *until += dt;
                }
            }
            let flow: Vec<Component> = self
                .components
                .iter()
                .filter(|c| c.kind.flow())
                .cloned()
                .collect();
            let (docks, _) = self.docks_of(doc, &flow);
            self.place_flow_parts(&docks, now);
            return (true, happened);
        }
        let flow: Vec<Component> = self
            .components
            .iter()
            .filter(|c| c.kind.flow())
            .cloned()
            .collect();
        let (docks, shelves) = self.docks_of(doc, &flow);
        let by_object: BTreeMap<u64, &Component> = flow.iter().map(|c| (c.object, c)).collect();
        // Each source's parts in the flow, for its kanban.
        let mut in_flow: BTreeMap<u64, u32> = BTreeMap::new();
        if flow
            .iter()
            .any(|c| c.kind == Kind::Source && c.props.kanban > 0)
        {
            for p in self.parts.iter().filter(|p| p.flow.is_some()) {
                *in_flow.entry(p.origin).or_insert(0) += 1;
            }
        }

        // Sources put parts in, while their way out has room.
        for c in flow.iter().filter(|c| c.kind == Kind::Source) {
            let Some(&(top, ..)) = docks.get(&c.object) else {
                continue;
            };
            // A kanban: no more in until one of its own has left.
            let mine = in_flow.get(&c.object).copied().unwrap_or(0);
            let pulled = c.props.kanban == 0 || mine < c.props.kanban;
            let room =
                (self.stations.get(&c.object).map_or(0, |s| s.out.len()) as u32) < c.props.capacity;
            // With orders: each released as its time comes, part by part.
            if self.orders.contains_key(&c.object) {
                let clock = self.flow_clock();
                let next = self.orders.get_mut(&c.object).and_then(|orders| {
                    orders
                        .iter_mut()
                        .enumerate()
                        .find(|(_, o)| o.at <= clock && o.released < o.qty)
                        .map(|(k, o)| (k, o.product))
                });
                if let (Some((k, product)), true, true) = (next, pulled, room) {
                    self.orders.get_mut(&c.object).unwrap()[k].released += 1;
                    self.add_part(top + DVec3::Z * RAW.1 / 2.0, false);
                    let uid = self.parts.last().expect("just added").uid;
                    let p = self.parts.last_mut().unwrap();
                    p.flow = Some(Place::At(c.object));
                    p.product = product;
                    p.origin = c.object;
                    p.look = c.props.look;
                    p.order = k as u32 + 1;
                    let s = self.stations.entry(c.object).or_default();
                    s.made += 1;
                    s.out.push(uid);
                    happened = true;
                }
                continue;
            }
            if c.props.feed <= 0.0 {
                continue;
            }
            let s = self.stations.entry(c.object).or_default();
            // The first at once, then one every feed, as drawn.
            s.fed += dt;
            let first = s.made == 0;
            if !pulled {
                // Held back, it waits no longer than its feed when let go.
                s.fed = s.fed.min(s.wait);
            }
            if s.fed >= s.wait && room && pulled {
                let wait = if first && s.wait == 0.0 { 0.0 } else { s.wait };
                let next = self.draw(c.props.feed, c.props.dist, c.props.spread);
                let s = self.stations.entry(c.object).or_default();
                s.fed -= wait;
                s.wait = next;
                s.made += 1;
                let product = self.product_of(c.object);
                self.add_part(top + DVec3::Z * RAW.1 / 2.0, false);
                let uid = self.parts.last().expect("just added").uid;
                let p = self.parts.last_mut().unwrap();
                p.flow = Some(Place::At(c.object));
                p.product = product;
                p.origin = c.object;
                p.look = c.props.look;
                self.stations.entry(c.object).or_default().out.push(uid);
                happened = true;
            }
        }

        // Parts that have come where they were going.
        let arrived: Vec<(u64, u64)> = self
            .parts
            .iter()
            .filter_map(|p| match &p.flow {
                Some(Place::Moving { to, end, .. }) if now >= *end => Some((p.uid, *to)),
                _ => None,
            })
            .collect();
        for (uid, to) in arrived {
            happened = true;
            if let Some(i) = self.part_index(uid) {
                self.parts[i].rail = None;
            }
            let Some(&t) = by_object.get(&to) else {
                // Its target gone: the part goes too.
                if let Some(i) = self.part_index(uid) {
                    self.parts.remove(i);
                    self.removed.push(uid);
                }
                continue;
            };
            match t.kind {
                Kind::Sink => {
                    if let Some(i) = self.part_index(uid) {
                        let p = self.parts.remove(i);
                        self.removed.push(uid);
                        let lead = now - p.born;
                        let st = &mut self.flow_stats;
                        st.sunk += 1;
                        st.lead_sum += lead;
                        st.lead_min = Some(st.lead_min.map_or(lead, |m| m.min(lead)));
                        st.lead_max = st.lead_max.max(lead);
                        let b = &mut st.by_product[(p.product as usize) % PRODUCTS.len()];
                        b.0 += 1;
                        b.1 += lead;
                        // Its order a part nearer done; done, when.
                        if p.order > 0 {
                            let clock = self.flow_clock();
                            if let Some(o) = self
                                .orders
                                .get_mut(&p.origin)
                                .and_then(|o| o.get_mut(p.order as usize - 1))
                            {
                                o.done += 1;
                                if o.done >= o.qty && o.finished.is_none() {
                                    o.finished = Some(clock);
                                }
                            }
                        }
                        self.stations.entry(to).or_default().made += 1;
                    }
                }
                Kind::Buffer => {
                    self.stations.entry(to).or_default().out.push(uid);
                    if let Some(i) = self.part_index(uid) {
                        self.parts[i].flow = Some(Place::At(to));
                    }
                }
                // Onto the belt at its start, standing, to be carried.
                Kind::Conveyor => {
                    let start = self
                        .belts(doc)
                        .into_iter()
                        .find(|b| b.object == to)
                        .map(|b| b.centre + b.along * (-b.length / 2.0 + RAW.0));
                    if let (Some(i), Some(start)) = (self.part_index(uid), start) {
                        let half = self.parts[i].size().1 / 2.0;
                        self.parts[i].world = DAffine3::from_translation(start + DVec3::Z * half);
                        self.parts[i].flow = Some(Place::Belt(to));
                    }
                }
                _ => {
                    self.stations.entry(to).or_default().queue.push(uid);
                    if let Some(i) = self.part_index(uid) {
                        self.parts[i].flow = Some(Place::At(to));
                    }
                }
            }
        }

        // Stations: broken down and repaired, a cycle done, the next begun.
        for c in flow.iter().filter(|c| c.kind == Kind::Station) {
            let s = self.stations.entry(c.object).or_default().clone();
            let mut s = s;
            if c.props.mtbf > 0.0 && s.fail_in.is_none() && s.down_until.is_none() {
                s.fail_in = Some(self.draw(c.props.mtbf, 4, 0.0));
            }
            match s.down_until {
                // Broken down: the work waits, its end put off.
                Some(until) if now < until => {
                    s.down += dt;
                    if let Some((end, _)) = s.working.as_mut() {
                        *end += dt;
                    }
                }
                Some(_) => {
                    s.down_until = None;
                    s.fail_in = Some(self.draw(c.props.mtbf, 4, 0.0));
                    happened = true;
                }
                None => {
                    if s.working.is_some() {
                        s.busy += dt;
                        if let Some(left) = s.fail_in.as_mut() {
                            *left -= dt;
                            if *left <= 0.0 {
                                s.fail_in = None;
                                let repair = self.draw(c.props.mttr, 4, 0.0);
                                s.down_until = Some(now + repair);
                                happened = true;
                            }
                        }
                    }
                }
            }
            if let Some((end, parts)) = s.working.clone().filter(|_| s.down_until.is_none()) {
                if now >= end {
                    s.working = None;
                    s.made += 1;
                    // Its operator free again, where they stand.
                    s.crew = None;
                    happened = true;
                    // The first goes on as the product; the rest are in it.
                    // Scrap, as often as the station makes it, goes.
                    let scrapped = c.props.scrap > 0.0 && self.uniform() < c.props.scrap;
                    let mut made = Vec::new();
                    for (k, uid) in parts.iter().enumerate() {
                        let Some(i) = self.part_index(*uid) else {
                            continue;
                        };
                        if k == 0 && !scrapped {
                            self.parts[i].finished = true;
                            // As this station makes it look, if it says.
                            if c.props.look > 0 {
                                self.parts[i].look = c.props.look;
                            }
                            made.push(*uid);
                        } else {
                            self.parts.remove(i);
                            self.removed.push(*uid);
                        }
                    }
                    if scrapped {
                        s.bad += 1;
                        self.flow_stats.scrapped += 1;
                        self.stations.insert(c.object, s.clone());
                    } else {
                        s.good += 1;
                    }
                    // More than one out: the rest made here, as old as it.
                    let born = made
                        .first()
                        .and_then(|u| self.part_index(*u))
                        .map_or(now, |i| self.parts[i].born);
                    for _ in (1..c.props.makes).filter(|_| !scrapped) {
                        let at = docks.get(&c.object).map_or(DVec3::ZERO, |d| d.0);
                        self.add_part(at + DVec3::Z * FINISHED.1 / 2.0, true);
                        let p = self.parts.last_mut().unwrap();
                        p.born = born;
                        p.flow = Some(Place::At(c.object));
                        p.look = c.props.look;
                        made.push(p.uid);
                    }
                    s.out.extend(made);
                }
            }
            let takes = c.props.takes.max(1) as usize;
            let ready = s.working.is_none()
                && s.down_until.is_none()
                && s.out.is_empty()
                && s.queue.len() >= takes;
            // A station that needs a person works once one is at it: called
            // if none is, waited for while they walk.
            let manned = c.props.crew == 0
                || match s.crew {
                    Some(op) => self.rovers.get(&op).is_none_or(|r| r.legs.is_empty()),
                    None => {
                        if ready {
                            s.crew = self.call_operator(doc, c.object);
                            happened |= s.crew.is_some();
                        }
                        false
                    }
                };
            if ready && manned {
                let parts: Vec<u64> = s.queue.drain(..takes).collect();
                // Its time for this product, and a change-over if it is
                // another than the last.
                let product = parts
                    .first()
                    .and_then(|u| self.part_index(*u))
                    .map_or(0, |i| self.parts[i].product);
                let mean = self.cycle_of(c, product);
                let mut cycle = self.draw(mean, c.props.dist, c.props.spread);
                if s.last.is_some_and(|l| l != product) {
                    cycle += c.props.setup;
                }
                s.last = Some(product);
                s.working = Some((now + cycle, parts));
                s.length = cycle;
                happened = true;
            }
            self.stations.insert(c.object, s);
        }

        // Each belt of the flow: room for one more at its start; and the part
        // at its end, if it is the flow's, out to the conveyor's next.
        self.belt_room.clear();
        for b in self.belts(doc) {
            if !flow.iter().any(|c| c.object == b.object) {
                continue;
            }
            let gap = 2.0 * RAW.0 + 0.005;
            let start = -b.length / 2.0 + RAW.0;
            let on: Vec<f64> = self
                .parts
                .iter()
                .filter(|p| p.held.is_none() && matches!(p.flow, None | Some(Place::Belt(_))))
                .filter_map(|p| b.along_of(p))
                .collect();
            let clear = !on.iter().any(|x| *x < start + gap);
            let most = ((b.length - 2.0 * RAW.0) / gap).floor().max(1.0) as usize;
            self.belt_room
                .insert(b.object, u32::from(clear && on.len() < most));
            if let Some(i) = b.at_end(&self.parts) {
                if matches!(self.parts[i].flow, Some(Place::Belt(c)) if c == b.object) {
                    let uid = self.parts[i].uid;
                    let s = self.stations.entry(b.object).or_default();
                    if !s.out.contains(&uid) {
                        s.out.push(uid);
                    }
                }
            }
        }

        // What is done goes on, to the first of its next with room.
        let mut coming = self.coming();
        for c in flow.iter().filter(|c| c.kind != Kind::Sink) {
            let next: Vec<&Component> = self
                .flow_next
                .get(&c.object)
                .into_iter()
                .flatten()
                .filter_map(|n| by_object.get(n).copied())
                .collect();
            let stocker = c.kind == Kind::Buffer && c.props.dispatch == 1;
            if next.is_empty() && !stocker {
                continue;
            }
            loop {
                let s = self.stations.entry(c.object).or_default();
                // A buffer last in first out gives its newest; a stocker
                // whichever of its parts has somewhere to go, the oldest
                // first — not all held up behind one whose tool is full.
                let order: Vec<usize> = if stocker {
                    (0..s.out.len()).collect()
                } else if c.kind == Kind::Buffer && c.props.policy == 1 {
                    s.out.len().checked_sub(1).into_iter().collect()
                } else {
                    (0..s.out.len().min(1)).collect()
                };
                let out = s.out.clone();
                let turn = s.turn;
                let mut chosen = None;
                for pick_at in order {
                    let uid = out[pick_at];
                    // By its recipe, from a stocker: the one tool of its next
                    // step. Else where the links go.
                    let by_recipe = self.recipe_step(c, uid);
                    let next: Vec<&Component> = match by_recipe {
                        Some(t) => by_object.get(&t).copied().into_iter().collect(),
                        None => next.clone(),
                    };
                    if next.is_empty() {
                        continue;
                    }
                    let product = self.part_index(uid).map_or(0, |i| self.parts[i].product);
                    // A link that takes certain products takes no others.
                    let takes = |t: &Component| {
                        by_recipe.is_some()
                            || self
                                .routes
                                .get(&(c.object, t.object))
                                .is_none_or(|p| p.contains(&product))
                    };
                    let open: Vec<usize> = (0..next.len())
                        .map(|k| (turn + k) % next.len())
                        .filter(|k| takes(next[*k]) && self.room(next[*k], &coming) > 0)
                        .collect();
                    let pick = match c.props.rule {
                        // The one with the fewest waiting and coming.
                        1 => open
                            .iter()
                            .copied()
                            .max_by_key(|k| self.room(next[*k], &coming)),
                        // Any of them, as it falls.
                        2 if !open.is_empty() => {
                            let r = self.uniform();
                            Some(open[((r * open.len() as f64) as usize).min(open.len() - 1)])
                        }
                        _ => open.first().copied(),
                    };
                    if let Some(k) = pick {
                        chosen = Some((pick_at, uid, next[k], k, by_recipe.is_some()));
                        break;
                    }
                }
                let Some((pick_at, uid, target, k, by_recipe)) = chosen else {
                    break;
                };
                let s = self.stations.entry(c.object).or_default();
                s.out.remove(pick_at);
                if !by_recipe {
                    s.turn = k + 1;
                }
                let Some(i) = self.part_index(uid) else {
                    continue;
                };
                if by_recipe {
                    self.parts[i].step += 1;
                }
                happened = true;
                *coming.entry(target.object).or_insert(0) += 1;
                // By AMR, or by hand: it waits here for one.
                if c.props.carrier > 0 {
                    self.parts[i].flow = Some(Place::Waiting {
                        at: c.object,
                        to: target.object,
                    });
                    self.jobs.push(super::fleet::Job {
                        part: uid,
                        from: c.object,
                        to: target.object,
                        amr: None,
                        phase: super::fleet::Phase::Waiting,
                        by: c.props.carrier,
                        gripped: false,
                    });
                    continue;
                }
                let from = self.parts[i].world.translation;
                let to = docks.get(&target.object).map_or(from, |d| d.0);
                // Off a belt's end: handed over at a walk, not at the belt's
                // own pace.
                let speed = if c.kind == Kind::Conveyor {
                    1.0
                } else {
                    c.props.speed
                };
                // Into a stocker with a crane, or out of it to its recipe's
                // next tool: across onto its rail, along it and across off
                // it. Sent on once the recipe is done, out of the bay: not
                // by the crane.
                let on_rail = |s: &Component| s.kind == Kind::Buffer && s.props.rail > 0;
                let crane = if on_rail(target) {
                    Some(target)
                } else if on_rail(c) && by_recipe {
                    Some(c)
                } else {
                    None
                };
                let rail = crane.and_then(|s| {
                    let &(on, along, _) = docks.get(&s.object)?;
                    // By overhead hoist: straight up to its rail, along it
                    // and straight down; its way marked +Z.
                    if s.props.rail == 2 {
                        let up = |p: DVec3| DVec3::new(p.x, p.y, super::HOIST_RIDE);
                        return Some([up(from), up(to), DVec3::Z]);
                    }
                    let onto = |p: DVec3| {
                        let q = on + along * (p - on).dot(along);
                        DVec3::new(q.x, q.y, p.z)
                    };
                    Some([onto(from), onto(to), along])
                });
                let length = match rail {
                    Some([a, b, _]) => from.distance(a) + a.distance(b) + b.distance(to),
                    None => from.distance(to),
                };
                let seconds = length / speed.max(0.01);
                self.parts[i].rail = rail;
                self.parts[i].flow = Some(Place::Moving {
                    from,
                    to: target.object,
                    start: now,
                    end: now + seconds,
                });
            }
        }

        happened |= self.carry_on(doc, now);
        // The series: every so often, more seldom as the run grows long.
        let from = self.flow_stats.from.unwrap_or(now);
        let t = now - from;
        let every = (t / 400.0).max(5.0);
        if t >= self.flow_stats.sampled + every || self.flow_stats.series.is_empty() {
            let wip = self.parts.iter().filter(|p| p.flow.is_some()).count() as f64;
            let sunk = self.flow_stats.sunk as f64;
            self.flow_stats.series.push([t, wip, sunk]);
            self.flow_stats.sampled = t;
            if self.flow_stats.series.len() > 1000 {
                let kept: Vec<[f64; 3]> =
                    self.flow_stats.series.iter().step_by(2).copied().collect();
                self.flow_stats.series = kept;
            }
        }
        // A rack's shelves, where it has any: its parts set out on them.
        self.shelf_tops = shelves;
        self.place_flow_parts(&docks, now);
        self.changed |= happened;
        (true, happened)
    }

    /// Each part of the flow where it now is: set out on the top of its
    /// component — worked on, then done, then waiting — or on its way.
    fn place_flow_parts(&mut self, docks: &BTreeMap<u64, (DVec3, DVec3, DVec3)>, now: f64) {
        let mut spots: BTreeMap<u64, DVec3> = BTreeMap::new();
        // Parts of a look — car bodies, glass, panels — too big to set out
        // in rows on a table: in line at a station, the one worked on at it
        // and the others behind; in lanes in a store; as far apart as the
        // biggest of them needs. Each faces the way its station does.
        let cars: BTreeMap<u64, (f64, f64, (f64, usize))> = self
            .parts
            .iter()
            .filter(|p| p.look > 0)
            .map(|p| {
                let look = (p.look as usize).min(LOOK_PITCH.len() - 1);
                (
                    p.uid,
                    (LOOK_PITCH[look].0, LOOK_PITCH[look].1, LOOK_STACK[look]),
                )
            })
            .collect();
        let mut facing: BTreeMap<u64, f64> = BTreeMap::new();
        // Those waiting at each for a carrier.
        let mut waiting_at: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
        // A robot's part not yet in its gripper stays exactly where it is —
        // the robot was sent there —, and the others are set out around it,
        // not onto its place (see below).
        let ungripped: Vec<u64> = self
            .jobs
            .iter()
            .filter(|j| j.by == 3 && j.amr.is_some() && !j.gripped)
            .map(|j| j.part)
            .collect();
        for j in self.jobs.iter().filter(|j| ungripped.contains(&j.part)) {
            waiting_at.entry(j.from).or_default().push(j.part);
        }
        // Places given out this time, for those that have gone to give up
        // theirs.
        let mut placed: std::collections::BTreeSet<u64> = std::collections::BTreeSet::new();
        for p in &self.parts {
            if let Some(Place::Waiting { at, .. }) = &p.flow {
                waiting_at.entry(*at).or_default().push(p.uid);
            }
        }
        for (object, s) in &self.stations {
            let Some(&(top, along, across)) = docks.get(object) else {
                continue;
            };
            // A belt's parts stay where the belt brought them: one waiting
            // at its end for a robot or an AMR is taken from there.
            if self
                .components
                .iter()
                .any(|c| c.object == *object && c.kind == Kind::Conveyor)
            {
                continue;
            }
            let order = s
                .working
                .iter()
                .flat_map(|(_, p)| p.iter())
                .chain(s.out.iter())
                .chain(s.queue.iter());
            // And those waiting here for an AMR.
            let order = order.chain(waiting_at.get(object).into_iter().flatten());
            let shelves = self.shelf_tops.get(object);
            let store = self
                .components
                .iter()
                .any(|c| c.object == *object && c.kind == Kind::Buffer);
            let order: Vec<u64> = order.copied().collect();
            let (pitch, lanes, (stack, high)) = order.iter().filter_map(|u| cars.get(u)).fold(
                (0.0_f64, 0.0_f64, (0.0_f64, 1_usize)),
                |a, b| {
                    let stack = if b.2 .0 > a.2 .0 { b.2 } else { a.2 };
                    (a.0.max(b.0), a.1.max(b.1), stack)
                },
            );
            if pitch > 0.0 {
                let yaw = along.y.atan2(along.x);
                for (k, uid) in order.iter().enumerate() {
                    let at = if store && stack > 0.0 {
                        // Stacked, so many high, the stacks in a row.
                        let (col, level) = ((k / high) as f64, (k % high) as f64);
                        top + along * col * pitch + DVec3::Z * level * stack
                    } else if store {
                        let (col, lane) = ((k % 4) as f64 - 1.5, (k / 4) as f64);
                        top + along * col * pitch + across * lane * lanes
                    } else {
                        top - along * k as f64 * pitch
                    };
                    spots.insert(*uid, at);
                    facing.insert(*uid, yaw);
                }
                continue;
            }
            // Each part in the place it was given when it came — the lowest
            // free one —, kept until it goes: one taken away leaves a gap
            // the next to come fills, the others not moving up.
            let mut used: std::collections::BTreeSet<usize> = order
                .iter()
                .filter_map(|u| self.slots.get(u).filter(|s| s.0 == *object).map(|s| s.1))
                .collect();
            for uid in &order {
                let k = match self.slots.get(uid) {
                    Some(&(o, k)) if o == *object => k,
                    _ => {
                        let k = (0..).find(|k| !used.contains(k)).unwrap_or(0);
                        used.insert(k);
                        self.slots.insert(*uid, (*object, k));
                        k
                    }
                };
                placed.insert(*uid);
                // On a rack: ten to a shelf, the lowest first; else rows of
                // five across the top, from its middle.
                let (level, i) = match shelves {
                    Some(tops) => (Some(tops[(k / 10).min(tops.len() - 1)]), k % 10),
                    None => (None, k),
                };
                let (col, row) = ((i % 5) as f64 - 2.0, (i / 5) as f64);
                let base = level.unwrap_or(top);
                spots.insert(*uid, base + along * col * PITCH + across * row * PITCH);
            }
        }
        // A part no longer set out on a table gives up its place.
        self.slots.retain(|u, _| placed.contains(u));
        for p in &mut self.parts {
            let half = p.size().1 / 2.0;
            // A robot's part not yet gripped: where it waited, as above.
            if ungripped.contains(&p.uid) && p.held.is_none() {
                if let Some(spot) = spots.get(&p.uid) {
                    let turn = DQuat::from_rotation_z(facing.get(&p.uid).copied().unwrap_or(0.0));
                    p.world = DAffine3::from_rotation_translation(turn, *spot + DVec3::Z * half);
                }
                continue;
            }
            match &p.flow {
                Some(Place::At(_) | Place::Waiting { .. }) => {
                    if let Some(spot) = spots.get(&p.uid) {
                        let turn =
                            DQuat::from_rotation_z(facing.get(&p.uid).copied().unwrap_or(0.0));
                        p.world =
                            DAffine3::from_rotation_translation(turn, *spot + DVec3::Z * half);
                    }
                }
                Some(Place::Moving {
                    from,
                    to,
                    start,
                    end,
                }) => {
                    let target = docks.get(to).map_or(*from, |d| d.0 + DVec3::Z * half);
                    let t = if end > start {
                        ((now - start) / (end - start)).clamp(0.0, 1.0)
                    } else {
                        1.0
                    };
                    // By a stocker's crane: along its path at an even pace,
                    // facing the rail's way.
                    if let Some([a, b, along]) = p.rail {
                        let overhead = along.z > 0.5;
                        let (a, b) = if overhead {
                            (a, b)
                        } else {
                            (DVec3::new(a.x, a.y, from.z), DVec3::new(b.x, b.y, target.z))
                        };
                        let legs = [(*from, a), (a, b), (b, target)];
                        let total: f64 = legs.iter().map(|(u, v)| u.distance(*v)).sum();
                        let mut left = t * total;
                        let mut at = target;
                        for (u, v) in legs {
                            let l = u.distance(v);
                            if left <= l {
                                at = if l > 1e-9 { u.lerp(v, left / l) } else { u };
                                break;
                            }
                            left -= l;
                        }
                        let way = if overhead { b - a } else { along };
                        let turn = if way.truncate().length() > 1e-6 {
                            DQuat::from_rotation_z(way.y.atan2(way.x))
                        } else {
                            DQuat::IDENTITY
                        };
                        p.world = DAffine3::from_rotation_translation(turn, at);
                        continue;
                    }
                    // A car faces the way it goes.
                    let way = target - *from;
                    let turn = if p.look > 0 && way.truncate().length() > 1e-6 {
                        DQuat::from_rotation_z(way.y.atan2(way.x))
                    } else {
                        DQuat::IDENTITY
                    };
                    // A sheet, a cassette or a FOUP goes overhead, lifted and
                    // set down again.
                    let lift = if matches!(p.look, 4 | 7 | 8) && way.truncate().length() > 2.0 {
                        DVec3::Z * LIFT * (std::f64::consts::PI * t).sin()
                    } else {
                        DVec3::ZERO
                    };
                    p.world =
                        DAffine3::from_rotation_translation(turn, from.lerp(target, t) + lift);
                }
                // Where its AMR is: placed by `carry_on`; on a belt, by it.
                Some(Place::Riding { .. } | Place::Belt(_)) | None => {}
            }
        }
    }

    /// What each station is doing, for the timeline.
    pub(super) fn flow_activities(&self) -> Vec<(u64, String, &'static str)> {
        self.components
            .iter()
            .filter(|c| c.kind == Kind::Station)
            .map(|c| {
                let s = self.stations.get(&c.object);
                let resting = self.flow_on && self.flow_cfg.resting(self.flow_clock());
                let activity = match s {
                    _ if resting => "rest",
                    Some(s) if s.down_until.is_some() => "down",
                    Some(s) if s.working.is_some() => "cut",
                    Some(s) if !s.out.is_empty() => "blocked",
                    _ => "idle",
                };
                (c.object, c.name.clone(), activity)
            })
            .collect()
    }

    /// Component `c` of the flow, for the page.
    pub(super) fn flow_json(&self, doc: &Document, c: &Component) -> Value {
        let s = self.stations.get(&c.object).cloned().unwrap_or_default();
        let now = self.last_tick.unwrap_or(0.0);
        json!({
            "next": self.flow_next.get(&c.object).map(|n| n.iter().map(u64::to_string).collect::<Vec<_>>()).unwrap_or_default(),
            "at": self
                .docks_seen
                .as_ref()
                .filter(|d| d.version == doc.version())
                .and_then(|d| d.docks.get(&c.object).copied())
                .or_else(|| Self::dock(doc, c))
                .map(|d| [d.0.x, d.0.y, d.0.z]),
            "queue": s.queue.len(),
            "out": s.out.len(),
            "working": s.working.is_some(),
            // In it now — waiting, at work and done —, and the most it may
            // hold at once.
            "inside": s.queue.len() + s.working.as_ref().map_or(0, |(_, p)| p.len()) + s.out.len(),
            "holds": self.holds_of(c),
            "left": s.working.as_ref().map(|(end, _)| (end - now).max(0.0)),
            // How far the work in hand has got, 0 to 1.
            "progress": s.working.as_ref().map(|(end, _)| {
                if s.length > 0.0 {
                    (1.0 - (end - now) / s.length).clamp(0.0, 1.0)
                } else {
                    1.0
                }
            }),
            "made": s.made,
            "down": s.down_until.is_some(),
            "mix": self.mix.get(&c.object),
            "orders": self.orders.get(&c.object).map(Vec::len),
            "cycles": self.cycles.get(&c.object),
            "routes": self
                .routes
                .iter()
                .filter(|((from, _), _)| *from == c.object)
                .map(|((_, to), p)| (to.to_string(), json!(p)))
                .collect::<serde_json::Map<_, _>>(),
            "oee": (c.kind == Kind::Station).then(|| self.oee(c)),
        })
    }

    /// What the equipment placed cost (the library's), set by the view.
    pub fn set_investment(&mut self, investment: f64) {
        if (self.investment - investment).abs() > 1e-9 {
            self.investment = investment;
            self.changed = true;
        }
    }

    /// The cost of the line as it ran: what was invested; its write-off and
    /// its operators' wages an hour; what a part costs at the rate it came
    /// out; and, with what a part brings in, the years to pay it back.
    pub fn cost(&self) -> Value {
        let (hours, years) = self.flow_cfg.horizon();
        let wages: f64 = self
            .components
            .iter()
            .filter(|c| c.kind == Kind::Operator)
            .map(|c| c.props.wage)
            .sum();
        let write_off = self.investment * 10000.0 / (hours * years);
        let per_hour = write_off + wages;
        let st = &self.flow_stats;
        let now = self.last_tick.unwrap_or(0.0);
        let elapsed = st.from.map_or(0.0, |f| (now - f).max(0.0));
        let rate = if elapsed > 0.0 {
            st.sunk as f64 / elapsed * 3600.0
        } else {
            0.0
        };
        let earned = self.flow_cfg.margin * rate - wages;
        json!({
            "investment": self.investment,
            "write_off": write_off,
            "wages": wages,
            "per_hour": per_hour,
            "per_part": (rate > 0.0).then(|| per_hour / rate),
            "payback": (self.flow_cfg.margin > 0.0 && earned > 0.0 && self.investment > 0.0)
                .then(|| self.investment * 10000.0 / (earned * hours)),
            "hours": hours,
            "years": years,
            "margin": self.flow_cfg.margin,
        })
    }

    /// The orders: how many, done, done by their due time, and how late
    /// those that were not, on average (seconds).
    fn orders_summary(&self) -> Value {
        let all: Vec<&Order> = self.orders.values().flatten().collect();
        if all.is_empty() {
            return Value::Null;
        }
        let done: Vec<&&Order> = all.iter().filter(|o| o.finished.is_some()).collect();
        let late: Vec<f64> = done
            .iter()
            .filter(|o| o.due > 0.0)
            .map(|o| o.finished.unwrap() - o.due)
            .filter(|l| *l > 0.0)
            .collect();
        json!({
            "total": all.len(),
            "released": all.iter().filter(|o| o.released > 0).count(),
            "done": done.len(),
            "on_time": done.len() - late.len(),
            "late_mean": (!late.is_empty()).then(|| late.iter().sum::<f64>() / late.len() as f64),
        })
    }

    /// Station `c`'s OEE since the flow started: availability (the time it
    /// was not broken down), performance (its mean cycle over the time it
    /// could work, for each part made) and quality (good of all made).
    pub(super) fn oee(&self, c: &Component) -> Value {
        let s = self.stations.get(&c.object).cloned().unwrap_or_default();
        let now = self.last_tick.unwrap_or(0.0);
        let planned =
            (self.flow_stats.from.map_or(0.0, |f| (now - f).max(0.0)) - s.rested).max(0.0);
        let run = (planned - s.down).max(0.0);
        let made = s.good + s.bad;
        let a = if planned > 0.0 { run / planned } else { 0.0 };
        let p = if run > 0.0 {
            (made as f64 * c.props.cycle / run).min(1.0)
        } else {
            0.0
        };
        let q = if made > 0 {
            s.good as f64 / made as f64
        } else {
            0.0
        };
        // And the shares of the planned time it worked and was down: what
        // makes it the bottleneck.
        let share = |x: f64| {
            if planned > 0.0 {
                (x / planned).min(1.0)
            } else {
                0.0
            }
        };
        json!({
            "availability": a,
            "performance": p,
            "quality": q,
            "oee": a * p * q,
            "good": s.good,
            "bad": s.bad,
            "busy": share(s.busy),
            "down": share(s.down),
        })
    }

    /// The flow as a whole: running or not, what reached the sinks.
    pub fn flow_summary(&self) -> Value {
        let st = &self.flow_stats;
        let now = self.last_tick.unwrap_or(0.0);
        let elapsed = st.from.map_or(0.0, |f| (now - f).max(0.0));
        json!({
            "on": self.flow_on,
            "elapsed": elapsed,
            "sunk": st.sunk,
            "per_hour": if elapsed > 0.0 { st.sunk as f64 / elapsed * 3600.0 } else { 0.0 },
            "lead_mean": (st.sunk > 0).then(|| st.lead_sum / st.sunk as f64),
            "lead_min": st.lead_min,
            "lead_max": (st.sunk > 0).then_some(st.lead_max),
            "wip": self.parts.iter().filter(|p| p.flow.is_some()).count(),
            "carrying": self.jobs.len(),
            "scrapped": st.scrapped,
            "products": st
                .by_product
                .iter()
                .enumerate()
                .filter(|(_, b)| b.0 > 0)
                .map(|(k, b)| json!({ "product": PRODUCTS[k], "sunk": b.0, "lead_mean": b.1 / b.0 as f64 }))
                .collect::<Vec<_>>(),
            "settings": self.flow_cfg.to_json(),
            "series": st.series,
            "orders": self.orders_summary(),
            "cost": self.cost(),
            "resting": self.flow_on && self.flow_cfg.resting(self.flow_clock()),
            "warming": self.flow_on && !self.warmed && self.flow_cfg.warmup > 0.0,
            "stations": self
                .components
                .iter()
                .filter(|c| c.kind == Kind::Station)
                .map(|c| {
                    let mut v = self.oee(c);
                    v["name"] = json!(c.name);
                    v
                })
                .collect::<Vec<_>>(),
            "problem": self.flow_problem,
        })
    }
}

#[cfg(test)]
mod tests;
