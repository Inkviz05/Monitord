use crate::config::TelegramConfig;
use crate::state::{
    AlertEvent, AlertEventKind, CheckKind, ResourceAlert, ResourceAlertKind, State,
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use teloxide::prelude::*;
use teloxide::types::{
    CallbackQuery, ChatId, InlineKeyboardButton, InlineKeyboardMarkup, Message, MessageId,
    ParseMode,
};
use thiserror::Error;
use tokio::sync::{watch, Mutex, RwLock};
use tracing::{info, warn};

#[derive(Debug, Error)]
pub enum TelegramError {
    #[error("ошибка запроса Telegram: {0}")]
    Request(#[from] teloxide::RequestError),
}

#[derive(Clone)]
struct TelegramRuntime {
    cfg: TelegramConfig,
    shared_state: Arc<RwLock<State>>,
    allowed_chats: HashSet<i64>,
    limiter: Arc<Mutex<RateLimiter>>,
    dashboard_messages: Arc<Mutex<HashMap<i64, i32>>>,
    speed_history: Arc<Mutex<VecDeque<SpeedSample>>>,
}

#[derive(Clone, Copy)]
enum Action {
    Start,
    Help,
    Refresh,
    Dashboard,
    System,
    Sensors,
    Network,
    Speed,
    Disks,
    Gpu,
    Alerts,
    ToggleAlerts,
    ToggleChecksAlert,
    ToggleCpuTempAlert,
    ToggleGpuTempAlert,
    ToggleCpuLoadAlert,
    ToggleGpuLoadAlert,
    ToggleRamUsageAlert,
    ToggleDiskUsageAlert,
}

impl Action {
    fn from_command(text: &str) -> Option<Self> {
        let first = text.split_whitespace().next()?;
        let normalized = first.split('@').next()?.to_lowercase();
        match normalized.as_str() {
            "/start" => Some(Self::Start),
            "/help" => Some(Self::Help),
            "/status" => Some(Self::Dashboard),
            "/system" => Some(Self::System),
            "/sensors" => Some(Self::Sensors),
            "/network" => Some(Self::Network),
            "/speed" | "/speedtest" => Some(Self::Speed),
            "/disks" => Some(Self::Disks),
            "/gpu" => Some(Self::Gpu),
            "/alerts_on" | "/alerts_off" | "/alerts_status" => Some(Self::Alerts),
            _ => None,
        }
    }

    fn from_callback(data: &str) -> Option<Self> {
        match data {
            "refresh" => Some(Self::Refresh),
            "dashboard" => Some(Self::Dashboard),
            "system" => Some(Self::System),
            "sensors" => Some(Self::Sensors),
            "network" => Some(Self::Network),
            "speed" => Some(Self::Speed),
            "disks" => Some(Self::Disks),
            "gpu" => Some(Self::Gpu),
            "alerts" => Some(Self::Alerts),
            "alerts_toggle" => Some(Self::ToggleAlerts),
            "alerts_checks_toggle" => Some(Self::ToggleChecksAlert),
            "alerts_cpu_temp_toggle" => Some(Self::ToggleCpuTempAlert),
            "alerts_gpu_temp_toggle" => Some(Self::ToggleGpuTempAlert),
            "alerts_cpu_load_toggle" => Some(Self::ToggleCpuLoadAlert),
            "alerts_gpu_load_toggle" => Some(Self::ToggleGpuLoadAlert),
            "alerts_ram_usage_toggle" => Some(Self::ToggleRamUsageAlert),
            "alerts_disk_usage_toggle" => Some(Self::ToggleDiskUsageAlert),
            "help" => Some(Self::Help),
            _ => None,
        }
    }
}

struct RenderedView {
    text: String,
    keyboard: InlineKeyboardMarkup,
}

pub async fn run_bot(
    bot: Bot,
    cfg: TelegramConfig,
    shared_state: Arc<RwLock<State>>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), TelegramError> {
    let runtime = TelegramRuntime {
        cfg: cfg.clone(),
        shared_state,
        allowed_chats: cfg.allowed_chat_ids.iter().copied().collect(),
        limiter: Arc::new(Mutex::new(RateLimiter::new(cfg.rate_limit_per_minute))),
        dashboard_messages: Arc::new(Mutex::new(HashMap::new())),
        speed_history: Arc::new(Mutex::new(VecDeque::new())),
    };

    let handler = dptree::entry()
        .branch(Update::filter_message().endpoint(handle_message))
        .branch(Update::filter_callback_query().endpoint(handle_callback));

    let mut dispatcher = Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![runtime])
        .build();

    let mut dispatch_handle = tokio::spawn(async move {
        dispatcher.dispatch().await;
    });

    tokio::select! {
        _ = shutdown.changed() => {
            dispatch_handle.abort();
            let _ = (&mut dispatch_handle).await;
            info!("остановка Telegram-бота");
            Ok(())
        }
        result = &mut dispatch_handle => {
            match result {
                Ok(()) => Ok(()),
                Err(join_err) if join_err.is_cancelled() => Ok(()),
                Err(join_err) => {
                    warn!(error = %join_err, "задача Telegram завершилась с ошибкой");
                    Ok(())
                }
            }
        }
    }
}

async fn handle_message(bot: Bot, msg: Message, runtime: TelegramRuntime) -> ResponseResult<()> {
    let chat_id = msg.chat.id.0;
    if !should_handle_message(msg.chat.is_private(), chat_id, &runtime.allowed_chats) {
        return Ok(());
    }

    if !consume_rate_limit(&runtime, chat_id).await {
        bot.send_message(
            msg.chat.id,
            "Слишком много запросов. Попробуйте чуть позже.",
        )
        .await?;
        return Ok(());
    }

    let action = msg
        .text()
        .and_then(Action::from_command)
        .unwrap_or(Action::Start);

    let response = render_action(action, chat_id, &runtime).await;
    upsert_dashboard_message(&bot, msg.chat.id, &runtime, response).await?;
    Ok(())
}

async fn handle_callback(
    bot: Bot,
    q: CallbackQuery,
    runtime: TelegramRuntime,
) -> ResponseResult<()> {
    let Some(data) = q.data.as_deref() else {
        return Ok(());
    };
    let Some(message) = q.message.as_ref() else {
        bot.answer_callback_query(q.id).await?;
        return Ok(());
    };

    let chat_id = message.chat.id.0;
    if !should_handle_message(message.chat.is_private(), chat_id, &runtime.allowed_chats) {
        bot.answer_callback_query(q.id).await?;
        return Ok(());
    }

    if !consume_rate_limit(&runtime, chat_id).await {
        bot.answer_callback_query(q.id)
            .text("Слишком много запросов. Попробуйте позже.")
            .await?;
        return Ok(());
    }

    {
        let mut map = runtime.dashboard_messages.lock().await;
        map.insert(chat_id, message.id.0);
    }

    if let Some(action) = Action::from_callback(data) {
        let response = render_action(action, chat_id, &runtime).await;
        upsert_dashboard_message(&bot, message.chat.id, &runtime, response).await?;
    }

    bot.answer_callback_query(q.id).await?;
    Ok(())
}

async fn render_action(action: Action, chat_id: i64, runtime: &TelegramRuntime) -> RenderedView {
    match action {
        Action::Start => RenderedView {
            text: "<b>monitord</b> запущен. Нажмите кнопку ниже для сводки.".to_string(),
            keyboard: main_menu(),
        },
        Action::Help => RenderedView {
            text: help_text(),
            keyboard: main_menu(),
        },
        Action::Refresh | Action::Dashboard => {
            let state = runtime.shared_state.read().await;
            let sample = make_speed_sample(&state);
            let text = format_status(&state, &runtime.cfg);
            drop(state);
            push_speed_sample(runtime, sample).await;
            RenderedView {
                text,
                keyboard: main_menu(),
            }
        }
        Action::System => {
            let state = runtime.shared_state.read().await;
            let sample = make_speed_sample(&state);
            let text = format_system(&state);
            drop(state);
            push_speed_sample(runtime, sample).await;
            RenderedView {
                text,
                keyboard: main_menu(),
            }
        }
        Action::Sensors => {
            let state = runtime.shared_state.read().await;
            let sample = make_speed_sample(&state);
            let text = format_sensors(&state);
            drop(state);
            push_speed_sample(runtime, sample).await;
            RenderedView {
                text,
                keyboard: main_menu(),
            }
        }
        Action::Network => {
            let state = runtime.shared_state.read().await;
            let sample = make_speed_sample(&state);
            let text = format_network(&state);
            drop(state);
            push_speed_sample(runtime, sample).await;
            RenderedView {
                text,
                keyboard: main_menu(),
            }
        }
        Action::Speed => {
            let state = runtime.shared_state.read().await;
            let sample = make_speed_sample(&state);
            let snapshot = state.clone();
            drop(state);
            push_speed_sample(runtime, sample).await;
            let history = {
                let history = runtime.speed_history.lock().await;
                history.clone()
            };
            RenderedView {
                text: format_speedtest(&snapshot, &history),
                keyboard: main_menu(),
            }
        }
        Action::Disks => {
            let state = runtime.shared_state.read().await;
            let sample = make_speed_sample(&state);
            let text = format_disks(&state);
            drop(state);
            push_speed_sample(runtime, sample).await;
            RenderedView {
                text,
                keyboard: main_menu(),
            }
        }
        Action::Gpu => {
            let state = runtime.shared_state.read().await;
            let sample = make_speed_sample(&state);
            let text = format_gpu_details(&state);
            drop(state);
            push_speed_sample(runtime, sample).await;
            RenderedView {
                text,
                keyboard: main_menu(),
            }
        }
        Action::Alerts => {
            let state = runtime.shared_state.read().await;
            let enabled =
                state.alerts_enabled_for_chat(chat_id, runtime.cfg.alerts.enabled_by_default);
            let text = format_alerts_page(&state, chat_id, runtime.cfg.alerts.enabled_by_default);
            let keyboard = alerts_menu(&state, chat_id, enabled);
            RenderedView { text, keyboard }
        }
        Action::ToggleAlerts => {
            let mut state = runtime.shared_state.write().await;
            let current =
                state.alerts_enabled_for_chat(chat_id, runtime.cfg.alerts.enabled_by_default);
            let next = !current;
            state.set_alerts_enabled_for_chat(chat_id, next);
            state.set_check_alerts_enabled_for_chat(chat_id, next);
            state.set_resource_alert_enabled_for_chat(chat_id, ResourceAlertKind::CpuTemp, next);
            state.set_resource_alert_enabled_for_chat(chat_id, ResourceAlertKind::GpuTemp, next);
            state.set_resource_alert_enabled_for_chat(chat_id, ResourceAlertKind::CpuLoad, next);
            state.set_resource_alert_enabled_for_chat(chat_id, ResourceAlertKind::GpuLoad, next);
            state.set_resource_alert_enabled_for_chat(chat_id, ResourceAlertKind::RamUsage, next);
            state.set_resource_alert_enabled_for_chat(chat_id, ResourceAlertKind::DiskUsage, next);
            let text = format_alerts_page(&state, chat_id, runtime.cfg.alerts.enabled_by_default);
            let keyboard = alerts_menu(&state, chat_id, next);
            RenderedView { text, keyboard }
        }
        Action::ToggleChecksAlert => {
            let mut state = runtime.shared_state.write().await;
            let current = state.check_alerts_enabled_for_chat(chat_id);
            state.set_check_alerts_enabled_for_chat(chat_id, !current);
            let enabled =
                state.alerts_enabled_for_chat(chat_id, runtime.cfg.alerts.enabled_by_default);
            let text = format_alerts_page(&state, chat_id, runtime.cfg.alerts.enabled_by_default);
            let keyboard = alerts_menu(&state, chat_id, enabled);
            RenderedView { text, keyboard }
        }
        Action::ToggleCpuTempAlert => {
            toggle_resource_alert(
                runtime,
                chat_id,
                ResourceAlertKind::CpuTemp,
                runtime.cfg.alerts.enabled_by_default,
            )
            .await
        }
        Action::ToggleGpuTempAlert => {
            toggle_resource_alert(
                runtime,
                chat_id,
                ResourceAlertKind::GpuTemp,
                runtime.cfg.alerts.enabled_by_default,
            )
            .await
        }
        Action::ToggleCpuLoadAlert => {
            toggle_resource_alert(
                runtime,
                chat_id,
                ResourceAlertKind::CpuLoad,
                runtime.cfg.alerts.enabled_by_default,
            )
            .await
        }
        Action::ToggleGpuLoadAlert => {
            toggle_resource_alert(
                runtime,
                chat_id,
                ResourceAlertKind::GpuLoad,
                runtime.cfg.alerts.enabled_by_default,
            )
            .await
        }
        Action::ToggleRamUsageAlert => {
            toggle_resource_alert(
                runtime,
                chat_id,
                ResourceAlertKind::RamUsage,
                runtime.cfg.alerts.enabled_by_default,
            )
            .await
        }
        Action::ToggleDiskUsageAlert => {
            toggle_resource_alert(
                runtime,
                chat_id,
                ResourceAlertKind::DiskUsage,
                runtime.cfg.alerts.enabled_by_default,
            )
            .await
        }
    }
}

async fn toggle_resource_alert(
    runtime: &TelegramRuntime,
    chat_id: i64,
    kind: ResourceAlertKind,
    default_enabled: bool,
) -> RenderedView {
    let mut state = runtime.shared_state.write().await;
    let current = state.resource_alert_enabled_for_chat(chat_id, kind);
    state.set_resource_alert_enabled_for_chat(chat_id, kind, !current);
    let enabled = state.alerts_enabled_for_chat(chat_id, default_enabled);
    let text = format_alerts_page(&state, chat_id, default_enabled);
    let keyboard = alerts_menu(&state, chat_id, enabled);
    RenderedView { text, keyboard }
}

fn alert_kind_title(kind: ResourceAlertKind) -> &'static str {
    match kind {
        ResourceAlertKind::CpuTemp => "CPU температура",
        ResourceAlertKind::GpuTemp => "GPU температура",
        ResourceAlertKind::CpuLoad => "CPU нагрузка",
        ResourceAlertKind::GpuLoad => "GPU нагрузка",
        ResourceAlertKind::RamUsage => "RAM использование",
        ResourceAlertKind::DiskUsage => "Диск заполнение",
    }
}

