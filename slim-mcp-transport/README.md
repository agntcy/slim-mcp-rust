# MCP tool-call verifier prototype

Branch `feat/mcp-request-verifier`, based on upstream
`196eb492b8705db1b482301dc1ddd5582947358d`.

This adds an optional sender check to the existing native MCP-over-SLIM client.
It follows SHADi's A2A `AgentVerifier.verify_request()` pattern without importing
SHADi or routing MCP through an A2A channel.

```text
MCP tools/call
  -> McpVerifier.verify_request(context)
  -> explicit approval: publish the original request over SLIM
  -> denial, verifier error or timeout: local JSON-RPC error; nothing published
```

## Usage

Implement `McpVerifier` in the trusted application hosting the transport. The
implementation can evaluate local policy or await an external policy service.
No policy engine, HTTP endpoint or CF dependency is built into this crate.

```rust,ignore
let transport = SlimClientWorker::new(config)
    .with_verifier(Arc::new(policy), Duration::from_secs(2))
    .into_transport();
let client = rmcp::serve_client((), transport).await?;
```

The verifier receives a fresh local evaluation UUID, the original JSON-RPC
request ID, the transport's actual destination, tool name, and optional MCP
`_meta`. Arguments are included only when `wants_content()` returns true.
Caller metadata and request IDs are not authenticated identity claims. The
existing SLIM authentication configuration continues to own peer authentication.

Each call is checked separately, even when a caller reuses a JSON-RPC ID. The
held request is borrowed during evaluation, then serialized without modification
only after approval. A denial returns local JSON-RPC error `-32001` with the
same request ID and leaves the session usable. Error text is fixed; verifier
implementations cannot accidentally return credentials in an error string.

The asynchronous verifier must yield while waiting. The decision deadline
cancels its future; a response arriving later cannot publish the held call.
Dropping a pending transport send or closing the transport also prevents
publication when the check has not yet completed. This cannot retract a call
that was already published.

## Boundaries and issues to resolve

- Opt-in, sender-side `tools/call` authorization only. Initialization, discovery,
  resources, prompts, notifications, replies and server-initiated sampling are
  unchanged. There is no receiver gate.
- Existing MCP clients must adopt this guarded transport. This is not automatic
  interception of Python clients, other SLIM apps, or arbitrary agents. A trusted
  proxy and suitable routing/containment would be needed for deployment-only
  adoption. The separate `mcp-proxy` binary is not wired to this sender verifier.
- The worker already serializes outgoing messages. A slow authorization also
  delays receiving on that session until the bounded check completes. Its queue
  remains bounded by rmcp's worker capacity (16); the decision timeout starts
  when the call is dequeued, not when the application first submits it.
- `_meta.traceparent` is preserved when supplied. The prototype neither creates
  application spans nor ensures their evidence is queryable in OXP. An external
  CF integration must establish that readiness before asking for disposition.
- There is no external policy-service client, mTLS integration, audit delivery or
  sandbox setup in this change. Those belong to the trusted embedding host.
  Our existing enforcement API also needs an MCP-specific identity/route mapping
  and metadata profile; labeling these tool calls as A2A would be incorrect.
- Upstream MCP transport currently uses SLIM 2.0.0 / rmcp 3.1.1, while latest
  SHADi uses SLIM 3.4.3. The tests below do not qualify interoperability between
  those versions, a deployed SLIM node, or MLS.

## Related upstream work

| Issue or PR | Relationship |
| --- | --- |
| [SHADi #369](https://github.com/agntcy/shadi/issues/369), completed in [#405](https://github.com/agntcy/shadi/pull/405) | Existing A2A per-request verifier; the model followed here. |
| [SHADi #370](https://github.com/agntcy/shadi/issues/370), completed | Local evaluation IDs and avoiding pre-check payload logging; followed here. |
| [SHADi #363](https://github.com/agntcy/shadi/issues/363), open | Policy epic explicitly excludes a built-in external call-out protocol. This prototype exposes a generic verifier only; upstream acceptance still needs discussion. |
| [SHADi #337](https://github.com/agntcy/shadi/issues/337), closed | Original host-configurable hook request, redirected into the existing policy mechanisms. |
| [SHADi #367](https://github.com/agntcy/shadi/issues/367), open | Destination policy at every A2A egress point; relevant precedent for route checks, not an MCP implementation. |
| [SHADi #368](https://github.com/agntcy/shadi/issues/368), open | A2A content rules; not implemented as MCP content checks here. |
| [SHADi #378](https://github.com/agntcy/shadi/issues/378), open | Reusable A2A executor; not a prerequisite for native MCP checks. |
| [slim-mcp-rust #43](https://github.com/agntcy/slim-mcp-rust/issues/43), open | Native MCP transport RFC. |
| [slim-mcp-rust #45](https://github.com/agntcy/slim-mcp-rust/pull/45), merged | Added the existing client/server transport this prototype extends. |
| [slim-mcp-python #15](https://github.com/agntcy/slim-mcp-python/issues/15), closed | Proposal about per-message signatures, schema pinning and replay protection. Those are separate from authorization; the issue's security claims were not validated by this prototype. |

No dedicated existing MCP per-call authorization issue was found in SHADi or
the SLIM MCP repos during the October 19, 2026 review. No issue or PR has been
opened for this prototype.

## Validation

```bash
cargo test --locked -p agntcy-slim-mcp-transport
```

The verifier tests use actual authenticated, in-process SLIM sessions and an MCP
server. Independent counters measure both received tool requests and executed
tools. They cover allow, deny, verifier failure, timeout and late approval,
repeated IDs, argument opt-in, metadata preservation, cancellation, session
reuse, and concurrent verdict/reply correlation. They do not use a simulated
transport, but they also do not exercise a remote node, CF/OXP or a sandbox.
