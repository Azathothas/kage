//! Discover and execute the plugins in a directory.
//!
//! [`load_all`] first evaluates the embedded `_defaults.lua` in its own
//! environment, then every plugin [`crate::package::discover`] finds in
//! the plugins directory (a `<name>.lua` script or a `<name>/` package),
//! in file-name order, then applies the `[keybindings]` table from
//! `config.toml` (see [`crate::PluginRuntimeBuilder::keybindings`]), and
//! finally evaluates the trusted `init.lua` when the runtime has a user
//! dir (see the `user` module). Each later layer overrides the earlier
//! ones. The load ends by firing `color_scheme` for the current theme, so
//! highlight setups written as autocmds apply.
//! Each plugin is loaded independently: a broken plugin, a bad
//! `[keybindings]` entry or a broken `init.lua` logs an error through
//! the runtime's host log and the load proceeds. The function returns a
//! summary the host can surface to the user. Names starting with `@` are
//! reserved for kage's own environments and are rejected, and so are
//! names that are not valid UTF-8: plugins are addressed by name in
//! config, so two entries could not share the fallback name.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use kage_core::sync::lock;
use mlua::Lua;

use crate::api::LogLevel;
use crate::error::PluginError;
use crate::package::{self, Found};
use crate::runtime::{EvalState, PluginRuntime};

/// Outcome of [`load_dir`]: plugins (script files or package
/// directories) that loaded cleanly and ones that did not.
#[derive(Debug, Default)]
pub struct LoadReport {
    /// Plugins that evaluated without raising.
    pub loaded: Vec<PathBuf>,
    /// Plugins that failed, paired with the error encountered.
    pub failed: Vec<(PathBuf, String)>,
    /// Plugins skipped because `[plugins] disabled` names them or the
    /// deprecated `[plugins] enabled` allowlist leaves them out.
    /// Reported, never silently dropped.
    pub skipped: Vec<PathBuf>,
    /// Outcome of the trusted `init.lua`, or `None` when there was none
    /// to load.
    pub init: Option<Result<(), String>>,
    /// One message per `[keybindings]` entry that could not be applied.
    pub keymap_errors: Vec<String>,
}

impl LoadReport {
    /// True if every plugin file, every `[keybindings]` entry and
    /// `init.lua` loaded successfully.
    #[must_use]
    pub fn all_ok(&self) -> bool {
        self.failed.is_empty()
            && self.keymap_errors.is_empty()
            && !matches!(self.init, Some(Err(_)))
    }
}

/// Evaluate every `*.lua` file in `dir` against `runtime`.
///
/// A script is read and evaluated as one chunk; a package is checked
/// against its manifest and runs its `init.lua` (see
/// [`crate::package`]). A plugin that fails is logged, recorded and
/// skipped.
///
/// The whole directory loads as one job on the runtime's Lua owner
/// thread. Plugins are processed sorted by file name, so a plugin named
/// `a.lua` always loads before `b.lua` or `b/`.
pub fn load_dir(dir: &Path, runtime: &PluginRuntime) -> Result<LoadReport, PluginError> {
    load_all(Some(dir), runtime)
}

/// Run the full load against `runtime`: `_defaults.lua`, the plugins in
/// `plugins_dir` as [`load_dir`] does, the `[keybindings]` table, then
/// the trusted `init.lua` when the runtime has a user dir, then fire
/// `color_scheme`. `None` loads no plugins.
pub fn load_all(
    plugins_dir: Option<&Path>,
    runtime: &PluginRuntime,
) -> Result<LoadReport, PluginError> {
    let eval = Arc::clone(&runtime.eval);
    let dir = plugins_dir.map(Path::to_path_buf);
    runtime
        .host
        .call(move |lua| load_on(lua, dir.as_deref(), &eval))?
}

/// Body of [`load_all`], run on the owner thread.
pub(crate) fn load_on(
    lua: &Lua,
    dir: Option<&Path>,
    eval: &EvalState,
) -> Result<LoadReport, PluginError> {
    if let Err(err) = eval.eval_defaults(lua) {
        lock(eval.sink()).log(LogLevel::Error, &format!("_defaults.lua: {err}"));
    }
    let mut report = match dir {
        Some(dir) => load_plugins(lua, dir, eval)?,
        None => LoadReport::default(),
    };
    report.keymap_errors = eval.keymaps.apply_toml(lua, &eval.keybindings)?;
    for err in &report.keymap_errors {
        lock(eval.sink()).log(LogLevel::Error, err);
    }
    report.init = crate::user::load(lua, eval);
    let theme = eval.options.theme();
    if let Err(err) = crate::highlight::fire_color_scheme(lua, eval.sink(), &theme) {
        lock(eval.sink()).log(LogLevel::Error, &format!("color_scheme: {err}"));
    }
    Ok(report)
}

