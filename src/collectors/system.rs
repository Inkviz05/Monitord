use crate::collectors::SystemSnapshot;
use crate::state::{DiskStat, GpuStat, NetStat, SensorStat, TempStat};
use std::collections::HashMap;
#[cfg(target_os = "linux")]
use std::fs;
use std::process::Command;
use sysinfo::{ComponentExt, CpuExt, DiskExt, NetworkExt, NetworksExt, System, SystemExt};
use tracing::debug;

pub fn collect_system(system: &mut System) -> SystemSnapshot {
    system.refresh_cpu();
    system.refresh_memory();
    system.refresh_processes();
    system.refresh_disks_list();
    system.refresh_disks();
    system.refresh_networks_list();
    system.refresh_networks();
    system.refresh_components_list();
    system.refresh_components();
    let host_name = system.host_name();
    let os_name = system.name();
    let os_version = system.os_version();
    let kernel_version = system.kernel_version();
    let cpu_brand = system.cpus().first().map(|c| c.brand().to_string());
    let uptime_seconds = system.uptime();
    let process_count = system.processes().len() as u64;
    let cpu_core_count = system.cpus().len() as u32;

    let cpu_usage_percent = if system.cpus().is_empty() {
        0.0
    } else {
        let sum: f32 = system.cpus().iter().map(|c| c.cpu_usage()).sum();
        (sum / system.cpus().len() as f32) as f64
    };

    let memory_total_bytes = system.total_memory() * 1024;
    let memory_used_bytes = system.used_memory() * 1024;

    let disks: Vec<DiskStat> = system
        .disks()
        .iter()
        .map(|d| {
            let total = d.total_space();
            let used = total.saturating_sub(d.available_space());
            DiskStat {
                mount: d.mount_point().to_string_lossy().to_string(),
                used_bytes: used,
                total_bytes: total,
            }
        })
        .collect();

    let net: Vec<NetStat> = system
        .networks()
        .iter()
        .map(|(iface, data)| NetStat {
            iface: iface.to_string(),
            rx_bytes_total: data.total_received(),
            tx_bytes_total: data.total_transmitted(),
            rx_bytes_per_sec: 0,
            tx_bytes_per_sec: 0,
        })
        .collect();

    let mut temps = collect_temps(system);
    let gpus = collect_gpu_stats(system);
    let (lhm_temps, lhm_gpus, lhm_sensors) = collect_lhm_snapshot();
    if !lhm_temps.is_empty() {
        temps.extend(lhm_temps);
    }
    let gpus = merge_gpu_stats(gpus, lhm_gpus);
    let sensors = collect_builtin_sensor_stats(
        cpu_usage_percent,
        memory_used_bytes,
        memory_total_bytes,
        &disks,
        &net,
        &temps,
        &gpus,
    );
    let sensors = merge_sensors(sensors, lhm_sensors);

    SystemSnapshot {
        host_name,
        os_name,
        os_version,
        kernel_version,
        cpu_brand,
        uptime_seconds,
        process_count,
        cpu_core_count,
        cpu_usage_percent,
        memory_used_bytes,
        memory_total_bytes,
        disks,
        net,
        temps,
        gpus,
        sensors,
    }
}

