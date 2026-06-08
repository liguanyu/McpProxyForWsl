import { invoke } from "@tauri-apps/api/core";
import "./styles.css";

type AppConfig = {
  upstream_base_url: string;
  listen_host: string;
  listen_port: number;
  public_host: string;
  public_port: number;
  enable_sse: boolean;
  enable_streamable_http: boolean;
  auto_start_proxy: boolean;
  debug_log_enabled: boolean;
  log_dir: string;
};

type ProxyStatus = {
  running: boolean;
  bind_url: string;
  public_sse_url: string;
  public_stream_url: string;
  upstream_sse_url: string;
  upstream_stream_url: string;
  recent_logs: string[];
};

type TestResult = {
  ok: boolean;
  status: number | null;
  message: string;
};

const app = document.querySelector<HTMLDivElement>("#app");

if (!app) {
  throw new Error("Missing #app root");
}

const appRoot = app;
let config: AppConfig | null = null;
let status: ProxyStatus | null = null;
let saveTimer: number | undefined;
let busy = false;
let lastMessage = "";
let shellRendered = false;

const fields: Array<keyof AppConfig> = [
  "upstream_base_url",
  "listen_host",
  "listen_port",
  "public_host",
  "public_port",
  "enable_sse",
  "enable_streamable_http",
  "auto_start_proxy",
  "debug_log_enabled",
  "log_dir",
];

async function init() {
  appRoot.innerHTML = `<div class="loading">Loading</div>`;
  config = await invoke<AppConfig>("get_config");
  status = await invoke<ProxyStatus>("get_status");
  renderShellOnce();
  updateView();
  window.setInterval(refreshStatus, 1500);
}

async function refreshStatus() {
  try {
    status = await invoke<ProxyStatus>("get_status");
    updateView();
  } catch (error) {
    lastMessage = String(error);
    updateView();
  }
}

function renderShellOnce() {
  if (shellRendered) {
    return;
  }

  if (!config || !status) {
    appRoot.innerHTML = `<div class="loading">Loading</div>`;
    return;
  }

  appRoot.innerHTML = `
    <section class="shell">
      <header class="topbar">
        <div>
          <h1>McpProxy</h1>
          <p>Rider MCP -> Windows -> WSL</p>
        </div>
        <div class="status" id="statusBadge">
          <span></span>
        </div>
      </header>

      <section class="toolbar">
        <button id="toggleProxy" class="primary"></button>
        <button id="testStreamUpstream">测试上游 Stream</button>
        <button id="testSseUpstream">测试上游 SSE</button>
        <button id="testStreamProxy">测试代理 Stream</button>
        <button id="testSseProxy">测试代理 SSE</button>
      </section>

      <div class="message" id="message" hidden></div>

      <section class="grid">
        <form class="panel" id="configForm">
          <h2>配置</h2>
          ${textInput("upstream_base_url", "上游 Base URL")}
          <div class="row">
            ${textInput("listen_host", "监听地址")}
            ${numberInput("listen_port", "监听端口")}
          </div>
          <div class="row">
            ${textInput("public_host", "WSL 访问地址")}
            ${numberInput("public_port", "暴露端口")}
          </div>
          <div class="checks">
            ${checkboxInput("enable_streamable_http", "启用 Streamable HTTP")}
            ${checkboxInput("enable_sse", "启用 SSE")}
            ${checkboxInput("auto_start_proxy", "启动应用时自动启动代理")}
            ${checkboxInput("debug_log_enabled", "写入 debug 日志")}
          </div>
          ${textInput("log_dir", "日志目录")}
        </form>

        <section class="panel">
          <h2>访问地址</h2>
          <dl class="facts">
            <dt>上游 Stream</dt><dd id="upstreamStreamUrl"></dd>
            <dt>上游 SSE</dt><dd id="upstreamSseUrl"></dd>
            <dt>WSL Stream</dt><dd id="publicStreamUrl"></dd>
            <dt>WSL SSE</dt><dd id="publicSseUrl"></dd>
          </dl>
          <h3>Streamable HTTP</h3>
          <pre id="streamSnippet"></pre>
          <h3>SSE</h3>
          <pre id="sseSnippet"></pre>
        </section>
      </section>

      <section class="panel logs">
        <h2>日志</h2>
        <pre id="logsText"></pre>
      </section>
    </section>
  `;

  shellRendered = true;
  bindEvents();
}

