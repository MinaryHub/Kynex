//! Scenarios: variants of a cell, run and compared. How many parts an hour
//! with one CNC machine, with two, with another gripper: the question a
//! machine-tending cell is bought on.
//!
//! A scenario is the project as it stood when saved — the equipment, the
//! cell's roles and the programs — kept in the project file. Comparing runs
//! each, away from the viewer, for a set time on the simulation's clock:
//! every program round and round, with an operator keeping the cell
//! supplied (raw parts on the infeed, finished parts taken off the
//! outfeed; see [`Cell::supply`]). A process flow, if the cell has one, runs
//! with them: its parts out are counted with theirs, its lead time and the
//! bottleneck said.

use std::time::{Duration, Instant};

use kreate_scene::Document;
use serde_json::{json, Value};

use crate::cell::Cell;
use crate::library::Library;
use crate::pose::Posing;
use crate::run::Runner;
use crate::timeline::{Now, Timeline};

/// The simulation's step, seconds: fine beside a gripper's half second.
const STEP: f64 = 0.02;

/// What a scenario holds: the project's equipment, roles and programs, as
/// the project file keeps them.
pub fn snapshot(name: &str, programs: Vec<Value>, cell: Vec<Value>, library: Vec<Value>) -> Value {
    json!({ "name": name, "programs": programs, "cell": cell, "library": library })
}

/// Runs scenario `saved` in the session's scene `session` for `seconds` on
/// the simulation's clock, and says how the cell did: the parts it
/// delivered and their rate, each machine's utilization, each robot's work
/// and waiting, and the alarm that stopped it, if one did.
#[cfg(test)]
pub fn simulate(session: &Document, saved: &Value, seconds: f64) -> Value {
    simulate_seeded(session, saved, seconds, crate::cell::SEED)
}

