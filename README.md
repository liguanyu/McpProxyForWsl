# McpProxy

Windows tray app for forwarding JetBrains Rider MCP endpoints to WSL.

Default upstream endpoints:

- `http://127.0.0.1:64342/sse`
- `http://127.0.0.1:64342/stream`

Default WSL endpoints:

- `http://172.21.112.1:23333/sse`
- `http://172.21.112.1:23333/stream`

## Run

GUI development:

```powershell
npm install
npm run tauri dev
```

CLI mode:

```powershell
.\src-tauri\target\debug\mcpproxy.exe --cli
.\src-tauri\target\debug\mcpproxy.exe --cli --config .\config.toml
.\src-tauri\target\debug\mcpproxy.exe --cli --transport streamable-http
.\src-tauri\target\debug\mcpproxy.exe --cli --transport sse
```

## Config

`config.toml` is created in the working directory on first start.

```toml
upstream_base_url = "http://127.0.0.1:64342"
listen_host = "0.0.0.0"
listen_port = 23333
public_host = "172.21.112.1"
public_port = 23333
primary_transport = "streamable-http"
enable_sse = true
enable_streamable_http = true
auto_start_proxy = true
debug_log_enabled = false
log_dir = "logs"
```

Error logs are always written. Debug and info logs are written only when
`debug_log_enabled = true`.

## Test

Windows:

```powershell
curl.exe --noproxy "*" -k -N --max-time 5 http://127.0.0.1:23333/sse
```

WSL:

```powershell
wsl.exe sh -lc "curl -k -N --max-time 5 http://172.21.112.1:23333/sse"
```

For Streamable HTTP, send an MCP `initialize` request to `/stream`.

## Build

```powershell
.\scripts\build.ps1
```

The build script sets:

```powershell
$Env:http_proxy = "http://127.0.0.1:1080"
$Env:https_proxy = "http://127.0.0.1:1080"
```

Current bundle outputs:

- `src-tauri\target\release\bundle\msi\McpProxy_0.1.0_x64_en-US.msi`
- `src-tauri\target\release\bundle\nsis\McpProxy_0.1.0_x64-setup.exe`

