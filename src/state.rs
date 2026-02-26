use crate::config::AlertsConfig;
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct State {
    pub started_at_unix: i64,
    pub last_collect_timestamp_seconds: i64,
    pub host_name: Option<String>,
    pub os_name: Option<String>,
    pub os_version: Option<String>,
    pub kernel_version: Option<String>,
    pub cpu_brand: Option<String>,
    pub system_uptime_seconds: u64,
    pub process_count: u64,
    pub cpu_core_count: u32,
    pub cpu_usage_percent: f64,
    pub memory_used_bytes: u64,
    pub memory_total_bytes: u64,
    pub disks: Vec<DiskStat>,
    pub net: Vec<NetStat>,
    pub internet_speed: Option<InternetSpeedStat>,
    pub temps: Vec<TempStat>,
    pub gpus: Vec<GpuStat>,
    pub sensors: Vec<SensorStat>,
    pub checks: CheckResults,
    pub alert_tracking: HashMap<CheckId, AlertTrackState>,
    pub chat_alert_prefs: HashMap<i64, bool>,
    pub chat_check_alert_prefs: HashMap<i64, bool>,
    pub chat_resource_alert_prefs: HashMap<i64, ResourceAlertPrefs>,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct CheckResults {
    pub http: Vec<HttpCheckResult>,
    pub tcp: Vec<TcpCheckResult>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DiskStat {
    pub mount: String,
    pub used_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct NetStat {
    pub iface: String,
    pub rx_bytes_total: u64,
    pub tx_bytes_total: u64,
    pub rx_bytes_per_sec: u64,
    pub tx_bytes_per_sec: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TempStat {
    pub sensor: String,
    pub temperature_celsius: f64,
    pub critical_temperature_celsius: Option<f64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct InternetSpeedStat {
    pub download_mbps: f64,
    pub upload_mbps: f64,
    pub latency_ms: Option<f64>,
    pub measured_at_unix: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct GpuStat {
    pub id: String,
    pub name: String,
    pub utilization_percent: Option<f64>,
    pub memory_used_bytes: Option<u64>,
    pub memory_total_bytes: Option<u64>,
    pub temperature_celsius: Option<f64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SensorStat {
    pub sensor_type: String,
    pub name: String,
    pub identifier: String,
    pub parent: String,
    pub value: f64,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct HttpCheckResult {
    pub name: String,
    pub up: bool,
    pub latency_ms: u64,
    pub status_code: u16,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TcpCheckResult {
    pub name: String,
    pub up: bool,
    pub latency_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CheckKind {
    Http,
    Tcp,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CheckId {
    pub kind: CheckKind,
    pub name: String,
}

#[derive(Debug, Clone, Default)]
pub struct AlertTrackState {
    pub consecutive_failures: u32,
    pub is_down: bool,
    pub last_alert_sent_at: Option<i64>,
    pub last_state_change_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub enum AlertEventKind {
    Down,
    Repeat,
    Recovered,
}

#[derive(Debug, Clone)]
pub struct AlertEvent {
    pub check_id: CheckId,
    pub kind: AlertEventKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceAlertKind {
    CpuTemp,
    GpuTemp,
    CpuLoad,
    GpuLoad,
    RamUsage,
    DiskUsage,
}

#[derive(Debug, Clone)]
pub struct ResourceAlert {
    pub kind: ResourceAlertKind,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct ResourceAlertPrefs {
    pub cpu_temp: bool,
    pub gpu_temp: bool,
    pub cpu_load: bool,
    pub gpu_load: bool,
    pub ram_usage: bool,
    pub disk_usage: bool,
}

impl Default for ResourceAlertPrefs {
    fn default() -> Self {
        Self {
            cpu_temp: true,
            gpu_temp: true,
            cpu_load: true,
            gpu_load: true,
            ram_usage: true,
            disk_usage: true,
        }
    }
}

impl State {
    pub fn new(now_unix: i64) -> Self {
        Self {
            started_at_unix: now_unix,
            ..Self::default()
        }
    }

    pub fn update_collected(
        &mut self,
        now_unix: i64,
        host_name: Option<String>,
        os_name: Option<String>,
        os_version: Option<String>,
        kernel_version: Option<String>,
        cpu_brand: Option<String>,
        uptime_seconds: u64,
        process_count: u64,
        cpu_core_count: u32,
        cpu_usage_percent: f64,
        memory_used_bytes: u64,
        memory_total_bytes: u64,
        disks: Vec<DiskStat>,
        mut net: Vec<NetStat>,
        internet_speed: Option<InternetSpeedStat>,
        temps: Vec<TempStat>,
        gpus: Vec<GpuStat>,
        sensors: Vec<SensorStat>,
        checks: CheckResults,
    ) {
        let prev_ts = self.last_collect_timestamp_seconds;
        let dt = now_unix.saturating_sub(prev_ts).max(1) as u64;
        let prev_net: HashMap<String, (u64, u64)> = self
            .net
            .iter()
            .map(|n| (n.iface.clone(), (n.rx_bytes_total, n.tx_bytes_total)))
            .collect();

        for iface in &mut net {
            if let Some((prev_rx, prev_tx)) = prev_net.get(&iface.iface) {
                iface.rx_bytes_per_sec = iface.rx_bytes_total.saturating_sub(*prev_rx) / dt;
                iface.tx_bytes_per_sec = iface.tx_bytes_total.saturating_sub(*prev_tx) / dt;
            } else {
                iface.rx_bytes_per_sec = 0;
                iface.tx_bytes_per_sec = 0;
            }
        }

        self.last_collect_timestamp_seconds = now_unix;
        self.host_name = host_name;
        self.os_name = os_name;
        self.os_version = os_version;
        self.kernel_version = kernel_version;
        self.cpu_brand = cpu_brand;
        self.system_uptime_seconds = uptime_seconds;
        self.process_count = process_count;
        self.cpu_core_count = cpu_core_count;
        self.cpu_usage_percent = cpu_usage_percent;
        self.memory_used_bytes = memory_used_bytes;
        self.memory_total_bytes = memory_total_bytes;
        self.disks = disks;
        self.net = net;
        self.internet_speed = internet_speed;
        self.temps = temps;
        self.gpus = gpus;
        self.sensors = sensors;
        self.checks = checks;
    }

    pub fn alerts_enabled_for_chat(&self, chat_id: i64, default_enabled: bool) -> bool {
        self.chat_alert_prefs
            .get(&chat_id)
            .copied()
            .unwrap_or(default_enabled)
    }

    pub fn set_alerts_enabled_for_chat(&mut self, chat_id: i64, enabled: bool) {
        self.chat_alert_prefs.insert(chat_id, enabled);
    }

    pub fn check_alerts_enabled_for_chat(&self, chat_id: i64) -> bool {
        self.chat_check_alert_prefs
            .get(&chat_id)
            .copied()
            .unwrap_or(true)
    }

    pub fn set_check_alerts_enabled_for_chat(&mut self, chat_id: i64, enabled: bool) {
        self.chat_check_alert_prefs.insert(chat_id, enabled);
    }

    pub fn resource_alert_enabled_for_chat(&self, chat_id: i64, kind: ResourceAlertKind) -> bool {
        let prefs = self
            .chat_resource_alert_prefs
            .get(&chat_id)
            .cloned()
            .unwrap_or_default();
        match kind {
            ResourceAlertKind::CpuTemp => prefs.cpu_temp,
            ResourceAlertKind::GpuTemp => prefs.gpu_temp,
            ResourceAlertKind::CpuLoad => prefs.cpu_load,
            ResourceAlertKind::GpuLoad => prefs.gpu_load,
            ResourceAlertKind::RamUsage => prefs.ram_usage,
            ResourceAlertKind::DiskUsage => prefs.disk_usage,
        }
    }

    pub fn set_resource_alert_enabled_for_chat(
        &mut self,
        chat_id: i64,
        kind: ResourceAlertKind,
        enabled: bool,
    ) {
        let prefs = self.chat_resource_alert_prefs.entry(chat_id).or_default();
        match kind {
            ResourceAlertKind::CpuTemp => prefs.cpu_temp = enabled,
            ResourceAlertKind::GpuTemp => prefs.gpu_temp = enabled,
            ResourceAlertKind::CpuLoad => prefs.cpu_load = enabled,
            ResourceAlertKind::GpuLoad => prefs.gpu_load = enabled,
            ResourceAlertKind::RamUsage => prefs.ram_usage = enabled,
            ResourceAlertKind::DiskUsage => prefs.disk_usage = enabled,
        }
    }

    pub fn apply_alert_rules(&mut self, cfg: &AlertsConfig, now_unix: i64) -> Vec<AlertEvent> {
        let mut events = Vec::new();

        for check in &self.checks.http {
            let check_id = CheckId {
                kind: CheckKind::Http,
                name: check.name.clone(),
            };
            update_alert_state(
                &mut self.alert_tracking,
                check_id,
                check.up,
                cfg,
                now_unix,
                &mut events,
            );
        }

        for check in &self.checks.tcp {
            let check_id = CheckId {
                kind: CheckKind::Tcp,
                name: check.name.clone(),
            };
            update_alert_state(
                &mut self.alert_tracking,
                check_id,
                check.up,
                cfg,
                now_unix,
                &mut events,
            );
        }

        events
    }
}

fn update_alert_state(
    tracking: &mut HashMap<CheckId, AlertTrackState>,
    check_id: CheckId,
    is_up: bool,
    cfg: &AlertsConfig,
    now_unix: i64,
    events: &mut Vec<AlertEvent>,
) {
    let entry = tracking.entry(check_id.clone()).or_default();

    if is_up {
        let was_down = entry.is_down;
        entry.consecutive_failures = 0;
        entry.is_down = false;
        if was_down {
            entry.last_state_change_at = Some(now_unix);
            if cfg.recovery_notify {
                events.push(AlertEvent {
                    check_id,
                    kind: AlertEventKind::Recovered,
                });
            }
        }
        return;
    }

    entry.consecutive_failures = entry.consecutive_failures.saturating_add(1);

    if !entry.is_down && entry.consecutive_failures >= cfg.fail_threshold {
        entry.is_down = true;
        entry.last_state_change_at = Some(now_unix);
        entry.last_alert_sent_at = Some(now_unix);
        events.push(AlertEvent {
            check_id,
            kind: AlertEventKind::Down,
        });
        return;
    }

    if entry.is_down {
        match entry.last_alert_sent_at {
            Some(last_sent) if (now_unix - last_sent) >= cfg.repeat_interval_secs as i64 => {
                entry.last_alert_sent_at = Some(now_unix);
                events.push(AlertEvent {
                    check_id,
                    kind: AlertEventKind::Repeat,
                });
            }
            None => {
                entry.last_alert_sent_at = Some(now_unix);
                events.push(AlertEvent {
                    check_id,
                    kind: AlertEventKind::Repeat,
                });
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alerts_cfg() -> AlertsConfig {
        AlertsConfig {
            enabled_by_default: false,
            repeat_interval_secs: 1800,
            fail_threshold: 3,
            recovery_notify: true,
            ..AlertsConfig::default()
        }
    }

    #[test]
    fn alerts_fail_threshold_and_repeat_and_recovery() {
        let mut state = State::new(0);
        let cfg = alerts_cfg();

        for i in 1..=2 {
            state.checks.http = vec![HttpCheckResult {
                name: "my-api".to_string(),
                up: false,
                latency_ms: 100,
                status_code: 500,
            }];
            let events = state.apply_alert_rules(&cfg, i);
            assert!(events.is_empty(), "unexpected event at fail {}", i);
        }

        state.checks.http = vec![HttpCheckResult {
            name: "my-api".to_string(),
            up: false,
            latency_ms: 100,
            status_code: 500,
        }];
        let events = state.apply_alert_rules(&cfg, 3);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].kind, AlertEventKind::Down));

        state.checks.http = vec![HttpCheckResult {
            name: "my-api".to_string(),
            up: false,
            latency_ms: 100,
            status_code: 500,
        }];
        let events = state.apply_alert_rules(&cfg, 4);
        assert!(events.is_empty());

        state.checks.http = vec![HttpCheckResult {
            name: "my-api".to_string(),
            up: false,
            latency_ms: 100,
            status_code: 500,
        }];
        let events = state.apply_alert_rules(&cfg, 3 + 1800);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].kind, AlertEventKind::Repeat));

        state.checks.http = vec![HttpCheckResult {
            name: "my-api".to_string(),
            up: true,
            latency_ms: 100,
            status_code: 200,
        }];
        let events = state.apply_alert_rules(&cfg, 20000);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].kind, AlertEventKind::Recovered));
    }
}