fn format_alerts_page(state: &State, chat_id: i64, default_enabled: bool) -> String {
    let global = state.alerts_enabled_for_chat(chat_id, default_enabled);
    let mut lines = vec!["<b>Настройки уведомлений</b>".to_string()];
    lines.push(format!(
        "Общие уведомления: {}",
        if global {
            "включены"
        } else {
            "выключены"
        }
    ));
    lines.push(String::new());

    let kinds = [
        ResourceAlertKind::CpuTemp,
        ResourceAlertKind::GpuTemp,
        ResourceAlertKind::CpuLoad,
        ResourceAlertKind::GpuLoad,
        ResourceAlertKind::RamUsage,
        ResourceAlertKind::DiskUsage,
    ];

    lines.push("Типы уведомлений:".to_string());
    let checks_mark = if state.check_alerts_enabled_for_chat(chat_id) {
        "✅"
    } else {
        "❌"
    };
    lines.push(format!("{} Проверки", checks_mark));
    for kind in kinds {
        let enabled = state.resource_alert_enabled_for_chat(chat_id, kind);
        let mark = if enabled { "✅" } else { "❌" };
        lines.push(format!("{} {}", mark, alert_kind_title(kind)));
    }

    lines.join("\n")
}

fn alerts_menu(state: &State, chat_id: i64, alerts_enabled: bool) -> InlineKeyboardMarkup {
    let button_title = if alerts_enabled {
        "🔔 Отключить всё"
    } else {
        "🔕 Включить всё"
    };

    let row_button = |kind: ResourceAlertKind, data: &'static str| {
        let enabled = state.resource_alert_enabled_for_chat(chat_id, kind);
        let icon = if enabled { "✅" } else { "❌" };
        InlineKeyboardButton::callback(format!("{} {}", icon, alert_kind_title(kind)), data)
    };

    InlineKeyboardMarkup::new(vec![
        vec![InlineKeyboardButton::callback(
            button_title,
            "alerts_toggle",
        )],
        vec![InlineKeyboardButton::callback(
            format!(
                "{} Проверки",
                if state.check_alerts_enabled_for_chat(chat_id) {
                    "✅"
                } else {
                    "❌"
                }
            ),
            "alerts_checks_toggle",
        )],
        vec![
            row_button(ResourceAlertKind::CpuTemp, "alerts_cpu_temp_toggle"),
            row_button(ResourceAlertKind::GpuTemp, "alerts_gpu_temp_toggle"),
        ],
        vec![
            row_button(ResourceAlertKind::CpuLoad, "alerts_cpu_load_toggle"),
            row_button(ResourceAlertKind::GpuLoad, "alerts_gpu_load_toggle"),
        ],
        vec![
            row_button(ResourceAlertKind::RamUsage, "alerts_ram_usage_toggle"),
            row_button(ResourceAlertKind::DiskUsage, "alerts_disk_usage_toggle"),
        ],
        vec![InlineKeyboardButton::callback("⬅ Назад", "dashboard")],
    ])
}

