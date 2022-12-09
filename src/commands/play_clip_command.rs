use serenity::builder;
use serenity::model::prelude::command::CommandOptionType;

pub fn register(
    command: &mut builder::CreateApplicationCommand,
) -> &mut builder::CreateApplicationCommand {
    // command
    //     .name("numberinput")
    //     .description("Test command for number input")
    //     .create_option(|option| {
    //         option
    //             .name("int")
    //             .description("An integer from 5 to 10")
    //             .kind(CommandOptionType::Integer)
    //             .min_int_value(5)
    //             .max_int_value(10)
    //             .required(true)
    //     })
    //     .create_option(|option| {
    //         option
    //             .name("number")
    //             .description("A float from -3.3 to 234.5")
    //             .kind(CommandOptionType::Number)
    //             .min_number_value(-3.3)
    //             .max_number_value(234.5)
    //             .required(true)
    //     })
    command
        .name("play")
        .description("Play a clip")
        .create_option(|option| {
            option
                .name("clip")
                .description("description")
                .kind(CommandOptionType::SubCommandGroup)
                .create_sub_option(|f| {
                    f.name("link")
                        .description("play a clip from a link")
                        .kind(CommandOptionType::SubCommand)
                        .create_sub_option(|f| {
                            f.name("link")
                                .description("link")
                                .kind(CommandOptionType::String)
                                .required(true)
                        })
                })
                .create_sub_option(|f| {
                    f.name("attachment")
                        .description("play a clip from an attachment")
                        .kind(CommandOptionType::SubCommand)
                        .create_sub_option(|f| {
                            f.name("attachment")
                                .description("attachment")
                                .kind(CommandOptionType::String)
                                .required(true)
                        })
                })
        })
    // .create_option(|option| {
    //     option
    //         .name("Link")
    //         .description("Link")
    //         .kind(CommandOptionType::String)
    //         .required(true)
    // })
}
