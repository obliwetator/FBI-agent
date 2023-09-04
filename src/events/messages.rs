use std::time::Instant;

use serenity::{
    client::Context,
    model::channel::{Message, MessageType},
};

use crate::event_handler::Handler;

pub async fn message(_self: &Handler, ctx: Context, msg: Message) {
    // let pool = db_helper::get_pool_from_ctx(&ctx).await;
    // db_helper::get_channels(&pool).await;

    let now = Instant::now();
    // println!("message is {}", msg.content);
    match msg.kind {
        MessageType::Regular => {}
        MessageType::ChannelFollowAdd => {}
        MessageType::GroupCallCreation => {}
        MessageType::GroupIconUpdate => {}
        MessageType::GroupNameUpdate => {}
        MessageType::GroupRecipientAddition => {}
        MessageType::GroupRecipientRemoval => {}
        MessageType::GuildDiscoveryDisqualified => {}
        MessageType::GuildDiscoveryRequalified => {}
        MessageType::GuildInviteReminder => {}
        MessageType::InlineReply => {}
        MessageType::MemberJoin => {}
        MessageType::NitroBoost => {}
        MessageType::NitroTier1 => {}
        MessageType::NitroTier2 => {}
        MessageType::NitroTier3 => {}
        MessageType::PinsAdd => {}
        MessageType::GuildDiscoveryGracePeriodInitialWarning => todo!(),
        MessageType::GuildDiscoveryGracePeriodFinalWarning => todo!(),
        MessageType::ThreadCreated => todo!(),
        MessageType::ChatInputCommand => {
            // info!("slash command message: {:#?}", msg);
        }
        MessageType::ThreadStarterMessage => todo!(),
        MessageType::ContextMenuCommand => todo!(),
        MessageType::AutoModerationAction => todo!(),
        MessageType::Unknown => {}
        _ => {
            println!("unkown type");
        }
    }
}

pub async fn message_delete(
    _self: &Handler,
    _ctx: Context,
    _channel_id: serenity::model::id::ChannelId,
    _deleted_message_id: serenity::model::id::MessageId,
    _guild_id: Option<serenity::model::id::GuildId>,
) {
    todo!()
}

pub async fn message_delete_bulk(
    _self: &Handler,
    _ctx: Context,
    _channel_id: serenity::model::id::ChannelId,
    _multiple_deleted_messages_ids: Vec<serenity::model::id::MessageId>,
    _guild_id: Option<serenity::model::id::GuildId>,
) {
    todo!()
}

pub async fn message_update(
    _self: &Handler,
    _ctx: Context,
    _old_if_available: Option<Message>,
    _new: Option<Message>,
    _event: serenity::model::event::MessageUpdateEvent,
) {
    todo!()
}
