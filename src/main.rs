// use std::env;
#![allow(unused_variables)]
use std::{collections::HashMap, sync::Arc};

use serenity::{model::channel::Message, prelude::*, CacheAndHttp, Result as SerenityResult};
use songbird::{driver::DecodeMode, Config, SerenityInit};
use sqlx::postgres::PgPoolOptions;
use tonic::transport::Server;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

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

pub struct MpmcChannels;
impl TypeMapKey for MpmcChannels {
    type Value = Arc<
        RwLock<
            HashMap<
                u64,
                (
                    tokio::sync::broadcast::Sender<i32>,
                    tokio::sync::broadcast::Receiver<i32>,
                ),
            >,
        >,
    >;
}

#[derive(Clone)]
pub struct Custom {
    cache_http: Arc<CacheAndHttp>,
    data: Arc<RwLock<TypeMap>>,
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
        .application_id(application_id)
        .await
        .expect("Err creating client");
    {
        let mut data = client.data.write().await;
        // data.insert::<MysqlConnection>(mysql_pool.clone());
        data.insert::<HasBossMusic>(HashMap::new());
        data.insert::<MpmcChannels>(Arc::new(RwLock::new(HashMap::new())));
    }

    let http_cache = client.cache_and_http.clone();
    let data = client.data.clone();

    let custom = Custom {
        cache_http: http_cache,
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

        println!("GreeterServer listening on {}", addr);

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
    //     println!("Client error: {:?}", why);
    // }
}

pub fn check_msg(result: SerenityResult<Message>) {
    if let Err(why) = result {
        println!("Error sending message: {:?}", why);
    }
}
