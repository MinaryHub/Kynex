//! An OPC UA client, as small as KYNEX needs: over UA TCP (`opc.tcp`), with
//! no message security (the `None` policy) and an anonymous user, it reads
//! and writes Boolean values by node. That is how a PLC's OPC UA server is
//! usually reached for virtual commissioning on a bench; a signed or
//! encrypted channel is not made.
//!
//! The binary encoding and the services are OPC 10000-6 (Mappings) and
//! OPC 10000-4 (Services): Hello and Acknowledge, OpenSecureChannel,
//! CreateSession and ActivateSession, Read and Write, CloseSession.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{SystemTime, UNIX_EPOCH};

/// The security policy of a channel with none.
const POLICY_NONE: &str = "http://opcfoundation.org/UA/SecurityPolicy#None";

/// The binary encodings' node ids of the services used (namespace 0).
mod id {
    pub const OPEN_REQUEST: u32 = 446;
    pub const OPEN_RESPONSE: u32 = 449;
    pub const CREATE_SESSION_REQUEST: u32 = 461;
    pub const CREATE_SESSION_RESPONSE: u32 = 464;
    pub const ACTIVATE_SESSION_REQUEST: u32 = 467;
    pub const ACTIVATE_SESSION_RESPONSE: u32 = 470;
    pub const CLOSE_SESSION_REQUEST: u32 = 473;
    pub const READ_REQUEST: u32 = 631;
    pub const READ_RESPONSE: u32 = 634;
    pub const WRITE_REQUEST: u32 = 673;
    pub const WRITE_RESPONSE: u32 = 676;
    pub const ANONYMOUS_IDENTITY_TOKEN: u32 = 321;
    pub const SERVICE_FAULT: u32 = 397;
}

/// The Value attribute.
const VALUE: u32 = 13;

/// A node's identity: its namespace, and a number or a name in it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum NodeId {
    Numeric(u16, u32),
    String(u16, String),
}

impl NodeId {
    /// Reads `ns=2;s=Lathe.Open`, `ns=3;i=1001`, `i=85` or `s=Open`, as
    /// OPC UA writes node ids.
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        let (ns, rest) = match text.strip_prefix("ns=") {
            Some(rest) => {
                let (n, rest) = rest
                    .split_once(';')
                    .ok_or_else(|| format!("{text}: ns= 뒤에 ;가 없습니다"))?;
                let n: u16 = n
                    .parse()
                    .map_err(|_| format!("{text}: 네임스페이스 번호가 아닙니다"))?;
                (n, rest)
            }
            None => (0, text),
        };
        if let Some(i) = rest.strip_prefix("i=") {
            let i: u32 = i
                .parse()
                .map_err(|_| format!("{text}: i= 뒤가 숫자가 아닙니다"))?;
            Ok(NodeId::Numeric(ns, i))
        } else if let Some(s) = rest.strip_prefix("s=") {
            if s.is_empty() {
                return Err(format!("{text}: s= 뒤가 비었습니다"));
            }
            Ok(NodeId::String(ns, s.to_string()))
        } else {
            Err(format!(
                "{text}: 노드 ID는 ns=2;s=이름 또는 ns=2;i=번호 꼴입니다"
            ))
        }
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeId::Numeric(ns, i) => write!(f, "ns={ns};i={i}"),
            NodeId::String(ns, s) => write!(f, "ns={ns};s={s}"),
        }
    }
}

// --- encoding -----------------------------------------------------------------

/// A message body as it is built.
#[derive(Default)]
struct Out(Vec<u8>);

