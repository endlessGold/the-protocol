use thiserror::Error;

#[derive(Debug, Error)]
pub enum PluginError {
    #[error("Plugin not found: {0}")]
    NotFound(String),

    #[error("Incompatible API version: plugin {plugin} requires {required}")]
    IncompatibleApiVersion {
        plugin: String,
        required: String,
    },

    #[error("Permission denied: plugin {plugin} cannot use {permission}")]
    PermissionDenied {
        plugin: String,
        permission: String,
    },

    #[error("Plugin initialization failed: {0}")]
    InitFailed(String),

    #[error("WASM compilation error: {0}")]
    Compilation(String),

    #[error("WASM instantiation error: {0}")]
    Instantiation(String),

    #[error("WASM runtime error: {0}")]
    RuntimeError(String),

    #[error("Function not exported: {0}")]
    FunctionNotFound(String),

    #[error("Memory error: {0}")]
    Memory(String),

    #[error("Buffer error: {0}")]
    Buffer(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Plugin lifecycle error: {0}")]
    Lifecycle(String),
}
