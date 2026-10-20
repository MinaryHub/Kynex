//! The viewer's HTTP server: the page and its scripts, and `/events`, a
//! Server-Sent Events stream of the scene.
//!
//! SSE rather than WebSocket because the viewer only listens: it is plain
//! HTTP that any browser reconnects by itself, and it needs nothing past the
//! standard library to serve. One thread per open browser tab; a studio or a
//! cell has a handful.

use std::collections::{BTreeMap, HashMap};
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

/// How long a robot request may wait for its answer.
const ROBOT_DEADLINE: Duration = Duration::from_secs(5);

/// The most bytes a robot request may carry.
const ROBOT_LIMIT: usize = 64 * 1024;

/// The most bytes a model imported may have.
pub const IMPORT_LIMIT: usize = 256 << 20;

/// How long a model's import may take to answer: a large STEP file is read
/// for a while.
const IMPORT_DEADLINE: Duration = Duration::from_secs(180);

/// How long a model's bytes may take to arrive.
const UPLOAD_DEADLINE: Duration = Duration::from_secs(120);

/// Something the page asked of the robots — a joint moved, a tool sent
/// somewhere: the JSON it posted, and where the answer goes. The answer is
/// JSON too, and goes back to the page as it is.
pub struct RobotRequest {
    pub body: String,
    pub reply: Sender<String>,
    /// Whether it came from this computer, not another on the network (a
    /// viewer shared with `--lan`): what reaches beyond the viewer, as a
    /// link to a PLC does, is only for this computer's page to change.
    pub local: bool,
    /// A file the page sent with it (`POST /import`): a model to import.
    pub file: Option<Vec<u8>>,
}

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
    /// KYNEX on its own: no Kreate, the cell laid out from its library.
    Alone,
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
            Status::Alone => json!({"state": "alone"}),
        }
        .to_string()
    }
}

/// A running viewer server.
pub struct Server {
    addr: SocketAddr,
    hub: Arc<Mutex<Hub>>,
    /// When it started: each update says how long after it was made, so
    /// that a page can play moves back as evenly as they were made.
    started: std::time::Instant,
}

struct Hub {
    view: ViewState,
    has_scene: bool,
    status: String,
    viewers: Vec<Sender<Arc<str>>>,
    /// Where a benchmark page's report goes, when one was asked for.
    bench: Option<Sender<String>>,
    /// Where the page's robot requests go, when something takes them.
    robot: Option<Sender<RobotRequest>>,
    /// What KYNEX keeps beside the scene — the robots posed here, their
    /// programs — as the last event of each name said: a tab that opens
    /// later is told too.
    states: BTreeMap<String, String>,
    /// The scene as `/scene.bin` serves it, made when first asked for after
    /// a change and shared by every viewer that asks before the next.
    bin: Option<Arc<Vec<u8>>>,
}

impl Hub {
    fn broadcast(&mut self, event: &str, data: &str) {
        let message: Arc<str> = frame(event, data).into();
        self.viewers.retain(|v| v.send(message.clone()).is_ok());
    }
}

