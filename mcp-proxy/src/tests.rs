// Copyright AGNTCY Contributors (https://github.com/agntcy)
// SPDX-License-Identifier: Apache-2.0

//! A client of `agntcy-slim-mcp-transport` calls an MCP server over HTTP
//! through the proxy, with and without MLS.

use std::sync::Arc;
use std::time::Duration;

use agntcy_slim_mcp_transport::{
    IdentityConfig, SessionConfig, SlimConnection, default_session_config, insecure_client_config,
};
use rmcp::handler::server::{router::tool::ToolRouter, wrapper::Parameters};
use rmcp::model::{CallToolRequestParams, ServerCapabilities, ServerConfig};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{ServerHandler, serve_client, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use slim_config::component::id::ID;
use slim_config::server::ServerConfig as SlimServerConfig;
use slim_config::tls::server::TlsServerConfig;
use slim_datapath::api::proto::dataplane::v1::Name;
use slim_service::{Service, ServiceConfiguration};

use crate::proxy::{IdentityConfig as ProxyIdentity, Proxy};

const SECRET: &str = "test-shared-secret-value-0123456789abcdef";

#[derive(Deserialize, JsonSchema)]
struct EchoParams {
    text: String,
}

/// An MCP server with one tool, `echo`, that returns its text.
#[derive(Clone)]
struct EchoServer {
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl EchoServer {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "Returns its text")]
    async fn echo(&self, Parameters(EchoParams { text }): Parameters<EchoParams>) -> String {
        text
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for EchoServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }
}

/// Starts `EchoServer` over HTTP on a free port and returns its URL.
async fn start_http_mcp_server() -> String {
    let service = StreamableHttpService::new(
        || Ok(EchoServer::new()),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default().disable_allowed_hosts(),
    );
    let router = axum::Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind the MCP server");
    let addr = listener.local_addr().expect("no local address");
    tokio::spawn(async move { axum::serve(listener, router).await });
    format!("http://{addr}/mcp")
}

/// Starts a SLIM node on a free port and returns it with its endpoint.
/// Dropping the node stops it.
async fn start_node() -> (Service, String) {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .expect("no free port")
        .port();
    let node = Service::new(ID::new_with_str("slim/node").unwrap());
    node.run_server(
        &SlimServerConfig::with_endpoint(&format!("127.0.0.1:{port}"))
            .with_tls_settings(TlsServerConfig::insecure()),
    )
    .await
    .expect("SLIM node failed to start");
    (node, format!("http://127.0.0.1:{port}"))
}

/// Calls `echo` through the proxy from a transport client whose session uses
/// `session_config`, and returns the answer.
async fn echo_through_the_proxy(session_config: SessionConfig, text: &str) -> String {
    let (_node, endpoint) = start_node().await;
    let mcp_server = start_http_mcp_server().await;

    let proxy_name = Name::from_strings(["test", "ns", "proxy"]);
    let proxy_service = ServiceConfiguration::new()
        .with_dataplane_client(vec![insecure_client_config(&endpoint)])
        .build_server(ID::new_with_str("slim/proxy").unwrap())
        .expect("failed to build the proxy's SLIM service");
    let proxy = tokio::spawn({
        let proxy_name = proxy_name.clone();
        async move {
            Proxy::new(proxy_name, mcp_server)
                .start(
                    proxy_service,
                    ProxyIdentity::SharedSecret(SECRET.to_string()),
                    Duration::from_secs(1),
                )
                .await
        }
    });

    let connection = SlimConnection::connect(
        insecure_client_config(&endpoint),
        Name::from_strings(["test", "ns", "client"]),
        IdentityConfig::shared_secret("client", SECRET),
    )
    .await
    .expect("client failed to connect");
    let transport = connection
        .client_transport_with_config(proxy_name, session_config)
        .await
        .expect("client transport failed");
    let client = serve_client((), transport)
        .await
        .expect("serve_client failed");

    let args = serde_json::json!({ "text": text });
    let result = client
        .call_tool(
            CallToolRequestParams::new("echo").with_arguments(args.as_object().unwrap().clone()),
        )
        .await
        .expect("call_tool failed");

    client.cancel().await.expect("client task panicked");
    proxy.abort();
    result
        .content
        .first()
        .and_then(|content| content.as_text())
        .map(|content| content.text.clone())
        .expect("echo returned no text")
}

#[tokio::test]
async fn a_transport_client_calls_an_http_server_through_the_proxy() {
    let answer = tokio::time::timeout(
        Duration::from_secs(30),
        echo_through_the_proxy(default_session_config(), "hello over MLS"),
    )
    .await
    .expect("no answer within 30s");
    assert_eq!(answer, "hello over MLS");
}

#[tokio::test]
async fn a_transport_client_without_mls_calls_through_the_proxy() {
    let mut config = default_session_config();
    config.mls_settings = None;
    let answer = tokio::time::timeout(
        Duration::from_secs(30),
        echo_through_the_proxy(config, "hello without MLS"),
    )
    .await
    .expect("no answer within 30s");
    assert_eq!(answer, "hello without MLS");
}
