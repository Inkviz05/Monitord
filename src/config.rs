use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::net::SocketAddr;
use std::path::Path;
use std::str::FromStr;
use thiserror::Error;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub listen: String,
    pub interval_secs: u64,
    #[serde(default)]
    pub http_checks: Vec<HttpCheckConfig>,
    #[serde(default)]
    pub tcp_checks: Vec<TcpCheckConfig>,
    #[serde(default)]
    pub telegram: TelegramConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HttpCheckConfig {
    pub name: String,
    pub url: String,
    pub timeout_ms: u64,
    #[serde(default = "default_expected_status")]
    pub expected_status: u16,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TcpCheckConfig {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TelegramConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_bot_token_env")]
    pub bot_token_env: String,
    #[serde(default)]
    pub bot_token: Option<String>,
    #[serde(default)]
    pub allowed_chat_ids: Vec<i64>,
    #[serde(default = "default_rate_limit_per_minute")]
    pub rate_limit_per_minute: u32,
    pub public_base_url: Option<String>,
    #[serde(default)]
    pub alerts: AlertsConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AlertsConfig {
    #[serde(default)]
    pub enabled_by_default: bool,
    #[serde(default = "default_repeat_interval_secs")]
    pub repeat_interval_secs: u64,
    #[serde(default = "default_fail_threshold")]
    pub fail_threshold: u32,
    #[serde(default = "default_recovery_notify")]
    pub recovery_notify: bool,
    #[serde(default = "default_resource_alerts_enabled")]
    pub resource_alerts_enabled: bool,
    #[serde(default = "default_gpu_load_threshold_percent")]
    pub gpu_load_threshold_percent: f64,
    #[serde(default = "default_gpu_temp_threshold_celsius")]
    pub gpu_temp_threshold_celsius: f64,
    #[serde(default = "default_cpu_temp_threshold_celsius")]
    pub cpu_temp_threshold_celsius: f64,
    #[serde(default = "default_cpu_load_threshold_percent")]
    pub cpu_load_threshold_percent: f64,
    #[serde(default = "default_ram_usage_threshold_percent")]
    pub ram_usage_threshold_percent: f64,
    #[serde(default = "default_disk_usage_threshold_percent")]
    pub disk_usage_threshold_percent: f64,
    #[serde(default = "default_resource_alert_cooldown_secs")]
    pub resource_alert_cooldown_secs: u64,
}

impl Default for TelegramConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bot_token_env: default_bot_token_env(),
            bot_token: None,
            allowed_chat_ids: Vec::new(),
            rate_limit_per_minute: default_rate_limit_per_minute(),
            public_base_url: None,
            alerts: AlertsConfig::default(),
        }
    }
}

impl Default for AlertsConfig {
    fn default() -> Self {
        Self {
            enabled_by_default: true,
            repeat_interval_secs: default_repeat_interval_secs(),
            fail_threshold: default_fail_threshold(),
            recovery_notify: default_recovery_notify(),
            resource_alerts_enabled: default_resource_alerts_enabled(),
            gpu_load_threshold_percent: default_gpu_load_threshold_percent(),
            gpu_temp_threshold_celsius: default_gpu_temp_threshold_celsius(),
            cpu_temp_threshold_celsius: default_cpu_temp_threshold_celsius(),
            cpu_load_threshold_percent: default_cpu_load_threshold_percent(),
            ram_usage_threshold_percent: default_ram_usage_threshold_percent(),
            disk_usage_threshold_percent: default_disk_usage_threshold_percent(),
            resource_alert_cooldown_secs: default_resource_alert_cooldown_secs(),
        }
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("не удалось прочитать файл конфигурации {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("не удалось разобрать YAML в {path}: {source}")]
    Parse {
        path: String,
        source: serde_yaml::Error,
    },
    #[error("ошибка валидации конфигурации: {0}")]
    Validation(String),
}

impl Config {
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path_ref = path.as_ref();
        let path_display = path_ref.display().to_string();
        let text = fs::read_to_string(path_ref).map_err(|source| ConfigError::Read {
            path: path_display.clone(),
            source,
        })?;

        let cfg: Config = serde_yaml::from_str(&text).map_err(|source| ConfigError::Parse {
            path: path_display,
            source,
        })?;

        cfg.validate()?;
        Ok(cfg)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.listen.trim().is_empty() {
            return Err(ConfigError::Validation(
                "поле listen обязательно".to_string(),
            ));
        }
        if SocketAddr::from_str(&self.listen).is_err() {
            return Err(ConfigError::Validation(
                "поле listen должно быть корректным адресом host:port".to_string(),
            ));
        }
        if self.interval_secs < 1 {
            return Err(ConfigError::Validation(
                "interval_secs должно быть >= 1".to_string(),
            ));
        }

        validate_http_checks(&self.http_checks)?;
        validate_tcp_checks(&self.tcp_checks)?;
        validate_telegram(&self.telegram)?;

        Ok(())
    }

    pub fn example_yaml() -> &'static str {
        include_str!("../config.yaml.example")
    }
}

