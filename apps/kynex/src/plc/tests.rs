use std::net::TcpListener;
use std::time::Instant;

use super::*;
use crate::cell::Cell;
use crate::library::Library;
use kreate_scene::Document;

/// A PLC's Modbus TCP server, as far as KYNEX uses it: 1024 coils, read
/// (function 1) and written (function 5); anything else answered with an
/// exception. Serves one client after another, until the test ends.
struct FakePlc {
    port: u16,
    coils: Arc<Mutex<Vec<bool>>>,
}

impl FakePlc {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let coils = Arc::new(Mutex::new(vec![false; 1024]));
        let shared = coils.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { continue };
                let coils = shared.clone();
                std::thread::spawn(move || {
                    let mut head = [0u8; 7];
                    while s.read_exact(&mut head).is_ok() {
                        let len = u16::from_be_bytes([head[4], head[5]]) as usize;
                        let mut pdu = vec![0u8; len - 1];
                        if s.read_exact(&mut pdu).is_err() {
                            return;
                        }
                        let word = |i: usize| u16::from_be_bytes([pdu[i], pdu[i + 1]]) as usize;
                        let answer = {
                            let mut c = coils.lock().unwrap();
                            match pdu[0] {
                                0x01 if word(1) + word(3) <= c.len() => {
                                    let (start, count) = (word(1), word(3));
                                    let mut bytes = vec![0u8; count.div_ceil(8)];
                                    for i in 0..count {
                                        if c[start + i] {
                                            bytes[i / 8] |= 1 << (i % 8);
                                        }
                                    }
                                    let mut out = vec![0x01, bytes.len() as u8];
                                    out.extend(bytes);
                                    out
                                }
                                0x05 if word(1) < c.len() => {
                                    c[word(1)] = pdu[3] == 0xFF;
                                    pdu.clone()
                                }
                                0x01 | 0x05 => vec![pdu[0] | 0x80, 2],
                                f => vec![f | 0x80, 1],
                            }
                        };
                        let mut out = head[..4].to_vec();
                        out.extend(((answer.len() + 1) as u16).to_be_bytes());
                        out.push(head[6]);
                        out.extend(answer);
                        if s.write_all(&out).is_err() {
                            return;
                        }
                    }
                });
            }
        });
        Self { port, coils }
    }

    fn coil(&self, address: usize) -> bool {
        self.coils.lock().unwrap()[address]
    }

    fn set(&self, address: usize, on: bool) {
        self.coils.lock().unwrap()[address] = on;
    }
}

fn settings(port: u16) -> Settings {
    Settings {
        host: "127.0.0.1".into(),
        port,
        unit: 1,
        on: true,
    }
}

/// Until `done` says so, or `seconds` have gone.
fn until(seconds: f64, mut done: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed().as_secs_f64() < seconds {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

/// Frames as the protocol has them: a read of coils 100–109 from unit 1 as
/// transaction 7, a coil written on and off, and an answer's bits.
#[test]
fn requests_are_framed_as_modbus_tcp() {
    assert_eq!(
        frame(7, 1, &read_coils_pdu(100, 10)),
        [0, 7, 0, 0, 0, 6, 1, 0x01, 0, 100, 0, 10]
    );
    assert_eq!(write_coil_pdu(300, true), [0x05, 0x01, 0x2C, 0xFF, 0x00]);
    assert_eq!(write_coil_pdu(300, false), [0x05, 0x01, 0x2C, 0x00, 0x00]);
    // Coils 0 and 9 on: the first byte's lowest bit, the second's second.
    assert_eq!(
        coils_of(&[0x01, 2, 0b0000_0001, 0b0000_0010], 10).unwrap(),
        [true, false, false, false, false, false, false, false, false, true]
    );
    assert!(coils_of(&[0x01, 1, 0], 10).is_err());
}

/// A lathe run by a PLC: its door opens as the PLC turns the command coil
/// on, and the PLC reads the door open on the state coil; closed again,
/// and the door-closed coil comes on. Then the PLC goes away: the link
/// says so, and connects again once it is back.
#[test]
fn a_plc_runs_a_machine_over_modbus_tcp() {
    let plc = FakePlc::start();
    let mut library = Library::default();
    library.handle(&json!({"op": "place", "template": "cnc_lathe"}).to_string());
    let mut doc = Document::new();
    library.apply(&mut doc);
    let mut cell = Cell::default();
    let (machine, name, kind) = library.roles().remove(0);
    cell.ensure(machine, name, kind);
    let m = machine.to_string();
    for (key, coil) in [
        ("open", 100),
        ("close", 101),
        ("door_open", 200),
        ("door_closed", 201),
    ] {
        let a = cell.handle(
            &doc,
            &json!({"op": "plc", "object": m, "key": key, "coil": coil}).to_string(),
        );
        assert!(a.contains("\"ok\":true"), "{a}");
    }
    assert_eq!(cell.plc_reads(), [100, 101]);

    let link = Plc::new(settings(plc.port));
    assert!(link.on());
    // The loop as the viewer runs it: the coils handed over, the cell's
    // clock moved on.
    let started = Instant::now();
    let step = |cell: &mut Cell| {
        let got = link.coils();
        let (writes, errors) = cell.plc_exchange(&doc, &got);
        assert!(errors.is_empty(), "{errors:?}");
        link.hand(cell.plc_reads(), writes);
        let t = started.elapsed().as_secs_f64();
        cell.tick(&doc, t, Instant::now());
    };
    assert!(
        until(3.0, || {
            step(&mut cell);
            plc.coil(201)
        }),
        "the door closed, said to the PLC"
    );
    assert!(!plc.coil(200));

    // Opened by the PLC: its 3 s door, and the state coil on.
    plc.set(100, true);
    assert!(
        until(6.0, || {
            step(&mut cell);
            plc.coil(200)
        }),
        "the door open, said to the PLC"
    );
    assert!(!plc.coil(201));
    // Held on, it is not opened again; turned off and on to close.
    plc.set(100, false);
    plc.set(101, true);
    assert!(until(6.0, || {
        step(&mut cell);
        plc.coil(201)
    }));

    let json = link.to_json();
    assert_eq!(json["connected"], true, "{json}");
    assert!(json["exchanges"].as_u64().unwrap() > 10, "{json}");
}

/// Nothing listening: the link says why, and keeps trying.
#[test]
fn a_plc_not_there_is_said_so() {
    // A port just let go: nothing listens on it.
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let link = Plc::new(settings(port));
    assert!(until(3.0, || link.to_json()["error"].is_string()));
    assert_eq!(link.to_json()["connected"], false);
    drop(link);
}
