// Copyright AGNTCY Contributors (https://github.com/agntcy)
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;

use rmcp::service::RoleClient;
use rmcp::transport::worker::{Worker, WorkerConfig, WorkerContext, WorkerQuitReason, WorkerTransport};
use slim_datapath::api::{ProtoName, ProtoSessionType};
use slim_session::SessionConfig;

use crate::SlimApp;
use crate::error::SlimTransportError;

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
}

impl SlimClientWorker {
    pub fn new(config: SlimClientConfig) -> Self {
        Self { config }
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
        WorkerConfig {
            name: Some("slim-client".to_string()),
            channel_buffer_capacity: 32,
        }
    }

    async fn run(
        self,
        mut ctx: WorkerContext<Self>,
    ) -> Result<(), WorkerQuitReason<Self::Error>> {
        let SlimClientConfig { app, destination, session_config } = self.config;

        let cfg = session_config.unwrap_or(SessionConfig {
            session_type: ProtoSessionType::PointToPoint,
            initiator: true,
            max_retries: Some(5),
            interval: Some(std::time::Duration::from_secs(1)),
            mls_settings: None,
            metadata: Default::default(),
        });

        let (session_ctx, completion) = app
            .create_session(cfg, destination, None)
            .await
            .map_err(|e| WorkerQuitReason::fatal(SlimTransportError::Session(e.to_string()), "create_session"))?;

        completion
            .await
            .map_err(|e| WorkerQuitReason::fatal(SlimTransportError::Session(e.to_string()), "session completion"))?;

        let (session_weak, mut session_rx) = session_ctx.into_parts();
        let session = session_weak
            .upgrade()
            .ok_or_else(|| WorkerQuitReason::fatal(SlimTransportError::NoSession, "session upgrade"))?;
        let dst = session.dst().clone();

        loop {
            tokio::select! {
                _ = ctx.cancellation_token.cancelled() => {
                    return Err(WorkerQuitReason::Cancelled);
                }

                send_req = ctx.from_handler_rx.recv() => {
                    let req = send_req
                        .ok_or(WorkerQuitReason::HandlerTerminated)?;
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
                            return Err(WorkerQuitReason::fatal(
                                SlimTransportError::Session(e.to_string()),
                                "session recv error",
                            ));
                        }
                        None => return Err(WorkerQuitReason::TransportClosed),
                    }
                }
            }
        }
    }
}
