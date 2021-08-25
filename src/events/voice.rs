use std::collections::HashMap;
use std::sync::Arc;

use serenity::{
    async_trait,
    client::Context,
    model::id::{ChannelId, GuildId},
    prelude::Mutex,
};
use songbird::{
    model::payload::{ClientConnect, ClientDisconnect, Speaking},
    CoreEvent, Event, EventContext, EventHandler as VoiceEventHandler,
};
use tokio::{
    io::AsyncWriteExt,
    process::{Child, Command},
};

pub const RECORDING_FILE_PATH: &str = "/home/ubuntu/FBIagent/voice_recordings";
const DISCORD_SAMPLE_RATE: u16 = 48000;

#[derive(Clone)]
struct Receiver {
    ctx_main: Arc<Context>,
    /// the key is the ssrc, the value is the user id
    /// If the value is none that means we don't want to record that user (for now bots)
    ssrc_hashmap: Arc<Mutex<HashMap<u32, Option<u64>>>>,
    user_id_hashmap: Arc<Mutex<HashMap<u64, u32>>>,
    ssrc_ffmpeg_hashmap: Arc<Mutex<HashMap<u32, Child>>>,
    // Each ssrc needs to have it's own buffer
    buffer: Arc<Mutex<HashMap<u32, Vec<i16>>>>,
    now: Arc<Mutex<HashMap<u32, std::time::Instant>>>,
    guild_id: GuildId,
    user_status: Arc<Mutex<HashMap<u32, UserStatus>>>,
}

