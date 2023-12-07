use crate::{event_handler::Handler, get_lock_read, MpmcChannels};
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
use std::sync::Arc;
use std::{collections::HashMap, time::Duration};
use tokio::task::JoinHandle;
use tokio::{
    io::AsyncWriteExt,
    process::{Child, Command},
};
use tracing::{error, info};

pub const RECORDING_FILE_PATH: &str = "/home/tulipan/projects/FBI-agent/voice_recordings";
pub const CLIPS_FILE_PATH: &str = "/home/tulipan/projects/FBI-agent/clips";
// 6MB buffer. Hold around 30 sec of audio
// const BUFFER_SIZE: usize = 6 * 1024 * 1024;
// const DISCORD_SAMPLE_RATE: u16 = 48000;

struct SsrcStruct {
    // handle: JoinHandle<()>,
    // tx: tokio::sync::mpsc::Sender<CustomMsg>,
    user_id: Option<u64>,
    // rx: tokio::sync::mpsc::Receiver<CustomMsg>,
}

#[derive(Debug)]
enum CustomMsg {
    // Get { speaking: bool },
    // Set { speaking: bool },
}

#[derive(Clone)]
struct Receiver {
    buffer: Arc<Mutex<HashMap<u32, Vec<i16>>>>,
    channel_id: ChannelId,
    ctx_main: Arc<Context>,
    // now: Arc<Mutex<HashMap<u32, std::time::Instant>>>,
    guild_id: GuildId,
    s_r: Arc<(
        tokio::sync::broadcast::Sender<i32>,
        tokio::sync::broadcast::Receiver<i32>,
    )>,
    ssrc_ffmpeg_hashmap: Arc<Mutex<HashMap<u32, Child>>>,
    /// the key is the ssrc, the value is the user id
    /// If the value is none that means we don't want to record that user (for now bots)
    ssrc_hashmap: Arc<Mutex<HashMap<u32, SsrcStruct>>>,
    user_id_hashmap: Arc<Mutex<HashMap<u64, u32>>>,
}

impl Receiver {
    pub async fn new(
        ctx: Arc<Context>,
        guild_id: GuildId,
        channel_id: ChannelId,
        s_r: Arc<(
            tokio::sync::broadcast::Sender<i32>,
            tokio::sync::broadcast::Receiver<i32>,
        )>,
    ) -> Self {
        // You can manage state here, such as a buffer of audio packet bytes so
        // you can later store them in intervals.

        let me = Self {
            ctx_main: ctx,
            ssrc_hashmap: Arc::new(Mutex::new(HashMap::new())),
            user_id_hashmap: Arc::new(Mutex::new(HashMap::new())),
            ssrc_ffmpeg_hashmap: Arc::new(Mutex::new(HashMap::new())),
            // now: Arc::new(Mutex::new(HashMap::new())),
            guild_id,
            channel_id,
            buffer: Arc::new(Mutex::new(HashMap::new())),
            s_r: s_r.clone(),
        };

        me.spawn_task((s_r.0.clone(), s_r.0.subscribe())).await;

        me
    }

    pub async fn leave_voice_channel(&self) {
        {
            info!(
                "Size of ssrc hashmap: {}",
                self.ssrc_hashmap.lock().await.len()
            );
        }

        if self.ssrc_hashmap.lock().await.is_empty() {
            info!("Disconnect self");
            // dissconect self.
            // might be optional
            let manager = songbird::get(&self.ctx_main).await.unwrap();
            // let _ = manager.remove(self.guild_id).await;
        }
    }