/// The same, the flow's random numbers drawn from `seed`.
pub fn simulate_seeded(session: &Document, saved: &Value, seconds: f64, seed: u64) -> Value {
    let list = |key: &str| saved[key].as_array().cloned().unwrap_or_default();
    let name = saved["name"].as_str().unwrap_or_default();
    let mut library = Library::from_saved(&list("library"));
    let mut doc = session.clone();
    library.apply(&mut doc);
    let mut cell = Cell::from_saved(list("cell"));
    for (uid, item, kind) in library.roles() {
        cell.ensure(uid, item, kind);
    }
    cell.adopt(&doc);
    let mut posing = Posing::from_saved(list("programs"));
    posing.adopt(&doc);
    // Every program round and round.
    let mut programs: Vec<(u64, crate::program::Program)> = Vec::new();
    for (root, main) in posing.mains() {
        match main {
            Ok(mut p) => {
                p.close = true;
                programs.push((root, p));
            }
            Err(e) => return failed(name, &e),
        }
    }
    let mut runners = Vec::new();
    for (root, program) in programs {
        match Runner::new(root, program, &doc, 0.0) {
            Ok(r) => runners.push(r),
            Err(e) => return failed(name, &e),
        }
    }
    // A process flow, if there is one, runs too.
    let flow = cell.has_flow();
    if flow {
        cell.seed_flow(seed);
        if let Err(e) = cell.flow("start") {
            return failed(name, &e);
        }
    }
    if runners.is_empty() && !flow {
        return failed(name, "프로그램도 공정 흐름(소재 공급)도 없습니다");
    }
    let machines = cell.activities(&doc).len();

    cell.supply(&doc);
    let mut timeline = Timeline::new(0.0);
    let mut delivered = 0;
    let start = Instant::now();
    let mut t = 0.0;
    let mut alarm = None;
    while t <= seconds {
        let now = start + Duration::from_secs_f64(t);
        for r in &mut runners {
            r.tick(t, now, &mut posing, &mut cell, &doc);
        }
        cell.tick(&doc, t, now);
        delivered += cell.supply(&doc);
        // Warmed up: the figures from here.
        if cell.take_warmed() {
            timeline = Timeline::new(t);
            delivered = 0;
        }
        let mut lanes: Vec<Now> = runners
            .iter()
            .map(|r| Now {
                key: r.root.to_string(),
                name: crate::collide::label(&doc, r.root),
                group: "robot",
                activity: r.activity(),
            })
            .collect();
        lanes.extend(
            cell.activities(&doc)
                .into_iter()
                .map(|(uid, name, activity)| Now {
                    key: uid.to_string(),
                    name,
                    group: "machine",
                    activity,
                }),
        );
        timeline.sample(t, lanes);
        if let Some(r) = runners.iter().find(|r| r.stopped()) {
            alarm = Some(
                r.to_json(t)["detail"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
            );
            break;
        }
        t += STEP;
    }

    let record = timeline.to_json();
    let elapsed = timeline.elapsed();
    let summary = cell.flow_summary();
    if flow {
        delivered += summary["sunk"].as_u64().unwrap_or(0) as usize;
    }
    let share = |lane: &Value, keys: &[&str]| -> f64 {
        keys.iter()
            .filter_map(|k| lane["shares"][*k].as_f64())
            .sum()
    };
    let lanes = record["lanes"].as_array().cloned().unwrap_or_default();
    let in_group = |g: &'static str| lanes.iter().filter(move |l| l["group"] == g);
    // At work: machining, driving, turning, walking, working, moving,
    // gripping. The bottleneck is what was the most.
    const BUSY: [&str; 7] = ["cut", "drive", "turn", "walk", "work", "move", "grip"];
    let bottleneck = lanes
        .iter()
        .map(|l| (l, share(l, &BUSY)))
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .filter(|(_, busy)| *busy > 0.0)
        .map(|(l, busy)| json!({ "name": l["name"], "busy": busy }));
    json!({
        "name": name,
        "ok": true,
        "seconds": elapsed,
        "machines": machines,
        "robots": runners.len(),
        "parts": delivered,
        "per_hour": if elapsed > 0.0 { delivered as f64 / elapsed * 3600.0 } else { 0.0 },
        "alarm": alarm,
        "lead_mean": flow.then(|| summary["lead_mean"].clone()),
        "wip": flow.then(|| summary["wip"].clone()),
        "bottleneck": bottleneck,
        "machine_lanes": in_group("machine").map(|l| json!({
            "name": l["name"],
            "utilization": share(l, &BUSY),
            "blocked": share(l, &["blocked"]),
        })).collect::<Vec<_>>(),
        "robot_lanes": in_group("robot").map(|l| json!({
            "name": l["name"],
            "work": share(l, &["move", "grip"]),
            "wait": share(l, &["wait", "pause"]),
        })).collect::<Vec<_>>(),
    })
}

/// Student's t for a 95 % interval with `n` runs (n − 1 degrees of
/// freedom).
fn t95(n: usize) -> f64 {
    const T: [f64; 10] = [12.71, 4.30, 3.18, 2.78, 2.57, 2.45, 2.36, 2.31, 2.26, 2.23];
    T.get(n.saturating_sub(2)).copied().unwrap_or(1.96)
}

/// The mean of `xs`, and the half-width of its 95 % confidence interval.
fn mean_ci(xs: &[f64]) -> (f64, f64) {
    let n = xs.len();
    if n == 0 {
        return (0.0, 0.0);
    }
    let mean = xs.iter().sum::<f64>() / n as f64;
    if n < 2 {
        return (mean, 0.0);
    }
    let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1) as f64;
    (mean, t95(n) * (var / n as f64).sqrt())
}

