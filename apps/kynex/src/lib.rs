//! KYNEX from the command line.
//!
//! ```text
//! kynex                       (as `kynex view --open`: KYNEX on its own)
//! kynex view [--project FILE] [--port 8090] [--lan] [--open]
//! kynex view ADDRESS|--kreate [--key KEY] [--port 8090] [--lan] [--open]
//! kynex link [ADDRESS] [--key KEY] [--name NAME] [--json]
//! kynex inspect FILE.kreate [--json]
//! kynex compare [--project FILE] [--file FILE.kreate] [--hours 1] [--reps 1] [--json]
//! ```
//!
//! `link` joins the shared session a Kreate opened (File ▸ Shared Session ▸
//! Host) and reports every edit as it lands: bodies added, moved and
//! removed, and each robot's joints. ADDRESS defaults to this machine on
//! Kreate's port; the key can also come from `KYNEX_KEY`.
//!
//! `--json` writes one JSON object per line instead, for a program to read.
//!
//! `view` serves the 3D viewer at `http://127.0.0.1:8090`, for any browser
//! on any system: KYNEX on its own, its cell laid out from its library and
//! the models it imports, in the project last opened. Given a Kreate
//! session's address (or `--kreate` for this machine's), it does what
//! `link` does and draws that scene as well. `--lan` lets other machines on
//! the network open it too.
//!
//! `compare` runs the scenarios saved in the project file for some hours on
//! the simulation's clock and prints how each did (see `scenario`).

use std::path::Path;
use std::time::{Duration, Instant};

use kreate_scene::robot::JointKind;
use kynex_link::{KreateLink, LinkConfig, LinkEvent};
use kynex_model::{Body, Diff, JointState, Model, Robot, Role, Shape};
use kynex_view::json as vjson;
use serde_json::{json, Value};

mod bench;
mod cell;
mod collide;
mod edges;
mod floor;
mod import;
mod library;
mod mqtt;
mod mtconnect;
mod paths;
mod plc;
mod pose;
mod program;
mod rtde;
mod run;
mod scenario;
pub mod studio;
mod timeline;
mod umati;
mod view;

const USAGE: &str = "\
kynex                                   (KYNEX on its own, in the browser)
kynex view [--port 8090] [--lan] [--open] [--project FILE]
kynex view ADDRESS|--kreate [--key KEY] [--name NAME] [--port 8090] [--lan] [--open] [--project FILE]
kynex view --file FILE.kreate [--port 8090] [--lan] [--open] [--project FILE]
kynex compare [--project FILE] [--file FILE.kreate] [--hours 1] [--reps 1] [--json]
kynex link [ADDRESS] [--key KEY] [--name NAME] [--json]
kynex inspect FILE.kreate [--json]
kynex bench [--tier cell|line|factory|factory-xl|all] [--browser] [--port 8091]";

/// The command line: `kynex …` (see [`USAGE`]).
pub fn cli() {
    let mut args = std::env::args().skip(1);
    let result = match args.next().as_deref() {
        Some("view") => view::run(args.collect()),
        Some("bench") => bench::run(args.collect()),
        Some("compare") => scenario::run(args.collect()),
        Some("link") => link(args.collect()),
        Some("inspect") => inspect(args.collect()),
        // Started with nothing, as an installed program is: on its own,
        // in the browser.
        None => view::run(vec!["--open".into()]),
        Some("-h" | "--help") => {
            println!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command {other:?}\n{USAGE}")),
    };
    if let Err(e) = result {
        eprintln!("kynex: {e}");
        std::process::exit(2);
    }
}

struct Out {
    json: bool,
    start: Instant,
}

impl Out {
    fn say(&self, text: &str) {
        if !self.json {
            let t = self.start.elapsed().as_secs_f64();
            println!("[{t:9.3}] {text}");
        }
    }

    fn emit(&self, value: Value) {
        if self.json {
            println!("{value}");
        }
    }
}

fn link(args: Vec<String>) -> Result<(), String> {
    let mut address = None;
    let mut key = std::env::var("KYNEX_KEY").unwrap_or_default();
    let mut name = None;
    let mut json = false;
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--key" => key = it.next().ok_or("--key needs a value")?,
            "--name" => name = Some(it.next().ok_or("--name needs a value")?),
            "--json" => json = true,
            a if a.starts_with('-') => return Err(format!("unknown option {a:?}\n{USAGE}")),
            a => address = Some(a.to_string()),
        }
    }
    let mut config = LinkConfig::new(address.unwrap_or_else(|| "127.0.0.1".into()));
    config.key = key;
    if let Some(name) = name {
        config.name = name;
    }
    let out = Out {
        json,
        start: Instant::now(),
    };
    out.say(&format!("Kreate 세션에 연결하는 중: {}", config.address));
    let mut link = KreateLink::new(config);
    let mut model = Model::default();

    loop {
        let events = link.wait(Duration::from_millis(250));
        let mut rebuild = false;
        for event in events {
            match event {
                LinkEvent::Joined { site, peers } => {
                    let names: Vec<_> = peers.iter().map(|p| p.name.as_str()).collect();
                    out.say(&format!(
                        "참가함 (site {site}), 참가자: {}",
                        names.join(", ")
                    ));
                    out.emit(json!({"event": "joined", "site": site, "peers": names}));
                }
                LinkEvent::WaitingForScene => {
                    out.say("세션에 아직 장면이 없습니다. Kreate에서 세션을 열면 이어집니다.");
                    out.emit(json!({"event": "waiting_for_scene"}));
                }
                LinkEvent::Offline(reason) => {
                    out.say(&format!("연결 안 됨: {reason} (다시 시도합니다)"));
                    out.emit(json!({"event": "offline", "reason": reason}));
                }
                LinkEvent::PeerJoined(peer) => {
                    out.say(&format!("{} 님이 들어왔습니다", peer.name));
                    out.emit(json!({"event": "peer_joined", "name": peer.name, "site": peer.site}));
                }
                LinkEvent::PeerLeft { site } => {
                    out.say(&format!("site {site} 님이 나갔습니다"));
                    out.emit(json!({"event": "peer_left", "site": site}));
                }
                LinkEvent::SceneReplaced => {
                    model = Model::from_document(link.document());
                    out.say(&format!(
                        "장면을 받았습니다: 물체 {}개, 로봇 {}대",
                        model.bodies.len(),
                        model.robots.len()
                    ));
                    for body in model.bodies.values() {
                        out.say(&format!("  {}", describe(body)));
                    }
                    for robot in &model.robots {
                        out.say(&robot_line(robot));
                    }
                    out.emit(json!({"event": "scene", "model": vjson::model(&model)}));
                }
                LinkEvent::Changed(_) => rebuild = true,
            }
        }
        // Several edits in one poll make one report: the scene as it now is.
        if rebuild {
            let newer = Model::from_document(link.document());
            let diff = model.diff(&newer);
            report(&out, &model, &newer, &diff);
            model = newer;
        }
    }
}

