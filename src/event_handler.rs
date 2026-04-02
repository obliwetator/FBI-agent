// use std::env;

use std::collections::HashMap;

use serenity::{
    async_trait,
    model::{
        channel::Message,
        gateway::Ready,
        guild::Guild,
        prelude::{
            GuildChannel, GuildId, GuildScheduledEventUserAddEvent,
            GuildScheduledEventUserRemoveEvent, ScheduledEvent, StageInstance, StickerId,
            ThreadListSyncEvent, ThreadMember, ThreadMembersUpdateEvent,
            automod::{ActionExecution, Rule},
        },
        sticker::Sticker,
    },
    prelude::*,
};

use sqlx::{Pool, Postgres};
use tracing::info;

use crate::{database, events, get_lock_read};

pub struct Handler {
    pub(crate) database: Pool<Postgres>,
}

#[async_trait]
impl EventHandler for Handler {
    /// Dispatched when an auto moderation rule was created.
    ///
    /// Provides said rule's data.
    async fn auto_moderation_rule_create(&self, _ctx: Context, _rule: Rule) {}

    /// Dispatched when an auto moderation rule was updated.
    ///
    /// Provides said rule's data.
    async fn auto_moderation_rule_update(&self, _ctx: Context, _rule: Rule) {}

    /// Dispatched when an auto moderation rule was deleted.
    ///
    /// Provides said rule's data.
    async fn auto_moderation_rule_delete(&self, _ctx: Context, _rule: Rule) {}

    /// Dispatched when an auto moderation rule was triggered and an action was executed.
    ///
    /// Provides said action execution's data.
    async fn auto_moderation_action_execution(&self, _ctx: Context, _execution: ActionExecution) {}

    async fn cache_ready(&self, ctx: Context, guilds: Vec<serenity::model::id::GuildId>) {
        let guild_cached: &Vec<Guild> = &guilds
            .iter()
            .map(|guild| {
                let x = guild.to_guild_cached(&ctx).unwrap().to_owned();
                x
            })
            .collect();
        let lock = get_lock_read(&ctx).await;

        {
            // Acquire the write lock once for the entire loop instead of once per
            // iteration.  Repeatedly dropping and re-acquiring the write lock lets
            // readers (e.g. get_channel_with_most_members) slip in between iterations
            // and causes unnecessary contention.
            let mut guard = lock.write().await;
            for ele in guild_cached {
                if let Some(channel_id) = &ele.afk_metadata {
                    guard.insert(ele.id.get(), Some(channel_id.afk_channel_id.get()));
                } else {
                    guard.insert(ele.id.get(), None);
                }
            }
        }

        let _ = database::update_info(self, &ctx, &guilds).await;
        let _ = database::channels::update_guilds(self, &ctx, &guilds).await;
        let _ = database::channels::update_guild_channels(self, &ctx, &guilds).await;
    }

    async fn channel_pins_update(
        &self,
        _ctx: Context,
        _pin: serenity::model::event::ChannelPinsUpdateEvent,
    ) {
        events::channels::channel_pins_update().await;
    }

    async fn guild_ban_addition(
        &self,
        _ctx: Context,
        _guild_id: serenity::model::id::GuildId,
        _banned_user: serenity::model::prelude::User,
    ) {
        events::guilds::guild_ban_addition(self, _ctx, _guild_id, _banned_user).await;
    }

    async fn guild_ban_removal(
        &self,
        _ctx: Context,
        _guild_id: serenity::model::id::GuildId,
        _unbanned_user: serenity::model::prelude::User,
    ) {
        events::guilds::guild_ban_removal(self, _ctx, _guild_id, _unbanned_user).await;
    }

    async fn guild_create(
        &self,
        _ctx: Context,
        _guild: serenity::model::guild::Guild,
        _is_new: Option<bool>,
    ) {
        events::guilds::guild_create(self, _ctx, _guild, _is_new).await;
    }

    async fn guild_delete(
        &self,
        _ctx: Context,
        _incomplete: serenity::model::guild::UnavailableGuild,
        _full: Option<serenity::model::guild::Guild>,
    ) {
        events::guilds::guild_delete(self, _ctx, _incomplete, _full).await;
    }

