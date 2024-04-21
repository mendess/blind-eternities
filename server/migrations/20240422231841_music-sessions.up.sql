-- Add up migration script here
CREATE TABLE music_sessions (
    id VARCHAR(6) NOT NULL,
    hostname VARCHAR(253) NOT NULL,
    expires_at TIMESTAMP NOT NULL
);

CREATE UNIQUE INDEX music_session_unique_ids ON music_sessions (id);
CREATE UNIQUE INDEX music_sessions_unique_hostnames ON music_sessions (hostname);
