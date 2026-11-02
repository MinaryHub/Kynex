//! The viewer on tablets and phones: shared with the other devices on the
//! network while KYNEX runs, from the viewer itself (도구 ▸ 태블릿·모바일에서
//! 열기), not only with `--lan` when it starts.
//!
//! Shared, KYNEX listens on the network as well — on its port plus 100
//! (8190), or any free one — and lets in only a device that comes with the
//! access code: the address shown, and its QR code, carry it, and the
//! device's browser keeps it. A new code turns away every device that had
//! the old one. Whether it is shared, and the code, are kept for this user
//! (`%APPDATA%\kynex\share.json`): a tablet's bookmark works the next day.
//!
//! What a device let in may do is what `--lan` let it do: watch, run, lay
//! out; not what reaches past the viewer — a PLC, a project's files (see
//! `view::reaches_plc`) — nor this panel or updating KYNEX.

use std::net::SocketAddr;
use std::path::PathBuf;

use kynex_view::server::Server;
use serde_json::{json, Value};

/// How far past the viewer's own port the shared one is.
const PORT_OFFSET: u16 = 100;

/// The letters an access code is made of: none that reads as another.
const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";

/// The access code's length: 31^10, out of reach of guessing on a LAN.
const KEY_LEN: usize = 10;

pub struct Share {
    on: bool,
    key: String,
    /// The viewer's own port.
    port: u16,
    /// Where the network reaches it: the shared listener once opened, or
    /// the viewer's own when it was started with `--lan`.
    addr: Option<SocketAddr>,
    /// Started with `--lan`: listening on the network from the start.
    lan: bool,
    error: Option<String>,
}

fn file() -> PathBuf {
    crate::paths::app_dir().join("share.json")
}

fn new_key() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    (0..KEY_LEN)
        .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
        .collect()
}

impl Share {
    /// Sharing as it was left, for the viewer on `port`; `lan` when started
    /// with `--lan`, which shares from the start.
    pub fn start(server: &Server, port: u16, lan: bool) -> Self {
        let kept: Value = std::fs::read_to_string(file())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        let key = kept["key"]
            .as_str()
            .filter(|k| k.len() >= 8 && k.bytes().all(|b| ALPHABET.contains(&b)))
            .map(str::to_string)
            .unwrap_or_else(new_key);
        let mut share = Share {
            on: false,
            key,
            port,
            addr: lan.then(|| SocketAddr::from(([0, 0, 0, 0], port))),
            lan,
            error: None,
        };
        let on = lan || (kept["on"].as_bool().unwrap_or(false) && !cfg!(test));
        share.set(server, on);
        share.keep();
        share
    }

    /// The address a device opens, with the access code; none while not
    /// shared.
    pub fn url(&self) -> Option<String> {
        let addr = self.addr.filter(|_| self.on)?;
        Some(format!(
            "http://{}:{}/?key={}",
            crate::view::lan_address(),
            addr.port(),
            self.key
        ))
    }

    fn set(&mut self, server: &Server, on: bool) {
        self.error = None;
        if on && self.addr.is_none() {
            let wanted = self.port.saturating_add(PORT_OFFSET);
            match server
                .share(&format!("0.0.0.0:{wanted}"))
                .or_else(|_| server.share("0.0.0.0:0"))
            {
                Ok(addr) => self.addr = Some(addr),
                Err(e) => {
                    self.error = Some(format!("네트워크에서 열지 못했습니다: {e}"));
                    self.on = false;
                    server.set_lan_key(None);
                    return;
                }
            }
        }
        self.on = on;
        server.set_lan_key(on.then(|| self.key.clone()));
    }

    fn keep(&self) {
        if cfg!(test) {
            return;
        }
        let dir = crate::paths::app_dir();
        if std::fs::create_dir_all(&dir).is_ok() {
            // `--lan` is for this run: not kept as on.
            let on = self.on && !self.lan;
            let _ = std::fs::write(file(), json!({ "on": on, "key": self.key }).to_string());
        }
    }

    /// `{"op":"share","on":true|false}`, or `{"op":"share","action":"new_key"}`
    /// — from this computer's page alone.
    pub fn op(&mut self, server: &Server, body: &Value, local: bool) -> Value {
        if !local {
            return json!({ "ok": false, "error": "공유 설정은 KYNEX를 실행한 컴퓨터에서만 바꿀 수 있습니다" });
        }
        if body["action"] == "new_key" {
            self.key = new_key();
            let on = self.on;
            self.set(server, on);
        } else if let Some(on) = body["on"].as_bool() {
            self.set(server, on);
        }
        self.keep();
        match &self.error {
            Some(e) => json!({ "ok": false, "error": e }),
            None => json!({ "ok": true }),
        }
    }

    /// What the page shows: on or not, the address and its QR code.
    pub fn to_json(&self) -> Value {
        let url = self.url();
        let qr = url.as_deref().and_then(qr_svg);
        json!({
            "on": self.on,
            "url": url,
            "qr": qr,
            "lan": self.lan,
            "error": self.error,
        })
    }
}

/// `text` as a QR code, an SVG picture.
fn qr_svg(text: &str) -> Option<String> {
    use qrcode::render::svg;
    let code = qrcode::QrCode::new(text.as_bytes()).ok()?;
    Some(
        code.render::<svg::Color>()
            .min_dimensions(220, 220)
            .quiet_zone(true)
            .dark_color(svg::Color("#000000"))
            .light_color(svg::Color("#ffffff"))
            .build(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_is_hard_to_guess_and_easy_to_read() {
        let a = new_key();
        assert_eq!(a.len(), KEY_LEN);
        assert!(a.bytes().all(|b| ALPHABET.contains(&b)));
        assert_ne!(a, new_key());
    }

    /// Shared from the page: on the network with the code, then not.
    #[test]
    fn shared_and_not() {
        let server = Server::start("127.0.0.1:0").unwrap();
        let mut share = Share::start(&server, server.addr().port(), false);
        assert!(share.url().is_none());
        assert!(!share.op(&server, &json!({"on": true}), false)["ok"]
            .as_bool()
            .unwrap());
        assert!(share.op(&server, &json!({"on": true}), true)["ok"]
            .as_bool()
            .unwrap());
        let url = share.url().unwrap();
        assert!(url.contains(&format!("?key={}", share.key)));
        let shown = share.to_json();
        assert!(shown["qr"].as_str().unwrap().contains("<svg"));
        let old = share.key.clone();
        share.op(&server, &json!({"action": "new_key"}), true);
        assert_ne!(share.key, old);
        share.op(&server, &json!({"on": false}), true);
        assert!(share.url().is_none());
    }
}
