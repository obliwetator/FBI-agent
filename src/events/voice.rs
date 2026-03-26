use crate::{event_handler::Handler, events::voice_receiver::Receiver, get_lock_read};
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
use tracing::{error, info};

pub async fn voice_server_update(
    _self: &Handler,
    _ctx: Context,
    _update: serenity::model::event::VoiceServerUpdateEvent,
) {
}

fn log_voice_state_changes(
    old: &serenity::model::prelude::VoiceState,
    new_state: &serenity::model::prelude::VoiceState,
) {
    if old.deaf != new_state.deaf {
        info!(
            "User server deafened changed: {} -> {}",
            old.deaf, new_state.deaf
        );
    }
    if old.mute != new_state.mute {
        info!(
            "User server muted changed: {} -> {}",
            old.mute, new_state.mute
        );
    }
    if old.self_deaf != new_state.self_deaf {
        info!(
            "User self deafened changed: {} -> {}",
            old.self_deaf, new_state.self_deaf
        );
    }
    if old.self_mute != new_state.self_mute {
        info!(
            "User self muted changed: {} -> {}",
            old.self_mute, new_state.self_mute
        );
    }
    if old.self_stream != new_state.self_stream {
        info!(
            "User stream status changed: {:?} -> {:?}",
            old.self_stream, new_state.self_stream
        );
    }
    if old.self_video != new_state.self_video {
        info!(
            "User video status changed: {} -> {}",
            old.self_video, new_state.self_video
        );
    }
    if old.suppress != new_state.suppress {
        info!(
            "User suppress status changed: {} -> {}",
            old.suppress, new_state.suppress
        );
    }
    if old.request_to_speak_timestamp != new_state.request_to_speak_timestamp {
        info!(
            "User request to speak changed: {:?} -> {:?}",
            old.request_to_speak_timestamp, new_state.request_to_speak_timestamp
        );
    }
}

pub async fn voice_state_update(
    _self: &Handler,
    ctx: Context,
    old_state: Option<serenity::model::prelude::VoiceState>,
    new_state: serenity::model::prelude::VoiceState,
) {
    // Notify the dashboard stream of any user voice state changes
    {
        let data_read = ctx.data.read().await;
        if let Some(metrics) = data_read.get::<crate::BotMetricsKey>() {
            let _ = metrics.voice_update_tx.send(());

            // Track user start times
            let user_id = new_state.user_id.get();
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64;

            if let Some(new_ch) = new_state.channel_id {
                if let Some(old) = &old_state {
                    if old.channel_id != Some(new_ch) {
                        // User switched channels
                        metrics.user_start_times.insert(user_id, now);
                    }
                } else {
                    // User joined a channel
                    metrics.user_start_times.insert(user_id, now);
                }
            } else {
                // User left the channel completely
                metrics.user_start_times.remove(&user_id);
            }
        }
    }

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
            if let Some(new_channel_id) = new_state.channel_id {
                if let Some(old_channel_id) = old.channel_id {
                    // We can check for various things that happened after the user has connected
                    // We don't care about any events at the moment
                    if new_channel_id == old_channel_id {
                        // An action happened that was NOT switching channels.
                        log_voice_state_changes(&old, &new_state);
                        return;
                    } else {
                        // user switched channels
                        info!("user switched channels");
                    }
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
        if let Some(channel_id) = old_state.as_ref().and_then(|s| s.channel_id) {
            let current_channel = match channel_id.to_channel(&ctx).await {
                Ok(ch) => ch,
                Err(err) => {
                    error!("Could not resolve current channel: {}", err);
                    return false;
                }
            };

            let guild_channel = match current_channel.guild() {
                Some(gc) => gc,
                None => {
                    error!("Not a guild channel");
                    return false;
                }
            };

            let vec = match guild_channel.members(&ctx) {
                Ok(m) => m,
                Err(err) => {
                    error!("Could not get channel members: {}", err);
                    return false;
                }
            };
            // Get all users in the channel
            let members: Vec<Member> = vec
                .into_iter()
                // Don't include bots
                .filter(|f| !f.user.bot)
                .collect();

            // No Human users left, just the bot is left
            if members.len() == 0 {
                // info!("No more human users left. Leaving channel");
                return true;
            } else {
                // trace!("Human users still in channel.");
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

    let guild_id = match new_state.guild_id {
        Some(id) => id,
        None => return None,
    };

    // Extract only the single value we need, then drop the read guard immediately.
    // Holding the guard across the channel-iteration loop (which calls into the cache)
    // would block any concurrent writer (e.g. cache_ready) for the entire duration.
    let afk_channel_id_option: Option<u64> = {
        let lock_guard = lock.read().await;
        lock_guard.get(&guild_id.get()).copied().unwrap_or(None)
    };

    // Clone the channels out of the cache so we don't hold a DashMap guard
    // while doing further cache lookups inside the loop (guild_channel.members).
    let channels: Vec<_> = ctx
        .cache
        .guild(guild_id)
        .expect("cannot clone guild from cache")
        .channels
        .values()
        .cloned()
        .collect();

    let mut highest_channel_id: ChannelId = ChannelId::new(1);
    let mut highest_channel_len: usize = 0;
    for guild_channel in &channels {
        let channel_id = guild_channel.id;
        if let Some(afk_channel_id) = afk_channel_id_option {
            // Ignore channels that are meant for afk
            if afk_channel_id == channel_id.get() {
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
    let manager = songbird::get(ctx)
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    let existed = manager.get(guild_id).is_some();
    if manager.remove(guild_id).await.is_ok() && existed {
        let data_read = ctx.data.read().await;
        if let Some(metrics) = data_read.get::<crate::BotMetricsKey>() {
            metrics
                .active_voice_connections
                .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
            let _ = metrics.update_tx.send(());
        }
    }

    info!("Left the voice channel");
}

pub async fn connect_to_voice_channel(
    pool: Pool<Postgres>,
    ctx: &Context,
    guild_id: GuildId,
    channel_id: ChannelId,
    user_id: u64,
) {
    info!("Connecting to voice channel");
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
    let result_handler_lock = manager.join(guild_id, channel_id).await;
    match result_handler_lock {
        Ok(handler_lock) => {
            info!("Joined {}", channel_id);

            if let Some(old_ch) = old_channel {
                // switching channels. Don't re-register. Cleanup
                info!("Clean up switching chanels");
            } else {
                let metrics = {
                    let data_read = ctx.data.read().await;
                    let m = data_read
                        .get::<crate::BotMetricsKey>()
                        .expect("BotMetrics not found");
                    m.active_voice_connections
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let _ = m.update_tx.send(());
                    m.clone()
                };

                let mut handler = handler_lock.lock().await;

                let ctx1 = Arc::new(ctx.clone());
                let receiver = Receiver::new(pool, ctx1, guild_id, channel_id, metrics).await;

                handler.add_global_event(CoreEvent::SpeakingStateUpdate.into(), receiver.clone());
                handler.add_global_event(CoreEvent::VoiceTick.into(), receiver.clone());
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
