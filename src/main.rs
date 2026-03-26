// use std::env;
#![allow(unused_variables)]
use std::{collections::HashMap, env, sync::Arc};

use serenity::{all::ApplicationId, client::Cache, http::Http, prelude::*};
use songbird::{Config, SerenityInit, driver::DecodeMode};
use sqlx::postgres::PgPoolOptions;
use tonic::transport::Server;
use tracing::{Level, info};
use tracing_subscriber::FmtSubscriber;

use crate::{
    event_handler::Handler,
    grpc::{MyJammer, hello_world::jammer_server::JammerServer},
};
use std::sync::atomic::{AtomicU32, AtomicU64};
use std::time::Instant;

/// Per-guild recording health, mirroring the global counters on BotMetrics.
pub struct GuildRecordingMetrics {
    pub active_recordings: AtomicU32,
    pub ffmpeg_spawn_failures: AtomicU32,
    pub ffmpeg_process_crashes: AtomicU32,
    pub audio_packets_received: AtomicU64,
    pub audio_packets_dropped: AtomicU64,
}

impl GuildRecordingMetrics {
    fn new() -> Self {
        Self {
            active_recordings: AtomicU32::new(0),
            ffmpeg_spawn_failures: AtomicU32::new(0),
            ffmpeg_process_crashes: AtomicU32::new(0),
            audio_packets_received: AtomicU64::new(0),
            audio_packets_dropped: AtomicU64::new(0),
        }
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
    // Voice recording pipeline — per-guild breakdown
    pub guild_recording_metrics: dashmap::DashMap<u64, Arc<GuildRecordingMetrics>>,
    // Discord gateway health
    pub gateway_reconnects: AtomicU32,
    pub driver_reconnects: AtomicU32,
    pub voice_state_updates_received: AtomicU64,
    // Database health
    pub db_query_errors: AtomicU32,
    pub db_insert_failures: AtomicU32,
    // gRPC server health
    pub grpc_active_streams: AtomicU32,
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
            guild_recording_metrics: dashmap::DashMap::new(),
            gateway_reconnects: AtomicU32::new(0),
            driver_reconnects: AtomicU32::new(0),
            voice_state_updates_received: AtomicU64::new(0),
            db_query_errors: AtomicU32::new(0),
            db_insert_failures: AtomicU32::new(0),
            grpc_active_streams: AtomicU32::new(0),
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

// use crate::http::hello;

pub mod commands;
pub mod config;
mod database;
pub mod event_handler;
pub mod events;
pub mod grpc;
pub mod http;

pub struct HasBossMusic;
impl TypeMapKey for HasBossMusic {
    type Value = HashMap<u64, Option<String>>;
}

pub struct HelperStruct;
impl TypeMapKey for HelperStruct {
    type Value = Arc<RwLock<HashMap<u64, Option<u64>>>>;
}

#[derive(Clone)]
pub struct Custom {
    cache: Arc<Cache>,
    _http: Arc<Http>,
    data: Arc<RwLock<TypeMap>>,
}

pub async fn get_lock_read(ctx: &Context) -> Arc<RwLock<HashMap<u64, Option<u64>>>> {
    let lock = {
        let data_write = ctx.data.read().await;
        data_write
            .get::<HelperStruct>()
            .expect("Expected Helper Struct")
            .clone()
    };

    lock
}

#[tokio::main]
async fn main() {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");
    // install global collector configured based on RUST_LOG env var.
    unsafe { env::set_var("RUST_BACKTRACE", "1") };
    let subscriber = FmtSubscriber::builder()
        // .with_thread_names(true)
        // .with_file(true)
        // .with_target(true)
        // .with_line_number(true)
        // all spans/events with a level higher than TRACE (e.g, debug, info, warn, etc.)
        // will be written to stdout.
        .with_max_level(Level::INFO)
        .pretty()
        // completes the builder.
        .finish();

    // tracing_subscriber::registry()
    //     // add the console layer to the subscriber
    //     .with(
    //         console_subscriber::ConsoleLayer::builder()
    //             .server_addr(([127, 0, 0, 1], 5555))
    //             .retention(Duration::from_secs(180))
    //             .spawn(),
    //     )
    //     // add other layers...
    //     .with(
    //         tracing_subscriber::fmt::layer()
    //             .pretty()
    //             .with_filter(tracing_subscriber::filter::LevelFilter::INFO),
    //     )
    //     // .with(...)
    //     .init();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
    // create relevant folders
    if !std::path::Path::new(events::voice_receiver::RECORDING_FILE_PATH).exists() {
        match tokio::fs::create_dir_all(events::voice_receiver::RECORDING_FILE_PATH).await {
            Ok(_) => {}
            Err(err) => {
                panic!("cannot create path: {}", err)
            }
        };
    }

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(config::DB_URL)
        .await
        .expect("cannot connect to database");

    // let a = conn.exec_map("SELECT * FROM guilds WHERE id IN (:id)", db_param, | id | DBGuild { id });
    // Configure the client with your Discord bot token in the environment.
    #[cfg(debug_assertions)]
    let (token, application_id) = (config::TOKEN_DEBUG, config::APPLICATION_ID_DEBUG);

    #[cfg(not(debug_assertions))]
    let (token, application_id) = (config::TOKEN_RELEASE, config::APPLICATION_ID_RELEASE);

    // Here, we need to configure Songbird to decode all incoming voice packets.
    // If you want, you can do this on a per-call basis---here, we need it to
    // read the audio data that other people are sending us!
    let songbird_config = Config::default().decode_mode(DecodeMode::Decode);

    let intents = GatewayIntents::all();
    // Create a new instance of the Client, logging in as a bot. This will
    // automatically prepend your bot token with "Bot ", which is a requirement
    // by Discord for bot users.
    let mut client = Client::builder(token, intents)
        .event_handler(Handler { database: pool })
        .intents(intents)
        .register_songbird_from_config(songbird_config)
        .application_id(ApplicationId::new(application_id))
        .await
        .expect("Err creating client");
    {
        let mut data = client.data.write().await;
        // data.insert::<MysqlConnection>(mysql_pool.clone());
        data.insert::<HelperStruct>(Arc::new(RwLock::new(HashMap::new())));
        data.insert::<HasBossMusic>(HashMap::new());
        data.insert::<BotMetricsKey>(Arc::new(BotMetrics::default()));
    }

    let http = client.http.clone();
    let cache = client.cache.clone();
    let data = client.data.clone();

    let custom = Custom {
        cache,
        _http: http,
        data,
    };

    // Grab the metrics Arc before moving `client` into the spawn below.
    let process_metrics = {
        let data_read = client.data.read().await;
        data_read.get::<BotMetricsKey>().cloned().expect("BotMetrics not inserted")
    };

    let one = tokio::spawn(async move {
        client.start().await.unwrap();
    });

    // Background task: sample process health every 15 seconds.
    tokio::spawn(async move {
        loop {
            // RSS from /proc/self/status (VmRSS line, value in kB)
            if let Ok(status) = tokio::fs::read_to_string("/proc/self/status").await {
                for line in status.lines() {
                    if let Some(rest) = line.strip_prefix("VmRSS:") {
                        let kb: u64 = rest.split_whitespace().next()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0);
                        process_metrics.process_rss_bytes.store(kb * 1024, std::sync::atomic::Ordering::Relaxed);
                        break;
                    }
                }
            }

            // Open file descriptors: count entries in /proc/self/fd
            if let Ok(mut dir) = tokio::fs::read_dir("/proc/self/fd").await {
                let mut count: u32 = 0;
                while dir.next_entry().await.ok().flatten().is_some() {
                    count += 1;
                }
                process_metrics.process_open_fds.store(count, std::sync::atomic::Ordering::Relaxed);
            }

            // Tokio runtime task count (requires tokio_unstable)
            let task_count = tokio::runtime::Handle::current()
                .metrics()
                .active_tasks_count() as u32;
            process_metrics.tokio_active_tasks.store(task_count, std::sync::atomic::Ordering::Relaxed);

            tokio::time::sleep(std::time::Duration::from_secs(15)).await;
        }
    });

    let three = tokio::spawn(async move {
        let addr = "[::1]:50052".parse().unwrap();

        let jammer = MyJammer::new(custom.clone());

        info!("GreeterServer listening on {}", addr);

        Server::builder()
            .add_service(JammerServer::new(jammer.clone()))
            .add_service(crate::grpc::hello_world::dashboard_server::DashboardServer::new(jammer))
            .serve(addr)
            .await
    });

    let (first, third) = tokio::join!(one, three);
}
