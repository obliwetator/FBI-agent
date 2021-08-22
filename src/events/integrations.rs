use serenity::client::Context;

pub async fn guild_integrations_update(_ctx: Context, _guild_id: serenity::model::id::GuildId) {
    todo!()
}

pub async fn integration_create(_ctx: Context, _integration: serenity::model::guild::Integration) {
    todo!()
}

pub async fn integration_update(_ctx: Context, _integration: serenity::model::guild::Integration) {
    todo!()
}

pub async fn integration_delete(
    _ctx: Context,
    _integration_id: serenity::model::id::IntegrationId,
    _guild_id: serenity::model::id::GuildId,
    _application_id: Option<serenity::model::id::ApplicationId>,
) {
    todo!()
}
