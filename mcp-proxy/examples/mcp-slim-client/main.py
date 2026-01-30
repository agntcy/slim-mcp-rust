# Copyright AGNTCY Contributors (https://github.com/agntcy)
# SPDX-License-Identifier: Apache-2.0

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
    org = "org"
    ns = "mcp"
    mcp_server = "proxy"

    # Create SLIM client app with upstream connection
    client_name = slim_bindings.Name(org, ns, "client1")
    config = slim_bindings.new_insecure_client_config("http://127.0.0.1:46357")
    client_app, connection_id = await create_local_app(client_name, config)

    # Set route to destination through upstream connection
    destination = slim_bindings.Name(org, ns, mcp_server)
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
