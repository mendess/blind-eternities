pub mod persistent_conn;

use std::net::TcpListener;

use actix_rt::task::spawn_blocking;
use blind_eternities::{
    configuration::{get_configuration, DbSettings},
    startup,
};
use common::{
    domain::Hostname,
    telemetry::{get_subscriber, init_subscriber},
};
use fake::{Fake, StringFaker};
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
pub struct TestApp<const CREATE_DB: bool = true> {
    pub address: String,
    pub persistent_conn_port: u16,
    pub db_pool: PgPool,
    pub db_name: String,
    pub http: reqwest::Client,
    pub token: uuid::Uuid,
}

impl<const CREATE_DB: bool> Drop for TestApp<CREATE_DB> {
    fn drop(&mut self) {
        if !CREATE_DB {
            return;
        }
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
                }
            });
        })
        .join()
        {
            eprintln!("drop thread panicked {e:?}");
        }
    }
}

pub struct TestAppBuilder<const CREATE_DB: bool> {
    allow_any_localhost_token: bool,
}

impl<const CREATE_DB: bool> TestAppBuilder<CREATE_DB> {
    pub fn allow_any_localhost_token(&mut self, b: bool) -> &mut Self {
        self.allow_any_localhost_token = b;
        self
    }

    pub async fn spawn(&mut self) -> TestApp<CREATE_DB> {
        Lazy::force(&TRACING);

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

        let connection = if CREATE_DB {
            tracing::debug!("configuring database");
            configure_database(&conf.db).await
        } else {
            PgPool::connect_lazy(&conf.db.connection_string())
                .expect("failed to connect to postgres")
        };

        tracing::debug!("starting server");
        let server = startup::run(
            listener,
            persistent_conns_listener,
            connection.clone(),
            self.allow_any_localhost_token,
        )
        .expect("Failed to bind address");
        let _ = tokio::spawn(server);
        let app = TestApp::<CREATE_DB> {
            address: format!("http://localhost:{}", port),
            persistent_conn_port,
            db_pool: connection,
            db_name: conf.db.name,
            http: reqwest::Client::new(),
            token: uuid::Uuid::new_v4(),
        };
        if CREATE_DB {
            tracing::debug!("inserting auth token");
            insert_test_token(&app.db_pool, app.token).await;
        }
        tracing::debug!(?app, "app created");
        app
    }
}

impl TestAppBuilder<true> {
    pub fn without_db(self) -> TestAppBuilder<false> {
        TestAppBuilder {
            allow_any_localhost_token: self.allow_any_localhost_token,
        }
    }
}

impl TestApp<true> {
    pub async fn spawn() -> Self {
        Self::builder().spawn().await
    }
}

impl TestApp<false> {
    pub async fn spawn_without_db() -> Self {
        TestApp::<true>::builder().without_db().spawn().await
    }
}

impl TestApp<true> {
    pub fn builder() -> TestAppBuilder<true> {
        TestAppBuilder {
            allow_any_localhost_token: true,
        }
    }
}

impl<const CREATE_DB: bool> TestApp<CREATE_DB> {
    pub fn get(&self, path: &str) -> reqwest::RequestBuilder {
        self.http.get(format!("{}/{}", self.address, path))
    }

    pub fn get_authed(&self, path: &str) -> reqwest::RequestBuilder {
        self.get(path).bearer_auth(self.token)
    }

    pub fn post(&self, path: &str) -> reqwest::RequestBuilder {
        self.http.post(format!("{}/{}", self.address, path))
    }

    #[allow(dead_code)]
    pub fn post_authed(&self, path: &str) -> reqwest::RequestBuilder {
        self.post(path).bearer_auth(self.token)
    }

    #[allow(dead_code)]
    pub fn patch_authed(&self, path: &str) -> reqwest::RequestBuilder {
        self.http
            .patch(format!("{}/{}", self.address, path))
            .bearer_auth(self.token)
    }

    #[allow(dead_code)]
    pub fn delete_authed(&self, path: &str) -> reqwest::RequestBuilder {
        self.http
            .delete(format!("{}/{}", self.address, path))
            .bearer_auth(self.token)
    }
}

async fn insert_test_token(pool: &PgPool, token: Uuid) {
    sqlx::query!(
        "INSERT INTO api_tokens (token, created_at, hostname) VALUES ($1, NOW(), $2)",
        token,
        "hostname"
    )
    .execute(pool)
    .await
    .expect("Failed to insert token");
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
    let connection_pool = PgPool::connect_lazy(&config.connection_string())
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
