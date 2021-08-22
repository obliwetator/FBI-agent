// #![allow(dead_code, unused_variables)]

use mysql_async::{Conn, Pool};
use serenity::client::Context;

use crate::MysqlConnection;

pub async fn get_conn_from_pool(pool: &Pool) -> Conn {
    let conn = pool.get_conn().await.unwrap();

    conn
}

pub async fn get_conn_from_ctx(ctx: &Context) -> Conn {
    let pool = ctx
        .data
        .read()
        .await
        .get::<MysqlConnection>()
        .cloned()
        .unwrap();
    let conn = pool.get_conn().await.unwrap();

    conn
}

pub mod channels;
pub mod emojis;
pub mod guilds;
pub mod invites;
pub mod messages;
pub mod roles;
pub mod text_channel;
pub mod users;
pub mod voice;
