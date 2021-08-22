use mysql_async::Pool;
use serenity::client::Context;

use crate::MysqlConnection;

#[derive(Debug)]
pub struct Channel {
    pub channel_id: i64,
}

pub async fn _get_channels() {}

pub async fn get_pool_from_ctx(ctx: &Context) -> Pool {
    let pool = ctx
        .data
        .read()
        .await
        .get::<MysqlConnection>()
        .cloned()
        .unwrap();

    pool
}
