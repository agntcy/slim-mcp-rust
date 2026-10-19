// Copyright AGNTCY Contributors (https://github.com/agntcy)
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use agntcy_slim_mcp_transport::{
    IdentityConfig, McpRequestContext, McpVerifier, SlimClientConfig, SlimClientWorker,
    SlimServerListener, VerificationError,
};
use async_trait::async_trait;
use rmcp::{
    ErrorData, ServerHandler,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ServerCapabilities,
        ServerInfo,
    },
    serve_client, serve_server,
    service::{RequestContext, RoleServer},
    transport::Transport,
};
use slim_config::component::id::ID;
use slim_datapath::api::{ProtoName, ProtoSessionType};
use slim_service::Service;
use slim_session::SessionConfig;
use tokio::sync::{Mutex, Notify};

#[derive(Default)]
struct Counters {
    received: AtomicUsize,
    executed: AtomicUsize,
}

struct CountingServer(Arc<Counters>);

impl ServerHandler for CountingServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        self.0.executed.fetch_add(1, Ordering::SeqCst);
        Ok(CallToolResult::success(vec![ContentBlock::text(request.name)]).into())
    }
}

struct CountingTransport<T> {
    inner: T,
    counts: Arc<Counters>,
}

impl<T: Transport<RoleServer>> Transport<RoleServer> for CountingTransport<T> {
    type Error = T::Error;

    fn send(
        &mut self,
        message: rmcp::model::ServerJsonRpcMessage,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        self.inner.send(message)
    }

    async fn receive(&mut self) -> Option<rmcp::model::ClientJsonRpcMessage> {
        let message = self.inner.receive().await?;
        if let rmcp::model::JsonRpcMessage::Request(request) = &message
            && matches!(
                request.request,
                rmcp::model::ClientRequest::CallToolRequest(_)
            )
        {
            self.counts.received.fetch_add(1, Ordering::SeqCst);
        }
        Some(message)
    }

    async fn close(&mut self) -> Result<(), Self::Error> {
        self.inner.close().await
    }
}

#[derive(Default)]
struct Policy {
    seen: Mutex<Vec<(uuid::Uuid, String, String, bool)>>,
    include_arguments: bool,
    entered: Notify,
    late_approvals: AtomicUsize,
}

