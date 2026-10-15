//! A PLC on the network: virtual commissioning. KYNEX is the client, as
//! Visual Components is to a PLC's server; the PLC's program runs the
//! cell's machines as it would the real ones. Over Modbus TCP, or OPC UA
//! (see [`opcua`]).
//!
//! The cell's machines are wired to the PLC's addresses (see
//! `Component::plc`): a command — a door to open, a cycle to start — is
//! given as the PLC turns its address on; a state — the door open, the
//! cycle done — is written to its address as it changes. Over Modbus an
//! address is a coil, counted from 0 as Modbus addresses them (coil 00001
//! is address 0); over OPC UA it is a Boolean variable's node id
//! (`ns=2;s=Lathe.Open`).
//!
//! The link runs on a thread of its own, exchanging every [`EVERY`]: it
//! reads the commands (Modbus function 1, OPC UA Read) and writes each
//! state that changed (function 5, Write), and every one again after it
//! connects. The page and the cell never wait on the network.

pub mod opcua;

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::{json, Value};

/// How often the link exchanges with the PLC.
pub const EVERY: Duration = Duration::from_millis(50);

/// How KYNEX talks to the PLC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    Modbus,
    OpcUa,
}

impl Protocol {
    pub fn tag(self) -> &'static str {
        match self {
            Protocol::Modbus => "modbus",
            Protocol::OpcUa => "opcua",
        }
    }
}

/// Where the PLC is.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub protocol: Protocol,
    pub host: String,
    pub port: u16,
    /// The Modbus unit (slave) the PLC answers as.
    pub unit: u8,
    /// The OPC UA server's path after its host and port, if its endpoint
    /// has one (`/freeopcua/server/`).
    pub path: String,
    /// Whether to connect.
    pub on: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            protocol: Protocol::Modbus,
            host: "127.0.0.1".into(),
            port: 502,
            unit: 1,
            path: String::new(),
            on: false,
        }
    }
}

impl Settings {
    pub fn of(v: &Value) -> Self {
        let d = Self::default();
        let protocol = match v["protocol"].as_str() {
            Some("opcua") => Protocol::OpcUa,
            _ => Protocol::Modbus,
        };
        Self {
            protocol,
            path: v["path"]
                .as_str()
                .map_or(d.path, |p| p.trim().chars().take(200).collect()),
            host: v["host"].as_str().map_or(d.host, |h| h.trim().to_string()),
            port: v["port"]
                .as_u64()
                .and_then(|p| u16::try_from(p).ok())
                .unwrap_or(d.port),
            unit: v["unit"]
                .as_u64()
                .and_then(|u| u8::try_from(u).ok())
                .unwrap_or(d.unit),
            on: v["on"].as_bool().unwrap_or(d.on),
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "protocol": self.protocol.tag(),
            "host": self.host,
            "port": self.port,
            "unit": self.unit,
            "path": self.path,
            "on": self.on,
        })
    }

    /// The OPC UA endpoint's URL.
    pub fn url(&self) -> String {
        let path = self.path.trim();
        let slash = if path.is_empty() || path.starts_with('/') {
            ""
        } else {
            "/"
        };
        format!("opc.tcp://{}:{}{slash}{path}", self.host, self.port)
    }
}

// --- the protocol ------------------------------------------------------------

/// A request: its MBAP header (transaction `tid`, unit `unit`) and `pdu`.
fn frame(tid: u16, unit: u8, pdu: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(7 + pdu.len());
    out.extend_from_slice(&tid.to_be_bytes());
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&((pdu.len() + 1) as u16).to_be_bytes());
    out.push(unit);
    out.extend_from_slice(pdu);
    out
}

/// Read `count` coils from `start` (function 1).
fn read_coils_pdu(start: u16, count: u16) -> Vec<u8> {
    let mut pdu = vec![0x01];
    pdu.extend_from_slice(&start.to_be_bytes());
    pdu.extend_from_slice(&count.to_be_bytes());
    pdu
}

/// Write coil `address` on or off (function 5).
fn write_coil_pdu(address: u16, on: bool) -> Vec<u8> {
    let mut pdu = vec![0x05];
    pdu.extend_from_slice(&address.to_be_bytes());
    pdu.extend_from_slice(if on { &[0xFF, 0x00] } else { &[0x00, 0x00] });
    pdu
}

