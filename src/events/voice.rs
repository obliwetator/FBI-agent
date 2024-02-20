use crate::{event_handler::Handler, get_lock_read, MpmcChannels};
use chrono::Datelike;
use serenity::{
    async_trait,
    client::Context,
    model::{
        id::{ChannelId, GuildId},
        prelude::Member,
    },
    prelude::Mutex,
};
use songbird::{
    model::payload::{ClientDisconnect, Speaking},
    CoreEvent, Event, EventContext, EventHandler as VoiceEventHandler,
};
use sqlx::{Pool, Postgres};
use std::sync::Arc;
use std::{collections::HashMap, time::Duration};
use tokio::{
    io::AsyncWriteExt,
    process::{Child, Command},
};
use tokio::{sync::RwLock, task::JoinHandle};
use tracing::{error, info, trace};

pub const RECORDING_FILE_PATH: &str = "/home/tulipan/projects/FBI-agent/voice_recordings";
pub const CLIPS_FILE_PATH: &str = "/home/tulipan/projects/FBI-agent/clips";
// 6MB buffer. Hold around 30 sec of audio
// const BUFFER_SIZE: usize = 6 * 1024 * 1024;
// const DISCORD_SAMPLE_RATE: u16 = 48000;

#[derive(Clone)]
struct Receiver {
    pool: Pool<Postgres>,
    buffer: Arc<Mutex<HashMap<u32, Vec<i16>>>>,
    channel_id: ChannelId,
    ctx_main: Arc<Context>,
    now: Arc<RwLock<HashMap<u32, chrono::DateTime<chrono::Utc>>>>,
    file_name: Arc<RwLock<HashMap<u32, String>>>,
    guild_id: GuildId,
    // ssrc
    ssrc_ffmpeg_hashmap: Arc<RwLock<HashMap<u32, Child>>>,
    // user_id
    user_id_hashmap: Arc<RwLock<HashMap<u64, u32>>>,
    member_struct_ssrc: Arc<RwLock<HashMap<u32, Arc<VoiceChannelMembersData>>>>,
    member_struct_id: Arc<RwLock<HashMap<u64, Arc<VoiceChannelMembersData>>>>,
}

struct VoiceChannelMembersData {
    now: chrono::DateTime<chrono::Utc>,
    file_name: String,
    ssrc: u32,
    ffmpeg_handle: Child,
    buffer: Vec<i16>,
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
        // You can manage state here, such as a buffer of audio packet bytes so
        // you can later store them in intervals.

        let me: Receiver = Self {
            pool,
            now: Arc::new(RwLock::new(HashMap::new())),
            file_name: Arc::new(RwLock::new(HashMap::new())),
            ctx_main: ctx,
            user_id_hashmap: Arc::new(RwLock::new(HashMap::new())),
            ssrc_ffmpeg_hashmap: Arc::new(RwLock::new(HashMap::new())),
            // now: Arc::new(Mutex::new(HashMap::new())),
            guild_id,
            channel_id,
            buffer: Arc::new(Mutex::new(HashMap::new())),
            member_struct_ssrc: Arc::new(RwLock::new(HashMap::new())),
            member_struct_id: Arc::new(RwLock::new(HashMap::new())),
        };

        me.spawn_task((s_r.0.clone(), s_r.1.clone())).await;

