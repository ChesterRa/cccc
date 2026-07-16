from __future__ import annotations

import os
import inspect
import secrets
from typing import Any, Dict

from ...providers.notebooklm.adapter import get_notebooklm_adapter
from ...providers.notebooklm.errors import NotebookLMProviderError
from .group_space_store import get_space_provider_state, load_space_provider_secrets

_NOTEBOOKLM_AUTH_KEY = "NOTEBOOKLM_AUTH_JSON"


class SpaceProviderError(RuntimeError):
    def __init__(
        self,
        code: str,
        message: str,
        *,
        transient: bool = False,
        degrade_provider: bool = False,
    ) -> None:
        super().__init__(message)
        self.code = str(code or "space_upstream_error")
        self.transient = bool(transient)
        self.degrade_provider = bool(degrade_provider)


def _truthy_env(name: str) -> bool:
    value = str(os.environ.get(name) or "").strip().lower()
    return value in {"1", "true", "yes", "y", "on"}


def _notebooklm_stub_enabled() -> bool:
    return _truthy_env("CCCC_NOTEBOOKLM_STUB")


def notebooklm_real_enabled() -> bool:
    try:
        state = get_space_provider_state("notebooklm")
        return bool(state.get("real_enabled"))
    except Exception:
        return _truthy_env("CCCC_NOTEBOOKLM_REAL")


def _resolve_notebooklm_auth_json() -> str:
    raw_env = str(os.environ.get("CCCC_NOTEBOOKLM_AUTH_JSON") or "").strip()
    if raw_env:
        return raw_env
    secrets_map = load_space_provider_secrets("notebooklm")
    return str(secrets_map.get(_NOTEBOOKLM_AUTH_KEY) or "").strip()


def _notebooklm_ingest(*, remote_space_id: str, kind: str, payload: Dict[str, Any]) -> Dict[str, Any]:
    if notebooklm_real_enabled():
        adapter = get_notebooklm_adapter()
        auth_json_raw = _resolve_notebooklm_auth_json()
        try:
            ingest_fn = adapter.ingest
            supports_auth_override = False
            try:
                params = inspect.signature(ingest_fn).parameters
                supports_auth_override = "auth_json_raw" in params
            except Exception:
                supports_auth_override = False
            if supports_auth_override:
                return ingest_fn(
                    remote_space_id=remote_space_id,
                    kind=kind,
                    payload=payload,
                    auth_json_raw=auth_json_raw,
                )
            return ingest_fn(
                remote_space_id=remote_space_id,
                kind=kind,
                payload=payload,
            )
        except NotebookLMProviderError as e:
            raise SpaceProviderError(
                e.code,
                str(e),
                transient=bool(e.transient),
                degrade_provider=bool(e.degrade_provider),
            ) from e
    if not _notebooklm_stub_enabled():
        raise SpaceProviderError(
            "space_provider_disabled",
            "notebooklm provider adapter is not configured in this build",
            transient=False,
            degrade_provider=True,
        )
    return {
        "provider": "notebooklm",
        "remote_space_id": str(remote_space_id or ""),
        "kind": str(kind or ""),
        "accepted": True,
        "stub": True,
        "payload_keys": sorted(payload.keys()),
    }


def _notebooklm_query(*, remote_space_id: str, query: str, options: Dict[str, Any]) -> Dict[str, Any]:
    if notebooklm_real_enabled():
        adapter = get_notebooklm_adapter()
        auth_json_raw = _resolve_notebooklm_auth_json()
        try:
            query_fn = adapter.query
            supports_auth_override = False
            try:
                params = inspect.signature(query_fn).parameters
                supports_auth_override = "auth_json_raw" in params
            except Exception:
                supports_auth_override = False
            if supports_auth_override:
                return query_fn(
                    remote_space_id=remote_space_id,
                    query=query,
                    options=options,
                    auth_json_raw=auth_json_raw,
                )
            return query_fn(
                remote_space_id=remote_space_id,
                query=query,
                options=options,
            )
        except NotebookLMProviderError as e:
            raise SpaceProviderError(
                e.code,
                str(e),
                transient=bool(e.transient),
                degrade_provider=bool(e.degrade_provider),
            ) from e
    if not _notebooklm_stub_enabled():
        raise SpaceProviderError(
            "space_provider_disabled",
            "notebooklm provider adapter is not configured in this build",
            transient=False,
            degrade_provider=True,
        )
    prompt = str(query or "").strip()
    answer = f"[NotebookLM stub] {prompt}" if prompt else "[NotebookLM stub] empty query"
    return {
        "answer": answer,
        "references": [],
        "remote_space_id": str(remote_space_id or ""),
        "options": dict(options or {}),
        "stub": True,
    }


