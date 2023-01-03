use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use serde_json::json;
use serde_repr::{Deserialize_repr, Serialize_repr};
use serenity::prelude::{RwLock, TypeMap};
use songbird::{input::Restartable, SongbirdKey};
use tracing::info;

use crate::{events::voice::CLIPS_FILE_PATH, Custom};
#[derive(Debug, Deserialize)]
pub struct Response {
    clip_name: Option<String>,
    guild_id: Option<u64>,
}

#[derive(Serialize_repr, Deserialize_repr, PartialEq, Debug)]
#[repr(u8)]
pub enum Ras {
    OK,
    NotPresentInChannel,
    Unkown,
}

// #[get("/")]
// pub async fn hello(data: web::Data<Arc<Http>>) -> impl Responder {
//     let a = data.get_ref();

//     ChannelId(850192712124858389).say(a, "hello").await.unwrap();

//     HttpResponse::Ok().body("hello world\n");
// }
// basic handler that responds with a static string
pub async fn root(
    // State(cache): State<Arc<Cache>>,
    // State(http): State<Arc<Http>>,
    State(custom): State<Custom>,
    query: Query<Response>,
) -> impl axum::response::IntoResponse {
    info!("QUERY {:#?}", query);
    if query.clip_name.is_some() && query.guild_id.is_some() {
        // play clip in the channel

        let guild = match custom
            .cache_http
            .cache
            .guild(*query.guild_id.as_ref().unwrap())
        {
            Some(ok) => ok,
            None => {
                return (StatusCode::OK, Json(json!({ "code": Ras::Unkown })));
            }
        };

        let channels = guild.channels;

        for (key, channel) in channels {
            if let serenity::model::prelude::Channel::Guild(guild_channel) = channel {
                match guild_channel.kind {
                    serenity::model::prelude::ChannelType::Text => {}
                    // serenity::model::prelude::ChannelType::Private => todo!(),
                    serenity::model::prelude::ChannelType::Voice => {
                        let res = guild_channel
                            .members(&custom.cache_http.cache)
                            .await
                            .unwrap();
                        for (index, member) in res.iter().enumerate() {
                            info!("mebers: {:#?}", member);
                            if member.user.id == 877617434029350972 {
                                info!(
                                    "Ladies and gentlemen, We got him in c {}",
                                    guild_channel.id.0
                                );

                                handle_play_audio_to_channel(
                                    *query.guild_id.as_ref().unwrap(),
                                    query.clip_name.as_ref().unwrap(),
                                    custom.data.clone(),
                                )
                                .await;
                                let json = json!({
                                    "code": Ras::OK,
                                });

                                return (StatusCode::OK, Json(json));
                            }
                        }

                        // info!("members: {:#?}", res);
                    }
                    _ => {}
                }
            }
        }
        let json = json!({
            "code": 1,
        });

        (
            StatusCode::OK,
            Json(json!({ "code": Ras::NotPresentInChannel })),
        )
    } else {
        (StatusCode::OK, Json(json!({ "code": Ras::Unkown })))
    }
}

async fn handle_play_audio_to_channel(id: u64, clip_name: &str, data: Arc<RwLock<TypeMap>>) {
    let data = data.read().await;
    let manager = data.get::<SongbirdKey>().cloned().unwrap();
    // let result =songbird::ffmpeg(format!("{}{}", RECORDING_FILE_PATH, "/projects/FBI-agent/voice_recordings/362257054829641758/2022/December/1670620473631-161172393719496704-QazZ.ogg")).await.unwrap();

    // TODO: This does not return and error if the wrong file path is given?
    info!(" Clips to play : {}/{}.ogg", CLIPS_FILE_PATH, clip_name);
    let result = Restartable::ffmpeg(format!("{}/{}.ogg", CLIPS_FILE_PATH, clip_name), false)
        .await
        .unwrap();

    let input = songbird::input::Input::from(result);
    let handler = manager.get(id).unwrap();
    let handler_lock = handler.lock().await.enqueue_source(input);
    let _ = handler_lock.set_volume(0.5);
}
