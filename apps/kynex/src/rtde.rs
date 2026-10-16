//! A real Universal Robots arm followed: its joints and its outputs read
//! over RTDE (the Real-Time Data Exchange, TCP port 30004), so the robot
//! in the scene moves as the real one does — a digital twin of it.
//!
//! RTDE, protocol 2: a version asked for, the outputs to be sent set up
//! (a recipe: names in, types back), then started; the controller sends a
//! data package at the frequency asked. Every package is a header — its
//! size, big-endian, with the header; its type, a letter — and a payload,
//! big-endian. Only reading: nothing is written to the robot.
//!
//! Its outputs are numbered as KYNEX numbers I/O: 0–7 standard, 8–15
//! configurable, 16–17 the tool's (`actual_digital_output_bits`).

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// RTDE's port on every UR controller.
pub const PORT: u16 = 30004;

const VERSION: u8 = b'V';
const SETUP_OUTPUTS: u8 = b'O';
const START: u8 = b'S';
const PAUSE: u8 = b'P';
const DATA: u8 = b'U';
const TEXT: u8 = b'M';

/// How often the controller is asked to send, a second: enough to see the
/// arm move smoothly, little enough for any network.
const FREQUENCY: f64 = 25.0;

/// What is asked for, in this order.
const OUTPUTS: &str = "actual_q,actual_digital_output_bits,runtime_state";

/// The robot as last read.
#[derive(Clone, Debug, PartialEq)]
pub struct State {
    /// Joint positions, radians, base to wrist 3.
    pub q: [f64; 6],
    /// Digital outputs, bit `n` output `n` (KYNEX's numbering).
    pub outputs: u64,
    /// The program's state: 1 stopped, 2 playing, 3 pausing, 4 paused, 5
    /// resuming (0 stopping).
    pub runtime: u32,
}

impl State {
    pub fn output(&self, n: u32) -> bool {
        n < 64 && self.outputs & (1 << n) != 0
    }

    pub fn to_json(&self) -> Value {
        json!({
            "q": self.q,
            "outputs": self.outputs,
            "program": match self.runtime {
                2 => "실행 중",
                3 | 4 => "일시 정지",
                5 => "재개 중",
                _ => "정지",
            },
        })
    }
}

/// One package: its type and payload.
fn send(stream: &mut impl Write, kind: u8, payload: &[u8]) -> Result<(), String> {
    let size = u16::try_from(payload.len() + 3).map_err(|_| "RTDE: package too long")?;
    let mut out = Vec::with_capacity(payload.len() + 3);
    out.extend_from_slice(&size.to_be_bytes());
    out.push(kind);
    out.extend_from_slice(payload);
    stream.write_all(&out).map_err(|e| format!("RTDE: {e}"))
}

/// The next package from the controller.
fn receive(stream: &mut impl Read) -> Result<(u8, Vec<u8>), String> {
    let mut head = [0u8; 3];
    stream
        .read_exact(&mut head)
        .map_err(|e| format!("RTDE: {e}"))?;
    let size = u16::from_be_bytes([head[0], head[1]]) as usize;
    if size < 3 {
        return Err("RTDE: a package shorter than its header".into());
    }
    let mut payload = vec![0u8; size - 3];
    stream
        .read_exact(&mut payload)
        .map_err(|e| format!("RTDE: {e}"))?;
    Ok((head[2], payload))
}

/// The answer of type `kind` to what was just asked: text messages the
/// controller sends meanwhile passed over.
fn answer(stream: &mut impl Read, kind: u8) -> Result<Vec<u8>, String> {
    for _ in 0..16 {
        let (k, payload) = receive(stream)?;
        if k == kind {
            return Ok(payload);
        }
        if k != TEXT && k != DATA {
            return Err(format!(
                "RTDE: answer {:?} to {:?}",
                k as char, kind as char
            ));
        }
    }
    Err(format!("RTDE: no answer to {:?}", kind as char))
}

/// A data package of the recipe [`OUTPUTS`] read.
pub fn parse(payload: &[u8]) -> Result<State, String> {
    // The recipe's id, 6 doubles, a u64, a u32.
    if payload.len() < 1 + 48 + 8 + 4 {
        return Err(format!("RTDE: a data package of {} bytes", payload.len()));
    }
    let d = &payload[1..];
    let mut q = [0.0; 6];
    for (i, x) in q.iter_mut().enumerate() {
        *x = f64::from_be_bytes(d[i * 8..i * 8 + 8].try_into().unwrap());
    }
    Ok(State {
        q,
        outputs: u64::from_be_bytes(d[48..56].try_into().unwrap()),
        runtime: u32::from_be_bytes(d[56..60].try_into().unwrap()),
    })
}

