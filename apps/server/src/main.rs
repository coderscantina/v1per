use std::future::IntoFuture;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, Subcommand};
use tokio::sync::watch;
use tracing_subscriber::EnvFilter;
use v1per_server::background::Background;
use v1per_server::config::{self, Config, SECRETS_KEY_ENV, SecretsArgs, ServeArgs};
use v1per_server::db::Db;
use v1per_server::identity::secrets::SecretKey;
use v1per_server::imports::{self, Outcome};
use v1per_server::init::{self, Exit, Failure, InitArgs};
use v1per_server::lock::DataLock;
use v1per_server::maintenance::commands::{
    self as maintenance, BackupArgs, DiagnosticsArgs, RestoreArgs, RetentionArgs, UpgradeArgs,
};
use v1per_server::maintenance::versions;
use v1per_server::probes::agent::{self, ProbeArgs};
use v1per_server::{AppState, router};
use v1per_storage::{Budgets, Storage};

/// How long open requests may take to finish after a stop signal.
const DRAIN: Duration = Duration::from_secs(10);
/// How long admitted heartbeats may take to reach DuckDB at shutdown. What is left stays in
/// the durable inbox and moves at the next start.
const STORAGE_DRAIN: Duration = Duration::from_secs(10);

#[derive(Parser)]
#[command(
    name = "v1per",
    version,
    about = "Self-hosted uptime monitoring and status pages"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Creates a data directory with the instance admin and the secrets key.
    Init {
        #[arg(long, env = "V1PER_DATA_DIR")]
        data_dir: PathBuf,
        #[arg(long)]
        admin_email: String,
        /// File holding the admin password. Without it a password is generated and printed once.
        #[arg(long)]
        admin_password_file: Option<PathBuf>,
        #[command(flatten)]
        secrets: SecretsArgs,
    },
    /// Serves the admin app, its API and the live event streams.
    Serve(ServeArgs),
    /// Runs a remote probe agent: checks the monitors the server assigns to this probe and
    /// sends the results. It only connects out to the server.
    Probe(ProbeArgs),
    /// Imports from another monitoring tool.
    Import {
        #[command(subcommand)]
        command: imports::Command,
    },
    /// Writes a verified backup archive. While `serve` runs, the server takes it with a short
    /// pause of heartbeat transfer; checks and admission go on.
    Backup(BackupArgs),
    /// Restores a backup into a new, empty data directory.
    Restore(RestoreArgs),
    /// Removes heartbeats and records past their retention now. `serve` runs it hourly.
    Retention(RetentionArgs),
    /// Prints versions, storage, inbox, probe, outbox, retention and import health.
    Diagnostics(DiagnosticsArgs),
    /// Backs up a stopped data directory, then migrates it to this release.
    Upgrade(UpgradeArgs),
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("V1PER_LOG")
                .unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn")),
        )
        .with_writer(std::io::stderr)
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
        .init();

    let workers = match &cli.command {
        Command::Serve(args) => args.worker_threads.max(1),
        Command::Init { .. } => 1,
        _ => 2,
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(workers)
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            return fail(Failure::new(Exit::Internal, "internal", error.to_string()));
        }
    };
    let result = runtime.block_on(async {
        match cli.command {
            Command::Init {
                data_dir,
                admin_email,
                admin_password_file,
                secrets,
            } => run_init(data_dir, admin_email, admin_password_file, secrets).await,
            Command::Serve(args) => serve(args).await,
            Command::Probe(args) => agent::run(args, shutdown_signal()).await,
            Command::Import {
                command: imports::Command::Kuma(args),
            } => run_import(args).await,
            Command::Backup(args) => {
                print(maintenance::backup(args, secrets_key().as_deref())).await
            }
            Command::Restore(args) => {
                print(maintenance::restore(args, secrets_key().as_deref())).await
            }
            Command::Retention(args) => {
                print(maintenance::retention(args, secrets_key().as_deref())).await
            }
            Command::Diagnostics(args) => {
                print(maintenance::diagnostics(args, secrets_key().as_deref())).await
            }
            Command::Upgrade(args) => {
                print(maintenance::upgrade(args, secrets_key().as_deref())).await
            }
        }
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => fail(failure),
    }
}

/// Prints what an operator command reports on stdout.
async fn print(report: impl Future<Output = Result<String, Failure>>) -> Result<(), Failure> {
    print!("{}", report.await?);
    Ok(())
}

fn fail(failure: Failure) -> ExitCode {
    eprintln!("error: {}: {}", failure.code, failure.message);
    ExitCode::from(failure.exit as u8)
}

