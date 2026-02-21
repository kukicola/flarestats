import { invoke } from "@tauri-apps/api/core";
import { LogicalSize } from "@tauri-apps/api/dpi";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import Chart from "chart.js/auto";
import type { TooltipModel } from "chart.js";
import { formatNumber, formatTimestamp, escapeAttr } from "./utils";

interface Settings {
  token: string;
  account_id: string;
  period: string;
  exclude_bots: boolean;
}

interface SeriesPoint {
  timestamp: string;
  visits: number;
  page_views: number;
}

interface SiteData {
  name: string;
  visits: number;
  page_views: number;
  series: SeriesPoint[];
}

const app = document.getElementById("app")!;
let charts: Chart[] = [];
let cachedData: SiteData[] | null = null;
let currentView: "dashboard" | "settings" = "dashboard";
let isLoading = false;

function popover(inner: string): string {
  return `
    <div class="popover-container">
      <div class="arrow"></div>
      <div class="popover">${inner}</div>
    </div>
  `;
}

async function init() {
  const settings = await invoke<Settings>("get_settings");
  if (!settings.token || !settings.account_id) {
    showSettings();
  } else {
    showDashboard();
  }

  getCurrentWindow().onFocusChanged(({ payload: focused }) => {
    if (focused && currentView === "dashboard") {
      loadAnalytics();
    }
  });

  listen("open-settings", () => showSettings());
}

function showDashboard() {
  currentView = "dashboard";
  destroyCharts();

  app.innerHTML = popover(`
    <div class="header">
      <h1>FlareStats</h1>
      <div class="header-actions">
        <button class="icon-btn" id="refresh-btn" title="Refresh">
          <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5">
            <path d="M13.5 8a5.5 5.5 0 11-1.3-3.56"/>
            <path d="M13.5 2.5v3h-3"/>
          </svg>
        </button>
        <button class="icon-btn" id="settings-btn" title="Settings">
          <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5">
            <circle cx="8" cy="8" r="2"/>
            <path d="M13.3 10a1.1 1.1 0 00.2 1.2l.04.04a1.33 1.33 0 11-1.88 1.88l-.04-.04a1.1 1.1 0 00-1.2-.2 1.1 1.1 0 00-.67 1.01v.12a1.33 1.33 0 11-2.67 0v-.06A1.1 1.1 0 006 12.84a1.1 1.1 0 00-1.2.2l-.04.04a1.33 1.33 0 11-1.88-1.88l.04-.04a1.1 1.1 0 00.2-1.2 1.1 1.1 0 00-1.01-.67h-.12a1.33 1.33 0 110-2.67h.06A1.1 1.1 0 003.16 6a1.1 1.1 0 00-.2-1.2l-.04-.04a1.33 1.33 0 111.88-1.88l.04.04a1.1 1.1 0 001.2.2h.05a1.1 1.1 0 00.67-1.01v-.12a1.33 1.33 0 012.67 0v.06a1.1 1.1 0 00.67 1.01 1.1 1.1 0 001.2-.2l.04-.04a1.33 1.33 0 111.88 1.88l-.04.04a1.1 1.1 0 00-.2 1.2v.05a1.1 1.1 0 001.01.67h.12a1.33 1.33 0 010 2.67h-.06a1.1 1.1 0 00-1.01.67z"/>
          </svg>
        </button>
      </div>
    </div>
    <div class="content" id="dashboard-content">
      <div class="loading">
        <div class="spinner"></div>
        <div class="loading-text">Loading analytics...</div>
      </div>
    </div>
  `);

  document.getElementById("settings-btn")!.addEventListener("click", showSettings);
  document.getElementById("refresh-btn")!.addEventListener("click", () => {
    loadAnalytics();
  });

  loadAnalytics();
}

function setRefreshing(active: boolean) {
  const btn = document.getElementById("refresh-btn");
  if (btn) btn.classList.toggle("refreshing", active);
}

async function loadAnalytics() {
  if (isLoading) return;
  const content = document.getElementById("dashboard-content");
  if (!content) return;

  if (!cachedData) {
    content.innerHTML = `
      <div class="loading">
        <div class="spinner"></div>
        <div class="loading-text">Loading analytics...</div>
      </div>
    `;
  }

  isLoading = true;
  setRefreshing(true);
  try {
    const data = await invoke<SiteData[]>("fetch_analytics");
    cachedData = data;
    renderSites(data);
    await resizeWindow();
  } catch (e) {
    if (!cachedData) {
      content.innerHTML = `
        <div class="error">
          <div class="error-message">${escapeHtml(String(e))}</div>
          <button class="btn btn-secondary" id="error-settings-btn">Open Settings</button>
        </div>
      `;
      document.getElementById("error-settings-btn")?.addEventListener("click", showSettings);
    }
  } finally {
    isLoading = false;
    setRefreshing(false);
  }
}

