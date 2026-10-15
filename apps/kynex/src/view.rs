//! `kynex view`: the link, drawn in a browser.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use std::sync::mpsc::{Receiver, RecvTimeoutError};

use kreate_scene::{Changes, Document};
use kynex_link::{KreateLink, LinkConfig, LinkEvent};
use kynex_model::Model;
use kynex_view::server::RobotRequest;
use kynex_view::{Server, Status};
use serde_json::Value;

use crate::cell::{self, Cell};
use crate::collide::{self, Collider};
use crate::library::{self, Library};
use crate::pose::{Posing, Project};
use crate::run::{Clock, Runner};
use crate::scenario;
use crate::timeline::{Now, Timeline};

const PORT: u16 = 8090;

/// Where the project — programs, the cell's roles and placed equipment —
/// is kept between runs, unless `--project` says.
const PROJECT: &str = "kynex-project.json";

/// Where programs were kept before the project file held more: read once,
/// if the project file is not there yet.
const OLD_PROGRAMS: &str = "kynex-programs.json";

pub fn run(args: Vec<String>) -> Result<(), String> {
    let mut address = None;
    let mut key = std::env::var("KYNEX_KEY").unwrap_or_default();
    let mut name = None;
    let mut port = PORT;
    let mut lan = false;
    let mut open = false;
    let mut file = None;
    let mut empty = false;
    let mut project: Option<PathBuf> = None;
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--key" => key = it.next().ok_or("--key needs a value")?,
            "--name" => name = Some(it.next().ok_or("--name needs a value")?),
            "--port" => {
                port = it
                    .next()
                    .and_then(|p| p.parse().ok())
                    .ok_or("--port needs a number")?
            }
            "--lan" => lan = true,
            "--open" => open = true,
            "--file" => file = Some(it.next().ok_or("--file needs a path")?),
            // No Kreate: a cell laid out from the library alone.
            "--empty" => empty = true,
            // `--programs` is the old name, from before the file held more.
            "--project" | "--programs" => {
                project = Some(it.next().ok_or("--project needs a path")?.into())
            }
            a if a.starts_with('-') => return Err(format!("unknown option {a:?}")),
            a => address = Some(a.to_string()),
        }
    }

    // Only this machine unless asked: the viewer shows the whole scene to
    // whoever can reach the port.
    let bind = if lan {
        format!("0.0.0.0:{port}")
    } else {
        format!("127.0.0.1:{port}")
    };
    let server = Server::start(&bind).map_err(|e| format!("could not listen on {bind}: {e}"))?;
    let local = format!("http://localhost:{}", server.addr().port());
    println!("KYNEX 뷰어: {local}");
    if lan {
        println!(
            "  같은 네트워크의 다른 기기: http://{}:{}",
            lan_address(),
            server.addr().port()
        );
    }
    if open {
        open_browser(&local);
    }

    let requests = server.robot_requests();
    let mut stage = Stage::open(project)?;
    if let Some(path) = file {
        let file = std::fs::File::open(&path).map_err(|e| format!("{path}: {e}"))?;
        let loaded = kreate_io::container::read(std::io::BufReader::new(file))
            .map_err(|e| format!("{path}: {e}"))?;
        return offline(&server, &requests, stage, loaded.document, &path);
    }
    if empty {
        return offline(&server, &requests, stage, Document::new(), "새 프로젝트");
    }

    let mut config = LinkConfig::new(address.unwrap_or_else(|| "127.0.0.1".into()));
    config.key = key;
    if let Some(name) = name {
        config.name = name;
    }
    let addr = config.address.clone();
    println!("Kreate 세션: {addr}");
    let mut link = KreateLink::new(config);
    let mut peers: Vec<(u16, String)> = Vec::new();
    server.status(&Status::Waiting {
        address: addr.clone(),
    });

    let mut checks = Checks::default();
    // The scene as last shown, while a check of it is still owed.
    let mut owed: Option<Document> = None;
    loop {
        let events = link.poll();
        let quiet = events.is_empty();
        let mut changes: Option<Changes> = None;
        let mut replaced = false;
        for event in events {
            match event {
                LinkEvent::Joined { peers: list, .. } => {
                    peers = list.into_iter().map(|p| (p.site, p.name)).collect();
                    println!("참가함. 참가자: {}", names(&peers).join(", "));
                }
                LinkEvent::PeerJoined(p) => {
                    peers.push((p.site, p.name));
                }
                LinkEvent::PeerLeft { site } => peers.retain(|(s, _)| *s != site),
                LinkEvent::WaitingForScene => {
                    println!("세션에 아직 장면이 없습니다");
                    server.status(&Status::Waiting {
                        address: addr.clone(),
                    });
                }
                LinkEvent::Offline(reason) => {
                    println!("연결 안 됨: {reason}");
                    server.status(&Status::Offline {
                        address: addr.clone(),
                        reason,
                    });
                }
                LinkEvent::SceneReplaced => {
                    replaced = true;
                    changes = None;
                }
                LinkEvent::Changed(c) => {
                    let all = changes.get_or_insert_with(Changes::default);
                    all.changed.extend(c.changed);
                    all.removed.extend(c.removed);
                    all.settings |= c.settings;
                }
            }
        }
        if link.is_live() && !quiet {
            server.status(&Status::Live {
                address: addr.clone(),
                peers: names(&peers),
            });
        }
        if replaced {
            stage.replaced(link.document());
        } else if let Some(changes) = &changes {
            stage.collider.forget(changes);
            stage.rebase(link.document());
        }
        // The page's requests: a pose asked for shows as soon as it is
        // worked out, so they are waited on rather than the link when both
        // are quiet.
        let mut asked: Vec<RobotRequest> = requests.try_iter().collect();
        if quiet && asked.is_empty() && !replaced && changes.is_none() {
            asked.extend(requests.recv_timeout(Duration::from_millis(3)).ok());
        }
        let any_asked = !asked.is_empty();
        if stage.answer(link.document(), asked) && !replaced {
            changes.get_or_insert_with(Changes::default);
        }
        if replaced || changes.is_some() {
            let (posed, more) = stage.shown(link.document());
            let shown = posed.as_ref().unwrap_or(link.document());
            if replaced {
                let model = Model::from_document(shown);
                server.reset(shown);
                println!(
                    "장면을 받았습니다: 물체 {}개, 로봇 {}대",
                    model.bodies.len(),
                    model.robots.len()
                );
            } else {
                let mut all = changes.take().unwrap_or_default();
                all.changed.extend(more.changed);
                all.removed.extend(more.removed);
                server.update(shown, Some(&all));
                changes = Some(all);
            }
            checks.owe();
            owed = posed;
        }
        if checks.due() {
            let shown = owed.take();
            let shown = shown.as_ref().unwrap_or(link.document());
            let started = Instant::now();
            let contacts = stage.collider.contacts(shown, &stage.cell.held());
            server.state("contacts", &collide::contacts_json(shown, &contacts));
            checks.done(started.elapsed());
        }
        if replaced || changes.is_some() || any_asked || stage.ticking() {
            stage.tell(&server, link.document(), replaced);
        }
    }
}

