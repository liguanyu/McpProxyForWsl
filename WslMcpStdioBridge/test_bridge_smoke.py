#!/usr/bin/env python3
from __future__ import annotations

import asyncio
import json
import os
import sys
from pathlib import Path

from mcp.client.session import ClientSession
from mcp.client.stdio import StdioServerParameters, stdio_client


BRIDGE = Path(__file__).with_name("bridge.py")


async def main() -> int:
    os.environ["NO_PROXY"] = "127.0.0.1,localhost,172.21.112.1"
    os.environ["no_proxy"] = "127.0.0.1,localhost,172.21.112.1"

    params = StdioServerParameters(
        command=sys.executable,
        args=[str(BRIDGE)],
        cwd=str(BRIDGE.parent),
    )

    async with stdio_client(params) as (read_stream, write_stream):
        async with ClientSession(read_stream, write_stream) as session:
            init = await session.initialize()
            print(f"bridge: {init.serverInfo.name} {init.serverInfo.version}")
            tools = await session.list_tools()
            second_tools = await session.list_tools()
            print(f"tool_count: {len(tools.tools)}")
            print(f"second_tool_count: {len(second_tools.tools)}")
            for tool in tools.tools[:10]:
                print(f"tool: {tool.name}")
            if not tools.tools:
                print("ERROR: tools/list returned no tools", file=sys.stderr)
                return 1
            if len(tools.tools) != len(second_tools.tools):
                print("ERROR: repeated tools/list returned different counts", file=sys.stderr)
                return 1

            print(
                json.dumps(
                    {
                        "first_tool": tools.tools[0].name,
                        "transport_smoke": "ok",
                    },
                    ensure_ascii=False,
                )
            )
    return 0


if __name__ == "__main__":
    raise SystemExit(asyncio.run(main()))
