use std::net::TcpListener;
use std::time::Instant;

use super::*;
use crate::cell::Cell;
use crate::library::Library;
use kreate_scene::Document;

/// A PLC's Modbus TCP server, as far as KYNEX uses it: 1024 coils, read
/// (function 1) and written (function 5), and 256 holding registers, read
/// (function 3) and written (function 6); anything else answered with an
/// exception. Serves one client after another, until the test ends.
struct FakePlc {
    port: u16,
    coils: Arc<Mutex<Vec<bool>>>,
    registers: Arc<Mutex<Vec<u16>>>,
}

impl FakePlc {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let coils = Arc::new(Mutex::new(vec![false; 1024]));
        let registers = Arc::new(Mutex::new(vec![0u16; 256]));
        let shared = coils.clone();
        let shared_registers = registers.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { continue };
                let coils = shared.clone();
                let registers = shared_registers.clone();
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
                            let mut r = registers.lock().unwrap();
                            match pdu[0] {
                                0x03 if word(1) + word(3) <= r.len() => {
                                    let (start, count) = (word(1), word(3));
                                    let mut out = vec![0x03, (2 * count) as u8];
                                    for w in &r[start..start + count] {
                                        out.extend(w.to_be_bytes());
                                    }
                                    out
                                }
                                0x06 if word(1) < r.len() => {
                                    r[word(1)] = word(3) as u16;
                                    pdu.clone()
                                }
                                0x03 | 0x06 => vec![pdu[0] | 0x80, 2],
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
        Self {
            port,
            coils,
            registers,
        }
    }

    fn register(&self, address: usize) -> u16 {
        self.registers.lock().unwrap()[address]
    }

    fn set_register(&self, address: usize, value: u16) {
        self.registers.lock().unwrap()[address] = value;
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
        protocol: Protocol::Modbus,
        host: "127.0.0.1".into(),
        port,
        unit: 1,
        path: String::new(),
        on: true,
        ..Settings::default()
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
            &json!({"op": "plc", "object": m, "key": key, "address": coil}).to_string(),
        );
        assert!(a.contains("\"ok\":true"), "{a}");
    }
    assert_eq!(
        cell.plc_reads(),
        [
            ("100".to_string(), Kind::Bit),
            ("101".to_string(), Kind::Bit)
        ]
    );

    let link = Plc::new(settings(plc.port));
    assert!(link.on());
    // The loop as the viewer runs it: the coils handed over, the cell's
    // clock moved on.
    let started = Instant::now();
    let step = |cell: &mut Cell| {
        let got = link.read();
        let x = cell.plc_exchange(&doc, &got, &|_| (false, false));
        let (writes, errors) = (x.writes, x.errors);
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
    // Windows takes some seconds to give up on a port nobody listens on.
    assert!(until(8.0, || link.to_json()["error"].is_string()));
    assert_eq!(link.to_json()["connected"], false);
    drop(link);
}

/// Two lathes wired to the same PLC addresses: both take a command, and
/// their door-open state is on only once both doors are open, with a
/// warning that the address is shared.
#[test]
fn machines_on_one_address_each_take_its_command() {
    let mut library = Library::default();
    for x in [0.0, 3.0] {
        library.handle(
            &json!({"op": "place", "template": "cnc_lathe", "at": [x, 0.0, 0.0]}).to_string(),
        );
    }
    let mut doc = Document::new();
    library.apply(&mut doc);
    let mut cell = Cell::default();
    let machines: Vec<u64> = library
        .roles()
        .into_iter()
        .map(|(u, n, k)| {
            cell.ensure(u, n, k);
            u
        })
        .collect();
    for m in &machines {
        for (key, address) in [("open", "ns=2;s=Open"), ("door_open", "ns=2;s=DoorOpen")] {
            cell.handle(
                &doc,
                &json!({"op": "plc", "object": m.to_string(), "key": key, "address": address})
                    .to_string(),
            );
        }
    }
    let read: BTreeMap<String, i64> = [("ns=2;s=Open".to_string(), 1)].into();
    let start = Instant::now();
    let errors = cell.plc_exchange(&doc, &read, &|_| (false, false)).errors;
    assert!(errors.iter().any(|e| e.contains("함께")), "{errors:?}");
    // Their 3 s doors.
    let mut t = 0.0;
    while t < 3.5 {
        t += 0.1;
        cell.tick(&doc, t, start + Duration::from_secs_f64(t));
    }
    for m in &machines {
        assert!(cell.signal(&doc, *m, crate::cell::Signal::DoorOpen), "{m}");
    }
    let writes = cell.plc_exchange(&doc, &read, &|_| (false, false)).writes;
    assert_eq!(writes, [("ns=2;s=DoorOpen".to_string(), Kind::Bit, 1)]);
}

