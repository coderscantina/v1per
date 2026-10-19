//! Sets `V1PER_VERSION`, the version `v1per --version` prints: the crate version, plus the
//! release commit when the build passes `V1PER_GIT_SHA` (the Docker image does), e.g.
//! `26.10.19 (010a9a6)`.

fn main() {
    println!("cargo:rerun-if-env-changed=V1PER_GIT_SHA");
    let version = std::env::var("CARGO_PKG_VERSION").expect("cargo sets CARGO_PKG_VERSION");
    let full = match std::env::var("V1PER_GIT_SHA") {
        Ok(sha) if !sha.is_empty() => format!("{version} ({sha})"),
        _ => version,
    };
    println!("cargo:rustc-env=V1PER_VERSION={full}");
}