def _notebooklm_create_space(*, title: str) -> Dict[str, Any]:
    if notebooklm_real_enabled():
        adapter = get_notebooklm_adapter()
        auth_json_raw = _resolve_notebooklm_auth_json()
        try:
            create_fn = adapter.create_notebook
            supports_auth_override = False
            try:
                params = inspect.signature(create_fn).parameters
                supports_auth_override = "auth_json_raw" in params
            except Exception:
                supports_auth_override = False
            if supports_auth_override:
                return create_fn(
                    title=title,
                    auth_json_raw=auth_json_raw,
                )
            return create_fn(title=title)
        except NotebookLMProviderError as e:
            raise SpaceProviderError(
                e.code,
                str(e),
                transient=bool(e.transient),
                degrade_provider=bool(e.degrade_provider),
            ) from e
    if not _notebooklm_stub_enabled():
        raise SpaceProviderError(
            "space_provider_disabled",
            "notebooklm provider adapter is not configured in this build",
            transient=False,
            degrade_provider=True,
        )
    return {
        "provider": "notebooklm",
        "remote_space_id": f"nb_stub_{secrets.token_hex(8)}",
        "title": str(title or "CCCC Space"),
        "created": True,
        "stub": True,
    }


def _notebooklm_list_spaces() -> Dict[str, Any]:
    if notebooklm_real_enabled():
        adapter = get_notebooklm_adapter()
        auth_json_raw = _resolve_notebooklm_auth_json()
        try:
            list_fn = adapter.list_notebooks
            supports_auth_override = False
            try:
                params = inspect.signature(list_fn).parameters
                supports_auth_override = "auth_json_raw" in params
            except Exception:
                supports_auth_override = False
            if supports_auth_override:
                return list_fn(auth_json_raw=auth_json_raw)
            return list_fn()
        except NotebookLMProviderError as e:
            raise SpaceProviderError(
                e.code,
                str(e),
                transient=bool(e.transient),
                degrade_provider=bool(e.degrade_provider),
            ) from e
    if not _notebooklm_stub_enabled():
        raise SpaceProviderError(
            "space_provider_disabled",
            "notebooklm provider adapter is not configured in this build",
            transient=False,
            degrade_provider=True,
        )
    return {
        "provider": "notebooklm",
        "spaces": [],
        "stub": True,
    }


def _notebooklm_list_sources(*, remote_space_id: str) -> Dict[str, Any]:
    if notebooklm_real_enabled():
        adapter = get_notebooklm_adapter()
        auth_json_raw = _resolve_notebooklm_auth_json()
        try:
            list_fn = adapter.list_sources
            supports_auth_override = False
            try:
                params = inspect.signature(list_fn).parameters
                supports_auth_override = "auth_json_raw" in params
            except Exception:
                supports_auth_override = False
            if supports_auth_override:
                return list_fn(
                    remote_space_id=remote_space_id,
                    auth_json_raw=auth_json_raw,
                )
            return list_fn(remote_space_id=remote_space_id)
        except NotebookLMProviderError as e:
            raise SpaceProviderError(
                e.code,
                str(e),
                transient=bool(e.transient),
                degrade_provider=bool(e.degrade_provider),
            ) from e
    if not _notebooklm_stub_enabled():
        raise SpaceProviderError(
            "space_provider_disabled",
            "notebooklm provider adapter is not configured in this build",
            transient=False,
            degrade_provider=True,
        )
    return {
        "provider": "notebooklm",
        "remote_space_id": str(remote_space_id or ""),
        "sources": [],
        "stub": True,
    }


