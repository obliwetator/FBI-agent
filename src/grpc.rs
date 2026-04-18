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

use crate::config::APPLICATION_ID_RELEASE;
use crate::{BotMetricsKey, Custom};

use serenity::prelude::{RwLock, TypeMap};
use songbird::SongbirdKey;

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

        // Grab the metrics Arc once before spawning so we can track stream lifetime.
        let stream_metrics = {
            let data_guard = data_cache.data.read().await;
            data_guard.get::<BotMetricsKey>().cloned()
        };
        if let Some(m) = &stream_metrics {
            m.grpc_active_streams.fetch_add(1, Ordering::Relaxed);
        }

        tokio::spawn(async move {
            loop {
                let data_guard = data_cache.data.read().await;

                let mut total_guilds = 0;
                let mut commands_executed = 0;
                let mut active_voice_connections = 0;
                let mut uptime_seconds = 0;

                let mut active_recordings = 0;
                let mut ffmpeg_spawn_failures = 0;
                let mut ffmpeg_process_crashes = 0;
                let mut audio_packets_received = 0i64;
                let mut audio_packets_dropped = 0i64;
                let mut gateway_reconnects = 0;
                let mut driver_reconnects = 0;
                let mut voice_state_updates_received = 0i64;
                let mut db_query_errors = 0;
                let mut db_insert_failures = 0;
                let mut grpc_active_streams = 0;
                let mut process_rss_bytes = 0i64;
                let mut process_open_fds = 0;
                let mut tokio_active_tasks = 0;
                let mut messages_received = 0;
                let mut last_voice_packet_time = 0;

                if let Some(metrics) = data_guard.get::<BotMetricsKey>() {
                    commands_executed = metrics.commands_executed.load(Ordering::Relaxed) as i32;
                    active_voice_connections =
                        metrics.active_voice_connections.load(Ordering::Relaxed) as i32;
                    uptime_seconds = metrics.start_time.elapsed().as_secs() as i64;
                    active_recordings = metrics.active_recordings.load(Ordering::Relaxed) as i32;
                    ffmpeg_spawn_failures =
                        metrics.ffmpeg_spawn_failures.load(Ordering::Relaxed) as i32;
                    ffmpeg_process_crashes =
                        metrics.ffmpeg_process_crashes.load(Ordering::Relaxed) as i32;
                    audio_packets_received =
                        metrics.audio_packets_received.load(Ordering::Relaxed) as i64;
                    audio_packets_dropped =
                        metrics.audio_packets_dropped.load(Ordering::Relaxed) as i64;
                    gateway_reconnects = metrics.gateway_reconnects.load(Ordering::Relaxed) as i32;
                    driver_reconnects = metrics.driver_reconnects.load(Ordering::Relaxed) as i32;
                    voice_state_updates_received =
                        metrics.voice_state_updates_received.load(Ordering::Relaxed) as i64;
                    db_query_errors = metrics.db_query_errors.load(Ordering::Relaxed) as i32;
                    db_insert_failures = metrics.db_insert_failures.load(Ordering::Relaxed) as i32;
                    grpc_active_streams =
                        metrics.grpc_active_streams.load(Ordering::Relaxed) as i32;
                    process_rss_bytes = metrics.process_rss_bytes.load(Ordering::Relaxed) as i64;
                    process_open_fds = metrics.process_open_fds.load(Ordering::Relaxed) as i32;
                    tokio_active_tasks = metrics.tokio_active_tasks.load(Ordering::Relaxed) as i32;
                    messages_received = metrics.messages_received.load(Ordering::Relaxed) as i32;
                    last_voice_packet_time = metrics.last_voice_packet_time.load(Ordering::Relaxed);
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
                    active_recordings,
                    ffmpeg_spawn_failures,
                    ffmpeg_process_crashes,
                    audio_packets_received,
                    audio_packets_dropped,
                    gateway_reconnects,
                    driver_reconnects,
                    voice_state_updates_received,
                    db_query_errors,
                    db_insert_failures,
                    grpc_active_streams,
                    process_rss_bytes,
                    process_open_fds,
                    tokio_active_tasks,
                    messages_received,
                    last_voice_packet_time,
                };

                if tx.send(Ok(response)).await.is_err() {
                    break;
                }

                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            if let Some(m) = &stream_metrics {
                m.grpc_active_streams.fetch_sub(1, Ordering::Relaxed);
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
        let dashboard_stream_metrics = {
            let data_guard = data_cache.data.read().await;
            data_guard.get::<BotMetricsKey>().cloned()
        };
        if let Some(m) = &dashboard_stream_metrics {
            m.grpc_active_streams.fetch_add(1, Ordering::Relaxed);
        }

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

            // Mark initial watch values as seen so changed() only fires on actual updates.
            let _ = global_rx.borrow_and_update();
            let _ = voice_rx.borrow_and_update();

            loop {
                // Read topic BEFORE sending so we always push the current state on
                // every iteration — including on startup when the topic may already be
                // set (race between Task-1 processing the subscribe and this task
                // starting its loop).
                let topic = topic_rx.borrow().clone();

                if topic == "global" {
                    let data_guard = data_cache.data.read().await;

                    let mut total_guilds = 0;
                    let mut commands_executed = 0;
                    let mut active_voice_connections = 0;
                    let mut start_time_secs = 0;
                    let mut active_recordings_ds = 0i32;
                    let mut ffmpeg_spawn_failures_ds = 0i32;
                    let mut ffmpeg_process_crashes_ds = 0i32;
                    let mut audio_packets_received_ds = 0i64;
                    let mut audio_packets_dropped_ds = 0i64;
                    let mut gateway_reconnects_ds = 0i32;
                    let mut driver_reconnects_ds = 0i32;
                    let mut voice_state_updates_ds = 0i64;
                    let mut db_query_errors_ds = 0i32;
                    let mut db_insert_failures_ds = 0i32;
                    let mut grpc_active_streams_ds = 0i32;
                    let mut process_rss_bytes_ds = 0i64;
                    let mut process_open_fds_ds = 0i32;
                    let mut tokio_active_tasks_ds = 0i32;
                    let mut messages_received_ds = 0i32;
                    let mut last_voice_packet_time_ds = 0i64;

                    if let Some(metrics) = data_guard.get::<BotMetricsKey>() {
                        commands_executed =
                            metrics.commands_executed.load(Ordering::Relaxed) as i32;
                        active_voice_connections =
                            metrics.active_voice_connections.load(Ordering::Relaxed) as i32;
                        start_time_secs = metrics.start_time.elapsed().as_secs() as i64;
                        active_recordings_ds =
                            metrics.active_recordings.load(Ordering::Relaxed) as i32;
                        ffmpeg_spawn_failures_ds =
                            metrics.ffmpeg_spawn_failures.load(Ordering::Relaxed) as i32;
                        ffmpeg_process_crashes_ds =
                            metrics.ffmpeg_process_crashes.load(Ordering::Relaxed) as i32;
                        audio_packets_received_ds =
                            metrics.audio_packets_received.load(Ordering::Relaxed) as i64;
                        audio_packets_dropped_ds =
                            metrics.audio_packets_dropped.load(Ordering::Relaxed) as i64;
                        gateway_reconnects_ds =
                            metrics.gateway_reconnects.load(Ordering::Relaxed) as i32;
                        driver_reconnects_ds =
                            metrics.driver_reconnects.load(Ordering::Relaxed) as i32;
                        voice_state_updates_ds =
                            metrics.voice_state_updates_received.load(Ordering::Relaxed) as i64;
                        db_query_errors_ds = metrics.db_query_errors.load(Ordering::Relaxed) as i32;
                        db_insert_failures_ds =
                            metrics.db_insert_failures.load(Ordering::Relaxed) as i32;
                        grpc_active_streams_ds =
                            metrics.grpc_active_streams.load(Ordering::Relaxed) as i32;
                        process_rss_bytes_ds =
                            metrics.process_rss_bytes.load(Ordering::Relaxed) as i64;
                        process_open_fds_ds =
                            metrics.process_open_fds.load(Ordering::Relaxed) as i32;
                        tokio_active_tasks_ds =
                            metrics.tokio_active_tasks.load(Ordering::Relaxed) as i32;
                        messages_received_ds =
                            metrics.messages_received.load(Ordering::Relaxed) as i32;
                        last_voice_packet_time_ds =
                            metrics.last_voice_packet_time.load(Ordering::Relaxed);
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
                        "guilds": guilds,
                        "active_recordings": active_recordings_ds,
                        "ffmpeg_spawn_failures": ffmpeg_spawn_failures_ds,
                        "ffmpeg_process_crashes": ffmpeg_process_crashes_ds,
                        "audio_packets_received": audio_packets_received_ds,
                        "audio_packets_dropped": audio_packets_dropped_ds,
                        "gateway_reconnects": gateway_reconnects_ds,
                        "driver_reconnects": driver_reconnects_ds,
                        "voice_state_updates_received": voice_state_updates_ds,
                        "db_query_errors": db_query_errors_ds,
                        "db_insert_failures": db_insert_failures_ds,
                        "grpc_active_streams": grpc_active_streams_ds,
                        "process_rss_bytes": process_rss_bytes_ds,
                        "process_open_fds": process_open_fds_ds,
                        "tokio_active_tasks": tokio_active_tasks_ds,
                        "messages_received": messages_received_ds,
                        "last_voice_packet_time": last_voice_packet_time_ds,
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
                            let mut user_start_times_json = serde_json::Map::new();

                            let data_guard = data_cache.data.read().await;
                            let (user_start_times, guild_rec_metrics) =
                                if let Some(metrics) = data_guard.get::<BotMetricsKey>() {
                                    (
                                        Some(metrics.user_start_times.clone()),
                                        Some(metrics.guild_metrics(guild_id)),
                                    )
                                } else {
                                    (None, None)
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

                                        if let Some(times) = &user_start_times {
                                            if let Some(time) = times.get(&user_id.get()) {
                                                user_start_times_json.insert(
                                                    user_id.get().to_string(),
                                                    serde_json::Value::Number((*time).into()),
                                                );
                                            }
                                        }
                                    }
                                }
                            }

                            let recording_metrics_json = guild_rec_metrics.map(|m| {
                                serde_json::json!({
                                    "active_recordings": m.active_recordings.load(Ordering::Relaxed),
                                    "ffmpeg_spawn_failures": m.ffmpeg_spawn_failures.load(Ordering::Relaxed),
                                    "ffmpeg_process_crashes": m.ffmpeg_process_crashes.load(Ordering::Relaxed),
                                    "audio_packets_received": m.audio_packets_received.load(Ordering::Relaxed),
                                    "audio_packets_dropped": m.audio_packets_dropped.load(Ordering::Relaxed),
                                    "last_voice_packet_time": m.last_voice_packet_time.load(Ordering::Relaxed),
                                })
                            });

                            let event = hello_world::DashboardEvent {
                                event_type: "GUILD_VOICE_UPDATE".to_string(),
                                json_payload: serde_json::json!({
                                    "voice_states": voice_states_json,
                                    "user_start_times": user_start_times_json,
                                    "recording_metrics": recording_metrics_json,
                                })
                                .to_string(),
                            };

                            if tx_clone.send(Ok(event)).await.is_err() {
                                break;
                            }
                        }
                    }
                }

                // Wait for the topic to change or a relevant metric update before
                // sending the next payload.
                tokio::select! {
                    result = topic_rx.changed() => {
                        if result.is_err() { break; }
                    }
                    result = global_rx.changed(), if topic == "global" => {
                        if result.is_err() { break; }
                    }
                    result = voice_rx.changed(), if topic.starts_with("guild_voice:") => {
                        if result.is_err() { break; }
                    }
                    _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {
                        // Periodic push so the UI stays live even when no event fires.
                    }
                }
            }
            if let Some(m) = &dashboard_stream_metrics {
                m.grpc_active_streams.fetch_sub(1, Ordering::Relaxed);
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

        match self
            .data_cache
            .jam_cooldown
            .check_and_record(&self.data_cache.pool, data.guild_id, data.user_id)
            .await
        {
            crate::cooldown::CheckResult::Allowed => {}
            crate::cooldown::CheckResult::OnCooldown { remaining_secs } => {
                let reply = JamResponse {
                    resp: JamResponseEnum::Cooldown.into(),
                    cooldown_remaining_seconds: remaining_secs,
                };
                return Ok(Response::new(reply));
            }
        }

        let guild = match self
            .data_cache
            .cache
            .guild(GuildId::new(data.guild_id.try_into().unwrap()))
        {
            Some(ok) => ok.to_owned(),
            None => {
                let reply = JamResponse {
                    resp: JamResponseEnum::Unkown.into(),
                    cooldown_remaining_seconds: 0,
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
                        // Check if bot is in ANY channel. Otherwise the bot can't play stuff
                        if member.user.id == APPLICATION_ID_RELEASE {
                            info!(
                                "Ladies and gentlemen, We got him in c {}",
                                guild_channel.id.get()
                            );

                            handle_play_audio_to_channel(
                                data.guild_id,
                                &data.clip_name,
                                data.user_id,
                                self.data_cache.data.clone(),
                                self.data_cache.pool.clone(),
                            )
                            .await;

                            let reply = JamResponse {
                                resp: JamResponseEnum::Ok.into(),
                                cooldown_remaining_seconds: 0,
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
            cooldown_remaining_seconds: 0,
        };
        return Ok(Response::new(reply));
    }
}

async fn handle_play_audio_to_channel(
    id: i64,
    clip_name: &str,
    user_id: i64,
    data: Arc<RwLock<TypeMap>>,
    pool: sqlx::Pool<sqlx::Postgres>,
) {
    let manager = {
        let data_guard = data.read().await;
        data_guard.get::<SongbirdKey>().cloned().unwrap()
    };

    let guild_id = GuildId::new(id.try_into().unwrap());
    if let Err(e) =
        crate::commands::voice_controls::play_clip(&pool, &manager, guild_id, clip_name, user_id).await
    {
        tracing::error!("Failed to play clip from grpc: {}", e);
    }
}
