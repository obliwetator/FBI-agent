//! Startup zombie reaper for `audio_files`.
//!
//! Recording end is written by the leave/disconnect path in
//! `events/voice_receiver.rs`. If the bot crashed or was killed before that
//! ran, rows stay `end_ts IS NULL` forever and pollute "live" detection in
//! the web UI. By definition nothing can still be live across a process
//! restart, so on startup we close every NULL row.
//!
//! Default mode: row stays, `end_ts = start_ts`, `reaped = TRUE`. Files on
//! disk are left alone — they may still be partially playable. Audit them
//! with:
//!
//! ```sql
//! SELECT * FROM audio_files WHERE reaped = TRUE ORDER BY start_ts DESC;
//! ```
//!
//! Purge mode (`REAPER_PURGE=1`): one-shot wipe — delete the `.ogg` and any
//! `hls-{stem}/` cache dir, then `DELETE FROM audio_files` for those rows.
//! Use after a long zombie buildup when you don't want to inspect each one.
//!
//! `bot_reaper_state.last_reap_ts` lets us skip rows already covered by an
//! earlier reap so we don't rescan history every boot.

use chrono::{DateTime, Datelike, Utc};
use sakiot_paths::{RECORDING_ROOT, RecordingKey};
use sqlx::{Pool, Postgres};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{error, info, warn};

pub async fn reap_zombie_recordings(pool: &Pool<Postgres>) {
    let purge = std::env::var("REAPER_PURGE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    let now_ms = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_millis() as i64,
        Err(e) => {
            error!("reaper: clock before epoch: {}", e);
            return;
        }
    };

    let last_reap_ts: i64 =
        match sqlx::query_scalar!("SELECT last_reap_ts FROM bot_reaper_state WHERE id = 1")
            .fetch_optional(pool)
            .await
        {
            Ok(Some(v)) => v,
            Ok(None) => 0,
            Err(e) => {
                error!("reaper: read last_reap_ts failed: {}", e);
                return;
            }
        };

    let zombies = match sqlx::query!(
        "SELECT file_name, guild_id, channel_id, start_ts
           FROM audio_files
          WHERE end_ts IS NULL AND start_ts > $1",
        last_reap_ts
    )
    .fetch_all(pool)
    .await
    {
        Ok(r) => r,
        Err(e) => {
            error!("reaper: zombie select failed: {}", e);
            return;
        }
    };

    if zombies.is_empty() {
        info!(last_reap_ts, "reaper: no zombies");
    }

    let mut deleted_files = 0usize;
    let mut missing_files = 0usize;

    if purge {
        warn!("REAPER_PURGE=1 — deleting zombie files from disk");
        for z in &zombies {
            let Some(start_ts) = z.start_ts else {
                warn!(file_name = %z.file_name, "reaper: NULL start_ts, skipping fs delete");
                continue;
            };
            let Some(dt) = DateTime::<Utc>::from_timestamp_millis(start_ts) else {
                warn!(file_name = %z.file_name, start_ts, "reaper: bad start_ts, skipping fs delete");
                continue;
            };
            let key = RecordingKey::new(
                z.guild_id,
                z.channel_id,
                dt.year(),
                dt.month(),
                z.file_name.clone(),
            );
            let path = key.recording_path(RECORDING_ROOT);
            match std::fs::remove_file(&path) {
                Ok(_) => {
                    deleted_files += 1;
                    info!(path = %path.display(), "reaper: deleted zombie recording");
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    missing_files += 1;
                }
                Err(e) => {
                    error!(path = %path.display(), error = %e, "reaper: delete failed");
                }
            }
            let hls = key.live_dir(RECORDING_ROOT);
            if hls.exists() {
                if let Err(e) = std::fs::remove_dir_all(&hls) {
                    warn!(path = %hls.display(), error = %e, "reaper: hls cleanup failed");
                }
            }
        }
    }

    let rows_changed = if purge {
        match sqlx::query!(
            "DELETE FROM audio_files WHERE end_ts IS NULL AND start_ts > $1",
            last_reap_ts
        )
        .execute(pool)
        .await
        {
            Ok(r) => r.rows_affected(),
            Err(e) => {
                error!("reaper: zombie delete failed: {}", e);
                return;
            }
        }
    } else {
        match sqlx::query!(
            "UPDATE audio_files
                SET end_ts = start_ts, reaped = TRUE
                WHERE end_ts IS NULL AND start_ts > $1",
            last_reap_ts
        )
        .execute(pool)
        .await
        {
            Ok(r) => r.rows_affected(),
            Err(e) => {
                error!("reaper: zombie update failed: {}", e);
                return;
            }
        }
    };

    if let Err(e) = sqlx::query!(
        "UPDATE bot_reaper_state SET last_reap_ts = $1 WHERE id = 1",
        now_ms
    )
    .execute(pool)
    .await
    {
        error!("reaper: bump last_reap_ts failed: {}", e);
    }

    info!(
        purge,
        zombies = zombies.len(),
        rows_changed,
        deleted_files,
        missing_files,
        last_reap_ts,
        now_ms,
        "startup zombie reaper done"
    );
}
