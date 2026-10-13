//! The viewer's files, built into the binary: one executable carries the
//! whole viewer, and a machine with no internet draws the same page.

pub fn get(path: &str) -> Option<(&'static str, &'static [u8])> {
    const JS: &str = "text/javascript; charset=utf-8";
    Some(match path {
        "/" | "/index.html" => (
            "text/html; charset=utf-8",
            include_bytes!("../web/index.html"),
        ),
        "/app.js" => (JS, include_bytes!("../web/app.js")),
        "/app.css" => ("text/css; charset=utf-8", include_bytes!("../web/app.css")),
        "/vendor/three.module.min.js" => (JS, include_bytes!("../web/vendor/three.module.min.js")),
        "/vendor/OrbitControls.js" => (JS, include_bytes!("../web/vendor/OrbitControls.js")),
        "/favicon.svg" => ("image/svg+xml", include_bytes!("../web/favicon.svg")),
        _ => return None,
    })
}
