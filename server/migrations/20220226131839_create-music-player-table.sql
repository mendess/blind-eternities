-- Add migration script here
CREATE TABLE music_player (
    hostname VARCHAR(253) NOT NULL,
    player INT NOT NULL,
    priority SERIAL NOT NULL,


    PRIMARY KEY (hostname, player),
    FOREIGN KEY (hostname) REFERENCES machine_status(hostname)
);
