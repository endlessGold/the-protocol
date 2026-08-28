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