    async fn guild_emojis_update(
        &self,
        _ctx: Context,
        _guild_id: serenity::model::id::GuildId,
        _current_state: std::collections::HashMap<
            serenity::model::id::EmojiId,
            serenity::model::guild::Emoji,
        >,
    ) {
        events::emojis::guild_emojis_update(self, _ctx, _guild_id, _current_state).await;
    }

    async fn guild_integrations_update(
        &self,
        _ctx: Context,
        _guild_id: serenity::model::id::GuildId,
    ) {
        events::integrations::guild_integrations_update(self, _ctx, _guild_id).await;
    }

    async fn guild_member_addition(
        &self,
        _ctx: Context,
        _new_member: serenity::model::guild::Member,
    ) {
        events::guilds::guild_member_addition(self, _ctx, _new_member).await;
    }

    async fn guild_member_removal(
        &self,
        _ctx: Context,
        _guild_id: serenity::model::id::GuildId,
        _user: serenity::model::prelude::User,
        _member_data_if_available: Option<serenity::model::guild::Member>,
    ) {
        events::guilds::guild_member_removal(
            self,
            _ctx,
            _guild_id,
            _user,
            _member_data_if_available,
        )
        .await;
    }

    async fn guild_members_chunk(
        &self,
        _ctx: Context,
        _chunk: serenity::model::event::GuildMembersChunkEvent,
    ) {
        events::guilds::guild_members_chunk(self, _ctx, _chunk).await;
    }

    async fn guild_role_create(&self, _ctx: Context, _new: serenity::model::guild::Role) {
        events::roles::guild_role_create(self, _ctx, _new).await;
    }

    async fn guild_role_delete(
        &self,
        _ctx: Context,
        _guild_id: serenity::model::id::GuildId,
        _removed_role_id: serenity::model::id::RoleId,
        _removed_role_data_if_available: Option<serenity::model::guild::Role>,
    ) {
        events::roles::guild_role_delete(
            self,
            _ctx,
            _guild_id,
            _removed_role_id,
            _removed_role_data_if_available,
        )
        .await;
    }

    async fn guild_role_update(
        &self,
        _ctx: Context,
        _old_data_if_available: Option<serenity::model::guild::Role>,
        _new: serenity::model::guild::Role,
    ) {
        events::roles::guild_role_update(self, _ctx, _old_data_if_available, _new).await;
    }

    /// Dispatched when the stickers are updated.
    ///
    /// Provides the guild's id and the new state of the stickers in the guild.
    async fn guild_stickers_update(
        &self,
        _ctx: Context,
        _guild_id: GuildId,
        _current_state: HashMap<StickerId, Sticker>,
    ) {
    }

    async fn guild_update(
        &self,
        _ctx: Context,
        _old_data_if_available: Option<serenity::model::guild::Guild>,
        _new_but_incomplete: serenity::model::guild::PartialGuild,
    ) {
        events::guilds::guild_update(self, _ctx, _old_data_if_available, _new_but_incomplete).await;
    }

    async fn invite_create(&self, _ctx: Context, _data: serenity::model::event::InviteCreateEvent) {
        events::invites::invite_create(self, _ctx, _data).await;
    }

    async fn invite_delete(&self, _ctx: Context, _data: serenity::model::event::InviteDeleteEvent) {
        events::invites::invite_delete(self, _ctx, _data).await;
    }

    async fn message(&self, _ctx: Context, msg: Message) {
        events::messages::message(self, _ctx, msg).await;
    }

