use actix_web::{web, HttpResponse, Responder};

use crate::persistent_connections::Connections;

pub fn routes() -> actix_web::Scope {
    web::scope("/persistent-connections").route("", web::get().to(index))
}

async fn index(connections: web::Data<Connections>) -> impl Responder {
    //     let connected = conn.connected_hosts().await;
    //     let good = futures::stream::iter(connected)
    //         .map(|(h, gen)| {
    //             let conn = &conn;
    //             async move {
    //                 (
    //                     conn.request(
    //                         &h,
    //                         Local::Music(MusicCmd {
    //                             index: None,
    //                             username: None,
    //                             command: MusicCmdKind::Current,
    //                         }),
    //                     )
    //                     .await,
    //                     (h, gen),
    //                 )
    //             }
    //         })
    //         .buffer_unordered(usize::MAX)
    //         .fold(Vec::new(), |mut good, (result, (hostname, gen))| {
    //             let conn = &conn;
    //             async move {
    //                 match result {
    //                     Ok(_) => good.push(hostname),
    //                     Err(ConnectionError::ConnectionDropped) => {
    //                         conn.remove(hostname, gen).await;
    //                     }
    //                     Err(ConnectionError::NotFound) => {}
    //                 }
    //                 good
    //             }
    //         })
    //         .await;
    //     Ok(HttpResponse::Ok().json(good))
    let connected = connections.connected_hosts().await;
    HttpResponse::Ok().json(connected.into_iter().map(|(h, _)| h).collect::<Vec<_>>())
}
