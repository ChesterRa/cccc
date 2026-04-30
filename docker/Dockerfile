# Stage 1: Build WebUI
FROM node:20-bookworm-slim AS web-builder
ARG HTTP_PROXY
ARG HTTPS_PROXY
ENV http_proxy=${HTTP_PROXY} https_proxy=${HTTPS_PROXY}
WORKDIR /web
COPY web/package*.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

# Stage 2: Final image
FROM python:3.11-slim-bookworm
ARG HTTP_PROXY
ARG HTTPS_PROXY
ENV http_proxy=${HTTP_PROXY} https_proxy=${HTTPS_PROXY}

ENV CCCC_BROWSER_TOOLS="xvfb fonts-noto-color-emoji fonts-unifont libfontconfig1 libfreetype6 xfonts-scalable fonts-liberation fonts-ipafont-gothic fonts-wqy-zenhei fonts-tlwg-loma-otf fonts-freefont-ttf"
ENV CCCC_BROWSER_DEPS="libasound2 libatk-bridge2.0-0 libatk1.0-0 libatspi2.0-0 libcairo2 libcups2 libdbus-1-3 libdrm2 libgbm1 libglib2.0-0 libnspr4 libnss3 libpango-1.0-0 libx11-6 libxcb1 libxcomposite1 libxdamage1 libxext6 libxfixes3 libxkbcommon0 libxrandr2"

# Install OS dependencies plus minimal Chromium shared libraries for projected browser/runtime auth.
RUN set -eux; \
    apt-get update; \
    apt-get install -y --no-install-recommends \
      curl ca-certificates git bash libstdc++6 \
      ${CCCC_BROWSER_TOOLS} \
      ${CCCC_BROWSER_DEPS}; \
    rm -rf /var/lib/apt/lists/*

# Reuse the already-pinned Node.js 20 runtime from the Web build stage.
# This avoids NodeSource/Debian package fallback drift and guarantees npm is present.
COPY --from=web-builder /usr/local/bin/node /usr/local/bin/node
COPY --from=web-builder /usr/local/lib/node_modules /usr/local/lib/node_modules
RUN ln -sf ../lib/node_modules/npm/bin/npm-cli.js /usr/local/bin/npm \
    && ln -sf ../lib/node_modules/npm/bin/npx-cli.js /usr/local/bin/npx \
    && node --version \
    && npm --version \
    && npx --version

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
     && echo '{"hasCompletedOnboarding":true,"bypassPermissionsModeAccepted":true}' > /home/cccc/.claude.json

# Configuration
ENV CCCC_HOME=/data

# Daemon IPC - TCP mode for SDK access
# K8s sidecar: 127.0.0.1 (same Pod shares network namespace)
# Local dev:   override with -e CCCC_DAEMON_HOST=0.0.0.0 -e CCCC_DAEMON_ALLOW_REMOTE=1
ENV CCCC_DAEMON_TRANSPORT=tcp
ENV CCCC_DAEMON_HOST=127.0.0.1
ENV CCCC_DAEMON_PORT=9765

# Web server - bind all interfaces inside container; host publishing should stay loopback by default
ENV CCCC_WEB_HOST=0.0.0.0
ENV CCCC_WEB_PORT=8848

EXPOSE 8848 9765
VOLUME ["/data", "/workspace"]

# cccc (without args) starts both daemon and web server
CMD ["cccc"]
