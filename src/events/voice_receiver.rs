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
use std::sync::Arc;
use std::{collections::HashMap, time::Duration};
use tokio::{
    io::AsyncWriteExt,
    process::{Child, Command},
};
use tokio::{sync::RwLock, task::JoinHandle};
use tracing::{error, info};

pub const RECORDING_FILE_PATH: &str = "/home/tulipan/projects/FBI-agent/voice_recordings";
pub const CLIPS_FILE_PATH: &str = "/home/tulipan/projects/FBI-agent/clips";

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
    ssrc_ffmpeg_hashmap: Arc<RwLock<HashMap<u32, Child>>>,
    user_id_hashmap: Arc<RwLock<HashMap<u64, u32>>>,
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
        s_r: Arc<(
            // Handler
            tokio::sync::broadcast::Sender<i32>,
            // Receiver
            tokio::sync::broadcast::Sender<i32>,
        )>,
    ) -> Self {
        // You can manage state here, such as a buffer of audio packet bytes, so
        // you can later store them in intervals.

        let me: Receiver = Self {
            inner: Arc::new(InnerReceiver {
                pool,
                now: Arc::new(RwLock::new(HashMap::new())),
                file_name: Arc::new(RwLock::new(HashMap::new())),
                ctx_main: ctx,
                user_id_hashmap: Arc::new(RwLock::new(HashMap::new())),
                ssrc_ffmpeg_hashmap: Arc::new(RwLock::new(HashMap::new())),
                guild_id,
                channel_id,
                // member_struct_ssrc: Arc::new(RwLock::new(HashMap::new())),
                // member_struct_id: Arc::new(RwLock::new(HashMap::new())),
            }),
        };

        me.spawn_task((s_r.0.clone(), s_r.1.clone())).await;

        me
    }

    pub async fn spawn_task(
        &self,
        s_r: (
            // Handler
            tokio::sync::broadcast::Sender<i32>,
            // Receiver
            tokio::sync::broadcast::Sender<i32>,
        ),
    ) -> JoinHandle<()> {
        info!("Spawn task");

        let clone_ffmpeg = self.inner.ssrc_ffmpeg_hashmap.clone();
        let clone_now = self.inner.now.clone();
        let clone_file = self.inner.file_name.clone();
        let clone_user_id = self.inner.ssrc_ffmpeg_hashmap.clone();
        let clone_pool = self.inner.pool.clone();

        tokio::task::spawn(async move {
            let clo_ffmpeg = clone_ffmpeg;
            let clo_now = clone_now;
            let clo_file = clone_file;
            let clo_user_id = clone_user_id;
            let clo_pool = clone_pool;
            let mut receiver_receiver = s_r.1.subscribe();
            loop {
                info!("ready to receive termination signal");
                // Temporary solution. We wait and hopefully the file is processed
                let res = receiver_receiver.recv().await.unwrap();
                if res == 1 {
                    tokio::time::sleep(Duration::from_secs(10)).await;

                    s_r.0.send(2).unwrap();
                } else {
                    info!("Unknown code");
                    tokio::time::sleep(Duration::from_secs(1)).await;

                    s_r.0.send(2).unwrap();
                }
            }
        })
    }
}

#[async_trait]
impl VoiceEventHandler for Receiver {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        use EventContext as Ctx;
        match ctx {
            Ctx::SpeakingStateUpdate(Speaking {
                speaking,
                ssrc,
                user_id,
                ..
            }) => {
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

                let guild = self
                    .inner
                    .ctx_main
                    .cache
                    .guild(self.inner.guild_id)
                    .unwrap()
                    .to_owned();

                let member = match guild.member(&self.inner.ctx_main, user_id.0).await {
                    Ok(m) => m,
                    Err(e) => {
                        error!("Failed to get member: {}", e);
                        return None;
                    }
                };

                if member.user.bot {
                    info!("is a bot");
                } else {
                    info!("is NOT bot");

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
                        if self
                            .inner
                            .ssrc_ffmpeg_hashmap
                            .read()
                            .await
                            .get(ssrc)
                            .is_some()
                        {
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
                            let child = spawn_ffmpeg(&path);
                            self.inner
                                .ssrc_ffmpeg_hashmap
                                .write()
                                .await
                                .insert(*ssrc, child);

                            info!("1 file created for ssrc: {}", *ssrc);
                        }
                    }
                }
            }

