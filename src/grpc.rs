use std::convert::TryInto;
use std::sync::Arc;

use serenity::model::prelude::GuildId;
use tonic::{Request, Response, Status};

use hello_world::jammer_server::Jammer;

use tracing::info;

use crate::Custom;

use serenity::prelude::{RwLock, TypeMap};
use songbird::SongbirdKey;

use crate::events::voice_receiver::CLIPS_FILE_PATH;

use self::hello_world::jam_response::JamResponseEnum;
use self::hello_world::{JamData, JamResponse};

pub mod hello_world {
    tonic::include_proto!("helloworld");
}

pub struct MyJammer {
    data_cache: Custom,
}

impl MyJammer {
    pub fn new(data_cache: Custom) -> Self {
        Self { data_cache }
    }
}

#[tonic::async_trait]
impl Jammer for MyJammer {
    async fn jam_it(&self, request: Request<JamData>) -> Result<Response<JamResponse>, Status> {
        info!("Got a request from {:?}", request);

        let data = request.into_inner();

        let guild = match self
            .data_cache
            .cache
            .guild(GuildId::new(data.guild_id.try_into().unwrap()))
        {
            Some(ok) => ok.to_owned(),
            None => {
                let reply = JamResponse {
                    resp: JamResponseEnum::Unkown.into(),
                };
                return Ok(Response::new(reply));
                // return (StatusCode::OK, Json(json!({ "code": Ras::Unkown })));
            }
        };

        let channels = &guild.channels;

        for (key, guild_channel) in channels {
            match guild_channel.kind {
                serenity::model::prelude::ChannelType::Text => {}
                // serenity::model::prelude::ChannelType::Private => todo!(),
                serenity::model::prelude::ChannelType::Voice => {
                    let res = guild_channel.members(&self.data_cache.cache).unwrap();
                    for (index, member) in res.iter().enumerate() {
                        if member.user.id == 877617434029350972 {
                            info!(
                                "Ladies and gentlemen, We got him in c {}",
                                guild_channel.id.get()
                            );

                            handle_play_audio_to_channel(
                                data.guild_id,
                                &data.clip_name,
                                self.data_cache.data.clone(),
                            )
                            .await;

                            let reply = JamResponse {
                                resp: JamResponseEnum::Ok.into(),
                            };
                            return Ok(Response::new(reply));
                        }
                    }
                }
                _ => {}
            }
        }

        let reply = JamResponse {
            resp: JamResponseEnum::NotPressent.into(),
        };
        return Ok(Response::new(reply));
    }
}

async fn handle_play_audio_to_channel(id: i64, clip_name: &str, data: Arc<RwLock<TypeMap>>) {
    let data = data.read().await;
    let manager = data.get::<SongbirdKey>().cloned().unwrap();

    // TODO: This does not return and error if the wrong file path is given?
    info!(" Clips to play : {}/{}.ogg", CLIPS_FILE_PATH, clip_name);
    let result = songbird::input::File::new(format!("{}/{}.ogg", CLIPS_FILE_PATH, clip_name));

    let input = songbird::input::Input::from(result);
    let handler = manager.get(GuildId::new(id.try_into().unwrap())).unwrap();
    let driver = &mut handler.lock().await;
    let handler_lock = driver.enqueue_input(input);
    let _ = handler_lock.await.set_volume(0.5);
}