fn collect_builtin_sensor_stats(
    cpu_usage_percent: f64,
    memory_used_bytes: u64,
    memory_total_bytes: u64,
    disks: &[DiskStat],
    net: &[NetStat],
    temps: &[TempStat],
    gpus: &[GpuStat],
) -> Vec<SensorStat> {
    let mut out = Vec::new();
    out.push(SensorStat {
        sensor_type: "load".to_string(),
        name: "CPU Package".to_string(),
        identifier: "/cpu/package/load".to_string(),
        parent: "/cpu/package".to_string(),
        value: cpu_usage_percent,
        min: None,
        max: None,
    });
    out.push(SensorStat {
        sensor_type: "load".to_string(),
        name: "CPU Total".to_string(),
        identifier: "/cpu/total/load".to_string(),
        parent: "/cpu/total".to_string(),
        value: cpu_usage_percent,
        min: None,
        max: None,
    });

    let mem_used_gb = memory_used_bytes as f64 / 1024.0 / 1024.0 / 1024.0;
    let mem_total_gb = memory_total_bytes as f64 / 1024.0 / 1024.0 / 1024.0;
    let mem_free_gb = mem_total_gb - mem_used_gb;
    let mem_load = if memory_total_bytes > 0 {
        (memory_used_bytes as f64 / memory_total_bytes as f64) * 100.0
    } else {
        0.0
    };
    out.push(SensorStat {
        sensor_type: "data".to_string(),
        name: "Memory Used".to_string(),
        identifier: "/memory/used".to_string(),
        parent: "/memory".to_string(),
        value: mem_used_gb,
        min: None,
        max: None,
    });
    out.push(SensorStat {
        sensor_type: "data".to_string(),
        name: "Memory Total".to_string(),
        identifier: "/memory/total".to_string(),
        parent: "/memory".to_string(),
        value: mem_total_gb,
        min: None,
        max: None,
    });
    out.push(SensorStat {
        sensor_type: "data".to_string(),
        name: "Memory Free".to_string(),
        identifier: "/memory/free".to_string(),
        parent: "/memory".to_string(),
        value: mem_free_gb.max(0.0),
        min: None,
        max: None,
    });
    out.push(SensorStat {
        sensor_type: "load".to_string(),
        name: "Memory Load".to_string(),
        identifier: "/memory/load".to_string(),
        parent: "/memory".to_string(),
        value: mem_load,
        min: None,
        max: None,
    });

    for d in disks {
        let parent = format!("/disk/{}", d.mount);
        let load = if d.total_bytes > 0 {
            (d.used_bytes as f64 / d.total_bytes as f64) * 100.0
        } else {
            0.0
        };
        out.push(SensorStat {
            sensor_type: "load".to_string(),
            name: format!("Disk {} Used", d.mount),
            identifier: format!("{parent}/load"),
            parent: parent.clone(),
            value: load,
            min: None,
            max: None,
        });
        out.push(SensorStat {
            sensor_type: "data".to_string(),
            name: format!("Disk {} Used", d.mount),
            identifier: format!("{parent}/used"),
            parent: parent.clone(),
            value: d.used_bytes as f64 / 1024.0 / 1024.0 / 1024.0,
            min: None,
            max: None,
        });
        out.push(SensorStat {
            sensor_type: "data".to_string(),
            name: format!("Disk {} Total", d.mount),
            identifier: format!("{parent}/total"),
            parent,
            value: d.total_bytes as f64 / 1024.0 / 1024.0 / 1024.0,
            min: None,
            max: None,
        });
        out.push(SensorStat {
            sensor_type: "data".to_string(),
            name: format!("Disk {} Free", d.mount),
            identifier: format!("{}/free", format!("/disk/{}", d.mount)),
            parent: format!("/disk/{}", d.mount),
            value: d.total_bytes.saturating_sub(d.used_bytes) as f64 / 1024.0 / 1024.0 / 1024.0,
            min: None,
            max: None,
        });
    }

    for n in net {
        let parent = format!("/net/{}", n.iface);
        out.push(SensorStat {
            sensor_type: "throughput".to_string(),
            name: format!("{} RX", n.iface),
            identifier: format!("{parent}/rx"),
            parent: parent.clone(),
            value: n.rx_bytes_per_sec as f64,
            min: None,
            max: None,
        });
        out.push(SensorStat {
            sensor_type: "throughput".to_string(),
            name: format!("{} TX", n.iface),
            identifier: format!("{parent}/tx"),
            parent: parent.clone(),
            value: n.tx_bytes_per_sec as f64,
            min: None,
            max: None,
        });
        out.push(SensorStat {
            sensor_type: "data".to_string(),
            name: format!("{} RX Total", n.iface),
            identifier: format!("{parent}/rx_total"),
            parent: parent.clone(),
            value: n.rx_bytes_total as f64 / 1024.0 / 1024.0,
            min: None,
            max: None,
        });
        out.push(SensorStat {
            sensor_type: "data".to_string(),
            name: format!("{} TX Total", n.iface),
            identifier: format!("{parent}/tx_total"),
            parent,
            value: n.tx_bytes_total as f64 / 1024.0 / 1024.0,
            min: None,
            max: None,
        });
    }

    for t in temps {
        out.push(SensorStat {
            sensor_type: "temperature".to_string(),
            name: t.sensor.clone(),
            identifier: format!("/temperature/{}", t.sensor),
            parent: "/temperature".to_string(),
            value: t.temperature_celsius,
            min: None,
            max: t.critical_temperature_celsius,
        });
    }

    for g in gpus {
        let parent = format!("/gpu/{}", g.id);
        if let Some(v) = g.utilization_percent {
            out.push(SensorStat {
                sensor_type: "load".to_string(),
                name: format!("{} Load", g.name),
                identifier: format!("{parent}/load"),
                parent: parent.clone(),
                value: v,
                min: None,
                max: None,
            });
        }
        if let Some(v) = g.temperature_celsius {
            out.push(SensorStat {
                sensor_type: "temperature".to_string(),
                name: format!("{} Temp", g.name),
                identifier: format!("{parent}/temperature"),
                parent: parent.clone(),
                value: v,
                min: None,
                max: None,
            });
        }
        if let Some(v) = g.memory_used_bytes {
            out.push(SensorStat {
                sensor_type: "smalldata".to_string(),
                name: format!("{} Memory Used", g.name),
                identifier: format!("{parent}/memory_used"),
                parent: parent.clone(),
                value: v as f64 / 1024.0 / 1024.0,
                min: None,
                max: None,
            });
        }
        if let Some(v) = g.memory_total_bytes {
            out.push(SensorStat {
                sensor_type: "smalldata".to_string(),
                name: format!("{} Memory Total", g.name),
                identifier: format!("{parent}/memory_total"),
                parent: parent.clone(),
                value: v as f64 / 1024.0 / 1024.0,
                min: None,
                max: None,
            });
        }
        if let (Some(used), Some(total)) = (g.memory_used_bytes, g.memory_total_bytes) {
            if total > 0 {
                out.push(SensorStat {
                    sensor_type: "load".to_string(),
                    name: format!("{} Memory Load", g.name),
                    identifier: format!("{parent}/memory_load"),
                    parent: parent.clone(),
                    value: (used as f64 / total as f64) * 100.0,
                    min: None,
                    max: None,
                });
            }
        }
    }

    out
}

