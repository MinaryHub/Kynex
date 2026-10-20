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

/// The fastest the simulation's clock may run, times real time: a shift of
/// a factory in a couple of minutes, the cell still stepped finely.
const MOST_SPEED: f64 = 300.0;

/// Where programs were kept before the project file held more: read once,
/// if the project file is not there yet.
const OLD_PROGRAMS: &str = "kynex-programs.json";

pub fn run(args: Vec<String>) -> Result<(), String> {
    run_with(args, None)
}

/// [`run`], saying where the viewer is on `ready` once it is served (for
/// KYNEX's own window, `crate::studio`).
pub fn run_with(
    args: Vec<String>,
    ready: Option<std::sync::mpsc::Sender<Result<String, String>>>,
) -> Result<(), String> {
    let mut address = None;
    let mut key = std::env::var("KYNEX_KEY").unwrap_or_default();
    let mut name = None;
    let mut port = PORT;
    let mut lan = false;
    let mut open = false;
    let mut file = None;
    let mut kreate = false;
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
            // No Kreate: a cell laid out from the library alone. What
            // KYNEX does anyway unless asked to join a Kreate session.
            "--empty" => {}
            // Kreate's session on this machine, at its usual port.
            "--kreate" => kreate = true,
            // `--programs` is the old name, from before the file held more.
            "--project" | "--programs" => {
                project = Some(it.next().ok_or("--project needs a path")?.into())
            }
            a if a.starts_with('-') => return Err(format!("unknown option {a:?}")),
            a => address = Some(a.to_string()),
        }
    }

    // The project first: one another KYNEX holds is not served at all.
    let mut stage = Stage::open(project)?;

    // Only this machine unless asked: the viewer shows the whole scene to
    // whoever can reach the port.
    let bind = if lan {
        format!("0.0.0.0:{port}")
    } else {
        format!("127.0.0.1:{port}")
    };
    let server = Server::start(&bind).map_err(|e| format!("could not listen on {bind}: {e}"))?;
    // The address served on, not `localhost`: that may be IPv6's ::1 first,
    // where another program could be listening on the same port and be
    // shown in KYNEX's window or the browser in its place.
    let local = format!("http://127.0.0.1:{}", server.addr().port());
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
    if let Some(ready) = ready {
        let _ = ready.send(Ok(local.clone()));
    }

    let requests = server.robot_requests();
    if let Some(path) = file {
        let file = std::fs::File::open(&path).map_err(|e| format!("{path}: {e}"))?;
        let loaded = kreate_io::container::read(std::io::BufReader::new(file))
            .map_err(|e| format!("{path}: {e}"))?;
        let status = Status::File { path: path.clone() };
        return offline(&server, &requests, stage, loaded.document, &path, status);
    }
    // KYNEX on its own, unless a Kreate session is named.
    if address.is_none() && !kreate {
        return offline(
            &server,
            &requests,
            stage,
            Document::new(),
            "단독 실행",
            Status::Alone,
        );
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
        let touched = stage.answer(link.document(), asked);
        // Another project opened: everything shown anew.
        if stage.take_reset() {
            replaced = true;
            changes = None;
        } else if touched && !replaced {
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

/// A model being read: where it comes when read, where its answer goes,
/// and whether it is placed where it stood in its file.
type Importing = (
    Receiver<Result<crate::import::Imported, String>>,
    std::sync::mpsc::Sender<String>,
    bool,
);

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
    /// The site's drawing under the scene; whether it changed since saved,
    /// and since the viewers were told.
    floor: crate::floor::Floor,
    floor_changed: bool,
    floor_told: bool,
    /// The project file as last saved on purpose — as it was opened, or
    /// when 저장 was last pressed —, none if it was not there: what it goes
    /// back to when another is opened without saving this one. It is
    /// written whenever anything changes, all the same, so that nothing is
    /// lost to a crash.
    kept: Option<String>,
    /// When the cell was last told, and whether the page has asked for
    /// anything since: told at once if it has.
    cell_told: Option<Instant>,
    asked: bool,
    /// Saved variants of the cell, the last comparison of them, and whether
    /// they changed since they were last saved.
    scenarios: Vec<Value>,
    compared: Vec<Value>,
    scenarios_changed: bool,
    /// A comparison running away from the loop: its results, when done.
    comparing: Option<Receiver<Vec<Value>>>,
    /// A sweep running there (what it varies, its results to come), and
    /// the last one done.
    sweeping: Option<(Value, Receiver<Result<Vec<Value>, String>>)>,
    swept: Value,
    /// The PLC the cell's machines are wired to, and whether where it is
    /// changed since last saved.
    plc: crate::plc::Plc,
    plc_changed: bool,
    /// The real machines the cell's follow, by machine (`crate::mtconnect`).
    mirrors: std::collections::BTreeMap<u64, crate::mtconnect::Mirror>,
    /// The real robots followed, by robot (`crate::rtde`): their joints as
    /// last shown, and each grip point's output as last seen.
    followers: std::collections::BTreeMap<u64, crate::rtde::Follow>,
    /// The AMRs following real ones over MQTT (`crate::mqtt`), by AMR.
    feeds: std::collections::BTreeMap<u64, (crate::mtconnect::Source, crate::mqtt::Feed)>,
    followed: std::collections::BTreeMap<u64, [f64; 6]>,
    /// The robots carrying a process flow's parts (see `crate::carry`).
    carrying: crate::carry::Carry,
    follow_grips: std::collections::BTreeMap<u64, bool>,
    /// The project file, and whether another was just opened in its place.
    file: PathBuf,
    reset: bool,
    /// Where projects are made, opened and listed (`projects_dir`).
    projects: PathBuf,
    /// Edits to undo and to redo.
    history: History,
    /// The project file held for this KYNEX while it is open.
    _lock: Option<std::fs::File>,
    /// Models being read, each with where its answer goes.
    importing: Vec<Importing>,
}

/// The project as it was before each edit, to go back to, and as it was
/// before each undo, to go forward to again.
#[derive(Default)]
struct History {
    undo: Vec<(Value, &'static str)>,
    redo: Vec<(Value, &'static str)>,
    /// The last edit: what it was to, and when. More of the same within
    /// [`ONE_EDIT`] — a handle dragged, a number typed — is one edit.
    last: Option<(String, Instant)>,
}

/// How many edits can be undone.
const MOST_UNDONE: usize = 100;
/// How close together edits of one thing are one edit.
const ONE_EDIT: Duration = Duration::from_millis(1500);

/// How often the timeline goes to the viewers while it grows.
const TIMELINE_EVERY: std::time::Duration = std::time::Duration::from_millis(500);
/// How often the cell is told while only the clock moves it on: a big
/// flow's state is long to write.
const CELL_EVERY: std::time::Duration = std::time::Duration::from_millis(100);

impl Stage {
    /// From the project file `project`, or the default one.
    fn open(project: Option<PathBuf>) -> Result<Self, String> {
        // The one named, or else the first free of the recent ones.
        let (file, lock) = match &project {
            Some(file) => {
                if let Some(dir) = file.parent().filter(|d| !d.as_os_str().is_empty()) {
                    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                }
                (file.clone(), crate::paths::hold(file)?)
            }
            None => crate::paths::open_default()?,
        };
        crate::paths::remember(&file);
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
            cell_told: None,
            asked: false,
            floor: crate::floor::Floor::of(&saved.floor),
            floor_changed: false,
            floor_told: false,
            kept: std::fs::read_to_string(&file).ok(),
            scenarios: saved.scenarios,
            compared: Vec::new(),
            scenarios_changed: false,
            comparing: None,
            sweeping: None,
            swept: Value::Null,
            // Where the PLC is, from the file; never linked to it until
            // someone at this computer says so: a project from elsewhere
            // does not drive a PLC on opening.
            plc: crate::plc::Plc::new(crate::plc::Settings {
                on: false,
                ..crate::plc::Settings::of(&saved.plc)
            }),
            plc_changed: false,
            mirrors: Default::default(),
            followers: Default::default(),
            feeds: Default::default(),
            followed: Default::default(),
            carrying: Default::default(),
            follow_grips: Default::default(),
            file,
            reset: false,
            projects: crate::paths::projects_dir(),
            history: History::default(),
            importing: Vec::new(),
            _lock: lock,
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
        self.asked |= !asked.is_empty();
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
            // An edit can be undone: the project as it was before it.
            let before = undoable(&op).then(|| self.snapshot(doc));
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
            } else if op == "undo" || op == "redo" {
                touched = true;
                self.history_op(doc, op == "undo")
            } else if op == "project" {
                touched = true;
                self.project_op(doc, &request.body)
            } else if op == "scenario" {
                touched = true;
                self.scenario_op(doc, &request.body)
            } else if op == "floor" {
                touched = true;
                let req: Value = serde_json::from_str(&request.body).unwrap_or_default();
                match self.floor.handle(&req) {
                    Ok(()) => {
                        self.floor_changed = true;
                        self.floor_told = false;
                        serde_json::json!({ "ok": true }).to_string()
                    }
                    Err(e) => serde_json::json!({ "ok": false, "error": e }).to_string(),
                }
            } else if op == "follow" {
                touched = true;
                self.follow_op(doc, &request.body)
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
            } else if op == "import"
                && serde_json::from_str::<Value>(&request.body).unwrap_or_default()["as"] == "floor"
            {
                // The site's drawing: an image, laid under the scene.
                touched = true;
                let answer = match request.file.as_deref().map(|b| self.floor.set_image(b)) {
                    Some(Ok(())) => {
                        self.floor_changed = true;
                        self.floor_told = false;
                        serde_json::json!({ "ok": true })
                    }
                    Some(Err(e)) => serde_json::json!({ "ok": false, "error": e }),
                    None => serde_json::json!({ "ok": false, "error": "no file" }),
                };
                answer.to_string()
            } else if op == "import" {
                // A model file: read away from the loop, placed when read.
                match request.file {
                    Some(bytes) => {
                        self.import(&request.body, bytes, request.reply);
                        touched = true;
                        continue;
                    }
                    None => serde_json::json!({ "ok": false, "error": "no file" }).to_string(),
                }
            } else if library::is_op(&op) {
                touched = true;
                self.library_op(doc, &request.body)
            } else if cell::is_op(&op)
                && !(op == "release" && robot.is_some() && !request.body.contains("\"gripper\""))
            {
                // Against the scene as shown: a grip is where the posed
                // robot's gripper is. (A robot let go of — `release` with a
                // robot and no gripper — is the posing's, below.)
                let scene = self.base.as_ref().unwrap_or(doc);
                let posed = self.posing.shown(scene);
                let flowing = self.cell.flow_running();
                let reply = self
                    .cell
                    .handle(posed.as_ref().unwrap_or(scene), &request.body);
                touched |= self.cell.take_changed();
                // A process flow started with nothing else running: a new
                // record of it.
                if op == "flow"
                    && !flowing
                    && self.cell.flow_running()
                    && !self.runners.iter().any(|r| !r.stopped())
                {
                    self.timeline = Some(Timeline::new(self.clock.time));
                    self.timeline_told = None;
                }
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
            if let Some(before) = before {
                self.remember(doc, before, &op, &request.body);
            }
            // A page that gave up waiting has nobody to answer.
            let _ = request.reply.send(reply);
        }
        // Models read meanwhile, placed.
        touched |= self.imported(doc);
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
        self.cell.set_investment(self.library.investment());
        touched |= self.cell.tick(scene, time, now);
        // A process flow's warm-up over: its record begins again from here.
        if self.cell.take_warmed() {
            self.timeline = Some(Timeline::new(time));
            self.timeline_told = None;
            touched = true;
        }
        // Machines following real ones: their agents asked, their states
        // taken.
        let (feeds, sources): (Vec<_>, Vec<_>) = self
            .cell
            .mirror_sources()
            .into_iter()
            .partition(|(_, s)| matches!(s, crate::mtconnect::Source::Mqtt { .. }));
        // AMRs following real ones: their fleet's messages, their places.
        self.feeds
            .retain(|uid, (s, _)| feeds.iter().any(|(u, f)| u == uid && f == s));
        for (uid, source) in feeds {
            let crate::mtconnect::Source::Mqtt {
                broker,
                topic,
                user,
                password,
            } = &source
            else {
                continue;
            };
            let (_, feed) = self.feeds.entry(uid).or_insert_with(|| {
                (
                    source.clone(),
                    crate::mqtt::Feed::start(broker, user, password, topic),
                )
            });
            let seen = feed.rover();
            self.cell
                .follow_amr(scene, uid, seen.as_ref(), feed.to_json());
        }
        self.mirrors
            .retain(|uid, m| sources.iter().any(|(u, s)| u == uid && *s == m.source));
        for (uid, source) in sources {
            let link = self
                .mirrors
                .entry(uid)
                .or_insert_with(|| crate::mtconnect::Mirror::start(source));
            self.cell
                .mirror(uid, link.observation().as_ref(), time, link.to_json());
            // A umati server's certificate met first is trusted, and kept,
            // its link going on as it is.
            if let Some(thumbprint) = link.thumbprint() {
                if let Some(source) = self.cell.trust_mirror(uid, &thumbprint) {
                    link.source = source;
                }
            }
        }
        touched |= self.cell.take_changed();
        // Real robots followed: posed as they stand, their grippers as their
        // outputs say.
        touched |= {
            // The scene lent while the posing and the cell change.
            let base = self.base.take();
            let scene = base.as_ref().unwrap_or(doc);
            let followers = &self.followers;
            let carried = self.carrying.step(
                scene,
                time,
                &mut self.cell,
                &self.posing,
                &mut self.runners,
                &|root| followers.contains_key(&root),
            );
            let moved = self.follow_on(scene) | carried;
            self.base = base;
            moved
        };
        let scene = self.base.as_ref().unwrap_or(doc);
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
        // A sweep done.
        if let Some((what, results)) = &self.sweeping {
            match results.try_recv() {
                Ok(found) => {
                    let mut what = what.clone();
                    match found {
                        Ok(runs) => what["results"] = Value::Array(runs),
                        Err(e) => what["error"] = Value::String(e),
                    }
                    self.swept = what;
                    self.sweeping = None;
                    touched = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.sweeping = None;
                    touched = true;
                }
            }
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
        let flowing = self.cell.flow_running();
        if let Some(timeline) = self
            .timeline
            .as_mut()
            .filter(|_| !self.runners.is_empty() || flowing)
        {
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
                if self.runners.iter().all(|r| r.stopped()) && !flowing {
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
            || !self.mirrors.is_empty()
            || !self.followers.is_empty()
            || !self.feeds.is_empty()
            || self.comparing.is_some()
            || self.sweeping.is_some()
            || !self.importing.is_empty()
    }

    /// Starts robot `root`'s program, from its start. The other robots'
    /// programs go on: a cell runs together. With none going on, the run
    /// is a new one, with a new record.
    fn play(&mut self, doc: &Document, root: u64) -> Result<(), String> {
        if self.followers.contains_key(&root) {
            return Err("실제 로봇을 따라가는 중에는 프로그램을 실행하지 않습니다".into());
        }
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
                    self.clock.speed = v.clamp(0.1, MOST_SPEED);
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

    /// Follows a real UR robot over RTDE (`host`), or stops (no host).
    fn follow_op(&mut self, doc: &Document, body: &str) -> String {
        let req: Value = serde_json::from_str(body).unwrap_or_default();
        let scene = self.base.as_ref().unwrap_or(doc);
        let answer = (|| -> Result<(), String> {
            let root = req["robot"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .ok_or("no robot")?;
            let host = req["host"].as_str().map(str::trim).unwrap_or("");
            self.followers.remove(&root);
            self.followed.remove(&root);
            if host.is_empty() {
                return Ok(());
            }
            let kin = kynex_kin::Kinematics::of(scene, root).ok_or("no such robot")?;
            if kin.vars.len() != 6 {
                return Err("실제 로봇 따라가기는 6축 UR 로봇만 됩니다 (RTDE)".into());
            }
            let plain = host.len() <= 100
                && host
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '_'));
            if !plain {
                return Err("로봇 주소는 IP나 호스트 이름입니다 (예: 192.168.0.10)".into());
            }
            // The program shown stops: the robot is the real one's now.
            self.runners.retain(|r| r.root != root);
            self.followers
                .insert(root, crate::rtde::Follow::start(host));
            Ok(())
        })();
        match answer {
            Ok(()) => serde_json::json!({ "ok": true }),
            Err(e) => serde_json::json!({ "ok": false, "error": e }),
        }
        .to_string()
    }

    /// Each robot followed posed as the real one stands, and its grip
    /// points closed or opened as its outputs wired to them say. Returns
    /// whether anything moved.
    fn follow_on(&mut self, scene: &Document) -> bool {
        let mut touched = false;
        let io = if self.followers.is_empty() {
            return false;
        } else {
            self.cell.export(scene)
        };
        let roots: Vec<u64> = self.followers.keys().copied().collect();
        for root in roots {
            let Some(state) = self.followers[&root].state() else {
                continue;
            };
            let Some(kin) = kynex_kin::Kinematics::of(scene, root) else {
                continue;
            };
            if self.followed.get(&root) != Some(&state.q) {
                self.followed.insert(root, state.q);
                self.posing.hold_at(&kin, root, &state.q);
                crate::pose::mark(scene, root, &mut self.placed);
                touched = true;
            }
            for gripper in io.grippers.iter().filter(|g| kin.contains(**g)) {
                let Some(port) = io.port(*gripper, "close") else {
                    continue;
                };
                let closed = state.output(port);
                if self.follow_grips.get(gripper) == Some(&closed) {
                    continue;
                }
                let first = !self.follow_grips.contains_key(gripper);
                self.follow_grips.insert(*gripper, closed);
                // Seen first: only noted, not acted on.
                if first {
                    continue;
                }
                let posed = self.posing.shown(scene);
                let at = posed.as_ref().unwrap_or(scene);
                let _ = if closed {
                    self.cell.grip(at, *gripper)
                } else {
                    self.cell.release(at, *gripper)
                };
                touched |= self.cell.take_changed();
            }
        }
        touched
    }

    /// The robots followed, and how it is going, for the page.
    fn following_json(&self) -> Value {
        Value::Object(
            self.followers
                .iter()
                .map(|(root, f)| (root.to_string(), f.to_json()))
                .collect(),
        )
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
            "run" | "sweep" if self.comparing.is_some() || self.sweeping.is_some() => {
                Err("비교를 이미 실행하고 있습니다".into())
            }
            // One setting of one component, tried at each of some values,
            // on the cell as it stands.
            "sweep" => (|| {
                let object = req["object"].as_str().ok_or("no object")?.to_string();
                let prop = req["prop"].as_str().ok_or("no prop")?.to_string();
                let values: Vec<f64> = req["values"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_f64)
                    .filter(|v| v.is_finite())
                    .collect();
                if values.is_empty() || values.len() > scenario::MOST_VALUES {
                    return Err(format!("값은 1–{}개입니다", scenario::MOST_VALUES));
                }
                let seconds = req["seconds"]
                    .as_f64()
                    .unwrap_or(3600.0)
                    .clamp(60.0, 86400.0);
                let reps = req["reps"].as_u64().unwrap_or(1).clamp(1, 50) as usize;
                let scene = self.base.as_ref().unwrap_or(doc);
                let base = scenario::snapshot(
                    "",
                    self.posing.saved_programs(scene),
                    self.cell.to_saved(),
                    self.library.to_saved(),
                );
                let name = base["cell"]
                    .as_array()
                    .and_then(|c| c.iter().find(|r| r["object"] == object.as_str()))
                    .map(|r| r["name"].clone())
                    .ok_or("그 컴포넌트에는 셀 역할이 없습니다")?;
                let what = serde_json::json!({
                    "object": object, "name": name, "prop": prop,
                    "label": req["label"].as_str().unwrap_or(&prop),
                    "unit": req["unit"].as_str().unwrap_or_default(),
                    "scale": req["scale"].as_f64().unwrap_or(1.0),
                    "values": values,
                });
                let (send, results) = std::sync::mpsc::channel();
                let session = doc.clone();
                std::thread::spawn(move || {
                    let _ = send.send(scenario::sweep(
                        &session, &base, &object, &prop, &values, seconds, reps,
                    ));
                });
                self.sweeping = Some((what, results));
                Ok(())
            })(),
            "run" => {
                let seconds = req["seconds"]
                    .as_f64()
                    .unwrap_or(3600.0)
                    .clamp(60.0, 86400.0);
                // Away from the loop: an hour of a few cells takes seconds.
                // Each as often as asked, its own random numbers each time.
                let reps = req["reps"].as_u64().unwrap_or(1).clamp(1, 50) as usize;
                let (send, results) = std::sync::mpsc::channel();
                let (session, scenarios) = (doc.clone(), self.scenarios.clone());
                std::thread::spawn(move || {
                    let compared = scenarios
                        .iter()
                        .map(|s| scenario::replicate(&session, s, seconds, reps))
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
                // What works on parts: machines and stations.
                "machines": count(s, "cell", "cnc")
                    + count(s, "cell", "molder")
                    + count(s, "cell", "station"),
                "programs": count(s, "programs", ""),
                "equipment": count(s, "library", ""),
            })).collect::<Vec<_>>(),
            "compared": self.compared,
            "comparing": self.comparing.is_some(),
            "sweeping": self.sweeping.is_some(),
            "swept": self.swept,
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
        let mut pose: Value =
            serde_json::from_str(&self.posing.to_json(running, self.clock.to_json()))
                .unwrap_or_default();
        pose["following"] = self.following_json();
        server.state("pose", &pose.to_string());
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
        let due = replaced
            || std::mem::take(&mut self.asked)
            || self.cell_told.is_none_or(|t| t.elapsed() >= CELL_EVERY);
        if due {
            server.state("cell", &self.cell.to_json(scene));
            self.cell_told = Some(Instant::now());
        }
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
        server.state("project", &self.project_json().to_string());
        // The drawing only when it changed: it may be megabytes.
        if !self.floor_told {
            server.state("floor", &self.floor.to_json().to_string());
            self.floor_told = true;
        }
        let floor = std::mem::take(&mut self.floor_changed);
        server.state("history", &self.history_json().to_string());
        if programs || roles || items || scenarios || plc || floor || replaced {
            if let Err(e) = self.save(doc) {
                println!("프로젝트를 저장하지 못했습니다: {e}");
            }
        }
    }

    /// Writes the project file as things stand.
    fn save(&mut self, doc: &Document) -> Result<(), String> {
        let scene = self.base.as_ref().unwrap_or(doc);
        let rest = Project {
            cell: self.cell.to_saved(),
            library: self.library.to_saved(),
            scenarios: self.scenarios.clone(),
            floor: self.floor.to_json(),
            plc: {
                let mut plc = self.plc.settings.to_json();
                plc["signals"] = self.cell.signals_saved();
                plc
            },
        };
        self.posing.save(scene, rest)
    }

    /// The `project` event: the project file open, and where projects are
    /// kept.
    fn project_json(&self) -> Value {
        serde_json::json!({
            "name": self.file.file_stem().map(|s| s.to_string_lossy().to_string()),
            "path": self.file.display().to_string(),
            "dir": self.projects.display().to_string(),
        })
    }

    /// Makes a new project, opens one, or saves this one under another
    /// name; or lists those in the projects folder, and the demos that come
    /// with KYNEX; or opens a demo, as a copy of it in that folder. Only by
    /// name, in that folder: no path from a page reaches the file system.
    fn project_op(&mut self, doc: &Document, body: &str) -> String {
        let req: Value = serde_json::from_str(body).unwrap_or_default();
        let dir = self.projects.clone();
        // Opening another, this one saved first; or not, put back as it was
        // last saved.
        let keep = req["keep"] != false;
        let named = || -> Result<PathBuf, String> {
            let name = project_name(req["name"].as_str().unwrap_or(""))?;
            Ok(dir.join(format!("{name}.json")))
        };
        let answer: Result<Value, String> = match req["action"].as_str().unwrap_or_default() {
            "list" => {
                let mut projects = list_projects(&dir);
                // The one open, if it is one of these.
                for p in &mut projects {
                    let file = dir.join(format!("{}.json", p["name"].as_str().unwrap_or("")));
                    p["current"] = (file == self.file).into();
                }
                let demos: Vec<Value> = crate::demos::DEMOS
                    .iter()
                    .map(|d| serde_json::json!({ "name": d.name, "about": d.about }))
                    .collect();
                // And the samples, one for each thing KYNEX does, with the
                // level each is for.
                let samples: Vec<Value> = crate::demos::SAMPLES
                    .iter()
                    .map(|(level, d)| serde_json::json!({ "name": d.name, "about": d.about, "level": level }))
                    .collect();
                Ok(serde_json::json!({ "projects": projects, "demos": demos, "samples": samples }))
            }
            // A demo: copied into the projects folder, not over a project
            // of that name there, and opened.
            "demo" => crate::demos::demo(req["name"].as_str().unwrap_or(""))
                .ok_or_else(|| "그 이름의 예제가 없습니다".to_string())
                .and_then(|demo| crate::demos::copy(demo, &dir))
                .and_then(|file| {
                    let name = file.file_stem().map(|s| s.to_string_lossy().to_string());
                    self.switch(doc, &file, keep)?;
                    Ok(serde_json::json!({ "name": name }))
                }),
            "new" => named().and_then(|file| {
                if file.exists() {
                    return Err("같은 이름의 프로젝트가 이미 있습니다".into());
                }
                self.switch(doc, &file, keep)
            }),
            "open" => named().and_then(|file| {
                if !file.is_file() {
                    return Err("그 이름의 프로젝트가 없습니다".into());
                }
                self.switch(doc, &file, keep)
            }),
            // This project as it stands, kept: what it goes back to if
            // another is opened without saving it.
            "save" => self.save(doc).map(|()| {
                self.kept = std::fs::read_to_string(&self.file).ok();
                Value::Null
            }),
            "save_as" => named().and_then(|file| {
                if file.exists() {
                    return Err("같은 이름의 프로젝트가 이미 있습니다".into());
                }
                std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                // Held from now on in place of the old one.
                let lock = crate::paths::hold(&file)?;
                let before = std::mem::replace(&mut self.file, file.clone());
                self.posing.set_file(&file);
                if let Err(e) = self.save(doc) {
                    self.posing.set_file(&before);
                    self.file = before;
                    return Err(e);
                }
                self._lock = lock;
                self.kept = std::fs::read_to_string(&file).ok();
                crate::paths::remember(&file);
                println!("프로젝트를 다른 이름으로 저장했습니다: {}", file.display());
                Ok(Value::Null)
            }),
            other => Err(format!("unknown action {other:?}")),
        };
        match answer {
            Ok(Value::Null) => serde_json::json!({ "ok": true }),
            Ok(mut v) => {
                v["ok"] = true.into();
                v
            }
            Err(e) => serde_json::json!({ "ok": false, "error": e }),
        }
        .to_string()
    }

    /// Leaves this project, saved, for the one in `file` (made if it is not
    /// there): what ran stops, the PLC and the real machines are let go,
    /// and the viewers are shown the scene anew.
    fn switch(&mut self, doc: &Document, file: &Path, keep: bool) -> Result<Value, String> {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        // Read first: a file that cannot be read leaves this project open.
        let mut next = Stage::open(Some(file.to_path_buf()))?;
        next.projects = self.projects.clone();
        let left = if keep { self.save(doc) } else { self.discard() };
        if let Err(e) = left {
            println!("프로젝트를 저장하지 못했습니다: {e}");
        }
        *self = next;
        self.replaced(doc);
        self.save(doc)?;
        self.kept = std::fs::read_to_string(&self.file).ok();
        self.reset = true;
        Ok(Value::Null)
    }

    /// The project file put back as it was last saved on purpose, what
    /// changed since left out; gone, if it was not there then.
    fn discard(&self) -> Result<(), String> {
        let done = match &self.kept {
            Some(text) => std::fs::write(&self.file, text),
            None => match std::fs::remove_file(&self.file) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                other => other,
            },
        };
        done.map_err(|e| format!("{}: {e}", self.file.display()))?;
        println!("저장하지 않고 닫았습니다: {}", self.file.display());
        Ok(())
    }

    /// A request to the placed equipment (`library`), and the cell's roles
    /// it brings, moves or takes away.
    fn library_op(&mut self, doc: &Document, body: &str) -> String {
        // The scene before, to carry the parts on moved equipment with it,
        // and take those on removed equipment away.
        let before = self.base.clone().unwrap_or_else(|| doc.clone());
        let (reply, roles) = self.library.handle(body);
        self.cell.forget(&roles.stale);
        for (uid, name, kind) in roles.added {
            self.cell.ensure(uid, name, kind);
        }
        for (uid, props) in &roles.props {
            self.cell.set_props(*uid, props);
        }
        for (from, to) in &roles.copied {
            self.cell.copy_props(*from, *to);
        }
        for (uid, name) in &roles.renamed {
            self.cell.rename(*uid, name);
        }
        self.rebase(doc);
        let after = self.base.as_ref().unwrap_or(doc);
        self.cell.carry(&before, after);
        self.cell.forget(&roles.removed);
        reply
    }

    /// Reads model file `bytes` (named, and its unit, in `body`) on a thread
    /// of its own; [`Self::imported`] places it when read and answers
    /// `reply`.
    fn import(&mut self, body: &str, bytes: Vec<u8>, reply: std::sync::mpsc::Sender<String>) {
        let req: Value = serde_json::from_str(body).unwrap_or_default();
        let name = req["name"].as_str().unwrap_or("").to_string();
        let unit = req["unit"].as_f64();
        let keep = req["keep"] == true;
        let (send, read) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = send.send(crate::import::read_any(&name, &bytes, unit));
        });
        self.importing.push((read, reply, keep));
    }

    /// Models read since last asked: each kept as the user's own equipment
    /// and placed at the origin, one edit to undo. Whether any was.
    fn imported(&mut self, doc: &Document) -> bool {
        let mut any = false;
        let mut waiting = Vec::new();
        for (read, reply, keep) in std::mem::take(&mut self.importing) {
            let model = match read.try_recv() {
                Ok(model) => model,
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    waiting.push((read, reply, keep));
                    continue;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    Err("모델을 읽다가 멈췄습니다".to_string())
                }
            };
            let answer = model.and_then(|m| {
                let before = self.snapshot(doc);
                let mut at = [0.0; 3];
                let (key, m) = match m {
                    crate::import::Imported::Shape(m) => {
                        if keep {
                            at = [m.at.x, m.at.y, m.at.z];
                        }
                        (
                            self.library.save_custom(&m.doc, m.root, &m.name, &[])?,
                            m.name_notes(),
                        )
                    }
                    crate::import::Imported::Robot(r) => {
                        let name = r.name.clone();
                        (self.library.add_model(r), (name, Vec::new()))
                    }
                };
                let placed = self.library_op(
                    doc,
                    &serde_json::json!({"op": "place", "template": key, "at": at, "snap": false})
                        .to_string(),
                );
                let placed: Value = serde_json::from_str(&placed).unwrap_or_default();
                self.remember(doc, before, "import", "");
                Ok(serde_json::json!({
                    "ok": true,
                    "key": key,
                    "name": m.0,
                    "item": placed["item"],
                    "notes": m.1,
                    "at": at,
                }))
            });
            any = true;
            let answer = answer.unwrap_or_else(|e| serde_json::json!({ "ok": false, "error": e }));
            let _ = reply.send(answer.to_string());
        }
        self.importing = waiting;
        any
    }

    /// What undoing and redoing go back and forward to: the equipment, the
    /// cell's roles, the programs and the PLC's signals.
    fn snapshot(&mut self, doc: &Document) -> Value {
        let scene = self.base.as_ref().unwrap_or(doc);
        serde_json::json!({
            "programs": self.posing.saved_programs(scene),
            "cell": self.cell.to_saved(),
            "library": self.library.to_saved(),
            "signals": self.cell.signals_saved(),
            // As Visual Components' interact: what is dragged by hand undoes
            // too — robots posed, doors left open.
            "held": self.posing.held_json(),
            "doors": self.cell.doors_json(),
        })
    }

    /// Keeps `before` to undo to, if request `op` (`body`) changed the
    /// project. More of the same edit, close behind, adds nothing.
    fn remember(&mut self, doc: &Document, before: Value, op: &str, body: &str) {
        if self.snapshot(doc) == before {
            return;
        }
        let key = edit_key(op, body);
        let now = Instant::now();
        let h = &mut self.history;
        // Only what goes on and on is one edit: a handle dragged, sizes or a
        // step's numbers typed. Two points taught are two.
        let same = matches!(
            op,
            "move" | "move_many" | "resize" | "edit" | "jog" | "ik" | "door_to"
        ) && h
            .last
            .as_ref()
            .is_some_and(|(k, at)| *k == key && now.duration_since(*at) < ONE_EDIT);
        if !same || h.undo.is_empty() {
            h.undo.push((before, edit_label(op)));
            if h.undo.len() > MOST_UNDONE {
                h.undo.remove(0);
            }
        }
        h.last = Some((key, now));
        h.redo.clear();
    }

    /// Undoes the last edit, or redoes the last undone.
    fn history_op(&mut self, doc: &Document, undo: bool) -> String {
        let popped = if undo {
            self.history.undo.pop()
        } else {
            self.history.redo.pop()
        };
        let Some((state, label)) = popped else {
            let nothing = if undo {
                "되돌릴 것이 없습니다"
            } else {
                "다시 할 것이 없습니다"
            };
            return serde_json::json!({ "ok": false, "error": nothing }).to_string();
        };
        let now = self.snapshot(doc);
        if undo {
            self.history.redo.push((now, label));
        } else {
            self.history.undo.push((now, label));
        }
        self.history.last = None;
        self.restore(doc, &state);
        serde_json::json!({ "ok": true, "edit": label }).to_string()
    }

    /// Puts the project back as `state` (a [`Self::snapshot`]) has it. What
    /// runs stops; the robots stay as posed.
    fn restore(&mut self, doc: &Document, state: &Value) {
        let list = |key: &str| state[key].as_array().cloned().unwrap_or_default();
        // Only what differs: a pose undone leaves the cell, its parts in
        // the machines, as it is.
        let now = self.snapshot(doc);
        let differs = |key: &str| state[key] != now[key];
        if ["library", "cell", "signals", "programs"]
            .iter()
            .any(|k| differs(k))
        {
            self.runners.clear();
            self.timeline = None;
            if differs("library") {
                self.library.restore(&list("library"));
            }
            if differs("cell") || differs("library") {
                self.cell.restore(list("cell"));
            }
            if let Err(e) = self.cell.set_signals(&state["signals"]) {
                println!("PLC 신호를 되돌리지 못했습니다: {e}");
            }
            let actives = self.posing.undo_to(list("programs"));
            self.replaced(doc);
            self.posing.set_actives(&actives);
            self.plc_changed = true;
        }
        if differs("held") || differs("library") || differs("programs") {
            let scene = self.base.as_ref().unwrap_or(doc);
            for root in self.posing.set_held(&state["held"]) {
                crate::pose::mark(scene, root, &mut self.placed);
            }
        }
        if differs("doors") || differs("cell") || differs("library") {
            self.cell.set_doors(&state["doors"]);
        }
    }

    /// The `history` event: what undoing and redoing would do.
    fn history_json(&self) -> Value {
        serde_json::json!({
            "undo": self.history.undo.last().map(|(_, l)| l),
            "redo": self.history.redo.last().map(|(_, l)| l),
        })
    }

    /// Whether the project changed and the viewers must be shown all anew.
    fn take_reset(&mut self) -> bool {
        std::mem::take(&mut self.reset)
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
    status: Status,
) -> Result<(), String> {
    stage.replaced(&doc);
    let (posed, _) = stage.shown(&doc);
    let shown = posed.as_ref().unwrap_or(&doc);
    server.reset(shown);
    server.status(&status);
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
        // Another project opened: everything shown anew.
        let reset = stage.take_reset();
        if touched || reset {
            let (posed, more) = stage.shown(&doc);
            let shown = posed.as_ref().unwrap_or(&doc);
            if reset {
                server.reset(shown);
            } else {
                server.update(shown, Some(&more));
            }
            let contacts = stage.collider.contacts(shown, &stage.cell.held());
            server.state("contacts", &collide::contacts_json(shown, &contacts));
        }
        if any_asked || touched || reset || stage.ticking() {
            stage.tell(server, &doc, reset);
        }
    }
}

/// Why request `op` (`body`) from another computer — a viewer shared with
/// `--lan` — is refused, if it could reach a PLC. While KYNEX is linked to
/// one, every request is: what changes the cell, its programs or its
/// machines can change what goes to the PLC, so another computer only
/// watches. Linked or not, so is what sets the PLC up for later: where it
/// is and what is wired to it, a program step on its signals, and a
/// scenario or a project taken back, which brings programs and wiring with
/// it. `None`
/// when it does not.
fn reaches_plc(op: &str, body: &str, linked: bool) -> Option<&'static str> {
    if linked {
        return Some("PLC에 연결된 동안 다른 컴퓨터에서는 보기만 할 수 있습니다");
    }
    if matches!(
        op,
        "plc_link" | "plc" | "signals" | "scenario" | "mirror" | "follow" | "undo" | "redo"
    ) {
        return Some(
            "PLC·MTConnect 설정, 시나리오 불러오기, 되돌리기는 KYNEX를 실행한 컴퓨터에서만 할 수 있습니다",
        );
    }
    // Opening a project brings its PLC wiring and machines followed; and
    // its files are this computer's.
    if op == "project" {
        return Some("프로젝트 만들기·열기·저장은 KYNEX를 실행한 컴퓨터에서만 할 수 있습니다");
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

/// Whether request `op` can change the project, and so can be undone.
/// Moving robots by hand, running and looking change nothing kept.
fn undoable(op: &str) -> bool {
    !matches!(
        op,
        "" | "play"
            | "floor"
            | "flow"
            | "follow"
            | "stop"
            | "speed"
            | "export"
            | "urscript"
            | "reach"
            | "program"
            | "routine_select"
            | "undo"
            | "redo"
            | "project"
            | "plc_link"
            | "import"
    )
}

/// What an edit is to: its kind and the thing it touches. Edits to one
/// thing close together are one.
fn edit_key(op: &str, body: &str) -> String {
    let req: Value = serde_json::from_str(body).unwrap_or_default();
    let of = |k: &str| req[k].to_string();
    // Several moved together: the same ones.
    let moved: Vec<String> = req["moves"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| m["item"].to_string())
        .collect();
    format!(
        "{op} {} {} {} {} {}",
        of("item"),
        of("object"),
        of("robot"),
        of("index"),
        moved.join(",")
    )
}

/// An edit, as the page says it was undone or redone.
fn edit_label(op: &str) -> &'static str {
    match op {
        "place" => "장비 놓기",
        "move" | "move_many" => "장비 이동",
        "duplicate" => "장비 복제",
        "jog" | "ik" | "goto" | "release" => "로봇 자세",
        "door_to" => "문 열림",
        "import" => "모델 가져오기",
        "lock" => "장비 잠금",
        "rename" => "장비 이름",
        "resize" => "장비 크기·설정",
        "remove" => "장비 삭제",
        "save_custom" | "delete_custom" => "내 장비",
        "teach" => "지점 티칭",
        "retouch" => "지점 다시 티칭",
        "add" => "단계 추가",
        "edit" => "단계 수정",
        "delete" => "단계 삭제",
        "clear" => "프로그램 비우기",
        "shift" | "steps_move" => "단계 순서",
        "steps_paste" => "단계 붙여넣기",
        "copy" => "구간 복제",
        "edge" | "edge_path" => "모서리 경로",
        "routine_new" | "routine_rename" | "routine_delete" => "루틴",
        "role" | "auto" => "셀 역할",
        "plc" | "signals" | "io" => "신호 연결",
        "mirror" => "실제 장비 따라가기",
        "scenario" => "시나리오 불러오기",
        _ => "편집",
    }
}

/// A project's name as a file's in the projects folder may be: no folder
/// in it, nothing a file system takes for something else.
fn project_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    let bad = |c: char| c.is_control() || r#"/\:*?"<>|"#.contains(c);
    if name.is_empty() {
        return Err("프로젝트 이름을 넣으세요".into());
    }
    if name.chars().count() > 60 {
        return Err("프로젝트 이름은 60자까지입니다".into());
    }
    if name.chars().any(bad) || name.starts_with('.') || name.ends_with('.') {
        return Err(r#"프로젝트 이름에는 / \ : * ? " < > | 를 쓸 수 없고, 점으로 시작하거나 끝날 수 없습니다"#.into());
    }
    // Names Windows keeps for devices, with any extension.
    let stem = name
        .split('.')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_uppercase();
    let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit());
    if device {
        return Err("그 이름은 Windows가 장치 이름으로 씁니다".into());
    }
    Ok(name.to_string())
}

