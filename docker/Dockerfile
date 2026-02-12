# Stage 1: Build WebUI
FROM node:20-slim AS web-builder
ARG HTTP_PROXY
ARG HTTPS_PROXY
ENV http_proxy=${HTTP_PROXY} https_proxy=${HTTPS_PROXY}
WORKDIR /web
COPY web/package*.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

# Stage 2: Final image
FROM python:3.11-slim
ARG HTTP_PROXY
ARG HTTPS_PROXY
ENV http_proxy=${HTTP_PROXY} https_proxy=${HTTPS_PROXY}

# Install dependencies + Node.js
RUN apt-get update && apt-get install -y --no-install-recommends \
    curl ca-certificates git bash \
    && curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
    && apt-get install -y nodejs \
    && rm -rf /var/lib/apt/lists/*

# Install Claude Code CLI and move to global path
RUN curl -fsSL https://claude.ai/install.sh | bash \
    && cp /root/.local/bin/claude /usr/local/bin/claude

# Install other CLI tools via npm
RUN npm install -g \
    @google/gemini-cli \
    @openai/codex \
    @factory/cli

# Install CCCC
WORKDIR /app
COPY pyproject.toml MANIFEST.in LICENSE README.md README.zh-CN.md README.ja.md ./
COPY src/ ./src/

# Copy built WebUI (vite outputs to ../src/cccc/ports/web/dist)
COPY --from=web-builder /src/cccc/ports/web/dist ./src/cccc/ports/web/dist

RUN pip install --no-cache-dir .

# Workspace for project files (mount volumes here)
WORKDIR /workspace
RUN rm -rf /app

# Clear build-time proxy
ENV http_proxy="" https_proxy=""

# Create non-root user (Claude CLI refuses --dangerously-skip-permissions as root)
RUN useradd -m -s /bin/bash cccc \
    && mkdir -p /data /workspace \
    && chown -R cccc:cccc /data /workspace

USER cccc
ENV HOME=/home/cccc
ENV PATH="/home/cccc/.local/bin:${PATH}"

# Set up Claude CLI for non-root user
RUN mkdir -p /home/cccc/.local/bin \
    && ln -s /usr/local/bin/claude /home/cccc/.local/bin/claude \
    && echo '{"hasCompletedOnboarding":true}' > /home/cccc/.claude.json

# Configuration
ENV CCCC_HOME=/data

# Daemon IPC - TCP mode for SDK access
# K8s sidecar: 127.0.0.1 (same Pod shares network namespace)
# Local dev:   override with -e CCCC_DAEMON_HOST=0.0.0.0 -e CCCC_DAEMON_ALLOW_REMOTE=1
ENV CCCC_DAEMON_TRANSPORT=tcp
ENV CCCC_DAEMON_HOST=127.0.0.1
ENV CCCC_DAEMON_PORT=9765

# Web server - 0.0.0.0 for external access
ENV CCCC_WEB_HOST=0.0.0.0
ENV CCCC_WEB_PORT=8848

EXPOSE 8848 9765
VOLUME ["/data", "/workspace"]

# cccc (without args) starts both daemon and web server
CMD ["cccc"]
