$ErrorActionPreference = "Stop"

$Env:http_proxy = "http://127.0.0.1:1080"
$Env:https_proxy = "http://127.0.0.1:1080"

npm install
npm run build
npm run tauri build

