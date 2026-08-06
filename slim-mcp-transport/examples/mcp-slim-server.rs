// Copyright AGNTCY Contributors (https://github.com/agntcy)
// SPDX-License-Identifier: Apache-2.0

//! Example: MCP server that can be run over HTTP or natively over SLIM.
//!
//! HTTP mode (compatible with slim-mcp-proxy):
//!   cargo run -p agntcy-slim-mcp-transport --example mcp-slim-server -- --transport http --port 8000
//!
//! Native SLIM mode (direct client↔server, no proxy):
//!   cargo run -p agntcy-slim-mcp-transport --example mcp-slim-server -- \
//!     --transport slim --local-name org/mcp/server \
//!     --shared-secret secretsecretsecretsecretsecretsecret

use std::sync::Arc;

use agntcy_slim_mcp_transport::{IdentityConfig, SlimServerListener};
use clap::{Parser, ValueEnum};
use rmcp::{
    ErrorData, ServerHandler,
    handler::server::{router::prompt::PromptRouter, router::tool::ToolRouter, wrapper::Parameters},
    model::{
        GetPromptRequestParams, GetPromptResult, ListPromptsResult, ListResourcesResult,
        PaginatedRequestParams, PromptMessage, PromptMessageRole,
        RawResource, ReadResourceRequestParams, ReadResourceResult, Resource,
        ResourceContents, ServerCapabilities, ServerInfo, SubscribeRequestParams,
        UnsubscribeRequestParams,
    },
    prompt, prompt_handler, prompt_router,
    service::RequestContext,
    serve_server,
    tool, tool_handler, tool_router,
};
use rmcp::service::RoleServer;
use rmcp::transport::{
    StreamableHttpServerConfig, StreamableHttpService,
    streamable_http_server::session::local::LocalSessionManager,
};
use schemars::JsonSchema;
use serde::Deserialize;
use slim_config::client::ClientConfig;
use slim_config::component::id::ID;
use slim_datapath::api::ProtoName;
use slim_service::ServiceConfiguration;
use tracing::{error, info};

// ── CLI ───────────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(about = "MCP server — HTTP or native SLIM transport")]
struct Args {
    /// Transport to use: http or slim
    #[arg(long, default_value = "http")]
    transport: TransportMode,

    /// HTTP listen port (http mode)
    #[arg(long, default_value = "8000")]
    port: u16,

    /// Local server name in the form org/ns/name (slim mode)
    #[arg(long, default_value = "org/mcp/server")]
    local_name: String,

    /// SLIM server endpoint URL (slim mode)
    #[arg(long, default_value = "http://127.0.0.1:46357")]
    slim_endpoint: String,

    /// Shared secret for authentication (slim mode)
    #[arg(long)]
    shared_secret: Option<String>,

    /// SPIRE Workload API socket path (slim mode)
    #[arg(long)]
    spire_socket_path: Option<String>,

    /// SPIRE target SPIFFE ID (slim mode)
    #[arg(long)]
    spire_target_spiffe_id: Option<String>,

    /// SPIRE JWT audiences, comma-separated (slim mode)
    #[arg(long)]
    spire_jwt_audiences: Option<String>,
}

#[derive(ValueEnum, Clone, Debug)]
enum TransportMode {
    Http,
    Slim,
}

// ── Static resources ──────────────────────────────────────────────────────────

const RESOURCES: &[(&str, &str, &str)] = &[
    ("greeting", "file:///greeting.txt", "Hello! This is a sample text resource."),
    ("help", "file:///help.txt", "This server provides a few sample text resources for testing."),
    ("about", "file:///about.txt", "This is the simple-resource MCP server implementation."),
];

// ── Server handler ────────────────────────────────────────────────────────────

#[derive(Clone)]
struct McpServer {
    tool_router: ToolRouter<Self>,
    prompt_router: PromptRouter<Self>,
}

impl McpServer {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
            prompt_router: Self::prompt_router(),
        }
    }
}

// ── Tool: fetch ───────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
struct FetchParams {
    #[schemars(description = "URL to fetch")]
    url: String,
}

#[tool_router]
impl McpServer {
    #[tool(description = "Fetches a website and returns its content")]
    async fn fetch(&self, Parameters(FetchParams { url }): Parameters<FetchParams>) -> String {
        match reqwest::get(&url).await {
            Ok(resp) => resp.text().await.unwrap_or_else(|e| format!("error reading body: {e}")),
            Err(e) => format!("error fetching {url}: {e}"),
        }
    }
}

