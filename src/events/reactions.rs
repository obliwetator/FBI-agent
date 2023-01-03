use serenity::client::Context;

pub async fn reaction_add(_ctx: Context, _add_reaction: serenity::model::channel::Reaction) {
    todo!()
}

pub async fn reaction_remove(_ctx: Context, _removed_reaction: serenity::model::channel::Reaction) {
    todo!()
}

pub async fn reaction_remove_all(
    _ctx: Context,
    _channel_id: serenity::model::id::ChannelId,
    _removed_from_message_id: serenity::model::id::MessageId,
) {
    todo!()
}
