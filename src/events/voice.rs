use std::collections::HashMap;
use std::io::Write;
use std::{fs::File, io::Cursor, path::Path, sync::Arc};

use serenity::{
    async_trait,
    client::Context,
    model::id::{ChannelId, GuildId},
    prelude::Mutex,
};
use songbird::{
    input,
    model::payload::{ClientConnect, ClientDisconnect, Speaking},
    Call, CoreEvent, Event, EventContext, EventHandler as VoiceEventHandler, TrackEvent,
};
use tokio::io::AsyncWriteExt;
use tokio::time::sleep;

use crate::{database, HasBossMusic};

pub const RECORDING_FILE_PATH: &str = "/home/ubuntu/FBIagent/voice_recordings";
const DISCORD_SAMPLE_RATE: u16 = 48000;

#[derive(Clone)]
struct Receiver {
    ctx_main: Arc<Context>,
    /// the key is the ssrc, the value is the user id
    /// If the value is none that means we don't want to record that user (for now bots)
    ssrc_hashmap: Arc<Mutex<HashMap<u32, Option<u64>>>>,
    ssrc_file_hashmap: Arc<Mutex<HashMap<u32, File>>>,
    buffer: Arc<Mutex<Vec<u8>>>,
    now: Arc<Mutex<HashMap<u32, std::time::Instant>>>,
    guild_id: GuildId,
}

impl Receiver {
    pub async fn new(ctx: Arc<Context>, guild_id: GuildId) -> Self {
        // You can manage state here, such as a buffer of audio packet bytes so
        // you can later store them in intervals.
        Self {
            ctx_main: ctx,
            guild_id,
            ssrc_hashmap: Arc::new(Mutex::new(HashMap::new())),
            buffer: Arc::new(Mutex::new(vec![0; 1024 * 1024])),
            now: Arc::new(Mutex::new(HashMap::new())),
            ssrc_file_hashmap: Arc::new(Mutex::new(HashMap::new())),
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
                self.ssrc_hashmap.lock().await.insert(1, Some(1));
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
                    if self.ssrc_hashmap.lock().await.get(&*ssrc).is_none() {
                        self.ssrc_hashmap.lock().await.insert(*ssrc, None);
                    }
                } else {
                    println!("is NOT bot");
                    // no user in map add
                    if self.ssrc_hashmap.lock().await.get(&*ssrc).is_none() {
                        self.ssrc_hashmap
                            .lock()
                            .await
                            .insert(*ssrc, Some(user_id.unwrap().0));
                    }

                    if self.ssrc_file_hashmap.lock().await.get(ssrc).is_some() {
                        // // If we have a file handle write to file
                        // let file = self.ssrc_file_hashmap.lock().await.get(ssrc).unwrap();
                    } else {
                        // Create a file and write
                        let file = create_file(*ssrc, self.guild_id, user_id.unwrap().0).await;
                        println!("file created for ssrc: {}", *ssrc);

                        self.ssrc_file_hashmap.lock().await.insert(*ssrc, file);
                    }
                }
            }
            Ctx::SpeakingUpdate(data) => {
                // You can implement logic here which reacts to a user starting
                // or stopping speaking.

                // TODO: When user is silence add 0's to the file
                // 1) Count how long the user wasn't speaking. Add the equivalent ammount on the next packet update
                // 2) While the user is set as stopped contiously add 0's (much harder?)
                self.ssrc_hashmap.lock().await.insert(2, Some(2));
                println!(
                    "Source {} has {} speaking.",
                    data.ssrc,
                    if data.speaking { "started" } else { "stopped" },
                );
            }
            Ctx::VoicePacket(data) => {
                self.ssrc_hashmap.lock().await.insert(3, Some(3));
                // An event which fires for every received audio packet,
                // containing the decoded data.
                if let Some(audio) = data.audio {
                    // println!(
                    //     "Audio packet's first 5 samples: {:?}",
                    //     audio.get(..5.min(audio.len()))
                    // );
                    // println!(
                    //     "Audio packet sequence {:05} has {:04} bytes (decompressed from {}), SSRC {}",
                    //     data.packet.sequence.0,
                    //     audio.len() * std::mem::size_of::<i16>(),
                    //     data.packet.payload.len(),
                    //     data.packet.ssrc,
                    // );

                    if let Some(mut file) =
                        self.ssrc_file_hashmap.lock().await.get(&data.packet.ssrc)
                    {
                        let slice_i16 = audio.get(..audio.len()).unwrap();
                        let mut result: Vec<u8> = Vec::new();
                        for &n in slice_i16 {
                            // TODO: Use buffer
                            let _ = result.write_i16_le(n).await;
                        }

                        file.write_all(&*result).expect("cannot write to file");
                    } else {
                    }
                } else {
                    println!("RTP packet, but no audio. Driver may not be configured to decode.");
                }
            }
            Ctx::RtcpPacket(data) => {
                self.ssrc_hashmap.lock().await.insert(4, Some(4));
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

                println!("Client disconnected: user {:?}", user_id);
            }
            _ => {
                // We won't be registering this struct for any more event classes.
                unimplemented!()
            }
        }

        None
    }
}

