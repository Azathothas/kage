//! Installing a release into a plugins directory, and reading back what
//! is installed there.

use std::collections::BTreeMap;
use std::path::Path;

use kage_plugin::package::{self, Found, Manifest};
use semver::Version;

use crate::lock::{Lock, Locked, sha256_hex};
use crate::transport::{Reply, Transport};
use crate::{Error, Index, Release};

/// The most files one package may install.
const MAX_FILES: usize = 256;

/// The most bytes one package may install.
const MAX_BYTES: u64 = 4 * 1024 * 1024;

/// What an install did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    /// The package name.
    pub name: String,
    /// The version now installed.
    pub version: Version,
    /// The version it replaced, when one was installed from a registry.
    pub previous: Option<Version>,
    /// Every capability the package may request. None is granted.
    pub capabilities: Vec<String>,
}

pub(crate) fn install(
    transport: &dyn Transport,
    registry: &str,
    plugins_dir: &Path,
    index: &Index,
    name: &str,
    release: &Release,
    force: bool,
) -> Result<Installed, Error> {
    package::check_name(name).map_err(Error::Integrity)?;
    check_commit(&release.commit)?;
    let mut lock = Lock::load(plugins_dir)?;
    let previous = lock.plugins.get(name);
    check_target(plugins_dir, name, previous, force)?;
    let files = download(transport, index, name, release)?;
    check_manifest(name, release, &files)?;

    let io = |what: &str, path: &Path, err: std::io::Error| {
        Error::Io(format!("{what} {}: {err}", path.display()))
    };
    std::fs::create_dir_all(plugins_dir).map_err(|e| io("create", plugins_dir, e))?;
    let target = plugins_dir.join(name);
    let staging = plugins_dir.join(format!(".{name}.kura-new"));
    let old = plugins_dir.join(format!(".{name}.kura-old"));
    remove_dir(&staging).map_err(|e| io("clear", &staging, e))?;
    remove_dir(&old).map_err(|e| io("clear", &old, e))?;
    for (rel, bytes) in &files {
        let path = staging.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| io("create", parent, e))?;
        }
        std::fs::write(&path, bytes).map_err(|e| io("write", &path, e))?;
    }
    if force {
        let script = plugins_dir.join(format!("{name}.lua"));
        if script.is_file() {
            std::fs::remove_file(&script).map_err(|e| io("remove", &script, e))?;
        }
    }
    let replacing = target.is_dir();
    if replacing {
        std::fs::rename(&target, &old).map_err(|e| io("move aside", &target, e))?;
    }
    if let Err(err) = std::fs::rename(&staging, &target) {
        if replacing {
            let _ = std::fs::rename(&old, &target);
        }
        let _ = remove_dir(&staging);
        return Err(io("move into place", &target, err));
    }
    let _ = remove_dir(&old);

    let previous = previous.map(|locked| locked.version.clone());
    lock.plugins.insert(
        name.to_owned(),
        Locked {
            version: release.version.clone(),
            registry: registry.to_owned(),
            commit: release.commit.clone(),
            files: files
                .iter()
                .map(|(rel, bytes)| (rel.clone(), sha256_hex(bytes)))
                .collect(),
        },
    );
    lock.save(plugins_dir)?;
    Ok(Installed {
        name: name.to_owned(),
        version: release.version.clone(),
        previous,
        capabilities: release.capabilities.clone(),
    })
}

/// Refuses an install that would clobber something the user owns.
fn check_target(
    plugins_dir: &Path,
    name: &str,
    locked: Option<&Locked>,
    force: bool,
) -> Result<(), Error> {
    let target = plugins_dir.join(name);
    if target
        .symlink_metadata()
        .is_ok_and(|meta| meta.file_type().is_symlink())
    {
        return Err(Error::Conflict(format!(
            "{name} is a link to a local checkout; remove the link to install from the registry"
        )));
    }
    if force {
        return Ok(());
    }
    if plugins_dir.join(format!("{name}.lua")).is_file() {
        return Err(Error::Conflict(format!(
            "a script named {name}.lua is installed; remove it, or force the install to \
             replace it"
        )));
    }
    if target.exists() {
        let Some(locked) = locked else {
            return Err(Error::Conflict(format!(
                "a local package named {name} is installed; force the install to replace it"
            )));
        };
        let changed = locked.changed_files(&target);
        if !changed.is_empty() {
            return Err(Error::Conflict(format!(
                "{name} has local changes ({}); force the install to discard them",
                changed.join(", ")
            )));
        }
    }
    Ok(())
}

