use std::{env, error::Error};

pub struct DiscordConfig {
    pub token: String,
    pub application_id: u64,
}

pub fn db_url() -> Result<String, env::VarError> {
    env::var("DATABASE_URL")
}

#[cfg(debug_assertions)]
pub fn discord_config() -> Result<DiscordConfig, Box<dyn Error + Send + Sync>> {
    Ok(DiscordConfig {
        token: env::var("DISCORD_TOKEN_DEBUG")?,
        application_id: env::var("APPLICATION_ID_DEBUG")?.parse()?,
    })
}

#[cfg(not(debug_assertions))]
pub fn discord_config() -> Result<DiscordConfig, Box<dyn Error + Send + Sync>> {
    Ok(DiscordConfig {
        token: env::var("DISCORD_TOKEN_RELEASE")?,
        application_id: env::var("APPLICATION_ID_RELEASE")?.parse()?,
    })
}

pub fn application_id_release() -> Result<u64, Box<dyn Error + Send + Sync>> {
    Ok(env::var("APPLICATION_ID_RELEASE")?.parse()?)
}

#[cfg(debug_assertions)]
pub const SERVICE_NAME: &str = "fbi-agent-debug";
#[cfg(not(debug_assertions))]
pub const SERVICE_NAME: &str = "fbi-agent";
