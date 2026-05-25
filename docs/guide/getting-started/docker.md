# Docker Deployment

Run CCCC in a Docker container — ideal for servers, teams, and reproducible environments.

## AI-Assisted Deployment

Copy the prompt below and paste it to any AI assistant — it will guide you through the entire deployment interactively.

::: details Click to copy AI deployment prompt

```text
You are a deployment assistant for CCCC (Multi-Agent Collaboration Kernel).
Guide the user step-by-step through Docker deployment. Ask questions interactively, don't dump all steps at once.

## What you're deploying
CCCC is a multi-agent collaboration hub. The Docker image includes Python 3.11, Node.js 20,
and pre-installed AI agent CLIs (Claude Code, Gemini CLI, Codex CLI, Factory CLI).

## Step 1: Get the source code
Ask: "Do you already have the CCCC repo cloned? If yes, what's the path?"
If no:
  git clone https://github.com/ChesterRa/cccc && cd cccc

## Step 2: Build the image
  docker build -f docker/Dockerfile -t cccc .
Note: multi-stage build — first compiles Web UI (Node.js), then packages Python daemon.
If build fails, check: Docker version >= 20.10, sufficient disk space, network access to npm/PyPI.

## Step 3: Collect user config
Ask each one individually:
1. "What port do you want the Web UI on? (default: 8848)"
2. "Where are your project files? (absolute path, will be mounted to /workspace)"
3. "Which AI agent API keys do you have? (ANTHROPIC_AUTH_TOKEN / OPENAI_API_KEY / GEMINI_API_KEY)"
4. "Will this stay localhost-only until you create an Admin Access Token in Web Access?"

## Step 4: Run the container
Build the docker run command from the user's answers:
  docker run -d \
    -p 127.0.0.1:{port}:8848 \
    -v cccc-data:/data \
    -v {project_path}:/workspace \
    -e {API_KEY_ENV}={api_key} \
    --name cccc \
    cccc

## Step 5: Verify
Run these and report results:
  docker logs cccc
  docker exec cccc cccc doctor

## Troubleshooting knowledge (use when relevant, don't preemptively dump):
- "cannot be used with root/sudo privileges": The Dockerfile uses a non-root `cccc` user. Ensure using the latest Dockerfile.
- Volume permission errors after upgrading: `docker run --rm -v cccc-data:/data python:3.11-slim chown -R 1000:1000 /data`
- Claude CLI onboarding already pre-configured via: `{"hasCompletedOnboarding":true}` in /home/cccc/.claude.json
- Custom Claude CLI config: `docker exec cccc sh -c 'cat > /home/cccc/.claude.json << EOF\n{your json}\nEOF'`
- Check runtime CLIs: `docker exec cccc claude --version` / `gemini --version` / `codex --version`

## Optional: Docker Compose
If user prefers Compose, point them to the bundled docker/docker-compose.yml:
  cp docker/.env.example docker/.env
  # Edit docker/.env with port, API keys, workspace path, proxy, then create an Admin Access Token in Web Access before non-local exposure.
  # From project root:
  docker compose --env-file docker/.env -f docker/docker-compose.yml up -d --build
  # Or from docker directory:
  cd docker && docker compose up -d --build

## Key environment variables reference:
| Variable | Default | Description |
|----------|---------|-------------|
| CCCC_HOME | /data | Data directory |
| CCCC_WEB_HOST | 0.0.0.0 | Web bind address inside the container |
| CCCC_WEB_PORT | 8848 | Web port |
| CCCC_DAEMON_TRANSPORT | tcp | IPC transport |
| CCCC_DAEMON_HOST | 127.0.0.1 | Daemon bind address |
| CCCC_DAEMON_PORT | 9765 | Daemon IPC port |
| ANTHROPIC_AUTH_TOKEN | (none) | Auth token for Claude |
| OPENAI_API_KEY | (none) | API key for Codex runtime |
| GEMINI_API_KEY | (none) | API key for Gemini CLI runtime |

## Tone: concise, practical, one step at a time. Confirm each step succeeds before moving on.
```

:::

## Prerequisites

