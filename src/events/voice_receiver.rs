use chrono::Datelike;
use serenity::{
    async_trait,
    client::Context,
    model::id::{ChannelId, GuildId},
};
use songbird::{
    Event, EventContext, EventHandler as VoiceEventHandler,
    events::context_data::{ConnectData, DisconnectData},
    model::payload::{ClientDisconnect, Speaking},
};
use sqlx::{Pool, Postgres};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use tokio::sync::RwLock;
use tokio::{
    io::AsyncWriteExt,
    process::{Child, Command},
};
use tracing::{error, info, warn};

pub const RECORDING_FILE_PATH: &str = "./voice_recordings";
pub const CLIPS_FILE_PATH: &str = "./clips";

#[repr(i32)]
pub enum VoiceEventType {
    FfmpegSpawn = 1,
    FfmpegExitSuccess = 2,
    FfmpegCrash = 3,
    ZombieReaped = 4,
}

#[derive(Clone)]
pub struct Receiver {
    inner: Arc<InnerReceiver>,
}

pub struct InnerReceiver {
    pool: Pool<Postgres>,
    channel_id: ChannelId,
    ctx_main: Arc<Context>,
    now: Arc<RwLock<HashMap<u32, chrono::DateTime<chrono::Utc>>>>,
    file_name: Arc<RwLock<HashMap<u32, String>>>,
    guild_id: GuildId,
    ssrc_ffmpeg_hashmap: Arc<RwLock<HashMap<u32, tokio::sync::mpsc::Sender<Vec<u8>>>>>,
    user_id_hashmap: Arc<RwLock<HashMap<u64, u32>>>,
    bot_ssrcs: Arc<RwLock<HashSet<u32>>>,
    bot_user_id_hashmap: Arc<RwLock<HashMap<u64, u32>>>,
    metrics: Arc<crate::BotMetrics>,
    guild_metrics: Arc<crate::GuildRecordingMetrics>,
    pub last_voice_packet_time: AtomicI64,
}

#[allow(dead_code)]
struct VoiceChannelMembersData {
    now: chrono::DateTime<chrono::Utc>,
    file_name: String,
    ssrc: u32,
    ffmpeg_handle: Child,
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
        // You can manage state here, such as a buffer of audio packet bytes, so
        // you can later store them in intervals.

        let guild_metrics = metrics.guild_metrics(guild_id.get());
        let inner = Arc::new(InnerReceiver {
            pool,
            now: Arc::new(RwLock::new(HashMap::new())),
            file_name: Arc::new(RwLock::new(HashMap::new())),
            ctx_main: ctx,
            user_id_hashmap: Arc::new(RwLock::new(HashMap::new())),
            ssrc_ffmpeg_hashmap: Arc::new(RwLock::new(HashMap::new())),
            bot_ssrcs: Arc::new(RwLock::new(HashSet::new())),
            bot_user_id_hashmap: Arc::new(RwLock::new(HashMap::new())),
            guild_id,
            channel_id,
            metrics,
            guild_metrics,
            last_voice_packet_time: AtomicI64::new(chrono::Utc::now().timestamp_millis()),
        });

        // Spawn Health Checker / Reaper
        let inner_weak = Arc::downgrade(&inner);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;

                // If Receiver was dropped, break the loop to prevent memory leak
                let inner_clone = match inner_weak.upgrade() {
                    Some(i) => i,
                    None => break,
                };

                let mut users_to_remove = Vec::new();

                // Read the current users we are tracking
                let user_map = inner_clone.user_id_hashmap.read().await;
                if user_map.is_empty() {
                    continue;
                }

