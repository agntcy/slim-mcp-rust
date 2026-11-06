// Copyright AGNTCY Contributors (https://github.com/agntcy)
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;

use slim_config::client::ClientConfig;
use slim_config::component::id::ID;
use slim_datapath::api::ProtoName;
use slim_service::Service;
use slim_session::SessionError;
use slim_session::notification::Notification;
use tokio::sync::mpsc;

use crate::SlimApp;
use crate::auth::IdentityConfig;
use crate::client::{SlimClientConfig, SlimClientTransport, SlimClientWorker};
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

    /// Opens a client transport to the MCP server at `destination`, with a
    /// point-to-point session.
    pub async fn client_transport(
        &self,
        destination: ProtoName,
    ) -> Result<SlimClientTransport, SlimTransportError> {
        self.app
            .set_route(&destination, self.conn_id)
            .await
            .map_err(service_error)?;
        let config = SlimClientConfig {
            app: self.app.clone(),
            destination,
            session_config: None,
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
