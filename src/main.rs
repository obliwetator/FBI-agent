use std::{collections::HashMap, env, error::Error, sync::Arc};

use serenity::{all::ApplicationId, client::Cache, http::Http, prelude::*};
use songbird::{Config, SerenityInit, driver::DecodeMode};
use sqlx::postgres::PgPoolOptions;
use tonic::transport::Server;
use tracing::{error, info};

use crate::{
    event_handler::Handler,
    grpc::{MyJammer, hello_world::jammer_server::JammerServer},
};

pub mod metrics;
pub mod reaper;
pub use metrics::*;

pub mod commands;
pub mod config;
pub mod cooldown;
mod database;
pub mod event_handler;
pub mod events;
pub mod grpc;
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
    if let Some(lock) = {
        let data_write = ctx.data.read().await;
        data_write.get::<HelperStruct>().cloned()
    } {
        return lock;
    }

    error!("HelperStruct missing from typemap; recreating it");
    let lock = Arc::new(RwLock::new(HashMap::new()));
    let mut data_write = ctx.data.write().await;
    data_write.insert::<HelperStruct>(lock.clone());
    lock
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    dotenvy::dotenv().ok();

    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| "Failed to install rustls crypto provider")?;

    crate::telemetry::init_telemetry()?;

    // create relevant folders
    let path = env::current_dir()?;
    info!("{}", path.display());
    if !std::path::Path::new(events::voice_receiver::RECORDING_FILE_PATH).exists() {
        tokio::fs::create_dir_all(events::voice_receiver::RECORDING_FILE_PATH).await?;
    }

    let db_url = config::db_url()?;
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await?;

    reaper::reap_zombie_recordings(&pool).await;

    // let a = conn.exec_map("SELECT * FROM guilds WHERE id IN (:id)", db_param, | id | DBGuild { id });
    // Configure the client with your Discord bot token in the environment.
    let discord_config = config::discord_config()?;

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

    let mut client = Client::builder(discord_config.token, intents)
        .event_handler(Handler {
            database: pool.clone(),
            jam_cooldown: jam_cooldown.clone(),
        })
        .intents(intents)
        .register_songbird_from_config(songbird_config)
        .application_id(ApplicationId::new(discord_config.application_id))
        .await?;
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
        data_read.get::<BotMetricsKey>().cloned()
    };
    let Some(process_metrics) = process_metrics else {
        return Err("BotMetrics not inserted".into());
    };

    let bot = tokio::spawn(async move {
        if let Err(err) = client.start().await {
            error!("Discord client exited with error: {}", err);
        }
    });

    // Background task: sample process health every 15 seconds.
    BotMetrics::start_sysinfo_monitoring(process_metrics.clone());

    // Register OpenTelemetry metrics.
    BotMetrics::register_otel_metrics(process_metrics);

    let grpc_server = tokio::spawn(async move {
        #[cfg(debug_assertions)]
        let addr = match "[::1]:50053".parse() {
            Ok(addr) => addr,
            Err(err) => {
                error!("Invalid gRPC debug address: {}", err);
                return;
            }
        };
        #[cfg(not(debug_assertions))]
        let addr = match "[::1]:50052".parse() {
            Ok(addr) => addr,
            Err(err) => {
                error!("Invalid gRPC release address: {}", err);
                return;
            }
        };

        let jammer = MyJammer::new(custom.clone());

        info!("GreeterServer listening on {}", addr);

        Server::builder()
            .add_service(JammerServer::new(jammer.clone()))
            .add_service(crate::grpc::hello_world::dashboard_server::DashboardServer::new(jammer))
            .serve(addr)
            .await
            .unwrap_or_else(|err| error!("gRPC server failed: {}", err));
    });

    let (bot_result, grpc_result) = tokio::join!(bot, grpc_server);
    if let Err(err) = bot_result {
        error!("Discord task join error: {}", err);
    }
    if let Err(err) = grpc_result {
        error!("gRPC task join error: {}", err);
    }

    Ok(())
}
