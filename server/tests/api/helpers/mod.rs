pub mod persistent_conn;

use std::{net::TcpListener, sync::OnceLock};

use actix_rt::task::spawn_blocking;
use blind_eternities::{
    auth,
    configuration::{get_configuration, DbSettings},
    startup,
};
use common::{
    domain::Hostname,
    telemetry::{get_subscriber, init_subscriber},
};
use fake::{Fake, StringFaker};
use sqlx::{pool::PoolOptions, Connection, Executor, PgConnection, PgPool};
use uuid::Uuid;

fn init_tracing() {
    static TRACING: OnceLock<()> = OnceLock::new();
    TRACING.get_or_init(|| {
        if std::env::var("TEST_LOG").is_ok() {
            init_subscriber(get_subscriber(
                "test".into(),
                "debug".into(),
                std::io::stdout,
            ));
        } else {
            init_subscriber(get_subscriber("test".into(), "debug".into(), std::io::sink));
        }
    });
}

#[derive(Clone, Debug)]
pub struct TestApp {
    pub address: String,
    pub persistent_conn_port: u16,
    pub db_pool: PgPool,
    pub db_name: String,
    pub http: reqwest::Client,
    pub auth_token: uuid::Uuid,
}

impl TestApp {
    pub async fn spawn() -> Self {
        init_tracing();

        tracing::debug!("creating socket");
        let listener = TcpListener::bind(("localhost", 0)).expect("Failed to bind random port");
        let persistent_conns_listener = tokio::net::TcpListener::bind(("localhost", 0))
            .await
            .expect("Failed to bind random port");
        let port = listener.local_addr().unwrap().port();
        let persistent_conn_port = persistent_conns_listener.local_addr().unwrap().port();
        let _spawn_span = tracing::debug_span!("spawning test app", port);

        tracing::debug!("loading configuration");
        let mut conf =
            spawn_blocking(|| get_configuration().expect("Failed to read configuration"))
                .await
                .unwrap();
        conf.db.name = Uuid::new_v4().to_string();

        tracing::debug!("configuring database");
        let connection = configure_database(&conf.db).await;

        tracing::debug!("starting server");
        let server = startup::run(
            listener,
            persistent_conns_listener,
            connection.clone(),
            startup::RunConfig {
                override_num_workers: Some(1),
                enable_metrics: false,
            },
        )
        .expect("Failed to bind address");
        tokio::spawn(server);
        let app = TestApp {
            address: format!("http://localhost:{}", port),
            persistent_conn_port,
            db_pool: connection,
            db_name: conf.db.name,
            http: reqwest::Client::new(),
            auth_token: uuid::Uuid::new_v4(),
        };
        tracing::debug!("inserting auth token");
        auth::insert_token::<auth::Admin>(&app.db_pool, app.auth_token, "hostname")
            .await
            .expect("failed to insert admin token");
        tracing::debug!(?app, "app created");
        app
    }
}

impl TestApp {
    pub fn get(&self, path: &str) -> reqwest::RequestBuilder {
        self.http.get(format!("{}/{}", self.address, path))
    }

    pub fn get_authed(&self, path: &str) -> reqwest::RequestBuilder {
        self.get(path).bearer_auth(self.auth_token)
    }

    pub fn post(&self, path: &str) -> reqwest::RequestBuilder {
        self.http.post(format!("{}/{}", self.address, path))
    }

    #[allow(dead_code)]
    pub fn post_authed(&self, path: &str) -> reqwest::RequestBuilder {
        self.post(path).bearer_auth(self.auth_token)
    }

    #[allow(dead_code)]
    pub fn patch_authed(&self, path: &str) -> reqwest::RequestBuilder {
        self.http
            .patch(format!("{}/{}", self.address, path))
            .bearer_auth(self.auth_token)
    }

    #[allow(dead_code)]
    pub fn delete_authed(&self, path: &str) -> reqwest::RequestBuilder {
        self.http
            .delete(format!("{}/{}", self.address, path))
            .bearer_auth(self.auth_token)
    }

    pub async fn downgrade_to<R: auth::Role>(mut self) -> Self {
        self.auth_token = self.add_auth_token::<R>().await;
        self
    }

    pub async fn add_auth_token<R: auth::Role>(&self) -> uuid::Uuid {
        let uuid = Uuid::new_v4();
        auth::insert_token::<R>(&self.db_pool, uuid, fake_hostname().as_ref())
            .await
            .expect("failed to insert_token new auth token");
        uuid
    }
}

impl Drop for TestApp {
    fn drop(&mut self) {
        let db_name = std::mem::take(&mut self.db_name);
        if let Err(e) = std::thread::spawn(move || {
            std::iter::from_fn(|| {
                Some(
                    tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build(),
                )
            })
            .take(5)
            .enumerate()
            .inspect(|(i, _)| {
                if *i > 0 {
                    std::thread::sleep(std::time::Duration::from_secs_f64(0.5))
                }
            })
            .find_map(|(_, r)| r.ok())
            .expect("failed to spawn drop runtime")
            .block_on(async move {
                let mut conn = PgConnection::connect("postgres://postgres:postgres@localhost:5432")
                    .await
                    .expect("Failed to connect to postgres");
                if let Err(e) = sqlx::query(&format!(r#"DROP DATABASE "{}" WITH (FORCE)"#, db_name))
                    .execute(&mut conn)
                    .await
                {
                    eprintln!("Failed to drop database {}: {:?}", db_name, e)
                } else {
                    eprintln!("dropped database '{db_name}'");
                }
            });
        })
        .join()
        {
            eprintln!("drop thread panicked {e:?}");
        }
    }
}

async fn configure_database(config: &DbSettings) -> PgPool {
    // Create database
    let mut connection = PgConnection::connect(&config.connection_string_without_db())
        .await
        .expect("Failed to connect to Postgres");
    connection
        .execute(format!(r#"CREATE DATABASE "{}";"#, config.name).as_str())
        .await
        .expect("Failed to create database.");

    // Migrate database
    let connection_pool = PoolOptions::new()
        .max_connections(1)
        .connect_lazy(&config.connection_string())
        .expect("Failed to connect to Postgres.");
    sqlx::migrate!("./migrations")
        .run(&connection_pool)
        .await
        .expect("Failed to migrate the database");

    connection_pool
}

pub fn fake_hostname() -> Hostname {
    StringFaker::with((b'a'..=b'z').collect(), 4..20)
        .fake::<String>()
        .parse::<Hostname>()
        .unwrap()
}
