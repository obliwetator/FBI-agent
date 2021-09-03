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
            now: Arc::new(Mutex::new(HashMap::new())),
            guild_id,
            user_status: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

struct UserStatus {
    // user_id: u64,
// start: std::time::Instant,
// speaking: bool,
// time_passed: u128,
// start2: std::time::Instant,
}

impl UserStatus {
    fn new(user_id: u64, start: std::time::Instant, muted: bool) -> Self {
        Self {
            // user_id,
            // start,
            // speaking: muted,
            // time_passed: 0,
            // start2: std::time::Instant::now(),
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

                let member = self
                    .ctx_main
                    .cache
                    .member(self.guild_id, user_id.unwrap().0)
                    .await
                    .unwrap();

                if member.user.bot {
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
                            let path =
                                create_path(*ssrc, self.guild_id, user_id.unwrap().0, member).await;

                            let child = spawn_ffmpeg(&path);
                            self.ssrc_ffmpeg_hashmap.lock().await.insert(*ssrc, child);

                            println!("file created for ssrc: {}", *ssrc);
                        }
                    }
                }
            }
            Ctx::SpeakingUpdate(data) => {
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
                }
                // else {
                //     {
                //         // User started speaking again. Time how long user WASN'T speaking
                //         // let mut user_status = self.user_status.lock().await;
                //         // if let Some(user_status) = user_status.get_mut(&data.ssrc) {
                //         //     user_status.speaking = true;
                //         //     // Time elapsed since we started timeing it above
                //         //     user_status.time_passed = user_status.start.elapsed().as_millis();

                //         //     println!(
                //         //         "time passed not speaking in millis: {}",
                //         //         user_status.time_passed
                //         //     );
                //         // } else {
                //         //     // println!("no user status speaking");
                //         // }
                //     }
                // }
            }
            Ctx::VoicePacket(data) => {
                let now = std::time::Instant::now();

                if let Some(child) = self
                    .ssrc_ffmpeg_hashmap
                    .lock()
                    .await
                    .get_mut(&data.packet.ssrc)
                {
                    if let Some(audio_i16) = data.audio {
                        // let mut consecutive = 0u32;
                        // println!(
                        //     "Audio packet's first 5 samples: {:?}",
                        //     // audio.get(..5.min(audio.len()))
                        //     audio
                        // );

                        // println!(
                        // 	"Audio packet sequence {:05} has {:04} bytes (decompressed from {}), SSRC {}",
                        // 	data.packet.sequence.0,
                        // 	audio.len() * std::mem::size_of::<i16>(),
                        // 	data.packet.payload.len(),
                        // 	data.packet.ssrc,
                        // );
                        if let Some(stdin) = child.stdin.as_mut() {
                            // if let Some(status) =
                            //     self.user_status.lock().await.get_mut(&data.packet.ssrc)
                            // {
                            //     if status.time_passed > 0 {
                            //         println!("from mute");
                            //         // the size is double because ffmpeg expects i16
                            //         let to_write = {
                            //             let mut value = status.time_passed
                            //                 * (DISCORD_SAMPLE_RATE / 1000) as u128
                            //                 * 2; // Silence to write from the time not speaking

                            //             // we need even amount of bytes
                            //             if (value % 2) == 1 {
                            //                 value += 1;
                            //             }

                            //             value
                            //         };

                            //         println!("to_write: {}", to_write);
                            //         // From rough observation discord sends an addiitonal ~9.5 packets of silence
                            //         // Since we write u8 we need twice as many

                            //         let vec: Vec<u8> = vec![0u8; to_write as usize];
                            //         match stdin.write_all(&*vec).await {
                            //             Ok(_) => {}
                            //             Err(err) => {
                            //                 println!("Could not write to stdin: {}", err)
                            //             }
                            //         };
                            //         println!("HERE: {}", now.elapsed().as_micros());
                            //         status.speaking = false;
                            //         status.start = std::time::Instant::now();
                            //         status.time_passed = 0;
                            //         // When the user stops speaking we receive at least 6(?) packets of silence. Ignroe them
                            //         return None;
                            //     }
                            // }
                            let mut result: Vec<u8> = Vec::new();
                            for &n in audio_i16 {
                                // TODO: Use buffer
                                let _ = result.write_i16_le(n).await;

                                // if n == 0 {
                                //     consecutive += 1;
                                // }
                            }

                            // If more than half of the samples are silence don't write that packet.
                            // if consecutive > 1000 {
                            //     return None;
                            // }

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
                }

                // println!("Messsage time elapsed micro:{}", now.elapsed().as_micros());
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
                let member = self
                    .ctx_main
                    .cache
                    .member(self.guild_id, user_id.0)
                    .await
                    .unwrap();
                if member.user.bot {
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
                            let path =
                                create_path(*audio_ssrc, self.guild_id, user_id.0, member).await;

                            let child = spawn_ffmpeg(&path);
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
                    // Bots
                    let a = self.user_id_hashmap.lock().await;
                    if let Some(ab) = a.get(&user_id.0) {
                        let b = self.ssrc_hashmap.lock().await;
                        if let Some(ba) = b.get(ab) {
                            if ba.is_none() {
                                // bot ignore
                                return None;
                            }
                        } else {
                            return None;
                        }
                    } else {
                        return None;
                    }
                }

                {
                    let _ = self
                        .user_status
                        .lock()
                        .await
                        .remove(self.user_id_hashmap.lock().await.get(&user_id.0).unwrap())
                        .unwrap();
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
                    println!(
                        "stdout from wait_with_output {}",
                        String::from_utf8(output.stdout).unwrap()
                    );
                    println!(
                        "stderr from wait_with_output {}",
                        String::from_utf8(output.stderr).unwrap()
                    );
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

fn spawn_ffmpeg(path: &str) -> Child {
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
        .stderr(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .spawn();

    command.unwrap()
}

// TODO: username instead of id
async fn create_path(
    ssrc: u32,
    guild_id: GuildId,
    user_id: u64,
    member: serenity::model::guild::Member,
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
        "{}/{}/{}/{}/",
        &RECORDING_FILE_PATH, guild_id.0, year, month
    );
    let combined_path = format!(
        "{}/{}/{}/{}/{}-{}-{}-{}-{}-{}-{}",
        &RECORDING_FILE_PATH,
        guild_id.0,
        year,
        month,
        day_name,
        day_number,
        hour,
        minute,
        seconds,
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

        if let Some(channel) = old_state.unwrap().channel_id {
            if channel
                .to_channel_cached(&ctx)
                .await
                .unwrap()
                .guild()
                .unwrap()
                .members(&ctx)
                .await
                .unwrap()
                .len()
                == 1
            // just the bot is left
            // TODO: fix that atrocity ^
            // TODO: Other bots might be present in the channel. Don't count bots
            {
                leave_voice_channel(&ctx, guild_id.unwrap()).await;
            }
        }

        println!("is_none");
        return;
    }

    if new_state.member.unwrap().user.bot {
        // Ignore bots
        println!("bot");
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
        .guild(guild_id.unwrap())
        .await
        .expect("cannot clone guild from cache")
        .channels;

    let mut highest_channel_id: ChannelId = ChannelId(0);
    let mut highest_channel_len: usize = 0;

    for (channel_id, guild_channel) in all_channels {
        if guild_channel.kind == serenity::model::channel::ChannelType::Voice
            && guild_channel.members(&ctx).await.expect("cannot get").len() >= highest_channel_len
        {
            highest_channel_len = guild_channel.members(&ctx).await.expect("cannot get").len();
            highest_channel_id = guild_channel.id;
        }
    }

    println!("highest channel id: {}", highest_channel_id);
    println!("highest channel len: {}", highest_channel_len);

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

async fn leave_voice_channel(ctx: &Context, guild_id: GuildId) {
    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    let _ = manager.remove(guild_id).await;

    println!("Left the voice channel");
}

pub async fn connect_to_voice_channel(ctx: &Context, guild_id: GuildId, channel_id: ChannelId) {
    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

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
            // Remove any old events in case the bot swaps channels
            handler.remove_all_global_events();

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
