# 快速上手

10 分钟让 CCCC 跑起来。

## 选择你的方式

CCCC 提供两种上手方式：

<div class="vp-card-container">

### [Web UI 快速上手](./web)

**推荐大多数用户使用**

- 可视化管理 Agent
- 点击式配置
- 实时终端视图
- 移动端友好

### [CLI 快速上手](./cli)

**终端爱好者专属**

- 命令行全面控制
- 可脚本化、自动化
- 适合 CI/CD 集成
- 高级用户功能

### [Docker 部署](./docker)

**适用于服务器和团队**

- 一条命令部署
- 预装 AI Agent CLI
- 卷持久化数据
- Docker Compose 和 K8s 就绪

</div>

## 前置条件

两种方式都需要：

- **Python 3.9+**
- 至少一个 AI Agent CLI：
  - [Claude Code](https://docs.anthropic.com/en/docs/claude-code)（推荐）
  - [Codex CLI](https://github.com/openai/codex)
  - [GitHub Copilot CLI](https://docs.github.com/en/copilot)
  - 或其他支持的运行时

## 安装

### 从旧版本升级

如果已安装旧版 cccc-pair（如 0.3.x），必须先卸载：

```bash
# pipx 用户
pipx uninstall cccc-pair

# pip 用户
pip uninstall cccc-pair

# 如需清理残留二进制文件
rm -f ~/.local/bin/cccc ~/.local/bin/ccccd
```

::: warning 0.4.x 版本的破坏性变更
0.4.x 版本的命令结构与 0.3.x 完全不同。旧的 `init`、`run`、`bridge` 命令已替换为 `attach`、`daemon`、`mcp` 等。
:::

### 从 PyPI 安装

```bash
pip install -U cccc-pair
```

### 从 TestPyPI 安装（用于 RC 测试）

```bash
pip install -U --pre \
  --index-url https://test.pypi.org/simple \
  --extra-index-url https://pypi.org/simple \
  cccc-pair
```

### 从源码安装

```bash
git clone https://github.com/ChesterRa/cccc
cd cccc
pip install -e .
```

## 验证安装

```bash
cccc doctor
```

此命令会检查 Python 版本、可用运行时和系统配置。

## 下一步

- [Web UI 快速上手](./web) — 使用可视化界面开始
- [CLI 快速上手](./cli) — 使用命令行开始
- [Docker 部署](./docker) — 在 Docker 容器中部署 CCCC
- [SDK 概览](/zh/sdk/) — 将 CCCC 集成到外部应用/服务
- [使用场景](/zh/guide/use-cases) — 了解高投入产出比的真实场景
- [运维手册](/zh/guide/operations) — 以运维级可靠性运行 CCCC
- [定位](/zh/reference/positioning) — 确定 CCCC 在你技术栈中的位置
