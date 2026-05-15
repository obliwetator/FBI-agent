use crate::{event_handler::Handler, events::voice_receiver::Receiver, get_lock_read};
use serenity::{
    client::Context,
    model::id::{ChannelId, GuildId},
};
use songbird::CoreEvent;
use sqlx::{Pool, Postgres};
use std::{sync::Arc, time::Duration};
use tracing::{error, info, warn};

// Voice state event type IDs match rows seeded in the voice_state_event_types
// migrations.
const EVT_SERVER_MUTE: i32 = 1;
const EVT_SERVER_UNMUTE: i32 = 2;
const EVT_SERVER_DEAFEN: i32 = 3;
const EVT_SERVER_UNDEAFEN: i32 = 4;
const EVT_SELF_MUTE: i32 = 5;
const EVT_SELF_UNMUTE: i32 = 6;
const EVT_SELF_DEAFEN: i32 = 7;
const EVT_SELF_UNDEAFEN: i32 = 8;
const EVT_SUPPRESS_ON: i32 = 9;
const EVT_SUPPRESS_OFF: i32 = 10;
const EVT_STREAM_START: i32 = 11;
const EVT_STREAM_STOP: i32 = 12;
const EVT_VIDEO_ON: i32 = 13;
const EVT_VIDEO_OFF: i32 = 14;
const EVT_CHANNEL_JOIN: i32 = 15;
const EVT_CHANNEL_LEAVE: i32 = 16;
const EVT_CHANNEL_SWITCH: i32 = 17;
pub(super) const EVT_RECORDING_PAUSE: i32 = 18;
pub(super) const EVT_RECORDING_RESUME: i32 = 19;
pub(super) const EVT_USER_RECORDING_PAUSE: i32 = 20;
pub(super) const EVT_USER_RECORDING_RESUME: i32 = 21;

const LOG_VOICE_STATE_CHANGES: bool = false;
const EMPTY_CHANNEL_LEAVE_DEBOUNCE: Duration = Duration::from_secs(3);

const VOICE_FLAG_SERVER_MUTE: u8 = 1 << 0;
const VOICE_FLAG_SERVER_DEAF: u8 = 1 << 1;
const VOICE_FLAG_SELF_MUTE: u8 = 1 << 2;
const VOICE_FLAG_SELF_DEAF: u8 = 1 << 3;
const VOICE_FLAG_SUPPRESS: u8 = 1 << 4;
const VOICE_FLAG_VIDEO: u8 = 1 << 5;

pub async fn voice_server_update(
    _self: &Handler,
    _ctx: Context,
    _update: serenity::model::event::VoiceServerUpdateEvent,
) {
}

pub(super) async fn insert_voice_event(
    pool: &Pool<Postgres>,
    guild_id: i64,
    channel_id: Option<i64>,
    user_id: i64,
    event_type_id: i32,
) {
    if let Err(err) = sqlx::query!(
        "INSERT INTO voice_state_events (guild_id, channel_id, user_id, event_type_id) \
         VALUES ($1, $2, $3, $4)",
        guild_id,
        channel_id,
        user_id,
        event_type_id
    )
    .execute(pool)
    .await
    {
        warn!("Failed to insert voice_state_event: {}", err);
    }
}

struct VoiceFlagEvent {
    label: &'static str,
    enabled_event_type_id: i32,
    disabled_event_type_id: i32,
}

fn voice_state_flags(state: &serenity::model::prelude::VoiceState) -> u8 {
    let mut flags = 0;
    if state.mute {
        flags |= VOICE_FLAG_SERVER_MUTE;
    }
    if state.deaf {
        flags |= VOICE_FLAG_SERVER_DEAF;
    }
    if state.self_mute {
        flags |= VOICE_FLAG_SELF_MUTE;
    }
    if state.self_deaf {
        flags |= VOICE_FLAG_SELF_DEAF;
    }
    if state.suppress {
        flags |= VOICE_FLAG_SUPPRESS;
    }
    if state.self_video {
        flags |= VOICE_FLAG_VIDEO;
    }
    flags
}

