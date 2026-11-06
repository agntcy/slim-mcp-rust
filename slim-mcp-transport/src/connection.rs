// Copyright AGNTCY Contributors (https://github.com/agntcy)
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;

use slim_config::client::ClientConfig;
use slim_config::component::id::ID;
use slim_datapath::api::ProtoName;
use slim_service::Service;
use slim_session::notification::Notification;
use slim_session::{SessionConfig, SessionError};
use tokio::sync::mpsc;

use crate::SlimApp;
use crate::auth::IdentityConfig;
use crate::client::{
    SlimClientConfig, SlimClientTransport, SlimClientWorker, default_session_config,
};
use crate::error::SlimTransportError;
use crate::server::SlimServerListener;

/// Client configuration for a SLIM node reached without TLS, such as
/// `http://localhost:46357`.
pub fn insecure_client_config(endpoint: &str) -> ClientConfig {
    let mut config = ClientConfig::with_endpoint(endpoint);
    config.tls_setting.insecure = true;
    config
}

/// A SLIM app connected to a SLIM node and subscribed to its name.
///
/// Dropping the SLIM service disconnects it from the node, so the connection
/// and every transport made from it hold a reference to the service.
pub struct SlimConnection {
    service: Arc<Service>,
    app: Arc<SlimApp>,
    notifications: mpsc::Receiver<Result<Notification, SessionError>>,
    name: ProtoName,
    conn_id: u64,
}

impl SlimConnection {
    /// Connects to the SLIM node in `client_config`, creates an app for
    /// `name` with `identity`, and subscribes it to `name`.
    pub async fn connect(
        client_config: ClientConfig,
        name: ProtoName,
        identity: IdentityConfig,
    ) -> Result<Self, SlimTransportError> {
        let id = ID::new_with_str("slim/0").map_err(service_error)?;
        let service = Arc::new(Service::new(id));
        let (provider, verifier) = identity.into_auth_pair().await?;
        let (app, notifications) = service
            .create_app(&name, provider, verifier)
            .map_err(service_error)?;
        let conn_id = service
            .connect(&client_config)
            .await
            .map_err(service_error)?;
        app.subscribe(&name, Some(conn_id))
            .await
            .map_err(service_error)?;

        Ok(Self {
            service,
            app: Arc::new(app),
            notifications,
            name,
            conn_id,
        })
    }

    /// The name this connection is subscribed to.
    pub fn name(&self) -> &ProtoName {
        &self.name
    }

    /// Opens a client transport to the MCP server at `destination`, with
    /// [`default_session_config`]: a point-to-point session with MLS.
    pub async fn client_transport(
        &self,
        destination: ProtoName,
    ) -> Result<SlimClientTransport, SlimTransportError> {
        self.client_transport_with_config(destination, default_session_config())
            .await
    }

    /// Opens a client transport to the MCP server at `destination`, with
    /// `session_config`. Use it to turn MLS off for a peer that doesn't use
    /// it.
    pub async fn client_transport_with_config(
        &self,
        destination: ProtoName,
        session_config: SessionConfig,
    ) -> Result<SlimClientTransport, SlimTransportError> {
        self.app
            .set_route(&destination, self.conn_id)
            .await
            .map_err(service_error)?;
        let config = SlimClientConfig {
            app: self.app.clone(),
            destination,
            session_config: Some(session_config),
        };
        Ok(SlimClientWorker::with_service(config, self.service.clone()).into_transport())
    }

    /// Turns this connection into a listener for MCP sessions opened to its
    /// name.
    pub fn into_listener(self) -> SlimServerListener {
        SlimServerListener::with_service(self.app, self.notifications, self.service)
    }
}

fn service_error(e: impl std::fmt::Display) -> SlimTransportError {
    SlimTransportError::Service(e.to_string())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use slim_config::server::ServerConfig;
    use slim_config::tls::server::TlsServerConfig;
    use slim_session::PeerIdentity;

    use super::*;

    const SECRET: &str = "test-shared-secret-value-0123456789abcdef";

    /// Opens a session from a client to a server through an in-process SLIM
    /// node, and returns the server's view of it: its configuration and the
    /// client's verified identity.
    async fn accepted_session(
        session_config: Option<SessionConfig>,
    ) -> (SessionConfig, Option<PeerIdentity>) {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .expect("no free port")
            .port();
        let node = Service::new(ID::new_with_str("slim/node").unwrap());
        node.run_server(
            &ServerConfig::with_endpoint(&format!("127.0.0.1:{port}"))
                .with_tls_settings(TlsServerConfig::insecure()),
        )
        .await
        .expect("SLIM node failed to start");
        let endpoint = format!("http://127.0.0.1:{port}");
        let identity = IdentityConfig::shared_secret("test", SECRET);

        let server = SlimConnection::connect(
            insecure_client_config(&endpoint),
            ProtoName::from_strings(["test", "ns", "server"]),
            identity.clone(),
        )
        .await
        .expect("server failed to connect");
        let server_name = server.name().clone();
        let mut listener = server.into_listener();

        let client = SlimConnection::connect(
            insecure_client_config(&endpoint),
            ProtoName::from_strings(["test", "ns", "client"]),
            identity,
        )
        .await
        .expect("client failed to connect");
        let _transport = match session_config {
            None => client.client_transport(server_name).await,
            Some(config) => {
                client
                    .client_transport_with_config(server_name, config)
                    .await
            }
        }
        .expect("client transport failed");

        let ctx = tokio::time::timeout(Duration::from_secs(10), listener.next_session())
            .await
            .expect("no session within 10s")
            .expect("listener closed")
            .expect("listener error");
        let session = ctx.session_arc().expect("session gone");
        (session.session_config(), session.peer_identity())
    }

    #[tokio::test]
    async fn client_sessions_use_mls_by_default() {
        let (config, peer) = accepted_session(None).await;
        assert!(config.mls_settings.is_some(), "session without MLS");
        assert!(peer.is_some(), "no verified client identity");
    }

    #[tokio::test]
    async fn client_can_turn_mls_off() {
        let mut config = default_session_config();
        config.mls_settings = None;
        let (config, peer) = accepted_session(Some(config)).await;
        assert!(config.mls_settings.is_none(), "session with MLS");
        assert!(peer.is_none(), "verified identity without MLS");
    }
}