/// What a Modbus exception code says.
fn exception(code: u8) -> String {
    let what = match code {
        1 => "지원하지 않는 기능",
        2 => "없는 주소",
        3 => "잘못된 값",
        4 => "장치 오류",
        6 => "장치 바쁨",
        _ => "예외",
    };
    format!("Modbus 예외 {code}: {what}")
}

/// One request and its answer's PDU, over `stream`.
fn exchange(stream: &mut TcpStream, tid: u16, unit: u8, pdu: &[u8]) -> Result<Vec<u8>, String> {
    stream
        .write_all(&frame(tid, unit, pdu))
        .map_err(|e| e.to_string())?;
    let mut head = [0u8; 7];
    stream.read_exact(&mut head).map_err(|e| e.to_string())?;
    let len = u16::from_be_bytes([head[4], head[5]]) as usize;
    if !(2..=254).contains(&len) {
        return Err(format!("응답 길이가 맞지 않습니다: {len}"));
    }
    let mut body = vec![0u8; len - 1];
    stream.read_exact(&mut body).map_err(|e| e.to_string())?;
    if u16::from_be_bytes([head[0], head[1]]) != tid {
        return Err("다른 요청의 응답입니다".into());
    }
    match body.first() {
        Some(f) if *f == pdu[0] | 0x80 => Err(exception(*body.get(1).unwrap_or(&0))),
        Some(f) if *f == pdu[0] => Ok(body),
        _ => Err("응답이 요청과 맞지 않습니다".into()),
    }
}

/// The coils `start`… as a read answer `body` has them.
fn coils_of(body: &[u8], count: u16) -> Result<Vec<bool>, String> {
    let bytes = *body.get(1).ok_or("짧은 응답")? as usize;
    let bits = body.get(2..2 + bytes).ok_or("짧은 응답")?;
    if bytes * 8 < count as usize {
        return Err("짧은 응답".into());
    }
    Ok((0..count as usize)
        .map(|i| bits[i / 8] & (1 << (i % 8)) != 0)
        .collect())
}

// --- the link -----------------------------------------------------------------

/// What the link and the cell share.
#[derive(Default)]
struct Shared {
    /// The commands' addresses to read.
    reads: Vec<String>,
    /// The states as they should be, by address.
    writes: BTreeMap<String, bool>,
    /// The commands as last read, by address.
    got: BTreeMap<String, bool>,
    connected: bool,
    error: Option<String>,
    /// Addresses the PLC would not read or write, last exchange: the link
    /// goes on with the rest.
    problems: Vec<String>,
    /// A command the cell could not take, last time one came.
    warning: Option<String>,
    /// Why the link last dropped, kept after it connects again, and how
    /// often it has.
    dropped: Option<String>,
    drops: u64,
    /// Exchanges done since connecting.
    exchanges: u64,
}