                if let Some(guild) = inner_clone.ctx_main.cache.guild(inner_clone.guild_id) {
                    for (&uid, &ssrc) in user_map.iter() {
                        // Check if Discord still thinks the user is in the voice channel
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
                        "Reaper: User {} (SSRC {}) is no longer in voice state, but process is alive. Reaping zombie.",
                        uid, ssrc
                    );

                    inner_clone.user_id_hashmap.write().await.remove(&uid);

                    // Dropping the tx from ssrc_ffmpeg_hashmap closes stdin, gracefully stopping ffmpeg
                    if inner_clone
                        .ssrc_ffmpeg_hashmap
                        .write()
                        .await
                        .remove(&ssrc)
                        .is_some()
                    {
                        // Log audit event for the zombie reap
                        let _ = sqlx::query(
                            "INSERT INTO voice_events_audit (guild_id, user_id, ssrc, event_type_id, details) VALUES ($1, $2, $3, $4, $5)"
                        )
                        .bind(inner_clone.guild_id.get() as i64)
                        .bind(uid as i64)
                        .bind(ssrc as i64)
                        .bind(VoiceEventType::ZombieReaped as i32)
                        .bind("Reaper killed zombie process")
                        .execute(&inner_clone.pool)
                        .await;
                    }
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
                // Discord voice calls use RTP, where every sender uses a randomly allocated
                // *Synchronisation Source* (SSRC) to allow receivers to tell which audio
                // stream a received packet belongs to. As this number is not derived from
                // the sender's user_id, only Discord Voice Gateway messages like this one
                // inform us about which random SSRC a user has been allocated. Future voice
                // packets will contain *only* the SSRC.
                //
                // You can implement logic here so that you can differentiate users'
                // SSRCs and map the SSRC to the User ID and maintain this state.
                // Using this map, you can map the `ssrc` in `voice_packet`
                // to the user ID and handle their audio packets separately.
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
                        // Use a single write-lock for the check-and-insert to avoid a
                        // TOCTOU race: if we checked with a read lock and then upgraded
                        // to a write lock, another concurrent event could slip in between
                        // and spawn a duplicate ffmpeg process for the same SSRC.
                        let mut ffmpeg_map = self.inner.ssrc_ffmpeg_hashmap.write().await;
                        if ffmpeg_map.contains_key(ssrc) {
                            // we have already spawned a ffmpeg process
                            // This should happen when the bot gets reconnected to the voice channel by the framework.
                            error!("already got ffmpeg process");
                        } else {
                            info!("New ffmpeg process");
                            // Create a process
                            let now = chrono::Utc::now();
                            {
                                self.inner.now.write().await.insert(*ssrc, now);
                            }
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
                            let mut child = match spawn_ffmpeg(&path) {
                                Ok(c) => {
                                    self.inner
                                        .metrics
                                        .active_recordings
                                        .fetch_add(1, Ordering::Relaxed);
                                    self.inner
                                        .guild_metrics
                                        .active_recordings
                                        .fetch_add(1, Ordering::Relaxed);
                                    c
                                }
                                Err(e) => {
                                    error!("Failed to spawn ffmpeg for ssrc {}: {}", ssrc, e);
                                    let _ = sqlx::query(
                                        "INSERT INTO voice_events_audit (guild_id, user_id, ssrc, event_type_id, details) VALUES ($1, $2, $3, $4, $5)"
                                    )
                                    .bind(self.inner.guild_id.get() as i64)
                                    .bind(user_id.0 as i64)
                                    .bind(*ssrc as i64)
                                    .bind(VoiceEventType::FfmpegCrash as i32)
                                    .bind(format!("Failed to spawn: {}", e))
                                    .execute(&self.inner.pool)
                                    .await;

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

                            let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<u8>>(4000);

                            ffmpeg_map.insert(*ssrc, tx);
                            // Log successful spawn
                            let _ = sqlx::query(
                                "INSERT INTO voice_events_audit (guild_id, user_id, ssrc, event_type_id, details) VALUES ($1, $2, $3, $4, $5)"
                            )
                            .bind(self.inner.guild_id.get() as i64)
                            .bind(user_id.0 as i64)
                            .bind(*ssrc as i64)
                            .bind(VoiceEventType::FfmpegSpawn as i32)
                            .bind("Spawned successfully")
                            .execute(&self.inner.pool)
                            .await;

                            // Drop the write lock now that the entry is committed, before
                            // spawning the background task so VoiceTick can read immediately.
                            drop(ffmpeg_map);

                            let inner_clone = self.inner.clone();
                            let guild_metrics_clone = self.inner.guild_metrics.clone();
                            let ssrc_clone = *ssrc;
                            let user_id_clone = user_id.0;

                            tokio::spawn(async move {
                                if let Some(mut stdin) = child.stdin.take() {
                                    while let Some(data) = rx.recv().await {
                                        if let Err(e) = stdin.write_all(&data).await {
                                            error!("Failed to write to ffmpeg stdin: {}", e);
                                            break;
                                        }
                                    }
                                    info!(
                                        "mpsc receiver channel for ssrc {} closed (rx.recv() returned None) or write failed. Closing ffmpeg stdin.",
                                        ssrc_clone
                                    );
                                }

                                info!("wait for child");
                                let output = match child.wait_with_output().await {
                                    Ok(out) => out,
                                    Err(e) => {
                                        error!("Failed to wait for child process: {}", e);
                                        inner_clone
                                            .metrics
                                            .active_recordings
                                            .fetch_sub(1, Ordering::Relaxed);
                                        guild_metrics_clone
                                            .active_recordings
                                            .fetch_sub(1, Ordering::Relaxed);
                                        return;
                                    }
                                };
                                info!("wait is over");

                                if !output.status.success() {
                                    let code = output.status.code().unwrap_or(-1);
                                    error!(
                                        "ffmpeg exited with non-zero status {} for ssrc {}",
                                        code, ssrc_clone
                                    );

                                    let _ = sqlx::query(
                                        "INSERT INTO voice_events_audit (guild_id, user_id, ssrc, event_type_id, details) VALUES ($1, $2, $3, $4, $5)"
                                    )
                                    .bind(inner_clone.guild_id.get() as i64)
                                    .bind(user_id_clone as i64)
                                    .bind(ssrc_clone as i64)
                                    .bind(VoiceEventType::FfmpegCrash as i32)
                                    .bind(format!("Exited with status {}", code))
                                    .execute(&inner_clone.pool)
                                    .await;

                                    inner_clone
                                        .metrics
                                        .ffmpeg_process_crashes
                                        .fetch_add(1, Ordering::Relaxed);
                                    guild_metrics_clone
                                        .ffmpeg_process_crashes
                                        .fetch_add(1, Ordering::Relaxed);
                                } else {
                                    let _ = sqlx::query(
                                        "INSERT INTO voice_events_audit (guild_id, user_id, ssrc, event_type_id, details) VALUES ($1, $2, $3, $4, $5)"
                                    )
                                    .bind(inner_clone.guild_id.get() as i64)
                                    .bind(user_id_clone as i64)
                                    .bind(ssrc_clone as i64)
                                    .bind(VoiceEventType::FfmpegExitSuccess as i32)
                                    .bind("Exited successfully")
                                    .execute(&inner_clone.pool)
                                    .await;
                                }
                                inner_clone
                                    .metrics
                                    .active_recordings
                                    .fetch_sub(1, Ordering::Relaxed);
                                guild_metrics_clone
                                    .active_recordings
                                    .fetch_sub(1, Ordering::Relaxed);

                                let lock_now = inner_clone.now.read().await;
                                let now = match lock_now.get(&ssrc_clone) {
                                    Some(t) => t,
                                    None => {
                                        error!("No start time found for ssrc {}", ssrc_clone);
                                        return;
                                    }
                                };
                                let time_elapsed = chrono::Utc::now()
                                    .signed_duration_since(*now)
                                    .num_milliseconds();
                                drop(lock_now);

                                let lock_file = inner_clone.file_name.read().await;
                                let file_name = match lock_file.get(&ssrc_clone) {
                                    Some(f) => f.clone(),
                                    None => {
                                        error!("No file name found for ssrc {}", ssrc_clone);
                                        return;
                                    }
                                };
                                drop(lock_file);

                                let last_person_in_channel =
                                    { inner_clone.user_id_hashmap.read().await.len() == 0 };
                                // 2 = JOINED 3 = LAST
                                let state = { if last_person_in_channel { 3 } else { 2 } };

                                info!("File name :{}", file_name);

                                match sqlx::query!(
                                    "UPDATE audio_files
                                        SET end_ts = audio_files.start_ts + $1, state_leave = $2
                                        WHERE file_name = $3",
                                    time_elapsed,
                                    state,
                                    file_name
                                )
                                .execute(&inner_clone.pool)
                                .await
                                {
                                    Ok(_) => {
                                        info!("Updated table row");
                                    }
                                    Err(err) => {
                                        error!("{}", err);
                                        inner_clone
                                            .metrics
                                            .db_query_errors
                                            .fetch_add(1, Ordering::Relaxed);
                                    }
                                };

                                // CRITICAL FIX: Ensure the transmitter is removed from the hashmap when the process ends
                                // so that VoiceTick stops trying to send to a closed pipe, and future voice events can spawn a new process.
                                inner_clone.ssrc_ffmpeg_hashmap.write().await.remove(&ssrc_clone);
                            });

                            info!("1 file created for ssrc: {}", *ssrc);
                        }
                    }
                }
                None::<Event>
            }.instrument(tracing::info_span!("SpeakingStateUpdate", ssrc = %ssrc, user_id = ?user_id)).await;
            }

            Ctx::RtpPacket(packet) => {
                // Those are the un decoded opus packets
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

                for (ssrc, data) in &tick.speaking {
                    if self.inner.bot_ssrcs.read().await.contains(ssrc) {
                        continue;
                    }

                    self.inner
                        .metrics
                        .audio_packets_received
                        .fetch_add(1, Ordering::Relaxed);
                    self.inner
                        .guild_metrics
                        .audio_packets_received
                        .fetch_add(1, Ordering::Relaxed);

                    let tx = {
                        self.inner
                            .ssrc_ffmpeg_hashmap
                            .read()
                            .await
                            .get(ssrc)
                            .cloned()
                    };

                    if let Some(tx) = tx {
                        if let Some(decoded_voice) = data.decoded_voice.as_ref() {
                            // Pre-allocate the exact byte count (2 bytes per i16 sample)
                            // and convert synchronously — avoids creating ~1920 async futures
                            // per 20ms tick that were previously caused by write_i16_le().await.
                            let mut result: Vec<u8> = Vec::with_capacity(decoded_voice.len() * 2);
                            for &n in decoded_voice {
                                result.extend_from_slice(&n.to_le_bytes());
                            }

                            if let Err(e) = tx.try_send(result) {
                                // Channel full means ffmpeg is temporarily behind.
                                // This drops the packet rather than blocking the VoiceTick
                                // handler (which would stall all other SSRCs too).
                                //
                                // Rate-limit the warning: only log once every 100 drops
                                // per SSRC to avoid flooding the log.
                                let count = self
                                    .inner
                                    .metrics
                                    .audio_packets_dropped
                                    .fetch_add(1, Ordering::Relaxed);
                                self.inner
                                    .guild_metrics
                                    .audio_packets_dropped
                                    .fetch_add(1, Ordering::Relaxed);
                                if count % 100 == 0 {
                                    warn!(
                                        "Audio packet dropped for ssrc {} (total drops so far: {}): {}",
                                        ssrc,
                                        count + 1,
                                        e
                                    );
                                }
                            }
                        } else {
                            error!("Decode disabled");
                        }
                    } else {
                        error!("No child");
                    }
                }
            }
            Ctx::RtcpPacket(data) => {
                // An event which fires for every received rtcp packet,
                // containing the call statistics and reporting information.
                // info!("RTCP packet received: {:?}", data.packet);
            }

            Ctx::DriverDisconnect(DisconnectData { kind, reason, .. }) => {
                info!("Disconnected \n kind: {:?} \n reason {:?}", kind, reason);

                self.inner
                    .metrics
                    .driver_disconnects
                    .fetch_add(1, Ordering::Relaxed);

                // CRITICAL FIX: Explicitly clear the hashmaps on disconnect.
                // This manually drops the `tx` channels, which breaks the circular reference deadlock
                // and gracefully shuts down all zombie ffmpeg processes waiting on `rx`.
                self.inner.ssrc_ffmpeg_hashmap.write().await.clear();
                self.inner.user_id_hashmap.write().await.clear();
                self.inner.bot_ssrcs.write().await.clear();
                self.inner.bot_user_id_hashmap.write().await.clear();
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

                    // By removing the sender from the hash map, the `mpsc::Sender` is dropped.
                    // This causes `rx.recv().await` in the spawned task to return None,
                    // which closes `ffmpeg`'s stdin and proceeds to finish the process and run the DB updates.
                    match self.inner.ssrc_ffmpeg_hashmap.write().await.remove(&ssrc) {
                        Some(_) => {
                            info!("Removed sender for ssrc {}", ssrc);
                        }
                        None => {
                            error!("No child process found for ssrc {}", ssrc);
                            return None;
                        }
                    };
                    None::<Event>
                }
                .instrument(tracing::info_span!("ClientDisconnect", user_id = %user_id))
                .await;
            }
            _ => {
                // We won't be registering this struct for any more event classes.
                unimplemented!()
            }
        }

        None
    }
}