/// What KYNEX adds to the session's scene: placed equipment, robots posed
/// and their programs, the cell's roles and its parts, and the collision
/// check over them all.
struct Stage {
    posing: Posing,
    cell: Cell,
    library: Library,
    collider: Collider,
    /// The session's scene with the placed equipment in it, while there is
    /// any: what robots are posed in and programs are made for.
    base: Option<Document>,
    /// What the viewers need to look at again for the equipment.
    placed: Changes,
    /// The programs running, a robot's each, and the clock they run by.
    runners: Vec<Runner>,
    /// When the viewers were last told, while a run goes on.
    ticked: Option<Instant>,
    clock: Clock,
    /// What the robot and the machines did since the run started, and when
    /// the viewers were last told.
    timeline: Option<Timeline>,
    timeline_told: Option<Instant>,
    /// Saved variants of the cell, the last comparison of them, and whether
    /// they changed since they were last saved.
    scenarios: Vec<Value>,
    compared: Vec<Value>,
    scenarios_changed: bool,
    /// A comparison running away from the loop: its results, when done.
    comparing: Option<Receiver<Vec<Value>>>,
    /// The PLC the cell's machines are wired to, and whether where it is
    /// changed since last saved.
    plc: crate::plc::Plc,
    plc_changed: bool,
}

/// How often the timeline goes to the viewers while it grows.
const TIMELINE_EVERY: std::time::Duration = std::time::Duration::from_millis(500);

