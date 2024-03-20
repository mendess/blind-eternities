-- Add migration script here
CREATE TYPE role AS ENUM ('admin', 'music');

ALTER TABLE api_tokens
ADD COLUMN role role;

UPDATE api_tokens SET role = 'admin';

ALTER TABLE api_tokens
ALTER COLUMN role SET NOT NULL;