/// Every file of `release`, downloaded and checked against the index.
fn download(
    transport: &dyn Transport,
    index: &Index,
    name: &str,
    release: &Release,
) -> Result<BTreeMap<String, Vec<u8>>, Error> {
    if release.files.len() > MAX_FILES {
        return Err(Error::Integrity(format!(
            "{name} {} lists {} files; the limit is {MAX_FILES}",
            release.version,
            release.files.len()
        )));
    }
    let total: u64 = release.files.iter().map(|file| file.size).sum();
    if total > MAX_BYTES {
        return Err(Error::Integrity(format!(
            "{name} {} is {total} bytes; the limit is {MAX_BYTES}",
            release.version
        )));
    }
    let base = index.base.trim_end_matches('/');
    let mut files = BTreeMap::new();
    for entry in &release.files {
        check_path(&entry.path)?;
        let url = format!("{base}/{}/plugins/{name}/{}", release.commit, entry.path);
        let bytes = match transport
            .get(&url, None, entry.size)
            .map_err(Error::Network)?
        {
            Reply::Body { bytes, .. } => bytes,
            Reply::NotModified => {
                return Err(Error::Network(format!("{url}: unexpected 304")));
            }
        };
        if bytes.len() as u64 != entry.size || sha256_hex(&bytes) != entry.sha256 {
            return Err(Error::Integrity(format!(
                "{name} {}: {} does not match the index; nothing was installed",
                release.version, entry.path
            )));
        }
        if files.insert(entry.path.clone(), bytes).is_some() {
            return Err(Error::Integrity(format!(
                "{name} {} lists {} twice",
                release.version, entry.path
            )));
        }
    }
    Ok(files)
}

/// The downloaded manifest must be the release the index describes,
/// and loadable by this kage.
fn check_manifest(
    name: &str,
    release: &Release,
    files: &BTreeMap<String, Vec<u8>>,
) -> Result<(), Error> {
    if !files.contains_key(package::ENTRY) {
        return Err(Error::Integrity(format!(
            "{name} {} has no {}",
            release.version,
            package::ENTRY
        )));
    }
    let text = files
        .get(package::MANIFEST)
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .ok_or_else(|| {
            Error::Integrity(format!(
                "{name} {} has no readable {}",
                release.version,
                package::MANIFEST
            ))
        })?;
    let manifest = Manifest::parse(text).map_err(Error::Integrity)?;
    if manifest.name != name || manifest.version != release.version {
        return Err(Error::Integrity(format!(
            "{name} {}: its plugin.toml says {} {}",
            release.version, manifest.name, manifest.version
        )));
    }
    if manifest.api > kage_plugin::api::API_VERSION {
        return Err(Error::NeedsNewerKage {
            release: format!("{name}@{}", release.version),
            api: manifest.api,
        });
    }
    Ok(())
}

/// A path the index may name: `plugin.toml`, `init.lua`, or a `.lua`
/// module under `lua/`, with plain segments only.
fn check_path(path: &str) -> Result<(), Error> {
    let plain = path.split('/').all(|segment| {
        !segment.is_empty()
            && segment != "."
            && segment != ".."
            && segment
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
    });
    let shipped = path == package::MANIFEST
        || path == package::ENTRY
        || path.starts_with("lua/") && Path::new(path).extension().is_some_and(|ext| ext == "lua");
    if plain && shipped {
        Ok(())
    } else {
        Err(Error::Integrity(format!(
            "the index names a file kage does not install: {path}"
        )))
    }
}

fn check_commit(commit: &str) -> Result<(), Error> {
    let hex = commit.bytes().all(|b| b.is_ascii_hexdigit());
    if hex && matches!(commit.len(), 40 | 64) {
        Ok(())
    } else {
        Err(Error::Integrity(format!(
            "the index names a commit that is not a hash: {commit}"
        )))
    }
}

