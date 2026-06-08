#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    fs::{self, OpenOptions},
    io::Write,
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{anyhow, Context, Result};
use axum::{
    body::{to_bytes, Body},
    extract::{OriginalUri, State as AxumState},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::any,
    Router,
};
use bytes::Bytes;
use chrono::Local;
use clap::Parser;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager, WindowEvent,
};
use tokio::{
    sync::{oneshot, Mutex, RwLock},
    task::JoinHandle,
};

#[derive(Parser, Debug, Clone)]
#[command(author, version, about)]
struct CliArgs {
    #[arg(long)]
    cli: bool,
    #[arg(long)]
    no_gui: bool,
    #[arg(long)]
    config: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppConfig {
    upstream_base_url: String,
    listen_host: String,
    listen_port: u16,
    public_host: String,
    public_port: u16,
    enable_sse: bool,
    enable_streamable_http: bool,
    auto_start_proxy: bool,
    debug_log_enabled: bool,
    log_dir: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            upstream_base_url: "http://127.0.0.1:64342".to_string(),
            listen_host: "0.0.0.0".to_string(),
            listen_port: 23333,
            public_host: "172.21.112.1".to_string(),
            public_port: 23333,
            enable_sse: true,
            enable_streamable_http: true,
            auto_start_proxy: true,
            debug_log_enabled: false,
            log_dir: "logs".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct ProxyStatus {
    running: bool,
    bind_url: String,
    public_sse_url: String,
    public_stream_url: String,
    upstream_sse_url: String,
    upstream_stream_url: String,
    recent_logs: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct TestResult {
    ok: bool,
    status: Option<u16>,
    message: String,
}

struct ProxyHandle {
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

struct AppRuntime {
    config_path: PathBuf,
    config: Arc<RwLock<AppConfig>>,
    proxy: Mutex<Option<ProxyHandle>>,
    logger: AppLogger,
}

#[derive(Clone)]
struct AppLogger {
    cwd: PathBuf,
    recent: Arc<Mutex<Vec<String>>>,
}

impl AppLogger {
    fn new(cwd: PathBuf) -> Self {
        Self {
            cwd,
            recent: Arc::new(Mutex::new(Vec::new())),
        }
    }

    async fn recent(&self) -> Vec<String> {
        self.recent.lock().await.clone()
    }

    async fn debug(&self, config: &AppConfig, message: impl AsRef<str>) {
        self.write(config, "DEBUG", message.as_ref()).await;
    }

    async fn info(&self, config: &AppConfig, message: impl AsRef<str>) {
        self.write(config, "INFO", message.as_ref()).await;
    }

    async fn error(&self, config: &AppConfig, message: impl AsRef<str>) {
        self.write(config, "ERROR", message.as_ref()).await;
    }

    async fn write(&self, config: &AppConfig, level: &str, message: &str) {
        let line = format!(
            "{} [{}] {}",
            Local::now().format("%Y-%m-%d %H:%M:%S"),
            level,
            message
        );

        {
            let mut recent = self.recent.lock().await;
            recent.push(line.clone());
            if recent.len() > 200 {
                let keep_from = recent.len() - 200;
                recent.drain(0..keep_from);
            }
        }

        let should_write = level == "ERROR" || config.debug_log_enabled;
        if !should_write {
            return;
        }

        let log_dir = resolve_maybe_relative(&self.cwd, &config.log_dir);
        if fs::create_dir_all(&log_dir).is_err() {
            return;
        }

        let log_file = log_dir.join(format!(
            "mcpproxy-{}.log",
            Local::now().format("%Y-%m-%d")
        ));
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(log_file) {
            let _ = writeln!(file, "{line}");
        }
    }
}

#[derive(Clone)]
struct ProxyServerState {
    client: reqwest::Client,
    config: Arc<RwLock<AppConfig>>,
    logger: AppLogger,
}

fn main() {
    let args = CliArgs::parse();
    if args.cli || args.no_gui {
        let runtime = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        if let Err(error) = runtime.block_on(run_cli(args)) {
            eprintln!("McpProxy failed: {error:#}");
            std::process::exit(1);
        }
        return;
    }

    run_gui(args);
}

async fn run_cli(args: CliArgs) -> Result<()> {
    let runtime = create_runtime(args.config.clone()).await?;
    start_proxy_runtime(runtime.clone()).await?;
    let config = runtime.config.read().await.clone();
    println!("McpProxy running");
    println!("SSE:    {}", public_url(&config, "/sse"));
    println!("Stream: {}", public_url(&config, "/stream"));
    println!("Press Ctrl+C to stop.");
    tokio::signal::ctrl_c().await?;
    stop_proxy_runtime(runtime).await?;
    Ok(())
}

fn run_gui(args: CliArgs) {
    let runtime_result =
        tauri::async_runtime::block_on(create_runtime(args.config.clone()));
    let runtime = match runtime_result {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("failed to initialize runtime: {error:#}");
            std::process::exit(1);
        }
    };

    tauri::Builder::default()
        .manage(runtime.clone())
        .setup(move |app| {
            setup_tray(app, runtime.clone())?;
            let runtime_for_start = runtime.clone();
            tauri::async_runtime::spawn(async move {
                let config = runtime_for_start.config.read().await.clone();
                if config.auto_start_proxy {
                    if let Err(error) = start_proxy_runtime(runtime_for_start.clone()).await {
                        runtime_for_start
                            .logger
                            .error(&config, format!("failed to auto-start proxy: {error:#}"))
                            .await;
                    }
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            get_status,
            start_proxy,
            stop_proxy,
            test_connection
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn setup_tray(app: &mut tauri::App, runtime: Arc<AppRuntime>) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示窗口", true, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle", "启动/停止代理", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &toggle, &quit])?;

    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "toggle" => {
                let runtime = runtime.clone();
                tauri::async_runtime::spawn(async move {
                    if is_proxy_running(&runtime).await {
                        let _ = stop_proxy_runtime(runtime).await;
                    } else {
                        let _ = start_proxy_runtime(runtime).await;
                    }
                });
            }
            "quit" => {
                let handle = app.app_handle().clone();
                let runtime = runtime.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = stop_proxy_runtime(runtime).await;
                    handle.exit(0);
                });
            }
            _ => {}
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

#[tauri::command]
async fn get_config(runtime: tauri::State<'_, Arc<AppRuntime>>) -> Result<AppConfig, String> {
    Ok(runtime.config.read().await.clone())
}

#[tauri::command]
async fn save_config(
    runtime: tauri::State<'_, Arc<AppRuntime>>,
    config: AppConfig,
) -> Result<(), String> {
    validate_config(&config).map_err(stringify_error)?;
    let was_running = is_proxy_running(&runtime).await;
    if was_running {
        stop_proxy_runtime(runtime.inner().clone())
            .await
            .map_err(stringify_error)?;
    }
    save_config_to_path(&runtime.config_path, &config).map_err(stringify_error)?;
    *runtime.config.write().await = config.clone();
    runtime.logger.info(&config, "configuration saved").await;
    if was_running {
        start_proxy_runtime(runtime.inner().clone())
            .await
            .map_err(stringify_error)?;
    }
    Ok(())
}

#[tauri::command]
async fn get_status(runtime: tauri::State<'_, Arc<AppRuntime>>) -> Result<ProxyStatus, String> {
    let config = runtime.config.read().await.clone();
    Ok(status_from_config(
        &config,
        is_proxy_running(&runtime).await,
        runtime.logger.recent().await,
    ))
}

#[tauri::command]
async fn start_proxy(runtime: tauri::State<'_, Arc<AppRuntime>>) -> Result<(), String> {
    start_proxy_runtime(runtime.inner().clone())
        .await
        .map_err(stringify_error)
}

#[tauri::command]
async fn stop_proxy(runtime: tauri::State<'_, Arc<AppRuntime>>) -> Result<(), String> {
    stop_proxy_runtime(runtime.inner().clone())
        .await
        .map_err(stringify_error)
}

#[tauri::command]
async fn test_connection(
    runtime: tauri::State<'_, Arc<AppRuntime>>,
    transport: String,
    target: String,
) -> Result<TestResult, String> {
    let config = runtime.config.read().await.clone();
    test_endpoint(&config, &transport, &target)
        .await
        .map_err(stringify_error)
}

async fn create_runtime(config_arg: Option<PathBuf>) -> Result<Arc<AppRuntime>> {
    let cwd = std::env::current_dir().context("failed to read current directory")?;
    let config_path = config_arg.unwrap_or_else(|| cwd.join("config.toml"));
    let config = load_or_create_config(&config_path)?;
    let logger = AppLogger::new(cwd);
    Ok(Arc::new(AppRuntime {
        config_path,
        config: Arc::new(RwLock::new(config)),
        proxy: Mutex::new(None),
        logger,
    }))
}

async fn start_proxy_runtime(runtime: Arc<AppRuntime>) -> Result<()> {
    let mut proxy = runtime.proxy.lock().await;
    if proxy.is_some() {
        return Ok(());
    }

    let config = runtime.config.read().await.clone();
    validate_config(&config)?;
    let bind_addr = bind_addr(&config)?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(300))
        .build()
        .context("failed to build HTTP client")?;

    let state = ProxyServerState {
        client,
        config: runtime.config.clone(),
        logger: runtime.logger.clone(),
    };
    let router = Router::new().route("/{*path}", any(proxy_handler)).with_state(state);
    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .with_context(|| format!("failed to bind {bind_addr}"))?;
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    runtime
        .logger
        .info(&config, format!("proxy listening on {bind_addr}"))
        .await;

    let logger = runtime.logger.clone();
    let config_for_task = config.clone();
    let task = tokio::spawn(async move {
        let result = axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await;
        if let Err(error) = result {
            logger
                .error(&config_for_task, format!("proxy server error: {error}"))
                .await;
        }
    });

    *proxy = Some(ProxyHandle {
        shutdown: Some(shutdown_tx),
        task,
    });
    Ok(())
}

async fn stop_proxy_runtime(runtime: Arc<AppRuntime>) -> Result<()> {
    let handle = runtime.proxy.lock().await.take();
    if let Some(mut handle) = handle {
        if let Some(shutdown) = handle.shutdown.take() {
            let _ = shutdown.send(());
        }
        let _ = tokio::time::timeout(Duration::from_secs(3), handle.task).await;
        let config = runtime.config.read().await.clone();
        runtime.logger.info(&config, "proxy stopped").await;
    }
    Ok(())
}

async fn is_proxy_running(runtime: &Arc<AppRuntime>) -> bool {
    runtime.proxy.lock().await.is_some()
}

async fn proxy_handler(
    AxumState(state): AxumState<ProxyServerState>,
    method: Method,
    uri: OriginalUri,
    headers: HeaderMap,
    body: Body,
) -> Response {
    match proxy_request(state.clone(), method, uri, headers, body).await {
        Ok(response) => response,
        Err(error) => {
            let config = state.config.read().await.clone();
            state
                .logger
                .error(&config, format!("proxy request failed: {error:#}"))
                .await;
            let body = format!("McpProxy error: {error:#}");
            (StatusCode::BAD_GATEWAY, body).into_response()
        }
    }
}

async fn proxy_request(
    state: ProxyServerState,
    method: Method,
    uri: OriginalUri,
    headers: HeaderMap,
    body: Body,
) -> Result<Response> {
    let config = state.config.read().await.clone();
    let path_and_query = uri
        .0
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    ensure_transport_enabled(&config, path_and_query)?;
    let upstream_url = upstream_url(&config, path_and_query);
    state
        .logger
        .debug(&config, format!("{} {}", method, upstream_url))
        .await;

    let body_bytes = to_bytes(body, usize::MAX)
        .await
        .context("failed to read request body")?;
    let reqwest_method = reqwest::Method::from_bytes(method.as_str().as_bytes())
        .context("failed to convert HTTP method")?;
    let mut request = state.client.request(reqwest_method, upstream_url);

    for (name, value) in headers.iter() {
        if is_hop_by_hop_header(name.as_str()) || name.as_str().eq_ignore_ascii_case("host") {
            continue;
        }
        request = request.header(name.as_str(), value.as_bytes());
    }

    let upstream_response = request
        .body(body_bytes)
        .send()
        .await
        .context("failed to call upstream Rider MCP server")?;
    let status = upstream_response.status();
    let is_sse = upstream_response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_ascii_lowercase().contains("text/event-stream"))
        .unwrap_or(false);

    let mut builder = Response::builder().status(status);
    for (name, value) in upstream_response.headers().iter() {
        if is_hop_by_hop_header(name.as_str()) || name.as_str().eq_ignore_ascii_case("content-length")
        {
            continue;
        }
        if let Ok(value) = value.to_str() {
            let rewritten = rewrite_header_value(&config, value);
            builder = builder.header(name.as_str(), rewritten);
        }
    }

    let upstream_base_url = config.upstream_base_url.trim_end_matches('/').to_string();
    let public_base_url = public_base_url(&config);
    let stream = upstream_response.bytes_stream().map(move |chunk| {
        chunk.map(|bytes| {
            if is_sse {
                rewrite_sse_chunk(&bytes, &upstream_base_url, &public_base_url)
            } else {
                bytes
            }
        })
    });

    builder
        .body(Body::from_stream(stream))
        .context("failed to build proxy response")
}

fn ensure_transport_enabled(config: &AppConfig, path_and_query: &str) -> Result<()> {
    let path = path_and_query.split('?').next().unwrap_or(path_and_query);
    if (path == "/sse" || path.starts_with("/message")) && !config.enable_sse {
        return Err(anyhow!("SSE transport is disabled"));
    }
    if path == "/stream" && !config.enable_streamable_http {
        return Err(anyhow!("Streamable HTTP transport is disabled"));
    }
    Ok(())
}

fn rewrite_sse_chunk(bytes: &Bytes, upstream_base_url: &str, public_base_url: &str) -> Bytes {
    let text = String::from_utf8_lossy(bytes);
    let rewritten = text
        .replace(upstream_base_url, public_base_url)
        .replace("data: /message", &format!("data: {public_base_url}/message"))
        .replace("data:/message", &format!("data:{public_base_url}/message"));
    Bytes::from(rewritten)
}

fn rewrite_header_value(config: &AppConfig, value: &str) -> String {
    value.replace(
        config.upstream_base_url.trim_end_matches('/'),
        &public_base_url(config),
    )
}

async fn test_endpoint(config: &AppConfig, transport: &str, target: &str) -> Result<TestResult> {
    let transport = normalize_transport(transport)?;
    let url = match (transport.as_str(), target) {
        ("sse", "upstream") => upstream_url(config, "/sse"),
        ("sse", _) => local_proxy_url(config, "/sse"),
        ("streamable-http", "upstream") => upstream_url(config, "/stream"),
        ("streamable-http", _) => local_proxy_url(config, "/stream"),
        _ => return Err(anyhow!("unsupported transport")),
    };

    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .context("failed to build test client")?;

    let response = if transport == "sse" {
        client.get(url.clone()).send().await
    } else {
        client
            .post(url.clone())
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .body(
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2025-06-18",
                        "capabilities": {},
                        "clientInfo": {
                            "name": "mcpproxy-test",
                            "version": "0.1.0"
                        }
                    }
                })
                .to_string(),
            )
            .send()
            .await
    };

