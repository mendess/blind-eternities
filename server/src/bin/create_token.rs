use blind_eternities::configuration::get_configuration;
use sqlx::PgPool;
use std::env::args;
use uuid::Uuid;

#[tokio::main]
async fn main() {
    let hostname = match args().nth(1) {
        Some(hostname) => hostname,
        None => {
            println!("Usage {} [HOSTNAME]", args().next().unwrap());
            std::process::exit(1);
        }
    };

    let uuid = Uuid::new_v4();

    let conf = get_configuration().expect("Failed to read configuration");
    let conn_string = conf.db.connection_string();

    let connection = PgPool::connect(&conn_string)
        .await
        .expect("Failed to connect to Postgres");

    let r = sqlx::query!(
        "INSERT INTO api_tokens (token, created_at, hostname) VALUES ($1, NOW(), $2)",
        uuid,
        hostname
    )
    .execute(&connection)
    .await;

    if let Err(e) = r {
        println!("Failed to create token: {:?}", e);
        std::process::exit(1);
    } else {
        println!("Token created: '{}'", uuid);
    }
}
