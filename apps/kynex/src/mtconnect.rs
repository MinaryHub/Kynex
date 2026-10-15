//! A real machine tool followed over MTConnect: a machine of the cell
//! mirrors one on the shop floor — its door, whether it is cutting, how
//! many parts it has made — as the machine's MTConnect agent reports it.
//! The agent is asked for the device's current state (`/<device>/current`,
//! MTConnect Streams) about once a second, over plain HTTP, as agents on a
//! shop floor serve it (`http://host:5000`).
//!
//! What is read, the first of each in the device's stream: `Execution`
//! (`ACTIVE` is cutting), `DoorState` (`OPEN`, `UNLATCHED`, `CLOSED`),
//! `PartCount`, `Availability` and `Program`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// How often the agent is asked.
pub const EVERY: Duration = Duration::from_secs(1);
/// The longest answer KYNEX reads.
const LONGEST: usize = 16 * 1024 * 1024;

/// What a device's current state says.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Observation {
    pub device: String,
    pub execution: Option<String>,
    pub door: Option<String>,
    pub part_count: Option<i64>,
    pub availability: Option<String>,
    pub program: Option<String>,
}

impl Observation {
    pub fn to_json(&self) -> Value {
        json!({
            "device": self.device,
            "execution": self.execution,
            "door": self.door,
            "part_count": self.part_count,
            "availability": self.availability,
            "program": self.program,
        })
    }
}

// --- HTTP --------------------------------------------------------------------

/// `url`, `http://host[:port]/path`, as host, port and path.
pub fn split_url(url: &str) -> Result<(String, u16, String), String> {
    let rest = url
        .trim()
        .strip_prefix("http://")
        .ok_or("MTConnect 에이전트 주소는 http://로 시작합니다")?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (
            h,
            p.parse::<u16>()
                .map_err(|_| format!("{url}: 포트가 숫자가 아닙니다"))?,
        ),
        None => (authority, 80),
    };
    if host.is_empty() {
        return Err(format!("{url}: 호스트가 없습니다"));
    }
    Ok((
        host.to_string(),
        port,
        path.trim_end_matches('/').to_string(),
    ))
}

/// GETs `path` from `host:port`: the answer's body, whole.
pub fn get(host: &str, port: u16, path: &str) -> Result<String, String> {
    let at = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("{host}: {e}"))?
        .next()
        .ok_or_else(|| format!("{host}: 주소를 찾지 못했습니다"))?;
    let mut stream = TcpStream::connect_timeout(&at, Duration::from_secs(2))
        .map_err(|e| format!("{at}: {e}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(3))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(3))).ok();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nAccept: application/xml\r\nConnection: close\r\n\r\n"
    )
    .map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(stream.take(LONGEST as u64 + 64 * 1024));
    let mut status = String::new();
    reader.read_line(&mut status).map_err(|e| e.to_string())?;
    let code = status.split_whitespace().nth(1).unwrap_or("");
    let mut length = None;
    let mut chunked = false;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
            return Err("MTConnect: 응답 머리가 끝나지 않았습니다".into());
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            let (k, v) = (k.trim().to_ascii_lowercase(), v.trim());
            if k == "content-length" {
                length = v.parse::<usize>().ok();
            }
            if k == "transfer-encoding" && v.eq_ignore_ascii_case("chunked") {
                chunked = true;
            }
        }
    }
    if code != "200" {
        return Err(format!(
            "MTConnect: 에이전트가 {}로 답했습니다",
            status.trim()
        ));
    }
    let mut body = Vec::new();
    if chunked {
        loop {
            let mut size = String::new();
            reader.read_line(&mut size).map_err(|e| e.to_string())?;
            let n = usize::from_str_radix(size.trim().split(';').next().unwrap_or(""), 16)
                .map_err(|_| "MTConnect: 조각 크기가 맞지 않습니다".to_string())?;
            if n == 0 {
                break;
            }
            if body.len() + n > LONGEST {
                return Err("MTConnect: 응답이 너무 깁니다".into());
            }
            let start = body.len();
            body.resize(start + n, 0);
            reader
                .read_exact(&mut body[start..])
                .map_err(|e| e.to_string())?;
            let mut crlf = [0u8; 2];
            reader.read_exact(&mut crlf).map_err(|e| e.to_string())?;
        }
    } else if let Some(n) = length {
        if n > LONGEST {
            return Err("MTConnect: 응답이 너무 깁니다".into());
        }
        body.resize(n, 0);
        reader.read_exact(&mut body).map_err(|e| e.to_string())?;
    } else {
        reader.read_to_end(&mut body).map_err(|e| e.to_string())?;
    }
    String::from_utf8(body).map_err(|_| "MTConnect: 응답이 UTF-8이 아닙니다".to_string())
}

// --- XML ---------------------------------------------------------------------

/// `&amp;` and its kind undone.
fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// The attribute `name` of a start tag's inside, `tag`.
fn attribute(tag: &str, name: &str) -> Option<String> {
    let mut rest = tag;
    while let Some(i) = rest.find(name) {
        let before = rest[..i].chars().last();
        let after = &rest[i + name.len()..];
        if before.is_some_and(char::is_whitespace) {
            if let Some(v) = after.trim_start().strip_prefix('=') {
                let v = v.trim_start();
                let quote = v.chars().next()?;
                if quote == '"' || quote == '\'' {
                    let end = v[1..].find(quote)?;
                    return Some(unescape(&v[1..1 + end]));
                }
            }
        }
        rest = &rest[i + name.len()..];
    }
    None
}