fn merge_sensors(base: Vec<SensorStat>, extra: Vec<SensorStat>) -> Vec<SensorStat> {
    if base.is_empty() {
        return extra;
    }
    if extra.is_empty() {
        return base;
    }
    let mut map: HashMap<(String, String), SensorStat> = HashMap::new();
    for s in base {
        map.insert((s.sensor_type.clone(), s.identifier.clone()), s);
    }
    for s in extra {
        map.insert((s.sensor_type.clone(), s.identifier.clone()), s);
    }
    map.into_values().collect()
}

fn collect_temps(system: &System) -> Vec<TempStat> {
    let mut temps: Vec<TempStat> = system
        .components()
        .iter()
        .map(|c| TempStat {
            sensor: c.label().to_string(),
            temperature_celsius: c.temperature() as f64,
            critical_temperature_celsius: c.critical().map(|v| v as f64),
        })
        .filter(|t| t.temperature_celsius > 0.0)
        .collect();

    let sys_count = temps.len();
    let nvidia = collect_temps_from_nvidia_smi();
    let win = collect_windows_temps();
    let lin = collect_linux_temps();
    debug!(
        sysinfo_temps = sys_count,
        nvidia_temps = nvidia.len(),
        windows_temps = win.len(),
        linux_temps = lin.len(),
        "результат сбора температур по источникам"
    );

    temps.extend(nvidia);
    temps.extend(win);
    temps.extend(lin);

    temps
}

