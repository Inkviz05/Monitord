use crate::config::{Config, HttpCheckConfig, TcpCheckConfig};
use crate::state::{CheckResults, HttpCheckResult, TcpCheckResult};
use reqwest::Client;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::time;
use tracing::warn;

pub async fn collect_checks(client: &Client, cfg: &Config) -> (CheckResults, u64) {
    let mut http_results = Vec::with_capacity(cfg.http_checks.len());
    let mut errors = 0_u64;
    for check in &cfg.http_checks {
        let (result, had_error) = run_http_check(client, check).await;
        if had_error {
            errors += 1;
        }
        http_results.push(result);
    }

    let mut tcp_results = Vec::with_capacity(cfg.tcp_checks.len());
    for check in &cfg.tcp_checks {
        let (result, had_error) = run_tcp_check(check).await;
        if had_error {
            errors += 1;
        }
        tcp_results.push(result);
    }

    (
        CheckResults {
            http: http_results,
            tcp: tcp_results,
        },
        errors,
    )
}

async fn run_http_check(client: &Client, cfg: &HttpCheckConfig) -> (HttpCheckResult, bool) {
    let start = Instant::now();
    let req = client
        .get(&cfg.url)
        .timeout(Duration::from_millis(cfg.timeout_ms));

    let (up, status_code, had_error) = match req.send().await {
        Ok(resp) => {
            let code = resp.status().as_u16();
            (code == cfg.expected_status, code, false)
        }
        Err(err) => {
            warn!(check = %cfg.name, error = %err, "http check failed");
            (false, 0, true)
        }
    };

    (
        HttpCheckResult {
            name: cfg.name.clone(),
            up,
            latency_ms: start.elapsed().as_millis() as u64,
            status_code,
        },
        had_error,
    )
}

async fn run_tcp_check(cfg: &TcpCheckConfig) -> (TcpCheckResult, bool) {
    let start = Instant::now();
    let addr = format!("{}:{}", cfg.host, cfg.port);

    let (up, had_error) = match time::timeout(
        Duration::from_millis(cfg.timeout_ms),
        TcpStream::connect(&addr),
    )
    .await
    {
        Ok(Ok(_stream)) => (true, false),
        Ok(Err(err)) => {
            warn!(check = %cfg.name, address = %addr, error = %err, "tcp check failed");
            (false, true)
        }
        Err(_elapsed) => {
            warn!(check = %cfg.name, address = %addr, "tcp check timeout");
            (false, true)
        }
    };

    (
        TcpCheckResult {
            name: cfg.name.clone(),
            up,
            latency_ms: start.elapsed().as_millis() as u64,
        },
        had_error,
    )
}
