# Routes

- `[root]`: Hello world
    - `/machine`
        - `/status`
            `use common::domain::{Hostname, machine_status::{MachineStatus, MachineStatusFull}}`
            - `GET`
                response: `Map<Hostname, MachineStatusFull>`
            - `POST`
                body: `MachineStatus`

    - `/health_check` returns Ok
    - `/remote-spark/{machine}` forwards a command to a remote spark
        - `POST`
            body: `spark-protocol::Local`
    - `/music`
        - `/player`
            - `GET` list all players
            - `/{machine}/{index}`
                - `POST` create a new player
                - `DELETE` delete the player
                - `PATCH` pull the player to the top
                - `/last` the last queued position
                    - `GET`
                        response: `usize | null`
                    - `DELETE` resets to null
                    - `POST`
                        body: `usize`

