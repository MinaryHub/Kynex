//! LS ELECTRIC (LSIS) XGT PLCs over their Ethernet modules' dedicated
//! protocol (FEnet, the "XGT 전용 프로토콜", TCP port 2004): XGK, XGI, XGR,
//! and XGB with built-in Ethernet.
//!
//! Each request and answer is a 20-byte header — the company id
//! `LSIS-XGT`, who sends it (0x33 a client, 0x11 the PLC), an invoke id,
//! the length of what follows, and a check sum of the header — then an
//! instruction, little-endian: read (0x54) or write (0x58) individual
//! variables of one data type, each by its name as XG5000 writes it.
//!
//! Addresses are direct variables, `%` then the device, the size and the
//! address: `%MX100` (a bit), `%MB10`, `%MW10`, `%DW100`, `%MD20`, `%PX0`,
//! `%QX0.0.0` on XGI. The size letter says what is read or written: X a
//! bit, B a byte, W a word (signed), D a double word (signed).

use std::io::{Read, Write};
use std::net::TcpStream;

const COMPANY: &[u8; 10] = b"LSIS-XGT\0\0";
const CLIENT: u8 = 0x33;
const PLC: u8 = 0x11;
const READ: u16 = 0x54;
const WRITE: u16 = 0x58;

/// How much a variable holds, and its data type as the protocol numbers
/// it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    Bit,
    Byte,
    Word,
    Dword,
}

impl Size {
    fn code(self) -> u16 {
        match self {
            Size::Bit => 0,
            Size::Byte => 1,
            Size::Word => 2,
            Size::Dword => 3,
        }
    }

    fn bytes(self) -> usize {
        match self {
            Size::Bit | Size::Byte => 1,
            Size::Word => 2,
            Size::Dword => 4,
        }
    }
}

/// A direct variable: its name as sent, and its size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Variable {
    pub name: String,
    pub size: Size,
}

/// A variable as written: `%MX100`, `%DW100` and the like.
pub fn parse(text: &str) -> Result<Variable, String> {
    let bad = || format!("{text}: LS PLC 주소가 아닙니다 (예: %MX100, %MW10, %DW100)");
    let t = text.trim().to_ascii_uppercase();
    let rest = t.strip_prefix('%').ok_or_else(bad)?;
    let mut chars = rest.chars();
    // The device: P, M, K, F, T, C, L, N, D, R, U, Z, I, Q, W.
    let device = chars.next().ok_or_else(bad)?;
    if !"PMKFTCLNDRUZIQWS".contains(device) {
        return Err(bad());
    }
    let size = match chars.next() {
        Some('X') => Size::Bit,
        Some('B') => Size::Byte,
        Some('W') => Size::Word,
        Some('D') => Size::Dword,
        _ => return Err(bad()),
    };
    let address = chars.as_str();
    // Decimal, hex for XGK's bit within a word, dots for XGI's I/O.
    let ok = !address.is_empty()
        && address.len() <= 12
        && address.chars().all(|c| c.is_ascii_hexdigit() || c == '.');
    if !ok || t.len() > 16 {
        return Err(bad());
    }
    Ok(Variable { name: t, size })
}

/// A frame: the header for `instruction`, then it.
fn frame(invoke: u16, instruction: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(20 + instruction.len());
    out.extend_from_slice(COMPANY);
    out.extend_from_slice(&[0, 0]); // PLC info: the client's, none
    out.push(0); // CPU info
    out.push(CLIENT);
    out.extend_from_slice(&invoke.to_le_bytes());
    out.extend_from_slice(&(instruction.len() as u16).to_le_bytes());
    out.push(0); // the Ethernet module's base and slot: this one
    let sum = out.iter().fold(0u8, |s, b| s.wrapping_add(*b));
    out.push(sum);
    out.extend_from_slice(instruction);
    out
}

/// A connection to a PLC's Ethernet module.
pub struct Client {
    stream: TcpStream,
    invoke: u16,
}

impl Client {
    pub fn open(stream: TcpStream) -> Self {
        Self { stream, invoke: 0 }
    }