def _notebooklm_get_source_fulltext(*, remote_space_id: str, source_id: str) -> Dict[str, Any]:
    if notebooklm_real_enabled():
        adapter = get_notebooklm_adapter()
        auth_json_raw = _resolve_notebooklm_auth_json()
        try:
            get_fn = adapter.get_source_fulltext
            supports_auth_override = False
            try:
                params = inspect.signature(get_fn).parameters
                supports_auth_override = "auth_json_raw" in params
            except Exception:
                supports_auth_override = False
            if supports_auth_override:
                return get_fn(
                    remote_space_id=remote_space_id,
                    source_id=source_id,
                    auth_json_raw=auth_json_raw,
                )
            return get_fn(
                remote_space_id=remote_space_id,
                source_id=source_id,
            )
        except NotebookLMProviderError as e:
            raise SpaceProviderError(
                e.code,
                str(e),
                transient=bool(e.transient),
                degrade_provider=bool(e.degrade_provider),
            ) from e
    if not _notebooklm_stub_enabled():
        raise SpaceProviderError(
            "space_provider_disabled",
            "notebooklm provider adapter is not configured in this build",
            transient=False,
            degrade_provider=True,
        )
    return {
        "provider": "notebooklm",
        "remote_space_id": str(remote_space_id or ""),
        "source_id": str(source_id or ""),
        "title": "",
        "kind": "",
        "url": "",
        "content": "",
        "char_count": 0,
        "stub": True,
    }


def _notebooklm_add_file_source(*, remote_space_id: str, file_path: str) -> Dict[str, Any]:
    if notebooklm_real_enabled():
        adapter = get_notebooklm_adapter()
        auth_json_raw = _resolve_notebooklm_auth_json()
        try:
            add_fn = adapter.add_file_source
            supports_auth_override = False
            try:
                params = inspect.signature(add_fn).parameters
                supports_auth_override = "auth_json_raw" in params
            except Exception:
                supports_auth_override = False
            if supports_auth_override:
                return add_fn(
                    remote_space_id=remote_space_id,
                    file_path=file_path,
                    auth_json_raw=auth_json_raw,
                )
            return add_fn(
                remote_space_id=remote_space_id,
                file_path=file_path,
            )
        except NotebookLMProviderError as e:
            raise SpaceProviderError(
                e.code,
                str(e),
                transient=bool(e.transient),
                degrade_provider=bool(e.degrade_provider),
            ) from e
    if not _notebooklm_stub_enabled():
        raise SpaceProviderError(
            "space_provider_disabled",
            "notebooklm provider adapter is not configured in this build",
            transient=False,
            degrade_provider=True,
        )
    return {
        "provider": "notebooklm",
        "remote_space_id": str(remote_space_id or ""),
        "source_id": f"src_stub_{secrets.token_hex(8)}",
        "title": os.path.basename(str(file_path or "")),
        "accepted": True,
        "stub": True,
    }


def _notebooklm_delete_source(*, remote_space_id: str, source_id: str) -> Dict[str, Any]:
    if notebooklm_real_enabled():
        adapter = get_notebooklm_adapter()
        auth_json_raw = _resolve_notebooklm_auth_json()
        try:
            delete_fn = adapter.delete_source
            supports_auth_override = False
            try:
                params = inspect.signature(delete_fn).parameters
                supports_auth_override = "auth_json_raw" in params
            except Exception:
                supports_auth_override = False
            if supports_auth_override:
                return delete_fn(
                    remote_space_id=remote_space_id,
                    source_id=source_id,
                    auth_json_raw=auth_json_raw,
                )
            return delete_fn(
                remote_space_id=remote_space_id,
                source_id=source_id,
            )
        except NotebookLMProviderError as e:
            raise SpaceProviderError(
                e.code,
                str(e),
                transient=bool(e.transient),
                degrade_provider=bool(e.degrade_provider),
            ) from e
    if not _notebooklm_stub_enabled():
        raise SpaceProviderError(
            "space_provider_disabled",
            "notebooklm provider adapter is not configured in this build",
            transient=False,
            degrade_provider=True,
        )
    return {
        "provider": "notebooklm",
        "remote_space_id": str(remote_space_id or ""),
        "source_id": str(source_id or ""),
        "deleted": True,
        "stub": True,
    }


