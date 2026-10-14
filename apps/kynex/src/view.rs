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
        if replaced || changes.is_some() || any_asked {
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
}

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
        let counts = (posing.pending(), saved.cell.len(), saved.library.len());
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
            cell: Cell::from_saved(saved.cell),
            collider: Collider::default(),
            base: None,
            placed: Changes::default(),
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
            let reply = if library::is_op(&op) {
                let (reply, roles) = self.library.handle(&request.body);
                for (uid, name, kind) in roles.added {
                    self.cell.ensure(uid, name, kind);
                }
                self.cell.forget(&roles.removed);
                self.rebase(doc);
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
                let (reply, changed) = self.posing.handle(scene, &request.body);
                touched |= !changed.changed.is_empty();
                reply
            };
            // A page that gave up waiting has nobody to answer.
            let _ = request.reply.send(reply);
        }
        // A program playing moves the robot on; the machines run.
        let now = Instant::now();
        touched |= !self.posing.tick(now).changed.is_empty();
        touched |= self.cell.tick(self.base.as_ref().unwrap_or(doc), now);
        touched
    }

    /// Tells the viewers how the robots are posed here, their programs,
    /// the cell and the equipment, and saves what changed.
    fn tell(&mut self, server: &Server, doc: &Document, replaced: bool) {
        let scene = self.base.as_ref().unwrap_or(doc);
        server.state("pose", &self.posing.to_json());
        server.state("cell", &self.cell.to_json(scene));
        server.state("library", &self.library.to_json());
        let programs = self.posing.take_programs_changed();
        if programs {
            server.state(
                "programs",
                &self.posing.programs_json(scene, &mut self.collider),
            );
        }
        let roles = self.cell.take_roles_changed();
        let items = self.library.take_items_changed();
        if programs || roles || items || replaced {
            let rest = Project {
                cell: self.cell.to_saved(),
                library: self.library.to_saved(),
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
        let first = if stage.posing.is_playing() || stage.cell.is_running() {
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
        if any_asked || touched {
            stage.tell(server, &doc, false);
        }
    }
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
