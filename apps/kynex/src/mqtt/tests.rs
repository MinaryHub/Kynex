use std::net::TcpListener;

use super::*;

/// VDA 5050's state message, cut down to what is read.
fn vda(x: f64, y: f64, theta: f64) -> Vec<u8> {
    json!({
        "headerId": 7,
        "manufacturer": "Acme",
        "serialNumber": "amr-01",
        "agvPosition": { "x": x, "y": y, "theta": theta, "mapId": "hall", "positionInitialized": true },
        "driving": true,
        "batteryState": { "batteryCharge": 81.5, "charging": false },
        "errors": [{ "errorType": "noRoute", "errorLevel": "WARNING", "errorDescription": "길 막힘" }],
    })
    .to_string()
    .into_bytes()
}

/// A broker of the user's own making: takes the connection (answering
/// `code`), the subscription, and publishes `messages` on the topic.
fn broker(code: u8, messages: Vec<Vec<u8>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let (first, rest) = receive(&mut s, 1 << 16).unwrap();
        assert_eq!(first, 0x10);
        assert_eq!(&rest[..7], b"\x00\x04MQTT\x04");
        // A user and a password: flags 0xC2.
        assert_eq!(rest[7], 0xc2);
        s.write_all(&[0x20, 2, 0, code]).unwrap();
        if code != 0 {
            return;
        }
        let (first, rest) = receive(&mut s, 1 << 16).unwrap();
        assert_eq!(first, 0x82);
        let n = u16::from_be_bytes([rest[2], rest[3]]) as usize;
        let topic = String::from_utf8(rest[4..4 + n].to_vec()).unwrap();
        s.write_all(&[0x90, 3, rest[0], rest[1], 0]).unwrap();
        for m in messages {
            let mut body = Vec::new();
            string(&mut body, &topic.replace('+', "amr-01"));
            body.extend_from_slice(&m);
            s.write_all(&packet(0x30, &body)).unwrap();
            std::thread::sleep(Duration::from_millis(20));
        }
        std::thread::sleep(Duration::from_millis(500));
    });
    address
}

#[test]
fn a_vda_5050_state_is_where_the_robot_is() {
    let r = rover_of(&vda(3.5, -1.25, 1.57)).unwrap();
    assert_eq!((r.x, r.y, r.theta), (3.5, -1.25, 1.57));
    assert_eq!(r.driving, Some(true));
    assert_eq!(r.battery, Some(81.5));
    assert_eq!(r.errors, ["길 막힘"]);
    // A fleet of other habits: x, y, yaw at the top.
    let r = rover_of(br#"{"x": 1, "y": 2, "yaw": 0.5}"#).unwrap();
    assert_eq!((r.x, r.y, r.theta), (1.0, 2.0, 0.5));
    assert!(rover_of(br#"{"agvPosition": {"theta": 1}}"#).is_none());
    assert!(rover_of(b"not json").is_none());
}

#[test]
fn a_long_packet_s_length_goes_in_more_bytes() {
    let p = packet(0x30, &vec![0u8; 300]);
    assert_eq!(&p[..3], &[0x30, 0xac, 0x02]);
    assert_eq!(receive(&mut &p[..], 1000).unwrap().1.len(), 300);
}

#[test]
fn what_a_broker_publishes_is_read() {
    let address = broker(0, vec![vda(1.0, 2.0, 0.0), vda(1.5, 2.0, 0.1)]);
    let mut c = Client::open(&address, "kynex", "secret", "uagv/v2/Acme/+/state").unwrap();
    let mut got = Vec::new();
    let start = Instant::now();
    while got.len() < 2 && start.elapsed() < Duration::from_secs(3) {
        if let Some((topic, payload)) = c.next().unwrap() {
            assert_eq!(topic, "uagv/v2/Acme/amr-01/state");
            got.push(rover_of(&payload).unwrap().x);
        }
    }
    assert_eq!(got, [1.0, 1.5]);
}

#[test]
fn a_broker_that_refuses_says_why() {
    let address = broker(5, Vec::new());
    let err = Client::open(&address, "kynex", "wrong", "#").err().unwrap();
    assert!(err.contains("비밀번호"), "{err}");
}

#[test]
fn a_feed_keeps_the_last_place_said() {
    let address = broker(0, (0..10).map(|k| vda(k as f64 * 0.1, 0.0, 0.0)).collect());
    let feed = Feed::start(&address, "kynex", "secret", "a/+/state");
    let start = Instant::now();
    while feed.rover().is_none_or(|r| r.x < 0.85) && start.elapsed() < Duration::from_secs(3) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!((feed.rover().unwrap().x - 0.9).abs() < 1e-9);
    assert_eq!(feed.to_json()["battery"], 81.5);
}
