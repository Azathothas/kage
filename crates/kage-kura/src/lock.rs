//! `kura.lock`: what the client installed into a plugins directory.
//!
//! The lock lives beside the plugins it describes, so a plugins
//! directory synced between machines carries it along. Each entry
//! records the release and the sha256 of every file written, which is
//! how the client tells its own packages from local ones and an
//! untouched package from one edited in place.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::Error;

/// The lock file's name in the plugins directory.
pub const LOCK_FILE: &str = "kura.lock";

/// The lock format this client writes.
const FORMAT: u32 = 1;

const HEADER: &str = "# Written by kage: the plugins installed from a registry.\n\
                      # Edit with `kage plugin`, not by hand.\n\n";

/// The lock of one plugins directory.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lock {
    /// The lock format.
    pub version: u32,
    /// Installed packages, by name.
    #[serde(default)]
    pub plugins: BTreeMap<String, Locked>,
}

/// One installed package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Locked {
    /// The installed release.
    pub version: Version,
    /// The index URL it came from.
    pub registry: String,
    /// The registry commit that held its files.
    pub commit: String,
    /// The sha256 of every file written, by path.
    pub files: BTreeMap<String, String>,
}

impl Lock {
    /// The lock of `plugins_dir`, empty when there is none.
    ///
    /// # Errors
    ///
    /// The file cannot be read, does not parse, or has a newer format.
    pub fn load(plugins_dir: &Path) -> Result<Self, Error> {
        let path = plugins_dir.join(LOCK_FILE);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self {
                    version: FORMAT,
                    ..Self::default()
                });
            }
            Err(err) => return Err(Error::Lock(format!("{}: {err}", path.display()))),
        };
        let lock: Self =
            toml::from_str(&text).map_err(|e| Error::Lock(format!("{}: {e}", path.display())))?;
        if lock.version > FORMAT {
            return Err(Error::Lock(format!(
                "{} has format {}, newer than this kage writes; update kage",
                path.display(),
                lock.version
            )));
        }
        Ok(lock)
    }

    /// Writes the lock into `plugins_dir`, or removes the file when the
    /// lock is empty.
    ///
    /// # Errors
    ///
    /// The file cannot be written or removed.
    pub fn save(&self, plugins_dir: &Path) -> Result<(), Error> {
        let path = plugins_dir.join(LOCK_FILE);
        if self.plugins.is_empty() {
            return match std::fs::remove_file(&path) {
                Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
                    Err(Error::Lock(format!("{}: {err}", path.display())))
                }
                _ => Ok(()),
            };
        }
        let lock = Self {
            version: FORMAT,
            plugins: self.plugins.clone(),
        };
        let body = toml::to_string(&lock).map_err(|e| Error::Lock(e.to_string()))?;
        kage_core::fsutil::atomic_write(&path, format!("{HEADER}{body}").as_bytes())
            .map_err(|e| Error::Lock(format!("{}: {e}", path.display())))
    }

    /// Drops `name` from the lock of `plugins_dir`, if it is there.
    ///
    /// # Errors
    ///
    /// The lock cannot be read or written.
    pub fn forget(plugins_dir: &Path, name: &str) -> Result<(), Error> {
        let mut lock = Self::load(plugins_dir)?;
        if lock.plugins.remove(name).is_some() {
            lock.save(plugins_dir)?;
        }
        Ok(())
    }
}

impl Locked {
    /// The files of the package at `dir` that differ from what was
    /// installed: changed, missing, or added under `lua/`.
    #[must_use]
    pub fn changed_files(&self, dir: &Path) -> Vec<String> {
        let mut changed: Vec<String> = self
            .files
            .iter()
            .filter(|(path, sha)| sha256_of(&dir.join(path)).as_deref() != Some(sha.as_str()))
            .map(|(path, _)| path.clone())
            .collect();
        for path in shipped_files(dir) {
            if !self.files.contains_key(&path) {
                changed.push(path);
            }
        }
        changed.sort();
        changed
    }
}

/// The sha256 of the file at `path`, lowercase hex.
pub(crate) fn sha256_of(path: &Path) -> Option<String> {
    std::fs::read(path).ok().map(|bytes| sha256_hex(&bytes))
}

/// The sha256 of `bytes`, lowercase hex.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// The paths, relative to `dir`, of the files a package ships:
/// `plugin.toml`, `init.lua` and everything under `lua/`.
pub(crate) fn shipped_files(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for top in [kage_plugin::package::MANIFEST, kage_plugin::package::ENTRY] {
        if dir.join(top).is_file() {
            out.push(top.to_owned());
        }
    }
    let mut stack: Vec<PathBuf> = vec![dir.join(kage_plugin::package::MODULES)];
    while let Some(next) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&next) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(rel) = path.strip_prefix(dir) {
                let rel: Vec<_> = rel
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy())
                    .collect();
                out.push(rel.join("/"));
            }
        }
    }
    out.sort();
    out
}
