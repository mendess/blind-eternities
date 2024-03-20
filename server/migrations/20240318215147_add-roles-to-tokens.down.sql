-- Add migration script here
ALTER TABLE api_tokens
DROP COLUMN role;

DROP TYPE role;
