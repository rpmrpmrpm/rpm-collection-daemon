//! `rpm-collection-daemon --config <file>`. See the README for the settings.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use rpm_daemon::{config, logger, supervisor::Supervisor};
use rpm_peer::beacon::Announcer;
use tokio_util::sync::CancellationToken;

const USAGE: &str = "Usage: rpm-collection-daemon [--config <file>]

Serves the rpm library folders under the configured root to the rpm apps on the
local network. Settings come from the file and from RPM_ROOT, RPM_LIMIT,
RPM_NAME, RPM_DATA, RPM_SMB_SHARE and RPM_PORT, which win. RPM_LOG sets the log
level (error, warn, info, debug).";

fn main() -> ExitCode {
    let mut config_path: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" | "-c" => match args.next() {
                Some(path) => config_path = Some(PathBuf::from(path)),
                None => {
                    eprintln!("--config needs a file\n\n{USAGE}");
                    return ExitCode::from(2);
                }
            },
            "--version" | "-V" => {
                println!("rpm-collection-daemon {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            "--help" | "-h" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("unknown argument {other}\n\n{USAGE}");
                return ExitCode::from(2);
            }
        }
    }

    logger::init(std::env::var("RPM_LOG").ok().as_deref());

    let config = match config::load(config_path.as_deref(), &|name| std::env::var(name).ok()) {
        Ok(config) => config,
        Err(error) => {
            log::error!("{error}");
            return ExitCode::from(78);
        }
    };

    // A NAS has few cores and little memory. Two worker threads serve many readers, because almost
    // all of the work is waiting on the disk and the network.
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            log::error!("the runtime could not start: {error}");
            return ExitCode::FAILURE;
        }
    };

    runtime.block_on(async move {
        let announcer = match Announcer::new(&config::host_name()) {
            Ok(announcer) => Some(announcer),
            Err(error) => {
                log::warn!("multicast DNS could not start, so rpm will not find this NAS: {error}");
                None
            }
        };
        let supervisor = match Supervisor::new(config, announcer) {
            Ok(supervisor) => supervisor,
            Err(error) => {
                log::error!("{error}");
                return ExitCode::FAILURE;
            }
        };
        if let Err(error) = supervisor.start().await {
            log::error!("the NAS could not start listening: {error}");
            return ExitCode::FAILURE;
        }
        let stop = CancellationToken::new();
        let running = tokio::spawn(supervisor.clone().run(stop.clone()));
        wait_for_stop().await;
        log::info!("stopping");
        stop.cancel();
        let _ = running.await;
        // Give multicast DNS a moment to send the goodbye for each announcement.
        tokio::time::sleep(Duration::from_millis(300)).await;
        ExitCode::SUCCESS
    })
}

async fn wait_for_stop() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let Ok(mut terminate) = signal(SignalKind::terminate()) else {
            let _ = tokio::signal::ctrl_c().await;
            return;
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
