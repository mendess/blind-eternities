CREATE TABLE machine_status (
    hostname VARCHAR(128) PRIMARY KEY,
    local_ip VARCHAR(32) NOT NULL,
    external_ip VARCHAR(32) NOT NULL,
    gateway_ip VARCHAR(32) NOT NULL,
    gateway_mac VARCHAR(23),
    last_heartbeat TIMESTAMP NOT NULL
);
