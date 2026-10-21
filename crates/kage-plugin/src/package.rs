//! Plugin discovery and package plugins.
//!
//! The plugins directory holds two kinds of plugin, both named by the
//! entry's file name:
//!
//! * a script, `<name>.lua`: one file, no manifest;
//! * a package, `<name>/`: a [`MANIFEST`] (`plugin.toml`), an [`ENTRY`]
//!   (`init.lua`) and optional modules under [`MODULES`] (`lua/`), which
//!   the package loads with a `require` confined to that directory (see
//!   [`crate::require`]).
//!
//! A package's manifest is enforced, not advisory: its `name` must be
//! the directory's, its `api` must not exceed [`API_VERSION`] (checked
//! before any of its code runs), and its `capabilities` cap what the
//! package may hold, whatever `[plugins.capabilities]` grants.
//!
//! [`discover`] is the one place that decides what the directory holds,
//! for the loader and for hosts that list plugins. Entries whose name
//! starts with `.` are skipped, so an installer can stage a package in a
//! dot directory and rename it into place.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use mlua::Lua;
use serde::Deserialize;

use crate::api::API_VERSION;
use crate::capabilities::Capability;
use crate::require::require_fn;
use crate::runtime::EvalState;

/// A package's manifest file.
pub const MANIFEST: &str = "plugin.toml";

/// A package's entry file.
pub const ENTRY: &str = "init.lua";

/// The directory a package's `require` loads modules from.
pub const MODULES: &str = "lua";

/// A package's `plugin.toml`.
///
/// Keys kage does not know are ignored, so a manifest written for a
/// newer kage still loads when its `api` allows.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Manifest {
    /// The package name, which must equal its directory's name.
    pub name: String,
    /// The package version.
    pub version: semver::Version,
    /// The minimum `kage.api_version()` the package needs.
    pub api: i64,
    /// One line on what the package does.
    #[serde(default)]
    pub description: String,
    /// Every capability the package may request.
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// Who wrote the package.
    #[serde(default)]
    pub authors: Vec<String>,
    /// An SPDX license expression.
    #[serde(default)]
    pub license: Option<String>,
    /// The package's home page.
    #[serde(default)]
    pub homepage: Option<String>,
    /// Where the package's source lives.
    #[serde(default)]
    pub repository: Option<String>,
    /// Keywords to find the package by.
    #[serde(default)]
    pub tags: Vec<String>,
}

impl Manifest {
    /// Parses and checks a manifest's text.
    ///
    /// # Errors
    ///
    /// The text is not TOML of this shape, the name is not a valid
    /// plugin name, `api` is below 1, or a capability is unknown.
    pub fn parse(text: &str) -> Result<Self, String> {
        let manifest: Self = toml::from_str(text).map_err(|e| format!("{MANIFEST}: {e}"))?;
        check_name(&manifest.name).map_err(|e| format!("{MANIFEST}: {e}"))?;
        if manifest.api < 1 {
            return Err(format!("{MANIFEST}: api must be 1 or more"));
        }
        for name in &manifest.capabilities {
            Capability::parse(name).map_err(|e| format!("{MANIFEST}: {e}"))?;
        }
        Ok(manifest)
    }

    /// Reads and checks the manifest of the package in `dir`, whose
    /// name must match the directory's.
    ///
    /// # Errors
    ///
    /// The file cannot be read, does not parse (see [`Self::parse`]),
    /// or names another package.
    pub fn read(dir: &Path) -> Result<Self, String> {
        let path = dir.join(MANIFEST);
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let manifest = Self::parse(&text)?;
        let dir_name = dir.file_name().and_then(OsStr::to_str).unwrap_or_default();
        if manifest.name != dir_name {
            return Err(format!(
                "{MANIFEST} names `{}`, but its directory is `{dir_name}`",
                manifest.name
            ));
        }
        Ok(manifest)
    }

    /// Checks that this kage provides the API generation the package
    /// needs.
    ///
    /// # Errors
    ///
    /// The package needs a newer `api` than [`API_VERSION`].
    pub fn check_api(&self) -> Result<(), String> {
        if self.api > API_VERSION {
            return Err(format!(
                "{} {} needs api {}, and this kage provides {API_VERSION}",
                self.name, self.version, self.api
            ));
        }
        Ok(())
    }

    /// The declared capabilities, already checked by [`Self::parse`].
    pub(crate) fn capability_set(&self) -> HashSet<Capability> {
        self.capabilities
            .iter()
            .filter_map(|name| Capability::parse(name).ok())
            .collect()
    }
}