impl Receiver {
    pub async fn new(ctx: Arc<Context>, guild_id: GuildId) -> Self {
        // You can manage state here, such as a buffer of audio packet bytes so
        // you can later store them in intervals.
        Self {
            ctx_main: ctx,
            ssrc_hashmap: Arc::new(Mutex::new(HashMap::new())),
            user_id_hashmap: Arc::new(Mutex::new(HashMap::new())),
            ssrc_ffmpeg_hashmap: Arc::new(Mutex::new(HashMap::new())),
            buffer: Arc::new(Mutex::new(HashMap::new())),
            now: Arc::new(Mutex::new(HashMap::new())),
            guild_id,
            user_status: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

struct UserStatus {
    user_id: u64,
    start: std::time::Instant,
    speaking: bool,
    time_passed: f64,
    start2: std::time::Instant,
    time_passed2: f64,
}

impl UserStatus {
    fn new(user_id: u64, start: std::time::Instant, muted: bool) -> Self {
        Self {
            user_id,
            start,
            speaking: muted,
            time_passed: 0.0,
            start2: std::time::Instant::now(),
            time_passed2: 0.0,
        }
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
                println!(
                    "Speaking state update: user {:?} has SSRC {:?}, using {:?}",
                    user_id, ssrc, speaking,
                );

                let a = self
                    .ctx_main
                    .cache
                    .member(self.guild_id, user_id.unwrap().0)
                    .await
                    .unwrap();

                if a.user.bot {
                    println!("is a bot");
                    // Don't record bots
                    {
                        self.ssrc_hashmap.lock().await.insert(*ssrc, None);
                    }
                } else {
                    println!("is NOT bot");
                    // no user in map add
                    {
                        self.ssrc_hashmap
                            .lock()
                            .await
                            .insert(*ssrc, Some(user_id.unwrap().0));
                    }

                    {
                        self.user_id_hashmap
                            .lock()
                            .await
                            .insert(user_id.unwrap().0, *ssrc);
                    }

                    {
                        self.user_status.lock().await.insert(
                            *ssrc,
                            UserStatus::new(user_id.unwrap().0, std::time::Instant::now(), false),
                        );
                    }

                    {
                        if self.ssrc_ffmpeg_hashmap.lock().await.get(ssrc).is_some() {
                            // we have already spawned an ffmpeg process
                        } else {
                            // Create a process
                            let path = create_path(*ssrc, self.guild_id, user_id.unwrap().0).await;

                            let command = Command::new("ffmpeg")
                                .args(["-channel_layout", "stereo"])
                                .args(["-ac", "2"]) // channel count
                                .args(["-ar", "48000"]) // sample rate
                                .args(["-f", "s16le"]) // input type
                                .args(["-i", "-"]) // Input name ("-" is pipe)
                                .args(["-b:a", "64k"]) // bitrate
                                .arg(format!("{}.ogg", path)) // output
                                .stdin(std::process::Stdio::piped())
                                .stderr(std::process::Stdio::null())
                                .stdout(std::process::Stdio::null())
                                .spawn();

                            let child = command.unwrap();
                            self.ssrc_ffmpeg_hashmap.lock().await.insert(*ssrc, child);

                            println!("file created for ssrc: {}", *ssrc);
                        }
                    }
                    {
                        // add a buffer for user
                        self.buffer.lock().await.insert(*ssrc, Vec::new());
                    }
                }
            }
            Ctx::SpeakingUpdate(data) => {
                // {
                //     if !data.speaking {
                //         if let Some(mut file) = self.ssrc_file_hashmap.lock().await.get(&data.ssrc)
                //         {
                //             if let Some(audio) = self.buffer.lock().await.get(&data.ssrc) {

                //                 // file.write_all(&*result).expect("cannot write to file");
                //                 // {
                //                 //     let _x = audio;
                //                 // };

                //                 // self.buffer
                //                 //     .lock()
                //                 //     .await
                //                 //     .get_mut(&data.ssrc)
                //                 //     .unwrap()
                //                 //     .clear();
                //             }
                //         } else {
                //         }
                //     }
                // }
                // You can implement logic here which reacts to a user starting
                // or stopping speaking.

                // TODO: When user is silence add 0's to the file
                // 1) Count how long the user wasn't speaking. Add the equivalent ammount on the next packet update
                // 2) While the user is set as stopped contiously add 0's (much harder?)
                // println!(
                //     "Source {} has {} speaking.",
                //     data.ssrc,
                //     if data.speaking { "started" } else { "stopped" },
                // );
                // not speaking
                if !data.speaking {
                    {
                        // // User stopped speaking. start a timer
                        // let mut a = self.user_status.lock().await;
                        // if let Some(user_status) = a.get_mut(&data.ssrc) {
                        //     user_status.speaking = false;
                        //     // reset
                        //     user_status.start = std::time::Instant::now();
                        //     // reset
                        //     user_status.time_passed = 0.0;
                        //     println!("reset");
                        // } else {
                        //     println!("no user status NOT speaking");
                        // }
                    }
                } else {
                    {
                        // User started speaking again. Time how long user WASN'T speaking
                        let mut a = self.user_status.lock().await;
                        if let Some(user_status) = a.get_mut(&data.ssrc) {
                            user_status.speaking = true;
                            // Time elapsed since we started timeing it above
                            user_status.time_passed = user_status.start.elapsed().as_secs_f64();

                            println!("time passed not speaking: {}", user_status.time_passed);
                        } else {
                            println!("no user status speaking");
                        }
                    }
                }
            }
            Ctx::VoicePacket(data) => {
                let now = std::time::Instant::now();

                if let Some(child) = self
                    .ssrc_ffmpeg_hashmap
                    .lock()
                    .await
                    .get_mut(&data.packet.ssrc)
                {
                    if let Some(audio) = data.audio {
                        let slice_i16 = audio;
                        let mut result: Vec<u8> = Vec::new();
                        for &n in slice_i16 {
                            // TODO: Use buffer
                            let _ = result.write_i16_le(n).await;
                        }

                        if let Some(stdin) = child.stdin.as_mut() {
                            if let Some(status) =
                                self.user_status.lock().await.get_mut(&data.packet.ssrc)
                            {
                                // floating point errors
                                if status.time_passed > 0.05 {
                                    println!("from mute");
                                    let to_write = (status.time_passed * DISCORD_SAMPLE_RATE as f64)
                                        .round()
                                        as u64;
                                    let mut vec = vec![0u8; to_write as usize];
                                    for _ in 0..to_write {
                                        vec.push(0u8);
                                    }
                                    match stdin.write_all(&*vec).await {
                                        Ok(_) => {}
                                        Err(err) => {
                                            println!("Could not write to stdin: {}", err)
                                        }
                                    };

                                    status.speaking = false;
                                    status.start = std::time::Instant::now();
                                    status.time_passed = 0.0;
                                }
                            }
                            match stdin.write_all(&*result).await {
                                Ok(_) => {}
                                Err(err) => {
                                    println!("Could not write to stdin: {}", err)
                                }
                            };
                        } else {
                            println!("no stdin");
                        }
                    } else {
                        println!("No audio");
                    }
                } else {
                    println!("No handle for ffmpeg. Maybe a bot?")
                }

                println!("Messsage time elapsed micro:{}", now.elapsed().as_micros());
            }
            Ctx::RtcpPacket(data) => {
                // An event which fires for every received rtcp packet,
                // containing the call statistics and reporting information.
                // println!("RTCP packet received: {:?}", data.packet);
            }
            Ctx::ClientConnect(ClientConnect {
                audio_ssrc,
                video_ssrc,
                user_id,
                ..
            }) => {
                println!("Someone connected {}", user_id);
                let a = self
                    .ctx_main
                    .cache
                    .member(self.guild_id, user_id.0)
                    .await
                    .unwrap();
                if a.user.bot {
                    println!("bot connected");
                } else {
                    {
                        self.ssrc_hashmap
                            .lock()
                            .await
                            .insert(*audio_ssrc, Some(user_id.0));
                    }

                    {
                        self.user_id_hashmap
                            .lock()
                            .await
                            .insert(user_id.0, *audio_ssrc);
                    }
                    println!("user inserted");
                    {
                        self.user_status.lock().await.insert(
                            *audio_ssrc,
                            UserStatus::new(user_id.0, std::time::Instant::now(), false),
                        );
                    }
                    {
                        if self
                            .ssrc_ffmpeg_hashmap
                            .lock()
                            .await
                            .get(audio_ssrc)
                            .is_some()
                        {
                            // we have already spawned an ffmpeg process
                        } else {
                            // Create a process
                            let path = create_path(*audio_ssrc, self.guild_id, user_id.0).await;

                            let command = Command::new("ffmpeg")
                                .args(["-channel_layout", "stereo"])
                                .args(["-ac", "2"]) // channel count
                                .args(["-ar", "48000"]) // sample rate
                                .args(["-f", "s16le"]) // input type
                                .args(["-i", "-"]) // Input name ("-" is pipe)
                                .args(["-bufsize", "64k"])
                                .args(["-b:a", "64k"]) // bitrate
                                .arg(format!("{}.ogg", path)) // output
                                .stdin(std::process::Stdio::piped())
                                // .stderr(std::process::Stdio::piped())
                                .stdout(std::process::Stdio::piped())
                                .spawn();

                            let child = command.unwrap();
                            self.ssrc_ffmpeg_hashmap
                                .lock()
                                .await
                                .insert(*audio_ssrc, child);

                            println!("file created for ssrc: {}", *audio_ssrc);
                        }
                    }
                }

                // You can implement your own logic here to handle a user who has joined the
                // voice channel e.g., allocate structures, map their SSRC to User ID.

                println!(
                    "Client connected: user {:?} has audio SSRC {:?}, video SSRC {:?}",
                    user_id, audio_ssrc, video_ssrc,
                );
            }
            Ctx::ClientDisconnect(ClientDisconnect { user_id, .. }) => {
                // You can implement your own logic here to handle a user who has left the
                // voice channel e.g., finalise processing of statistics etc.
                // You will typically need to map the User ID to their SSRC; observed when
                // speaking or connecting.
                {
                    let status = self
                        .user_status
                        .lock()
                        .await
                        .remove(self.user_id_hashmap.lock().await.get(&user_id.0).unwrap())
                        .unwrap();
                    let time = status.start2.elapsed().as_secs_f64();
                    println!("time in channel: {}", time);
                }

                {
                    let ssrc = match self.user_id_hashmap.lock().await.remove(&user_id.0) {
                        Some(ok) => ok,
                        None => {
                            println!("tried to remove bot");
                            return None;
                        }
                    };
                    let a = self.ssrc_ffmpeg_hashmap.lock().await.remove(&ssrc).unwrap();

                    let output = a.wait_with_output().await.unwrap();
                    // TODO: Remove
                    println!("stdout {}", String::from_utf8(output.stdout).unwrap());
                    // println!("stderr {}", String::from_utf8(output.stderr).unwrap());
                    //
                    println!("Client disconnected: user {:?}", user_id);
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

// TODO: username instead of id
async fn create_path(ssrc: u32, guild_id: GuildId, user_id: u64) -> String {
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

    let dir_path = format!("{}/{}/{}/{}", &RECORDING_FILE_PATH, guild_id.0, year, month);
    let combined_path = format!(
        "{}/{}/{}/{}/{}-{}-{}-{}-{}",
        &RECORDING_FILE_PATH, guild_id.0, year, month, day_name, day_number, hour, minute, user_id,
    );

    combined_path
}

pub async fn voice_server_update(
    _ctx: Context,
    _update: serenity::model::event::VoiceServerUpdateEvent,
) {
}

pub async fn voice_state_update(
    ctx: Context,
    guild_id: Option<serenity::model::id::GuildId>,
    old_state: Option<serenity::model::prelude::VoiceState>,
    new_state: serenity::model::prelude::VoiceState,
) {
    if new_state.channel_id.is_none() {
        // Someone left the channel
        // TODO: Logic to stop recording if < x people
        println!("is_none");
        return;
    }

    if new_state.member.unwrap().user.bot {
        // Ignore bots
        println!("bot");
        return;
    }

    let channel = new_state
        .channel_id
        .unwrap()
        .to_channel_cached(&ctx)
        .await
        .unwrap();

    // The bot will join a voice channel with the following priorities
    // 1) There must be at least 3 people in a channel
    // 2) If 2 or more channels have the same count, will join the channel that has the highest average role (TODO)
    // 3) If above is equal will join a channel with the oldest member

    let all_channels = ctx
        .cache
        .guild(guild_id.unwrap())
        .await
        .expect("cannot clone guild from cache")
        .channels;

    let mut highest_channel_id: ChannelId = ChannelId(0);
    let mut highest_channel_len: usize = 0;

    for (channel_id, guild_channel) in all_channels {
        if guild_channel.kind == serenity::model::channel::ChannelType::Voice {
            if highest_channel_len > guild_channel.members(&ctx).await.expect("cannot get").len() {
                // do nothing
            } else {
                highest_channel_len = guild_channel.members(&ctx).await.expect("cannot get").len();
                highest_channel_id = guild_channel.id;
            }
        }
    }

    if highest_channel_len > 0 {
        connect_to_voice_channel(&ctx, guild_id.unwrap(), highest_channel_id).await;
    }

    // if new_state.member.as_ref().unwrap().user.bot {
    //     // ignore bots
    //     return;
    // }
    // if new_state.channel_id.is_some() {
    //     if let Some(state) = old_state {
    //         // User switches channels or anything else
    //         if state.channel_id.unwrap() != new_state.channel_id.unwrap() {
    //             // User switches channels
    //             play_boss_music(&ctx, &new_state, guild_id).await;
    //         }
    //     } else {
    //         // User Joins a voice channel FOR THE FIRST TIME
    //         play_boss_music(&ctx, &new_state, guild_id).await;
    //     }
    // } else {
    //     // user leaves a voice channel
    // }
}

pub async fn connect_to_voice_channel(ctx: &Context, guild_id: GuildId, channel_id: ChannelId) {
    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    {
        if let Some(result) = manager.get(guild_id) {
            if let Some(channel) = result.lock().await.current_channel() {
                println!(
                    "manager channel {}",
                    result.lock().await.current_channel().unwrap()
                );
            } else {
                println!("no channel joined");
            }
        } else {
            println!("manager not present??");
        }

        println!("our channel {}", channel_id);
    }

    // Don't connect to a channel we are already in
    // TODO: will have to re-register all the event handler + songbird might already be doing this for us
    // {
    //     // Bot is already in the channel we are trying to connect.
    //     if let Some(result) = manager.get(guild_id) {
    //         let channel = { result.lock().await.current_channel().unwrap() };
    //         if channel == channel_id.into() {
    //             println!("bot trying to connect to the same channel");
    //             return;
    //         }
    //     }
    // }

    let (handler_lock, conn_result) = manager.join(guild_id, channel_id).await;

    match conn_result {
        Ok(()) => {
            println!("Joined {}", channel_id);

            // NOTE: this skips listening for the actual connection result.
            let mut handler = handler_lock.lock().await;
            let ctx1 = Arc::new(ctx.clone());
            let receiver = Receiver::new(ctx1, guild_id).await;

            handler.add_global_event(CoreEvent::SpeakingStateUpdate.into(), receiver.clone());

            handler.add_global_event(CoreEvent::SpeakingUpdate.into(), receiver.clone());

            handler.add_global_event(CoreEvent::VoicePacket.into(), receiver.clone());

            handler.add_global_event(CoreEvent::RtcpPacket.into(), receiver.clone());

            handler.add_global_event(CoreEvent::ClientConnect.into(), receiver.clone());

            handler.add_global_event(CoreEvent::ClientDisconnect.into(), receiver.clone());

            drop(receiver);
        }
        Err(err) => {
            panic!("Could not join channel: {}", err)
        }
    }
}
