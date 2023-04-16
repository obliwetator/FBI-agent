use crate::Handler;
use serenity::{
    async_trait,
    client::Context,
    model::id::{ChannelId, GuildId},
    prelude::Mutex,
};
use songbird::{
    model::payload::{ClientDisconnect, Speaking},
    CoreEvent, Event, EventContext, EventHandler as VoiceEventHandler,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
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
    handle: JoinHandle<()>,
    tx: tokio::sync::mpsc::Sender<CustomMsg>,
    user_id: Option<u64>,
    // rx: tokio::sync::mpsc::Receiver<CustomMsg>,
}

#[derive(Debug)]
enum CustomMsg {
    Get { speaking: bool },
    Set { speaking: bool },
}

#[derive(Clone)]
struct Receiver {
    ctx_main: Arc<Context>,
    /// the key is the ssrc, the value is the user id
    /// If the value is none that means we don't want to record that user (for now bots)
    ssrc_hashmap: Arc<Mutex<HashMap<u32, SsrcStruct>>>,
    user_id_hashmap: Arc<Mutex<HashMap<u64, u32>>>,
    ssrc_ffmpeg_hashmap: Arc<Mutex<HashMap<u32, Child>>>,
    // now: Arc<Mutex<HashMap<u32, std::time::Instant>>>,
    guild_id: GuildId,
    channel_id: ChannelId,
    buffer: Arc<Mutex<HashMap<u32, Vec<i16>>>>,

    is_speaking: Arc<Mutex<HashMap<u32, bool>>>,
}

impl Receiver {
    pub async fn new(ctx: Arc<Context>, guild_id: GuildId, channel_id: ChannelId) -> Self {
        // You can manage state here, such as a buffer of audio packet bytes so
        // you can later store them in intervals.
        Self {
            ctx_main: ctx,
            ssrc_hashmap: Arc::new(Mutex::new(HashMap::new())),
            user_id_hashmap: Arc::new(Mutex::new(HashMap::new())),
            ssrc_ffmpeg_hashmap: Arc::new(Mutex::new(HashMap::new())),
            // now: Arc::new(Mutex::new(HashMap::new())),
            guild_id,
            channel_id,
            buffer: Arc::new(Mutex::new(HashMap::new())),

            is_speaking: Arc::new(Mutex::new(HashMap::new())),
        }
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
            let _ = manager.remove(self.guild_id).await;
        }
    }

    pub async fn spawn_task(
        &self,
        ssrc: u32,
        mut rx: tokio::sync::mpsc::Receiver<CustomMsg>,
    ) -> JoinHandle<()> {
        tokio::task::spawn(async move {
            // let mut interval = tokio::time::interval(std::time::Duration::from_millis(2000));
            while let Some(cmd) = rx.recv().await {
                info!("Received msg for {} with the content {:#?}", ssrc, cmd);
            }
        })
    }
}

#[async_trait]
impl VoiceEventHandler for Receiver {
    #[allow(unused_variables)]
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
                    // {
                    //     self.is_speaking.lock().await.insert(*ssrc, false);
                    // }

