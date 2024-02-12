// use std::env;
#![allow(unused_variables)]
use std::{collections::HashMap, sync::Arc};

use serenity::{
    all::ApplicationId, client::Cache, http::Http, model::channel::Message, prelude::*,
    Result as SerenityResult,
};
use songbird::{driver::DecodeMode, Config, SerenityInit};
use sqlx::postgres::PgPoolOptions;
use tonic::transport::Server;
use tracing::{info, Level};
use tracing_subscriber::{prelude::*, FmtSubscriber};

use crate::{
    event_handler::Handler,
    grpc::{hello_world::jammer_server::JammerServer, MyJammer},
};

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

pub struct MpmcChannels;
impl TypeMapKey for MpmcChannels {
    type Value = Arc<
        RwLock<
            HashMap<
                u64,
                (
                    // Handler
                    tokio::sync::broadcast::Sender<i32>,
                    // Receiver
                    tokio::sync::broadcast::Sender<i32>,
                ),
            >,
        >,
    >;
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
    // install global collector configured based on RUST_LOG env var.
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

    tracing::log::info!("yak shaving completed.");
    // create relevant folders
    if !std::path::Path::new(events::voice::RECORDING_FILE_PATH).exists() {
        match tokio::fs::create_dir_all(events::voice::RECORDING_FILE_PATH).await {
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
    let token = config::TOKEN;
    let application_id = config::APPLICATION_ID;

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
        data.insert::<MpmcChannels>(Arc::new(RwLock::new(HashMap::new())));
    }

    let http = client.http.clone();
    let cache = client.cache.clone();
    let data = client.data.clone();

    let custom = Custom {
        cache,
        _http: http,
        data,
    };

    let one = tokio::spawn(async move {
        client.start().await.unwrap();
    });

    // // client.cache_and_http.cache.guild(id)
    // let two = tokio::spawn(async move {
    //     // build our application with a route
    //     let app = Router::new().route("/", get(root)).with_state(&custom);

    //     // `GET /` goes to `root`

    //     // run our app with hyper
    //     // `axum::Server` is a re-export of `hyper::Server`
    //     let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    //     tracing::debug!("listening on {}", addr);
    //     axum::Server::bind(&addr)
    //         .serve(app.into_make_service())
    //         .await
    //         .unwrap();
    // });

    let three = tokio::spawn(async move {
        let addr = "[::1]:50052".parse().unwrap();

        let jammer = MyJammer::new(custom);

        info!("GreeterServer listening on {}", addr);

        Server::builder()
            .add_service(JammerServer::new(jammer))
            .serve(addr)
            .await
    });

    let (first, third) = tokio::join!(one, three);

    // // Finally, start a single shard, and start listening to events.
    // //
    // // Shards will automatically attempt to reconnect, and will perform
    // // exponential backoff until it reconnects.
    // if let Err(why) = client.start().await {
    //     info!("Client error: {:?}", why);
    // }
}

pub fn check_msg(result: SerenityResult<Message>) {
    if let Err(why) = result {
        info!("Error sending message: {:?}", why);
    }
}