            Ctx::RtpPacket(packet) => {
                // Those are the un decoded opus packets
            }
            Ctx::VoiceTick(tick) => {
                for (ssrc, data) in &tick.speaking {
                    if let Some(child) = self.inner.ssrc_ffmpeg_hashmap.write().await.get_mut(ssrc)
                    {
                        if let Some(stdin) = child.stdin.as_mut() {
                            if let Some(decoded_voice) = data.decoded_voice.as_ref() {
                                let mut result: Vec<u8> = Vec::new();

                                for &n in decoded_voice {
                                    // TODO: Use buffer
                                    let _ = result.write_i16_le(n).await;
                                }

                                match stdin.write_all(&result).await {
                                    Ok(_) => {}
                                    Err(err) => {
                                        error!("Could not write to stdin: {}", err)
                                    }
                                };
                            } else {
                                error!("Decode disabled");
                            }
                        } else {
                            error!("no stdin");
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

            Ctx::DriverDisconnect(DisconnectData {
                kind,
                reason,
                channel_id,
                guild_id,
                session_id,
                ..
            }) => {
                info!("Disconnected \n kind: {:?} \n reason {:?}", kind, reason)
            }
            Ctx::DriverConnect(ConnectData {
                channel_id,
                guild_id,
                session_id,
                server,
                ssrc,
                ..
            }) => {
                info!("Connected")
            }
            Ctx::DriverReconnect(ConnectData {
                channel_id,
                guild_id,
                session_id,
                server,
                ssrc,
                ..
            }) => {
                info!("Reconnected")
            }

            Ctx::ClientDisconnect(ClientDisconnect { user_id }) => {
                error!("client disconnected id: {}", user_id);

                let ssrc = match self.inner.user_id_hashmap.write().await.remove(&user_id.0) {
                    Some(ok) => ok,
                    None => {
                        info!("tried to remove bot");
                        return None;
                    }
                };

                let mut child = match self.inner.ssrc_ffmpeg_hashmap.write().await.remove(&ssrc) {
                    Some(c) => c,
                    None => {
                        error!("No child process found for ssrc {}", ssrc);
                        return None;
                    }
                };

                info!("wait for child");
                // Close stdin so ffmpeg knows the stream has ended and can exit
                drop(child.stdin.take());

                // Note: waiting with output requires stderr/stdout to be properly piped
                // otherwise this can hang or fail
                let output = match child.wait_with_output().await {
                    Ok(out) => out,
                    Err(e) => {
                        error!("Failed to wait for child process: {}", e);
                        return None;
                    }
                };
                info!("wait is over");

                let stdout = String::from_utf8(output.stdout).unwrap_or_default();
                let stderr = String::from_utf8(output.stderr).unwrap_or_default();

                info!("stdout {:#?}", stdout);
                info!("stderr {:#?}", stderr);

                let lock_now = self.inner.now.read().await;
                let now = match lock_now.get(&ssrc) {
                    Some(t) => t,
                    None => {
                        error!("No start time found for ssrc {}", ssrc);
                        return None;
                    }
                };
                let time_elapsed = chrono::Utc::now()
                    .signed_duration_since(*now)
                    .num_milliseconds();

                let lock_file: tokio::sync::RwLockReadGuard<'_, HashMap<u32, String>> =
                    self.inner.file_name.read().await;
                let file_name = match lock_file.get(&ssrc) {
                    Some(f) => f,
                    None => {
                        error!("No file name found for ssrc {}", ssrc);
                        return None;
                    }
                };

                let last_person_in_channel = { self.inner.user_id_hashmap.read().await.len() == 0 };
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
                .execute(&self.inner.pool)
                .await
                {
                    Ok(ok) => {
                        info!("Updated table row");
                        ok
                    }
                    Err(err) => {
                        error!("{}", err);
                        panic!()
                    }
                };
            }
            _ => {
                // We won't be registering this struct for any more event classes.
                unimplemented!()
            }
        }

        None
    }
}

fn spawn_ffmpeg(path: &str) -> Child {
    let command = Command::new("ffmpeg")
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
        // We use the -use_wallclock_as_timestamps argument
        .args(["-flush_packets", "1"]) // Write to the file on every packet. While this is wasteful it allows semi realtime audio playback.
        .arg(format!("{}.ogg", path)) // output
        .stdin(std::process::Stdio::piped())
        // We read from stdout/stderr later with wait_with_output, so they must be piped
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn();

    command.unwrap()
}

// TODO: username instead of id
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
    let month = format!("{}", now.format("%B"));
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
            panic!()
        }
    };

    combined_path
}
