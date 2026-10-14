//! `kynex view`: the link, drawn in a browser.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use std::sync::mpsc::{Receiver, RecvTimeoutError};

use kreate_scene::{Changes, Document};
use kynex_link::{KreateLink, LinkConfig, LinkEvent};
use kynex_model::Model;
use kynex_view::server::RobotRequest;
use kynex_view::{Server, Status};

use crate::collide::{self, Collider};
use crate::pose::Posing;

const PORT: u16 = 8090;

/// Where programs are kept between runs, unless `--programs` says.
const PROGRAMS: &str = "kynex-programs.json";

pub fn run(args: Vec<String>) -> Result<(), String> {
    let mut address = None;
    let mut key = std::env::var("KYNEX_KEY").unwrap_or_default();
    let mut name = None;
    let mut port = PORT;
    let mut lan = false;
    let mut open = false;
    let mut file = None;
    let mut programs = PathBuf::from(PROGRAMS);
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
            "--programs" => programs = it.next().ok_or("--programs needs a path")?.into(),
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
    let mut posing = Posing::with_file(&programs)?;
    println!(
        "프로그램 파일: {}{}",
        programs.display(),
        match posing.pending() {
            0 => String::new(),
            n => format!(" (프로그램 {n}개)"),
        }
    );
    if let Some(path) = file {
        return show_file(&server, &requests, posing, &path);
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

    let mut collider = Collider::default();
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
        // The page's requests: a pose asked for shows as soon as it is
        // worked out, so they are waited on rather than the link when both
        // are quiet.
        let mut asked: Vec<RobotRequest> = requests.try_iter().collect();
        if quiet && asked.is_empty() && !replaced && changes.is_none() {
            asked.extend(requests.recv_timeout(Duration::from_millis(3)).ok());
        }
        let any_asked = !asked.is_empty();
        let mut posed = answer(&mut posing, link.document(), asked);
        posed.changed.extend(posing.tick(Instant::now()).changed);
        if !posed.changed.is_empty() && !replaced {
            let all = changes.get_or_insert_with(Changes::default);
            all.changed.extend(posed.changed);
        }
        if replaced {
            // The programs are timed in the scene: a new one times them anew,
            // and may hold the robots of programs from the file.
            posing.adopt(link.document());
            posing.programs_changed();
            collider.clear();
        } else if let Some(changes) = &changes {
            collider.forget(changes);
        }
        if replaced || changes.is_some() {
            let posed = posing.shown(link.document());
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
                server.update(shown, changes.as_ref());
            }
            checks.owe();
            owed = posed;
        }
        if checks.due() {
            let shown = owed.take();
            let shown = shown.as_ref().unwrap_or(link.document());
            let started = Instant::now();
            let contacts = collider.contacts(shown);
            server.state("contacts", &collide::contacts_json(shown, &contacts));
            checks.done(started.elapsed());
        }
        if replaced || changes.is_some() || any_asked {
            tell(&server, &mut posing, &mut collider, link.document());
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

/// Tells the viewers how the robots are posed here, and their programs
/// when those changed.
fn tell(server: &Server, posing: &mut Posing, collider: &mut Collider, doc: &Document) {
    server.state("pose", &posing.to_json());
    if posing.take_programs_changed() {
        server.state("programs", &posing.programs_json(doc, collider));
        if let Err(e) = posing.save(doc) {
            println!("프로그램을 저장하지 못했습니다: {e}");
        }
    }
}

/// Answers the page's robot requests, and says what the viewers need to
/// look at again.
fn answer(posing: &mut Posing, doc: &Document, asked: Vec<RobotRequest>) -> Changes {
    let mut touched = Changes::default();
    for request in asked {
        let (reply, changed) = posing.handle(doc, &request.body);
        // A page that gave up waiting has nobody to answer.
        let _ = request.reply.send(reply);
        touched.changed.extend(changed.changed);
    }
    touched
}

fn show_file(
    server: &Server,
    requests: &Receiver<RobotRequest>,
    mut posing: Posing,
    path: &str,
) -> Result<(), String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
    let loaded = kreate_io::container::read(std::io::BufReader::new(file))
        .map_err(|e| format!("{path}: {e}"))?;
    let doc = loaded.document;
    let model = Model::from_document(&doc);
    server.reset(&doc);
    server.status(&Status::File { path: path.into() });
    println!(
        "{path}: 물체 {}개, 로봇 {}대. Ctrl+C로 끝냅니다.",
        model.bodies.len(),
        model.robots.len()
    );
    posing.adopt(&doc);
    let mut collider = Collider::default();
    tell(server, &mut posing, &mut collider, &doc);
    server.state(
        "contacts",
        &collide::contacts_json(&doc, &collider.contacts(&doc)),
    );
    // The file does not change: only a pose asked for, or a program
    // playing, does.
    loop {
        let first = if posing.is_playing() {
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
        let mut touched = answer(&mut posing, &doc, asked);
        touched.changed.extend(posing.tick(Instant::now()).changed);
        if !touched.changed.is_empty() {
            let shown = posing.shown(&doc);
            let shown = shown.as_ref().unwrap_or(&doc);
            server.update(shown, Some(&touched));
            let contacts = collider.contacts(shown);
            server.state("contacts", &collide::contacts_json(shown, &contacts));
        }
        if any_asked || !touched.changed.is_empty() {
            tell(server, &mut posing, &mut collider, &doc);
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