/// A link to a PLC, while it is wanted.
pub struct Plc {
    pub settings: Settings,
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Plc {
    pub fn new(settings: Settings) -> Self {
        let mut plc = Self {
            settings: Settings::default(),
            shared: Arc::default(),
            stop: Arc::default(),
            thread: None,
        };
        plc.set(settings);
        plc
    }

    /// Connects to where `settings` say, or lets go, as they say.
    pub fn set(&mut self, settings: Settings) {
        if settings == self.settings && (self.thread.is_some() == settings.on) {
            return;
        }
        self.halt();
        self.settings = settings;
        {
            let mut s = self.shared.lock().unwrap();
            s.connected = false;
            s.error = None;
            s.exchanges = 0;
            s.got.clear();
            s.warning = None;
            s.problems.clear();
            s.drops = 0;
            s.dropped = None;
        }
        if self.settings.on {
            let (shared, stop, settings) = (
                self.shared.clone(),
                self.stop.clone(),
                self.settings.clone(),
            );
            self.thread = Some(std::thread::spawn(move || run(&settings, &shared, &stop)));
        }
    }

    fn halt(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        self.stop.store(false, Ordering::Relaxed);
    }

    /// Whether it exchanges with a PLC.
    pub fn on(&self) -> bool {
        self.thread.is_some()
    }

    /// The commands as last read, by address.
    pub fn read(&self) -> BTreeMap<String, bool> {
        self.shared.lock().unwrap().got.clone()
    }

    /// Hands over the addresses to read and the states as they should be.
    pub fn hand(&self, reads: Vec<String>, writes: Vec<(String, bool)>) {
        let mut s = self.shared.lock().unwrap();
        s.reads = reads;
        s.writes = writes.into_iter().collect();
    }

    /// What went wrong giving the cell the PLC's commands, last time
    /// something did: shown with the link.
    pub fn warn(&self, errors: Vec<String>) {
        if !errors.is_empty() {
            self.shared.lock().unwrap().warning = Some(errors.join(", "));
        }
    }

    /// The `plc` event: where, and how it is going.
    pub fn to_json(&self) -> Value {
        let s = self.shared.lock().unwrap();
        json!({
            "settings": self.settings.to_json(),
            "connected": s.connected,
            "error": s.error,
            "warning": s.warning,
            "problems": s.problems,
            "dropped": s.dropped,
            "drops": s.drops,
            "exchanges": s.exchanges,
            "values": s.got.iter().map(|(a, v)| json!([a, v])).chain(
                s.writes.iter().map(|(a, v)| json!([a, v]))).collect::<Vec<_>>(),
        })
    }
}

impl Drop for Plc {
    fn drop(&mut self) {
        self.halt();
    }
}

/// A connection to a PLC, by protocol.
enum Conn {
    Modbus {
        stream: TcpStream,
        tid: u16,
        unit: u8,
    },
    OpcUa(opcua::Session),
}

impl Conn {
    fn open(settings: &Settings) -> Result<Self, String> {
        let stream = connect(settings)?;
        Ok(match settings.protocol {
            Protocol::Modbus => Conn::Modbus {
                stream,
                tid: 0,
                unit: settings.unit,
            },
            Protocol::OpcUa => Conn::OpcUa(opcua::Session::open(stream, &settings.url())?),
        })
    }

    /// The commands at `addresses`, and the addresses that could not be
    /// read. An error is the connection's: it is made again.
    fn read(
        &mut self,
        addresses: &[String],
    ) -> Result<(BTreeMap<String, bool>, Vec<String>), String> {
        let mut got = BTreeMap::new();
        let mut problems = Vec::new();
        match self {
            Conn::Modbus { stream, tid, unit } => {
                let mut coils: Vec<(u16, &String)> = Vec::new();
                for a in addresses {
                    match a.parse::<u16>() {
                        Ok(n) => coils.push((n, a)),
                        Err(_) => problems.push(format!("{a}: 코일 번호(0–65535)가 아닙니다")),
                    }
                }
                coils.sort();
                // In spans of at most 2000.
                let mut at = 0;
                while at < coils.len() {
                    let start = coils[at].0;
                    let end = coils[at..]
                        .iter()
                        .map(|c| c.0)
                        .take_while(|n| (*n as u32) < start as u32 + 2000)
                        .last()
                        .unwrap();
                    let count = end - start + 1;
                    *tid = tid.wrapping_add(1);
                    let body = exchange(stream, *tid, *unit, &read_coils_pdu(start, count))?;
                    let bits = coils_of(&body, count)?;
                    while at < coils.len() && coils[at].0 <= end {
                        got.insert(coils[at].1.clone(), bits[(coils[at].0 - start) as usize]);
                        at += 1;
                    }
                }
            }
            Conn::OpcUa(session) => {
                let mut nodes = Vec::new();
                let mut named = Vec::new();
                for a in addresses {
                    match opcua::NodeId::parse(a) {
                        Ok(n) => {
                            nodes.push(n);
                            named.push(a);
                        }
                        Err(e) => problems.push(e),
                    }
                }
                if !nodes.is_empty() {
                    for (a, value) in named.into_iter().zip(session.read(&nodes)?) {
                        match value {
                            Ok(on) => {
                                got.insert(a.clone(), on);
                            }
                            Err(e) => problems.push(e),
                        }
                    }
                }
            }
        }
        Ok((got, problems))
    }