impl Stage {
    /// From the project file `project`, or the default one.
    fn open(project: Option<PathBuf>) -> Result<Self, String> {
        let file = project.clone().unwrap_or_else(|| PathBuf::from(PROJECT));
        // The project file's forerunner, read once into it.
        let from = if project.is_none() && !file.exists() && Path::new(OLD_PROGRAMS).exists() {
            PathBuf::from(OLD_PROGRAMS)
        } else {
            file.clone()
        };
        let (posing, saved) = Posing::with_file(&from, &file)?;
        // Equipment placed, not the user's own kinds of it.
        let placed = saved.library.iter().filter(|v| v["id"].is_u64()).count();
        let counts = (posing.pending(), saved.cell.len(), placed);
        println!(
            "프로젝트 파일: {}{}",
            file.display(),
            match counts {
                (0, 0, 0) => String::new(),
                (p, r, l) => format!(" (프로그램 {p}개, 셀 역할 {r}개, 장비 {l}개)"),
            }
        );
        Ok(Self {
            posing,
            library: Library::from_saved(&saved.library),
            cell: {
                let mut cell = Cell::from_saved(saved.cell);
                if let Err(e) = cell.set_signals(&saved.plc["signals"]) {
                    println!("PLC 신호를 읽지 못했습니다: {e}");
                }
                cell
            },
            collider: Collider::default(),
            base: None,
            placed: Changes::default(),
            runners: Vec::new(),
            ticked: None,
            clock: Clock::default(),
            timeline: None,
            timeline_told: None,
            scenarios: saved.scenarios,
            compared: Vec::new(),
            scenarios_changed: false,
            comparing: None,
            // Where the PLC is, from the file; never linked to it until
            // someone at this computer says so: a project from elsewhere
            // does not drive a PLC on opening.
            plc: crate::plc::Plc::new(crate::plc::Settings {
                on: false,
                ..crate::plc::Settings::of(&saved.plc)
            }),
            plc_changed: false,
        })
    }

    /// Puts the placed equipment into the session's scene `doc` anew: after
    /// the scene or the equipment changed.
    fn rebase(&mut self, doc: &Document) {
        let mut base = (!self.library.items().is_empty()).then(|| doc.clone());
        let changes = self
            .library
            .apply(base.as_mut().unwrap_or(&mut Document::new()));
        self.placed.changed.extend(changes.changed);
        self.placed.removed.extend(changes.removed);
        for (uid, name, kind) in self.library.roles() {
            self.cell.ensure(uid, name, kind);
        }
        self.base = base;
    }

    /// A new scene: the programs and roles from the file may find their
    /// objects in it, and the programs are timed in it anew.
    fn replaced(&mut self, doc: &Document) {
        self.collider.clear();
        self.rebase(doc);
        let scene = self.base.as_ref().unwrap_or(doc);
        self.posing.adopt(scene);
        self.posing.programs_changed();
        self.cell.adopt(scene);
    }

    /// The scene as shown: the equipment in it, the robots posed and the
    /// cell's parts, or `None` when it is `doc` as it is; and what the
    /// viewers need to look at again besides what changed in `doc`.
    fn shown(&mut self, doc: &Document) -> (Option<Document>, Changes) {
        let scene = self.base.as_ref().unwrap_or(doc);
        let mut shown = self.posing.shown(scene).or_else(|| self.base.clone());
        let mut more = std::mem::take(&mut self.placed);
        let parts = if self.cell.needs_scene() {
            self.cell.apply(shown.get_or_insert_with(|| scene.clone()))
        } else {
            self.cell.untouched(scene)
        };
        more.changed.extend(parts.changed);
        more.removed.extend(parts.removed);
        (shown, more)
    }

