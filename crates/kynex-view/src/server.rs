//! The viewer's HTTP server: the page and its scripts, and `/events`, a
//! Server-Sent Events stream of the scene.
//!
//! SSE rather than WebSocket because the viewer only listens: it is plain
//! HTTP that any browser reconnects by itself, and it needs nothing past the
//! standard library to serve. One thread per open browser tab; a studio or a
//! cell has a handful.

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use kreate_scene::{Changes, Document};
use serde_json::json;

use crate::assets;
use crate::scene::ViewState;

/// How often an idle stream says something, so a closed tab is noticed and
/// a proxy does not time the stream out.
const KEEPALIVE: Duration = Duration::from_secs(15);

/// The most bytes a request line and its headers may take.
const HEAD_LIMIT: u64 = 16 * 1024;

/// The most connections served at once: open viewer tabs, each holding its
/// stream, and the page loads beside them.
const MAX_CONNECTIONS: usize = 64;

/// The most of those one machine may hold: a few tabs and their page loads.
const MAX_PER_PEER: usize = 16;

/// How long a request, headers and body, may take to arrive.
const REQUEST_DEADLINE: Duration = Duration::from_secs(10);

/// How long a write may wait on a peer that is not reading.
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

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
                    let gate = Arc::new(Mutex::new(Gate::default()));
                    for stream in listener.incoming().flatten() {
                        // A thread per connection, so a bounded number of
                        // them, and a share of those per peer: past either,
                        // a connection is closed at the door.
                        let Ok(peer) = stream.peer_addr().map(|a| a.ip()) else {
                            continue;
                        };
                        if !Gate::enter(&gate, peer) {
                            drop(stream);
                            continue;
                        }
                        let hub = hub.clone();
                        let done = gate.clone();
                        let spawned = thread::Builder::new()
                            .name("kynex-view-client".into())
                            .spawn(move || {
                                let _ = serve(stream, &hub);
                                Gate::leave(&done, peer);
                            });
                        if spawned.is_err() {
                            Gate::leave(&gate, peer);
                        }
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
    pub fn reset(&self, doc: &Document) {
        let mut hub = self.hub.lock().expect("viewer hub");
        hub.view.reset(doc);
        hub.has_scene = true;
        let scene = hub.view.scene();
        hub.broadcast("scene", &scene);
    }

    /// An edit to the scene the viewers have. See [`ViewState::update`].
    pub fn update(&self, doc: &Document, changes: Option<&Changes>) {
        let mut hub = self.hub.lock().expect("viewer hub");
        if !hub.has_scene {
            drop(hub);
            return self.reset(doc);
        }
        if let Some(update) = hub.view.update(doc, changes) {
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

/// A reader that gives up at a point in time rather than after a quiet
/// spell: each read waits only for what is left until `until`.
struct Deadline {
    stream: TcpStream,
    until: Instant,
}

impl Read for Deadline {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let left = self.until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "request too slow"));
        }
        self.stream.set_read_timeout(Some(left))?;
        self.stream.read(buf)
    }
}

/// Who holds how many connections, so that one peer cannot hold them all.
#[derive(Default)]
struct Gate {
    total: usize,
    by_peer: HashMap<IpAddr, usize>,
}

impl Gate {
    fn enter(gate: &Mutex<Gate>, peer: IpAddr) -> bool {
        let mut g = gate.lock().expect("connection gate");
        let mine = g.by_peer.get(&peer).copied().unwrap_or(0);
        if g.total >= MAX_CONNECTIONS || mine >= MAX_PER_PEER {
            return false;
        }
        g.total += 1;
        *g.by_peer.entry(peer).or_default() += 1;
        true
    }

    fn leave(gate: &Mutex<Gate>, peer: IpAddr) {
        let mut g = gate.lock().expect("connection gate");
        g.total = g.total.saturating_sub(1);
        if let Some(n) = g.by_peer.get_mut(&peer) {
            *n -= 1;
            if *n == 0 {
                g.by_peer.remove(&peer);
            }
        }
    }
}

