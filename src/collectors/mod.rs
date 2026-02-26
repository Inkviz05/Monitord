pub mod checks;
pub mod system;

use crate::state::{DiskStat, GpuStat, NetStat, SensorStat, TempStat};

#[derive(Debug, Clone)]
pub struct SystemSnapshot {
    pub host_name: Option<String>,
    pub os_name: Option<String>,
    pub os_version: Option<String>,
    pub kernel_version: Option<String>,
    pub cpu_brand: Option<String>,
    pub uptime_seconds: u64,
    pub process_count: u64,
    pub cpu_core_count: u32,
    pub cpu_usage_percent: f64,
    pub memory_used_bytes: u64,
    pub memory_total_bytes: u64,
    pub disks: Vec<DiskStat>,
    pub net: Vec<NetStat>,
    pub temps: Vec<TempStat>,
    pub gpus: Vec<GpuStat>,
    pub sensors: Vec<SensorStat>,
}
