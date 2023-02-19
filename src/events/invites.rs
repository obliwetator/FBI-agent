use serenity::client::Context;

use crate::Handler;

pub async fn invite_create(
    _self: &Handler,
    _ctx: Context,
    _data: serenity::model::event::InviteCreateEvent,
) {
    todo!()
}

pub async fn invite_delete(
    _self: &Handler,
    _ctx: Context,
    _data: serenity::model::event::InviteDeleteEvent,
) {
    todo!()
}
