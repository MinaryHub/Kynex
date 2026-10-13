//! The KYNEX viewer: a scene drawn in a web browser, kept live.
//!
//! The browser is the one 3D surface every machine has: Windows, macOS,
//! Linux and a tablet on the shop floor all draw WebGL. So KYNEX serves the
//! viewer itself — a page, its script and three.js, all inside the binary —
//! and pushes the scene down a Server-Sent Events stream as it changes.
//! Nothing to install, nothing fetched from the internet.
//!
//! - [`scene`]: a scene as the viewer's JSON, whole or by the edit.
//! - [`server`]: the HTTP server and the stream.

mod assets;
mod base64;
pub mod json;
pub mod scene;
pub mod server;

pub use scene::ViewState;
pub use server::{Server, Status};