def _notebooklm_rename_source(*, remote_space_id: str, source_id: str, new_title: str) -> Dict[str, Any]:
    if notebooklm_real_enabled():
        adapter = get_notebooklm_adapter()
        auth_json_raw = _resolve_notebooklm_auth_json()
        try:
            rename_fn = adapter.rename_source
            supports_auth_override = False
            try:
                params = inspect.signature(rename_fn).parameters
                supports_auth_override = "auth_json_raw" in params
            except Exception:
                supports_auth_override = False
            if supports_auth_override:
                return rename_fn(
                    remote_space_id=remote_space_id,
                    source_id=source_id,
                    new_title=new_title,
                    auth_json_raw=auth_json_raw,
                )
            return rename_fn(
                remote_space_id=remote_space_id,
                source_id=source_id,
                new_title=new_title,
            )
        except NotebookLMProviderError as e:
            raise SpaceProviderError(
                e.code,
                str(e),
                transient=bool(e.transient),
                degrade_provider=bool(e.degrade_provider),
            ) from e
    if not _notebooklm_stub_enabled():
        raise SpaceProviderError(
            "space_provider_disabled",
            "notebooklm provider adapter is not configured in this build",
            transient=False,
            degrade_provider=True,
        )
    return {
        "provider": "notebooklm",
        "remote_space_id": str(remote_space_id or ""),
        "source_id": str(source_id or ""),
        "title": str(new_title or ""),
        "renamed": True,
        "stub": True,
    }


def _notebooklm_refresh_source(*, remote_space_id: str, source_id: str) -> Dict[str, Any]:
    if notebooklm_real_enabled():
        adapter = get_notebooklm_adapter()
        auth_json_raw = _resolve_notebooklm_auth_json()
        try:
            refresh_fn = adapter.refresh_source
            supports_auth_override = False
            try:
                params = inspect.signature(refresh_fn).parameters
                supports_auth_override = "auth_json_raw" in params
            except Exception:
                supports_auth_override = False
            if supports_auth_override:
                return refresh_fn(
                    remote_space_id=remote_space_id,
                    source_id=source_id,
                    auth_json_raw=auth_json_raw,
                )
            return refresh_fn(
                remote_space_id=remote_space_id,
                source_id=source_id,
            )
        except NotebookLMProviderError as e:
            raise SpaceProviderError(
                e.code,
                str(e),
                transient=bool(e.transient),
                degrade_provider=bool(e.degrade_provider),
            ) from e
    if not _notebooklm_stub_enabled():
        raise SpaceProviderError(
            "space_provider_disabled",
            "notebooklm provider adapter is not configured in this build",
            transient=False,
            degrade_provider=True,
        )
    return {
        "provider": "notebooklm",
        "remote_space_id": str(remote_space_id or ""),
        "source_id": str(source_id or ""),
        "refreshed": True,
        "stub": True,
    }


def _notebooklm_list_artifacts(*, remote_space_id: str, kind: str = "") -> Dict[str, Any]:
    if notebooklm_real_enabled():
        adapter = get_notebooklm_adapter()
        auth_json_raw = _resolve_notebooklm_auth_json()
        try:
            list_fn = adapter.list_artifacts
            supports_auth_override = False
            try:
                params = inspect.signature(list_fn).parameters
                supports_auth_override = "auth_json_raw" in params
            except Exception:
                supports_auth_override = False
            if supports_auth_override:
                return list_fn(
                    remote_space_id=remote_space_id,
                    kind=kind,
                    auth_json_raw=auth_json_raw,
                )
            return list_fn(remote_space_id=remote_space_id, kind=kind)
        except NotebookLMProviderError as e:
            raise SpaceProviderError(
                e.code,
                str(e),
                transient=bool(e.transient),
                degrade_provider=bool(e.degrade_provider),
            ) from e
    if not _notebooklm_stub_enabled():
        raise SpaceProviderError(
            "space_provider_disabled",
            "notebooklm provider adapter is not configured in this build",
            transient=False,
            degrade_provider=True,
        )
    return {
        "provider": "notebooklm",
        "remote_space_id": str(remote_space_id or ""),
        "kind": str(kind or ""),
        "artifacts": [],
        "stub": True,
    }


