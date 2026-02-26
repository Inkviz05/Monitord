use crate::metrics::Metrics;
use crate::state::{
    CheckResults, DiskStat, GpuStat, InternetSpeedStat, NetStat, SensorStat, State as AgentState,
    TempStat,
};
use axum::body::Body;
use axum::extract::State;
use axum::http::{header::CONTENT_TYPE, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{routing::get, Json, Router};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct HttpAppState {
    pub metrics: Arc<Metrics>,
    pub state: Arc<RwLock<AgentState>>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ApiState {
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
}

impl From<&AgentState> for ApiState {
    fn from(value: &AgentState) -> Self {
        Self {
            started_at_unix: value.started_at_unix,
            last_collect_timestamp_seconds: value.last_collect_timestamp_seconds,
            host_name: value.host_name.clone(),
            os_name: value.os_name.clone(),
            os_version: value.os_version.clone(),
            kernel_version: value.kernel_version.clone(),
            cpu_brand: value.cpu_brand.clone(),
            system_uptime_seconds: value.system_uptime_seconds,
            process_count: value.process_count,
            cpu_core_count: value.cpu_core_count,
            cpu_usage_percent: value.cpu_usage_percent,
            memory_used_bytes: value.memory_used_bytes,
            memory_total_bytes: value.memory_total_bytes,
            disks: value.disks.clone(),
            net: value.net.clone(),
            internet_speed: value.internet_speed.clone(),
            temps: value.temps.clone(),
            gpus: value.gpus.clone(),
            sensors: value.sensors.clone(),
            checks: value.checks.clone(),
        }
    }
}

pub fn build_router(metrics: Arc<Metrics>, state: Arc<RwLock<AgentState>>) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/metrics", get(metrics_handler))
        .route("/api/state", get(state_handler))
        .with_state(HttpAppState { metrics, state })
}

async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

async fn metrics_handler(State(state): State<HttpAppState>) -> Response {
    state.metrics.inc_scrape_count();
    match state.metrics.encode_metrics() {
        Ok(encoded) => {
            let mut response = Response::new(Body::from(encoded));
            response.headers_mut().insert(
                CONTENT_TYPE,
                HeaderValue::from_static("text/plain; version=0.0.4"),
            );
            response
        }
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("ошибка кодирования метрик: {err}"),
        )
            .into_response(),
    }
}

async fn state_handler(State(state): State<HttpAppState>) -> impl IntoResponse {
    let guard = state.state.read().await;
    Json(ApiState::from(&*guard))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::Request;
    use tower::ServiceExt;

    #[tokio::test]
    async fn healthz_returns_ok() {
        let metrics = Metrics::new().expect("инициализация метрик");
        let state = Arc::new(RwLock::new(crate::state::State::new(0)));
        let app = build_router(metrics, state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(bytes.as_ref(), b"ok");
    }

    #[tokio::test]
    async fn metrics_contains_uptime() {
        let metrics = Metrics::new().expect("инициализация метрик");
        let state = Arc::new(RwLock::new(crate::state::State::new(0)));
        let app = build_router(metrics.clone(), state);
        let snapshot_state = crate::state::State::new(0);
        metrics.update_from_state(&snapshot_state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(text.contains("agent_uptime_seconds"));
    }

    #[tokio::test]
    async fn api_state_returns_json() {
        let metrics = Metrics::new().expect("инициализация метрик");
        let state = Arc::new(RwLock::new(crate::state::State::new(10)));
        let app = build_router(metrics, state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/state")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(text.contains("\"cpu_usage_percent\""));
    }
}