        me
    }

    // pub async fn leave_voice_channel(&self) {
    //     {
    //         info!(
    //             "Size of ssrc hashmap: {}",
    //             self.ssrc_hashmap.lock().await.len()
    //         );
    //     }

    //     if self.ssrc_hashmap.lock().await.is_empty() {
    //         info!("Disconnect self");
    //         // dissconect self.
    //         // might be optional
    //         let manager = songbird::get(&self.ctx_main).await.unwrap();
    //         // let _ = manager.remove(self.guild_id).await;
    //     }
    // }

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

        let clone_ffmpeg = self.ssrc_ffmpeg_hashmap.clone();
        let clone_now = self.now.clone();
        let clone_file = self.file_name.clone();
        let clone_user_id = self.ssrc_ffmpeg_hashmap.clone();
        let clone_pool = self.pool.clone();

        tokio::task::spawn(async move {
            let clo_ffmpeg = clone_ffmpeg;
            let clo_now = clone_now;
            let clo_file = clone_file;
            let clo_user_id = clone_user_id;
            let clo_pool = clone_pool;
            let mut receiver_receiver = s_r.1.subscribe();
            loop {
                // Temporary solution. We wait and hopefully the file is processed
                let res = receiver_receiver.recv().await.unwrap();
                if res == 1 {
                    info!("RES == 1");
                    tokio::time::sleep(Duration::from_secs(1)).await;

                    s_r.0.send(2).unwrap();
                } else if res == 2 {
                    info!("RES == 2");
                    tokio::time::sleep(Duration::from_secs(1)).await;

                    s_r.0.send(1).unwrap();
                } else {
                    info!("Unkown code");
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

                let member = self
                    .ctx_main
                    .cache
                    .member(self.guild_id, user_id.unwrap().0)
                    .unwrap()
                    .to_owned();

                if member.user.bot {
                    info!("is a bot");
                    // Don't record bots
                    {
                        // self.ssrc_hashmap.lock().await.insert(*ssrc, None);
                    }
                } else {
                    let now = chrono::Utc::now();

                    {
                        self.buffer.lock().await.insert(*ssrc, vec![]);
                    }
                    info!("is NOT bot");

                    let is_channel_empty = {
                        let len = self.user_id_hashmap.read().await.len();
                        len == 0
                    };

                    {
                        self.user_id_hashmap
                            .write()
                            .await
                            .insert(user_id.unwrap().0, *ssrc);
                    }

                    // self.was_only_user = { self.user_id_hashmap };

                    {
                        if self.ssrc_ffmpeg_hashmap.read().await.get(ssrc).is_some() {
                            // we have already spawned an ffmpeg process
                            error!("already got ffmpegf process");
                        } else {
                            info!("New ffmpegf process");
                            // Create a process
                            let now = chrono::Utc::now();
                            {
                                self.now.write().await.insert(*ssrc, now);
                            }
                            let path = create_path(
                                self,
                                now,
                                &self.pool,
                                *ssrc,
                                self.guild_id,
                                self.channel_id,
                                user_id.unwrap().0,
                                &member,
                                is_channel_empty,
                            )
                            .await;
                            let child = spawn_ffmpeg(&path);

                            // let vec = Arc::new(VoiceChannelMembersData {
                            //     buffer: vec![],
                            //     file_name: "".to_string(),
                            //     now: now,
                            //     ssrc: *ssrc,
                            //     ffmpeg_handle: child,
                            // });

                            // {
                            //     let a = self
                            //         .member_struct_id
                            //         .write()
                            //         .await
                            //         .insert(user_id.unwrap().0, vec.clone());
                            // }

                            // {
                            //     let b = self
                            //         .member_struct_ssrc
                            //         .write()
                            //         .await
                            //         .insert(*ssrc, vec.clone());
                            // }
                            self.ssrc_ffmpeg_hashmap.write().await.insert(*ssrc, child);

                            info!("1 file created for ssrc: {}", *ssrc);
                        }
                    }
                }
            }

            Ctx::RtpPacket(packet) => {
                // Those are the undecoded opus packets
            }
            Ctx::VoiceTick(tick) => {
                for (ssrc, data) in &tick.speaking {
                    if let Some(child) = self.ssrc_ffmpeg_hashmap.write().await.get_mut(ssrc) {
                        // let mut buffer = self.buffer.lock().await;
                        // let res = buffer.get_mut(ssrc).unwrap();
                        if let Some(stdin) = child.stdin.as_mut() {
                            // let mut result: Vec<u8> = Vec::new();

                            // let _ = result.write_all(rtp.payload()).await;

                            // for &n in audio_i16 {
                            //     // TODO: Use buffer
                            //     let _ = result.write_i16_le(n).await;
                            // }
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
                                info!(" Decode disabled.");
                            }
                        } else {
                            info!("no stdin");
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

            Ctx::ClientDisconnect(ClientDisconnect { user_id }) => {
                error!("client disconets id: {}", user_id);
                // You can implement your own logic here to handle a user who has left the
                // voice channel e.g., finalise processing of statistics etc.
                // You will typically need to map the User ID to their SSRC; observed when
                // speaking or connecting.

                // {
                //     // Bots
                //     let get_user_hashmap = self.user_id_hashmap.read().await;
                //     if let Some(get_user_ssrc) = get_user_hashmap.get(&user_id.0) {
                //         let mut get_user_ssrc_hashmap = self.ssrc_hashmap.write().await;
                //         if let Some(data) = get_user_ssrc_hashmap.remove(get_user_ssrc) {
                //             if data.user_id.is_none() {
                //                 // bot ignore
                //                 info!("bot ignore");
                //             }
                //         } else {
                //             error!("no ssrc in hashmap");
                //         }
                //     } else {
                //         error!("no user id in hashmap");
                //     }
                // }

                {
                    let ssrc = match self.user_id_hashmap.write().await.remove(&user_id.0) {
                        Some(ok) => ok,
                        None => {
                            info!("tried to remove bot");
                            // self.leave_voice_channel().await;
                            return None;
                        }
                    };

                    let child = self
                        .ssrc_ffmpeg_hashmap
                        .write()
                        .await
                        .remove(&ssrc)
                        .unwrap();

                    let output = child.wait_with_output().await.unwrap();

                    let lock_now = self.now.read().await;
                    let now = lock_now.get(&ssrc).unwrap();

                    let time_elapsed = chrono::Utc::now()
                        .signed_duration_since(*now)
                        .num_milliseconds();

                    let lock_file: tokio::sync::RwLockReadGuard<'_, HashMap<u32, String>> =
                        self.file_name.read().await;
                    let file_name = lock_file.get(&ssrc).unwrap();

                    let last_person_in_channel = { self.user_id_hashmap.read().await.len() == 0 };
                    // 2 = JOINED 3 = LAST 4 = FIRST_LAST
                    let state = {
                        if last_person_in_channel {
                            3
                        } else {
                            2
                        }
                    };

                    match sqlx::query!(
                        "UPDATE audio_files
						SET end_ts = audio_files.start_ts + $1, state_leave = $2
						WHERE file_name = $3",
                        time_elapsed,
                        state,
                        file_name
                    )
                    .execute(&self.pool)
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
        .args(["-async", "1"]) // this will input silence when there is no packets comming.
        // HOWEVER. It will not work if the packets do not have a timestamp attached to them. We can tell ffmpeg to attach its own timestamps and figure the timings by itself
        // We use the -use_wallclock_as_timestamps argument
        .args(["-flush_packets", "1"]) // Write to the file on every packet. While this is wasteful it allows semi realtime audio playback.
        .arg(format!("{}.ogg", path)) // output
        .stdin(std::process::Stdio::piped())
        // The command will hangup if the pipe is not consumed. So set it to null if we are not doing anything with it.
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
    // Delete the
    match std::fs::create_dir_all(dir_path) {
        Ok(_) => {}
        Err(err) => {
            panic!("cannot create path: {}", err);
        }
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

pub async fn voice_server_update(
    _self: &Handler,
    _ctx: Context,
    _update: serenity::model::event::VoiceServerUpdateEvent,
) {
}

pub async fn voice_state_update(
    _self: &Handler,
    ctx: Context,
    old_state: Option<serenity::model::prelude::VoiceState>,
    new_state: serenity::model::prelude::VoiceState,
) {
    info!("Voice state update");
    if let Some(member) = &new_state.member {
        if member.user.bot {
            // Ignore bots
            info!("bot");
            return;
        }

        let leave = match handle_no_people_in_channel(&new_state, &ctx, &old_state, member).await {
            Some(value) => value,
            None => true,
        };

        if let Some(old) = old_state {
            if new_state.channel_id.is_some() {
                if new_state.channel_id.unwrap() == old.channel_id.unwrap() {
                    // An action happened that was NOT switching channels.
                    // We don't care about those
                    info!("An action happened that was NOT switching channels");
                    return;
                }
            }
        }

        let (highest_channel_id, highest_channel_len) =
            match get_channel_with_most_members(&ctx, &new_state).await {
                Some(value) => value,
                None => {
                    leave_voice_channel(&ctx, new_state.guild_id.unwrap()).await;
                    return;
                }
            };

        if highest_channel_len > 0 {
            let user_id = new_state.user_id;
            connect_to_voice_channel(
                _self.database.clone(),
                &ctx,
                new_state.guild_id.unwrap(),
                highest_channel_id,
                user_id.get(),
            )
            .await;
        } else if leave {
            info!("Leave");
            leave_voice_channel(&ctx, new_state.guild_id.unwrap()).await;
        }
    } else {
        error!("No member in new_state");
        panic!();
    }
}

// If no people are in the current channels we are safe to leave
async fn handle_no_people_in_channel(
    new_state: &serenity::model::prelude::VoiceState,
    ctx: &Context,
    old_state: &Option<serenity::model::prelude::VoiceState>,
    member: &Member,
) -> Option<bool> {
    let mut leave = false;
    if new_state.channel_id.is_none() {
        if member.user.id == ctx.cache.current_user().id {
            // From testing this will never trigger because the bot is disconnected before an event is received
            // Check if the our application was kicked
            info!("bot was kicked/left");
            let guild_id = new_state.guild_id.unwrap();
            leave = true;
            leave_voice_channel(ctx, guild_id).await;
        }
        // Someone left the channel
        // Check if that person was the last HUMAN user to leave the channel
        // NOTE: There can be other people in different channels. Check for this

        if let Some(channel_id) = old_state.as_ref().unwrap().channel_id {
            let current_channel = channel_id.to_channel(&ctx).await.unwrap();

            let guild_channel = current_channel.guild().unwrap();

            let vec = guild_channel.members(&ctx).unwrap();
            let members: Vec<Member> = vec
                .into_iter()
                // Don't include bots
                .filter(|f| !f.user.bot)
                .collect();

            // No Human users left
            if members.len() == 0
            // just the bot is left
            {
                info!("No more human users left. Leaving channel");
                // leave = true;
                // leave_voice_channel(&ctx, new_state.guild_id.unwrap()).await;
                return None;
            } else {
                trace!("Human users still in channel.");
            }
        }
    }
    Some(leave)
}

async fn get_channel_with_most_members(
    ctx: &Context,
    new_state: &serenity::model::prelude::VoiceState,
) -> Option<(ChannelId, usize)> {
    let lock = get_lock_read(ctx).await;

    let lock_guard = lock.read().await;
    let afk_channel_id_option = lock_guard.get(&new_state.guild_id.unwrap().get()).unwrap();
    let all_channels = &ctx
        .cache
        .guild(new_state.guild_id.unwrap())
        .expect("cannot clone guild from cache")
        .channels;
    let mut highest_channel_id: ChannelId = ChannelId::new(1);
    let mut highest_channel_len: usize = 0;
    for (channel_id, guild_channel) in all_channels {
        if let Some(afk_channel_id) = afk_channel_id_option {
            // Ignore channels that are meant for afk
            if *afk_channel_id == channel_id.get() {
                info!("Ignore AFK channel");
                continue;
            }
        }
        if let serenity::model::prelude::ChannelType::Voice = guild_channel.kind {
            let count = match guild_channel.members(ctx) {
                Ok(mut ok) => {
                    ok.retain(|f| !f.user.bot);
                    ok
                }
                Err(_) => {
                    error!("This should not trigger");
                    return None;
                }
            };

            if count.len() > highest_channel_len {
                highest_channel_len = count.len();
                highest_channel_id = guild_channel.id;
            }
        }
    }
    Some((highest_channel_id, highest_channel_len))
}

async fn leave_voice_channel(ctx: &Context, guild_id: GuildId) {
    {
        let lock = {
            let data_read = ctx.data.read().await;
            data_read
                .get::<MpmcChannels>()
                .expect("Expected struct")
                .clone()
        };

        let hash_map = lock.write().await;
        let (handler, receiver) = hash_map
            .get(&guild_id.get())
            .expect("channel not innitialized");

        handle_graceful_shutdown(handler.clone(), receiver.clone(), 1).await;
    }

    {
        // Cleanup channel
        let lock = {
            let data_read = ctx.data.read().await;
            data_read
                .get::<MpmcChannels>()
                .expect("Expected struct")
                .clone()
        };
        let mut hash_map = lock.write().await;

        if hash_map.contains_key(&guild_id.get()) {
            // Clean the channels
            let _ = hash_map.remove(&guild_id.get());
        };
    }

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    let _ = manager.remove(guild_id).await;

    info!("Left the voice channel");
}

pub async fn connect_to_voice_channel(
    pool: Pool<Postgres>,
    ctx: &Context,
    guild_id: GuildId,
    channel_id: ChannelId,
    user_id: u64,
) {
    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    if let Some(arc_call) = manager.get(guild_id) {
        // alreday have the call dont rejoin
        let ch = arc_call.lock().await.current_channel().unwrap();

        if ch.0.get() as u64 != channel_id.get() {
            info!("Switching channels");
            join_ch(
                pool,
                manager,
                guild_id,
                channel_id,
                ctx,
                user_id,
                Some(ch.0.get()),
            )
            .await;
        } else {
            info!("already in channel");
        }
    } else {
        // join channel

        join_ch(pool, manager, guild_id, channel_id, ctx, user_id, None).await;
    }
}

async fn join_ch(
    pool: Pool<Postgres>,
    manager: Arc<songbird::Songbird>,
    guild_id: GuildId,
    channel_id: ChannelId,
    ctx: &Context,
    user_id: u64,
    old_channel: Option<u64>,
) {
    let lock = {
        let data_read = ctx.data.read().await;
        data_read
            .get::<MpmcChannels>()
            .expect("Expected struct")
            .clone()
    };

    let mut hash_map = lock.write().await;
    let channel = {
        let result = match hash_map.get(&guild_id.get()) {
            Some(ok) => {
                info!("channels already exists");

                let handler = ok.0.clone();
                let receiver = ok.1.clone();
                handle_graceful_shutdown(handler, receiver, 2).await;

                ok
            }
            None => {
                info!("new channels");
                let (sender_for_handler, _) = tokio::sync::broadcast::channel::<i32>(10);
                let (sender_for_receiver, _) = tokio::sync::broadcast::channel::<i32>(10);
                hash_map.insert(guild_id.get(), (sender_for_handler, sender_for_receiver));

                hash_map.get(&guild_id.get()).unwrap()
            }
        };
        (result.0.clone(), result.1.clone())
    };

    let result_handler_lock = manager.join(guild_id, channel_id).await;
    match result_handler_lock {
        Ok(handler_lock) => {
            info!("Joined {}", channel_id);

            if let Some(old_ch) = old_channel {
                // switching channels. Don't re-register. Cleanup
                info!("Clean up switching chanels");

                // hash_map
                //     .remove(&guild_id.0)
                //     .expect("Expected key from old channel when switching channels");
            } else {
                let mut handler = handler_lock.lock().await;
                // let res = handler.join(channel_id).await;
                // match res {
                //     Ok(_) => {}
                //     Err(err) => {
                //         panic!("cannot join channel 1: {}", err);
                //     }
                // }
                // Remove any old events in case the bot swaps channels
                // handler.remove_all_global_events();

                let ctx1 = Arc::new(ctx.clone());
                let receiver =
                    Receiver::new(pool, ctx1, guild_id, channel_id, Arc::new(channel)).await;

                handler.add_global_event(CoreEvent::SpeakingStateUpdate.into(), receiver.clone());

                handler.add_global_event(CoreEvent::VoiceTick.into(), receiver.clone());

                // handler.add_global_event(CoreEvent::RtpPacket.into(), receiver.clone());

                handler.add_global_event(CoreEvent::RtcpPacket.into(), receiver.clone());

                handler.add_global_event(CoreEvent::ClientDisconnect.into(), receiver.clone());

                // handler.add_global_event(CoreEvent::DriverConnect.into(), receiver.clone());
                // handler.add_global_event(CoreEvent::DriverDisconnect.into(), receiver.clone());
            }
        }
        Err(err) => {
            manager.remove(guild_id).await.unwrap();
            panic!("cannot join channel 2: {}", err);
        }
    }
}

async fn handle_graceful_shutdown(
    handler: tokio::sync::broadcast::Sender<i32>,
    receiver: tokio::sync::broadcast::Sender<i32>,
    code: i32,
) {
    info!("handle graceful shutdown");
    // Subscribe for a response
    let mut r1 = handler.subscribe();

    // Termination signal
    receiver.send(code).unwrap();

    // Get response
    let res = r1.recv().await.unwrap();

    if res == 2 {
        info!("Response received, shutting down")
    } else if res == 1 {
        info!("Response received, cleanup complete")
    } else {
        error!("RES IS :{}", res);
        panic!();
    }
}
