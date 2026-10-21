//! `kage plugin`: the plugins directory and the plugin registry.
//!
//! `install` and `update` fetch packages from the registry
//! `[plugins] registry` names (kura unless set) through [`kage_kura`].
//! Neither grants a capability on its own: each lists what the package
//! declares and asks, and `--grant` grants it without asking. `grant`
//! and `revoke` edit `[plugins.capabilities]`, and `enable` and
//! `disable` edit `[plugins] disabled`, in the user config.

use std::io::{BufRead as _, IsTerminal as _, Write as _};
use std::path::PathBuf;
use std::process::ExitCode;

use kage_core::config::{Config, PluginsConfig};
use kage_kura::{Index, Kind, Lock, Registry};
use kage_plugin::package::{self, Found, Manifest};
use semver::Version;
use serde_json::json;

use crate::PluginAction;

/// Run one `kage plugin` action.
pub(crate) fn run(action: PluginAction) -> ExitCode {
    match Ctx::load().and_then(|ctx| ctx.run(action)) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(err) => {
            eprintln!("kage: {err}");
            ExitCode::FAILURE
        }
    }
}

/// What every action works on.
struct Ctx {
    plugins_dir: PathBuf,
    config_path: PathBuf,
    /// The user config alone: what `kage plugin` edits.
    user: PluginsConfig,
    registry: Registry,
}

impl Ctx {
    fn load() -> Result<Self, String> {
        let plugins_dir = crate::plugins_dir()?;
        let config_path = Config::default_path().ok_or("no config directory")?;
        let user = Config::load_default()
            .map_err(|e| format!("config: {e}"))?
            .plugins;
        Ok(Self {
            plugins_dir,
            config_path,
            registry: registry(&user),
            user,
        })
    }

    fn run(&self, action: PluginAction) -> Result<bool, String> {
        match action {
            PluginAction::List => self.list(),
            PluginAction::Search { query, refresh } => self.search(&query.join(" "), refresh),
            PluginAction::Info { name, refresh } => self.info(&name, refresh),
            PluginAction::Install {
                package,
                force,
                grant,
            } => self.install(&package, force, grant),
            PluginAction::Update {
                names,
                force,
                grant,
            } => self.update(&names, force, grant),
            PluginAction::Remove { name } => self.remove(&name),
            PluginAction::Grant { name, capabilities } => self.grant(&name, &capabilities),
            PluginAction::Revoke { name, capabilities } => self.revoke(&name, &capabilities),
            PluginAction::Enable { name } => self.set_loads(&name, true),
            PluginAction::Disable { name } => self.set_loads(&name, false),
        }
    }

    /// The registry index, saying so when a cached copy stands in.
    fn index(&self, refresh: bool) -> Result<Index, String> {
        let fetched = self.registry.index(refresh).map_err(|e| match e {
            kage_kura::Error::Network(why) => {
                format!("cannot reach the plugin registry: {why}")
            }
            other => format!("{}: {other}", self.registry.url()),
        })?;
        if let Some(why) = fetched.stale {
            eprintln!(
                "kage: {} is unreachable ({why}); using the cached index",
                self.registry.url()
            );
        }
        Ok(fetched.index)
    }

    fn list(&self) -> Result<bool, String> {
        let lock = Lock::load(&self.plugins_dir).map_err(|e| e.to_string())?;
        let index = if lock.plugins.is_empty() {
            None
        } else {
            self.registry.index(false).ok().map(|fetched| fetched.index)
        };
        let statuses =
            kage_kura::status(&self.plugins_dir, index.as_ref()).map_err(|e| e.to_string())?;
        if statuses.is_empty() {
            println!("no plugins in {}", self.plugins_dir.display());
            return Ok(true);
        }
        let effective = effective_plugins(&self.user);
        let width = statuses.iter().map(|s| s.name.len()).max().unwrap_or(0);
        for status in &statuses {
            let kind = match status.kind {
                Kind::Script => "script",
                Kind::Package => "package",
            };
            let version = status
                .version
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default();
            let source = match &status.locked {
                Some(locked) if locked.registry == kage_kura::DEFAULT_REGISTRY => "kura",
                Some(_) => "registry",
                None => "local",
            };
            let mut notes = Vec::new();
            if !effective.loads(&status.name) {
                notes.push("disabled".to_owned());
            }
            if let Some(update) = &status.update {
                notes.push(format!("update {update}"));
            }
            if let Some(why) = &status.yanked {
                notes.push(format!("yanked: {why}"));
            }
            if !status.changed.is_empty() {
                notes.push(format!("changed: {}", status.changed.join(", ")));
            }
            let line = format!(
                "{:<width$}  {kind:<7}  {version:<9}  {source:<8}  {}",
                status.name,
                notes.join(", ")
            );
            println!("{}", line.trim_end());
        }
        Ok(true)
    }