/// Scenario `saved` run `reps` times, each with its own seed (1, 2, …):
/// its figures their means, parts an hour and lead time each with the
/// half-width of a 95 % confidence interval, the bottleneck the one most
/// often found. One run failing, the scenario does.
pub fn replicate(session: &Document, saved: &Value, seconds: f64, reps: usize) -> Value {
    let reps = reps.clamp(1, 50);
    let runs: Vec<Value> = (1..=reps as u64)
        .map(|seed| simulate_seeded(session, saved, seconds, seed))
        .collect();
    if let Some(failed) = runs.iter().find(|r| r["ok"] == false) {
        return failed.clone();
    }
    let of = |key: &str| -> Vec<f64> { runs.iter().filter_map(|r| r[key].as_f64()).collect() };
    let (per_hour, per_hour_ci) = mean_ci(&of("per_hour"));
    let (parts, _) = mean_ci(&of("parts"));
    let leads = of("lead_mean");
    let lead = (!leads.is_empty()).then(|| mean_ci(&leads));
    // Each lane's shares, averaged over the runs, by its place.
    let lanes = |group: &str, keys: &[&str]| -> Vec<Value> {
        let first = runs[0][group].as_array().cloned().unwrap_or_default();
        first
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let mut v = l.clone();
                for k in keys {
                    let xs: Vec<f64> = runs
                        .iter()
                        .filter_map(|r| r[group][i][*k].as_f64())
                        .collect();
                    v[*k] = json!(mean_ci(&xs).0);
                }
                v
            })
            .collect()
    };
    // The bottleneck most often, at its mean share.
    let mut found: Vec<(String, Vec<f64>)> = Vec::new();
    for r in &runs {
        if let (Some(name), Some(busy)) = (
            r["bottleneck"]["name"].as_str(),
            r["bottleneck"]["busy"].as_f64(),
        ) {
            match found.iter_mut().find(|(n, _)| n == name) {
                Some((_, b)) => b.push(busy),
                None => found.push((name.to_string(), vec![busy])),
            }
        }
    }
    let bottleneck = found
        .iter()
        .max_by_key(|(_, b)| b.len())
        .map(|(name, b)| json!({ "name": name, "busy": mean_ci(b).0, "times": b.len() }));
    let mut out = runs[0].clone();
    out["reps"] = json!(reps);
    out["parts"] = json!(parts.round() as u64);
    out["per_hour"] = json!(per_hour);
    out["per_hour_ci"] = json!(per_hour_ci);
    out["lead_mean"] = lead.map_or(Value::Null, |l| json!(l.0));
    out["lead_ci"] = lead.map_or(Value::Null, |l| json!(l.1));
    out["bottleneck"] = bottleneck.unwrap_or(Value::Null);
    out["machine_lanes"] = json!(lanes("machine_lanes", &["utilization", "blocked"]));
    out["robot_lanes"] = json!(lanes("robot_lanes", &["work", "wait"]));
    out
}

/// `kynex compare`: runs every scenario in the project file and prints how
/// each did, side by side. The scene is the `.kreate` file `--file`, or
/// none: a cell laid out from the library alone.
pub fn run(args: Vec<String>) -> Result<(), String> {
    let mut project = crate::paths::default_project();
    let mut file = None;
    let mut hours = 1.0;
    let mut reps = 1;
    let mut as_json = false;
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--project" => project = it.next().ok_or("--project needs a path")?.into(),
            "--file" => file = Some(it.next().ok_or("--file needs a path")?),
            "--hours" => {
                hours = it
                    .next()
                    .and_then(|h| h.parse::<f64>().ok())
                    .filter(|h| *h > 0.0 && *h <= 24.0)
                    .ok_or("--hours needs a number of hours, up to 24")?
            }
            "--reps" => {
                reps = it
                    .next()
                    .and_then(|r| r.parse::<usize>().ok())
                    .filter(|r| (1..=50).contains(r))
                    .ok_or("--reps needs a number of runs, 1 to 50")?
            }
            "--json" => as_json = true,
            a => return Err(format!("unknown argument {a:?}")),
        }
    }
    let text =
        std::fs::read_to_string(&project).map_err(|e| format!("{}: {e}", project.display()))?;
    let all: Value =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", project.display()))?;
    let scenarios = all["scenarios"].as_array().cloned().unwrap_or_default();
    if scenarios.is_empty() {
        return Err(format!(
            "{}: 시나리오가 없습니다. 뷰어의 타임라인 ▸ 시나리오에서 저장하세요",
            project.display()
        ));
    }
    let session = match &file {
        Some(path) => {
            let f = std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
            kreate_io::container::read(std::io::BufReader::new(f))
                .map_err(|e| format!("{path}: {e}"))?
                .document
        }
        None => Document::new(),
    };
    let results: Vec<Value> = scenarios
        .iter()
        .map(|s| replicate(&session, s, hours * 3600.0, reps))
        .collect();
    if as_json {
        for r in &results {
            println!("{r}");
        }
        return Ok(());
    }
    println!("{}", table(&results, hours));
    Ok(())
}

