CREATE TABLE bot_instances (
    instance_id TEXT PRIMARY KEY,
    role TEXT NOT NULL CHECK (role IN ('active', 'drain')),
    state TEXT NOT NULL CHECK (state IN ('active', 'draining', 'stopped')),
    heartbeat_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE voice_session_leases (
    guild_id BIGINT PRIMARY KEY,
    channel_id BIGINT NOT NULL,
    owner_instance_id TEXT NOT NULL REFERENCES bot_instances(instance_id) ON DELETE CASCADE,
    state TEXT NOT NULL CHECK (state IN ('active', 'draining')),
    heartbeat_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX voice_session_leases_owner_idx
    ON voice_session_leases (owner_instance_id);

CREATE INDEX voice_session_leases_heartbeat_idx
    ON voice_session_leases (heartbeat_at);