#[cfg(target_os = "windows")]
fn collect_windows_temps() -> Vec<TempStat> {
    let wmic = collect_windows_temps_wmic();
    if !wmic.is_empty() {
        return wmic;
    }

    let cim = collect_windows_temps_cim();
    if !cim.is_empty() {
        return cim;
    }

    collect_windows_temps_typeperf()
}

#[cfg(target_os = "windows")]
fn collect_windows_temps_wmic() -> Vec<TempStat> {
    let output = Command::new("wmic")
        .args([
            "/namespace:\\\\root\\wmi",
            "PATH",
            "MSAcpi_ThermalZoneTemperature",
            "get",
            "CurrentTemperature,InstanceName",
            "/format:csv",
        ])
        .output();

    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    let text = decode_cmd_stdout(&output.stdout);

    text.lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split(',').map(|v| v.trim()).collect();
            if parts.len() < 3 {
                return None;
            }
            let instance = parts[1];
            let raw = parts[2].parse::<f64>().ok()?;
            if raw <= 0.0 {
                return None;
            }
            let celsius = normalize_windows_thermal_zone_temp(raw)?;
            Some(TempStat {
                sensor: format!("ACPI {} (fallback)", instance),
                temperature_celsius: celsius,
                critical_temperature_celsius: None,
            })
        })
        .collect()
}

#[cfg(target_os = "windows")]
fn collect_windows_temps_cim() -> Vec<TempStat> {
    let cmd = "$t=Get-CimInstance -Namespace root/wmi -ClassName MSAcpi_ThermalZoneTemperature -ErrorAction SilentlyContinue; if ($null -ne $t) { $t | ForEach-Object { \"$($_.InstanceName)|$($_.CurrentTemperature)\" } }";
    let Some(output) = run_powershell(cmd) else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    let text = decode_cmd_stdout(&output.stdout);

    let out: Vec<TempStat> = text
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(2, '|').map(str::trim);
            let sensor = parts.next()?;
            let raw = parts.next()?.parse::<f64>().ok()?;
            if sensor.is_empty() || raw <= 0.0 {
                return None;
            }

            Some(TempStat {
                sensor: format!("ACPI {} (fallback)", sensor),
                temperature_celsius: normalize_windows_thermal_zone_temp(raw)?,
                critical_temperature_celsius: None,
            })
        })
        .collect();

    out
}

#[cfg(target_os = "windows")]
fn collect_windows_temps_typeperf() -> Vec<TempStat> {
    let Some(output) = run_typeperf(["\\Thermal Zone Information(*)\\Temperature", "-sc", "1"])
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    let text = decode_cmd_stdout(&output.stdout);

    let mut candidates = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("Exiting") || line.starts_with("The command") {
            continue;
        }

        for token in line.split([',', '"', ';', ' ']) {
            if token.is_empty() {
                continue;
            }
            // Skip timestamp fragments from typeperf CSV lines.
            if token.contains(':') || token.contains('/') || token.contains('\\') {
                continue;
            }
            if let Some(v) = parse_f64_loose(token) {
                if v > 100.0 {
                    candidates.push(v);
                }
            }
        }
    }

    let Some(raw_value) = candidates.into_iter().max_by(|a, b| a.total_cmp(b)) else {
        return Vec::new();
    };
    let Some(value) = normalize_windows_thermal_zone_temp(raw_value) else {
        return Vec::new();
    };

    vec![TempStat {
        sensor: "ACPI CPU thermal zone (fallback)".to_string(),
        temperature_celsius: value,
        critical_temperature_celsius: None,
    }]
}

#[cfg(not(target_os = "windows"))]
fn collect_windows_temps() -> Vec<TempStat> {
    Vec::new()
}