def _notebooklm_generate_artifact(*, remote_space_id: str, kind: str, options: Dict[str, Any]) -> Dict[str, Any]:
    if notebooklm_real_enabled():
        adapter = get_notebooklm_adapter()
        auth_json_raw = _resolve_notebooklm_auth_json()
        try:
            generate_fn = adapter.generate_artifact
            supports_auth_override = False
            try:
                params = inspect.signature(generate_fn).parameters
                supports_auth_override = "auth_json_raw" in params
            except Exception:
                supports_auth_override = False
            if supports_auth_override:
                return generate_fn(
                    remote_space_id=remote_space_id,
                    kind=kind,
                    options=options,
                    auth_json_raw=auth_json_raw,
                )
            return generate_fn(
                remote_space_id=remote_space_id,
                kind=kind,
                options=options,
            )
        except NotebookLMProviderError as e:
            raise SpaceProviderError(
                e.code,
                str(e),
                transient=bool(e.transient),
                degrade_provider=bool(e.degrade_provider),
            ) from e
    if not _notebooklm_stub_enabled():
        raise SpaceProviderError(
            "space_provider_disabled",
            "notebooklm provider adapter is not configured in this build",
            transient=False,
            degrade_provider=True,
        )
    return {
        "provider": "notebooklm",
        "remote_space_id": str(remote_space_id or ""),
        "kind": str(kind or ""),
        "task_id": f"art_stub_{secrets.token_hex(8)}",
        "status": "completed",
        "stub": True,
    }


def _notebooklm_wait_artifact(
    *,
    remote_space_id: str,
    task_id: str,
    timeout_seconds: float,
    initial_interval: float,
    max_interval: float,
) -> Dict[str, Any]:
    if notebooklm_real_enabled():
        adapter = get_notebooklm_adapter()
        auth_json_raw = _resolve_notebooklm_auth_json()
        try:
            wait_fn = adapter.wait_artifact
            supports_auth_override = False
            try:
                params = inspect.signature(wait_fn).parameters
                supports_auth_override = "auth_json_raw" in params
            except Exception:
                supports_auth_override = False
            if supports_auth_override:
                return wait_fn(
                    remote_space_id=remote_space_id,
                    task_id=task_id,
                    timeout_seconds=timeout_seconds,
                    initial_interval=initial_interval,
                    max_interval=max_interval,
                    auth_json_raw=auth_json_raw,
                )
            return wait_fn(
                remote_space_id=remote_space_id,
                task_id=task_id,
                timeout_seconds=timeout_seconds,
                initial_interval=initial_interval,
                max_interval=max_interval,
            )
        except NotebookLMProviderError as e:
            raise SpaceProviderError(
                e.code,
                str(e),
                transient=bool(e.transient),
                degrade_provider=bool(e.degrade_provider),
            ) from e
    if not _notebooklm_stub_enabled():
        raise SpaceProviderError(
            "space_provider_disabled",
            "notebooklm provider adapter is not configured in this build",
            transient=False,
            degrade_provider=True,
        )
    return {
        "provider": "notebooklm",
        "remote_space_id": str(remote_space_id or ""),
        "task_id": str(task_id or ""),
        "status": "completed",
        "stub": True,
    }


