//! Siemens S7 over ISO-on-TCP (port 102): S7-300/400 and, with PUT/GET
//! allowed and blocks not optimised, S7-1200/1500 — what the S7
//! communication of a CPU's own Ethernet port speaks.
//!
//! A connection is made in three steps, each a TPKT (RFC 1006: version 3,
//! a length) around COTP (ISO 8073): a connection request to the CPU's
//! rack and slot, the S7 "setup communication" that agrees a PDU size, and
//! then jobs: read variables (function 0x04) and write variables (0x05).
//!
//! Addresses as TIA Portal and STEP 7 write them, German or English:
//! `DB1.DBX0.0` (a bit of a data block), `DB1.DBB2`, `DB1.DBW2` (a word,
//! signed), `DB1.DBD4` (a double word, signed); `M10.0`, `MB10`, `MW10`,
//! `MD10`; inputs `I0.0` (`E0.0`), `IW2`; outputs `Q0.0` (`A0.0`), `QW2`.

use std::io::{Read, Write};
use std::net::TcpStream;

/// An area of the CPU's memory, as S7 numbers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Area {
    Input = 0x81,
    Output = 0x82,
    Marker = 0x83,
    Db = 0x84,
}

/// How much an address holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Width {
    Bit(u8),
    Byte,
    Word,
    Dword,
}

impl Width {
    fn bytes(self) -> usize {
        match self {
            Width::Bit(_) | Width::Byte => 1,
            Width::Word => 2,
            Width::Dword => 4,
        }
    }

    /// The transport size a request item says: bit, byte, word, dword.
    fn code(self) -> u8 {
        match self {
            Width::Bit(_) => 0x01,
            Width::Byte => 0x02,
            Width::Word => 0x04,
            Width::Dword => 0x06,
        }
    }
}

/// Where a value is in the CPU.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Address {
    pub area: Area,
    /// The data block's number; 0 outside one.
    pub db: u16,
    pub byte: u32,
    pub width: Width,
}

/// An address as written: `DB1.DBX0.0`, `MW10`, `Q0.3` and the like.
pub fn parse(text: &str) -> Result<Address, String> {
    let bad =
        || format!("{text}: S7 주소가 아닙니다 (예: DB1.DBX0.0, DB1.DBW2, M10.0, MW10, Q0.0)");
    let t = text.trim().to_ascii_uppercase();
    // A data block's: `DB<n>.DB<X|B|W|D><byte>[.<bit>]`.
    let (area, db, rest) = if let Some(rest) = t.strip_prefix("DB") {
        let (n, rest) = rest.split_once('.').ok_or_else(bad)?;
        let db: u16 = n.parse().map_err(|_| bad())?;
        let rest = rest.strip_prefix("DB").ok_or_else(bad)?;
        (Area::Db, db, rest.to_string())
    } else {
        let mut chars = t.chars();
        let area = match chars.next() {
            Some('M') => Area::Marker,
            Some('I' | 'E') => Area::Input,
            Some('Q' | 'A') => Area::Output,
            _ => return Err(bad()),
        };
        (area, 0, chars.as_str().to_string())
    };
    // `X` (or none, outside a block) and a bit; or `B`, `W`, `D`.
    let (kind, number) = match rest.chars().next() {
        Some(c @ ('X' | 'B' | 'W' | 'D')) => (c, &rest[1..]),
        Some(c) if c.is_ascii_digit() && area != Area::Db => ('X', rest.as_str()),
        _ => return Err(bad()),
    };
    let width;
    let byte: u32;
    if kind == 'X' {
        let (b, bit) = number.split_once('.').ok_or_else(bad)?;
        byte = b.parse().map_err(|_| bad())?;
        let bit: u8 = bit.parse().map_err(|_| bad())?;
        if bit > 7 {
            return Err(format!("{text}: 비트 번호는 0–7입니다"));
        }
        width = Width::Bit(bit);
    } else {
        byte = number.parse().map_err(|_| bad())?;
        width = match kind {
            'B' => Width::Byte,
            'W' => Width::Word,
            _ => Width::Dword,
        };
    }
    if byte > 0xffff {
        return Err(bad());
    }
    Ok(Address {
        area,
        db,
        byte,
        width,
    })
}