#[cfg(target_os = "linux")]
fn collect_linux_temps() -> Vec<TempStat> {
    let Ok(entries) = fs::read_dir("/sys/class/thermal") else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|v| v.to_str()) else {
            continue;
        };
        if !name.starts_with("thermal_zone") {
            continue;
        }

        let temp_path = path.join("temp");
        let typ_path = path.join("type");
        let temp_raw = fs::read_to_string(temp_path).ok();
        let typ = fs::read_to_string(typ_path)
            .ok()
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| name.to_string());
        let Some(temp_raw) = temp_raw else {
            continue;
        };
        let Ok(v) = temp_raw.trim().parse::<f64>() else {
            continue;
        };
        let celsius = if v > 1000.0 { v / 1000.0 } else { v };
        if celsius > 0.0 {
            out.push(TempStat {
                sensor: typ,
                temperature_celsius: celsius,
                critical_temperature_celsius: None,
            });
        }
    }

    out
}

#[cfg(not(target_os = "linux"))]
fn collect_linux_temps() -> Vec<TempStat> {
    Vec::new()
}

fn collect_gpu_stats(system: &System) -> Vec<GpuStat> {
    let mut gpus = collect_nvidia_smi();
    if !gpus.is_empty() {
        return gpus;
    }

    for component in system.components() {
        let label_lower = component.label().to_lowercase();
        if label_lower.contains("gpu")
            || label_lower.contains("nvidia")
            || label_lower.contains("amdgpu")
        {
            gpus.push(GpuStat {
                id: label_lower.clone(),
                name: component.label().to_string(),
                utilization_percent: None,
                memory_used_bytes: None,
                memory_total_bytes: None,
                temperature_celsius: Some(component.temperature() as f64),
            });
        }
    }

    if gpus.is_empty() {
        gpus = collect_windows_gpu_stats();
    }

    gpus
}

fn merge_gpu_stats(base: Vec<GpuStat>, extra: Vec<GpuStat>) -> Vec<GpuStat> {
    if base.is_empty() {
        return extra;
    }
    if extra.is_empty() {
        return base;
    }

    let mut merged = base.clone();
    for e in extra {
        if let Some(existing) = merged.iter_mut().find(|b| {
            b.name.eq_ignore_ascii_case(&e.name)
                || (!b.id.is_empty() && !e.id.is_empty() && b.id == e.id)
        }) {
            if e.utilization_percent.is_some() {
                existing.utilization_percent = e.utilization_percent;
            }
            if e.memory_used_bytes.is_some() {
                existing.memory_used_bytes = e.memory_used_bytes;
            }
            if e.memory_total_bytes.is_some() {
                existing.memory_total_bytes = e.memory_total_bytes;
            }
            if e.temperature_celsius.is_some() {
                existing.temperature_celsius = e.temperature_celsius;
            }
            if !e.name.trim().is_empty() {
                existing.name = e.name.clone();
            }
        } else {
            merged.push(e);
        }
    }

    merged
}

