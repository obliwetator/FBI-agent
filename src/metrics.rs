use serenity::prelude::TypeMapKey;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU32, AtomicU64};
use std::time::Instant;

/// Per-guild recording health, mirroring the global counters on BotMetrics.
pub struct GuildRecordingMetrics {
    pub active_recordings: AtomicU32,
    pub ffmpeg_spawn_failures: AtomicU32,
    pub ffmpeg_process_crashes: AtomicU32,
    pub audio_packets_received: AtomicU64,
    pub audio_packets_dropped: AtomicU64,
    pub last_voice_packet_time: AtomicI64,
}

impl GuildRecordingMetrics {
    pub fn new() -> Self {
        Self {
            active_recordings: AtomicU32::new(0),
            ffmpeg_spawn_failures: AtomicU32::new(0),
            ffmpeg_process_crashes: AtomicU32::new(0),
            audio_packets_received: AtomicU64::new(0),
            audio_packets_dropped: AtomicU64::new(0),
            last_voice_packet_time: AtomicI64::new(0),
        }
    }
}

impl Default for GuildRecordingMetrics {
    fn default() -> Self {
        Self::new()
    }
}

pub struct BotMetrics {
    pub start_time: Instant,
    pub commands_executed: AtomicU32,
    pub active_voice_connections: AtomicU32,
    pub update_tx: tokio::sync::watch::Sender<()>,
    pub voice_update_tx: tokio::sync::watch::Sender<()>,
    pub user_start_times: dashmap::DashMap<u64, i64>,
    // Voice recording pipeline — global aggregates
    pub active_recordings: AtomicU32,
    pub ffmpeg_spawn_failures: AtomicU32,
    pub ffmpeg_process_crashes: AtomicU32,
    pub audio_packets_received: AtomicU64,
    pub audio_packets_dropped: AtomicU64,
    pub last_voice_packet_time: AtomicI64,
    // Voice recording pipeline — per-guild breakdown
    pub guild_recording_metrics: dashmap::DashMap<u64, Arc<GuildRecordingMetrics>>,
    // Discord gateway health
    pub gateway_reconnects: AtomicU32,
    pub gateway_disconnects: AtomicU32,
    pub driver_reconnects: AtomicU32,
    pub driver_disconnects: AtomicU32,
    pub voice_state_updates_received: AtomicU64,
    // Database health
    pub db_query_errors: AtomicU32,
    pub db_insert_failures: AtomicU32,
    // gRPC server health
    pub grpc_active_streams: AtomicU32,
    // Bot activity
    pub messages_received: AtomicU32,
    // Process health (sampled every 15s)
    pub process_rss_bytes: AtomicU64,
    pub process_open_fds: AtomicU32,
    pub tokio_active_tasks: AtomicU32,
}

impl BotMetrics {
    /// Returns the metrics entry for `guild_id`, creating it on first access.
    pub fn guild_metrics(&self, guild_id: u64) -> Arc<GuildRecordingMetrics> {
        self.guild_recording_metrics
            .entry(guild_id)
            .or_insert_with(|| Arc::new(GuildRecordingMetrics::new()))
            .clone()
    }

    /// Registers standard BotMetrics counters to OpenTelemetry as Observable Gauges.
    pub fn register_otel_metrics(metrics: Arc<Self>) {
        let meter = opentelemetry::global::meter(crate::config::SERVICE_NAME);

        // ── simple gauges (atomic → u64) ──
        macro_rules! gauge {
            ($name:literal, $desc:literal, $field:ident) => {
                gauge!($name, $desc, $field, |v| v as u64);
            };
            ($name:literal, $desc:literal, $field:ident, $map:expr) => {
                {
                    let m = metrics.clone();
                    meter
                        .u64_observable_gauge($name)
                        .with_description($desc)
                        .with_callback(move |observer| {
                            observer.observe(
                                $map(
                                    m.$field
                                        .load(std::sync::atomic::Ordering::Relaxed),
                                ),
                                &[],
                            );
                        })
                        .build();
                }
            };
        }

        // ── per-guild gauges (GuildRecordingMetrics field → u64) ──
        macro_rules! guild_gauge {
            ($name:literal, $desc:literal, $field:ident) => {
                guild_gauge!($name, $desc, $field, |v| v as u64);
            };
            ($name:literal, $desc:literal, $field:ident, $map:expr) => {
                {
                    let m = metrics.clone();
                    meter
                        .u64_observable_gauge($name)
                        .with_description($desc)
                        .with_callback(move |observer| {
                            for entry in m.guild_recording_metrics.iter() {
                                let guild_id = entry.key();
                                let guild_metrics = entry.value();
                                observer.observe(
                                    $map(
                                        guild_metrics
                                            .$field
                                            .load(std::sync::atomic::Ordering::Relaxed),
                                    ),
                                    &[opentelemetry::KeyValue::new(
                                        "guild_id",
                                        guild_id.to_string(),
                                    )],
                                );
                            }
                        })
                        .build();
                }
            };
        }

        gauge!("process_rss_bytes", "RSS memory usage in bytes", process_rss_bytes, std::convert::identity);
        guild_gauge!("guild_active_recordings", "Number of active recordings per guild", active_recordings);
        guild_gauge!("guild_ffmpeg_process_crashes", "Number of ffmpeg process crashes per guild", ffmpeg_process_crashes);
        gauge!("commands_executed", "Total commands executed", commands_executed);
        gauge!("active_voice_connections", "Number of active voice connections", active_voice_connections);
        gauge!("active_recordings", "Number of active recordings", active_recordings);
        gauge!("audio_packets_received", "Total audio packets received", audio_packets_received, std::convert::identity);
        gauge!("tokio_active_tasks", "Tokio runtime active tasks", tokio_active_tasks);
        gauge!("audio_packets_dropped", "Total audio packets dropped globally", audio_packets_dropped, std::convert::identity);
        guild_gauge!("guild_audio_packets_dropped", "Number of audio packets dropped per guild", audio_packets_dropped, std::convert::identity);
        gauge!("gateway_reconnects", "Total Discord gateway reconnects", gateway_reconnects);
        gauge!("gateway_disconnects", "Total Discord gateway disconnects", gateway_disconnects);
        gauge!("driver_reconnects", "Total Songbird driver reconnects", driver_reconnects);
        gauge!("driver_disconnects", "Total Songbird driver disconnects", driver_disconnects);
    }

