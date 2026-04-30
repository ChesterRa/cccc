# CCCC Desktop（桌面端）

基于 [Tauri v2](https://tauri.app/) 构建的 CCCC Web UI 原生桌面套壳。

> **状态**：社区贡献 — macOS 已测试可用。Windows / Linux 构建理论可行，尚未验证。

## 新增功能

| 功能 | 说明 |
|------|------|
| 原生 `.app` / `.exe` | 无需打开浏览器，像普通桌面应用一样启动 CCCC |
| 透明标题栏（macOS） | `titleBarStyle: Overlay`，界面无边框，视觉更简洁 |
| 原生窗口拖动 | 通过 `NSWindow.setMovableByWindowBackground` 实现，即使 UI 由远程来源（`localhost:8848`）提供服务也能正常拖动 |
| 打包 DMG | `tauri build` 生成可签名的 `.app` 包和 `.dmg` 安装包 |

---

## 前置条件

| 工具 | 版本 |
|------|------|
| [Rust](https://rustup.rs/) | 1.77+ |
| [Node.js](https://nodejs.org/) | 18+ |
| [CCCC 守护进程](https://github.com/ChesterRa/cccc) | 运行于 `localhost:8848` |

macOS 还需要 Xcode 命令行工具：

```bash
xcode-select --install
```

---

## 快速开始

```bash
# 1. 安装 Tauri CLI
cd desktop
npm install

# 2. 先确保 CCCC 守护进程正在运行
cccc               # 或: cccc daemon start

# 3. 开发模式启动（热更新来自 localhost:8848）
npm run dev

# 4. 构建发布包
npm run build
#    → desktop/src-tauri/target/release/bundle/macos/cccc.app
#    → desktop/src-tauri/target/release/bundle/dmg/cccc_x.y.z_x64.dmg
```

---

## 工作原理

本套壳是一个**纯壳**——自身不包含任何前端代码。`tauri.conf.json` 将开发 URL 和发布 URL 都指向 `http://localhost:8848/ui/`，Tauri webview 直接渲染 CCCC 守护进程提供的页面。

### 透明标题栏 + 拖动（macOS）

macOS 的 `titleBarStyle: Overlay` 会隐藏默认的窗口边框，并在内容区域上方叠加红绿灯按钮。由于 webview 加载的是**远程来源**，Tauri 的 `data-tauri-drag-region` 属性和 JS `startDragging()` API 都会被渲染器沙箱阻断。我们通过两层机制解决：

1. **CSS 垫片**（通过 `on_page_load` 注入）：在每个页面追加一个 28px 透明覆盖 div，让 UI body 向下偏移，避开红绿灯按钮区域。

2. **原生 NSWindow API**（Rust `setup`）：`setMovableByWindowBackground: YES` 告知 macOS 将所有未被遮挡的窗口背景像素视为拖动区域，不受内容来源限制。

---

## 项目结构

```
desktop/
├── package.json               npm 包装器（仅含 @tauri-apps/cli）
├── src-tauri/
│   ├── Cargo.toml             Rust crate（cccc-desktop）
│   ├── tauri.conf.json        Tauri 应用配置
│   ├── capabilities/
│   │   └── default.json       权限集
│   ├── icons/                 应用图标（各尺寸 + .icns / .ico）
│   └── src/
│       ├── main.rs            可执行文件入口
│       └── lib.rs             Tauri builder + 垫片注入
└── README.md                  英文文档
```
