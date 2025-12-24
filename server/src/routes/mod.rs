pub mod admin;
pub mod machine_status;
pub mod music;
pub mod persistent_connections;
pub mod playlist;
pub mod walls;

use std::sync::Arc;

use axum::{Router, extract::FromRef};
use sqlx::PgPool;

use crate::persistent_connections::ws::SocketIo;
use common::net::auth_client::Client;

pub mod dirs {
    #[derive(Debug)]
    pub struct Directories {
        base: std::path::PathBuf,
    }

    impl Directories {
        pub fn new(path: std::path::PathBuf) -> Self {
            Self { base: path }
        }
    }

    pub trait Directory {
        fn get(&self) -> std::path::PathBuf;

        fn file(&self, path: &str) -> std::path::PathBuf {
            let mut buf = self.get();
            buf.push(path);
            buf
        }
    }

    macro_rules! directories {
        ( $($node:ident/ { $($child:tt)* } )+ ) => {
            $(

                impl Directories {
                    pub fn $node(&self) -> $node::$node {
                        $node::$node { parent: self.base.clone() }
                    }
                }

                pub mod $node {
                    #[allow(non_camel_case_types)]
                    #[derive(Debug, Clone)]
                    pub struct $node {
                        pub(super) parent: std::path::PathBuf
                    }


                    impl $node {
                        fn build(mut self) -> std::path::PathBuf {
                            self.parent.push(std::stringify!($node));
                            self.parent
                        }
                    }
                    directories!(@children ($node) $($child)*);
                }
            )*
        };

        (@children ($parent:ident) $node:ident/ { $($child:tt)* } $($rest:tt)*) => {
            pub mod $node {
                directories!(@node $parent, $node);
                impl $node {
                    fn build(self, path: &mut std::path::PathBuf) {
                        self.parent.build(path);
                        path.push(::std::stringify!($node));
                    }
                }
                directories!(@children ($node) $($child)*);
            }
            directories!(@children ($parent) $($rest)*);
        };

        (@children ($parent:ident) $node:ident $($rest:tt)*) => {
            pub mod $node {
                directories!(@node $parent, $node);
            }
            directories!(@children ($parent) $($rest)*);
        };

        (@children ($($parent:ident),*)) => {};

        (@node $parent:ident, $node:ident) => {
            #[allow(non_camel_case_types)]
            #[derive(Debug, Clone)]
            pub struct $node {
                parent: super::$parent,
            }

            impl super::$parent {
                pub fn $node(self) -> $node {
                    $node { parent: self }
                }
            }

            impl $crate::routes::dirs::Directory for $node {
                fn get(&self) -> std::path::PathBuf {
                    static CREATE_DIR: std::sync::Once = std::sync::Once::new();
                    let mut path = self.clone().parent.build();
                    path.push(::std::stringify!($node));
                    CREATE_DIR.call_once(|| {
                        std::fs::create_dir_all(&path).unwrap();
                    });
                    path
                }
            }
        }
    }

    directories! {
        music/ {
            audio
            meta
            thumb
            mtogo
        }
        walls/ {
            phone
            small
            all
        }
    }
}

#[derive(Debug)]
pub struct Apis {
    pub navidrome: Client,
}

#[derive(Debug, Clone, FromRef)]
pub struct RouterState {
    dirs: Arc<dirs::Directories>,
    db: Arc<PgPool>,
    socket_io: SocketIo,
    apis: Arc<Apis>,
}

pub fn router(db: Arc<PgPool>, socket_io: SocketIo, dirs: dirs::Directories, apis: Apis) -> Router {
    Router::new()
        .nest("/admin", admin::routes())
        .nest("/machine", machine_status::routes())
        .nest("/persistent-connections", persistent_connections::routes())
        .nest("/music", music::routes())
        .nest("/playlist", playlist::routes())
        .nest("/walls", walls::routes())
        .with_state(RouterState {
            db,
            socket_io,
            dirs: Arc::new(dirs),
            apis: Arc::new(apis),
        })
}
