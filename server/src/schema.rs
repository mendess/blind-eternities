table! {
    machine_status (hostname) {
        hostname -> Varchar,
        local_ip -> Varchar,
        external_ip -> Varchar,
        gateway_ip -> Varchar,
        gateway_mac -> Nullable<Varchar>,
        last_heartbit -> Timestamp,
    }
}
