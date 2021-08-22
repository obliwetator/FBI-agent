use serenity::client::Context;

pub async fn guild_role_create(
    _ctx: Context,
    _guild_id: serenity::model::id::GuildId,
    _new: serenity::model::guild::Role,
) {
    todo!()
}

pub async fn guild_role_delete(
    _ctx: Context,
    _guild_id: serenity::model::id::GuildId,
    _removed_role_id: serenity::model::id::RoleId,
    _removed_role_data_if_available: Option<serenity::model::guild::Role>,
) {
    todo!()
}

pub async fn guild_role_update(
    _ctx: Context,
    _guild_id: serenity::model::id::GuildId,
    _old_data_if_available: Option<serenity::model::guild::Role>,
    _new: serenity::model::guild::Role,
) {
    todo!()
}
