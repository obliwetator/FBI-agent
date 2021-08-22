use std::collections::{HashMap, HashSet};

use serenity::{client::Context, model::guild::Member};

pub async fn sync_users(ctx: &Context) {
    let guild_members = get_guild_members().await;
    let users_to_add: Vec<Member> = Vec::new();

    for discord_guild in ctx.cache.guilds().await {
        for member in discord_guild.to_guild_cached(&ctx).await.unwrap().members {
            if let Some(guild_users) = guild_members.get(discord_guild.as_u64()) {}
        }
    }
}

async fn get_guild_members() -> HashMap<u64, HashSet<u64>> {
    todo!()
}
