//! Scenarios: variants of a cell, run and compared. How many parts an hour
//! with one CNC machine, with two, with another gripper: the question a
//! machine-tending cell is bought on.
//!
//! A scenario is the project as it stood when saved — the equipment, the
//! cell's roles and the programs — kept in the project file. Comparing runs
//! each, away from the viewer, for a set time on the simulation's clock:
//! every program round and round, with an operator keeping the cell
//! supplied (raw parts on the infeed, finished parts taken off the
//! outfeed; see [`Cell::supply`]).

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
pub fn simulate(session: &Document, saved: &Value, seconds: f64) -> Value {
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
    if runners.is_empty() {
        return failed(name, "프로그램이 없습니다");
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
    let share = |lane: &Value, keys: &[&str]| -> f64 {
        keys.iter()
            .filter_map(|k| lane["shares"][*k].as_f64())
            .sum()
    };
    let lanes = record["lanes"].as_array().cloned().unwrap_or_default();
    let in_group = |g: &'static str| lanes.iter().filter(move |l| l["group"] == g);
    json!({
        "name": name,
        "ok": true,
        "seconds": elapsed,
        "machines": machines,
        "robots": runners.len(),
        "parts": delivered,
        "per_hour": if elapsed > 0.0 { delivered as f64 / elapsed * 3600.0 } else { 0.0 },
        "alarm": alarm,
        "machine_lanes": in_group("machine").map(|l| json!({
            "name": l["name"],
            "utilization": share(l, &["cut"]),
            "blocked": share(l, &["blocked"]),
        })).collect::<Vec<_>>(),
        "robot_lanes": in_group("robot").map(|l| json!({
            "name": l["name"],
            "work": share(l, &["move", "grip"]),
            "wait": share(l, &["wait", "pause"]),
        })).collect::<Vec<_>>(),
    })
}

/// `kynex compare`: runs every scenario in the project file and prints how
/// each did, side by side. The scene is the `.kreate` file `--file`, or
/// none: a cell laid out from the library alone.
pub fn run(args: Vec<String>) -> Result<(), String> {
    let mut project = crate::paths::default_project();
    let mut file = None;
    let mut hours = 1.0;
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
        .map(|s| simulate(&session, s, hours * 3600.0))
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
            "CNC",
            "완성",
            "시간당",
            "CNC 가동률",
            "로봇 대기",
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
    let widths: Vec<usize> = (0..7)
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