fn voice_flag_event(flag: u8) -> Option<VoiceFlagEvent> {
    match flag {
        VOICE_FLAG_SERVER_MUTE => Some(VoiceFlagEvent {
            label: "User server muted changed",
            enabled_event_type_id: EVT_SERVER_MUTE,
            disabled_event_type_id: EVT_SERVER_UNMUTE,
        }),
        VOICE_FLAG_SERVER_DEAF => Some(VoiceFlagEvent {
            label: "User server deafened changed",
            enabled_event_type_id: EVT_SERVER_DEAFEN,
            disabled_event_type_id: EVT_SERVER_UNDEAFEN,
        }),
        VOICE_FLAG_SELF_MUTE => Some(VoiceFlagEvent {
            label: "User self muted changed",
            enabled_event_type_id: EVT_SELF_MUTE,
            disabled_event_type_id: EVT_SELF_UNMUTE,
        }),
        VOICE_FLAG_SELF_DEAF => Some(VoiceFlagEvent {
            label: "User self deafened changed",
            enabled_event_type_id: EVT_SELF_DEAFEN,
            disabled_event_type_id: EVT_SELF_UNDEAFEN,
        }),
        VOICE_FLAG_SUPPRESS => Some(VoiceFlagEvent {
            label: "User suppress status changed",
            enabled_event_type_id: EVT_SUPPRESS_ON,
            disabled_event_type_id: EVT_SUPPRESS_OFF,
        }),
        VOICE_FLAG_VIDEO => Some(VoiceFlagEvent {
            label: "User video status changed",
            enabled_event_type_id: EVT_VIDEO_ON,
            disabled_event_type_id: EVT_VIDEO_OFF,
        }),
        _ => None,
    }
}

async fn record_changed_voice_flag_events(
    pool: &Pool<Postgres>,
    guild_id: i64,
    channel_id: Option<i64>,
    user_id: i64,
    log_changes: bool,
    old_flags: u8,
    new_flags: u8,
) {
    let mut changed_flags = old_flags ^ new_flags;
    while changed_flags != 0 {
        let flag = 1u8 << changed_flags.trailing_zeros();
        changed_flags &= !flag;

        let Some(event) = voice_flag_event(flag) else {
            continue;
        };
        let old_value = old_flags & flag != 0;
        let new_value = new_flags & flag != 0;

        if log_changes {
            info!("{}: {} -> {}", event.label, old_value, new_value);
        }

        insert_voice_event(
            pool,
            guild_id,
            channel_id,
            user_id,
            if new_value {
                event.enabled_event_type_id
            } else {
                event.disabled_event_type_id
            },
        )
        .await;
    }
}

