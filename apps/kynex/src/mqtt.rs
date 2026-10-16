//! Mobile robots followed over MQTT: an AMR or AGV fleet's state messages
//! read from a broker, so an AMR in the scene drives as the real one does.
//!
//! The messages are VDA 5050's `state` (the AGV interface German industry
//! set: `<interface>/<version>/<manufacturer>/<serial>/state`), its
//! `agvPosition` in metres and radians on the vehicle's map, `driving`,
//! `batteryState`, `errors`; or, for a fleet of other habits, a JSON object
//! with `x`, `y` and `theta` (or `yaw`) at its top. The map's origin is the
//! world's.
//!
//! MQTT 3.1.1 with only what reading takes, over plain TCP: connect (a
//! user and a password if the broker wants them), subscribe at QoS 0, and
//! read what is published, pinging so the broker keeps the line.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// MQTT's own port.
pub const PORT: u16 = 1883;

/// Seconds between pings, half the keep-alive the broker is told.
const KEEP_ALIVE: u16 = 30;

/// A string as MQTT writes one: its length, two bytes, then it.
fn string(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&(s.len() as u16).to_be_bytes());
    out.extend_from_slice(s.as_bytes());
}

/// A packet: its first byte, its remaining length as MQTT's variable
/// length, then the rest.
fn packet(first: u8, rest: &[u8]) -> Vec<u8> {
    let mut out = vec![first];
    let mut n = rest.len();
    loop {
        let mut b = (n % 128) as u8;
        n /= 128;
        if n > 0 {
            b |= 0x80;
        }
        out.push(b);
        if n == 0 {
            break;
        }
    }
    out.extend_from_slice(rest);
    out
}

/// The next packet: its first byte and the rest. A packet longer than
/// `most` is refused: a state message is a few kilobytes.
#[cfg(test)]
fn receive(stream: &mut impl Read, most: usize) -> std::io::Result<(u8, Vec<u8>)> {
    let mut first = [0u8; 1];
    stream.read_exact(&mut first)?;
    let rest = rest_of(stream, most)?;
    Ok((first[0], rest))
}

/// A packet's remaining length and the rest of it, its first byte read.
fn rest_of(stream: &mut impl Read, most: usize) -> std::io::Result<Vec<u8>> {
    let (mut n, mut shift) = (0usize, 0);
    loop {
        let mut b = [0u8; 1];
        stream.read_exact(&mut b)?;
        n |= ((b[0] & 0x7f) as usize) << shift;
        if b[0] & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift > 21 {
            return Err(std::io::Error::other("MQTT: a length too long"));
        }
    }
    if n > most {
        return Err(std::io::Error::other(format!(
            "MQTT: a packet of {n} bytes"
        )));
    }
    let mut rest = vec![0u8; n];
    stream.read_exact(&mut rest)?;
    Ok(rest)
}

/// The next packet from `stream`, `None` if none began within its read
/// timeout. Once begun, it is read whole, however long that takes: a
/// packet cut in two would leave the line out of step.
fn poll(stream: &mut TcpStream, most: usize) -> std::io::Result<Option<(u8, Vec<u8>)>> {
    let mut first = [0u8; 1];
    match stream.read(&mut first) {
        Ok(0) => return Err(std::io::ErrorKind::UnexpectedEof.into()),
        Ok(_) => {}
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) =>
        {
            return Ok(None)
        }
        Err(e) => return Err(e),
    }
    let short = stream.read_timeout()?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let rest = rest_of(stream, most);
    stream.set_read_timeout(short)?;
    Ok(Some((first[0], rest?)))
}

/// A broker's address, `host` or `host:port`, with MQTT's port if none.
fn address(broker: &str) -> String {
    let broker = broker
        .trim()
        .trim_start_matches("mqtt://")
        .trim_end_matches('/');
    if broker
        .rsplit_once(':')
        .is_some_and(|(_, p)| p.parse::<u16>().is_ok())
    {
        broker.to_string()
    } else {
        format!("{broker}:{PORT}")
    }
}

/// A link to a broker, subscribed.
pub struct Client {
    stream: TcpStream,
    pinged: Instant,
}

