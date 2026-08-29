use std::sync::Arc;

use async_trait::async_trait;
use dashmap::DashMap;
use thiserror::Error;

use protocol_protocol::{Command, CommandResponse};

#[derive(Debug, Error)]
pub enum RoutingError {
    #[error("Unknown command: {0}")]
    UnknownCommand(String),

    #[error("Handler error: {0}")]
    HandlerError(String),
}

#[async_trait]
pub trait CommandHandler: Send + Sync {
    async fn handle(
        &self,
        command: Command,
        session_id: u64,
    ) -> Result<CommandResponse, RoutingError>;
}

pub struct CommandRouter {
    handlers: DashMap<String, Arc<dyn CommandHandler>>,
}

impl CommandRouter {
    pub fn new() -> Self {
        Self {
            handlers: DashMap::new(),
        }
    }

    pub fn register(&self, command_type: &str, handler: Arc<dyn CommandHandler>) {
        self.handlers.insert(command_type.to_string(), handler);
        tracing::debug!("Registered command handler: {}", command_type);
    }

    pub async fn route(
        &self,
        command: Command,
        session_id: u64,
    ) -> Result<CommandResponse, RoutingError> {
        let handler = self
            .handlers
            .get(&command.command_type)
            .ok_or_else(|| RoutingError::UnknownCommand(command.command_type.clone()))?;

        handler.handle(command, session_id).await
    }
}

impl Default for CommandRouter {
    fn default() -> Self {
        Self::new()
    }
}