    /// Starts a background task to monitor process health (memory, FDs, active tasks).
    pub fn start_sysinfo_monitoring(metrics: Arc<Self>) {
        tokio::spawn(async move {
            let mut sys = sysinfo::System::new();
            let pid = sysinfo::get_current_pid().unwrap_or(sysinfo::Pid::from_u32(0));

            loop {
                // Memory (RSS) from sysinfo
                sys.refresh_processes_specifics(
                    sysinfo::ProcessesToUpdate::Some(&[pid]),
                    true,
                    sysinfo::ProcessRefreshKind::nothing().with_memory(),
                );
                if let Some(process) = sys.process(pid) {
                    metrics
                        .process_rss_bytes
                        .store(process.memory(), std::sync::atomic::Ordering::Relaxed);
                }

                // Open file descriptors: count entries in /proc/self/fd on linux
                #[cfg(target_os = "linux")]
                if let Ok(mut dir) = tokio::fs::read_dir("/proc/self/fd").await {
                    let mut count: u32 = 0;
                    while dir.next_entry().await.ok().flatten().is_some() {
                        count += 1;
                    }
                    metrics
                        .process_open_fds
                        .store(count, std::sync::atomic::Ordering::Relaxed);
                }

                // Tokio runtime task count (requires tokio_unstable)
                let task_count = tokio::runtime::Handle::current()
                    .metrics()
                    .num_alive_tasks() as u32;
                metrics
                    .tokio_active_tasks
                    .store(task_count, std::sync::atomic::Ordering::Relaxed);

                tokio::time::sleep(std::time::Duration::from_secs(15)).await;
            }
        });
    }
}

impl Default for BotMetrics {
    fn default() -> Self {
        let (tx, _) = tokio::sync::watch::channel(());
        let (voice_tx, _) = tokio::sync::watch::channel(());
        Self {
            start_time: Instant::now(),
            commands_executed: AtomicU32::new(0),
            active_voice_connections: AtomicU32::new(0),
            update_tx: tx,
            voice_update_tx: voice_tx,
            user_start_times: dashmap::DashMap::new(),
            active_recordings: AtomicU32::new(0),
            ffmpeg_spawn_failures: AtomicU32::new(0),
            ffmpeg_process_crashes: AtomicU32::new(0),
            audio_packets_received: AtomicU64::new(0),
            audio_packets_dropped: AtomicU64::new(0),
            last_voice_packet_time: AtomicI64::new(0),
            guild_recording_metrics: dashmap::DashMap::new(),
            gateway_reconnects: AtomicU32::new(0),
            gateway_disconnects: AtomicU32::new(0),
            driver_reconnects: AtomicU32::new(0),
            driver_disconnects: AtomicU32::new(0),
            voice_state_updates_received: AtomicU64::new(0),
            db_query_errors: AtomicU32::new(0),
            db_insert_failures: AtomicU32::new(0),
            grpc_active_streams: AtomicU32::new(0),
            messages_received: AtomicU32::new(0),
            process_rss_bytes: AtomicU64::new(0),
            process_open_fds: AtomicU32::new(0),
            tokio_active_tasks: AtomicU32::new(0),
        }
    }
}

pub struct BotMetricsKey;
impl TypeMapKey for BotMetricsKey {
    type Value = Arc<BotMetrics>;
}