impl Out {
    fn u8(&mut self, v: u8) -> &mut Self {
        self.0.push(v);
        self
    }
    fn u16(&mut self, v: u16) -> &mut Self {
        self.0.extend(v.to_le_bytes());
        self
    }
    fn u32(&mut self, v: u32) -> &mut Self {
        self.0.extend(v.to_le_bytes());
        self
    }
    fn i32(&mut self, v: i32) -> &mut Self {
        self.0.extend(v.to_le_bytes());
        self
    }
    fn i64(&mut self, v: i64) -> &mut Self {
        self.0.extend(v.to_le_bytes());
        self
    }
    fn f64(&mut self, v: f64) -> &mut Self {
        self.0.extend(v.to_le_bytes());
        self
    }
    fn string(&mut self, s: Option<&str>) -> &mut Self {
        match s {
            Some(s) => {
                self.i32(s.len() as i32);
                self.0.extend(s.as_bytes());
            }
            None => {
                self.i32(-1);
            }
        }
        self
    }
    fn node(&mut self, n: &NodeId) -> &mut Self {
        match n {
            NodeId::Numeric(0, i) if *i < 256 => self.u8(0x00).u8(*i as u8),
            NodeId::Numeric(ns, i) if *ns < 256 && *i < 65536 => {
                self.u8(0x01).u8(*ns as u8).u16(*i as u16)
            }
            NodeId::Numeric(ns, i) => self.u8(0x02).u16(*ns).u32(*i),
            NodeId::String(ns, s) => self.u8(0x03).u16(*ns).string(Some(s)),
        }
    }
    /// The node id of a type's binary encoding, before a request's body.
    fn type_id(&mut self, i: u32) -> &mut Self {
        self.node(&NodeId::Numeric(0, i))
    }
    /// A request's header: the session's token, a handle, a timeout.
    fn header(&mut self, token: &NodeId, handle: u32) -> &mut Self {
        self.node(token)
            .i64(now())
            .u32(handle)
            .u32(0) // no diagnostics
            .string(None) // audit entry
            .u32(10_000) // timeout hint, ms
            .u8(0x00) // additional header: a null extension object
            .u8(0x00)
            .u8(0x00)
    }
}

/// Now, as OPC UA counts time: 100 ns ticks since 1601.
fn now() -> i64 {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    11_644_473_600 * 10_000_000 + (since.as_nanos() / 100) as i64
}

