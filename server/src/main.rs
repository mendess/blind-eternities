use anyhow::Context;
use blind_eternities::configuration::{Settings, get_configuration};
use clap::Parser;
use common::telemetry::{get_subscriber, init_subscriber};
use sqlx::{Connection, Executor, PgConnection, PgPool};
use tokio::net::TcpListener;

#[derive(clap::Parser)]
struct Args {
    #[arg(short, long)]
    config: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let Args { config } = Args::parse();
    init_subscriber(get_subscriber(
        "blind-eternities".into(),
        "info".into(),
        std::io::stdout,
    ));

    let conf = get_configuration(config.as_deref()).expect("Failed to read configuration");
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
        TcpListener::bind(("0.0.0.0", conf.port))
            .await
            .context("binding http socket")?,
        TcpListener::bind("0.0.0.0:9000")
            .await
            .context("binding metrics listener port")?,
        connection,
        conf.playlist_config,
    )?
    .await
    .context("running blind_eternities")?;
    Ok(())
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
