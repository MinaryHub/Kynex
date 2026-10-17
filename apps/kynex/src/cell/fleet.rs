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

/// Seconds for a person to pick a part up, or put it down.
pub const HAND: f64 = 2.0;

/// How far in front of what they work at a person stands, metres.
const REACH: f64 = 0.6;

/// How far from a flow component the stop it is served at may be, metres.
const NEAR: f64 = 4.0;

/// A part to carry, and how far it has got.
#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub part: u64,
    pub from: u64,
    pub to: u64,
    /// The AMR, or the person, carrying it.
    pub amr: Option<u64>,
    pub phase: Phase,
    /// How: by AMR (1) or by hand (2), as the sender says.
    pub by: u32,
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

    /// Where a person stands to work at, or hand a part to, component
    /// `object`: in front of it, on the floor.
    fn spot(doc: &Document, c: &Component) -> Option<DVec3> {
        let (top, _, across) = Self::dock(doc, c)?;
        let id = id_of(doc, c.object)?;
        let depth = doc
            .objects()
            .filter(|(o, _)| *o == id || is_below(doc, *o, id))
            .filter_map(|(o, _)| bounds(doc, o))
            .map(|(lo, hi)| (hi - lo).dot(across).abs())
            .fold(0.0, f64::max);
        let front = top - across * (depth / 2.0 + REACH);
        Some(DVec3::new(front.x, front.y, 0.0))
    }

    /// A person free: not walking, not working at a station, not carrying.
    fn free_operators(&self) -> Vec<u64> {
        self.components
            .iter()
            .filter(|c| c.kind == Kind::Operator)
            .map(|c| c.object)
            .filter(|o| {
                !self.jobs.iter().any(|j| j.amr == Some(*o))
                    && !self.stations.values().any(|s| s.crew == Some(*o))
                    && self.rovers.get(o).is_none_or(|r| r.legs.is_empty())
            })
            .collect()
    }

    /// Person `who` walks to `to`, then faces `toward`.
    fn walk(&mut self, doc: &Document, who: u64, to: DVec3, toward: DVec3) -> Option<()> {
        let c = self.components.iter().find(|c| c.object == who)?.clone();
        let mut r = self.rover(doc, &c)?;
        let way = to - r.at;
        r.legs.clear();
        if way.truncate().length() > 1e-3 {
            r.legs.push(Leg::Turn(way.y.atan2(way.x)));
            r.legs.push(Leg::Walk(to));
        }
        let face = toward - to;
        if face.truncate().length() > 1e-3 {
            r.legs.push(Leg::Turn(face.y.atan2(face.x)));
        }
        self.rovers.insert(who, r);
        Some(())
    }

    /// The free person nearest component `object` sent to work at it; who,
    /// if anyone was free.
    pub(super) fn call_operator(&mut self, doc: &Document, object: u64) -> Option<u64> {
        let c = self.components.iter().find(|c| c.object == object)?.clone();
        let spot = Self::spot(doc, &c)?;
        let who = self
            .free_operators()
            .into_iter()
            .filter_map(|o| {
                let oc = self.components.iter().find(|x| x.object == o)?;
                Some((
                    o,
                    self.rover(doc, oc)?.at.truncate().distance(spot.truncate()),
                ))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))?
            .0;
        let toward = Self::dock(doc, &c)?.0;
        self.walk(doc, who, spot, toward)?;
        Some(who)
    }

    /// Sends person `who` to hand component `object` a part, or take one.
    fn walk_to(&mut self, doc: &Document, who: u64, object: u64) -> Result<(), String> {
        let c = self
            .components
            .iter()
            .find(|c| c.object == object)
            .cloned()
            .ok_or("no such component")?;
        let spot = Self::spot(doc, &c).ok_or("no place to stand")?;
        let toward = Self::dock(doc, &c).ok_or("no place to stand")?.0;
        self.walk(doc, who, spot, toward).ok_or("no such person")?;
        Ok(())
    }

    /// The parts waiting for a robot to carry them: each one's identity,
    /// where it is, and where it is to be put (the top of what it goes to,
    /// as it would stand there).
    pub fn robot_jobs(&self, doc: &Document) -> Vec<(u64, DVec3, DVec3)> {
        self.jobs
            .iter()
            .filter(|j| j.by == 3 && j.amr.is_none() && j.part != 0)
            .filter_map(|j| {
                let p = self.parts.iter().find(|p| p.uid == j.part)?;
                let to = self.components.iter().find(|c| c.object == j.to)?;
                let (top, ..) = Self::dock(doc, to)?;
                Some((
                    j.part,
                    p.world.translation,
                    top + DVec3::Z * (p.size().1 / 2.0 + 0.002),
                ))
            })
            .collect()
    }

    /// Part `part`'s job taken by robot `robot`: the part its to carry.
    pub fn robot_take(&mut self, part: u64, robot: u64) {
        if let Some(j) = self.jobs.iter_mut().find(|j| j.part == part) {
            j.amr = Some(robot);
            j.phase = Phase::Carrying;
            let to = j.to;
            if let Some(i) = self.part_index(part) {
                self.parts[i].flow = Some(Place::Riding { amr: robot, to });
            }
        }
    }

    /// Part `part` put down by its robot: handed over where it goes; or,
    /// the robot stopped short (`failed`), put back to wait for another.
    pub fn robot_done(&mut self, part: u64, failed: Option<String>) {
        let now = self.last_tick.unwrap_or(0.0);
        let Some(k) = self.jobs.iter().position(|j| j.part == part) else {
            return;
        };
        let job = self.jobs[k].clone();
        let Some(i) = self.part_index(part) else {
            self.jobs.remove(k);
            return;
        };
        self.parts[i].held = None;
        match failed {
            None => {
                self.parts[i].flow = Some(Place::Moving {
                    from: self.parts[i].world.translation,
                    to: job.to,
                    start: now,
                    end: now,
                });
                self.jobs.remove(k);
            }
            Some(why) => {
                self.parts[i].flow = Some(Place::Waiting {
                    at: job.from,
                    to: job.to,
                });
                self.jobs[k].amr = None;
                self.jobs[k].phase = Phase::Waiting;
                self.flow_problem = Some(why);
            }
        }
        self.changed = true;
    }

    /// Says why the flow cannot go on as it should.
    pub fn flow_trouble(&mut self, why: String) {
        if self.flow_problem.as_ref() != Some(&why) {
            self.flow_problem = Some(why);
            self.changed = true;
        }
    }

    /// What each person is doing, for the timeline: walking, working (at a
    /// station at work, or handling a part), or idle.
    pub(super) fn operator_activities(&self) -> Vec<(u64, String, &'static str)> {
        self.components
            .iter()
            .filter(|c| c.kind == Kind::Operator)
            .map(|c| {
                let o = c.object;
                let walking = self.rovers.get(&o).is_some_and(|r| !r.legs.is_empty());
                let working = self
                    .stations
                    .values()
                    .any(|s| s.crew == Some(o) && s.working.is_some())
                    || self.jobs.iter().any(|j| {
                        j.amr == Some(o)
                            && matches!(j.phase, Phase::Loading(_) | Phase::Unloading(_))
                    });
                let a = if walking {
                    "walk"
                } else if working {
                    "work"
                } else {
                    "idle"
                };
                (o, c.name.clone(), a)
            })
            .collect()
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
            let hand = job.by == 2;
            // A robot's job is the view's, which runs robots.
            if job.by == 3 {
                continue;
            }
            let next = match job.phase {
                // By hand: the free person nearest the sender.
                Phase::Waiting if hand => {
                    let near = self
                        .components
                        .iter()
                        .find(|c| c.object == job.from)
                        .cloned();
                    let Some(spot) = near.as_ref().and_then(|c| Self::spot(doc, c)) else {
                        continue;
                    };
                    let who = self
                        .free_operators()
                        .into_iter()
                        .filter_map(|o| {
                            let oc = self.components.iter().find(|x| x.object == o)?;
                            Some((
                                o,
                                self.rover(doc, oc)?.at.truncate().distance(spot.truncate()),
                            ))
                        })
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                        .map(|(o, _)| o);
                    let Some(who) = who else {
                        if !self.components.iter().any(|c| c.kind == Kind::Operator) {
                            self.flow_problem =
                                Some("작업자가 나르게 했지만 작업자 역할이 없습니다".into());
                        }
                        continue;
                    };
                    if let Err(e) = self.walk_to(doc, who, job.from) {
                        self.flow_problem = Some(e);
                        continue;
                    }
                    self.jobs[k].amr = Some(who);
                    self.batch(k, who);
                    Phase::Fetching
                }
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
                                self.batch(k, amr);
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
                Phase::Fetching if idle(self, job.amr.unwrap()) => {
                    Phase::Loading(now + if hand { HAND } else { LOAD })
                }
                Phase::Loading(until) if now >= until => {
                    if let Some(i) = self.part_index(job.part) {
                        self.parts[i].flow = Some(Place::Riding {
                            amr: job.amr.unwrap(),
                            to: job.to,
                        });
                    }
                    let sent = if hand {
                        self.walk_to(doc, job.amr.unwrap(), job.to)
                    } else {
                        match self.stop_near(doc, job.to) {
                            Some(stop) => self.send(doc, job.amr.unwrap(), stop),
                            None => {
                                Err("부품을 받을 공정 가까이(4 m)에 AMR 정지 위치가 없습니다"
                                    .into())
                            }
                        }
                    };
                    if let Err(e) = sent {
                        self.flow_problem = Some(e);
                        continue;
                    }
                    Phase::Carrying
                }
                Phase::Carrying if idle(self, job.amr.unwrap()) => {
                    Phase::Unloading(now + if hand { HAND } else { LOAD })
                }
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
        // Parts on board where the AMR's top plate now is; carried by hand,
        // before the person, at waist height.
        let people: BTreeMap<u64, DAffine3> = self
            .components
            .iter()
            .filter(|c| c.kind == Kind::Operator)
            .filter_map(|c| {
                let r = self.rover(doc, c)?;
                Some((c.object, pose_of(r.at, r.yaw)))
            })
            .collect();
        // Several on board: side by side.
        let mut seat: BTreeMap<u64, f64> = BTreeMap::new();
        for p in &mut self.parts {
            if let Some(Place::Riding { amr, .. }) = &p.flow {
                let half = p.size().1 / 2.0;
                let k = seat.entry(*amr).or_insert(0.0);
                let side = *k * (2.0 * RAW.0 + 0.01);
                *k += 1.0;
                if let Some(top) = decks.get(amr) {
                    p.world = DAffine3::from_translation(top.transform_point3(DVec3::new(
                        side - 0.1,
                        0.0,
                        half,
                    )));
                } else if let Some(pose) = people.get(amr) {
                    p.world = DAffine3::from_translation(pose.transform_point3(DVec3::new(
                        0.35,
                        side - 0.05,
                        1.0 + half,
                    )));
                }
            }
        }
        happened
    }

    /// Job `k` given to `carrier`: as many more waiting the same way — from
    /// the same place to the same place, the same way carried — as it
    /// carries at once (its 적재량), loaded and carried with it.
    fn batch(&mut self, k: usize, carrier: u64) {
        let most = self
            .components
            .iter()
            .find(|c| c.object == carrier)
            .map_or(1, |c| c.props.capacity.max(1)) as usize;
        let (from, to, by) = (self.jobs[k].from, self.jobs[k].to, self.jobs[k].by);
        let mut taken = 1;
        for j in 0..self.jobs.len() {
            if taken >= most {
                break;
            }
            let other = &mut self.jobs[j];
            // A job just done waits only to be cleared away: not one to take.
            if j != k
                && other.part != 0
                && other.amr.is_none()
                && other.phase == Phase::Waiting
                && (other.from, other.to, other.by) == (from, to, by)
            {
                other.amr = Some(carrier);
                other.phase = Phase::Fetching;
                taken += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests;
