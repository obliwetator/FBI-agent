use serenity::{model::prelude::GuildId, prelude::Context};

use crate::event_handler::Handler;

pub(crate) async fn update_guild_channels(handler: &Handler, ctx: &Context, guilds: &Vec<GuildId>) {
    // todo!()
}

pub(crate) async fn update_guilds(handler: &Handler, ctx: &Context, guilds: &Vec<GuildId>) {}