- [Docker](https://docs.docker.com/get-docker/) installed (20.10+)
- At least one AI agent API key (e.g. `ANTHROPIC_AUTH_TOKEN` for Claude)

## Quick Start

### 1. Build the Image

```bash
git clone https://github.com/ChesterRa/cccc
cd cccc
docker build -f docker/Dockerfile -t cccc .
```

::: tip Build Context
The build uses a multi-stage approach: first compiles the Web UI (Node.js), then packages the Python daemon with pre-installed AI agent CLIs (Claude Code, Gemini CLI, Codex CLI).
:::

### 2. Run the Container

```bash
docker run -d \
  --init \
  -p 127.0.0.1:8848:8848 \
  -v cccc-data:/data \
  -v /path/to/your/projects:/workspace \
  -e ANTHROPIC_AUTH_TOKEN=sk-ant-xxx \
  --name cccc \
  cccc
```

Open `http://localhost:8848` in your browser. The sample command binds the host port to `127.0.0.1` on purpose, so you can safely create an **Admin Access Token** in **Settings > Web Access** before any broader exposure.

The image now includes the minimal shared libraries plus `Xvfb` needed for projected browser surfaces such as NotebookLM sign-in and Presentation browser sessions. Playwright and Chromium binaries are still installed lazily on first use instead of being pre-baked into the image.

### 3. Verify

```bash
# Check container is running
docker logs cccc

# Health check
docker exec cccc cccc doctor
```

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `CCCC_HOME` | `/data` | Data directory (groups, ledger, config) |
| `CCCC_WEB_HOST` | `0.0.0.0` | Web server bind address inside the container |
| `CCCC_WEB_PORT` | `8848` | Web server port |
| `CCCC_DAEMON_TRANSPORT` | `tcp` | Daemon IPC transport (`tcp` or `unix`) |
| `CCCC_DAEMON_HOST` | `127.0.0.1` | Daemon bind address |
| `CCCC_DAEMON_PORT` | `9765` | Daemon IPC port |
| `ANTHROPIC_AUTH_TOKEN` | _(none)_ | Auth token for Claude Code runtime (do not set together with `ANTHROPIC_API_KEY`) |
| `ANTHROPIC_BASE_URL` | _(none)_ | Custom API endpoint for Claude Code |
| `OPENAI_API_KEY` | _(none)_ | API key for Codex runtime |
| `OPENAI_BASE_URL` | _(none)_ | CCCC compatibility entry for Codex custom endpoints. CCCC maps this to Codex CLI's `openai_base_url` runtime config when launching Codex actors. |
| `GEMINI_API_KEY` | _(none)_ | API key for Gemini CLI runtime |

### Volume Mounts

| Container Path | Purpose |
|---------------|---------|
| `/data` | Persistent CCCC state (groups, ledger, daemon config) |
| `/workspace` | Project files for agents to work on |

::: warning Protect Your Data
Always mount `/data` to a named volume or host path to persist state across container restarts.
:::

## Advanced Usage

### Expose Daemon IPC for SDK Access

If you need to access the daemon IPC from outside the container (e.g. for SDK integration):

```bash
docker run -d \
  --init \
  -p 127.0.0.1:8848:8848 \
  -p 127.0.0.1:9765:9765 \
  -v cccc-data:/data \
  -v /path/to/projects:/workspace \
  -e CCCC_DAEMON_HOST=0.0.0.0 \
  -e CCCC_DAEMON_ALLOW_REMOTE=1 \
  --name cccc \
  cccc
```

Projected browser sessions now default to a headed browser for better site compatibility. In server/container environments without a native display, CCCC uses its own `Xvfb` display automatically. The image also includes `x11vnc` so embedded browser panels can use the VNC-backed viewer when the browser is running on that CCCC-owned display, while delivery and automation still use the daemon-owned CDP session. CCCC does not attach VNC to an inherited host desktop display; those sessions use the CDP screencast fallback instead. The VNC endpoint is localhost-only inside the container and is intended for trusted single-user deployments; browser access from CCCC Web is proxied through the authenticated WebSocket route. If you use projected browser features heavily and see Chromium renderer crashes inside the container, add `--ipc=host` to the `docker run` command. Playwright's Docker guidance recommends a larger shared-memory budget for Chromium workloads.

### Custom Claude CLI Configuration

The container comes with Claude CLI pre-configured (onboarding skipped). To customize further:

```bash
# Write config from host
docker exec cccc sh -c 'cat > /home/cccc/.claude.json << EOF
{
  "hasCompletedOnboarding": true,
  "customApiKey": "your-key"
}
EOF'

# Or copy a config file in
docker cp ~/.claude.json cccc:/home/cccc/.claude.json
```

### Run with Docker Compose

The repo ships a ready-to-use `docker/docker-compose.yml`. First copy and edit the env file:

```bash
cp docker/.env.example docker/.env
# Edit docker/.env — set API keys, workspace path, etc. Then create an Admin Access Token in Web Access before non-local exposure.
```

Create the data volume (required on first run, as the compose file uses `external: true`):

```bash
docker volume create cccc-data
```

Then choose either way to start:

```bash
# Option A: Run from project root (recommended)
docker compose --env-file docker/.env -f docker/docker-compose.yml up -d --build

# Option B: Run from docker directory
cd docker
docker compose up -d --build
```

::: info First Run vs Update
`--build` builds the image from source. On subsequent runs, you can omit it if the image hasn't changed — `docker compose up -d` will reuse the existing image.
:::

The `.env` file controls ports, volumes, API keys, and build-time proxy. See `docker/.env.example` for all options.

The bundled Compose file already enables `init: true`, which helps reap short-lived browser helper processes cleanly.

::: tip Build Behind a Proxy
Set `HTTP_PROXY` and `HTTPS_PROXY` in `.env` to pass proxy settings during `docker compose build`. Both build stages (Node.js and Python) will use the proxy for `curl`, `npm`, and `pip`.
:::

#### Daily Operations

```bash
# Update deployment (build + restart)
docker compose --env-file docker/.env -f docker/docker-compose.yml up -d --build

# Force full rebuild (no cache)
docker compose --env-file docker/.env -f docker/docker-compose.yml up -d --build --no-cache

# View logs
docker compose --env-file docker/.env -f docker/docker-compose.yml logs -f

# Stop
docker compose --env-file docker/.env -f docker/docker-compose.yml down
```

### K8s Sidecar Pattern

For Kubernetes deployments, the daemon defaults to `127.0.0.1:9765` — suitable for sidecar containers sharing the same Pod network namespace. No extra configuration needed for intra-Pod communication.

Security note: the Docker examples now bind host ports to `127.0.0.1` by default. Keep that local-only posture until you have created an Admin Access Token in Web Access and intentionally chosen a remote boundary.

## Troubleshooting

### "cannot be used with root/sudo privileges"

Claude CLI refuses to run with `--dangerously-skip-permissions` as root. The Dockerfile already creates a non-root `cccc` user to handle this. If you see this error, make sure you're using the latest Dockerfile.

### Volume Permission Issues

If you previously ran the container as root and then switched to the non-root user, existing volume data may have root ownership:

```bash
# Fix permissions on the data volume
docker run --rm -v cccc-data:/data python:3.11-slim \
  chown -R 1000:1000 /data
```

### Agent CLI Not Found

The image ships with Claude Code, Gemini CLI, and Codex CLI pre-installed. If a runtime isn't detected:

```bash
# Check available runtimes
docker exec cccc cccc doctor

# Verify CLI availability
docker exec cccc claude --version
docker exec cccc gemini --version
docker exec cccc codex --version
```

### Container Logs

```bash
# Real-time logs
docker logs -f cccc

# Last 100 lines
docker logs --tail 100 cccc
```

## Pre-installed Tools

The Docker image includes:

| Tool | Purpose |
|------|---------|
| Python 3.11 | CCCC daemon runtime |
| Node.js 20 | Agent CLI runtime (npm-based tools) |
| [Claude Code](https://docs.anthropic.com/en/docs/claude-code) | Anthropic's AI coding agent |
| [Gemini CLI](https://github.com/google/gemini-cli) | Google's AI coding agent |
| [Codex CLI](https://github.com/openai/codex) | OpenAI's AI coding agent |
| [Factory CLI](https://www.factory.ai/) | Factory's AI coding agent |
| Git | Version control |

## Next Steps

- [Web UI Quick Start](./web) - Configure agents through the visual interface
- [CLI Quick Start](./cli) - Manage CCCC from the command line
- [Operations Runbook](/guide/operations) - Production operations guide
- [Secure Remote Access](/guide/operations#_5-secure-remote-access) - Set up Cloudflare Access or Tailscale
