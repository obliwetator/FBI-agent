use chrono::Datelike;
use sakiot_paths::{CLIPS_ROOT, RECORDING_ROOT, RecordingKey};
use serenity::{
    async_trait,
    client::Context,
    model::id::{ChannelId, GuildId},
};
use songbird::{
    Event, EventContext, EventHandler as VoiceEventHandler,
    packet::{Packet, PacketSize, rtp::RtpExtensionPacket},
    events::context_data::{ConnectData, DisconnectData},
    model::payload::{ClientDisconnect, Speaking},
};
use sqlx::{Pool, Postgres};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::BufWriter;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use tokio::sync::{Mutex, RwLock};
use tracing::{error, info, warn};

use crate::events::ogg_opus_writer::OggOpusWriter;

pub const RECORDING_FILE_PATH: &str = RECORDING_ROOT;
pub const CLIPS_FILE_PATH: &str = CLIPS_ROOT;

/// 20 ms of audio per Opus packet — matches Discord's tick rate.
const TICK_MS: i64 = 20;

#[repr(i32)]
pub enum VoiceEventType {
    WriterOpen = 1,
    WriterClose = 2,
    WriterError = 3,
    ZombieReaped = 4,
}

/// One per-user recording: the streaming writer plus the metadata needed to
/// finalize the audio_files row when the writer closes.
struct UserRecording {
    writer: OggOpusWriter<BufWriter<File>>,
    file_name: String,
    start_time: chrono::DateTime<chrono::Utc>,
    user_id: u64,
    ssrc: u32,
}

#[derive(Clone)]
pub struct Receiver {
    inner: Arc<InnerReceiver>,
}

pub struct InnerReceiver {
    pool: Pool<Postgres>,
    channel_id: ChannelId,
    ctx_main: Arc<Context>,
    guild_id: GuildId,
    /// Active per-user recordings keyed by SSRC.
    ssrc_writer_hashmap: Arc<RwLock<HashMap<u32, Arc<Mutex<UserRecording>>>>>,
    user_id_hashmap: Arc<RwLock<HashMap<u64, u32>>>,
    bot_ssrcs: Arc<RwLock<HashSet<u32>>>,
    bot_user_id_hashmap: Arc<RwLock<HashMap<u64, u32>>>,
    metrics: Arc<crate::BotMetrics>,
    guild_metrics: Arc<crate::GuildRecordingMetrics>,
    pub last_voice_packet_time: AtomicI64,
    /// Wallclock millisecond when the first non-bot user joined this session.
    /// 0 = inactive. Used to pad new joiners' files with leading silence so
    /// every per-user .ogg shares granule-zero = session-start.
    session_start_ms: AtomicI64,
}

impl Drop for Receiver {
    fn drop(&mut self) {
        info!("Receiver dropped");
    }
}

impl Drop for InnerReceiver {
    fn drop(&mut self) {
        info!("Inner Receiver dropped");
    }
}

impl Receiver {
    pub async fn new(
        pool: Pool<Postgres>,
        ctx: Arc<Context>,
        guild_id: GuildId,
        channel_id: ChannelId,
        metrics: Arc<crate::BotMetrics>,
    ) -> Self {
        let guild_metrics = metrics.guild_metrics(guild_id.get());
        let inner = Arc::new(InnerReceiver {
            pool,
            ctx_main: ctx,
            user_id_hashmap: Arc::new(RwLock::new(HashMap::new())),
            ssrc_writer_hashmap: Arc::new(RwLock::new(HashMap::new())),
            bot_ssrcs: Arc::new(RwLock::new(HashSet::new())),
            bot_user_id_hashmap: Arc::new(RwLock::new(HashMap::new())),
            guild_id,
            channel_id,
            metrics,
            guild_metrics,
            last_voice_packet_time: AtomicI64::new(chrono::Utc::now().timestamp_millis()),
            session_start_ms: AtomicI64::new(0),
        });

        // Spawn Health Checker / Reaper
        let inner_weak = Arc::downgrade(&inner);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;

                let inner_clone = match inner_weak.upgrade() {
                    Some(i) => i,
                    None => break,
                };

                let mut users_to_remove = Vec::new();

                let user_map = inner_clone.user_id_hashmap.read().await;
                if user_map.is_empty() {
                    continue;
                }

                if let Some(guild) = inner_clone.ctx_main.cache.guild(inner_clone.guild_id) {
                    for (&uid, &ssrc) in user_map.iter() {
                        if !guild
                            .voice_states
                            .contains_key(&serenity::model::id::UserId::new(uid))
                        {
                            users_to_remove.push((uid, ssrc));
                        }
                    }
                }
                drop(user_map);

                for (uid, ssrc) in users_to_remove {
                    warn!(
                        "Reaper: User {} (SSRC {}) is no longer in voice state. Closing writer.",
                        uid, ssrc
                    );
                    inner_clone.user_id_hashmap.write().await.remove(&uid);
                    finalize_writer(&inner_clone, ssrc, VoiceEventType::ZombieReaped).await;
                }
            }
        });

        Self { inner }
    }

    pub fn last_voice_packet_time(&self) -> i64 {
        self.inner.last_voice_packet_time.load(Ordering::Relaxed)
    }
}

