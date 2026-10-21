//! `_kage/registry/search` and `_kage/registry/install`: the plugin
//! registry for clients, over the same [`kage_kura`] client and user
//! config as `kage plugin`.

use kage_acp::acp::{
    RegistryCapability, RegistryInstallRequest, RegistryInstallResult, RegistryPackage,
    RegistrySearchRequest, RegistrySearchResult,
};
use kage_core::config::Config;
use kage_jsonrpc::RpcError;
use kage_kura::{Index, Lock, Registry};

fn invalid(message: impl Into<String>) -> RpcError {
    RpcError::new(-32602, message.into())
}

fn user_registry() -> Result<Registry, RpcError> {
    let user = Config::load_default()
        .map_err(|e| RpcError::internal(format!("config: {e}")))?
        .plugins;
    Ok(crate::plugin_cmd::registry(&user))
}

pub(super) fn search(req: &RegistrySearchRequest) -> Result<RegistrySearchResult, RpcError> {
    let registry = user_registry()?;
    let fetched = registry
        .index(req.refresh)
        .map_err(|e| RpcError::internal(e.to_string()))?;
    let dir = crate::plugins_dir().map_err(RpcError::internal)?;
    let lock = Lock::load(&dir).unwrap_or_default();
    let packages = fetched
        .index
        .search(&req.query)
        .into_iter()
        .map(|(name, package)| {
            let (version, unavailable, capabilities) = match fetched.index.resolve(name, None) {
                Ok(release) => (
                    Some(release.version.to_string()),
                    None,
                    release
                        .capabilities
                        .iter()
                        .map(|cap| RegistryCapability {
                            name: cap.clone(),
                            summary: kage_plugin::capability_summary(cap)
                                .unwrap_or_default()
                                .to_owned(),
                        })
                        .collect(),
                ),
                Err(err) => (None, Some(err.to_string()), Vec::new()),
            };
            RegistryPackage {
                name: name.to_owned(),
                description: package.description.clone().unwrap_or_default(),
                tags: package.tags.clone().unwrap_or_default(),
                homepage: package.homepage.clone(),
                version,
                unavailable,
                capabilities,
                installed: lock.plugins.get(name).map(|l| l.version.to_string()),
            }
        })
        .collect();
    Ok(RegistrySearchResult {
        registry: registry.url().to_owned(),
        stale: fetched.stale,
        packages,
    })
}

pub(super) fn install(req: &RegistryInstallRequest) -> Result<RegistryInstallResult, RpcError> {
    let spec = match &req.version {
        Some(version) => format!("{}@{version}", req.name),
        None => req.name.clone(),
    };
    let (name, version) = crate::plugin_cmd::parse_spec(&spec).map_err(invalid)?;
    let registry = user_registry()?;
    let index = |refresh: bool| -> Result<Index, RpcError> {
        registry
            .index(refresh)
            .map(|fetched| fetched.index)
            .map_err(|e| RpcError::internal(e.to_string()))
    };
    let mut current = index(false)?;
    if matches!(
        current.resolve(name, version.as_ref()),
        Err(kage_kura::Error::NotFound(_))
    ) {
        current = index(true)?;
    }
    let release = current
        .resolve(name, version.as_ref())
        .map_err(|e| invalid(e.to_string()))?;
    let dir = crate::plugins_dir().map_err(RpcError::internal)?;
    let installed = registry
        .install(&dir, &current, name, release, req.force)
        .map_err(|e| match e {
            kage_kura::Error::Conflict(_)
            | kage_kura::Error::Integrity(_)
            | kage_kura::Error::NeedsNewerKage { .. } => invalid(e.to_string()),
            other => RpcError::internal(other.to_string()),
        })?;
    Ok(RegistryInstallResult {
        name: installed.name,
        version: installed.version.to_string(),
        previous: installed.previous.map(|v| v.to_string()),
        capabilities: installed.capabilities,
    })
}
