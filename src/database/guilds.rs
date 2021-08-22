use mysql_async::prelude::*;
use mysql_async::Pool;
use serenity::client::Context;
use serenity::model::guild::Guild;
use serenity::model::id::GuildId;
use std::collections::HashSet;

use crate::database::get_conn_from_pool;
use crate::helpers::db_helper;

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct DBGuild {
    id: u64,
    name: String,
    owner_id: Option<u64>,
    icon: Option<String>,
    bot_active: i8,
}

pub async fn add_guilds(pool: &Pool, guilds_to_add: Vec<Guild>) {
    let mut conn = get_conn_from_pool(pool).await;
    let mut sql = String::from("INSERT INTO guilds (id, name, icon, owner_id) VALUES ");

    for guild in guilds_to_add {
        sql.push_str(
            format!(
                "('{}', '{}', {}, '{}'),",
                guild.id,
                guild.name,
                match guild.icon {
                    Some(icon) => {
                        format!("'{}'", icon)
                    }
                    None => {
                        "NULL".to_string()
                    }
                },
                guild.owner_id.0
            )
            .as_str(),
        );
    }

    sql = format!(
        "{}{}",
        sql.split_at(sql.len() - 1).0,
        " ON DUPLICATE KEY UPDATE owner_id=VALUES(owner_id), bot_active = 1;"
    );

    match conn.query_drop(sql).await {
        Ok(_) => {}
        Err(err) => {
            panic!("error sql query add_guilds: {}", err)
        }
    };
}

pub async fn add_guild() {}

pub async fn remove_guild_members_from_guild() {}

pub async fn remove_guild_member_from_guild() {}

pub async fn change_guild_member_nickname() {}
/// Returns a HashSet with the the ID's of the guilds present in the DB
pub async fn get_guilds(pool: &Pool, guilds: Vec<GuildId>) -> HashSet<u64> {
    let mut conn = get_conn_from_pool(pool).await;

    let mut sql = String::from("SELECT id FROM guilds WHERE id IN (");

    for guild in guilds.iter() {
        sql.push_str(format!("{},", guild).as_str());
    }

    sql = format!("{}{}", sql.split_at(sql.len() - 1).0, ");");

    let mut db_guilds = conn.query_iter(sql).await.unwrap();

    let mut guild_set: HashSet<u64> = HashSet::new();

    db_guilds
        .for_each(|mut guild| {
            let id = guild.take::<u64, usize>(0).unwrap();
            guild_set.insert(id);
        })
        .await
        .unwrap();

    guild_set
}

/**
Gets **ALL** guilds
*/
pub async fn get_all_guilds(pool: &Pool) {
    let _conn = get_conn_from_pool(pool);

    let _sql = String::from("SELECT id FROM guilds");

    // let a= pool.acquire().await.unwrap().fetch_all("SELECT * FROM guilds WHERE id IN ('85342800492634112')").await.unwrap();
    // // let result = sqlx::query_as!(DBGuild, "", [85342800492634112, 81384788765712384]).fetch_all(pool).await.unwrap();
}

/**
Check the cache againt our db to see if the guilds are in sync

This runs once when the bot is started
 */
pub async fn sync_guilds(ctx: &Context, guilds: Vec<serenity::model::id::GuildId>) {
    let pool = db_helper::get_pool_from_ctx(ctx).await;
    let mut guilds = get_guilds(&pool, guilds).await;
    // We compare the current guilds from the gateway betwwen the DB. Add any guilds we don't have
    let mut guilds_to_add: Vec<Guild> = Vec::new();

    for discord_guild in ctx.cache.guilds().await {
        if !guilds.contains(discord_guild.as_u64()) {
            if let Some(guild) = ctx.cache.guild(discord_guild).await {
                guilds_to_add.push(guild);
            } else {
                println!(
                    "guild with id {} was not found in the cache",
                    discord_guild.0
                );
            }
        } else {
            // Guild in db do nothing
        }
        guilds.remove(discord_guild.as_u64());
    }
    // add the guilds to the DB
    if !guilds_to_add.is_empty() {
        add_guilds(&pool, guilds_to_add).await;
    }
    // TOOD:
    if !guilds.is_empty() {
        println!("TODO: \"remove\" guilds that the bot is not present in")
    }
    println!("To remove: {:#?}", guilds);
}
