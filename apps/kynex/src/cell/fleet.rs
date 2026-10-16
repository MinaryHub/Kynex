//! AMRs as a fleet: a network of paths between their stops, the shortest
//! way along it, traffic kept apart, and parts of a process flow carried
//! from station to station.
//!
//! Stops linked (`Cell::lanes`, both ways) make the network an AMR drives
//! on: sent to a stop, it goes to the stop of the network nearest it, then
//! stop by stop along the shortest way. With no links it drives straight,
//! as before.
//!
//! Traffic: an AMR holds the stop it is at or driving to; another waits
//! before driving to a stop held, and goes on once it is let go of. Two
//! AMRs driving at each other along one lane would wait for each other
//! for ever: lanes one way round, or a passing place, avoid that, as on a
//! real floor.
//!
//! A flow component that sends by AMR (`Props::carrier` 1) has each part
//! it sends fetched by the first AMR free: it drives to the stop nearest
//! the sender, loads (`LOAD` seconds), drives to the stop nearest where the
//! part goes, and unloads there.

use super::flow::Place;
use super::*;

/// Seconds to load or unload a part at a stop.
pub const LOAD: f64 = 5.0;

/// How far from a flow component the stop it is served at may be, metres.
const NEAR: f64 = 4.0;

/// A part to carry, and how far it has got.
#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub part: u64,
    pub from: u64,
    pub to: u64,
    pub amr: Option<u64>,
    pub phase: Phase,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    /// For an AMR to be free.
    Waiting,
    /// The AMR on its way to the sender's stop.
    Fetching,
    /// Being loaded, until then.
    Loading(f64),
    /// The AMR on its way with it.
    Carrying,
    /// Being unloaded, until then.
    Unloading(f64),
}

impl Cell {
    /// Links stop `object` to the stops `next`, both ways: the lanes AMRs
    /// drive along.
    pub fn set_lanes(&mut self, object: u64, next: Vec<u64>) -> Result<(), String> {
        let is_stop = |u: u64| {
            self.components
                .iter()
                .any(|c| c.object == u && c.kind == Kind::Stop)
        };
        if !is_stop(object) {
            return Err("AMR 정지 위치가 아닙니다".into());
        }
        if next.iter().any(|n| !is_stop(*n) || *n == object) {
            return Err("다른 AMR 정지 위치와만 이을 수 있습니다".into());
        }
        // Both ways: what was linked to it and is no more is unlinked too.
        for lanes in self.lanes.values_mut() {
            lanes.retain(|n| *n != object);
        }
        for n in &next {
            let back = self.lanes.entry(*n).or_default();
            if !back.contains(&object) {
                back.push(object);
            }
        }
        self.lanes.insert(object, next);
        self.lanes.retain(|_, v| !v.is_empty());
        self.roles_changed = true;
        self.changed = true;
        Ok(())
    }

    /// Each stop's place on the floor, by stop.
    pub(super) fn stop_places(&self, doc: &Document) -> BTreeMap<u64, (DVec3, f64)> {
        self.components
            .iter()
            .filter(|c| c.kind == Kind::Stop)
            .filter_map(|c| {
                Some((
                    c.object,
                    floor_pose(&doc.world_transform(id_of(doc, c.object)?)),
                ))
            })
            .collect()
    }

    /// The legs from `from` to stop `stop` along the lanes: to the stop of
    /// the network nearest, then stop by stop the shortest way, and turned
    /// as the stop faces. None if the lanes do not lead there; straight
    /// there if there are no lanes.
    pub(super) fn route(&self, doc: &Document, from: DVec3, stop: u64) -> Result<Vec<Leg>, String> {
        let places = self.stop_places(doc);
        let &(to, facing) = places.get(&stop).ok_or("no such stop")?;
        let mut legs = Vec::new();
        let in_network = |n: &u64| self.lanes.contains_key(n);
        let path: Vec<u64> = if self.lanes.is_empty() || !in_network(&stop) {
            vec![stop]
        } else {
            let start = places
                .iter()
                .filter(|(n, _)| in_network(n))
                .min_by(|a, b| {
                    a.1 .0
                        .truncate()
                        .distance(from.truncate())
                        .total_cmp(&b.1 .0.truncate().distance(from.truncate()))
                })
                .map(|(n, _)| *n)
                .ok_or("no stop on the lanes")?;
            // Dijkstra, the graph being small.
            let mut dist: BTreeMap<u64, f64> = BTreeMap::new();
            let mut prev: BTreeMap<u64, u64> = BTreeMap::new();
            let mut open: Vec<u64> = vec![start];
            dist.insert(start, 0.0);
            while let Some(k) = open
                .iter()
                .enumerate()
                .min_by(|a, b| dist[a.1].total_cmp(&dist[b.1]))
                .map(|(k, _)| k)
            {
                let n = open.swap_remove(k);
                if n == stop {
                    break;
                }
                for m in self.lanes.get(&n).into_iter().flatten() {
                    let (Some(a), Some(b)) = (places.get(&n), places.get(m)) else {
                        continue;
                    };
                    let d = dist[&n] + a.0.truncate().distance(b.0.truncate());
                    if dist.get(m).is_none_or(|old| d < *old) {
                        dist.insert(*m, d);
                        prev.insert(*m, n);
                        if !open.contains(m) {
                            open.push(*m);
                        }
                    }
                }
            }
            if !dist.contains_key(&stop) {
                return Err("AMR 경로가 그 정지 위치까지 이어져 있지 않습니다".into());
            }
            let mut path = vec![stop];
            while let Some(p) = prev.get(path.last().unwrap()) {
                path.push(*p);
            }
            path.reverse();
            path
        };
        let mut at = from;
        for n in path {
            let p = places[&n].0;
            let way = p - at;
            if way.truncate().length() > 1e-3 {
                legs.push(Leg::Turn(way.y.atan2(way.x)));
            }
            legs.push(Leg::Via(n));
            at = p;
        }
        legs.push(Leg::Turn(facing));
        let _ = to;
        Ok(legs)
    }

