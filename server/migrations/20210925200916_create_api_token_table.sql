-- Add migration script here
CREATE TABLE api_tokens (
    token UUID PRIMARY KEY,
    created_at TIMESTAMP NOT NULL,
    hostname VARCHAR(253) NOT NULL
);