/// Checks that `name` can name a package: ASCII letters, digits, `-`
/// and `_`.
///
/// # Errors
///
/// The name is empty or holds another character.
pub fn check_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(format!(
            "`{name}` is not a plugin name: use letters, digits, dashes or underscores"
        ));
    }
    Ok(())
}

/// One entry of the plugins directory that is, or tries to be, a plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    /// A `<name>.lua` file.
    Script {
        /// The file stem.
        name: String,
        /// The file.
        path: PathBuf,
    },
    /// A `<name>/` directory with a `plugin.toml`.
    Package {
        /// The directory name.
        name: String,
        /// The directory.
        dir: PathBuf,
    },
    /// An entry that cannot load.
    Invalid {
        /// The entry.
        path: PathBuf,
        /// Why it cannot load.
        error: String,
    },
}

impl Found {
    /// The file or directory this entry is.
    #[must_use]
    pub fn path(&self) -> &Path {
        match self {
            Self::Script { path, .. } | Self::Invalid { path, .. } => path,
            Self::Package { dir, .. } => dir,
        }
    }
}

/// The plugins in `dir`, in file-name order. A missing directory holds
/// none. A script and a package of the same name are both
/// [`Found::Invalid`], so neither silently shadows the other.
///
/// # Errors
///
/// The directory exists but cannot be read.
pub fn discover(dir: &Path) -> std::io::Result<Vec<Found>> {
    let read_dir = match std::fs::read_dir(dir) {
        Ok(read_dir) => read_dir,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };
    let mut found = Vec::new();
    for entry in read_dir {
        if let Some(entry) = classify(entry?.path()) {
            found.push(entry);
        }
    }
    found.sort_by(|a, b| a.path().file_name().cmp(&b.path().file_name()));
    flag_conflicts(&mut found);
    Ok(found)
}

fn classify(path: PathBuf) -> Option<Found> {
    let file_name = path.file_name()?;
    if file_name.as_encoded_bytes().starts_with(b".") {
        return None;
    }
    let invalid = |path: PathBuf, error: &str| {
        Some(Found::Invalid {
            path,
            error: error.to_owned(),
        })
    };
    if path.is_dir() {
        let has_manifest = path.join(MANIFEST).is_file();
        if !has_manifest && !path.join(ENTRY).is_file() {
            return None;
        }
        let Some(name) = file_name.to_str().map(str::to_owned) else {
            return invalid(path, "plugin directory name is not valid UTF-8");
        };
        if name.starts_with('@') {
            return invalid(path, "names starting with '@' are reserved");
        }
        if !has_manifest {
            return invalid(path, "a package needs a plugin.toml beside its init.lua");
        }
        return Some(Found::Package { name, dir: path });
    }
    if path.extension()? != "lua" {
        return None;
    }
    let Some(name) = path.file_stem().and_then(OsStr::to_str).map(str::to_owned) else {
        return invalid(path, "plugin file name is not valid UTF-8");
    };
    if name.starts_with('@') {
        return invalid(path, "names starting with '@' are reserved");
    }
    Some(Found::Script { name, path })
}

fn flag_conflicts(found: &mut [Found]) {
    let names = |script: bool| -> HashSet<String> {
        found
            .iter()
            .filter_map(|entry| match entry {
                Found::Script { name, .. } if script => Some(name.clone()),
                Found::Package { name, .. } if !script => Some(name.clone()),
                _ => None,
            })
            .collect()
    };
    let clashes: HashSet<String> = names(true).intersection(&names(false)).cloned().collect();
    for entry in found.iter_mut() {
        let (Found::Script { name, .. } | Found::Package { name, .. }) = entry else {
            continue;
        };
        if clashes.contains(name) {
            let error = format!("both {name}.lua and {name}/ exist; remove one");
            *entry = Found::Invalid {
                path: entry.path().to_path_buf(),
                error,
            };
        }
    }
}

