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

use super::secure::{
    Identity, Keys, Peer, APPLICATION_URI, BASIC256SHA256, PASSWORD_ALGORITHM, SIGNATURE_ALGORITHM,
};

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
    pub const CLOSE_CHANNEL_REQUEST: u32 = 452;
    pub const GET_ENDPOINTS_REQUEST: u32 = 428;
    pub const GET_ENDPOINTS_RESPONSE: u32 = 431;
    pub const USER_NAME_IDENTITY_TOKEN: u32 = 324;
    pub const TRANSLATE_REQUEST: u32 = 554;
    pub const TRANSLATE_RESPONSE: u32 = 557;
    pub const READ_REQUEST: u32 = 631;
    pub const READ_RESPONSE: u32 = 634;
    pub const WRITE_REQUEST: u32 = 673;
    pub const WRITE_RESPONSE: u32 = 676;
    pub const ANONYMOUS_IDENTITY_TOKEN: u32 = 321;
    pub const SERVICE_FAULT: u32 = 397;
}

/// The Value attribute.
const VALUE: u32 = 13;

/// The largest chunk KYNEX takes: its receive buffer, as Hello says.
const CHUNK: u32 = 65_536;
/// The largest message KYNEX takes, and in how many chunks at most, as
/// Hello says; a server that sends more is cut off. KYNEX's answers are a
/// few kilobytes.
const MAX_MESSAGE: u32 = 4 * 1024 * 1024;
const MAX_CHUNKS: u32 = 128;
/// How deep diagnostic information may nest.
const MAX_DIAGNOSTIC_DEPTH: usize = 16;

/// A node's identity: its namespace, and a number or a name in it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
        self.diagnostic_within(0)
    }
    /// A DiagnosticInfo, and the ones nested in it, no deeper than a
    /// server has any need to: a hostile one does not run KYNEX's stack out.
    fn diagnostic_within(&mut self, depth: usize) -> Result<(), String> {
        if depth > MAX_DIAGNOSTIC_DEPTH {
            return Err("OPC UA: 진단 정보가 너무 깊이 겹쳐 있습니다".into());
        }
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
            self.diagnostic_within(depth + 1)?;
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
    /// An EndpointDescription.
    fn endpoint(&mut self) -> Result<Endpoint, String> {
        self.string()?; // its url
                        // The server's ApplicationDescription.
        self.string()?;
        self.string()?;
        self.localized()?;
        self.u32()?;
        self.string()?;
        self.string()?;
        for _ in 0..self.count()? {
            self.string()?;
        }
        let certificate = self.bytes()?.unwrap_or_default().to_vec();
        let mode = self.u32()?;
        let policy = self.string()?.unwrap_or_default();
        let mut tokens = Vec::new();
        for _ in 0..self.count()? {
            let id = self.string()?.unwrap_or_default();
            let kind = self.u32()?;
            self.string()?; // issued token type
            self.string()?; // issuer endpoint
            let policy = self.string()?.unwrap_or_default();
            tokens.push((id, kind, policy));
        }
        self.string()?; // transport profile
        self.u8()?; // security level
        Ok(Endpoint {
            certificate,
            mode,
            policy,
            tokens,
        })
    }
    /// A Variant: its type, and its value — a Boolean or a number as a
    /// whole number (a Boolean 0 or 1, a real rounded), a text (a String,
    /// a LocalizedText's text, a QualifiedName's name), or a list of
    /// Strings.
    fn variant(&mut self) -> Result<Option<(u8, Datum)>, String> {
        let kind = self.u8()?;
        let t = kind & 0x3F;
        if kind & 0x80 != 0 {
            if t != 12 || kind & 0x40 != 0 {
                return Err("OPC UA: 문자열 목록 말고는 배열 값을 읽지 않습니다".into());
            }
            let mut out = Vec::new();
            for _ in 0..self.count()? {
                out.push(self.string()?.unwrap_or_default());
            }
            return Ok(Some((t, Datum::Texts(out))));
        }
        match t {
            12 => return Ok(Some((t, Datum::Text(self.string()?.unwrap_or_default())))),
            20 => {
                self.u16()?;
                return Ok(Some((t, Datum::Text(self.string()?.unwrap_or_default()))));
            }
            21 => {
                let mask = self.u8()?;
                if mask & 1 != 0 {
                    self.string()?; // locale
                }
                let text = if mask & 2 != 0 {
                    self.string()?.unwrap_or_default()
                } else {
                    String::new()
                };
                return Ok(Some((t, Datum::Text(text))));
            }
            _ => {}
        }
        let take8 =
            |r: &mut Self| -> Result<[u8; 8], String> { Ok(r.take(8)?.try_into().unwrap()) };
        Ok(Some((
            t,
            match t {
                0 => return Ok(None),
                1 => i64::from(self.u8()? != 0),
                2 => i64::from(self.u8()? as i8),
                3 => i64::from(self.u8()?),
                4 => i64::from(self.u16()? as i16),
                5 => i64::from(self.u16()?),
                6 => i64::from(self.i32()?),
                7 => i64::from(self.u32()?),
                8 => i64::from_le_bytes(take8(self)?),
                9 => u64::from_le_bytes(take8(self)?).min(i64::MAX as u64) as i64,
                10 => f32::from_le_bytes(self.take(4)?.try_into().unwrap()).round() as i64,
                11 => f64::from_le_bytes(take8(self)?).round() as i64,
                k => return Err(format!("OPC UA: 읽을 수 없는 값 형식 {k}")),
            },
        ))
        .map(|(t, v)| (t, Datum::Number(v))))
    }
}