#[cfg(target_os = "windows")]
fn collect_lhm_snapshot() -> (Vec<TempStat>, Vec<GpuStat>, Vec<SensorStat>) {
    let script = "$n=@('root/LibreHardwareMonitor','root/OpenHardwareMonitor'); foreach($ns in $n){ try { $s=Get-CimInstance -Namespace $ns -ClassName Sensor -ErrorAction Stop } catch { continue }; if($s){ $s | ForEach-Object { \"$($_.SensorType)|$($_.Name)|$($_.Value)|$($_.Min)|$($_.Max)|$($_.Identifier)|$($_.Parent)\" }; break } }";
    let Some(output) = run_powershell(script) else {
        return (Vec::new(), Vec::new(), Vec::new());
    };
    if !output.status.success() {
        return (Vec::new(), Vec::new(), Vec::new());
    }

    let text = decode_cmd_stdout(&output.stdout);

    #[derive(Default)]
    struct GpuAcc {
        name: String,
        util: Option<f64>,
        mem_used: Option<u64>,
        mem_total: Option<u64>,
        temp: Option<f64>,
    }

    let mut temps = Vec::new();
    let mut gpus: std::collections::HashMap<String, GpuAcc> = std::collections::HashMap::new();
    let mut sensors = Vec::new();

    for line in text.lines() {
        let parts: Vec<&str> = line.split('|').map(str::trim).collect();
        if parts.len() < 7 {
            continue;
        }
        let sensor_type = parts[0].to_ascii_lowercase();
        let name = parts[1];
        let value = parse_f64_loose(parts[2]);
        let min = parse_f64_loose(parts[3]);
        let max = parse_f64_loose(parts[4]);
        let ident = parts[5].to_ascii_lowercase();
        let parent = parts[6];
        let parent_lc = parent.to_ascii_lowercase();
        let name_lc = name.to_ascii_lowercase();

        if let Some(v) = value {
            if v.is_finite() {
                sensors.push(SensorStat {
                    sensor_type: sensor_type.clone(),
                    name: name.to_string(),
                    identifier: ident.clone(),
                    parent: parent.to_string(),
                    value: v,
                    min,
                    max,
                });
            }
        }

        if sensor_type == "temperature" && is_lhm_cpu_temp_sensor(&ident, &parent_lc, &name_lc) {
            if let Some(v) = value {
                if v > 0.0 {
                    temps.push(TempStat {
                        sensor: format!("CPU {}", name),
                        temperature_celsius: v,
                        critical_temperature_celsius: None,
                    });
                }
            }
        }

        if ident.contains("/gpu-") {
            let key = if parent.is_empty() {
                ident.clone()
            } else {
                parent.to_string()
            };
            let acc = gpus.entry(key.clone()).or_default();
            if acc.name.is_empty() {
                acc.name = key;
            }

            match sensor_type.as_str() {
                "temperature" => {
                    if let Some(v) = value {
                        if v > 0.0 {
                            acc.temp = Some(v);
                        }
                    }
                }
                "load" => {
                    let n = name.to_ascii_lowercase();
                    if n.contains("core") || n.contains("d3d") || n.contains("gpu") {
                        if let Some(v) = value {
                            acc.util = Some(v);
                        }
                    }
                }
                "smalldata" | "data" => {
                    let n = name.to_ascii_lowercase();
                    if let Some(v) = value {
                        if n.contains("memory used") || n.contains("gpu memory used") {
                            acc.mem_used = Some((v.max(0.0) as u64).saturating_mul(1024 * 1024));
                        } else if n.contains("memory total") || n.contains("gpu memory total") {
                            acc.mem_total = Some((v.max(0.0) as u64).saturating_mul(1024 * 1024));
                        }
                    }
                }
                _ => {}
            }
        }
    }

    if temps.is_empty() {
        let fallback_cpu_temp = sensors
            .iter()
            .filter(|s| s.sensor_type == "temperature")
            .filter(|s| {
                let txt = format!("{} {} {}", s.name, s.parent, s.identifier).to_ascii_lowercase();
                !txt.contains("gpu")
                    && !txt.contains("nvidia")
                    && !txt.contains("amdgpu")
                    && !txt.contains("radeon")
                    && (txt.contains("cpu")
                        || txt.contains("package")
                        || txt.contains("core")
                        || txt.contains("tctl")
                        || txt.contains("tdie")
                        || txt.contains("amdcpu")
                        || txt.contains("intelcpu"))
            })
            .max_by(|a, b| a.value.total_cmp(&b.value));
        if let Some(s) = fallback_cpu_temp {
            temps.push(TempStat {
                sensor: format!("CPU {}", s.name),
                temperature_celsius: s.value,
                critical_temperature_celsius: s.max,
            });
        }
    }

    let gpu_stats = gpus
        .into_iter()
        .enumerate()
        .map(|(i, (_k, g))| GpuStat {
            id: i.to_string(),
            name: if g.name.is_empty() {
                format!("gpu-{i}")
            } else {
                g.name
            },
            utilization_percent: g.util,
            memory_used_bytes: g.mem_used,
            memory_total_bytes: g.mem_total,
            temperature_celsius: g.temp,
        })
        .collect();

    (temps, gpu_stats, sensors)
}

#[cfg(not(target_os = "windows"))]
fn collect_lhm_snapshot() -> (Vec<TempStat>, Vec<GpuStat>, Vec<SensorStat>) {
    (Vec::new(), Vec::new(), Vec::new())
}

