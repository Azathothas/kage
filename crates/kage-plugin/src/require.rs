//! The confined `require` of the trusted user config and of package
//! plugins.
//!
//! Each `require` is bound to one environment and one module root.
//! Module names are dot-separated segments of `[A-Za-z0-9_-]`. `a.b`
//! resolves to `<root>/a/b.lua`, then `<root>/a/b/init.lua`, and a path
//! that canonicalizes outside the root (through a symlink) is rejected.
//! Modules run in the bound environment and are cached until the next
//! reload, when the environment is rebuilt.

use std::path::{Path, PathBuf};

use mlua::{Function, Lua, Table};

/// Source of the `require` wrapper that caches modules and detects
/// cycles over the Rust searcher.
const REQUIRE: &str = include_str!("../lua/require.lua");

/// A `require` that loads modules under `root` into `env`.
pub(crate) fn require_fn(lua: &Lua, env: &Table, root: PathBuf) -> mlua::Result<Function> {
    let find = lua.create_function(move |lua, (name, env): (String, Table)| {
        let path = resolve(&root, &name).map_err(mlua::Error::external)?;
        let source = std::fs::read_to_string(&path)
            .map_err(|err| mlua::Error::external(format!("require: {}: {err}", path.display())))?;
        lua.load(source)
            .set_name(format!("@{}", path.display()))
            .set_environment(env)
            .into_function()
    })?;
    lua.load(REQUIRE)
        .set_name("=kage/require.lua")
        .call((find, env.clone()))
}

/// Resolve module `name` to a file under `root`: `a.b` tries
/// `a/b.lua`, then `a/b/init.lua`. Invalid names and files whose
/// canonical path leaves `root` are rejected.
pub(crate) fn resolve(root: &Path, name: &str) -> Result<PathBuf, String> {
    let valid = !name.is_empty()
        && name.split('.').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        });
    if !valid {
        return Err(format!("require: invalid module name '{name}'"));
    }
    let not_found = || {
        format!(
            "require: module '{name}' not found under {}",
            root.display()
        )
    };
    let root = root.canonicalize().map_err(|_| not_found())?;
    let base: PathBuf = name.split('.').collect();
    for candidate in [base.with_extension("lua"), base.join("init.lua")] {
        let Ok(path) = root.join(candidate).canonicalize() else {
            continue;
        };
        if !path.starts_with(&root) {
            return Err(format!("require: module '{name}' resolves outside lua/"));
        }
        return Ok(path);
    }
    Err(not_found())
}
