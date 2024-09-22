use blind_eternities::{auth, configuration::get_configuration};
use sqlx::{PgPool, Row};
use std::env;
use uuid::Uuid;

async fn _main() -> i32 {
    let mut args = env::args().skip(1);
    let hostname = match args.next() {
        Some(hostname) => hostname,
        None => {
            println!(
                "Usage {} HOSTNAME [-d|--delete]",
                env::args().next().unwrap()
            );
            return 1;
        }
    };

    let delete = matches!(args.next().as_deref(), Some("-d" | "--delete"));

    let conf = get_configuration().expect("Failed to read configuration");
    let conn_string = conf.db.connection_string();

    println!("connecting to db: {conn_string}");
    let connection = PgPool::connect(&conn_string)
        .await
        .expect("Failed to connect to Postgres");

    let r = if delete {
        println!("deleting: {hostname}");
        sqlx::query("DELETE FROM api_tokens WHERE hostname = $1 RETURNING token")
            .bind(&hostname)
            .fetch_one(&connection)
            .await
            .map(|row| row.get(0))
    } else {
        let uuid = Uuid::new_v4();

        println!("inserting new token: {hostname}");
        auth::insert_token::<auth::Admin>(&connection, uuid, &hostname)
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
