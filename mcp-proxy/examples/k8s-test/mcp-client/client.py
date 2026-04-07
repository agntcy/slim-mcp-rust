#!/usr/bin/env python3
# Copyright AGNTCY Contributors (https://github.com/agntcy)
# SPDX-License-Identifier: Apache-2.0

"""
MCP Client for testing kubernetes-mcp-server through the MCP proxy.

This client connects to the MCP proxy via SLIM protocol and issues
commands to the kubernetes-mcp-server running behind it.
"""

import argparse
import asyncio
import logging

import slim_bindings
from mcp import ClientSession

from slim_mcp import create_local_app, create_client_streams

# Configure logging
logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)


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
    parser = argparse.ArgumentParser(description="Kubernetes MCP SLIM Client")
    parser.add_argument(
        "--local-name",
        type=str,
        default="org/mcp/k8s-client",
        help="Local client name in the form org/ns/name (default: org/mcp/k8s-client)",
    )
    parser.add_argument(
        "--proxy-name",
        type=str,
        default="org/mcp/k8s-proxy",
        help="MCP proxy name in the form org/ns/name (default: org/mcp/k8s-proxy)",
    )
    parser.add_argument(
        "--slim-endpoint",
        type=str,
        default="http://127.0.0.1:46357",
        help="SLIM endpoint URL (default: http://127.0.0.1:46357)",
    )
    parser.add_argument(
        "--shared-secret",
        type=str,
        default=None,
        help="Shared secret for authentication",
    )
    parser.add_argument(
        "--spire-socket-path",
        type=str,
        default=None,
        help="SPIRE Workload API socket path (e.g., unix:/tmp/spire-agent/public/api.sock)",
    )
    parser.add_argument(
        "--spire-target-spiffe-id",
        type=str,
        default=None,
        help="SPIRE target SPIFFE ID",
    )
    parser.add_argument(
        "--spire-jwt-audiences",
        type=str,
        default=None,
        help="SPIRE JWT audiences (comma-separated)",
    )
    args = parser.parse_args()

    # Parse local name
    local_name_parts = args.local_name.split('/')
    if len(local_name_parts) != 3:
        logger.error("Local name must be in the form org/ns/name")
        return 1
    
    # Parse proxy name
    proxy_name_parts = args.proxy_name.split('/')
    if len(proxy_name_parts) != 3:
        logger.error("Proxy name must be in the form org/ns/name")
        return 1

    # Create SLIM client app with upstream connection
    client_name = slim_bindings.Name(local_name_parts[0], local_name_parts[1], local_name_parts[2])
    config = slim_bindings.new_insecure_client_config(args.slim_endpoint)
    
    # Parse JWT audiences if provided
    jwt_audiences = None
    if args.spire_jwt_audiences:
        jwt_audiences = [a.strip() for a in args.spire_jwt_audiences.split(",") if a.strip()]
    
    # Create local app with authentication settings
    client_app, connection_id = await create_local_app(
        client_name,
        config,
        shared_secret=args.shared_secret,
        spire_socket_path=args.spire_socket_path,
        spire_target_spiffe_id=args.spire_target_spiffe_id,
        spire_jwt_audiences=jwt_audiences,
    )

    # Set route to destination through upstream connection
    destination = slim_bindings.Name(proxy_name_parts[0], proxy_name_parts[1], proxy_name_parts[2])
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
