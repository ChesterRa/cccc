# SDK 概览

当你需要将 CCCC 与外部应用和服务集成时，可以使用官方 SDK。

## 官方 SDK

- 仓库：[ChesterRa/cccc-sdk](https://github.com/ChesterRa/cccc-sdk)
- Python 包：`cccc-sdk`（导入为 `cccc_sdk`）
- TypeScript 包：`cccc-sdk`

## 安装

```bash
pip install -U cccc-sdk
npm install cccc-sdk
```

## 与 CCCC 核心的关系

- CCCC 核心（`cccc-pair`）是运行时控制平面（daemon + ledger + ports）。
- SDK 是该运行中的控制平面的客户端接口。
- SDK 不替代核心，也不自行持久化状态。

## 下一步

- [客户端 SDK](./CLIENT_SDK)