    /// Answers the page's requests. Returns whether the viewers have
    /// anything to look at again.
    fn answer(&mut self, doc: &Document, asked: Vec<RobotRequest>) -> bool {
        let mut touched = false;
        for request in asked {
            let op = serde_json::from_str::<Value>(&request.body)
                .ok()
                .and_then(|v| v["op"].as_str().map(str::to_string))
                .unwrap_or_default();
            let robot = serde_json::from_str::<Value>(&request.body)
                .ok()
                .and_then(|v| v["robot"].as_str().and_then(|s| s.parse::<u64>().ok()));
            // Moving a robot by hand stops its program.
            if matches!(op.as_str(), "jog" | "ik" | "release" | "goto") {
                let before = self.runners.len();
                self.runners
                    .retain(|r| robot.is_some_and(|robot| robot != r.root));
                touched |= self.runners.len() != before;
            }
            let reply = if let Some(why) = (!request.local)
                .then(|| reaches_plc(&op, &request.body, self.plc.on()))
                .flatten()
            {
                serde_json::json!({ "ok": false, "error": why }).to_string()
            } else if op == "plc_link" {
                // Where the PLC is, and whether to be linked to it.
                let req: Value = serde_json::from_str(&request.body).unwrap_or_default();
                let mut settings = crate::plc::Settings::of(&req);
                // The server certificate trusted stays, unless the request
                // says otherwise (empty: trust the next one met).
                if req.get("trust").is_none() {
                    settings.trust = self.plc.settings.trust.clone();
                }
                self.plc.set(settings);
                self.plc_changed = true;
                touched = true;
                serde_json::json!({ "ok": true }).to_string()
            } else if op == "scenario" {
                touched = true;
                self.scenario_op(doc, &request.body)
            } else if matches!(op.as_str(), "play" | "stop" | "speed") {
                let answer = self.run_op(doc, &op, &request.body);
                touched = true;
                answer
            } else if op == "save_custom" {
                // An object of the scene, and what is below it, kept as
                // equipment of the user's own, its roles with it.
                let scene = self.base.as_ref().unwrap_or(doc);
                let req: Value = serde_json::from_str(&request.body).unwrap_or(Value::Null);
                let object = req["object"].as_str().and_then(|s| s.parse::<u64>().ok());
                let name = req["name"].as_str().unwrap_or("");
                let answer = match object.and_then(|o| {
                    scene
                        .object_by_uid(kreate_scene::Uid::from_raw(o))
                        .map(|id| (o, id))
                }) {
                    Some((o, id)) => {
                        let roles = self.cell.roles_under(scene, id);
                        match self.library.save_custom(scene, o, name, &roles) {
                            Ok(key) => serde_json::json!({ "ok": true, "key": key }),
                            Err(e) => serde_json::json!({ "ok": false, "error": e }),
                        }
                    }
                    None => serde_json::json!({ "ok": false, "error": "no such object" }),
                };
                touched = true;
                answer.to_string()
            } else if library::is_op(&op) {
                // The scene before, to carry the parts on moved equipment
                // with it, and take those on removed equipment away.
                let before = self.base.clone().unwrap_or_else(|| doc.clone());
                let (reply, roles) = self.library.handle(&request.body);
                self.cell.forget(&roles.stale);
                for (uid, name, kind) in roles.added {
                    self.cell.ensure(uid, name, kind);
                }
                for (uid, props) in &roles.props {
                    self.cell.set_props(*uid, props);
                }
                self.rebase(doc);
                let after = self.base.as_ref().unwrap_or(doc);
                self.cell.carry(&before, after);
                self.cell.forget(&roles.removed);
                touched = true;
                reply
            } else if cell::is_op(&op) {
                // Against the scene as shown: a grip is where the posed
                // robot's gripper is.
                let scene = self.base.as_ref().unwrap_or(doc);
                let posed = self.posing.shown(scene);
                let reply = self
                    .cell
                    .handle(posed.as_ref().unwrap_or(scene), &request.body);
                touched |= self.cell.take_changed();
                reply
            } else {
                let scene = self.base.as_ref().unwrap_or(doc);
                if op == "urscript" || op == "export" {
                    self.posing.cell_io = self.cell.export(scene);
                }
                let (reply, changed) = self.posing.handle(scene, &request.body);
                touched |= !changed.changed.is_empty();
                // The robots posed anew, for the viewers to draw.
                self.placed.changed.extend(changed.changed);
                self.placed.removed.extend(changed.removed);
                reply
            };
            // A page that gave up waiting has nobody to answer.
            let _ = request.reply.send(reply);
        }
        // The clock: the program running moves on, the machines run.
        let now = Instant::now();
        let time = self.clock.advance(now);
        let scene = self.base.as_ref().unwrap_or(doc);
        for runner in &mut self.runners {
            if runner.tick(time, now, &mut self.posing, &mut self.cell, scene) {
                touched = true;
                // The robot the program moves, drawn where it now is.
                crate::pose::mark(scene, runner.root, &mut self.placed);
            }
        }
        touched |= self.cell.tick(scene, time, now);
        // A PLC's commands given, the machines' states handed back.
        let mut starts: Vec<u64> = Vec::new();
        if self.plc.on() {
            let runners = &self.runners;
            let robot = |root: u64| {
                runners
                    .iter()
                    .find(|r| r.root == root)
                    .map_or((false, false), |r| (!r.stopped(), r.finished()))
            };
            let exchange = self.cell.plc_exchange(scene, &self.plc.read(), &robot);
            let errors = exchange.errors;
            // Robots the PLC starts and stops.
            for root in exchange.stop {
                self.runners.retain(|r| r.root != root);
                touched = true;
            }
            // Started once the scene is let go of, below.
            starts = exchange.start;
            self.plc.warn(errors);
            self.plc.hand(self.cell.plc_reads(), exchange.writes);
            // The server certificate met first is trusted, and kept.
            if self.plc.trust_on_first_use().is_some() {
                self.plc_changed = true;
            }
            touched |= self.cell.take_changed();
        }
        // A program going round is kept supplied, as an operator would:
        // raw parts on the infeed, finished parts off the outfeed — whole
        // pallets at a time for the pallets it goes through place by place.
        for runner in self.runners.iter().filter(|r| r.looping() && !r.stopped()) {
            self.cell.swap_pallets(scene, &runner.counted_pallets());
            touched |= self.cell.take_changed();
        }
        // A comparison done.
        if let Some(results) = &self.comparing {
            match results.try_recv() {
                Ok(compared) => {
                    self.compared = compared;
                    self.comparing = None;
                    touched = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.comparing = None;
                    touched = true;
                }
            }
        }
        // While a run goes on, the timeline grows with it.
        if let Some(timeline) = self.timeline.as_mut().filter(|_| !self.runners.is_empty()) {
            // The tick the last run stops on is recorded too: an alarm shows.
            if !timeline.ended() {
                let mut lanes: Vec<Now> = self
                    .runners
                    .iter()
                    .map(|runner| Now {
                        key: runner.root.to_string(),
                        name: crate::collide::label(scene, runner.root),
                        group: "robot",
                        activity: runner.activity(),
                    })
                    .collect();
                lanes.extend(self.cell.activities(scene).into_iter().map(
                    |(uid, name, activity)| Now {
                        key: uid.to_string(),
                        name,
                        group: "machine",
                        activity,
                    },
                ));
                timeline.sample(time, lanes);
                if self.runners.iter().all(|r| r.stopped()) {
                    timeline.end();
                }
            }
        }
        // Robots the PLC started.
        for root in starts {
            let name = crate::collide::label(self.base.as_ref().unwrap_or(doc), root);
            if let Err(e) = self.play(doc, root) {
                self.plc.warn(vec![format!("{name}: {e}")]);
            }
            touched = true;
        }
        touched
    }

