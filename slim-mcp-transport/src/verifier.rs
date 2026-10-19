// Copyright AGNTCY Contributors (https://github.com/agntcy)
// SPDX-License-Identifier: Apache-2.0

//! Optional authorization of outgoing MCP tool calls, before SLIM publication.
use async_trait::async_trait;
use rmcp::model::{JsonObject, RequestId, RequestMetaObject};
use slim_datapath::api::ProtoName;
use uuid::Uuid;

/// Context for one held tool call. Request IDs and metadata are caller supplied;
/// the evaluation ID is generated locally and the destination comes from transport config.
pub struct McpRequestContext<'a> {
    pub evaluation_id: Uuid,
    pub request_id: &'a RequestId,
    pub destination: &'a ProtoName,
    pub tool_name: &'a str,
    pub metadata: Option<&'a RequestMetaObject>,
    /// Tool arguments are exposed only when the verifier requests content.
    pub arguments: Option<&'a JsonObject>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum VerificationError {
    #[error("MCP tool call denied")]
    Denied,
    #[error("MCP authorization unavailable")]
    Unavailable,
    #[error("MCP authorization timed out")]
    Timeout,
}

/// Return `Ok(())` only to permit this tool call. An implementation may consult
/// an external policy service; this crate defines no call-out protocol.
///
/// Implementations must yield while waiting and stop work when their future is
/// dropped. The transport bounds this future; no late approval can publish a call.
#[async_trait]
pub trait McpVerifier: Send + Sync {
    async fn verify_request(
        &self,
        request: &McpRequestContext<'_>,
    ) -> Result<(), VerificationError>;

    fn wants_content(&self) -> bool {
        false
    }
}