/// `V1PER_SECRETS_KEY` from the environment only, so the key never shows in the process list.
fn secrets_key() -> Option<String> {
    std::env::var(SECRETS_KEY_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

async fn run_init(
    data_dir: PathBuf,
    admin_email: String,
    password_file: Option<PathBuf>,
    secrets: SecretsArgs,
) -> Result<(), Failure> {
    let admin_password = match password_file {
        Some(path) => Some(
            std::fs::read_to_string(&path)
                .map(|text| text.trim_end_matches(['\n', '\r']).to_owned())
                .map_err(|e| {
                    Failure::new(Exit::Usage, "usage", format!("{}: {e}", path.display()))
                })?,
        ),
        None => None,
    };
    let done = init::init(InitArgs {
        data_dir: data_dir.clone(),
        admin_email,
        admin_password,
        secrets_key: secrets_key(),
        secrets_key_file: secrets.secrets_key_file,
    })
    .await?;
    eprintln!("initialized {}", data_dir.display());
    if let Some(password) = done.generated_password {
        // stdout only, once. Logs and stderr never see it.
        println!("{password}");
    }
    Ok(())
}

async fn run_import(args: imports::KumaArgs) -> Result<(), Failure> {
    match imports::run(args, secrets_key().as_deref()).await? {
        Outcome::DryRun(report) => print!("{report}"),
        Outcome::Imported { manifest, path } => {
            print!("{}", imports::summary(&manifest, &path));
            if !manifest.reconciled {
                return Err(Failure::new(
                    Exit::Internal,
                    "not_reconciled",
                    format!("the import differs from its source; see {}", path.display()),
                ));
            }
        }
    }
    Ok(())
}

/// Serves until SIGTERM or Ctrl-C, then stops in order: no new connections, live streams end,
/// open requests get [`DRAIN`] to finish, checks stop and the evaluator records what is queued,
/// heartbeat storage drains its inbox for up to [`STORAGE_DRAIN`] and closes, then the control
/// database closes.
async fn serve(args: ServeArgs) -> Result<(), Failure> {
    let usage = |e: anyhow::Error| Failure::new(Exit::Usage, "usage", e.to_string());
    let clock = config::clock(args.frozen_clock.as_deref()).map_err(usage)?;
    let config = Config::from_args(&args).map_err(usage)?;
    let dir = config.data_dir.clone();
    init::check_initialized(&dir)?;
    let _lock = DataLock::acquire(&dir)?;
    versions::check_dir(&dir, args.migrate).await?;
    let secrets = SecretKey::load(
        &dir,
        secrets_key().as_deref(),
        args.secrets.secrets_key_file.as_deref(),
    )
    .map_err(|e| Failure::new(Exit::Refused, "secrets_key", e.to_string()))?;
    let db = Db::open(&dir)
        .await
        .map_err(|e| Failure::storage(&dir, e))?;
    imports::refuse_while_running(&db).await?;
    let storage = Storage::open(&dir, Budgets::default())
        .await
        .map(Arc::new)
        .map_err(|e| Failure::storage(&dir, e))?;

    match &config.web_dir {
        Some(dir) => tracing::info!(dir = %dir.display(), "serving the web app"),
        None => tracing::warn!("no web app found; set V1PER_WEB_DIR to serve the admin app"),
    }
    if config.public_origin.is_none() {
        tracing::warn!(
            "V1PER_PUBLIC_URL is not set: commands are checked against the Host header and the session cookie is not marked Secure"
        );
    }
    if clock.is_test() {
        tracing::warn!(
            now_ms = clock.now_ms(),
            "the application clock is frozen; development builds only"
        );
    }

    let bind = config.bind;
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .map_err(|e| Failure::new(Exit::Internal, "bind_failed", format!("{bind}: {e}")))?;
    let state = AppState::new(db.clone(), config, clock, secrets, storage.clone());
    let background = Background::start(&state);
    tracing::info!(addr = %bind, version = env!("CARGO_PKG_VERSION"), "v1per listening");

    let (stop, stopped) = watch::channel(false);
    let signalled = state.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        // Live streams never end by themselves; end them so draining waits only for requests.
        signalled.hub.close();
        let _ = stop.send(true);
    });
    let until_stopped = |mut stopped: watch::Receiver<bool>| async move {
        let _ = stopped.wait_for(|stop| *stop).await;
    };
    let server = axum::serve(
        listener,
        router(state.clone()).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(until_stopped(stopped.clone()))
    .into_future();
    let served = tokio::select! {
        result = server => result.map_err(|e| Failure::new(Exit::Internal, "internal", e.to_string())),
        _ = async {
            until_stopped(stopped).await;
            tokio::time::sleep(DRAIN).await;
        } => {
            tracing::warn!("requests still open {DRAIN:?} after the stop signal; closing them");
            Ok(())
        }
    };

    tracing::info!("stopping background work");
    background.stop().await;
    drop(state);
    if tokio::time::timeout(STORAGE_DRAIN, storage.drain())
        .await
        .is_err()
    {
        tracing::warn!("heartbeats still in the inbox; they move to DuckDB at the next start");
    }
    if let Err(error) = storage.shutdown().await {
        tracing::error!(%error, "closing heartbeat storage failed");
    }
    db.close().await;
    tracing::info!("stopped");
    served
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = terminate => {} }
}