    /// While a program runs or a PLC is linked, every so often whether or
    /// not anything moved:
    /// a pause counting down, a wait going on, shown as they go.
    fn ticking(&mut self) -> bool {
        // A PLC linked changes on its own: its state, its values.
        if !self.runners.iter().any(|r| !r.stopped()) && !self.plc.on() {
            return false;
        }
        let due = self
            .ticked
            .is_none_or(|t| t.elapsed() >= Duration::from_millis(250));
        if due {
            self.ticked = Some(Instant::now());
        }
        due
    }

    /// Whether anything runs by the clock: the loop then keeps time.
    fn running(&self) -> bool {
        self.runners.iter().any(|r| !r.stopped())
            || self.cell.is_running()
            || self.plc.on()
            || self.comparing.is_some()
    }

    /// Starts robot `root`'s program, from its start. The other robots'
    /// programs go on: a cell runs together. With none going on, the run
    /// is a new one, with a new record.
    fn play(&mut self, doc: &Document, root: u64) -> Result<(), String> {
        // Its calls replaced by what they call.
        let (program, origin) = self.posing.runnable(root)?;
        let scene = self.base.as_ref().unwrap_or(doc);
        let mut runner = Runner::new(root, program, scene, self.clock.time)?;
        runner.origin = origin;
        let alone = !self.runners.iter().any(|r| r.root != root && !r.stopped());
        self.runners.retain(|r| r.root != root && !r.stopped());
        self.runners.push(runner);
        // Parts to take: the infeed stocked as the run starts.
        self.cell.stock(scene);
        if alone {
            self.timeline = Some(Timeline::new(self.clock.time));
            self.timeline_told = None;
        }
        Ok(())
    }

