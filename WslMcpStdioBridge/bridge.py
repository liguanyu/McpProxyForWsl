#!/usr/bin/env python3
from __future__ import annotations

import asyncio
import os
import sys
import tomllib
from contextlib import asynccontextmanager
from dataclasses import dataclass
from pathlib import Path
from typing import AsyncIterator

from mcp import types
from mcp.client.session import ClientSession
from mcp.client.sse import sse_client
from mcp.client.streamable_http import streamablehttp_client
from mcp.server import Server
from mcp.server.stdio import stdio_server


CONFIG_PATH = Path(__file__).with_name("config.toml")
DEFAULT_CONFIG = """transport = "streamable-http"
stream_url = "http://172.21.112.1:23333/stream"
sse_url = "http://172.21.112.1:23333/sse"
request_timeout_seconds = 30
sse_read_timeout_seconds = 300
no_proxy = "127.0.0.1,localhost,172.21.112.1"
"""


@dataclass(frozen=True)
class BridgeConfig:
    transport: str
    stream_url: str
    sse_url: str
    request_timeout_seconds: float
    sse_read_timeout_seconds: float
    no_proxy: str


def log(message: str) -> None:
    print(f"[WslMcpStdioBridge] {message}", file=sys.stderr, flush=True)


def ensure_config() -> None:
    if not CONFIG_PATH.exists():
        CONFIG_PATH.write_text(DEFAULT_CONFIG, encoding="utf-8")
        log(f"created default config: {CONFIG_PATH}")


def load_config() -> BridgeConfig:
    ensure_config()
    data = tomllib.loads(CONFIG_PATH.read_text(encoding="utf-8"))
    transport = str(data.get("transport", "streamable-http")).strip().lower()
    if transport in {"stream", "streamable", "streamable_http"}:
        transport = "streamable-http"
    if transport not in {"streamable-http", "sse"}:
        raise ValueError("config transport must be 'streamable-http' or 'sse'")

    return BridgeConfig(
        transport=transport,
        stream_url=str(data.get("stream_url", "http://172.21.112.1:23333/stream")),
        sse_url=str(data.get("sse_url", "http://172.21.112.1:23333/sse")),
        request_timeout_seconds=float(data.get("request_timeout_seconds", 30)),
        sse_read_timeout_seconds=float(data.get("sse_read_timeout_seconds", 300)),
        no_proxy=str(data.get("no_proxy", "127.0.0.1,localhost,172.21.112.1")),
    )


def apply_no_proxy(config: BridgeConfig) -> None:
    os.environ["NO_PROXY"] = config.no_proxy
    os.environ["no_proxy"] = config.no_proxy


@asynccontextmanager
async def upstream_session(config: BridgeConfig) -> AsyncIterator[ClientSession]:
    apply_no_proxy(config)

    if config.transport == "streamable-http":
        log(f"connecting upstream streamable-http: {config.stream_url}")
        async with streamablehttp_client(
            config.stream_url,
            timeout=config.request_timeout_seconds,
            sse_read_timeout=config.sse_read_timeout_seconds,
            terminate_on_close=False,
        ) as (read_stream, write_stream, _get_session_id):
            async with ClientSession(read_stream, write_stream) as session:
                await session.initialize()
                yield session
        return

    log(f"connecting upstream sse: {config.sse_url}")
    async with sse_client(
        config.sse_url,
        timeout=config.request_timeout_seconds,
        sse_read_timeout=config.sse_read_timeout_seconds,
    ) as (read_stream, write_stream):
        async with ClientSession(read_stream, write_stream) as session:
            await session.initialize()
            yield session


server = Server(
    "wsl-mcp-stdio-bridge",
    version="0.1.0",
    instructions="Dynamic stdio bridge to Windows McpProxy/Rider MCP tools.",
)


@server.list_tools()
async def list_tools() -> list[types.Tool]:
    config = load_config()
    async with upstream_session(config) as session:
        result = await session.list_tools()
        log(f"tools/list returned {len(result.tools)} tools from upstream")
        return list(result.tools)


@server.call_tool(validate_input=False)
async def call_tool(name: str, arguments: dict | None) -> types.CallToolResult:
    config = load_config()
    async with upstream_session(config) as session:
        result = await session.call_tool(name, arguments or {})
        log(f"tools/call forwarded: {name}")
        return result


async def main() -> None:
    load_config()
    async with stdio_server() as (read_stream, write_stream):
        await server.run(
            read_stream,
            write_stream,
            server.create_initialization_options(),
        )


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except Exception as exc:
        log(f"fatal: {exc!r}")
        raise

