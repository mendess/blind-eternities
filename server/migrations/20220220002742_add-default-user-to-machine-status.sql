-- Add migration script here

ALTER TABLE machine_status ADD COLUMN default_user VARCHAR(253) DEFAULT NULL;
