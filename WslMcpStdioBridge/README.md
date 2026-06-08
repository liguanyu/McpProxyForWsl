# WSL MCP Stdio Bridge

Python stdio MCP bridge for exposing Windows Rider MCP tools through McpProxy.

Agent side:

```text
agent -> stdio -> bridge.py
```

Upstream side:

```text
bridge.py -> http://172.21.112.1:23333/stream -> Windows McpProxy -> Rider MCP
```

## Setup In WSL

From the repository root, copy the bridge directory to your WSL home directory:

```bash
mkdir -p ~/WslMcpStdioBridge
cp -r WslMcpStdioBridge/. ~/WslMcpStdioBridge/
```

Create the Python environment in the copied directory:

```bash
cd ~/WslMcpStdioBridge
uv venv .venv --python python3
. .venv/bin/activate
uv pip install -r requirements.txt
```

`bridge.py` has no startup arguments. It reads `config.toml` from this directory
and creates it on first run.

Default config:

```toml
transport = "streamable-http"
stream_url = "http://172.21.112.1:23333/stream"
sse_url = "http://172.21.112.1:23333/sse"
request_timeout_seconds = 30
sse_read_timeout_seconds = 300
no_proxy = "127.0.0.1,localhost,172.21.112.1"
```

Use `transport = "sse"` to test the legacy SSE path. The agent-facing stdio
interface does not change.

## Smoke Test

Make sure Windows McpProxy is already running, then:

```bash
cd ~/WslMcpStdioBridge
. .venv/bin/activate
python test_bridge_smoke.py
```

The test starts `bridge.py` through stdio, initializes it as an MCP client, and
calls `tools/list`. The bridge requests upstream tools every time.

## Agent Config

For Codex running in WSL, add this to `~/.codex/config.toml`:

```toml
[mcp_servers.rider-studio]
command = "bash"
args = ["-lc", "cd ~/WslMcpStdioBridge && . .venv/bin/activate && python bridge.py"]
startup_timeout_sec = 20
tool_timeout_sec = 60
```

When the agent runs in WSL:

```json
{
  "mcpServers": {
    "rider-studio": {
      "command": "~/WslMcpStdioBridge/.venv/bin/python",
      "args": ["~/WslMcpStdioBridge/bridge.py"]
    }
  }
}
```

When the agent runs on Windows and should launch the WSL bridge:

```json
{
  "mcpServers": {
    "rider-studio": {
      "command": "wsl.exe",
      "args": [
        "bash",
        "-lc",
        "cd ~/WslMcpStdioBridge && . .venv/bin/activate && python bridge.py"
      ]
    }
  }
}
```

## Notes

- Do not write normal logs to stdout. stdout is reserved for MCP JSON-RPC.
- Logs go to stderr.
- This bridge only declares and proxies tools for now. It does not declare
  resources or prompts.