/// A message body as it is read.
struct In<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> In<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(n).filter(|e| *e <= self.bytes.len());
        let end = end.ok_or("OPC UA: 메시지가 짧습니다")?;
        let out = &self.bytes[self.at..end];
        self.at = end;
        Ok(out)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> Result<i32, String> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn skip(&mut self, n: usize) -> Result<(), String> {
        self.take(n).map(|_| ())
    }
    /// A String or a ByteString.
    fn bytes(&mut self) -> Result<Option<&'a [u8]>, String> {
        let n = self.i32()?;
        if n < 0 {
            return Ok(None);
        }
        self.take(n as usize).map(Some)
    }
    fn string(&mut self) -> Result<Option<String>, String> {
        Ok(self
            .bytes()?
            .map(|b| String::from_utf8_lossy(b).into_owned()))
    }
    fn node(&mut self) -> Result<NodeId, String> {
        let kind = self.u8()?;
        let node = match kind & 0x3F {
            0 => NodeId::Numeric(0, self.u8()? as u32),
            1 => {
                let ns = self.u8()? as u16;
                NodeId::Numeric(ns, self.u16()? as u32)
            }
            2 => {
                let ns = self.u16()?;
                NodeId::Numeric(ns, self.u32()?)
            }
            3 => {
                let ns = self.u16()?;
                NodeId::String(ns, self.string()?.unwrap_or_default())
            }
            // A GUID or opaque bytes: kept as a name nobody gave.
            4 => {
                let ns = self.u16()?;
                self.skip(16)?;
                NodeId::String(ns, "<guid>".into())
            }
            5 => {
                let ns = self.u16()?;
                self.bytes()?;
                NodeId::String(ns, "<opaque>".into())
            }
            k => return Err(format!("OPC UA: 모르는 노드 ID 형식 {k}")),
        };
        // An expanded node id's namespace URI and server index.
        if kind & 0x80 != 0 {
            self.string()?;
        }
        if kind & 0x40 != 0 {
            self.u32()?;
        }
        Ok(node)
    }
    fn localized(&mut self) -> Result<(), String> {
        let mask = self.u8()?;
        if mask & 1 != 0 {
            self.string()?;
        }
        if mask & 2 != 0 {
            self.string()?;
        }
        Ok(())
    }
    fn diagnostic(&mut self) -> Result<(), String> {
        let mask = self.u8()?;
        for bit in [0x01, 0x02, 0x04, 0x08] {
            if mask & bit != 0 {
                self.i32()?;
            }
        }
        if mask & 0x10 != 0 {
            self.string()?;
        }
        if mask & 0x20 != 0 {
            self.u32()?;
        }
        if mask & 0x40 != 0 {
            self.diagnostic()?;
        }
        Ok(())
    }
    fn extension(&mut self) -> Result<(), String> {
        self.node()?;
        match self.u8()? {
            0 => Ok(()),
            _ => self.bytes().map(|_| ()),
        }
    }
    /// An array's length, none being 0.
    fn count(&mut self) -> Result<usize, String> {
        Ok(self.i32()?.max(0) as usize)
    }
    /// A response's header: its service result, which must be good.
    fn header(&mut self) -> Result<(), String> {
        self.skip(8)?; // timestamp
        self.u32()?; // request handle
        let result = self.u32()?;
        self.diagnostic()?;
        for _ in 0..self.count()? {
            self.string()?;
        }
        self.extension()?;
        if result & 0x8000_0000 != 0 {
            return Err(status(result));
        }
        Ok(())
    }
    /// A Variant: a Boolean, or a number read as on when it is not 0.
    fn variant_bool(&mut self) -> Result<Option<bool>, String> {
        let kind = self.u8()?;
        if kind & 0xC0 != 0 {
            return Err("OPC UA: 배열 값은 읽지 않습니다".into());
        }
        Ok(match kind & 0x3F {
            0 => None,
            1 | 2 | 3 => Some(self.u8()? != 0),
            4 | 5 => Some(self.u16()? != 0),
            6 | 7 => Some(self.u32()? != 0),
            8 | 9 => Some(u64::from_le_bytes(self.take(8)?.try_into().unwrap()) != 0),
            10 => Some(f32::from_le_bytes(self.take(4)?.try_into().unwrap()) != 0.0),
            11 => Some(f64::from_le_bytes(self.take(8)?.try_into().unwrap()) != 0.0),
            k => return Err(format!("OPC UA: Boolean으로 읽을 수 없는 값 형식 {k}")),
        })
    }
}

/// A status code as words, for the bad ones met most.
fn status(code: u32) -> String {
    let name = match code {
        0x8034_0000 => "BadNodeIdUnknown",
        0x8033_0000 => "BadNodeIdInvalid",
        0x8074_0000 => "BadTypeMismatch",
        0x803B_0000 => "BadNotWritable",
        0x803A_0000 => "BadNotReadable",
        0x801F_0000 => "BadUserAccessDenied",
        0x8021_0000 => "BadIdentityTokenInvalid",
        0x8025_0000 => "BadSessionIdInvalid",
        0x8026_0000 => "BadSessionClosed",
        0x8013_0000 => "BadSecurityChecksFailed",
        0x8055_0000 => "BadSecurityPolicyRejected",
        0x8035_0000 => "BadAttributeIdInvalid",
        _ => "",
    };
    format!("OPC UA 0x{code:08X} {name}").trim_end().to_string()
}

// --- the connection -------------------------------------------------------------

/// A session on a server: its channel, and the token its requests carry.
pub struct Session {
    stream: TcpStream,
    channel: u32,
    token_id: u32,
    sequence: u32,
    request: u32,
    auth: NodeId,
}