/// A request item for `a`: one of it, at its bit address.
fn item(a: &Address) -> [u8; 12] {
    let bit = match a.width {
        Width::Bit(b) => b as u32,
        _ => 0,
    };
    let at = a.byte * 8 + bit;
    [
        0x12,
        0x0a,
        0x10,
        a.width.code(),
        0,
        1,
        (a.db >> 8) as u8,
        a.db as u8,
        a.area as u8,
        (at >> 16) as u8,
        (at >> 8) as u8,
        at as u8,
    ]
}

/// What a CPU's return code means.
fn refused(code: u8) -> String {
    match code {
        0x03 => "접근 거부 (S7-1200/1500: PUT/GET 허용, 최적화되지 않은 블록이어야 함)".into(),
        0x05 => "주소가 범위 밖입니다".into(),
        0x06 => "데이터 형식이 맞지 않습니다".into(),
        0x0a => "없는 데이터 블록입니다".into(),
        n => format!("S7 오류 0x{n:02X}"),
    }
}

/// A connection to a CPU, its PDU size agreed.
pub struct Client {
    stream: TcpStream,
    pdu: usize,
    reference: u16,
}

impl Client {
    /// Connects over `stream` (to port 102) to the CPU in `rack` and
    /// `slot`: 0 and 1 for an S7-1200/1500, 0 and 2 for an S7-300.
    pub fn open(mut stream: TcpStream, rack: u8, slot: u8) -> Result<Self, String> {
        // COTP connection request: our TSAP 01 00, theirs a PG's (01) to
        // the rack and slot; TPDUs of up to 1024 bytes.
        let cr = [
            0x11,
            0xe0,
            0,
            0,
            0,
            1,
            0,
            0xc0,
            1,
            0x0a,
            0xc1,
            2,
            1,
            0,
            0xc2,
            2,
            1,
            rack * 0x20 + slot,
        ];
        send_tpkt(&mut stream, &cr)?;
        let cc = receive_tpkt(&mut stream)?;
        if cc.get(1) != Some(&0xd0) {
            return Err("S7: CPU가 연결을 받지 않았습니다 (랙·슬롯을 확인하세요)".into());
        }
        let mut client = Self {
            stream,
            pdu: 240,
            reference: 0,
        };
        // Setup communication: one job at a time each way, a PDU of 960.
        let answer = client.job(&[0xf0, 0, 0, 1, 0, 1, 0x03, 0xc0], &[])?;
        let size = answer
            .param
            .get(6..8)
            .map(|b| u16::from_be_bytes([b[0], b[1]]) as usize)
            .ok_or("S7: setup communication 응답이 짧습니다")?;
        client.pdu = size.clamp(240, 960);
        Ok(client)
    }

    /// Sends a job (`param`, `data`) and returns the CPU's answer.
    fn job(&mut self, param: &[u8], data: &[u8]) -> Result<Answer, String> {
        self.reference = self.reference.wrapping_add(1);
        let mut out = vec![0x02, 0xf0, 0x80, 0x32, 0x01, 0, 0];
        out.extend_from_slice(&self.reference.to_be_bytes());
        out.extend_from_slice(&(param.len() as u16).to_be_bytes());
        out.extend_from_slice(&(data.len() as u16).to_be_bytes());
        out.extend_from_slice(param);
        out.extend_from_slice(data);
        send_tpkt(&mut self.stream, &out)?;
        let tpdu = receive_tpkt(&mut self.stream)?;
        // COTP data (3 bytes), then S7: 0x32, ack-data 3 has a 12-byte
        // header with an error class and code.
        let s7 = tpdu.get(3..).ok_or("S7: 응답이 짧습니다")?;
        if s7.first() != Some(&0x32) || s7.len() < 12 {
            return Err("S7: S7 응답이 아닙니다".into());
        }
        let (class, code) = (s7[10], s7[11]);
        if class != 0 || code != 0 {
            return Err(format!("S7: 요청 거절 (오류 {class:02X}{code:02X})"));
        }
        let pl = u16::from_be_bytes([s7[6], s7[7]]) as usize;
        let dl = u16::from_be_bytes([s7[8], s7[9]]) as usize;
        let param = s7.get(12..12 + pl).ok_or("S7: 응답이 짧습니다")?.to_vec();
        let data = s7
            .get(12 + pl..12 + pl + dl)
            .ok_or("S7: 응답이 짧습니다")?
            .to_vec();
        Ok(Answer { param, data })
    }