async fn create_file(ssrc: u32, guild_id: GuildId, user_id: u64) -> File {
    let start = std::time::SystemTime::now();
    let since_the_epoch = start
        .duration_since(std::time::UNIX_EPOCH)
        .expect("Time went backwards");

    let now = chrono::Utc::now();
    let year = format!("{}", now.format("%Y"));
    let month = format!("{}", now.format("%B"));
    let day = format!("{}", now.format("%A"));
    let hour = format!("{}", now.format("%H"));
    let minute = format!("{}", now.format("%M"));

    let dir_path = format!("{}/{}/{}/{}", &RECORDING_FILE_PATH, guild_id.0, year, month);
    let combined_path = format!(
        "{}/{}/{}/{}/{}-{}:{}-{}",
        &RECORDING_FILE_PATH, guild_id.0, year, month, day, hour, minute, user_id,
    );
    let path = Path::new(combined_path.as_str());
    let display = path.display();
    std::fs::create_dir_all(&dir_path).expect("cannot create recursive path");
    let file = match File::create(&path) {
        Err(why) => panic!("couldn't create {}: {}", display, why),
        Ok(file) => file,
    };

    file
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

    // let members = new_state
    //     .channel_id
    //     .unwrap()
    //     .to_channel_cached(&ctx)
    //     .await
    //     .expect("cannot get channel from cachce")
    //     .guild()
    //     .unwrap()
    //     .members(&ctx)
    //     .await
    //     .expect("cannot get voice channel members");

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
        connect_to_voice_channel(&ctx, guild_id.unwrap(), ChannelId(362257054829641762)).await;
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
        }
        Err(err) => {
            panic!("Could not join channel: {}", err)
        }
    }
}

pub async fn play_boss_music(
    ctx: &Context,
    new_state: &serenity::model::prelude::VoiceState,
    guild_id: Option<serenity::model::id::GuildId>,
) {
    let result: bool;

    {
        let data = ctx.data.read().await;
        let has_boss_music = data.get::<HasBossMusic>().unwrap();
        result = has_boss_music.contains_key(new_state.user_id.as_u64());
    }

    // Check the hashmap if that user has a value set
    if result {
        {
            let data = ctx.data.read().await;
            let has_boss_music = data.get::<HasBossMusic>().unwrap();
            let result = has_boss_music.get(new_state.user_id.as_u64()).unwrap();
            // we have a match
            if let Some(song_name) = result {
                // User has boss music
                establish_connection(ctx, song_name, new_state, guild_id).await;
            } else {
                // This user does not have a boss music return
                // TODO: check for msg reference

                // if (msgRef) {
                // 	msgRef.reply("This user does not have boss music")
                // }
                todo!();
                // return
            }
        }
    } else {
        {
            // no match go to db
            let result = database::voice::get_user_boss_music(ctx, new_state.user_id.0).await;
            let mut data = ctx.data.write().await;
            let has_boss_music = data.get_mut::<HasBossMusic>().unwrap();
            if let Some(song_name) = result {
                let song = song_name.clone();
                has_boss_music.insert(new_state.user_id.0, Some(song_name));
                drop(data);
                establish_connection(ctx, &song, new_state, guild_id).await;
            // establish_connection(ctx, song_name, new_state, guild_id).await;
            } else {
                has_boss_music.insert(new_state.user_id.0, None);
            }
            let a = match songbird::ytdl("").await {
                Ok(result) => result,
                Err(_) => {
                    panic!("cannot dl youtube link")
                }
            };
            let cursor: Cursor<Vec<u8>> = Cursor::new(Vec::new());
            let b = a.reader;
        }

        // if let Some(song_name) = result {
        // helper.has_bos_music.insert("target", song_name)
        // let mut handler = establish(ctx, "song_name", new_state, guild_id).await.expect("cannot get handler");
        // handler.lock().await;

        // } else {
        // return
        //}
    }
}

