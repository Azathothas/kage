//! Stamps `KAGE_VERSION`, the version every kage binary reports: the
//! crate version, or `nightly` for a nightly build, followed by the
//! commit a CI build was made from.

fn main() {
    println!("cargo:rerun-if-env-changed=KAGE_BUILD_SHA");
    println!("cargo:rerun-if-env-changed=KAGE_CHANNEL");
    let version = if std::env::var("KAGE_CHANNEL").as_deref() == Ok("nightly") {
        "nightly".to_owned()
    } else {
        std::env::var("CARGO_PKG_VERSION").expect("cargo sets the package version")
    };
    let version = match std::env::var("KAGE_BUILD_SHA") {
        Ok(sha) if !sha.is_empty() => format!("{version} ({})", &sha[..sha.len().min(7)]),
        _ => version,
    };
    println!("cargo:rustc-env=KAGE_VERSION={version}");
}