#[tracing::instrument(skip_all, name = "spawn_ffmpeg")]
fn spawn_ffmpeg(path: &str) -> Result<Child, std::io::Error> {
    Command::new("ffmpeg")
        // .arg("-re") // realtime
        .args(["-use_wallclock_as_timestamps", "true"]) // Attach timestamps to packets. Read -af aresample=async=1
        .args(["-f", "s16le"]) // input type
        .args(["-channel_layout", "stereo"])
        .args(["-ar", "48000"]) // sample rate
        .args(["-ac", "2"]) // channel count
        .args(["-i", "pipe:0"]) // Input name
        .args(["-c:a", "libvorbis"])
        .args(["-async", "1"]) // this will input silence when there is no packets coming.
        // HOWEVER. It will not work if the packets do not have a timestamp attached to them. We can tell ffmpeg to attach its own timestamps and figure the timings by itself
        .args(["-flush_packets", "1"]) // Write to the file on every packet. While this is wasteful it allows semi realtime audio playback.
        .arg(format!("{}.ogg", path)) // output
        .stdin(std::process::Stdio::piped())
        // stdout: ffmpeg writes nothing useful to stdout for this use case.
        .stdout(std::process::Stdio::null())
        // stderr: piped so we can drain it continuously in a background reader.
        // Do NOT use Stdio::null() here if you want to see ffmpeg errors.
        // Do NOT use wait_with_output() to read it — that only reads after the
        // process exits, letting the 64 KB pipe buffer fill and deadlock ffmpeg.
        .stderr(std::process::Stdio::null())
        .spawn()
}