    fn search(&self, query: &str, refresh: bool) -> Result<bool, String> {
        let index = self.index(refresh)?;
        let found = index.search(query);
        if found.is_empty() {
            println!("no package matches `{query}`");
            return Ok(true);
        }
        let width = found.iter().map(|(name, _)| name.len()).max().unwrap_or(0);
        for (name, package) in found {
            let latest = package
                .latest()
                .map_or_else(|| "-".to_owned(), |release| release.version.to_string());
            let line = format!(
                "{name:<width$}  {latest:<9}  {}",
                package.description.as_deref().unwrap_or_default()
            );
            println!("{}", line.trim_end());
        }
        Ok(true)
    }

    fn info(&self, name: &str, refresh: bool) -> Result<bool, String> {
        let index = self.index(refresh)?;
        let package = index
            .packages
            .get(name)
            .ok_or_else(|| format!("{name} is not in the registry"))?;
        let Some(latest) = package.latest() else {
            return Err(format!("every release of {name} is yanked"));
        };
        println!("{name} {}", latest.version);
        if let Some(description) = &package.description {
            println!("  {description}");
        }
        let field = |label: &str, value: &str| {
            if !value.is_empty() {
                println!("  {label:<13} {value}");
            }
        };
        let caps: Vec<String> = latest
            .capabilities
            .iter()
            .map(|cap| match kage_plugin::capability_summary(cap) {
                Some(summary) => format!("{cap} ({summary})"),
                None => cap.clone(),
            })
            .collect();
        field("capabilities", &caps.join(", "));
        field("needs api", &latest.api.to_string());
        field(
            "authors",
            &package.authors.clone().unwrap_or_default().join(", "),
        );
        field("license", package.license.as_deref().unwrap_or_default());
        field("homepage", package.homepage.as_deref().unwrap_or_default());
        field(
            "repository",
            package.repository.as_deref().unwrap_or_default(),
        );
        field("tags", &package.tags.clone().unwrap_or_default().join(", "));
        let releases: Vec<String> = package
            .releases
            .iter()
            .map(|release| match &release.yanked {
                Some(why) => format!("{} (yanked: {why})", release.version),
                None => release.version.to_string(),
            })
            .collect();
        field("releases", &releases.join(", "));
        let installed = Lock::load(&self.plugins_dir)
            .ok()
            .and_then(|lock| lock.plugins.get(name).map(|l| l.version.to_string()));
        field("installed", installed.as_deref().unwrap_or("no"));
        Ok(true)
    }

    fn install(&self, spec: &str, force: bool, grant: bool) -> Result<bool, String> {
        let (name, version) = parse_spec(spec)?;
        let mut index = self.index(false)?;
        // A fresh cache can predate a release published minutes ago.
        if matches!(
            index.resolve(name, version.as_ref()),
            Err(kage_kura::Error::NotFound(_))
        ) {
            index = self.index(true)?;
        }
        let release = index
            .resolve(name, version.as_ref())
            .map_err(|e| e.to_string())?;
        let installed = self
            .registry
            .install(&self.plugins_dir, &index, name, release, force)
            .map_err(|e| e.to_string())?;
        match &installed.previous {
            Some(previous) if previous != &installed.version => {
                println!("updated {name} {previous} -> {}", installed.version);
            }
            _ => println!("installed {name} {}", installed.version),
        }
        self.offer_grants(name, &installed.capabilities, grant)?;
        self.note_disabled(name);
        Ok(true)
    }

