# CCCC Client SDK 设计文档（草案）

> 目标：在不改动内核的前提下，为外部系统（如 AdCreatorAI Orchestrator）提供稳定、低风险的 Client SDK（IPC 优先），复用 CCCC daemon 的能力。

## 1. 设计目标

- 最小改动：保留 daemon 单写者与权限模型
- 可观测：统一错误码与日志上下文
- 易集成：Python SDK + type hints + docstrings
- 可测试：支持 mock IPC 与 daemon fixture

## 2. 非目标

- Web UI / CLI
- 内核重构或多租户
- 远程公网暴露（HTTP transport 暂缓）

## 3. 架构与依赖关系

```
AdCreatorAI Orchestrator
          |
          v
     CCCC Client SDK
          |
      IPC socket
          |
        ccccd
          |
        Kernel
```

最小依赖：
- contracts/v1（数据模型）
- daemon IPC 协议（call_daemon）

## 4. 模块结构（建议）

```
src/cccc/sdk/
  __init__.py          # 导出 CCCCClient
  client.py            # 高层 API
  transport.py         # IPC 传输层
  models.py            # 类型定义（引用 contracts）
  errors.py            # 统一错误码
```

## 5. API 设计（Python，草案）

```py
from typing import Any, Dict, List, Optional

class CCCCError(Exception):
    code: str
    message: str

class CCCCClient:
    def __init__(self, cccc_home: Optional[str] = None, timeout_s: float = 10.0):
        """Create client. cccc_home defaults to ~/.cccc"""

    # ---- group ----
    def group_list(self) -> Dict[str, Any]:
        """List groups"""

    def group_info(self, group_id: str) -> Dict[str, Any]:
        """Get group info"""

    def group_set_state(self, group_id: str, state: str, by: str) -> Dict[str, Any]:
        """Set group state: active|idle|paused|stopped"""

    # ---- actor ----
    def actor_list(self, group_id: str) -> Dict[str, Any]:
        """List actors"""

    def actor_add(self, group_id: str, by: str, actor_id: str, runtime: str, runner: str = "pty") -> Dict[str, Any]:
        """Add actor (foreman only)"""

    def actor_start(self, group_id: str, by: str, actor_id: str) -> Dict[str, Any]:
        """Start actor"""

    def actor_stop(self, group_id: str, by: str, actor_id: str) -> Dict[str, Any]:
        """Stop actor"""

    def actor_restart(self, group_id: str, by: str, actor_id: str) -> Dict[str, Any]:
        """Restart actor"""

    # ---- message / inbox ----
    def message_send(self, group_id: str, by: str, text: str, to: Optional[List[str]] = None) -> Dict[str, Any]:
        """Send message"""

    def message_reply(self, group_id: str, by: str, reply_to: str, text: str) -> Dict[str, Any]:
        """Reply message"""

    def inbox_list(self, group_id: str, actor_id: str, kind_filter: str = "chat", limit: int = 50) -> Dict[str, Any]:
        """List inbox"""

    def inbox_mark_read(self, group_id: str, actor_id: str, event_id: str) -> Dict[str, Any]:
        """Mark read"""

    # ---- context ----
    def context_get(self, group_id: str) -> Dict[str, Any]:
        """Get context"""

    def context_sync(self, group_id: str, ops: List[Dict[str, Any]], dry_run: bool = False) -> Dict[str, Any]:
        """Sync context"""
```

## 6. Transport 设计（IPC 优先）

- 默认使用 `ccccd.sock`
- 自动发现：`CCCC_HOME` -> `~/.cccc`
- 超时控制：默认 10s，可配置
- 失败提示：daemon 未启动 / socket 不存在

## 7. 错误码（建议）

```
DAEMON_NOT_RUNNING
SOCKET_NOT_FOUND
TIMEOUT
INVALID_REQUEST
PERMISSION_DENIED
GROUP_NOT_FOUND
ACTOR_NOT_FOUND
MCP_ERROR
INTERNAL
```

## 8. 测试清单

单测（mock IPC）
- 连接失败 / 超时
- 权限错误
- 基础 API 返回结构

集成测试（daemon fixture）
- 启动 daemon -> group_list
- actor_add/start/stop
- message_send/inbox_list
- context_get/sync

## 9. 集成到 AdCreatorAI 的方式

- Orchestrator 通过 SDK 调用 CCCC
- 统一 traceId 写入 message payload
- 前端不直接访问 CCCC

## 10. 里程碑

Week 1
- transport + group/actor/message/inbox
- errors + docstrings

Week 2
- context API
- demo 脚本
- 集成测试