    /// Runs a program, stops it, or sets the clock's speed.
    fn run_op(&mut self, doc: &Document, op: &str, body: &str) -> String {
        let req: Value = serde_json::from_str(body).unwrap_or_default();
        let answer = match op {
            "speed" => match req["speed"].as_f64() {
                Some(v) if v > 0.0 => {
                    self.clock.speed = v.clamp(0.1, 100.0);
                    Ok(())
                }
                _ => Err("speed is a positive number".to_string()),
            },
            // One robot's program, or every one's.
            "stop" => {
                match req["robot"].as_str().and_then(|s| s.parse::<u64>().ok()) {
                    Some(root) => self.runners.retain(|r| r.root != root),
                    None => self.runners.clear(),
                }
                Ok(())
            }
            _ => req["robot"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .ok_or_else(|| "no robot".to_string())
                .and_then(|root| self.play(doc, root)),
        };
        match answer {
            Ok(()) => serde_json::json!({ "ok": true }),
            Err(e) => serde_json::json!({ "ok": false, "error": e }),
        }
        .to_string()
    }

    /// Saves the cell as it stands as a scenario, takes one back, removes
    /// one, or runs them all and compares them.
    fn scenario_op(&mut self, doc: &Document, body: &str) -> String {
        let req: Value = serde_json::from_str(body).unwrap_or_default();
        let index = req["index"]
            .as_u64()
            .map(|i| i as usize)
            .filter(|i| *i < self.scenarios.len());
        let answer: Result<(), String> = match req["action"].as_str().unwrap_or_default() {
            "save" => {
                let name = req["name"].as_str().map(str::trim).unwrap_or_default();
                let name = if name.is_empty() {
                    format!("시나리오 {}", self.scenarios.len() + 1)
                } else {
                    name.to_string()
                };
                let scene = self.base.as_ref().unwrap_or(doc);
                let saved = scenario::snapshot(
                    &name,
                    self.posing.saved_programs(scene),
                    self.cell.to_saved(),
                    self.library.to_saved(),
                );
                // The same name again: saved over.
                match self
                    .scenarios
                    .iter()
                    .position(|s| s["name"] == name.as_str())
                {
                    Some(i) => self.scenarios[i] = saved,
                    None => self.scenarios.push(saved),
                }
                self.scenarios_changed = true;
                Ok(())
            }
            "load" => index.ok_or("no such scenario".to_string()).map(|i| {
                let s = self.scenarios[i].clone();
                let list = |key: &str| s[key].as_array().cloned().unwrap_or_default();
                self.runners.clear();
                self.timeline = None;
                self.library.restore(&list("library"));
                self.cell.restore(list("cell"));
                self.posing.restore(list("programs"));
                self.replaced(doc);
            }),
            "remove" => index.ok_or("no such scenario".to_string()).map(|i| {
                self.scenarios.remove(i);
                self.compared.clear();
                self.scenarios_changed = true;
            }),
            "run" if self.comparing.is_some() => Err("비교를 이미 실행하고 있습니다".into()),
            "run" => {
                let seconds = req["seconds"]
                    .as_f64()
                    .unwrap_or(3600.0)
                    .clamp(60.0, 86400.0);
                // Away from the loop: an hour of a few cells takes seconds.
                let (send, results) = std::sync::mpsc::channel();
                let (session, scenarios) = (doc.clone(), self.scenarios.clone());
                std::thread::spawn(move || {
                    let compared = scenarios
                        .iter()
                        .map(|s| scenario::simulate(&session, s, seconds))
                        .collect();
                    let _ = send.send(compared);
                });
                self.comparing = Some(results);
                Ok(())
            }
            other => Err(format!("unknown action {other:?}")),
        };
        match answer {
            Ok(()) => serde_json::json!({ "ok": true }),
            Err(e) => serde_json::json!({ "ok": false, "error": e }),
        }
        .to_string()
    }

    /// The `scenarios` event: those saved, and the last comparison.
    fn scenarios_json(&self) -> String {
        let count = |s: &Value, key: &str, kind: &str| {
            s[key].as_array().map_or(0, |l| {
                l.iter()
                    .filter(|v| kind.is_empty() || v["kind"] == kind)
                    .count()
            })
        };
        serde_json::json!({
            "list": self.scenarios.iter().map(|s| serde_json::json!({
                "name": s["name"],
                "machines": count(s, "cell", "cnc"),
                "programs": count(s, "programs", ""),
                "equipment": count(s, "library", ""),
            })).collect::<Vec<_>>(),
            "compared": self.compared,
            "comparing": self.comparing.is_some(),
        })
        .to_string()
    }

    /// Tells the viewers how the robots are posed here, their programs,
    /// the cell and the equipment, and saves what changed.
    fn tell(&mut self, server: &Server, doc: &Document, replaced: bool) {
        let scene = self.base.as_ref().unwrap_or(doc);
        let running = Value::Array(
            self.runners
                .iter()
                .map(|r| r.to_json(self.clock.time))
                .collect(),
        );
        server.state("pose", &self.posing.to_json(running, self.clock.to_json()));
        // At its pace while the run goes on; at once when it has ended, so
        // the last of it is shown.
        let ended = self.timeline.as_ref().is_some_and(|t| t.ended());
        let due = ended
            || self
                .timeline_told
                .is_none_or(|t| t.elapsed() >= TIMELINE_EVERY);
        if let Some(timeline) = self.timeline.as_ref().filter(|_| due) {
            server.state("timeline", &timeline.to_json().to_string());
            self.timeline_told = Some(Instant::now());
        }
        server.state("cell", &self.cell.to_json(scene));
        server.state("library", &self.library.to_json());
        server.state("templates", self.library.thumbnails());
        server.state("scenarios", &self.scenarios_json());
        let programs = self.posing.take_programs_changed();
        if programs {
            server.state(
                "programs",
                &self
                    .posing
                    .programs_json(scene, &mut self.collider, &self.cell),
            );
        }
        let roles = self.cell.take_roles_changed();
        let items = self.library.take_items_changed();
        let scenarios = std::mem::take(&mut self.scenarios_changed);
        let plc = std::mem::take(&mut self.plc_changed);
        server.state("plc", &self.plc.to_json().to_string());
        if programs || roles || items || scenarios || plc || replaced {
            let rest = Project {
                cell: self.cell.to_saved(),
                library: self.library.to_saved(),
                scenarios: self.scenarios.clone(),
                plc: {
                    let mut plc = self.plc.settings.to_json();
                    plc["signals"] = self.cell.signals_saved();
                    plc
                },
            };
            if let Err(e) = self.posing.save(scene, rest) {
                println!("프로젝트를 저장하지 못했습니다: {e}");
            }
        }
    }
}

/// When the scene's contacts are next worked out. After every change in a
/// small scene; in a large one, where a check takes tens of milliseconds,
/// at most a quarter of the time, so that the link and the page's requests
/// keep their pace. A change since the last check is always checked in the
/// end.
#[derive(Default)]
struct Checks {
    owed: bool,
    last: Option<Instant>,
    took: Duration,
}

impl Checks {
    fn owe(&mut self) {
        self.owed = true;
    }