#[cfg(target_os = "windows")]
fn collect_windows_gpu_stats() -> Vec<GpuStat> {
    let script = "$controllers=Get-CimInstance Win32_VideoController -ErrorAction SilentlyContinue; if(-not $controllers){return}; $eng=Get-CimInstance Win32_PerfFormattedData_GPUPerformanceCounters_GPUEngine -ErrorAction SilentlyContinue; $proc=Get-Counter '\\GPU Process Memory(*)\\Dedicated Usage' -ErrorAction SilentlyContinue; $util=0; if($eng){ $util=($eng | Measure-Object -Property UtilizationPercentage -Sum).Sum }; if($util -lt 0){$util=0}; if($util -gt 100){$util=100}; $used=0; if($proc){ $used=($proc.CounterSamples | Measure-Object -Property CookedValue -Sum).Sum }; if($used -lt 0){$used=0}; $idx=0; foreach($c in $controllers){ $name=$c.Name; $total=0; if($c.AdapterRAM){$total=[double]$c.AdapterRAM}; \"${idx}|${name}|${util}|${used}|${total}\"; $idx++ }";
    let Some(output) = run_powershell(script) else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    let Ok(text) = String::from_utf8(output.stdout) else {
        return Vec::new();
    };

    text.lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('|').map(str::trim).collect();
            if parts.len() < 5 {
                return None;
            }

            let utilization_percent = parse_f64_loose(parts[2]);
            let memory_used_bytes = parse_f64_loose(parts[3]).map(|v| v.max(0.0) as u64);
            let memory_total_bytes = parse_f64_loose(parts[4]).map(|v| v.max(0.0) as u64);

            Some(GpuStat {
                id: parts[0].to_string(),
                name: parts[1].to_string(),
                utilization_percent,
                memory_used_bytes,
                memory_total_bytes,
                temperature_celsius: None,
            })
        })
        .collect()
}

#[cfg(not(target_os = "windows"))]
fn collect_windows_gpu_stats() -> Vec<GpuStat> {
    Vec::new()
}

fn collect_nvidia_smi() -> Vec<GpuStat> {
    let output = run_nvidia_smi(&[
        "--query-gpu=index,name,utilization.gpu,memory.used,memory.total,temperature.gpu",
        "--format=csv,noheader,nounits",
    ]);

    let Some(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    let Ok(text) = String::from_utf8(output.stdout) else {
        return Vec::new();
    };

    text.lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split(',').map(|v| v.trim()).collect();
            if parts.len() < 6 {
                return None;
            }

            let utilization_percent = Some(parse_f64_loose(parts[2]).unwrap_or(0.0));
            let memory_used_bytes = Some(
                parse_u64_loose(parts[3])
                    .unwrap_or(0)
                    .saturating_mul(1024 * 1024),
            );
            let memory_total_bytes = Some(
                parse_u64_loose(parts[4])
                    .unwrap_or(0)
                    .saturating_mul(1024 * 1024),
            );
            let temperature_celsius = parse_f64_loose(parts[5]);

            Some(GpuStat {
                id: parts[0].to_string(),
                name: parts[1].to_string(),
                utilization_percent,
                memory_used_bytes,
                memory_total_bytes,
                temperature_celsius,
            })
        })
        .collect()
}

fn collect_temps_from_nvidia_smi() -> Vec<TempStat> {
    let output = run_nvidia_smi(&[
        "--query-gpu=name,temperature.gpu",
        "--format=csv,noheader,nounits",
    ]);

    let Some(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    let Ok(text) = String::from_utf8(output.stdout) else {
        return Vec::new();
    };

    text.lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split(',').map(|v| v.trim()).collect();
            if parts.len() < 2 {
                return None;
            }

            let temp = parse_f64_loose(parts[1])?;
            if temp <= 0.0 {
                return None;
            }

            Some(TempStat {
                sensor: format!("GPU {}", parts[0]),
                temperature_celsius: temp,
                critical_temperature_celsius: None,
            })
        })
        .collect()
}

