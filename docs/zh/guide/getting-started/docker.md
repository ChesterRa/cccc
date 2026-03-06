# Docker 部署

在 Docker 容器中运行 CCCC — 适用于服务器、团队和可复现环境。

## AI 辅助部署

复制下方提示词并粘贴给任意 AI 助手 — 它会引导你交互式完成整个部署。

::: details 点击复制 AI 部署提示词

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
2. "Set a CCCC_WEB_TOKEN for authentication (any random string you choose):"
3. "Where are your project files? (absolute path, will be mounted to /workspace)"
4. "Which AI agent API keys do you have? (ANTHROPIC_AUTH_TOKEN / OPENAI_API_KEY / GEMINI_API_KEY)"

## Step 4: Run the container
Build the docker run command from the user's answers:
  docker run -d \
    -p {port}:8848 \
    -v cccc-data:/data \
    -v {project_path}:/workspace \
    -e CCCC_WEB_TOKEN={token} \
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
  # Edit docker/.env with their values (CCCC_WEB_TOKEN, API keys, port, workspace path, proxy)
  # From project root:
  docker compose --env-file docker/.env -f docker/docker-compose.yml up -d --build
  # Or from docker directory:
  cd docker && docker compose up -d --build

## Key environment variables reference:
| Variable | Default | Description |
|----------|---------|-------------|
| CCCC_HOME | /data | Data directory |
| CCCC_WEB_HOST | 0.0.0.0 | Web bind address |
| CCCC_WEB_PORT | 8848 | Web port |
| CCCC_WEB_TOKEN | (none) | Auth token (required) |
| CCCC_DAEMON_TRANSPORT | tcp | IPC transport |
| CCCC_DAEMON_HOST | 127.0.0.1 | Daemon bind address |
| CCCC_DAEMON_PORT | 9765 | Daemon IPC port |
| ANTHROPIC_AUTH_TOKEN | (none) | Auth token for Claude |
| OPENAI_API_KEY | (none) | API key for Codex runtime |
| GEMINI_API_KEY | (none) | API key for Gemini CLI runtime |

## Tone: concise, practical, one step at a time. Confirm each step succeeds before moving on.
```

:::

## 前置条件

- [Docker](https://docs.docker.com/get-docker/)（20.10+）
- 至少一个 AI Agent API Key（如 Claude 的 `ANTHROPIC_AUTH_TOKEN`）

## 快速开始

### 1. 构建镜像

```bash
git clone https://github.com/ChesterRa/cccc
cd cccc
docker build -f docker/Dockerfile -t cccc .
```

::: tip 构建说明
构建使用多阶段方式：首先编译 Web UI（Node.js），然后打包包含预装 AI Agent CLI（Claude Code、Gemini CLI、Codex CLI）的 Python Daemon。
:::

### 2. 运行容器

```bash
docker run -d \
  -p 8848:8848 \
  -v cccc-data:/data \
  -v /path/to/your/projects:/workspace \
  -e CCCC_WEB_TOKEN=your-secret-token \
  -e ANTHROPIC_AUTH_TOKEN=sk-ant-xxx \
  --name cccc \
  cccc
```

在浏览器中打开 `http://localhost:8848` 访问 Web UI。

### 3. 验证

```bash
# 检查容器运行状态
docker logs cccc

# 健康检查
docker exec cccc cccc doctor
```

## 配置

### 环境变量

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `CCCC_HOME` | `/data` | 数据目录（工作组、Ledger、配置） |
| `CCCC_WEB_HOST` | `0.0.0.0` | Web 服务绑定地址 |
| `CCCC_WEB_PORT` | `8848` | Web 服务端口 |
| `CCCC_WEB_TOKEN` | _（无）_ | **必填。** Web UI 认证令牌 |
| `CCCC_DAEMON_TRANSPORT` | `tcp` | Daemon IPC 传输方式（`tcp` 或 `unix`） |
| `CCCC_DAEMON_HOST` | `127.0.0.1` | Daemon 绑定地址 |
| `CCCC_DAEMON_PORT` | `9765` | Daemon IPC 端口 |
| `ANTHROPIC_AUTH_TOKEN` | _（无）_ | Claude Code 运行时的认证令牌（不要与 `ANTHROPIC_API_KEY` 同时设置） |
| `ANTHROPIC_BASE_URL` | _（无）_ | Claude Code 自定义 API 端点 |
| `OPENAI_API_KEY` | _（无）_ | Codex 运行时的 API Key |
| `OPENAI_BASE_URL` | _（无）_ | Codex 自定义 API 端点 |
| `GEMINI_API_KEY` | _（无）_ | Gemini CLI 运行时的 API Key |

### 卷挂载

| 容器路径 | 用途 |
|----------|------|
| `/data` | 持久化 CCCC 状态（工作组、Ledger、Daemon 配置） |
| `/workspace` | Agent 工作的项目文件 |

::: warning 保护你的数据
务必将 `/data` 挂载到命名卷或宿主机路径，以便在容器重启时持久化状态。
:::

## 高级用法

