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

use agntcy_slim_mcp_transport::{
    IdentityConfig, ProtoName, SlimConnection, insecure_client_config,
};
use clap::Parser;
use rmcp::ClientLifecycleMode;
use rmcp::ClientServiceExt;
use rmcp::model::{
    CallToolRequestParams, ClientConfig as RmcpClientConfig, GetPromptRequestParams,
    ProtocolVersion, ReadResourceRequestParams, SubscriptionFilter,
};
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
        Err(e) => {
            error!("invalid --local-name: {e}");
            return;
        }
    };
    let server_name = match ProtoName::parse_name(&args.server_name) {
        Ok(n) => n,
        Err(e) => {
            error!("invalid --server-name: {e}");
            return;
        }
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

    let connection = SlimConnection::connect(
        insecure_client_config(&args.slim_endpoint),
        local_name,
        identity,
    )
    .await
    .expect("failed to connect to SLIM");

    info!(name = %server_name, "connecting to MCP server via SLIM...");

    let transport = connection
        .client_transport(server_name)
        .await
        .expect("failed to open the client transport");

    let client_info = RmcpClientConfig::default();
    //).with_protocol_version(ProtocolVersion::V_2026_07_28);
    let client = match client_info
        .serve_with_lifecycle(
            transport,
            ClientLifecycleMode::Auto {
                preferred_versions: vec![ProtocolVersion::V_2026_07_28],
                legacy_version: Some(ProtocolVersion::V_2025_11_25),
            },
        )
        .await
    {
        Ok(c) => c,
        Err(e) => {
            error!("failed to connect: {e}");
            return;
        }
    };

    let server_info = client.peer_info();
    info!("connected to server: {server_info:#?}");

    // list_tools
    let tools = match client.list_tools(Default::default()).await {
        Ok(result) => result.tools,
        Err(e) => {
            error!("list_tools failed: {e}");
            let _ = client.cancel().await;
            return;
        }
    };
    println!("=== list_tools ({} tool(s)) ===", tools.len());
    for tool in &tools {
        println!(
            "  - {} : {}",
            tool.name,
            tool.description.as_deref().unwrap_or("")
        );
    }

    // call_tool "fetch"
    let fetch_result = match client
        .call_tool(
            CallToolRequestParams::new("fetch").with_arguments(
                serde_json::json!({"url": "https://example.com"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
    {
        Ok(result) => result,
        Err(e) => {
            error!("call_tool failed: {e}");
            let _ = client.cancel().await;
            return;
        }
    };
    println!("=== call_tool(fetch) ===");
    for item in &fetch_result.content {
        println!("  {:?}", item);
    }

    // list_resources
    let resources = match client.list_resources(Default::default()).await {
        Ok(result) => result.resources,
        Err(e) => {
            error!("list_resources failed: {e}");
            let _ = client.cancel().await;
            return;
        }
    };
    println!("=== list_resources ({} resource(s)) ===", resources.len());
    for r in &resources {
        println!("  - {} ({})", r.name, r.uri);
    }

    // subscribe_resource
    let _subscription = match client
        .listen(
            SubscriptionFilter::builder()
                .resource_subscription("file:///greeting.txt")
                .build(),
        )
        .await
    {
        Ok(sub) => {
            println!("=== subscribe(file:///greeting.txt) ok ===");
            sub
        }
        Err(e) => {
            error!("subscribe failed: {e}");
            let _ = client.cancel().await;
            return;
        }
    };

    // read_resource
    let read_result = match client
        .read_resource(ReadResourceRequestParams::new("file:///greeting.txt"))
        .await
    {
        Ok(result) => result,
        Err(e) => {
            error!("read_resource failed: {e}");
            let _ = client.cancel().await;
            return;
        }
    };
    println!("=== read_resource(file:///greeting.txt) ===");
    for content in &read_result.contents {
        println!("  {:?}", content);
    }

    // subscription is cancelled automatically when _subscription is dropped

    // list_prompts
    let prompts = match client.list_prompts(Default::default()).await {
        Ok(result) => result.prompts,
        Err(e) => {
            error!("list_prompts failed: {e}");
            let _ = client.cancel().await;
            return;
        }
    };
    println!("=== list_prompts ({} prompt(s)) ===", prompts.len());
    for p in &prompts {
        println!(
            "  - {} : {}",
            p.name,
            p.description.as_deref().unwrap_or("")
        );
    }

    // get_prompt
    let prompt_result = match client.get_prompt(
        GetPromptRequestParams::new("simple")
            .with_arguments(serde_json::json!({"context": "User is a software developer", "topic": "Rust async programming"}).as_object().unwrap().clone()),
    ).await {
        Ok(result) => result,
        Err(e) => { error!("get_prompt failed: {e}"); let _ = client.cancel().await; return; }
    };
    println!(
        "=== get_prompt(simple) ({} message(s)) ===",
        prompt_result.messages.len()
    );
    for msg in &prompt_result.messages {
        println!("  [{:?}] {:?}", msg.role, msg.content);
    }

    println!("=== all MCP operations completed successfully ===");
    if let Err(e) = client.cancel().await {
        error!("cancel failed: {e}");
    }
}
