//! Client for kura, the kage plugin registry.
//!
//! Layering: depends on `kage-core`, `kage-plugin` and `kage-tools`.
//!
//! A [`Registry`] reads the registry's [`Index`], cached on disk and
//! revalidated by entity tag, and installs a release of a package into a
//! plugins directory. An install downloads every file the index lists,
//! checks its size and sha256 and the package's manifest, stages the
//! package in a dot directory the plugin loader ignores, and renames it
//! into place, so a reload never sees half a package. What was installed
//! is recorded in the directory's [`Lock`].
//!
//! The client never grants capabilities. An install reports what the
//! package declares, and the host asks the user.

pub mod index;
mod install;
pub mod lock;
pub mod transport;

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub use index::{FileEntry, Index, Package, Release};
pub use install::{Installed, Kind, PluginStatus, remove, status};
pub use lock::{LOCK_FILE, Lock, Locked};
pub use transport::{Http, Reply, Transport};

/// The registry kage reads unless `[plugins] registry` names another.
pub const DEFAULT_REGISTRY: &str = "https://kura.kage.qaidvoid.dev/";

/// The largest index read.
const INDEX_MAX_BYTES: u64 = 16 * 1024 * 1024;

/// How long a cached index is used before it is revalidated.
const INDEX_TTL: Duration = Duration::from_secs(60 * 60);

/// What can go wrong between kage and a registry.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The registry could not be reached.
    #[error("{0}")]
    Network(String),
    /// The index is missing, malformed or too new.
    #[error("{0}")]
    Index(String),
    /// No such package, or no such release of it.
    #[error("{0} is not in the registry")]
    NotFound(String),
    /// The release was withdrawn.
    #[error("{release} is yanked: {reason}")]
    Yanked {
        /// `name@version`.
        release: String,
        /// Why it was withdrawn.
        reason: String,
    },
    /// The release needs a newer plugin API than this kage provides.
    #[error(
        "{release} needs plugin api {api}, and this kage provides {}; update kage",
        kage_plugin::api::API_VERSION
    )]
    NeedsNewerKage {
        /// `name@version`.
        release: String,
        /// The api it needs.
        api: i64,
    },
    /// A downloaded file or the package does not match the index.
    #[error("{0}")]
    Integrity(String),
    /// Something already in the plugins directory is in the way.
    #[error("{0}")]
    Conflict(String),
    /// The lock file cannot be read or written.
    #[error("{0}")]
    Lock(String),
    /// A file in the plugins directory cannot be written.
    #[error("{0}")]
    Io(String),
}

/// An index, and why it may be out of date.
#[derive(Debug, Clone)]
pub struct Fetched {
    /// The index.
    pub index: Index,
    /// Set when the registry could not be reached and a cached index
    /// stands in: the reason.
    pub stale: Option<String>,
}

/// One registry: its index URL, where its index is cached, and how it
/// is reached.
pub struct Registry {
    url: String,
    cache_dir: Option<PathBuf>,
    transport: Box<dyn Transport>,
}

impl std::fmt::Debug for Registry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Registry")
            .field("url", &self.url)
            .field("cache_dir", &self.cache_dir)
            .finish_non_exhaustive()
    }
}

impl Registry {
    /// The registry at `url`, over HTTPS, caching its index in
    /// `cache_dir` when given.
    #[must_use]
    pub fn new(url: impl Into<String>, cache_dir: Option<PathBuf>) -> Self {
        Self::with_transport(url, cache_dir, Box::new(Http::default()))
    }

    /// [`Self::new`] over another transport.
    #[must_use]
    pub fn with_transport(
        url: impl Into<String>,
        cache_dir: Option<PathBuf>,
        transport: Box<dyn Transport>,
    ) -> Self {
        Self {
            url: url.into(),
            cache_dir,
            transport,
        }
    }

    /// The index URL.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The registry's index. A cached copy younger than an hour is used
    /// as is unless `refresh`; an older one is revalidated, and stands in
    /// when the registry cannot be reached.
    ///
    /// # Errors
    ///
    /// The registry cannot be reached and nothing is cached, or the
    /// index does not parse.
    pub fn index(&self, refresh: bool) -> Result<Fetched, Error> {
        let cache = self.cache_paths();
        let cached = cache.as_ref().and_then(|(body, etag)| {
            let bytes = std::fs::read(body).ok()?;
            let index = Index::parse(&bytes).ok()?;
            let age = std::fs::metadata(body)
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|at| SystemTime::now().duration_since(at).ok())
                .unwrap_or(Duration::MAX);
            let etag = std::fs::read_to_string(etag).ok();
            Some((index, age, etag))
        });
        if let Some((index, age, _)) = &cached
            && !refresh
            && *age < INDEX_TTL
        {
            return Ok(Fetched {
                index: index.clone(),
                stale: None,
            });
        }
        let etag = cached.as_ref().and_then(|(_, _, etag)| etag.as_deref());
        match self.transport.get(&self.url, etag, INDEX_MAX_BYTES) {
            Ok(Reply::NotModified) => {
                let (index, ..) = cached.ok_or_else(|| {
                    Error::Index("the registry answered 304 to an unconditional request".into())
                })?;
                if let Some((body, _)) = &cache {
                    touch(body);
                }
                Ok(Fetched { index, stale: None })
            }
            Ok(Reply::Body { bytes, etag }) => {
                let index = Index::parse(&bytes)?;
                if let Some((body, etag_path)) = &cache {
                    store_cache(body, etag_path, &bytes, etag.as_deref());
                }
                Ok(Fetched { index, stale: None })
            }
            Err(err) => match cached {
                Some((index, ..)) => Ok(Fetched {
                    index,
                    stale: Some(err),
                }),
                None => Err(Error::Network(err)),
            },
        }
    }

    /// Installs `release` of package `name` from `index` into
    /// `plugins_dir`. `force` replaces a script of the same name, a local
    /// package, or an installed one with local changes; a symlinked
    /// package is never replaced.
    ///
    /// # Errors
    ///
    /// Something is in the way (see above), a download fails or does
    /// not match the index, or the directory cannot be written.
    pub fn install(
        &self,
        plugins_dir: &Path,
        index: &Index,
        name: &str,
        release: &Release,
        force: bool,
    ) -> Result<Installed, Error> {
        install::install(
            self.transport.as_ref(),
            &self.url,
            plugins_dir,
            index,
            name,
            release,
            force,
        )
    }

    /// The index body and etag files, named after the URL so two
    /// registries never share a cache.
    fn cache_paths(&self) -> Option<(PathBuf, PathBuf)> {
        let dir = self.cache_dir.as_ref()?;
        let key = &lock::sha256_hex(self.url.as_bytes())[..16];
        Some((
            dir.join(format!("{key}.json")),
            dir.join(format!("{key}.etag")),
        ))
    }
}

/// Marks a revalidated cache as fresh again. A failure only means the
/// next read revalidates once more.
fn touch(path: &Path) {
    if let Ok(file) = std::fs::File::options().append(true).open(path) {
        let _ = file.set_modified(SystemTime::now());
    }
}

/// Stores a fetched index. The cache is an optimization, so a failure to
/// write it is not an error.
fn store_cache(body: &Path, etag_path: &Path, bytes: &[u8], etag: Option<&str>) {
    if let Some(dir) = body.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = kage_core::fsutil::atomic_write(body, bytes);
    match etag {
        Some(etag) => drop(kage_core::fsutil::atomic_write(etag_path, etag.as_bytes())),
        None => drop(std::fs::remove_file(etag_path)),
    }
}
