use std::{net::TcpListener, ops::Range};

use actix_rt::task::spawn_blocking;
use blind_eternities::{
    configuration::{get_configuration, DbSettings},
    startup,
};
use common::telemetry::{get_subscriber, init_subscriber};
use fake::StringFaker;
use once_cell::sync::Lazy;
use sqlx::{Connection, Executor, PgConnection, PgPool};
use uuid::Uuid;

static TRACING: Lazy<()> = Lazy::new(|| {
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

#[derive(Clone, Debug)]
pub struct TestApp {
    pub address: String,
    pub db_pool: PgPool,
    pub db_name: String,
    pub http: reqwest::Client,
    pub token: uuid::Uuid,
}

impl Drop for TestApp {
    fn drop(&mut self) {
        let db_name = std::mem::take(&mut self.db_name);
        std::thread::spawn(move || {
            tokio::runtime::Runtime::new()
                .expect("spawn a short runtime")
                .block_on(async move {
                    let mut conn =
                        PgConnection::connect("postgres://postgres:postgres@localhost:5432")
                            .await
                            .expect("Failed to connect to postgres");
                    if let Err(e) =
                        sqlx::query(&format!(r#"DROP DATABASE "{}" WITH (FORCE)"#, db_name))
                            .execute(&mut conn)
                            .await
                    {
                        eprintln!("Failed to drop database {}: {:?}", db_name, e)
                    }
                });
        })
        .join()
        .expect("failed to join drop db thread");
    }
}

pub struct TestAppBuilder {
    allow_any_localhost_token: bool,
}

impl TestAppBuilder {
    pub fn allow_any_localhost_token(&mut self, b: bool) -> &mut Self {
        self.allow_any_localhost_token = b;
        self
    }

    pub async fn spawn(&mut self) -> TestApp {
        Lazy::force(&TRACING);

        tracing::debug!("creating socket");
        let listener = TcpListener::bind(("localhost", 0)).expect("Failed to bind random port");
        let port = listener.local_addr().unwrap().port();
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
        let server = startup::run(listener, connection.clone(), self.allow_any_localhost_token)
            .expect("Failed to bind address");
        let _ = tokio::spawn(server);
        let app = TestApp {
            address: format!("http://localhost:{}", port),
            db_pool: connection,
            db_name: conf.db.name,
            http: reqwest::Client::new(),
            token: uuid::Uuid::new_v4(),
        };
        tracing::debug!("inserting auth token");
        app.insert_test_token().await;
        tracing::debug!(?app, "app created");
        app
    }
}

impl TestApp {
    pub async fn spawn() -> Self {
        Self::builder().spawn().await
    }

    pub fn builder() -> TestAppBuilder {
        TestAppBuilder {
            allow_any_localhost_token: true,
        }
    }

    async fn insert_test_token(&self) {
        sqlx::query!(
            "INSERT INTO api_tokens (token, created_at, hostname) VALUES ($1, NOW(), $2)",
            self.token,
            "hostname"
        )
        .execute(&self.db_pool)
        .await
        .expect("Failed to insert token");
    }

    pub async fn post_machine_status(&self, body: impl ToString) -> reqwest::Response {
        self.post("machine/status")
            .header("Content-Type", "application/json")
            .bearer_auth(self.token)
            .body(body.to_string())
            .send()
            .await
            .expect("Failed to execute request")
    }

    pub fn get(&self, path: &str) -> reqwest::RequestBuilder {
        self.http.get(&format!("{}/{}", self.address, path))
    }

    #[allow(dead_code)]
    pub fn get_authed(&self, path: &str) -> reqwest::RequestBuilder {
        self.get(path).bearer_auth(self.token)
    }

    pub fn post(&self, path: &str) -> reqwest::RequestBuilder {
        self.http.post(&format!("{}/{}", self.address, path))
    }

    #[allow(dead_code)]
    pub fn post_authed(&self, path: &str) -> reqwest::RequestBuilder {
        self.post(path).bearer_auth(self.token)
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
    let connection_pool = PgPool::connect(&config.connection_string())
        .await
        .expect("Failed to connect to Postgres.");
    sqlx::migrate!("./migrations")
        .run(&connection_pool)
        .await
        .expect("Failed to migrate the database");

    connection_pool
}

pub fn fake_hostname() -> StringFaker<Range<usize>> {
    StringFaker::with((b'a'..b'z').collect(), 4..20)
}
