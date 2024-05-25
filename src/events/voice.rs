use crate::{
    event_handler::Handler, events::voice_receiver::Receiver, get_lock_read, MpmcChannels,
};
use serenity::{
    client::Context,
    model::{
        id::{ChannelId, GuildId},
        prelude::Member,
    },
};
use songbird::CoreEvent;
use sqlx::{Pool, Postgres};
use std::sync::Arc;
use tracing::{error, info, trace};

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
        let guild_id = match new_state.guild_id {
            Some(ok) => ok,
            None => {
                // This should never happen
                error!("No guild id in voice_state_update");
                panic!()
            }
        };
        if member.user.bot {
            // Ignore bots
            info!("bot");
            return;
        }

        let leave = handle_no_people_in_channel(&new_state, &ctx, &old_state, member).await;
        if leave {
            leave_voice_channel(&ctx, guild_id).await;
            return;
        }

        if let Some(old) = old_state {
            if new_state.channel_id.is_some() {
                // We can check for various things that happened after the user has connected
                // We don't care about any events at the moment
                if new_state.channel_id.unwrap() == old.channel_id.unwrap() {
                    // An action happened that was NOT switching channels.
                    // We don't care about those
                    info!("An action happened that was NOT switching channels");
                    return;
                } else if new_state.channel_id.unwrap() != old.channel_id.unwrap() {
                    // user switched channels
                    info!("user switched channels");
                }
            }
        }

        let (highest_channel_id, highest_channel_len) =
            match get_channel_with_most_members(&ctx, &new_state).await {
                Some(value) => value,
                None => {
                    leave_voice_channel(&ctx, guild_id).await;
                    return;
                }
            };

        if highest_channel_len > 0 {
            let user_id = new_state.user_id;
            connect_to_voice_channel(
                _self.database.clone(),
                &ctx,
                guild_id,
                highest_channel_id,
                user_id.get(),
            )
            .await;
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
) -> bool {
    if new_state.channel_id.is_none() {
        // Someone left the channel

        // Check if the BOT has left the channel
        if member.user.id == ctx.cache.current_user().id {
            // From testing this will never trigger because the bot is disconnected before an event is received
            // Check if the our application was kicked
            info!("bot was kicked/left");
            let guild_id = new_state.guild_id.unwrap();
            leave_voice_channel(ctx, guild_id).await;
            return true;
        }

        // Check if that person was the last HUMAN user to leave the channel
        // TODO: There can be other people in different channels. Check for this

        // get the channel id that user was in before disconnecting
        if let Some(channel_id) = old_state.as_ref().unwrap().channel_id {
            let current_channel = channel_id.to_channel(&ctx).await.unwrap();
            let guild_channel = current_channel.guild().unwrap();
            let vec = guild_channel.members(&ctx).unwrap();
            // Get all users in the channel
            let members: Vec<Member> = vec
                .into_iter()
                // Don't include bots
                .filter(|f| !f.user.bot)
                .collect();

            // No Human users left, just the bot is left
            if members.len() == 0 {
                info!("No more human users left. Leaving channel");
                return true;
            } else {
                trace!("Human users still in channel.");
                return false;
            }
        }
    }
    return false;
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

        let mut hash_map = lock.write().await;
        let (handler, receiver) = hash_map
            .remove(&guild_id.get())
            .expect("channel not innitialized");

        handle_graceful_shutdown(handler.clone(), receiver.clone(), 1).await;
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
                // info!("channels already exists");

                // let handler = ok.0.clone();
                // let receiver = ok.1.clone();
                // handle_graceful_shutdown(handler, receiver, 2).await;

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

                handler.add_global_event(CoreEvent::DriverConnect.into(), receiver.clone());
                handler.add_global_event(CoreEvent::DriverReconnect.into(), receiver.clone());
                handler.add_global_event(CoreEvent::DriverDisconnect.into(), receiver.clone());
            }
        }
        Err(err) => {
            manager.remove(guild_id).await.unwrap();
            panic!("cannot join channel 2: {}", err);
        }
    }
}

// Events sent to the voice_state_update do not reach the bot voice receiver.
// Since the logic for leaving the channel is in the voice_state_update we do not know the state of the receiver
// As such we send effectivly a SIGTERM command.
// The bot will disconnect itself when it has finished processing.
async fn handle_graceful_shutdown(
    handler: tokio::sync::broadcast::Sender<i32>,
    receiver: tokio::sync::broadcast::Sender<i32>,
    code: i32,
) {
    info!("handle graceful shutdown");
    // Subscribe for a response
    let mut r1 = handler.subscribe();

    // Termination signal
    match receiver.send(code) {
        Ok(ok) => {
            info!("Sent to {ok} receivers")
        }
        Err(_) => {
            info!("No receivers to receive signal");
            return;
        }
    }

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
