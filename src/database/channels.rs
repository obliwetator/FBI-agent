use serenity::{model::prelude::GuildId, prelude::Context};

use crate::event_handler::Handler;

pub(crate) async fn update_guild_channels(
    _handler: &Handler,
    _ctx: &Context,
    _guilds: &Vec<GuildId>,
) {
    // todo!()
}

pub(crate) async fn update_guilds(_handler: &Handler, _ctx: &Context, _guilds: &Vec<GuildId>) {}
