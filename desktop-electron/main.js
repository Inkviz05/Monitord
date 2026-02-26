const { app, BrowserWindow, ipcMain, Menu, Tray, nativeImage } = require('electron');
const path = require('path');
const fs = require('fs/promises');
const fsSync = require('fs');
const YAML = require('yaml');
const { spawn } = require('child_process');

let agentChild = null;
const projectRoot = path.join(__dirname, '..');
let telegramEnabledRuntime = false;
let backendBaseUrlRuntime = 'http://127.0.0.1:9108';
let agentTransitioning = false;
let agentOpChain = Promise.resolve();
let mainWindow = null;
let tray = null;
let isQuitRequested = false;

function runtimeConfigPath() {
  return path.join(app.getPath('userData'), 'config.yaml');
}

function defaultConfigTemplatePath() {
  if (app.isPackaged) {
    return path.join(process.resourcesPath, 'config.yaml.example');
  }
  return path.join(projectRoot, 'config.yaml.example');
}

function iconsBasePath() {
  if (app.isPackaged) {
    return path.join(process.resourcesPath, 'icons');
  }
  return path.join(__dirname, 'assets', 'icons');
}

function pickTrayIconPath() {
  const base = iconsBasePath();
  const candidates = process.platform === 'win32'
    ? ['tray-32.png', 'tray-24.png', 'tray-20.png', 'tray-16.png', 'app-256.png']
    : ['tray-16.png', 'tray-24.png', 'tray-32.png', 'app-256.png'];
  for (const file of candidates) {
    const full = path.join(base, file);
    if (fsSync.existsSync(full)) {
      return full;
    }
  }
  return null;
}

function pickWindowIconPath() {
  const base = iconsBasePath();
  const candidates = process.platform === 'win32'
    ? ['app.ico', 'app-256.png', 'app-128.png']
    : ['app-256.png', 'app-128.png', 'app.ico'];
  for (const file of candidates) {
    const full = path.join(base, file);
    if (fsSync.existsSync(full)) {
      return full;
    }
  }
  return null;
}

function setupTray() {
  if (tray) {
    return;
  }
  const iconPath = pickTrayIconPath();
  if (!iconPath) {
    return;
  }

  let image = nativeImage.createFromPath(iconPath);
  if (!image || image.isEmpty()) {
    return;
  }
  if (process.platform === 'win32') {
    image = image.resize({ width: 16, height: 16, quality: 'best' });
  }

  tray = new Tray(image);
  tray.setToolTip('monitord desktop');
  tray.setContextMenu(
    Menu.buildFromTemplate([
      {
        label: 'Открыть',
        click: () => {
          if (!mainWindow) {
            createWindow();
            return;
          }
          if (mainWindow.isMinimized()) {
            mainWindow.restore();
          }
          mainWindow.show();
          mainWindow.focus();
        },
      },
      { type: 'separator' },
      {
        label: 'Выход',
        click: () => {
          isQuitRequested = true;
          app.quit();
        },
      },
    ]),
  );
  tray.on('double-click', () => {
    if (!mainWindow) {
      createWindow();
      return;
    }
    if (mainWindow.isMinimized()) {
      mainWindow.restore();
    }
    mainWindow.show();
    mainWindow.focus();
  });
  tray.on('click', () => {
    if (!mainWindow) {
      createWindow();
      return;
    }
    if (mainWindow.isMinimized()) {
      mainWindow.restore();
    }
    mainWindow.show();
    mainWindow.focus();
  });
}

function withAgentOp(fn) {
  const run = agentOpChain.then(
    async () => {
      agentTransitioning = true;
      try {
        return await fn();
      } finally {
        agentTransitioning = false;
      }
    },
    async () => {
      agentTransitioning = true;
      try {
        return await fn();
      } finally {
        agentTransitioning = false;
      }
    },
  );
  agentOpChain = run.catch(() => {});
  return run;
}

function defaultMonitordBin() {
  if (app.isPackaged) {
    return process.platform === 'win32'
      ? path.join(process.resourcesPath, 'bin', 'monitord.exe')
      : path.join(process.resourcesPath, 'bin', 'monitord');
  }

  const primaryBin = process.platform === 'win32'
    ? path.join(projectRoot, 'build_target', 'debug', 'monitord.exe')
    : path.join(projectRoot, 'build_target', 'debug', 'monitord');
  if (fsSync.existsSync(primaryBin)) {
    return primaryBin;
  }

  const verifyBin = process.platform === 'win32'
    ? path.join(projectRoot, 'build_target_verify', 'debug', 'monitord.exe')
    : path.join(projectRoot, 'build_target_verify', 'debug', 'monitord');
  if (fsSync.existsSync(verifyBin)) {
    return verifyBin;
  }

  return primaryBin;
}

