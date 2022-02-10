use std::net::TcpListener;

use blind_eternities::{configuration::{Settings, get_configuration}, startup::run};
use common::telemetry::{get_subscriber, init_subscriber};
use sqlx::{PgPool, PgConnection, Connection, Executor};

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    init_subscriber(get_subscriber(
        "blind-eternities".into(),
        "info".into(),
        std::io::stdout,
    ));

    let conf = get_configuration().expect("Failed to read configuration");
    let conn_string = conf.db.connection_string();

    let connection = if conf.db.migrate {
        migrate(&conf).await
    } else {
        PgPool::connect(&conn_string)
            .await
            .expect("Failed to connect to Postgres")
    };

    run(
        TcpListener::bind(("0.0.0.0", conf.port))?,
        connection,
        conf.allow_any_localhost_token,
    )?
    .await
}

async fn migrate(config: &Settings) -> PgPool {
    let mut connection = PgConnection::connect(&config.db.connection_string_without_db())
        .await
        .expect("Failed to connect to Postgres");
    let exists = connection
        .fetch_one(
            format!("SELECT 1 FROM pg_catalog.pg_database WHERE datname = '{}'", config.db.name)
                .as_str()
        )
        .await;
    match exists {
        Ok(_) => {},
        Err(sqlx::Error::RowNotFound) => {
            connection
                .execute(format!(r#"CREATE DATABASE "{}";"#, config.db.name).as_str())
                .await
                .expect("Failed to create database.");
        },
        Err(e) => {
            Result::<(), _>::Err(e).expect("failed to inspect db");
        }
    }
    let connection = PgPool::connect(&config.db.connection_string())
        .await
        .expect("Failed to connect to Postgres");

    sqlx::migrate!("./migrations")
        .run(&connection)
        .await
        .expect("Failed to migrate the database");
    connection
}
