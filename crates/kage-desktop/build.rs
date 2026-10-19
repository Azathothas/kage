//! Stamps `KAGE_VERSION`, the version the About page shows: the crate
//! version, or `nightly` for a nightly build, followed by the commit a
//! CI build was made from. On Windows it also embeds the executable
//! icon for Explorer, the taskbar and Alt-Tab.

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

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    println!("cargo:rerun-if-changed=assets/kage.ico");
    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("assets/kage.ico");
    resource.compile().expect("the Windows icon embeds");
}