/// The comparison as a table of text.
fn table(results: &[Value], hours: f64) -> String {
    let pct = |v: f64| format!("{:.0}%", v * 100.0);
    let mean = |list: &Value, key: &str| {
        let l = list.as_array().cloned().unwrap_or_default();
        if l.is_empty() {
            0.0
        } else {
            l.iter().filter_map(|x| x[key].as_f64()).sum::<f64>() / l.len() as f64
        }
    };
    let rows: Vec<Vec<String>> = std::iter::once(
        [
            "시나리오",
            "장비",
            "완성",
            "시간당",
            "장비 가동률",
            "로봇 대기",
            "리드타임",
            "병목",
            "알람",
        ]
        .map(String::from)
        .to_vec(),
    )
    .chain(results.iter().map(|r| {
        vec![
            r["name"].as_str().unwrap_or_default().to_string(),
            r["machines"].as_u64().unwrap_or(0).to_string(),
            r["parts"].as_u64().unwrap_or(0).to_string(),
            format!("{:.1}", r["per_hour"].as_f64().unwrap_or(0.0)),
            pct(mean(&r["machine_lanes"], "utilization")),
            pct(mean(&r["robot_lanes"], "wait")),
            r["lead_mean"]
                .as_f64()
                .map_or("-".to_string(), |l| format!("{l:.0} s")),
            r["bottleneck"]["name"]
                .as_str()
                .map_or("-".to_string(), |n| {
                    format!(
                        "{n} {}",
                        pct(r["bottleneck"]["busy"].as_f64().unwrap_or(0.0))
                    )
                }),
            r["alarm"].as_str().unwrap_or("-").to_string(),
        ]
    }))
    .collect();
    // Hangul takes two columns of a terminal.
    let width = |s: &str| {
        s.chars()
            .map(|c| {
                if ('\u{1100}'..='\u{d7a3}').contains(&c) {
                    2
                } else {
                    1
                }
            })
            .sum::<usize>()
    };
    let columns = rows[0].len();
    let widths: Vec<usize> = (0..columns)
        .map(|i| rows.iter().map(|r| width(&r[i])).max().unwrap_or(0))
        .collect();
    let mut out = format!("{hours}시간 실행\n");
    for row in &rows {
        let cells: Vec<String> = row
            .iter()
            .enumerate()
            .map(|(i, cell)| {
                let pad = " ".repeat(widths[i] - width(cell));
                // The name and the alarm read left to right; numbers line up right.
                if i == 0 || i == 6 {
                    format!("{cell}{pad}")
                } else {
                    format!("{pad}{cell}")
                }
            })
            .collect();
        out += cells.join("  ").trim_end();
        out.push('\n');
    }
    out
}