function resolveProjectPath(p) {
  if (!p) return p;
  if (path.isAbsolute(p)) return p;
  if (app.isPackaged) return path.join(process.resourcesPath, p);
  return path.join(projectRoot, p);
}

function resolveConfigPath(p) {
  if (!p) return runtimeConfigPath();
  if (path.isAbsolute(p)) return p;
  const normalized = String(p).replace(/\\/g, '/').replace(/^\.\//, '');
  if (normalized === 'config.yaml') {
    return runtimeConfigPath();
  }
  if (app.isPackaged) {
    return path.join(process.resourcesPath, normalized);
  }
  return path.join(projectRoot, p);
}

async function ensureRuntimeConfigExists() {
  const targetPath = runtimeConfigPath();
  if (fsSync.existsSync(targetPath)) {
    return targetPath;
  }

  const templatePath = defaultConfigTemplatePath();
  try {
    const template = await fs.readFile(templatePath, 'utf8');
    const parsed = YAML.parse(template) || {};
    parsed.telegram = parsed.telegram || {};
    parsed.telegram.enabled = false;
    if (!Array.isArray(parsed.telegram.allowed_chat_ids)) {
      parsed.telegram.allowed_chat_ids = [];
    }
    await fs.mkdir(path.dirname(targetPath), { recursive: true });
    await fs.writeFile(targetPath, YAML.stringify(parsed), 'utf8');
    return targetPath;
  } catch (_) {
    const fallback = `listen: "127.0.0.1:9108"\ninterval_secs: 5\nhttp_checks: []\ntcp_checks: []\ntelegram:\n  enabled: false\n  bot_token_env: "TELEGRAM_BOT_TOKEN"\n  bot_token: null\n  allowed_chat_ids: []\n  public_base_url: null\n  rate_limit_per_minute: 30\n  alerts:\n    enabled_by_default: true\n    fail_threshold: 3\n    repeat_interval_secs: 300\n    recovery_notify: true\n    cpu_load_threshold_percent: 92\n    cpu_temp_threshold_celsius: 85\n    gpu_load_threshold_percent: 92\n    gpu_temp_threshold_celsius: 75\n    ram_usage_threshold_percent: 92\n    disk_usage_threshold_percent: 95\n    resource_alerts_enabled: true\n    resource_alert_cooldown_secs: 10\n`;
    await fs.mkdir(path.dirname(targetPath), { recursive: true });
    await fs.writeFile(targetPath, fallback, 'utf8');
    return targetPath;
  }
}

async function setTelegramEnabledInConfig(configPath, enabled) {
  try {
    const text = await fs.readFile(configPath, 'utf8');
    const parsed = YAML.parse(text) || {};
    parsed.telegram = parsed.telegram || {};
    const target = !!enabled;
    const current = !!parsed.telegram.enabled;
    if (current === target) {
      return false;
    }
    parsed.telegram.enabled = target;
    await fs.writeFile(configPath, YAML.stringify(parsed), 'utf8');
    return true;
  } catch (_) {
    // best effort: startup path will report real error to UI
    return false;
  }
}

function createWindow() {
  Menu.setApplicationMenu(null);
  const iconPath = pickWindowIconPath();
  mainWindow = new BrowserWindow({
    width: 1320,
    height: 840,
    minWidth: 980,
    minHeight: 640,
    autoHideMenuBar: true,
    backgroundColor: '#0b0f14',
    icon: iconPath || undefined,
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: false,
    },
  });

  mainWindow.loadFile(path.join(__dirname, 'src', 'index.html'));
  mainWindow.on('close', (event) => {
    if (isQuitRequested || !tray) {
      return;
    }
    // Keep backend alive in background when user closes window.
    event.preventDefault();
    mainWindow.hide();
    mainWindow.setSkipTaskbar(true);
  });
  mainWindow.on('show', () => {
    mainWindow.setSkipTaskbar(false);
  });
  mainWindow.on('closed', () => {
    mainWindow = null;
  });
}

function buildArgs(configPath, telegramEnabled) {
  return ['--config', configPath];
}

function resolveBackendBaseUrl(listen) {
  const raw = String(listen || '').trim();
  if (!raw) return 'http://127.0.0.1:9108';

  let host = '127.0.0.1';
  let port = '9108';

  if (raw.startsWith('[')) {
    const idx = raw.lastIndexOf(']:');
    if (idx > 0) {
      host = raw.slice(1, idx);
      port = raw.slice(idx + 2);
    }
  } else {
    const idx = raw.lastIndexOf(':');
    if (idx > 0) {
      host = raw.slice(0, idx);
      port = raw.slice(idx + 1);
    }
  }

  if (!host || host === '0.0.0.0' || host === '::') {
    host = '127.0.0.1';
  }
  return `http://${host}:${port}`;
}