fn main_menu() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![
            InlineKeyboardButton::callback("Обновить", "refresh"),
            InlineKeyboardButton::callback("Обзор", "dashboard"),
            InlineKeyboardButton::callback("Система", "system"),
        ],
        vec![
            InlineKeyboardButton::callback("Сенсоры", "sensors"),
            InlineKeyboardButton::callback("GPU", "gpu"),
            InlineKeyboardButton::callback("Speedtest", "speed"),
        ],
        vec![
            InlineKeyboardButton::callback("Уведомления", "alerts"),
            InlineKeyboardButton::callback("Помощь", "help"),
        ],
    ])
}

fn help_text() -> String {
    [
        "<b>Команды</b>",
        "• /status - общая сводка",
        "• /system - информация об ОС и CPU/RAM",
        "• /sensors - сводка по сенсорам",
        "• /network - трафик по интерфейсам",
        "• /speed - speedtest интернета",
        "• /disks - диски",
        "• /gpu - видеокарта",
        "• /alerts_status - статус уведомлений",
    ]
    .join("\n")
}

async fn consume_rate_limit(runtime: &TelegramRuntime, chat_id: i64) -> bool {
    let now = now_unix();
    let mut limiter = runtime.limiter.lock().await;
    limiter.allow(chat_id, now)
}

