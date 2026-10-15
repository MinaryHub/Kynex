use std::net::TcpListener;

use super::*;

/// The MTConnect Institute's public demo agent's `/current`, as it answered
/// on 2026-10-15: an OKUMA lathe and a Mazak, as their agent reports them.
const DEMO: &str = include_str!("demo_current.xml");

/// Each device as the demo agent reports it; the first that is not the
/// agent's own when none is named; a device it has not got refused.
#[test]
fn a_device_s_state_is_read_from_a_real_agent_s_answer() {
    let okuma = parse(DEMO, "OKUMA").unwrap();
    assert_eq!(okuma.device, "OKUMA");
    assert_eq!(okuma.execution.as_deref(), Some("ACTIVE"));
    assert_eq!(okuma.door, None, "an OKUMA lathe reports no door");
    assert_eq!(okuma.part_count, Some(0));
    assert_eq!(okuma.availability.as_deref(), Some("AVAILABLE"));
    assert_eq!(okuma.program.as_deref(), Some("IMTS-2022-1E-MM.MIN"));

    let mazak = parse(DEMO, "Mazak").unwrap();
    assert_eq!(mazak.door.as_deref(), Some("CLOSED"));
    assert_eq!(mazak.execution.as_deref(), Some("ACTIVE"));
    assert_eq!(mazak.program.as_deref(), Some("101010"));
    // By its uuid too.
    assert_eq!(parse(DEMO, "OKUMA.123456").unwrap().device, "OKUMA");

    assert_eq!(parse(DEMO, "").unwrap().device, "OKUMA");
    assert!(parse(DEMO, "Haas").unwrap_err().contains("Haas"));
    let error = r#"<?xml version="1.0"?><MTConnectError><Errors><Error errorCode="NO_DEVICE">Could not find the device 'Haas'</Error></Errors></MTConnectError>"#;
    assert!(parse(error, "Haas").unwrap_err().contains("Could not find"));
}

/// Agent addresses as shop floors write them.
#[test]
fn agent_addresses_split_into_host_port_and_path() {
    assert_eq!(
        split_url("http://10.0.0.5:5000").unwrap(),
        ("10.0.0.5".into(), 5000, "".into())
    );
    assert_eq!(
        split_url("http://agent/mtc/").unwrap(),
        ("agent".into(), 80, "/mtc".into())
    );
    assert!(split_url("https://agent").is_err());
    assert!(split_url("http://:5000").is_err());
}

/// An agent answering in chunks, and one with a length, read whole.
#[test]
fn an_answer_is_read_whole_in_chunks_or_by_length() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for (i, stream) in listener.incoming().enumerate().take(2) {
            let mut s = stream.unwrap();
            // The whole request head, before answering.
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") && s.read(&mut byte).unwrap_or(0) == 1 {
                head.push(byte[0]);
            }
            let reply = if i == 0 {
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n".to_string()
            } else {
                "HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nbody".to_string()
            };
            s.write_all(reply.as_bytes()).unwrap();
        }
    });
    assert_eq!(get("127.0.0.1", port, "/current").unwrap(), "hello world");
    assert_eq!(get("127.0.0.1", port, "/current").unwrap(), "body");
}

/// The public demo agent, live.
#[test]
#[ignore = "asks demo.mtconnect.org over the internet"]
fn the_public_demo_agent_is_read_live() {
    let mirror = Mirror::start(Source::MtConnect {
        agent: "http://demo.mtconnect.org:5000".into(),
        device: "OKUMA".into(),
    });
    let start = Instant::now();
    while mirror.observation().is_none() && start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(100));
    }
    let seen = mirror.observation().expect(&mirror.to_json().to_string());
    println!("{}", seen.to_json());
    assert_eq!(seen.device, "OKUMA");
    assert!(seen.execution.is_some());
}

/// Nothing that could break out of the request line: an address or a
/// device with a line break, a space or a slash where it does not belong
/// is refused before anything is sent.
#[test]
fn an_address_cannot_inject_into_the_request() {
    assert!(split_url("http://agent:5000/x\r\nX-Evil: 1").is_err());
    assert!(split_url("http://agent:5000/a b").is_err());
    assert!(get("127.0.0.1", 1, "/Mazak\r\nHost: evil/current").is_err());
    assert!(device_safe("Mazak-1.A_b~"));
    assert!(!device_safe("Mazak/../x"));
    assert!(!device_safe("a\r\nb"));
}

/// An agent trickling its answer a byte at a time is let go at the
/// deadline, not waited on for ever.
#[test]
#[ignore = "takes the 10 s deadline"]
fn a_trickling_agent_is_let_go() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = s.read(&mut buf);
        for b in b"HTTP/1.1 200 OK\r\nX-Slow: "
            .iter()
            .chain(std::iter::repeat(&b'a'))
        {
            if s.write_all(&[*b]).is_err() {
                return;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    });
    let start = Instant::now();
    assert!(get("127.0.0.1", port, "/current").is_err());
    assert!(
        start.elapsed() < Duration::from_secs(15),
        "{:?}",
        start.elapsed()
    );
}

/// A umati source given as one URL: its host, port and path; a password
/// never saved; a URL that is not OPC UA's refused.
#[test]
fn a_umati_server_is_given_as_one_url() {
    let v = serde_json::json!({
        "kind": "umati", "url": "opc.tcp://10.0.0.5:4841/umati/",
        "node": "ns=3;s=Machine1", "security": "encrypt",
        "user": "kynex", "password": "secret",
    });
    let Some(Source::Umati { settings, node }) = Source::of(&v) else {
        panic!("a umati source");
    };
    assert_eq!(
        (
            settings.host.as_str(),
            settings.port,
            settings.path.as_str()
        ),
        ("10.0.0.5", 4841, "/umati/")
    );
    assert_eq!(node, "ns=3;s=Machine1");
    let source = Source::Umati { settings, node };
    assert!(source.check().is_ok());
    let saved = source.to_json();
    assert_eq!(saved["url"], "opc.tcp://10.0.0.5:4841/umati/");
    assert!(!saved.to_string().contains("secret"));

    let default_port =
        serde_json::json!({ "kind": "umati", "url": "opc.tcp://machine", "node": "i=85" });
    let Some(Source::Umati { settings, .. }) = Source::of(&default_port) else {
        panic!("a umati source");
    };
    assert_eq!((settings.host.as_str(), settings.port), ("machine", 4840));
    assert!(Source::of(
        &serde_json::json!({ "kind": "umati", "url": "http://machine", "node": "i=85" })
    )
    .is_none());
}
