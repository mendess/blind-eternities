pub mod ws;

use std::{
    fmt,
    future::IntoFuture,
    sync::{Arc, OnceLock},
};

use blind_eternities::{
    auth,
    configuration::{Apis, DbSettings, Settings},
    routes::dirs::Directories,
    startup,
};
use common::{
    domain::Hostname,
    telemetry::{get_subscriber, init_subscriber},
};
use fake::{Fake, StringFaker};
use reqwest::StatusCode;
use sqlx::{Connection, Executor, PgConnection, PgPool, pool::PoolOptions};
use tokio::net::TcpListener;
use uuid::Uuid;

use crate::{assert_status, timeout};

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

#[derive(Clone)]
pub struct TestApp {
    pub address: String,
    pub persistent_conn_port: u16,
    pub db_pool: PgPool,
    pub db_name: String,
    pub http: reqwest::Client,
    pub auth_token: uuid::Uuid,
    #[allow(dyn_drop)]
    pub _drop_guards: Vec<Arc<dyn Drop>>,
}

impl fmt::Debug for TestApp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            address,
            persistent_conn_port,
            db_pool,
            db_name,
            http,
            auth_token,
            _drop_guards,
        } = self;
        f.debug_struct("TestApp")
            .field("address", address)
            .field("persistent_conn_port", persistent_conn_port)
            .field("db_pool", db_pool)
            .field("db_name", db_name)
            .field("http", http)
            .field("auth_token", auth_token)
            .finish()
    }
}

impl TestApp {
    pub async fn spawn() -> Self {
        init_tracing();

        tracing::debug!("creating socket");
        let listener = TcpListener::bind(("localhost", 0))
            .await
            .expect("Failed to bind random port");
        let persistent_conns_listener = TcpListener::bind(("localhost", 0))
            .await
            .expect("Failed to bind random port");
        let port = listener.local_addr().unwrap().port();
        let persistent_conn_port = persistent_conns_listener.local_addr().unwrap().port();
        let _spawn_span = tracing::debug_span!("spawning test app", port);

        let data_dir = tempfile::tempdir().expect("failed to create song dir");

        let conf = Settings {
            data_dir: data_dir.path().to_owned(),
            port: 8000,
            db: DbSettings {
                username: "postgres".into(),
                password: "postgres".into(),
                port: 5432,
                host: "localhost".into(),
                name: Uuid::new_v4().to_string(),
                migrate: false,
            },
            persistent_conn_port,
            enable_metrics: true,
            apis: Apis {
                navidrome: "http://0.0.0.0:0".parse().unwrap(),
            },
        };

        tracing::debug!("configuring database");
        let connection = configure_database(&conf.db).await;

        tracing::debug!("starting server");
        let server = startup::run(
            listener,
            None,
            connection.clone(),
            Directories::new(conf.data_dir),
            conf.apis,
        )
        .expect("Failed to bind address");
        tokio::spawn(server.into_future());
        let app = TestApp {
            address: format!("http://localhost:{port}"),
            persistent_conn_port,
            db_pool: connection,
            db_name: conf.db.name,
            http: reqwest::Client::new(),
            auth_token: uuid::Uuid::new_v4(),
            _drop_guards: vec![Arc::new(data_dir)],
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

    pub async fn send_cmd(
        &self,
        hostname: Hostname,
        cmd: impl Into<spark_protocol::Command>,
    ) -> spark_protocol::Response {
        let resp = self
            .post_authed(&format!("persistent-connections/ws/send/{hostname}"))
            .json(&cmd.into())
            .send()
            .await
            .expect("success");
        assert_status!(StatusCode::OK, resp.status());
        resp.json().await.expect("deserialized successfully")
    }

    pub async fn simulate_device_ws<C, R>(
        &self,
        Simulation {
            hostname,
            expect_to_receive,
            respond_with,
        }: Simulation<'_, C, R>,
    ) -> tokio::task::JoinHandle<()>
    where
        C: Into<spark_protocol::Command> + Send + 'static,
        R: Into<spark_protocol::Response> + Send + 'static,
    {
        let mut device = timeout!(self.connect_device_ws(hostname));

        tokio::spawn(async move {
            let (req, reply) = timeout!(device.recv()).expect("success recv");
            assert_eq!(expect_to_receive.into(), req);
            timeout!(reply.reply(respond_with.into()));
        })
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
                if let Err(e) = sqlx::query(&format!(r#"DROP DATABASE "{db_name}" WITH (FORCE)"#))
                    .execute(&mut conn)
                    .await
                {
                    eprintln!("Failed to drop database {db_name}: {e:?}")
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

pub struct Simulation<'s, C, R> {
    pub hostname: &'s Hostname,
    pub expect_to_receive: C,
    pub respond_with: R,
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
