use serenity::client::Context;

pub async fn guild_emojis_update(
    _ctx: Context,
    _guild_id: serenity::model::id::GuildId,
    _current_state: std::collections::HashMap<
        serenity::model::id::EmojiId,
        serenity::model::guild::Emoji,
    >,
) {
    todo!()
}
