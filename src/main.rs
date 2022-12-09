// use std::env;
#![allow(unused_variables)]
use std::collections::HashMap;

use serenity::{
    async_trait,
    model::{
        channel::Message,
        gateway::Ready,
        prelude::{
            automod::{ActionExecution, Rule},
            GuildChannel, GuildId, GuildScheduledEventUserAddEvent,
            GuildScheduledEventUserRemoveEvent, PartialGuildChannel, ScheduledEvent, StageInstance,
            StickerId, ThreadListSyncEvent, ThreadMember, ThreadMembersUpdateEvent,
        },
        sticker::Sticker,
    },
    prelude::*,
    Result as SerenityResult,
};
use songbird::{driver::DecodeMode, Config, SerenityInit};
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

pub mod commands;
pub mod config;
pub mod events;

struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn application_command_permissions_update(
        &self,
        _ctx: Context,
        _permission: serenity::model::prelude::command::CommandPermission,
    ) {
    }

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
        // // Ensure we have the same guilds as we curently received
        // guilds::sync_guilds(&ctx, guilds).await;
        // // Ensure the same users are present in the DB. NOTE: in large this will probably wont work(?).
        // users::sync_users(&ctx).await;
        // // Ensure the same roles are present in the DB.
        // roles::sync_roles(&ctx).await;
        // // Ensure the same channels are present in the DB.
        // channels::sync_channels(&ctx).await;
        // // Diffrent roles can have diffrent permissions in different channels.
        // channels::sync_channel_roles(&ctx).await;
        // emojis::sync_emojis(&ctx).await;
    }

    async fn channel_create(
        &self,
        _ctx: Context,
        _channel: &serenity::model::channel::GuildChannel,
    ) {
        events::channels::channel_create().await;
    }

    async fn category_create(
        &self,
        _ctx: Context,
        _category: &serenity::model::channel::ChannelCategory,
    ) {
        events::channels::category_create().await;
    }

    async fn category_delete(
        &self,
        _ctx: Context,
        _category: &serenity::model::channel::ChannelCategory,
    ) {
        events::channels::category_delete().await;
    }

    async fn channel_delete(
        &self,
        _ctx: Context,
        _channel: &serenity::model::channel::GuildChannel,
    ) {
        events::channels::channel_delete().await;
    }

    async fn channel_pins_update(
        &self,
        _ctx: Context,
        _pin: serenity::model::event::ChannelPinsUpdateEvent,
    ) {
        events::channels::channel_pins_update().await;
    }

    async fn channel_update(
        &self,
        _ctx: Context,
        _old: Option<serenity::model::channel::Channel>,
        _new: serenity::model::channel::Channel,
    ) {
        events::channels::channel_update().await;
    }

    async fn guild_ban_addition(
        &self,
        _ctx: Context,
        _guild_id: serenity::model::id::GuildId,
        _banned_user: serenity::model::prelude::User,
    ) {
        events::guilds::guild_ban_addition(_ctx, _guild_id, _banned_user).await;
    }

    async fn guild_ban_removal(
        &self,
        _ctx: Context,
        _guild_id: serenity::model::id::GuildId,
        _unbanned_user: serenity::model::prelude::User,
    ) {
        events::guilds::guild_ban_removal(_ctx, _guild_id, _unbanned_user).await;
    }

    async fn guild_create(
        &self,
        _ctx: Context,
        _guild: serenity::model::guild::Guild,
        _is_new: bool,
    ) {
        events::guilds::guild_create(_ctx, _guild, _is_new).await;
    }

    async fn guild_delete(
        &self,
        _ctx: Context,
        _incomplete: serenity::model::guild::UnavailableGuild,
        _full: Option<serenity::model::guild::Guild>,
    ) {
        events::guilds::guild_delete(_ctx, _incomplete, _full).await;
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
        events::emojis::guild_emojis_update(_ctx, _guild_id, _current_state).await;
    }

    async fn guild_integrations_update(
        &self,
        _ctx: Context,
        _guild_id: serenity::model::id::GuildId,
    ) {
        events::integrations::guild_integrations_update(_ctx, _guild_id).await;
    }

    async fn guild_member_addition(
        &self,
        _ctx: Context,
        _new_member: serenity::model::guild::Member,
    ) {
        events::guilds::guild_member_addition(_ctx, _new_member).await;
    }

    async fn guild_member_removal(
        &self,
        _ctx: Context,
        _guild_id: serenity::model::id::GuildId,
        _user: serenity::model::prelude::User,
        _member_data_if_available: Option<serenity::model::guild::Member>,
    ) {
        events::guilds::guild_member_removal(_ctx, _guild_id, _user, _member_data_if_available)
            .await;
    }

    async fn guild_member_update(
        &self,
        _ctx: Context,
        _old_if_available: Option<serenity::model::guild::Member>,
        _new: serenity::model::guild::Member,
    ) {
        events::guilds::guild_member_update(_ctx, _old_if_available, _new).await;
    }

    async fn guild_members_chunk(
        &self,
        _ctx: Context,
        _chunk: serenity::model::event::GuildMembersChunkEvent,
    ) {
        events::guilds::guild_members_chunk(_ctx, _chunk).await;
    }

    async fn guild_role_create(&self, _ctx: Context, _new: serenity::model::guild::Role) {
        events::roles::guild_role_create(_ctx, _new).await;
    }

    async fn guild_role_delete(
        &self,
        _ctx: Context,
        _guild_id: serenity::model::id::GuildId,
        _removed_role_id: serenity::model::id::RoleId,
        _removed_role_data_if_available: Option<serenity::model::guild::Role>,
    ) {
        events::roles::guild_role_delete(
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
        events::roles::guild_role_update(_ctx, _old_data_if_available, _new).await;
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

    async fn guild_unavailable(&self, _ctx: Context, _guild_id: serenity::model::id::GuildId) {}

    async fn guild_update(
        &self,
        _ctx: Context,
        _old_data_if_available: Option<serenity::model::guild::Guild>,
        _new_but_incomplete: serenity::model::guild::PartialGuild,
    ) {
        events::guilds::guild_update(_ctx, _old_data_if_available, _new_but_incomplete).await;
    }

    async fn invite_create(&self, _ctx: Context, _data: serenity::model::event::InviteCreateEvent) {
        events::invites::invite_create(_ctx, _data).await;
    }

    async fn invite_delete(&self, _ctx: Context, _data: serenity::model::event::InviteDeleteEvent) {
        events::invites::invite_delete(_ctx, _data).await;
    }

    async fn message(&self, _ctx: Context, msg: Message) {
        events::messages::message(_ctx, msg).await;
    }

    async fn message_delete(
        &self,
        _ctx: Context,
        _channel_id: serenity::model::id::ChannelId,
        _deleted_message_id: serenity::model::id::MessageId,
        _guild_id: Option<serenity::model::id::GuildId>,
    ) {
        events::messages::message_delete(_ctx, _channel_id, _deleted_message_id, _guild_id).await;
    }
    async fn message_delete_bulk(
        &self,
        _ctx: Context,
        _channel_id: serenity::model::id::ChannelId,
        _multiple_deleted_messages_ids: Vec<serenity::model::id::MessageId>,
        _guild_id: Option<serenity::model::id::GuildId>,
    ) {
        events::messages::message_delete_bulk(
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
        events::messages::message_update(_ctx, _old_if_available, _new, _event).await;
    }

    async fn reaction_add(&self, _ctx: Context, _add_reaction: serenity::model::channel::Reaction) {
        events::reactions::reaction_add(_ctx, _add_reaction).await;
    }
    async fn reaction_remove(
        &self,
        _ctx: Context,
        _removed_reaction: serenity::model::channel::Reaction,
    ) {
        events::reactions::reaction_remove(_ctx, _removed_reaction).await;
    }
    async fn reaction_remove_all(
        &self,
        _ctx: Context,
        _channel_id: serenity::model::id::ChannelId,
        _removed_from_message_id: serenity::model::id::MessageId,
    ) {
        events::reactions::reaction_remove_all(_ctx, _channel_id, _removed_from_message_id).await;
    }
    // TODO
    async fn presence_replace(&self, _ctx: Context, _: Vec<serenity::model::prelude::Presence>) {}
    // TODO
    async fn presence_update(&self, _ctx: Context, _new_data: serenity::model::prelude::Presence) {}
    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);

        let guild_id = GuildId(850192711649722368);

        let commands = GuildId::set_application_commands(&guild_id, &ctx.http, |commands| {
            commands
                .create_application_command(|command| {
                    commands::play_clip_command::register(command)
                })
                .create_application_command(|command| commands::clip_it::register(command))
        })
        .await
        .unwrap();

        // serenity::model::prelude::command::Command::create_global_application_command(
        //     &ctx.http,
        //     |command| commands::play_clip_command::register(command),
        // )
        // .await
        // .unwrap();
    }

    // TODO
    async fn resume(&self, _ctx: Context, _: serenity::model::event::ResumedEvent) {}

    // TODO
    async fn shard_stage_update(
        &self,
        _ctx: Context,
        _: serenity::client::bridge::gateway::event::ShardStageUpdateEvent,
    ) {
    }

    // TODO
    async fn typing_start(&self, _ctx: Context, _: serenity::model::event::TypingStartEvent) {}

    // TODO
    async fn unknown(&self, _ctx: Context, _name: String, _raw: serde_json::Value) {}

    // TODO
    async fn user_update(
        &self,
        _ctx: Context,
        _old_data: serenity::model::prelude::CurrentUser,
        _new: serenity::model::prelude::CurrentUser,
    ) {
        println!("bot Updated. Old: {:#?}, New: {:#?}", _old_data, _new);
    }

    async fn voice_server_update(
        &self,
        _ctx: Context,
        _update: serenity::model::event::VoiceServerUpdateEvent,
    ) {
        events::voice::voice_server_update(_ctx, _update).await;
    }

    async fn voice_state_update(
        &self,
        _ctx: Context,
        _old: Option<serenity::model::prelude::VoiceState>,
        _new: serenity::model::prelude::VoiceState,
    ) {
        events::voice::voice_state_update(_ctx, _old, _new).await;
    }

    // TODO
    async fn webhook_update(
        &self,
        _ctx: Context,
        _guild_id: serenity::model::id::GuildId,
        _belongs_to_channel_id: serenity::model::id::ChannelId,
    ) {
    }

    async fn interaction_create(
        &self,
        _ctx: Context,
        _interaction: serenity::model::prelude::interaction::Interaction,
    ) {
        events::interactions::interaction_create(_ctx, _interaction).await;
    }

    async fn integration_create(
        &self,
        _ctx: Context,
        _integration: serenity::model::guild::Integration,
    ) {
        events::integrations::integration_create(_ctx, _integration).await;
    }

    async fn integration_update(
        &self,
        _ctx: Context,
        _integration: serenity::model::guild::Integration,
    ) {
        events::integrations::integration_update(_ctx, _integration).await;
    }

    async fn integration_delete(
        &self,
        _ctx: Context,
        _integration_id: serenity::model::id::IntegrationId,
        _guild_id: serenity::model::id::GuildId,
        _application_id: Option<serenity::model::id::ApplicationId>,
    ) {
        events::integrations::integration_delete(_ctx, _integration_id, _guild_id, _application_id)
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

    /// Dispatched when a thread is updated.
    ///
    /// Provides the updated thread.
    async fn thread_update(&self, _ctx: Context, _thread: GuildChannel) {}

    /// Dispatched when a thread is deleted.
    ///
    /// Provides the partial deleted thread.
    async fn thread_delete(&self, _ctx: Context, _thread: PartialGuildChannel) {}

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

// pub struct MysqlConnection;

// impl TypeMapKey for MysqlConnection {
//     type Value = mysql_async::Pool;
// }

pub struct HasBossMusic;
impl TypeMapKey for HasBossMusic {
    type Value = HashMap<u64, Option<String>>;
}

#[tokio::main]
async fn main() {
    // install global collector configured based on RUST_LOG env var.
    let subscriber = FmtSubscriber::builder()
        // .with_thread_names(true)
        // .with_file(true)
        // .with_target(true)
        // .with_line_number(true)
        // all spans/events with a level higher than TRACE (e.g, debug, info, warn, etc.)
        // will be written to stdout.
        .with_max_level(Level::INFO)
        .pretty()
        // completes the builder.
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    info!("pog");
    tracing::log::info!("yak shaving completed.");
    // create relevant folders
    if !std::path::Path::new(events::voice::RECORDING_FILE_PATH).exists() {
        match tokio::fs::create_dir_all(events::voice::RECORDING_FILE_PATH).await {
            Ok(_) => {}
            Err(err) => {
                panic!("cannot create path: {}", err)
            }
        };
    }
    // let mysql_pool = mysql_async::Pool::new(config::DB_URL);
    // let conn = mysql_pool.get_conn().await.unwrap();

    // let a = conn.exec_map("SELECT * FROM guilds WHERE id IN (:id)", db_param, | id | DBGuild { id });
    // Configure the client with your Discord bot token in the environment.
    let token = config::TOKEN;
    let application_id = config::APPLICATION_ID;

    // Here, we need to configure Songbird to decode all incoming voice packets.
    // If you want, you can do this on a per-call basis---here, we need it to
    // read the audio data that other people are sending us!
    let songbird_config = Config::default().decode_mode(DecodeMode::Decode);

    let intents = GatewayIntents::all();
    // Create a new instance of the Client, logging in as a bot. This will
    // automatically prepend your bot token with "Bot ", which is a requirement
    // by Discord for bot users.
    let mut client = Client::builder(&token, intents)
        .event_handler(Handler)
        .intents(intents)
        .register_songbird_from_config(songbird_config)
        .application_id(application_id)
        .await
        .expect("Err creating client");
    {
        let mut data = client.data.write().await;
        // data.insert::<MysqlConnection>(mysql_pool.clone());
        data.insert::<HasBossMusic>(HashMap::new());
    }

    // Finally, start a single shard, and start listening to events.
    //
    // Shards will automatically attempt to reconnect, and will perform
    // exponential backoff until it reconnects.
    if let Err(why) = client.start().await {
        println!("Client error: {:?}", why);
    }
}

pub fn check_msg(result: SerenityResult<Message>) {
    if let Err(why) = result {
        println!("Error sending message: {:?}", why);
    }
}
