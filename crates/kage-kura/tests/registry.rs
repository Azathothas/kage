//! The client against an in-memory registry.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use kage_kura::{
    Error, FileEntry, Index, Kind, LOCK_FILE, Lock, Package, Registry, Release, Reply, Transport,
};
use semver::Version;
use sha2::{Digest as _, Sha256};

const INDEX_URL: &str = "https://kura.test/";
const BASE: &str = "https://raw.test/kura";

/// The index body and its etag.
type Served = Option<(Vec<u8>, String)>;

/// Each request's URL and the etag it sent.
type Calls = Vec<(String, Option<String>)>;

/// Serves files by URL and the index with an etag, recording requests.
#[derive(Clone, Default)]
struct Fake {
    files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    index: Arc<Mutex<Served>>,
    calls: Arc<Mutex<Calls>>,
    down: Arc<Mutex<bool>>,
}

impl Transport for Fake {
    fn get(&self, url: &str, etag: Option<&str>, max_bytes: u64) -> Result<Reply, String> {
        self.calls
            .lock()
            .unwrap()
            .push((url.to_owned(), etag.map(str::to_owned)));
        if *self.down.lock().unwrap() {
            return Err("unreachable".to_owned());
        }
        if url == INDEX_URL {
            let index = self.index.lock().unwrap().clone().ok_or("no index")?;
            if etag == Some(index.1.as_str()) {
                return Ok(Reply::NotModified);
            }
            return Ok(Reply::Body {
                bytes: index.0,
                etag: Some(index.1),
            });
        }
        let bytes = self.files.lock().unwrap().get(url).cloned().ok_or("404")?;
        if bytes.len() as u64 > max_bytes {
            return Err("too large".to_owned());
        }
        Ok(Reply::Body { bytes, etag: None })
    }
}

impl Fake {
    fn calls_to(&self, url: &str) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(u, _)| u == url)
            .count()
    }
}

fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn manifest(name: &str, version: &str, api: i64, caps: &[&str]) -> String {
    let caps: Vec<String> = caps.iter().map(|c| format!("\"{c}\"")).collect();
    format!(
        "name = \"{name}\"\nversion = \"{version}\"\napi = {api}\ncapabilities = [{}]\n",
        caps.join(", ")
    )
}

/// A release of `name` whose files the fake serves at `commit`.
fn release(
    fake: &Fake,
    name: &str,
    version: &str,
    commit: char,
    files: &[(&str, &str)],
    caps: &[&str],
) -> Release {
    let commit: String = std::iter::repeat_n(commit, 40).collect();
    let mut entries = Vec::new();
    for (path, body) in files {
        let url = format!("{BASE}/{commit}/plugins/{name}/{path}");
        fake.files
            .lock()
            .unwrap()
            .insert(url, body.as_bytes().to_vec());
        entries.push(FileEntry {
            path: (*path).to_owned(),
            sha256: sha(body.as_bytes()),
            size: body.len() as u64,
        });
    }
    Release {
        version: Version::parse(version).unwrap(),
        api: 3,
        capabilities: caps.iter().map(|c| (*c).to_owned()).collect(),
        commit,
        yanked: None,
        files: entries,
    }
}

fn web(fake: &Fake, version: &str, commit: char, module: Option<&str>) -> Release {
    let toml = manifest("web", version, 3, &["net"]);
    let mut files = vec![
        ("plugin.toml", toml.as_str()),
        ("init.lua", "require('web.util')"),
    ];
    if let Some(module) = module {
        files.push(("lua/web/util.lua", module));
    }
    release(fake, "web", version, commit, &files, &["net"])
}

fn index(packages: Vec<(&str, Vec<Release>)>) -> Index {
    Index {
        schema: 1,
        base: BASE.to_owned(),
        commit: "f".repeat(40),
        packages: packages
            .into_iter()
            .map(|(name, mut releases)| {
                releases.sort_by(|a, b| b.version.cmp(&a.version));
                let package = Package {
                    description: Some(format!("the {name} package")),
                    authors: None,
                    license: None,
                    homepage: None,
                    repository: None,
                    tags: Some(vec!["web".to_owned()]),
                    releases,
                };
                (name.to_owned(), package)
            })
            .collect(),
    }
}

fn registry(fake: &Fake, cache: Option<&Path>) -> Registry {
    Registry::with_transport(
        INDEX_URL,
        cache.map(Path::to_path_buf),
        Box::new(fake.clone()),
    )
}

fn v(text: &str) -> Version {
    Version::parse(text).unwrap()
}

#[test]
fn resolve_takes_the_newest_loadable_release() {
    let fake = Fake::default();
    let mut newer = web(&fake, "0.3.0", 'c', Some("return {}"));
    newer.api = 99;
    let mut yanked = web(&fake, "0.2.0", 'b', Some("return {}"));
    yanked.yanked = Some("leaks".to_owned());
    let old = web(&fake, "0.1.0", 'a', Some("return {}"));
    let index = index(vec![("web", vec![newer, yanked, old])]);
    assert_eq!(index.resolve("web", None).unwrap().version, v("0.1.0"));
    let err = index.resolve("web", Some(&v("0.2.0"))).unwrap_err();
    assert!(matches!(err, Error::Yanked { .. }), "{err}");
    let err = index.resolve("web", Some(&v("0.3.0"))).unwrap_err();
    assert!(err.to_string().contains("needs plugin api 99"), "{err}");
    assert!(matches!(
        index.resolve("nope", None),
        Err(Error::NotFound(_))
    ));
    assert_eq!(index.search("WEB pack").len(), 1);
    assert!(index.search("mail").is_empty());
}

