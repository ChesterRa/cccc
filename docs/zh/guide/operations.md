# 运维手册

本页面面向需要日常可靠运行 CCCC 的运维人员。

## 1) 运行时拓扑

默认运行时主目录：
- `CCCC_HOME=~/.cccc`

关键路径：
- `~/.cccc/registry.json`
- `~/.cccc/daemon/ccccd.sock`
- `~/.cccc/daemon/ccccd.log`
- `~/.cccc/groups/<group_id>/group.yaml`
- `~/.cccc/groups/<group_id>/ledger.jsonl`

## 2) 启动与健康检查

### 启动

```bash
cccc
```

### 健康基线

```bash
cccc doctor
cccc daemon status
cccc groups
```

预期结果：
- Daemon 可达
- 运行时已检测到
- 活跃的工作组列表可加载

## 3) 故障排查顺序

当工作组出现卡顿时：

1. 检查 Daemon 健康状态。
2. 检查工作组状态（`active/idle/paused/stopped`）。
3. 检查 Actor 运行时状态。
4. 检查消息义务（reply-required / attention 确认）。
5. 检查自动化和投递节流。

常用命令：

```bash
cccc daemon status
cccc actor list
cccc inbox --actor-id <actor_id>
cccc tail -n 100 -f
```

## 4) 快速恢复手册

### Actor 级恢复（首选）

```bash
cccc actor restart <actor_id>
```

在工作组级重启之前，优先使用此方式。

### 工作组级恢复

```bash
cccc group stop
cccc group start
```

### Daemon 级恢复（最后手段）

```bash
cccc daemon stop
cccc daemon start
```

## 5) 安全远程访问

基本要求：
- 设置 `CCCC_WEB_TOKEN`。
- 使用 Cloudflare Access 或 Tailscale 作为网络边界。

禁止操作：
- 不要在没有访问网关的情况下直接暴露 Web UI。
- 不要将密钥存储在代码仓库文件中。

## 6) 升级手册（RC 安全）

### 升级前

1. 停止活跃的高风险会话。
2. 备份 `CCCC_HOME`。
3. 记录当前版本和基本运行状态。

### 升级

```bash
python -m pip install -U cccc-pair
```

### 升级后

```bash
cccc doctor
cccc daemon status
cccc mcp
```

进行一次小型端到端冒烟测试：
- 创建/加入工作组
- 添加/启动 Actor
- 发送/回复消息
- 验证 Ledger 和收件箱行为

## 7) 备份与恢复

### 备份（最小化）

备份 `CCCC_HOME`：
- 注册表
- Daemon 日志（可选）
- 所有工作组（`group.yaml`、Ledger、状态）

### 恢复

1. 停止 Daemon。
2. 恢复 `CCCC_HOME` 目录。
3. 启动 Daemon 并通过 `cccc doctor` 验证。

## 8) 运维准则

- 保持单一信息源：决策应记录在 CCCC 消息中。
- 对关键请求使用 `reply_required`。
- 当范围明确时，优先使用指定收件人，而非广播。
- 保持自动化聚焦于客观提醒，而非聊天噪音。

## 9) 升级处理清单

如果问题反复出现：

1. 收集证据：
   - 工作组 ID
   - Actor ID
   - 事件 ID
   - 最近的 `cccc tail -n 100`
2. 记录可复现的操作序列。
3. 划分严重级别（`P0/P1/P2`）。
4. 在发布记录中登记修复或风险。
