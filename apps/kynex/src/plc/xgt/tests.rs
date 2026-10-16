use std::collections::BTreeMap;
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use super::*;

/// A PLC of the user's own making: variables by name, each its bytes, as
/// last written. A name of device `Z` it does not have.
fn plc() -> (String, Arc<Mutex<BTreeMap<String, Vec<u8>>>>) {
    let memory: Arc<Mutex<BTreeMap<String, Vec<u8>>>> = Arc::default();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let mem = memory.clone();
    std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        loop {
            let mut head = [0u8; 20];
            if s.read_exact(&mut head).is_err() {
                return;
            }
            assert_eq!(&head[..10], COMPANY);
            assert_eq!(head[13], CLIENT);
            let sum = head[..19].iter().fold(0u8, |a, b| a.wrapping_add(*b));
            assert_eq!(head[19], sum, "the header's check sum");
            let n = u16::from_le_bytes([head[16], head[17]]) as usize;
            let mut body = vec![0u8; n];
            s.read_exact(&mut body).unwrap();
            let le = |at: usize| u16::from_le_bytes([body[at], body[at + 1]]);
            let (command, kind, count) = (le(0), le(2), le(6) as usize);
            let mut at = 8;
            let mut names = Vec::new();
            for _ in 0..count {
                let n = le(at) as usize;
                names.push(String::from_utf8(body[at + 2..at + 2 + n].to_vec()).unwrap());
                at += 2 + n;
            }
            let width = [1, 1, 2, 4][kind as usize];
            let missing = names.iter().any(|n| n.starts_with("%Z"));
            let mut out = Vec::new();
            out.extend_from_slice(&(command + 1).to_le_bytes());
            out.extend_from_slice(&kind.to_le_bytes());
            out.extend_from_slice(&[0, 0]);
            if missing {
                out.extend_from_slice(&1u16.to_le_bytes());
                out.extend_from_slice(&0x0003u16.to_le_bytes());
            } else {
                out.extend_from_slice(&[0, 0]);
                out.extend_from_slice(&(count as u16).to_le_bytes());
                let mut m = mem.lock().unwrap();
                for name in &names {
                    if command == READ {
                        let v = m.get(name).cloned().unwrap_or(vec![0; width]);
                        out.extend_from_slice(&(width as u16).to_le_bytes());
                        out.extend_from_slice(&v);
                    } else {
                        let n = le(at) as usize;
                        m.insert(name.clone(), body[at + 2..at + 2 + n].to_vec());
                        at += 2 + n;
                    }
                }
            }
            let mut answer = Vec::new();
            answer.extend_from_slice(COMPANY);
            answer.extend_from_slice(&[0, 0, 0xa0, PLC]);
            answer.extend_from_slice(&head[14..16]);
            answer.extend_from_slice(&(out.len() as u16).to_le_bytes());
            answer.extend_from_slice(&[0, 0]);
            answer.extend_from_slice(&out);
            s.write_all(&answer).unwrap();
        }
    });
    (address, memory)
}

#[test]
fn variables_read_as_xg5000_writes_them() {
    assert_eq!(parse("%MX100").unwrap().size, Size::Bit);
    assert_eq!(
        parse("%dw100").unwrap(),
        Variable {
            name: "%DW100".into(),
            size: Size::Word
        }
    );
    assert_eq!(parse("%MD20").unwrap().size, Size::Dword);
    assert_eq!(parse("%QX0.0.1").unwrap().size, Size::Bit);
    assert_eq!(parse("%PX0010A").unwrap().size, Size::Bit);
    for bad in [
        "MX100",
        "%MY1",
        "%XW1",
        "%MW",
        "%MW1;2",
        "%MW123456789012345",
    ] {
        assert!(parse(bad).is_err(), "{bad}");
    }
}

#[test]
fn a_frame_s_header_says_who_sends_it_and_sums_itself() {
    let f = frame(0x0102, &[0x54, 0, 2, 0]);
    assert_eq!(&f[..8], b"LSIS-XGT");
    assert_eq!(f[13], 0x33);
    assert_eq!(&f[14..18], &[0x02, 0x01, 4, 0]);
    assert_eq!(f[19], f[..19].iter().fold(0u8, |a, b| a.wrapping_add(*b)));
    assert_eq!(&f[20..], &[0x54, 0, 2, 0]);
}

#[test]
fn bits_words_and_double_words_go_to_a_plc_and_come_back() {
    let (address, memory) = plc();
    let mut c = Client::open(TcpStream::connect(&address).unwrap());
    let v = |t: &str| parse(t).unwrap();
    c.write(&v("%MX100"), 1).unwrap().unwrap();
    c.write(&v("%DW100"), -300).unwrap().unwrap();
    c.write(&v("%MD20"), 70000).unwrap().unwrap();
    assert_eq!(memory.lock().unwrap()["%DW100"], (-300i16).to_le_bytes());
    let got = c
        .read(&[v("%DW100"), v("%MX100"), v("%MX101"), v("%MD20")])
        .unwrap();
    assert_eq!(got, [Ok(-300), Ok(1), Ok(0), Ok(70000)]);
    // A device the PLC has not: said, the link going on.
    let got = c.read(&[v("%ZW0")]).unwrap();
    assert!(got[0].as_ref().unwrap_err().contains("0x0003"));
    assert!(c.write(&v("%ZX0"), 1).unwrap().is_err());
    assert_eq!(c.read(&[v("%MX100")]).unwrap(), [Ok(1)]);
}
