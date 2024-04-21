-- Add down migration script here
DROP INDEX music_sessions_unique_hostnames;
DROP INDEX music_session_unique_ids;

DROP TABLE music_sessions;
