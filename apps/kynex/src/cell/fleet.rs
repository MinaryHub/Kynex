//! AMRs as a fleet: a network of paths between their stops, the shortest
//! way along it, traffic kept apart, and parts of a process flow carried
//! from station to station.
//!
//! Stops linked (`Cell::lanes`, both ways, or one way when asked) make
//! the network an AMR drives on: sent to a stop, it goes to the stop of the network nearest it, then
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

/// The share of a full charge under which an AMR takes no more jobs and,
/// once free, goes to charge.
const LOW: f64 = 0.2;

/// A part to carry, and how far it has got.
/// A part for a robot to carry: the part, where it is, where it goes, the
/// link's two ends, and the robot the link names (none: any).
pub type RobotJob = (u64, DVec3, DVec3, [u64; 2], Option<u64>);

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
    /// By a robot: the one the link names, or any (none).
    pub robot: Option<u64>,
    /// By a robot (3): whether its gripper has taken the part yet. Until
    /// then the part keeps its place where it waits; after, it goes with
    /// the gripper, and once let go, on to where it goes.
    pub gripped: bool,
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
    /// Links stop `object` to the stops `next`, both ways — or, `oneway`,
    /// from it to them only, as a loop driven one way round, where two
    /// AMRs never meet head on: the lanes AMRs drive along.
    pub fn set_lanes(&mut self, object: u64, next: Vec<u64>, oneway: bool) -> Result<(), String> {
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
        if oneway {
            // From it only: the ways into it are the other stops' own.
            self.lanes.insert(object, next);
            self.lanes.retain(|_, v| !v.is_empty());
            self.roles_changed = true;
            self.changed = true;
            return Ok(());
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
        self.route_avoiding(doc, from, stop, None)
    }

    /// The same, not through stop `avoid`.
    fn route_avoiding(
        &self,
        doc: &Document,
        from: DVec3,
        stop: u64,
        avoid: Option<u64>,
    ) -> Result<Vec<Leg>, String> {
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
                for m in self
                    .lanes
                    .get(&n)
                    .into_iter()
                    .flatten()
                    .filter(|m| Some(**m) != avoid)
                {
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
        // The docks worked out for the scene as it is, if they were.
        let seen = self
            .docks_seen
            .as_ref()
            .filter(|s| s.version == doc.version())
            .and_then(|s| s.docks.get(&object))
            .map(|d| d.0);
        let at = seen.or_else(|| Self::dock(doc, c).map(|d| d.0))?.truncate();
        self.stop_places(doc)
            .into_iter()
            .map(|(n, (p, _))| (n, p.truncate().distance(at)))
            .filter(|(_, d)| *d <= NEAR)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(n, _)| n)
    }

    /// Whether any AMR is free for a job (see `free_amr`).
    fn any_free_amr(&self) -> bool {
        self.components.iter().any(|c| self.free(c))
    }

    /// Whether AMR `c` is free for a job (see `free_amr`).
    fn free(&self, c: &Component) -> bool {
        c.kind == Kind::Amr
            && c.mirror.is_none()
            && (c.props.battery <= 0.0 || self.energy.get(&c.object).is_none_or(|e| *e >= LOW))
            && !self.jobs.iter().any(|j| j.amr == Some(c.object))
            && !self.charging.contains(&c.object)
            && (self.flow_on || self.rovers.get(&c.object).is_none_or(|r| r.legs.is_empty()))
    }

    /// How far it is from `from` to each stop along the lanes: to the stop
    /// of the network nearest, then the shortest way; straight to a stop
    /// off the network, or to every stop if there are no lanes.
    fn lane_distances(
        &self,
        places: &BTreeMap<u64, (DVec3, f64)>,
        from: DVec3,
    ) -> BTreeMap<u64, f64> {
        let from = from.truncate();
        let straight = |n: &u64| {
            places
                .get(n)
                .map_or(f64::INFINITY, |p| p.0.truncate().distance(from))
        };
        let mut dist: BTreeMap<u64, f64> = places.keys().map(|n| (*n, straight(n))).collect();
        let start = places
            .keys()
            .filter(|n| self.lanes.contains_key(*n))
            .min_by(|a, b| straight(a).total_cmp(&straight(b)))
            .copied();
        let Some(start) = start else {
            return dist;
        };
        // Dijkstra over the network, the graph being small.
        let mut lane: BTreeMap<u64, f64> = BTreeMap::from([(start, straight(&start))]);
        let mut open = vec![start];
        while let Some(k) = open
            .iter()
            .enumerate()
            .min_by(|a, b| lane[a.1].total_cmp(&lane[b.1]))
            .map(|(k, _)| k)
        {
            let n = open.swap_remove(k);
            for m in self.lanes.get(&n).into_iter().flatten() {
                let (Some(a), Some(b)) = (places.get(&n), places.get(m)) else {
                    continue;
                };
                let d = lane[&n] + a.0.truncate().distance(b.0.truncate());
                if lane.get(m).is_none_or(|old| d < *old) {
                    lane.insert(*m, d);
                    if !open.contains(m) {
                        open.push(*m);
                    }
                }
            }
        }
        // On the network, only along it: a stop it does not reach, never.
        for (n, d) in dist.iter_mut() {
            if self.lanes.contains_key(n) || self.lanes.values().any(|l| l.contains(n)) {
                *d = lane.get(n).copied().unwrap_or(f64::INFINITY);
            }
        }
        dist
    }

    /// Gives the parts waiting for an AMR to the AMRs free, as a fleet
    /// manager does: the nearest pair first — the AMR that gets soonest,
    /// along the lanes, to where a part waits — then the next nearest, and
    /// so on. Not job by job in turn, which sends an AMR just free past a
    /// part beside it to one across the floor. Returns whether any went.
    fn dispatch(&mut self, doc: &Document) -> bool {
        let places = self.stop_places(doc);
        let mut free: Vec<(u64, BTreeMap<u64, f64>)> = self
            .components
            .iter()
            .filter(|c| self.free(c))
            .filter_map(|c| {
                Some((
                    c.object,
                    self.lane_distances(&places, self.rover(doc, c)?.at),
                ))
            })
            .collect();
        let mut waiting: Vec<(usize, u64)> = self
            .jobs
            .iter()
            .enumerate()
            .filter(|(_, j)| {
                j.by == 1 && j.part != 0 && j.amr.is_none() && j.phase == Phase::Waiting
            })
            .filter_map(|(k, j)| Some((k, self.stop_near(doc, j.from)?)))
            .collect();
        let mut changed = false;
        while !free.is_empty() && !waiting.is_empty() {
            let best = free
                .iter()
                .enumerate()
                .flat_map(|(f, (_, d))| {
                    waiting.iter().enumerate().map(move |(w, (_, stop))| {
                        (f, w, d.get(stop).copied().unwrap_or(f64::INFINITY))
                    })
                })
                .filter(|(.., d)| d.is_finite())
                .min_by(|a, b| a.2.total_cmp(&b.2));
            let Some((f, w, _)) = best else {
                break;
            };
            let (amr, _) = free.swap_remove(f);
            let (k, stop) = waiting.swap_remove(w);
            match self.send(doc, amr, stop) {
                Ok(()) => {
                    self.jobs[k].amr = Some(amr);
                    self.jobs[k].phase = Phase::Fetching;
                    self.batch(k, amr);
                    changed = true;
                    // Those it took with it are not waiting any more.
                    let jobs = &self.jobs;
                    waiting.retain(|(j, _)| jobs[*j].amr.is_none());
                }
                Err(e) => self.flow_problem = Some(e),
            }
        }
        changed
    }

    /// The stops an AMR with nothing to do parks at: those at the end of a
    /// lane — linked to one stop only, a spur off the way — that are no
    /// charger and no flow component's stop. None if the lanes have none:
    /// then an idle AMR stays where it is.
    fn parking(&self, doc: &Document) -> Vec<u64> {
        let served: HashSet<u64> = self
            .components
            .iter()
            .filter(|c| c.kind.flow())
            .filter_map(|c| self.stop_near(doc, c.object))
            .collect();
        self.components
            .iter()
            .filter(|c| c.kind == Kind::Stop && c.props.charger == 0)
            .map(|c| c.object)
            .filter(|s| self.lanes.get(s).is_some_and(|l| l.len() == 1))
            .filter(|s| !served.contains(s))
            .collect()
    }

    /// Whether AMR `amr` stands with nothing to do: not driving, on no
    /// job, not charging, not following a real one.
    fn idle_amr(&self, amr: u64) -> bool {
        self.components
            .iter()
            .any(|c| c.object == amr && c.kind == Kind::Amr && c.mirror.is_none())
            && !self.jobs.iter().any(|j| j.amr == Some(amr))
            && !self.charging.contains(&amr)
            && self.rovers.get(&amr).is_none_or(|r| r.legs.is_empty())
    }

    /// Looks after the fleet each step: an AMR low on charge (under
    /// [`LOW`]) and free goes to the nearest charger and charges there until
    /// nearly full; and two AMRs each waiting for the stop the other holds
    /// are got apart — one goes round another way if there is one, else
    /// it is said. Returns whether anything changed.
    pub(super) fn fleet_care(&mut self, doc: &Document, dt: f64) -> bool {
        let mut changed = false;
        let places = self.stop_places(doc);
        let chargers: Vec<u64> = self
            .components
            .iter()
            .filter(|c| c.kind == Kind::Stop && c.props.charger == 1)
            .map(|c| c.object)
            .collect();
        let amrs: Vec<Component> = self
            .components
            .iter()
            .filter(|c| c.kind == Kind::Amr && c.props.battery > 0.0 && c.mirror.is_none())
            .cloned()
            .collect();
        for c in amrs {
            let Some(r) = self.rover(doc, &c) else {
                continue;
            };
            let idle = r.legs.is_empty();
            let busy = self.jobs.iter().any(|j| j.amr == Some(c.object));
            let e = *self.energy.entry(c.object).or_insert(1.0);
            let at_charger = chargers.iter().find(|s| {
                places
                    .get(s)
                    .is_some_and(|(p, _)| p.truncate().distance(r.at.truncate()) < 0.05)
            });
            if idle && at_charger.is_some() && self.charging.contains(&c.object) {
                let e = self.energy.entry(c.object).or_insert(1.0);
                *e = (*e + dt / (c.props.charge * 60.0)).min(1.0);
                if *e >= 0.95 {
                    self.charging.remove(&c.object);
                    changed = true;
                }
                continue;
            }
            if e < LOW && idle && !busy && !self.charging.contains(&c.object) {
                let nearest = chargers
                    .iter()
                    .filter_map(|s| {
                        Some((*s, places.get(s)?.0.truncate().distance(r.at.truncate())))
                    })
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(s, _)| s);
                match nearest {
                    Some(s) => {
                        if self.send(doc, c.object, s).is_ok() {
                            self.charging.insert(c.object);
                            changed = true;
                        }
                    }
                    None => {
                        self.flow_problem = Some(format!(
                            "{}: 배터리가 떨어져 가는데 충전소가 없습니다",
                            c.name
                        ))
                    }
                }
            }
        }
        // In a running flow, an AMR with nothing to do goes to park, out of
        // the others' way: to the nearest free parking stop, if the lanes
        // have any (see `parking`).
        if self.flow_on {
            let idle: Vec<Component> = self
                .components
                .iter()
                .filter(|c| c.kind == Kind::Amr && self.idle_amr(c.object))
                .cloned()
                .collect();
            let parks = if idle.is_empty() {
                Vec::new()
            } else {
                self.parking(doc)
            };
            for c in idle.iter().filter(|_| !parks.is_empty()) {
                let Some(r) = self.rover(doc, c) else {
                    continue;
                };
                let near = |s: &u64| {
                    places.get(s).map_or(f64::INFINITY, |(p, _)| {
                        p.truncate().distance(r.at.truncate())
                    })
                };
                if parks.iter().any(|s| near(s) < 0.05) {
                    continue;
                }
                // Free: no one there, or on the way there.
                let taken = |s: &u64| {
                    self.holds.contains_key(s)
                        || self.rovers.iter().any(|(a, o)| {
                            *a != c.object
                                && o.legs.iter().rev().find_map(|l| match l {
                                    Leg::Via(n) => Some(*n),
                                    _ => None,
                                }) == Some(*s)
                        })
                };
                let park = parks
                    .iter()
                    .filter(|s| !taken(s))
                    .min_by(|a, b| near(a).total_cmp(&near(b)))
                    .copied();
                if let Some(s) = park {
                    if self.send(doc, c.object, s).is_ok() {
                        changed = true;
                    }
                }
            }
        }
        // In a running flow, one waiting on a stop where an AMR stands with
        // nothing to do: that one goes aside, to a free stop next to it —
        // off the waiting one's way if it can. (Sent there by hand, out of
        // a flow, it stays: it was put there.)
        if self.flow_on {
            let waits: Vec<(u64, u64)> = self.blocked.iter().map(|(a, s)| (*a, *s)).collect();
            for (a, s) in waits {
                let Some(&b) = self.holds.get(&s) else {
                    continue;
                };
                if b == a || !self.idle_amr(b) {
                    continue;
                }
                let ahead: Vec<u64> = self.rovers.get(&a).map_or(Vec::new(), |r| {
                    r.legs
                        .iter()
                        .filter_map(|l| match l {
                            Leg::Via(n) => Some(*n),
                            _ => None,
                        })
                        .collect()
                });
                let aside = self
                    .lanes
                    .get(&s)
                    .into_iter()
                    .flatten()
                    .copied()
                    .filter(|n| !self.holds.contains_key(n))
                    .min_by_key(|n| ahead.contains(n));
                if let Some(n) = aside {
                    if self.send(doc, b, n).is_ok() {
                        changed = true;
                    }
                }
            }
        }
        // Two waiting on each other.
        let waits: Vec<(u64, u64)> = self.blocked.iter().map(|(a, s)| (*a, *s)).collect();
        let mut stuck: Vec<(u64, u64)> = Vec::new();
        for (a, s) in waits {
            let Some(&b) = self.holds.get(&s) else {
                continue;
            };
            let Some(&t) = self.blocked.get(&b) else {
                continue;
            };
            if self.holds.get(&t) != Some(&a) || !self.blocked.contains_key(&a) {
                continue;
            }
            // Either of the two goes round, or aside, whichever can; said
            // only if neither can.
            let Some(c) = self.components.iter().find(|c| c.object == a).cloned() else {
                continue;
            };
            let Some(mut r) = self.rover(doc, &c) else {
                continue;
            };
            let Some(dest) = r.legs.iter().rev().find_map(|l| match l {
                Leg::Via(n) => Some(*n),
                _ => None,
            }) else {
                continue;
            };
            // Round another way; or, if it is going to that very stop, aside
            // to a free stop next to it first, to let the other by.
            let round = if dest != s {
                self.route_avoiding(doc, r.at, dest, Some(s)).ok()
            } else {
                let here = self
                    .holds
                    .iter()
                    .find(|(stop, who)| **who == a && **stop != s)
                    .map(|(stop, _)| *stop);
                let aside = here.and_then(|h| {
                    self.lanes
                        .get(&h)?
                        .iter()
                        .copied()
                        .find(|n| *n != s && !self.holds.contains_key(n))
                });
                aside.and_then(|n| {
                    let mut legs = self.route(doc, r.at, n).ok()?;
                    let there = places.get(&n)?.0;
                    legs.extend(
                        self.route(doc, DVec3::new(there.x, there.y, r.at.z), dest)
                            .ok()?,
                    );
                    Some(legs)
                })
            };
            match round {
                Some(legs) => {
                    r.legs = legs;
                    self.rovers.insert(a, r);
                    self.blocked.remove(&a);
                    self.blocked.remove(&b);
                    stuck.retain(|(x, y)| (*x, *y) != (b, a));
                    changed = true;
                }
                None if !stuck.contains(&(b, a)) && self.blocked.contains_key(&b) => {
                    // The other may yet get out of the way.
                    stuck.push((a, b));
                }
                None => {
                    let name = |u: u64| {
                        self.components
                            .iter()
                            .find(|c| c.object == u)
                            .map_or(String::new(), |c| c.name.clone())
                    };
                    self.flow_problem = Some(format!(
                        "AMR 교착: {}와 {}가 서로의 정지 위치를 기다립니다 (비켜설 길이 없음)",
                        name(a),
                        name(b)
                    ));
                }
            }
        }
        changed
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
        self.free_operators_for(None)
    }

    /// Those free to carry a part from `from` to `to`: one who works at
    /// certain stations only carries what comes from or goes to them — a
    /// checker at goods in does not walk across the floor to the packing
    /// benches; one who may work anywhere carries anything.
    fn free_carriers(&self, from: u64, to: u64) -> Vec<u64> {
        self.free_operators()
            .into_iter()
            .filter(|o| {
                self.skills
                    .get(o)
                    .is_none_or(|k| k.contains(&from) || k.contains(&to))
            })
            .collect()
    }

    /// The same, those who may work at station `at` if one is named, and
    /// on their shift now.
    fn free_operators_for(&self, at: Option<u64>) -> Vec<u64> {
        self.components
            .iter()
            .filter(|c| c.kind == Kind::Operator && self.on_shift(c))
            .map(|c| c.object)
            .filter(|o| {
                at.is_none_or(|s| self.skills.get(o).is_none_or(|k| k.contains(&s)))
                    && !self.jobs.iter().any(|j| j.amr == Some(*o))
                    && !self.stations.values().any(|s| s.crew == Some(*o))
                    && self.rovers.get(o).is_none_or(|r| r.legs.is_empty())
            })
            .collect()
    }

    /// Whether operator `c` is on shift now, the flow's clock read as hours
    /// of the day from its start.
    pub(super) fn on_shift(&self, c: &Component) -> bool {
        let (from, to) = (c.props.shift_from, c.props.shift_to);
        if from == to {
            return true;
        }
        let hour = (self.flow_clock() / 3600.0).rem_euclid(24.0);
        if from < to {
            hour >= from && hour < to
        } else {
            hour >= from || hour < to
        }
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
            .free_operators_for(Some(object))
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
    pub fn robot_jobs(&self, doc: &Document) -> Vec<RobotJob> {
        let belts = self.belts(doc);
        self.jobs
            .iter()
            .filter(|j| j.by == 3 && j.amr.is_none() && j.part != 0)
            .filter_map(|j| {
                let p = self.parts.iter().find(|p| p.uid == j.part)?;
                let to = self.components.iter().find(|c| c.object == j.to)?;
                // Onto a belt at its start, where the flow puts its parts:
                // not its middle, which may be out of the robot's reach.
                let top = match belts.iter().find(|b| b.object == j.to) {
                    Some(b) => b.centre + b.along * (-b.length / 2.0 + super::RAW.0),
                    None => Self::dock(doc, to)?.0,
                };
                Some((
                    j.part,
                    p.world.translation,
                    top + DVec3::Z * (p.size().1 / 2.0 + 0.002),
                    [j.from, j.to],
                    j.robot,
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

    /// Part `part`'s job for a robot that no robot can do — its place out
    /// of every one's reach —: given up. The part is back where it was, to
    /// go another way; that way left alone a while (see `cut_off`).
    pub fn robot_cannot(&mut self, part: u64, why: String) {
        let version = self.docks_seen.as_ref().map_or(0, |d| d.layout);
        let Some(k) = self
            .jobs
            .iter()
            .position(|j| j.part == part && j.amr.is_none())
        else {
            return;
        };
        let job = self.jobs.remove(k);
        let to = self
            .components
            .iter()
            .find(|c| c.object == job.to)
            .map(|c| c.name.clone())
            .unwrap_or_default();
        self.unreachable
            .insert((job.from, job.to), (version, why.clone()));
        let belt = self
            .components
            .iter()
            .any(|c| c.object == job.from && c.kind == Kind::Conveyor);
        if let Some(i) = self.part_index(part) {
            if belt {
                // Back on the belt, at its end: told again as there.
                self.parts[i].flow = Some(Place::Belt(job.from));
            } else {
                self.parts[i].flow = Some(Place::At(job.from));
                self.stations.entry(job.from).or_default().out.insert(0, part);
            }
        }
        self.flow_problem = Some(format!("{to}(으)로는 보내지 않습니다 — {why}"));
        self.changed = true;
    }

    /// Whether the link from `from` to `to` is left alone now: no robot
    /// could carry along it in the scene as it is.
    pub(super) fn cut_off(&self, from: u64, to: u64) -> bool {
        let version = self.docks_seen.as_ref().map_or(0, |d| d.layout);
        self.unreachable
            .get(&(from, to))
            .is_some_and(|(v, _)| *v == version)
    }

    /// Why no robot can bring parts to `to`, if none can now.
    pub(super) fn unreachable_to(&self, to: u64) -> Option<&str> {
        let version = self.docks_seen.as_ref().map_or(0, |d| d.layout);
        self.unreachable
            .iter()
            .find(|((_, t), (v, _))| *t == to && *v == version)
            .map(|(_, (_, why))| why.as_str())
    }

    /// Where a robot takes a part from on link `from` → `to` and where it
    /// puts it: off a belt's end, onto a belt's start, else each place's
    /// dock — for its reach to be worked out before any part goes.
    pub fn link_ends(&self, doc: &Document, from: u64, to: u64) -> Option<(DVec3, DVec3)> {
        let belts = self.belts(doc);
        let comp = |o: u64| self.components.iter().find(|c| c.object == o);
        let at = match belts.iter().find(|b| b.object == from) {
            Some(b) => b.centre + b.along * (b.length / 2.0 - super::RAW.0),
            None => Self::dock(doc, comp(from)?)?.0,
        };
        let put = match belts.iter().find(|b| b.object == to) {
            Some(b) => b.centre + b.along * (-b.length / 2.0 + super::RAW.0),
            None => Self::dock(doc, comp(to)?)?.0,
        };
        let up = DVec3::Z * (super::RAW.1 / 2.0 + 0.002);
        Some((at + up, put + up))
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
                } else if self.flow_on && !self.on_shift(c) {
                    "off"
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
        if self.any_free_amr() {
            happened |= self.dispatch(doc);
        }
        for k in 0..self.jobs.len() {
            let job = self.jobs[k].clone();
            let idle = |cell: &Cell, a: u64| cell.rovers.get(&a).is_none_or(|r| r.legs.is_empty());
            let hand = job.by == 2;
            // A robot's job is the view's, which runs robots.
            if job.by == 3 {
                continue;
            }
            let next = match job.phase {
                // By hand: the free person nearest the sender, of those who
                // may carry it.
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
                        .free_carriers(job.from, job.to)
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
                // By AMR: given out together, before this pass (`dispatch`);
                // here only said if there is no stop to fetch it from.
                Phase::Waiting => {
                    if self.stop_near(doc, job.from).is_none() {
                        self.flow_problem =
                            Some("AMR로 보내는 공정 가까이(4 m)에 AMR 정지 위치가 없습니다".into());
                    }
                    continue;
                }
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