impl Session {
    /// Connects to the server at `stream`'s other end, as `url` names it:
    /// a channel with no security, a session, an anonymous user.
    pub fn open(stream: TcpStream, url: &str) -> Result<Self, String> {
        let mut s = Self {
            stream,
            channel: 0,
            token_id: 0,
            sequence: 0,
            request: 0,
            auth: NodeId::Numeric(0, 0),
        };
        s.hello(url)?;
        s.open_channel()?;
        let policy = s.create_session(url)?;
        s.activate_session(&policy)?;
        Ok(s)
    }

    fn hello(&mut self, url: &str) -> Result<(), String> {
        let mut body = Out::default();
        body.u32(0) // protocol version
            .u32(65_536) // receive buffer
            .u32(65_536) // send buffer
            .u32(0) // no message size limit
            .u32(0) // no chunk count limit
            .string(Some(url));
        self.send(b"HEL", &body.0)?;
        let (kind, _) = self.receive()?;
        match &kind {
            b"ACK" => Ok(()),
            _ => Err(format!(
                "OPC UA: Hello에 {}로 답했습니다",
                String::from_utf8_lossy(&kind)
            )),
        }
    }

    fn open_channel(&mut self) -> Result<(), String> {
        let mut body = Out::default();
        body.u32(0) // no channel yet
            .string(Some(POLICY_NONE))
            .i32(-1) // no certificate
            .i32(-1); // no thumbprint
        self.sequence_header(&mut body);
        body.type_id(id::OPEN_REQUEST);
        let handle = self.next_request();
        body.header(&NodeId::Numeric(0, 0), handle)
            .u32(0) // client protocol version
            .u32(0) // issue
            .u32(1) // security mode None
            .i32(-1) // no nonce
            .u32(3_600_000); // lifetime, ms
        self.send(b"OPN", &body.0)?;
        let (kind, reply) = self.receive()?;
        if &kind != b"OPN" {
            return Err(self.error_of(&kind, &reply, "OpenSecureChannel"));
        }
        let mut r = In::new(&reply);
        self.channel = r.u32()?;
        r.string()?; // policy
        r.bytes()?; // certificate
        r.bytes()?; // thumbprint
        r.skip(8)?; // sequence header
        expect(&mut r, id::OPEN_RESPONSE)?;
        r.header()?;
        r.u32()?; // server protocol version
        r.u32()?; // the token's channel
        self.token_id = r.u32()?;
        Ok(())
    }

    /// Creates a session; returns the policy id of the server's anonymous
    /// user token on a channel with no security.
    fn create_session(&mut self, url: &str) -> Result<String, String> {
        let mut body = Out::default();
        body.type_id(id::CREATE_SESSION_REQUEST);
        let handle = self.next_request();
        body.header(&NodeId::Numeric(0, 0), handle);
        // The client: an application of type client.
        body.string(Some("urn:kynex:client"))
            .string(Some("urn:kynex"))
            .u8(0x02)
            .string(Some("KYNEX"))
            .u32(1)
            .string(None)
            .string(None)
            .i32(-1);
        body.string(None) // server URI
            .string(Some(url))
            .string(Some("KYNEX"))
            .i32(-1) // no nonce
            .i32(-1) // no certificate
            .f64(60_000.0) // session timeout, ms
            .u32(0); // no response size limit
        let reply = self.call(&body.0, id::CREATE_SESSION_RESPONSE)?;
        let mut r = In::new(&reply);
        r.header()?;
        r.node()?; // session id
        self.auth = r.node()?;
        r.skip(8)?; // revised timeout
        r.bytes()?; // nonce
        r.bytes()?; // certificate
                    // The endpoints: the anonymous token's policy on one with no
                    // security.
        let mut policy = None;
        for _ in 0..r.count()? {
            r.string()?; // url
                         // The server's description.
            r.string()?;
            r.string()?;
            r.localized()?;
            r.u32()?;
            r.string()?;
            r.string()?;
            for _ in 0..r.count()? {
                r.string()?;
            }
            r.bytes()?; // certificate
            let mode = r.u32()?;
            let uri = r.string()?.unwrap_or_default();
            for _ in 0..r.count()? {
                let id = r.string()?.unwrap_or_default();
                let kind = r.u32()?;
                r.string()?;
                r.string()?;
                r.string()?;
                if mode == 1 && uri == POLICY_NONE && kind == 0 && policy.is_none() {
                    policy = Some(id);
                }
            }
            r.string()?; // transport profile
            r.u8()?; // security level
        }
        policy.ok_or_else(|| {
            "OPC UA: 서버에 보안 없음(None)·익명 접속 엔드포인트가 없습니다".to_string()
        })
    }

