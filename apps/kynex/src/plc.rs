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
pub mod secure;

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
    /// OPC UA: how the channel's messages are kept, the user (none for
    /// anonymous) and the password — the password only in memory, never
    /// saved — and the server certificate's thumbprint KYNEX trusts (taken
    /// on the first connection, refused when it changes).
    pub security: opcua::Mode,
    pub user: String,
    pub password: String,
    pub trust: String,
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
            security: opcua::Mode::None,
            user: String::new(),
            password: String::new(),
            trust: String::new(),
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
            security: match v["security"].as_str() {
                Some("sign") => opcua::Mode::Sign,
                Some("encrypt") => opcua::Mode::SignAndEncrypt,
                _ => opcua::Mode::None,
            },
            user: v["user"]
                .as_str()
                .map_or(d.user, |u| u.trim().chars().take(100).collect()),
            password: v["password"]
                .as_str()
                .map_or(d.password, |p| p.chars().take(200).collect()),
            trust: v["trust"].as_str().map_or(d.trust, |t| {
                t.chars()
                    .filter(char::is_ascii_hexdigit)
                    .take(40)
                    .collect::<String>()
                    .to_uppercase()
            }),
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
            "security": match self.security {
                opcua::Mode::None => "none",
                opcua::Mode::Sign => "sign",
                opcua::Mode::SignAndEncrypt => "encrypt",
            },
            "user": self.user,
            "trust": self.trust,
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

/// Read `count` holding registers from `start` (function 3).
fn read_registers_pdu(start: u16, count: u16) -> Vec<u8> {
    let mut pdu = vec![0x03];
    pdu.extend_from_slice(&start.to_be_bytes());
    pdu.extend_from_slice(&count.to_be_bytes());
    pdu
}

/// Write holding register `address` (function 6): a 16-bit word, a
/// negative number as its two's complement.
fn write_register_pdu(address: u16, value: i64) -> Vec<u8> {
    let mut pdu = vec![0x06];
    pdu.extend_from_slice(&address.to_be_bytes());
    pdu.extend_from_slice(&(value.clamp(-32768, 65535) as u16).to_be_bytes());
    pdu
}

/// The registers `start`… as a read answer `body` has them, as signed
/// 16-bit numbers.
fn registers_of(body: &[u8], count: u16) -> Result<Vec<i64>, String> {
    let bytes = *body.get(1).ok_or("짧은 응답")? as usize;
    let words = body.get(2..2 + bytes).ok_or("짧은 응답")?;
    if bytes < 2 * count as usize {
        return Err("짧은 응답".into());
    }
    Ok(words
        .chunks(2)
        .take(count as usize)
        .map(|w| i64::from(i16::from_be_bytes([w[0], w[1]])))
        .collect())
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

/// What an address holds: a bit (a Modbus coil, an OPC UA Boolean) or a
/// number (a Modbus holding register, an OPC UA numeric variable). Values
/// are whole numbers either way, a bit's 0 or 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Bit,
    Number,
}