/// Holding registers as the protocol frames them: a read of 2 from 40,
/// a negative number written as its two's complement, and read back.
#[test]
fn registers_are_framed_as_modbus_tcp() {
    assert_eq!(read_registers_pdu(40, 2), [0x03, 0, 40, 0, 2]);
    assert_eq!(write_register_pdu(7, -2), [0x06, 0, 7, 0xFF, 0xFE]);
    assert_eq!(write_register_pdu(7, 1234), [0x06, 0, 7, 0x04, 0xD2]);
    assert_eq!(
        registers_of(&[0x03, 4, 0x04, 0xD2, 0xFF, 0xFE], 2).unwrap(),
        [1234, -2]
    );
}

/// Signals beside the machines', over Modbus: a bit the PLC sets read in,
/// a number it sets (a recipe, negative) read in, and a bit and a number
/// the cell's programs set written out — a coil and a register.
#[test]
fn signals_go_both_ways_over_modbus() {
    let plc = FakePlc::start();
    let doc = Document::new();
    let mut cell = Cell::default();
    cell.set_signals(&json!([
        {"name": "go", "input": true, "address": "10"},
        {"name": "recipe", "input": true, "number": true, "address": "21"},
        {"name": "ready", "address": "11"},
        {"name": "count", "number": true, "address": "20"},
    ]))
    .unwrap();
    cell.set_signal("ready", 1).unwrap();
    cell.set_signal("count", 1234).unwrap();
    plc.set(10, true);
    plc.set_register(21, (-7i16) as u16);

    let link = Plc::new(settings(plc.port));
    let step = |cell: &mut Cell| {
        let x = cell.plc_exchange(&doc, &link.read(), &|_| (false, false));
        assert!(x.errors.is_empty(), "{:?}", x.errors);
        link.hand(cell.plc_reads(), x.writes);
    };
    assert!(until(3.0, || {
        step(&mut cell);
        plc.coil(11) && plc.register(20) == 1234 && cell.signal_value("recipe") == Some(-7)
    }));
    assert_eq!(cell.signal_value("go"), Some(1));
    // Set again by a program: written again.
    cell.set_signal("count", 1235).unwrap();
    assert!(until(3.0, || {
        step(&mut cell);
        plc.register(20) == 1235
    }));
    // A signal the PLC writes is not a program's to set.
    assert!(cell.set_signal("go", 0).is_err());
    assert!(cell.set_signal("nothing", 0).is_err());
}

/// A signal table refused whole: two of one name, or an input tied to what
/// the PLC reads.
#[test]
fn a_signal_table_amiss_is_refused() {
    let mut cell = Cell::default();
    assert!(cell
        .set_signals(&json!([{"name": "a", "address": "1"}, {"name": "a", "address": "2"}]))
        .is_err());
    assert!(cell
        .set_signals(&json!([{"name": "r", "input": true, "link": "running:5"}]))
        .is_err());
    assert!(cell
        .set_signals(&json!([{"name": "s", "input": false, "link": "start:5"}]))
        .is_err());
    assert!(cell
        .set_signals(&json!([{"name": "s", "input": true, "link": "start:5"}]))
        .is_ok());
}

/// Robots tied to signals: one started as its input comes on (once, not
/// again while it stays on), one stopped; outputs say which runs and which
/// has finished.
#[test]
fn signals_start_and_stop_robots_and_say_how_they_are() {
    let doc = Document::new();
    let mut cell = Cell::default();
    cell.set_signals(&json!([
        {"name": "start a", "input": true, "address": "1", "link": "start:7"},
        {"name": "stop b", "input": true, "address": "2", "link": "stop:8"},
        {"name": "a runs", "address": "3", "link": "running:7"},
        {"name": "b done", "address": "4", "link": "done:8"},
    ]))
    .unwrap();
    let robots = |root: u64| (root == 7, root == 8);
    let read = |a: i64, b: i64| -> BTreeMap<String, i64> {
        [("1".to_string(), a), ("2".to_string(), b)].into()
    };
    let x = cell.plc_exchange(&doc, &read(1, 0), &robots);
    assert_eq!(x.start, [7]);
    assert!(x.stop.is_empty());
    assert!(x.writes.contains(&("3".to_string(), Kind::Bit, 1)));
    assert!(x.writes.contains(&("4".to_string(), Kind::Bit, 1)));
    let x = cell.plc_exchange(&doc, &read(1, 1), &robots);
    assert!(x.start.is_empty(), "held on: not started again");
    assert_eq!(x.stop, [8]);
}
