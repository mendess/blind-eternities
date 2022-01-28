-- Add migration script here
CREATE TABLE bulb (
    id UUID PRIMARY KEY,
    ip VARCHAR(39) NOT NULL,
    mac VARCHAR(39) NOT NULL,
    owner VARCHAR(253) NOT NULL,
    name VARCHAR(256),

    FOREIGN KEY (owner) REFERENCES machine_status(hostname)
);
