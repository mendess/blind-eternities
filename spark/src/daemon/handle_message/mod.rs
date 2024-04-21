#[cfg(feature = "music-ctl")]
pub mod music;

use std::{os::unix::prelude::CommandExt, sync::Mutex, thread, time::Duration};

use spark_protocol::{Command, ErrorResponse, SuccessfulResponse};

pub async fn rxtx(cmd: Command) -> Result<SuccessfulResponse, ErrorResponse> {
    match cmd {
        Command::Heartbeat => Ok(SuccessfulResponse::Unit),
        Command::Reload => {
            std::thread::spawn(reload()?);
            Ok(SuccessfulResponse::Unit)
        }
        #[cfg(feature = "music-ctl")]
        Command::Music(m) => music::handle(m).await,
        #[cfg(not(feature = "music-ctl"))]
        Command::Music(_) => Err(ErrorResponse::RequestFailed(
            "music control is not enabled on this machine".into(),
        )),
        Command::Version => Ok(SuccessfulResponse::Version(
            env!("CARGO_PKG_VERSION").into(),
        )),
    }
}

pub fn reload() -> Result<impl FnOnce(), ErrorResponse> {
    static RELOADING: Mutex<()> = Mutex::new(());
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => return Err(ErrorResponse::RequestFailed(e.to_string())),
    };
    if RELOADING.try_lock().is_err() {
        return Err(ErrorResponse::RequestFailed("already reloading".into()));
    }
    Ok(move || {
        thread::sleep(Duration::from_secs(1));
        let _guard = RELOADING.lock().unwrap();
        tracing::info!("realoading spark daemon");
        let e = std::process::Command::new(exe).arg("daemon").exec();
        tracing::error!(?e, "exec self failed");
        if let Some(arg0) = std::env::args().next() {
            let e = std::process::Command::new(arg0).arg("daemon").exec();
            tracing::error!(?e, "exec arg0 failed");
        }
        drop(_guard)
    })
}