def _notebooklm_download_artifact(
    *,
    remote_space_id: str,
    kind: str,
    output_path: str,
    artifact_id: str = "",
    output_format: str = "",
) -> Dict[str, Any]:
    if notebooklm_real_enabled():
        adapter = get_notebooklm_adapter()
        auth_json_raw = _resolve_notebooklm_auth_json()
        try:
            download_fn = adapter.download_artifact
            supports_auth_override = False
            try:
                params = inspect.signature(download_fn).parameters
                supports_auth_override = "auth_json_raw" in params
            except Exception:
                supports_auth_override = False
            if supports_auth_override:
                return download_fn(
                    remote_space_id=remote_space_id,
                    kind=kind,
                    output_path=output_path,
                    artifact_id=artifact_id,
                    output_format=output_format,
                    auth_json_raw=auth_json_raw,
                )
            return download_fn(
                remote_space_id=remote_space_id,
                kind=kind,
                output_path=output_path,
                artifact_id=artifact_id,
                output_format=output_format,
            )
        except NotebookLMProviderError as e:
            raise SpaceProviderError(
                e.code,
                str(e),
                transient=bool(e.transient),
                degrade_provider=bool(e.degrade_provider),
            ) from e
    if not _notebooklm_stub_enabled():
        raise SpaceProviderError(
            "space_provider_disabled",
            "notebooklm provider adapter is not configured in this build",
            transient=False,
            degrade_provider=True,
        )
    target = str(output_path or "").strip()
    if not target:
        raise SpaceProviderError(
            "space_job_invalid",
            "output_path is required",
            transient=False,
            degrade_provider=False,
        )
    try:
        os.makedirs(os.path.dirname(target) or ".", exist_ok=True)
        with open(target, "w", encoding="utf-8") as f:
            f.write(f"[stub artifact] kind={kind} artifact_id={artifact_id}\n")
    except Exception as e:
        raise SpaceProviderError(
            "space_provider_upstream_error",
            f"failed to write stub artifact file: {e}",
            transient=False,
            degrade_provider=False,
        ) from e
    return {
        "provider": "notebooklm",
        "remote_space_id": str(remote_space_id or ""),
        "kind": str(kind or ""),
        "artifact_id": str(artifact_id or ""),
        "output_path": target,
        "downloaded": True,
        "stub": True,
    }


def provider_ingest(
    provider: str,
    *,
    remote_space_id: str,
    kind: str,
    payload: Dict[str, Any],
) -> Dict[str, Any]:
    pid = str(provider or "").strip() or "notebooklm"
    if pid == "notebooklm":
        return _notebooklm_ingest(remote_space_id=remote_space_id, kind=kind, payload=payload)
    raise SpaceProviderError("space_job_invalid", f"unsupported provider: {pid}")


def provider_query(
    provider: str,
    *,
    remote_space_id: str,
    query: str,
    options: Dict[str, Any],
) -> Dict[str, Any]:
    pid = str(provider or "").strip() or "notebooklm"
    if pid == "notebooklm":
        return _notebooklm_query(remote_space_id=remote_space_id, query=query, options=options)
    raise SpaceProviderError("space_job_invalid", f"unsupported provider: {pid}")


def provider_create_space(
    provider: str,
    *,
    title: str,
) -> Dict[str, Any]:
    pid = str(provider or "").strip() or "notebooklm"
    if pid == "notebooklm":
        return _notebooklm_create_space(title=title)
    raise SpaceProviderError("space_job_invalid", f"unsupported provider: {pid}")


def provider_list_spaces(provider: str) -> Dict[str, Any]:
    pid = str(provider or "").strip() or "notebooklm"
    if pid == "notebooklm":
        return _notebooklm_list_spaces()
    raise SpaceProviderError("space_job_invalid", f"unsupported provider: {pid}")


def provider_list_sources(
    provider: str,
    *,
    remote_space_id: str,
) -> Dict[str, Any]:
    pid = str(provider or "").strip() or "notebooklm"
    if pid == "notebooklm":
        return _notebooklm_list_sources(remote_space_id=remote_space_id)
    raise SpaceProviderError("space_job_invalid", f"unsupported provider: {pid}")