#[async_trait]
impl VoiceEventHandler for Receiver {
    #[tracing::instrument(skip_all, name = "receiver_act", fields(guild_id = %self.inner.guild_id))]
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        use EventContext as Ctx;
        use tracing::Instrument;
        match ctx {
            Ctx::SpeakingStateUpdate(Speaking {
                speaking,
                ssrc,
                user_id,
                ..
            }) => {
                let _ = async {
                info!(
                    "Speaking state update: user {:?} has SSRC {:?}, using {:?}",
                    user_id, ssrc, speaking,
                );

                let Some(user_id) = user_id else {
                    error!("No user_id in SpeakingStateUpdate");
                    return None;
                };

                let guild = match self.inner.ctx_main.cache.guild(self.inner.guild_id) {
                    Some(g) => g.to_owned(),
                    None => {
                        error!("Guild {} not in cache", self.inner.guild_id);
                        return None;
                    }
                };

                let member = match guild.member(&self.inner.ctx_main, user_id.0).await {
                    Ok(m) => m,
                    Err(e) => {
                        error!("Failed to get member: {}", e);
                        return None;
                    }
                };

                if member.user.bot {
                    info!("is a bot");
                    self.inner.bot_ssrcs.write().await.insert(*ssrc);
                    self.inner
                        .bot_user_id_hashmap
                        .write()
                        .await
                        .insert(user_id.0, *ssrc);
                } else {
                    info!("is NOT bot");

                    crate::database::user_names::observe(
                        &self.inner.pool,
                        self.inner.guild_id.get(),
                        &member.user,
                        Some(&member),
                    )
                    .await;

                    let is_channel_empty = {
                        let len = self.inner.user_id_hashmap.read().await.len();
                        len == 0
                    };

                    {
                        self.inner
                            .user_id_hashmap
                            .write()
                            .await
                            .insert(user_id.0, *ssrc);
                    }

                    {
                        // Single write-lock for the check-and-insert to avoid TOCTOU.
                        let mut writer_map = self.inner.ssrc_writer_hashmap.write().await;
                        if writer_map.contains_key(ssrc) {
                            error!("already got writer for ssrc {}", ssrc);
                        } else {
                            info!("New writer for ssrc {}", ssrc);
                            let now = chrono::Utc::now();
                            let now_ms = now.timestamp_millis();

                            // Anchor session start on first joiner; reuse for later joiners.
                            let session_start = match self.inner.session_start_ms.compare_exchange(
                                0,
                                now_ms,
                                Ordering::SeqCst,
                                Ordering::SeqCst,
                            ) {
                                Ok(_) => now_ms,
                                Err(existing) => existing,
                            };

                            let path = create_path(
                                self,
                                now,
                                &self.inner.pool,
                                *ssrc,
                                self.inner.guild_id,
                                self.inner.channel_id,
                                user_id.0,
                                &member,
                                is_channel_empty,
                            )
                            .await;

                            let file = match File::create(format!("{}.ogg", path)) {
                                Ok(f) => f,
                                Err(e) => {
                                    error!("Failed to create file for ssrc {}: {}", ssrc, e);
                                    self.inner
                                        .metrics
                                        .ffmpeg_spawn_failures
                                        .fetch_add(1, Ordering::Relaxed);
                                    self.inner
                                        .guild_metrics
                                        .ffmpeg_spawn_failures
                                        .fetch_add(1, Ordering::Relaxed);
                                    return None;
                                }
                            };

                            let mut writer = match OggOpusWriter::new(BufWriter::new(file), *ssrc, 0) {
                                Ok(w) => w,
                                Err(e) => {
                                    error!("Failed to init OggOpusWriter for ssrc {}: {}", ssrc, e);
                                    return None;
                                }
                            };

                            // Pad with leading silence so this file's granule-zero aligns
                            // with session-start. First joiner: session_start == now_ms → 0 frames.
                            let lead_ms = now_ms.saturating_sub(session_start);
                            let lead_frames = (lead_ms / TICK_MS) as u64;
                            if lead_frames > 0 {
                                if let Err(e) = writer.write_silence(lead_frames) {
                                    error!(
                                        "Failed to write {} leading silence frames for ssrc {}: {}",
                                        lead_frames, ssrc, e
                                    );
                                }
                            }

                            let file_name = RecordingKey::stem_for(now_ms, user_id.0 as i64);
                            let recording = UserRecording {
                                writer,
                                file_name,
                                start_time: now,
                                user_id: user_id.0,
                                ssrc: *ssrc,
                            };
                            writer_map.insert(*ssrc, Arc::new(Mutex::new(recording)));

                            self.inner
                                .metrics
                                .active_recordings
                                .fetch_add(1, Ordering::Relaxed);
                            self.inner
                                .guild_metrics
                                .active_recordings
                                .fetch_add(1, Ordering::Relaxed);

                            let _ = sqlx::query(
                                "INSERT INTO voice_events_audit (guild_id, user_id, ssrc, event_type_id, details) VALUES ($1, $2, $3, $4, $5)"
                            )
                            .bind(self.inner.guild_id.get() as i64)
                            .bind(user_id.0 as i64)
                            .bind(*ssrc as i64)
                            .bind(VoiceEventType::WriterOpen as i32)
                            .bind("Writer opened")
                            .execute(&self.inner.pool)
                            .await;

                            info!("1 file created for ssrc: {}", *ssrc);
                        }
                    }
                }
                None::<Event>
            }.instrument(tracing::info_span!("SpeakingStateUpdate", ssrc = %ssrc, user_id = ?user_id)).await;
            }

            Ctx::RtpPacket(_packet) => {
                // Raw RTP — unused; we read Opus payload from VoiceTick instead.
            }
            Ctx::VoiceTick(tick) => {
                if !tick.speaking.is_empty() {
                    let now = chrono::Utc::now().timestamp_millis();
                    self.inner
                        .last_voice_packet_time
                        .store(now, Ordering::Relaxed);
                    self.inner
                        .metrics
                        .last_voice_packet_time
                        .store(now, Ordering::Relaxed);
                    self.inner
                        .guild_metrics
                        .last_voice_packet_time
                        .store(now, Ordering::Relaxed);
                }

                // Snapshot the active SSRCs (skip bots).
                let active: Vec<(u32, Arc<Mutex<UserRecording>>)> = {
                    let map = self.inner.ssrc_writer_hashmap.read().await;
                    let bots = self.inner.bot_ssrcs.read().await;
                    map.iter()
                        .filter(|(s, _)| !bots.contains(s))
                        .map(|(s, w)| (*s, w.clone()))
                        .collect()
                };

                for (ssrc, recording) in active {
                    let speaking_data = tick.speaking.get(&ssrc);

                    // Pull the raw Opus payload bytes if the user spoke this tick.
                    let opus_bytes: Option<Vec<u8>> = speaking_data.and_then(|d| {
                        d.packet.as_ref().map(|rtp| {
                            let view = rtp.rtp();
                            let payload = view.payload();
                            // NB: in songbird 0.6 VoiceTick, `payload_end_pad` is an
                            // absolute end index into `payload`, not a tail-pad count
                            // (see songbird/src/driver/tasks/udp_rx/ssrc_state.rs:86).
                            let start = rtp.payload_offset.min(payload.len());
                            let end = rtp.payload_end_pad.min(payload.len());
                            if end <= start {
                                return Vec::new();
                            }
                            let body = &payload[start..end];
                            // RTP header extension (Discord uses one-byte form) sits
                            // inside the body — skip it before handing bytes to Opus.
                            let opus = if view.get_extension() != 0 {
                                match RtpExtensionPacket::new(body) {
                                    Some(ext) => {
                                        let off = ext.packet_size();
                                        if off >= body.len() { &[][..] } else { &body[off..] }
                                    }
                                    None => body,
                                }
                            } else {
                                body
                            };
                            opus.to_vec()
                        })
                    });

                    let mut rec = recording.lock().await;
                    let result = match opus_bytes.as_deref() {
                        Some(bytes) if !bytes.is_empty() => {
                            self.inner
                                .metrics
                                .audio_packets_received
                                .fetch_add(1, Ordering::Relaxed);
                            self.inner
                                .guild_metrics
                                .audio_packets_received
                                .fetch_add(1, Ordering::Relaxed);
                            rec.writer.write_packet(bytes)
                        }
                        _ => rec.writer.write_silence(1),
                    };
                    if let Err(e) = result {
                        error!("Writer error for ssrc {}: {}", ssrc, e);
                    }
                }
            }
            Ctx::RtcpPacket(_data) => {}

            Ctx::DriverDisconnect(DisconnectData { kind, reason, .. }) => {
                info!("Disconnected \n kind: {:?} \n reason {:?}", kind, reason);

                self.inner
                    .metrics
                    .driver_disconnects
                    .fetch_add(1, Ordering::Relaxed);

                // Snapshot SSRCs and finalize each writer (writes EOS, runs DB update).
                let ssrcs: Vec<u32> = {
                    let map = self.inner.ssrc_writer_hashmap.read().await;
                    map.keys().copied().collect()
                };
                for ssrc in ssrcs {
                    finalize_writer(&self.inner, ssrc, VoiceEventType::WriterClose).await;
                }

                self.inner.user_id_hashmap.write().await.clear();
                self.inner.bot_ssrcs.write().await.clear();
                self.inner.bot_user_id_hashmap.write().await.clear();
                self.inner.session_start_ms.store(0, Ordering::SeqCst);
            }
            Ctx::DriverConnect(ConnectData { .. }) => {
                info!("Connected")
            }
            Ctx::DriverReconnect(ConnectData { .. }) => {
                info!("Reconnected");
                self.inner
                    .metrics
                    .driver_reconnects
                    .fetch_add(1, Ordering::Relaxed);
            }

            Ctx::ClientDisconnect(ClientDisconnect { user_id }) => {
                let _ = async {
                    error!("client disconnected id: {}", user_id);

                    let is_bot_ssrc = self
                        .inner
                        .bot_user_id_hashmap
                        .write()
                        .await
                        .remove(&user_id.0);
                    if let Some(bot_ssrc) = is_bot_ssrc {
                        info!("Removed bot with id: {} and ssrc: {}", user_id.0, bot_ssrc);
                        self.inner.bot_ssrcs.write().await.remove(&bot_ssrc);
                        return None;
                    }

                    let ssrc = match self.inner.user_id_hashmap.write().await.remove(&user_id.0) {
                        Some(ok) => ok,
                        None => {
                            info!("tried to remove bot");
                            return None;
                        }
                    };

                    finalize_writer(&self.inner, ssrc, VoiceEventType::WriterClose).await;
                    None::<Event>
                }
                .instrument(tracing::info_span!("ClientDisconnect", user_id = %user_id))
                .await;
            }
            _ => {
                unimplemented!()
            }
        }