    async fn message_delete(
        &self,
        _ctx: Context,
        _channel_id: serenity::model::id::ChannelId,
        _deleted_message_id: serenity::model::id::MessageId,
        _guild_id: Option<serenity::model::id::GuildId>,
    ) {
        events::messages::message_delete(self, _ctx, _channel_id, _deleted_message_id, _guild_id)
            .await;
    }
    async fn message_delete_bulk(
        &self,
        _ctx: Context,
        _channel_id: serenity::model::id::ChannelId,
        _multiple_deleted_messages_ids: Vec<serenity::model::id::MessageId>,
        _guild_id: Option<serenity::model::id::GuildId>,
    ) {
        events::messages::message_delete_bulk(
            self,
            _ctx,
            _channel_id,
            _multiple_deleted_messages_ids,
            _guild_id,
        )
        .await;
    }
    async fn message_update(
        &self,
        _ctx: Context,
        _old_if_available: Option<Message>,
        _new: Option<Message>,
        _event: serenity::model::event::MessageUpdateEvent,
    ) {
        events::messages::message_update(self, _ctx, _old_if_available, _new, _event).await;
    }

    async fn reaction_add(&self, _ctx: Context, _add_reaction: serenity::model::channel::Reaction) {
        events::reactions::reaction_add(self, _ctx, _add_reaction).await;
    }
    async fn reaction_remove(
        &self,
        _ctx: Context,
        _removed_reaction: serenity::model::channel::Reaction,
    ) {
        events::reactions::reaction_remove(self, _ctx, _removed_reaction).await;
    }
    async fn reaction_remove_all(
        &self,
        _ctx: Context,
        _channel_id: serenity::model::id::ChannelId,
        _removed_from_message_id: serenity::model::id::MessageId,
    ) {
        events::reactions::reaction_remove_all(self, _ctx, _channel_id, _removed_from_message_id)
            .await;
    }
    // TODO
    async fn presence_replace(&self, _ctx: Context, _: Vec<serenity::model::prelude::Presence>) {}
    // TODO
    async fn presence_update(&self, _ctx: Context, _new_data: serenity::model::prelude::Presence) {}
    async fn ready(&self, ctx: Context, ready: Ready) {
        info!("{} is connected!", ready.user.name);

        let guild_id = GuildId::new(850192711649722368);
        database::update_guild_present(ready.guilds, self).await;

        if let Err(why) = guild_id
            .set_commands(&ctx.http, vec![crate::commands::jam::register()])
            .await
        {
            info!("Cannot register slash commands: {}", why);
        }
    }

    async fn resume(&self, _ctx: Context, _: serenity::model::event::ResumedEvent) {
        info!("Resumed");
        let data_read = _ctx.data.read().await;
        if let Some(metrics) = data_read.get::<crate::BotMetricsKey>() {
            metrics
                .gateway_reconnects
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let _ = metrics.update_tx.send(());
        }
    }

    // TODO
    async fn shard_stage_update(&self, _ctx: Context, _: serenity::gateway::ShardStageUpdateEvent) {
    }

    // TODO
    async fn typing_start(&self, _ctx: Context, _: serenity::model::event::TypingStartEvent) {}

    // TODO
    async fn user_update(
        &self,
        _ctx: Context,
        _old_data: Option<serenity::model::prelude::CurrentUser>,
        _new: serenity::model::prelude::CurrentUser,
    ) {
        info!("bot Updated. Old: {:#?}, New: {:#?}", _old_data, _new);
    }

    async fn voice_server_update(
        &self,
        _ctx: Context,
        _update: serenity::model::event::VoiceServerUpdateEvent,
    ) {
        events::voice::voice_server_update(self, _ctx, _update).await;
    }