fn report(out: &Out, older: &Model, newer: &Model, diff: &Diff) {
    if diff.is_empty() {
        return;
    }
    for uid in &diff.added {
        out.say(&format!("+ {}", describe(&newer.bodies[uid])));
    }
    for uid in &diff.changed {
        // Whatever a moving robot carries moves with it: its joint line
        // says that, once, rather than a line for every part.
        if carried(newer, *uid, &diff.robots) {
            continue;
        }
        out.say(&format!("~ {}", describe(&newer.bodies[uid])));
    }
    for uid in &diff.removed {
        out.say(&format!("- {}", older.bodies[uid].name));
    }
    for root in &diff.robots {
        if let Some(robot) = newer.robot(*root) {
            out.say(&robot_line(robot));
        }
    }
    let bodies = |uids: &[u64]| -> Vec<Value> {
        uids.iter()
            .filter_map(|u| newer.bodies.get(u))
            .map(vjson::body)
            .collect()
    };
    out.emit(json!({
        "event": "changed",
        "added": bodies(&diff.added),
        "changed": bodies(&diff.changed),
        "removed": diff.removed,
        "robots": diff.robots.iter().filter_map(|r| newer.robot(*r)).map(vjson::robot).collect::<Vec<_>>(),
    }));
}

/// Whether `uid` hangs, at any depth, from the root of one of `robots`.
fn carried(model: &Model, uid: u64, robots: &[u64]) -> bool {
    let mut at = Some(uid);
    for _ in 0..=model.bodies.len() {
        let Some(u) = at else { return false };
        if robots.contains(&u) {
            return true;
        }
        at = model.bodies.get(&u).and_then(|b| b.parent);
    }
    false
}

fn inspect(args: Vec<String>) -> Result<(), String> {
    let json = args.iter().any(|a| a == "--json");
    let path = args
        .iter()
        .find(|a| !a.starts_with('-'))
        .ok_or(format!("which file?\n{USAGE}"))?;
    let file = std::fs::File::open(Path::new(path)).map_err(|e| format!("{path}: {e}"))?;
    let loaded = kreate_io::container::read(std::io::BufReader::new(file))
        .map_err(|e| format!("{path}: {e}"))?;
    let model = Model::from_document(&loaded.document);
    if json {
        println!("{}", vjson::model(&model));
        return Ok(());
    }
    println!(
        "{path}: 물체 {}개, 로봇 {}대",
        model.bodies.len(),
        model.robots.len()
    );
    for body in model.bodies.values() {
        println!("  {}", describe(body));
    }
    for robot in &model.robots {
        println!("{}", robot_line(robot));
    }
    Ok(())
}

// --- words -----------------------------------------------------------------

fn describe(body: &Body) -> String {
    let p = body.world.translation;
    let what = match body.shape {
        Shape::Empty => "empty".to_string(),
        Shape::Mesh { verts, faces } => format!("mesh {verts}v/{faces}f"),
        Shape::Camera => "camera".into(),
        Shape::Light => "light".into(),
        Shape::Armature => "armature".into(),
    };
    let role = match body.role {
        Some(Role::Link) => " link",
        Some(Role::Joint) => " joint",
        Some(Role::Collision) => " collision",
        Some(Role::Rig) => " rig",
        None => "",
    };
    let rigid = match body.rigid {
        Some(r) if r.dynamic => format!(" rigid {:.2} kg", r.mass),
        Some(_) => " passive".into(),
        None => String::new(),
    };
    format!(
        "{} ({what}{role}{rigid}) @ ({:.3}, {:.3}, {:.3}) m",
        body.name, p.x, p.y, p.z
    )
}

fn robot_line(robot: &Robot) -> String {
    let joints: Vec<String> = robot
        .joints
        .iter()
        .filter(|j| j.kind.moves())
        .map(joint_text)
        .collect();
    format!(
        "⚙ {}: {}",
        robot.name,
        if joints.is_empty() {
            "움직이는 관절 없음".into()
        } else {
            joints.join(", ")
        }
    )
}

fn joint_text(j: &JointState) -> String {
    let (value, unit) = match j.kind {
        JointKind::Prismatic => (j.position * 1000.0, "mm"),
        _ => (j.position.to_degrees(), "°"),
    };
    let warn = if j.out_of_limits() {
        " ⚠한계초과"
    } else {
        ""
    };
    format!("{} {value:.1}{unit}{warn}", j.name)
}
