//! What each robot and machine did, moment by moment, while a program ran:
//! the Gantt chart a cell is judged by. From it, how much of the time each
//! machine was cutting, how much the robot worked and waited, and how many
//! parts an hour the cell makes.
//!
//! Sampled each tick of the simulation's clock: a lane's span goes on while
//! its activity stays the same, and a new one starts when it changes.

use serde_json::{json, Value};

/// One thing a lane is doing now: its key, its name, its group (`robot`
/// or `machine`), and its activity.
pub struct Now {
    pub key: String,
    pub name: String,
    pub group: &'static str,
    pub activity: &'static str,
}

#[derive(Debug, Clone, PartialEq)]
struct Span {
    from: f64,
    to: f64,
    activity: &'static str,
}

#[derive(Debug, Clone)]
struct Lane {
    key: String,
    name: String,
    group: &'static str,
    spans: Vec<Span>,
}

/// The record of a run, from when it started.
#[derive(Debug, Default)]
pub struct Timeline {
    start: f64,
    now: f64,
    lanes: Vec<Lane>,
    /// Whether the run has ended: nothing more is recorded.
    ended: bool,
}

/// Activities that count as a machine making parts.
const CUTTING: &str = "cut";

impl Timeline {
    /// A record starting at `time`.
    pub fn new(time: f64) -> Self {
        Self {
            start: time,
            now: time,
            lanes: Vec::new(),
            ended: false,
        }
    }

    /// Records no more: the run has ended, its last state recorded.
    pub fn end(&mut self) {
        self.ended = true;
    }

    pub fn ended(&self) -> bool {
        self.ended
    }

    /// What every lane is doing at `time`.
    pub fn sample(&mut self, time: f64, now: Vec<Now>) {
        if self.ended || time < self.now {
            return;
        }
        for n in now {
            let lane = match self.lanes.iter_mut().position(|l| l.key == n.key) {
                Some(i) => &mut self.lanes[i],
                None => {
                    self.lanes.push(Lane {
                        key: n.key,
                        name: n.name,
                        group: n.group,
                        spans: Vec::new(),
                    });
                    self.lanes.last_mut().unwrap()
                }
            };
            match lane.spans.last_mut() {
                Some(last) if last.activity == n.activity => last.to = time,
                Some(last) => {
                    last.to = time;
                    lane.spans.push(Span {
                        from: time,
                        to: time,
                        activity: n.activity,
                    });
                }
                None => lane.spans.push(Span {
                    // A lane first seen late was doing this since the start.
                    from: self.start,
                    to: time,
                    activity: n.activity,
                }),
            }
        }
        self.now = time;
    }

    /// Seconds recorded.
    pub fn elapsed(&self) -> f64 {
        self.now - self.start
    }

    /// Cycles the machines finished: cuts that ended before now.
    pub fn parts_made(&self) -> usize {
        self.lanes
            .iter()
            .flat_map(|l| l.spans.iter().enumerate().map(move |(i, s)| (l, i, s)))
            .filter(|(l, i, s)| s.activity == CUTTING && *i + 1 < l.spans.len())
            .count()
    }

    /// Each lane's seconds per activity.
    fn totals(lane: &Lane) -> Vec<(&'static str, f64)> {
        let mut out: Vec<(&'static str, f64)> = Vec::new();
        for s in &lane.spans {
            let d = s.to - s.from;
            match out.iter_mut().find(|(a, _)| *a == s.activity) {
                Some((_, t)) => *t += d,
                None => out.push((s.activity, d)),
            }
        }
        out
    }

    /// What the `timeline` event says.
    pub fn to_json(&self) -> Value {
        let elapsed = self.elapsed();
        let share = |t: f64| if elapsed > 0.0 { t / elapsed } else { 0.0 };
        let made = self.parts_made();
        json!({
            "start": self.start,
            "now": self.now,
            "elapsed": elapsed,
            "parts": made,
            "per_hour": if elapsed > 0.0 { made as f64 / elapsed * 3600.0 } else { 0.0 },
            "lanes": self.lanes.iter().map(|l| {
                let totals = Self::totals(l);
                json!({
                    "key": l.key,
                    "name": l.name,
                    "group": l.group,
                    "spans": l.spans.iter().map(|s| json!([s.from - self.start, s.to - self.start, s.activity])).collect::<Vec<_>>(),
                    "totals": totals.iter().map(|(a, t)| (a.to_string(), json!(t))).collect::<serde_json::Map<_, _>>(),
                    "shares": totals.iter().map(|(a, t)| (a.to_string(), json!(share(*t)))).collect::<serde_json::Map<_, _>>(),
                })
            }).collect::<Vec<_>>(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now(key: &str, group: &'static str, activity: &'static str) -> Now {
        Now {
            key: key.into(),
            name: key.into(),
            group,
            activity,
        }
    }

    #[test]
    fn spans_follow_what_each_lane_does() {
        let mut t = Timeline::new(10.0);
        t.sample(
            10.0,
            vec![now("cnc", "machine", "idle"), now("robot", "robot", "move")],
        );
        t.sample(
            12.0,
            vec![now("cnc", "machine", "cut"), now("robot", "robot", "wait")],
        );
        t.sample(
            20.0,
            vec![now("cnc", "machine", "cut"), now("robot", "robot", "wait")],
        );
        t.sample(
            22.0,
            vec![now("cnc", "machine", "idle"), now("robot", "robot", "move")],
        );
        t.sample(
            30.0,
            vec![now("cnc", "machine", "cut"), now("robot", "robot", "wait")],
        );
        let j = t.to_json();
        assert_eq!(j["elapsed"], 20.0);
        let cnc = &j["lanes"][0];
        assert_eq!(cnc["spans"][1], json!([2.0, 12.0, "cut"]));
        // Cut from 12 to 22, and again from 30: one finished.
        assert_eq!(j["parts"], 1);
        assert!((cnc["shares"]["cut"].as_f64().unwrap() - 0.5).abs() < 1e-9);
        assert!((j["per_hour"].as_f64().unwrap() - 180.0).abs() < 1e-9);
        let robot = &j["lanes"][1];
        assert!((robot["totals"]["wait"].as_f64().unwrap() - 10.0).abs() < 1e-9);
    }
}
