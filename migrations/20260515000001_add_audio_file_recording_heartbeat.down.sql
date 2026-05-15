DROP INDEX IF EXISTS audio_files_recording_heartbeat_idx;
DROP INDEX IF EXISTS audio_files_recording_owner_idx;

ALTER TABLE audio_files
    DROP COLUMN recording_heartbeat_at,
    DROP COLUMN recording_owner_instance_id;
