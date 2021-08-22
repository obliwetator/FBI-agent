use crate::database::get_conn_from_ctx;
use mysql_async::prelude::Queryable;
use serenity::client::Context;

pub async fn update_voice_channel_user_limit() {}

pub async fn update_voice_channel_user_bitrate() {}

pub async fn add_voice_state() {}
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct UserBossMusic {
    song_name: Option<String>,
}

pub async fn get_user_boss_music(ctx: &Context, user_id: u64) -> Option<String> {
    let mut conn = get_conn_from_ctx(ctx).await;

    let sql = format!(
        "SELECT song_name FROM guild_user_boss_music WHERE user_id = '{}'",
        user_id
    );

    let result: Option<String> = match conn.query_first(sql).await {
        Ok(result) => result,
        Err(_) => None,
    };

    result
}

pub async fn add_user_boss_music(ctx: &Context, user_id: &u64, file_name: &str) {
    let mut conn = get_conn_from_ctx(ctx).await;

    let sql = format!("INSERT IGNORE INTO guild_user_boss_music (user_id, song_name) VALUES ('{}', '{}.ogg') ON DUPLICATE KEY UPDATE song_name = '{}.ogg'", user_id, file_name, file_name);

    match conn.query_drop(sql).await {
        Ok(_) => {}
        Err(why) => {
            println!("Error with add_user_boss_music query: {}", why)
        }
    }
}