        None
    }
}

/// Close the writer for `ssrc`, run the audio_files DB update, decrement
/// active counters. Idempotent — silently no-ops if the writer is already gone.
async fn finalize_writer(
    inner: &Arc<InnerReceiver>,
    ssrc: u32,
    event_type: VoiceEventType,
) {
    let entry = inner.ssrc_writer_hashmap.write().await.remove(&ssrc);
    let Some(arc) = entry else {
        return;
    };

    // Lock the writer to wait out any in-flight tick write, then finalize.
    // We don't try to unwrap the Arc; we just clone the metadata we need
    // and let the Arc drop naturally after this scope.
    let mut rec = arc.lock().await;

    if let Err(e) = rec.writer.finish() {
        error!("Failed to finalize writer for ssrc {}: {}", ssrc, e);
        let _ = sqlx::query(
            "INSERT INTO voice_events_audit (guild_id, user_id, ssrc, event_type_id, details) VALUES ($1, $2, $3, $4, $5)"
        )
        .bind(inner.guild_id.get() as i64)
        .bind(rec.user_id as i64)
        .bind(ssrc as i64)
        .bind(VoiceEventType::WriterError as i32)
        .bind(format!("finish: {}", e))
        .execute(&inner.pool)
        .await;
    }

    inner.metrics.active_recordings.fetch_sub(1, Ordering::Relaxed);
    inner
        .guild_metrics
        .active_recordings
        .fetch_sub(1, Ordering::Relaxed);

    let time_elapsed = chrono::Utc::now()
        .signed_duration_since(rec.start_time)
        .num_milliseconds();
    let last_person_in_channel = inner.user_id_hashmap.read().await.is_empty();
    // 2 = JOINED 3 = LAST
    let state = if last_person_in_channel { 3 } else { 2 };

    let file_name = rec.file_name.clone();
    let user_id = rec.user_id;
    let rec_ssrc = rec.ssrc;
    drop(rec);

    info!("File name: {}", file_name);
    if let Err(err) = sqlx::query!(
        "UPDATE audio_files
            SET end_ts = audio_files.start_ts + $1, state_leave = $2
            WHERE file_name = $3",
        time_elapsed,
        state,
        file_name
    )
    .execute(&inner.pool)
    .await
    {
        error!("{}", err);
        inner
            .metrics
            .db_query_errors
            .fetch_add(1, Ordering::Relaxed);
    }

    let _ = sqlx::query(
        "INSERT INTO voice_events_audit (guild_id, user_id, ssrc, event_type_id, details) VALUES ($1, $2, $3, $4, $5)"
    )
    .bind(inner.guild_id.get() as i64)
    .bind(user_id as i64)
    .bind(rec_ssrc as i64)
    .bind(event_type as i32)
    .bind("Writer closed")
    .execute(&inner.pool)
    .await;
}

