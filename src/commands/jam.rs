use serenity::builder::{CreateCommand, CreateCommandOption};
use serenity::model::prelude::CommandOptionType;

use crate::events::voice_receiver::CLIPS_FILE_PATH;
use serenity::model::prelude::GuildId;
use songbird::Songbird;
use sqlx::{Pool, Postgres};
use tracing::info;

pub async fn play_clip(
    pool: &Pool<Postgres>,
    manager: &std::sync::Arc<Songbird>,
    guild_id: GuildId,
    clip_name: &str,
) -> Result<String, String> {
    let row = sqlx::query!(
        "SELECT saved_file_name FROM clips WHERE guild_id = $1 AND name = $2",
        guild_id.get() as i64,
        clip_name
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("Database error: {}", e))?;

    let saved_file_name = if let Some(record) = row {
        record
            .saved_file_name
            .unwrap_or_else(|| format!("{}.ogg", clip_name))
    } else {
        return Err(format!("Clip '{}' not found in database.", clip_name));
    };

    info!("Clips to play: {}/{}", CLIPS_FILE_PATH, saved_file_name);
    let result = songbird::input::File::new(format!("{}/{}", CLIPS_FILE_PATH, saved_file_name));
    let input = songbird::input::Input::from(result);

    let handler = match manager.get(guild_id) {
        Some(h) => h,
        None => return Err("I am not currently in a voice channel.".to_string()),
    };

    let handler_lock = handler.lock().await.enqueue(input.into()).await;
    let _ = handler_lock.set_volume(0.5);

    Ok(format!("Now jamming: {}", clip_name))
}

pub fn register() -> CreateCommand {
    CreateCommand::new("jam")
        .description("Play a clip in the current voice channel")
        .add_option(
            CreateCommandOption::new(CommandOptionType::String, "clip", "The clip to play")
                .required(true)
                .set_autocomplete(true),
        )
}
