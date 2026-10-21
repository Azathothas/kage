//! The registry index: every package kura holds and every release of
//! each, as `kage-kura`'s CI publishes it.

use std::collections::BTreeMap;

use semver::Version;
use serde::{Deserialize, Serialize};

use crate::Error;

/// The newest index schema this client reads.
pub const SCHEMA: u32 = 1;

/// A registry's index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Index {
    /// The index schema.
    pub schema: u32,
    /// Where release files are fetched from:
    /// `<base>/<commit>/plugins/<name>/<path>`.
    pub base: String,
    /// The registry commit the index was built from.
    pub commit: String,
    /// Every package, by name.
    pub packages: BTreeMap<String, Package>,
}

/// One package and its releases.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Package {
    /// One line on what the package does.
    #[serde(default)]
    pub description: Option<String>,
    /// Who wrote the package.
    #[serde(default)]
    pub authors: Option<Vec<String>>,
    /// An SPDX license expression.
    #[serde(default)]
    pub license: Option<String>,
    /// The package's home page.
    #[serde(default)]
    pub homepage: Option<String>,
    /// Where the package is developed.
    #[serde(default)]
    pub repository: Option<String>,
    /// Keywords to find the package by.
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// Every release, newest first.
    pub releases: Vec<Release>,
}

/// One release of a package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    /// The release's version.
    pub version: Version,
    /// The lowest `kage.api_version()` it needs.
    pub api: i64,
    /// Every capability it may request.
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// The registry commit that holds its files.
    pub commit: String,
    /// Why the release was withdrawn, when it was.
    #[serde(default)]
    pub yanked: Option<String>,
    /// The files kage installs.
    pub files: Vec<FileEntry>,
}

/// One installed file of a release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    /// The path inside the package directory.
    pub path: String,
    /// The file's sha256, lowercase hex.
    pub sha256: String,
    /// The file's size in bytes.
    pub size: u64,
}

impl Index {
    /// Parses an index.
    ///
    /// # Errors
    ///
    /// The text is not an index of a schema this client reads.
    pub fn parse(text: &[u8]) -> Result<Self, Error> {
        #[derive(Deserialize)]
        struct Head {
            schema: u32,
        }
        let head: Head = serde_json::from_slice(text)
            .map_err(|e| Error::Index(format!("not a kura index: {e}")))?;
        if head.schema > SCHEMA {
            return Err(Error::Index(format!(
                "the registry index uses schema {}, newer than this kage reads ({SCHEMA}); \
                 update kage",
                head.schema
            )));
        }
        serde_json::from_slice(text).map_err(|e| Error::Index(format!("not a kura index: {e}")))
    }

    /// The release of package `name` to install: `version` when given,
    /// else the newest that is not yanked and that this kage can load.
    ///
    /// # Errors
    ///
    /// No such package or version, the version is yanked or needs a
    /// newer kage, or no release fits.
    pub fn resolve(&self, name: &str, version: Option<&Version>) -> Result<&Release, Error> {
        let package = self
            .packages
            .get(name)
            .ok_or_else(|| Error::NotFound(name.to_owned()))?;
        let api = kage_plugin::api::API_VERSION;
        if let Some(version) = version {
            let release = package
                .releases
                .iter()
                .find(|release| &release.version == version)
                .ok_or_else(|| Error::NotFound(format!("{name}@{version}")))?;
            if let Some(reason) = &release.yanked {
                return Err(Error::Yanked {
                    release: format!("{name}@{version}"),
                    reason: reason.clone(),
                });
            }
            if release.api > api {
                return Err(Error::NeedsNewerKage {
                    release: format!("{name}@{version}"),
                    api: release.api,
                });
            }
            return Ok(release);
        }
        let usable = package.releases.iter().filter(|r| r.yanked.is_none());
        if let Some(release) = usable
            .clone()
            .filter(|release| release.api <= api)
            .max_by(|a, b| a.version.cmp(&b.version))
        {
            return Ok(release);
        }
        match usable.max_by(|a, b| a.version.cmp(&b.version)) {
            Some(newest) => Err(Error::NeedsNewerKage {
                release: format!("{name}@{}", newest.version),
                api: newest.api,
            }),
            None => Err(Error::NotFound(format!("{name} (every release is yanked)"))),
        }
    }

    /// Packages whose name, description or tags contain every word of
    /// `query`, case-insensitively, by name. An empty query matches all.
    #[must_use]
    pub fn search(&self, query: &str) -> Vec<(&str, &Package)> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        self.packages
            .iter()
            .filter(|(name, package)| {
                let text = format!(
                    "{name} {} {}",
                    package.description.as_deref().unwrap_or_default(),
                    package.tags.as_deref().unwrap_or_default().join(" ")
                )
                .to_lowercase();
                words.iter().all(|word| text.contains(word))
            })
            .map(|(name, package)| (name.as_str(), package))
            .collect()
    }
}

impl Package {
    /// The newest release that is not yanked, whatever its `api`.
    #[must_use]
    pub fn latest(&self) -> Option<&Release> {
        self.releases
            .iter()
            .filter(|release| release.yanked.is_none())
            .max_by(|a, b| a.version.cmp(&b.version))
    }
}
