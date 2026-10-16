use std::net::TcpListener;

use super::*;

/// A controller of the user's own making, as RTDE has it: answers the
/// version, the recipe and the start, a text message among them, then
/// sends `states`, and holds the line open a while.
fn controller(types: &'static str, states: Vec<State>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let (kind, payload) = receive(&mut s).unwrap();
        assert_eq!((kind, payload), (VERSION, vec![0, 2]));
        send(&mut s, TEXT, b"\x00hello").unwrap();
        send(&mut s, VERSION, &[1]).unwrap();
        let (kind, payload) = receive(&mut s).unwrap();
        assert_eq!(kind, SETUP_OUTPUTS);
        assert_eq!(
            f64::from_be_bytes(payload[..8].try_into().unwrap()),
            FREQUENCY
        );
        assert_eq!(&payload[8..], OUTPUTS.as_bytes());
        let mut recipe = vec![1];
        recipe.extend_from_slice(types.as_bytes());
        send(&mut s, SETUP_OUTPUTS, &recipe).unwrap();
        if receive(&mut s).map(|r| r.0) != Ok(START) {
            return;
        }
        send(&mut s, START, &[1]).unwrap();
        for state in states {
            let mut data = vec![1];
            for x in state.q {
                data.extend_from_slice(&x.to_be_bytes());
            }
            data.extend_from_slice(&state.outputs.to_be_bytes());
            data.extend_from_slice(&state.runtime.to_be_bytes());
            if send(&mut s, DATA, &data).is_err() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        std::thread::sleep(Duration::from_millis(300));
    });
    address
}

fn state(k: f64) -> State {
    State {
        q: [k, -1.57, 1.2, -1.2, -1.57, 0.5 * k],
        // The tool's first output on.
        outputs: 1 << 16 | 0b101,
        runtime: 2,
    }
}

#[test]
fn a_controller_s_joints_and_outputs_are_read_as_it_sends_them() {
    let address = controller("VECTOR6D,UINT64,UINT32", vec![state(0.1), state(0.2)]);
    let mut link = Link::open(&address).unwrap();
    assert_eq!(link.next().unwrap(), state(0.1));
    let s = link.next().unwrap();
    assert_eq!(s, state(0.2));
    assert!(s.output(16) && s.output(0) && s.output(2) && !s.output(1));
    assert_eq!(s.to_json()["program"], "실행 중");
}

#[test]
fn a_recipe_the_controller_cannot_send_is_refused() {
    let address = controller("VECTOR6D,NOT_FOUND,UINT32", Vec::new());
    let err = Link::open(&address).err().unwrap();
    assert!(err.contains("NOT_FOUND"), "{err}");
}

#[test]
fn a_robot_followed_is_its_last_state_while_it_sends() {
    let states = (0..40).map(|k| state(k as f64 * 0.01)).collect();
    let address = controller("VECTOR6D,UINT64,UINT32", states);
    let follow = Follow::start(&address);
    let start = Instant::now();
    while follow.state().is_none() && start.elapsed() < Duration::from_secs(3) {
        std::thread::sleep(Duration::from_millis(10));
    }
    let s = follow.state().expect("a state within 3 s");
    assert!(s.q[0] >= 0.0 && s.q[1] == -1.57);
    assert_eq!(follow.to_json()["connected"], true);
}

#[test]
fn nobody_there_is_said() {
    // A port nothing listens on.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    drop(listener);
    assert!(Link::open(&address).is_err());
}
