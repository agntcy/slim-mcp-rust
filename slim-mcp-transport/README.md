# agntcy-slim-mcp-transport

A [SLIM](https://github.com/agntcy/slim) transport for [rmcp](https://crates.io/crates/rmcp), the MCP Rust SDK. MCP clients and servers reach each other by SLIM name through a SLIM node, with no HTTP endpoint to expose.

Sessions use MLS by default: messages are encrypted end to end, and the server's tools get the caller's verified identity.

## Quick start

```toml
[dependencies]
agntcy-slim-mcp-transport = "0.1"
rmcp = { version = "3.8", features = ["client", "server"] }
tokio = { version = "1", features = ["full"] }
```

Both sides connect to a SLIM node with a name and an identity. These examples use a node without TLS on `localhost:46357` and a shared secret read from `SLIM_SHARED_SECRET`, which must be at least 32 bytes.

A server serves every session opened to its name:

```rust,no_run
use agntcy_slim_mcp_transport::{IdentityConfig, ProtoName, SlimConnection, insecure_client_config};
use rmcp::{ServerHandler, serve_server};

#[derive(Clone)]
struct MyServer;

impl ServerHandler for MyServer {}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let connection = SlimConnection::connect(
        insecure_client_config("http://localhost:46357"),
        ProtoName::from_strings(["org", "ns", "mcp-server"]),
        IdentityConfig::shared_secret("mcp-server", std::env::var("SLIM_SHARED_SECRET")?),
    )
    .await?;

    let mut listener = connection.into_listener();
    while let Some(transport) = listener.accept().await {
        let transport = transport?;
        tokio::spawn(async move {
            if let Ok(server) = serve_server(MyServer, transport).await {
                let _ = server.waiting().await;
            }
        });
    }
    Ok(())
}
```

A client opens a session to the server's name:

```rust,no_run
use agntcy_slim_mcp_transport::{IdentityConfig, ProtoName, SlimConnection, insecure_client_config};
use rmcp::serve_client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let connection = SlimConnection::connect(
        insecure_client_config("http://localhost:46357"),
        ProtoName::from_strings(["org", "ns", "mcp-client"]),
        IdentityConfig::shared_secret("mcp-client", std::env::var("SLIM_SHARED_SECRET")?),
    )
    .await?;

    let transport = connection
        .client_transport(ProtoName::from_strings(["org", "ns", "mcp-server"]))
        .await?;
    let client = serve_client((), transport).await?;
    println!("{:#?}", client.list_tools(Default::default()).await?);
    client.cancel().await?;
    Ok(())
}
```

## Identity and MLS

- With MLS, each request a tool handles carries the caller's verified identity: `context.extensions.get::<PeerIdentity>()` on its `RequestContext`. Use it to authorize calls.
- To talk to a peer that doesn't use MLS, open the session with `client_transport_with_config` and `mls_settings: None`. Tools then get no identity.
- `IdentityConfig` also supports SPIRE. For a node with TLS, build the `ClientConfig` with its TLS settings instead of `insecure_client_config`.

## Examples

[`examples/`](https://github.com/agntcy/slim-mcp-rust/tree/main/slim-mcp-transport/examples) has a server and a client that run over SLIM or HTTP. The server also runs behind [`mcp-proxy`](https://github.com/agntcy/slim-mcp-rust/tree/main/mcp-proxy), which exposes an existing HTTP MCP server over SLIM without changing it.
