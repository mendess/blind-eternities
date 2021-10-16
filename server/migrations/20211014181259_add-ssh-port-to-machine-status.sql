-- Add migration script here

ALTER TABLE machine_status ADD COLUMN ssh_port INT;
