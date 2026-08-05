// Copyright AGNTCY Contributors (https://github.com/agntcy)
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;

use rmcp::service::RoleServer;
use rmcp::transport::worker::{Worker, WorkerConfig, WorkerContext, WorkerQuitReason, WorkerTransport};
use slim_session::SessionError;
use slim_session::context::SessionContext;
use slim_session::notification::Notification;
use tokio::sync::mpsc;

use crate::SlimApp;
use crate::error::SlimTransportError;

/// A Transport<RoleServer> for one accepted incoming SLIM session.
pub type SlimServerTransport = WorkerTransport<SlimServerWorker>;

/// Listens for incoming SLIM sessions and yields a [`SlimServerTransport`] per session.
pub struct SlimServerListener {
    slim_rx: mpsc::Receiver<Result<Notification, SessionError>>,
    app: Arc<SlimApp>,
}

impl SlimServerListener {
    /// Create a listener from a pre-built, pre-subscribed SLIM app.
    ///
    /// The caller is responsible for calling [`slim_service::Service::run`] and
    /// subscribing the app to its name before passing it here.
    pub fn new(
        app: Arc<SlimApp>,
        slim_rx: mpsc::Receiver<Result<Notification, SessionError>>,
    ) -> Self {
        Self { slim_rx, app }
    }

    /// Accept the next incoming session. Returns `None` when the listener is closed.
    ///
    /// Each returned transport holds an `Arc` to the underlying `App` so the
    /// message-processing loop stays alive for the full session lifetime.
    pub async fn accept(&mut self) -> Option<Result<SlimServerTransport, SlimTransportError>> {
        loop {
            match self.slim_rx.recv().await? {
                Ok(Notification::NewSession(ctx)) => {
                    let worker = SlimServerWorker {
                        ctx,
                        _app: self.app.clone(),
                    };
                    return Some(Ok(WorkerTransport::spawn(worker)));
                }
                Ok(Notification::NewMessage(_)) => continue,
                Err(e) => return Some(Err(SlimTransportError::Session(e.to_string()))),
            }
        }
    }
}

/// Worker driving a single server-side SLIM session as an MCP transport.
pub struct SlimServerWorker {
    pub(crate) ctx: SessionContext,
    // Keeps the App's message-processing loop alive for the session lifetime.
    _app: Arc<SlimApp>,
}

impl Worker for SlimServerWorker {
    type Error = SlimTransportError;
    type Role = RoleServer;

    fn err_closed() -> Self::Error {
        SlimTransportError::Closed
    }

    fn err_join(e: tokio::task::JoinError) -> Self::Error {
        SlimTransportError::Join(e)
    }

    fn config(&self) -> WorkerConfig {
        WorkerConfig {
            name: Some("slim-server".to_string()),
            channel_buffer_capacity: 32,
        }
    }

    async fn run(
        self,
        mut ctx: WorkerContext<Self>,
    ) -> Result<(), WorkerQuitReason<Self::Error>> {
        let (session_weak, mut session_rx) = self.ctx.into_parts();
        let session = session_weak
            .upgrade()
            .ok_or_else(|| WorkerQuitReason::fatal(SlimTransportError::NoSession, "session upgrade"))?;
        let dst = session.dst().clone();
        let mut incoming_conn_id: Option<u64> = None;

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

                    let result = if let Some(conn) = incoming_conn_id {
                        session.publish_to(&dst, conn, bytes, None, None).await
                    } else {
                        session.publish(&dst, bytes, None, None).await
                    };
                    result.map_err(|e| WorkerQuitReason::fatal(SlimTransportError::Session(e.to_string()), "publish"))?;
                    let _ = req.responder.send(Ok(()));
                }

                incoming = session_rx.recv() => {
                    match incoming {
                        Some(Ok(msg)) => {
                            if incoming_conn_id.is_none() {
                                incoming_conn_id = Some(msg.get_incoming_conn());
                            }
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
