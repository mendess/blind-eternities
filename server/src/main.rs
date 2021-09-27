use std::net::TcpListener;

use blind_eternities::{
    configuration::get_configuration,
    startup::run,
    telemetry::{get_subscriber, init_subscriber},
};
use sqlx::PgPool;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    init_subscriber(get_subscriber(
        "blind-eternities".into(),
        "info".into(),
        std::io::stdout,
    ));

    let conf = get_configuration().expect("Failed to read configuration");
    let conn_string = conf.db.connection_string();

    let connection = PgPool::connect(&conn_string)
        .await
        .expect("Failed to connect to Postgres");

    run(TcpListener::bind(("localhost", conf.port))?, connection)?.await
}