fn load_plugins(lua: &Lua, dir: &Path, eval: &EvalState) -> Result<LoadReport, PluginError> {
    let found = package::discover(dir).map_err(|err| PluginError::Io {
        path: dir.to_path_buf(),
        source: err,
    })?;
    let sink = eval.sink();
    if !eval.enabled.is_empty() {
        lock(sink).log(
            LogLevel::Warn,
            "[plugins] enabled is deprecated: list the plugins to leave out in \
             [plugins] disabled instead",
        );
    }
    let mut report = LoadReport::default();
    for entry in found {
        let (path, result) = match entry {
            Found::Invalid { path, error } => {
                lock(sink).log(
                    LogLevel::Error,
                    &format!("plugin '{}': {error}", path.display()),
                );
                report.failed.push((path, error));
                continue;
            }
            Found::Script { name, path } if eval.is_enabled(&name) => {
                let result = std::fs::read_to_string(&path)
                    .map_err(|err| format!("read failed: {err}"))
                    .and_then(|source| {
                        eval.eval_plugin(lua, &name, &source)
                            .map(drop)
                            .map_err(|err| err.to_string())
                    });
                (path, result)
            }
            Found::Package { name, dir } if eval.is_enabled(&name) => {
                let result = package::eval_package(lua, eval, &name, &dir);
                (dir, result)
            }
            Found::Script { name, path } | Found::Package { name, dir: path } => {
                lock(sink).log(
                    LogLevel::Info,
                    &format!("plugin '{name}' is disabled in [plugins]; skipped"),
                );
                report.skipped.push(path);
                continue;
            }
        };
        match result {
            Ok(()) => report.loaded.push(path),
            Err(err) => {
                lock(sink).log(
                    LogLevel::Error,
                    &format!("plugin '{}': {err}", path.display()),
                );
                report.failed.push((path, err));
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn loads_every_lua_file() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("a.lua"),
            "kage.register_command({ name='a', description='', handler=function() end })",
        )
        .unwrap();
        fs::write(
            dir.path().join("b.lua"),
            "kage.register_command({ name='b', description='', handler=function() end })",
        )
        .unwrap();
        fs::write(dir.path().join("notes.txt"), "skipped").unwrap();

        let rt = PluginRuntime::new().unwrap();
        let report = load_dir(dir.path(), &rt).unwrap();
        assert_eq!(report.loaded.len(), 2);
        assert!(report.failed.is_empty());
        assert_eq!(rt.registered_commands().len(), 2);
    }

    #[test]
    fn loads_files_sorted_by_name() {
        let dir = tempdir().unwrap();
        for name in ["z", "a", "m"] {
            fs::write(dir.path().join(format!("{name}.lua")), "").unwrap();
        }
        let rt = PluginRuntime::new().unwrap();
        let report = load_dir(dir.path(), &rt).unwrap();
        let names: Vec<_> = report
            .loaded
            .iter()
            .map(|p| p.file_stem().unwrap().to_str().unwrap())
            .collect();
        assert_eq!(names, ["a", "m", "z"]);
    }

    #[test]
    fn defaults_run_before_plugins() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("a.lua"),
            "kage.on('agent_start', function() kage.notify('a') end)",
        )
        .unwrap();
        let (rec, sink) = crate::testing::recording_sink();
        let rt = PluginRuntime::builder()
            .sink(sink)
            .defaults("kage.on('agent_start', function() kage.notify('defaults') end)")
            .build()
            .unwrap();
        let report = load_dir(dir.path(), &rt).unwrap();
        assert_eq!(report.loaded.len(), 1);
        rt.dispatch_event("agent_start", &serde_json::json!({}))
            .unwrap();
        assert_eq!(rec.snapshot().notifications, ["defaults", "a"]);

        rt.reload_dir(dir.path()).unwrap();
        assert_eq!(rt.handler_count("agent_start"), 2);
    }

    #[test]
    fn embedded_defaults_load_cleanly() {
        let (rec, rt) = crate::testing::runtime_with_recording(PathBuf::from("."));
        load_dir(Path::new("/nonexistent/here"), &rt).unwrap();
        assert!(rec.snapshot().logs.is_empty());
    }

    #[test]
    fn enabled_allowlist_skips_unlisted_plugins() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("trusted.lua"),
            "kage.register_command({ name='t', description='', handler=function() end })",
        )
        .unwrap();
        fs::write(
            dir.path().join("other.lua"),
            "kage.register_command({ name='o', description='', handler=function() end })",
        )
        .unwrap();

        let rt = PluginRuntime::builder()
            .enabled(vec!["trusted".to_owned()])
            .build()
            .unwrap();
        let report = load_dir(dir.path(), &rt).unwrap();
        assert_eq!(report.loaded.len(), 1);
        assert_eq!(report.skipped.len(), 1);
        assert!(report.skipped[0].ends_with("other.lua"));
        let commands = rt.registered_commands();
        assert_eq!(commands.len(), 1);
        assert!(rt.registered_commands().iter().any(|c| c.name() == "t"));
        assert!(!rt.registered_commands().iter().any(|c| c.name() == "o"));
    }

    #[test]
    fn disabled_plugins_are_skipped_and_the_rest_load() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("on.lua"), "").unwrap();
        fs::write(dir.path().join("off.lua"), "").unwrap();
        let (rec, sink) = crate::testing::recording_sink();
        let rt = PluginRuntime::builder()
            .sink(sink)
            .disabled(vec!["off".to_owned()])
            .build()
            .unwrap();
        let report = load_dir(dir.path(), &rt).unwrap();
        assert_eq!(report.loaded.len(), 1);
        assert!(report.skipped[0].ends_with("off.lua"));
        assert!(
            !rec.snapshot()
                .logs
                .iter()
                .any(|(_, msg)| msg.contains("deprecated"))
        );
    }

    #[test]
    fn the_deprecated_allowlist_still_applies_and_warns() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.lua"), "").unwrap();
        fs::write(dir.path().join("b.lua"), "").unwrap();
        let (rec, sink) = crate::testing::recording_sink();
        let rt = PluginRuntime::builder()
            .sink(sink)
            .enabled(vec!["a".to_owned(), "b".to_owned()])
            .disabled(vec!["b".to_owned()])
            .build()
            .unwrap();
        let report = load_dir(dir.path(), &rt).unwrap();
        assert_eq!(report.loaded.len(), 1);
        assert!(report.loaded[0].ends_with("a.lua"));
        assert!(rec.snapshot().logs.iter().any(|(level, msg)| {
            *level == LogLevel::Warn && msg.contains("[plugins] enabled is deprecated")
        }));
    }

    #[test]
    fn empty_allowlist_loads_everything() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("a.lua"),
            "kage.register_command({ name='a', description='', handler=function() end })",
        )
        .unwrap();
        let rt = PluginRuntime::builder().enabled(vec![]).build().unwrap();
        let report = load_dir(dir.path(), &rt).unwrap();
        assert_eq!(report.loaded.len(), 1);
        assert!(report.skipped.is_empty());
    }

    #[test]
    fn one_broken_plugin_does_not_abort_the_rest() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.lua"), "this is not valid lua = =").unwrap();
        fs::write(
            dir.path().join("b.lua"),
            "kage.register_command({ name='b', description='', handler=function() end })",
        )
        .unwrap();

        let rt = PluginRuntime::new().unwrap();
        let report = load_dir(dir.path(), &rt).unwrap();
        assert_eq!(report.loaded.len(), 1);
        assert_eq!(report.failed.len(), 1);
        assert!(!report.all_ok());
        assert_eq!(rt.registered_commands().len(), 1);
    }

    #[test]
    fn missing_dir_is_treated_as_empty() {
        let rt = PluginRuntime::new().unwrap();
        let report = load_dir(Path::new("/nonexistent/here"), &rt).unwrap();
        assert!(report.loaded.is_empty());
        assert!(report.failed.is_empty());
    }

    #[test]
    fn ignores_non_lua_extensions() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "not a plugin").unwrap();
        fs::write(dir.path().join("README.md"), "# nope").unwrap();
        let rt = PluginRuntime::new().unwrap();
        let report = load_dir(dir.path(), &rt).unwrap();
        assert!(report.loaded.is_empty());
        assert!(report.failed.is_empty());
    }

    // APFS and NTFS reject non-UTF-8 names, so the fixture only
    // exists on Linux.
    #[cfg(target_os = "linux")]
    #[test]
    fn non_utf8_stems_fail_instead_of_colliding() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let dir = tempdir().unwrap();
        for stem in [&b"bad\xff1"[..], &b"bad\xff2"[..]] {
            let mut path = dir.path().join(OsStr::from_bytes(stem));
            path.set_extension("lua");
            fs::write(path, "kage.register_command({})").unwrap();
        }
        let rt = PluginRuntime::new().unwrap();
        let report = load_dir(dir.path(), &rt).unwrap();
        assert_eq!(report.loaded.len(), 0);
        assert_eq!(report.failed.len(), 2);
        assert!(rt.registered_commands().is_empty());
        for (path, err) in &report.failed {
            assert!(
                err.contains("not valid UTF-8"),
                "{path:?}: expected a UTF-8 error, got {err}"
            );
        }
    }
}
