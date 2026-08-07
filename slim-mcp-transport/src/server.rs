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
use tracing::{debug, warn};

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
                    debug!("incoming SLIM session accepted");
                    let worker = SlimServerWorker {
                        ctx,
                        _app: self.app.clone(),
                    };
                    return Some(Ok(WorkerTransport::spawn(worker)));
                }
                Ok(Notification::NewMessage(_)) => continue,
                Err(e) => {
                    warn!(error = %e, "SLIM listener error");
                    return Some(Err(SlimTransportError::Session(e.to_string())));
                }
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
        let mut cfg = WorkerConfig::default();
        cfg.name = Some("slim-server".to_string());
        cfg
    }

    async fn run(
        self,
        mut ctx: WorkerContext<Self>,
    ) -> Result<(), WorkerQuitReason<Self::Error>> {
        let session = self
            .ctx
            .session_arc()
            .ok_or_else(|| WorkerQuitReason::fatal(SlimTransportError::NoSession, "session upgrade"))?;
        let dst = session.dst().clone();
        debug!(%dst, "SLIM server session started");

        let (err_tx, mut err_rx) =
            tokio::sync::mpsc::channel::<WorkerQuitReason<SlimTransportError>>(2);

        // Capture what the receiver task needs before spawn_receiver consumes self.ctx.
        let to_handler_tx = ctx.to_handler_tx.clone();
        let ct_rx = ctx.cancellation_token.clone();
        let ct = ctx.cancellation_token.clone();
        let err_tx_rx = err_tx.clone();
        let dst_rx = dst.clone();

        self.ctx.spawn_receiver(move |mut rx, _session_weak| async move {
            loop {
                tokio::select! {
                    _ = ct_rx.cancelled() => return,
                    incoming = rx.recv() => {
                        match incoming {
                            Some(Ok(msg)) => {
                                debug!(%dst_rx, "received message from SLIM");
                                let content = match msg.get_payload() {
                                    Some(c) => c,
                                    None => {
                                        let _ = err_tx_rx.send(WorkerQuitReason::fatal(
                                            SlimTransportError::Session("missing payload".into()),
                                            "get_payload",
                                        )).await;
                                        return;
                                    }
                                };
                                let payload = match content.as_application_payload() {
                                    Ok(p) => p,
                                    Err(e) => {
                                        let _ = err_tx_rx.send(WorkerQuitReason::fatal(
                                            SlimTransportError::Session(e.to_string()),
                                            "decode payload",
                                        )).await;
                                        return;
                                    }
                                };
                                let json_msg = match serde_json::from_slice(&payload.blob) {
                                    Ok(m) => m,
                                    Err(e) => {
                                        let _ = err_tx_rx.send(WorkerQuitReason::fatal(
                                            SlimTransportError::Serde(e),
                                            "deserialize incoming",
                                        )).await;
                                        return;
                                    }
                                };
                                if to_handler_tx.send(json_msg).await.is_err() {
                                    let _ = err_tx_rx.send(WorkerQuitReason::HandlerTerminated).await;
                                    return;
                                }
                            }
                            Some(Err(e)) => {
                                warn!(%dst_rx, error = %e, "SLIM session error");
                                let _ = err_tx_rx.send(WorkerQuitReason::fatal(
                                    SlimTransportError::Session(e.to_string()),
                                    "session recv error",
                                )).await;
                                return;
                            }
                            None => {
                                debug!(%dst_rx, "SLIM session closed");
                                let _ = err_tx_rx.send(WorkerQuitReason::TransportClosed).await;
                                return;
                            }
                        }
                    }
                }
            }
        });

        let dst_tx = dst.clone();
        let err_tx_tx = err_tx.clone();
        let session_for_close = session.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = ctx.cancellation_token.cancelled() => return,
                    send_req = ctx.from_handler_rx.recv() => {
                        let req = match send_req {
                            Some(r) => r,
                            None => {
                                let _ = err_tx_tx.send(WorkerQuitReason::HandlerTerminated).await;
                                return;
                            }
                        };
                        debug!(%dst_tx, message = ?req.message, "sending message to SLIM");
                        let bytes = match serde_json::to_vec(&req.message) {
                            Ok(b) => b,
                            Err(e) => {
                                let _ = err_tx_tx.send(WorkerQuitReason::fatal(
                                    SlimTransportError::Serde(e),
                                    "serialize outgoing",
                                )).await;
                                return;
                            }
                        };
                        match session.publish(&dst_tx, bytes, None, None).await {
                            Ok(_) => {
                                debug!(%dst_tx, message = ?req.message, "successfully sent message to SLIM");
                                let _ = req.responder.send(Ok(()));
                            }
                            Err(e) => {
                                let _ = err_tx_tx.send(WorkerQuitReason::fatal(
                                    SlimTransportError::Session(e.to_string()),
                                    "publish",
                                )).await;
                                return;
                            }
                        }
                    }
                }
            }
        });

        drop(err_tx);

        tokio::select! {
            _ = ct.cancelled() => {
                debug!(%dst, "cancellation received, closing SLIM session");
                if let Ok(completion) = session_for_close.close()
                    && let Err(e) = completion.await
                {
                    warn!(%dst, error = %e, "error draining SLIM session on close");
                }
                Err(WorkerQuitReason::Cancelled)
            }
            reason = err_rx.recv() => Err(reason.unwrap_or(WorkerQuitReason::TransportClosed)),
        }
    }
}