async fn record_voice_events(
    pool: &Pool<Postgres>,
    old: Option<&serenity::model::prelude::VoiceState>,
    new: &serenity::model::prelude::VoiceState,
    log_changes: bool,
) {
    let Some(guild_id) = new.guild_id.map(|g| g.get() as i64) else {
        return;
    };
    let user_id = new.user_id.get() as i64;
    let new_channel = new.channel_id.map(|c| c.get() as i64);

    // Channel transition events.
    match (old.and_then(|o| o.channel_id), new.channel_id) {
        (None, Some(new_ch)) => {
            if log_changes {
                info!("User joined voice channel: {}", new_ch);
            }
            insert_voice_event(
                pool,
                guild_id,
                Some(new_ch.get() as i64),
                user_id,
                EVT_CHANNEL_JOIN,
            )
            .await;
        }
        (Some(old_ch), None) => {
            if log_changes {
                info!("User left voice channel: {}", old_ch);
            }
            insert_voice_event(
                pool,
                guild_id,
                Some(old_ch.get() as i64),
                user_id,
                EVT_CHANNEL_LEAVE,
            )
            .await;
        }
        (Some(old_ch), Some(new_ch)) if old_ch != new_ch => {
            if log_changes {
                info!("User switched voice channels: {} -> {}", old_ch, new_ch);
            }
            insert_voice_event(
                pool,
                guild_id,
                Some(new_ch.get() as i64),
                user_id,
                EVT_CHANNEL_SWITCH,
            )
            .await;
        }
        _ => {}
    }

    // Per-field diffs require a previous state.
    let Some(old) = old else { return };

    record_changed_voice_flag_events(
        pool,
        guild_id,
        new_channel,
        user_id,
        log_changes,
        voice_state_flags(old),
        voice_state_flags(new),
    )
    .await;

    let old_streaming = old.self_stream.unwrap_or(false);
    let new_streaming = new.self_stream.unwrap_or(false);
    if old_streaming != new_streaming {
        if log_changes {
            info!(
                "User stream status changed: {:?} -> {:?}",
                old.self_stream, new.self_stream
            );
        }
        insert_voice_event(
            pool,
            guild_id,
            new_channel,
            user_id,
            if new_streaming {
                EVT_STREAM_START
            } else {
                EVT_STREAM_STOP
            },
        )
        .await;
    }

    if log_changes && old.request_to_speak_timestamp != new.request_to_speak_timestamp {
        info!(
            "User request to speak changed: {:?} -> {:?}",
            old.request_to_speak_timestamp, new.request_to_speak_timestamp
        );
    }
}