impl Client {
    /// Connects to `broker` as `user` (none if empty) and subscribes to
    /// `topic` (MQTT's `+` and `#` allowed).
    pub fn open(broker: &str, user: &str, password: &str, topic: &str) -> Result<Self, String> {
        let address = address(broker);
        let to = address
            .to_socket_addrs()
            .map_err(|e| format!("{address}: {e}"))?
            .next()
            .ok_or_else(|| format!("{address}: 주소를 찾지 못했습니다"))?;
        let mut stream = TcpStream::connect_timeout(&to, Duration::from_secs(3))
            .map_err(|e| format!("{address}: {e}"))?;
        // Short reads: a ping goes out on time whatever comes in.
        stream
            .set_read_timeout(Some(Duration::from_millis(500)))
            .map_err(|e| e.to_string())?;
        let mut body = Vec::new();
        string(&mut body, "MQTT");
        body.push(4);
        let mut flags = 0x02; // a clean session
        if !user.is_empty() {
            flags |= 0x80;
            if !password.is_empty() {
                flags |= 0x40;
            }
        }
        body.push(flags);
        body.extend_from_slice(&(KEEP_ALIVE * 2).to_be_bytes());
        // A name of its own: two KYNEX on one broker each get theirs.
        let id = format!(
            "kynex-{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos() as u64 & 0xffff_ffff)
        );
        string(&mut body, &id);
        if !user.is_empty() {
            string(&mut body, user);
            if !password.is_empty() {
                string(&mut body, password);
            }
        }
        stream
            .write_all(&packet(0x10, &body))
            .map_err(|e| format!("MQTT: {e}"))?;
        let (first, rest) = wait(&mut stream).map_err(|e| format!("MQTT: {e}"))?;
        if first != 0x20 || rest.len() < 2 {
            return Err("MQTT: 브로커가 아닙니다".into());
        }
        match rest[1] {
            0 => {}
            4 | 5 => return Err("MQTT: 사용자 이름이나 비밀번호가 맞지 않습니다".into()),
            n => return Err(format!("MQTT: 연결이 거절되었습니다 (코드 {n})")),
        }
        let mut body = 1u16.to_be_bytes().to_vec();
        string(&mut body, topic);
        body.push(0);
        stream
            .write_all(&packet(0x82, &body))
            .map_err(|e| format!("MQTT: {e}"))?;
        loop {
            let (first, rest) = wait(&mut stream).map_err(|e| format!("MQTT: {e}"))?;
            if first == 0x90 {
                if rest.get(2) == Some(&0x80) {
                    return Err(format!("MQTT: 구독이 거절되었습니다 ({topic})"));
                }
                break;
            }
        }
        Ok(Self {
            stream,
            pinged: Instant::now(),
        })
    }

    /// The next message published, `None` if none came for a moment.
    pub fn next(&mut self) -> Result<Option<(String, Vec<u8>)>, String> {
        if self.pinged.elapsed() >= Duration::from_secs(KEEP_ALIVE as u64) {
            self.stream
                .write_all(&[0xc0, 0])
                .map_err(|e| format!("MQTT: {e}"))?;
            self.pinged = Instant::now();
        }
        match poll(&mut self.stream, 1 << 20) {
            Ok(Some((first, rest))) if first & 0xf0 == 0x30 => {
                let n = u16::from_be_bytes([
                    *rest.first().ok_or("MQTT: short")?,
                    *rest.get(1).ok_or("MQTT: short")?,
                ]) as usize;
                let topic =
                    String::from_utf8_lossy(rest.get(2..2 + n).ok_or("MQTT: short")?).into_owned();
                // A packet id after the topic, at QoS 1 or 2.
                let skip = if first & 0x06 != 0 { 2 } else { 0 };
                let payload = rest.get(2 + n + skip..).unwrap_or_default().to_vec();
                Ok(Some((topic, payload)))
            }
            Ok(_) => Ok(None),
            Err(e) => Err(format!("MQTT: {e}")),
        }
    }

    pub fn close(mut self) {
        let _ = self.stream.write_all(&[0xe0, 0]);
    }
}