/// One connection: a file, or the stream.
fn serve(stream: TcpStream, hub: &Mutex<Hub>) -> io::Result<()> {
    // A peer that stops reading is let go after this long, and its thread
    // and connection slot with it.
    stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
    // The whole request, headers and body, within one deadline: a byte
    // every few seconds does not keep a connection alive.
    let mut reader = BufReader::new(Deadline {
        stream: stream.try_clone()?,
        until: Instant::now() + REQUEST_DEADLINE,
    });
    // The request line and headers together, at most HEAD_LIMIT bytes: a
    // line with no end cannot make this read forever or hold memory.
    let mut request = String::new();
    let mut length = 0usize;
    let mut host = None;
    {
        let mut head = (&mut reader).take(HEAD_LIMIT);
        head.read_line(&mut request)?;
        let mut line = String::new();
        loop {
            line.clear();
            let n = head.read_line(&mut line)?;
            if n == 0 || line == "\r\n" || line == "\n" {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                let name = name.trim();
                if name.eq_ignore_ascii_case("content-length") {
                    length = value.trim().parse().unwrap_or(0);
                } else if name.eq_ignore_ascii_case("host") {
                    host = Some(value.trim().to_string());
                }
            }
        }
    }
    let mut parts = request.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("/");
    let path = path.split('?').next().unwrap_or("/");
    let mut out = stream;
    if !host_allowed(host.as_deref()) {
        return respond(&mut out, "403 Forbidden", "text/plain", b"unknown host");
    }
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

/// Whether a request named a host this server answers to: `localhost`, or
/// an address by its number.
///
/// A web page elsewhere can point a name it owns at 127.0.0.1 (DNS
/// rebinding) and then read this server as if it were its own origin; its
/// requests still carry its own name in `Host`. So a name other than
/// `localhost` is turned away, and numbers — which no outside page can
/// claim — are let in, which keeps `--lan` working by IP.
fn host_allowed(host: Option<&str>) -> bool {
    let Some(host) = host else { return false };
    let name = if let Some(rest) = host.strip_prefix('[') {
        // [v6]:port
        return rest
            .split(']')
            .next()
            .is_some_and(|ip| ip.parse::<std::net::Ipv6Addr>().is_ok());
    } else {
        host.rsplit_once(':').map_or(host, |(name, _)| name)
    };
    name.eq_ignore_ascii_case("localhost") || name.parse::<std::net::Ipv4Addr>().is_ok()
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
    // What there is now, then everything from here on. Taking the one and
    // joining the other under the same lock means no edit falls between the
    // two; the writing happens after it is let go, so a viewer that reads
    // slowly — or not at all — holds up only its own thread, never the hub.
    let (tx, rx): (Sender<Arc<str>>, Receiver<Arc<str>>) = mpsc::channel();
    let opening = {
        let mut hub = hub.lock().expect("viewer hub");
        let mut opening = frame("status", &hub.status);
        if hub.has_scene {
            opening.push_str(&frame("scene", &hub.view.scene()));
        }
        hub.viewers.push(tx);
        opening
    };
    out.write_all(opening.as_bytes())?;
    out.flush()?;
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
        write!(s, "GET {path} HTTP/1.1\r\nHost: localhost:8090\r\n\r\n").unwrap();
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
    fn a_page_from_another_name_is_turned_away() {
        let server = Server::start("127.0.0.1:0").unwrap();
        let ask = |host: &str| {
            let mut s = TcpStream::connect(server.addr()).unwrap();
            write!(s, "GET /events HTTP/1.1\r\n{host}\r\n").unwrap();
            let mut out = String::new();
            s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let _ = s.read_to_string(&mut out);
            out
        };
        // A rebound name: the page's own, pointing here.
        assert!(ask("Host: evil.example:8090\r\n").starts_with("HTTP/1.1 403"));
        assert!(ask("").starts_with("HTTP/1.1 403"), "no Host at all");
    }

    #[test]
    fn a_viewer_that_never_reads_holds_up_nobody() {
        let server = Server::start("127.0.0.1:0").unwrap();
        // A scene of some megabytes: more than any socket buffer holds.
        let mut doc = Document::new();
        for i in 0..400 {
            doc.add_mesh_object(
                format!("Ball{i}"),
                kreate_mesh::primitives::uv_sphere(1.0, 48, 24),
            );
        }
        server.reset(&doc);

        // Asks for the stream and then reads nothing, ever.
        let mut stuck = TcpStream::connect(server.addr()).unwrap();
        write!(stuck, "GET /events HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
        while server.viewers() == 0 {
            thread::sleep(Duration::from_millis(5));
        }

        // Edits keep flowing to everyone else without waiting on it.
        let started = std::time::Instant::now();
        let first = doc.objects().next().unwrap().0;
        for k in 0..20 {
            doc.object_mut(first).unwrap().transform.translation.z = k as f64;
            server.update(&doc, None);
        }
        server.status(&Status::Waiting {
            address: "x".into(),
        });
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
        assert!(get(server.addr(), "/").starts_with("HTTP/1.1 200"));
        drop(stuck);
    }

    #[test]
    fn one_machine_cannot_hold_every_connection() {
        let server = Server::start("127.0.0.1:0").unwrap();
        // Its share, held open and silent.
        let held: Vec<TcpStream> = (0..MAX_PER_PEER)
            .map(|_| TcpStream::connect(server.addr()).unwrap())
            .collect();
        thread::sleep(Duration::from_millis(200));
        // One more is closed at the door, unanswered.
        let mut s = TcpStream::connect(server.addr()).unwrap();
        let _ = write!(s, "GET / HTTP/1.1\r\nHost: localhost\r\n\r\n");
        s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let mut out = Vec::new();
        let _ = s.read_to_end(&mut out);
        assert!(out.is_empty());
        // Letting go frees the share.
        drop(held);
        thread::sleep(Duration::from_millis(300));
        assert!(get(server.addr(), "/").starts_with("HTTP/1.1 200"));
    }

    #[test]
    fn hosts_by_name_and_by_number() {
        assert!(host_allowed(Some("localhost:8090")));
        assert!(host_allowed(Some("LOCALHOST")));
        assert!(host_allowed(Some("127.0.0.1:8090")));
        assert!(host_allowed(Some("192.168.0.12:8090")));
        assert!(host_allowed(Some("[::1]:8090")));
        assert!(!host_allowed(Some("localhost.evil.example")));
        assert!(!host_allowed(Some("evil.example:8090")));
        assert!(!host_allowed(Some("[not-an-ip]:1")));
        assert!(!host_allowed(None));
    }

    #[test]
    fn a_header_with_no_end_is_cut_off() {
        let server = Server::start("127.0.0.1:0").unwrap();
        let mut s = TcpStream::connect(server.addr()).unwrap();
        // Far past the limit, never ending the line.
        let junk = vec![b'a'; 200 * 1024];
        let started = std::time::Instant::now();
        let _ = s.write_all(b"GET / HTTP/1.1\r\nX: ");
        let _ = s.write_all(&junk);
        let mut out = Vec::new();
        s.set_read_timeout(Some(Duration::from_secs(8))).unwrap();
        // The server answers and closes rather than reading for ever. Its
        // close may arrive as a reset, with the rest of ours unread: either
        // way the read ends well before the timeout.
        let _ = s.read_to_end(&mut out);
        assert!(started.elapsed() < Duration::from_secs(6));
        // And it still serves.
        assert!(get(server.addr(), "/").starts_with("HTTP/1.1 200"));
    }

    #[test]
    fn a_viewer_gets_the_scene_then_each_edit() {
        let server = Server::start("127.0.0.1:0").unwrap();
        let mut doc = Document::new();
        let cube = doc.add_mesh_object("Cube", kreate_mesh::primitives::cube(1.0));
        server.reset(&doc);

        let mut s = TcpStream::connect(server.addr()).unwrap();
        write!(s, "GET /events HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").unwrap();
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
        server.update(&doc, None);
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
