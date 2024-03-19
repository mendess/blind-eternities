-- Add migration script here
ALTER TABLE machine_status DROP COLUMN gateway_ip;
ALTER TABLE machine_status DROP COLUMN local_ip;
ALTER TABLE machine_status DROP COLUMN gateway_mac;

CREATE TABLE ip_connection (
    hostname VARCHAR(253) NOT NULL,
    local_ip VARCHAR(39) NOT NULL,
    gateway_ip VARCHAR(39) NOT NULL,
    gateway_mac VARCHAR(39),

    FOREIGN KEY (hostname) REFERENCES machine_status(hostname)
);