    match response {
        Ok(response) => {
            let status = response.status();
            let ok = status.is_success();
            let message = if ok {
                format!("{transport} {target} connection OK: {url}")
            } else {
                format!("{transport} {target} returned HTTP {status}: {url}")
            };
            Ok(TestResult {
                ok,
                status: Some(status.as_u16()),
                message,
            })
        }
        Err(error) => Ok(TestResult {
            ok: false,
            status: None,
            message: format!("{transport} {target} failed: {error}"),
        }),
    }
}

fn load_or_create_config(path: &Path) -> Result<AppConfig> {
    if !path.exists() {
        let config = AppConfig::default();
        save_config_to_path(path, &config)?;
        return Ok(config);
    }

    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read config {}", path.display()))?;
    let config: AppConfig = toml::from_str(&text)
        .with_context(|| format!("failed to parse config {}", path.display()))?;
    validate_config(&config)?;
    Ok(config)
}

fn save_config_to_path(path: &Path, config: &AppConfig) -> Result<()> {
    validate_config(config)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory {}", parent.display()))?;
    }
    let text = toml::to_string_pretty(config).context("failed to serialize config")?;
    fs::write(path, text).with_context(|| format!("failed to write config {}", path.display()))
}

fn validate_config(config: &AppConfig) -> Result<()> {
    config
        .upstream_base_url
        .parse::<url::Url>()
        .context("upstream_base_url must be a valid URL")?;
    config
        .listen_host
        .parse::<IpAddr>()
        .context("listen_host must be an IP address")?;
    if config.listen_port == 0 || config.public_port == 0 {
        return Err(anyhow!("ports must be greater than 0"));
    }
    if !config.enable_sse && !config.enable_streamable_http {
        return Err(anyhow!("at least one transport must be enabled"));
    }
    Ok(())
}