                    {
                        self.buffer.lock().await.insert(*ssrc, vec![]);
                    }
                    info!("is NOT bot");
                    // no user in map add
                    {
                        // Have to clone outisde the task
                        let ssrc_clone = ssrc.clone();
                        let (tx, rx) = mpsc::channel::<CustomMsg>(32);
                        let task = self.spawn_task(ssrc_clone, rx).await;

                        let data = SsrcStruct {
                            handle: task,
                            user_id: Some(user_id.unwrap().0),
                            tx,
                        };

                        self.ssrc_hashmap.lock().await.insert(*ssrc, data);
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

                // if !data.speaking {
                //     {
                //         if let Some(speaking) = self.is_speaking.lock().await.get_mut(&data.ssrc) {
                //             *speaking = false;
                //         }
                //         if let Some(duration) = self.how_long.lock().await.get(&data.ssrc) {
                //             *self.duration.lock().await.get_mut(&data.ssrc).unwrap() =
                //                 duration.elapsed().as_millis();
                //         }
                //     }
                // } else {
                //     let mut res = self.is_speaking.lock().await;
                //     if let Some(speaking) = res.get_mut(&data.ssrc) {
                //         *speaking = true;
                //     }
                // }
            }
            Ctx::VoicePacket(data) => {
                let res = self.is_speaking.lock().await;

                // let is_speaking = res.get(&data.packet.ssrc);

                // if let Some(is_speaking) = is_speaking {
                //     if *is_speaking {
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
                            self.leave_voice_channel().await;
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

                self.leave_voice_channel().await;
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
    if new_state.channel_id.is_none() {
        // Someone left the channel
        // TODO: Logic to stop recording if < x people

        // if let Some(channel) = old_state.unwrap().channel_id {
        //     if channel
        //         .to_channel_cached(&ctx)
        //         .unwrap()
        //         .guild()
        //         .unwrap()
        //         .members(&ctx)
        //         .await
        //         .unwrap()
        //         .len()
        //         == 1
        //     // just the bot is left
        //     // TODO: fix that atrocity ^
        //     // TODO: Other bots might be present in the channel. Don't count bots
        //     {
        //         leave_voice_channel(&ctx, new_state.guild_id.unwrap()).await;
        //     }
        // }

        return;
    }

    if new_state.member.unwrap().user.bot {
        // Ignore bots
        info!("bot");
        return;
    }

    if let Some(old) = old_state {
        if new_state.channel_id.unwrap() == old.channel_id.unwrap() {
            // An action happened that was NOT switching channels.
            // We don't care about those

            return;
        }
    }

    // The bot will join a voice channel with the following priorities
    // 1) There must be at least 3 people in a channel
    // 2) If 2 or more channels have the same count, will join the channel that has the highest average role (TODO)
    // 3) If above is equal will join a channel with the oldest member

    let all_channels = ctx
        .cache
        .guild(new_state.guild_id.unwrap())
        .expect("cannot clone guild from cache")
        .channels;

    let mut highest_channel_id: ChannelId = ChannelId(0);
    let mut highest_channel_len: usize = 0;

    for (channel_id, guild_channel) in all_channels {
        match guild_channel {
            serenity::model::prelude::Channel::Guild(guild_guild_channel) => {
                if let serenity::model::prelude::ChannelType::Voice = guild_guild_channel.kind {
                    let count = match guild_guild_channel.members(&ctx).await {
                        Ok(ok) => ok,
                        Err(_) => {
                            error!("This should not trigger");
                            return;
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

    info!("highest channel id: {}", highest_channel_id);
    info!("highest channel len: {}", highest_channel_len);

    if highest_channel_len > 0 {
        connect_to_voice_channel(&ctx, new_state.guild_id.unwrap(), highest_channel_id).await;
    }
}

pub async fn connect_to_voice_channel(ctx: &Context, guild_id: GuildId, channel_id: ChannelId) {
    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    info!("CH {:?}", channel_id);

    // Don't connect to a channel we are already in
    // TODO: will have to re-register all the event handler + songbird might already be doing this for us
    // {
    //     // Bot is already in the channel we are trying to connect.
    //     if let Some(result) = manager.get(guild_id) {
    //         let channel = { result.lock().await.current_channel().unwrap() };
    //         if channel == channel_id.into() {
    //             info!("bot trying to connect to the same channel");
    //             return;
    //         }
    //     }
    // }

    if let Some(arc_call) = manager.get(guild_id) {
        // alreday have the call dont rejoin
        info!("already in channel");
    } else {
        // join channel

        let (handler_lock, res) = manager.join(guild_id, channel_id).await;
        match res {
            Ok(_) => {
                info!("Joined {}", channel_id);
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
                let receiver = Receiver::new(ctx1, guild_id, channel_id).await;

                handler.add_global_event(CoreEvent::SpeakingStateUpdate.into(), receiver.clone());

                handler.add_global_event(CoreEvent::SpeakingUpdate.into(), receiver.clone());

                handler.add_global_event(CoreEvent::VoicePacket.into(), receiver.clone());

                handler.add_global_event(CoreEvent::RtcpPacket.into(), receiver.clone());

                // handler.add_global_event(CoreEvent::DriverConnect.into(), receiver.clone());
                // handler.add_global_event(CoreEvent::DriverDisconnect.into(), receiver.clone());

                handler.add_global_event(CoreEvent::ClientDisconnect.into(), receiver.clone());
            }
            Err(err) => {
                manager.remove(guild_id).await.unwrap();
                panic!("cannot join channel 2: {}", err);
            }
        }
    }
}
