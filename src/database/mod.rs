pub mod channels;

use crate::Handler;
use serenity::{
    model::prelude::{Channel, Guild, GuildId},
    prelude::Context,
};
use sqlx::Postgres;

const BIND_LIMIT: usize = 65535;

pub(crate) async fn update_info(handler: &Handler, ctx: &Context, guilds: &Vec<GuildId>) {
    let guild_cached = &guilds
        .iter()
        .map(|guild| {
            let x = guild.to_guild_cached(&ctx).unwrap();
            x
        })
        .collect();

    update_guilds(guild_cached, handler).await;
    update_roles(guild_cached, handler).await;
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
                    b.push_bind(role.1.guild_id.0 as i64)
                        .push_bind(role.0 .0 as i64)
                        .push_bind(role.1.permissions.bits() as i64)
                        .push_bind(&role.1.name);
                },
            )
            .push(" ON CONFLICT (role_id) DO UPDATE SET permission=EXCLUDED.permission");

        let query = query_builder.build();

        let res = query.execute(&handler.database).await.unwrap();
    }
}

async fn update_permissions(guild_cached: &Vec<Guild>, handler: &Handler) {
    for guild in guild_cached {
        let ch = &guild.channels;

        for (ch_id, channel) in ch {
            let mut query_builder: sqlx::QueryBuilder<Postgres> = sqlx::QueryBuilder::new(
                "INSERT INTO channel_permissions (channel_id, target_id, kind, allow, deny) ",
            );

            match channel {
                Channel::Guild(ok) => {
                    let perm = &ok.permission_overwrites;
                    if perm.len() > 0 {
                        query_builder
                        .push_values(perm.into_iter().take(BIND_LIMIT / 5), |mut b, p| {
                            let kind = {
                                match p.kind {
                                    serenity::model::prelude::PermissionOverwriteType::Member(
                                        ok,
                                    ) => ("user", ok.0 as i64),
                                    serenity::model::prelude::PermissionOverwriteType::Role(ok) => {
                                        ("role", ok.0 as i64)
                                    }
                                    _ => {
                                        panic!("this should not happen")
                                    }
                                }
                            };


                            b.push_bind(ok.id.0 as i64)
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
                Channel::Private(_) => {}
                Channel::Category(_) => {}
                _ => {}
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
                b.push_bind(guild.id.0 as i64)
                    .push_bind(guild.owner_id.0 as i64);
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
            .push_values(
                ch.into_iter().take(BIND_LIMIT / 4),
                |mut b, channel| match channel.1 {
                    Channel::Guild(ok) => {
                        b.push_bind(ok.id.0 as i64)
                            .push_bind(ok.guild_id.0 as i64)
                            .push_bind(ok.kind as i32)
                            .push_bind(ok.name());
                    }
                    Channel::Private(ok) => {
                        // ignore private channels
                        // b.push_bind(ok.id.0 as i64).push_bind(ok.guild_id.0 as i64);
                    }
                    Channel::Category(ok) => {
                        b.push_bind(ok.id.0 as i64)
                            .push_bind(ok.guild_id.0 as i64)
                            .push_bind(ok.kind as i32)
                            .push_bind(ok.name());
                    }
                    _ => {}
                },
            )
            .push(" ON CONFLICT (channel_id) DO UPDATE SET name=EXCLUDED.name ");

        let query = query_builder.build();

        let res = query.execute(&handler.database).await.unwrap();
    }
}