#[test]
fn a_newer_index_schema_is_refused() {
    let err = Index::parse(br#"{"schema": 2}"#).unwrap_err();
    assert!(err.to_string().contains("update kage"), "{err}");
}

#[test]
fn an_install_verifies_writes_and_locks_the_package() {
    let fake = Fake::default();
    let release = web(&fake, "0.1.0", 'a', Some("return {}"));
    let index = index(vec![("web", vec![release.clone()])]);
    let plugins = tempfile::tempdir().unwrap();
    let installed = registry(&fake, None)
        .install(plugins.path(), &index, "web", &release, false)
        .unwrap();
    assert_eq!(installed.version, v("0.1.0"));
    assert_eq!(installed.previous, None);
    assert_eq!(installed.capabilities, ["net"]);
    let dir = plugins.path().join("web");
    assert!(dir.join("lua/web/util.lua").is_file());
    let names: Vec<_> = std::fs::read_dir(plugins.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    assert_eq!(names, [LOCK_FILE, "web"], "no staging left behind");
    let lock = Lock::load(plugins.path()).unwrap();
    let locked = &lock.plugins["web"];
    assert_eq!(locked.version, v("0.1.0"));
    assert_eq!(locked.registry, INDEX_URL);
    assert_eq!(locked.files.len(), 3);
    assert!(locked.changed_files(&dir).is_empty());
    let found = kage_plugin::package::discover(plugins.path()).unwrap();
    assert!(
        matches!(&found[0], kage_plugin::package::Found::Package { name, .. } if name == "web")
    );
}

#[test]
fn a_tampered_file_installs_nothing() {
    let fake = Fake::default();
    let release = web(&fake, "0.1.0", 'a', Some("return {}"));
    let url = format!("{BASE}/{}/plugins/web/init.lua", release.commit);
    fake.files
        .lock()
        .unwrap()
        .insert(url, b"require('evil')!!".to_vec());
    let index = index(vec![("web", vec![release.clone()])]);
    let plugins = tempfile::tempdir().unwrap();
    let err = registry(&fake, None)
        .install(plugins.path(), &index, "web", &release, false)
        .unwrap_err();
    assert!(matches!(err, Error::Integrity(_)), "{err}");
    assert_eq!(std::fs::read_dir(plugins.path()).unwrap().count(), 0);
}

#[test]
fn an_index_cannot_name_a_path_outside_the_package() {
    let fake = Fake::default();
    let mut release = web(&fake, "0.1.0", 'a', None);
    release.files[1].path = "../escape.lua".to_owned();
    let index = index(vec![("web", vec![release.clone()])]);
    let plugins = tempfile::tempdir().unwrap();
    let err = registry(&fake, None)
        .install(plugins.path(), &index, "web", &release, false)
        .unwrap_err();
    assert!(err.to_string().contains("does not install"), "{err}");
}

#[test]
fn a_manifest_unlike_the_index_installs_nothing() {
    let fake = Fake::default();
    let toml = manifest("web", "9.9.9", 3, &[]);
    let release = release(
        &fake,
        "web",
        "0.1.0",
        'a',
        &[("plugin.toml", &toml), ("init.lua", "")],
        &[],
    );
    let index = index(vec![("web", vec![release.clone()])]);
    let plugins = tempfile::tempdir().unwrap();
    let err = registry(&fake, None)
        .install(plugins.path(), &index, "web", &release, false)
        .unwrap_err();
    assert!(
        err.to_string().contains("plugin.toml says web 9.9.9"),
        "{err}"
    );
}

#[test]
fn an_install_will_not_clobber_what_the_user_owns() {
    let fake = Fake::default();
    let release = web(&fake, "0.1.0", 'a', Some("return {}"));
    let index = index(vec![("web", vec![release.clone()])]);
    let registry = registry(&fake, None);

    let plugins = tempfile::tempdir().unwrap();
    std::fs::write(plugins.path().join("web.lua"), "").unwrap();
    let err = registry
        .install(plugins.path(), &index, "web", &release, false)
        .unwrap_err();
    assert!(err.to_string().contains("script named web.lua"), "{err}");
    registry
        .install(plugins.path(), &index, "web", &release, true)
        .unwrap();
    assert!(!plugins.path().join("web.lua").exists());

    let local = tempfile::tempdir().unwrap();
    std::fs::create_dir(local.path().join("web")).unwrap();
    std::fs::write(local.path().join("web/plugin.toml"), "").unwrap();
    let err = registry
        .install(local.path(), &index, "web", &release, false)
        .unwrap_err();
    assert!(err.to_string().contains("local package"), "{err}");
}

#[cfg(unix)]
#[test]
fn a_linked_checkout_is_never_replaced() {
    let fake = Fake::default();
    let release = web(&fake, "0.1.0", 'a', Some("return {}"));
    let index = index(vec![("web", vec![release.clone()])]);
    let checkout = tempfile::tempdir().unwrap();
    std::fs::write(checkout.path().join("plugin.toml"), "mine").unwrap();
    let plugins = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(checkout.path(), plugins.path().join("web")).unwrap();
    let err = registry(&fake, None)
        .install(plugins.path(), &index, "web", &release, true)
        .unwrap_err();
    assert!(
        err.to_string().contains("link to a local checkout"),
        "{err}"
    );
    assert_eq!(
        std::fs::read_to_string(checkout.path().join("plugin.toml")).unwrap(),
        "mine"
    );
}

#[test]
fn an_update_replaces_the_whole_package_unless_it_was_edited() {
    let fake = Fake::default();
    let old = web(&fake, "0.1.0", 'a', Some("return {}"));
    let new = web(&fake, "0.2.0", 'b', None);
    let index = index(vec![("web", vec![new.clone(), old.clone()])]);
    let registry = registry(&fake, None);
    let plugins = tempfile::tempdir().unwrap();
    registry
        .install(plugins.path(), &index, "web", &old, false)
        .unwrap();

    let statuses = kage_kura::status(plugins.path(), Some(&index)).unwrap();
    assert_eq!(statuses[0].kind, Kind::Package);
    assert_eq!(statuses[0].update, Some(v("0.2.0")));

    let module = plugins.path().join("web/lua/web/util.lua");
    std::fs::write(&module, "-- mine").unwrap();
    let err = registry
        .install(plugins.path(), &index, "web", &new, false)
        .unwrap_err();
    assert!(
        err.to_string().contains("local changes (lua/web/util.lua)"),
        "{err}"
    );
    let statuses = kage_kura::status(plugins.path(), Some(&index)).unwrap();
    assert_eq!(statuses[0].changed, ["lua/web/util.lua"]);

    let installed = registry
        .install(plugins.path(), &index, "web", &new, true)
        .unwrap();
    assert_eq!(installed.previous, Some(v("0.1.0")));
    assert!(!module.exists(), "files the new release dropped are gone");
    let statuses = kage_kura::status(plugins.path(), Some(&index)).unwrap();
    assert_eq!(statuses[0].update, None);
    assert_eq!(statuses[0].version, Some(v("0.2.0")));
}

#[test]
fn a_yanked_install_is_flagged() {
    let fake = Fake::default();
    let release = web(&fake, "0.1.0", 'a', None);
    let registry = registry(&fake, None);
    let plugins = tempfile::tempdir().unwrap();
    let live = index(vec![("web", vec![release.clone()])]);
    registry
        .install(plugins.path(), &live, "web", &release, false)
        .unwrap();
    let mut withdrawn = release.clone();
    withdrawn.yanked = Some("breaks shell".to_owned());
    let later = index(vec![("web", vec![withdrawn])]);
    let statuses = kage_kura::status(plugins.path(), Some(&later)).unwrap();
    assert_eq!(statuses[0].yanked.as_deref(), Some("breaks shell"));
}

#[test]
fn the_index_is_cached_revalidated_and_kept_when_offline() {
    let fake = Fake::default();
    let release = web(&fake, "0.1.0", 'a', None);
    let body = serde_json::to_vec(&index(vec![("web", vec![release])])).unwrap();
    *fake.index.lock().unwrap() = Some((body, "\"v1\"".to_owned()));
    let cache = tempfile::tempdir().unwrap();
    let registry = registry(&fake, Some(cache.path()));

    let first = registry.index(false).unwrap();
    assert!(first.stale.is_none());
    registry.index(false).unwrap();
    assert_eq!(fake.calls_to(INDEX_URL), 1, "a fresh cache is used as is");

    registry.index(true).unwrap();
    let calls = fake.calls.lock().unwrap().clone();
    assert_eq!(calls.last().unwrap().1.as_deref(), Some("\"v1\""));

    *fake.down.lock().unwrap() = true;
    let offline = registry.index(true).unwrap();
    assert_eq!(offline.stale.as_deref(), Some("unreachable"));
    assert!(offline.index.packages.contains_key("web"));

    let empty = tempfile::tempdir().unwrap();
    let cold = Registry::with_transport(
        INDEX_URL,
        Some(empty.path().to_path_buf()),
        Box::new(fake.clone()),
    );
    assert!(matches!(cold.index(false), Err(Error::Network(_))));
}

#[test]
fn forgetting_the_last_package_removes_the_lock() {
    let fake = Fake::default();
    let release = web(&fake, "0.1.0", 'a', None);
    let index = index(vec![("web", vec![release.clone()])]);
    let plugins = tempfile::tempdir().unwrap();
    registry(&fake, None)
        .install(plugins.path(), &index, "web", &release, false)
        .unwrap();
    Lock::forget(plugins.path(), "web").unwrap();
    assert!(!plugins.path().join(LOCK_FILE).exists());
}
