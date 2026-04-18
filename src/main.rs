// use std::env;
#![allow(unused_variables)]
use std::{collections::HashMap, env, sync::Arc};

use serenity::{all::ApplicationId, client::Cache, http::Http, prelude::*};
use songbird::{Config, SerenityInit, driver::DecodeMode};
use sqlx::postgres::PgPoolOptions;
use tonic::transport::Server;
use tracing::info;

use crate::{
    event_handler::Handler,
    grpc::{MyJammer, hello_world::jammer_server::JammerServer},
};

pub mod metrics;
pub use metrics::*;

// use crate::http::hello;

pub mod commands;
pub mod config;
pub mod cooldown;
mod database;
pub mod event_handler;
pub mod events;
pub mod grpc;
pub mod http;
pub mod telemetry;

#[cfg(test)]
mod tests;

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
    pub pool: sqlx::Pool<sqlx::Postgres>,
    pub jam_cooldown: crate::cooldown::JamCooldown,
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

    crate::telemetry::init_telemetry();

    // create relevant folders
    let path = env::current_dir().unwrap();
    info!("{}", path.display());
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
    let songbird_config = Config::default()
        .decode_mode(DecodeMode::Decode(songbird::driver::DecodeConfig::default()));

    let intents = GatewayIntents::all();
    // Create a new instance of the Client, logging in as a bot. This will
    // automatically prepend your bot token with "Bot ", which is a requirement
    // by Discord for bot users.
    let jam_cooldown = crate::cooldown::JamCooldown::new();

    let mut client = Client::builder(token, intents)
        .event_handler(Handler {
            database: pool.clone(),
            jam_cooldown: jam_cooldown.clone(),
        })
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
        pool: pool.clone(),
        jam_cooldown: jam_cooldown.clone(),
    };

    // Grab the metrics Arc before moving `client` into the spawn below.
    let process_metrics = {
        let data_read = client.data.read().await;
        data_read
            .get::<BotMetricsKey>()
            .cloned()
            .expect("BotMetrics not inserted")
    };

    let one = tokio::spawn(async move {
        client.start().await.unwrap();
    });

    // Background task: sample process health every 15 seconds.
    BotMetrics::start_sysinfo_monitoring(process_metrics.clone());

    // Register OpenTelemetry metrics.
    BotMetrics::register_otel_metrics(process_metrics);

    let three = tokio::spawn(async move {
        #[cfg(debug_assertions)]
        let addr = "[::1]:50053".parse().unwrap();
        #[cfg(not(debug_assertions))]
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
