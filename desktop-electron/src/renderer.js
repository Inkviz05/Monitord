const tabs = ['overview', 'sensors', 'agent', 'config'];
let current = null;
let currentCfg = null;
let lastAgentStatus = { running: false, telegramEnabled: false, pid: null };
let activeTab = 'overview';
let fetchFailures = 0;
let lastAutoStartAttemptAt = 0;
let firstHydrationDone = false;
let pollInFlight = false;
let pollTimer = null;
const sensorView = {
  mode: 'beginner',
  selectedType: '',
};
const speedHistory = [];
const SPEED_HISTORY_LIMIT = 180;
const perfHistory = [];
const PERF_HISTORY_LIMIT = 240;
const DEFAULT_BASE_URL = 'http://127.0.0.1:9108';
const DEFAULT_CONFIG_PATH = './config.yaml';
const THEME_STORAGE_KEY = 'monitord_theme';

function setActiveTab(tab) {
  activeTab = tabs.includes(tab) ? tab : 'overview';
  for (const b of document.querySelectorAll('.nav-item')) {
    b.classList.toggle('active', b.dataset.tab === activeTab);
  }
  for (const t of tabs) {
    document.getElementById(`tab-${t}`).classList.toggle('active', t === activeTab);
  }

  const pageTitle = document.getElementById('pageTitle');
  const titles = {
    overview: 'Панель мониторинга',
    sensors: 'Сенсоры',
    agent: 'Агент',
    config: 'Настройки',
  };
  if (pageTitle) {
    pageTitle.textContent = titles[activeTab] || 'monitord';
  }
  document.body.classList.toggle('agent-tab', activeTab === 'agent');
  if (current) renderActiveTab();
}

for (const btn of document.querySelectorAll('.nav-item')) {
  btn.addEventListener('click', () => setActiveTab(btn.dataset.tab));
}

const statusLine = document.getElementById('statusLine');

function renderActiveTab() {
  if (!current) return;
  if (activeTab === 'overview') {
    renderOverview(current);
    return;
  }
  if (activeTab === 'sensors') {
    renderSensors(current);
  }
}

function gb(v) {
  return (v / 1024 / 1024 / 1024).toFixed(1);
}

function mb(v) {
  return (v / 1024 / 1024).toFixed(1);
}

function kbps(v) {
  return `${Math.max(0, Math.round((v || 0) / 1024))} KB/s`;
}

function mbps(v) {
  return `${Math.max(0, ((v || 0) * 8 / 1_000_000)).toFixed(1)} Mbps`;
}

function warnClass(v, t1, t2) {
  if (v >= t2) return 'danger';
  if (v >= t1) return 'warn';
  return 'ok';
}

