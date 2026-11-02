//! KYNEX keeps itself up to date: it asks GitHub for the latest release
//! (`releases/latest` of MinaryHub/Kynex, which every push to `main` makes),
//! and when that is newer, fetches the file for this computer while KYNEX
//! runs, checks it against the SHA-256 GitHub gives, and installs it the
//! next time KYNEX starts — or at once, from the viewer (the version by the
//! brand ▸ 지금 업데이트), KYNEX starting again on its own after.
//!
//! How it is installed follows how KYNEX was:
//!
//! - **Setup** (`KYNEX-<v>-setup.exe`, `unins000.exe` beside the program):
//!   the new setup, silent, over the old; then KYNEX again.
//! - **Portable** (the Windows zip, the Linux and macOS tar.gz, in a folder
//!   KYNEX can write to): the programs replaced where they are; then KYNEX
//!   again, with the same arguments.
//! - **Manual** (the MSI, which IT deploys and updates; a folder KYNEX cannot
//!   write to; a system no release is built for): only told, with the
//!   release's page.
//! - **Dev** (built from source, `-dev`, or run from `target`): never.
//!
//! Downloads go through the system's own `curl` (Windows 10 has one; else
//! PowerShell), which takes the system's proxy and certificates as they
//! are. Whether to update on its own is kept for this user
//! (`%APPDATA%\kynex\update.json`, `{"auto": true}` unless turned off);
//! `KYNEX_NO_UPDATE=1` stops it from looking at all.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kynex_view::server::Server;
use serde_json::{json, Value};

/// Where releases are made.
pub const REPO: &str = "MinaryHub/Kynex";

/// How long after starting KYNEX first looks, and how often after.
const FIRST_LOOK: Duration = Duration::from_secs(20);
const LOOK_EVERY: Duration = Duration::from_secs(6 * 3600);

/// A version, `major.minor.build`: compared by its numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u32, pub u32, pub u32);

impl Version {
    /// `0.1.194`, `v0.1.194` or `0.1.194-dev`.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim().trim_start_matches('v');
        let text = text.split(['-', '+']).next()?;
        let mut n = text.split('.').map(|p| p.parse::<u32>().ok());
        let v = Version(n.next()??, n.next()??, n.next()??);
        n.next().is_none().then_some(v)
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// This KYNEX's version.
pub fn current() -> Option<Version> {
    Version::parse(env!("KYNEX_VERSION"))
}

/// How this KYNEX was installed, and so how it is updated (see the module).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Setup,
    Portable,
    Manual,
    Dev,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Setup => "setup",
            Kind::Portable => "portable",
            Kind::Manual => "manual",
            Kind::Dev => "dev",
        }
    }

    /// Whether KYNEX can install an update itself.
    fn installs(self) -> bool {
        matches!(self, Kind::Setup | Kind::Portable)
    }
}

/// How this KYNEX was installed, by where it runs from.
pub fn kind() -> Kind {
    if env!("KYNEX_VERSION").ends_with("-dev") || current().is_none() {
        return Kind::Dev;
    }
    let Ok(exe) = std::env::current_exe() else {
        return Kind::Manual;
    };
    // Built by cargo, run where it was built.
    if exe.components().any(|c| c.as_os_str() == "target") {
        return Kind::Dev;
    }
    let Some(dir) = exe.parent() else {
        return Kind::Manual;
    };
    if asset_suffix().is_none() {
        return Kind::Manual;
    }
    if cfg!(windows) && dir.join("unins000.exe").is_file() {
        return Kind::Setup;
    }
    if writable(dir) {
        Kind::Portable
    } else {
        Kind::Manual
    }
}

/// Whether KYNEX can write to `dir`.
fn writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".kynex-update-{}", std::process::id()));
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// The end of the release file's name for this system, after
/// `KYNEX-<version>`: none where no release is built for it.
fn asset_suffix() -> Option<&'static str> {
    if cfg!(all(windows, target_arch = "x86_64")) {
        Some("-windows-x64.zip")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("-linux-x64.tar.gz")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("-macos-arm64.tar.gz")
    } else {
        None
    }
}

