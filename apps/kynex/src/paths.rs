//! Where KYNEX keeps things on this computer: its own settings for this
//! user (`%APPDATA%\kynex`, or `~/.config/kynex`), and the user's projects
//! (`Documents\KYNEX`). Never the folder KYNEX is installed in, which an
//! installed program cannot write to.

use std::path::{Path, PathBuf};

/// The file a project starts as when there is none yet.
pub const NEW_PROJECT: &str = "새 프로젝트.json";

/// The project file KYNEX used before it kept projects in the projects
/// folder: in the folder it was started in.
const OLD_PROJECT: &str = "kynex-project.json";

/// How many recent projects are kept.
const RECENT: usize = 10;

/// KYNEX's own settings for this user.
pub fn app_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("kynex")
}

/// Where projects are made, opened and listed: `KYNEX` in the user's
/// documents, or `KYNEX_PROJECTS` if set.
pub fn projects_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("KYNEX_PROJECTS") {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    home.join("Documents").join("KYNEX")
}

/// The projects opened last, the latest first: those still there.
pub fn recent() -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(app_dir().join("recent.json")) else {
        return Vec::new();
    };
    let list: Vec<String> = serde_json::from_str(&text).unwrap_or_default();
    list.into_iter()
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .collect()
}

/// Project `file` opened now: first among the recent. Not while testing,
/// which must leave the user's settings alone.
pub fn remember(file: &Path) {
    if cfg!(test) {
        return;
    }
    let file = std::path::absolute(file).unwrap_or_else(|_| file.to_path_buf());
    let mut list = recent();
    list.retain(|p| *p != file);
    list.insert(0, file);
    list.truncate(RECENT);
    let text: Vec<String> = list.iter().map(|p| p.display().to_string()).collect();
    let dir = app_dir();
    if std::fs::create_dir_all(&dir).is_ok() {
        let _ = std::fs::write(
            dir.join("recent.json"),
            serde_json::to_string_pretty(&text).unwrap_or_default(),
        );
    }
}

/// Project `file` held for this KYNEX: two writing one file would undo each
/// other's work. A file beside it (`<project>.lock`) kept open with no
/// sharing while the project is: another KYNEX cannot open it, and the
/// system lets it go when this one ends, however it ends. Where files are
/// not locked so (not Windows), nothing is held.
pub fn hold(file: &Path) -> Result<Option<std::fs::File>, String> {
    let lock = {
        let mut name = file.as_os_str().to_owned();
        name.push(".lock");
        PathBuf::from(name)
    };
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    match options.open(&lock) {
        Ok(mut f) => {
            let _ = std::io::Write::write_all(&mut f, std::process::id().to_string().as_bytes());
            Ok(cfg!(windows).then_some(f))
        }
        Err(e) if e.raw_os_error() == Some(32) => Err(format!(
            "{}: 다른 KYNEX가 이 프로젝트를 열고 있습니다",
            file.display()
        )),
        Err(e) => Err(format!("{}: {e}", lock.display())),
    }
}

/// The project to open when none is named: the last opened; else one left
/// in the folder KYNEX was started in by an older KYNEX; else a new one in
/// the projects folder.
pub fn default_project() -> PathBuf {
    if let Some(last) = recent().into_iter().next() {
        return last;
    }
    if Path::new(OLD_PROJECT).is_file() {
        return PathBuf::from(OLD_PROJECT);
    }
    projects_dir().join(NEW_PROJECT)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A project held by one KYNEX is not opened by another, until let go.
    #[test]
    #[cfg(windows)]
    fn a_project_open_here_is_not_opened_again() {
        let dir = std::env::temp_dir().join(format!("kynex-hold-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("셀.json");
        let held = hold(&file).unwrap();
        assert!(held.is_some());
        let again = hold(&file);
        assert!(again.is_err(), "held twice");
        drop(held);
        assert!(hold(&file).is_ok(), "let go, taken again");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
