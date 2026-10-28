//! The viewer's version, shown on its page (see `tools/version.rs`).

include!("../../tools/version.rs");

fn main() {
    println!("cargo:rerun-if-changed=../../tools/version.rs");
    let (version, commit) = kynex_version();
    println!("cargo:rustc-env=KYNEX_VERSION={version}");
    println!("cargo:rustc-env=KYNEX_COMMIT={commit}");
}