/// The release file's name for version `v`, installed as `kind`.
fn asset_name(v: &str, kind: Kind) -> Option<String> {
    if kind == Kind::Setup {
        return Some(format!("KYNEX-{v}-setup.exe"));
    }
    asset_suffix().map(|s| format!("KYNEX-{v}{s}"))
}

/// A file of a release.
#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
    /// Its SHA-256, in hex, when GitHub gives it.
    pub sha256: Option<String>,
}

/// The latest release, and its file for this computer.
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: Version,
    /// Its page on GitHub.
    pub page: String,
    pub asset: Option<Asset>,
}

/// The release GitHub's API describes in `v`, with its file for `kind`.
fn parse_release(v: &Value, kind: Kind) -> Result<Release, String> {
    let tag = v["tag_name"].as_str().ok_or("릴리스에 버전이 없습니다")?;
    let version = Version::parse(tag).ok_or_else(|| format!("알 수 없는 버전: {tag}"))?;
    let wanted = asset_name(&version.to_string(), kind);
    let asset = v["assets"].as_array().and_then(|list| {
        list.iter()
            .find(|a| wanted.as_deref().is_some_and(|w| a["name"] == w))
            .and_then(|a| {
                Some(Asset {
                    name: a["name"].as_str()?.to_string(),
                    url: a["browser_download_url"].as_str()?.to_string(),
                    size: a["size"].as_u64().unwrap_or(0),
                    sha256: a["digest"]
                        .as_str()
                        .and_then(|d| d.strip_prefix("sha256:"))
                        .map(str::to_ascii_lowercase),
                })
            })
    });
    Ok(Release {
        version,
        page: v["html_url"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| format!("https://github.com/{REPO}/releases/latest")),
        asset,
    })
}

/// Where downloads are kept until installed.
fn updates_dir() -> PathBuf {
    crate::paths::app_dir().join("updates")
}