fn failed(name: &str, why: &str) -> Value {
    json!({ "name": name, "ok": false, "alarm": why })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A line of the library's flow equipment, its station's time `cycle`,
    /// saved as a scenario.
    fn line(name: &str, cycle: f64) -> Value {
        let mut library = Library::default();
        for (template, x) in [
            ("flow_source", 0.0),
            ("flow_station", 2.0),
            ("flow_sink", 4.0),
        ] {
            library.handle(
                &json!({"op": "place", "template": template, "at": [x, 0.0, 0.0]}).to_string(),
            );
        }
        let mut doc = Document::new();
        library.apply(&mut doc);
        let mut cell = Cell::default();
        let roles = library.roles();
        for (uid, item, kind) in &roles {
            cell.ensure(*uid, item.clone(), *kind);
        }
        let ask = |cell: &mut Cell, req: Value| {
            let a: Value = serde_json::from_str(&cell.handle(&doc, &req.to_string())).unwrap();
            assert_eq!(a["ok"], true, "{a}");
        };
        let u: Vec<String> = roles.iter().map(|r| r.0.to_string()).collect();
        ask(
            &mut cell,
            json!({"op": "role", "object": u[0], "kind": "source", "props": {"feed": 10.0}}),
        );
        ask(
            &mut cell,
            json!({"op": "role", "object": u[1], "kind": "station", "props": {"cycle": cycle}}),
        );
        ask(
            &mut cell,
            json!({"op": "flow_next", "object": u[0], "next": [u[1]]}),
        );
        ask(
            &mut cell,
            json!({"op": "flow_next", "object": u[1], "next": [u[2]]}),
        );
        snapshot(name, Vec::new(), cell.to_saved(), library.to_saved())
    }

    #[test]
    fn a_flow_alone_is_a_scenario_compared_by_its_parts_and_bottleneck() {
        let slow = simulate(&Document::new(), &line("느린 공정", 30.0), 600.0);
        let quick = simulate(&Document::new(), &line("빠른 공정", 8.0), 600.0);
        assert_eq!(slow["ok"], true, "{slow}");
        // A part every 30 s, or every 10 s as the source feeds them.
        let (a, b) = (
            slow["parts"].as_u64().unwrap(),
            quick["parts"].as_u64().unwrap(),
        );
        assert!((18..=20).contains(&a), "{slow}");
        assert!((57..=60).contains(&b), "{quick}");
        assert!(slow["lead_mean"].as_f64().unwrap() > 30.0);
        // The slow station is the bottleneck, always at work.
        assert_eq!(slow["bottleneck"]["name"], "공정 스테이션 1");
        assert!(
            slow["bottleneck"]["busy"].as_f64().unwrap() > 0.95,
            "{slow}"
        );
        let text = table(&[slow, quick], 600.0 / 3600.0);
        assert!(
            text.contains("병목") && text.contains("공정 스테이션 1"),
            "{text}"
        );
    }

    #[test]
    fn runs_repeated_with_their_own_seeds_give_a_mean_and_its_interval() {
        let mut saved = line("들쭉날쭉", 9.0);
        // The station's times exponential about 9 s.
        for role in saved["cell"].as_array_mut().unwrap() {
            if role["kind"] == "station" {
                role["props"]["dist"] = json!(4);
            }
        }
        let r = replicate(&Document::new(), &saved, 1800.0, 5);
        assert_eq!(r["reps"], 5);
        let (rate, ci) = (
            r["per_hour"].as_f64().unwrap(),
            r["per_hour_ci"].as_f64().unwrap(),
        );
        // Fed every 10 s, worked about every 9: under 360 an hour, and the
        // runs differ.
        assert!(rate > 250.0 && rate < 360.0, "{r}");
        assert!(ci > 0.0 && ci < 40.0, "{r}");
        // The same seeds, the same answer.
        let again = replicate(&Document::new(), &saved, 1800.0, 5);
        assert_eq!(again["per_hour"], r["per_hour"]);
        // Fixed times vary not at all.
        let fixed = replicate(&Document::new(), &line("고정", 9.0), 1800.0, 3);
        assert!(fixed["per_hour_ci"].as_f64().unwrap() < 1e-9, "{fixed}");
    }
}
