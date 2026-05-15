ALTER TABLE audio_files
    ADD COLUMN recording_owner_instance_id TEXT NULL REFERENCES bot_instances(instance_id) ON DELETE SET NULL,
    ADD COLUMN recording_heartbeat_at TIMESTAMPTZ NULL;

CREATE INDEX audio_files_recording_owner_idx
    ON audio_files (recording_owner_instance_id)
    WHERE recording_owner_instance_id IS NOT NULL;

CREATE INDEX audio_files_recording_heartbeat_idx
    ON audio_files (recording_heartbeat_at)
    WHERE recording_heartbeat_at IS NOT NULL;