/// What the device `device` (by name or uuid; the first device that is not
/// the agent's own when empty) says in an MTConnect Streams document.
pub fn parse(xml: &str, device: &str) -> Result<Observation, String> {
    if !xml.contains("MTConnectStreams") {
        if xml.contains("MTConnectError") {
            // The first <Error …>'s text, not <Errors>'s.
            let why = xml
                .find("<Error ")
                .or_else(|| xml.find("<Error>"))
                .and_then(|i| xml[i..].find('>').map(|j| i + j + 1))
                .map(|i| xml[i..].split('<').next().unwrap_or(""))
                .unwrap_or("")
                .trim();
            return Err(format!("MTConnect 오류: {}", unescape(why)));
        }
        return Err("MTConnect: Streams 문서가 아닙니다".into());
    }
    let mut out = Observation::default();
    let mut in_device = false;
    let mut found = false;
    let mut at = 0;
    while let Some(open) = xml[at..].find('<').map(|i| at + i) {
        let Some(close) = xml[open..].find('>').map(|i| open + i) else {
            break;
        };
        let tag = &xml[open + 1..close];
        at = close + 1;
        if tag.starts_with('?') || tag.starts_with('!') {
            continue;
        }
        if let Some(name) = tag.strip_prefix('/') {
            if name.trim() == "DeviceStream" && in_device {
                break;
            }
            continue;
        }
        let name = tag
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_end_matches('/');
        if name == "DeviceStream" {
            let dev_name = attribute(tag, "name").unwrap_or_default();
            let uuid = attribute(tag, "uuid").unwrap_or_default();
            in_device = if device.is_empty() {
                dev_name != "Agent"
            } else {
                dev_name == device || uuid == device
            };
            if in_device {
                found = true;
                out.device = dev_name;
            }
            continue;
        }
        if !in_device || tag.ends_with('/') {
            continue;
        }
        // An observation: its text, up to its end tag.
        let Some(end) = xml[at..].find('<') else {
            break;
        };
        let text = unescape(xml[at..at + end].trim());
        let slot = match name {
            "Execution" => &mut out.execution,
            "DoorState" => &mut out.door,
            "Availability" => &mut out.availability,
            "Program" => &mut out.program,
            "PartCount" => {
                if out.part_count.is_none() {
                    out.part_count = text.parse::<f64>().ok().map(|v| v as i64);
                }
                continue;
            }
            _ => continue,
        };
        if slot.is_none() && !text.is_empty() {
            *slot = Some(text);
        }
    }
    if !found {
        return Err(if device.is_empty() {
            "MTConnect: 에이전트에 장비가 없습니다".to_string()
        } else {
            format!("MTConnect: 에이전트에 {device} 장비가 없습니다")
        });
    }
    Ok(out)
}

// --- the link ----------------------------------------------------------------

/// Where a machine's agent is, and which of its devices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub agent: String,
    pub device: String,
}

#[derive(Default)]
struct Shared {
    last: Option<Observation>,
    error: Option<String>,
    at: Option<Instant>,
}

/// A machine's agent, asked about once a second on a thread of its own.
pub struct Mirror {
    pub source: Source,
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Mirror {
    pub fn start(source: Source) -> Self {
        let shared: Arc<Mutex<Shared>> = Arc::default();
        let stop: Arc<AtomicBool> = Arc::default();
        let (s, st, src) = (shared.clone(), stop.clone(), source.clone());
        let thread = std::thread::spawn(move || {
            while !st.load(Ordering::Relaxed) {
                let result = split_url(&src.agent).and_then(|(host, port, base)| {
                    let path = if src.device.is_empty() {
                        format!("{base}/current")
                    } else {
                        format!("{base}/{}/current", src.device)
                    };
                    parse(&get(&host, port, &path)?, &src.device)
                });
                {
                    let mut s = s.lock().unwrap();
                    match result {
                        Ok(o) => {
                            s.last = Some(o);
                            s.error = None;
                            s.at = Some(Instant::now());
                        }
                        Err(e) => s.error = Some(e),
                    }
                }
                let start = Instant::now();
                while start.elapsed() < EVERY && !st.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        });
        Self {
            source,
            shared,
            stop,
            thread: Some(thread),
        }
    }

    /// The device's state as last read, unless the agent has not answered
    /// for five times as long as it is asked.
    pub fn observation(&self) -> Option<Observation> {
        let s = self.shared.lock().unwrap();
        s.at.filter(|at| at.elapsed() < EVERY * 5)
            .and(s.last.clone())
    }

    /// How it is going: the last state read, how long ago, and what is
    /// wrong, if anything.
    pub fn to_json(&self) -> Value {
        let s = self.shared.lock().unwrap();
        json!({
            "agent": self.source.agent,
            "device": self.source.device,
            "last": s.last.as_ref().map(Observation::to_json),
            // Whole seconds: the page told once a second, not every tick.
            "age": s.at.map(|a| a.elapsed().as_secs()),
            "error": s.error,
        })
    }
}

impl Drop for Mirror {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests;