function renderSites(sites: SiteData[]) {
  const content = document.getElementById("dashboard-content");
  if (!content) return;

  destroyCharts();

  if (sites.length === 0) {
    content.innerHTML = `<div class="empty">No sites found.</div>`;
    return;
  }

  content.innerHTML = `<div id="sites-inner">${sites.map((site, i) => `
    <div class="site-card">
      <div class="site-header">
        <span class="site-name">${escapeHtml(site.name)}</span>
        <div class="site-stats">
          <div class="stat">
            <span class="stat-value visits">${formatNumber(site.visits)}</span>
            <span class="stat-label">Visits</span>
          </div>
          <div class="stat">
            <span class="stat-value pageviews">${formatNumber(site.page_views)}</span>
            <span class="stat-label">Views</span>
          </div>
        </div>
      </div>
      <div class="site-chart">
        <canvas id="chart-${i}"></canvas>
      </div>
    </div>
  `).join("")}</div>`;

  sites.forEach((site, i) => {
    const canvas = document.getElementById(`chart-${i}`) as HTMLCanvasElement;
    if (canvas && site.series.length > 0) {
      createChart(canvas, site.series);
    }
  });
}

function externalTooltip(context: { chart: Chart; tooltip: TooltipModel<"bar"> }) {
  const { chart, tooltip } = context;
  const container = chart.canvas.parentNode as HTMLElement;

  let el = container.querySelector<HTMLElement>(".custom-tooltip");
  if (!el) {
    el = document.createElement("div");
    el.className = "custom-tooltip";
    container.style.position = "relative";
    container.appendChild(el);
  }

  if (tooltip.opacity === 0) {
    el.style.opacity = "0";
    return;
  }

  if (!tooltip.dataPoints?.length) {
    el.style.opacity = "0";
    return;
  }

  const idx = tooltip.dataPoints[0].dataIndex;
  const visits = chart.data.datasets[0].data[idx] as number;
  const extra = chart.data.datasets[1].data[idx] as number;
  const label = chart.data.labels?.[idx] ?? "";

  el.innerHTML = `
    <div class="tt-title">${label}</div>
    <div class="tt-row">
      <span class="tt-dot visits"></span>
      <span class="tt-label">Visits</span>
      <span class="tt-val">${visits}</span>
    </div>
    <div class="tt-row">
      <span class="tt-dot pageviews"></span>
      <span class="tt-label">Page Views</span>
      <span class="tt-val">${visits + extra}</span>
    </div>
  `;

  el.style.opacity = "1";

  const tipW = el.offsetWidth;
  let left = tooltip.caretX - tipW / 2;
  if (left < 0) left = 0;
  if (left + tipW > chart.canvas.offsetWidth) left = chart.canvas.offsetWidth - tipW;

  el.style.left = left + "px";
  el.style.top = "0px";
}

function createChart(canvas: HTMLCanvasElement, series: SeriesPoint[]) {
  const labels = series.map((p) => formatTimestamp(p.timestamp));
  const visitsData = series.map((p) => p.visits);
  const extraViewsData = series.map((p) => Math.max(0, p.page_views - p.visits));

  const isDark = window.matchMedia("(prefers-color-scheme: dark)").matches;

  const chart = new Chart(canvas, {
    type: "bar",
    data: {
      labels,
      datasets: [
        {
          label: "Visits",
          data: visitsData,
          backgroundColor: isDark ? "rgba(10, 132, 255, 0.7)" : "rgba(0, 122, 255, 0.55)",
          borderRadius: 1,
          borderSkipped: false,
        },
        {
          label: "Extra Views",
          data: extraViewsData,
          backgroundColor: isDark ? "rgba(191, 90, 242, 0.6)" : "rgba(175, 82, 222, 0.45)",
          borderRadius: { topLeft: 1, topRight: 1, bottomLeft: 0, bottomRight: 0 },
          borderSkipped: false,
        },
      ],
    },
    options: {
      responsive: true,
      maintainAspectRatio: false,
      interaction: {
        intersect: false,
        mode: "index",
      },
      scales: {
        x: {
          display: false,
          stacked: true,
        },
        y: {
          display: false,
          stacked: true,
          beginAtZero: true,
        },
      },
      plugins: {
        legend: { display: false },
        tooltip: {
          enabled: false,
          external: externalTooltip,
        },
      },
      animation: false,
    },
  });

  charts.push(chart);
}