fn run_nvidia_smi(args: &[&str]) -> Option<std::process::Output> {
    if let Ok(output) = Command::new("nvidia-smi").args(args).output() {
        return Some(output);
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(output) = Command::new(r"C:\Windows\System32\nvidia-smi.exe")
            .args(args)
            .output()
        {
            return Some(output);
        }
    }

    None
}

fn parse_f64_loose(input: &str) -> Option<f64> {
    let trimmed = input.trim();
    if let Ok(v) = trimmed.parse::<f64>() {
        return Some(v);
    }

    if let Ok(v) = trimmed.replace(',', ".").parse::<f64>() {
        return Some(v);
    }

    let filtered: String = trimmed
        .chars()
        .filter(|c| {
            c.is_ascii_digit()
                || *c == '.'
                || *c == ','
                || *c == 'e'
                || *c == 'E'
                || *c == '-'
                || *c == '+'
        })
        .collect();
    if filtered.is_empty() {
        return None;
    }

    filtered.replace(',', ".").parse::<f64>().ok()
}

fn is_lhm_cpu_temp_sensor(identifier: &str, parent: &str, name: &str) -> bool {
    let combined = format!("{identifier}|{parent}|{name}");
    let has_gpu_marker = ["gpu", "nvidia", "amdgpu", "radeon"]
        .iter()
        .any(|m| combined.contains(m));
    if has_gpu_marker {
        return false;
    }

    let has_cpu_marker = [
        "/intelcpu/",
        "/amdcpu/",
        "/cpu/",
        "cpu package",
        "cpu core",
        "package",
        "tctl",
        "tdie",
        "core",
        "ccd",
        "ccx",
    ]
    .iter()
    .any(|m| combined.contains(m));
    if !has_cpu_marker {
        return false;
    }

    let has_temp_marker = ["temperature", "temp", "tctl", "tdie", "package"]
        .iter()
        .any(|m| combined.contains(m));
    has_temp_marker
}

fn parse_u64_loose(input: &str) -> Option<u64> {
    parse_f64_loose(input).map(|v| if v < 0.0 { 0 } else { v as u64 })
}

fn normalize_windows_thermal_zone_temp(raw: f64) -> Option<f64> {
    if !raw.is_finite() || raw <= 0.0 {
        return None;
    }

    // Some counters expose tenths of Kelvin; others expose Kelvin/Celsius.
    let mut v = raw;
    if v > 1000.0 {
        v /= 10.0;
    }
    if v > 200.0 {
        v -= 273.15;
    }

    if !(0.0..=130.0).contains(&v) {
        return None;
    }
    Some(v)
}

fn decode_cmd_stdout(bytes: &[u8]) -> String {
    if let Ok(utf8) = std::str::from_utf8(bytes) {
        return utf8.to_string();
    }

    if bytes.len() >= 2 && bytes.len() % 2 == 0 {
        let mut u16buf = Vec::with_capacity(bytes.len() / 2);
        let mut i = 0;
        while i + 1 < bytes.len() {
            u16buf.push(u16::from_le_bytes([bytes[i], bytes[i + 1]]));
            i += 2;
        }
        if let Ok(s) = String::from_utf16(&u16buf) {
            return s;
        }
    }

    String::from_utf8_lossy(bytes).to_string()
}

#[cfg(target_os = "windows")]
fn run_powershell(script: &str) -> Option<std::process::Output> {
    let wrapped_script = format!(
        "[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false); $OutputEncoding=[System.Text.UTF8Encoding]::new($false); chcp 65001 > $null; {script}"
    );
    if let Ok(output) = Command::new("powershell")
        .args(["-NoProfile", "-Command", &wrapped_script])
        .output()
    {
        return Some(output);
    }

    Command::new(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe")
        .args(["-NoProfile", "-Command", &wrapped_script])
        .output()
        .ok()
}

#[cfg(target_os = "windows")]
fn run_typeperf<const N: usize>(args: [&str; N]) -> Option<std::process::Output> {
    if let Ok(output) = Command::new("typeperf").args(args).output() {
        return Some(output);
    }

    Command::new(r"C:\Windows\System32\typeperf.exe")
        .args(args)
        .output()
        .ok()
}