    /// Writes `on` to the state at `address`: `Ok(Err(why))` when the PLC
    /// would not take it, an error when the connection failed.
    fn write(&mut self, address: &str, on: bool) -> Result<Result<(), String>, String> {
        match self {
            Conn::Modbus { stream, tid, unit } => {
                let Ok(n) = address.parse::<u16>() else {
                    return Ok(Err(format!("{address}: 코일 번호(0–65535)가 아닙니다")));
                };
                *tid = tid.wrapping_add(1);
                match exchange(stream, *tid, *unit, &write_coil_pdu(n, on)) {
                    Ok(_) => Ok(Ok(())),
                    Err(e) if e.starts_with("Modbus 예외") => Ok(Err(format!("{address}: {e}"))),
                    Err(e) => Err(e),
                }
            }
            Conn::OpcUa(session) => {
                let node = match opcua::NodeId::parse(address) {
                    Ok(n) => n,
                    Err(e) => return Ok(Err(e)),
                };
                match session.write(&node, on) {
                    Ok(()) => Ok(Ok(())),
                    // A status the server gave: the node's, not the link's.
                    Err(e) if e.contains("OPC UA 0x") => Ok(Err(e)),
                    Err(e) => Err(e),
                }
            }
        }
    }

    fn close(self) {
        if let Conn::OpcUa(session) = self {
            session.close();
        }
    }
}

/// The link's thread: connects, and exchanges until told to stop; after a
/// failure, connects again a second later.
fn run(settings: &Settings, shared: &Mutex<Shared>, stop: &AtomicBool) {
    while !stop.load(Ordering::Relaxed) {
        let mut conn = match Conn::open(settings) {
            Ok(c) => c,
            Err(e) => {
                fail(shared, e);
                pause(stop, Duration::from_secs(1));
                continue;
            }
        };
        {
            let mut s = shared.lock().unwrap();
            s.connected = true;
            s.error = None;
        }
        // Every state is written once on connecting.
        let mut written: BTreeMap<String, bool> = BTreeMap::new();
        while !stop.load(Ordering::Relaxed) {
            let (reads, writes) = {
                let s = shared.lock().unwrap();
                (s.reads.clone(), s.writes.clone())
            };
            let result = (|| -> Result<(BTreeMap<String, bool>, Vec<String>), String> {
                let (got, mut problems) = conn.read(&reads)?;
                for (address, on) in &writes {
                    if written.get(address) == Some(on) {
                        continue;
                    }
                    if let Err(why) = conn.write(address, *on)? {
                        problems.push(why);
                    }
                    // Not tried again until it changes: a PLC that will not
                    // take it is said so once, not hammered.
                    written.insert(address.clone(), *on);
                }
                Ok((got, problems))
            })();
            match result {
                Ok((got, problems)) => {
                    let mut s = shared.lock().unwrap();
                    s.got = got;
                    s.exchanges += 1;
                    s.error = None;
                    s.problems = problems;
                }
                Err(e) => {
                    fail(shared, e);
                    break;
                }
            }
            pause(stop, EVERY);
        }
        conn.close();
        pause(stop, Duration::from_secs(1));
    }
    shared.lock().unwrap().connected = false;
}

fn connect(settings: &Settings) -> Result<TcpStream, String> {
    let at = (settings.host.as_str(), settings.port)
        .to_socket_addrs()
        .map_err(|e| format!("{}: {e}", settings.host))?
        .next()
        .ok_or_else(|| format!("{}: 주소를 찾지 못했습니다", settings.host))?;
    let stream = TcpStream::connect_timeout(&at, Duration::from_secs(1))
        .map_err(|e| format!("{at}: {e}"))?;
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
    Ok(stream)
}

fn fail(shared: &Mutex<Shared>, error: String) {
    let mut s = shared.lock().unwrap();
    s.connected = false;
    if s.connected {
        s.drops += 1;
        s.dropped = Some(error.clone());
    }
    s.error = Some(error);
}

/// Sleeps `how_long`, or less if told to stop.
fn pause(stop: &AtomicBool, how_long: Duration) {
    let step = Duration::from_millis(10);
    let mut slept = Duration::ZERO;
    while slept < how_long && !stop.load(Ordering::Relaxed) {
        std::thread::sleep(step);
        slept += step;
    }
}

#[cfg(test)]
mod tests;