    fn due(&self) -> bool {
        self.owed && self.last.is_none_or(|last| last.elapsed() >= self.took * 3)
    }

    fn done(&mut self, took: Duration) {
        self.owed = false;
        self.last = Some(Instant::now());
        self.took = took;
    }
}

/// A scene with no Kreate session: a saved file, or nothing at all for a
/// cell laid out from the library. Only what is asked of KYNEX changes it.
fn offline(
    server: &Server,
    requests: &Receiver<RobotRequest>,
    mut stage: Stage,
    doc: Document,
    label: &str,
) -> Result<(), String> {
    stage.replaced(&doc);
    let (posed, _) = stage.shown(&doc);
    let shown = posed.as_ref().unwrap_or(&doc);
    server.reset(shown);
    server.status(&Status::File { path: label.into() });
    let model = Model::from_document(shown);
    println!(
        "{label}: 물체 {}개, 로봇 {}대. Ctrl+C로 끝냅니다.",
        model.bodies.len(),
        model.robots.len()
    );
    stage.tell(server, &doc, true);
    loop {
        let first = if stage.running() {
            match requests.recv_timeout(Duration::from_millis(5)) {
                Ok(r) => Some(r),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            }
        } else {
            match requests.recv() {
                Ok(r) => Some(r),
                Err(_) => return Ok(()),
            }
        };
        let asked: Vec<RobotRequest> = first.into_iter().chain(requests.try_iter()).collect();
        let any_asked = !asked.is_empty();
        // Asked, or moved on by the clock: a door sliding, a cycle running.
        let touched = stage.answer(&doc, asked);
        if touched {
            let (posed, more) = stage.shown(&doc);
            let shown = posed.as_ref().unwrap_or(&doc);
            server.update(shown, Some(&more));
            let contacts = stage.collider.contacts(shown, &stage.cell.held());
            server.state("contacts", &collide::contacts_json(shown, &contacts));
        }
        if any_asked || touched || stage.ticking() {
            stage.tell(server, &doc, false);
        }
    }
}

/// Why request `op` (`body`) from another computer — a viewer shared with
/// `--lan` — is refused, if it could reach a PLC. While KYNEX is linked to
/// one, every request is: what changes the cell, its programs or its
/// machines can change what goes to the PLC, so another computer only
/// watches. Linked or not, so is what sets the PLC up for later: where it
/// is and what is wired to it, a program step on its signals, and a
/// scenario taken back, which brings programs and wiring with it. `None`
/// when it does not.
fn reaches_plc(op: &str, body: &str, linked: bool) -> Option<&'static str> {
    if linked {
        return Some("PLC에 연결된 동안 다른 컴퓨터에서는 보기만 할 수 있습니다");
    }
    if matches!(op, "plc_link" | "plc" | "signals" | "scenario") {
        return Some("PLC 설정과 시나리오 불러오기는 KYNEX를 실행한 컴퓨터에서만 할 수 있습니다");
    }
    if matches!(op, "add" | "edit") {
        let req: Value = serde_json::from_str(body).unwrap_or_default();
        let kind = req["step"]["type"].as_str().unwrap_or("");
        if kind.starts_with("signal") {
            return Some("PLC 신호 동작은 KYNEX를 실행한 컴퓨터에서만 넣을 수 있습니다");
        }
    }
    None
}