/// Evaluate the package `name` in `dir`: check its manifest, set its
/// capability ceiling, give its environment a `require` over `lua/`, and
/// run `init.lua`.
pub(crate) fn eval_package(
    lua: &Lua,
    eval: &EvalState,
    name: &str,
    dir: &Path,
) -> Result<(), String> {
    let manifest = Manifest::read(dir)?;
    manifest.check_api()?;
    let entry = dir.join(ENTRY);
    let source =
        std::fs::read_to_string(&entry).map_err(|e| format!("{}: {e}", entry.display()))?;
    eval.set_ceiling(name, manifest.capability_set());
    let env = eval.env(lua, name).map_err(|e| e.to_string())?;
    let require = require_fn(lua, &env, dir.join(MODULES)).map_err(|e| e.to_string())?;
    env.raw_set("require", require).map_err(|e| e.to_string())?;
    let chunk = format!("@{}", entry.display());
    eval.eval_in(lua, name, &chunk, env, &source)
        .map(drop)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;

    use tempfile::{TempDir, tempdir};

    use super::*;
    use crate::PluginRuntime;
    use crate::loader::{LoadReport, load_dir};
    use crate::testing::{RecordingSink, recording_sink};

    fn write(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn manifest(name: &str, extra: &str) -> String {
        format!("name = \"{name}\"\nversion = \"0.1.0\"\napi = 3\n{extra}")
    }

    fn package(root: &Path, name: &str, extra: &str, init: &str) {
        write(root, &format!("{name}/plugin.toml"), &manifest(name, extra));
        write(root, &format!("{name}/init.lua"), init);
    }

    fn load(
        plugins: &TempDir,
        grants: &[(&str, &[&str])],
    ) -> (RecordingSink, PluginRuntime, LoadReport) {
        let grants: BTreeMap<String, Vec<String>> = grants
            .iter()
            .map(|(name, caps)| {
                let caps = caps.iter().map(|cap| (*cap).to_owned()).collect();
                ((*name).to_owned(), caps)
            })
            .collect();
        let (rec, sink) = recording_sink();
        let rt = PluginRuntime::builder()
            .sink(sink)
            .capabilities(grants)
            .build()
            .unwrap();
        let report = load_dir(plugins.path(), &rt).unwrap();
        (rec, rt, report)
    }

    fn only_failure(report: &LoadReport) -> &str {
        assert!(report.loaded.is_empty(), "{report:?}");
        assert_eq!(report.failed.len(), 1, "{report:?}");
        &report.failed[0].1
    }

    #[test]
    fn a_manifest_parses_and_ignores_unknown_keys() {
        let parsed = Manifest::parse(&manifest(
            "web",
            "capabilities = [\"net\"]\ntags = [\"search\"]\nfuture_key = 1\n",
        ))
        .unwrap();
        assert_eq!(parsed.name, "web");
        assert_eq!(parsed.version, semver::Version::new(0, 1, 0));
        assert_eq!(parsed.api, 3);
        assert_eq!(parsed.capabilities, ["net"]);
        assert_eq!(parsed.tags, ["search"]);
    }

    #[test]
    fn a_bad_manifest_is_refused() {
        let cases = [
            (
                "name = \"a b\"\nversion = \"0.1.0\"\napi = 3",
                "not a plugin name",
            ),
            ("name = \"a\"\nversion = \"one\"\napi = 3", "version"),
            (
                "name = \"a\"\nversion = \"0.1.0\"\napi = 0",
                "api must be 1",
            ),
            ("name = \"a\"\nversion = \"0.1.0\"", "api"),
            (
                "name = \"a\"\nversion = \"0.1.0\"\napi = 3\ncapabilities = [\"root\"]",
                "unknown capability",
            ),
        ];
        for (text, want) in cases {
            let err = Manifest::parse(text).unwrap_err();
            assert!(err.contains(want), "{text}: {err}");
        }
    }

    #[test]
    fn discover_tells_scripts_packages_and_strays_apart() {
        let root = tempdir().unwrap();
        write(root.path(), "a.lua", "");
        package(root.path(), "b", "", "");
        write(root.path(), "c/init.lua", "");
        write(root.path(), "data/notes.txt", "");
        package(root.path(), ".b.tmp", "", "");
        write(root.path(), "notes.txt", "");
        let found = discover(root.path()).unwrap();
        assert_eq!(found.len(), 3, "{found:?}");
        assert!(matches!(&found[0], Found::Script { name, .. } if name == "a"));
        assert!(matches!(&found[1], Found::Package { name, .. } if name == "b"));
        assert!(matches!(&found[2], Found::Invalid { error, .. } if error.contains("plugin.toml")));
    }

    #[test]
    fn a_script_and_a_package_of_one_name_both_fail() {
        let root = tempdir().unwrap();
        write(root.path(), "x.lua", "kage.notify('script')");
        package(root.path(), "x", "", "kage.notify('package')");
        let (rec, _, report) = load(&root, &[]);
        assert_eq!(report.failed.len(), 2);
        assert!(
            report
                .failed
                .iter()
                .all(|(_, err)| err.contains("remove one"))
        );
        assert!(rec.snapshot().notifications.is_empty());
    }

    #[test]
    fn a_package_loads_its_modules() {
        let root = tempdir().unwrap();
        write(root.path(), "p/lua/p/util.lua", "return { word = 'mod' }");
        write(root.path(), "p/lua/p/deep/init.lua", "return 'deep'");
        package(
            root.path(),
            "p",
            "",
            "kage.notify(require('p.util').word .. ' ' .. require('p.deep'))",
        );
        let (rec, _, report) = load(&root, &[]);
        assert_eq!(report.loaded.len(), 1, "{report:?}");
        assert_eq!(rec.snapshot().notifications, ["mod deep"]);
    }

    #[test]
    fn a_script_has_no_require() {
        let root = tempdir().unwrap();
        write(
            root.path(),
            "s.lua",
            "kage.notify(tostring(require == nil))",
        );
        let (rec, _, _) = load(&root, &[]);
        assert_eq!(rec.snapshot().notifications, ["true"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_package_cannot_require_outside_its_lua_dir() {
        let root = tempdir().unwrap();
        write(root.path(), "secret.lua", "return 'escaped'");
        package(root.path(), "p", "", "require('evil')");
        fs::create_dir_all(root.path().join("p/lua")).unwrap();
        std::os::unix::fs::symlink(
            root.path().join("secret.lua"),
            root.path().join("p/lua/evil.lua"),
        )
        .unwrap();
        let (_, _, report) = load(&root, &[]);
        let failed: Vec<_> = report
            .failed
            .iter()
            .filter(|(path, _)| path.ends_with("p"))
            .collect();
        assert_eq!(failed.len(), 1, "{report:?}");
        assert!(failed[0].1.contains("resolves outside lua/"), "{failed:?}");
    }

    #[test]
    fn a_package_named_unlike_its_dir_does_not_load() {
        let root = tempdir().unwrap();
        write(root.path(), "p/plugin.toml", &manifest("q", ""));
        write(root.path(), "p/init.lua", "kage.notify('ran')");
        let (rec, _, report) = load(&root, &[]);
        assert!(only_failure(&report).contains("names `q`"));
        assert!(rec.snapshot().notifications.is_empty());
    }

    #[test]
    fn a_package_for_a_newer_api_runs_no_code() {
        let root = tempdir().unwrap();
        write(
            root.path(),
            "p/plugin.toml",
            "name = \"p\"\nversion = \"1.0.0\"\napi = 99",
        );
        write(root.path(), "p/init.lua", "kage.notify('ran')");
        let (rec, _, report) = load(&root, &[]);
        assert!(only_failure(&report).contains("needs api 99"));
        assert!(rec.snapshot().notifications.is_empty());
    }

    #[test]
    fn a_package_holds_only_what_it_declares() {
        let root = tempdir().unwrap();
        package(
            root.path(),
            "ok",
            "capabilities = [\"net\"]",
            "kage.notify(tostring(kage.request_capabilities({ 'net' }).net))",
        );
        package(
            root.path(),
            "greedy",
            "capabilities = [\"net\"]",
            "kage.request_capabilities({ 'exec' })",
        );
        let grants: &[(&str, &[&str])] = &[("ok", &["net"]), ("greedy", &["net", "exec"])];
        let (rec, _, report) = load(&root, grants);
        assert_eq!(rec.snapshot().notifications, ["true"]);
        assert_eq!(report.failed.len(), 1);
        assert!(
            report.failed[0].1.contains("does not declare"),
            "{report:?}"
        );
    }

    #[test]
    fn an_undeclared_capability_keeps_its_events_from_a_package() {
        let root = tempdir().unwrap();
        package(
            root.path(),
            "p",
            "",
            "kage.on('message_end', function() end)",
        );
        let (rec, rt, report) = load(&root, &[("p", &["context"])]);
        assert_eq!(report.loaded.len(), 1);
        assert_eq!(rt.handler_count("message_end"), 0);
        assert!(
            rec.snapshot()
                .logs
                .iter()
                .any(|(_, msg)| msg.contains("needs the context capability")),
        );
    }

    #[test]
    fn the_allowlist_names_packages_too() {
        let root = tempdir().unwrap();
        package(root.path(), "p", "", "kage.notify('p')");
        let (rec, sink) = recording_sink();
        let rt = PluginRuntime::builder()
            .sink(sink)
            .enabled(vec!["other".to_owned()])
            .build()
            .unwrap();
        let report = load_dir(root.path(), &rt).unwrap();
        assert_eq!(report.skipped.len(), 1);
        assert!(rec.snapshot().notifications.is_empty());
    }

    #[test]
    fn a_reload_reads_changed_modules() {
        let root = tempdir().unwrap();
        write(root.path(), "p/lua/m.lua", "return 'one'");
        package(root.path(), "p", "", "kage.notify(require('m'))");
        let (rec, rt, _) = load(&root, &[]);
        write(root.path(), "p/lua/m.lua", "return 'two'");
        rt.reload_dir(root.path()).unwrap();
        assert_eq!(rec.snapshot().notifications, ["one", "two"]);
    }
}