/// A value as read: a number (a Boolean 0 or 1), a text, or a list of
/// texts.
#[derive(Clone, Debug, PartialEq)]
pub enum Datum {
    Number(i64),
    Text(String),
    Texts(Vec<String>),
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

/// How a channel's messages are kept: not at all, signed, or signed and
/// encrypted (Basic256Sha256).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    None,
    Sign,
    SignAndEncrypt,
}

impl Mode {
    /// MessageSecurityMode, as the protocol numbers it.
    pub fn code(self) -> u32 {
        match self {
            Mode::None => 1,
            Mode::Sign => 2,
            Mode::SignAndEncrypt => 3,
        }
    }

    /// The policy KYNEX uses for it.
    pub fn policy(self) -> &'static str {
        match self {
            Mode::None => POLICY_NONE,
            _ => BASIC256SHA256,
        }
    }
}

/// An endpoint a server offers: its certificate, how its messages
/// are kept, and the users it takes.
#[derive(Clone, Debug)]
pub struct Endpoint {
    pub certificate: Vec<u8>,
    pub mode: u32,
    pub policy: String,
    /// Its user tokens: policy id, kind (0 anonymous, 1 user name), and the
    /// policy a password is encrypted with ("" for the endpoint's).
    pub tokens: Vec<(String, u32, String)>,
}

/// How to connect: the channel's keeping, KYNEX's identity and the
/// server's certificate when it is kept, and a user, if not anonymous.
pub struct Options {
    pub mode: Mode,
    pub identity: Option<Identity>,
    pub server: Option<Peer>,
    pub user: Option<(String, String)>,
}

impl Options {
    pub fn anonymous() -> Self {
        Self {
            mode: Mode::None,
            identity: None,
            server: None,
            user: None,
        }
    }
}

/// A secure channel's parties and keys.
struct Channel {
    mode: Mode,
    identity: Identity,
    server: Peer,
    client_nonce: [u8; 32],
    /// The client's keys, for what KYNEX sends; the server's, for what it
    /// receives.
    client: Option<Keys>,
    server_keys: Option<Keys>,
}

/// A session on a server: its channel, and the token its requests carry.
pub struct Session {
    stream: TcpStream,
    channel: u32,
    token_id: u32,
    sequence: u32,
    request: u32,
    auth: NodeId,
    secure: Option<Channel>,
    /// The server's last sequence number taken.
    server_sequence: Option<u32>,
    /// Each node's value type, as last read (OPC 10000-6 built-in type ids).
    types: std::collections::HashMap<NodeId, u8>,
}

