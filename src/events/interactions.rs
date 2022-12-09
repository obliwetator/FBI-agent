use serenity::{
    model::prelude::interaction::{Interaction, InteractionResponseType},
    prelude::*,
};

pub async fn interaction_create(ctx: Context, interaction: Interaction) {
    match interaction {
        Interaction::Ping(_) => todo!(),
        Interaction::ApplicationCommand(application_command) => {
            let content = match application_command.data.name.as_str() {
                "help" => ":(".to_string(),
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

// pub async fn application_command_create(_ctx: Context, _application_command: ApplicationCommand) {
//     todo!()
// }

// pub async fn application_command_update(_ctx: Context, _application_command: ApplicationCommand) {
//     todo!()
// }

// pub async fn application_command_delete(_ctx: Context, _application_command: ApplicationCommand) {
//     todo!()
// }
