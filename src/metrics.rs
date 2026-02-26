use crate::state::State;
use prometheus::core::Collector;
use prometheus::{opts, Counter, CounterVec, Encoder, Gauge, GaugeVec, Registry, TextEncoder};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
pub struct Metrics {
    registry: Registry,
    pub agent_cpu_usage_percent: Gauge,
    pub agent_memory_used_bytes: Gauge,
    pub agent_memory_total_bytes: Gauge,
    pub agent_ram_used_bytes: Gauge,
    pub agent_ram_total_bytes: Gauge,
    pub agent_ram_usage_percent: Gauge,
    pub agent_disk_used_bytes: GaugeVec,
    pub agent_disk_total_bytes: GaugeVec,
    pub agent_disk_usage_percent: GaugeVec,
    pub agent_disk_count: Gauge,
    pub agent_temperature_celsius: GaugeVec,
    pub agent_temperature_critical_celsius: GaugeVec,
    pub agent_temperature_sensor_count: Gauge,
    pub agent_net_rx_bytes_total: GaugeVec,
    pub agent_net_tx_bytes_total: GaugeVec,
    pub agent_net_rx_bytes_per_sec: GaugeVec,
    pub agent_net_tx_bytes_per_sec: GaugeVec,
    pub agent_net_iface_count: Gauge,
    pub agent_net_rx_bytes_per_sec_total: Gauge,
    pub agent_net_tx_bytes_per_sec_total: Gauge,
    pub agent_gpu_utilization_percent: GaugeVec,
    pub agent_gpu_memory_used_bytes: GaugeVec,
    pub agent_gpu_memory_total_bytes: GaugeVec,
    pub agent_gpu_memory_usage_percent: GaugeVec,
    pub agent_gpu_temperature_celsius: GaugeVec,
    pub agent_gpu_count: Gauge,
    pub agent_sensor_value: GaugeVec,
    pub agent_sensor_min: GaugeVec,
    pub agent_sensor_max: GaugeVec,
    pub agent_sensor_count: Gauge,
    pub agent_sensor_type_count: GaugeVec,
    pub agent_sensor_type_avg: GaugeVec,
    pub agent_sensor_type_min: GaugeVec,
    pub agent_sensor_type_max: GaugeVec,
    pub agent_sensor_parent_count: GaugeVec,
    pub agent_sensor_parent_avg: GaugeVec,
    pub agent_sensor_parent_max: GaugeVec,
    pub agent_http_check_up: GaugeVec,
    pub agent_http_check_latency_ms: GaugeVec,
    pub agent_http_check_status_code: GaugeVec,
    pub agent_tcp_check_up: GaugeVec,
    pub agent_tcp_check_latency_ms: GaugeVec,
    pub agent_http_checks_total: Gauge,
    pub agent_http_checks_up: Gauge,
    pub agent_http_checks_down: Gauge,
    pub agent_tcp_checks_total: Gauge,
    pub agent_tcp_checks_up: Gauge,
    pub agent_tcp_checks_down: Gauge,
    pub agent_checks_total: Gauge,
    pub agent_checks_up: Gauge,
    pub agent_checks_down: Gauge,
    pub agent_checks_down_ratio_percent: Gauge,
    pub agent_uptime_seconds: Gauge,
    pub agent_scrape_count_total: Counter,
    pub agent_collect_errors_total: CounterVec,
    pub agent_alerts_sent_total: CounterVec,
    pub agent_last_collect_timestamp_seconds: Gauge,
}

