use serenity::client::Context;

use crate::event_handler::Handler;

pub async fn guild_ban_addition(
    _self: &Handler,
    _ctx: Context,
    _guild_id: serenity::model::id::GuildId,
    _banned_user: serenity::model::prelude::User,
) {
    todo!()
}

pub async fn guild_ban_removal(
    _self: &Handler,
    _ctx: Context,
    _guild_id: serenity::model::id::GuildId,
    _unbanned_user: serenity::model::prelude::User,
) {
    todo!()
}

pub async fn guild_create(
    _self: &Handler,
    _ctx: Context,
    guild: serenity::model::guild::Guild,
    is_new: Option<bool>,
) {
    // info!("guild data : {:?}", is_new);
    // database::guilds::sync_guilds(guild, is_new).await;
}

pub async fn guild_delete(
    _self: &Handler,
    _ctx: Context,
    _incomplete: serenity::model::guild::UnavailableGuild,
    _full: Option<serenity::model::guild::Guild>,
) {
    todo!()
}

pub async fn guild_member_removal(
    _self: &Handler,
    _ctx: Context,
    _guild_id: serenity::model::id::GuildId,
    _user: serenity::model::prelude::User,
    _member_data_if_available: Option<serenity::model::guild::Member>,
) {
    todo!()
}

pub async fn guild_member_addition(
    _self: &Handler,
    _ctx: Context,
    _new_member: serenity::model::guild::Member,
) {
    todo!()
}

pub async fn guild_member_update(
    _self: &Handler,
    _ctx: Context,
    _old_if_available: Option<serenity::model::guild::Member>,
    _new: serenity::model::guild::Member,
) {
    todo!()
}

pub async fn guild_members_chunk(
    _self: &Handler,
    _ctx: Context,
    _chunk: serenity::model::event::GuildMembersChunkEvent,
) {
    todo!()
}

pub async fn guild_update(
    _self: &Handler,
    _ctx: Context,
    _old_data_if_available: Option<serenity::model::guild::Guild>,
    _new_but_incomplete: serenity::model::guild::PartialGuild,
) {
    todo!()
}
