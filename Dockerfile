# One-process v1per image: the release binary with the built web app beside it. The same
# image runs `v1per serve` (default) and `v1per probe`.
#
#   docker build --build-arg V1PER_GIT_SHA=$(git rev-parse --short HEAD) -t v1per .
#
# The data directory is /data. Run `v1per init` on it once before `serve`; see
# docs/guide/install.md.

FROM oven/bun:1.4 AS web
WORKDIR /src/apps/web
COPY apps/web/package.json apps/web/bun.lock ./
RUN bun install --frozen-lockfile
COPY apps/web ./
RUN bun run build

FROM rust:1.99-trixie AS server
# Lower it (--build-arg CARGO_BUILD_JOBS=2) on builders with little memory: DuckDB's C++
# compile needs a lot of memory per job.
ARG CARGO_BUILD_JOBS
# The release commit, shown by `v1per --version`.
ARG V1PER_GIT_SHA
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY apps/server apps/server
COPY crates crates
# The Kuma importer embeds Kuma's schema at compile time.
COPY fixtures/kuma-2.5.0/schema.sql fixtures/kuma-2.5.0/schema.sql
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p v1per-server --bin v1per \
    && cp target/release/v1per /v1per

FROM debian:trixie-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl tzdata \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home-dir /data v1per \
    && install -d -o v1per -g v1per /data /probe-data
# /probe-data is the probe agent's result buffer; mount a volume there to keep it.
# `serve` finds the web app in the `web` directory next to the executable.
COPY --from=server /v1per /opt/v1per/v1per
COPY --from=web /src/apps/web/dist /opt/v1per/web
ENV PATH=/opt/v1per:$PATH \
    V1PER_DATA_DIR=/data \
    V1PER_PROBE_DATA_DIR=/probe-data \
    V1PER_BIND=0.0.0.0:8080
USER v1per
VOLUME /data
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=30s \
    CMD curl -fsS http://127.0.0.1:8080/health || exit 1
ENTRYPOINT ["v1per"]
CMD ["serve"]