/// The broker's answer, within a few seconds.
fn wait(stream: &mut TcpStream) -> std::io::Result<(u8, Vec<u8>)> {
    let start = Instant::now();
    loop {
        if let Some(p) = poll(stream, 1 << 16)? {
            return Ok(p);
        }
        if start.elapsed() >= Duration::from_secs(3) {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
    }
}

/// A mobile robot as its state message says.
#[derive(Clone, Debug, PartialEq)]
pub struct Rover {
    pub x: f64,
    pub y: f64,
    pub theta: f64,
    pub driving: Option<bool>,
    pub battery: Option<f64>,
    pub errors: Vec<String>,
}

/// A state message read: VDA 5050's, or one with `x`, `y`, `theta` at its
/// top. None if it says no position.
pub fn rover_of(payload: &[u8]) -> Option<Rover> {
    let v: Value = serde_json::from_slice(payload).ok()?;
    let at = if v["agvPosition"].is_object() {
        &v["agvPosition"]
    } else {
        &v
    };
    let x = at["x"].as_f64()?;
    let y = at["y"].as_f64()?;
    let theta = at["theta"].as_f64().or(at["yaw"].as_f64()).unwrap_or(0.0);
    if !(x.is_finite() && y.is_finite() && theta.is_finite()) {
        return None;
    }
    Some(Rover {
        x,
        y,
        theta,
        driving: v["driving"].as_bool(),
        battery: v["batteryState"]["batteryCharge"]
            .as_f64()
            .or(v["battery"].as_f64()),
        errors: v["errors"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|e| {
                e["errorDescription"]
                    .as_str()
                    .or(e["errorType"].as_str())
                    .map(|s| s.chars().take(200).collect())
            })
            .take(5)
            .collect(),
    })
}

#[derive(Default)]
struct Shared {
    last: Option<Rover>,
    error: Option<String>,
    at: Option<Instant>,
    connected: bool,
}

/// A broker followed on a thread of its own: subscribed, read as it
/// publishes, connected again a moment after a fault.
pub struct Feed {
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Feed {
    pub fn start(broker: &str, user: &str, password: &str, topic: &str) -> Self {
        let shared: Arc<Mutex<Shared>> = Arc::default();
        let stop: Arc<AtomicBool> = Arc::default();
        let (s, st) = (shared.clone(), stop.clone());
        let (broker, user, password, topic) = (
            broker.to_string(),
            user.to_string(),
            password.to_string(),
            topic.to_string(),
        );
        let thread = std::thread::spawn(move || {
            while !st.load(Ordering::Relaxed) {
                match Client::open(&broker, &user, &password, &topic) {
                    Ok(mut client) => {
                        {
                            let mut s = s.lock().unwrap();
                            s.connected = true;
                            s.error = None;
                        }
                        while !st.load(Ordering::Relaxed) {
                            match client.next() {
                                Ok(Some((_, payload))) => {
                                    let mut s = s.lock().unwrap();
                                    match rover_of(&payload) {
                                        Some(r) => {
                                            s.last = Some(r);
                                            s.at = Some(Instant::now());
                                            s.error = None;
                                        }
                                        None => {
                                            s.error =
                                                Some("위치(x, y)가 없는 메시지를 받았습니다".into())
                                        }
                                    }
                                }
                                Ok(None) => {}
                                Err(e) => {
                                    s.lock().unwrap().error = Some(e);
                                    break;
                                }
                            }
                        }
                        s.lock().unwrap().connected = false;
                        client.close();
                    }
                    Err(e) => s.lock().unwrap().error = Some(e),
                }
                let start = Instant::now();
                while start.elapsed() < Duration::from_secs(2) && !st.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        });
        Self {
            shared,
            stop,
            thread: Some(thread),
        }
    }

    /// The robot as last said, once anything has been.
    pub fn rover(&self) -> Option<Rover> {
        self.shared.lock().unwrap().last.clone()
    }

    /// How it is going, for the page.
    pub fn to_json(&self) -> Value {
        let s = self.shared.lock().unwrap();
        json!({
            "connected": s.connected,
            "age": s.at.map(|a| a.elapsed().as_secs()),
            "driving": s.last.as_ref().and_then(|r| r.driving),
            "battery": s.last.as_ref().and_then(|r| r.battery),
            "errors": s.last.as_ref().map(|r| r.errors.clone()).unwrap_or_default(),
            "error": s.error,
        })
    }
}

impl Drop for Feed {
    /// Told to stop, the thread ends within a read's time by itself.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        drop(self.thread.take());
    }
}

#[cfg(test)]
mod tests;
