#!/usr/bin/env sh
# Builds the deployable package: the release `v1per` binary with the built web app beside it
# in `web/`, which `v1per serve` finds without configuration. Bun is needed only here, at
# build time.
#
#   apps/server/scripts/package.sh [output-dir]    default: <target>/package/v1per
#
# V1PER_PACKAGE_FEATURES adds cargo features to the server build, e.g. `dev-clock` for
# browser journeys. Default: none, as for a release.
set -eu

root=$(cd "$(dirname "$0")/../../.." && pwd)
target=${CARGO_TARGET_DIR:-$root/target}
out=${1:-$target/package/v1per}
features=${V1PER_PACKAGE_FEATURES:-}

(cd "$root/apps/web" && bun install --frozen-lockfile && bun run build)
cargo build --release --locked --manifest-path "$root/Cargo.toml" -p v1per-server --bin v1per \
  ${features:+--features "$features"}

rm -rf "$out"
mkdir -p "$out"
cp "$target/release/v1per" "$out/v1per"
cp -R "$root/apps/web/dist" "$out/web"
echo "packaged $out"