/// What the link and the cell share.
#[derive(Default)]
struct Shared {
    /// The addresses to read, and what each holds.
    reads: Vec<(String, Kind)>,
    /// What to write, by address, as it should be.
    writes: BTreeMap<String, (Kind, i64)>,
    /// What was last read, by address.
    got: BTreeMap<String, i64>,
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
    /// The thumbprint of the server's certificate, as last connected.
    server_thumbprint: Option<String>,
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
    pub fn set(&mut self, mut settings: Settings) {
        if settings.password.is_empty() && settings.user == self.settings.user {
            settings.password = self.settings.password.clone();
        }
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

    /// What was last read, by address.
    pub fn read(&self) -> BTreeMap<String, i64> {
        self.shared.lock().unwrap().got.clone()
    }

    /// Hands over the addresses to read and what to write, as it should
    /// be.
    pub fn hand(&self, reads: Vec<(String, Kind)>, writes: Vec<(String, Kind, i64)>) {
        let mut s = self.shared.lock().unwrap();
        s.reads = reads;
        s.writes = writes.into_iter().map(|(a, k, v)| (a, (k, v))).collect();
    }

    /// The server certificate's thumbprint, when KYNEX trusts none yet and
    /// has just met one: trusted from now on (and so saved).
    pub fn trust_on_first_use(&mut self) -> Option<String> {
        if !self.settings.trust.is_empty() {
            return None;
        }
        let seen = self.shared.lock().unwrap().server_thumbprint.clone()?;
        self.settings.trust = seen.clone();
        Some(seen)
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
            "server_thumbprint": s.server_thumbprint,
            "has_password": !self.settings.password.is_empty(),
            // KYNEX's own certificate, for a PLC's administrator to trust:
            // where it is kept, and its thumbprint once it is made.
            "pki": secure::pki_dir().display().to_string(),
            "client_thumbprint": std::fs::read(secure::pki_dir().join("kynex_cert.der"))
                .ok()
                .map(|der| secure::hex(&secure::thumbprint(&der))),
            "dropped": s.dropped,
            "drops": s.drops,
            "exchanges": s.exchanges,
            "values": s.got.iter().map(|(a, v)| json!([a, v])).chain(
                s.writes.iter().map(|(a, (_, v))| json!([a, v]))).collect::<Vec<_>>(),
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

/// Modbus addresses of one kind, read in spans of at most `most`: each
/// span's values by `read(start, count)`, put back by address.
fn spans(
    addresses: &[(u16, &String)],
    most: u16,
    mut read: impl FnMut(u16, u16) -> Result<Vec<i64>, String>,
    got: &mut BTreeMap<String, i64>,
) -> Result<(), String> {
    let mut at = 0;
    while at < addresses.len() {
        let start = addresses[at].0;
        let end = addresses[at..]
            .iter()
            .map(|c| c.0)
            .take_while(|n| (*n as u32) < start as u32 + most as u32)
            .last()
            .unwrap();
        let count = end - start + 1;
        let values = read(start, count)?;
        while at < addresses.len() && addresses[at].0 <= end {
            got.insert(
                addresses[at].1.clone(),
                values[(addresses[at].0 - start) as usize],
            );
            at += 1;
        }
    }
    Ok(())
}

impl Conn {
    /// A connection as `settings` say; with it, the thumbprint of the
    /// server's certificate, when the channel is kept with it.
    fn open(settings: &Settings) -> Result<(Self, Option<String>), String> {
        let stream = connect(settings)?;
        if settings.protocol == Protocol::Modbus {
            return Ok((
                Conn::Modbus {
                    stream,
                    tid: 0,
                    unit: settings.unit,
                },
                None,
            ));
        }
        let url = settings.url();
        let user =
            (!settings.user.is_empty()).then(|| (settings.user.clone(), settings.password.clone()));
        let mode = settings.security;
        if mode == opcua::Mode::None {
            let options = opcua::Options {
                user,
                ..opcua::Options::anonymous()
            };
            return Ok((
                Conn::OpcUa(opcua::Session::open(stream, &url, options)?),
                None,
            ));
        }
        // The server's certificate, from its endpoints: the one it is kept
        // with as asked, and only if it is the one trusted.
        drop(stream);
        let endpoints = opcua::Session::endpoints(connect(settings)?, &url)?;
        let endpoint = endpoints
            .iter()
            .find(|e| e.mode == mode.code() && e.policy == mode.policy())
            .ok_or("OPC UA: 서버에 Basic256Sha256 보안 방식의 엔드포인트가 없습니다")?;
        let server = secure::Peer::of(&endpoint.certificate)?;
        let thumbprint = secure::hex(&server.thumbprint());
        if !settings.trust.is_empty() && settings.trust != thumbprint {
            return Err(format!(
                "OPC UA: 서버 인증서가 믿기로 한 것과 다릅니다 (믿는 지문 {}, 지금 {thumbprint})",
                settings.trust
            ));
        }
        let identity = secure::Identity::load_or_create(&secure::pki_dir())?;
        let options = opcua::Options {
            mode,
            identity: Some(identity),
            server: Some(server),
            user,
        };
        let session = opcua::Session::open(connect(settings)?, &url, options)?;
        Ok((Conn::OpcUa(session), Some(thumbprint)))
    }

    /// What is at `addresses`, and the addresses that could not be read.
    /// An error is the connection's: it is made again.
    fn read(
        &mut self,
        addresses: &[(String, Kind)],
    ) -> Result<(BTreeMap<String, i64>, Vec<String>), String> {
        let mut got = BTreeMap::new();
        let mut problems = Vec::new();
        match self {
            Conn::Modbus { stream, tid, unit } => {
                let mut coils: Vec<(u16, &String)> = Vec::new();
                let mut registers: Vec<(u16, &String)> = Vec::new();
                for (a, kind) in addresses {
                    match (a.parse::<u16>(), kind) {
                        (Ok(n), Kind::Bit) => coils.push((n, a)),
                        (Ok(n), Kind::Number) => registers.push((n, a)),
                        (Err(_), _) => problems.push(format!("{a}: 주소 번호(0–65535)가 아닙니다")),
                    }
                }
                coils.sort();
                registers.sort();
                spans(
                    &coils,
                    2000,
                    |start, count| {
                        *tid = tid.wrapping_add(1);
                        let body = exchange(stream, *tid, *unit, &read_coils_pdu(start, count))?;
                        Ok(coils_of(&body, count)?.into_iter().map(i64::from).collect())
                    },
                    &mut got,
                )?;
                spans(
                    &registers,
                    125,
                    |start, count| {
                        *tid = tid.wrapping_add(1);
                        let body =
                            exchange(stream, *tid, *unit, &read_registers_pdu(start, count))?;
                        registers_of(&body, count)
                    },
                    &mut got,
                )?;
            }
            Conn::OpcUa(session) => {
                let mut nodes = Vec::new();
                let mut named = Vec::new();
                for (a, kind) in addresses {
                    match opcua::NodeId::parse(a) {
                        Ok(n) => {
                            nodes.push(n);
                            named.push((a, kind));
                        }
                        Err(e) => problems.push(e),
                    }
                }
                if !nodes.is_empty() {
                    for ((a, kind), value) in named.into_iter().zip(session.read(&nodes)?) {
                        match value {
                            Ok(v) => {
                                let v = if *kind == Kind::Bit {
                                    i64::from(v != 0)
                                } else {
                                    v
                                };
                                got.insert(a.clone(), v);
                            }
                            Err(e) => problems.push(e),
                        }
                    }
                }
            }
        }
        Ok((got, problems))
    }

    /// Writes `value` to `address`: `Ok(Err(why))` when the PLC would not
    /// take it, an error when the connection failed.
    fn write(
        &mut self,
        address: &str,
        kind: Kind,
        value: i64,
    ) -> Result<Result<(), String>, String> {
        match self {
            Conn::Modbus { stream, tid, unit } => {
                let Ok(n) = address.parse::<u16>() else {
                    return Ok(Err(format!("{address}: 주소 번호(0–65535)가 아닙니다")));
                };
                *tid = tid.wrapping_add(1);
                let pdu = match kind {
                    Kind::Bit => write_coil_pdu(n, value != 0),
                    Kind::Number => write_register_pdu(n, value),
                };
                match exchange(stream, *tid, *unit, &pdu) {
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
                match session.write(&node, value, kind == Kind::Number) {
                    Ok(()) => Ok(Ok(())),
                    // A status the server gave: the node's, not the link's.
                    Err(e) if e.contains("OPC UA 0x") || e.contains("형식") => Ok(Err(e)),
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
            Ok((c, thumbprint)) => {
                shared.lock().unwrap().server_thumbprint = thumbprint;
                c
            }
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
        let mut written: BTreeMap<String, i64> = BTreeMap::new();
        while !stop.load(Ordering::Relaxed) {
            let (reads, writes) = {
                let s = shared.lock().unwrap();
                (s.reads.clone(), s.writes.clone())
            };
            let result = (|| -> Result<(BTreeMap<String, i64>, Vec<String>), String> {
                let (got, mut problems) = conn.read(&reads)?;
                for (address, (kind, value)) in &writes {
                    if written.get(address) == Some(value) {
                        continue;
                    }
                    if let Err(why) = conn.write(address, *kind, *value)? {
                        problems.push(why);
                    }
                    // Not tried again until it changes: a PLC that will not
                    // take it is said so once, not hammered.
                    written.insert(address.clone(), *value);
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