    /// Sends `instruction`, and returns the answer's, its command checked.
    fn ask(&mut self, instruction: &[u8], command: u16) -> Result<Vec<u8>, String> {
        self.invoke = self.invoke.wrapping_add(1);
        self.stream
            .write_all(&frame(self.invoke, instruction))
            .map_err(|e| format!("XGT: {e}"))?;
        let mut head = [0u8; 20];
        self.stream
            .read_exact(&mut head)
            .map_err(|e| format!("XGT: {e}"))?;
        if &head[..8] != b"LSIS-XGT" || head[13] != PLC {
            return Err("XGT: LS PLC의 응답이 아닙니다".into());
        }
        let n = u16::from_le_bytes([head[16], head[17]]) as usize;
        let mut body = vec![0u8; n];
        self.stream
            .read_exact(&mut body)
            .map_err(|e| format!("XGT: {e}"))?;
        if u16::from_le_bytes([head[14], head[15]]) != self.invoke {
            return Err("XGT: 다른 요청의 응답입니다".into());
        }
        if body.len() < 10 || u16::from_le_bytes([body[0], body[1]]) != command + 1 {
            return Err("XGT: 응답 명령이 맞지 않습니다".into());
        }
        Ok(body)
    }

    /// The values of `vars`, each a signed number (a bit 0 or 1), or why
    /// the PLC would not give them. Variables of one size are asked
    /// together, sixteen at a time, as the protocol allows.
    pub fn read(&mut self, vars: &[Variable]) -> Result<Vec<Result<i64, String>>, String> {
        let mut out: Vec<Result<i64, String>> = vec![Err(String::new()); vars.len()];
        for size in [Size::Bit, Size::Byte, Size::Word, Size::Dword] {
            let these: Vec<usize> = (0..vars.len()).filter(|i| vars[*i].size == size).collect();
            for chunk in these.chunks(16) {
                let mut ins = Vec::new();
                ins.extend_from_slice(&READ.to_le_bytes());
                ins.extend_from_slice(&size.code().to_le_bytes());
                ins.extend_from_slice(&[0, 0]);
                ins.extend_from_slice(&(chunk.len() as u16).to_le_bytes());
                for i in chunk {
                    let name = vars[*i].name.as_bytes();
                    ins.extend_from_slice(&(name.len() as u16).to_le_bytes());
                    ins.extend_from_slice(name);
                }
                let body = self.ask(&ins, READ)?;
                let status = u16::from_le_bytes([body[6], body[7]]);
                if status != 0 {
                    // The whole request refused: each of its variables.
                    let code = u16::from_le_bytes([body[8], body[9]]);
                    for i in chunk {
                        out[*i] = Err(refused(code));
                    }
                    continue;
                }
                let mut d = &body[10..];
                for i in chunk {
                    if d.len() < 2 {
                        return Err("XGT: 읽기 응답이 짧습니다".into());
                    }
                    let n = u16::from_le_bytes([d[0], d[1]]) as usize;
                    let v = d.get(2..2 + n).ok_or("XGT: 읽기 응답이 짧습니다")?;
                    if n < size.bytes() {
                        return Err("XGT: 읽기 응답이 짧습니다".into());
                    }
                    out[*i] = Ok(match size {
                        Size::Bit => i64::from(v[0] & 1),
                        Size::Byte => i64::from(v[0]),
                        Size::Word => i64::from(i16::from_le_bytes([v[0], v[1]])),
                        Size::Dword => i64::from(i32::from_le_bytes([v[0], v[1], v[2], v[3]])),
                    });
                    d = &d[2 + n..];
                }
            }
        }
        Ok(out)
    }

    /// Writes `value` to `var`: `Ok(Err(why))` when the PLC would not take
    /// it, an error when the connection failed.
    pub fn write(&mut self, var: &Variable, value: i64) -> Result<Result<(), String>, String> {
        let data: Vec<u8> = match var.size {
            Size::Bit => vec![u8::from(value != 0)],
            Size::Byte => vec![value as u8],
            Size::Word => (value as i16).to_le_bytes().to_vec(),
            Size::Dword => (value as i32).to_le_bytes().to_vec(),
        };
        let mut ins = Vec::new();
        ins.extend_from_slice(&WRITE.to_le_bytes());
        ins.extend_from_slice(&var.size.code().to_le_bytes());
        ins.extend_from_slice(&[0, 0]);
        ins.extend_from_slice(&1u16.to_le_bytes());
        let name = var.name.as_bytes();
        ins.extend_from_slice(&(name.len() as u16).to_le_bytes());
        ins.extend_from_slice(name);
        ins.extend_from_slice(&(data.len() as u16).to_le_bytes());
        ins.extend_from_slice(&data);
        let body = self.ask(&ins, WRITE)?;
        let status = u16::from_le_bytes([body[6], body[7]]);
        if status == 0 {
            Ok(Ok(()))
        } else {
            Ok(Err(refused(u16::from_le_bytes([body[8], body[9]]))))
        }
    }
}

/// The PLC's error code, as its Ethernet manual lists them.
fn refused(code: u16) -> String {
    format!("PLC가 거절했습니다 (오류 코드 0x{code:04X}: FEnet 매뉴얼의 오류 코드 표 참고)")
}

#[cfg(test)]
mod tests;