    fn activate_session(&mut self, policy: &str) -> Result<(), String> {
        let mut body = Out::default();
        body.type_id(id::ACTIVATE_SESSION_REQUEST);
        let handle = self.next_request();
        let auth = self.auth.clone();
        body.header(&auth, handle);
        body.string(None).i32(-1); // no signature
        body.i32(-1); // no software certificates
        body.i32(-1); // no locales
                      // An anonymous user, its policy the server's.
        let mut token = Out::default();
        token.string(Some(policy));
        body.type_id(id::ANONYMOUS_IDENTITY_TOKEN)
            .u8(0x01)
            .i32(token.0.len() as i32);
        body.0.extend(token.0);
        body.string(None).i32(-1); // no user signature
        let reply = self.call(&body.0, id::ACTIVATE_SESSION_RESPONSE)?;
        In::new(&reply).header()
    }

    /// The Boolean values of `nodes`, each or why it could not be read.
    pub fn read(&mut self, nodes: &[NodeId]) -> Result<Vec<Result<bool, String>>, String> {
        let mut body = Out::default();
        body.type_id(id::READ_REQUEST);
        let handle = self.next_request();
        let auth = self.auth.clone();
        body.header(&auth, handle)
            .f64(0.0) // max age: the value now
            .u32(3) // no timestamps
            .i32(nodes.len() as i32);
        for n in nodes {
            body.node(n)
                .u32(VALUE)
                .string(None) // index range
                .u16(0)
                .string(None); // data encoding
        }
        let reply = self.call(&body.0, id::READ_RESPONSE)?;
        let mut r = In::new(&reply);
        r.header()?;
        let n = r.count()?;
        let mut out = Vec::with_capacity(n);
        for node in nodes.iter().take(n) {
            let mask = r.u8()?;
            let value = if mask & 0x01 != 0 {
                r.variant_bool()?
            } else {
                None
            };
            let code = if mask & 0x02 != 0 { r.u32()? } else { 0 };
            for (bit, len) in [(0x04, 8), (0x10, 2), (0x08, 8), (0x20, 2)] {
                if mask & bit != 0 {
                    r.skip(len)?;
                }
            }
            out.push(if code & 0x8000_0000 != 0 {
                Err(format!("{node}: {}", status(code)))
            } else {
                value.ok_or_else(|| format!("{node}: 값이 없습니다"))
            });
        }
        if out.len() != nodes.len() {
            return Err("OPC UA: 읽은 값의 수가 맞지 않습니다".into());
        }
        Ok(out)
    }

    /// Writes `on` to `node`'s value, as a Boolean.
    pub fn write(&mut self, node: &NodeId, on: bool) -> Result<(), String> {
        let mut body = Out::default();
        body.type_id(id::WRITE_REQUEST);
        let handle = self.next_request();
        let auth = self.auth.clone();
        body.header(&auth, handle).i32(1);
        body.node(node)
            .u32(VALUE)
            .string(None)
            .u8(0x01) // a data value with a value only
            .u8(1) // Boolean
            .u8(u8::from(on));
        let reply = self.call(&body.0, id::WRITE_RESPONSE)?;
        let mut r = In::new(&reply);
        r.header()?;
        if r.count()? < 1 {
            return Err("OPC UA: 쓰기 결과가 없습니다".into());
        }
        match r.u32()? {
            code if code & 0x8000_0000 != 0 => Err(format!("{node}: {}", status(code))),
            _ => Ok(()),
        }
    }

