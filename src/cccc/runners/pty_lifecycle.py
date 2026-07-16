from __future__ import annotations

import threading
from typing import Dict, Optional, Tuple


LifecycleKey = Tuple[str, str]


class _OperationState:
    def __init__(self, *, phase: str, token: object) -> None:
        self.phase = phase
        self.token = token


class _LifecycleLease:
    def __init__(
        self,
        gate: "LifecycleGate",
        *,
        scope: str,
        token: object,
        key: Optional[LifecycleKey] = None,
        group_id: Optional[str] = None,
    ) -> None:
        self._gate = gate
        self._scope = scope
        self._token = token
        self._key = key
        self._group_id = group_id
        self._released = False

    def __enter__(self) -> "_LifecycleLease":
        return self

    def __exit__(self, _exc_type, _exc, _traceback) -> None:
        if self._released:
            return
        self._released = True
        if self._scope == "operation" and self._key is not None:
            self._gate._release_operation(self._key, self._token)
        elif self._scope == "group" and self._group_id is not None:
            self._gate._release_group_bulk(self._group_id, self._token)
        elif self._scope == "global":
            self._gate._release_global_bulk(self._token)


class LifecycleGate:
    def __init__(self) -> None:
        self._condition = threading.Condition()
        self._operations: Dict[LifecycleKey, _OperationState] = {}
        self._group_bulk_tokens: Dict[str, object] = {}
        self._global_bulk_token: Optional[object] = None

    def begin_start(self, key: LifecycleKey) -> _LifecycleLease:
        token = object()
        with self._condition:
            while (
                self._global_bulk_token is not None
                or key[0] in self._group_bulk_tokens
                or key in self._operations
            ):
                self._condition.wait()
            self._operations[key] = _OperationState(phase="start", token=token)
        return _LifecycleLease(self, scope="operation", key=key, token=token)

    def begin_stop(self, key: LifecycleKey) -> Optional[_LifecycleLease]:
        token = object()
        with self._condition:
            while True:
                state = self._operations.get(key)
                if state is not None and state.phase == "stop":
                    return None
                if self._global_bulk_token is not None or key[0] in self._group_bulk_tokens:
                    return None
                if state is None:
                    self._operations[key] = _OperationState(phase="stop", token=token)
                    break
                self._condition.wait()
        return _LifecycleLease(self, scope="operation", key=key, token=token)

    def begin_bulk_stop(self, *, group_id: Optional[str]) -> _LifecycleLease:
        token = object()
        if group_id is not None:
            return self._begin_group_stop(group_id, token=token)
        return self._begin_global_stop(token=token)

    def _begin_group_stop(self, group_id: str, *, token: object) -> _LifecycleLease:
        with self._condition:
            while self._global_bulk_token is not None or group_id in self._group_bulk_tokens:
                self._condition.wait()
            self._group_bulk_tokens[group_id] = token
            try:
                while any(key[0] == group_id for key in self._operations):
                    self._condition.wait()
            except BaseException:
                if self._group_bulk_tokens.get(group_id) is token:
                    self._group_bulk_tokens.pop(group_id, None)
                self._condition.notify_all()
                raise
        return _LifecycleLease(self, scope="group", group_id=group_id, token=token)

    def _begin_global_stop(self, *, token: object) -> _LifecycleLease:
        with self._condition:
            while self._global_bulk_token is not None:
                self._condition.wait()
            self._global_bulk_token = token
            try:
                while self._group_bulk_tokens or self._operations:
                    self._condition.wait()
            except BaseException:
                if self._global_bulk_token is token:
                    self._global_bulk_token = None
                self._condition.notify_all()
                raise
        return _LifecycleLease(self, scope="global", token=token)

    def _release_operation(self, key: LifecycleKey, token: object) -> None:
        with self._condition:
            state = self._operations.get(key)
            if state is None or state.token is not token:
                return
            self._operations.pop(key, None)
            self._condition.notify_all()

    def _release_group_bulk(self, group_id: str, token: object) -> None:
        with self._condition:
            if self._group_bulk_tokens.get(group_id) is not token:
                return
            self._group_bulk_tokens.pop(group_id, None)
            self._condition.notify_all()

    def _release_global_bulk(self, token: object) -> None:
        with self._condition:
            if self._global_bulk_token is not token:
                return
            self._global_bulk_token = None
            self._condition.notify_all()