fn normalize_transport(value: &str) -> Result<String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "sse" => Ok("sse".to_string()),
        "stream" | "streamable" | "streamable-http" | "streamable_http" => {
            Ok("streamable-http".to_string())
        }
        other => Err(anyhow!("unsupported transport: {other}")),
    }
}

fn bind_addr(config: &AppConfig) -> Result<SocketAddr> {
    let ip = config
        .listen_host
        .parse::<IpAddr>()
        .context("listen_host must be an IP address")?;
    Ok(SocketAddr::new(ip, config.listen_port))
}

fn status_from_config(config: &AppConfig, running: bool, recent_logs: Vec<String>) -> ProxyStatus {
    ProxyStatus {
        running,
        bind_url: format!("http://{}:{}", config.listen_host, config.listen_port),
        public_sse_url: public_url(config, "/sse"),
        public_stream_url: public_url(config, "/stream"),
        upstream_sse_url: upstream_url(config, "/sse"),
        upstream_stream_url: upstream_url(config, "/stream"),
        recent_logs,
    }
}

fn upstream_url(config: &AppConfig, path_and_query: &str) -> String {
    format!(
        "{}{}",
        config.upstream_base_url.trim_end_matches('/'),
        ensure_leading_slash(path_and_query)
    )
}

fn public_url(config: &AppConfig, path: &str) -> String {
    format!("{}{}", public_base_url(config), ensure_leading_slash(path))
}

fn public_base_url(config: &AppConfig) -> String {
    format!("http://{}:{}", config.public_host, config.public_port)
}

fn local_proxy_url(config: &AppConfig, path: &str) -> String {
    format!("http://127.0.0.1:{}{}", config.listen_port, ensure_leading_slash(path))
}

fn ensure_leading_slash(value: &str) -> String {
    if value.starts_with('/') {
        value.to_string()
    } else {
        format!("/{value}")
    }
}

fn resolve_maybe_relative(cwd: &Path, path: &str) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    }
}

fn is_hop_by_hop_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "proxy-connection"
    )
}

fn stringify_error(error: anyhow::Error) -> String {
    format!("{error:#}")
}
