use std::net as std_net;

use blind_eternities::configuration::{get_configuration, Settings};
use common::telemetry::{get_subscriber, init_subscriber};
use sqlx::{Connection, Executor, PgConnection, PgPool};
use tokio::net as tokio_net;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    init_subscriber(get_subscriber(
        "blind-eternities".into(),
        "info".into(),
        std::io::stdout,
    ));

    let conf = get_configuration().expect("Failed to read configuration");
    tracing::info!(initial_configuration = ?conf);
    let conn_string = conf.db.connection_string();

    let connection = if conf.db.migrate {
        migrate(&conf).await
    } else {
        PgPool::connect(&conn_string)
            .await
            .expect("Failed to connect to Postgres")
    };

    blind_eternities::startup::run(
        std_net::TcpListener::bind(("0.0.0.0", conf.port))?,
        tokio_net::TcpListener::bind(("0.0.0.0", conf.persistent_conn_port)).await?,
        connection,
        blind_eternities::startup::RunConfig {
            override_num_workers: None,
            enable_metrics: true,
        },
    )?
    .await
}

async fn migrate(config: &Settings) -> PgPool {
    let mut connection = PgConnection::connect(&config.db.connection_string_without_db())
        .await
        .expect("Failed to connect to Postgres");
    let exists = connection
        .fetch_one(
            format!(
                "SELECT 1 FROM pg_catalog.pg_database WHERE datname = '{}'",
                config.db.name
            )
            .as_str(),
        )
        .await;
    match exists {
        Ok(_) => {}
        Err(sqlx::Error::RowNotFound) => {
            connection
                .execute(format!(r#"CREATE DATABASE "{}";"#, config.db.name).as_str())
                .await
                .expect("Failed to create database.");
        }
        Err(e) => {
            panic!("{1}: {:?}", e, "failed to inspect db");
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
