// Copyright AGNTCY Contributors (https://github.com/agntcy)
// SPDX-License-Identifier: Apache-2.0

use std::{sync::Arc, time::Duration};

use rmcp::model::{ClientRequest, ErrorCode, ErrorData, JsonRpcError, JsonRpcMessage};
use rmcp::service::RoleClient;
use rmcp::transport::worker::{
    Worker, WorkerConfig, WorkerContext, WorkerQuitReason, WorkerTransport,
};
use slim_datapath::api::{ProtoName, ProtoSessionType};
use slim_session::SessionConfig;

use crate::SlimApp;
use crate::error::SlimTransportError;
use crate::verifier::{McpRequestContext, McpVerifier, VerificationError};
use tracing::{debug, warn};

/// A `Transport<RoleClient>` backed by a single outgoing SLIM session.
pub type SlimClientTransport = WorkerTransport<SlimClientWorker>;

/// Configuration for [`SlimClientWorker`].
pub struct SlimClientConfig {
    pub app: Arc<SlimApp>,
    pub destination: ProtoName,
    /// Session configuration; defaults to point-to-point initiated by this side.
    pub session_config: Option<SessionConfig>,
}

/// Worker driving a single client-side SLIM session as an MCP transport.
pub struct SlimClientWorker {
    config: SlimClientConfig,
    verifier: Option<(Arc<dyn McpVerifier>, Duration)>,
}

impl SlimClientWorker {
    pub fn new(config: SlimClientConfig) -> Self {
        Self {
            config,
            verifier: None,
        }
    }

    /// Authorize each `tools/call` before publishing it. Other MCP methods keep
    /// their existing behavior. Existing callers remain unguarded unless opted in.
    pub fn with_verifier(mut self, verifier: Arc<dyn McpVerifier>, timeout: Duration) -> Self {
        self.verifier = Some((verifier, timeout));
        self
    }

    /// Convenience: spawn the transport immediately.
    pub fn into_transport(self) -> SlimClientTransport {
        WorkerTransport::spawn(self)
    }
}

impl Worker for SlimClientWorker {
    type Error = SlimTransportError;
    type Role = RoleClient;

    fn err_closed() -> Self::Error {
        SlimTransportError::Closed
    }

    fn err_join(e: tokio::task::JoinError) -> Self::Error {
        SlimTransportError::Join(e)
    }

    fn config(&self) -> WorkerConfig {
        let mut cfg = WorkerConfig::default();
        cfg.name = Some("slim-client".to_string());
        cfg
    }

    async fn run(self, mut ctx: WorkerContext<Self>) -> Result<(), WorkerQuitReason<Self::Error>> {
        let verifier = self.verifier;
        let SlimClientConfig {
            app,
            destination,
            session_config,
        } = self.config;

        let cfg = session_config.unwrap_or(SessionConfig {
            session_type: ProtoSessionType::PointToPoint,
            initiator: true,
            max_retries: Some(5),
            interval: Some(std::time::Duration::from_secs(1)),
            mls_settings: None,
            metadata: Default::default(),
        });

        debug!(%destination, "creating SLIM session");
        let (session_ctx, completion) =
            app.create_session(cfg, destination, None)
                .await
                .map_err(|e| {
                    WorkerQuitReason::fatal(
                        SlimTransportError::Session(e.to_string()),
                        "create_session",
                    )
                })?;

        completion.await.map_err(|e| {
            WorkerQuitReason::fatal(
                SlimTransportError::Session(e.to_string()),
                "session completion",
            )
        })?;

        let (session_weak, mut session_rx) = session_ctx.into_parts();
        let session = session_weak.upgrade().ok_or_else(|| {
            WorkerQuitReason::fatal(SlimTransportError::NoSession, "session upgrade")
        })?;
        let dst = session.dst().clone();
        debug!(%dst, "SLIM session established");

        loop {
            tokio::select! {
                _ = ctx.cancellation_token.cancelled() => {
                    return Err(WorkerQuitReason::Cancelled);
                }

                send_req = ctx.from_handler_rx.recv() => {
                    let req = send_req
                        .ok_or(WorkerQuitReason::HandlerTerminated)?;
                    if req.responder.is_closed() {
                        continue;
                    }
                    if let (Some((verifier, budget)), JsonRpcMessage::Request(request)) = (&verifier, &req.message)
                        && let ClientRequest::CallToolRequest(call) = &request.request
                    {
                        let context = McpRequestContext {
                            evaluation_id: uuid::Uuid::new_v4(),
                            request_id: &request.id,
                            destination: &dst,
                            tool_name: &call.params.name,
                            metadata: call.params.meta.as_ref(),
                            arguments: verifier.wants_content().then_some(call.params.arguments.as_ref()).flatten(),
                        };
                        let deadline = tokio::time::Instant::now() + *budget;
                        let result = tokio::select! {
                            biased;
                            _ = ctx.cancellation_token.cancelled() => return Err(WorkerQuitReason::Cancelled),
                            result = tokio::time::timeout_at(deadline, verifier.verify_request(&context)) => {
                                result.unwrap_or(Err(VerificationError::Timeout))
                            }
                        };
                        let result = if tokio::time::Instant::now() >= deadline {
                            Err(VerificationError::Timeout)
                        } else {
                            result
                        };
                        if let Err(error) = result {
                            let response = JsonRpcMessage::Error(JsonRpcError::new(
                                Some(request.id.clone()),
                                ErrorData::new(ErrorCode(-32001), error.to_string(), None),
                            ));
                            if req.responder.send(Ok(())).is_ok() {
                                ctx.send_to_handler(response).await?;
                            }
                            continue;
                        }
                    }
                    // A canceled send must not be resumed by a late approval.
                    if ctx.cancellation_token.is_cancelled() {
                        return Err(WorkerQuitReason::Cancelled);
                    }
                    if req.responder.is_closed() {
                        continue;
                    }
                    debug!(%dst, "sending message to SLIM");
                    let bytes = serde_json::to_vec(&req.message)
                        .map_err(|e| WorkerQuitReason::fatal(SlimTransportError::Serde(e), "serialize outgoing"))?;
                    session
                        .publish(&dst, bytes, None, None)
                        .await
                        .map_err(|e| WorkerQuitReason::fatal(SlimTransportError::Session(e.to_string()), "publish"))?;
                    let _ = req.responder.send(Ok(()));
                }

                incoming = session_rx.recv() => {
                    match incoming {
                        Some(Ok(msg)) => {
                            debug!(%dst, "received message from SLIM");
                            let content = msg
                                .get_payload()
                                .ok_or_else(|| WorkerQuitReason::fatal(
                                    SlimTransportError::Session("missing payload".into()),
                                    "get_payload",
                                ))?;
                            let payload = content
                                .as_application_payload()
                                .map_err(|e| WorkerQuitReason::fatal(
                                    SlimTransportError::Session(e.to_string()),
                                    "decode payload",
                                ))?;
                            let json_msg = serde_json::from_slice(&payload.blob)
                                .map_err(|e| WorkerQuitReason::fatal(SlimTransportError::Serde(e), "deserialize incoming"))?;
                            ctx.send_to_handler(json_msg).await?;
                        }
                        Some(Err(e)) => {
                            warn!(%dst, error = %e, "SLIM session error");
                            return Err(WorkerQuitReason::fatal(
                                SlimTransportError::Session(e.to_string()),
                                "session recv error",
                            ));
                        }
                        None => {
                            debug!(%dst, "SLIM session closed");
                            return Err(WorkerQuitReason::TransportClosed);
                        }
                    }
                }
            }
        }
    }
}