pub async fn voice_state_update(
    _self: &Handler,
    ctx: Context,
    old_state: Option<serenity::model::prelude::VoiceState>,
    new_state: serenity::model::prelude::VoiceState,
) {
    if should_skip_voice_state_for_lease(_self, new_state.guild_id).await {
        return;
    }

    // Persist voice events for non-bot users (timeline overlay on recordings).
    let is_bot = new_state
        .member
        .as_ref()
        .map(|m| m.user.bot)
        .unwrap_or(false);
    if !is_bot {
        record_voice_events(
            &_self.database,
            old_state.as_ref(),
            &new_state,
            LOG_VOICE_STATE_CHANGES,
        )
        .await;
    }

    // Notify the dashboard stream of any user voice state changes
    {
        let data_read = ctx.data.read().await;
        if let Some(metrics) = data_read.get::<crate::BotMetricsKey>() {
            let _ = metrics.voice_update_tx.send(());

            // Track user start times
            let user_id = new_state.user_id.get();
            if let Some(guild_id) = new_state.guild_id {
                metrics.track_voice_presence(
                    guild_id.get(),
                    user_id,
                    new_state
                        .channel_id
                        .map(|channel_id| crate::VoiceUserPresence {
                            channel_id: channel_id.get(),
                            is_bot,
                            server_mute: new_state.mute,
                            server_deaf: new_state.deaf,
                            self_mute: new_state.self_mute,
                            self_deaf: new_state.self_deaf,
                            suppress: new_state.suppress,
                            streaming: new_state.self_stream.unwrap_or(false),
                            video: new_state.self_video,
                        }),
                );
            }
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or_else(|err| {
                    error!("System clock before UNIX_EPOCH: {}", err);
                    0
                });

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
                error!("No guild id in voice_state_update");
                return;
            }
        };
        if member.user.bot {
            // Ignore bots
            return;
        }

        if let Some(channel_id) = empty_channel_candidate(&new_state, &ctx, &old_state).await {
            schedule_leave_if_still_empty(
                ctx.clone(),
                _self.database.clone(),
                guild_id,
                channel_id,
            );
            return;
        }

        if let Some(old) = old_state
            && let Some(new_channel_id) = new_state.channel_id
            && let Some(old_channel_id) = old.channel_id
        {
            // We can check for various things that happened after the user has connected
            // We don't care about any events at the moment
            if new_channel_id == old_channel_id {
                // An action happened that was NOT switching channels.
                return;
            } else {
                // user switched channels
            }
        }

        let (highest_channel_id, highest_channel_len) =
            match get_channel_with_most_members(&ctx, &new_state).await {
                Some(value) => value,
                None => {
                    warn!(
                        guild_id = guild_id.get(),
                        "Skipping voice join because channel membership could not be inspected"
                    );
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
    }
}

async fn should_skip_voice_state_for_lease(handler: &Handler, guild_id: Option<GuildId>) -> bool {
    let Some(guild_id) = guild_id else {
        return handler.runtime.is_draining();
    };

    match crate::deployment::active_lease_owner(&handler.database, guild_id).await {
        Ok(Some(owner)) => owner != handler.runtime.config().instance_id,
        Ok(None) => handler.runtime.is_draining(),
        Err(err) => {
            warn!(
                guild_id = guild_id.get(),
                "failed to inspect voice lease before voice-state routing: {}", err
            );
            handler.runtime.is_draining()
        }
    }
}

// If no humans remain, schedule a delayed re-check before leaving.
async fn empty_channel_candidate(
    new_state: &serenity::model::prelude::VoiceState,
    ctx: &Context,
    old_state: &Option<serenity::model::prelude::VoiceState>,
) -> Option<ChannelId> {
    if new_state.channel_id.is_none() {
        // Someone left the channel

        // get the channel id that user was in before disconnecting
        if let Some(channel_id) = old_state.as_ref().and_then(|s| s.channel_id) {
            match human_member_count(ctx, channel_id).await {
                Ok(0) => return Some(channel_id),
                Ok(_) => return None,
                Err(err) => {
                    warn!(
                        channel_id = channel_id.get(),
                        "Could not inspect channel before scheduling leave: {}", err
                    );
                    return None;
                }
            }
        }
    }
    None
}

async fn human_member_count(ctx: &Context, channel_id: ChannelId) -> Result<usize, String> {
    let current_channel = channel_id
        .to_channel(ctx)
        .await
        .map_err(|err| format!("could not resolve channel: {}", err))?;

    let guild_channel = current_channel
        .guild()
        .ok_or_else(|| "not a guild channel".to_string())?;

    let mut members = guild_channel
        .members(ctx)
        .map_err(|err| format!("could not get channel members: {}", err))?;

    members.retain(|member| !member.user.bot);
    Ok(members.len())
}

fn schedule_leave_if_still_empty(
    ctx: Context,
    pool: Pool<Postgres>,
    guild_id: GuildId,
    channel_id: ChannelId,
) {
    tokio::spawn(async move {
        info!(
            guild_id = guild_id.get(),
            channel_id = channel_id.get(),
            "empty channel leave scheduled"
        );

        tokio::time::sleep(EMPTY_CHANNEL_LEAVE_DEBOUNCE).await;

        let Some(manager) = songbird::get(&ctx).await else {
            error!("Songbird manager missing while rechecking empty voice channel");
            return;
        };
        let manager = manager.clone();

        let Some(call) = manager.get(guild_id) else {
            info!(
                guild_id = guild_id.get(),
                channel_id = channel_id.get(),
                "empty channel leave cancelled: bot moved/disconnected"
            );
            return;
        };

        let current_channel = call
            .lock()
            .await
            .current_channel()
            .map(|channel| channel.0.get());
        if current_channel != Some(channel_id.get()) {
            info!(
                guild_id = guild_id.get(),
                channel_id = channel_id.get(),
                current_channel = current_channel,
                "empty channel leave cancelled: bot moved/disconnected"
            );
            return;
        }

        match human_member_count(&ctx, channel_id).await {
            Ok(0) => {
                info!(
                    guild_id = guild_id.get(),
                    channel_id = channel_id.get(),
                    "empty channel leave confirmed"
                );
                leave_voice_channel(&ctx, &pool, guild_id).await;
            }
            Ok(count) => {
                info!(
                    guild_id = guild_id.get(),
                    channel_id = channel_id.get(),
                    humans = count,
                    "empty channel leave cancelled: humans returned"
                );
            }
            Err(err) => {
                warn!(
                    guild_id = guild_id.get(),
                    channel_id = channel_id.get(),
                    "empty channel leave cancelled: could not inspect channel: {}",
                    err
                );
            }
        }
    });
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
    let channels: Vec<_> = match ctx.cache.guild(guild_id) {
        Some(guild) => guild.channels.values().cloned().collect(),
        None => {
            error!(
                "Guild {} missing from cache while choosing voice channel",
                guild_id
            );
            return None;
        }
    };

    let mut highest_channel_id: ChannelId = ChannelId::new(1);
    let mut highest_channel_len: usize = 0;
    for guild_channel in &channels {
        let channel_id = guild_channel.id;
        if let Some(afk_channel_id) = afk_channel_id_option {
            // Ignore channels that are meant for afk
            if afk_channel_id == channel_id.get() {
                continue;
            }
        }
        if let serenity::model::prelude::ChannelType::Voice = guild_channel.kind {
            let count = match human_member_count(ctx, channel_id).await {
                Ok(count) => count,
                Err(err) => {
                    warn!(
                        channel_id = channel_id.get(),
                        "Could not inspect voice channel members: {}", err
                    );
                    return None;
                }
            };

            if count > highest_channel_len {
                highest_channel_len = count;
                highest_channel_id = guild_channel.id;
            }
        }
    }
    Some((highest_channel_id, highest_channel_len))
}

async fn leave_voice_channel(ctx: &Context, pool: &Pool<Postgres>, guild_id: GuildId) {
    let Some(manager) = songbird::get(ctx).await else {
        error!("Songbird manager missing while leaving voice channel");
        return;
    };
    let manager = manager.clone();

    let existed = manager.get(guild_id).is_some();
    if manager.remove(guild_id).await.is_ok() && existed {
        let data_read = ctx.data.read().await;
        if let Some(metrics) = data_read.get::<crate::BotMetricsKey>() {
            metrics
                .active_voice_connections
                .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
            let _ = metrics.update_tx.send(());
        }
        if let Some(runtime) = data_read.get::<crate::runtime::RuntimeStateKey>() {
            crate::deployment::release_voice_session(pool, runtime, guild_id).await;
        }
    }
}

pub async fn connect_to_voice_channel(
    pool: Pool<Postgres>,
    ctx: &Context,
    guild_id: GuildId,
    channel_id: ChannelId,
    user_id: u64,
) {
    if crate::runtime::is_draining_ctx(ctx).await {
        info!(
            guild_id = guild_id.get(),
            channel_id = channel_id.get(),
            "skipping voice connect while instance is draining"
        );
        return;
    }

    let Some(manager) = songbird::get(ctx).await else {
        error!("Songbird manager missing while connecting to voice channel");
        return;
    };
    let manager = manager.clone();

    if let Some(runtime) = crate::runtime::state_from_ctx(ctx).await {
        match crate::deployment::active_lease_owner(&pool, guild_id).await {
            Ok(Some(owner)) if owner != runtime.config().instance_id => {
                info!(
                    guild_id = guild_id.get(),
                    channel_id = channel_id.get(),
                    owner = %owner,
                    "skipping voice connect because another instance owns lease"
                );
                return;
            }
            Ok(_) => {}
            Err(err) => {
                warn!(
                    guild_id = guild_id.get(),
                    "failed to inspect voice lease before join: {}", err
                );
            }
        }
    }

    if let Some(arc_call) = manager.get(guild_id) {
        // already have call, check current channel
        let current = arc_call.lock().await.current_channel();

        match current {
            Some(ch) if ch.0.get() == channel_id.get() => {}
            Some(ch) => {
                join_ch(
                    pool,
                    manager,
                    guild_id,
                    channel_id,
                    ctx,
                    user_id,
                    JoinMode::Switch {
                        _old_channel: ch.0.get(),
                    },
                )
                .await;
            }
            None => {
                // Disconnected (e.g. kicked). Keep existing receiver handlers
                // so recoverable recording state can resume on DriverConnect.
                info!("Call exists but disconnected, rejoining");
                join_ch(
                    pool,
                    manager,
                    guild_id,
                    channel_id,
                    ctx,
                    user_id,
                    JoinMode::RejoinDisconnected,
                )
                .await;
            }
        }
    } else {
        join_ch(
            pool,
            manager,
            guild_id,
            channel_id,
            ctx,
            user_id,
            JoinMode::Fresh,
        )
        .await;
    }
}

enum JoinMode {
    Fresh,
    RejoinDisconnected,
    Switch { _old_channel: u64 },
}

async fn join_ch(
    pool: Pool<Postgres>,
    manager: Arc<songbird::Songbird>,
    guild_id: GuildId,
    channel_id: ChannelId,
    ctx: &Context,
    _user_id: u64,
    mode: JoinMode,
) {
    if !matches!(mode, JoinMode::Switch { .. }) {
        let handler_lock = manager.get_or_insert(guild_id);
        let result = {
            let mut handler = handler_lock.lock().await;
            if matches!(mode, JoinMode::Fresh) {
                register_voice_receiver(
                    &mut handler,
                    pool.clone(),
                    ctx,
                    guild_id,
                    channel_id,
                    true,
                )
                .await;
            }
            handler.join(channel_id).await
        };

        match result {
            Ok(join) => match join.await {
                Ok(()) => {
                    record_active_voice_connection(ctx).await;
                    if let Some(runtime) = crate::runtime::state_from_ctx(ctx).await {
                        crate::deployment::claim_voice_session(
                            &pool, &runtime, guild_id, channel_id,
                        )
                        .await;
                    }
                }
                Err(err) => {
                    error!("cannot join channel {}: {}", channel_id, err);
                    if let Err(remove_err) = manager.remove(guild_id).await {
                        error!("failed to clean up failed voice join: {}", remove_err);
                    }
                }
            },
            Err(err) => {
                error!("cannot join channel {}: {}", channel_id, err);
                if let Err(remove_err) = manager.remove(guild_id).await {
                    error!("failed to clean up failed voice join: {}", remove_err);
                }
            }
        }
        return;
    }

    let result_handler_lock = manager.join(guild_id, channel_id).await;
    match result_handler_lock {
        Ok(_) => {
            // switching channels. Don't re-register. Cleanup
            info!("Clean up switching chanels");
            if let Some(runtime) = crate::runtime::state_from_ctx(ctx).await {
                crate::deployment::claim_voice_session(&pool, &runtime, guild_id, channel_id).await;
            }
        }
        Err(err) => {
            error!("cannot join channel {}: {}", channel_id, err);
            if let Err(remove_err) = manager.remove(guild_id).await {
                error!("failed to clean up failed voice join: {}", remove_err);
            }
        }
    }
}

async fn register_voice_receiver(
    handler: &mut songbird::Call,
    pool: Pool<Postgres>,
    ctx: &Context,
    guild_id: GuildId,
    channel_id: ChannelId,
    reset_existing_handlers: bool,
) {
    if reset_existing_handlers {
        handler.remove_all_global_events();
    }

    let metrics = {
        let data_read = ctx.data.read().await;
        let Some(m) = data_read.get::<crate::BotMetricsKey>() else {
            error!("BotMetrics missing while joining voice channel");
            return;
        };
        m.clone()
    };

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

async fn record_active_voice_connection(ctx: &Context) {
    let data_read = ctx.data.read().await;
    let Some(metrics) = data_read.get::<crate::BotMetricsKey>() else {
        error!("BotMetrics missing while recording active voice connection");
        return;
    };

    metrics
        .active_voice_connections
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let _ = metrics.update_tx.send(());
}