    fn update(&self, names: &[String], force: bool, grant: bool) -> Result<bool, String> {
        let lock = Lock::load(&self.plugins_dir).map_err(|e| e.to_string())?;
        let names: Vec<String> = if names.is_empty() {
            lock.plugins.keys().cloned().collect()
        } else {
            names.to_vec()
        };
        if names.is_empty() {
            println!("no plugin was installed from a registry");
            return Ok(true);
        }
        let index = self.index(true)?;
        let mut ok = true;
        for name in &names {
            let Some(locked) = lock.plugins.get(name) else {
                eprintln!("kage: {name} was not installed from a registry");
                ok = false;
                continue;
            };
            let yanked = index.packages.get(name).and_then(|package| {
                let release = package
                    .releases
                    .iter()
                    .find(|r| r.version == locked.version)?;
                release.yanked.clone()
            });
            let release = match index.resolve(name, None) {
                Ok(release) => release,
                Err(err) => {
                    eprintln!("kage: {name}: {err}");
                    ok = false;
                    continue;
                }
            };
            if release.version <= locked.version {
                match yanked {
                    Some(why) => eprintln!(
                        "kage: {name} {} is yanked ({why}); `kage plugin install \
                         {name}@{} --force` moves to the newest good release",
                        locked.version, release.version
                    ),
                    None => println!("{name} {} is up to date", locked.version),
                }
                continue;
            }
            match self
                .registry
                .install(&self.plugins_dir, &index, name, release, force)
            {
                Ok(installed) => {
                    println!("updated {name} {} -> {}", locked.version, installed.version);
                    self.offer_grants(name, &installed.capabilities, grant)?;
                }
                Err(err) => {
                    eprintln!("kage: {name}: {err}");
                    ok = false;
                }
            }
        }
        Ok(ok)
    }

    fn remove(&self, name: &str) -> Result<bool, String> {
        if kage_kura::remove(&self.plugins_dir, name).map_err(|e| e.to_string())? {
            println!("removed {name}");
            Ok(true)
        } else {
            Err(format!("no plugin named {name} is installed"))
        }
    }

    /// Grants plugin `name` the `capabilities` given, or every one its
    /// manifest declares.
    fn grant(&self, name: &str, capabilities: &[String]) -> Result<bool, String> {
        let declared = self.declared(name)?;
        let wanted: Vec<String> = if capabilities.is_empty() {
            declared.clone().ok_or_else(|| {
                format!("{name} is a script, which declares nothing; name the capabilities")
            })?
        } else {
            capabilities.to_vec()
        };
        for cap in &wanted {
            kage_plugin::check_capability(cap)?;
            if declared
                .as_ref()
                .is_some_and(|declared| !declared.contains(cap))
            {
                return Err(format!(
                    "{name} does not declare {cap} in its plugin.toml, so it could never use it"
                ));
            }
        }
        let mut all = self.granted(name);
        all.extend(wanted);
        self.write_grants(name, all)?;
        Ok(true)
    }

    /// Revokes the `capabilities` given from plugin `name`, or all.
    fn revoke(&self, name: &str, capabilities: &[String]) -> Result<bool, String> {
        let mut all = self.granted(name);
        all.retain(|cap| !capabilities.is_empty() && !capabilities.contains(cap));
        self.write_grants(name, all)?;
        Ok(true)
    }

    /// What `[plugins.capabilities]` grants plugin `name` now.
    fn granted(&self, name: &str) -> Vec<String> {
        self.user
            .capabilities
            .get(name)
            .cloned()
            .unwrap_or_default()
    }

    /// The capabilities installed package `name` declares; `None` for
    /// a script.
    fn declared(&self, name: &str) -> Result<Option<Vec<String>>, String> {
        let found = package::discover(&self.plugins_dir).map_err(|e| e.to_string())?;
        for entry in found {
            match entry {
                Found::Package { name: n, dir } if n == name => {
                    return Manifest::read(&dir).map(|m| Some(m.capabilities));
                }
                Found::Script { name: n, .. } if n == name => return Ok(None),
                _ => {}
            }
        }
        Err(format!("no plugin named {name} is installed"))
    }

    fn write_grants(&self, name: &str, mut caps: Vec<String>) -> Result<(), String> {
        caps.sort();
        caps.dedup();
        let value = (!caps.is_empty()).then(|| json!(caps));
        self.write(&["plugins", "capabilities", name], value.as_ref())?;
        if caps.is_empty() {
            println!("{name} holds no capabilities");
        } else {
            println!("{name} may hold: {}", caps.join(", "));
        }
        Ok(())
    }

    /// After an install: lists the declared capabilities not granted
    /// yet, and grants them when `grant` is set or the user says yes.
    fn offer_grants(&self, name: &str, declared: &[String], grant: bool) -> Result<(), String> {
        let granted = self.granted(name);
        let missing: Vec<&String> = declared.iter().filter(|c| !granted.contains(c)).collect();
        if missing.is_empty() {
            return Ok(());
        }
        eprintln!("{name} asks for:");
        for cap in &missing {
            eprintln!(
                "  {cap:<13} {}",
                kage_plugin::capability_summary(cap).unwrap_or_default()
            );
        }
        if grant || ask("Grant them?") {
            let mut all = granted;
            all.extend(missing.into_iter().cloned());
            return self.write_grants(name, all);
        }
        eprintln!(
            "not granted: {name} runs without them. `kage plugin grant {name}` grants them later."
        );
        Ok(())
    }