def provider_get_source_fulltext(
    provider: str,
    *,
    remote_space_id: str,
    source_id: str,
) -> Dict[str, Any]:
    pid = str(provider or "").strip() or "notebooklm"
    if pid == "notebooklm":
        return _notebooklm_get_source_fulltext(remote_space_id=remote_space_id, source_id=source_id)
    raise SpaceProviderError("space_job_invalid", f"unsupported provider: {pid}")


def provider_add_file_source(
    provider: str,
    *,
    remote_space_id: str,
    file_path: str,
) -> Dict[str, Any]:
    pid = str(provider or "").strip() or "notebooklm"
    if pid == "notebooklm":
        return _notebooklm_add_file_source(remote_space_id=remote_space_id, file_path=file_path)
    raise SpaceProviderError("space_job_invalid", f"unsupported provider: {pid}")


def provider_delete_source(
    provider: str,
    *,
    remote_space_id: str,
    source_id: str,
) -> Dict[str, Any]:
    pid = str(provider or "").strip() or "notebooklm"
    if pid == "notebooklm":
        return _notebooklm_delete_source(remote_space_id=remote_space_id, source_id=source_id)
    raise SpaceProviderError("space_job_invalid", f"unsupported provider: {pid}")


def provider_rename_source(
    provider: str,
    *,
    remote_space_id: str,
    source_id: str,
    new_title: str,
) -> Dict[str, Any]:
    pid = str(provider or "").strip() or "notebooklm"
    if pid == "notebooklm":
        return _notebooklm_rename_source(remote_space_id=remote_space_id, source_id=source_id, new_title=new_title)
    raise SpaceProviderError("space_job_invalid", f"unsupported provider: {pid}")


def provider_refresh_source(
    provider: str,
    *,
    remote_space_id: str,
    source_id: str,
) -> Dict[str, Any]:
    pid = str(provider or "").strip() or "notebooklm"
    if pid == "notebooklm":
        return _notebooklm_refresh_source(remote_space_id=remote_space_id, source_id=source_id)
    raise SpaceProviderError("space_job_invalid", f"unsupported provider: {pid}")


def provider_list_artifacts(
    provider: str,
    *,
    remote_space_id: str,
    kind: str = "",
) -> Dict[str, Any]:
    pid = str(provider or "").strip() or "notebooklm"
    if pid == "notebooklm":
        return _notebooklm_list_artifacts(remote_space_id=remote_space_id, kind=kind)
    raise SpaceProviderError("space_job_invalid", f"unsupported provider: {pid}")


def provider_generate_artifact(
    provider: str,
    *,
    remote_space_id: str,
    kind: str,
    options: Dict[str, Any],
) -> Dict[str, Any]:
    pid = str(provider or "").strip() or "notebooklm"
    if pid == "notebooklm":
        return _notebooklm_generate_artifact(remote_space_id=remote_space_id, kind=kind, options=options)
    raise SpaceProviderError("space_job_invalid", f"unsupported provider: {pid}")


def provider_wait_artifact(
    provider: str,
    *,
    remote_space_id: str,
    task_id: str,
    timeout_seconds: float,
    initial_interval: float,
    max_interval: float,
) -> Dict[str, Any]:
    pid = str(provider or "").strip() or "notebooklm"
    if pid == "notebooklm":
        return _notebooklm_wait_artifact(
            remote_space_id=remote_space_id,
            task_id=task_id,
            timeout_seconds=timeout_seconds,
            initial_interval=initial_interval,
            max_interval=max_interval,
        )
    raise SpaceProviderError("space_job_invalid", f"unsupported provider: {pid}")


def provider_download_artifact(
    provider: str,
    *,
    remote_space_id: str,
    kind: str,
    output_path: str,
    artifact_id: str = "",
    output_format: str = "",
) -> Dict[str, Any]:
    pid = str(provider or "").strip() or "notebooklm"
    if pid == "notebooklm":
        return _notebooklm_download_artifact(
            remote_space_id=remote_space_id,
            kind=kind,
            output_path=output_path,
            artifact_id=artifact_id,
            output_format=output_format,
        )
    raise SpaceProviderError("space_job_invalid", f"unsupported provider: {pid}")