fn remove_dir(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_dir_all(path) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err),
        _ => Ok(()),
    }
}

/// Removes plugin `name` from `plugins_dir`: its script, its package
/// directory, or both, and its lock entry. A symlinked package loses
/// only its link, never the checkout it points at. Returns whether
/// anything was there.
///
/// # Errors
///
/// A file cannot be removed, or the lock cannot be updated.
pub fn remove(plugins_dir: &Path, name: &str) -> Result<bool, Error> {
    package::check_name(name).map_err(Error::Conflict)?;
    let io =
        |path: &Path, err: std::io::Error| Error::Io(format!("remove {}: {err}", path.display()));
    let script = plugins_dir.join(format!("{name}.lua"));
    let dir = plugins_dir.join(name);
    let is_package = dir.join(package::MANIFEST).is_file() || dir.join(package::ENTRY).is_file();
    let mut found = false;
    if script.is_file() {
        std::fs::remove_file(&script).map_err(|e| io(&script, e))?;
        found = true;
    }
    if is_package {
        let linked = dir
            .symlink_metadata()
            .is_ok_and(|meta| meta.file_type().is_symlink());
        if linked {
            std::fs::remove_file(&dir)
                .or_else(|_| std::fs::remove_dir(&dir))
                .map_err(|e| io(&dir, e))?;
        } else {
            std::fs::remove_dir_all(&dir).map_err(|e| io(&dir, e))?;
        }
        found = true;
    }
    Lock::forget(plugins_dir, name)?;
    Ok(found)
}

/// A plugin's kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A `<name>.lua` file.
    Script,
    /// A `<name>/` package.
    Package,
}

/// One installed plugin as the registry sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginStatus {
    /// The plugin name.
    pub name: String,
    /// Script or package.
    pub kind: Kind,
    /// The package's own version, from its manifest.
    pub version: Option<Version>,
    /// Its lock entry, when it came from a registry.
    pub locked: Option<Locked>,
    /// Files changed since it was installed from a registry.
    pub changed: Vec<String>,
    /// A newer release this kage can load, when the index has one.
    pub update: Option<Version>,
    /// Why the installed release was withdrawn, when it was.
    pub yanked: Option<String>,
}

/// Every plugin in `plugins_dir`, with what `index` says about the ones
/// a registry installed.
///
/// # Errors
///
/// The directory or its lock cannot be read.
pub fn status(plugins_dir: &Path, index: Option<&Index>) -> Result<Vec<PluginStatus>, Error> {
    let lock = Lock::load(plugins_dir)?;
    let found = package::discover(plugins_dir)
        .map_err(|e| Error::Io(format!("{}: {e}", plugins_dir.display())))?;
    let mut out = Vec::new();
    for entry in found {
        let (name, kind, dir) = match entry {
            Found::Script { name, .. } => (name, Kind::Script, None),
            Found::Package { name, dir } => (name, Kind::Package, Some(dir)),
            Found::Invalid { .. } => continue,
        };
        let version = dir
            .as_deref()
            .and_then(|dir| Manifest::read(dir).ok())
            .map(|manifest| manifest.version);
        let locked = dir.as_ref().and_then(|_| lock.plugins.get(&name).cloned());
        let changed = match (&locked, &dir) {
            (Some(locked), Some(dir)) => locked.changed_files(dir),
            _ => Vec::new(),
        };
        let package = locked.as_ref().and_then(|_| index?.packages.get(&name));
        let update = package
            .and_then(|_| index?.resolve(&name, None).ok())
            .filter(|release| locked.as_ref().is_some_and(|l| release.version > l.version))
            .map(|release| release.version.clone());
        let yanked = package.and_then(|package| {
            let installed = &locked.as_ref()?.version;
            let release = package.releases.iter().find(|r| &r.version == installed)?;
            release.yanked.clone()
        });
        out.push(PluginStatus {
            name,
            kind,
            version,
            locked,
            changed,
            update,
            yanked,
        });
    }
    Ok(out)
}
