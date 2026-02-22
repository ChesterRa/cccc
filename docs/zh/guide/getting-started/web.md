# Web UI 快速上手

通过 Web 界面开始使用 CCCC。

## 第 1 步：启动 CCCC

打开终端并运行：

```bash
cccc
```

这会同时启动 Daemon 和 Web UI。

## 第 2 步：打开 Web UI

在浏览器中访问：

```
http://127.0.0.1:8848/
```

你应该能看到 CCCC Web 界面。

## 第 3 步：创建工作组

1. 点击侧边栏的 **+** 按钮
2. 或者关联一个已有项目：

```bash
# 在另一个终端中
cd /path/to/your/project
cccc attach .
```

3. 刷新 Web UI 即可看到新工作组

## 第 4 步：添加第一个 Agent

1. 点击顶部的 **Add Actor**
2. 填写表单：
   - **Actor ID**：例如 `assistant`
   - **Runtime**：选择你已安装的 CLI（如 Claude）
   - **Runner**：PTY（终端）或 Headless
3. 点击 **Create**

## 第 5 步：配置 MCP（仅首次需要）

如果这是你第一次使用该运行时：

```bash
cccc setup --runtime claude   # 或 codex, droid 等
```

这会配置 Agent 与 CCCC 的通信协议。

## 第 6 步：启动 Agent

1. 在标签页中找到你的 Agent
2. 点击 **Play** 按钮启动
3. 等待 Agent 初始化

Agent 的终端输出会显示在标签页中。

## 第 7 步：发送第一条消息

1. 点击 **Chat** 标签页
2. 在输入框中输入：
   ```
   你好！请自我介绍一下。
   ```
3. 按 `Ctrl+Enter` / `Cmd+Enter`，或点击发送

## 第 8 步：观察 Agent 工作

1. 切换到 Agent 标签页查看终端输出
2. 观察 Agent 处理你的请求
3. 响应会出现在 Chat 标签页中

## 添加更多 Agent

要添加第二个协作 Agent：

1. 再次点击 **Add Actor**
2. 使用不同的 ID（如 `reviewer`）
3. 可选择不同的运行时
4. 启动 Agent

现在你可以：
- 发送给所有人：直接输入消息
- 发送给特定 Agent：使用 `@assistant` 或 `@reviewer`

## 使用 Context 面板

点击 **Context** 打开侧面板：

- **Vision**：设置项目目标
- **Sketch**：记录执行方案
- **Tasks**：跟踪工作项
- **Notes**：记录心得

Agent 可以读取和更新这些共享上下文。

## Web UI 功能一览

| 功能 | 操作方式 |
|------|----------|
| 切换工作组 | 点击侧边栏中的工作组 |
| Agent 终端 | 点击 Agent 标签页 |
| 发送消息 | Chat 标签页输入框 |
| @提及 | 输入 `@` 触发自动补全 |
| 回复消息 | 点击回复图标 |
| 设置 | 顶部齿轮图标 |
| 主题 | 点击月亮/太阳图标 |

## 快捷键

| 快捷键 | 操作 |
|--------|------|
| `Ctrl+Enter` / `Cmd+Enter` | 发送消息 |
| `Enter` | 换行 |
| `@` | 打开提及菜单 |
| `Escape` | 取消回复 / 关闭菜单 |
| `↑` `↓` | 导航提及菜单 |
| `Tab` / `Enter` | 选择提及 |

## 故障排查

### Web UI 无法加载？

1. 检查 Daemon 是否运行：
   ```bash
   cccc daemon status
   ```

2. 尝试其他端口：
   ```bash
   CCCC_WEB_PORT=9000 cccc
   ```

### Agent 无法启动？

1. 检查终端标签页的错误信息
2. 验证 MCP 配置：
   ```bash
   cccc setup --runtime <name>
   ```

### 看不到项目？

在项目目录中运行 `cccc attach .`，然后刷新 Web UI。

## 下一步

- [工作流](/zh/guide/workflows) — 学习协作模式
- [Web UI 指南](/zh/guide/web-ui) — 详细 UI 文档
- [IM Bridge](/zh/guide/im-bridge/) — 设置移动端访问
