#!/usr/bin/env python3
# Copyright AGNTCY Contributors (https://github.com/agntcy)
# SPDX-License-Identifier: Apache-2.0

"""
MCP Client for testing kubernetes-mcp-server through the MCP proxy.

This client connects to the MCP proxy via SLIM protocol and issues
commands to the kubernetes-mcp-server running behind it.
"""

import asyncio

import slim_bindings
from mcp import ClientSession

from slim_mcp import create_local_app, create_client_streams


async def test_kubernetes_operations(mcp_session: ClientSession):
    """Test various Kubernetes operations through the MCP interface."""
    
    # First, list available tools
    try:
        tools = await mcp_session.list_tools()
        print(f"Available tools: {len(tools.tools)}")
        for tool in tools.tools:
            print(f"  - {tool.name}: {tool.description}")
    except Exception as e:
        print(f"Failed to list tools: {e}")
    
    # List all pods in the mcp-system namespace
    try:
        result = await mcp_session.call_tool("pods_list_in_namespace", {"namespace": "mcp-system"})
        
        if result and len(result.content) > 0:
            print("\nPods in mcp-system:")
            print("-" * 60)
            for content in result.content:
                if hasattr(content, 'text'):
                    print(content.text)
            print("-" * 60)
        else:
            print("No pods found in mcp-system namespace")
        
        return True
    except Exception as e:
        print(f"Failed to list pods: {e}")
        return False


async def main():
    """Main entry point for the Kubernetes MCP test client."""
    org = "org"
    ns = "mcp"
    mcp_server = "k8s-proxy"

    # Create SLIM client app with upstream connection
    client_name = slim_bindings.Name(org, ns, "k8s-client")
    config = slim_bindings.new_insecure_client_config("http://127.0.0.1:46357")
    
    client_app, connection_id = await create_local_app(client_name, config)

    # Set route to destination through upstream connection
    destination = slim_bindings.Name(org, ns, mcp_server)
    if connection_id is not None:
        await client_app.set_route_async(destination, connection_id)

    # Connect to server using MCP client streams
    async with create_client_streams(client_app, destination) as (read, write):
        async with ClientSession(read, write) as mcp_session:
            await mcp_session.initialize()
            
            # Run tests
            success = await test_kubernetes_operations(mcp_session)
            
            if success:
                print("\n✅ All tests completed successfully!")
            else:
                print("\n❌ Some tests failed")
                return 1
    
    return 0


if __name__ == "__main__":
    exit_code = asyncio.run(main())
    exit(exit_code)
