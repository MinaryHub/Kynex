//! The viewer's HTTP server: the page and its scripts, and `/events`, a
//! Server-Sent Events stream of the scene.
//!
//! SSE rather than WebSocket because the viewer only listens: it is plain
//! HTTP that any browser reconnects by itself, and it needs nothing past the
//! standard library to serve. One thread per open browser tab; a studio or a
//! cell has a handful.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use kreate_scene::{Changes, Document};
use kynex_model::Model;
use serde_json::json;

use crate::assets;
use crate::scene::ViewState;

/// How often an idle stream says something, so a closed tab is noticed and
/// a proxy does not time the stream out.
const KEEPALIVE: Duration = Duration::from_secs(15);

/// Where the scene the viewer shows comes from, and how that is going.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// In a Kreate session.
    Live { address: String, peers: Vec<String> },
    /// The session is there but holds no scene yet.
    Waiting { address: String },
    /// Not connected; trying again.
    Offline { address: String, reason: String },
    /// A saved file, not a session.
    File { path: String },
}

impl Status {
    fn to_json(&self) -> String {
        match self {
            Status::Live { address, peers } => {
                json!({"state": "live", "address": address, "peers": peers})
            }
            Status::Waiting { address } => json!({"state": "waiting", "address": address}),
            Status::Offline { address, reason } => {
                json!({"state": "offline", "address": address, "reason": reason})
            }
            Status::File { path } => json!({"state": "file", "address": path}),
        }
        .to_string()
    }
}

/// A running viewer server.
pub struct Server {
    addr: SocketAddr,
    hub: Arc<Mutex<Hub>>,
}

struct Hub {
    view: ViewState,
    has_scene: bool,
    status: String,
    viewers: Vec<Sender<Arc<str>>>,
    /// Where a benchmark page's report goes, when one was asked for.
    bench: Option<Sender<String>>,
}

impl Hub {
    fn broadcast(&mut self, event: &str, data: &str) {
        let message: Arc<str> = frame(event, data).into();
        self.viewers.retain(|v| v.send(message.clone()).is_ok());
    }
}

fn frame(event: &str, data: &str) -> String {
    format!("event: {event}\ndata: {data}\n\n")
}

