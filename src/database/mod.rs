pub mod channels;

use crate::event_handler::Handler;
use serenity::{
    all::UnavailableGuild,
    model::prelude::{Guild, GuildId},
    prelude::Context,
};
use sqlx::Postgres;

const BIND_LIMIT: usize = 65535;

pub(crate) async fn update_info(handler: &Handler, ctx: &Context, guilds: &Vec<GuildId>) {
    let guild_cached = &guilds
        .iter()
        .map(|guild| {
            let x = guild.to_guild_cached(&ctx).unwrap().to_owned();
            x
        })
        .collect();

    update_guilds(guild_cached, handler).await;
    update_roles(guild_cached, handler).await;

    // TODO: Remove roles that are not present
    // TODO: Remove roles that are not present while the bot is running
    update_user_roles(guild_cached, handler).await;
    update_channels(guild_cached, handler).await;
    update_permissions(guild_cached, handler).await;
}

async fn update_roles(guild_cached: &Vec<Guild>, handler: &Handler) {
    for guild in guild_cached {
        let mut query_builder: sqlx::QueryBuilder<Postgres> =
            sqlx::QueryBuilder::new("INSERT INTO roles (guild_id, role_id, permission, name) ");
        query_builder
            .push_values(
                (&guild.roles).into_iter().take(BIND_LIMIT / 4),
                |mut b, role| {
                    b.push_bind(role.1.guild_id.get() as i64)
                        .push_bind(role.0.get() as i64)
                        .push_bind(role.1.permissions.bits() as i64)
                        .push_bind(&role.1.name);
                },
            )
            .push(" ON CONFLICT (role_id) DO UPDATE SET permission=EXCLUDED.permission");

        let query = query_builder.build();

        let res = query.execute(&handler.database).await.unwrap();
    }
}

async fn update_user_roles(guild_cached: &Vec<Guild>, handler: &Handler) {
    for guild in guild_cached {
        for (user_id, user) in guild.members.iter() {
            let perm = &user.roles;
            if perm.len() > 0 {
                let mut query_builder: sqlx::QueryBuilder<Postgres> =
                    sqlx::QueryBuilder::new("INSERT INTO user_roles (user_id, role_id) ");

                query_builder
                    .push_values(perm.into_iter().take(BIND_LIMIT / 4), |mut b, role| {
                        b.push_bind(user_id.get() as i64)
                            .push_bind(role.get() as i64);
                    })
                    .push(" ON CONFLICT (user_id, role_id) DO UPDATE SET role_id=EXCLUDED.role_id");

                let query = query_builder.build();
                let res = query.execute(&handler.database).await.unwrap();
            }
        }
    }
}

async fn update_permissions(guild_cached: &Vec<Guild>, handler: &Handler) {
    for guild in guild_cached {
        let ch = &guild.channels;

        for (ch_id, channel) in ch {
            let mut query_builder: sqlx::QueryBuilder<Postgres> = sqlx::QueryBuilder::new(
                "INSERT INTO channel_permissions (channel_id, target_id, kind, allow, deny) ",
            );

            let perm = &channel.permission_overwrites;
            if perm.len() > 0 {
                query_builder
                        .push_values(perm.into_iter().take(BIND_LIMIT / 5), |mut b, p| {
                            let kind = {
                                match p.kind {
                                    serenity::model::prelude::PermissionOverwriteType::Member(
                                        channel,
                                    ) => ("user", channel.get() as i64),
                                    serenity::model::prelude::PermissionOverwriteType::Role(channel) => {
                                        ("role", channel.get() as i64)
                                    }
                                    _ => {
                                        panic!("this should not happen")
                                    }
                                }
                            };


                            b.push_bind(channel.id.get() as i64)
                                .push_bind(kind.1)
                                .push_bind(kind.0)
                                .push_bind(p.allow.bits() as i64)
                                .push_bind(p.deny.bits() as i64);
                        })
                        .push(
                            " ON CONFLICT (channel_id, target_id) DO UPDATE SET allow = EXCLUDED.allow, deny = EXCLUDED.deny",
                        );

                let query = query_builder.build();

                let res = query.execute(&handler.database).await.unwrap();
            }
        }

        // let res = query.execute(&handler.database).await.unwrap();
    }
}

async fn update_guilds(guild_cached: &Vec<Guild>, handler: &Handler) {
    let mut query_builder: sqlx::QueryBuilder<Postgres> =
        sqlx::QueryBuilder::new("INSERT INTO guilds (id, owner_id) ");

    query_builder
        .push_values(
            guild_cached.into_iter().take(BIND_LIMIT / 2),
            |mut b, guild| {
                b.push_bind(guild.id.get() as i64)
                    .push_bind(guild.owner_id.get() as i64);
            },
        )
        .push(" ON CONFLICT DO NOTHING ");

    let query = query_builder.build();

    let res = query.execute(&handler.database).await.unwrap();
}

async fn update_channels(guild_cached: &Vec<Guild>, handler: &Handler) {
    // "INSERT INTO channel_permissions (channel_id, target_id, kind, allow, deny) ",

    for guild in guild_cached {
        let mut query_builder: sqlx::QueryBuilder<Postgres> =
            sqlx::QueryBuilder::new("INSERT INTO channels (channel_id, guild_id, type, name) ");
        let ch = &guild.channels;

        query_builder
            .push_values(ch.into_iter().take(BIND_LIMIT / 4), |mut b, channel| {
                b.push_bind(channel.1.id.get() as i64)
                    .push_bind(channel.1.guild_id.get() as i64)
                    .push_bind(u8::from(channel.1.kind) as i32)
                    .push_bind(channel.1.name());
            })
            .push(" ON CONFLICT (channel_id) DO UPDATE SET name=EXCLUDED.name ");

        let query = query_builder.build();

        let res = query.execute(&handler.database).await.unwrap();
    }
}

pub async fn update_guild_present(guilds: Vec<UnavailableGuild>, handler: &Handler) {
    // TODO. We only check the guild we are currently in. Check if the bot has left/kicked from any guild.
    let mut query_builder: sqlx::QueryBuilder<Postgres> =
        sqlx::QueryBuilder::new("INSERT INTO guilds_present (guild_id) ");

    query_builder
        .push_values(guilds.into_iter().take(BIND_LIMIT), |mut b, guild| {
            let value = guild.id.get();

            b.push_bind(guild.id.get() as i64);
        })
        .push(" ON CONFLICT (guild_id) DO NOTHING");

    let query = query_builder.build();

    let res = query.execute(&handler.database).await.unwrap();
}
