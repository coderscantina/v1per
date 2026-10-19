# Install

v1per ships as one Docker image, `ghcr.io/coderscantina/v1per`, for linux/amd64 and linux/arm64. The same image runs the server (`v1per serve`, the default) and remote probe agents (`v1per probe`). It needs no database server, queue or Node runtime.

## Image tags

| Tag | Use |
| --- | --- |
| `:latest` | The newest release |
| `:YY.M.D`, for example `:26.10.19` | A release by date. Pin this in production |
| `:YY.M.D-<shorthash>`, for example `:26.10.19-010a9a6` | The same release, named with its commit |

The image runs as a non-root user (uid 10001), keeps its data in the `/data` volume and listens on port 8080. It has a healthcheck on `/health`.

## Install with Docker

Create the data directory once. This creates the instance admin and the secrets key, and prints the admin password a single time:

```sh
docker run --rm -v v1per-data:/data ghcr.io/coderscantina/v1per:latest \
  init --admin-email you@example.com
```

Copy the password now. It is stored nowhere in clear text and is never printed again. To choose the password yourself, mount a file holding it and pass `--admin-password-file`. Passwords need 12 to 256 characters.

Then start the server:

```sh
docker run -d --name v1per --restart unless-stopped --stop-timeout 35 \
  -v v1per-data:/data -p 127.0.0.1:8080:8080 \
  -e V1PER_PUBLIC_URL=https://status.example.com \
  ghcr.io/coderscantina/v1per:latest
```

Give the container 30 seconds or more to stop. On SIGTERM the server finishes open requests and moves queued results into storage before it exits.

`init` refuses a data directory that already holds data (`directory_not_empty`) or one it already set up (`already_initialized`). `serve` refuses a directory `init` has not finished (`not_initialized`).

## Install with docker compose

[deploy/docker-compose.yml](../../deploy/docker-compose.yml) runs the server and has an optional probe service:

```sh
export V1PER_PUBLIC_URL=https://status.example.com
docker compose run --rm v1per init --admin-email you@example.com   # once, prints the password
docker compose up -d
```

The compose file publishes port 8080 on `127.0.0.1` only, for a reverse proxy on the same host. Set `V1PER_IMAGE` to pin a release tag, and `V1PER_INSTANCE_NAME` to name the instance.

The probe service belongs on another host in practice. See [Probes](probes.md#run-an-agent).

## Install on Coolify

[deploy/coolify/docker-compose.yml](../../deploy/coolify/docker-compose.yml) is the Coolify variant.

1. Create a Docker Compose resource from the file.
2. Set `V1PER_IMAGE` to a release, for example `ghcr.io/coderscantina/v1per:26.10.19`, and `V1PER_PUBLIC_URL` to the admin address.
3. Set the domain of the `v1per` service to the admin host with the container port, for example `https://status.example.com:8080`.
4. Run `v1per init --admin-email you@example.com` once on the data volume before the first start, for example from the service's terminal or with a one-off container on the same volume.
5. Deploy.

Custom status page domains are added to the same service as extra domains. Coolify's Traefik passes the Host header through, which is all v1per needs.

## Reverse proxy and custom domains

Run TLS at a reverse proxy. v1per serves plain HTTP and picks a status page by the `Host` header, so the proxy must pass the original Host through unchanged.

[deploy/Caddyfile](../../deploy/Caddyfile) is an example. Caddy passes the Host header unchanged and gets a certificate for each listed domain:

```
status.example.com {
	reverse_proxy 127.0.0.1:8080
}

status.customer.example {
	reverse_proxy 127.0.0.1:8080
}
```

To add a custom domain for a status page:

1. Add the hostname in the page's Domains tab in the admin app.
2. Point the domain's DNS record at the proxy.
3. Add the domain to the proxy so it gets a certificate. With Caddy, add a block like the second one above and reload Caddy.

Other proxies:

- **nginx** needs `proxy_set_header Host $host;` and, because live updates use server-sent events, `proxy_buffering off;`.
- **Traefik and Coolify** pass the Host header by default.
- **Caddy** does not buffer server-sent events, so it needs nothing extra.

Set `V1PER_PUBLIC_URL` to the admin address behind the proxy. Commands from the admin app must come from that origin, and an `https` URL marks the session cookie `Secure`. Without it the server logs a warning at start. See [Configuration](configuration.md).

## First sign-in

1. Open the public URL, for example `https://status.example.com`. The admin app shows the sign-in page.
2. Sign in with the email you passed to `init` and the printed password.
3. Open your account and set up two-factor authentication.
4. In the instance settings, check the instance name, the public URL and the timezone, and set up the mail server. Email channels and status page sign-in links need it.

The instance admin owns the root node. From there, create nodes for your clients or teams, add monitors and invite people. See [Status pages](status-pages.md#nodes-roles-and-invites) for how roles and invites scope what people see.

## Build from source

Docker is the supported way to run v1per. To build the image yourself from a checkout:

```sh
docker build --build-arg V1PER_GIT_SHA=$(git rev-parse --short HEAD) -t v1per .
```

`cargo build --release -p v1per-server --bin v1per` builds the binary alone. It serves the admin app from a `web` directory next to the executable, or from `--web-dir`, which you build with Bun in `apps/web`.