fn make_speed_sample(state: &State) -> SpeedSample {
    let (rx, tx) = if let Some(speed) = state.internet_speed.as_ref() {
        let down = (speed.download_mbps.max(0.0) * 1_000_000.0 / 8.0).round() as u64;
        let up = (speed.upload_mbps.max(0.0) * 1_000_000.0 / 8.0).round() as u64;
        (down, up)
    } else {
        network_speed_totals(state)
    };
    SpeedSample {
        ts: now_unix(),
        rx,
        tx,
    }
}

async fn push_speed_sample(runtime: &TelegramRuntime, sample: SpeedSample) {
    const WINDOW_SECS: i64 = 600;
    const MAX_POINTS: usize = 600;
    let mut history = runtime.speed_history.lock().await;
    history.push_back(sample);

    let cutoff = now_unix().saturating_sub(WINDOW_SECS);
    while history.len() > MAX_POINTS {
        history.pop_front();
    }
    while let Some(front) = history.front() {
        if front.ts < cutoff {
            history.pop_front();
        } else {
            break;
        }
    }
}

async fn upsert_dashboard_message(
    bot: &Bot,
    chat_id: ChatId,
    runtime: &TelegramRuntime,
    view: RenderedView,
) -> ResponseResult<()> {
    let existing = {
        let map = runtime.dashboard_messages.lock().await;
        map.get(&chat_id.0).copied()
    };

    if let Some(msg_id) = existing {
        let result = bot
            .edit_message_text(chat_id, MessageId(msg_id), view.text.clone())
            .parse_mode(ParseMode::Html)
            .reply_markup(view.keyboard.clone())
            .await;
        if result.is_ok() {
            return Ok(());
        }
    }

    let sent = bot
        .send_message(chat_id, view.text)
        .parse_mode(ParseMode::Html)
        .reply_markup(view.keyboard)
        .await?;

    let mut map = runtime.dashboard_messages.lock().await;
    map.insert(chat_id.0, sent.id.0);
    Ok(())
}

