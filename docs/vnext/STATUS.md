# CCCC vNext — Status & Roadmap

> 最后更新：2026-02-11

## 当前状态（面向 rc19）

项目已进入 `v0.4.0rc19` 的质量收敛阶段，当前主线目标是：

- 关闭实现与文档漂移
- 补齐 CI/Release 质量门禁
- 完成发布演练并形成 go/no-go 结论

发布主看板与执行资料位于 `docs/release/`。

## 已完成（本轮关键收敛）

- 建立全仓审计矩阵（覆盖 tracked + 本地工作文件）
- 建立 release board / findings register / gate / rehearsal 报告
- 修复 MCP 与 automation 在 `group_state=stopped` 的能力漂移
- 补齐 Daemon IPC 标准中的遗漏操作定义（5 项）并新增文档一致性测试
- 修复权限边界类型面漂移（`group.settings_update`）并补回归测试
- 补齐 `contracts.v1.event` 对核心 `group.*` 事件的模型覆盖并新增一致性测试
- 修复多语言 README 与 docs 的 MCP 工具数量硬编码漂移
- 修复 CLI/架构/标准/发布文档中的已知 `P1` 漂移项
- CI 与 release workflow 增加 `pytest` 执行门禁

## 当前未闭环项（高优先）

- 发布治理资产：`CHANGELOG` / `SECURITY` / `SUPPORT` 文档定稿
- 发布演练：轮子安装、升级路径、回滚路径在干净 runner 上复核

## 下一步里程碑

1. 完成发布治理资产定稿并固化对外发布文案
2. 完成 `R8` 全链路演练并记录 go/no-go
3. 执行 `R9`（tag + TestPyPI）

## 相关文档

- 发布计划：`docs/release/RC19_RELEASE_BOARD.md`
- 执行清单：`docs/release/RC19_EXECUTION_CHECKLIST.md`
- 问题台账：`docs/release/RC19_FINDINGS_REGISTER.md`
- 质量门禁：`docs/release/RC19_GATES.md`