#[async_trait]
impl McpVerifier for Policy {
    async fn verify_request(
        &self,
        request: &McpRequestContext<'_>,
    ) -> Result<(), VerificationError> {
        assert_eq!(
            request.destination,
            &ProtoName::from_strings(["test", "ns", "server"])
        );
        if let Some(metadata) = request.metadata {
            assert_eq!(
                serde_json::to_value(metadata).unwrap()["traceparent"],
                "00-0123456789abcdef0123456789abcdef-0123456789abcdef-01"
            );
        }
        self.seen.lock().await.push((
            request.evaluation_id,
            request.request_id.to_string(),
            request.tool_name.into(),
            request.arguments.is_some(),
        ));
        self.entered.notify_one();
        match request.tool_name {
            "deny" => Err(VerificationError::Denied),
            "unavailable" => Err(VerificationError::Unavailable),
            "slow" => {
                tokio::time::sleep(Duration::from_millis(150)).await;
                self.late_approvals.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn wants_content(&self) -> bool {
        self.include_arguments
    }
}

// Uses real SLIM sessions and an independent server execution counter. No TCP,
// CF service, sandbox, or MLS enforcement is implied by these in-process tests.
async fn fixture(
    policy: Arc<Policy>,
    budget: Duration,
) -> (
    agntcy_slim_mcp_transport::SlimClientTransport,
    Arc<Counters>,
    tokio::task::JoinHandle<()>,
) {
    let service = Arc::new(Service::new(
        ID::new_with_str("slim/verifier-test").unwrap(),
    ));
    let server_name = ProtoName::from_strings(["test", "ns", "server"]);
    let client_name = ProtoName::from_strings(["test", "ns", "client"]);
    let auth = IdentityConfig::shared_secret("test", "test-shared-secret-value-0123456789abcdef");
    let (provider, verifier) = auth.clone().into_auth_pair().await.unwrap();
    let (server_app, server_rx) = service
        .create_app(&server_name, provider, verifier)
        .unwrap();
    server_app.subscribe(&server_name, None).await.unwrap();
    let (provider, verifier) = auth.into_auth_pair().await.unwrap();
    let (client_app, _) = service
        .create_app(&client_name, provider, verifier)
        .unwrap();
    client_app.subscribe(&client_name, None).await.unwrap();
    let mut listener = SlimServerListener::new(Arc::new(server_app), server_rx);
    let transport = SlimClientWorker::new(SlimClientConfig {
        app: Arc::new(client_app),
        destination: server_name,
        session_config: Some(SessionConfig {
            session_type: ProtoSessionType::PointToPoint,
            initiator: true,
            max_retries: Some(3),
            interval: Some(Duration::from_millis(100)),
            mls_settings: None,
            metadata: Default::default(),
        }),
    })
    .with_verifier(policy, budget)
    .into_transport();
    let executions = Arc::new(Counters::default());
    let counter = executions.clone();
    let server = tokio::spawn(async move {
        let _service = service;
        let transport = listener.accept().await.unwrap().unwrap();
        let transport = CountingTransport {
            inner: transport,
            counts: counter.clone(),
        };
        if let Ok(running) = serve_server(CountingServer(counter), transport).await {
            let _ = running.waiting().await;
        }
    });
    (transport, executions, server)
}

#[tokio::test]
async fn allow_deny_and_failure_do_not_break_the_session() {
    let policy = Arc::new(Policy::default());
    let (transport, executions, server) = fixture(policy.clone(), Duration::from_millis(20)).await;
    let client = tokio::time::timeout(Duration::from_secs(5), serve_client((), transport))
        .await
        .unwrap()
        .unwrap();
    // Initialization and discovery do not consume tool-call approvals.
    client.peer().list_tools(None).await.unwrap();
    assert!(policy.seen.lock().await.is_empty());
    for (tool, expected) in [
        ("allow", 1),
        ("deny", 1),
        ("unavailable", 1),
        ("slow", 1),
        ("allow", 2),
    ] {
        let params = CallToolRequestParams::new(tool).with_arguments(
            serde_json::from_value(serde_json::json!({"secret":"not-copied"})).unwrap(),
        );
        let result = tokio::time::timeout(Duration::from_secs(2), client.peer().call_tool(params))
            .await
            .unwrap();
        if tool == "allow" {
            assert!(result.is_ok(), "{result:?}");
        } else {
            assert!(result.is_err(), "{tool} reached the tool server");
        }
        assert_eq!(executions.executed.load(Ordering::SeqCst), expected);
        assert_eq!(executions.received.load(Ordering::SeqCst), expected);
    }
    tokio::time::sleep(Duration::from_millis(180)).await;
    assert_eq!(policy.late_approvals.load(Ordering::SeqCst), 0);
    assert_eq!(executions.executed.load(Ordering::SeqCst), 2);
    assert_eq!(executions.received.load(Ordering::SeqCst), 2);
    let seen = policy.seen.lock().await;
    assert_eq!(seen.len(), 5);
    assert!(seen.iter().all(|entry| !entry.3));
    assert_eq!(
        seen.iter()
            .map(|entry| entry.0)
            .collect::<HashSet<_>>()
            .len(),
        5
    );
    drop(seen);
    client.cancellation_token().cancel();
    server.abort();
    let _ = server.await;
}

fn wire_call(id: u32, tool: &str) -> rmcp::model::ClientJsonRpcMessage {
    serde_json::from_value(serde_json::json!({
        "jsonrpc":"2.0", "id":id, "method":"tools/call",
        "params":{"name":tool, "arguments":{"application_id":"demo"}, "_meta":{"traceparent":"00-0123456789abcdef0123456789abcdef-0123456789abcdef-01"}}
    })).unwrap()
}

#[tokio::test]
async fn repeated_request_ids_get_distinct_evaluations_and_content_is_opt_in() {
    let policy = Arc::new(Policy {
        include_arguments: true,
        ..Default::default()
    });
    let (mut transport, executions, server) = fixture(policy.clone(), Duration::from_secs(1)).await;
    for _ in 0..2 {
        transport.send(wire_call(7, "deny")).await.unwrap();
        let reply = tokio::time::timeout(Duration::from_secs(2), transport.receive())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(serde_json::to_value(reply).unwrap()["id"], 7);
    }
    let seen = policy.seen.lock().await;
    assert_eq!(seen.len(), 2);
    assert_ne!(seen[0].0, seen[1].0);
    assert_eq!(seen[0].1, seen[1].1);
    assert!(seen.iter().all(|entry| entry.3));
    drop(seen);
    assert_eq!(executions.executed.load(Ordering::SeqCst), 0);
    assert_eq!(executions.received.load(Ordering::SeqCst), 0);
    transport.close().await.unwrap();
    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn canceled_send_cannot_be_published_after_approval() {
    let policy = Arc::new(Policy::default());
    let (mut transport, executions, server) = fixture(policy.clone(), Duration::from_secs(1)).await;
    let send = tokio::spawn(transport.send(wire_call(1, "slow")));
    tokio::time::timeout(Duration::from_secs(3), policy.entered.notified())
        .await
        .unwrap();
    send.abort();
    let _ = send.await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(policy.late_approvals.load(Ordering::SeqCst), 1);
    assert_eq!(executions.executed.load(Ordering::SeqCst), 0);
    assert_eq!(executions.received.load(Ordering::SeqCst), 0);
    transport.close().await.unwrap();
    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn concurrent_calls_keep_verdicts_and_replies_correlated() {
    let policy = Arc::new(Policy::default());
    let (transport, executions, server) = fixture(policy.clone(), Duration::from_secs(1)).await;
    let client = serve_client((), transport).await.unwrap();
    let mut sends = tokio::task::JoinSet::new();
    for i in 0..12 {
        let peer = client.peer().clone();
        sends.spawn(async move {
            let allowed = i % 2 == 0;
            let tool = if allowed {
                format!("allow-{i}")
            } else {
                "deny".into()
            };
            let result = peer
                .call_tool(CallToolRequestParams::new(tool.clone()))
                .await;
            assert_eq!(result.is_ok(), allowed);
            if let Ok(response) = result {
                let value = serde_json::to_value(response).unwrap();
                assert_eq!(value["content"][0]["text"], tool);
            }
        });
    }
    while let Some(result) = tokio::time::timeout(Duration::from_secs(5), sends.join_next())
        .await
        .unwrap()
    {
        result.unwrap();
    }
    assert_eq!(executions.executed.load(Ordering::SeqCst), 6);
    assert_eq!(executions.received.load(Ordering::SeqCst), 6);
    assert_eq!(policy.seen.lock().await.len(), 12);
    client.cancellation_token().cancel();
    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn zero_budget_rejects_even_an_immediate_approval() {
    let policy = Arc::new(Policy::default());
    let (transport, counts, server) = fixture(policy, Duration::ZERO).await;
    let client = serve_client((), transport).await.unwrap();
    assert!(
        client
            .peer()
            .call_tool(CallToolRequestParams::new("allow"))
            .await
            .is_err()
    );
    assert_eq!(counts.received.load(Ordering::SeqCst), 0);
    assert_eq!(counts.executed.load(Ordering::SeqCst), 0);
    client.cancellation_token().cancel();
    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn closing_transport_cancels_an_in_progress_check() {
    let policy = Arc::new(Policy::default());
    let (mut transport, counts, server) = fixture(policy.clone(), Duration::from_secs(1)).await;
    let send = tokio::spawn(transport.send(wire_call(1, "slow")));
    tokio::time::timeout(Duration::from_secs(3), policy.entered.notified())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(1), transport.close())
        .await
        .unwrap()
        .unwrap();
    assert!(send.await.unwrap().is_err());
    tokio::time::sleep(Duration::from_millis(180)).await;
    assert_eq!(policy.late_approvals.load(Ordering::SeqCst), 0);
    assert_eq!(counts.received.load(Ordering::SeqCst), 0);
    assert_eq!(counts.executed.load(Ordering::SeqCst), 0);
    server.abort();
    let _ = server.await;
}