    /// The stop a flow component is served at: the nearest, within reach.
    fn stop_near(&self, doc: &Document, object: u64) -> Option<u64> {
        let c = self.components.iter().find(|c| c.object == object)?;
        let at = Self::dock(doc, c)?.0.truncate();
        self.stop_places(doc)
            .into_iter()
            .map(|(n, (p, _))| (n, p.truncate().distance(at)))
            .filter(|(_, d)| *d <= NEAR)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(n, _)| n)
    }

    /// An AMR free for a job: one not driving, not on a job, not following
    /// a real one.
    fn free_amr(&self) -> Option<u64> {
        self.components
            .iter()
            .filter(|c| c.kind == Kind::Amr && c.mirror.is_none())
            .map(|c| c.object)
            .find(|a| {
                !self.jobs.iter().any(|j| j.amr == Some(*a))
                    && self.rovers.get(a).is_none_or(|r| r.legs.is_empty())
            })
    }

    /// Sends AMR `amr` to stop `stop` along the lanes.
    fn send(&mut self, doc: &Document, amr: u64, stop: u64) -> Result<(), String> {
        let c = self
            .components
            .iter()
            .find(|c| c.object == amr)
            .cloned()
            .ok_or("no such AMR")?;
        let mut r = self.rover(doc, &c).ok_or("no such AMR")?;
        r.legs = self.route(doc, r.at, stop)?;
        self.rovers.insert(amr, r);
        Ok(())
    }

    /// Moves the AMRs' jobs on: an AMR given to each waiting part, sent,
    /// loading and unloading timed, the part carried on its top plate and
    /// handed over where it goes. Returns whether anything happened.
    pub(super) fn carry_on(&mut self, doc: &Document, now: f64) -> bool {
        let mut happened = false;
        let decks: BTreeMap<u64, DAffine3> = self
            .decks(doc)
            .into_iter()
            .map(|d| {
                (
                    d.object,
                    d.pose * DAffine3::from_translation(DVec3::new(0.0, 0.0, d.hi.z)),
                )
            })
            .collect();
        for k in 0..self.jobs.len() {
            let job = self.jobs[k].clone();
            let idle = |cell: &Cell, a: u64| cell.rovers.get(&a).is_none_or(|r| r.legs.is_empty());
            let next = match job.phase {
                Phase::Waiting => match self.free_amr() {
                    Some(amr) => {
                        let Some(stop) = self.stop_near(doc, job.from) else {
                            self.flow_problem = Some(
                                "AMR로 보내는 공정 가까이(4 m)에 AMR 정지 위치가 없습니다".into(),
                            );
                            continue;
                        };
                        match self.send(doc, amr, stop) {
                            Ok(()) => {
                                self.jobs[k].amr = Some(amr);
                                Phase::Fetching
                            }
                            Err(e) => {
                                self.flow_problem = Some(e);
                                continue;
                            }
                        }
                    }
                    None => continue,
                },
                Phase::Fetching if idle(self, job.amr.unwrap()) => Phase::Loading(now + LOAD),
                Phase::Loading(until) if now >= until => {
                    if let Some(i) = self.part_index(job.part) {
                        self.parts[i].flow = Some(Place::Riding {
                            amr: job.amr.unwrap(),
                            to: job.to,
                        });
                    }
                    let Some(stop) = self.stop_near(doc, job.to) else {
                        self.flow_problem =
                            Some("부품을 받을 공정 가까이(4 m)에 AMR 정지 위치가 없습니다".into());
                        continue;
                    };
                    if let Err(e) = self.send(doc, job.amr.unwrap(), stop) {
                        self.flow_problem = Some(e);
                        continue;
                    }
                    Phase::Carrying
                }
                Phase::Carrying if idle(self, job.amr.unwrap()) => Phase::Unloading(now + LOAD),
                Phase::Unloading(until) if now >= until => {
                    // Handed over: arrived where it goes, as a part carried
                    // straight there would.
                    if let Some(i) = self.part_index(job.part) {
                        self.parts[i].flow = Some(Place::Moving {
                            from: self.parts[i].world.translation,
                            to: job.to,
                            start: now,
                            end: now,
                        });
                    }
                    self.jobs[k].amr = None;
                    self.jobs[k].phase = Phase::Waiting;
                    self.jobs[k].part = 0;
                    happened = true;
                    continue;
                }
                _ => continue,
            };
            self.jobs[k].phase = next;
            happened = true;
        }
        self.jobs.retain(|j| j.part != 0);
        // Parts on board where the AMR's top plate now is.
        for p in &mut self.parts {
            if let Some(Place::Riding { amr, .. }) = &p.flow {
                if let Some(top) = decks.get(amr) {
                    let half = p.size().1 / 2.0;
                    p.world = DAffine3::from_translation(top.transform_point3(DVec3::Z * half));
                }
            }
        }
        happened
    }
}

#[cfg(test)]
mod tests;