function spawnAgentProcess({ binPath, configPath, telegramEnabled }) {
  const args = buildArgs(configPath, telegramEnabled);
  if (fsSync.existsSync(binPath)) {
    return spawn(binPath, args, {
      cwd: projectRoot,
      stdio: 'ignore',
      windowsHide: true,
    });
  }

  if (app.isPackaged) {
    throw new Error(`Не найден исполняемый файл агента: ${binPath}`);
  }

  return spawn('cargo', ['run', '--', ...args], {
    cwd: projectRoot,
    stdio: 'ignore',
    windowsHide: true,
  });
}

function waitForExit(child, timeoutMs = 6000) {
  return new Promise((resolve) => {
    if (!child || child.exitCode !== null || child.killed) {
      resolve();
      return;
    }

    let done = false;
    const finish = () => {
      if (done) return;
      done = true;
      resolve();
    };

    const timer = setTimeout(finish, timeoutMs);
    child.once('exit', () => {
      clearTimeout(timer);
      finish();
    });
  });
}

function attachAgentLifecycle(child) {
  child.on('exit', () => {
    if (agentChild === child) {
      agentChild = null;
      telegramEnabledRuntime = false;
    }
  });
}

async function stopAgentProcess() {
  if (!agentChild) return true;
  const child = agentChild;

  try {
    if (process.platform === 'win32' && child.pid) {
      const killer = spawn('taskkill', ['/PID', String(child.pid), '/T', '/F'], {
        windowsHide: true,
        stdio: 'ignore',
      });
      await waitForExit(killer, 5000);
    } else {
      child.kill('SIGTERM');
    }

    await waitForExit(child, 7000);
    if (child.exitCode === null && !child.killed) {
      child.kill('SIGKILL');
      await waitForExit(child, 2000);
    }
    return true;
  } catch (_) {
    return false;
  } finally {
    if (agentChild === child) {
      agentChild = null;
      telegramEnabledRuntime = false;
    }
  }
}

async function waitForBackendReady(baseUrl, child, timeoutMs = 30000) {
  const startedAt = Date.now();

  while (Date.now() - startedAt < timeoutMs) {
    if (!child || child.exitCode !== null) {
      return false;
    }

    try {
      const response = await fetch(`${baseUrl}/healthz`, { method: 'GET' });
      if (response.ok) {
        return true;
      }
    } catch (_) {
      // backend may still be starting
    }

    await new Promise((resolve) => setTimeout(resolve, 150));
  }

  return false;
}

async function readListenFromConfig(configPath) {
  try {
    const text = await fs.readFile(configPath, 'utf8');
    const parsed = YAML.parse(text) || {};
    return parsed.listen || '127.0.0.1:9108';
  } catch (_) {
    return '127.0.0.1:9108';
  }
}

async function startAgentManaged({ binPath, configPath, telegramEnabled, syncTelegramConfig = true }) {
  if (syncTelegramConfig) {
    await setTelegramEnabledInConfig(configPath, telegramEnabled);
  }
  const child = spawnAgentProcess({ binPath, configPath, telegramEnabled });
  agentChild = child;
  telegramEnabledRuntime = telegramEnabled;
  attachAgentLifecycle(child);

  const listen = await readListenFromConfig(configPath);
  const baseUrl = resolveBackendBaseUrl(listen);
  backendBaseUrlRuntime = baseUrl;

  const ready = await waitForBackendReady(baseUrl, child);
  if (!ready) {
    await stopAgentProcess();
    return { ok: false, message: 'Сервис не вышел в готовность (healthz)', baseUrl };
  }

  return { ok: true, baseUrl };
}

async function ensureBackendStarted() {
  await withAgentOp(async () => {
    if (agentChild) return;
    const configPath = resolveConfigPath('./config.yaml');
    const binPath = defaultMonitordBin();
    const started = await startAgentManaged({
      binPath,
      configPath,
      telegramEnabled: false,
      syncTelegramConfig: false,
    });
    if (!started.ok) {
      await setTelegramEnabledInConfig(configPath, false);
      await startAgentManaged({
        binPath,
        configPath,
        telegramEnabled: false,
        syncTelegramConfig: false,
      });
    }
  });
}

app.whenReady().then(async () => {
  await ensureRuntimeConfigExists();
  createWindow();
  setupTray();
  ensureBackendStarted().catch(() => {});

  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) {
      createWindow();
    }
  });
});

app.on('window-all-closed', () => {
  if (isQuitRequested && process.platform !== 'darwin') {
    app.quit();
  }
});

app.on('before-quit', () => {
  isQuitRequested = true;
  // shutdown best-effort
  stopAgentProcess();
});

