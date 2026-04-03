# Local test

This folder provides examples application to run an end-to-end test using the mpc-proxy

## How to run the test

### 1. Run the slim node
```bash
task mcp-proxy:test:run-slim
```

### 2. Run the MCP server
```bash
task mcp-proxy:test:run-mcp-server
```

### 3. Run the MCP proxy
```bash
task mcp-proxy:run-mcp-proxy
```

### 4. Run the client
```bash
task mcp-proxy:test:run-mcp-client
```