#[tracing::instrument(skip_all, name = "create_path")]
async fn create_path(
    _self: &Receiver,
    now: chrono::DateTime<chrono::Utc>,
    pool: &Pool<Postgres>,
    _ssrc: u32,
    guild_id: GuildId,
    channel_id: ChannelId,
    user_id: u64,
    _member: &serenity::model::guild::Member,
    is_channel_empty: bool,
) -> String {
    let file_name = RecordingKey::stem_for(now.timestamp_millis(), user_id as i64);
    let key = RecordingKey::new(
        guild_id.get() as i64,
        channel_id.get() as i64,
        now.year(),
        now.month(),
        file_name.clone(),
    );

    let dir_path = key.recording_dir(RECORDING_ROOT);
    let combined_path = key.recording_dir(RECORDING_ROOT).join(&file_name);

    if let Err(err) = std::fs::create_dir_all(&dir_path) {
        error!("cannot create path {}: {}", dir_path.display(), err);
    };

    let null: Option<i64> = None;

    match sqlx::query!(
        "INSERT INTO audio_files
	(file_name, guild_id, channel_id, user_id, year, month, start_ts, end_ts, state_enter) VALUES
	($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        file_name,
        guild_id.get() as i64,
        channel_id.get() as i64,
        user_id as i64,
        now.year(),
        now.month() as i32,
        now.timestamp_millis(),
        null,
        if is_channel_empty { 1 } else { 2 }
    )
    .execute(pool)
    .await
    {
        Ok(ok) => ok,
        Err(err) => {
            error!("{}", err);
            _self
                .inner
                .metrics
                .db_insert_failures
                .fetch_add(1, Ordering::Relaxed);
            _self
                .inner
                .metrics
                .db_query_errors
                .fetch_add(1, Ordering::Relaxed);
            panic!()
        }
    };

    combined_path.to_string_lossy().into_owned()
}
