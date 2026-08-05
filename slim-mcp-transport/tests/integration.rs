// Copyright AGNTCY Contributors (https://github.com/agntcy)
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;

use agntcy_slim_mcp_transport::{IdentityConfig, SlimClientConfig, SlimClientWorker, SlimServerListener};
use rmcp::{ServerHandler, serve_client, serve_server};
use slim_config::component::id::ID;
use slim_datapath::api::{ProtoName, ProtoSessionType};
use slim_service::Service;
use slim_session::SessionConfig;

/// A minimal MCP server that returns no tools, resources, or prompts.
#[derive(Clone)]
struct MinimalServer;

impl ServerHandler for MinimalServer {}

#[tokio::test]
async fn mcp_list_tools_roundtrip_over_slim() {
    let svc_id = ID::new_with_str("slim/test-svc").unwrap();
    let service = Arc::new(Service::new(svc_id));

    let server_name = ProtoName::from_strings(["test", "ns", "server"]);
    let client_name = ProtoName::from_strings(["test", "ns", "client"]);
    // SharedSecret::new requires a secret of at least 32 bytes.
    let auth = IdentityConfig::shared_secret("test", "test-shared-secret-value-0123456789abcdef");

    // Create and subscribe the server app.
    let (server_auth_provider, server_auth_verifier) =
        auth.clone().into_auth_pair().await.expect("server auth failed");
    let (server_app, server_rx) = service
        .create_app(&server_name, server_auth_provider, server_auth_verifier)
        .expect("server create_app failed");
    server_app
        .subscribe(&server_name, None)
        .await
        .expect("server subscribe failed");
    let server_app = Arc::new(server_app);

    // Create and subscribe the client app.
    let (client_auth_provider, client_auth_verifier) =
        auth.into_auth_pair().await.expect("client auth failed");
    let (client_app, _client_rx) = service
        .create_app(&client_name, client_auth_provider, client_auth_verifier)
        .expect("client create_app failed");
    client_app
        .subscribe(&client_name, None)
        .await
        .expect("client subscribe failed");
    let client_app = Arc::new(client_app);

    // Bind the server listener.
    let mut listener = SlimServerListener::new(server_app, server_rx);

    // Create the client transport.
    let session_cfg = SessionConfig {
        session_type: ProtoSessionType::PointToPoint,
        initiator: true,
        max_retries: Some(3),
        interval: Some(std::time::Duration::from_millis(100)),
        mls_settings: None,
        metadata: Default::default(),
    };
    let client_transport = SlimClientWorker::new(SlimClientConfig {
        app: client_app,
        destination: server_name,
        session_config: Some(session_cfg),
    })
    .into_transport();

    // Accept the session on the server side concurrently with serving the client.
    let server_task = tokio::spawn(async move {
        let transport = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            async { listener.accept().await.expect("accept returned None").expect("accept returned error") },
        )
        .await
        .expect("server accept timed out");

        serve_server(MinimalServer, transport)
            .await
            .expect("serve_server failed")
    });

    // serve_client drives the MCP initialization handshake.
    let client = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        serve_client((), client_transport),
    )
    .await
    .expect("serve_client timed out")
    .expect("serve_client failed");

    // Exercise the protocol.
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        client.peer().list_tools(None),
    )
    .await
    .expect("list_tools timed out")
    .expect("list_tools failed");

    assert!(result.tools.is_empty(), "expected no tools from MinimalServer");

    client.cancellation_token().cancel();
    server_task.await.expect("server task panicked").cancellation_token().cancel();
}