    /// Closes the session, and the channel with it.
    pub fn close(mut self) {
        let mut body = Out::default();
        body.type_id(id::CLOSE_SESSION_REQUEST);
        let handle = self.next_request();
        let auth = self.auth.clone();
        body.header(&auth, handle).u8(1); // delete subscriptions
        let _ = self.call(&body.0, 0);
        let mut clo = Out::default();
        clo.u32(self.channel).u32(self.token_id);
        self.sequence_header(&mut clo);
        let _ = self.send(b"CLO", &clo.0);
    }

    // --- messages ---

    fn next_request(&mut self) -> u32 {
        self.request += 1;
        self.request
    }

    fn sequence_header(&mut self, out: &mut Out) {
        self.sequence += 1;
        out.u32(self.sequence).u32(self.request + 1);
    }

    /// A service request on the channel; its response's body after the
    /// response type, which must be `want` (or anything, for 0).
    fn call(&mut self, request: &[u8], want: u32) -> Result<Vec<u8>, String> {
        let mut body = Out::default();
        body.u32(self.channel).u32(self.token_id);
        self.sequence += 1;
        body.u32(self.sequence).u32(self.request);
        body.0.extend(request);
        self.send(b"MSG", &body.0)?;
        let (kind, reply) = self.receive()?;
        if &kind != b"MSG" {
            return Err(self.error_of(&kind, &reply, "요청"));
        }
        let mut r = In::new(&reply);
        r.skip(4 + 4 + 8)?; // channel, token, sequence header
        match r.node()? {
            NodeId::Numeric(0, i) if i == want || want == 0 => {}
            NodeId::Numeric(0, id::SERVICE_FAULT) => {
                r.header()?;
                return Err("OPC UA: ServiceFault".into());
            }
            other => return Err(format!("OPC UA: 뜻밖의 응답 {other}")),
        }
        Ok(reply[r.at..].to_vec())
    }

    fn error_of(&self, kind: &[u8; 3], reply: &[u8], what: &str) -> String {
        if kind == b"ERR" {
            let mut r = In::new(reply);
            let code = r.u32().unwrap_or(0);
            let reason = r.string().ok().flatten().unwrap_or_default();
            format!("OPC UA {what}: {} {reason}", status(code))
                .trim_end()
                .to_string()
        } else {
            format!(
                "OPC UA {what}: {}로 답했습니다",
                String::from_utf8_lossy(kind)
            )
        }
    }

    /// Sends a whole message of type `kind` in one chunk.
    fn send(&mut self, kind: &[u8; 3], body: &[u8]) -> Result<(), String> {
        let mut out = Vec::with_capacity(8 + body.len());
        out.extend(kind);
        out.push(b'F');
        out.extend(((8 + body.len()) as u32).to_le_bytes());
        out.extend(body);
        self.stream.write_all(&out).map_err(|e| e.to_string())
    }

