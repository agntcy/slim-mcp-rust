# SLIM-MCP proxy
This proxy connects existing MCP servers that serve Streamable HTTP to the SLIM network. It receives messages from applications on SLIM, forwards them to the server's HTTP endpoint, and sends the replies back.

## How to run the code
You can use the commands provided in the Taskfile to run the client and server located in the example folder.

### Run the SLIM node
To run the SLIM node, use the following command:
```bash
task mcp-proxy:test:run-slim
```
### Run the SLIM-MCP proxy
To run the SLIM-MCP proxy, use the following command:
```bash
task mcp-proxy:run
```
### Run the MCP Server
To run the MCP server example, use the following command:
```bash
task mcp-proxy:test:run-mcp-server
```
The MCP server will start listening on `http://localhost:8000/mcp`.
### Run the MCP Client
To run the MCP client example, use the following command:
```bash
task mcp-proxy:test:run-mcp-client
```
The client uses the SLIM transport, so it will connect to the SLIM node and communicate with the MCP server through the proxy.
