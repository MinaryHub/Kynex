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

/// The flow's calendar and warm-up: work and rest in turn (minutes of
/// each; no rest, no calendar), and the time at the start left out of its
/// figures, seconds.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Settings {
    pub work: f64,
    pub rest: f64,
    pub warmup: f64,
}

impl Settings {
    pub fn of(v: &Value) -> Self {
        let n = |k: &str| v[k].as_f64().unwrap_or(0.0).clamp(0.0, 1.0e6);
        Self {
            work: n("work"),
            rest: n("rest"),
            warmup: n("warmup"),
        }
    }

    pub fn to_json(self) -> Value {
        json!({ "work": self.work, "rest": self.rest, "warmup": self.warmup })
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
                        "공정 흐름에 소재 공급이 없습니다. 셀 역할 '소재 공급'을 정하세요".into(),
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
                self.rng = SEED;
                self.warmed = false;
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
        Some((top, along, across))
    }

    /// Room for more at component `t`: what may still come to it.
    fn room(&self, t: &Component) -> u32 {
        if t.kind == Kind::Sink {
            return u32::MAX;
        }
        let s = self.stations.get(&t.object).cloned().unwrap_or_default();
        let coming = self
            .parts
            .iter()
            .filter(|p| {
                matches!(&p.flow,
                    Some(Place::Moving { to, .. } | Place::Waiting { to, .. } | Place::Riding { to, .. })
                        if *to == t.object)
            })
            .count();
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

    pub(super) fn part_index(&self, uid: u64) -> Option<usize> {
        self.parts.iter().position(|p| p.uid == uid)
    }

    /// Moves the flow on by `dt` seconds. Returns whether it runs, and
    /// whether anything happened: a part in, worked, moved on or out.
    pub(super) fn flow_step(&mut self, doc: &Document, dt: f64) -> (bool, bool) {
        if !self.flow_on {
            return (false, false);
        }
        let now = self.last_tick.unwrap_or(0.0);
        let mut happened = false;
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
            let docks: BTreeMap<u64, (DVec3, DVec3, DVec3)> = self
                .components
                .iter()
                .filter(|c| c.kind.flow())
                .filter_map(|c| Some((c.object, Self::dock(doc, c)?)))
                .collect();
            self.place_flow_parts(&docks, now);
            return (true, happened);
        }
        let flow: Vec<Component> = self
            .components
            .iter()
            .filter(|c| c.kind.flow())
            .cloned()
            .collect();
        let docks: BTreeMap<u64, (DVec3, DVec3, DVec3)> = flow
            .iter()
            .filter_map(|c| Some((c.object, Self::dock(doc, c)?)))
            .collect();

        // Sources put parts in, while their way out has room.
        for c in flow.iter().filter(|c| c.kind == Kind::Source) {
            let Some(&(top, ..)) = docks.get(&c.object) else {
                continue;
            };
            if c.props.feed <= 0.0 {
                continue;
            }
            let s = self.stations.entry(c.object).or_default();
            // The first at once, then one every feed, as drawn.
            s.fed += dt;
            let first = s.made == 0;
            if s.fed >= s.wait && (s.out.len() as u32) < c.props.capacity {
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
            let Some(t) = flow.iter().find(|c| c.object == to) else {
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
        for c in flow.iter().filter(|c| c.kind != Kind::Sink) {
            let next: Vec<&Component> = self
                .flow_next
                .get(&c.object)
                .into_iter()
                .flatten()
                .filter_map(|n| flow.iter().find(|t| t.object == *n))
                .collect();
            if next.is_empty() {
                continue;
            }
            loop {
                let s = self.stations.entry(c.object).or_default();
                // A buffer last in first out gives its newest.
                let pick_at = if c.kind == Kind::Buffer && c.props.policy == 1 {
                    s.out.len().saturating_sub(1)
                } else {
                    0
                };
                let Some(&uid) = s.out.get(pick_at) else {
                    break;
                };
                let turn = s.turn;
                let product = self.part_index(uid).map_or(0, |i| self.parts[i].product);
                // A link that takes certain products takes no others.
                let takes = |t: &Component| {
                    self.routes
                        .get(&(c.object, t.object))
                        .is_none_or(|p| p.contains(&product))
                };
                let pick = (0..next.len())
                    .map(|k| (turn + k) % next.len())
                    .find(|k| takes(next[*k]) && self.room(next[*k]) > 0);
                let Some(k) = pick else { break };
                let target = next[k];
                let s = self.stations.entry(c.object).or_default();
                s.out.remove(pick_at);
                s.turn = k + 1;
                let Some(i) = self.part_index(uid) else {
                    continue;
                };
                happened = true;
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
                let seconds = from.distance(to) / speed.max(0.01);
                self.parts[i].flow = Some(Place::Moving {
                    from,
                    to: target.object,
                    start: now,
                    end: now + seconds,
                });
            }
        }

        happened |= self.carry_on(doc, now);
        // A rack's shelves, where it has any: its parts set out on them.
        self.shelf_tops = flow
            .iter()
            .filter(|c| c.kind == Kind::Buffer)
            .filter_map(|c| {
                let id = id_of(doc, c.object)?;
                let mut tops: Vec<DVec3> = doc
                    .objects()
                    .filter(|(o, x)| is_below(doc, *o, id) && x.name.starts_with("shelf"))
                    .filter_map(|(o, _)| bounds(doc, o))
                    .map(|(lo, hi)| DVec3::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0, hi.z))
                    .collect();
                tops.sort_by(|a, b| a.z.total_cmp(&b.z));
                (!tops.is_empty()).then_some((c.object, tops))
            })
            .collect();
        self.place_flow_parts(&docks, now);
        self.changed |= happened;
        (true, happened)
    }

    /// Each part of the flow where it now is: set out on the top of its
    /// component — worked on, then done, then waiting — or on its way.
    fn place_flow_parts(&mut self, docks: &BTreeMap<u64, (DVec3, DVec3, DVec3)>, now: f64) {
        let mut spots: BTreeMap<u64, DVec3> = BTreeMap::new();
        for (object, s) in &self.stations {
            let Some(&(top, along, across)) = docks.get(object) else {
                continue;
            };
            let order = s
                .working
                .iter()
                .flat_map(|(_, p)| p.iter())
                .chain(s.out.iter())
                .chain(s.queue.iter());
            // And those waiting here for an AMR.
            let waiting: Vec<u64> = self
                .parts
                .iter()
                .filter(|p| matches!(&p.flow, Some(Place::Waiting { at, .. }) if at == object))
                .map(|p| p.uid)
                .collect();
            let order = order.chain(waiting.iter());
            let shelves = self.shelf_tops.get(object);
            for (k, uid) in order.enumerate() {
                // On a rack: ten to a shelf, the lowest first; else rows of
                // five across the top, from its middle.
                let (level, k) = match shelves {
                    Some(tops) => (Some(tops[(k / 10).min(tops.len() - 1)]), k % 10),
                    None => (None, k),
                };
                let (col, row) = ((k % 5) as f64 - 2.0, (k / 5) as f64);
                let base = level.unwrap_or(top);
                spots.insert(*uid, base + along * col * PITCH + across * row * PITCH);
            }
        }
        for p in &mut self.parts {
            let half = p.size().1 / 2.0;
            match &p.flow {
                Some(Place::At(_) | Place::Waiting { .. }) => {
                    if let Some(spot) = spots.get(&p.uid) {
                        p.world = DAffine3::from_translation(*spot + DVec3::Z * half);
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
                    p.world = DAffine3::from_translation(from.lerp(target, t));
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
            "at": Self::dock(doc, c).map(|d| [d.0.x, d.0.y, d.0.z]),
            "queue": s.queue.len(),
            "out": s.out.len(),
            "working": s.working.is_some(),
            "left": s.working.as_ref().map(|(end, _)| (end - now).max(0.0)),
            "made": s.made,
            "down": s.down_until.is_some(),
            "mix": self.mix.get(&c.object),
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
        json!({ "availability": a, "performance": p, "quality": q, "oee": a * p * q, "good": s.good, "bad": s.bad })
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
