# slim-mcp-rust

Rust integrations between [SLIM](https://github.com/agntcy/slim) and [MCP](https://modelcontextprotocol.io):

- [`agntcy-slim-mcp-transport`](slim-mcp-transport): a SLIM transport for [rmcp](https://crates.io/crates/rmcp), the MCP Rust SDK. Rust MCP clients and servers connect to SLIM directly. It's on [crates.io](https://crates.io/crates/agntcy-slim-mcp-transport).
- [`mcp-proxy`](mcp-proxy): connects an existing MCP server to SLIM without changing its code. It's published as the [`agntcy-slim-mcp-proxy`](https://crates.io/crates/agntcy-slim-mcp-proxy) crate, the `ghcr.io/agntcy/slim-mcp-rust/mcp-proxy` image, and a [Helm chart](charts/mcp-proxy) at `oci://ghcr.io/agntcy/slim-mcp-rust/helm/mcp-proxy`.

## Which one to use

Use the transport when the MCP server or client is written in Rust with rmcp. There's no proxy in between, sessions use MLS by default, and tools get the caller's verified identity.

Use `mcp-proxy` for an MCP server you can't or don't want to change. It can be written in any language, as long as it serves Streamable HTTP: the proxy takes SLIM sessions and forwards their messages to the server's HTTP endpoint.

## Transport

```toml
[dependencies]
agntcy-slim-mcp-transport = "0.1"
rmcp = { version = "3.9", features = ["client"] }
tokio = { version = "1", features = ["full"] }
```

A client connects to a SLIM node, then opens a session to the server's SLIM name:

```rust,no_run
use agntcy_slim_mcp_transport::{IdentityConfig, ProtoName, SlimConnection, insecure_client_config};

let connection = SlimConnection::connect(
    insecure_client_config("http://localhost:46357"),
    ProtoName::from_strings(["org", "ns", "mcp-client"]),
    IdentityConfig::shared_secret("mcp-client", std::env::var("SLIM_SHARED_SECRET")?),
)
.await?;

let transport = connection
    .client_transport(ProtoName::from_strings(["org", "ns", "mcp-server"]))
    .await?;
let client = rmcp::serve_client((), transport).await?;
```

The [transport README](slim-mcp-transport/README.md) covers the server side, identity and MLS options, and the examples.
