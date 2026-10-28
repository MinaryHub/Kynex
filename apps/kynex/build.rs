//! The programs' version (see `tools/version.rs`), and their icon and
//! version as Windows shows them (`assets/kynex.rc`).

include!("../../tools/version.rs");

fn main() {
    println!("cargo:rerun-if-changed=assets/kynex.rc");
    println!("cargo:rerun-if-changed=assets/kynex.ico");
    println!("cargo:rerun-if-changed=../../tools/version.rs");
    let (version, commit) = kynex_version();
    println!("cargo:rustc-env=KYNEX_VERSION={version}");
    println!("cargo:rustc-env=KYNEX_COMMIT={commit}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut parts: Vec<String> = version
        .split(['.', '-', '+'])
        .take(3)
        .map(|p| p.parse::<u16>().unwrap_or(0).to_string())
        .collect();
    parts.resize(3, "0".into());
    parts.push("0".into());
    let commas = format!("KYNEX_VERSION_COMMAS={}", parts.join(","));
    let quoted = format!("KYNEX_VERSION=\"{version}\"");
    embed_resource::compile("assets/kynex.rc", [commas.as_str(), quoted.as_str()])
        .manifest_optional()
        .expect("the programs' resources");
}