    /// A whole message: its type and its body, the chunks of a long one
    /// joined (each chunk after the first without its channel and
    /// sequence headers).
    fn receive(&mut self) -> Result<([u8; 3], Vec<u8>), String> {
        let mut body: Vec<u8> = Vec::new();
        loop {
            let mut head = [0u8; 8];
            self.stream
                .read_exact(&mut head)
                .map_err(|e| e.to_string())?;
            let kind = [head[0], head[1], head[2]];
            let size = u32::from_le_bytes(head[4..8].try_into().unwrap()) as usize;
            if !(8..=16 * 1024 * 1024).contains(&size) {
                return Err(format!("OPC UA: 메시지 크기가 맞지 않습니다: {size}"));
            }
            let mut chunk = vec![0u8; size - 8];
            self.stream
                .read_exact(&mut chunk)
                .map_err(|e| e.to_string())?;
            if body.is_empty() {
                body = chunk;
            } else {
                // Channel, token and sequence header: 16 bytes.
                body.extend_from_slice(chunk.get(16..).unwrap_or_default());
            }
            match head[3] {
                b'F' => return Ok((kind, body)),
                b'C' => continue,
                _ => return Err("OPC UA: 메시지가 중간에 취소됐습니다".into()),
            }
        }
    }
}

/// The response type at the start of `r`, which must be `want`.
fn expect(r: &mut In, want: u32) -> Result<(), String> {
    match r.node()? {
        NodeId::Numeric(0, i) if i == want => Ok(()),
        other => Err(format!("OPC UA: 뜻밖의 응답 {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_ids_read_as_opc_ua_writes_them() {
        assert_eq!(
            NodeId::parse("ns=2;s=Lathe.Open").unwrap(),
            NodeId::String(2, "Lathe.Open".into())
        );
        assert_eq!(NodeId::parse("i=85").unwrap(), NodeId::Numeric(0, 85));
        assert_eq!(
            NodeId::parse("ns=3;i=70000").unwrap(),
            NodeId::Numeric(3, 70000)
        );
        // Siemens's names, quotes and dots and all.
        assert_eq!(
            NodeId::parse("ns=3;s=\"DB_Cell\".\"open\"").unwrap(),
            NodeId::String(3, "\"DB_Cell\".\"open\"".into())
        );
        assert!(NodeId::parse("ns=x;i=1").is_err());
        assert!(NodeId::parse("100").is_err());
        assert_eq!(NodeId::parse("ns=2;s=A").unwrap().to_string(), "ns=2;s=A");
    }

    /// Against a server on 127.0.0.1:4842/kynex/ with Boolean variables
    /// `ns=2;s=Lathe.open` and `Lathe.door_open`: read, write, read back.
    #[test]
    #[ignore = "needs an OPC UA server on port 4842"]
    fn a_server_is_read_and_written() {
        let stream = std::net::TcpStream::connect("127.0.0.1:4842").unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let t = std::time::Instant::now();
        let mut s = Session::open(stream, "opc.tcp://127.0.0.1:4842/kynex/").unwrap();
        println!("open {:?}", t.elapsed());
        let nodes = [
            NodeId::parse("ns=2;s=Lathe.open").unwrap(),
            NodeId::parse("ns=2;s=Lathe.door_open").unwrap(),
        ];
        for k in 0..3 {
            let t = std::time::Instant::now();
            let got = s.read(&nodes).unwrap();
            println!("read {k}: {got:?} {:?}", t.elapsed());
            let t = std::time::Instant::now();
            s.write(&nodes[1], k % 2 == 0).unwrap();
            println!("write {k}: {:?}", t.elapsed());
        }
        s.close();
    }

    /// Node ids in their shortest binary form, and read back.
    #[test]
    fn node_ids_encode_in_their_shortest_form() {
        for (node, bytes) in [
            (NodeId::Numeric(0, 85), vec![0x00, 85]),
            (NodeId::Numeric(2, 1001), vec![0x01, 2, 0xE9, 0x03]),
            (
                NodeId::Numeric(300, 70000),
                vec![0x02, 0x2C, 0x01, 0x70, 0x11, 0x01, 0x00],
            ),
            (
                NodeId::String(2, "Ab".into()),
                vec![0x03, 2, 0, 2, 0, 0, 0, b'A', b'b'],
            ),
        ] {
            let mut out = Out::default();
            out.node(&node);
            assert_eq!(out.0, bytes, "{node}");
            assert_eq!(In::new(&bytes).node().unwrap(), node);
        }
    }
}
