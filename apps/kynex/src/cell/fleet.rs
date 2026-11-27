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

/// How far in from its start a robot puts a part onto a belt, metres: its
/// gripper, wider than the part, clear of the drum at the belt's end.
pub const BELT_IN: f64 = 0.15;

/// Where on belt `b` a robot puts a part: [`BELT_IN`] from its start, or
/// its middle if it is shorter.
pub(super) fn belt_drop(b: &Belt) -> f64 {
    -b.length / 2.0 + BELT_IN.min(b.length / 2.0)
}

/// How many other stops a stop may have lanes with, either way: a
/// crossroads, four ways out of it.
pub const MOST_LANES: usize = 4;

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
    /// By AMR (1): the robot it hands the part to, the AMR stopping beside
    /// it; then, by that robot (3): the AMR standing there with the part on
    /// its top plate until the robot grips it.
    pub handoff: Option<u64>,
    pub docked: Option<u64>,
    /// When it began to wait for a carrier, on the flow's clock.
    pub since: f64,
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
        let mut lanes = self.lanes.clone();
        if !oneway {
            // Both ways: what was linked to it and is no more is unlinked too.
            for to in lanes.values_mut() {
                to.retain(|n| *n != object);
            }
            for n in &next {
                let back = lanes.entry(*n).or_default();
                if !back.contains(&object) {
                    back.push(object);
                }
            }
        }
        // One way: from it only, the ways into it the other stops' own.
        lanes.insert(object, next);
        self.keep_lanes(lanes)
    }

    /// The lanes `lanes` kept, unless a stop would have them with more
    /// than [`MOST_LANES`] others — more than it had: a project saved
    /// with more is read as it is, only kept from more —; then they stay
    /// as they were.
    fn keep_lanes(&mut self, mut lanes: BTreeMap<u64, Vec<u64>>) -> Result<(), String> {
        lanes.retain(|_, v| !v.is_empty());
        let others = |lanes: &BTreeMap<u64, Vec<u64>>| {
            let mut out: BTreeMap<u64, HashSet<u64>> = BTreeMap::new();
            for (a, to) in lanes {
                for b in to {
                    out.entry(*a).or_default().insert(*b);
                    out.entry(*b).or_default().insert(*a);
                }
            }
            out
        };
        let (before, after) = (others(&self.lanes), others(&lanes));
        let had = |stop: &u64| before.get(stop).map_or(0, HashSet::len);
        if let Some((stop, _)) = after
            .iter()
            .find(|(stop, o)| o.len() > MOST_LANES && o.len() > had(stop))
        {
            let name = self
                .components
                .iter()
                .find(|c| c.object == *stop)
                .map_or(String::new(), |c| c.name.clone());
            return Err(format!(
                "AMR 정지 위치 하나에는 길을 {MOST_LANES}곳까지만 낼 수 있습니다 ({name})"
            ));
        }
        self.lanes = lanes;
        self.roles_changed = true;
        self.changed = true;
        Ok(())
    }

    /// The lane between stops `a` and `b` set, the others left as they
    /// are: both ways, `a` to `b` only, `b` to `a` only, or none.
    pub fn set_lane(&mut self, a: u64, b: u64, way: &str) -> Result<(), String> {
        let is_stop = |u: u64| {
            self.components
                .iter()
                .any(|c| c.object == u && c.kind == Kind::Stop)
        };
        if !is_stop(a) || !is_stop(b) || a == b {
            return Err("다른 두 AMR 정지 위치 사이에만 길을 낼 수 있습니다".into());
        }
        let (ab, ba) = match way {
            "both" => (true, true),
            "to" => (true, false),
            "from" => (false, true),
            "none" => (false, false),
            _ => return Err("way: both, to, from, none".into()),
        };
        let mut lanes = self.lanes.clone();
        for (x, y, on) in [(a, b, ab), (b, a, ba)] {
            let next = lanes.entry(x).or_default();
            next.retain(|n| *n != y);
            if on {
                next.push(y);
            }
        }
        self.keep_lanes(lanes)
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

    /// The stop a flow component is served at: the nearest of `places`
    /// (`stop_places`), within reach. The places are worked out once by the
    /// caller, which asks for many: each is a walk up the scene, and asked
    /// anew for each part they were most of a run's time.
    fn stop_near_in(
        &self,
        doc: &Document,
        places: &BTreeMap<u64, (DVec3, f64)>,
        object: u64,
    ) -> Option<u64> {
        let c = self.components.iter().find(|c| c.object == object)?;
        // The docks worked out for the scene as it is, if they were.
        let seen = self
            .docks_seen
            .as_ref()
            .filter(|s| s.version == doc.version())
            .and_then(|s| s.docks.get(&object))
            .map(|d| d.0);
        let at = seen.or_else(|| Self::dock(doc, c).map(|d| d.0))?;
        Self::nearest_stop(places, at)
    }

    /// The stop of `places` nearest point `at`, within reach of it, if there
    /// is one.
    fn nearest_stop(places: &BTreeMap<u64, (DVec3, f64)>, at: DVec3) -> Option<u64> {
        let at = at.truncate();
        places
            .iter()
            .map(|(n, (p, _))| (*n, p.truncate().distance(at)))
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
            && !self
                .jobs
                .iter()
                .any(|j| j.amr == Some(c.object) || j.docked == Some(c.object))
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
        let mut waiting: Vec<(usize, u64)> = self
            .jobs
            .iter()
            .enumerate()
            .filter(|(_, j)| {
                j.by == 1 && j.part != 0 && j.amr.is_none() && j.phase == Phase::Waiting
            })
            .filter_map(|(k, j)| Some((k, self.stop_near_in(doc, &places, j.from)?)))
            .collect();
        // Nothing waits: no AMR's ways need working out.
        if waiting.is_empty() {
            return false;
        }
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
        // A link that names its AMR is that AMR's alone.
        let named: Vec<Option<u64>> = self.jobs.iter().map(|j| j.robot).collect();
        // An AMR sets out with a full load (its 적재량), or once the first
        // part has waited its 모아 싣기 time: not with each part as it comes.
        let now = self.last_tick.unwrap_or(0.0);
        let ready = |cell: &Cell, amr: u64, k: usize| {
            let Some(c) = cell.components.iter().find(|c| c.object == amr) else {
                return true;
            };
            let most = c.props.capacity.max(1) as usize;
            let j = &cell.jobs[k];
            let same: Vec<&Job> = cell
                .jobs
                .iter()
                .filter(|o| o.part != 0 && o.amr.is_none() && o.phase == Phase::Waiting)
                .filter(|o| (o.from, o.to, o.by) == (j.from, j.to, j.by))
                .collect();
            let first = same.iter().map(|o| o.since).fold(f64::INFINITY, f64::min);
            most <= 1 || same.len() >= most || now - first >= c.props.gather
        };
        let mut changed = false;
        while !free.is_empty() && !waiting.is_empty() {
            let cell: &Cell = self;
            let best = free
                .iter()
                .enumerate()
                .flat_map(|(f, (amr, d))| {
                    let named = &named;
                    let ready = &ready;
                    waiting.iter().enumerate().map(move |(w, (k, stop))| {
                        let theirs = named[*k].is_none_or(|r| r == *amr) && ready(cell, *amr, *k);
                        (
                            f,
                            w,
                            if theirs {
                                d.get(stop).copied().unwrap_or(f64::INFINITY)
                            } else {
                                f64::INFINITY
                            },
                        )
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
        let places = self.stop_places(doc);
        let served: HashSet<u64> = self
            .components
            .iter()
            .filter(|c| c.kind.flow())
            .filter_map(|c| self.stop_near_in(doc, &places, c.object))
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
            && !self
                .jobs
                .iter()
                .any(|j| j.amr == Some(amr) || j.docked == Some(amr))
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
        // Two or more AMRs waiting on each other in a cycle (A→B→…→A).
        let waits: Vec<(u64, u64)> = self.blocked.iter().map(|(a, s)| (*a, *s)).collect();
        let mut seen_in_cycle: HashSet<u64> = HashSet::new();
        for (start, _) in waits {
            if seen_in_cycle.contains(&start) || !self.blocked.contains_key(&start) {
                continue;
            }
            // Walk the wait graph: amr -> blocked stop -> holding amr.
            let mut chain: Vec<(u64, u64)> = Vec::new();
            let mut cur = start;
            let cycle = loop {
                let Some(&stop) = self.blocked.get(&cur) else {
                    break None;
                };
                if let Some(pos) = chain.iter().position(|(who, _)| *who == cur) {
                    break Some(chain[pos..].to_vec());
                }
                chain.push((cur, stop));
                let Some(&holder) = self.holds.get(&stop) else {
                    break None;
                };
                if holder == cur {
                    break None;
                }
                cur = holder;
            };
            let Some(cycle) = cycle else {
                continue;
            };
            if cycle.len() < 2 {
                continue;
            }
            for &(who, _) in &cycle {
                seen_in_cycle.insert(who);
            }
            // Try each AMR in the cycle: go round another way, or aside to
            // a free neighbour stop to break the loop.
            let mut broken = false;
            for &(a, s) in &cycle {
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
                if let Some(legs) = round {
                    r.legs = legs;
                    self.rovers.insert(a, r);
                    for &(member, _) in &cycle {
                        self.blocked.remove(&member);
                    }
                    changed = true;
                    broken = true;
                    break;
                }
            }
            if !broken {
                let names: Vec<String> = cycle
                    .iter()
                    .map(|(u, _)| {
                        self.components
                            .iter()
                            .find(|c| c.object == *u)
                            .map_or(String::new(), |c| c.name.clone())
                    })
                    .collect();
                self.flow_problem = Some(format!(
                    "AMR 교착: {}가 서로의 정지 위치를 기다립니다 (비켜설 길이 없음)",
                    names.join(", ")
                ));
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

    /// Stationary floor equipment footprints (inflated by operator half-width)
    /// that walking operators should route around instead of clipping through.
    fn operator_obstacles(
        &self,
        doc: &Document,
        from: (f64, f64),
        to: (f64, f64),
    ) -> Vec<[f64; 4]> {
        const PAD: f64 = 0.25;
        let mut rects = Vec::new();
        for c in &self.components {
            if matches!(
                c.kind,
                Kind::Operator
                    | Kind::Amr
                    | Kind::Stop
                    | Kind::Sensor
                    | Kind::Gripper
                    | Kind::Door
                    | Kind::Connector
            ) {
                continue;
            }
            let Some(id) = id_of(doc, c.object) else {
                continue;
            };
            let mut lo = DVec3::splat(f64::INFINITY);
            let mut hi = DVec3::splat(f64::NEG_INFINITY);
            for (o, _) in doc
                .objects()
                .filter(|(o, _)| *o == id || is_below(doc, *o, id))
            {
                if let Some((a, b)) = bounds(doc, o) {
                    lo = lo.min(a);
                    hi = hi.max(b);
                }
            }
            if !lo.is_finite() || lo.z > 1.9 || hi.z < 0.15 {
                continue;
            }
            let r = [lo.x - PAD, lo.y - PAD, hi.x + PAD, hi.y + PAD];
            if pt_in_rect(from, r) || pt_in_rect(to, r) {
                continue;
            }
            rects.push(r);
        }
        rects
    }

    /// Person `who` walks to `to` (routing around stationary floor equipment),
    /// then faces `toward`.
    fn walk(&mut self, doc: &Document, who: u64, to: DVec3, toward: DVec3) -> Option<()> {
        let c = self.components.iter().find(|c| c.object == who)?.clone();
        let mut r = self.rover(doc, &c)?;
        r.legs.clear();
        let start = (r.at.x, r.at.y);
        let goal = (to.x, to.y);
        if (to - r.at).truncate().length() > 1e-3 {
            let rects = self.operator_obstacles(doc, start, goal);
            let waypoints = plan_walk(start, goal, &rects);
            let mut prev = r.at;
            for (wx, wy) in waypoints {
                let wp = DVec3::new(wx, wy, to.z);
                let seg = wp - prev;
                if seg.truncate().length() > 1e-3 {
                    r.legs.push(Leg::Turn(seg.y.atan2(seg.x)));
                    r.legs.push(Leg::Walk(wp));
                    prev = wp;
                }
            }
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
        let decks = self.decks(doc);
        self.jobs
            .iter()
            .filter(|j| j.by == 3 && j.amr.is_none() && j.part != 0)
            // Handing to an AMR: once the AMR stands beside the robot.
            .filter(|j| j.handoff.is_none() || j.docked.is_some_and(|a| self.rovers.get(&a).is_none_or(|r| r.legs.is_empty())))
            .filter_map(|j| {
                let p = self.parts.iter().find(|p| p.uid == j.part)?;
                let to = self.components.iter().find(|c| c.object == j.to)?;
                // Onto a belt near its start, where the flow puts its
                // parts: not its middle, which may be out of the robot's
                // reach, nor its very end, the gripper on the drum.
                // Onto an AMR, its top plate's middle.
                let deck = j.handoff.and_then(|a| decks.iter().find(|d| d.object == a));
                let top = match (deck, belts.iter().find(|b| b.object == j.to)) {
                    (Some(d), _) => d.pose.transform_point3((d.lo + d.hi) / 2.0 * DVec3::new(1.0, 1.0, 0.0) + DVec3::Z * d.hi.z),
                    (None, Some(b)) => b.point(belt_drop(b)),
                    (None, None) => Self::dock(doc, to)?.0,
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
            // Put onto the AMR standing by: it carries it on, loaded now.
            None if job.handoff.is_some() && job.docked.is_some() => {
                let a = job.docked.unwrap();
                self.parts[i].flow = Some(Place::Riding { amr: a, to: job.to });
                let j = &mut self.jobs[k];
                j.by = 1;
                j.amr = Some(a);
                j.robot = Some(a);
                j.handoff = None;
                j.docked = None;
                j.gripped = false;
                j.phase = Phase::Loading(now);
            }
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

    /// Part `part`'s robot stopped from outside, its carrying program gone:
    /// in the gripper, the part goes on to where it was going; not yet
    /// taken, it waits again for a robot. Nothing went wrong, so no problem
    /// is told.
    pub fn robot_dropped(&mut self, part: u64) {
        let held = self
            .part_index(part)
            .is_some_and(|i| self.parts[i].held.is_some());
        let told = self.flow_problem.clone();
        self.robot_done(part, (!held).then(String::new));
        self.flow_problem = told;
    }

    /// Part `part`'s job for a robot that no robot can do — its place out
    /// of every one's reach —: done without one. The part goes along the
    /// link as drawn, in the time a robot would take — half a metre a
    /// second, two seconds to grip and let go —, and the link is marked out
    /// of reach (see `unreachable_to`). The flow is the user's: it is never
    /// sent another way.
    pub fn robot_skip(&mut self, part: u64, why: String) {
        let now = self.last_tick.unwrap_or(0.0);
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
        let dest = self
            .docks_seen
            .as_ref()
            .and_then(|d| d.docks.get(&job.to).map(|d| d.0));
        if let Some(i) = self.part_index(part) {
            let from = self.parts[i].world.translation;
            let far = dest.map_or(0.0, |d| d.distance(from));
            self.parts[i].held = None;
            self.parts[i].flow = Some(Place::Moving {
                from,
                to: job.to,
                start: now,
                end: now + far / 0.5 + 2.0,
            });
        }
        self.flow_problem = Some(format!(
            "{to}: 로봇이 닿지 않아 운반 시간만 셈합니다 — {why}"
        ));
        self.changed = true;
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
            Some(b) => b.point(b.length / 2.0 - super::RAW.0),
            None => Self::dock(doc, comp(from)?)?.0,
        };
        let put = match belts.iter().find(|b| b.object == to) {
            Some(b) => b.point(belt_drop(b)),
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
        // The stops' places, worked out at most once this pass: the stops
        // stand where they are while it goes.
        let places = std::cell::OnceCell::new();
        for k in 0..self.jobs.len() {
            let job = self.jobs[k].clone();
            let idle = |cell: &Cell, a: u64| cell.rovers.get(&a).is_none_or(|r| r.legs.is_empty());
            let hand = job.by == 2;
            // A robot's job is the view's, which runs robots: but one handing
            // to an AMR calls the AMR first, to stand beside the robot.
            if job.by == 3 {
                if let (Some(_), None) = (job.handoff, job.docked) {
                    let places = places.get_or_init(|| self.stop_places(doc));
                    let stop = job.robot.and_then(|r| {
                        let at = doc.world_transform(id_of(doc, r)?).translation;
                        Self::nearest_stop(places, at)
                    });
                    // Whichever AMR is free, the nearest the robot's stop:
                    // the AMR drawn is the fleet's place in the picture.
                    let near = stop.and_then(|stop| {
                        self.components
                            .iter()
                            .filter(|c| self.free(c))
                            .filter_map(|c| {
                                let d = self.lane_distances(places, self.rover(doc, c)?.at);
                                Some((c.object, d.get(&stop).copied().unwrap_or(f64::INFINITY)))
                            })
                            .filter(|(_, d)| d.is_finite())
                            .min_by(|a, b| a.1.total_cmp(&b.1))
                            .map(|(a, _)| a)
                    });
                    match (stop, near) {
                        (Some(stop), Some(a)) => match self.send(doc, a, stop) {
                            Ok(()) => {
                                self.jobs[k].docked = Some(a);
                                self.jobs[k].handoff = Some(a);
                                happened = true;
                            }
                            Err(e) => self.flow_problem = Some(e),
                        },
                        (Some(_), None) => {}
                        (None, _) => {
                            self.flow_problem =
                                Some("로봇 가까이(4 m)에 AMR 정지 위치가 없습니다".into());
                        }
                    }
                }
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
                    let places = places.get_or_init(|| self.stop_places(doc));
                    if self.stop_near_in(doc, places, job.from).is_none() {
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
                    // Handing to a robot: to the stop nearest the robot.
                    let places = places.get_or_init(|| self.stop_places(doc));
                    let toward = job.handoff.and_then(|r| {
                        let at = doc.world_transform(id_of(doc, r)?).translation;
                        Self::nearest_stop(places, at)
                    });
                    let sent = if hand {
                        self.walk_to(doc, job.amr.unwrap(), job.to)
                    } else if let Some(stop) = toward {
                        self.send(doc, job.amr.unwrap(), stop)
                    } else {
                        match self.stop_near_in(doc, places, job.to) {
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
                // Beside the robot it hands to: the robot's to take, off
                // the top plate, the AMR standing by till it has.
                Phase::Carrying if job.handoff.is_some() && idle(self, job.amr.unwrap()) => {
                    let j = &mut self.jobs[k];
                    j.docked = j.amr.take();
                    j.robot = j.handoff.take();
                    j.by = 3;
                    Phase::Waiting
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

fn pt_in_rect(p: (f64, f64), r: [f64; 4]) -> bool {
    p.0 >= r[0] && p.0 <= r[2] && p.1 >= r[1] && p.1 <= r[3]
}

fn seg_hits_rect(a: (f64, f64), b: (f64, f64), r: [f64; 4]) -> bool {
    let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    for (p, q) in [
        (-dx, a.0 - r[0]),
        (dx, r[2] - a.0),
        (-dy, a.1 - r[1]),
        (dy, r[3] - a.1),
    ] {
        if p.abs() < 1e-12 {
            if q < 0.0 {
                return false;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                if t > t1 {
                    return false;
                }
                if t > t0 {
                    t0 = t;
                }
            } else {
                if t < t0 {
                    return false;
                }
                if t < t1 {
                    t1 = t;
                }
            }
        }
    }
    t0 <= t1
}

/// Plans a 2D walking path from `from` to `to` avoiding axis-aligned `rects`,
/// using a direct fast path when unobstructed and bounded 8-connected A* +
/// line-of-sight smoothing when blocked. Falls back to `[to]` if unreachable.
pub(super) fn plan_walk(from: (f64, f64), to: (f64, f64), rects: &[[f64; 4]]) -> Vec<(f64, f64)> {
    let seg_clear = |a: (f64, f64), b: (f64, f64)| rects.iter().all(|r| !seg_hits_rect(a, b, *r));
    if rects.is_empty() || seg_clear(from, to) {
        return vec![to];
    }
    const STEP: f64 = 0.35;
    const MAX_EXPAND: usize = 8000;
    let mut min_x = from.0.min(to.0);
    let mut max_x = from.0.max(to.0);
    let mut min_y = from.1.min(to.1);
    let mut max_y = from.1.max(to.1);
    for r in rects {
        min_x = min_x.min(r[0]);
        min_y = min_y.min(r[1]);
        max_x = max_x.max(r[2]);
        max_y = max_y.max(r[3]);
    }
    min_x -= 2.0;
    min_y -= 2.0;
    max_x += 2.0;
    max_y += 2.0;
    let nx = ((max_x - min_x) / STEP).ceil().max(1.0) as i32;
    let ny = ((max_y - min_y) / STEP).ceil().max(1.0) as i32;
    if (nx as i64) * (ny as i64) > 100_000 {
        return vec![to];
    }
    let to_grid = |p: (f64, f64)| -> (i32, i32) {
        (
            ((p.0 - min_x) / STEP).round().clamp(0.0, nx as f64) as i32,
            ((p.1 - min_y) / STEP).round().clamp(0.0, ny as f64) as i32,
        )
    };
    let to_world =
        |g: (i32, i32)| -> (f64, f64) { (min_x + g.0 as f64 * STEP, min_y + g.1 as f64 * STEP) };
    let start = to_grid(from);
    let goal = to_grid(to);
    if start == goal {
        return vec![to];
    }
    let blocked = |g: (i32, i32)| -> bool {
        if g == start || g == goal {
            return false;
        }
        let p = to_world(g);
        rects.iter().any(|r| pt_in_rect(p, *r))
    };
    let heuristic = |a: (i32, i32), b: (i32, i32)| -> u32 {
        let dx = (a.0 - b.0).unsigned_abs();
        let dy = (a.1 - b.1).unsigned_abs();
        let (lo, hi) = if dx < dy { (dx, dy) } else { (dy, dx) };
        hi * 1000 + lo * 414
    };
    use std::cmp::Reverse;
    use std::collections::{BinaryHeap, HashMap};
    let mut open: BinaryHeap<(Reverse<u32>, u32, (i32, i32))> = BinaryHeap::new();
    let mut g_score: HashMap<(i32, i32), u32> = HashMap::new();
    let mut came_from: HashMap<(i32, i32), (i32, i32)> = HashMap::new();
    g_score.insert(start, 0);
    open.push((Reverse(heuristic(start, goal)), 0, start));
    let mut expanded = 0;
    let mut found = false;
    const DIRS: [(i32, i32, u32); 8] = [
        (1, 0, 1000),
        (-1, 0, 1000),
        (0, 1, 1000),
        (0, -1, 1000),
        (1, 1, 1414),
        (1, -1, 1414),
        (-1, 1, 1414),
        (-1, -1, 1414),
    ];
    while let Some((_, g, cur)) = open.pop() {
        if cur == goal {
            found = true;
            break;
        }
        if g > *g_score.get(&cur).unwrap_or(&u32::MAX) {
            continue;
        }
        expanded += 1;
        if expanded > MAX_EXPAND {
            break;
        }
        for (dx, dy, cost) in DIRS {
            let nxt = (cur.0 + dx, cur.1 + dy);
            if nxt.0 < 0 || nxt.0 > nx || nxt.1 < 0 || nxt.1 > ny || blocked(nxt) {
                continue;
            }
            if dx != 0 && dy != 0 && (blocked((cur.0 + dx, cur.1)) || blocked((cur.0, cur.1 + dy)))
            {
                continue;
            }
            let ng = g.saturating_add(cost);
            if ng < *g_score.get(&nxt).unwrap_or(&u32::MAX) {
                g_score.insert(nxt, ng);
                came_from.insert(nxt, cur);
                open.push((Reverse(ng.saturating_add(heuristic(nxt, goal))), ng, nxt));
            }
        }
    }
    if !found {
        return vec![to];
    }
    let mut raw = Vec::new();
    let mut cur = goal;
    while cur != start {
        raw.push(cur);
        cur = came_from[&cur];
    }
    raw.reverse();
    let mut pts = Vec::with_capacity(raw.len() + 2);
    pts.push(from);
    for g in raw {
        if g != goal {
            pts.push(to_world(g));
        }
    }
    pts.push(to);
    // Greedy line-of-sight string-pulling to eliminate grid stair-stepping.
    let mut smooth = Vec::new();
    let mut i = 0;
    while i + 1 < pts.len() {
        let mut best = i + 1;
        for j in (i + 2..pts.len()).rev() {
            if seg_clear(pts[i], pts[j]) {
                best = j;
                break;
            }
        }
        smooth.push(pts[best]);
        i = best;
    }
    smooth
}

#[cfg(test)]
mod tests;
