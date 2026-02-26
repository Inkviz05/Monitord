# monitord

`monitord` — агент мониторинга на Rust + desktop UI (Electron):
- системные метрики (CPU/RAM/диски/сеть/температуры/GPU)
- экспорт метрик для Prometheus (`/metrics`)
- Telegram-бот (личные чаты, allowlist, алерты)
- desktop-приложение с автозапуском backend и настройкой конфига

## Возможности

- Асинхронная архитектура на `tokio`
- Единое состояние в памяти (`Arc<RwLock<State>>`)
- Быстрый `/metrics` без тяжелых вычислений в handler
- Graceful shutdown по `Ctrl+C`
- Desktop UI: мониторинг, управление сервисом, включение/выключение Telegram-бота, редактирование настроек

## Структура

- Rust backend: `src/`
- Desktop UI (Electron): `desktop-electron/`
- Конфиг-шаблон: `config.yaml.example`

## Локальный запуск (Rust backend)

1. Создайте `config.yaml`:

```bash
cp config.yaml.example config.yaml
```

2. Если Telegram включен (`telegram.enabled: true`), задайте токен:

PowerShell:

```powershell
$env:TELEGRAM_BOT_TOKEN="<токен>"
```

bash:

```bash
export TELEGRAM_BOT_TOKEN="<токен>"
```

3. Запустите backend:

```bash
cargo run --target-dir build_target -- --config ./config.yaml
```

Печать шаблона конфига:

```bash
cargo run --target-dir build_target -- --print-default-config
```

## Локальный запуск (Desktop)

```bash
cd desktop-electron
npm install
npm start
```

Важно:
- Desktop запускает backend автоматически.
- В development-режиме берется `./config.yaml` из корня проекта.

## HTTP API

- `GET /healthz` -> `ok`
- `GET /metrics` -> Prometheus text format
- `GET /api/state` -> JSON-снимок состояния (используется desktop UI)

Проверка:

```bash
curl http://127.0.0.1:9108/healthz
curl http://127.0.0.1:9108/metrics
curl http://127.0.0.1:9108/api/state
```

## Telegram-бот

Основные команды:

- `/start`
- `/help`
- `/status` (дашборд)
- `/system`
- `/gpu`
- `/network`
- `/speedtest`
- `/alerts_on`, `/alerts_off`, `/alerts_status`

Сообщения из групп/каналов игнорируются.

## Сборка desktop в `.exe` / installer / portable

Из корня проекта:

```bash
npm run pack:win
```

Дополнительно:

```bash
npm run pack:win:portable
npm run pack:dir
npm run pack:linux
npm run pack:mac
```

Что делают команды:

- `pack:win` — Windows installer (NSIS)
- `pack:win:portable` — portable `.exe`
- `pack:dir` — unpacked папка приложения
- `pack:linux` — `AppImage` + `deb`
- `pack:mac` — `dmg` + `zip`

Артефакты: `desktop-electron/dist/`.

Важно по платформам:

- Стабильно собирать нужно на целевой ОС (Linux на Linux, macOS на macOS).
- На Windows гарантированно корректны `pack:win`, `pack:win:portable`, `pack:dir`.

## Runtime-конфиг в установленной desktop-версии

В packaged-режиме приложение использует конфиг в `%APPDATA%`:

- `%APPDATA%/monitord desktop/config.yaml`

Если файла нет, он создается автоматически из `config.yaml.example`.

## Prometheus

Пример `scrape_configs`:

```yaml
scrape_configs:
  - job_name: "monitord"
    static_configs:
      - targets: ["127.0.0.1:9108"]
```

## Ограничения MVP

- Настройки алертов per chat (runtime переключатели) хранятся в памяти и сбрасываются после рестарта.
- Доступность температур/части сенсоров зависит от драйверов и окружения ОС.
- Кросс-платформенная упаковка desktop должна выполняться на соответствующей платформе.