impl Server {
    /// Starts serving on `bind`, e.g. `127.0.0.1:8090` (port 0 picks one).
    pub fn start(bind: &str) -> io::Result<Self> {
        let listener = TcpListener::bind(bind)?;
        let addr = listener.local_addr()?;
        let hub = Arc::new(Mutex::new(Hub {
            view: ViewState::new(),
            has_scene: false,
            status: Status::Waiting {
                address: String::new(),
            }
            .to_json(),
            viewers: Vec::new(),
            bench: None,
        }));
        {
            let hub = hub.clone();
            thread::Builder::new()
                .name("kynex-view".into())
                .spawn(move || {
                    for stream in listener.incoming().flatten() {
                        let hub = hub.clone();
                        let _ = thread::Builder::new()
                            .name("kynex-view-client".into())
                            .spawn(move || {
                                let _ = serve(stream, &hub);
                            });
                    }
                })?;
        }
        Ok(Self { addr, hub })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// How many browser tabs are watching.
    pub fn viewers(&self) -> usize {
        self.hub.lock().map(|h| h.viewers.len()).unwrap_or(0)
    }

    /// A whole new scene: every viewer starts again from it.
    pub fn reset(&self, doc: &Document, model: &Model) {
        let mut hub = self.hub.lock().expect("viewer hub");
        hub.view.reset(doc, model);
        hub.has_scene = true;
        let scene = hub.view.scene();
        hub.broadcast("scene", &scene);
    }

    /// An edit to the scene the viewers have. See [`ViewState::update`].
    pub fn update(&self, doc: &Document, model: &Model, changes: Option<&Changes>) {
        let mut hub = self.hub.lock().expect("viewer hub");
        if !hub.has_scene {
            drop(hub);
            return self.reset(doc, model);
        }
        if let Some(update) = hub.view.update(doc, model, changes) {
            hub.broadcast("update", &update);
        }
    }

    /// Takes reports from the viewer's benchmark mode (`/?bench`): each
    /// arrives as the JSON the page posted to `/bench`. Until this is called
    /// the server takes no posts at all.
    pub fn bench_reports(&self) -> Receiver<String> {
        let (tx, rx) = mpsc::channel();
        self.hub.lock().expect("viewer hub").bench = Some(tx);
        rx
    }

    pub fn status(&self, status: &Status) {
        let mut hub = self.hub.lock().expect("viewer hub");
        let json = status.to_json();
        if hub.status != json {
            hub.status = json.clone();
            hub.broadcast("status", &json);
        }
    }
}

/// One connection: a file, or the stream.
fn serve(stream: TcpStream, hub: &Mutex<Hub>) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request = String::new();
    reader.read_line(&mut request)?;
    // The headers: only a post's length matters here.
    let mut line = String::new();
    let mut read = 0usize;
    let mut length = 0usize;
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        read += n;
        if n == 0 || line == "\r\n" || line == "\n" || read > 64 * 1024 {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                length = value.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut parts = request.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("/");
    let path = path.split('?').next().unwrap_or("/");
    let mut out = stream;
    if method == "POST" && path == "/bench" {
        let Some(bench) = hub.lock().expect("viewer hub").bench.clone() else {
            return respond(&mut out, "404 Not Found", "text/plain", b"not benchmarking");
        };
        if length > 1 << 20 {
            return respond(
                &mut out,
                "413 Payload Too Large",
                "text/plain",
                b"too large",
            );
        }
        let mut body = vec![0u8; length];
        reader.read_exact(&mut body)?;
        let _ = bench.send(String::from_utf8_lossy(&body).into_owned());
        return respond(&mut out, "204 No Content", "text/plain", b"");
    }
    if method != "GET" {
        return respond(
            &mut out,
            "405 Method Not Allowed",
            "text/plain",
            b"GET only",
        );
    }
    if path == "/events" {
        return stream_events(out, hub);
    }
    match assets::get(path) {
        Some((mime, body)) => respond(&mut out, "200 OK", mime, body),
        None => respond(&mut out, "404 Not Found", "text/plain", b"not found"),
    }
}

fn respond(out: &mut TcpStream, status: &str, mime: &str, body: &[u8]) -> io::Result<()> {
    write!(
        out,
        "HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\n\
         Cache-Control: no-cache\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    out.write_all(body)?;
    out.flush()
}

fn stream_events(mut out: TcpStream, hub: &Mutex<Hub>) -> io::Result<()> {
    out.set_read_timeout(None)?;
    out.set_nodelay(true)?;
    out.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\n\
          Connection: keep-alive\r\n\r\n",
    )?;
    // What there is now, then everything from here on — under the one lock,
    // so no edit falls between the two.
    let (tx, rx): (Sender<Arc<str>>, Receiver<Arc<str>>) = mpsc::channel();
    {
        let mut hub = hub.lock().expect("viewer hub");
        out.write_all(frame("status", &hub.status).as_bytes())?;
        if hub.has_scene {
            out.write_all(frame("scene", &hub.view.scene()).as_bytes())?;
        }
        out.flush()?;
        hub.viewers.push(tx);
    }
    loop {
        match rx.recv_timeout(KEEPALIVE) {
            Ok(message) => out.write_all(message.as_bytes())?,
            Err(RecvTimeoutError::Timeout) => out.write_all(b": keepalive\n\n")?,
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
        }
        out.flush()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn get(addr: SocketAddr, path: &str) -> String {
        let mut s = TcpStream::connect(addr).unwrap();
        write!(s, "GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    }

    #[test]
    fn the_page_and_its_scripts_are_served() {
        let server = Server::start("127.0.0.1:0").unwrap();
        let page = get(server.addr(), "/");
        assert!(page.starts_with("HTTP/1.1 200"));
        assert!(page.contains("text/html"));
        assert!(get(server.addr(), "/vendor/three.module.min.js").contains("javascript"));
        assert!(get(server.addr(), "/nope").starts_with("HTTP/1.1 404"));
    }

    #[test]
    fn a_viewer_gets_the_scene_then_each_edit() {
        let server = Server::start("127.0.0.1:0").unwrap();
        let mut doc = Document::new();
        let cube = doc.add_mesh_object("Cube", kreate_mesh::primitives::cube(1.0));
        server.reset(&doc, &Model::from_document(&doc));

        let mut s = TcpStream::connect(server.addr()).unwrap();
        write!(s, "GET /events HTTP/1.1\r\n\r\n").unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut reader = BufReader::new(s);
        let mut seen = String::new();
        let mut line = String::new();
        while !seen.contains("event: scene") {
            line.clear();
            reader.read_line(&mut line).unwrap();
            seen.push_str(&line);
        }
        while server.viewers() == 0 {
            thread::sleep(Duration::from_millis(5));
        }

        doc.object_mut(cube).unwrap().transform.translation.z = 1.5;
        server.update(&doc, &Model::from_document(&doc), None);
        loop {
            line.clear();
            reader.read_line(&mut line).unwrap();
            if line.starts_with("event: update") {
                break;
            }
        }
        line.clear();
        reader.read_line(&mut line).unwrap();
        let data: serde_json::Value =
            serde_json::from_str(line.trim_start_matches("data: ").trim()).unwrap();
        assert_eq!(data["objects"][0]["matrix"][14], 1.5);
    }
}
