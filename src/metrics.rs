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

        let m1 = metrics.clone();
        meter
            .u64_observable_gauge("process_rss_bytes")
            .with_description("RSS memory usage in bytes")
            .with_callback(move |observer| {
                observer.observe(
                    m1.process_rss_bytes
                        .load(std::sync::atomic::Ordering::Relaxed),
                    &[],
                );
            })
            .build();

        let m4_guild = metrics.clone();
        meter
            .u64_observable_gauge("guild_active_recordings")
            .with_description("Number of active recordings per guild")
            .with_callback(move |observer| {
                for entry in m4_guild.guild_recording_metrics.iter() {
                    let guild_id = entry.key();
                    let guild_metrics = entry.value();
                    observer.observe(
                        guild_metrics
                            .active_recordings
                            .load(std::sync::atomic::Ordering::Relaxed)
                            as u64,
                        &[opentelemetry::KeyValue::new(
                            "guild_id",
                            guild_id.to_string(),
                        )],
                    );
                }
            })
            .build();

        let m_crash_guild = metrics.clone();
        meter
            .u64_observable_gauge("guild_ffmpeg_process_crashes")
            .with_description("Number of ffmpeg process crashes per guild")
            .with_callback(move |observer| {
                for entry in m_crash_guild.guild_recording_metrics.iter() {
                    let guild_id = entry.key();
                    let guild_metrics = entry.value();
                    observer.observe(
                        guild_metrics
                            .ffmpeg_process_crashes
                            .load(std::sync::atomic::Ordering::Relaxed)
                            as u64,
                        &[opentelemetry::KeyValue::new(
                            "guild_id",
                            guild_id.to_string(),
                        )],
                    );
                }
            })
            .build();

        let m2 = metrics.clone();
        meter
            .u64_observable_gauge("commands_executed")
            .with_description("Total commands executed")
            .with_callback(move |observer| {
                observer.observe(
                    m2.commands_executed
                        .load(std::sync::atomic::Ordering::Relaxed) as u64,
                    &[],
                );
            })
            .build();

        let m3 = metrics.clone();
        meter
            .u64_observable_gauge("active_voice_connections")
            .with_description("Number of active voice connections")
            .with_callback(move |observer| {
                observer.observe(
                    m3.active_voice_connections
                        .load(std::sync::atomic::Ordering::Relaxed) as u64,
                    &[],
                );
            })
            .build();

        let m4 = metrics.clone();
        meter
            .u64_observable_gauge("active_recordings")
            .with_description("Number of active recordings")
            .with_callback(move |observer| {
                observer.observe(
                    m4.active_recordings
                        .load(std::sync::atomic::Ordering::Relaxed) as u64,
                    &[],
                );
            })
            .build();

        let m5 = metrics.clone();
        meter
            .u64_observable_gauge("audio_packets_received")
            .with_description("Total audio packets received")
            .with_callback(move |observer| {
                observer.observe(
                    m5.audio_packets_received
                        .load(std::sync::atomic::Ordering::Relaxed),
                    &[],
                );
            })
            .build();

        let m6 = metrics.clone();
        meter
            .u64_observable_gauge("tokio_active_tasks")
            .with_description("Tokio runtime active tasks")
            .with_callback(move |observer| {
                observer.observe(
                    m6.tokio_active_tasks
                        .load(std::sync::atomic::Ordering::Relaxed) as u64,
                    &[],
                );
            })
            .build();

        let m7 = metrics.clone();
        meter
            .u64_observable_gauge("audio_packets_dropped")
            .with_description("Total audio packets dropped globally")
            .with_callback(move |observer| {
                observer.observe(
                    m7.audio_packets_dropped
                        .load(std::sync::atomic::Ordering::Relaxed),
                    &[],
                );
            })
            .build();

        let m8 = metrics.clone();
        meter
            .u64_observable_gauge("guild_audio_packets_dropped")
            .with_description("Number of audio packets dropped per guild")
            .with_callback(move |observer| {
                for entry in m8.guild_recording_metrics.iter() {
                    let guild_id = entry.key();
                    let guild_metrics = entry.value();
                    observer.observe(
                        guild_metrics
                            .audio_packets_dropped
                            .load(std::sync::atomic::Ordering::Relaxed),
                        &[opentelemetry::KeyValue::new(
                            "guild_id",
                            guild_id.to_string(),
                        )],
                    );
                }
            })
            .build();

        let m9 = metrics.clone();
        meter
            .u64_observable_gauge("gateway_reconnects")
            .with_description("Total Discord gateway reconnects")
            .with_callback(move |observer| {
                observer.observe(
                    m9.gateway_reconnects
                        .load(std::sync::atomic::Ordering::Relaxed) as u64,
                    &[],
                );
            })
            .build();

        let m10 = metrics.clone();
        meter
            .u64_observable_gauge("gateway_disconnects")
            .with_description("Total Discord gateway disconnects")
            .with_callback(move |observer| {
                observer.observe(
                    m10.gateway_disconnects
                        .load(std::sync::atomic::Ordering::Relaxed) as u64,
                    &[],
                );
            })
            .build();

        let m11 = metrics.clone();
        meter
            .u64_observable_gauge("driver_reconnects")
            .with_description("Total Songbird driver reconnects")
            .with_callback(move |observer| {
                observer.observe(
                    m11.driver_reconnects
                        .load(std::sync::atomic::Ordering::Relaxed) as u64,
                    &[],
                );
            })
            .build();

        let m12 = metrics.clone();
        meter
            .u64_observable_gauge("driver_disconnects")
            .with_description("Total Songbird driver disconnects")
            .with_callback(move |observer| {
                observer.observe(
                    m12.driver_disconnects
                        .load(std::sync::atomic::Ordering::Relaxed) as u64,
                    &[],
                );
            })
            .build();
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
