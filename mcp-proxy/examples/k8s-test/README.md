# Test MCP proxy in K8S

This guide walks you through deploying the MCP proxy stack (SLIM server, MCP proxy, and kubernetes-mcp-server) into your kind cluster using Helm.


## 1. Create cluster

```bash
task mcp-proxy:k8s:create-cluster
```

## 2. Install all Helm charts

```bash
task mcp-proxy:k8s:install-all-charts
```

This installs the MCP proxy, a SLIM node and the [kubernetes-mcp-server](https://github.com/containers/kubernetes-mcp-server/tree/main)

## 3. Test

Start port forwarding
```bash
task mcp-proxy:k8s:port-forward
```

Run the client
```bash
task mcp-proxy:k8s:run-client
```