/// The projects in `dir`, the last changed first: name and when, in
/// seconds since 1970.
fn list_projects(dir: &Path) -> Vec<Value> {
    let mut found: Vec<(String, u64)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension()? != "json" || !entry.file_type().ok()?.is_file() {
                return None;
            }
            let name = path.file_stem()?.to_str()?.to_string();
            project_name(&name).ok()?;
            let modified = entry
                .metadata()
                .ok()?
                .modified()
                .ok()?
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            Some((name, modified))
        })
        .collect();
    found.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    found
        .into_iter()
        .take(500)
        .map(|(name, modified)| serde_json::json!({ "name": name, "modified": modified }))
        .collect()
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
    use serde_json::json;

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

    /// A request to the stage as the page sends it, from `local` or not.
    fn ask(stage: &mut Stage, doc: &Document, body: Value, local: bool) -> Value {
        let (reply, answer) = std::sync::mpsc::channel();
        stage.answer(
            doc,
            vec![RobotRequest {
                body: body.to_string(),
                reply,
                local,
                file: None,
            }],
        );
        serde_json::from_str(&answer.recv().unwrap()).unwrap()
    }

    /// As Visual Components' interact: a robot's joint dragged (many jogs,
    /// one edit) and a door dragged part open undo and redo like any edit;
    /// undoing a pose leaves the cell's parts where they are.
    #[test]
    fn what_is_dragged_by_hand_is_undone_too() {
        let dir = std::env::temp_dir().join(format!("kynex-undo-drag-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let doc = Document::new();
        let mut stage = Stage::open(Some(dir.join("drag.json"))).unwrap();
        stage.replaced(&doc);
        ask(
            &mut stage,
            &doc,
            json!({"op": "place", "template": "ur5e", "at": [0, 0, 0]}),
            true,
        );
        ask(
            &mut stage,
            &doc,
            json!({"op": "place", "template": "cnc_lathe", "at": [2, 0, 0]}),
            true,
        );
        let scene = stage.base.clone().unwrap();
        let robot = kynex_model::robots(&scene)[0].root;
        let kin = kynex_kin::Kinematics::of(&scene, robot).unwrap();
        let joint = kin.vars[0].uid;
        let machine = stage
            .cell
            .to_saved()
            .iter()
            .find(|c| c["kind"] == "cnc")
            .unwrap()["object"]
            .as_str()
            .unwrap()
            .to_string();
        let edits = stage.history.undo.len();

        for q in [0.2, 0.4, 0.6] {
            let jog = json!({"op": "jog", "robot": robot.to_string(), "joint": joint.to_string(), "position": q});
            assert_eq!(ask(&mut stage, &doc, jog, true)["ok"], true);
        }
        let door = json!({"op": "door_to", "object": machine, "open": 0.5});
        assert_eq!(ask(&mut stage, &doc, door, true)["ok"], true);
        assert_eq!(stage.history.undo.len(), edits + 2, "a drag, a door");
        assert_eq!(stage.history_json()["undo"], "문 열림");

        let undo = json!({"op": "undo"});
        assert_eq!(ask(&mut stage, &doc, undo.clone(), true)["edit"], "문 열림");
        assert_eq!(stage.cell.doors_json(), json!({}));
        assert_eq!(ask(&mut stage, &doc, undo, true)["edit"], "로봇 자세");
        assert_eq!(stage.posing.held_json(), json!({}), "as placed");
        let redo = json!({"op": "redo"});
        assert_eq!(
            ask(&mut stage, &doc, redo.clone(), true)["edit"],
            "로봇 자세"
        );
        let held = stage.posing.held_json();
        assert_eq!(held[robot.to_string()][joint.to_string()], 0.6);
        assert_eq!(ask(&mut stage, &doc, redo, true)["edit"], "문 열림");
        assert_eq!(stage.cell.doors_json()[&machine][0], 0.5);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Equipment placed, moved (twice, close together: one edit) and
    /// removed; each undone in turn and done again; a new edit leaves
    /// nothing to redo; never from another computer.
    #[test]
    fn edits_are_undone_and_redone() {
        let dir = std::env::temp_dir().join(format!("kynex-undo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let doc = Document::new();
        let mut stage = Stage::open(Some(dir.join("undo.json"))).unwrap();
        stage.replaced(&doc);
        let place = json!({"op": "place", "template": "cnc_lathe", "at": [0, 0, 0]});
        let item = ask(&mut stage, &doc, place, true)["item"].clone();
        let at = |stage: &Stage| {
            stage
                .library
                .to_saved()
                .iter()
                .find_map(|v| v["at"].as_array().cloned())
        };
        for x in [1.0, 2.0] {
            let moved = json!({"op": "move", "item": item, "at": [x, 0, 0]});
            assert_eq!(ask(&mut stage, &doc, moved, true)["ok"], true);
        }
        assert_eq!(
            ask(
                &mut stage,
                &doc,
                json!({"op": "remove", "item": item}),
                true
            )["ok"],
            true
        );
        assert_eq!(stage.history.undo.len(), 3, "placed, moved, removed");
        assert_eq!(stage.history_json()["undo"], "장비 삭제");

        let undo = json!({"op": "undo"});
        assert_eq!(ask(&mut stage, &doc, undo.clone(), false)["ok"], false);
        assert_eq!(
            ask(&mut stage, &doc, undo.clone(), true)["edit"],
            "장비 삭제"
        );
        assert_eq!(at(&stage).unwrap()[0], 2.0);
        assert_eq!(
            ask(&mut stage, &doc, undo.clone(), true)["edit"],
            "장비 이동"
        );
        assert_eq!(at(&stage).unwrap()[0], 0.0);
        assert_eq!(
            ask(&mut stage, &doc, undo.clone(), true)["edit"],
            "장비 놓기"
        );
        assert!(stage.library.items().is_empty());
        assert_eq!(ask(&mut stage, &doc, undo, true)["ok"], false);

        let redo = json!({"op": "redo"});
        assert_eq!(
            ask(&mut stage, &doc, redo.clone(), true)["edit"],
            "장비 놓기"
        );
        assert_eq!(
            ask(&mut stage, &doc, redo.clone(), true)["edit"],
            "장비 이동"
        );
        assert_eq!(at(&stage).unwrap()[0], 2.0);
        let again = json!({"op": "move", "item": item, "at": [3, 0, 0]});
        assert_eq!(ask(&mut stage, &doc, again, true)["ok"], true);
        assert_eq!(
            ask(&mut stage, &doc, redo, true)["ok"],
            false,
            "a new edit: nothing to redo"
        );
        // Two placed at once are two edits, not one.
        let edits = stage.history.undo.len();
        for x in [5, 6] {
            let place = json!({"op": "place", "template": "fence", "at": [x, 0, 0]});
            assert_eq!(ask(&mut stage, &doc, place, true)["ok"], true);
        }
        assert_eq!(stage.history.undo.len(), edits + 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Names a file may have in the projects folder, and those it may not.
    #[test]
    fn a_project_s_name_is_a_file_s_in_the_folder() {
        assert_eq!(project_name("  셀 A-1 (시험)  ").unwrap(), "셀 A-1 (시험)");
        for bad in [
            "",
            "   ",
            "../x",
            "a/b",
            r"a",
            "c:x",
            "a*",
            ".hidden",
            "end.",
            "CON",
            "com1",
            "LPT9.json",
            &"가".repeat(61),
        ] {
            assert!(project_name(bad).is_err(), "{bad:?}");
        }
        assert!(project_name("console").is_ok());
    }

    /// A project made (the one before saved), equipment placed in it, saved
    /// under another name,
    /// another made empty, and the first opened again with its equipment;
    /// never from another computer.
    #[test]
    fn projects_are_made_saved_as_and_opened_by_name() {
        let dir = std::env::temp_dir().join(format!("kynex-projects-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let doc = Document::new();
        let mut stage = Stage::open(Some(dir.join("start.json"))).unwrap();
        stage.projects = dir.clone();
        stage.replaced(&doc);

        let project =
            |action: &str, name: &str| json!({"op": "project", "action": action, "name": name});
        let remote = ask(&mut stage, &doc, project("list", ""), false);
        assert_eq!(remote["ok"], false, "{remote}");

        assert_eq!(
            ask(&mut stage, &doc, project("new", "셀 A"), true)["ok"],
            true
        );
        assert!(stage.take_reset());
        assert!(dir.join("셀 A.json").is_file());
        let placed = ask(
            &mut stage,
            &doc,
            json!({"op": "place", "template": "cnc_lathe", "at": [0, 0, 0]}),
            true,
        );
        assert_eq!(placed["ok"], true, "{placed}");
        assert_eq!(
            ask(&mut stage, &doc, project("save_as", "셀 B"), true)["ok"],
            true
        );
        assert!(!stage.take_reset(), "saved as: the same project goes on");
        let again = ask(&mut stage, &doc, project("new", "셀 B"), true);
        assert_eq!(again["ok"], false, "{again}");

        assert_eq!(
            ask(&mut stage, &doc, project("new", "빈 셀"), true)["ok"],
            true
        );
        assert!(stage.library.items().is_empty());
        let missing = ask(&mut stage, &doc, project("open", "없는 셀"), true);
        assert_eq!(missing["ok"], false, "{missing}");
        let outside = ask(&mut stage, &doc, project("open", "../start"), true);
        assert_eq!(outside["ok"], false, "{outside}");

        assert_eq!(
            ask(&mut stage, &doc, project("open", "셀 B"), true)["ok"],
            true
        );
        assert_eq!(stage.library.items().len(), 1);
        let list = ask(&mut stage, &doc, project("list", ""), true);
        let names: Vec<(&str, bool)> = list["projects"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| (p["name"].as_str().unwrap(), p["current"] == true))
            .collect();
        // The one started with saved as it was left, too.
        assert_eq!(names.len(), 4, "{list}");
        assert!(names.contains(&("start", false)));
        assert!(names.contains(&("셀 B", true)));
        assert!(names.contains(&("셀 A", false)) && names.contains(&("빈 셀", false)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Another project opened without saving this one: this one goes back
    /// to as it was last saved, what changed since left out; opened saving
    /// it, what changed is kept.
    #[test]
    fn another_project_opened_with_or_without_saving_this_one() {
        let dir = std::env::temp_dir().join(format!("kynex-keep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let doc = Document::new();
        let mut stage = Stage::open(Some(dir.join("가.json"))).unwrap();
        stage.projects = dir.clone();
        stage.replaced(&doc);
        let project = |action: &str, name: &str, keep: bool| json!({"op": "project", "action": action, "name": name, "keep": keep});
        let place = json!({"op": "place", "template": "flow_station", "at": [0, 0, 0]});
        let items = |file: &str| -> usize {
            let v: Value =
                serde_json::from_str(&std::fs::read_to_string(dir.join(file)).unwrap()).unwrap();
            v["library"].as_array().map_or(0, Vec::len)
        };
        // One item, saved; another after it, not.
        ask(&mut stage, &doc, place.clone(), true);
        assert_eq!(
            ask(&mut stage, &doc, project("save", "", true), true)["ok"],
            true
        );
        ask(&mut stage, &doc, place.clone(), true);
        stage.save(&doc).unwrap();
        assert_eq!(items("가.json"), 2);
        let opened = ask(&mut stage, &doc, project("new", "나", false), true);
        assert_eq!(opened["ok"], true, "{opened}");
        assert_eq!(items("가.json"), 1, "the second item left out");
        // Back, an item more, and away again saving it.
        ask(&mut stage, &doc, project("open", "가", true), true);
        ask(&mut stage, &doc, place, true);
        ask(&mut stage, &doc, project("open", "나", true), true);
        assert_eq!(items("가.json"), 2, "kept");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The demos that come with KYNEX listed beside the projects, and one
    /// opened as a copy in the projects folder — again, as another copy,
    /// the first left as it is.
    #[test]
    fn a_demo_opens_as_a_copy_in_the_projects_folder() {
        let dir = std::env::temp_dir().join(format!("kynex-demo-open-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let doc = Document::new();
        let mut stage = Stage::open(Some(dir.join("start.json"))).unwrap();
        stage.projects = dir.clone();
        stage.replaced(&doc);
        let project =
            |action: &str, name: &str| json!({"op": "project", "action": action, "name": name});
        let list = ask(&mut stage, &doc, project("list", ""), true);
        let demos: Vec<&str> = list["demos"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["name"].as_str().unwrap())
            .collect();
        assert_eq!(demos.len(), crate::demos::DEMOS.len(), "{list}");
        assert!(demos.contains(&"반도체 공장"));

        let opened = ask(&mut stage, &doc, project("demo", "LCD 공장"), true);
        assert_eq!(opened["ok"], true, "{opened}");
        assert_eq!(opened["name"], "LCD 공장");
        assert!(stage.take_reset());
        assert!(!stage.library.items().is_empty(), "its equipment");
        assert_eq!(stage.file, dir.join("LCD 공장.json"));
        let again = ask(&mut stage, &doc, project("demo", "LCD 공장"), true);
        assert_eq!(again["name"], "LCD 공장 2", "{again}");
        assert!(dir.join("LCD 공장.json").is_file());
        let missing = ask(&mut stage, &doc, project("demo", "없는 예제"), true);
        assert_eq!(missing["ok"], false, "{missing}");
        let _ = std::fs::remove_dir_all(&dir);
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