impl Session {
    fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            channel: 0,
            token_id: 0,
            sequence: 0,
            request: 0,
            auth: NodeId::Numeric(0, 0),
            secure: None,
            server_sequence: None,
            types: Default::default(),
        }
    }

    /// The endpoints the server at `stream`'s other end offers, asked on a
    /// channel with no security, which is then closed.
    pub fn endpoints(stream: TcpStream, url: &str) -> Result<Vec<Endpoint>, String> {
        let mut s = Self::new(stream);
        s.hello(url)?;
        s.open_channel()?;
        let mut body = Out::default();
        body.type_id(id::GET_ENDPOINTS_REQUEST);
        let handle = s.next_request();
        body.header(&NodeId::Numeric(0, 0), handle)
            .string(Some(url))
            .i32(-1) // locales
            .i32(-1); // profiles
        let reply = s.call(&body.0, id::GET_ENDPOINTS_RESPONSE)?;
        let mut r = In::new(&reply);
        r.header()?;
        let mut out = Vec::new();
        for _ in 0..r.count()? {
            out.push(r.endpoint()?);
        }
        s.close_channel();
        Ok(out)
    }

    /// Connects to the server at `stream`'s other end, as `url` names it:
    /// a channel kept as `options` say, a session, a user.
    pub fn open(stream: TcpStream, url: &str, options: Options) -> Result<Self, String> {
        let mut s = Self::new(stream);
        if options.mode != Mode::None {
            let (Some(identity), Some(server)) = (options.identity, options.server) else {
                return Err("OPC UA: 보안 채널에는 인증서가 있어야 합니다".into());
            };
            s.secure = Some(Channel {
                mode: options.mode,
                identity,
                server,
                client_nonce: super::secure::nonce(),
                client: None,
                server_keys: None,
            });
        }
        s.hello(url)?;
        s.open_channel()?;
        let (endpoints, server_nonce, server_cert) = s.create_session(url)?;
        // The user token the server takes, on an endpoint kept as this
        // channel is.
        let mode = options.mode;
        let kind = if options.user.is_some() { 1 } else { 0 };
        let (endpoint_policy, token) = endpoints
            .iter()
            .filter(|e| e.mode == mode.code() && e.policy == mode.policy())
            .find_map(|e| {
                e.tokens
                    .iter()
                    .find(|t| t.1 == kind)
                    .map(|t| (e.policy.clone(), t.clone()))
            })
            .ok_or_else(|| {
                format!(
                    "OPC UA: 서버에 이 보안 방식으로 {} 접속하는 엔드포인트가 없습니다",
                    if kind == 1 {
                        "사용자 이름으로"
                    } else {
                        "익명으로"
                    }
                )
            })?;
        s.activate_session(
            &token,
            &endpoint_policy,
            options.user,
            &server_nonce,
            &server_cert,
        )?;
        Ok(s)
    }

    fn hello(&mut self, url: &str) -> Result<(), String> {
        let mut body = Out::default();
        body.u32(0) // protocol version
            .u32(CHUNK) // receive buffer
            .u32(CHUNK) // send buffer
            .u32(MAX_MESSAGE)
            .u32(MAX_CHUNKS)
            .string(Some(url));
        self.write_message(b"HEL", &body.0)?;
        let (kind, _) = self.read_raw()?;
        match &kind {
            b"ACK" => Ok(()),
            _ => Err(format!(
                "OPC UA: Hello에 {}로 답했습니다",
                String::from_utf8_lossy(&kind)
            )),
        }
    }

    fn open_channel(&mut self) -> Result<(), String> {
        let handle = self.next_request();
        let mut body = Out::default();
        body.type_id(id::OPEN_REQUEST);
        body.header(&NodeId::Numeric(0, 0), handle)
            .u32(0) // client protocol version
            .u32(0); // issue
        match &self.secure {
            Some(c) => {
                body.u32(c.mode.code());
                body.i32(32);
                body.0.extend(c.client_nonce);
            }
            None => {
                body.u32(Mode::None.code()).i32(-1);
            }
        }
        body.u32(3_600_000); // lifetime, ms
        self.sequence += 1;
        let (sequence, request) = (self.sequence, self.request);
        let message = self.seal_asymmetric(&body.0, sequence, request)?;
        self.stream.write_all(&message).map_err(|e| e.to_string())?;
        let (kind, reply) = self.read_raw()?;
        if &kind != b"OPN" {
            return Err(self.error_of(&kind, &reply, "OpenSecureChannel"));
        }
        let body = self.unseal_asymmetric(&reply)?;
        let mut r = In::new(&body);
        expect(&mut r, id::OPEN_RESPONSE)?;
        r.header()?;
        r.u32()?; // server protocol version
        r.u32()?; // the token's channel
        self.token_id = r.u32()?;
        r.skip(8)?; // created at
        r.u32()?; // revised lifetime
        let server_nonce = r.bytes()?.unwrap_or_default().to_vec();
        if let Some(c) = self.secure.as_mut() {
            if server_nonce.len() != 32 {
                return Err("OPC UA: 서버 nonce가 32바이트가 아닙니다".into());
            }
            c.client = Some(Keys::derive(&server_nonce, &c.client_nonce));
            c.server_keys = Some(Keys::derive(&c.client_nonce, &server_nonce));
        }
        Ok(())
    }

    /// Creates a session; returns the server's endpoints, its nonce and
    /// its certificate.
    fn create_session(&mut self, url: &str) -> Result<(Vec<Endpoint>, Vec<u8>, Vec<u8>), String> {
        let mut body = Out::default();
        body.type_id(id::CREATE_SESSION_REQUEST);
        let handle = self.next_request();
        body.header(&NodeId::Numeric(0, 0), handle);
        // The client: an application of type client.
        body.string(Some(APPLICATION_URI))
            .string(Some("urn:kynex"))
            .u8(0x02)
            .string(Some("KYNEX"))
            .u32(1)
            .string(None)
            .string(None)
            .i32(-1);
        body.string(None) // server URI
            .string(Some(url))
            .string(Some("KYNEX"));
        let client_nonce = super::secure::nonce();
        match &self.secure {
            Some(c) => {
                body.i32(32);
                body.0.extend(client_nonce);
                body.i32(c.identity.der.len() as i32);
                body.0.extend(&c.identity.der);
            }
            None => {
                body.i32(-1).i32(-1);
            }
        }
        body.f64(60_000.0) // session timeout, ms
            .u32(0); // no response size limit
        let reply = self.call(&body.0, id::CREATE_SESSION_RESPONSE)?;
        let mut r = In::new(&reply);
        r.header()?;
        r.node()?; // session id
        self.auth = r.node()?;
        r.skip(8)?; // revised timeout
        let nonce = r.bytes()?.unwrap_or_default().to_vec();
        let certificate = r.bytes()?.unwrap_or_default().to_vec();
        let mut endpoints = Vec::new();
        for _ in 0..r.count()? {
            endpoints.push(r.endpoint()?);
        }
        for _ in 0..r.count()? {
            r.bytes()?; // software certificate
            r.bytes()?; // its signature
        }
        r.string()?; // the server's signature: its algorithm
        let signature = r.bytes()?.unwrap_or_default().to_vec();
        // On a kept channel, the server proves it holds the key of the
        // certificate the channel was opened with, signing KYNEX's
        // certificate and this session's nonce: no session replayed.
        if let Some(c) = &self.secure {
            if certificate != c.server.der {
                return Err("OPC UA: 세션의 서버 인증서가 채널의 것과 다릅니다".into());
            }
            let mut signed = c.identity.der.clone();
            signed.extend(client_nonce);
            if !c.server.verify(&signed, &signature) {
                return Err("OPC UA: 서버의 세션 서명이 맞지 않습니다".into());
            }
            if nonce.len() < 32 {
                return Err("OPC UA: 서버 nonce가 짧습니다".into());
            }
        }
        Ok((endpoints, nonce, certificate))
    }

    fn activate_session(
        &mut self,
        token: &(String, u32, String),
        endpoint_policy: &str,
        user: Option<(String, String)>,
        server_nonce: &[u8],
        server_cert: &[u8],
    ) -> Result<(), String> {
        let mut body = Out::default();
        body.type_id(id::ACTIVATE_SESSION_REQUEST);
        let handle = self.next_request();
        let auth = self.auth.clone();
        body.header(&auth, handle);
        // The client's signature: of the server's certificate and nonce,
        // on a kept channel.
        match &self.secure {
            Some(c) => {
                let mut signed = server_cert.to_vec();
                signed.extend(server_nonce);
                let signature = c.identity.sign(&signed);
                body.string(Some(SIGNATURE_ALGORITHM))
                    .i32(signature.len() as i32);
                body.0.extend(signature);
            }
            None => {
                body.string(None).i32(-1);
            }
        }
        body.i32(-1); // no software certificates
        body.i32(-1); // no locales
        let mut t = Out::default();
        match user {
            // A user name, its password encrypted for the server as its
            // policy says.
            Some((name, password)) => {
                let policy = if token.2.is_empty() {
                    endpoint_policy
                } else {
                    &token.2
                };
                t.string(Some(&token.0)).string(Some(&name));
                if policy == POLICY_NONE {
                    // Unencrypted by its policy: sent only inside an
                    // encrypted channel, never in the clear.
                    let encrypted = self
                        .secure
                        .as_ref()
                        .is_some_and(|c| c.mode == Mode::SignAndEncrypt);
                    if !encrypted {
                        return Err(
                            "OPC UA: 서버가 비밀번호를 암호화하지 않고 받으려 합니다. 보안을 '서명·암호화'로 하세요"
                                .into(),
                        );
                    }
                    t.i32(password.len() as i32);
                    t.0.extend(password.as_bytes());
                    t.string(None);
                } else {
                    let server = Peer::of(server_cert)?;
                    let mut plain = Vec::new();
                    plain.extend(((password.len() + server_nonce.len()) as u32).to_le_bytes());
                    plain.extend(password.as_bytes());
                    plain.extend(server_nonce);
                    let sealed = server.encrypt(&plain)?;
                    t.i32(sealed.len() as i32);
                    t.0.extend(sealed);
                    t.string(Some(PASSWORD_ALGORITHM));
                }
                body.type_id(id::USER_NAME_IDENTITY_TOKEN);
            }
            None => {
                t.string(Some(&token.0));
                body.type_id(id::ANONYMOUS_IDENTITY_TOKEN);
            }
        }
        body.u8(0x01).i32(t.0.len() as i32);
        body.0.extend(t.0);
        body.string(None).i32(-1); // no user signature
        let reply = self.call(&body.0, id::ACTIVATE_SESSION_RESPONSE)?;
        In::new(&reply).header()
    }

    /// The values of `nodes` — a Boolean as 0 or 1, a number as a whole
    /// number — each or why it could not be read. Each node's type is
    /// kept, to write it in.
    pub fn read(&mut self, nodes: &[NodeId]) -> Result<Vec<Result<i64, String>>, String> {
        Ok(self
            .read_data(nodes)?
            .into_iter()
            .zip(nodes)
            .map(|(d, node)| match d {
                Ok(Datum::Number(v)) => Ok(v),
                Ok(_) => Err(format!("{node}: 숫자나 Boolean이 아닙니다")),
                Err(e) => Err(e),
            })
            .collect())
    }

    /// The values of `nodes`, whatever they hold, each or why it could not
    /// be read.
    pub fn read_data(&mut self, nodes: &[NodeId]) -> Result<Vec<Result<Datum, String>>, String> {
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
        // As many as were asked for, whatever the count the server gives.
        let mut out = Vec::with_capacity(n.min(nodes.len()));
        for node in nodes.iter().take(n) {
            let mask = r.u8()?;
            let value = if mask & 0x01 != 0 { r.variant()? } else { None };
            if let Some((t, _)) = value {
                self.types.insert(node.clone(), t);
            }
            let value = value.map(|(_, v)| v);
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

    /// The node `path` leads to from `start`, each step a browse name (its
    /// namespace index, and its name) along hierarchical references, as a
    /// companion specification names its parts.
    pub fn translate(&mut self, start: &NodeId, path: &[(u16, &str)]) -> Result<NodeId, String> {
        let mut body = Out::default();
        body.type_id(id::TRANSLATE_REQUEST);
        let handle = self.next_request();
        let auth = self.auth.clone();
        body.header(&auth, handle).i32(1); // one path
        body.node(start).i32(path.len() as i32);
        for (ns, name) in path {
            body.node(&NodeId::Numeric(0, 33)) // HierarchicalReferences
                .u8(0) // not inverse
                .u8(1) // its subtypes too
                .u16(*ns)
                .string(Some(name));
        }
        let reply = self.call(&body.0, id::TRANSLATE_RESPONSE)?;
        let mut r = In::new(&reply);
        r.header()?;
        let where_ = || {
            path.iter()
                .map(|(n, s)| format!("{n}:{s}"))
                .collect::<Vec<_>>()
                .join("/")
        };
        if r.count()? < 1 {
            return Err(format!("OPC UA: {} 경로의 결과가 없습니다", where_()));
        }
        let code = r.u32()?;
        if code & 0x8000_0000 != 0 {
            return Err(format!("OPC UA: {}: {}", where_(), status(code)));
        }
        if r.count()? < 1 {
            return Err(format!("OPC UA: {} 경로에 노드가 없습니다", where_()));
        }
        r.node()
    }

    /// Writes `value` to `node`'s value: as a Boolean, or else as a number
    /// in the node's own type — read once to learn it (an Int32 when it
    /// holds nothing yet).
    pub fn write(&mut self, node: &NodeId, value: i64, number: bool) -> Result<(), String> {
        let t = if number {
            match self.types.get(node) {
                Some(t) => *t,
                None => {
                    let _ = self.read(std::slice::from_ref(node))?;
                    *self.types.get(node).unwrap_or(&6)
                }
            }
        } else {
            1
        };
        let mut body = Out::default();
        body.type_id(id::WRITE_REQUEST);
        let handle = self.next_request();
        let auth = self.auth.clone();
        body.header(&auth, handle).i32(1);
        body.node(node)
            .u32(VALUE)
            .string(None)
            .u8(0x01) // a data value with a value only
            .u8(t);
        match t {
            1 => body.u8(u8::from(value != 0)),
            2 | 3 => body.u8(value as u8),
            4 | 5 => body.u16(value as u16),
            6 | 7 => body.u32(value as u32),
            8 | 9 => body.i64(value),
            10 => body.u32((value as f32).to_bits()),
            11 => body.f64(value as f64),
            _ => return Err(format!("{node}: 숫자로 쓸 수 없는 형식입니다")),
        };
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
        self.close_channel();
    }

    fn close_channel(&mut self) {
        let mut body = Out::default();
        body.type_id(id::CLOSE_CHANNEL_REQUEST);
        let handle = self.next_request();
        body.header(&NodeId::Numeric(0, 0), handle);
        if let Ok(message) = self.seal_symmetric(b"CLO", &body.0) {
            let _ = self.stream.write_all(&message);
        }
    }

    // --- messages ---

    fn next_request(&mut self) -> u32 {
        self.request += 1;
        self.request
    }

    /// A service request on the channel; its response's body after the
    /// response type, which must be `want` (or anything, for 0).
    fn call(&mut self, request: &[u8], want: u32) -> Result<Vec<u8>, String> {
        let message = self.seal_symmetric(b"MSG", request)?;
        self.stream.write_all(&message).map_err(|e| e.to_string())?;
        let reply = self.read_service()?;
        let mut r = In::new(&reply);
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

    /// Writes a message of type `kind` with `body` after its header, in
    /// one chunk: Hello's, which is not on the channel.
    fn write_message(&mut self, kind: &[u8; 3], body: &[u8]) -> Result<(), String> {
        let mut out = Vec::with_capacity(8 + body.len());
        out.extend(kind);
        out.push(b'F');
        out.extend(((8 + body.len()) as u32).to_le_bytes());
        out.extend(body);
        self.stream.write_all(&out).map_err(|e| e.to_string())
    }

    /// One chunk as it came: its type, its final-or-not flag, and all of it
    /// after the first eight bytes.
    fn read_chunk(&mut self) -> Result<([u8; 3], u8, Vec<u8>), String> {
        let mut head = [0u8; 8];
        self.stream
            .read_exact(&mut head)
            .map_err(|e| e.to_string())?;
        let kind = [head[0], head[1], head[2]];
        let size = u32::from_le_bytes(head[4..8].try_into().unwrap()) as usize;
        // No more than KYNEX said it takes.
        if !(8..=CHUNK as usize).contains(&size) {
            return Err(format!("OPC UA: 메시지 크기가 맞지 않습니다: {size}"));
        }
        let mut rest = vec![0u8; size - 8];
        self.stream
            .read_exact(&mut rest)
            .map_err(|e| e.to_string())?;
        Ok((kind, head[3], rest))
    }

    /// A message not on a symmetric channel — Acknowledge, an error, or
    /// OpenSecureChannel's answer — whole.
    fn read_raw(&mut self) -> Result<([u8; 3], Vec<u8>), String> {
        let (kind, flag, rest) = self.read_chunk()?;
        if flag != b'F' {
            return Err("OPC UA: 나뉜 메시지는 받지 않습니다".into());
        }
        Ok((kind, rest))
    }

    /// Checks an answer's channel, token and sequence header, `head` (the
    /// 8 bytes of channel and token, and the 8 of sequence number and
    /// request id): this channel's, its token's, its sequence number after
    /// the last one taken — so no message is taken twice — and an answer to
    /// the request last sent.
    fn check_sequence(&mut self, ids: &[u8], sequence: &[u8]) -> Result<(), String> {
        let word = |b: &[u8], i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
        if word(ids, 0) != self.channel || word(ids, 4) != self.token_id {
            return Err("OPC UA: 다른 채널의 메시지입니다".into());
        }
        let (number, request) = (word(sequence, 0), word(sequence, 4));
        if let Some(last) = self.server_sequence {
            // On from the last, or round past the top (OPC 10000-6 6.7.2.4).
            let wrapped = last > u32::MAX - 1024 && number < 1024;
            if number <= last && !wrapped {
                return Err("OPC UA: 이미 받은 메시지가 다시 왔습니다".into());
            }
        }
        self.server_sequence = Some(number);
        if request != self.request {
            return Err("OPC UA: 다른 요청의 응답입니다".into());
        }
        Ok(())
    }

    /// A service's answer: its chunks unsealed, their bodies joined.
    fn read_service(&mut self) -> Result<Vec<u8>, String> {
        let mut body: Vec<u8> = Vec::new();
        for _ in 0..MAX_CHUNKS {
            let (kind, flag, rest) = self.read_chunk()?;
            if &kind != b"MSG" {
                return Err(self.error_of(&kind, &rest, "요청"));
            }
            let (sequence, part) = self.unseal_symmetric(&kind, flag, &rest)?;
            self.check_sequence(&rest[..8], &sequence)?;
            if body.len() + part.len() > MAX_MESSAGE as usize {
                return Err("OPC UA: 메시지가 너무 깁니다".into());
            }
            body.extend(part);
            match flag {
                b'F' => return Ok(body),
                b'C' => continue,
                _ => return Err("OPC UA: 메시지가 중간에 취소됐습니다".into()),
            }
        }
        Err("OPC UA: 메시지 조각이 너무 많습니다".into())
    }

    /// OpenSecureChannel's request, `body`, as a whole message: on a kept
    /// channel signed with KYNEX's key and encrypted for the server's.
    fn seal_asymmetric(&self, body: &[u8], sequence: u32, request: u32) -> Result<Vec<u8>, String> {
        let mut security = Out::default();
        security.string(Some(
            self.secure
                .as_ref()
                .map_or(POLICY_NONE, |c| c.mode.policy()),
        ));
        let mut plain = Out::default();
        plain.u32(sequence).u32(request);
        plain.0.extend(body);
        let Some(c) = &self.secure else {
            security.i32(-1).i32(-1);
            let mut out = Out::default();
            out.0.extend(b"OPNF");
            out.u32((8 + 4 + security.0.len() + plain.0.len()) as u32)
                .u32(0);
            out.0.extend(security.0);
            out.0.extend(plain.0);
            return Ok(out.0);
        };
        security.i32(c.identity.der.len() as i32);
        security.0.extend(&c.identity.der);
        security.i32(20);
        security.0.extend(c.server.thumbprint());
        // Padded to whole blocks of the server's key, the signature
        // KYNEX's key long.
        let cipher_block = c.server.key_bytes();
        let plain_block = cipher_block - 42;
        let signature = c.identity.key_bytes();
        let extra = cipher_block > 256;
        let used = plain.0.len() + 1 + usize::from(extra) + signature;
        let padding = (plain_block - used % plain_block) % plain_block;
        plain.0.push(padding as u8);
        plain.0.extend(std::iter::repeat_n(padding as u8, padding));
        if extra {
            plain.0.push((padding >> 8) as u8);
        }
        let blocks = (plain.0.len() + signature) / plain_block;
        let size = 8 + 4 + security.0.len() + blocks * cipher_block;
        let mut head = Out::default();
        head.0.extend(b"OPNF");
        head.u32(size as u32).u32(0);
        head.0.extend(&security.0);
        let mut signed = head.0.clone();
        signed.extend(&plain.0);
        let sig = c.identity.sign(&signed);
        plain.0.extend(sig);
        let sealed = c.server.encrypt(&plain.0)?;
        head.0.extend(sealed);
        if head.0.len() != size {
            return Err("OPC UA: 암호화한 메시지 길이가 맞지 않습니다".into());
        }
        Ok(head.0)
    }

    /// OpenSecureChannel's answer after its first eight bytes: its body,
    /// decrypted with KYNEX's key and checked against the server's.
    fn unseal_asymmetric(&mut self, reply: &[u8]) -> Result<Vec<u8>, String> {
        let mut r = In::new(reply);
        self.channel = r.u32()?;
        let policy = r.string()?.unwrap_or_default();
        let sender = r.bytes()?.map(<[u8]>::to_vec);
        r.bytes()?; // receiver thumbprint
        let at = r.at;
        let Some(c) = &self.secure else {
            self.server_sequence = Some(r.u32()?);
            r.u32()?; // request id
            return Ok(reply[r.at..].to_vec());
        };
        if policy != c.mode.policy() {
            return Err(format!(
                "OPC UA: 서버가 다른 보안 정책으로 답했습니다: {policy}"
            ));
        }
        if sender.as_deref() != Some(c.server.der.as_slice()) {
            return Err("OPC UA: 서버가 엔드포인트와 다른 인증서로 답했습니다".into());
        }
        let plain = c.identity.decrypt(&reply[at..])?;
        let signature = c.server.key_bytes();
        if plain.len() < 8 + signature + 1 {
            return Err("OPC UA: 응답이 짧습니다".into());
        }
        let (content, sig) = plain.split_at(plain.len() - signature);
        // Signed as it was sent: its header with the encrypted size.
        let mut signed = Vec::with_capacity(8 + reply.len());
        signed.extend(b"OPNF");
        signed.extend(((8 + reply.len()) as u32).to_le_bytes());
        signed.extend(&reply[..at]);
        signed.extend(content);
        if !c.server.verify(&signed, sig) {
            return Err("OPC UA: 서버 서명이 맞지 않습니다".into());
        }
        let extra = c.identity.key_bytes() > 256;
        let padding = if extra {
            (content[content.len() - 1] as usize) << 8 | content[content.len() - 2] as usize
        } else {
            content[content.len() - 1] as usize
        };
        let end = content
            .len()
            .checked_sub(1 + usize::from(extra) + padding)
            .filter(|e| *e >= 8)
            .ok_or("OPC UA: 응답의 채움 길이가 맞지 않습니다")?;
        let first = u32::from_le_bytes(content[..4].try_into().unwrap());
        self.server_sequence = Some(first);
        Ok(content[8..end].to_vec())
    }

    /// A message of type `kind` on the channel, `body` its service: signed,
    /// and encrypted, with KYNEX's keys as the channel keeps it.
    fn seal_symmetric(&mut self, kind: &[u8; 3], body: &[u8]) -> Result<Vec<u8>, String> {
        self.sequence += 1;
        let mut plain = Out::default();
        plain.u32(self.sequence).u32(self.request);
        plain.0.extend(body);
        let keys = self
            .secure
            .as_ref()
            .and_then(|c| c.client.as_ref().map(|k| (c.mode, k)));
        let mut head = Out::default();
        head.0.extend(kind);
        head.0.push(b'F');
        match keys {
            None => {
                head.u32((16 + plain.0.len()) as u32)
                    .u32(self.channel)
                    .u32(self.token_id);
                head.0.extend(plain.0);
                Ok(head.0)
            }
            Some((mode, keys)) => {
                let encrypt = mode == Mode::SignAndEncrypt;
                if encrypt {
                    let used = plain.0.len() + 1 + 32;
                    let padding = (16 - used % 16) % 16;
                    plain.0.push(padding as u8);
                    plain.0.extend(std::iter::repeat_n(padding as u8, padding));
                }
                let size = 16 + plain.0.len() + 32;
                head.u32(size as u32).u32(self.channel).u32(self.token_id);
                let mut signed = head.0.clone();
                signed.extend(&plain.0);
                plain.0.extend(keys.mac(&signed));
                if encrypt {
                    keys.encrypt(&mut plain.0)?;
                }
                head.0.extend(plain.0);
                Ok(head.0)
            }
        }
    }

    /// A chunk on the channel after its first eight bytes: its service
    /// body, decrypted and checked with the server's keys as the channel
    /// keeps it.
    /// Returns its sequence header too.
    fn unseal_symmetric(
        &self,
        kind: &[u8; 3],
        flag: u8,
        rest: &[u8],
    ) -> Result<([u8; 8], Vec<u8>), String> {
        if rest.len() < 16 {
            return Err("OPC UA: 응답이 짧습니다".into());
        }
        let keys = self
            .secure
            .as_ref()
            .and_then(|c| c.server_keys.as_ref().map(|k| (c.mode, k)));
        let Some((mode, keys)) = keys else {
            let sequence: [u8; 8] = rest[8..16].try_into().unwrap();
            return Ok((sequence, rest[16..].to_vec()));
        };
        let mut content = rest[8..].to_vec();
        let encrypt = mode == Mode::SignAndEncrypt;
        if encrypt {
            keys.decrypt(&mut content)?;
        }
        if content.len() < 8 + 32 {
            return Err("OPC UA: 응답이 짧습니다".into());
        }
        let (data, mac) = content.split_at(content.len() - 32);
        let mut signed = Vec::with_capacity(16 + data.len());
        signed.extend(kind);
        signed.push(flag);
        signed.extend(((8 + rest.len()) as u32).to_le_bytes());
        signed.extend(&rest[..8]);
        signed.extend(data);
        if !keys.verify(&signed, mac) {
            return Err("OPC UA: 응답 서명이 맞지 않습니다".into());
        }
        let end = if encrypt {
            let padding = *data.last().ok_or("OPC UA: 응답이 짧습니다")? as usize;
            data.len()
                .checked_sub(1 + padding)
                .filter(|e| *e >= 8)
                .ok_or("OPC UA: 응답의 채움 길이가 맞지 않습니다")?
        } else {
            data.len()
        };
        let sequence: [u8; 8] = data[..8].try_into().unwrap();
        Ok((sequence, data[8..end].to_vec()))
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
        let mut s = Session::open(
            stream,
            "opc.tcp://127.0.0.1:4842/kynex/",
            Options::anonymous(),
        )
        .unwrap();
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
            s.write(&nodes[1], i64::from(k % 2 == 0), false).unwrap();
            println!("write {k}: {:?}", t.elapsed());
        }
        s.close();
    }

    /// Answers are taken once, in order, on this channel, for the request
    /// asked: one sent again, another channel's, an answer to another
    /// request are refused; a sequence number round past the top is not.
    #[test]
    fn an_answer_is_taken_once_in_order() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let stream = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let mut s = Session::new(stream);
        (s.channel, s.token_id, s.request) = (5, 9, 3);
        let ids = |channel: u32, token: u32| [channel.to_le_bytes(), token.to_le_bytes()].concat();
        let seq = |n: u32, request: u32| [n.to_le_bytes(), request.to_le_bytes()].concat();
        s.server_sequence = Some(100);
        assert!(s.check_sequence(&ids(5, 9), &seq(101, 3)).is_ok());
        assert!(s.check_sequence(&ids(5, 9), &seq(101, 3)).is_err(), "again");
        assert!(s.check_sequence(&ids(5, 9), &seq(50, 3)).is_err(), "older");
        assert!(
            s.check_sequence(&ids(6, 9), &seq(102, 3)).is_err(),
            "another channel"
        );
        assert!(
            s.check_sequence(&ids(5, 9), &seq(103, 4)).is_err(),
            "another request"
        );
        s.server_sequence = Some(u32::MAX - 3);
        assert!(
            s.check_sequence(&ids(5, 9), &seq(2, 3)).is_ok(),
            "round past the top"
        );
    }

    /// Diagnostic information nested a few deep reads; nested past the
    /// limit — as a hostile server could, a million deep — is refused
    /// rather than running the stack out.
    #[test]
    fn diagnostics_nest_only_so_deep() {
        let nested = |depth: usize| {
            let mut bytes = vec![0x40; depth];
            bytes.push(0x00);
            bytes
        };
        assert!(In::new(&nested(3)).diagnostic().is_ok());
        assert!(In::new(&nested(1_000_000)).diagnostic().is_err());
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
