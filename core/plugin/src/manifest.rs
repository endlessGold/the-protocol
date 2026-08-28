use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub description: String,
    pub api_version: String,
    pub permissions: PluginPermissions,
    pub resources: PluginResources,
    #[serde(default)]
    pub dependencies: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginPermissions {
    #[serde(default)]
    pub required: Vec<String>,
    #[serde(default)]
    pub optional: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginResources {
    #[serde(default = "default_memory_limit")]
    pub memory_limit: u32,
    #[serde(default = "default_fuel_limit")]
    pub fuel_limit: u64,
}

fn default_memory_limit() -> u32 {
    64 * 1024 * 1024
}

fn default_fuel_limit() -> u64 {
    1_000_000_000
}

impl Default for PluginResources {
    fn default() -> Self {
        Self {
            memory_limit: default_memory_limit(),
            fuel_limit: default_fuel_limit(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginState {
    Discovered,
    Loaded,
    Initialized,
    Enabled,
    Disabled,
    Error(String),
}

/// The plugin host API version this runtime implements. Plugins declare the
/// version they were built against via `plugin.toml`'s `api_version`; see
/// `validate_api_version`. Bump the MINOR component when adding host
/// functions, the MAJOR component when changing/removing existing ones.
pub const RUNTIME_API_VERSION: &str = "0.1.0";

/// Compare a plugin's declared `api_version` against the runtime's, per the
/// compatibility rules in docs/02-plugin/03-plugin-api-contract.md §8.3:
/// - different MAJOR -> reject (the plugin was built for an incompatible ABI)
/// - same MAJOR, plugin MINOR <= runtime MINOR -> compatible
/// - same MAJOR, plugin MINOR > runtime MINOR -> compatible, but warn (the
///   plugin may call host functions this runtime doesn't implement yet)
///
/// This was designed (see the doc above) but never actually called anywhere
/// - `PluginEngine::compile()` loaded a manifest's `api_version` field and
/// then ignored it, so an incompatible plugin would load silently and only
/// fail later, opaquely, the first time it called a host function that
/// doesn't exist. Wired into `compile()` so incompatible plugins are
/// rejected up front with a clear error instead.
///
/// Versions are parsed leniently (`MAJOR.MINOR[.PATCH...]`); a missing or
/// non-numeric segment is treated as `0` rather than rejected outright, since
/// a malformed version string is a plugin-authoring mistake this check isn't
/// meant to catch.
pub fn validate_api_version(
    plugin_version: &str,
    runtime_version: &str,
) -> Result<(), crate::error::PluginError> {
    fn parts(v: &str) -> (u32, u32) {
        let mut segments = v.split('.');
        let major = segments.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let minor = segments.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        (major, minor)
    }

    let (plugin_major, plugin_minor) = parts(plugin_version);
    let (runtime_major, runtime_minor) = parts(runtime_version);

    if plugin_major != runtime_major {
        return Err(crate::error::PluginError::IncompatibleApiVersion {
            plugin: plugin_version.to_string(),
            required: runtime_version.to_string(),
        });
    }

    if plugin_minor > runtime_minor {
        tracing::warn!(
            "Plugin declares api_version {} newer (minor) than runtime's {} - it may call host functions this runtime doesn't implement yet",
            plugin_version,
            runtime_version
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_matching_major_lower_or_equal_minor() {
        assert!(validate_api_version("0.1.0", "0.1.0").is_ok());
        assert!(validate_api_version("0.0.5", "0.1.0").is_ok());
    }

    #[test]
    fn warns_but_accepts_newer_minor() {
        assert!(validate_api_version("0.2.0", "0.1.0").is_ok());
    }

    #[test]
    fn rejects_different_major() {
        assert!(matches!(
            validate_api_version("1.0.0", "0.1.0"),
            Err(crate::error::PluginError::IncompatibleApiVersion { .. })
        ));
    }

    #[test]
    fn tolerates_malformed_versions_as_zero() {
        assert!(validate_api_version("not-a-version", "0.1.0").is_ok());
    }
}
