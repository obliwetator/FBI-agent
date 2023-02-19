use serenity::{
    model::prelude::interaction::{
        application_command::ApplicationCommandInteraction, Interaction, InteractionResponseType,
    },
    prelude::*,
};
use songbird::input::Restartable;

use crate::Handler;

use super::voice::RECORDING_FILE_PATH;

pub async fn interaction_create(_self: &Handler, ctx: Context, interaction: Interaction) {
    match interaction {
        Interaction::Ping(_) => todo!(),
        Interaction::ApplicationCommand(application_command) => {
            let content = match application_command.data.name.as_str() {
                "help" => ":(".to_string(),
                "play" => handle_play_audio(&application_command, &ctx, "").await,
                _ => format!(
                    "Unkown application_command with the name {}",
                    application_command.data.name
                ),
            };

            if let Err(why) = application_command
                .create_interaction_response(&ctx.http, |response| {
                    response
                        .kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|message| message.content(content))
                })
                .await
            {
                println!("Cannot respond to slash command: {}", why);
            }
        }
        Interaction::MessageComponent(_) => todo!(),
        Interaction::Autocomplete(_) => todo!(),
        Interaction::ModalSubmit(_) => todo!(),
    }
}

pub async fn handle_play_audio(
    application_command: &ApplicationCommandInteraction,
    ctx: &Context,
    file_name: &str,
) -> String {
    let manager = songbird::get(ctx).await.unwrap();
    // let result =songbird::ffmpeg(format!("{}{}", RECORDING_FILE_PATH, "/projects/FBI-agent/voice_recordings/362257054829641758/2022/December/1670620473631-161172393719496704-QazZ.ogg")).await.unwrap();

    // TODO: This does not return and error if the wrong file path is given?
    let result = Restartable::ffmpeg(
        format!(
            "{}{}",
            RECORDING_FILE_PATH,
            "/362257054829641758/2022/December/1670620473631-161172393719496704-QazZ.ogg"
        ),
        false,
    )
    .await
    .unwrap();

    let input = songbird::input::Input::from(result);
    let handler = manager.get(application_command.guild_id.unwrap()).unwrap();
    let handler_lock = handler.lock().await.enqueue_source(input);
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
