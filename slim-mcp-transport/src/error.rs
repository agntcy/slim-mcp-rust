// Copyright AGNTCY Contributors (https://github.com/agntcy)
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, thiserror::Error)]
pub enum SlimTransportError {
    #[error("transport closed")]
    Closed,

    #[error("join error: {0}")]
    Join(#[from] tokio::task::JoinError),

    #[error("SLIM session error: {0}")]
    Session(String),

    #[error("SLIM service error: {0}")]
    Service(String),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("auth error: {0}")]
    Auth(String),

    #[error("no session available")]
    NoSession,
}
