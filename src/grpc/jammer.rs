use std::sync::Arc;

use serenity::model::prelude::GuildId;
use serenity::prelude::{RwLock, TypeMap};
use songbird::SongbirdKey;
use tonic::{Request, Response, Status};
use tracing::info;

use crate::config::APPLICATION_ID_RELEASE;
use crate::cooldown::CheckResult;

use super::MyJammer;
use super::hello_world::jam_response::JamResponseEnum;
use super::hello_world::jammer_server::Jammer;
use super::hello_world::{JamData, JamResponse};

#[tonic::async_trait]
impl Jammer for MyJammer {
    async fn jam_it(&self, request: Request<JamData>) -> Result<Response<JamResponse>, Status> {
        info!("Got a request from {:?}", request);

        let data = request.into_inner();

        match self
            .data_cache
            .jam_cooldown
            .check_and_record(&self.data_cache.pool, data.guild_id, data.user_id)
            .await
        {
            CheckResult::Allowed => {}
            CheckResult::OnCooldown { remaining_secs } => {
                return Ok(Response::new(JamResponse {
                    resp: JamResponseEnum::Cooldown.into(),
                    cooldown_remaining_seconds: remaining_secs,
                }));
            }
        }

        let guild = match self
            .data_cache
            .cache
            .guild(GuildId::new(data.guild_id.try_into().unwrap()))
        {
            Some(g) => g.to_owned(),
            None => {
                return Ok(Response::new(JamResponse {
                    resp: JamResponseEnum::Unknown.into(),
                    cooldown_remaining_seconds: 0,
                }));
            }
        };

        for (_key, guild_channel) in &guild.channels {
            if guild_channel.kind != serenity::model::prelude::ChannelType::Voice {
                continue;
            }
            let members = guild_channel.members(&self.data_cache.cache).unwrap();
            for member in &members {
                if member.user.id == APPLICATION_ID_RELEASE {
                    info!(
                        "Ladies and gentlemen, We got him in c {}",
                        guild_channel.id.get()
                    );

                    handle_play_audio_to_channel(
                        data.guild_id,
                        &data.clip_name,
                        data.user_id,
                        self.data_cache.data.clone(),
                        self.data_cache.pool.clone(),
                    )
                    .await;

                    return Ok(Response::new(JamResponse {
                        resp: JamResponseEnum::Ok.into(),
                        cooldown_remaining_seconds: 0,
                    }));
                }
            }
        }

        Ok(Response::new(JamResponse {
            resp: JamResponseEnum::NotPresent.into(),
            cooldown_remaining_seconds: 0,
        }))
    }
}

async fn handle_play_audio_to_channel(
    id: i64,
    clip_name: &str,
    user_id: i64,
    data: Arc<RwLock<TypeMap>>,
    pool: sqlx::Pool<sqlx::Postgres>,
) {
    let manager = {
        let data_guard = data.read().await;
        data_guard.get::<SongbirdKey>().cloned().unwrap()
    };

    let guild_id = GuildId::new(id.try_into().unwrap());
    if let Err(e) =
        crate::commands::voice_controls::play_clip(&pool, &manager, guild_id, clip_name, user_id)
            .await
    {
        tracing::error!("Failed to play clip from grpc: {}", e);
    }
}
