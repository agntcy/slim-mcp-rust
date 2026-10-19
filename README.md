# slim-mcp-rust

The `feat/mcp-request-verifier` branch includes an experimental
[per-tool-call sender verifier](slim-mcp-transport/README.md).

This repo contains Rust integrations between SLIM and MCP.

Current integrations:

- mcp-proxy: An efficient proxy server that connects existing MCP servers into the SLIM network without modifying the MCP server code.

Coming soon:

- A custom transport for MCP rust-sdk that allows MCP servers written in Rust to connect directly to SLIM network.