/// `url` fetched into `path`, in at most `seconds`.
fn fetch(url: &str, path: &Path, seconds: u32) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let mut curl = Command::new(if cfg!(windows) { "curl.exe" } else { "curl" });
    curl.args(["-fsSL", "--retry", "2", "--connect-timeout", "20"])
        .args(["--max-time", &seconds.to_string()])
        .args(["-A", concat!("KYNEX/", env!("KYNEX_VERSION"))])
        .args([
            "-H",
            "Accept: application/vnd.github+json, application/octet-stream",
        ])
        .arg("-o")
        .arg(path)
        .arg(url);
    hidden(&mut curl);
    match curl.output() {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => Err(format!(
            "내려받지 못했습니다: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )),
        // No curl (Windows 10 before 1803): PowerShell's.
        Err(_) if cfg!(windows) => {
            let script = format!(
                "$ProgressPreference='SilentlyContinue';\
                 [Net.ServicePointManager]::SecurityProtocol=[Net.SecurityProtocolType]::Tls12;\
                 Invoke-WebRequest -UseBasicParsing -TimeoutSec {seconds} -UserAgent 'KYNEX' -Uri '{}' -OutFile '{}'",
                url.replace('\'', "''"),
                path.display().to_string().replace('\'', "''")
            );
            let mut ps = Command::new("powershell");
            ps.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
            hidden(&mut ps);
            match ps.output() {
                Ok(out) if out.status.success() => Ok(()),
                Ok(out) => Err(format!(
                    "내려받지 못했습니다: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                )),
                Err(e) => Err(format!("내려받지 못했습니다: {e}")),
            }
        }
        Err(e) => Err(format!("curl을 실행하지 못했습니다: {e}")),
    }
}

/// A program run with no console window of its own, on Windows.
fn hidden(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

/// The latest release, as GitHub says now.
pub fn latest(kind: Kind) -> Result<Release, String> {
    let path = updates_dir().join("latest.json");
    fetch(
        &format!("https://api.github.com/repos/{REPO}/releases/latest"),
        &path,
        30,
    )?;
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&path);
    let v: Value =
        serde_json::from_str(&text).map_err(|e| format!("GitHub의 답을 읽지 못했습니다: {e}"))?;
    parse_release(&v, kind)
}

/// Whether the file at `path` is `asset`: its size, and its SHA-256 when
/// known.
fn verify(path: &Path, size: u64, sha256: Option<&str>) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if size != 0 && bytes.len() as u64 != size {
        return Err(format!(
            "내려받은 파일의 크기가 다릅니다 ({} / {size} 바이트)",
            bytes.len()
        ));
    }
    if let Some(want) = sha256 {
        let got: String = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if got != want {
            return Err("내려받은 파일의 SHA-256이 릴리스와 다릅니다".into());
        }
    }
    Ok(())
}

/// The release's file, fetched and checked: where it is. Kept as the update
/// to install when KYNEX next starts.
pub fn download(release: &Release, kind: Kind) -> Result<PathBuf, String> {
    let asset = release
        .asset
        .as_ref()
        .ok_or("이 컴퓨터에 맞는 릴리스 파일이 없습니다")?;
    let dir = updates_dir();
    let path = dir.join(&asset.name);
    if verify(&path, asset.size, asset.sha256.as_deref()).is_err() {
        // Older downloads are of no more use.
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for e in entries.flatten() {
                if e.file_name().to_string_lossy().starts_with("KYNEX-") {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        let part = dir.join(format!("{}.part", asset.name));
        fetch(&asset.url, &part, 1800)?;
        if let Err(e) = verify(&part, asset.size, asset.sha256.as_deref()) {
            let _ = std::fs::remove_file(&part);
            return Err(e);
        }
        std::fs::rename(&part, &path).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    let pending = json!({
        "version": release.version.to_string(),
        "file": path.display().to_string(),
        "kind": kind.name(),
        "size": asset.size,
        "sha256": asset.sha256,
    });
    let _ = std::fs::write(dir.join("pending.json"), pending.to_string());
    Ok(path)
}

/// Installs the update in `file` (as `kind`), and ends this KYNEX for it;
/// `relaunch`: KYNEX started again after, with the same arguments. Returns
/// only if it could not.
pub fn install_and_exit(file: &Path, kind: Kind, relaunch: bool) -> String {
    match install(file, kind, relaunch) {
        Ok(()) => {
            // The page's answer on its way first.
            std::thread::sleep(Duration::from_millis(400));
            std::process::exit(0);
        }
        Err(e) => e,
    }
}

fn install(file: &Path, kind: Kind, relaunch: bool) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("KYNEX가 있는 폴더를 모릅니다")?;
    match kind {
        Kind::Setup => run_setup(file, &exe, relaunch),
        Kind::Portable => {
            replace_programs(file, dir)?;
            if relaunch {
                start_again(&exe)?;
            }
            Ok(())
        }
        Kind::Manual | Kind::Dev => Err("이 KYNEX는 스스로 업데이트하지 않습니다".into()),
    }
}

/// The arguments KYNEX is started again with: its own.
fn own_args() -> Vec<String> {
    std::env::args().skip(1).collect()
}

/// The viewer's port, for the KYNEX started again to wait for: it is free
/// once this one has ended.
static PORT: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(0);

/// The port this KYNEX's viewer is on.
pub fn serving_on(port: u16) {
    PORT.store(port, Ordering::Relaxed);
}

/// Starts `exe` again a moment after this one has ended: through the
/// system's shell, which outlives this KYNEX, so that even a KYNEX that
/// does not wait for its port (one from before this) finds it free. A
/// KYNEX that does waits for it as well (see [`relaunched`]).
fn start_again(exe: &Path) -> Result<(), String> {
    #[cfg(windows)]
    let mut cmd = {
        use std::os::windows::process::CommandExt;
        let mut line = String::from("ping -n 3 127.0.0.1 >nul & start \"\" ");
        line.push_str(&command_line(exe));
        let mut cmd = Command::new("cmd");
        cmd.raw_arg("/S /C \"").raw_arg(&line).raw_arg("\"");
        hidden(&mut cmd);
        cmd
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg(r#"sleep 2; exec "$0" "$@""#)
            .arg(exe)
            .args(own_args());
        cmd
    };
    cmd.env(RELAUNCHED, PORT.load(Ordering::Relaxed).to_string())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("KYNEX를 다시 시작하지 못했습니다: {e}"))
}

/// `exe` and this KYNEX's arguments, each quoted, for `cmd`.
#[cfg(windows)]
fn command_line(exe: &Path) -> String {
    let quote = |s: &str| format!("\"{}\"", s.replace('"', ""));
    let mut line = quote(&exe.display().to_string());
    for a in own_args() {
        line.push(' ');
        line.push_str(&quote(&a));
    }
    line
}

/// Set for a KYNEX started again by an update: the port the one before it
/// served on.
const RELAUNCHED: &str = "KYNEX_RELAUNCHED";

/// Whether this KYNEX was started again by an update — so it opens no
/// browser, the page there reconnecting by itself —, having waited until
/// the KYNEX before it has ended (its port free: its project let go too).
pub fn relaunched() -> bool {
    static ASKED: AtomicBool = AtomicBool::new(false);
    static WAS: AtomicBool = AtomicBool::new(false);
    if ASKED.swap(true, Ordering::SeqCst) {
        return WAS.load(Ordering::SeqCst);
    }
    let Some(port) = std::env::var(RELAUNCHED)
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
    else {
        return false;
    };
    WAS.store(true, Ordering::SeqCst);
    if port != 0 {
        for _ in 0..300 {
            if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    // Its files let go as it ended.
    std::thread::sleep(Duration::from_millis(300));
    true
}

/// The new setup, silent, over this KYNEX once it has ended; then KYNEX
/// again. Through `cmd`, which outlives this KYNEX and waits for the setup.
#[cfg(windows)]
fn run_setup(file: &Path, exe: &Path, relaunch: bool) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let mut line = format!(
        "\"{}\" /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /CLOSEAPPLICATIONS /SP-",
        file.display().to_string().replace('"', "")
    );
    if relaunch {
        line.push_str(" & start \"\" ");
        line.push_str(&command_line(exe));
    }
    let mut cmd = Command::new("cmd");
    cmd.raw_arg("/S /C \"")
        .raw_arg(&line)
        .raw_arg("\"")
        .env(RELAUNCHED, PORT.load(Ordering::Relaxed).to_string());
    hidden(&mut cmd);
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("설치 파일을 실행하지 못했습니다: {e}"))
}

#[cfg(not(windows))]
fn run_setup(_file: &Path, _exe: &Path, _relaunch: bool) -> Result<(), String> {
    Err("설치 파일은 Windows에서만 씁니다".into())
}

/// The programs in the release's `file` (zip or tar.gz) put in place of
/// those in `dir`. A running program is renamed out of the way first
/// (`.old`, removed when KYNEX next starts), which Windows allows.
fn replace_programs(file: &Path, dir: &Path) -> Result<(), String> {
    let names: &[&str] = if cfg!(windows) {
        &["kynex.exe", "kynex-studio.exe"]
    } else {
        &["kynex", "kynex-studio"]
    };
    let found = unpack(file, names)?;
    if !found.iter().any(|(n, _)| n == names[0]) {
        return Err("릴리스 파일에 kynex가 없습니다".into());
    }
    for (name, bytes) in found {
        let target = dir.join(&name);
        // Only the programs that are here: none added beside them.
        if name != names[0] && !target.exists() {
            continue;
        }
        let new = dir.join(format!("{name}.new"));
        std::fs::write(&new, &bytes).map_err(|e| format!("{}: {e}", new.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o755));
        }
        if cfg!(windows) && target.exists() {
            let old = dir.join(format!("{name}.old"));
            let _ = std::fs::remove_file(&old);
            std::fs::rename(&target, &old).map_err(|e| format!("{}: {e}", target.display()))?;
        }
        std::fs::rename(&new, &target).map_err(|e| format!("{}: {e}", target.display()))?;
    }
    Ok(())
}

/// The files named `names` anywhere in the archive `file`: each name and
/// its bytes.
fn unpack(file: &Path, names: &[&str]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let name = file.to_string_lossy();
    let mut out = Vec::new();
    if name.ends_with(".zip") {
        use std::io::Read;
        let f = std::fs::File::open(file).map_err(|e| e.to_string())?;
        let mut zip = zip::ZipArchive::new(f).map_err(|e| format!("zip: {e}"))?;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| format!("zip: {e}"))?;
            let base = entry
                .name()
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or("")
                .to_string();
            if names.contains(&base.as_str()) {
                let mut bytes = Vec::new();
                entry
                    .read_to_end(&mut bytes)
                    .map_err(|e| format!("zip: {e}"))?;
                out.push((base, bytes));
            }
        }
    } else {
        // tar.gz: the system's tar, into a folder of its own.
        let into = updates_dir().join(format!("unpacked-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&into);
        std::fs::create_dir_all(&into).map_err(|e| e.to_string())?;
        let status = Command::new("tar")
            .arg("-xzf")
            .arg(file)
            .arg("-C")
            .arg(&into)
            .status()
            .map_err(|e| format!("tar: {e}"))?;
        if !status.success() {
            let _ = std::fs::remove_dir_all(&into);
            return Err("릴리스 파일을 풀지 못했습니다".into());
        }
        let mut stack = vec![into.clone()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let p = e.path();
                let base = e.file_name().to_string_lossy().into_owned();
                if p.is_dir() {
                    stack.push(p);
                } else if names.contains(&base.as_str()) {
                    out.push((base, std::fs::read(&p).map_err(|e| e.to_string())?));
                }
            }
        }
        let _ = std::fs::remove_dir_all(&into);
    }
    Ok(out)
}

/// Whether KYNEX looks for updates at all.
fn enabled() -> bool {
    !cfg!(test) && std::env::var_os("KYNEX_NO_UPDATE").is_none_or(|v| v.is_empty() || v == "0")
}

/// Whether KYNEX updates on its own (kept for this user; on unless turned
/// off).
fn auto() -> bool {
    std::fs::read_to_string(crate::paths::app_dir().join("update.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v["auto"].as_bool())
        .unwrap_or(true)
}

fn set_auto(on: bool) {
    let dir = crate::paths::app_dir();
    if std::fs::create_dir_all(&dir).is_ok() {
        let _ = std::fs::write(dir.join("update.json"), json!({ "auto": on }).to_string());
    }
}

/// As KYNEX starts: what an earlier update left removed, and an update
/// downloaded before installed now — this KYNEX ending for it — when it is
/// newer, still checks out, and has not been tried already.
pub fn on_start() {
    static ONCE: AtomicBool = AtomicBool::new(false);
    if ONCE.swap(true, Ordering::SeqCst) || !enabled() {
        return;
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    {
        for name in ["kynex.exe.old", "kynex-studio.exe.old"] {
            let _ = std::fs::remove_file(dir.join(name));
        }
    }
    let kind = kind();
    if !kind.installs() || !auto() {
        return;
    }
    let pending_file = updates_dir().join("pending.json");
    let Some(pending) = std::fs::read_to_string(&pending_file)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
    else {
        return;
    };
    let version = pending["version"].as_str().and_then(Version::parse);
    let (Some(version), Some(now)) = (version, current()) else {
        return;
    };
    if version <= now || pending["kind"] != kind.name() {
        let _ = std::fs::remove_file(&pending_file);
        return;
    }
    // Tried once and still here: it did not take. Not again at every start.
    if pending["tried"] == version.to_string() {
        return;
    }
    let file = PathBuf::from(pending["file"].as_str().unwrap_or(""));
    let checked = verify(
        &file,
        pending["size"].as_u64().unwrap_or(0),
        pending["sha256"].as_str(),
    );
    if checked.is_err() {
        let _ = std::fs::remove_file(&pending_file);
        return;
    }
    let mut tried = pending.clone();
    tried["tried"] = json!(version.to_string());
    let _ = std::fs::write(&pending_file, tried.to_string());
    println!("KYNEX {now} → {version}: 업데이트를 설치합니다…");
    let why = install_and_exit(&file, kind, true);
    eprintln!("kynex: 업데이트를 설치하지 못했습니다: {why}");
}

/// Where the updater is, as the page shows it.
#[derive(Clone, Debug, PartialEq)]
enum State {
    /// Not looked yet.
    Idle,
    Checking,
    /// Up to date.
    Latest,
    /// A newer one, not fetched (not on its own, or not installable here).
    Available,
    Downloading,
    /// Fetched and checked: installed when KYNEX next starts, or now.
    Ready(PathBuf),
    Installing,
    Failed(String),
}

struct Inner {
    kind: Kind,
    auto: bool,
    state: State,
    latest: Option<Release>,
    busy: bool,
}

/// KYNEX's updates while it runs: looked for now and then, fetched when
/// newer, told to the viewers as the `updater` event (`update` is the scene's).
#[derive(Clone)]
pub struct Updater {
    inner: Arc<Mutex<Inner>>,
    server: Server,
}

impl Updater {
    /// Looks for updates in the background, every few hours, for as long
    /// as KYNEX runs.
    pub fn start(server: &Server) -> Self {
        let kind = kind();
        let updater = Updater {
            inner: Arc::new(Mutex::new(Inner {
                kind,
                auto: auto(),
                state: State::Idle,
                latest: None,
                busy: false,
            })),
            server: server.clone(),
        };
        updater.tell();
        if enabled() && kind != Kind::Dev {
            let me = updater.clone();
            let _ = std::thread::Builder::new()
                .name("kynex-update".into())
                .spawn(move || {
                    std::thread::sleep(FIRST_LOOK);
                    loop {
                        let fetch = me.inner.lock().map(|i| i.auto).unwrap_or(false);
                        me.look(fetch);
                        std::thread::sleep(LOOK_EVERY);
                    }
                });
        }
        updater
    }

    fn tell(&self) {
        let json = self.to_json();
        self.server.state("updater", &json.to_string());
    }

    fn set(&self, state: State) {
        if let Ok(mut i) = self.inner.lock() {
            i.state = state;
        }
        self.tell();
    }

    /// Asks GitHub; fetches the update too when `fetch` and it can be
    /// installed here. Waits for it all: on a thread of its own.
    fn look(&self, fetch: bool) {
        {
            let Ok(mut i) = self.inner.lock() else { return };
            if i.busy {
                return;
            }
            i.busy = true;
        }
        self.set(State::Checking);
        let kind = self.inner.lock().map(|i| i.kind).unwrap_or(Kind::Manual);
        let state = match latest(kind) {
            Err(e) => State::Failed(e),
            Ok(release) => {
                let newer = current().is_some_and(|now| release.version > now);
                if let Ok(mut i) = self.inner.lock() {
                    i.latest = Some(release.clone());
                }
                if !newer {
                    State::Latest
                } else if fetch && kind.installs() && release.asset.is_some() {
                    self.set(State::Downloading);
                    match download(&release, kind) {
                        Ok(path) => State::Ready(path),
                        Err(e) => State::Failed(e),
                    }
                } else {
                    State::Available
                }
            }
        };
        if let Ok(mut i) = self.inner.lock() {
            i.busy = false;
        }
        self.set(state);
    }

    /// The page's `{"op":"update", "action": "check" | "install" | "auto",
    /// "on": bool}` — from this computer's page alone.
    pub fn op(&self, body: &Value, local: bool) -> Value {
        if !local {
            return json!({ "ok": false, "error": "업데이트는 KYNEX를 실행한 컴퓨터에서만 할 수 있습니다" });
        }
        let (kind, busy) = match self.inner.lock() {
            Ok(i) => (i.kind, i.busy),
            Err(_) => return json!({ "ok": false, "error": "update" }),
        };
        match body["action"].as_str().unwrap_or("") {
            "auto" => {
                let on = body["on"].as_bool().unwrap_or(true);
                set_auto(on);
                if let Ok(mut i) = self.inner.lock() {
                    i.auto = on;
                }
                self.tell();
                json!({ "ok": true })
            }
            "check" => {
                if kind == Kind::Dev {
                    return json!({ "ok": false, "error": "개발 빌드는 업데이트를 찾지 않습니다" });
                }
                if !busy {
                    let me = self.clone();
                    let fetch = self.inner.lock().map(|i| i.auto).unwrap_or(false);
                    std::thread::spawn(move || me.look(fetch));
                }
                json!({ "ok": true })
            }
            "install" => {
                if !kind.installs() {
                    return json!({ "ok": false, "error": "이 KYNEX는 스스로 설치하지 못합니다: 릴리스 페이지에서 내려받으세요" });
                }
                if busy {
                    return json!({ "ok": false, "error": "업데이트를 확인하거나 내려받는 중입니다" });
                }
                let me = self.clone();
                std::thread::spawn(move || me.install_now());
                json!({ "ok": true })
            }
            _ => json!({ "ok": false, "error": "action: check, install, auto" }),
        }
    }

    /// The update fetched if it is not yet, then installed, KYNEX starting
    /// again after.
    fn install_now(&self) {
        let mut ready = match self
            .inner
            .lock()
            .map(|i| (i.state.clone(), i.latest.clone(), i.kind))
        {
            Ok(s) => s,
            Err(_) => return,
        };
        // Not looked yet: looked now.
        if ready.1.is_none() {
            self.set(State::Checking);
            match latest(ready.2) {
                Ok(release) => {
                    if let Ok(mut i) = self.inner.lock() {
                        i.latest = Some(release.clone());
                    }
                    ready.1 = Some(release);
                }
                Err(e) => return self.set(State::Failed(e)),
            }
        }
        let (path, kind) = match ready {
            (State::Ready(path), _, kind) => (path, kind),
            (_, Some(release), kind) if current().is_some_and(|now| release.version > now) => {
                self.set(State::Downloading);
                match download(&release, kind) {
                    Ok(path) => (path, kind),
                    Err(e) => return self.set(State::Failed(e)),
                }
            }
            _ => return self.set(State::Failed("설치할 새 버전이 없습니다".into())),
        };
        self.set(State::Installing);
        let why = install_and_exit(&path, kind, true);
        self.set(State::Failed(why));
    }

    fn to_json(&self) -> Value {
        let Ok(i) = self.inner.lock() else {
            return json!({});
        };
        let (state, error) = match &i.state {
            State::Idle => ("idle", None),
            State::Checking => ("checking", None),
            State::Latest => ("latest", None),
            State::Available => ("available", None),
            State::Downloading => ("downloading", None),
            State::Ready(_) => ("ready", None),
            State::Installing => ("installing", None),
            State::Failed(e) => ("error", Some(e.clone())),
        };
        json!({
            "current": env!("KYNEX_VERSION"),
            "latest": i.latest.as_ref().map(|r| r.version.to_string()),
            "page": i.latest.as_ref().map_or_else(
                || format!("https://github.com/{REPO}/releases/latest"),
                |r| r.page.clone(),
            ),
            "state": state,
            "error": error,
            "kind": i.kind.name(),
            "auto": i.auto,
        })
    }
}

/// `kynex update [--check]`: looks, and unless only checking, installs the
/// newer one at once.
pub fn run(args: Vec<String>) -> Result<(), String> {
    let check = match args.first().map(String::as_str) {
        None => false,
        Some("--check") => true,
        Some(other) => return Err(format!("unknown option {other:?}")),
    };
    let kind = kind();
    let now = env!("KYNEX_VERSION");
    println!("KYNEX {now} ({})", kind.name());
    let release = latest(kind)?;
    println!("최신 릴리스: {} ({})", release.version, release.page);
    if !current().is_some_and(|c| release.version > c) {
        println!("최신 버전입니다.");
        return Ok(());
    }
    if check {
        return Ok(());
    }
    if !kind.installs() {
        return Err(match kind {
            Kind::Dev => "개발 빌드는 업데이트하지 않습니다".into(),
            _ => format!(
                "이 KYNEX는 스스로 설치하지 못합니다. 내려받으세요: {}",
                release.page
            ),
        });
    }
    println!("내려받는 중…");
    let path = download(&release, kind)?;
    println!("설치합니다: {}", path.display());
    Err(install_and_exit(&path, kind, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_their_numbers() {
        assert_eq!(Version::parse("v0.1.194"), Some(Version(0, 1, 194)));
        assert_eq!(Version::parse("0.1.50-dev"), Some(Version(0, 1, 50)));
        assert_eq!(Version::parse("0.1"), None);
        assert_eq!(Version::parse("0.1.2.3"), None);
        assert_eq!(Version::parse("v0.x.1"), None);
        assert!(Version(0, 1, 200) > Version(0, 1, 99));
        assert!(Version(0, 2, 0) > Version(0, 1, 999));
        assert!(Version(1, 0, 0) > Version(0, 9, 9));
    }

    fn release_json() -> Value {
        json!({
            "tag_name": "v0.1.200",
            "html_url": "https://github.com/MinaryHub/Kynex/releases/tag/v0.1.200",
            "assets": [
                {"name": "KYNEX-0.1.200-setup.exe", "browser_download_url": "https://x/setup.exe", "size": 3, "digest": "sha256:AB"},
                {"name": "KYNEX-0.1.200-windows-x64.zip", "browser_download_url": "https://x/w.zip", "size": 4},
                {"name": "KYNEX-0.1.200-linux-x64.tar.gz", "browser_download_url": "https://x/l.tgz", "size": 5, "digest": "sha256:cd"},
                {"name": "KYNEX-0.1.200-macos-arm64.tar.gz", "browser_download_url": "https://x/m.tgz", "size": 6},
            ]
        })
    }

    /// The setup for a KYNEX installed by it; the system's own archive for
    /// one unpacked.
    #[test]
    fn the_file_for_how_kynex_was_installed() {
        let setup = parse_release(&release_json(), Kind::Setup).unwrap();
        assert_eq!(setup.version, Version(0, 1, 200));
        let a = setup.asset.unwrap();
        assert_eq!(a.name, "KYNEX-0.1.200-setup.exe");
        assert_eq!(a.sha256.as_deref(), Some("ab"));
        let portable = parse_release(&release_json(), Kind::Portable).unwrap();
        let name = portable.asset.map(|a| a.name);
        if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            assert_eq!(name.as_deref(), Some("KYNEX-0.1.200-linux-x64.tar.gz"));
        }
        if cfg!(all(windows, target_arch = "x86_64")) {
            assert_eq!(name.as_deref(), Some("KYNEX-0.1.200-windows-x64.zip"));
        }
        assert!(parse_release(&json!({}), Kind::Portable).is_err());
    }

    /// A file is taken only if its size and SHA-256 are the release's.
    #[test]
    fn a_download_is_checked() {
        let dir = std::env::temp_dir().join(format!("kynex-update-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("f");
        std::fs::write(&file, b"abc").unwrap();
        let sha = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert!(verify(&file, 3, Some(sha)).is_ok());
        assert!(verify(&file, 0, None).is_ok());
        assert!(verify(&file, 4, Some(sha)).is_err());
        assert!(verify(&file, 3, Some(&sha.replace('b', "c"))).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The programs out of a zip, wherever in it they are.
    #[test]
    fn programs_come_out_of_a_zip() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("kynex-unzip-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("KYNEX-0.1.200-windows-x64.zip");
        {
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&file).unwrap());
            let opts = zip::write::SimpleFileOptions::default();
            zip.start_file("kynex.exe", opts).unwrap();
            zip.write_all(b"new kynex").unwrap();
            zip.start_file("KYNEX-0.1.200/kynex-studio.exe", opts)
                .unwrap();
            zip.write_all(b"new studio").unwrap();
            zip.start_file("README.md", opts).unwrap();
            zip.write_all(b"readme").unwrap();
            zip.finish().unwrap();
        }
        let mut found = unpack(&file, &["kynex.exe", "kynex-studio.exe"]).unwrap();
        found.sort();
        assert_eq!(
            found,
            vec![
                ("kynex-studio.exe".to_string(), b"new studio".to_vec()),
                ("kynex.exe".to_string(), b"new kynex".to_vec()),
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
