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
}

/// What the sinks have seen since the flow started.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    pub from: Option<f64>,
    pub sunk: u64,
    pub lead_sum: f64,
    pub lead_min: Option<f64>,
    pub lead_max: f64,
}

/// Room between parts set out on a component, metres.
const PITCH: f64 = 2.0 * RAW.0 + 0.01;

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
            let s = self.stations.entry(c.object).or_default();
            if c.props.feed <= 0.0 {
                continue;
            }
            // The first at once, then one every feed.
            s.fed += dt;
            if s.made == 0 && s.out.is_empty() {
                s.fed = s.fed.max(c.props.feed);
            }
            if s.fed >= c.props.feed && (s.out.len() as u32) < c.props.capacity {
                s.fed -= c.props.feed;
                s.made += 1;
                self.add_part(top + DVec3::Z * RAW.1 / 2.0, false);
                let uid = self.parts.last().expect("just added").uid;
                self.parts.last_mut().unwrap().flow = Some(Place::At(c.object));
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
                        self.stations.entry(to).or_default().made += 1;
                    }
                }
                Kind::Buffer => {
                    self.stations.entry(to).or_default().out.push(uid);
                    if let Some(i) = self.part_index(uid) {
                        self.parts[i].flow = Some(Place::At(to));
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

        // Stations: a cycle done, and the next begun.
        for c in flow.iter().filter(|c| c.kind == Kind::Station) {
            let s = self.stations.entry(c.object).or_default().clone();
            let mut s = s;
            if let Some((end, parts)) = s.working.clone() {
                if now >= end {
                    s.working = None;
                    s.made += 1;
                    happened = true;
                    // The first goes on as the product; the rest are in it.
                    let mut made = Vec::new();
                    for (k, uid) in parts.iter().enumerate() {
                        let Some(i) = self.part_index(*uid) else {
                            continue;
                        };
                        if k == 0 {
                            self.parts[i].finished = true;
                            made.push(*uid);
                        } else {
                            self.parts.remove(i);
                            self.removed.push(*uid);
                        }
                    }
                    // More than one out: the rest made here, as old as it.
                    let born = made
                        .first()
                        .and_then(|u| self.part_index(*u))
                        .map_or(now, |i| self.parts[i].born);
                    for _ in 1..c.props.makes {
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
            if s.working.is_none() && s.out.is_empty() && s.queue.len() >= takes {
                let parts: Vec<u64> = s.queue.drain(..takes).collect();
                s.working = Some((now + c.props.cycle, parts));
                happened = true;
            }
            self.stations.insert(c.object, s);
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
                let Some(&uid) = s.out.first() else { break };
                let turn = s.turn;
                let pick = (0..next.len())
                    .map(|k| (turn + k) % next.len())
                    .find(|k| self.room(next[*k]) > 0);
                let Some(k) = pick else { break };
                let target = next[k];
                let s = self.stations.entry(c.object).or_default();
                s.out.remove(0);
                s.turn = k + 1;
                let Some(i) = self.part_index(uid) else {
                    continue;
                };
                happened = true;
                // By AMR: it waits here for one.
                if c.props.carrier == 1 {
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
                    });
                    continue;
                }
                let from = self.parts[i].world.translation;
                let to = docks.get(&target.object).map_or(from, |d| d.0);
                let seconds = from.distance(to) / c.props.speed.max(0.01);
                self.parts[i].flow = Some(Place::Moving {
                    from,
                    to: target.object,
                    start: now,
                    end: now + seconds,
                });
            }
        }

        happened |= self.carry_on(doc, now);
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
            for (k, uid) in order.enumerate() {
                // Rows of five across the top, from its middle.
                let (col, row) = ((k % 5) as f64 - 2.0, (k / 5) as f64);
                spots.insert(*uid, top + along * col * PITCH + across * row * PITCH);
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
                // Where its AMR is: placed by `carry_on`.
                Some(Place::Riding { .. }) | None => {}
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
                let activity = match s {
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
        })
    }

    /// The flow as a whole: running or not, what reached the sinks.
    pub(super) fn flow_summary(&self) -> Value {
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
            "problem": self.flow_problem,
        })
    }
}

#[cfg(test)]
mod tests;