    pub async fn spawn_task(
        &self,
        mut s_r: (
            tokio::sync::broadcast::Sender<i32>,
            tokio::sync::broadcast::Receiver<i32>,
        ),
    ) -> JoinHandle<()> {
        tokio::task::spawn(async move {
            loop {
                let res = s_r.1.recv().await.unwrap();
                if res == 2 {
                    let res = s_r.1.recv().await.unwrap();

                    tokio::time::sleep(Duration::from_secs(1)).await;

                    s_r.0.send(2).unwrap();
                } else {
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
                    .unwrap();

                if member.user.bot {
                    info!("is a bot");
                    // Don't record bots
                    {
                        // self.ssrc_hashmap.lock().await.insert(*ssrc, None);
                    }
                } else {
                    {
                        self.buffer.lock().await.insert(*ssrc, vec![]);
                    }
                    info!("is NOT bot");
                    // no user in map add
                    {
                        // Have to clone outisde the task
                        // let ssrc_clone = ssrc.clone();
                        // let (tx, rx) = mpsc::channel::<CustomMsg>(32);
                        // let task = self.spawn_task(ssrc_clone, rx).await;

                        // let data = SsrcStruct {
                        //     handle: task,
                        //     user_id: Some(user_id.unwrap().0),
                        //     tx,
                        // };

                        // self.ssrc_hashmap.lock().await.insert(*ssrc, data);
                    }

                    {
                        self.user_id_hashmap
                            .lock()
                            .await
                            .insert(user_id.unwrap().0, *ssrc);
                    }

                    {
                        if self.ssrc_ffmpeg_hashmap.lock().await.get(ssrc).is_some() {
                            // we have already spawned an ffmpeg process
                            error!("already got ffmpegf process");
                        } else {
                            info!("New ffmpegf process");
                            // Create a process
                            let path = create_path(
                                *ssrc,
                                self.guild_id,
                                self.channel_id,
                                user_id.unwrap().0,
                                &member,
                            )
                            .await;

                            let child = spawn_ffmpeg(&path);
                            self.ssrc_ffmpeg_hashmap.lock().await.insert(*ssrc, child);

                            info!("1 file created for ssrc: {}", *ssrc);
                        }
                    }
                }
            }
            Ctx::SpeakingUpdate(data) => {
                // You can implement logic here which reacts to a user starting
                // or stopping speaking.
            }
            Ctx::VoicePacket(data) => {
                if let Some(child) = self
                    .ssrc_ffmpeg_hashmap
                    .lock()
                    .await
                    .get_mut(&data.packet.ssrc)
                {
                    if let Some(audio_i16) = data.audio {
                        //     info!(
                        // 	"Audio packet sequence {:05} has {:04} bytes (decompressed from {}), SSRC {}",
                        // 	data.packet.sequence.0,
                        // 	audio_i16.len() * std::mem::size_of::<i16>(),
                        // 	data.packet.payload.len(),
                        // 	data.packet.ssrc,
                        // );

                        // {
                        //     let mut lock = self.size.lock().await;
                        //     let value = *lock.get(&data.packet.ssrc).unwrap();
                        //     // drop(lock);
                        //     *lock.get_mut(&data.packet.ssrc).unwrap() = audio_i16.len() + value;
                        // }

                        let mut buffer = self.buffer.lock().await;
                        let res = buffer.get_mut(&data.packet.ssrc).unwrap();
                        if let Some(stdin) = child.stdin.as_mut() {
                            let mut result: Vec<u8> = Vec::new();

                            for &n in audio_i16 {
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
                            info!("no stdin");
                        }
                    } else {
                        info!("No audio");
                    }
                } else {
                    error!("No child");
                }
                //     } else {
                //     }
                // }

                // info!("Messsage time elapsed micro:{}", now.elapsed().as_micros());
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

                {
                    // Bots
                    let get_user_hashmap = self.user_id_hashmap.lock().await;
                    if let Some(get_user_ssrc) = get_user_hashmap.get(&user_id.0) {
                        let mut get_user_ssrc_hashmap = self.ssrc_hashmap.lock().await;
                        if let Some(data) = get_user_ssrc_hashmap.remove(get_user_ssrc) {
                            if data.user_id.is_none() {
                                // bot ignore
                                info!("bot ignore");
                            }
                        } else {
                            error!("no ssrc in hashmap");
                        }
                    } else {
                        error!("no user id in hashmap");
                    }
                }

                {
                    let ssrc = match self.user_id_hashmap.lock().await.remove(&user_id.0) {
                        Some(ok) => ok,
                        None => {
                            info!("tried to remove bot");
                            // self.leave_voice_channel().await;
                            return None;
                        }
                    };

                    let child = self.ssrc_ffmpeg_hashmap.lock().await.remove(&ssrc).unwrap();

                    let output = child.wait_with_output().await.unwrap();
                    // TODO: Remove
                    info!(
                        "stdout from wait_with_output {}",
                        String::from_utf8(output.stdout).unwrap()
                    );
                    info!(
                        "stderr from wait_with_output {}",
                        String::from_utf8(output.stderr).unwrap()
                    );
                }

                // self.leave_voice_channel().await;
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
        .stderr(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .spawn();

    command.unwrap()
}

// TODO: username instead of id
async fn create_path(
    ssrc: u32,
    guild_id: GuildId,
    channel_id: ChannelId,
    user_id: u64,
    member: &serenity::model::guild::Member,
) -> String {
    let start = std::time::SystemTime::now();
    let since_the_epoch = start
        .duration_since(std::time::UNIX_EPOCH)
        .expect("Time went backwards");

    let now = chrono::Utc::now();
    let year = format!("{}", now.format("%Y"));
    let month = format!("{}", now.format("%B"));
    let day_number = format!("{}", now.format("%d"));
    let day_name = format!("{}", now.format("%A"));
    let hour = format!("{}", now.format("%H"));
    let minute = format!("{}", now.format("%M"));
    let seconds = format!("{}", now.format("%S"));

    let dir_path = format!(
        "{}/{}/{}/{}/{}/",
        &RECORDING_FILE_PATH, guild_id.0, channel_id.0, year, month
    );
    let combined_path = format!(
        "{}/{}/{}/{}/{}/{}-{}-{}",
        &RECORDING_FILE_PATH,
        guild_id.0,
        channel_id.0,
        year,
        month,
        since_the_epoch.as_millis(),
        user_id,
        member.user.name
    );

    // Try to create the dir in case it does not exist
    // Delete the
    match std::fs::create_dir_all(dir_path) {
        Ok(_) => {}
        Err(err) => {
            panic!("cannot create path: {}", err);
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

        // The bot will join a voice channel with the following priorities
        // 1) There must be at least 3 people in a channel
        // 2) If 2 or more channels have the same count, will join the channel that has the highest average role (TODO)
        // 3) If above is equal will join a channel with the oldest member

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
                &ctx,
                new_state.guild_id.unwrap(),
                highest_channel_id,
                user_id.0,
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
        // Someone left the channel
        // Check if that person was the last HUMAN user to leave the channel
        // NOTE: There can be other people in different channels. Check for this

        if let Some(channel) = old_state.as_ref().unwrap().channel_id {
            let members: Vec<Member> = channel
                .to_channel_cached(ctx)
                .unwrap()
                .guild()
                .unwrap()
                .members(ctx)
                .await
                .unwrap()
                .into_iter()
                .filter(|f| !f.user.bot)
                .collect();

            // No Human users left
            if members.len() == 0
            // just the bot is left
            // TODO: fix that atrocity ^
            {
                info!("No more human users left. Leaving channel");
                leave = true;
                // leave_voice_channel(&ctx, new_state.guild_id.unwrap()).await;
                return None;
            }
        }
    }
    // Check if the our application was kicked
    else if member.user.id == ctx.cache.current_user_id() && new_state.channel_id.is_none() {
        info!("bot was kicked/left");
        // Bot Left/Disconnected from the channel
        let guild_id = new_state.guild_id.unwrap();
        leave = true;
        leave_voice_channel(ctx, guild_id).await;
    }
    Some(leave)
}

async fn get_channel_with_most_members(
    ctx: &Context,
    new_state: &serenity::model::prelude::VoiceState,
) -> Option<(ChannelId, usize)> {
    let lock = get_lock_read(ctx).await;

    let lock_guard = lock.read().await;
    let afk_channel_id_option = lock_guard.get(&new_state.guild_id.unwrap().0).unwrap();
    let all_channels = ctx
        .cache
        .guild(new_state.guild_id.unwrap())
        .expect("cannot clone guild from cache")
        .channels;
    let mut highest_channel_id: ChannelId = ChannelId(0);
    let mut highest_channel_len: usize = 0;
    for (channel_id, guild_channel) in all_channels {
        if let Some(afk_channel_id) = afk_channel_id_option {
            // Ignore channels that are meant for afk
            if *afk_channel_id == channel_id.0 {
                info!("Guild has an afk channel with most members. Dont enter");
                continue;
            }
        }
        match guild_channel {
            serenity::model::prelude::Channel::Guild(guild_guild_channel) => {
                if let serenity::model::prelude::ChannelType::Voice = guild_guild_channel.kind {
                    let count = match guild_guild_channel.members(ctx).await {
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
                        highest_channel_id = guild_guild_channel.id;
                    }
                }
            }
            serenity::model::prelude::Channel::Private(ok) => {}
            serenity::model::prelude::Channel::Category(ok) => {}
            _ => {
                error!("unkown channel type");
                unimplemented!()
            }
        }
    }
    Some((highest_channel_id, highest_channel_len))
}

async fn leave_voice_channel(ctx: &Context, guild_id: GuildId) {
    let lock = {
        let data_read = ctx.data.read().await;
        data_read
            .get::<MpmcChannels>()
            .expect("Expected struct")
            .clone()
    };

    let hash_map = lock.write().await;
    let (s, r) = hash_map.get(&guild_id.0).expect("channel not innitialized");

    handle_graceful_shutdown(s.clone()).await;

    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    let _ = manager.remove(guild_id).await;

    info!("Left the voice channel");
}

pub async fn connect_to_voice_channel(
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
        if ch.0 != channel_id.0 {
            info!("Switching channels");
            join_ch(manager, guild_id, channel_id, ctx, user_id, Some(ch.0)).await;
        } else {
            info!("already in channel");
        }
    } else {
        // join channel

        join_ch(manager, guild_id, channel_id, ctx, user_id, None).await;
    }
}

async fn join_ch(
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
        let result = match hash_map.get(&guild_id.0) {
            Some(ok) => {
                info!("channels already exists");

                let s = ok.0.clone();
                handle_graceful_shutdown(s).await;

                ok
            }
            None => {
                info!("new channels");
                let (s, r) = tokio::sync::broadcast::channel::<i32>(10);
                hash_map.insert(guild_id.0, (s, r));

                hash_map.get(&guild_id.0).unwrap()
            }
        };

        (result.0.clone(), result.0.subscribe())
    };

    let (handler_lock, res) = manager.join(guild_id, channel_id).await;
    match res {
        Ok(_) => {
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
                let receiver = Receiver::new(ctx1, guild_id, channel_id, Arc::new(channel)).await;

                handler.add_global_event(CoreEvent::SpeakingStateUpdate.into(), receiver.clone());

                handler.add_global_event(CoreEvent::SpeakingUpdate.into(), receiver.clone());

                handler.add_global_event(CoreEvent::VoicePacket.into(), receiver.clone());

                handler.add_global_event(CoreEvent::RtcpPacket.into(), receiver.clone());

                // handler.add_global_event(CoreEvent::DriverConnect.into(), receiver.clone());
                // handler.add_global_event(CoreEvent::DriverDisconnect.into(), receiver.clone());

                handler.add_global_event(CoreEvent::ClientDisconnect.into(), receiver.clone());
            }
        }
        Err(err) => {
            manager.remove(guild_id).await.unwrap();
            panic!("cannot join channel 2: {}", err);
        }
    }
}

async fn handle_graceful_shutdown(s: tokio::sync::broadcast::Sender<i32>) {
    // Termination signal
    s.send(1).unwrap();

    // Subscribe for a response
    let mut r1 = s.subscribe();

    // Get response
    let res = r1.recv().await.unwrap();

    if res == 2 {
        info!("Response received, shutting down")
    } else {
        error!("RES IS :{}", res);
        panic!();
    }
}
