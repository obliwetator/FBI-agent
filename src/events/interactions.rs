use serenity::{
    model::interactions::{
        application_command::ApplicationCommand, Interaction, InteractionResponseType,
    },
    prelude::*,
};

pub async fn interaction_create(ctx: Context, interaction: Interaction) {
    if let Interaction::ApplicationCommand(command) = interaction {
        let content = match command.data.name.as_str() {
            "help" => ":(".to_string(),
            _ => format!("Unkown command with the name {}", command.data.name),
        };

        if let Err(why) = command
            .create_interaction_response(&ctx.http, |response| {
                response
                    .kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|message| message.content(content))
            })
            .await
        {
            println!("Cannot respond to slash command: {}", why);
        }
    } else if let Interaction::MessageComponent(command) = interaction {
    } else if let Interaction::Ping(command) = interaction {
    }
}

pub async fn application_command_create(_ctx: Context, _application_command: ApplicationCommand) {
    todo!()
}

pub async fn application_command_update(_ctx: Context, _application_command: ApplicationCommand) {
    todo!()
}

pub async fn application_command_delete(_ctx: Context, _application_command: ApplicationCommand) {
    todo!()
}
