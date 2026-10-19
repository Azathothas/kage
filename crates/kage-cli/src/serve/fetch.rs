//! `kage serve --fetch-web`: installs the web client bundle that
//! matches this build.
//!
//! Every release carries `kage-web.tar.xz` beside the `kage` archives,
//! at the same version, and the nightly carries the one built from the
//! same commit. The archive unpacks to a `web/` directory whose
//! `VERSION` file names the build, so serve can tell a stale bundle
//! from a current one.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Where release artifacts live.
const RELEASES_DOWNLOAD: &str = "https://github.com/QaidVoid/kage/releases/download";

/// The web client archive each release carries.
const ASSET: &str = "kage-web.tar.xz";

/// The most bytes the archive may weigh. It has stayed far under
/// this; the cap bounds a broken mirror's reply.
const ARCHIVE_CAP: u64 = 64 * 1024 * 1024;

/// The release the matching bundle ships in, and the identity its
/// `VERSION` file carries: the commit for a nightly build, the crate
/// version otherwise.
pub(crate) fn build() -> (String, &'static str) {
    match kage_core::build::COMMIT {
        Some(sha) if kage_core::build::NIGHTLY => ("nightly".to_owned(), sha),
        _ => (
            format!("v{}", env!("CARGO_PKG_VERSION")),
            env!("CARGO_PKG_VERSION"),
        ),
    }
}

/// The build the bundle in `dir` was made for, when it says.
pub(crate) fn installed(dir: &Path) -> Option<String> {
    std::fs::read_to_string(dir.join("VERSION"))
        .ok()
        .map(|text| text.trim().to_owned())
        .filter(|version| !version.is_empty())
}

/// Downloads the bundle matching this build and puts it at `dir`,
/// replacing whatever was there.
pub(crate) fn install(dir: &Path) -> Result<(), String> {
    let (tag, _) = build();
    let url = format!("{RELEASES_DOWNLOAD}/{tag}/{ASSET}");
    let bytes = download(&url)?;
    let parent = dir
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", dir.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    let scratch = parent.join(format!(".web-fetch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    let placed = unpack(&bytes, &scratch).and_then(|web| replace(&web, dir));
    let _ = std::fs::remove_dir_all(&scratch);
    placed
}

/// Gets `url`'s body, refusing past [`ARCHIVE_CAP`].
fn download(url: &str) -> Result<Vec<u8>, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .build()
        .into();
    let response = agent
        .get(url)
        .header("user-agent", concat!("kage/", env!("CARGO_PKG_VERSION")))
        .call()
        .map_err(|e| match e {
            ureq::Error::StatusCode(404) => format!("{url} was not found"),
            e => format!("get {url}: {e}"),
        })?;
    let mut bytes = Vec::new();
    response
        .into_body()
        .into_reader()
        .take(ARCHIVE_CAP)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("read {url}: {e}"))?;
    Ok(bytes)
}

/// Unpacks the archive under `scratch` and returns its `web/`
/// directory, refused when it holds no page.
fn unpack(bytes: &[u8], scratch: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(scratch)
        .map_err(|e| format!("cannot create {}: {e}", scratch.display()))?;
    let archive = scratch.join(ASSET);
    std::fs::write(&archive, bytes)
        .map_err(|e| format!("cannot write {}: {e}", archive.display()))?;
    compak::extract_archive(&archive, &scratch.to_path_buf())
        .map_err(|e| format!("cannot unpack {ASSET}: {e}"))?;
    let web = scratch.join("web");
    if !web.join("index.html").is_file() {
        return Err(format!("{ASSET} holds no web/index.html"));
    }
    Ok(web)
}

/// Moves the unpacked `web` to `dir`, the old bundle stepping aside
/// first so a failed move puts it back.
fn replace(web: &Path, dir: &Path) -> Result<(), String> {
    let mut aside = dir.as_os_str().to_owned();
    aside.push(".old");
    let aside = PathBuf::from(aside);
    let _ = std::fs::remove_dir_all(&aside);
    let moved = dir.exists();
    if moved {
        std::fs::rename(dir, &aside)
            .map_err(|e| format!("cannot move {} aside: {e}", dir.display()))?;
    }
    if let Err(e) = std::fs::rename(web, dir) {
        if moved {
            let _ = std::fs::rename(&aside, dir);
        }
        return Err(format!("cannot move the bundle to {}: {e}", dir.display()));
    }
    let _ = std::fs::remove_dir_all(&aside);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_release_build_reads_its_own_tag() {
        if !kage_core::build::NIGHTLY {
            let version = env!("CARGO_PKG_VERSION");
            assert_eq!(build(), (format!("v{version}"), version));
        }
    }

    #[test]
    fn the_installed_version_is_read_trimmed() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(installed(dir.path()), None);
        std::fs::write(dir.path().join("VERSION"), "0.4.0\n").unwrap();
        assert_eq!(installed(dir.path()).as_deref(), Some("0.4.0"));
    }

    #[cfg(unix)]
    #[test]
    fn a_release_archive_replaces_the_old_bundle() {
        let source = tempfile::tempdir().unwrap();
        let web = source.path().join("web");
        std::fs::create_dir_all(&web).unwrap();
        std::fs::write(web.join("index.html"), "<html>").unwrap();
        std::fs::write(web.join("VERSION"), "0.4.0\n").unwrap();
        let archive = source.path().join(ASSET);
        let built = std::process::Command::new("tar")
            .arg("-cJf")
            .arg(&archive)
            .arg("-C")
            .arg(source.path())
            .arg("web")
            .status()
            .expect("system tar");
        assert!(built.success());

        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("web");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("stale.js"), "old").unwrap();
        let scratch = root.path().join("scratch");
        let unpacked = unpack(&std::fs::read(&archive).unwrap(), &scratch).unwrap();
        replace(&unpacked, &dir).unwrap();

        assert_eq!(installed(&dir).as_deref(), Some("0.4.0"));
        assert!(dir.join("index.html").is_file());
        assert!(!dir.join("stale.js").exists(), "the old bundle is gone");
        assert!(!root.path().join("web.old").exists());
    }

    #[test]
    fn an_archive_without_a_page_is_refused() {
        let scratch = tempfile::tempdir().unwrap();
        let refused = unpack(b"not an archive", &scratch.path().join("s")).unwrap_err();
        assert!(refused.contains(ASSET), "{refused}");
    }
}
