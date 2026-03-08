use std::convert::TryInto;
use std::sync::Arc;

use serenity::model::prelude::GuildId;
use tonic::{Request, Response, Status};

use hello_world::dashboard_server::Dashboard;
use hello_world::jammer_server::Jammer;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use tracing::info;

use crate::{BotMetricsKey, Custom};

use serenity::prelude::{RwLock, TypeMap};
use songbird::SongbirdKey;

use crate::events::voice_receiver::CLIPS_FILE_PATH;

use self::hello_world::jam_response::JamResponseEnum;
use self::hello_world::{ActionResponse, Empty, GuildRequest, MetricsResponse};
use self::hello_world::{JamData, JamResponse};

pub mod hello_world {
    tonic::include_proto!("helloworld");
}

#[derive(Clone)]
pub struct MyJammer {
    data_cache: Custom,
}

impl MyJammer {
    pub fn new(data_cache: Custom) -> Self {
        Self { data_cache }
    }
}

#[tonic::async_trait]
impl Dashboard for MyJammer {
    type GetMetricsStream = ReceiverStream<Result<MetricsResponse, Status>>;

    async fn get_metrics(
        &self,
        request: Request<Empty>,
    ) -> Result<Response<Self::GetMetricsStream>, Status> {
        let (tx, rx) = mpsc::channel(4);
        let data_cache = self.data_cache.clone();

        tokio::spawn(async move {
            loop {
                let data_guard = data_cache.data.read().await;

                let mut total_guilds = 0;
                let mut commands_executed = 0;
                let mut active_voice_connections = 0;
                let mut uptime_seconds = 0;

                if let Some(metrics) = data_guard.get::<BotMetricsKey>() {
                    commands_executed = metrics.commands_executed.load(Ordering::Relaxed) as i32;
                    active_voice_connections =
                        metrics.active_voice_connections.load(Ordering::Relaxed) as i32;
                    uptime_seconds = metrics.start_time.elapsed().as_secs() as i64;
                }

                if let Some(_songbird) = data_guard.get::<SongbirdKey>() {
                    total_guilds = data_cache.cache.guilds().len() as i32;
                }

                drop(data_guard);

                let response = MetricsResponse {
                    total_guilds,
                    active_voice_connections,
                    uptime_seconds,
                    commands_executed,
                };

                if tx.send(Ok(response)).await.is_err() {
                    break;
                }

                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    type DashboardStreamStream = ReceiverStream<Result<hello_world::DashboardEvent, Status>>;

    async fn dashboard_stream(
        &self,
        request: Request<tonic::Streaming<hello_world::ClientMessage>>,
    ) -> Result<Response<Self::DashboardStreamStream>, Status> {
        let mut stream = request.into_inner();
        let (tx, rx) = mpsc::channel(100);
        let data_cache = self.data_cache.clone();

        let (topic_tx, mut topic_rx) = tokio::sync::watch::channel(String::new());

        tokio::spawn(async move {
            // Wait for subscription messages
            while let Ok(Some(msg)) = stream.message().await {
                if msg.action == "subscribe" {
                    let _ = topic_tx.send(msg.topic.clone());
                    info!("Client subscribed to topic: {}", msg.topic);
                } else if msg.action == "unsubscribe" {
                    info!("Client unsubscribed from topic: {}", msg.topic);
                    let _ = topic_tx.send(String::new());
                }
            }
        });

        let tx_clone = tx.clone();
        tokio::spawn(async move {
            let (mut global_rx, mut voice_rx) = {
                let data_guard = data_cache.data.read().await;
                if let Some(metrics) = data_guard.get::<BotMetricsKey>() {
                    (
                        metrics.update_tx.subscribe(),
                        metrics.voice_update_tx.subscribe(),
                    )
                } else {
                    return;
                }
            };

            // initial push
            let _ = global_rx.borrow_and_update();
            let _ = voice_rx.borrow_and_update();

            loop {
                let topic = topic_rx.borrow().clone();

                // Wait for either the topic to change, or a relevant metric to update
                tokio::select! {
                    topic_changed = topic_rx.changed() => {
                        if topic_changed.is_err() {
                            break;
                        }
                    }
                    global_changed = global_rx.changed(), if topic == "global" => {
                        if global_changed.is_err() {
                            break;
                        }
                    }
                    voice_changed = voice_rx.changed(), if topic.starts_with("guild_voice:") => {
                        if voice_changed.is_err() {
                            break;
                        }
                    }
                }

                let topic = topic_rx.borrow().clone();

                if topic == "global" {
                    let data_guard = data_cache.data.read().await;

                    let mut total_guilds = 0;
                    let mut commands_executed = 0;
                    let mut active_voice_connections = 0;
                    let mut start_time_secs = 0;

                    if let Some(metrics) = data_guard.get::<BotMetricsKey>() {
                        commands_executed =
                            metrics.commands_executed.load(Ordering::Relaxed) as i32;
                        active_voice_connections =
                            metrics.active_voice_connections.load(Ordering::Relaxed) as i32;
                        start_time_secs = metrics.start_time.elapsed().as_secs() as i64;
                    }

                    let mut guilds = Vec::new();

                    if let Some(_songbird) = data_guard.get::<SongbirdKey>() {
                        total_guilds = data_cache.cache.guilds().len() as i32;
                        for guild_id in data_cache.cache.guilds() {
                            if let Some(guild) = data_cache.cache.guild(guild_id) {
                                guilds.push(serde_json::json!({
                                    "id": guild_id.get().to_string(),
                                    "name": guild.name.clone()
                                }));
                            }
                        }
                    }

                    drop(data_guard);

                    let json_payload = serde_json::json!({
                        "total_guilds": total_guilds,
                        "active_voice_connections": active_voice_connections,
                        "uptime_seconds": start_time_secs,
                        "commands_executed": commands_executed,
                        "guilds": guilds
                    });

                    let event = hello_world::DashboardEvent {
                        event_type: "METRICS_UPDATE".to_string(),
                        json_payload: json_payload.to_string(),
                    };

                    if tx_clone.send(Ok(event)).await.is_err() {
                        break;
                    }
                } else if topic.starts_with("guild_voice:") {
                    if let Some(guild_id_str) = topic.strip_prefix("guild_voice:") {
                        if let Ok(guild_id) = guild_id_str.parse::<u64>() {
                            let mut voice_states_json = Vec::new();
                            let mut channel_start_times_json = serde_json::Map::new();

                            let data_guard = data_cache.data.read().await;
                            let channel_start_times: Option<dashmap::DashMap<u64, i64>> =
                                if let Some(metrics) = data_guard.get::<BotMetricsKey>() {
                                    Some(metrics.channel_start_times.clone())
                                } else {
                                    None
                                };
                            drop(data_guard);

                            if let Some(guild) = data_cache.cache.guild(GuildId::new(guild_id)) {
                                for (user_id, voice_state) in &guild.voice_states {
                                    if let Some(channel_id) = voice_state.channel_id {
                                        voice_states_json.push(serde_json::json!({
                                            "user_id": user_id.get().to_string(),
                                            "channel_id": channel_id.get().to_string(),
                                            "mute": voice_state.mute,
                                            "deaf": voice_state.deaf,
                                            "self_mute": voice_state.self_mute,
                                            "self_deaf": voice_state.self_deaf,
                                            "self_stream": voice_state.self_stream.unwrap_or(false),
                                            "self_video": voice_state.self_video,
                                            "suppress": voice_state.suppress,
                                        }));

                                        if let Some(times) = &channel_start_times {
                                            if let Some(time) = times.get(&channel_id.get()) {
                                                channel_start_times_json.insert(
                                                    channel_id.get().to_string(),
                                                    serde_json::Value::Number((*time).into()),
                                                );
                                            }
                                        }
                                    }
                                }
                            }

                            let event = hello_world::DashboardEvent {
                                event_type: "GUILD_VOICE_UPDATE".to_string(),
                                json_payload: serde_json::json!({
                                    "voice_states": voice_states_json,
                                    "channel_start_times": channel_start_times_json
                                })
                                .to_string(),
                            };

                            if tx_clone.send(Ok(event)).await.is_err() {
                                break;
                            }
                        }
                    }
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn disconnect_voice(
        &self,
        request: Request<GuildRequest>,
    ) -> Result<Response<ActionResponse>, Status> {
        let req = request.into_inner();
        let guild_id = GuildId::new(req.guild_id.try_into().unwrap());

        let data_guard = self.data_cache.data.read().await;
        if let Some(songbird) = data_guard.get::<SongbirdKey>() {
            let manager = songbird.clone();

            if manager.get(guild_id).is_some() {
                if let Err(e) = manager.remove(guild_id).await {
                    return Ok(Response::new(ActionResponse {
                        success: false,
                        message: format!("Failed to disconnect: {}", e),
                    }));
                }

                return Ok(Response::new(ActionResponse {
                    success: true,
                    message: "Successfully disconnected from voice".to_string(),
                }));
            }
        }

        Ok(Response::new(ActionResponse {
            success: false,
            message: "Bot is not in a voice channel in this guild".to_string(),
        }))
    }
}

#[tonic::async_trait]
impl Jammer for MyJammer {
    async fn jam_it(&self, request: Request<JamData>) -> Result<Response<JamResponse>, Status> {
        info!("Got a request from {:?}", request);

        let data = request.into_inner();

        let guild = match self
            .data_cache
            .cache
            .guild(GuildId::new(data.guild_id.try_into().unwrap()))
        {
            Some(ok) => ok.to_owned(),
            None => {
                let reply = JamResponse {
                    resp: JamResponseEnum::Unkown.into(),
                };
                return Ok(Response::new(reply));
                // return (StatusCode::OK, Json(json!({ "code": Ras::Unkown })));
            }
        };

        let channels = &guild.channels;

        for (key, guild_channel) in channels {
            match guild_channel.kind {
                serenity::model::prelude::ChannelType::Text => {}
                // serenity::model::prelude::ChannelType::Private => todo!(),
                serenity::model::prelude::ChannelType::Voice => {
                    let res = guild_channel.members(&self.data_cache.cache).unwrap();
                    for (index, member) in res.iter().enumerate() {
                        if member.user.id == 877617434029350972 {
                            info!(
                                "Ladies and gentlemen, We got him in c {}",
                                guild_channel.id.get()
                            );

                            handle_play_audio_to_channel(
                                data.guild_id,
                                &data.clip_name,
                                self.data_cache.data.clone(),
                            )
                            .await;

                            let reply = JamResponse {
                                resp: JamResponseEnum::Ok.into(),
                            };
                            return Ok(Response::new(reply));
                        }
                    }
                }
                _ => {}
            }
        }

        let reply = JamResponse {
            resp: JamResponseEnum::NotPressent.into(),
        };
        return Ok(Response::new(reply));
    }
}

async fn handle_play_audio_to_channel(id: i64, clip_name: &str, data: Arc<RwLock<TypeMap>>) {
    let manager = {
        let data_guard = data.read().await;
        data_guard.get::<SongbirdKey>().cloned().unwrap()
    };

    // TODO: This does not return and error if the wrong file path is given?
    info!(" Clips to play : {}/{}.ogg", CLIPS_FILE_PATH, clip_name);
    let result = songbird::input::File::new(format!("{}/{}.ogg", CLIPS_FILE_PATH, clip_name));

    let input = songbird::input::Input::from(result);
    let handler = manager.get(GuildId::new(id.try_into().unwrap())).unwrap();
    let driver = &mut handler.lock().await;
    let handler_lock = driver.enqueue_input(input);
    let _ = handler_lock.await.set_volume(0.5);
}
