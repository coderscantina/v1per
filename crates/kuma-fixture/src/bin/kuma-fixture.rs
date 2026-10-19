//! `kuma-fixture [--force] [--manifest <manifest.json>] <out.db>`
//!
//! Writes the synthetic Kuma 2.5.0 database to `<out.db>`. `--force` replaces an existing
//! file, `--manifest` also writes the manifest that describes it.

use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: kuma-fixture [--force] [--manifest <manifest.json>] <out.db>";

fn main() -> ExitCode {
    let mut force = false;
    let mut manifest = None;
    let mut out = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--force" => force = true,
            "--manifest" => match args.next() {
                Some(path) => manifest = Some(PathBuf::from(path)),
                None => return usage(),
            },
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            _ if out.is_none() && !arg.starts_with('-') => out = Some(PathBuf::from(arg)),
            _ => return usage(),
        }
    }
    let Some(out) = out else { return usage() };

    match run(&out, force, manifest) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("kuma-fixture: {error}");
            ExitCode::FAILURE
        }
    }
}

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

fn run(out: &PathBuf, force: bool, manifest: Option<PathBuf>) -> v1per_kuma_fixture::Result<()> {
    if force && out.exists() {
        std::fs::remove_file(out)?;
    }
    v1per_kuma_fixture::generate(out)?;
    if let Some(path) = manifest {
        let json = serde_json::to_string_pretty(&v1per_kuma_fixture::manifest(out)?)
            .expect("manifest is plain JSON");
        std::fs::write(path, json + "\n")?;
    }
    println!("wrote {}", out.display());
    Ok(())
}
