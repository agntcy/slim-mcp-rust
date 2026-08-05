// Copyright AGNTCY Contributors (https://github.com/agntcy)
// SPDX-License-Identifier: Apache-2.0

//! Example: MCP client that uses SLIM as the transport layer.
//!
//! Connects to a running MCP server (or proxy) over SLIM and exercises all MCP
//! primitives: tools, resources, and prompts.
//!
//! Usage:
//!   cargo run -p agntcy-slim-mcp-transport --example mcp-slim-client -- \
//!     --local-name org/mcp/client1 \
//!     --server-name org/mcp/proxy \
//!     --shared-secret secretsecretsecretsecretsecretsecret

use std::sync::Arc;

use agntcy_slim_mcp_transport::{IdentityConfig, SlimClientConfig, SlimClientWorker};
use clap::Parser;
use rmcp::model::{
    CallToolRequestParams, GetPromptRequestParams, ReadResourceRequestParams,
    SubscribeRequestParams, UnsubscribeRequestParams,
};
use rmcp::serve_client;
use slim_config::client::ClientConfig;
use slim_config::component::id::ID;
use slim_datapath::api::ProtoName;
use slim_service::ServiceConfiguration;
use tracing::{error, info};

#[derive(Parser, Debug)]
#[command(about = "MCP client over SLIM transport")]
struct Args {
    /// Local client name in the form org/ns/name
    #[arg(long, default_value = "org/mcp/client1")]
    local_name: String,

    /// Remote MCP server name in the form org/ns/name
    #[arg(long, default_value = "org/mcp/proxy")]
    server_name: String,

    /// SLIM server endpoint URL
    #[arg(long, default_value = "http://127.0.0.1:46357")]
    slim_endpoint: String,

    /// Shared secret for authentication
    #[arg(long)]
    shared_secret: Option<String>,

    /// SPIRE Workload API socket path
    #[arg(long)]
    spire_socket_path: Option<String>,

    /// SPIRE target SPIFFE ID
    #[arg(long)]
    spire_target_spiffe_id: Option<String>,

    /// SPIRE JWT audiences (comma-separated)
    #[arg(long)]
    spire_jwt_audiences: Option<String>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    let local_name = match ProtoName::parse_name(&args.local_name) {
        Ok(n) => n,
        Err(e) => { error!("invalid --local-name: {e}"); return; }
    };
    let server_name = match ProtoName::parse_name(&args.server_name) {
        Ok(n) => n,
        Err(e) => { error!("invalid --server-name: {e}"); return; }
    };

    let identity = if let Some(socket_path) = &args.spire_socket_path {
        let audiences = args
            .spire_jwt_audiences
            .as_deref()
            .unwrap_or("slim")
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();
        IdentityConfig::Spire {
            socket_path: Some(socket_path.clone()),
            target_spiffe_id: args.spire_target_spiffe_id.clone(),
            jwt_audiences: audiences,
        }
    } else if let Some(secret) = &args.shared_secret {
        IdentityConfig::shared_secret("client", secret)
    } else {
        error!("provide --shared-secret or --spire-socket-path");
        return;
    };

    // Build a Service that connects to the external SLIM server.
    let mut client_config = ClientConfig::with_endpoint(&args.slim_endpoint);
    client_config.tls_setting.insecure = true;
    let svc_config = ServiceConfiguration::new().with_dataplane_client(vec![client_config]);
    let svc_id = ID::new_with_str("slim/0").unwrap();
    let service = svc_config.build_server(svc_id).expect("failed to build service");

    // Create and subscribe the app — the caller's responsibility before handing
    // it to the transport.
    let (auth_provider, auth_verifier) = identity
        .into_auth_pair()
        .await
        .expect("failed to build auth pair");
    let (app, _app_rx) = service
        .create_app(&local_name, auth_provider, auth_verifier)
        .expect("failed to create app");
    service.run().await.expect("failed to start service");
    let conn_id = service
        .get_connection_id(&service.config().dataplane_clients()[0].endpoint)
        .expect("no connection to SLIM server");
    app.subscribe(&local_name, Some(conn_id))
        .await
        .expect("failed to subscribe");
    app.set_route(&server_name, conn_id)
        .await
        .expect("failed to set route");
    let app = Arc::new(app);

    info!(name = %server_name, "connecting to MCP server via SLIM...");

    let transport = SlimClientWorker::new(SlimClientConfig {
        app,
        destination: server_name,
        session_config: None,
    })
    .into_transport();
    let client = match serve_client((), transport).await {
        Ok(c) => c,
        Err(e) => {
            error!("failed to connect: {e}");
            return;
        }
    };
    let peer = client.peer();

    // list_tools
    match peer.list_tools(None).await {
        Ok(result) => info!("list_tools: {} tool(s): {:?}", result.tools.len(), result.tools.iter().map(|t| &t.name).collect::<Vec<_>>()),
        Err(e) => { error!("list_tools failed: {e}"); return; }
    }

    // call_tool "fetch"
    match peer.call_tool(CallToolRequestParams {
        meta: None,
        name: "fetch".into(),
        arguments: Some(serde_json::json!({"url": "https://example.com"}).as_object().unwrap().clone()),
        task: None,
    }).await {
        Ok(result) => info!("call_tool fetch: {:?}", result.content),
        Err(e) => { error!("call_tool failed: {e}"); return; }
    }

    // list_resources
    match peer.list_resources(None).await {
        Ok(result) => info!("list_resources: {} resource(s)", result.resources.len()),
        Err(e) => { error!("list_resources failed: {e}"); return; }
    }

    // subscribe_resource
    match peer.subscribe(SubscribeRequestParams {
        meta: None,
        uri: "file:///greeting.txt".to_string(),
    }).await {
        Ok(()) => info!("subscribed to file:///greeting.txt"),
        Err(e) => { error!("subscribe failed: {e}"); return; }
    }

    // read_resource
    match peer.read_resource(ReadResourceRequestParams {
        meta: None,
        uri: "file:///greeting.txt".to_string(),
    }).await {
        Ok(result) => info!("read_resource: {} content(s)", result.contents.len()),
        Err(e) => { error!("read_resource failed: {e}"); return; }
    }

    // unsubscribe_resource
    match peer.unsubscribe(UnsubscribeRequestParams {
        meta: None,
        uri: "file:///greeting.txt".to_string(),
    }).await {
        Ok(()) => info!("unsubscribed from file:///greeting.txt"),
        Err(e) => { error!("unsubscribe failed: {e}"); return; }
    }

    // list_prompts
    match peer.list_prompts(None).await {
        Ok(result) => info!("list_prompts: {} prompt(s)", result.prompts.len()),
        Err(e) => { error!("list_prompts failed: {e}"); return; }
    }

    // get_prompt
    match peer.get_prompt(GetPromptRequestParams {
        meta: None,
        name: "simple".to_string(),
        arguments: Some(serde_json::json!({"context": "User is a software developer", "topic": "Rust async programming"}).as_object().unwrap().clone()),
    }).await {
        Ok(result) => info!("get_prompt: {} message(s)", result.messages.len()),
        Err(e) => { error!("get_prompt failed: {e}"); return; }
    }

    info!("all MCP operations completed successfully");
    client.cancellation_token().cancel();
}