pub async fn send_alert_events(
    bot: &Bot,
    cfg: &TelegramConfig,
    state: Arc<RwLock<State>>,
    events: &[AlertEvent],
) -> usize {
    if events.is_empty() {
        return 0;
    }
    let mut sent = 0_usize;

    for chat_id in &cfg.allowed_chat_ids {
        let (enabled, checks_enabled) = {
            let guard = state.read().await;
            (
                guard.alerts_enabled_for_chat(*chat_id, cfg.alerts.enabled_by_default),
                guard.check_alerts_enabled_for_chat(*chat_id),
            )
        };
        if !enabled || !checks_enabled {
            continue;
        }

        let lines = events
            .iter()
            .filter(|e| !matches!(e.kind, AlertEventKind::Repeat))
            .map(format_alert_event)
            .collect::<Vec<_>>();
        if lines.is_empty() {
            continue;
        }

        let text = format!("<b>Уведомления по проверкам</b>\n{}", lines.join("\n"));
        if let Err(err) = bot
            .send_message(ChatId(*chat_id), text)
            .parse_mode(ParseMode::Html)
            .reply_markup(main_menu())
            .await
        {
            warn!(chat_id = *chat_id, error = %err, "не удалось отправить уведомления по проверкам");
        } else {
            sent += lines.len();
        }
    }
    sent
}

pub async fn send_text_alerts(
    bot: &Bot,
    cfg: &TelegramConfig,
    state: Arc<RwLock<State>>,
    alerts: &[ResourceAlert],
) -> usize {
    if alerts.is_empty() {
        return 0;
    }
    let mut sent = 0_usize;

    for chat_id in &cfg.allowed_chat_ids {
        let (enabled, filtered_texts) = {
            let guard = state.read().await;
            let enabled = guard.alerts_enabled_for_chat(*chat_id, cfg.alerts.enabled_by_default);
            let filtered = alerts
                .iter()
                .filter(|alert| guard.resource_alert_enabled_for_chat(*chat_id, alert.kind))
                .map(|alert| alert.text.clone())
                .collect::<Vec<_>>();
            (enabled, filtered)
        };
        if !enabled {
            continue;
        }
        if filtered_texts.is_empty() {
            continue;
        }

        let text = format!(
            "<b>Ресурсные уведомления</b>\n{}",
            filtered_texts.join("\n")
        );
        if let Err(err) = bot
            .send_message(ChatId(*chat_id), text)
            .parse_mode(ParseMode::Html)
            .reply_markup(main_menu())
            .await
        {
            warn!(chat_id = *chat_id, error = %err, "не удалось отправить ресурсные уведомления");
        } else {
            sent += filtered_texts.len();
        }
    }
    sent
}

fn format_alert_event(event: &AlertEvent) -> String {
    let check_kind = match event.check_id.kind {
        CheckKind::Http => "HTTP",
        CheckKind::Tcp => "TCP",
    };
    let label = match event.kind {
        AlertEventKind::Down => "НЕДОСТУПЕН",
        AlertEventKind::Repeat => "НЕДОСТУПЕН (повтор)",
        AlertEventKind::Recovered => "ВОССТАНОВЛЕН",
    };

    format!("{check_kind} '{}' - <b>{label}</b>", event.check_id.name)
}

pub fn should_handle_message(is_private: bool, chat_id: i64, allowed: &HashSet<i64>) -> bool {
    is_private && allowed.contains(&chat_id)
}

#[derive(Debug)]
struct RateLimiter {
    limit_per_minute: u32,
    timestamps_by_chat: HashMap<i64, VecDeque<i64>>,
}

#[derive(Debug, Clone)]
struct SpeedSample {
    ts: i64,
    rx: u64,
    tx: u64,
}

impl RateLimiter {
    fn new(limit_per_minute: u32) -> Self {
        Self {
            limit_per_minute,
            timestamps_by_chat: HashMap::new(),
        }
    }

    fn allow(&mut self, chat_id: i64, now_unix: i64) -> bool {
        let queue = self.timestamps_by_chat.entry(chat_id).or_default();
        while let Some(ts) = queue.front().copied() {
            if now_unix - ts >= 60 {
                queue.pop_front();
            } else {
                break;
            }
        }

        if queue.len() >= self.limit_per_minute as usize {
            return false;
        }

        queue.push_back(now_unix);
        true
    }
}

