use blind_eternities::configuration::get_configuration;
use sqlx::PgPool;
use std::env;
use uuid::Uuid;

async fn _main() -> i32 {
    let mut args = env::args().skip(1);
    let hostname = match args.next() {
        Some(hostname) => hostname,
        None => {
            println!("Usage {} [HOSTNAME]", env::args().next().unwrap());
            return 1;
        }
    };

    let delete = matches!(args.next().as_deref(), Some("-d" | "--delete"));

    let conf = get_configuration().expect("Failed to read configuration");
    let conn_string = conf.db.connection_string();

    let connection = PgPool::connect(&conn_string)
        .await
        .expect("Failed to connect to Postgres");

    let r = if delete {
        sqlx::query!(
            "DELETE FROM api_tokens WHERE hostname = $1 RETURNING token",
            hostname
        )
        .fetch_one(&connection)
        .await
        .map(|t| t.token)
    } else {
        let uuid = Uuid::new_v4();

        sqlx::query!(
            "INSERT INTO api_tokens (token, created_at, hostname) VALUES ($1, NOW(), $2)",
            uuid,
            hostname
        )
        .execute(&connection)
        .await
        .map(|_| uuid)
    };

    match r {
        Ok(uuid) if delete => {
            println!("Token deleted: '{}'", uuid);
            0
        }
        Ok(uuid) => {
            println!("Token created: '{}'", uuid);
            0
        }
        Err(e) => {
            println!("Failed to create token: {:#?}", e);
            1
        }
    }
}

#[tokio::main]
async fn main() {
    std::process::exit(_main().await);
}