    /// The values at `addresses`, each a signed number (a bit 0 or 1), or
    /// why the CPU would not give it. An error is the connection's.
    pub fn read(&mut self, addresses: &[Address]) -> Result<Vec<Result<i64, String>>, String> {
        let mut out = Vec::with_capacity(addresses.len());
        // As many as a PDU takes each way: 12 bytes an item asked, 4 and
        // up to 4 more an item answered.
        let most = ((self.pdu - 19) / 12).min((self.pdu - 14) / 8).max(1);
        for chunk in addresses.chunks(most) {
            let mut param = vec![0x04, chunk.len() as u8];
            for a in chunk {
                param.extend_from_slice(&item(a));
            }
            let answer = self.job(&param, &[])?;
            let mut d = &answer.data[..];
            for (i, a) in chunk.iter().enumerate() {
                if d.len() < 4 {
                    return Err("S7: 읽기 응답이 짧습니다".into());
                }
                let code = d[0];
                let n = a.width.bytes();
                if code != 0xff {
                    out.push(Err(refused(code)));
                    d = &d[4.min(d.len())..];
                    continue;
                }
                let value = d.get(4..4 + n).ok_or("S7: 읽기 응답이 짧습니다")?;
                out.push(Ok(match a.width {
                    Width::Bit(_) => i64::from(value[0] & 1),
                    Width::Byte => i64::from(value[0]),
                    Width::Word => i64::from(i16::from_be_bytes([value[0], value[1]])),
                    Width::Dword => {
                        i64::from(i32::from_be_bytes([value[0], value[1], value[2], value[3]]))
                    }
                }));
                // Each item's data padded to an even length, but the last.
                let mut step = 4 + n;
                if n % 2 == 1 && i + 1 < chunk.len() {
                    step += 1;
                }
                d = d.get(step..).unwrap_or_default();
            }
        }
        Ok(out)
    }

    /// Writes `value` to `a`: `Ok(Err(why))` when the CPU would not take
    /// it, an error when the connection failed.
    pub fn write(&mut self, a: &Address, value: i64) -> Result<Result<(), String>, String> {
        let mut param = vec![0x05, 1];
        param.extend_from_slice(&item(a));
        let bytes: Vec<u8> = match a.width {
            Width::Bit(_) => vec![u8::from(value != 0)],
            Width::Byte => vec![value as u8],
            Width::Word => (value as i16).to_be_bytes().to_vec(),
            Width::Dword => (value as i32).to_be_bytes().to_vec(),
        };
        // A bit's length in bits (1) as a bit; the rest in bits as bytes.
        let (size, length) = match a.width {
            Width::Bit(_) => (0x03, 1u16),
            _ => (0x04, (bytes.len() * 8) as u16),
        };
        let mut data = vec![0x00, size];
        data.extend_from_slice(&length.to_be_bytes());
        data.extend_from_slice(&bytes);
        let answer = self.job(&param, &data)?;
        match answer.data.first() {
            Some(0xff) => Ok(Ok(())),
            Some(code) => Ok(Err(refused(*code))),
            None => Err("S7: 쓰기 응답이 짧습니다".into()),
        }
    }
}

