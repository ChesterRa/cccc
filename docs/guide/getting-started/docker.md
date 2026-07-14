# Docker Deployment

The image builds the React/TypeScript UI with Node, builds the backend with Rust, and copies only release binaries plus runtime dependencies into the final Debian image. It contains no Python backend.

## Quick Start

```bash
docker volume create cccc-data
docker compose -f docker/docker-compose.yml up --build
```

Open <http://127.0.0.1:8848>.

## Configuration

Copy the example and set the workspace path and any agent credentials:

```bash
cp docker/.env.example docker/.env
```

Important variables:

| Variable | Container value | Purpose |
|---|---|---|
| `CCCC_RUST_HOME` | `/data` | Rust state volume |
| `CCCC_WEB_HOST` | `0.0.0.0` | Container Web bind |
| `CCCC_WEB_PORT` | `8848` | Container Web port |
| `CCCC_DAEMON_TRANSPORT` | `tcp` | Container daemon transport |
| `WORKSPACE_PATH` | host-defined | Project mounted at `/workspace` |
| `CCCC_PORT` | `8848` | Host loopback port |

The Compose file publishes only `127.0.0.1:${CCCC_PORT}` by default. Create an administrator access token before exposing it through a tunnel or reverse proxy.

## Direct Docker

```bash
docker build -f docker/Dockerfile -t cccc:rust .
docker run --rm -it \
  -p 127.0.0.1:8848:8848 \
  -v cccc-data:/data \
  -v "$PWD:/workspace" \
  cccc:rust
```

## Daily Operations

```bash
docker compose -f docker/docker-compose.yml logs -f cccc
docker compose -f docker/docker-compose.yml restart cccc
docker compose -f docker/docker-compose.yml down
docker compose -f docker/docker-compose.yml up -d --build
```

## Backup

Stop the service before copying the volume:

```bash
docker compose -f docker/docker-compose.yml down
docker run --rm -v cccc-data:/data -v "$PWD:/backup" \
  debian:bookworm-slim tar -C /data -czf /backup/cccc-rust-data.tar.gz .
```

The archive must include `/data/.cccc-rust-v1`.

## Troubleshooting

```bash
docker compose -f docker/docker-compose.yml ps
docker compose -f docker/docker-compose.yml logs --tail=200 cccc
docker compose -f docker/docker-compose.yml exec cccc cccc doctor
docker compose -f docker/docker-compose.yml exec cccc cccc runtime list
```

If the workspace is not visible, verify `WORKSPACE_PATH`. If `/data` is not writable, fix ownership for UID 1000 on the host volume. Do not mount an existing legacy `~/.cccc` directory at `/data`.
