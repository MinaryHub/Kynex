// KYNEX's version, for the build scripts that include it (`include!`):
// the workspace's major.minor, and a build number that goes up with every
// commit — the commits on the way to this one, the same on every machine
// and in CI. `KYNEX_BUILD` in the environment gives the number instead.
// Built from a tree with changes not committed, `-dev` after it: not a
// build anyone else has.
//
// Returns the version and the commit it was built from (short), or "".

fn kynex_version() -> (String, String) {
    let root = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(&root)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    // Built again when the commit changes, or what is staged.
    if let Some(dir) = git(&["rev-parse", "--absolute-git-dir"]) {
        for f in ["HEAD", "index", "refs/heads"] {
            println!("cargo:rerun-if-changed={dir}/{f}");
        }
    }
    println!("cargo:rerun-if-env-changed=KYNEX_BUILD");
    let base = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.1.0".into());
    let mut parts = base.split('.');
    let (major, minor) = (parts.next().unwrap_or("0"), parts.next().unwrap_or("1"));
    let given = std::env::var("KYNEX_BUILD").ok().filter(|b| !b.is_empty());
    let build = given
        .clone()
        .or_else(|| git(&["rev-list", "--count", "HEAD"]))
        .unwrap_or_else(|| "0".into());
    let dirty = given.is_none()
        && git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
    let commit = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_default();
    (
        format!("{major}.{minor}.{build}{}", if dirty { "-dev" } else { "" }),
        commit,
    )
}