ipcMain.handle('agent:start', async (_evt, payload) => {
  return withAgentOp(async () => {
    if (agentChild) {
      return { ok: false, message: 'Агент уже запущен' };
    }

    const rawBinPath = (payload && payload.binPath) || defaultMonitordBin();
    const binPath = resolveProjectPath(rawBinPath);
    const configPath = resolveConfigPath((payload && payload.configPath) || './config.yaml');
    const telegramEnabled = !!(payload && payload.telegramEnabled);

    try {
      const started = await startAgentManaged({ binPath, configPath, telegramEnabled });
      if (!started.ok) {
        return { ok: false, message: started.message };
      }
      return {
        ok: true,
        message: telegramEnabled
          ? 'Сервис запущен с Telegram-ботом'
          : 'Сервис запущен без Telegram-бота',
      };
    } catch (err) {
      agentChild = null;
      return { ok: false, message: `Не удалось запустить агент: ${String(err)}` };
    }
  });
});

ipcMain.handle('agent:stop', async () => {
  return withAgentOp(async () => {
    if (!agentChild) {
      return { ok: false, message: 'Агент не запущен' };
    }
    try {
      await stopAgentProcess();
      return { ok: true, message: 'Агент остановлен' };
    } catch (err) {
      return { ok: false, message: `Ошибка остановки: ${String(err)}` };
    }
  });
});

ipcMain.handle('agent:status', async () => {
  return {
    running: !!agentChild,
    pid: agentChild ? agentChild.pid : null,
    telegramEnabled: telegramEnabledRuntime,
    baseUrl: backendBaseUrlRuntime,
    transitioning: agentTransitioning,
  };
});

ipcMain.handle('telegram:set-enabled', async (_evt, enabled) => {
  return withAgentOp(async () => {
    const configPath = resolveConfigPath('./config.yaml');
    const binPath = defaultMonitordBin();
    const targetEnabled = !!enabled;
    const prevEnabled = telegramEnabledRuntime;

    try {
      await stopAgentProcess();
      const started = await startAgentManaged({
        binPath,
        configPath,
        telegramEnabled: targetEnabled,
      });
      if (!started.ok) {
        const rollback = await startAgentManaged({
          binPath,
          configPath,
          telegramEnabled: prevEnabled,
        });
        if (rollback.ok) {
          return {
            ok: false,
            message: `Не удалось переключить Telegram (${started.message}). Сервис восстановлен в прошлом режиме.`,
          };
        }
        return {
          ok: false,
          message: `Не удалось переключить Telegram (${started.message}) и восстановить предыдущий режим.`,
        };
      }
      return {
        ok: true,
        message: targetEnabled
          ? 'Telegram-бот включен (сервис перезапущен)'
          : 'Telegram-бот выключен (сервис перезапущен)',
      };
    } catch (err) {
      return { ok: false, message: `Ошибка переключения Telegram: ${String(err)}` };
    }
  });
});

ipcMain.handle('state:fetch', async (_evt, baseUrl) => {
  const url = `${baseUrl || 'http://127.0.0.1:9108'}/api/state`;
  try {
    const response = await fetch(url, { method: 'GET' });
    if (!response.ok) {
      return { ok: false, message: `HTTP ${response.status}` };
    }
    const json = await response.json();
    return { ok: true, data: json };
  } catch (err) {
    return { ok: false, message: String(err) };
  }
});

ipcMain.handle('config:read', async (_evt, configPath) => {
  try {
    const fullPath = resolveConfigPath(configPath);
    const text = await fs.readFile(fullPath, 'utf8');
    return { ok: true, text };
  } catch (err) {
    return { ok: false, message: String(err) };
  }
});

ipcMain.handle('config:write', async (_evt, payload) => {
  try {
    const fullPath = resolveConfigPath(payload.configPath);
    await fs.writeFile(fullPath, payload.text, 'utf8');
    return { ok: true };
  } catch (err) {
    return { ok: false, message: String(err) };
  }
});

ipcMain.handle('config:load-structured', async (_evt, configPath) => {
  try {
    const fullPath = resolveConfigPath(configPath || './config.yaml');
    const text = await fs.readFile(fullPath, 'utf8');
    const parsed = YAML.parse(text) || {};
    return { ok: true, data: parsed };
  } catch (err) {
    return { ok: false, message: String(err) };
  }
});

ipcMain.handle('config:save-structured', async (_evt, payload) => {
  try {
    const fullPath = resolveConfigPath(payload.configPath || './config.yaml');
    const yamlText = YAML.stringify(payload.data || {});
    await fs.writeFile(fullPath, yamlText, 'utf8');
    return { ok: true };
  } catch (err) {
    return { ok: false, message: String(err) };
  }
});