### 暴露 Daemon IPC 用于 SDK 访问

如需从容器外部访问 Daemon IPC（如 SDK 集成）：

```bash
docker run -d \
  -p 8848:8848 \
  -p 9765:9765 \
  -v cccc-data:/data \
  -v /path/to/projects:/workspace \
  -e CCCC_WEB_TOKEN=your-secret-token \
  -e CCCC_DAEMON_HOST=0.0.0.0 \
  -e CCCC_DAEMON_ALLOW_REMOTE=1 \
  --name cccc \
  cccc
```

### 自定义 Claude CLI 配置

容器预配置了 Claude CLI（已跳过 Onboarding）。如需进一步自定义：

```bash
# 从宿主机写入配置
docker exec cccc sh -c 'cat > /home/cccc/.claude.json << EOF
{
  "hasCompletedOnboarding": true,
  "customApiKey": "your-key"
}
EOF'

# 或复制配置文件
docker cp ~/.claude.json cccc:/home/cccc/.claude.json
```

### 使用 Docker Compose 运行

仓库自带 `docker/docker-compose.yml`。先复制并编辑环境变量文件：

```bash
cp docker/.env.example docker/.env
# 编辑 docker/.env — 设置 CCCC_WEB_TOKEN、API Key、workspace 路径等
```

创建数据卷（首次运行时需要，因为 Compose 文件使用 `external: true`）：

```bash
docker volume create cccc-data
```

然后选择一种方式启动：

```bash
# 方式 A：从项目根目录运行（推荐）
docker compose --env-file docker/.env -f docker/docker-compose.yml up -d --build

# 方式 B：从 docker 目录运行
cd docker
docker compose up -d --build
```

::: info 首次运行 vs 更新
`--build` 会从源码构建镜像。后续运行中，如果镜像未变更，可省略此参数 — `docker compose up -d` 会复用现有镜像。
:::

`.env` 文件控制端口、卷、API Key 和构建代理。详见 `docker/.env.example`。

::: tip 在代理后构建
在 `.env` 中设置 `HTTP_PROXY` 和 `HTTPS_PROXY`，以在 `docker compose build` 时传递代理设置。两个构建阶段（Node.js 和 Python）都会使用代理进行 `curl`、`npm` 和 `pip` 操作。
:::

#### 日常运维

```bash
# 更新部署（构建 + 重启）
docker compose --env-file docker/.env -f docker/docker-compose.yml up -d --build

# 强制完全重建（无缓存）
docker compose --env-file docker/.env -f docker/docker-compose.yml up -d --build --no-cache

# 查看日志
docker compose --env-file docker/.env -f docker/docker-compose.yml logs -f

# 停止
docker compose --env-file docker/.env -f docker/docker-compose.yml down
```

### K8s Sidecar 模式

Kubernetes 部署时，Daemon 默认绑定 `127.0.0.1:9765` — 适合共享同一 Pod 网络命名空间的 Sidecar 容器。Pod 内通信无需额外配置。

## 故障排查

### "cannot be used with root/sudo privileges"

Claude CLI 拒绝以 root 身份运行 `--dangerously-skip-permissions`。Dockerfile 已创建非 root 的 `cccc` 用户来处理此问题。如果看到此错误，请确保使用最新的 Dockerfile。

### 卷权限问题

如果之前以 root 运行容器后切换到非 root 用户，现有卷数据可能是 root 所有权：

```bash
# 修复数据卷权限
docker run --rm -v cccc-data:/data python:3.11-slim \
  chown -R 1000:1000 /data
```

### Agent CLI 未找到

镜像预装了 Claude Code、Gemini CLI 和 Codex CLI。如果运行时未被检测到：

```bash
# 检查可用运行时
docker exec cccc cccc doctor

# 验证 CLI 可用性
docker exec cccc claude --version
docker exec cccc gemini --version
docker exec cccc codex --version
```

### 容器日志

```bash
# 实时日志
docker logs -f cccc

# 最近 100 行
docker logs --tail 100 cccc
```

## 预装工具

Docker 镜像包含：

| 工具 | 用途 |
|------|------|
| Python 3.11 | CCCC Daemon 运行环境 |
| Node.js 20 | Agent CLI 运行环境（基于 npm 的工具） |
| [Claude Code](https://docs.anthropic.com/en/docs/claude-code) | Anthropic 的 AI 编码 Agent |
| [Gemini CLI](https://github.com/google/gemini-cli) | Google 的 AI 编码 Agent |
| [Codex CLI](https://github.com/openai/codex) | OpenAI 的 AI 编码 Agent |
| [Factory CLI](https://www.factory.ai/) | Factory 的 AI 编码 Agent |
| Git | 版本控制 |

## 下一步

- [Web UI 快速上手](./web) — 通过可视化界面配置 Agent
- [CLI 快速上手](./cli) — 通过命令行管理 CCCC
- [运维手册](/zh/guide/operations) — 生产环境运维指南
- [安全远程访问](/zh/guide/operations#_5-secure-remote-access) — 设置 Cloudflare Access 或 Tailscale
