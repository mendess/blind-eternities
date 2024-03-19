-- Add migration script here
DROP TABLE ip_connection;

ALTER TABLE machine_status ADD COLUMN gateway_ip VARCHAR(39) NOT NULL;
ALTER TABLE machine_status ADD COLUMN local_ip VARCHAR(39) NOT NULL;
ALTER TABLE machine_status ADD COLUMN gateway_mac VARCHAR(23);

