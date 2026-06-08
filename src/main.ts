import { invoke } from "@tauri-apps/api/core";
import "./styles.css";

type AppConfig = {
  upstream_base_url: string;
  listen_host: string;
  listen_port: number;
  public_host: string;
  public_port: number;
  primary_transport: string;
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

const fields: Array<keyof AppConfig> = [
  "upstream_base_url",
  "listen_host",
  "listen_port",
  "public_host",
  "public_port",
  "primary_transport",
  "enable_sse",
  "enable_streamable_http",
  "auto_start_proxy",
  "debug_log_enabled",
  "log_dir",
];

async function init() {
  config = await invoke<AppConfig>("get_config");
  status = await invoke<ProxyStatus>("get_status");
  render();
  window.setInterval(refreshStatus, 1500);
}

async function refreshStatus() {
  try {
    status = await invoke<ProxyStatus>("get_status");
    render();
  } catch (error) {
    lastMessage = String(error);
    render();
  }
}

function render() {
  if (!config || !status) {
    appRoot.innerHTML = `<div class="loading">Loading</div>`;
    return;
  }

  const streamSnippet = JSON.stringify(
    {
      type: "streamable-http",
      url: status.public_stream_url,
      headers: {},
    },
    null,
    2,
  );
  const sseSnippet = JSON.stringify(
    {
      type: "sse",
      url: status.public_sse_url,
      headers: {},
    },
    null,
    2,
  );

  appRoot.innerHTML = `
    <section class="shell">
      <header class="topbar">
        <div>
          <h1>McpProxy</h1>
          <p>Rider MCP -> Windows -> WSL</p>
        </div>
        <div class="status ${status.running ? "running" : "stopped"}">
          <span></span>
          ${status.running ? "Running" : "Stopped"}
        </div>
      </header>

      <section class="toolbar">
        <button id="toggleProxy" class="primary" ${busy ? "disabled" : ""}>
          ${status.running ? "停止代理" : "启动代理"}
        </button>
        <button id="testStreamUpstream" ${busy ? "disabled" : ""}>测试上游 Stream</button>
        <button id="testSseUpstream" ${busy ? "disabled" : ""}>测试上游 SSE</button>
        <button id="testStreamProxy" ${busy || !status.running ? "disabled" : ""}>测试代理 Stream</button>
        <button id="testSseProxy" ${busy || !status.running ? "disabled" : ""}>测试代理 SSE</button>
      </section>

      ${lastMessage ? `<div class="message">${escapeHtml(lastMessage)}</div>` : ""}

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
          ${selectInput("primary_transport", "主传输", [
            ["streamable-http", "streamable-http"],
            ["sse", "sse"],
          ])}
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
            <dt>上游 Stream</dt><dd>${escapeHtml(status.upstream_stream_url)}</dd>
            <dt>上游 SSE</dt><dd>${escapeHtml(status.upstream_sse_url)}</dd>
            <dt>WSL Stream</dt><dd>${escapeHtml(status.public_stream_url)}</dd>
            <dt>WSL SSE</dt><dd>${escapeHtml(status.public_sse_url)}</dd>
          </dl>
          <h3>Streamable HTTP</h3>
          <pre>${escapeHtml(streamSnippet)}</pre>
          <h3>SSE</h3>
          <pre>${escapeHtml(sseSnippet)}</pre>
        </section>
      </section>

      <section class="panel logs">
        <h2>日志</h2>
        <pre>${escapeHtml(status.recent_logs.slice(-80).join("\n"))}</pre>
      </section>
    </section>
  `;

  bindEvents();
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
  render();
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
      render();
    } catch (error) {
      lastMessage = String(error);
      render();
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

function selectInput(name: keyof AppConfig, label: string, options: Array<[string, string]>) {
  const current = String(config?.[name] ?? "");
  return `
    <label>
      <span>${label}</span>
      <select name="${name}">
        ${options
          .map(
            ([value, text]) =>
              `<option value="${escapeAttr(value)}" ${current === value ? "selected" : ""}>${escapeHtml(text)}</option>`,
          )
          .join("")}
      </select>
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