// TODO: username instead of id
#[tracing::instrument(skip_all, name = "create_path")]
async fn create_path(
    _self: &Receiver,
    now: chrono::DateTime<chrono::Utc>,
    pool: &Pool<Postgres>,
    ssrc: u32,
    guild_id: GuildId,
    channel_id: ChannelId,
    user_id: u64,
    member: &serenity::model::guild::Member,
    is_channel_empty: bool,
) -> String {
    let year = format!("{}", now.format("%Y"));
    let month = format!("{}", now.month());
    let day_number = format!("{}", now.format("%d"));
    let day_name = format!("{}", now.format("%A"));
    let hour = format!("{}", now.format("%H"));
    let minute = format!("{}", now.format("%M"));
    let seconds = format!("{}", now.format("%S"));

    let dir_path = format!(
        "{}/{}/{}/{}/{}/",
        &RECORDING_FILE_PATH,
        guild_id.get(),
        channel_id.get(),
        year,
        month
    );

    let file_name = format!(
        "{}-{}-{}",
        now.timestamp_millis(),
        user_id,
        member.user.name
    );

    {
        _self
            .inner
            .file_name
            .write()
            .await
            .insert(ssrc, file_name.to_owned());
    }
    let combined_path = format!(
        "{}/{}/{}/{}/{}/{}",
        &RECORDING_FILE_PATH,
        guild_id.get(),
        channel_id.get(),
        year,
        month,
        file_name
    );

    // Try to create the dir in case it does not exist
    if let Err(err) = std::fs::create_dir_all(&dir_path) {
        error!("cannot create path {}: {}", dir_path, err);
        // Returning an empty string or handling this more gracefully might be better
        // but for now we avoid panicking the whole application.
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

    combined_path
}
