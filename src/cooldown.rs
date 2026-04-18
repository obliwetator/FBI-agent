use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use sqlx::{Pool, Postgres};
use tracing::warn;

#[derive(Clone)]
pub struct JamCooldown {
    last: Arc<DashMap<(i64, i64), Instant>>,
}

pub enum CheckResult {
    Allowed,
    OnCooldown { remaining_secs: u32 },
}

impl JamCooldown {
    pub fn new() -> Self {
        Self {
            last: Arc::new(DashMap::new()),
        }
    }

    pub async fn check_and_record(
        &self,
        pool: &Pool<Postgres>,
        guild_id: i64,
        user_id: i64,
    ) -> CheckResult {
        let cooldown_secs = match resolve_cooldown(pool, guild_id, user_id).await {
            Ok(secs) => secs,
            Err(e) => {
                warn!("Cooldown lookup failed (guild={}, user={}): {}. Allowing.", guild_id, user_id, e);
                0
            }
        };

        if cooldown_secs == 0 {
            self.last.insert((guild_id, user_id), Instant::now());
            return CheckResult::Allowed;
        }

        let now = Instant::now();
        let cooldown = Duration::from_secs(cooldown_secs as u64);

        if let Some(prev) = self.last.get(&(guild_id, user_id)) {
            let elapsed = now.duration_since(*prev);
            if elapsed < cooldown {
                let remaining = cooldown - elapsed;
                let remaining_secs = remaining.as_secs() as u32 + 1;
                return CheckResult::OnCooldown { remaining_secs };
            }
        }

        self.last.insert((guild_id, user_id), now);
        CheckResult::Allowed
    }
}

async fn resolve_cooldown(
    pool: &Pool<Postgres>,
    guild_id: i64,
    user_id: i64,
) -> Result<i32, sqlx::Error> {
    let override_row = sqlx::query!(
        "SELECT cooldown_seconds FROM user_jam_cooldown_overrides WHERE guild_id = $1 AND user_id = $2",
        guild_id,
        user_id
    )
    .fetch_optional(pool)
    .await?;

    if let Some(row) = override_row {
        return Ok(row.cooldown_seconds);
    }

    let guild_row = sqlx::query!(
        "SELECT cooldown_seconds FROM guild_jam_cooldowns WHERE guild_id = $1",
        guild_id
    )
    .fetch_optional(pool)
    .await?;

    Ok(guild_row.map(|r| r.cooldown_seconds).unwrap_or(0))
}
