# Copyright AGNTCY Contributors (https://github.com/agntcy)
# SPDX-License-Identifier: Apache-2.0

import argparse
import asyncio
import datetime
import logging
import time

import slim_bindings
from mcp import ClientSession, types
from mcp.types import AnyUrl

from slim_mcp import create_local_app, create_client_streams

# Configure logging
logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

subscription = False
unsubscription = False

async def logging_callback_fn(
    params: types.LoggingMessageNotificationParams,
) -> None:
    logger.info(f"Received Server Log Notification {params}")
    if params.data == "subscribe_resource":
        global subscription
        subscription = True
    if params.data == "unsubscribe_resource":
        global unsubscription
        unsubscription = True

async def main():
    parser = argparse.ArgumentParser(description="MCP SLIM Client")
    parser.add_argument(
        "--local-name",
        type=str,
        default="org/mcp/client1",
        help="Local client name in the form org/ns/name (default: org/mcp/client1)",
    )
    parser.add_argument(
        "--proxy-name",
        type=str,
        default="org/mcp/proxy",
        help="MCP proxy name in the form org/ns/name (default: org/mcp/proxy)",
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
        return
    
    # Parse proxy name
    proxy_name_parts = args.proxy_name.split('/')
    if len(proxy_name_parts) != 3:
        logger.error("Proxy name must be in the form org/ns/name")
        return

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
        async with ClientSession(read, write, logging_callback=logging_callback_fn) as mcp_session:
            logger.info("initialize session")
            await mcp_session.initialize()

            # Test tool listing
            tools = await mcp_session.list_tools()
            if tools is None:
                logger.error("Failed to list tools")
                return
            logger.info(f"Successfully retrieved tools: {tools}")

            # Test use fetch tool
            res = await mcp_session.call_tool("fetch", {"url": "https://example.com"})
            if res is None:
                logger.error("Failed to use the fetch tool")
                return
            logger.info(f"Successfully used tool: {res}")

            # List available resources
            resources = await mcp_session.list_resources()
            if resources is None:
                logger.error("Failed to use list resources")
                return
            logger.info(f"Successfully list resources: {resources}")

            # Subscribe for a resource
            await mcp_session.subscribe_resource(AnyUrl("file:///greeting.txt"))

            time.sleep(1)
            if not subscription:
                logger.error("Failed to subscribe for the resource")
                return
            logger.info("Successfully subscribed to resource")

            # read a specific resource
            resource = await mcp_session.read_resource(AnyUrl("file:///greeting.txt"))
            if resource is None:
                logger.error("Failed to read a resource")
                return
            logger.info(f"Successfully used resource: {resource}")

            # Unsubscribe for a resource
            await mcp_session.unsubscribe_resource(AnyUrl("file:///greeting.txt"))

            time.sleep(1)
            if not unsubscription:
                logger.error("Failed to unsubscribe for the resource")
                return
            logger.info("Successfully unsubscribed from resource")

            # List available prompts
            prompts = await mcp_session.list_prompts()
            if prompts is None:
                logger.error("Failed to list the prompts")
                return
            logger.info(f"Successfully list prompts: {prompts}")

            # Get the prompt with arguments
            prompt = await mcp_session.get_prompt(
                "simple",
                {
                    "context": "User is a software developer",
                    "topic": "Python async programming",
                },
            )
            if prompt is None:
                logger.error("Failed to get the prompt")
                return
            logger.info(f"Successfully got prompt: {prompt}")

if __name__ == "__main__":
    asyncio.run(main())