// ── Prompt: simple ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
struct SimplePromptParams {
    #[schemars(description = "Additional context to consider")]
    context: Option<String>,
    #[schemars(description = "Specific topic to focus on")]
    topic: Option<String>,
}

#[prompt_router]
impl McpServer {
    #[prompt(description = "A simple prompt that can take optional context and topic arguments")]
    fn simple(&self, Parameters(SimplePromptParams { context, topic }): Parameters<SimplePromptParams>) -> Vec<PromptMessage> {
        let mut messages = Vec::new();
        if let Some(ctx) = context {
            messages.push(PromptMessage::new_text(PromptMessageRole::User, ctx));
        }
        let body = match topic {
            Some(t) => format!("Please help me with the following topic: {t}"),
            None => "Please help me with whatever questions I may have.".to_string(),
        };
        messages.push(PromptMessage::new_text(PromptMessageRole::User, body));
        messages
    }
}

// ── ServerHandler ─────────────────────────────────────────────────────────────

#[tool_handler(router = self.tool_router)]
#[prompt_handler(router = self.prompt_router)]
impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .enable_prompts()
                .build(),
            ..Default::default()
        }
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        Ok(ListResourcesResult::with_all_items(
            RESOURCES.iter().map(|(name, uri, _)| {
                Resource::new(
                    RawResource {
                        uri: (*uri).to_string(),
                        name: (*name).to_string(),
                        title: None,
                        description: None,
                        mime_type: Some("text/plain".to_string()),
                        size: None,
                        icons: None,
                        meta: None,
                    },
                    None,
                )
            }).collect(),
        ))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, ErrorData> {
        match RESOURCES.iter().find(|(_, uri, _)| *uri == request.uri) {
            Some((_, uri, text)) => Ok(ReadResourceResult {
                contents: vec![ResourceContents::TextResourceContents {
                    uri: (*uri).to_string(),
                    mime_type: Some("text/plain".to_string()),
                    text: (*text).to_string(),
                    meta: None,
                }],
            }),
            None => Err(ErrorData::resource_not_found(
                format!("resource not found: {}", request.uri),
                None,
            )),
        }
    }

    async fn subscribe(
        &self,
        _request: SubscribeRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        Ok(())
    }

    async fn unsubscribe(
        &self,
        _request: UnsubscribeRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        Ok(())
    }
}

// ── main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    match args.transport {
        TransportMode::Http => run_http(args.port).await,
        TransportMode::Slim => run_slim(args).await,
    }
}

async fn run_http(port: u16) {
    let service = StreamableHttpService::new(
        || Ok(McpServer::new()),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );
    let router = axum::Router::new().nest_service("/mcp", service);
    let addr = format!("127.0.0.1:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("failed to bind TCP listener");
    info!("MCP server listening on http://{addr}/mcp");
    axum::serve(listener, router).await.expect("HTTP server error");
}

async fn run_slim(args: Args) {
    let local_name = match ProtoName::parse_name(&args.local_name) {
        Ok(n) => n,
        Err(e) => { error!("invalid --local-name: {e}"); return; }
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
        IdentityConfig::shared_secret("server", secret)
    } else {
        error!("provide --shared-secret or --spire-socket-path for slim transport");
        return;
    };

    let mut client_config = ClientConfig::with_endpoint(&args.slim_endpoint);
    client_config.tls_setting.insecure = true;
    let svc_config = ServiceConfiguration::new().with_dataplane_client(vec![client_config]);
    let svc_id = ID::new_with_str("slim/0").unwrap();
    let service = svc_config.build_server(svc_id).expect("failed to build service");

    let (auth_provider, auth_verifier) = identity
        .into_auth_pair()
        .await
        .expect("failed to build auth pair");
    let (app, app_rx) = service
        .create_app(&local_name, auth_provider, auth_verifier)
        .expect("failed to create app");
    service.run().await.expect("failed to start service");
    let conn_id = service
        .get_connection_id(&service.config().dataplane_clients()[0].endpoint)
        .expect("no connection to SLIM server");
    app.subscribe(&local_name, Some(conn_id))
        .await
        .expect("failed to subscribe");
    let app = Arc::new(app);

    info!(name = %local_name, "MCP server listening for SLIM sessions");

    let mut listener = SlimServerListener::new(app, app_rx);
    loop {
        match listener.accept().await {
            Some(Ok(transport)) => {
                tokio::spawn(async move {
                    if let Err(e) = serve_server(McpServer::new(), transport).await {
                        error!("session error: {e}");
                    }
                });
            }
            Some(Err(e)) => error!("accept error: {e}"),
            None => {
                info!("listener closed");
                break;
            }
        }
    }
}