fn validate_http_checks(checks: &[HttpCheckConfig]) -> Result<(), ConfigError> {
    let mut names = HashSet::new();
    for check in checks {
        if check.name.trim().is_empty() {
            return Err(ConfigError::Validation(
                "http_checks[*].name не должен быть пустым".to_string(),
            ));
        }
        if !names.insert(check.name.clone()) {
            return Err(ConfigError::Validation(format!(
                "имя HTTP-проверки '{}' должно быть уникальным",
                check.name
            )));
        }
        if check.timeout_ms == 0 {
            return Err(ConfigError::Validation(format!(
                "http_checks '{}' timeout_ms должен быть > 0",
                check.name
            )));
        }
        if check.url.trim().is_empty() {
            return Err(ConfigError::Validation(format!(
                "http_checks '{}' url не должен быть пустым",
                check.name
            )));
        }
    }
    Ok(())
}

fn validate_tcp_checks(checks: &[TcpCheckConfig]) -> Result<(), ConfigError> {
    let mut names = HashSet::new();
    for check in checks {
        if check.name.trim().is_empty() {
            return Err(ConfigError::Validation(
                "tcp_checks[*].name не должен быть пустым".to_string(),
            ));
        }
        if !names.insert(check.name.clone()) {
            return Err(ConfigError::Validation(format!(
                "имя TCP-проверки '{}' должно быть уникальным",
                check.name
            )));
        }
        if check.host.trim().is_empty() {
            return Err(ConfigError::Validation(format!(
                "tcp_checks '{}' host не должен быть пустым",
                check.name
            )));
        }
        if check.port == 0 {
            return Err(ConfigError::Validation(format!(
                "tcp_checks '{}' port должен быть в диапазоне 1..65535",
                check.name
            )));
        }
        if check.timeout_ms == 0 {
            return Err(ConfigError::Validation(format!(
                "tcp_checks '{}' timeout_ms должен быть > 0",
                check.name
            )));
        }
    }
    Ok(())
}

