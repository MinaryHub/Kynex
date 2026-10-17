//! The site's drawing: a plan of the factory floor (PNG, JPEG or WebP) laid
//! under the scene at its true size, to place equipment on — the first
//! thing a layout is drawn against. Kept in the project file, the image in
//! it as a data URL, so a project stays one file.

use base64::Engine;
use serde_json::{json, Value};

/// The largest drawing taken, bytes.
pub const MOST_BYTES: usize = 8 << 20;

/// The drawing, if there is one, and where it lies: its width in metres
/// (its height as the image is), its centre on the floor, its turn
/// (degrees) and how strongly it shows.
#[derive(Debug, Clone, PartialEq)]
pub struct Floor {
    pub image: Option<String>,
    pub width: f64,
    pub x: f64,
    pub y: f64,
    pub yaw: f64,
    pub opacity: f64,
}

impl Default for Floor {
    fn default() -> Self {
        Self {
            image: None,
            width: 50.0,
            x: 0.0,
            y: 0.0,
            yaw: 0.0,
            opacity: 0.8,
        }
    }
}

/// The kind of image `bytes` are, by how they begin.
fn kind_of(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

impl Floor {
    /// As the project file keeps it; none, the usual.
    pub fn of(v: &Value) -> Self {
        let mut f = Self::default();
        f.image = v["image"]
            .as_str()
            .filter(|s| s.starts_with("data:image/"))
            .map(str::to_string);
        f.place(v);
        f
    }

    pub fn to_json(&self) -> Value {
        match &self.image {
            Some(image) => json!({
                "image": image,
                "width": self.width,
                "x": self.x,
                "y": self.y,
                "yaw": self.yaw,
                "opacity": self.opacity,
            }),
            None => json!({}),
        }
    }

    /// Where it lies, from `v`: what it names, the rest kept.
    fn place(&mut self, v: &Value) {
        let n = |k: &str| v[k].as_f64().filter(|x| x.is_finite());
        if let Some(w) = n("width") {
            self.width = w.clamp(1.0, 5000.0);
        }
        if let Some(x) = n("x") {
            self.x = x.clamp(-10000.0, 10000.0);
        }
        if let Some(y) = n("y") {
            self.y = y.clamp(-10000.0, 10000.0);
        }
        if let Some(a) = n("yaw") {
            self.yaw = a.rem_euclid(360.0);
        }
        if let Some(o) = n("opacity") {
            self.opacity = o.clamp(0.05, 1.0);
        }
    }

    /// The drawing from an image file's `bytes`; where it lies kept.
    pub fn set_image(&mut self, bytes: &[u8]) -> Result<(), String> {
        if bytes.len() > MOST_BYTES {
            return Err(format!("도면 이미지는 {} MB까지입니다", MOST_BYTES >> 20));
        }
        let kind = kind_of(bytes).ok_or("PNG, JPEG, WebP 이미지만 도면으로 쓸 수 있습니다")?;
        let data = base64::engine::general_purpose::STANDARD.encode(bytes);
        self.image = Some(format!("data:{kind};base64,{data}"));
        Ok(())
    }

    /// A request from the page: `set` where it lies, or `clear` it.
    pub fn handle(&mut self, req: &Value) -> Result<(), String> {
        match req["action"].as_str().unwrap_or_default() {
            "set" => {
                if self.image.is_none() {
                    return Err("먼저 도면 이미지를 불러오세요".into());
                }
                self.place(req);
                Ok(())
            }
            "clear" => {
                *self = Self::default();
                Ok(())
            }
            other => Err(format!("unknown action {other:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drawing_is_kept_with_where_it_lies() {
        let mut f = Floor::default();
        assert_eq!(f.to_json(), json!({}));
        // Nothing to place yet.
        assert!(f.handle(&json!({"action": "set", "width": 80})).is_err());
        // Not an image.
        assert!(f.set_image(b"hello").is_err());
        let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0];
        f.set_image(&png).unwrap();
        f.handle(&json!({"action": "set", "width": 80, "x": 12.5, "yaw": -90, "opacity": 3}))
            .unwrap();
        let saved = f.to_json();
        assert!(saved["image"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,"));
        assert_eq!(saved["width"], 80.0);
        assert_eq!(saved["yaw"], 270.0);
        assert_eq!(saved["opacity"], 1.0);
        // Read back as saved.
        assert_eq!(Floor::of(&saved), f);
        // Too big.
        assert!(f.set_image(&vec![0u8; MOST_BYTES + 1]).is_err());
        f.handle(&json!({"action": "clear"})).unwrap();
        assert_eq!(f, Floor::default());
    }
}