    async fn voice_state_update(
        &self,
        _ctx: Context,
        _old: Option<serenity::model::prelude::VoiceState>,
        _new: serenity::model::prelude::VoiceState,
    ) {
        {
            let data_read = _ctx.data.read().await;
            if let Some(metrics) = data_read.get::<crate::BotMetricsKey>() {
                metrics
                    .voice_state_updates_received
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
        events::voice::voice_state_update(self, _ctx, _old, _new).await;
    }

    // TODO
    async fn webhook_update(
        &self,
        _ctx: Context,
        _guild_id: serenity::model::id::GuildId,
        _belongs_to_channel_id: serenity::model::id::ChannelId,
    ) {
    }

    async fn interaction_create(&self, _ctx: Context, _interaction: serenity::all::Interaction) {
        {
            let data_read = _ctx.data.read().await;
            if let Some(metrics) = data_read.get::<crate::BotMetricsKey>() {
                metrics
                    .commands_executed
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let _ = metrics.update_tx.send(());
            }
        }
        events::interactions::interaction_create(self, _ctx, _interaction).await;
    }

    async fn integration_create(
        &self,
        _ctx: Context,
        _integration: serenity::model::guild::Integration,
    ) {
        events::integrations::integration_create(self, _ctx, _integration).await;
    }

    async fn integration_update(
        &self,
        _ctx: Context,
        _integration: serenity::model::guild::Integration,
    ) {
        events::integrations::integration_update(self, _ctx, _integration).await;
    }

    async fn integration_delete(
        &self,
        _ctx: Context,
        _integration_id: serenity::model::id::IntegrationId,
        _guild_id: serenity::model::id::GuildId,
        _application_id: Option<serenity::model::id::ApplicationId>,
    ) {
        events::integrations::integration_delete(
            self,
            _ctx,
            _integration_id,
            _guild_id,
            _application_id,
        )
        .await;
    }

    /// Dispatched when a stage instance is created.
    ///
    /// Provides the created stage instance.
    async fn stage_instance_create(&self, _ctx: Context, _stage_instance: StageInstance) {}

    /// Dispatched when a stage instance is updated.
    ///
    /// Provides the updated stage instance.
    async fn stage_instance_update(&self, _ctx: Context, _stage_instance: StageInstance) {}

    /// Dispatched when a stage instance is deleted.
    ///
    /// Provides the deleted stage instance.
    async fn stage_instance_delete(&self, _ctx: Context, _stage_instance: StageInstance) {}

    /// Dispatched when a thread is created or the current user is added
    /// to a private thread.
    ///
    /// Provides the thread.
    async fn thread_create(&self, _ctx: Context, _thread: GuildChannel) {}

    /// Dispatched when the current user gains access to a channel
    ///
    /// Provides the threads the current user can access, the thread members,
    /// the guild Id, and the channel Ids of the parent channels being synced.
    async fn thread_list_sync(&self, _ctx: Context, _thread_list_sync: ThreadListSyncEvent) {}

    /// Dispatched when the [`ThreadMember`] for the current user is updated.
    ///
    /// Provides the updated thread member.
    async fn thread_member_update(&self, _ctx: Context, _thread_member: ThreadMember) {}

    /// Dispatched when anyone is added to or removed from a thread. If the current user does not have the [`GatewayIntents::GUILDS`],
    /// then this event will only be sent if the current user was added to or removed from the thread.
    ///
    /// Provides the added/removed members, the approximate member count of members in the thread,
    /// the thread Id and its guild Id.
    ///
    /// [`GatewayIntents::GUILDS`]: crate::model::gateway::GatewayIntents::GUILDS
    async fn thread_members_update(
        &self,
        _ctx: Context,
        _thread_members_update: ThreadMembersUpdateEvent,
    ) {
    }

    /// Dispatched when a scheduled event is created.
    ///
    /// Provides data about the scheduled event.
    async fn guild_scheduled_event_create(&self, _ctx: Context, _event: ScheduledEvent) {}

    /// Dispatched when a scheduled event is updated.
    ///
    /// Provides data about the scheduled event.
    async fn guild_scheduled_event_update(&self, _ctx: Context, _event: ScheduledEvent) {}

    /// Dispatched when a scheduled event is deleted.
    ///
    /// Provides data about the scheduled event.
    async fn guild_scheduled_event_delete(&self, _ctx: Context, _event: ScheduledEvent) {}

    /// Dispatched when a guild member has subscribed to a scheduled event.
    ///
    /// Provides data about the subscription.
    async fn guild_scheduled_event_user_add(
        &self,
        _ctx: Context,
        _subscribed: GuildScheduledEventUserAddEvent,
    ) {
    }

    /// Dispatched when a guild member has unsubscribed from a scheduled event.
    ///
    /// Provides data about the cancelled subscription.
    async fn guild_scheduled_event_user_remove(
        &self,
        _ctx: Context,
        _unsubscribed: GuildScheduledEventUserRemoveEvent,
    ) {
    }
}
