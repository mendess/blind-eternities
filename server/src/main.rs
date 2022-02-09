use std::net::TcpListener;

use blind_eternities::{configuration::get_configuration, startup::run};
use common::telemetry::{get_subscriber, init_subscriber};
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

    if conf.db.migrate {
        sqlx::migrate!("./migrations")
            .run(&connection)
            .await
            .expect("Failed to migrate the database");
    }

    run(
        TcpListener::bind(("0.0.0.0", conf.port))?,
        connection,
        conf.allow_any_localhost_token,
    )?
    .await
}