/// A link to a controller, its outputs set up and started.
pub struct Link {
    stream: TcpStream,
}

impl Link {
    /// Connects to the controller at `host` (a name or an address, a port
    /// after a colon if not RTDE's own), and starts its outputs.
    pub fn open(host: &str) -> Result<Self, String> {
        let address = if host.contains(':') {
            host.to_string()
        } else {
            format!("{host}:{PORT}")
        };
        let to = address
            .to_socket_addrs()
            .map_err(|e| format!("{address}: {e}"))?
            .next()
            .ok_or_else(|| format!("{address}: 주소를 찾지 못했습니다"))?;
        let mut stream = TcpStream::connect_timeout(&to, Duration::from_secs(3))
            .map_err(|e| format!("{address}: {e}"))?;
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .map_err(|e| e.to_string())?;
        let _ = stream.set_nodelay(true);
        // Protocol 2.
        send(&mut stream, VERSION, &2u16.to_be_bytes())?;
        if answer(&mut stream, VERSION)?.first() != Some(&1) {
            return Err(
                "RTDE 프로토콜 2를 지원하지 않는 컨트롤러입니다 (PolyScope 3.4/5 이상 필요)".into(),
            );
        }
        let mut setup = FREQUENCY.to_be_bytes().to_vec();
        setup.extend_from_slice(OUTPUTS.as_bytes());
        send(&mut stream, SETUP_OUTPUTS, &setup)?;
        let recipe = answer(&mut stream, SETUP_OUTPUTS)?;
        let types = String::from_utf8_lossy(recipe.get(1..).unwrap_or_default()).to_string();
        if types != "VECTOR6D,UINT64,UINT32" {
            return Err(format!("RTDE: 출력 설정이 거절되었습니다 ({types})"));
        }
        send(&mut stream, START, &[])?;
        if answer(&mut stream, START)?.first() != Some(&1) {
            return Err("RTDE: 시작이 거절되었습니다".into());
        }
        Ok(Self { stream })
    }

    /// The next state the controller sends.
    pub fn next(&mut self) -> Result<State, String> {
        loop {
            let (kind, payload) = receive(&mut self.stream)?;
            match kind {
                DATA => return parse(&payload),
                TEXT => continue,
                other => return Err(format!("RTDE: unexpected {:?}", other as char)),
            }
        }
    }

    /// Stops the outputs, politely.
    pub fn close(mut self) {
        let _ = send(&mut self.stream, PAUSE, &[]);
    }
}

#[derive(Default)]
struct Shared {
    last: Option<State>,
    error: Option<String>,
    at: Option<Instant>,
}

/// A controller followed on a thread of its own: connected, read as it
/// sends, connected again a moment after a fault.
pub struct Follow {
    pub host: String,
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Follow {
    pub fn start(host: &str) -> Self {
        let shared: Arc<Mutex<Shared>> = Arc::default();
        let stop: Arc<AtomicBool> = Arc::default();
        let (s, st, h) = (shared.clone(), stop.clone(), host.to_string());
        let thread = std::thread::spawn(move || {
            while !st.load(Ordering::Relaxed) {
                match Link::open(&h) {
                    Ok(mut link) => {
                        while !st.load(Ordering::Relaxed) {
                            match link.next() {
                                Ok(state) => {
                                    let mut s = s.lock().unwrap();
                                    s.last = Some(state);
                                    s.error = None;
                                    s.at = Some(Instant::now());
                                }
                                Err(e) => {
                                    s.lock().unwrap().error = Some(e);
                                    break;
                                }
                            }
                        }
                        link.close();
                    }
                    Err(e) => s.lock().unwrap().error = Some(e),
                }
                // Again in a moment.
                let start = Instant::now();
                while start.elapsed() < Duration::from_secs(2) && !st.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        });
        Self {
            host: host.to_string(),
            shared,
            stop,
            thread: Some(thread),
        }
    }

    /// The robot as last read, unless nothing came for a second.
    pub fn state(&self) -> Option<State> {
        let s = self.shared.lock().unwrap();
        s.at.filter(|at| at.elapsed() < Duration::from_secs(1))
            .and(s.last.clone())
    }

    /// How it is going, for the page.
    pub fn to_json(&self) -> Value {
        let s = self.shared.lock().unwrap();
        let live = s.at.is_some_and(|at| at.elapsed() < Duration::from_secs(1));
        json!({
            "host": self.host,
            "connected": live,
            "state": s.last.as_ref().filter(|_| live).map(State::to_json),
            "error": if live { None } else { s.error.clone() },
        })
    }
}

impl Drop for Follow {
    /// Told to stop, the thread ends by itself within a read's time: the
    /// view does not wait on a network.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        drop(self.thread.take());
    }
}

#[cfg(test)]
mod tests;
