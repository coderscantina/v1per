//! `v1per-probe`: the remote probe agent (decision 0011). It checks the monitors the server
//! assigns to its probe and sends the results, and only connects out to the server.

mod agent;
mod buffer;
mod lock;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use tracing_subscriber::filter::Targets;
use url::Url;

use crate::buffer::Buffer;
use crate::lock::DataLock;

/// What `v1per probe` buffered in before decision 0011. This binary does not read it.
const LEGACY_BUFFER: &str = "buffer.sqlite";

/// Exit status when the server refuses the agent for good, as `v1per` uses it.
const EXIT_REFUSED: u8 = 3;
/// Exit status when the data directory cannot be used.
const EXIT_STORAGE: u8 = 4;

#[derive(Parser)]
#[command(
    name = "v1per-probe",
    version,
    about = "v1per remote probe agent: checks the monitors the server assigns to this probe"
)]
struct Args {
    /// The v1per server, e.g. `https://status.example.com`.
    #[arg(long, env = "V1PER_PROBE_SERVER")]
    server: Url,

    /// The token shown when the probe was registered. Prefer `V1PER_PROBE_TOKEN`: a flag shows
    /// in the process list.
    #[arg(long, env = "V1PER_PROBE_TOKEN", hide_env_values = true)]
    token: String,

    /// Directory for the result buffer. One agent at a time uses it.
    #[arg(long, env = "V1PER_PROBE_DATA_DIR", default_value = "probe-data")]
    data_dir: PathBuf,

    /// Largest result buffer on disk, in MiB. When the server stays unreachable longer than it
    /// holds, the oldest results are dropped and the drop is reported.
    #[arg(long, env = "V1PER_PROBE_BUFFER_MB", default_value_t = 64, value_parser = clap::value_parser!(u64).range(1..))]
    buffer_mb: u64,
}

fn main() -> ExitCode {
    let args = Args::parse();
    let filter = std::env::var("V1PER_LOG")
        .ok()
        .and_then(|value| value.parse::<Targets>().ok())
        .unwrap_or_else(|| Targets::new().with_default(tracing::Level::INFO));
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr())),
        )
        .with(filter)
        .init();

    let dir = &args.data_dir;
    let storage = |error: std::io::Error| {
        eprintln!("error: storage_unavailable: {}: {error}", dir.display());
        ExitCode::from(EXIT_STORAGE)
    };
    if let Err(error) = std::fs::create_dir_all(dir) {
        return storage(error);
    }
    let _lock = match DataLock::try_acquire(dir) {
        Ok(Some(lock)) => lock,
        Ok(None) => {
            eprintln!(
                "error: storage_unavailable: {}: another agent uses this data directory",
                dir.display()
            );
            return ExitCode::from(EXIT_STORAGE);
        }
        Err(error) => return storage(error),
    };
    if dir.join(LEGACY_BUFFER).exists() {
        tracing::warn!(
            file = %dir.join(LEGACY_BUFFER).display(),
            "found the buffer of an older agent; its undelivered results are not sent. Remove it once you no longer need it"
        );
    }
    let buffer = match Buffer::open(dir, args.buffer_mb * 1024 * 1024) {
        Ok(buffer) => buffer,
        Err(error) => return storage(error),
    };

    // One thread: checks wait on the network, and the agent stays small.
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("error: internal: {error}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(agent::run(
        args.server,
        args.token,
        buffer,
        shutdown_signal(),
    )) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: probe_refused: {message}");
            ExitCode::from(EXIT_REFUSED)
        }
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };
    tokio::select! { _ = ctrl_c => {}, _ = terminate => {} }
}
