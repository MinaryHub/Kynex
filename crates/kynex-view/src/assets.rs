//! The viewer's files, built into the binary: one executable carries the
//! whole viewer, and a machine with no internet draws the same page.

/// KYNEX's version, and the commit it was built from (see
/// `tools/version.rs`).
pub const VERSION: &str = env!("KYNEX_VERSION");
pub const COMMIT: &str = env!("KYNEX_COMMIT");

/// Names this build of the viewer's files: a page that reconnects and is
/// told another name was served by an older KYNEX, and reloads itself.
pub fn build_id() -> String {
    // FNV-1a over every file the page runs: a page differs when they do.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for path in [
        "/",
        "/app.js",
        "/batch.js",
        "/outline.js",
        "/robot.js",
        "/cell.js",
        "/library.js",
        "/timeline.js",
        "/stats.js",
        "/report.js",
        "/plc.js",
        "/project.js",
        "/floor.js",
        "/menu.js",
        "/i18n.js",
        "/i18n/en-page.js",
        "/i18n/en-cell.js",
        "/i18n/en-robot.js",
        "/i18n/en-panels.js",
        "/i18n/en-server.js",
        "/help.js",
        "/app.css",
    ] {
        for b in get(path).map(|(_, body)| body).unwrap_or_default() {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    format!("{h:016x}")
}

pub fn get(path: &str) -> Option<(&'static str, &'static [u8])> {
    const JS: &str = "text/javascript; charset=utf-8";
    Some(match path {
        "/" | "/index.html" => (
            "text/html; charset=utf-8",
            include_bytes!("../web/index.html"),
        ),
        "/app.js" => (JS, include_bytes!("../web/app.js")),
        "/batch.js" => (JS, include_bytes!("../web/batch.js")),
        "/outline.js" => (JS, include_bytes!("../web/outline.js")),
        "/robot.js" => (JS, include_bytes!("../web/robot.js")),
        "/cell.js" => (JS, include_bytes!("../web/cell.js")),
        "/library.js" => (JS, include_bytes!("../web/library.js")),
        "/timeline.js" => (JS, include_bytes!("../web/timeline.js")),
        "/stats.js" => (JS, include_bytes!("../web/stats.js")),
        "/report.js" => (JS, include_bytes!("../web/report.js")),
        "/plc.js" => (JS, include_bytes!("../web/plc.js")),
        "/project.js" => (JS, include_bytes!("../web/project.js")),
        "/floor.js" => (JS, include_bytes!("../web/floor.js")),
        "/menu.js" => (JS, include_bytes!("../web/menu.js")),
        "/i18n.js" => (JS, include_bytes!("../web/i18n.js")),
        "/i18n/en-page.js" => (JS, include_bytes!("../web/i18n/en-page.js")),
        "/i18n/en-cell.js" => (JS, include_bytes!("../web/i18n/en-cell.js")),
        "/i18n/en-robot.js" => (JS, include_bytes!("../web/i18n/en-robot.js")),
        "/i18n/en-panels.js" => (JS, include_bytes!("../web/i18n/en-panels.js")),
        "/i18n/en-server.js" => (JS, include_bytes!("../web/i18n/en-server.js")),
        "/help.js" => (JS, include_bytes!("../web/help.js")),
        "/app.css" => ("text/css; charset=utf-8", include_bytes!("../web/app.css")),
        "/vendor/three.module.min.js" => (JS, include_bytes!("../web/vendor/three.module.min.js")),
        "/vendor/OrbitControls.js" => (JS, include_bytes!("../web/vendor/OrbitControls.js")),
        "/vendor/TransformControls.js" => {
            (JS, include_bytes!("../web/vendor/TransformControls.js"))
        }
        "/favicon.svg" => ("image/svg+xml", include_bytes!("../web/favicon.svg")),
        _ => return None,
    })
}
