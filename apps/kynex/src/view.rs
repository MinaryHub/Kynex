//! `kynex view`: the link, drawn in a browser.

use std::time::Duration;

use kreate_scene::Changes;
use kynex_link::{KreateLink, LinkConfig, LinkEvent};
use kynex_model::Model;
use kynex_view::{Server, Status};

const PORT: u16 = 8090;

pub fn run(args: Vec<String>) -> Result<(), String> {
    let mut address = None;
    let mut key = std::env::var("KYNEX_KEY").unwrap_or_default();
    let mut name = None;
    let mut port = PORT;
    let mut lan = false;
    let mut open = false;
    let mut file = None;
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

    if let Some(path) = file {
        return show_file(&server, &path);
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

    loop {
        let events = link.wait(Duration::from_millis(250));
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
        if link.is_live() {
            server.status(&Status::Live {
                address: addr.clone(),
                peers: names(&peers),
            });
        }
        if replaced {
            let model = Model::from_document(link.document());
            server.reset(link.document());
            println!(
                "장면을 받았습니다: 물체 {}개, 로봇 {}대",
                model.bodies.len(),
                model.robots.len()
            );
        }
        if let Some(changes) = changes {
            server.update(link.document(), Some(&changes));
        }
    }
}

fn show_file(server: &Server, path: &str) -> Result<(), String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
    let loaded = kreate_io::container::read(std::io::BufReader::new(file))
        .map_err(|e| format!("{path}: {e}"))?;
    let model = Model::from_document(&loaded.document);
    server.reset(&loaded.document);
    server.status(&Status::File { path: path.into() });
    println!(
        "{path}: 물체 {}개, 로봇 {}대. Ctrl+C로 끝냅니다.",
        model.bodies.len(),
        model.robots.len()
    );
    loop {
        std::thread::park();
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