struct Answer {
    param: Vec<u8>,
    data: Vec<u8>,
}

fn send_tpkt(stream: &mut impl Write, body: &[u8]) -> Result<(), String> {
    let n = (body.len() + 4) as u16;
    let mut out = vec![3, 0];
    out.extend_from_slice(&n.to_be_bytes());
    out.extend_from_slice(body);
    stream.write_all(&out).map_err(|e| format!("S7: {e}"))
}

fn receive_tpkt(stream: &mut impl Read) -> Result<Vec<u8>, String> {
    let mut head = [0u8; 4];
    stream
        .read_exact(&mut head)
        .map_err(|e| format!("S7: {e}"))?;
    if head[0] != 3 {
        return Err("S7: TPKT가 아닙니다".into());
    }
    let n = u16::from_be_bytes([head[2], head[3]]) as usize;
    if n < 4 {
        return Err("S7: TPKT 길이가 잘못되었습니다".into());
    }
    let mut body = vec![0u8; n - 4];
    stream
        .read_exact(&mut body)
        .map_err(|e| format!("S7: {e}"))?;
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_read_as_step_7_writes_them() {
        let a = parse("DB1.DBX2.3").unwrap();
        assert_eq!(
            (a.area, a.db, a.byte, a.width),
            (Area::Db, 1, 2, Width::Bit(3))
        );
        assert_eq!(item(&a), [0x12, 0x0a, 0x10, 1, 0, 1, 0, 1, 0x84, 0, 0, 19]);
        assert_eq!(parse("db10.dbw4").unwrap().width, Width::Word);
        assert_eq!(parse("DB10.DBD8").unwrap().width, Width::Dword);
        let m = parse("M10.1").unwrap();
        assert_eq!((m.area, m.byte, m.width), (Area::Marker, 10, Width::Bit(1)));
        assert_eq!(parse("MW20").unwrap().width, Width::Word);
        assert_eq!(parse("E0.0").unwrap().area, Area::Input);
        assert_eq!(parse("A1.7").unwrap().area, Area::Output);
        assert_eq!(parse("QB3").unwrap().width, Width::Byte);
        for bad in [
            "DB1.DBX0.8",
            "DB1.X0.0",
            "Z0.0",
            "M10",
            "DB.DBW0",
            "100",
            "",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }

    /// A CPU of the user's own making, with data block 1 of 32 bytes: a
    /// bit written changes that bit alone, as a CPU's does. Another block
    /// is not there.
    fn cpu() -> (String, std::sync::Arc<std::sync::Mutex<[u8; 32]>>) {
        use std::sync::{Arc, Mutex};
        let db = Arc::new(Mutex::new([0u8; 32]));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let mem = db.clone();
        std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let cr = receive_tpkt(&mut s).unwrap();
            assert_eq!(cr[1], 0xe0);
            assert_eq!(cr[17], 1, "rack 0, slot 1");
            send_tpkt(&mut s, &[6, 0xd0, 0, 1, 0, 1, 0]).unwrap();
            loop {
                let Ok(t) = receive_tpkt(&mut s) else { return };
                let s7 = &t[3..];
                let pl = u16::from_be_bytes([s7[6], s7[7]]) as usize;
                let param = &s7[10..10 + pl];
                let data = &s7[10 + pl..];
                let (rparam, rdata): (Vec<u8>, Vec<u8>) = match param[0] {
                    0xf0 => (vec![0xf0, 0, 0, 1, 0, 1, 0x01, 0xe0], vec![]),
                    0x04 => {
                        let mut out = Vec::new();
                        let m = mem.lock().unwrap();
                        let n = param[1] as usize;
                        for k in 0..n {
                            let it = &param[2 + 12 * k..14 + 12 * k];
                            let db = u16::from_be_bytes([it[6], it[7]]);
                            let at = u32::from_be_bytes([0, it[9], it[10], it[11]]) as usize;
                            if db != 1 {
                                out.extend_from_slice(&[0x0a, 0, 0, 0]);
                                continue;
                            }
                            let (size, bytes) = match it[3] {
                                1 => (0x03, vec![(m[at / 8] >> (at % 8)) & 1]),
                                2 => (0x04, m[at / 8..at / 8 + 1].to_vec()),
                                4 => (0x04, m[at / 8..at / 8 + 2].to_vec()),
                                _ => (0x04, m[at / 8..at / 8 + 4].to_vec()),
                            };
                            let bits = if size == 3 { 1 } else { bytes.len() * 8 } as u16;
                            out.extend_from_slice(&[0xff, size]);
                            out.extend_from_slice(&bits.to_be_bytes());
                            out.extend_from_slice(&bytes);
                            if bytes.len() % 2 == 1 && k + 1 < n {
                                out.push(0);
                            }
                        }
                        (vec![0x04, n as u8], out)
                    }
                    _ => {
                        let it = &param[2..14];
                        let db = u16::from_be_bytes([it[6], it[7]]);
                        let at = u32::from_be_bytes([0, it[9], it[10], it[11]]) as usize;
                        let mut m = mem.lock().unwrap();
                        let code = if db != 1 {
                            0x0a
                        } else if it[3] == 1 {
                            let bit = 1 << (at % 8);
                            if data[4] & 1 == 1 {
                                m[at / 8] |= bit;
                            } else {
                                m[at / 8] &= !bit;
                            }
                            0xff
                        } else {
                            let n = (u16::from_be_bytes([data[2], data[3]]) / 8) as usize;
                            m[at / 8..at / 8 + n].copy_from_slice(&data[4..4 + n]);
                            0xff
                        };
                        (vec![0x05, 1], vec![code])
                    }
                };
                let mut out = vec![0x02, 0xf0, 0x80, 0x32, 0x03, 0, 0, s7[4], s7[5]];
                out.extend_from_slice(&(rparam.len() as u16).to_be_bytes());
                out.extend_from_slice(&(rdata.len() as u16).to_be_bytes());
                out.extend_from_slice(&[0, 0]);
                out.extend_from_slice(&rparam);
                out.extend_from_slice(&rdata);
                send_tpkt(&mut s, &out).unwrap();
            }
        });
        (address, db)
    }

    #[test]
    fn bits_and_words_go_to_a_cpu_and_come_back() {
        let (address, db) = cpu();
        let stream = TcpStream::connect(&address).unwrap();
        let mut c = Client::open(stream, 0, 1).unwrap();
        assert_eq!(c.pdu, 480);
        let at = |t: &str| parse(t).unwrap();
        // Two bits of one byte, each alone.
        c.write(&at("DB1.DBX2.1"), 1).unwrap().unwrap();
        c.write(&at("DB1.DBX2.3"), 1).unwrap().unwrap();
        c.write(&at("DB1.DBX2.1"), 0).unwrap().unwrap();
        assert_eq!(db.lock().unwrap()[2], 0b1000);
        c.write(&at("DB1.DBW4"), -2).unwrap().unwrap();
        c.write(&at("DB1.DBD8"), 70000).unwrap().unwrap();
        c.write(&at("DB1.DBB1"), 7).unwrap().unwrap();
        let got = c
            .read(&[
                at("DB1.DBX2.3"),
                at("DB1.DBX2.1"),
                at("DB1.DBB1"),
                at("DB1.DBW4"),
                at("DB1.DBD8"),
                at("DB2.DBX0.0"),
            ])
            .unwrap();
        assert_eq!(&got[..5], &[Ok(1), Ok(0), Ok(7), Ok(-2), Ok(70000)]);
        assert!(got[5].as_ref().unwrap_err().contains("데이터 블록"));
        assert!(c.write(&at("DB2.DBW0"), 1).unwrap().is_err());
    }
}
