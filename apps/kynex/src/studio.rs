//! KYNEX as a program of its own: the viewer served as `kynex view` serves
//! it, on its own (no Kreate), shown in a window of its own — the system's
//! WebView2, no browser, no address bar. Closing the window ends KYNEX; the
//! project is saved as it goes, so nothing waits to be saved.
//!
//! The port is 8090 if free, else the next of 8091–8099: the page keeps
//! what it remembers (its snap steps) by port. The window's own data —
//! WebView2's — is kept in `%APPDATA%\kynex\webview`, not beside the
//! program, which an installed program cannot write to.

/// Runs KYNEX in its window until the window is closed.
#[cfg(windows)]
pub fn run() {
    use std::time::Duration;
    use tao::dpi::LogicalSize;
    use tao::event::{Event, WindowEvent};
    use tao::event_loop::{ControlFlow, EventLoop};
    use tao::window::{Icon, WindowBuilder};

    let (tx, rx) = std::sync::mpsc::channel::<Result<String, String>>();
    std::thread::spawn(move || {
        let mut last = String::from("KYNEX를 시작하지 못했습니다");
        for port in 8090..=8099 {
            match crate::view::run_with(vec!["--port".into(), port.to_string()], Some(tx.clone())) {
                // Taken: the next.
                Err(e) if e.starts_with("could not listen") => last = e,
                Err(e) => {
                    let _ = tx.send(Err(e));
                    return;
                }
                Ok(()) => return,
            }
        }
        let _ = tx.send(Err(last));
    });
    let start = rx
        .recv_timeout(Duration::from_secs(120))
        .unwrap_or_else(|_| Err("KYNEX가 2분 안에 시작되지 않았습니다".into()));

    let event_loop = EventLoop::new();
    let icon = Icon::from_rgba(include_bytes!("../assets/kynex-64.rgba").to_vec(), 64, 64).ok();
    let window = match WindowBuilder::new()
        .with_title("KYNEX")
        .with_inner_size(LogicalSize::new(1440.0, 900.0))
        .with_window_icon(icon)
        .build(&event_loop)
    {
        Ok(w) => w,
        Err(e) => return fail(&format!("창을 열지 못했습니다: {e}")),
    };
    let mut context = wry::WebContext::new(Some(crate::paths::app_dir().join("webview")));
    let builder = wry::WebViewBuilder::new_with_web_context(&mut context);
    let builder = match &start {
        Ok(url) => builder.with_url(url),
        Err(e) => builder.with_html(error_page(e)),
    };
    let _webview = match builder.build(&window) {
        Ok(w) => w,
        Err(e) => return fail(&format!(
            "화면(WebView2)을 열지 못했습니다: {e}\nMicrosoft Edge WebView2 런타임을 설치하세요."
        )),
    };
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            *control_flow = ControlFlow::Exit;
        }
    });
}

/// Elsewhere, KYNEX in the browser.
#[cfg(not(windows))]
pub fn run() {
    if let Err(e) = crate::view::run(vec!["--open".into()]) {
        eprintln!("kynex: {e}");
        std::process::exit(2);
    }
}

/// Said in a box of the system's own, for when there is no window to say it
/// in.
#[cfg(windows)]
fn fail(text: &str) {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "user32")]
    extern "system" {
        fn MessageBoxW(
            hwnd: *mut core::ffi::c_void,
            text: *const u16,
            caption: *const u16,
            kind: u32,
        ) -> i32;
    }
    let wide = |s: &str| -> Vec<u16> { std::ffi::OsStr::new(s).encode_wide().chain([0]).collect() };
    let (text, caption) = (wide(text), wide("KYNEX"));
    // MB_ICONERROR.
    unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), caption.as_ptr(), 0x10) };
}

/// Why KYNEX did not start, as a page in its window.
#[cfg(windows)]
fn error_page(why: &str) -> String {
    let why = why
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!(
        "<!doctype html><meta charset=utf-8><title>KYNEX</title>\
         <body style=\"margin:0;display:grid;place-items:center;height:100vh;background:#15181d;color:#e6e8eb;font:15px system-ui,'Malgun Gothic',sans-serif\">\
         <div style=\"max-width:560px;padding:24px\"><h2 style=\"color:#ff8a3d\">KYNEX를 시작하지 못했습니다</h2>\
         <p style=\"white-space:pre-wrap;line-height:1.6\">{why}</p>\
         <p style=\"color:#9aa3ad\">창을 닫고 다시 실행하세요. 같은 프로젝트를 연 KYNEX가 이미 있으면 그 창을 쓰세요.</p></div>"
    )
}