fn format_status(state: &State, cfg: &TelegramConfig) -> String {
    let uptime = human_uptime(state.started_at_unix, now_unix());
    let ram_pct = percent(
        state.memory_used_bytes as f64,
        state.memory_total_bytes as f64,
    );
    let cpu_temp = format_cpu_temp(state);
    let (net_rx, net_tx) = network_speed_totals(state);

    let disks = state
        .disks
        .iter()
        .take(2)
        .map(|d| {
            format!(
                "• {}: {:.1}/{:.1} ГБ ({:.0}%)",
                d.mount,
                bytes_to_gb(d.used_bytes),
                bytes_to_gb(d.total_bytes),
                disk_used_pct(d)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let gpus = state
        .gpus
        .iter()
        .take(2)
        .map(|g| {
            format!(
                "• {} | load {} | temp {} | mem {}",
                g.name,
                g.utilization_percent
                    .map(|v| format!("{v:.0}%"))
                    .unwrap_or_else(|| "н/д".to_string()),
                g.temperature_celsius
                    .map(|v| format!("{v:.1}°C"))
                    .unwrap_or_else(|| "н/д".to_string()),
                match (g.memory_used_bytes, g.memory_total_bytes) {
                    (Some(u), Some(t)) => format!("{:.1}/{:.1} ГБ", bytes_to_gb(u), bytes_to_gb(t)),
                    _ => "н/д".to_string(),
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let mut out = vec![
        format!(
            "{} <b>monitord</b>",
            if state.cpu_usage_percent >= 92.0 || ram_pct >= 92.0 {
                "🔴"
            } else {
                "🟢"
            }
        ),
        format!("⏱ Аптайм: {}", uptime),
        format!("🧠 CPU: {:.1}% | 🌡 {}", state.cpu_usage_percent, cpu_temp),
        format!(
            "💾 RAM: {:.1}/{:.1} ГБ ({:.0}%)",
            bytes_to_gb(state.memory_used_bytes),
            bytes_to_gb(state.memory_total_bytes),
            ram_pct
        ),
        format!(
            "🌐 Сеть: ↓ {} / ↑ {}",
            bytes_per_sec_human(net_rx),
            bytes_per_sec_human(net_tx)
        ),
    ];

    if let Some(s) = state.internet_speed.as_ref() {
        out.push(format!(
            "🚀 Интернет: ↓ {:.1} Mbps / ↑ {:.1} Mbps{}",
            s.download_mbps,
            s.upload_mbps,
            s.latency_ms
                .map(|v| format!(" | ping {:.0} ms", v))
                .unwrap_or_default()
        ));
    }

    if !disks.is_empty() {
        out.push("\n💽 Диски:".to_string());
        out.push(disks);
    }

    if !gpus.is_empty() {
        out.push("\n🎮 GPU:".to_string());
        out.push(gpus);
    }

    out.push(format!(
        "\n🕒 {}",
        format_last_collect_line(state.last_collect_timestamp_seconds)
    ));
    if let Some(base) = cfg.public_base_url.as_ref() {
        out.push(format!(
            "🔗 /metrics: {}/metrics",
            base.trim_end_matches('/')
        ));
    }

    out.join("\n")
}

fn format_system(state: &State) -> String {
    let ram_pct = percent(
        state.memory_used_bytes as f64,
        state.memory_total_bytes as f64,
    );
    format!(
        "🖥 <b>Система</b>\n\nХост: {}\nОС: {} {}\nЯдро: {}\nCPU: {}\nЯдер: {}\nПроцессов: {}\nCPU temp: {}\nRAM: {:.1}/{:.1} ГБ ({:.0}%)\n\n🕒 {}",
        state.host_name.clone().unwrap_or_else(|| "н/д".to_string()),
        state.os_name.clone().unwrap_or_else(|| "н/д".to_string()),
        state.os_version.clone().unwrap_or_default(),
        state.kernel_version.clone().unwrap_or_else(|| "н/д".to_string()),
        state.cpu_brand.clone().unwrap_or_else(|| "н/д".to_string()),
        state.cpu_core_count,
        state.process_count,
        format_cpu_temp(state),
        bytes_to_gb(state.memory_used_bytes),
        bytes_to_gb(state.memory_total_bytes),
        ram_pct,
        format_last_collect_line(state.last_collect_timestamp_seconds),
    )
}

fn format_sensors(state: &State) -> String {
    if state.sensors.is_empty() {
        return "📟 <b>Сенсоры</b>\n\nНет данных.".to_string();
    }
    let mut grouped: HashMap<&str, usize> = HashMap::new();
    for s in &state.sensors {
        *grouped.entry(s.sensor_type.as_str()).or_insert(0) += 1;
    }
    let mut rows = grouped.into_iter().collect::<Vec<_>>();
    rows.sort_by(|a, b| b.1.cmp(&a.1));
    let summary = rows
        .iter()
        .take(10)
        .map(|(t, c)| format!("• {}: {}", sensor_type_ru(t), c))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "📟 <b>Сенсоры</b>\n\nВсего: {}\n\n{}\n\n🕒 {}",
        state.sensors.len(),
        summary,
        format_last_collect_line(state.last_collect_timestamp_seconds),
    )
}

fn format_network(state: &State) -> String {
    let mut ifaces = state.net.clone();
    ifaces.sort_by(|a, b| {
        let a_total = a.rx_bytes_per_sec.saturating_add(a.tx_bytes_per_sec);
        let b_total = b.rx_bytes_per_sec.saturating_add(b.tx_bytes_per_sec);
        b_total.cmp(&a_total)
    });

    let lines = ifaces
        .iter()
        .take(8)
        .map(|n| {
            format!(
                "• {}: ↓ {} / ↑ {}",
                n.iface,
                bytes_per_sec_human(n.rx_bytes_per_sec),
                bytes_per_sec_human(n.tx_bytes_per_sec)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let (rx, tx) = network_speed_totals(state);

    let internet_line = state
        .internet_speed
        .as_ref()
        .map(|s| {
            format!(
                "🚀 Интернет: ↓ {:.1} Mbps / ↑ {:.1} Mbps{}",
                s.download_mbps,
                s.upload_mbps,
                s.latency_ms
                    .map(|v| format!(" | ping {:.0} ms", v))
                    .unwrap_or_default()
            )
        })
        .unwrap_or_else(|| "🚀 Интернет speedtest: н/д".to_string());

    format!(
        "🌐 <b>Сеть</b>\n\nИтого: ↓ {} / ↑ {}\n{}\n\n{}\n\n🕒 {}",
        bytes_per_sec_human(rx),
        bytes_per_sec_human(tx),
        internet_line,
        if lines.is_empty() {
            "н/д".to_string()
        } else {
            lines
        },
        format_last_collect_line(state.last_collect_timestamp_seconds),
    )
}

fn format_speedtest(state: &State, history: &VecDeque<SpeedSample>) -> String {
    let now = now_unix();
    let cutoff = now.saturating_sub(60);
    let mut points: Vec<&SpeedSample> = history.iter().filter(|x| x.ts >= cutoff).collect();
    if points.is_empty() {
        if let Some(last) = history.back() {
            points.push(last);
        }
    }

    let (cur_rx, cur_tx) = points
        .last()
        .map(|p| (p.rx, p.tx))
        .unwrap_or_else(|| network_speed_totals(state));

    let mut avg_rx = 0_f64;
    let mut avg_tx = 0_f64;
    let mut peak_rx = 0_u64;
    let mut peak_tx = 0_u64;
    let mut peak_total = 0_u64;

    if !points.is_empty() {
        for p in &points {
            avg_rx += p.rx as f64;
            avg_tx += p.tx as f64;
            peak_rx = peak_rx.max(p.rx);
            peak_tx = peak_tx.max(p.tx);
            peak_total = peak_total.max(p.rx.saturating_add(p.tx));
        }
        avg_rx /= points.len() as f64;
        avg_tx /= points.len() as f64;
    }

    let measured = state
        .internet_speed
        .as_ref()
        .map(|s| {
            format!(
                "Измерено: ↓ {:.1} Mbps / ↑ {:.1} Mbps{}",
                s.download_mbps,
                s.upload_mbps,
                s.latency_ms
                    .map(|v| format!(" | ping {:.0} ms", v))
                    .unwrap_or_default()
            )
        })
        .unwrap_or_else(|| "Измерено: н/д".to_string());

    format!(
        "🚀 <b>Speedtest</b>\n\n{}\nТекущая: ↓ {} / ↑ {}\nСредняя (1 мин): ↓ {} / ↑ {}\nПик (1 мин): ↓ {} / ↑ {}\nПик суммарно: {}\n\n🕒 {}",
        measured,
        bytes_per_sec_human(cur_rx),
        bytes_per_sec_human(cur_tx),
        bytes_per_sec_human(avg_rx.round() as u64),
        bytes_per_sec_human(avg_tx.round() as u64),
        bytes_per_sec_human(peak_rx),
        bytes_per_sec_human(peak_tx),
        bytes_per_sec_human(peak_total),
        format_last_collect_line(state.last_collect_timestamp_seconds),
    )
}

fn format_disks(state: &State) -> String {
    let mut disks = state.disks.clone();
    disks.sort_by(|a, b| disk_used_pct(b).total_cmp(&disk_used_pct(a)));
    let lines = disks
        .iter()
        .map(|d| {
            format!(
                "• {}: {:.1}/{:.1} ГБ ({:.0}%)",
                d.mount,
                bytes_to_gb(d.used_bytes),
                bytes_to_gb(d.total_bytes),
                disk_used_pct(d)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "💽 <b>Диски</b>\n\n{}\n\n🕒 {}",
        if lines.is_empty() {
            "н/д".to_string()
        } else {
            lines
        },
        format_last_collect_line(state.last_collect_timestamp_seconds),
    )
}

fn format_gpu_details(state: &State) -> String {
    if state.gpus.is_empty() {
        return format!(
            "🎮 <b>GPU</b>\n\nНет данных\n\n🕒 {}",
            format_last_collect_line(state.last_collect_timestamp_seconds)
        );
    }

    let rows = state
        .gpus
        .iter()
        .map(|g| {
            let util = g
                .utilization_percent
                .map(|v| format!("{v:.1}%"))
                .unwrap_or_else(|| "н/д".to_string());
            let temp = g
                .temperature_celsius
                .map(|v| format!("{v:.1}°C"))
                .unwrap_or_else(|| "н/д".to_string());
            let mem = match (g.memory_used_bytes, g.memory_total_bytes) {
                (Some(used), Some(total)) => {
                    format!("{:.1}/{:.1} ГБ", bytes_to_gb(used), bytes_to_gb(total))
                }
                (Some(used), None) => format!("{:.1} ГБ", bytes_to_gb(used)),
                _ => "н/д".to_string(),
            };
            format!(
                "• {}\n  load {} | temp {} | mem {}",
                g.name, util, temp, mem
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "🎮 <b>GPU</b>\n\n{}\n\n🕒 {}",
        rows,
        format_last_collect_line(state.last_collect_timestamp_seconds)
    )
}

fn sensor_type_ru(t: &str) -> &'static str {
    match t.to_ascii_lowercase().as_str() {
        "temperature" => "Температура",
        "load" => "Нагрузка",
        "data" => "Данные",
        "smalldata" => "Память",
        "throughput" => "Скорость",
        "clock" => "Частота",
        "power" => "Мощность",
        "fan" => "Вентилятор",
        "voltage" => "Напряжение",
        "current" => "Ток",
        _ => "Прочее",
    }
}

fn format_cpu_temp(state: &State) -> String {
    cpu_temperature_from_state(state)
        .map(|v| format!("{:.1}°C", v))
        .unwrap_or_else(|| "н/д".to_string())
}

fn cpu_temperature_from_state(state: &State) -> Option<f64> {
    let preferred = [
        "cpu",
        "package",
        "tctl",
        "tdie",
        "coretemp",
        "k10temp",
        "thermal zone",
        "_tz",
    ];
    let mut picked: Option<f64> = None;

    for t in &state.temps {
        let sensor = t.sensor.to_lowercase();
        if preferred.iter().any(|k| sensor.contains(k)) {
            picked = Some(picked.map_or(t.temperature_celsius, |p| p.max(t.temperature_celsius)));
        }
    }

    if picked.is_some() {
        return picked;
    }

    state
        .temps
        .iter()
        .filter(|t| {
            let sensor = t.sensor.to_lowercase();
            !sensor.contains("gpu") && !sensor.contains("nvidia") && !sensor.contains("amdgpu")
        })
        .map(|t| t.temperature_celsius)
        .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
}

fn disk_used_pct(d: &crate::state::DiskStat) -> f64 {
    percent(d.used_bytes as f64, d.total_bytes as f64)
}

fn percent(used: f64, total: f64) -> f64 {
    if total <= 0.0 {
        0.0
    } else {
        (used / total) * 100.0
    }
}

fn bytes_to_gb(bytes: u64) -> f64 {
    (bytes as f64) / 1024.0 / 1024.0 / 1024.0
}

fn network_speed_totals(state: &State) -> (u64, u64) {
    state.net.iter().fold((0_u64, 0_u64), |acc, n| {
        (
            acc.0.saturating_add(n.rx_bytes_per_sec),
            acc.1.saturating_add(n.tx_bytes_per_sec),
        )
    })
}

fn bytes_per_sec_human(v: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;

    let vf = v as f64;
    if vf >= GB {
        format!("{:.2} GB/s", vf / GB)
    } else if vf >= MB {
        format!("{:.2} MB/s", vf / MB)
    } else if vf >= KB {
        format!("{:.2} KB/s", vf / KB)
    } else {
        format!("{} B/s", v)
    }
}

fn format_unix(ts: i64) -> String {
    let st = UNIX_EPOCH + Duration::from_secs(ts.max(0) as u64);
    humantime::format_rfc3339_seconds(st).to_string()
}

fn format_last_collect_line(last_collect_ts: i64) -> String {
    if last_collect_ts <= 0 {
        return "Последнее обновление: н/д".to_string();
    }

    let now = now_unix();
    let age = now.saturating_sub(last_collect_ts).max(0) as u64;
    let relative = if age < 60 {
        format!("{} сек назад", age)
    } else if age < 3600 {
        format!("{} мин назад", age / 60)
    } else {
        format!("{} ч назад", age / 3600)
    };

    format!(
        "Последнее обновление: {} ({})",
        format_unix(last_collect_ts),
        relative
    )
}

fn human_uptime(started_at: i64, now: i64) -> String {
    let diff = now.saturating_sub(started_at).max(0) as u64;
    let days = diff / 86_400;
    let hours = (diff % 86_400) / 3600;
    let mins = (diff % 3600) / 60;

    if days > 0 {
        format!("{}д {}ч {}м", days, hours, mins)
    } else if hours > 0 {
        format!("{}ч {}м", hours, mins)
    } else if mins > 0 {
        format!("{}м", mins)
    } else {
        format!("{}с", diff)
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authorization_ignores_non_private_and_not_allowed() {
        let allowed: HashSet<i64> = [100].into_iter().collect();

        assert!(!should_handle_message(false, 100, &allowed));
        assert!(!should_handle_message(true, 101, &allowed));
        assert!(should_handle_message(true, 100, &allowed));
    }

    #[test]
    fn rate_limiter_enforces_limit() {
        let mut limiter = RateLimiter::new(2);
        assert!(limiter.allow(1, 10));
        assert!(limiter.allow(1, 20));
        assert!(!limiter.allow(1, 30));
        assert!(limiter.allow(1, 71));
    }
}
