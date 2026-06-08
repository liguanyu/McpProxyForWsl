$ErrorActionPreference = "Stop"

Write-Host "[McpProxy] Build started." -ForegroundColor Cyan
Write-Host "[McpProxy] Installing npm dependencies..." -ForegroundColor Cyan
npm install

Write-Host "[McpProxy] Building frontend..." -ForegroundColor Cyan
npm run build

Write-Host "[McpProxy] Building Tauri app and bundles..." -ForegroundColor Cyan
npm run tauri build

Write-Host "[McpProxy] Build succeeded." -ForegroundColor Green
Write-Host "[McpProxy] Release exe: src-tauri\target\release\mcpproxy.exe" -ForegroundColor Green
Write-Host "[McpProxy] NSIS installer: src-tauri\target\release\bundle\nsis\McpProxy_0.1.0_x64-setup.exe" -ForegroundColor Green
Write-Host "[McpProxy] MSI installer: src-tauri\target\release\bundle\msi\McpProxy_0.1.0_x64_en-US.msi" -ForegroundColor Green

Write-Host ""
Read-Host "Press Enter to exit"