struct SongEndNotifier {
    arc_hanlder: Arc<Mutex<Call>>,
}

#[async_trait]
impl VoiceEventHandler for SongEndNotifier {
    async fn act(&self, _ctx: &EventContext<'_>) -> Option<Event> {
        match self.arc_hanlder.lock().await.leave().await {
            Ok(_) => {}
            Err(_) => {
                println!("cannot leave channel")
            }
        };
        None
    }
}

pub async fn establish(
    ctx: &Context,
    song_name: &str,
    new_state: &serenity::model::prelude::VoiceState,
    guild_id: Option<serenity::model::id::GuildId>,
) -> Option<Arc<serenity::prelude::Mutex<Call>>> {
    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.");

    let channel_id = new_state.channel_id;

    let connect_to = match channel_id {
        Some(channel) => channel,
        None => {
            println!("No channel id present");

            return None;
        }
    };
    let guildid = guild_id.unwrap();
    let arc_hanlder = manager.get_or_insert(guildid.into());
    let handler = arc_hanlder;

    Some(handler)
}

pub async fn establish_connection(
    ctx: &Context,
    song_name: &str,
    new_state: &serenity::model::prelude::VoiceState,
    guild_id: Option<serenity::model::id::GuildId>,
) {
    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.");

    let channel_id = new_state.channel_id;

    let connect_to = match channel_id {
        Some(channel) => channel,
        None => {
            println!("No channel id present");

            return;
        }
    };
    let guildid = guild_id.unwrap();
    let arc_hanlder = manager.get_or_insert(guildid.into());
    let mut handler = arc_hanlder.lock().await;
    handler.deafen(true).await.expect("cannot mute");
    handler.stop();

    match handler.join(connect_to.into()).await {
        Ok(ok) => ok,
        Err(_) => {
            panic!("cannot join voice channel")
        }
    };

    sleep(std::time::Duration::from_millis(200)).await;

    println!(
        "trying to play : {}",
        format!("/home/ubuntu/DiscordBotJS/audioClips/{}", song_name)
    );
    let source = match input::ffmpeg(format!(
        "/home/ubuntu/DiscordBotJS/audioClips/{}",
        song_name
    ))
    .await
    {
        Ok(source) => source,
        Err(why) => {
            println!("Err starting source: {:?}", why);
            return;
        }
    };

    let send_http = ctx.http.clone();
    // This handler object will allow you to, as needed,
    // control the audio track via events and further commands.
    let song = handler.play_source(source);
    let _ = song.add_event(
        Event::Track(TrackEvent::End),
        SongEndNotifier {
            arc_hanlder: arc_hanlder.clone(),
        },
    );

    // } else {
    //     let channel_id = new_state.channel_id;

    //     let handler_lock = manager.join(guild_id.unwrap(), connect_to).await;
    //     let mut handler = handler_lock.0.lock().await;

    //     let source =
    //         match input::ffmpeg("/home/ubuntu/DiscordBotJS/audioClips/46244119307453600.ogg").await
    //         {
    //             Ok(source) => source,
    //             Err(why) => {
    //                 println!("Err starting source: {:?}", why);

    //                 return;
    //             }
    //         };

    //     // This handler object will allow you to, as needed,
    //     // control the audio track via events and further commands.
    //     let song = handler.play_source(source);
    //     let send_http = ctx.http.clone();
    // }
}
