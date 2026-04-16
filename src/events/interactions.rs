use serenity::{
    all::{CommandDataOptionValue, CommandInteraction, Interaction},
    builder::{
        AutocompleteChoice, CreateAutocompleteResponse, CreateInteractionResponse,
        CreateInteractionResponseMessage,
    },
    client::Context,
};
use tracing::{info, warn};

use crate::event_handler::Handler;

use super::voice_receiver::RECORDING_FILE_PATH;

pub async fn interaction_create(_self: &Handler, ctx: Context, interaction: Interaction) {
    match interaction {
        Interaction::Ping(_) => {
            warn!("Unhandled interaction type: Ping");
        }
        Interaction::Command(application_command) => {
            let content = match application_command.data.name.as_str() {
                "help" => ":(".to_string(),
                "play" => handle_play_audio(&application_command, &ctx, "").await,
                "jam" => handle_jam(&application_command, &ctx, &_self.database).await,
                "queue" => handle_queue(&application_command, &ctx).await,
                "skip" => handle_skip(&application_command, &ctx).await,
                "stop" => handle_stop(&application_command, &ctx).await,
                "stamp" => {
                    crate::commands::stamp::handle_stamp(
                        &application_command,
                        &ctx,
                        &_self.database,
                    )
                    .await
                }
                _ => format!(
                    "Unknown application_command with the name {}",
                    application_command.data.name
                ),
            };

            if let Err(why) = application_command
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content(content)
                            .ephemeral(true),
                    ),
                )
                .await
            {
                info!("Cannot respond to slash command: {}", why);
            }
        }
        Interaction::Component(component) => {
            warn!(
                "Unhandled interaction type: Component (id={})",
                component.data.custom_id
            );
        }
        Interaction::Autocomplete(autocomplete) => {
            if autocomplete.data.name == "jam" {
                let focused_value = autocomplete
                    .data
                    .autocomplete()
                    .map(|a| a.value)
                    .unwrap_or("");

                let guild_id = autocomplete.guild_id.map(|id| id.get() as i64).unwrap_or(0);
                info!(
                    "Autocomplete request for 'jam' - guild_id: {}, focused_value: '{}'",
                    guild_id, focused_value
                );

                let choices = get_clip_choices(focused_value, &_self.database, guild_id).await;
                info!("Autocomplete returned {} choices", choices.len());

                if let Err(why) = autocomplete
                    .create_response(
                        &ctx.http,
                        CreateInteractionResponse::Autocomplete(
                            CreateAutocompleteResponse::new().set_choices(choices),
                        ),
                    )
                    .await
                {
                    warn!("Cannot respond to autocomplete: {}", why);
                }
            } else {
                warn!(
                    "Unhandled interaction type: Autocomplete (name={})",
                    autocomplete.data.name
                );
            }
        }
        Interaction::Modal(modal) => {
            warn!(
                "Unhandled interaction type: Modal (id={})",
                modal.data.custom_id
            );
        }
        _ => {
            warn!("Unhandled unknown interaction type");
        }
    }
}

/// Read the database and return up to 25 choices matching `query`.
async fn get_clip_choices(
    query: &str,
    pool: &sqlx::Pool<sqlx::Postgres>,
    guild_id: i64,
) -> Vec<AutocompleteChoice> {
    let query_wildcard = format!("%{}%", query);
    info!(
        "Querying clips - guild_id: {}, query: {}",
        guild_id, query_wildcard
    );

    let rows = sqlx::query!(
        "SELECT name, clip_id FROM clips WHERE guild_id = $1 AND name ILIKE $2 LIMIT 25",
        guild_id,
        query_wildcard
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let mut choices = Vec::new();
    for row in rows {
        if let (Some(name), clip_id) = (row.name, row.clip_id) {
            choices.push(AutocompleteChoice::new(name, clip_id));
        }
    }

    choices
}

async fn handle_jam(
    application_command: &CommandInteraction,
    ctx: &Context,
    pool: &sqlx::Pool<sqlx::Postgres>,
) -> String {
    info!("Handling jam command: {:?}", application_command.data);

    let clip_name = application_command.data.options.first().and_then(|o| {
        if let CommandDataOptionValue::String(s) = &o.value {
            Some(s.clone())
        } else {
            None
        }
    });

    let clip_name = match clip_name {
        Some(f) => f,
        None => {
            warn!("Jam command missing clip name");
            return "Please provide a clip name.".to_string();
        }
    };

    info!("Extracted clip name: {}", clip_name);

    let manager = match songbird::get(ctx).await {
        Some(m) => m,
        None => {
            warn!("Songbird manager not found");
            return "Voice system is not configured.".to_string();
        }
    };

    let guild_id = match application_command.guild_id {
        Some(id) => id,
        None => {
            warn!("Command not in a server");
            return "This command can only be used in a server.".to_string();
        }
    };

    info!(
        "Attempting to play clip {} in guild {}",
        clip_name, guild_id
    );

    match crate::commands::voice_controls::play_clip(pool, &manager, guild_id, &clip_name).await {
        Ok(msg) => {
            info!("Successfully played clip: {}", msg);
            msg
        }
        Err(e) => {
            warn!("Failed to play clip: {}", e);
            e
        }
    }
}

async fn handle_queue(application_command: &CommandInteraction, ctx: &Context) -> String {
    let manager = match songbird::get(ctx).await {
        Some(m) => m,
        None => return "Voice system is not configured.".to_string(),
    };

    let guild_id = match application_command.guild_id {
        Some(id) => id,
        None => return "This command can only be used in a server.".to_string(),
    };

    crate::commands::voice_controls::queue(&manager, guild_id).await
}

async fn handle_skip(application_command: &CommandInteraction, ctx: &Context) -> String {
    let manager = match songbird::get(ctx).await {
        Some(m) => m,
        None => return "Voice system is not configured.".to_string(),
    };

    let guild_id = match application_command.guild_id {
        Some(id) => id,
        None => return "This command can only be used in a server.".to_string(),
    };

    crate::commands::voice_controls::skip(&manager, guild_id).await
}

async fn handle_stop(application_command: &CommandInteraction, ctx: &Context) -> String {
    let manager = match songbird::get(ctx).await {
        Some(m) => m,
        None => return "Voice system is not configured.".to_string(),
    };

    let guild_id = match application_command.guild_id {
        Some(id) => id,
        None => return "This command can only be used in a server.".to_string(),
    };

    crate::commands::voice_controls::stop(&manager, guild_id).await
}

pub async fn handle_play_audio(
    application_command: &CommandInteraction,
    ctx: &Context,
    file_name: &str,
) -> String {
    let manager = match songbird::get(ctx).await {
        Some(m) => m,
        None => return "Voice system is not configured.".to_string(),
    };

    let guild_id = match application_command.guild_id {
        Some(id) => id,
        None => return "This command can only be used in a server.".to_string(),
    };

    // TODO: This does not return and error if the wrong file path is given?
    let result = songbird::input::File::new(format!(
        "{}{}",
        RECORDING_FILE_PATH,
        "/362257054829641758/2022/December/1670620473631-161172393719496704-QazZ.ogg"
    ));

    let input = songbird::input::Input::from(result);

    let handler = match manager.get(guild_id) {
        Some(h) => h,
        None => return "I am not currently in a voice channel.".to_string(),
    };

    let handler_lock = handler.lock().await.enqueue(input.into()).await;
    let _ = handler_lock.set_volume(0.5);

    "jamming".to_string()
}