function esc(v) {
  return String(v ?? '').replace(/[&<>"]/g, (m) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[m]));
}

function uiIcon(name) {
  const icons = {
    cpu: '<span class="ui-icon"><svg viewBox="0 0 24 24"><path d="M9 2h6v3h3v3h3v8h-3v3h-3v3H9v-3H6v-3H3V8h3V5h3zm-1 5v10h8V7z"/></svg></span>',
    temp: '<span class="ui-icon"><svg viewBox="0 0 24 24"><path d="M11 2a5 5 0 0 1 5 5v6.6a4.5 4.5 0 1 1-6 0V7a1 1 0 1 1 2 0v7.5l-.5.3A2.5 2.5 0 1 0 14 17V7a3 3 0 1 0-6 0v7a1 1 0 1 1-2 0V7a5 5 0 0 1 5-5z"/></svg></span>',
    ram: '<span class="ui-icon"><svg viewBox="0 0 24 24"><path d="M3 7h18v10H3zm2 2v6h14V9zM7 5h2v2H7zm4 0h2v2h-2zm4 0h2v2h-2z"/></svg></span>',
    gpu: '<span class="ui-icon"><svg viewBox="0 0 24 24"><path d="M3 6h14v12H3zm2 2v8h10V8zM19 9h2v2h-2zm0 4h2v2h-2z"/></svg></span>',
    net: '<span class="ui-icon"><svg viewBox="0 0 24 24"><path d="M12 18a2 2 0 1 0 0 4 2 2 0 0 0 0-4zm-7-2a9.9 9.9 0 0 1 14 0l1.4-1.4a11.9 11.9 0 0 0-16.8 0zM2 13a14 14 0 0 1 20 0l-1.4 1.4a12 12 0 0 0-17.2 0z"/></svg></span>',
    disk: '<span class="ui-icon"><svg viewBox="0 0 24 24"><path d="M3 5h18v14H3zm2 2v10h14V7zM7 15h10v2H7z"/></svg></span>',
    sensor: '<span class="ui-icon"><svg viewBox="0 0 24 24"><path d="M10.4 2.8 9 5.6a7.9 7.9 0 0 0-1.7 1L4.3 5.8 2.8 8.3l2.6 2a7.9 7.9 0 0 0 0 3.4l-2.6 2 1.5 2.5 3-0.8a7.9 7.9 0 0 0 1.7 1l1.4 2.8h3.2l1.4-2.8a7.9 7.9 0 0 0 1.7-1l3 0.8 1.5-2.5-2.6-2a7.9 7.9 0 0 0 0-3.4l2.6-2-1.5-2.5-3 0.8a7.9 7.9 0 0 0-1.7-1L13.6 2.8zM12 9a3 3 0 1 1 0 6 3 3 0 0 1 0-6z"/></svg></span>',
    bot: '<span class="ui-icon"><svg viewBox="0 0 24 24"><path d="M12 2a1 1 0 0 1 1 1v1h2a5 5 0 0 1 5 5v5a5 5 0 0 1-5 5h-2.5L9 22v-3H9a5 5 0 0 1-5-5V9a5 5 0 0 1 5-5h2V3a1 1 0 0 1 1-1zm-3 8a1.5 1.5 0 1 0 0 3 1.5 1.5 0 0 0 0-3zm6 0a1.5 1.5 0 1 0 0 3 1.5 1.5 0 0 0 0-3z"/></svg></span>',
    chart: '<span class="ui-icon"><svg viewBox="0 0 24 24"><path d="M3 19h18v2H3zM5 16l4-4 3 2 5-6 2 1-6.2 7.4L9 14l-2.6 2.6z"/></svg></span>',
    alert: '<span class="ui-icon"><svg viewBox="0 0 24 24"><path d="M12 2 3 6v6c0 5.1 3.4 9.8 9 11 5.6-1.2 9-5.9 9-11V6zm0 5a2 2 0 1 1 0 4 2 2 0 0 1 0-4zm1.2 10h-2.4v-2h2.4z"/></svg></span>',
  };
  return icons[name] || icons.sensor;
}

function metricIconByLabel(label) {
  const text = String(label || '').toLowerCase();
  if (text.includes('cpu') && text.includes('темпера')) return uiIcon('temp');
  if (text.includes('cpu')) return uiIcon('cpu');
  if (text.includes('ram') || text.includes('память')) return uiIcon('ram');
  if (text.includes('gpu') || text.includes('vram')) return uiIcon('gpu');
  if (text.includes('сеть')) return uiIcon('net');
  if (text.includes('диск')) return uiIcon('disk');
  if (text.includes('сенсор')) return uiIcon('sensor');
  return uiIcon('chart');
}

function cssVar(name, fallback = '') {
  const value = getComputedStyle(document.body).getPropertyValue(name);
  return (value || fallback).trim() || fallback;
}

function applyTheme(theme) {
  const normalized = theme === 'neon' ? 'neon' : 'graphite';
  document.body.classList.toggle('theme-neon', normalized === 'neon');
  document.body.classList.toggle('theme-graphite', normalized !== 'neon');
  document.body.dataset.theme = normalized;
  try {
    localStorage.setItem(THEME_STORAGE_KEY, normalized);
  } catch (_) {
    // ignore localStorage errors
  }
  document.getElementById('themeGraphiteBtn')?.classList.toggle('active', normalized === 'graphite');
  document.getElementById('themeNeonBtn')?.classList.toggle('active', normalized === 'neon');
}

function initTheme() {
  let theme = 'graphite';
  try {
    theme = localStorage.getItem(THEME_STORAGE_KEY) || 'graphite';
  } catch (_) {
    theme = 'graphite';
  }
  applyTheme(theme);
}

function deriveCpuTemp(s) {
  function maxOrZero(arr) {
    return arr.length ? Math.max(...arr) : 0;
  }
  const primaryCpuTemps = [
    ...s.temps
      .filter((t) => {
        const text = (t.sensor || '').toLowerCase();
        return /cpu|package|core|tctl|tdie|amdcpu|intelcpu/.test(text)
          && !/gpu|nvidia|amdgpu|radeon|acpi|thermal zone|_tz/.test(text)
          && t.temperature_celsius >= 0
          && t.temperature_celsius <= 130;
      })
      .map((t) => t.temperature_celsius || 0),
    ...s.sensors
      .filter((x) => {
        const st = (x.sensor_type || '').toLowerCase();
        const text = `${x.name} ${x.parent} ${x.identifier}`.toLowerCase();
        return st === 'temperature'
          && /cpu|package|core|tctl|tdie|amdcpu|intelcpu/.test(text)
          && !/gpu|nvidia|amdgpu|radeon|acpi|thermal zone|_tz/.test(text)
          && x.value >= 0
          && x.value <= 130;
      })
      .map((x) => x.value),
  ];
  const acpiCpuTemps = [
    ...s.temps
      .filter((t) => {
        const text = (t.sensor || '').toLowerCase();
        return /acpi|thermal zone|_tz/.test(text)
          && t.temperature_celsius >= 0
          && t.temperature_celsius <= 130;
      })
      .map((t) => t.temperature_celsius || 0),
    ...s.sensors
      .filter((x) => {
        const st = (x.sensor_type || '').toLowerCase();
        const text = `${x.name} ${x.parent} ${x.identifier}`.toLowerCase();
        return st === 'temperature'
          && /acpi|thermal zone|_tz/.test(text)
          && x.value >= 0
          && x.value <= 130;
      })
      .map((x) => x.value),
  ];
  return primaryCpuTemps.length ? maxOrZero(primaryCpuTemps) : maxOrZero(acpiCpuTemps);
}

function netTotals(s) {
  return s.net.reduce(
    (acc, n) => {
      acc.rx += n.rx_bytes_per_sec || 0;
      acc.tx += n.tx_bytes_per_sec || 0;
      return acc;
    },
    { rx: 0, tx: 0 },
  );
}

function pushSpeedSample(s) {
  const speed = s.internet_speed || null;
  const rx = speed && Number.isFinite(speed.download_mbps) ? Math.max(0, speed.download_mbps * 1024 * 1024 / 8) : netTotals(s).rx;
  const tx = speed && Number.isFinite(speed.upload_mbps) ? Math.max(0, speed.upload_mbps * 1024 * 1024 / 8) : netTotals(s).tx;
  speedHistory.push({
    ts: Date.now(),
    rx,
    tx,
  });
  while (speedHistory.length > SPEED_HISTORY_LIMIT) {
    speedHistory.shift();
  }
}

function pushPerfSample(cpuLoad, ramLoad, gpuLoad) {
  perfHistory.push({
    ts: Date.now(),
    cpu: Math.max(0, Math.min(100, cpuLoad || 0)),
    ram: Math.max(0, Math.min(100, ramLoad || 0)),
    gpu: Math.max(0, Math.min(100, gpuLoad || 0)),
  });
  while (perfHistory.length > PERF_HISTORY_LIMIT) {
    perfHistory.shift();
  }
}

function calcSpeedStats(windowSecs = 60) {
  if (!speedHistory.length) {
    return {
      curRx: 0,
      curTx: 0,
      avgRx: 0,
      avgTx: 0,
      peakRx: 0,
      peakTx: 0,
      peakTotal: 0,
      points: [],
    };
  }

  const now = Date.now();
  const cutoff = now - windowSecs * 1000;
  const points = speedHistory.filter((x) => x.ts >= cutoff);
  const active = points.length ? points : [speedHistory[speedHistory.length - 1]];
  const cur = active[active.length - 1];

  let sumRx = 0;
  let sumTx = 0;
  let peakRx = 0;
  let peakTx = 0;
  let peakTotal = 0;

  for (const p of active) {
    sumRx += p.rx;
    sumTx += p.tx;
    if (p.rx > peakRx) peakRx = p.rx;
    if (p.tx > peakTx) peakTx = p.tx;
    const total = p.rx + p.tx;
    if (total > peakTotal) peakTotal = total;
  }

  return {
    curRx: cur.rx,
    curTx: cur.tx,
    avgRx: sumRx / active.length,
    avgTx: sumTx / active.length,
    peakRx,
    peakTx,
    peakTotal,
    points: active,
  };
}

function renderOverviewSpeedtest() {
  const stats = calcSpeedStats(60);
  const recent = speedHistory.slice(-36);
  const maxTotal = Math.max(1, ...recent.map((x) => x.rx + x.tx));
  const bars = recent
    .map((x) => {
      const h = Math.max(6, Math.round(((x.rx + x.tx) / maxTotal) * 42));
      return `<span style="height:${h}px"></span>`;
    })
    .join('');

  document.getElementById('overviewSpeedtest').innerHTML = `
    <h3>${uiIcon('net')}Speedtest интернета (realtime)</h3>
    <div class="speedtest-stats">
      <div class="speedtest-row"><span>Текущая</span><b>↓ ${mbps(stats.curRx)} / ↑ ${mbps(stats.curTx)}</b></div>
      <div class="speedtest-row"><span>Средняя (1 мин)</span><b>↓ ${mbps(stats.avgRx)} / ↑ ${mbps(stats.avgTx)}</b></div>
      <div class="speedtest-row"><span>Пик (1 мин)</span><b>↓ ${mbps(stats.peakRx)} / ↑ ${mbps(stats.peakTx)}</b></div>
      <div class="speedtest-row"><span>Пик суммарно</span><b>${mbps(stats.peakTotal)}</b></div>
    </div>
    <div class="speedtest-chart">${bars || '<span style="height:6px"></span>'}</div>
  `;
}

function renderTrendChart() {
  const canvas = document.getElementById('overviewTrend');
  if (!(canvas instanceof HTMLCanvasElement)) return;

  const points = perfHistory.slice(-120);
  const dpr = Math.max(1, window.devicePixelRatio || 1);
  const rect = canvas.getBoundingClientRect();
  const cssWidth = Math.max(320, Math.floor(rect.width || canvas.clientWidth || 640));
  const cssHeight = Math.max(150, Math.floor(rect.height || 180));
  const targetWidth = Math.floor(cssWidth * dpr);
  const targetHeight = Math.floor(cssHeight * dpr);

  if (canvas.width !== targetWidth || canvas.height !== targetHeight) {
    canvas.width = targetWidth;
    canvas.height = targetHeight;
  }

  const ctx = canvas.getContext('2d');
  if (!ctx) return;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, cssWidth, cssHeight);

  const pad = { top: 20, right: 14, bottom: 24, left: 32 };
  const plotWidth = Math.max(1, cssWidth - pad.left - pad.right);
  const plotHeight = Math.max(1, cssHeight - pad.top - pad.bottom);

  ctx.strokeStyle = 'rgba(130,156,206,0.24)';
  ctx.lineWidth = 1;
  [0, 25, 50, 75, 100].forEach((value) => {
    const y = pad.top + (1 - value / 100) * plotHeight;
    ctx.beginPath();
    ctx.moveTo(pad.left, y);
    ctx.lineTo(cssWidth - pad.right, y);
    ctx.stroke();
  });

  ctx.fillStyle = 'rgba(152,173,212,0.72)';
  ctx.font = '11px Segoe UI';
  [0, 50, 100].forEach((value) => {
    const y = pad.top + (1 - value / 100) * plotHeight;
    ctx.fillText(`${value}%`, 2, y + 3);
  });

  if (points.length < 2) {
    ctx.fillStyle = 'rgba(152,173,212,0.82)';
    ctx.font = '12px Segoe UI';
    ctx.fillText('Собираю историю для графика...', pad.left + 8, pad.top + 18);
    return;
  }

  const series = [
    { key: 'cpu', label: 'CPU', color: cssVar('--accent', '#4ea1ff') },
    { key: 'ram', label: 'RAM', color: cssVar('--warn', '#ffb64a') },
    { key: 'gpu', label: 'GPU', color: cssVar('--ok', '#3fd8a1') },
  ];

  const xByIndex = (i) => pad.left + (i / Math.max(1, points.length - 1)) * plotWidth;
  const yByValue = (value) => pad.top + (1 - Math.max(0, Math.min(100, value)) / 100) * plotHeight;

  for (const s of series) {
    ctx.beginPath();
    points.forEach((p, i) => {
      const x = xByIndex(i);
      const y = yByValue(p[s.key]);
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    });
    ctx.strokeStyle = s.color;
    ctx.lineWidth = 2.2;
    ctx.lineJoin = 'round';
    ctx.lineCap = 'round';
    ctx.stroke();
  }

  let legendX = pad.left;
  const legendY = 12;
  for (const s of series) {
    ctx.fillStyle = s.color;
    ctx.fillRect(legendX, legendY - 7, 10, 3);
    ctx.fillStyle = 'rgba(221,233,255,0.9)';
    ctx.font = '11px Segoe UI';
    ctx.fillText(s.label, legendX + 14, legendY);
    legendX += 52;
  }
}

function renderRiskCard(s, cpuTemp, gpuTemp, ramPct, diskWorst) {
  const cpuLoad = s.cpu_usage_percent || 0;
  const gpuLoad = Math.max(0, ...s.gpus.map((g) => g.utilization_percent || 0));
  const risk = Math.max(
    0,
    Math.min(
      100,
      Math.round(
        cpuLoad * 0.23
        + ramPct * 0.2
        + gpuLoad * 0.2
        + gpuTemp * 0.2
        + cpuTemp * 0.1
        + diskWorst * 0.07,
      ),
    ),
  );

  const tone = risk >= 80 ? 'Критично' : risk >= 60 ? 'Высокий' : risk >= 40 ? 'Средний' : 'Низкий';
  const toneClass = risk >= 80 ? 'danger' : risk >= 60 ? 'warn' : 'ok';
  const ring = document.getElementById('riskGauge');
  const value = document.getElementById('riskValue');
  const badge = document.getElementById('riskBadge');
  if (!ring || !value || !badge) return;

  const r = 78;
  const c = 2 * Math.PI * r;
  const pct = risk / 100;
  const offset = c * (1 - pct);
  const endAngle = -Math.PI / 2 + (Math.PI * 2 * pct);
  const markerX = 110 + Math.cos(endAngle) * r;
  const markerY = 110 + Math.sin(endAngle) * r;

  ring.innerHTML = `
    <svg viewBox="0 0 220 220" aria-hidden="true">
      <defs>
        <linearGradient id="riskGradient" x1="0%" y1="0%" x2="100%" y2="0%">
          <stop offset="0%" stop-color="#3fd8a1"></stop>
          <stop offset="55%" stop-color="#ffb64a"></stop>
          <stop offset="100%" stop-color="#ff5e79"></stop>
        </linearGradient>
      </defs>
      <g class="risk-ticks">
        <line x1="110" y1="18" x2="110" y2="30"></line>
        <line x1="193" y1="110" x2="181" y2="110"></line>
        <line x1="110" y1="202" x2="110" y2="190"></line>
        <line x1="27" y1="110" x2="39" y2="110"></line>
      </g>
      <circle cx="110" cy="110" r="${r}" fill="none" stroke="rgba(63,94,142,0.35)" stroke-width="16"></circle>
      <circle cx="110" cy="110" r="${r}" fill="none" stroke="url(#riskGradient)" stroke-width="16"
        stroke-linecap="round" stroke-dasharray="${c.toFixed(2)}" stroke-dashoffset="${offset.toFixed(2)}"
        transform="rotate(-90 110 110)"></circle>
      <circle class="risk-marker" cx="${markerX.toFixed(2)}" cy="${markerY.toFixed(2)}" r="8"></circle>
    </svg>
  `;
  value.textContent = `${risk}`;
  badge.textContent = tone;
  badge.className = `score-badge ${toneClass}`;
}

function renderMiniLists(s) {
  const devices = [...s.gpus]
    .map((g) => ({ name: g.name || 'GPU', value: (g.utilization_percent || 0) + (g.temperature_celsius || 0) }))
    .concat(
      s.disks.map((d) => ({
        name: `Disk ${d.mount}`,
        value: d.total_bytes ? (d.used_bytes / d.total_bytes) * 100 : 0,
      })),
    )
    .sort((a, b) => b.value - a.value)
    .slice(0, 6);

  const threatRows = devices.map((d) => `
    <div class="mini-item">
      <div class="name">${esc(d.name)}</div>
      <div class="value">${d.value.toFixed(1)}%</div>
    </div>
  `).join('');
  const threatHost = document.getElementById('miniThreatRows');
  if (threatHost) {
    threatHost.innerHTML = threatRows || '<div class="status">Нет данных</div>';
  }

  const netRows = [...s.net]
    .sort((a, b) => (b.rx_bytes_per_sec + b.tx_bytes_per_sec) - (a.rx_bytes_per_sec + a.tx_bytes_per_sec))
    .slice(0, 6)
    .map((n) => `
      <div class="mini-item">
        <div class="name">${esc(n.iface)}</div>
        <div class="value">↓${kbps(n.rx_bytes_per_sec)} / ↑${kbps(n.tx_bytes_per_sec)}</div>
      </div>
    `)
    .join('');
  const netHost = document.getElementById('miniNetworkRows');
  if (netHost) {
    netHost.innerHTML = netRows || '<div class="status">Нет данных</div>';
  }
}

function renderOverview(s) {
  pushSpeedSample(s);
  const ramPct = s.memory_total_bytes ? (s.memory_used_bytes / s.memory_total_bytes) * 100 : 0;
  const gpuLoad = Math.max(0, ...s.gpus.map((g) => g.utilization_percent || 0));
  const gpuTemp = Math.max(0, ...s.gpus.map((g) => g.temperature_celsius || 0));
  const cpuTemp = deriveCpuTemp(s);
  const totals = netTotals(s);
  const netRx = totals.rx;
  const netTx = totals.tx;
  const diskWorst = s.disks.reduce((m, d) => {
    const p = d.total_bytes ? (d.used_bytes / d.total_bytes) * 100 : 0;
    return p > m ? p : m;
  }, 0);
  const gpuMemUsed = s.gpus.reduce((sum, g) => sum + (g.memory_used_bytes || 0), 0);
  const gpuMemTotal = s.gpus.reduce((sum, g) => sum + (g.memory_total_bytes || 0), 0);
  const gpuMemPct = gpuMemTotal > 0 ? (gpuMemUsed / gpuMemTotal) * 100 : 0;
  pushPerfSample(s.cpu_usage_percent, ramPct, gpuLoad);

  document.getElementById('overviewCards').innerHTML = [
    { k: 'CPU нагрузка', v: `${s.cpu_usage_percent.toFixed(1)}%`, c: warnClass(s.cpu_usage_percent, 80, 92), p: Math.max(0, Math.min(100, s.cpu_usage_percent)) },
    { k: 'CPU температура', v: cpuTemp > 0 ? `${cpuTemp.toFixed(1)} C` : 'н/д', c: warnClass(cpuTemp, 75, 85), p: Math.max(0, Math.min(100, cpuTemp)) },
    { k: 'RAM', v: `${gb(s.memory_used_bytes)}/${gb(s.memory_total_bytes)} GB`, c: warnClass(ramPct, 80, 92), p: Math.max(0, Math.min(100, ramPct)), compact: true },
    { k: 'RAM нагрузка', v: `${ramPct.toFixed(1)}%`, c: warnClass(ramPct, 80, 92), p: Math.max(0, Math.min(100, ramPct)) },
    { k: 'GPU нагрузка', v: `${gpuLoad.toFixed(1)}%`, c: warnClass(gpuLoad, 80, 92), p: Math.max(0, Math.min(100, gpuLoad)) },
    { k: 'GPU температура', v: `${gpuTemp.toFixed(1)} C`, c: warnClass(gpuTemp, 70, 80), p: Math.max(0, Math.min(100, gpuTemp)) },
    { k: 'VRAM', v: `${gb(gpuMemUsed)}/${gb(gpuMemTotal)} GB`, c: warnClass(gpuMemPct, 80, 92), p: Math.max(0, Math.min(100, gpuMemPct)) },
    { k: 'Сеть', v: `↓${kbps(netRx)} / ↑${kbps(netTx)}`, c: (netRx + netTx) > 0 ? 'ok' : 'warn', p: Math.max(0, Math.min(100, (netRx + netTx) / 20000)) },
    { k: 'Диск max', v: `${diskWorst.toFixed(1)}%`, c: warnClass(diskWorst, 85, 95), p: Math.max(0, Math.min(100, diskWorst)) },
    { k: 'Сенсоров', v: `${s.sensors.length}`, c: 'ok', p: Math.max(8, Math.min(100, s.sensors.length / 5)) },
  ]
    .map((x) => `<div class="card ${x.c} ${x.compact ? 'compact' : ''}"><div class="k">${metricIconByLabel(x.k)}${x.k}</div><div class="v">${x.v}</div><div class="m"><span style="width:${x.p}%"></span></div></div>`)
    .join('');

  const diskCount = s.disks.length;
  const ifaceCount = s.net.length;
  const gpuList = s.gpus.map((g) => g.name).filter(Boolean).join(', ');
  const hottest = [...s.temps]
    .filter((t) => Number.isFinite(t.temperature_celsius))
    .sort((a, b) => b.temperature_celsius - a.temperature_celsius)
    .slice(0, 3)
    .map((t) => `${t.sensor}: ${t.temperature_celsius.toFixed(1)} C`)
    .join(' | ');

  document.getElementById('hardwareInfo').innerHTML = `
    <div><b>Компьютер:</b> ${esc(s.host_name || 'н/д')}</div>
    <div><b>ОС:</b> ${esc(s.os_name || 'н/д')} ${esc(s.os_version || '')}</div>
    <div><b>Ядро ОС:</b> ${esc(s.kernel_version || 'н/д')}</div>
    <div><b>Процессор:</b> ${esc(s.cpu_brand || 'н/д')}</div>
    <div><b>Ядер CPU:</b> ${s.cpu_core_count}</div>
    <div><b>Процессов:</b> ${s.process_count}</div>
    <div><b>Аптайм:</b> ${s.system_uptime_seconds}s</div>
    <div><b>RAM:</b> ${gb(s.memory_used_bytes)} / ${gb(s.memory_total_bytes)} GB</div>
    <div><b>GPU:</b> ${esc(gpuList || 'н/д')}</div>
    <div><b>Диски:</b> ${diskCount}</div>
    <div><b>Сетевые интерфейсы:</b> ${ifaceCount}</div>
    <div><b>Горячие датчики:</b> ${esc(hottest || 'н/д')}</div>
  `;

  const topTemps = [...s.temps]
    .sort((a, b) => b.temperature_celsius - a.temperature_celsius)
    .slice(0, 8)
    .map((t) => `<div>${esc(t.sensor)}: <b>${t.temperature_celsius.toFixed(1)} C</b></div>`)
    .join('');
  const gpus = s.gpus
    .map((g) => `${esc(g.name)} | load ${(g.utilization_percent ?? 0).toFixed(1)}% | temp ${(g.temperature_celsius ?? 0).toFixed(1)}C | mem ${gb(g.memory_used_bytes ?? 0)}/${gb(g.memory_total_bytes ?? 0)} GB`)
    .map((row) => `<div>${row}</div>`)
    .join('');
  document.getElementById('hotspots').innerHTML = `<div><b>Топ температур</b></div>${topTemps || '<div>нет данных</div>'}<div style="margin-top:8px;"><b>GPU</b></div>${gpus || '<div>нет данных</div>'}`;

  const netRows = [...s.net]
    .sort((a, b) => b.rx_bytes_per_sec + b.tx_bytes_per_sec - (a.rx_bytes_per_sec + a.tx_bytes_per_sec))
    .slice(0, 8)
    .map((n) => `<tr><td>${esc(n.iface)}</td><td>${n.rx_bytes_per_sec}</td><td>${n.tx_bytes_per_sec}</td></tr>`)
    .join('');

  const diskRows = [...s.disks]
    .sort((a, b) => b.used_bytes / Math.max(1, b.total_bytes) - a.used_bytes / Math.max(1, a.total_bytes))
    .slice(0, 8)
    .map((d) => `<tr><td>${esc(d.mount)}</td><td>${gb(d.used_bytes)} / ${gb(d.total_bytes)} GB</td><td>${((d.used_bytes / Math.max(1, d.total_bytes)) * 100).toFixed(1)}%</td></tr>`)
    .join('');

  document.getElementById('overviewPanel').innerHTML = `
    <h3>${uiIcon('net')}Сеть</h3>
    <table class="table"><tr><th>Интерфейс</th><th>RX B/s</th><th>TX B/s</th></tr>${netRows}</table>
    <h3 style="margin-top:12px;">${uiIcon('disk')}Диски</h3>
    <table class="table"><tr><th>Точка</th><th>Объём</th><th>Заполнено</th></tr>${diskRows}</table>
  `;
  renderOverviewSpeedtest();
  renderTrendChart();
  renderRiskCard(s, cpuTemp, gpuTemp, ramPct, diskWorst);
  renderMiniLists(s);
}

function humanizeSensorType(sensorType) {
  const t = String(sensorType || '').toLowerCase();
  if (t === 'temperature') return 'Температура';
  if (t === 'load') return 'Нагрузка';
  if (t === 'throughput') return 'Скорость';
  if (t === 'data') return 'Данные';
  if (t === 'smalldata') return 'Память';
  if (t === 'clock') return 'Частота';
  if (t === 'power') return 'Мощность';
  if (t === 'fan') return 'Вентилятор';
  if (t === 'voltage') return 'Напряжение';
  if (t === 'current') return 'Ток';
  return sensorType || 'Неизвестно';
}

function humanizeSensorParent(parent) {
  const p = String(parent || '').trim();
  if (!p) return 'Система';
  const segments = p.split('/').filter(Boolean);
  if (!segments.length) return p;

  const head = (segments[0] || '').toLowerCase();
  const tail = segments.slice(1).join('/');
  if (head === 'memory') return 'ОЗУ';
  if (head === 'disk') return `Диск ${tail || '?'}`;
  if (head === 'net') return `Сеть ${tail || ''}`.trim();
  if (head === 'gpu') return `GPU ${tail || ''}`.trim();
  if (head === 'cpu') return `CPU ${tail || ''}`.trim();
  if (head === 'temperature') return 'Температуры';
  return p;
}

function humanizeSensorName(name, sensorType, parent, identifier) {
  const n = String(name || '').trim();
  const l = n.toLowerCase();
  const id = String(identifier || '').toLowerCase();

  if (l.includes('cpu thermal zone') || l.includes('acpi') || id.includes('thermal zone')) {
    return 'CPU (ACPI fallback)';
  }
  if (l === 'memory total') return 'Память: всего';
  if (l === 'memory used') return 'Память: занято';
  if (l === 'memory free') return 'Память: свободно';
  if (l === 'memory load') return 'Память: загрузка';
  if (l === 'cpu total') return 'CPU: общая загрузка';
  if (l === 'cpu package') return 'CPU: пакет';

  const diskMatch = n.match(/^Disk\s+(.+)\s+(Total|Used|Free)$/i);
  if (diskMatch) {
    const mount = diskMatch[1];
    const metric = diskMatch[2].toLowerCase();
    if (metric === 'total') return `Диск ${mount}: всего`;
    if (metric === 'used') return `Диск ${mount}: занято`;
    if (metric === 'free') return `Диск ${mount}: свободно`;
  }

  const netMatch = n.match(/^(.+)\s+(RX|TX)(\s+Total)?$/i);
  if (netMatch) {
    const iface = netMatch[1];
    const dir = netMatch[2].toUpperCase() === 'RX' ? 'приём' : 'передача';
    const total = netMatch[3] ? ' (всего)' : '';
    return `${iface}: ${dir}${total}`;
  }

  if (l.endsWith(' temp')) {
    return n.slice(0, -5) + ': температура';
  }
  if (l.endsWith(' load')) {
    return n.slice(0, -5) + ': загрузка';
  }
  if (l.endsWith(' memory total')) {
    return n.slice(0, -13) + ': память всего';
  }
  if (l.endsWith(' memory used')) {
    return n.slice(0, -12) + ': память занято';
  }
  if (l.endsWith(' memory load')) {
    return n.slice(0, -12) + ': память загрузка';
  }

  return n;
}

function renderSensors(s) {
  const cpuTemp = deriveCpuTemp(s);
  const gpuTemp = Math.max(0, ...s.gpus.map((g) => g.temperature_celsius || 0));
  const gpuLoad = Math.max(0, ...s.gpus.map((g) => g.utilization_percent || 0));
  const ramLoad = s.memory_total_bytes > 0 ? (s.memory_used_bytes / s.memory_total_bytes) * 100 : 0;
  const netLoad = s.net.reduce((sum, n) => sum + (n.rx_bytes_per_sec || 0) + (n.tx_bytes_per_sec || 0), 0);
  const worstDisk = s.disks.reduce((m, d) => {
    const p = d.total_bytes ? (d.used_bytes / d.total_bytes) * 100 : 0;
    return p > m ? p : m;
  }, 0);

  document.getElementById('sensorQuick').innerHTML = [
    { k: 'CPU температура', v: cpuTemp > 0 ? `${cpuTemp.toFixed(1)} C` : 'н/д', c: warnClass(cpuTemp, 75, 85), p: Math.max(0, Math.min(100, cpuTemp)) },
    { k: 'GPU температура', v: gpuTemp > 0 ? `${gpuTemp.toFixed(1)} C` : 'н/д', c: warnClass(gpuTemp, 70, 80), p: Math.max(0, Math.min(100, gpuTemp)) },
    { k: 'GPU нагрузка', v: `${gpuLoad.toFixed(1)}%`, c: warnClass(gpuLoad, 80, 92), p: Math.max(0, Math.min(100, gpuLoad)) },
    { k: 'RAM нагрузка', v: `${ramLoad.toFixed(1)}%`, c: warnClass(ramLoad, 80, 92), p: Math.max(0, Math.min(100, ramLoad)) },
    { k: 'Disk максимум', v: `${worstDisk.toFixed(1)}%`, c: warnClass(worstDisk, 85, 95), p: Math.max(0, Math.min(100, worstDisk)) },
    { k: 'Сеть', v: `${Math.round(netLoad / 1024)} KB/s`, c: netLoad > 0 ? 'ok' : 'warn', p: Math.max(0, Math.min(100, netLoad / 20000)) },
  ]
    .map((x) => `<div class="card ${x.c}"><div class="k">${metricIconByLabel(x.k)}${x.k}</div><div class="v">${x.v}</div><div class="m"><span style="width:${x.p}%"></span></div></div>`)
    .join('');

  function sensorWeight(item) {
    const t = (item.sensor_type || '').toLowerCase();
    if (t === 'temperature') return Math.abs(item.value) * 1.8;
    if (t === 'load') return Math.abs(item.value) * 1.6;
    if (t === 'throughput') return Math.abs(item.value) / 1000;
    if (t === 'power') return Math.abs(item.value) * 1.4;
    return Math.abs(item.value);
  }

  function formatSensorValue(item) {
    const t = (item.sensor_type || '').toLowerCase();
    const v = item.value;
    if (!Number.isFinite(v)) return 'n/a';
    if (t === 'temperature') return `${v.toFixed(1)} C`;
    if (t === 'load') return `${v.toFixed(1)}%`;
    if (t === 'throughput') return `${Math.round(v)} B/s`;
    if (t === 'data' || t === 'smalldata') return `${v.toFixed(2)} MB`;
    if (t === 'clock') return `${v.toFixed(1)} MHz`;
    if (t === 'voltage') return `${v.toFixed(3)} V`;
    if (t === 'current') return `${v.toFixed(3)} A`;
    if (t === 'power') return `${v.toFixed(2)} W`;
    if (t === 'fan') return `${Math.round(v)} RPM`;
    return v.toFixed(3);
  }

  const typeFilter = document.getElementById('sensorTypeFilter');
  const filterText = (document.getElementById('sensorFilter').value || '').trim().toLowerCase();
  const selectedType = sensorView.selectedType || '';

  function renderTypePills(types, selected) {
    if (!typeFilter) return;
    const allBtn = `<button type="button" class="type-pill ${selected ? '' : 'active'}" data-type="">Все типы</button>`;
    const items = types
      .map((t) => `<button type="button" class="type-pill ${selected === t ? 'active' : ''}" data-type="${esc(t)}">${esc(humanizeSensorType(t))}</button>`)
      .join('');
    typeFilter.innerHTML = allBtn + items;
  }

  if (sensorView.mode === 'beginner') {
    sensorView.selectedType = '';
    renderTypePills([], '');

    const beginnerRows = [];
    beginnerRows.push({
      type: 'Нагрузка',
      device: 'CPU',
      metric: 'CPU: загрузка',
      value: `${s.cpu_usage_percent.toFixed(1)}%`,
      cls: warnClass(s.cpu_usage_percent, 80, 92),
    });
    beginnerRows.push({
      type: 'Температура',
      device: 'CPU',
      metric: 'CPU: температура',
      value: cpuTemp > 0 ? `${cpuTemp.toFixed(1)} C` : 'н/д',
      cls: warnClass(cpuTemp, 75, 85),
    });
    beginnerRows.push({
      type: 'Память',
      device: 'ОЗУ',
      metric: 'RAM: занято/всего',
      value: `${gb(s.memory_used_bytes)}/${gb(s.memory_total_bytes)} GB (${ramLoad.toFixed(1)}%)`,
      cls: warnClass(ramLoad, 80, 92),
    });

    for (const gpu of s.gpus) {
      const load = gpu.utilization_percent ?? 0;
      const temp = gpu.temperature_celsius ?? 0;
      const vUsed = gpu.memory_used_bytes ?? 0;
      const vTotal = gpu.memory_total_bytes ?? 0;
      const vPct = vTotal > 0 ? (vUsed / vTotal) * 100 : 0;
      beginnerRows.push({
        type: 'Нагрузка',
        device: gpu.name || 'GPU',
        metric: 'GPU: загрузка/температура',
        value: `${load.toFixed(1)}% / ${temp.toFixed(1)} C`,
        cls: warnClass(Math.max(load, temp), 80, 92),
      });
      beginnerRows.push({
        type: 'Память',
        device: gpu.name || 'GPU',
        metric: 'VRAM: занято/всего',
        value: `${gb(vUsed)}/${gb(vTotal)} GB (${vPct.toFixed(1)}%)`,
        cls: warnClass(vPct, 80, 92),
      });
    }

    for (const d of s.disks) {
      const p = d.total_bytes > 0 ? (d.used_bytes / d.total_bytes) * 100 : 0;
      beginnerRows.push({
        type: 'Диск',
        device: `Диск ${d.mount}`,
        metric: 'Занято/всего',
        value: `${gb(d.used_bytes)}/${gb(d.total_bytes)} GB (${p.toFixed(1)}%)`,
        cls: warnClass(p, 85, 95),
      });
    }

    for (const n of s.net) {
      beginnerRows.push({
        type: 'Скорость',
        device: `Сеть ${n.iface}`,
        metric: 'Сеть: вход/выход',
        value: `${kbps(n.rx_bytes_per_sec)} / ${kbps(n.tx_bytes_per_sec)}`,
        cls: 'ok',
      });
    }

    const rowsData = beginnerRows
      .filter((x) => {
        if (!filterText) return true;
        const text = `${x.type} ${x.device} ${x.metric} ${x.value}`.toLowerCase();
        return text.includes(filterText);
      });

    const rows = rowsData
      .map((x) => `<tr class="${x.cls}"><td>${esc(x.type)}</td><td>${esc(x.device)}</td><td>${esc(x.metric)}</td><td>${esc(x.value)}</td></tr>`)
      .join('');

    document.getElementById('sensorMeta').textContent = `Режим: Новичок | Показано ${rowsData.length} основных метрик`;
    document.getElementById('sensorTable').innerHTML = `<table class="table"><tr><th>Тип</th><th>Устройство</th><th>Показатель</th><th>Значение</th></tr>${rows || '<tr><td colspan="4">нет данных</td></tr>'}</table>`;
    return;
  }

  const uniqTypes = [...new Set(s.sensors.map((x) => x.sensor_type).filter(Boolean))].sort();
  if (sensorView.selectedType && !uniqTypes.includes(sensorView.selectedType)) {
    sensorView.selectedType = '';
  }
  renderTypePills(uniqTypes, sensorView.selectedType);

  const rawRows = [...s.sensors]
    .filter((x) => (selectedType ? x.sensor_type === selectedType : true))
    .filter((x) => {
      const text = `${x.name} ${x.parent} ${x.identifier}`.toLowerCase();
      return !text.includes('fallback') && !text.includes('acpi') && !text.includes('thermal zone');
    })
    .filter((x) => {
      if (!filterText) return true;
      const text = `${x.sensor_type} ${x.name} ${x.parent} ${x.identifier} ${humanizeSensorType(x.sensor_type)} ${humanizeSensorParent(x.parent)} ${humanizeSensorName(x.name, x.sensor_type, x.parent, x.identifier)}`.toLowerCase();
      return text.includes(filterText);
    })
    .sort((a, b) => sensorWeight(b) - sensorWeight(a))
    .slice(0, 180);

  const rows = rawRows
    .map((x) => {
      const t = (x.sensor_type || '').toLowerCase();
      const cls = t === 'temperature' ? warnClass(x.value, 75, 85) : t === 'load' ? warnClass(x.value, 80, 92) : 'ok';
      return `<tr class="${cls}"><td>${esc(humanizeSensorType(x.sensor_type))}</td><td>${esc(humanizeSensorParent(x.parent))}</td><td>${esc(humanizeSensorName(x.name, x.sensor_type, x.parent, x.identifier))}</td><td>${formatSensorValue(x)}</td></tr>`;
    })
    .join('');
  document.getElementById('sensorMeta').textContent = `Режим: Профи | Показано ${rawRows.length} сенсоров из ${s.sensors.length}`;
  document.getElementById('sensorTable').innerHTML = `<table class="table"><tr><th>Тип</th><th>Устройство</th><th>Сенсор</th><th>Значение</th></tr>${rows || '<tr><td colspan="4">нет данных</td></tr>'}</table>`;
}

async function poll() {
  if (pollInFlight) return;
  pollInFlight = true;
  const result = await window.monitordApi.fetchState(DEFAULT_BASE_URL);
  try {
    if (!result.ok) {
      fetchFailures += 1;
      const st = await window.monitordApi.agentStatus();
      lastAgentStatus = st;
      if (st.transitioning) {
        statusLine.textContent = 'Выполняется переключение сервиса...';
        return;
      }

      const now = Date.now();
      if (!st.running && now - lastAutoStartAttemptAt > 8000) {
        lastAutoStartAttemptAt = now;
        const startRes = await window.monitordApi.startAgent({ telegramEnabled: false });
        statusLine.textContent = startRes.ok
          ? 'Сервис перезапущен, ожидаю данные...'
          : `Сервис недоступен: ${startRes.message}`;
        return;
      }

      statusLine.textContent = fetchFailures <= 5
        ? 'Сервис запускается или перезапускается...'
        : 'Нет связи с сервисом, выполняется переподключение';
      return;
    }
    fetchFailures = 0;
    current = result.data;
    if (!firstHydrationDone) {
      document.body.classList.add('is-hydrated');
      setTimeout(() => document.body.classList.add('ui-steady'), 500);
      firstHydrationDone = true;
    }
    statusLine.textContent = `Данные обновлены: ${new Date().toLocaleTimeString()}`;
    renderActiveTab();
  } finally {
    pollInFlight = false;
  }
}

document.getElementById('refreshBtn').addEventListener('click', poll);
document.getElementById('themeGraphiteBtn').addEventListener('click', () => {
  applyTheme('graphite');
  renderTrendChart();
});
document.getElementById('themeNeonBtn').addEventListener('click', () => {
  applyTheme('neon');
  renderTrendChart();
});
window.addEventListener('resize', () => {
  if (current && activeTab === 'overview') renderTrendChart();
});
function schedulePoll() {
  if (pollTimer) clearTimeout(pollTimer);
  const interval = document.hidden ? 5000 : 2000;
  pollTimer = setTimeout(async () => {
    await poll();
    schedulePoll();
  }, interval);
}
document.addEventListener('visibilitychange', () => {
  schedulePoll();
});
initTheme();
setActiveTab('overview');
poll().finally(schedulePoll);

async function refreshAgentStatus() {
  const st = await window.monitordApi.agentStatus();
  lastAgentStatus = st;
  const restartBtn = document.getElementById('restartAgentBtn');
  const toggleBtn = document.getElementById('toggleBotBtn');
  if (restartBtn) restartBtn.disabled = !!st.transitioning;
  if (toggleBtn) toggleBtn.disabled = !!st.transitioning;
  if (st.transitioning) {
    document.getElementById('agentStatus').textContent = 'Сервис: выполняется переключение...';
    return;
  }
  const runningText = st.running ? `работает (PID ${st.pid})` : 'остановлен';
  const botText = st.telegramEnabled ? 'включен' : 'выключен';
  document.getElementById('agentStatus').textContent = `Сервис: ${runningText} | Telegram-бот: ${botText}`;
  document.getElementById('toggleBotBtn').innerHTML = st.telegramEnabled
    ? `${uiIcon('bot')}Выключить Telegram-бота`
    : `${uiIcon('bot')}Включить Telegram-бота`;
}

setInterval(refreshAgentStatus, 2500);
refreshAgentStatus();

document.getElementById('restartAgentBtn').addEventListener('click', async () => {
  if (lastAgentStatus.transitioning) return;
  await window.monitordApi.stopAgent();
  const res = await window.monitordApi.startAgent({ telegramEnabled: !!lastAgentStatus.telegramEnabled });
  document.getElementById('agentStatus').textContent = res.message;
  await refreshAgentStatus();
});

document.getElementById('toggleBotBtn').addEventListener('click', async () => {
  if (lastAgentStatus.transitioning) return;
  const res = await window.monitordApi.setTelegramEnabled(!lastAgentStatus.telegramEnabled);
  document.getElementById('agentStatus').textContent = res.message;
  await refreshAgentStatus();
});

document.getElementById('sensorFilter').addEventListener('input', () => {
  if (current && activeTab === 'sensors') renderSensors(current);
});
document.getElementById('sensorTypeFilter').addEventListener('click', (e) => {
  const btn = e.target.closest('.type-pill');
  if (!btn) return;
  sensorView.selectedType = btn.dataset.type || '';
  if (current && activeTab === 'sensors') renderSensors(current);
});
document.getElementById('sensorModeBeginner').addEventListener('click', () => {
  sensorView.mode = 'beginner';
  document.getElementById('sensorModeBeginner').classList.add('active');
  document.getElementById('sensorModePro').classList.remove('active');
  if (current && activeTab === 'sensors') renderSensors(current);
});
document.getElementById('sensorModePro').addEventListener('click', () => {
  sensorView.mode = 'pro';
  document.getElementById('sensorModePro').classList.add('active');
  document.getElementById('sensorModeBeginner').classList.remove('active');
  if (current && activeTab === 'sensors') renderSensors(current);
});

function toNum(v, dflt) {
  const x = Number(v);
  return Number.isFinite(x) ? x : dflt;
}

function fillConfigForm(cfg) {
  document.getElementById('cfgListen').value = cfg.listen || '0.0.0.0:9108';
  document.getElementById('cfgInterval').value = cfg.interval_secs || 5;
  document.getElementById('cfgTelegramEnabled').checked = !!cfg.telegram?.enabled;
  document.getElementById('cfgTokenEnv').value = cfg.telegram?.bot_token_env || 'TELEGRAM_BOT_TOKEN';
  document.getElementById('cfgToken').value = cfg.telegram?.bot_token || '';
  document.getElementById('cfgAllowedIds').value = (cfg.telegram?.allowed_chat_ids || []).join(',');
  document.getElementById('cfgPublicUrl').value = cfg.telegram?.public_base_url || '';
  document.getElementById('cfgCpuLoad').value = cfg.telegram?.alerts?.cpu_load_threshold_percent ?? 92;
  document.getElementById('cfgCpuTemp').value = cfg.telegram?.alerts?.cpu_temp_threshold_celsius ?? 85;
  document.getElementById('cfgGpuLoad').value = cfg.telegram?.alerts?.gpu_load_threshold_percent ?? 92;
  document.getElementById('cfgGpuTemp').value = cfg.telegram?.alerts?.gpu_temp_threshold_celsius ?? 75;
  document.getElementById('cfgRam').value = cfg.telegram?.alerts?.ram_usage_threshold_percent ?? 92;
  document.getElementById('cfgDisk').value = cfg.telegram?.alerts?.disk_usage_threshold_percent ?? 95;
}

function collectConfigFromForm(base) {
  const cfg = structuredClone(base || {});
  cfg.listen = document.getElementById('cfgListen').value.trim();
  cfg.interval_secs = Math.max(1, toNum(document.getElementById('cfgInterval').value, 5));
  cfg.telegram = cfg.telegram || {};
  cfg.telegram.enabled = document.getElementById('cfgTelegramEnabled').checked;
  cfg.telegram.bot_token_env = document.getElementById('cfgTokenEnv').value.trim() || 'TELEGRAM_BOT_TOKEN';
  const token = document.getElementById('cfgToken').value.trim();
  cfg.telegram.bot_token = token ? token : null;
  cfg.telegram.allowed_chat_ids = document.getElementById('cfgAllowedIds').value.split(/[\s,;]+/).map((x) => x.trim()).filter(Boolean).map((x) => Number(x)).filter((x) => Number.isInteger(x));
  cfg.telegram.public_base_url = document.getElementById('cfgPublicUrl').value.trim() || null;
  cfg.telegram.alerts = cfg.telegram.alerts || {};
  cfg.telegram.alerts.cpu_load_threshold_percent = toNum(document.getElementById('cfgCpuLoad').value, 92);
  cfg.telegram.alerts.cpu_temp_threshold_celsius = toNum(document.getElementById('cfgCpuTemp').value, 85);
  cfg.telegram.alerts.gpu_load_threshold_percent = toNum(document.getElementById('cfgGpuLoad').value, 92);
  cfg.telegram.alerts.gpu_temp_threshold_celsius = toNum(document.getElementById('cfgGpuTemp').value, 75);
  cfg.telegram.alerts.ram_usage_threshold_percent = toNum(document.getElementById('cfgRam').value, 92);
  cfg.telegram.alerts.disk_usage_threshold_percent = toNum(document.getElementById('cfgDisk').value, 95);
  return cfg;
}

async function loadStructuredConfig() {
  const res = await window.monitordApi.loadStructuredConfig(DEFAULT_CONFIG_PATH);
  if (!res.ok) {
    statusLine.textContent = `Ошибка загрузки настроек: ${res.message}`;
    return;
  }
  currentCfg = res.data || {};
  fillConfigForm(currentCfg);
  statusLine.textContent = 'Настройки загружены';
}

async function saveStructuredConfig() {
  const newCfg = collectConfigFromForm(currentCfg);
  const res = await window.monitordApi.saveStructuredConfig({ configPath: DEFAULT_CONFIG_PATH, data: newCfg });
  if (!res.ok) {
    statusLine.textContent = `Ошибка сохранения настроек: ${res.message}`;
    return;
  }
  currentCfg = newCfg;
  statusLine.textContent = 'Настройки сохранены';
}

document.getElementById('loadCfgBtn').addEventListener('click', loadStructuredConfig);
document.getElementById('saveCfgBtn').addEventListener('click', saveStructuredConfig);
loadStructuredConfig();