impl Metrics {
    pub fn new() -> Result<Arc<Self>, prometheus::Error> {
        let registry = Registry::new();

        let agent_cpu_usage_percent = Gauge::with_opts(opts!(
            "agent_cpu_usage_percent",
            "Average CPU usage across cores in percent (0..100)"
        ))?;
        let agent_memory_used_bytes =
            Gauge::with_opts(opts!("agent_memory_used_bytes", "Used memory in bytes"))?;
        let agent_memory_total_bytes =
            Gauge::with_opts(opts!("agent_memory_total_bytes", "Total memory in bytes"))?;
        let agent_ram_used_bytes =
            Gauge::with_opts(opts!("agent_ram_used_bytes", "Used RAM in bytes"))?;
        let agent_ram_total_bytes =
            Gauge::with_opts(opts!("agent_ram_total_bytes", "Total RAM in bytes"))?;
        let agent_ram_usage_percent =
            Gauge::with_opts(opts!("agent_ram_usage_percent", "RAM usage in percent"))?;
        let agent_disk_used_bytes = GaugeVec::new(
            opts!("agent_disk_used_bytes", "Disk used bytes by mount"),
            &["mount"],
        )?;
        let agent_disk_total_bytes = GaugeVec::new(
            opts!("agent_disk_total_bytes", "Disk total bytes by mount"),
            &["mount"],
        )?;
        let agent_disk_usage_percent = GaugeVec::new(
            opts!("agent_disk_usage_percent", "Disk usage in percent by mount"),
            &["mount"],
        )?;
        let agent_disk_count =
            Gauge::with_opts(opts!("agent_disk_count", "Number of mounted disks"))?;
        let agent_temperature_celsius = GaugeVec::new(
            opts!(
                "agent_temperature_celsius",
                "Temperature by sensor in Celsius"
            ),
            &["sensor"],
        )?;
        let agent_temperature_critical_celsius = GaugeVec::new(
            opts!(
                "agent_temperature_critical_celsius",
                "Critical temperature threshold by sensor in Celsius"
            ),
            &["sensor"],
        )?;
        let agent_temperature_sensor_count = Gauge::with_opts(opts!(
            "agent_temperature_sensor_count",
            "Number of detected temperature sensors"
        ))?;
        let agent_net_rx_bytes_total = GaugeVec::new(
            opts!(
                "agent_net_rx_bytes_total",
                "Current total received bytes per interface"
            ),
            &["iface"],
        )?;
        let agent_net_tx_bytes_total = GaugeVec::new(
            opts!(
                "agent_net_tx_bytes_total",
                "Current total transmitted bytes per interface"
            ),
            &["iface"],
        )?;
        let agent_net_rx_bytes_per_sec = GaugeVec::new(
            opts!(
                "agent_net_rx_bytes_per_sec",
                "Current receive speed in bytes per second by interface"
            ),
            &["iface"],
        )?;
        let agent_net_tx_bytes_per_sec = GaugeVec::new(
            opts!(
                "agent_net_tx_bytes_per_sec",
                "Current transmit speed in bytes per second by interface"
            ),
            &["iface"],
        )?;
        let agent_net_iface_count = Gauge::with_opts(opts!(
            "agent_net_iface_count",
            "Number of network interfaces"
        ))?;
        let agent_net_rx_bytes_per_sec_total = Gauge::with_opts(opts!(
            "agent_net_rx_bytes_per_sec_total",
            "Total receive speed in bytes per second across all interfaces"
        ))?;
        let agent_net_tx_bytes_per_sec_total = Gauge::with_opts(opts!(
            "agent_net_tx_bytes_per_sec_total",
            "Total transmit speed in bytes per second across all interfaces"
        ))?;
        let agent_gpu_utilization_percent = GaugeVec::new(
            opts!(
                "agent_gpu_utilization_percent",
                "GPU utilization in percent (if available)"
            ),
            &["id", "name"],
        )?;
        let agent_gpu_memory_used_bytes = GaugeVec::new(
            opts!(
                "agent_gpu_memory_used_bytes",
                "GPU memory used in bytes (if available)"
            ),
            &["id", "name"],
        )?;
        let agent_gpu_memory_total_bytes = GaugeVec::new(
            opts!(
                "agent_gpu_memory_total_bytes",
                "GPU memory total in bytes (if available)"
            ),
            &["id", "name"],
        )?;
        let agent_gpu_memory_usage_percent = GaugeVec::new(
            opts!(
                "agent_gpu_memory_usage_percent",
                "GPU memory usage in percent (if used and total are available)"
            ),
            &["id", "name"],
        )?;
        let agent_gpu_temperature_celsius = GaugeVec::new(
            opts!(
                "agent_gpu_temperature_celsius",
                "GPU temperature in Celsius (if available)"
            ),
            &["id", "name"],
        )?;
        let agent_gpu_count =
            Gauge::with_opts(opts!("agent_gpu_count", "Number of detected GPUs"))?;
        let agent_sensor_value = GaugeVec::new(
            opts!(
                "agent_sensor_value",
                "Raw sensor value exported from collectors/LibreHardwareMonitor"
            ),
            &["sensor_type", "name", "identifier", "parent"],
        )?;
        let agent_sensor_min = GaugeVec::new(
            opts!(
                "agent_sensor_min",
                "Sensor min value exported from collectors/LibreHardwareMonitor"
            ),
            &["sensor_type", "name", "identifier", "parent"],
        )?;
        let agent_sensor_max = GaugeVec::new(
            opts!(
                "agent_sensor_max",
                "Sensor max value exported from collectors/LibreHardwareMonitor"
            ),
            &["sensor_type", "name", "identifier", "parent"],
        )?;
        let agent_sensor_count = Gauge::with_opts(opts!(
            "agent_sensor_count",
            "Total number of collected sensors"
        ))?;
        let agent_sensor_type_count = GaugeVec::new(
            opts!(
                "agent_sensor_type_count",
                "Number of collected sensors grouped by sensor_type"
            ),
            &["sensor_type"],
        )?;
        let agent_sensor_type_avg = GaugeVec::new(
            opts!(
                "agent_sensor_type_avg",
                "Average sensor value grouped by sensor_type"
            ),
            &["sensor_type"],
        )?;
        let agent_sensor_type_min = GaugeVec::new(
            opts!(
                "agent_sensor_type_min",
                "Minimum sensor value grouped by sensor_type"
            ),
            &["sensor_type"],
        )?;
        let agent_sensor_type_max = GaugeVec::new(
            opts!(
                "agent_sensor_type_max",
                "Maximum sensor value grouped by sensor_type"
            ),
            &["sensor_type"],
        )?;
        let agent_sensor_parent_count = GaugeVec::new(
            opts!(
                "agent_sensor_parent_count",
                "Number of sensors grouped by sensor_type and parent"
            ),
            &["sensor_type", "parent"],
        )?;
        let agent_sensor_parent_avg = GaugeVec::new(
            opts!(
                "agent_sensor_parent_avg",
                "Average sensor value grouped by sensor_type and parent"
            ),
            &["sensor_type", "parent"],
        )?;
        let agent_sensor_parent_max = GaugeVec::new(
            opts!(
                "agent_sensor_parent_max",
                "Maximum sensor value grouped by sensor_type and parent"
            ),
            &["sensor_type", "parent"],
        )?;

        let agent_http_check_up = GaugeVec::new(
            opts!("agent_http_check_up", "HTTP check up status 0/1"),
            &["name"],
        )?;
        let agent_http_check_latency_ms = GaugeVec::new(
            opts!("agent_http_check_latency_ms", "HTTP check latency in ms"),
            &["name"],
        )?;
        let agent_http_check_status_code = GaugeVec::new(
            opts!("agent_http_check_status_code", "HTTP check status code"),
            &["name"],
        )?;
        let agent_tcp_check_up = GaugeVec::new(
            opts!("agent_tcp_check_up", "TCP check up status 0/1"),
            &["name"],
        )?;
        let agent_tcp_check_latency_ms = GaugeVec::new(
            opts!("agent_tcp_check_latency_ms", "TCP check latency in ms"),
            &["name"],
        )?;

        let agent_http_checks_total = Gauge::with_opts(opts!(
            "agent_http_checks_total",
            "Total configured HTTP checks"
        ))?;
        let agent_http_checks_up =
            Gauge::with_opts(opts!("agent_http_checks_up", "HTTP checks in UP state"))?;
        let agent_http_checks_down =
            Gauge::with_opts(opts!("agent_http_checks_down", "HTTP checks in DOWN state"))?;
        let agent_tcp_checks_total = Gauge::with_opts(opts!(
            "agent_tcp_checks_total",
            "Total configured TCP checks"
        ))?;
        let agent_tcp_checks_up =
            Gauge::with_opts(opts!("agent_tcp_checks_up", "TCP checks in UP state"))?;
        let agent_tcp_checks_down =
            Gauge::with_opts(opts!("agent_tcp_checks_down", "TCP checks in DOWN state"))?;
        let agent_checks_total =
            Gauge::with_opts(opts!("agent_checks_total", "Total number of checks"))?;
        let agent_checks_up = Gauge::with_opts(opts!("agent_checks_up", "Checks in UP state"))?;
        let agent_checks_down =
            Gauge::with_opts(opts!("agent_checks_down", "Checks in DOWN state"))?;
        let agent_checks_down_ratio_percent = Gauge::with_opts(opts!(
            "agent_checks_down_ratio_percent",
            "Percentage of checks in DOWN state"
        ))?;

        let agent_uptime_seconds =
            Gauge::with_opts(opts!("agent_uptime_seconds", "Agent uptime in seconds"))?;
        let agent_scrape_count_total = Counter::with_opts(opts!(
            "agent_scrape_count_total",
            "Number of /metrics scrapes"
        ))?;
        let agent_collect_errors_total = CounterVec::new(
            opts!(
                "agent_collect_errors_total",
                "Collector errors total by collector"
            ),
            &["collector"],
        )?;
        let agent_alerts_sent_total = CounterVec::new(
            opts!("agent_alerts_sent_total", "Sent alerts total by kind"),
            &["kind"],
        )?;
        let agent_last_collect_timestamp_seconds = Gauge::with_opts(opts!(
            "agent_last_collect_timestamp_seconds",
            "Unix timestamp of the last collection"
        ))?;

        register(&registry, &agent_cpu_usage_percent)?;
        register(&registry, &agent_memory_used_bytes)?;
        register(&registry, &agent_memory_total_bytes)?;
        register(&registry, &agent_ram_used_bytes)?;
        register(&registry, &agent_ram_total_bytes)?;
        register(&registry, &agent_ram_usage_percent)?;
        register(&registry, &agent_disk_used_bytes)?;
        register(&registry, &agent_disk_total_bytes)?;
        register(&registry, &agent_disk_usage_percent)?;
        register(&registry, &agent_disk_count)?;
        register(&registry, &agent_temperature_celsius)?;
        register(&registry, &agent_temperature_critical_celsius)?;
        register(&registry, &agent_temperature_sensor_count)?;
        register(&registry, &agent_net_rx_bytes_total)?;
        register(&registry, &agent_net_tx_bytes_total)?;
        register(&registry, &agent_net_rx_bytes_per_sec)?;
        register(&registry, &agent_net_tx_bytes_per_sec)?;
        register(&registry, &agent_net_iface_count)?;
        register(&registry, &agent_net_rx_bytes_per_sec_total)?;
        register(&registry, &agent_net_tx_bytes_per_sec_total)?;
        register(&registry, &agent_gpu_utilization_percent)?;
        register(&registry, &agent_gpu_memory_used_bytes)?;
        register(&registry, &agent_gpu_memory_total_bytes)?;
        register(&registry, &agent_gpu_memory_usage_percent)?;
        register(&registry, &agent_gpu_temperature_celsius)?;
        register(&registry, &agent_gpu_count)?;
        register(&registry, &agent_sensor_value)?;
        register(&registry, &agent_sensor_min)?;
        register(&registry, &agent_sensor_max)?;
        register(&registry, &agent_sensor_count)?;
        register(&registry, &agent_sensor_type_count)?;
        register(&registry, &agent_sensor_type_avg)?;
        register(&registry, &agent_sensor_type_min)?;
        register(&registry, &agent_sensor_type_max)?;
        register(&registry, &agent_sensor_parent_count)?;
        register(&registry, &agent_sensor_parent_avg)?;
        register(&registry, &agent_sensor_parent_max)?;
        register(&registry, &agent_http_check_up)?;
        register(&registry, &agent_http_check_latency_ms)?;
        register(&registry, &agent_http_check_status_code)?;
        register(&registry, &agent_tcp_check_up)?;
        register(&registry, &agent_tcp_check_latency_ms)?;
        register(&registry, &agent_http_checks_total)?;
        register(&registry, &agent_http_checks_up)?;
        register(&registry, &agent_http_checks_down)?;
        register(&registry, &agent_tcp_checks_total)?;
        register(&registry, &agent_tcp_checks_up)?;
        register(&registry, &agent_tcp_checks_down)?;
        register(&registry, &agent_checks_total)?;
        register(&registry, &agent_checks_up)?;
        register(&registry, &agent_checks_down)?;
        register(&registry, &agent_checks_down_ratio_percent)?;
        register(&registry, &agent_uptime_seconds)?;
        register(&registry, &agent_scrape_count_total)?;
        register(&registry, &agent_collect_errors_total)?;
        register(&registry, &agent_alerts_sent_total)?;
        register(&registry, &agent_last_collect_timestamp_seconds)?;

        Ok(Arc::new(Self {
            registry,
            agent_cpu_usage_percent,
            agent_memory_used_bytes,
            agent_memory_total_bytes,
            agent_ram_used_bytes,
            agent_ram_total_bytes,
            agent_ram_usage_percent,
            agent_disk_used_bytes,
            agent_disk_total_bytes,
            agent_disk_usage_percent,
            agent_disk_count,
            agent_temperature_celsius,
            agent_temperature_critical_celsius,
            agent_temperature_sensor_count,
            agent_net_rx_bytes_total,
            agent_net_tx_bytes_total,
            agent_net_rx_bytes_per_sec,
            agent_net_tx_bytes_per_sec,
            agent_net_iface_count,
            agent_net_rx_bytes_per_sec_total,
            agent_net_tx_bytes_per_sec_total,
            agent_gpu_utilization_percent,
            agent_gpu_memory_used_bytes,
            agent_gpu_memory_total_bytes,
            agent_gpu_memory_usage_percent,
            agent_gpu_temperature_celsius,
            agent_gpu_count,
            agent_sensor_value,
            agent_sensor_min,
            agent_sensor_max,
            agent_sensor_count,
            agent_sensor_type_count,
            agent_sensor_type_avg,
            agent_sensor_type_min,
            agent_sensor_type_max,
            agent_sensor_parent_count,
            agent_sensor_parent_avg,
            agent_sensor_parent_max,
            agent_http_check_up,
            agent_http_check_latency_ms,
            agent_http_check_status_code,
            agent_tcp_check_up,
            agent_tcp_check_latency_ms,
            agent_http_checks_total,
            agent_http_checks_up,
            agent_http_checks_down,
            agent_tcp_checks_total,
            agent_tcp_checks_up,
            agent_tcp_checks_down,
            agent_checks_total,
            agent_checks_up,
            agent_checks_down,
            agent_checks_down_ratio_percent,
            agent_uptime_seconds,
            agent_scrape_count_total,
            agent_collect_errors_total,
            agent_alerts_sent_total,
            agent_last_collect_timestamp_seconds,
        }))
    }