    /// Turns plugin `name` on or off in `[plugins] disabled`. A config
    /// still on the deprecated `enabled` allowlist is moved over first,
    /// keeping every other plugin as it was.
    fn set_loads(&self, name: &str, on: bool) -> Result<bool, String> {
        package::check_name(name)?;
        let mut disabled = self.user.disabled.clone();
        let legacy = !self.user.enabled.is_empty();
        if legacy {
            let found = package::discover(&self.plugins_dir).map_err(|e| e.to_string())?;
            for entry in found {
                if let Found::Script { name: n, .. } | Found::Package { name: n, .. } = entry
                    && !self.user.enabled.contains(&n)
                {
                    disabled.push(n);
                }
            }
        }
        disabled.retain(|entry| entry != name);
        if !on {
            disabled.push(name.to_owned());
        }
        disabled.sort();
        disabled.dedup();
        let value = (!disabled.is_empty()).then(|| json!(disabled));
        self.write(&["plugins", "disabled"], value.as_ref())?;
        if legacy {
            self.write(&["plugins", "enabled"], None)?;
            eprintln!("kage: moved the deprecated [plugins] enabled list to disabled");
        }
        println!("{name} {}", if on { "enabled" } else { "disabled" });
        if on {
            self.note_disabled(name);
        }
        Ok(true)
    }

    /// Says so when this directory's project config still disables
    /// plugin `name`.
    fn note_disabled(&self, name: &str) {
        if !effective_plugins(&self.user).loads(name) {
            eprintln!("kage: {name} is disabled here; `kage plugin enable {name}` lets it load");
        }
    }

    fn write(&self, keys: &[&str], value: Option<&serde_json::Value>) -> Result<(), String> {
        let keys: Vec<String> = keys.iter().map(|key| (*key).to_owned()).collect();
        crate::rpc::config_set::set(&self.config_path, &keys, value).map_err(|e| e.message)
    }
}

/// The registry `[plugins] registry` in the user config names, kura
/// unless set, with its index cached under the cache root.
pub(crate) fn registry(user: &PluginsConfig) -> Registry {
    let url = user
        .registry
        .clone()
        .unwrap_or_else(|| kage_kura::DEFAULT_REGISTRY.to_owned());
    let cache = crate::cache_root().ok().map(|dir| dir.join("kura"));
    Registry::new(url, cache)
}

/// The plugin settings in force in the current directory, the project
/// config included, or `user` when that cannot be read.
fn effective_plugins(user: &PluginsConfig) -> PluginsConfig {
    std::env::current_dir()
        .ok()
        .and_then(|dir| Config::load_layered_raw(&dir).ok())
        .map_or_else(|| user.clone(), |config| config.plugins)
}

/// `name` or `name@version`.
pub(crate) fn parse_spec(spec: &str) -> Result<(&str, Option<Version>), String> {
    let (name, version) = match spec.split_once('@') {
        Some((name, version)) => {
            let version = Version::parse(version)
                .map_err(|e| format!("`{version}` is not a version: {e}"))?;
            (name, Some(version))
        }
        None => (spec, None),
    };
    package::check_name(name)?;
    Ok((name, version))
}

/// Asks a yes/no question on the terminal; no terminal means no.
fn ask(question: &str) -> bool {
    if !std::io::stdin().is_terminal() {
        return false;
    }
    let mut err = std::io::stderr().lock();
    let _ = write!(err, "{question} [y/N] ");
    let _ = err.flush();
    drop(err);
    let mut line = String::new();
    let _ = std::io::stdin().lock().read_line(&mut line);
    matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

#[cfg(test)]
mod tests {
    use super::parse_spec;

    #[test]
    fn a_spec_names_a_package_and_maybe_a_version() {
        assert_eq!(parse_spec("web").unwrap(), ("web", None));
        let (name, version) = parse_spec("web@1.2.3").unwrap();
        assert_eq!(
            (name, version.unwrap().to_string()),
            ("web", "1.2.3".into())
        );
        assert!(parse_spec("web@latest").is_err());
        assert!(parse_spec("../web").is_err());
    }
}
