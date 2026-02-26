mod collectors;
mod config;
mod http;
mod metrics;
mod state;
mod telegram;

use axum::serve;
use clap::Parser;
use collectors::checks::collect_checks;
use collectors::system::collect_system;
use config::Config;
use metrics::Metrics;
use reqwest::Client;
use state::{InternetSpeedStat, ResourceAlert, ResourceAlertKind, State};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use sysinfo::SystemExt;
use teloxide::Bot;
use tokio::net::TcpListener;
use tokio::sync::{watch, RwLock};
use tokio::time::MissedTickBehavior;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "monitord")]
#[command(version)]
struct Cli {
    #[arg(long, default_value = "./config.yaml")]
    config: String,
    #[arg(long)]
    print_default_config: bool,
    #[arg(long, conflicts_with = "telegram_off")]
    telegram_on: bool,
    #[arg(long, conflicts_with = "telegram_on")]
    telegram_off: bool,
}

#[tokio::main]
async fn main() {
    init_tracing();

    let cli = Cli::parse();
    if cli.print_default_config {
        println!("{}", Config::example_yaml());
        return;
    }

    let mut cfg = match Config::load_from_file(&cli.config) {
        Ok(cfg) => cfg,
        Err(err) => {
            error!(error = %err, "РЅРµ СѓРґР°Р»РѕСЃСЊ Р·Р°РіСЂСѓР·РёС‚СЊ РєРѕРЅС„РёРіСѓСЂР°С†РёСЋ");
            std::process::exit(1);
        }
    };
    if cli.telegram_on {
        cfg.telegram.enabled = true;
    } else if cli.telegram_off {
        cfg.telegram.enabled = false;
    }

    let telegram_token = if cfg.telegram.enabled {
        match ensure_telegram_settings(&cfg) {
            Ok(token) => Some(token),
            Err(err) => {
                error!(error = %err, "РЅРµ СѓРґР°Р»РѕСЃСЊ РїРѕРґРіРѕС‚РѕРІРёС‚СЊ РЅР°СЃС‚СЂРѕР№РєРё Telegram");
                std::process::exit(1);
            }
        }
    } else {
        None
    };

    info!(
        listen = %cfg.listen,
        interval_secs = cfg.interval_secs,
        "Р·Р°РїСѓСЃРє monitord"
    );

    let now = now_unix();
    let shared_state = Arc::new(RwLock::new(State::new(now)));
    let metrics = match Metrics::new() {
        Ok(m) => m,
        Err(err) => {
            error!(error = %err, "РЅРµ СѓРґР°Р»РѕСЃСЊ РёРЅРёС†РёР°Р»РёР·РёСЂРѕРІР°С‚СЊ РјРµС‚СЂРёРєРё");
            std::process::exit(1);
        }
    };

    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let http_task = {
        let cfg = cfg.clone();
        let metrics = metrics.clone();
        let http_state = shared_state.clone();
        let mut shutdown_rx = shutdown_rx.clone();
        tokio::spawn(async move {
            let app = http::build_router(metrics, http_state);
            let addr: SocketAddr = match cfg.listen.parse() {
                Ok(addr) => addr,
                Err(err) => {
                    error!(error = %err, listen = %cfg.listen, "РЅРµРєРѕСЂСЂРµРєС‚РЅС‹Р№ Р°РґСЂРµСЃ listen");
                    return;
                }
            };

            let listener = match TcpListener::bind(addr).await {
                Ok(l) => l,
                Err(err) => {
                    error!(error = %err, "РЅРµ СѓРґР°Р»РѕСЃСЊ Р·Р°РїСѓСЃС‚РёС‚СЊ HTTP-СЃРµСЂРІРµСЂ");
                    return;
                }
            };

            let server = serve(listener, app).with_graceful_shutdown(async move {
                let _ = shutdown_rx.changed().await;
            });

            if let Err(err) = server.await {
                error!(error = %err, "РѕС€РёР±РєР° HTTP-СЃРµСЂРІРµСЂР°");
            }
        })
    };

    let telegram_bot = if cfg.telegram.enabled {
        Some(Bot::new(telegram_token.unwrap_or_default()))
    } else {
        None
    };

    let telegram_task = if let Some(bot) = telegram_bot.clone() {
        let telegram_cfg = cfg.telegram.clone();
        let state = shared_state.clone();
        let shutdown = shutdown_rx.clone();
        Some(tokio::spawn(async move {
            if let Err(err) = telegram::run_bot(bot, telegram_cfg, state, shutdown).await {
                error!(error = %err, "РѕС€РёР±РєР° Р·Р°РґР°С‡Рё Telegram");
            }
        }))
    } else {
        None
    };

    let collector_task = {
        let cfg = cfg.clone();
        let metrics = metrics.clone();
        let shared_state = shared_state.clone();
        let mut shutdown = shutdown_rx.clone();
        let telegram_bot = telegram_bot.clone();
        tokio::spawn(async move {
            let client = Client::builder()
                .user_agent("monitord/0.1.0")
                .build()
                .unwrap_or_else(|_| Client::new());
            let mut system = sysinfo::System::new_all();
            let mut ticker = tokio::time::interval(Duration::from_secs(cfg.interval_secs));
            ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
            let mut resource_alert_last_sent: HashMap<String, i64> = HashMap::new();
            let mut internet_speed: Option<InternetSpeedStat> = None;
            let mut last_speedtest_unix = 0_i64;

            loop {
                tokio::select! {
                    _ = shutdown.changed() => {
                        info!("РїРѕР»СѓС‡РµРЅ СЃРёРіРЅР°Р» РѕСЃС‚Р°РЅРѕРІРєРё С†РёРєР»Р° СЃР±РѕСЂР°");
                        break;
                    }
                    _ = ticker.tick() => {
                        let system_snapshot = collect_system(&mut system);
                        let (check_results, check_errors) = collect_checks(&client, &cfg).await;
                        for _ in 0..check_errors {
                            metrics.inc_collect_error("checks");
                        }

                        let now = now_unix();
                        if now.saturating_sub(last_speedtest_unix) >= 30 {
                            match collect_internet_speed(&client).await {
                                Ok(sample) => {
                                    internet_speed = Some(sample);
                                    last_speedtest_unix = now;
                                }
                                Err(err) => {
                                    metrics.inc_collect_error("internet_speed");
                                    tracing::debug!(error = %err, "speedtest РЅРµ РІС‹РїРѕР»РЅРµРЅ");
                                }
                            }
                        }
                        let (snapshot, alert_events) = {
                            let mut guard = shared_state.write().await;
                            guard.update_collected(
                                now,
                                system_snapshot.host_name,
                                system_snapshot.os_name,
                                system_snapshot.os_version,
                                system_snapshot.kernel_version,
                                system_snapshot.cpu_brand,
                                system_snapshot.uptime_seconds,
                                system_snapshot.process_count,
                                system_snapshot.cpu_core_count,
                                system_snapshot.cpu_usage_percent,
                                system_snapshot.memory_used_bytes,
                                system_snapshot.memory_total_bytes,
                                system_snapshot.disks,
                                system_snapshot.net,
                                internet_speed.clone(),
                                system_snapshot.temps,
                                system_snapshot.gpus,
                                system_snapshot.sensors,
                                check_results,
                            );
                            let events = guard.apply_alert_rules(&cfg.telegram.alerts, now);
                            (guard.clone(), events)
                        };

                        metrics.update_from_state(&snapshot);

                        if let (Some(bot), true) = (&telegram_bot, cfg.telegram.enabled) {
                            let sent_check_alerts = telegram::send_alert_events(
                                bot,
                                &cfg.telegram,
                                shared_state.clone(),
                                &alert_events,
                            )
                            .await;
                            for _ in 0..sent_check_alerts {
                                metrics.inc_alert_sent("check");
                            }

                            let texts = collect_resource_alerts(
                                &snapshot,
                                &cfg.telegram.alerts,
                                now,
                                &mut resource_alert_last_sent,
                            );
                            let sent_resource_alerts = telegram::send_text_alerts(
                                bot,
                                &cfg.telegram,
                                shared_state.clone(),
                                &texts,
                            )
                            .await;
                            for _ in 0..sent_resource_alerts {
                                metrics.inc_alert_sent("resource");
                            }
                        }
                    }
                }
            }
        })
    };

    if let Err(err) = tokio::signal::ctrl_c().await {
        error!(error = %err, "РЅРµ СѓРґР°Р»РѕСЃСЊ РґРѕР¶РґР°С‚СЊСЃСЏ Ctrl+C");
    }
    info!("РїРѕР»СѓС‡РµРЅ Ctrl+C, РІС‹РїРѕР»РЅСЏРµС‚СЃСЏ РѕСЃС‚Р°РЅРѕРІРєР°");

    let _ = shutdown_tx.send(true);

    let _ = collector_task.await;
    if let Some(task) = telegram_task {
        let _ = task.await;
    }
    let _ = http_task.await;
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

async fn collect_internet_speed(client: &Client) -> Result<InternetSpeedStat, reqwest::Error> {
    const DOWNLOAD_BYTES: usize = 10_000_000;
    const UPLOAD_BYTES: usize = 2_000_000;

    let ping_start = Instant::now();
    let _ = client
        .head("https://speed.cloudflare.com/")
        .timeout(Duration::from_secs(6))
        .send()
        .await?;
    let latency_ms = ping_start.elapsed().as_secs_f64() * 1000.0;

    let down_url = format!("https://speed.cloudflare.com/__down?bytes={DOWNLOAD_BYTES}");
    let down_start = Instant::now();
    let down = client
        .get(down_url)
        .timeout(Duration::from_secs(20))
        .send()
        .await?;
    let down_bytes = down.bytes().await?;
    let down_secs = down_start.elapsed().as_secs_f64().max(0.001);
    let download_mbps = ((down_bytes.len() as f64) * 8.0 / 1_000_000.0) / down_secs;

    let upload_buf = vec![0_u8; UPLOAD_BYTES];
    let up_start = Instant::now();
    let _ = client
        .post("https://speed.cloudflare.com/__up")
        .timeout(Duration::from_secs(20))
        .body(upload_buf)
        .send()
        .await?;
    let up_secs = up_start.elapsed().as_secs_f64().max(0.001);
    let upload_mbps = ((UPLOAD_BYTES as f64) * 8.0 / 1_000_000.0) / up_secs;

    Ok(InternetSpeedStat {
        download_mbps,
        upload_mbps,
        latency_ms: Some(latency_ms),
        measured_at_unix: now_unix(),
    })
}

fn collect_resource_alerts(
    state: &State,
    alerts: &config::AlertsConfig,
    now_unix: i64,
    last_sent: &mut HashMap<String, i64>,
) -> Vec<ResourceAlert> {
    if !alerts.resource_alerts_enabled {
        return Vec::new();
    }

    let cooldown = alerts.resource_alert_cooldown_secs as i64;
    let mut out = Vec::new();

    let gpu_load_max = state
        .gpus
        .iter()
        .filter_map(|g| g.utilization_percent)
        .fold(0.0_f64, f64::max);
    if gpu_load_max >= alerts.gpu_load_threshold_percent
        && should_emit("gpu_load", now_unix, cooldown, last_sent)
    {
        out.push(ResourceAlert {
            kind: ResourceAlertKind::GpuLoad,
            text: format!(
                "⚠ <b>Высокая нагрузка GPU</b>\nТекущее значение: {:.1}% (порог {:.1}%)",
                gpu_load_max, alerts.gpu_load_threshold_percent
            ),
        });
    }

    let gpu_temp_max = state
        .gpus
        .iter()
        .filter_map(|g| g.temperature_celsius)
        .fold(0.0_f64, f64::max);
    if gpu_temp_max >= alerts.gpu_temp_threshold_celsius
        && should_emit("gpu_temp", now_unix, cooldown, last_sent)
    {
        out.push(ResourceAlert {
            kind: ResourceAlertKind::GpuTemp,
            text: format!(
                "🔥 <b>Высокая температура GPU</b>\nТекущее значение: {:.1}°C (порог {:.1}°C)",
                gpu_temp_max, alerts.gpu_temp_threshold_celsius
            ),
        });
    }

    if let Some(cpu_temp) = cpu_temperature_from_state(state) {
        if cpu_temp >= alerts.cpu_temp_threshold_celsius
            && should_emit("cpu_temp", now_unix, cooldown, last_sent)
        {
            out.push(ResourceAlert {
                kind: ResourceAlertKind::CpuTemp,
                text: format!(
                    "🔥 <b>Высокая температура CPU</b>\nТекущее значение: {:.1}°C (порог {:.1}°C)",
                    cpu_temp, alerts.cpu_temp_threshold_celsius
                ),
            });
        }
    }

    if state.cpu_usage_percent >= alerts.cpu_load_threshold_percent
        && should_emit("cpu_load", now_unix, cooldown, last_sent)
    {
        out.push(ResourceAlert {
            kind: ResourceAlertKind::CpuLoad,
            text: format!(
                "⚠ <b>Высокая нагрузка CPU</b>\nТекущее значение: {:.1}% (порог {:.1}%)",
                state.cpu_usage_percent, alerts.cpu_load_threshold_percent
            ),
        });
    }

    let ram_usage = if state.memory_total_bytes > 0 {
        (state.memory_used_bytes as f64 / state.memory_total_bytes as f64) * 100.0
    } else {
        0.0
    };
    if ram_usage >= alerts.ram_usage_threshold_percent
        && should_emit("ram_usage", now_unix, cooldown, last_sent)
    {
        out.push(ResourceAlert {
            kind: ResourceAlertKind::RamUsage,
            text: format!(
                "⚠ <b>Высокое использование RAM</b>\nТекущее значение: {:.1}% (порог {:.1}%)",
                ram_usage, alerts.ram_usage_threshold_percent
            ),
        });
    }

    let disk_worst = state
        .disks
        .iter()
        .map(|d| {
            let pct = if d.total_bytes > 0 {
                (d.used_bytes as f64 / d.total_bytes as f64) * 100.0
            } else {
                0.0
            };
            (d.mount.as_str(), pct)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((mount, used_pct)) = disk_worst {
        if used_pct >= alerts.disk_usage_threshold_percent
            && should_emit("disk_usage", now_unix, cooldown, last_sent)
        {
            out.push(ResourceAlert {
                kind: ResourceAlertKind::DiskUsage,
                text: format!(
                    "⚠ <b>Высокая заполненность диска</b>\nДиск: {mount}\nТекущее значение: {:.1}% (порог {:.1}%)",
                    used_pct, alerts.disk_usage_threshold_percent
                ),
            });
        }
    }

    out
}
fn should_emit(
    key: &str,
    now_unix: i64,
    cooldown_secs: i64,
    last_sent: &mut HashMap<String, i64>,
) -> bool {
    if let Some(last) = last_sent.get(key) {
        if now_unix - *last < cooldown_secs {
            return false;
        }
    }
    last_sent.insert(key.to_string(), now_unix);
    true
}

fn cpu_temperature_from_state(state: &State) -> Option<f64> {
    let primary_markers = ["cpu", "package", "tctl", "tdie", "coretemp", "k10temp"];
    let primary = state
        .temps
        .iter()
        .filter(|t| (0.0..=130.0).contains(&t.temperature_celsius))
        .filter(|t| {
            let s = t.sensor.to_lowercase();
            primary_markers.iter().any(|m| s.contains(m))
                && !s.contains("gpu")
                && !s.contains("nvidia")
                && !s.contains("amdgpu")
                && !s.contains("radeon")
                && !s.contains("acpi")
                && !s.contains("thermal zone")
                && !s.contains("_tz")
        })
        .map(|t| t.temperature_celsius)
        .max_by(|a, b| a.total_cmp(b));
    if primary.is_some() {
        return primary;
    }

    let fallback_non_gpu = state
        .temps
        .iter()
        .filter(|t| (0.0..=130.0).contains(&t.temperature_celsius))
        .filter(|t| {
            let s = t.sensor.to_lowercase();
            !s.contains("gpu")
                && !s.contains("nvidia")
                && !s.contains("amdgpu")
                && !s.contains("radeon")
        })
        .map(|t| t.temperature_celsius)
        .max_by(|a, b| a.total_cmp(b));
    if fallback_non_gpu.is_some() {
        return fallback_non_gpu;
    }

    state
        .temps
        .iter()
        .filter(|t| (0.0..=130.0).contains(&t.temperature_celsius))
        .filter(|t| {
            let s = t.sensor.to_lowercase();
            s.contains("acpi") || s.contains("thermal zone") || s.contains("_tz")
        })
        .map(|t| t.temperature_celsius)
        .max_by(|a, b| a.total_cmp(b))
}

fn resolve_telegram_token_from_env(env_name: &str) -> Option<String> {
    if let Ok(v) = std::env::var(env_name) {
        if !v.trim().is_empty() {
            return Some(v);
        }
    }
    None
}

fn ensure_telegram_settings(cfg: &Config) -> Result<String, String> {
    let env_name = cfg.telegram.bot_token_env.clone();
    let env_token = resolve_telegram_token_from_env(&env_name);
    let cfg_token = cfg
        .telegram
        .bot_token
        .as_ref()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());

    if cfg.telegram.allowed_chat_ids.is_empty() {
        return Err(
            "telegram.allowed_chat_ids РїСѓСЃС‚: СѓРєР°Р¶РёС‚Рµ С…РѕС‚СЏ Р±С‹ РѕРґРёРЅ chat id РІ config".to_string(),
        );
    }

    if let Some(v) = env_token {
        return Ok(v);
    }
    if let Some(v) = cfg_token {
        return Ok(v);
    }

    Err(format!(
        "РЅРµ РЅР°Р№РґРµРЅ С‚РѕРєРµРЅ Telegram: Р·Р°РґР°Р№С‚Рµ '{}' РІ РѕРєСЂСѓР¶РµРЅРёРё РёР»Рё telegram.bot_token РІ config",
        env_name
    ))
}
