use serenity::{
    all::{CommandInteraction, Interaction},
    builder::{CreateInteractionResponse, CreateInteractionResponseMessage},
    client::Context,
};
use tracing::info;

use crate::event_handler::Handler;

use super::voice_receiver::RECORDING_FILE_PATH;

pub async fn interaction_create(_self: &Handler, ctx: Context, interaction: Interaction) {
    match interaction {
        Interaction::Ping(_) => todo!(),
        Interaction::Command(application_command) => {
            let content = match application_command.data.name.as_str() {
                "help" => ":(".to_string(),
                "play" => handle_play_audio(&application_command, &ctx, "").await,
                _ => format!(
                    "Unknown application_command with the name {}",
                    application_command.data.name
                ),
            };

            if let Err(why) = application_command
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new().content(content),
                    ),
                )
                .await
            {
                info!("Cannot respond to slash command: {}", why);
            }
        }
        Interaction::Component(_) => todo!(),
        Interaction::Autocomplete(_) => todo!(),
        Interaction::Modal(_) => todo!(),
        _ => todo!(),
    }
}

pub async fn handle_play_audio(
    application_command: &CommandInteraction,
    ctx: &Context,
    file_name: &str,
) -> String {
    let manager = songbird::get(ctx).await.unwrap();
    // TODO: This does not return and error if the wrong file path is given?
    let result = songbird::input::File::new(format!(
        "{}{}",
        RECORDING_FILE_PATH,
        "/362257054829641758/2022/December/1670620473631-161172393719496704-QazZ.ogg"
    ));

    let input = songbird::input::Input::from(result);
    let handler = manager.get(application_command.guild_id.unwrap()).unwrap();
    let handler_lock = handler.lock().await.enqueue(input.into()).await;
    let _ = handler_lock.set_volume(0.5);

    "jamming".to_string()
}

// pub async fn application_command_create(_ctx: Context, _application_command: ApplicationCommand) {
//     todo!()
// }

// pub async fn application_command_update(_ctx: Context, _application_command: ApplicationCommand) {
//     todo!()
// }

// pub async fn application_command_delete(_ctx: Context, _application_command: ApplicationCommand) {
//     todo!()
// }