function destroyCharts() {
  charts.forEach((c) => c.destroy());
  charts = [];
}

async function showSettings() {
  currentView = "settings";
  destroyCharts();

  let settings: Settings;
  try {
    settings = await invoke<Settings>("get_settings");
  } catch {
    settings = { token: "", account_id: "", period: "24h", exclude_bots: true };
  }

  app.innerHTML = popover(`
    <div class="settings-header">
      <button class="icon-btn" id="back-btn" title="Back">
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5">
          <path d="M10 3L5 8l5 5"/>
        </svg>
      </button>
      <h2>Settings</h2>
    </div>
    <div class="content">
      <div class="settings-form">
        <div class="form-group">
          <label>API Token</label>
          <input type="password" id="input-token" value="${escapeAttr(settings.token)}" placeholder="Cloudflare API Token" />
        </div>
        <div class="form-group">
          <label>Account ID</label>
          <input type="text" id="input-account-id" value="${escapeAttr(settings.account_id)}" placeholder="Cloudflare Account ID" />
        </div>
        <div class="form-group">
          <label>Time Period</label>
          <div class="period-selector">
            <button class="period-btn ${settings.period === "24h" ? "active" : ""}" data-period="24h">24 Hours</button>
            <button class="period-btn ${settings.period === "7d" ? "active" : ""}" data-period="7d">7 Days</button>
            <button class="period-btn ${settings.period === "30d" ? "active" : ""}" data-period="30d">30 Days</button>
          </div>
        </div>
        <div class="form-group">
          <label class="toggle-row">
            <span>Exclude Bots</span>
            <input type="checkbox" id="input-exclude-bots" ${settings.exclude_bots !== false ? "checked" : ""} />
          </label>
        </div>
        <button class="btn btn-primary" id="save-btn">Save</button>
      </div>
    </div>
  `);

  let selectedPeriod = settings.period || "24h";

  document.getElementById("back-btn")!.addEventListener("click", () => {
    if (settings.token && settings.account_id) {
      showDashboard();
    }
  });

  document.querySelectorAll<HTMLButtonElement>(".period-btn").forEach((btn) => {
    btn.addEventListener("click", () => {
      document.querySelectorAll(".period-btn").forEach((b) => b.classList.remove("active"));
      btn.classList.add("active");
      selectedPeriod = btn.dataset.period!;
    });
  });

  document.getElementById("save-btn")!.addEventListener("click", async () => {
    const token = (document.getElementById("input-token") as HTMLInputElement).value.trim();
    const accountId = (document.getElementById("input-account-id") as HTMLInputElement).value.trim();

    if (!token || !accountId) {
      alert("Please fill in both API Token and Account ID");
      return;
    }

    try {
      const excludeBots = (document.getElementById("input-exclude-bots") as HTMLInputElement).checked;
      await invoke("save_settings", {
        settings: {
          token,
          account_id: accountId,
          period: selectedPeriod,
          exclude_bots: excludeBots,
        },
      });
      cachedData = null;
      showDashboard();
    } catch (e) {
      alert("Failed to save settings: " + e);
    }
  });
}

function escapeHtml(s: string): string {
  const div = document.createElement("div");
  div.textContent = s;
  return div.innerHTML;
}

async function resizeWindow() {
  const inner = document.getElementById("sites-inner");
  const header = document.querySelector(".header") as HTMLElement;
  const content = document.getElementById("dashboard-content");
  if (!inner || !header || !content) return;
  const arrow = 10;
  const appPadding = 10;
  const headerHeight = header.offsetHeight + 1;
  const contentPadding = parseFloat(getComputedStyle(content).paddingTop) + parseFloat(getComputedStyle(content).paddingBottom);
  const needed = arrow + headerHeight + inner.offsetHeight + contentPadding + appPadding;
  const height = Math.min(600, Math.ceil(needed));
  await getCurrentWindow().setSize(new LogicalSize(420, height));
}

init();