fn validate_telegram(cfg: &TelegramConfig) -> Result<(), ConfigError> {
    if cfg.rate_limit_per_minute < 1 {
        return Err(ConfigError::Validation(
            "telegram.rate_limit_per_minute должно быть >= 1".to_string(),
        ));
    }
    if cfg.alerts.fail_threshold < 1 {
        return Err(ConfigError::Validation(
            "telegram.alerts.fail_threshold должно быть >= 1".to_string(),
        ));
    }
    if cfg.alerts.repeat_interval_secs < 60 {
        return Err(ConfigError::Validation(
            "telegram.alerts.repeat_interval_secs должно быть >= 60".to_string(),
        ));
    }
    if !(0.0..=100.0).contains(&cfg.alerts.gpu_load_threshold_percent) {
        return Err(ConfigError::Validation(
            "telegram.alerts.gpu_load_threshold_percent должно быть в диапазоне 0..100".to_string(),
        ));
    }
    if cfg.alerts.gpu_temp_threshold_celsius <= 0.0 {
        return Err(ConfigError::Validation(
            "telegram.alerts.gpu_temp_threshold_celsius должно быть > 0".to_string(),
        ));
    }
    if cfg.alerts.cpu_temp_threshold_celsius <= 0.0 {
        return Err(ConfigError::Validation(
            "telegram.alerts.cpu_temp_threshold_celsius должно быть > 0".to_string(),
        ));
    }
    if !(0.0..=100.0).contains(&cfg.alerts.cpu_load_threshold_percent) {
        return Err(ConfigError::Validation(
            "telegram.alerts.cpu_load_threshold_percent должно быть в диапазоне 0..100".to_string(),
        ));
    }
    if !(0.0..=100.0).contains(&cfg.alerts.ram_usage_threshold_percent) {
        return Err(ConfigError::Validation(
            "telegram.alerts.ram_usage_threshold_percent должно быть в диапазоне 0..100"
                .to_string(),
        ));
    }
    if !(0.0..=100.0).contains(&cfg.alerts.disk_usage_threshold_percent) {
        return Err(ConfigError::Validation(
            "telegram.alerts.disk_usage_threshold_percent должно быть в диапазоне 0..100"
                .to_string(),
        ));
    }
    if cfg.alerts.resource_alert_cooldown_secs < 1 {
        return Err(ConfigError::Validation(
            "telegram.alerts.resource_alert_cooldown_secs должно быть >= 1".to_string(),
        ));
    }

    Ok(())
}

const fn default_expected_status() -> u16 {
    200
}

fn default_bot_token_env() -> String {
    "TELEGRAM_BOT_TOKEN".to_string()
}

const fn default_rate_limit_per_minute() -> u32 {
    30
}

const fn default_repeat_interval_secs() -> u64 {
    1800
}

const fn default_fail_threshold() -> u32 {
    3
}

const fn default_recovery_notify() -> bool {
    true
}

const fn default_resource_alerts_enabled() -> bool {
    true
}

const fn default_gpu_load_threshold_percent() -> f64 {
    92.0
}

const fn default_gpu_temp_threshold_celsius() -> f64 {
    75.0
}

const fn default_cpu_temp_threshold_celsius() -> f64 {
    85.0
}

const fn default_cpu_load_threshold_percent() -> f64 {
    92.0
}

const fn default_ram_usage_threshold_percent() -> f64 {
    92.0
}

const fn default_disk_usage_threshold_percent() -> f64 {
    95.0
}

const fn default_resource_alert_cooldown_secs() -> u64 {
    10
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_config() -> Config {
        Config {
            listen: "127.0.0.1:9108".to_string(),
            interval_secs: 5,
            http_checks: vec![],
            tcp_checks: vec![],
            telegram: TelegramConfig {
                enabled: false,
                bot_token_env: "TEST_TOKEN_ENV".to_string(),
                bot_token: None,
                allowed_chat_ids: vec![],
                rate_limit_per_minute: 30,
                public_base_url: None,
                alerts: AlertsConfig::default(),
            },
        }
    }

    #[test]
    fn telegram_enabled_allows_missing_env() {
        let mut cfg = valid_config();
        cfg.telegram.enabled = true;
        cfg.telegram.allowed_chat_ids = vec![1];
        cfg.telegram.bot_token_env = "MISSING_ENV_12345".to_string();
        std::env::remove_var("MISSING_ENV_12345");

        cfg.validate()
            .expect("валидация должна проходить, токен проверяется на этапе запуска");
    }

    #[test]
    fn telegram_enabled_allows_empty_allowed_chat_ids() {
        let mut cfg = valid_config();
        cfg.telegram.enabled = true;
        cfg.telegram.allowed_chat_ids = vec![];
        cfg.validate()
            .expect("валидация должна проходить, chat id проверяется на этапе запуска");
    }
}