fn names(peers: &[(u16, String)]) -> Vec<String> {
    peers.iter().map(|(_, n)| n.clone()).collect()
}

/// This machine's address on the local network: the interface a packet to
/// the outside would leave by. Nothing is sent.
fn lan_address() -> String {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("192.0.2.1:9")?;
            s.local_addr()
        })
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".into())
}

/// The default browser, on whichever system this is.
pub(crate) fn open_browser(url: &str) {
    let result = if cfg!(target_os = "windows") {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(url).spawn()
    };
    if result.is_err() {
        println!("브라우저를 열지 못했습니다. 주소를 직접 여세요: {url}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// From another computer: while linked, nothing; linked or not, the
    /// PLC's settings, a program step on its signals, a scenario taken back;
    /// anything else, as before.
    #[test]
    fn another_computer_cannot_reach_the_plc() {
        assert!(reaches_plc("plc_link", "{}", false).is_some());
        assert!(reaches_plc("signals", "{}", false).is_some());
        assert!(reaches_plc("scenario", r#"{"action":"load"}"#, false).is_some());
        let set = r#"{"op":"add","step":{"type":"signal_set","name":"go","value":1}}"#;
        assert!(reaches_plc("add", set, false).is_some());
        let edit = r#"{"op":"edit","index":2,"step":{"type":"signal_wait","name":"go","value":1}}"#;
        assert!(reaches_plc("edit", edit, false).is_some());
        let delay = r#"{"op":"add","step":{"type":"delay","seconds":1}}"#;
        assert!(reaches_plc("add", delay, false).is_none());
        assert!(reaches_plc("play", "{}", false).is_none());
        assert!(reaches_plc("jog", "{}", false).is_none());
        for op in [
            "add",
            "play",
            "machine",
            "role",
            "place",
            "jog",
            "routine_new",
        ] {
            assert!(reaches_plc(op, delay, true).is_some(), "{op}");
        }
    }

    #[test]
    fn a_check_waits_three_times_as_long_as_the_last_took() {
        let mut checks = Checks::default();
        assert!(!checks.due(), "nothing owed");
        checks.owe();
        assert!(checks.due(), "the first at once");
        checks.done(Duration::from_millis(40));
        checks.owe();
        assert!(!checks.due(), "not 120 ms after one of 40 ms");
        // A quick one is due again at once.
        checks.done(Duration::ZERO);
        checks.owe();
        assert!(checks.due());
    }
}