/// What tells a viewer there is a scene to fetch: its version, so a viewer
/// that is already fetching a later one can let this one go.
fn scene_notice(view: &ViewState) -> String {
    format!("{{\"v\":{}}}", view.version())
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
            robot: None,
            states: BTreeMap::new(),
            bin: None,
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
        Ok(Self {
            addr,
            hub,
            started: std::time::Instant::now(),
        })
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
        hub.bin = None;
        let notice = scene_notice(&hub.view);
        hub.broadcast("scene", &notice);
    }

    /// An edit to the scene the viewers have. See [`ViewState::update`].
    pub fn update(&self, doc: &Document, changes: Option<&Changes>) {
        let mut hub = self.hub.lock().expect("viewer hub");
        if !hub.has_scene {
            drop(hub);
            return self.reset(doc);
        }
        if let Some(update) = hub.view.update(doc, changes) {
            hub.bin = None;
            // Milliseconds since this KYNEX started, before the rest.
            let t = self.started.elapsed().as_secs_f64() * 1000.0;
            let update = match update.strip_prefix('{') {
                Some(rest) => format!("{{\"t\":{t:.1},{rest}"),
                None => update,
            };
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

    /// Takes the page's robot requests (`POST /robot`). Until this is
    /// called the server turns them away.
    pub fn robot_requests(&self) -> Receiver<RobotRequest> {
        let (tx, rx) = mpsc::channel();
        self.hub.lock().expect("viewer hub").robot = Some(tx);
        rx
    }

    /// Tells every viewer, now and later, something KYNEX keeps beside the
    /// scene: the JSON goes out as the `event` event when it differs from
    /// the last of that name. `pose` and `programs` are what the page reads.
    pub fn state(&self, event: &str, json: &str) {
        let mut hub = self.hub.lock().expect("viewer hub");
        if hub.states.get(event).map(String::as_str) != Some(json) {
            hub.states.insert(event.to_string(), json.to_string());
            hub.broadcast(event, json);
        }
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
        // This machine's own tabs all come from the loopback address: a
        // person with a few viewers open, each loading its page over several
        // connections, is not a peer hogging the server. The total still
        // holds for them.
        let shared = !peer.is_loopback();
        if g.total >= MAX_CONNECTIONS || (shared && mine >= MAX_PER_PEER) {
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
/// `%XX` and `+` in a query's value made the text they stand for.
fn percent_decode(v: &str) -> String {
    let b = v.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = b
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (b[i], hex) {
            (b'%', Some(x)) => {
                out.push(x);
                i += 3;
                continue;
            }
            (b'+', _) => out.push(b' '),
            (c, _) => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn serve(stream: TcpStream, hub: &Mutex<Hub>) -> io::Result<()> {
    let local = stream.peer_addr().is_ok_and(|a| a.ip().is_loopback());
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
    let mut json_body = false;
    let mut octet_body = false;
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
                } else if name.eq_ignore_ascii_case("content-type") {
                    let kind = value.trim().to_ascii_lowercase();
                    json_body = kind.starts_with("application/json");
                    octet_body = kind == "application/octet-stream";
                }
            }
        }
    }
    let mut parts = request.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("/");
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
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
    if method == "POST" && path == "/robot" {
        // Only JSON: a page elsewhere can post a form here without asking,
        // but not JSON, which the browser first asks this server about —
        // and this server never says yes.
        if !json_body {
            return respond(
                &mut out,
                "415 Unsupported Media Type",
                "text/plain",
                b"JSON only",
            );
        }
        if length > ROBOT_LIMIT {
            return respond(
                &mut out,
                "413 Payload Too Large",
                "text/plain",
                b"too large",
            );
        }
        let Some(robot) = hub.lock().expect("viewer hub").robot.clone() else {
            return respond(&mut out, "404 Not Found", "text/plain", b"no robots here");
        };
        let mut body = vec![0u8; length];
        reader.read_exact(&mut body)?;
        let (tx, rx) = mpsc::channel();
        let asked = robot.send(RobotRequest {
            body: String::from_utf8_lossy(&body).into_owned(),
            reply: tx,
            local,
            file: None,
        });
        return match asked
            .ok()
            .and_then(|_| rx.recv_timeout(ROBOT_DEADLINE).ok())
        {
            Some(answer) => respond(&mut out, "200 OK", "application/json", answer.as_bytes()),
            None => respond(
                &mut out,
                "503 Service Unavailable",
                "text/plain",
                b"no answer",
            ),
        };
    }
    // A model file, as raw bytes, its name and unit in the query: from this
    // computer only, and as octet-stream, which a page elsewhere cannot
    // send without first asking this server — which never says yes.
    if method == "POST" && path == "/import" {
        if !local {
            return respond(
                &mut out,
                "403 Forbidden",
                "text/plain",
                b"this computer only",
            );
        }
        if !octet_body {
            return respond(
                &mut out,
                "415 Unsupported Media Type",
                "text/plain",
                b"octet-stream only",
            );
        }
        if length == 0 || length > IMPORT_LIMIT {
            return respond(
                &mut out,
                "413 Payload Too Large",
                "text/plain",
                b"0 or too large",
            );
        }
        let Some(robot) = hub.lock().expect("viewer hub").robot.clone() else {
            return respond(&mut out, "404 Not Found", "text/plain", b"no robots here");
        };
        reader.get_mut().until = Instant::now() + UPLOAD_DEADLINE;
        let mut file = vec![0u8; length];
        reader.read_exact(&mut file)?;
        let query = |key: &str| {
            query
                .split('&')
                .filter_map(|kv| kv.split_once('='))
                .find(|(k, _)| *k == key)
                .map(|(_, v)| percent_decode(v))
        };
        let body = json!({
            "op": "import",
            "name": query("name").unwrap_or_default(),
            "unit": query("unit").and_then(|u| u.parse::<f64>().ok()),
            "keep": query("keep").as_deref() == Some("1"),
            // What the file is for: a model, or the site's drawing.
            "as": query("as").unwrap_or_default(),
        });
        let (tx, rx) = mpsc::channel();
        let asked = robot.send(RobotRequest {
            body: body.to_string(),
            reply: tx,
            local,
            file: Some(file),
        });
        return match asked
            .ok()
            .and_then(|_| rx.recv_timeout(IMPORT_DEADLINE).ok())
        {
            Some(answer) => respond(&mut out, "200 OK", "application/json", answer.as_bytes()),
            None => respond(
                &mut out,
                "503 Service Unavailable",
                "text/plain",
                b"no answer",
            ),
        };
    }
    if method != "GET" {
        return respond(
            &mut out,
            "405 Method Not Allowed",
            "text/plain",
            b"GET only",
        );
    }
    if path == "/scene.bin" {
        // Made under the lock, written after it, as the stream's opening is.
        let bin = {
            let mut hub = hub.lock().expect("viewer hub");
            if !hub.has_scene {
                drop(hub);
                return respond(&mut out, "404 Not Found", "text/plain", b"no scene yet");
            }
            if hub.bin.is_none() {
                hub.bin = Some(Arc::new(hub.view.scene_bin()));
            }
            hub.bin.clone().unwrap()
        };
        return respond(&mut out, "200 OK", "application/octet-stream", &bin);
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
        let mut opening = frame(
            "hello",
            &format!("{{\"build\":\"{}\"}}", assets::build_id()),
        );
        opening.push_str(&frame("status", &hub.status));
        for (event, json) in &hub.states {
            opening.push_str(&frame(event, json));
        }
        if hub.has_scene {
            opening.push_str(&frame("scene", &scene_notice(&hub.view)));
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

    fn post(addr: SocketAddr, path: &str, mime: &str, body: &str) -> String {
        let mut s = TcpStream::connect(addr).unwrap();
        write!(
            s,
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: {mime}\r\n\
             Content-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        // What came before a reset: a server answering a request it turns
        // away before reading its body may close on what is left of it.
        let mut out = Vec::new();
        let _ = s.read_to_end(&mut out);
        String::from_utf8_lossy(&out).into_owned()
    }

    #[test]
    fn a_robot_request_is_answered_by_whatever_takes_them() {
        let server = Server::start("127.0.0.1:0").unwrap();
        let addr = server.addr();
        let json = "application/json";
        assert!(post(addr, "/robot", json, "{}").starts_with("HTTP/1.1 404"));
        let requests = server.robot_requests();
        let answering = thread::spawn(move || {
            let r = requests.recv().unwrap();
            r.reply.send(format!("{{\"echo\":{}}}", r.body)).unwrap();
        });
        let out = post(addr, "/robot", json, "{\"op\":\"jog\"}");
        assert!(out.starts_with("HTTP/1.1 200"), "{out}");
        assert!(out.ends_with("{\"echo\":{\"op\":\"jog\"}}"), "{out}");
        answering.join().unwrap();
        // A form, which any page may send here unasked, is not taken.
        let form = "application/x-www-form-urlencoded";
        assert!(post(addr, "/robot", form, "op=jog").starts_with("HTTP/1.1 415"));
    }

    /// A model's bytes reach whatever takes requests, with its name and
    /// unit from the query; not as a form or as text, not empty.
    #[test]
    fn a_model_file_is_handed_on_with_its_name() {
        let server = Server::start("127.0.0.1:0").unwrap();
        let addr = server.addr();
        let requests = server.robot_requests();
        let answering = thread::spawn(move || {
            let r = requests.recv().unwrap();
            let file = r.file.clone().unwrap();
            r.reply
                .send(format!("{{\"got\":{},\"bytes\":{}}}", r.body, file.len()))
                .unwrap();
        });
        let octet = "application/octet-stream";
        let out = post(
            addr,
            "/import?name=%EB%B8%94%EB%A1%9D+1.stl&unit=0.001",
            octet,
            "solid x",
        );
        assert!(out.starts_with("HTTP/1.1 200"), "{out}");
        assert!(out.contains("\"name\":\"블록 1.stl\""), "{out}");
        assert!(out.contains("\"unit\":0.001"), "{out}");
        assert!(out.ends_with("\"bytes\":7}"), "{out}");
        answering.join().unwrap();
        for mime in ["text/plain", "multipart/form-data", "application/json"] {
            assert!(
                post(addr, "/import?name=a.stl", mime, "x").starts_with("HTTP/1.1 415"),
                "{mime}"
            );
        }
        assert!(post(addr, "/import?name=a.stl", octet, "").starts_with("HTTP/1.1 413"));
        assert_eq!(percent_decode("a%2Fb+c%zz%4"), "a/b c%zz%4");
    }

    #[test]
    fn the_scene_is_fetched_whole_and_as_bytes() {
        let server = Server::start("127.0.0.1:0").unwrap();
        let fetch = || {
            let mut s = TcpStream::connect(server.addr()).unwrap();
            write!(s, "GET /scene.bin HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
            let mut out = Vec::new();
            s.read_to_end(&mut out).unwrap();
            out
        };
        assert!(fetch().starts_with(b"HTTP/1.1 404"), "nothing to fetch yet");
        let mut doc = Document::new();
        doc.add_mesh_object("Cube", kreate_mesh::primitives::cube(1.0));
        server.reset(&doc);
        let out = fetch();
        assert!(out.starts_with(b"HTTP/1.1 200"));
        assert!(out.windows(4).any(|w| w == crate::scene::SCENE_MAGIC));
        // The same bytes for the next viewer, until the scene changes.
        assert_eq!(fetch(), out);
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
        let gate = Mutex::new(Gate::default());
        let other: IpAddr = "192.168.0.20".parse().unwrap();
        let near: IpAddr = "192.168.0.21".parse().unwrap();
        for _ in 0..MAX_PER_PEER {
            assert!(Gate::enter(&gate, other));
        }
        // Its share is used up; someone else's is not.
        assert!(!Gate::enter(&gate, other));
        assert!(Gate::enter(&gate, near));
        // Letting one go frees one.
        Gate::leave(&gate, other);
        assert!(Gate::enter(&gate, other));
    }

    #[test]
    fn this_machine_shares_only_the_total() {
        let gate = Mutex::new(Gate::default());
        let here: IpAddr = "127.0.0.1".parse().unwrap();
        for _ in 0..MAX_CONNECTIONS {
            assert!(Gate::enter(&gate, here), "past its share, within the total");
        }
        assert!(!Gate::enter(&gate, here), "but not past the total");
    }

    #[test]
    fn a_full_server_closes_the_door() {
        let server = Server::start("127.0.0.1:0").unwrap();
        // Every connection it will serve, held open and silent.
        let held: Vec<TcpStream> = (0..MAX_CONNECTIONS)
            .map(|_| TcpStream::connect(server.addr()).unwrap())
            .collect();
        thread::sleep(Duration::from_millis(300));
        let mut s = TcpStream::connect(server.addr()).unwrap();
        let _ = write!(s, "GET / HTTP/1.1\r\nHost: localhost\r\n\r\n");
        s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let mut out = Vec::new();
        let _ = s.read_to_end(&mut out);
        assert!(out.is_empty(), "closed unanswered");
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