function updateView() {
  if (!config || !status) {
    return;
  }

  renderShellOnce();

  const statusBadge = getElement<HTMLDivElement>("statusBadge");
  statusBadge.className = `status ${status.running ? "running" : "stopped"}`;
  statusBadge.innerHTML = `<span></span>${status.running ? "Running" : "Stopped"}`;

  const toggleProxy = getElement<HTMLButtonElement>("toggleProxy");
  toggleProxy.disabled = busy;
  toggleProxy.textContent = status.running ? "停止代理" : "启动代理";

  getElement<HTMLButtonElement>("testStreamUpstream").disabled = busy;
  getElement<HTMLButtonElement>("testSseUpstream").disabled = busy;
  getElement<HTMLButtonElement>("testStreamProxy").disabled = busy || !status.running;
  getElement<HTMLButtonElement>("testSseProxy").disabled = busy || !status.running;

  const message = getElement<HTMLDivElement>("message");
  message.hidden = !lastMessage;
  message.textContent = lastMessage;

  getElement<HTMLElement>("upstreamStreamUrl").textContent = status.upstream_stream_url;
  getElement<HTMLElement>("upstreamSseUrl").textContent = status.upstream_sse_url;
  getElement<HTMLElement>("publicStreamUrl").textContent = status.public_stream_url;
  getElement<HTMLElement>("publicSseUrl").textContent = status.public_sse_url;

  getElement<HTMLPreElement>("streamSnippet").textContent = JSON.stringify(
    {
      type: "streamable-http",
      url: status.public_stream_url,
      headers: {},
    },
    null,
    2,
  );
  getElement<HTMLPreElement>("sseSnippet").textContent = JSON.stringify(
    {
      type: "sse",
      url: status.public_sse_url,
      headers: {},
    },
    null,
    2,
  );
  updateLogs(status.recent_logs.slice(-80).join("\n"));
}

function updateLogs(logText: string) {
  const logsText = getElement<HTMLPreElement>("logsText");
  const previousScrollTop = logsText.scrollTop;
  const wasNearBottom =
    logsText.scrollTop + logsText.clientHeight >= logsText.scrollHeight - 16;

  logsText.textContent = logText;

  if (wasNearBottom) {
    logsText.scrollTop = logsText.scrollHeight;
  } else {
    logsText.scrollTop = previousScrollTop;
  }
}

function bindEvents() {
  document.querySelector("#toggleProxy")?.addEventListener("click", async () => {
    await runAction(async () => {
      if (status?.running) {
        await invoke("stop_proxy");
        lastMessage = "代理已停止";
      } else {
        await invoke("start_proxy");
        lastMessage = "代理已启动";
      }
      status = await invoke<ProxyStatus>("get_status");
    });
  });

  document.querySelector("#testStreamUpstream")?.addEventListener("click", () => {
    void testConnection("streamable-http", "upstream");
  });
  document.querySelector("#testSseUpstream")?.addEventListener("click", () => {
    void testConnection("sse", "upstream");
  });
  document.querySelector("#testStreamProxy")?.addEventListener("click", () => {
    void testConnection("streamable-http", "proxy");
  });
  document.querySelector("#testSseProxy")?.addEventListener("click", () => {
    void testConnection("sse", "proxy");
  });

  for (const field of fields) {
    const input = document.querySelector<HTMLInputElement | HTMLSelectElement>(`[name="${field}"]`);
    input?.addEventListener("input", () => {
      if (!config) return;
      if (input instanceof HTMLInputElement && input.type === "checkbox") {
        (config[field] as boolean) = input.checked;
      } else if (input instanceof HTMLInputElement && input.type === "number") {
        (config[field] as number) = Number(input.value);
      } else {
        (config[field] as string) = input.value;
      }
      scheduleSave();
    });
  }
}

async function testConnection(transport: string, target: string) {
  await runAction(async () => {
    const result = await invoke<TestResult>("test_connection", { transport, target });
    lastMessage = `${result.ok ? "OK" : "失败"}: ${result.message}`;
  });
}

async function runAction(action: () => Promise<void>) {
  busy = true;
  updateView();
  try {
    await action();
  } catch (error) {
    lastMessage = String(error);
  } finally {
    busy = false;
    await refreshStatus();
  }
}

function scheduleSave() {
  window.clearTimeout(saveTimer);
  saveTimer = window.setTimeout(async () => {
    if (!config) return;
    try {
      await invoke("save_config", { config });
      lastMessage = "配置已保存";
      status = await invoke<ProxyStatus>("get_status");
      updateView();
    } catch (error) {
      lastMessage = String(error);
      updateView();
    }
  }, 350);
}

function textInput(name: keyof AppConfig, label: string) {
  return `
    <label>
      <span>${label}</span>
      <input name="${name}" type="text" value="${escapeAttr(String(config?.[name] ?? ""))}" />
    </label>
  `;
}

function numberInput(name: keyof AppConfig, label: string) {
  return `
    <label>
      <span>${label}</span>
      <input name="${name}" type="number" min="1" max="65535" value="${escapeAttr(String(config?.[name] ?? ""))}" />
    </label>
  `;
}

function checkboxInput(name: keyof AppConfig, label: string) {
  return `
    <label class="check">
      <input name="${name}" type="checkbox" ${config?.[name] ? "checked" : ""} />
      <span>${label}</span>
    </label>
  `;
}

function getElement<T extends HTMLElement>(id: string) {
  const element = document.getElementById(id);
  if (!element) {
    throw new Error(`Missing #${id}`);
  }
  return element as T;
}

function escapeHtml(value: string) {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

function escapeAttr(value: string) {
  return escapeHtml(value).replaceAll("'", "&#39;");
}

void init();