    pub fn update_from_state(&self, state: &State) {
        self.agent_cpu_usage_percent.set(state.cpu_usage_percent);
        self.agent_memory_used_bytes
            .set(state.memory_used_bytes as f64);
        self.agent_memory_total_bytes
            .set(state.memory_total_bytes as f64);
        self.agent_ram_used_bytes
            .set(state.memory_used_bytes as f64);
        self.agent_ram_total_bytes
            .set(state.memory_total_bytes as f64);
        let ram_pct = if state.memory_total_bytes > 0 {
            (state.memory_used_bytes as f64 / state.memory_total_bytes as f64) * 100.0
        } else {
            0.0
        };
        self.agent_ram_usage_percent.set(ram_pct);
        self.agent_last_collect_timestamp_seconds
            .set(state.last_collect_timestamp_seconds as f64);

        self.agent_disk_used_bytes.reset();
        self.agent_disk_total_bytes.reset();
        self.agent_disk_usage_percent.reset();
        self.agent_temperature_celsius.reset();
        self.agent_temperature_critical_celsius.reset();
        self.agent_net_rx_bytes_total.reset();
        self.agent_net_tx_bytes_total.reset();
        self.agent_net_rx_bytes_per_sec.reset();
        self.agent_net_tx_bytes_per_sec.reset();
        self.agent_gpu_utilization_percent.reset();
        self.agent_gpu_memory_used_bytes.reset();
        self.agent_gpu_memory_total_bytes.reset();
        self.agent_gpu_memory_usage_percent.reset();
        self.agent_gpu_temperature_celsius.reset();
        self.agent_sensor_value.reset();
        self.agent_sensor_min.reset();
        self.agent_sensor_max.reset();
        self.agent_sensor_type_count.reset();
        self.agent_sensor_type_avg.reset();
        self.agent_sensor_type_min.reset();
        self.agent_sensor_type_max.reset();
        self.agent_sensor_parent_count.reset();
        self.agent_sensor_parent_avg.reset();
        self.agent_sensor_parent_max.reset();
        self.agent_http_check_up.reset();
        self.agent_http_check_latency_ms.reset();
        self.agent_http_check_status_code.reset();
        self.agent_tcp_check_up.reset();
        self.agent_tcp_check_latency_ms.reset();

        for d in &state.disks {
            self.agent_disk_used_bytes
                .with_label_values(&[&d.mount])
                .set(d.used_bytes as f64);
            self.agent_disk_total_bytes
                .with_label_values(&[&d.mount])
                .set(d.total_bytes as f64);
            let pct = if d.total_bytes > 0 {
                (d.used_bytes as f64 / d.total_bytes as f64) * 100.0
            } else {
                0.0
            };
            self.agent_disk_usage_percent
                .with_label_values(&[&d.mount])
                .set(pct);
        }
        self.agent_disk_count.set(state.disks.len() as f64);

        let mut total_rx_bps = 0_u64;
        let mut total_tx_bps = 0_u64;
        for n in &state.net {
            self.agent_net_rx_bytes_total
                .with_label_values(&[&n.iface])
                .set(n.rx_bytes_total as f64);
            self.agent_net_tx_bytes_total
                .with_label_values(&[&n.iface])
                .set(n.tx_bytes_total as f64);
            self.agent_net_rx_bytes_per_sec
                .with_label_values(&[&n.iface])
                .set(n.rx_bytes_per_sec as f64);
            self.agent_net_tx_bytes_per_sec
                .with_label_values(&[&n.iface])
                .set(n.tx_bytes_per_sec as f64);
            total_rx_bps = total_rx_bps.saturating_add(n.rx_bytes_per_sec);
            total_tx_bps = total_tx_bps.saturating_add(n.tx_bytes_per_sec);
        }
        self.agent_net_iface_count.set(state.net.len() as f64);
        self.agent_net_rx_bytes_per_sec_total
            .set(total_rx_bps as f64);
        self.agent_net_tx_bytes_per_sec_total
            .set(total_tx_bps as f64);

        for t in &state.temps {
            self.agent_temperature_celsius
                .with_label_values(&[&t.sensor])
                .set(t.temperature_celsius);
            if let Some(critical) = t.critical_temperature_celsius {
                self.agent_temperature_critical_celsius
                    .with_label_values(&[&t.sensor])
                    .set(critical);
            }
        }
        self.agent_temperature_sensor_count
            .set(state.temps.len() as f64);

        self.agent_gpu_count.set(state.gpus.len() as f64);
        for g in &state.gpus {
            let labels: [&str; 2] = [&g.id, &g.name];
            if let Some(v) = g.utilization_percent {
                self.agent_gpu_utilization_percent
                    .with_label_values(&labels)
                    .set(v);
            }
            if let Some(v) = g.memory_used_bytes {
                self.agent_gpu_memory_used_bytes
                    .with_label_values(&labels)
                    .set(v as f64);
            }
            if let Some(v) = g.memory_total_bytes {
                self.agent_gpu_memory_total_bytes
                    .with_label_values(&labels)
                    .set(v as f64);
            }
            if let (Some(used), Some(total)) = (g.memory_used_bytes, g.memory_total_bytes) {
                let pct = if total > 0 {
                    (used as f64 / total as f64) * 100.0
                } else {
                    0.0
                };
                self.agent_gpu_memory_usage_percent
                    .with_label_values(&labels)
                    .set(pct);
            }
            if let Some(v) = g.temperature_celsius {
                self.agent_gpu_temperature_celsius
                    .with_label_values(&labels)
                    .set(v);
            }
        }

        self.agent_sensor_count.set(state.sensors.len() as f64);
        let mut grouped: HashMap<&str, (f64, u64, f64, f64)> = HashMap::new();
        let mut grouped_parent: HashMap<(String, String), (f64, u64, f64)> = HashMap::new();
        for s in &state.sensors {
            let labels = [
                s.sensor_type.as_str(),
                s.name.as_str(),
                s.identifier.as_str(),
                s.parent.as_str(),
            ];
            self.agent_sensor_value
                .with_label_values(&labels)
                .set(s.value);
            if let Some(v) = s.min {
                self.agent_sensor_min.with_label_values(&labels).set(v);
            }
            if let Some(v) = s.max {
                self.agent_sensor_max.with_label_values(&labels).set(v);
            }

            let entry = grouped.entry(s.sensor_type.as_str()).or_insert((
                0.0_f64,
                0_u64,
                f64::MIN,
                f64::MAX,
            ));
            entry.0 += s.value;
            entry.1 = entry.1.saturating_add(1);
            if s.value > entry.2 {
                entry.2 = s.value;
            }
            if s.value < entry.3 {
                entry.3 = s.value;
            }

            let pkey = (s.sensor_type.clone(), s.parent.clone());
            let pentry = grouped_parent
                .entry(pkey)
                .or_insert((0.0_f64, 0_u64, f64::MIN));
            pentry.0 += s.value;
            pentry.1 = pentry.1.saturating_add(1);
            if s.value > pentry.2 {
                pentry.2 = s.value;
            }
        }
        for (sensor_type, (sum, count, max_value, min_value)) in grouped {
            self.agent_sensor_type_count
                .with_label_values(&[sensor_type])
                .set(count as f64);
            self.agent_sensor_type_avg
                .with_label_values(&[sensor_type])
                .set(if count > 0 { sum / count as f64 } else { 0.0 });
            self.agent_sensor_type_min
                .with_label_values(&[sensor_type])
                .set(if min_value.is_finite() {
                    min_value
                } else {
                    0.0
                });
            self.agent_sensor_type_max
                .with_label_values(&[sensor_type])
                .set(if max_value.is_finite() {
                    max_value
                } else {
                    0.0
                });
        }
        for ((sensor_type, parent), (sum, count, max_value)) in grouped_parent {
            let labels = [sensor_type.as_str(), parent.as_str()];
            self.agent_sensor_parent_count
                .with_label_values(&labels)
                .set(count as f64);
            self.agent_sensor_parent_avg
                .with_label_values(&labels)
                .set(if count > 0 { sum / count as f64 } else { 0.0 });
            self.agent_sensor_parent_max
                .with_label_values(&labels)
                .set(if max_value.is_finite() {
                    max_value
                } else {
                    0.0
                });
        }

        let http_total = state.checks.http.len() as f64;
        let http_up = state.checks.http.iter().filter(|c| c.up).count() as f64;
        let http_down = (http_total - http_up).max(0.0);
        self.agent_http_checks_total.set(http_total);
        self.agent_http_checks_up.set(http_up);
        self.agent_http_checks_down.set(http_down);

        let tcp_total = state.checks.tcp.len() as f64;
        let tcp_up = state.checks.tcp.iter().filter(|c| c.up).count() as f64;
        let tcp_down = (tcp_total - tcp_up).max(0.0);
        self.agent_tcp_checks_total.set(tcp_total);
        self.agent_tcp_checks_up.set(tcp_up);
        self.agent_tcp_checks_down.set(tcp_down);

        self.agent_checks_total.set(http_total + tcp_total);
        self.agent_checks_up.set(http_up + tcp_up);
        self.agent_checks_down.set(http_down + tcp_down);
        let checks_total = http_total + tcp_total;
        let down_ratio = if checks_total > 0.0 {
            ((http_down + tcp_down) / checks_total) * 100.0
        } else {
            0.0
        };
        self.agent_checks_down_ratio_percent.set(down_ratio);

        for c in &state.checks.http {
            self.agent_http_check_up
                .with_label_values(&[&c.name])
                .set(if c.up { 1.0 } else { 0.0 });
            self.agent_http_check_latency_ms
                .with_label_values(&[&c.name])
                .set(c.latency_ms as f64);
            self.agent_http_check_status_code
                .with_label_values(&[&c.name])
                .set(c.status_code as f64);
        }

        for c in &state.checks.tcp {
            self.agent_tcp_check_up
                .with_label_values(&[&c.name])
                .set(if c.up { 1.0 } else { 0.0 });
            self.agent_tcp_check_latency_ms
                .with_label_values(&[&c.name])
                .set(c.latency_ms as f64);
        }

        let now = now_unix();
        let uptime = now.saturating_sub(state.started_at_unix) as f64;
        self.agent_uptime_seconds.set(uptime);
    }

    pub fn inc_scrape_count(&self) {
        self.agent_scrape_count_total.inc();
    }

    pub fn inc_collect_error(&self, collector: &str) {
        self.agent_collect_errors_total
            .with_label_values(&[collector])
            .inc();
    }

    pub fn inc_alert_sent(&self, kind: &str) {
        self.agent_alerts_sent_total
            .with_label_values(&[kind])
            .inc();
    }

    pub fn encode_metrics(&self) -> Result<Vec<u8>, prometheus::Error> {
        let mut buf = Vec::new();
        let encoder = TextEncoder::new();
        let mf = self.registry.gather();
        encoder.encode(&mf, &mut buf)?;
        Ok(buf)
    }
}

fn register<T: Collector + Clone + 'static>(
    registry: &Registry,
    collector: &T,
) -> Result<(), prometheus::Error> {
    registry.register(Box::new(collector.clone()))
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
