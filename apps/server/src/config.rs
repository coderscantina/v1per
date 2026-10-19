//! Runtime settings, from command-line flags or `V1PER_*` environment variables.

use std::net::SocketAddr;
use std::path::PathBuf;

use clap::Args;

use crate::clock::Clock;

/// Environment variable holding the secrets key itself: base64 of 32 bytes. Read from the
/// environment only, never a flag, so the key does not show in the process list.
pub const SECRETS_KEY_ENV: &str = "V1PER_SECRETS_KEY";

#[derive(Clone, Args)]
pub struct ServeArgs {
    /// Persistent data directory created by `v1per init`.
    #[arg(long, env = "V1PER_DATA_DIR", default_value = "data")]
    pub data_dir: PathBuf,

    /// Address to listen on.
    #[arg(long, env = "V1PER_BIND", default_value = "127.0.0.1:8080")]
    pub bind: SocketAddr,

    /// Built web app to serve. Defaults to a `web` directory next to the executable.
    #[arg(long, env = "V1PER_WEB_DIR")]
    pub web_dir: Option<PathBuf>,

    /// Public URL the admin app is reached at, e.g. `https://status.example.com`. Commands
    /// must come from this origin, and an https URL marks the session cookie `Secure`. Without
    /// it, commands must come from the origin named by the request's Host header.
    #[arg(long, env = "V1PER_PUBLIC_URL")]
    pub public_url: Option<String>,

    /// Name of this instance, shown in the admin app. Without it, the name the instance admin
    /// sets in the app, or `v1per`.
    #[arg(long, env = "V1PER_INSTANCE_NAME")]
    pub instance_name: Option<String>,

    /// Tokio worker threads. The reference host has two vCPUs.
    #[arg(long, env = "V1PER_WORKER_THREADS", default_value_t = 2)]
    pub worker_threads: usize,

    #[command(flatten)]
    pub secrets: SecretsArgs,

    /// Applies pending schema migrations at start. Without it `serve` refuses data of an
    /// older release; `v1per upgrade` migrates with a backup first.
    #[arg(long, env = "V1PER_MIGRATE")]
    pub migrate: bool,

    /// Runs on an application clock frozen at this RFC 3339 instant, e.g.
    /// `2026-10-13T15:20:00+02:00`, for browser journeys on seeded data. Only builds with the
    /// `dev-clock` feature accept it; see [`clock`].
    #[arg(long, env = "V1PER_FROZEN_CLOCK")]
    pub frozen_clock: Option<String>,
}

/// Where the key that seals secrets comes from (decision 0006). `V1PER_SECRETS_KEY` wins, then
/// this file, then `secrets.key` in the data directory.
#[derive(Clone, Args)]
pub struct SecretsArgs {
    /// File holding the secrets key: 32 raw bytes or their base64, e.g. from
    /// `openssl rand -base64 32`. Without it and without `V1PER_SECRETS_KEY`, the key is
    /// `secrets.key` in the data directory.
    #[arg(long, env = "V1PER_SECRETS_KEY_FILE")]
    pub secrets_key_file: Option<PathBuf>,
}

/// The clock `serve` runs on: the system clock, or one frozen at `frozen`. A build without the
/// `dev-clock` feature refuses a frozen clock, so a packaged release always runs on real time.
pub fn clock(frozen: Option<&str>) -> anyhow::Result<Clock> {
    let Some(instant) = frozen else {
        return Ok(Clock::system());
    };
    anyhow::ensure!(
        cfg!(feature = "dev-clock"),
        "this build does not support a frozen clock (--frozen-clock, V1PER_FROZEN_CLOCK); build with the dev-clock feature"
    );
    let at = chrono::DateTime::parse_from_rfc3339(instant.trim())
        .map_err(|e| anyhow::anyhow!("--frozen-clock {instant}: not an RFC 3339 instant: {e}"))?;
    Ok(Clock::frozen(at.timestamp_millis()))
}

#[derive(Debug, Clone)]
pub struct Config {
    pub data_dir: PathBuf,
    pub bind: SocketAddr,
    pub web_dir: Option<PathBuf>,
    /// `scheme://host[:port]` without a trailing slash.
    pub public_origin: Option<String>,
    /// Fixes the instance name over the stored setting.
    pub instance_name: Option<String>,
}

impl Config {
    pub fn from_args(args: &ServeArgs) -> anyhow::Result<Self> {
        let public_origin = args.public_url.as_deref().map(origin_of).transpose()?;
        let web_dir = args.web_dir.clone().or_else(|| {
            let beside = std::env::current_exe().ok()?.parent()?.join("web");
            beside.join("index.html").is_file().then_some(beside)
        });
        Ok(Self {
            data_dir: args.data_dir.clone(),
            bind: args.bind,
            web_dir,
            public_origin,
            instance_name: args.instance_name.clone(),
        })
    }

    pub fn secure_cookies(&self) -> bool {
        self.public_origin
            .as_deref()
            .is_some_and(|origin| origin.starts_with("https://"))
    }
}

/// `https://example.com/status/` → `https://example.com`.
pub fn origin_of(url: &str) -> anyhow::Result<String> {
    let url = url.trim();
    let (scheme, rest) = url
        .split_once("://")
        .filter(|(scheme, _)| matches!(*scheme, "http" | "https"))
        .ok_or_else(|| anyhow::anyhow!("V1PER_PUBLIC_URL must start with http:// or https://"))?;
    let host = rest.split('/').next().unwrap_or_default();
    anyhow::ensure!(!host.is_empty(), "V1PER_PUBLIC_URL has no host");
    Ok(format!("{scheme}://{}", host.to_lowercase()))
}
