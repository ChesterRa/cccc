from __future__ import annotations

import asyncio
from contextlib import contextmanager
import json
import os
import threading
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, Mapping
from urllib.parse import urlparse

from .errors import NotebookLMProviderError
from .health import notebooklm_health_check, validate_notebooklm_auth_json


class NotebookLMAdapter:
    """Minimal provider adapter contract for Group Space M2 wiring.

    M2 scope:
    - validate enable/auth/compat preconditions
    - perform real ingest/query calls via vendored notebooklm-py boundary
    - preserve strict error typing for daemon-level degrade semantics
    """

    provider_id = "notebooklm"

    def health_check(self, auth_json_raw: str | None = None) -> Dict[str, Any]:
        return notebooklm_health_check(auth_json_raw=auth_json_raw)

    def ingest(
        self,
        *,
        remote_space_id: str,
        kind: str,
        payload: Dict[str, Any],
        auth_json_raw: str | None = None,
    ) -> Dict[str, Any]:
        self.health_check(auth_json_raw=auth_json_raw)
        notebook_id = str(remote_space_id or "").strip()
        if not notebook_id:
            raise NotebookLMProviderError(
                code="space_provider_not_configured",
                message="missing remote_space_id (NotebookLM notebook id)",
                transient=False,
                degrade_provider=True,
            )

        auth_payload = validate_notebooklm_auth_json(auth_json_raw=auth_json_raw)
        normalized_kind = str(kind or "context_sync").strip() or "context_sync"
        try:
            return _run_with_vendor_auth(
                auth_payload,
                _ingest_async(
                    notebook_id=notebook_id,
                    kind=normalized_kind,
                    payload=dict(payload or {}),
                    auth_payload=auth_payload,
                    timeout_seconds=_read_timeout_seconds(),
                ),
            )
        except Exception as e:
            raise _map_vendor_exception(e) from e

    def query(
        self,
        *,
        remote_space_id: str,
        query: str,
        options: Dict[str, Any],
        auth_json_raw: str | None = None,
    ) -> Dict[str, Any]:
        self.health_check(auth_json_raw=auth_json_raw)
        notebook_id = str(remote_space_id or "").strip()
        if not notebook_id:
            raise NotebookLMProviderError(
                code="space_provider_not_configured",
                message="missing remote_space_id (NotebookLM notebook id)",
                transient=False,
                degrade_provider=True,
            )
        question = str(query or "").strip()
        if not question:
            raise NotebookLMProviderError(
                code="space_job_invalid",
                message="query is required",
                transient=False,
                degrade_provider=False,
            )
        auth_payload = validate_notebooklm_auth_json(auth_json_raw=auth_json_raw)
        safe_options = dict(options or {})
        try:
            return _run_with_vendor_auth(
                auth_payload,
                _query_async(
                    notebook_id=notebook_id,
                    query=question,
                    options=safe_options,
                    auth_payload=auth_payload,
                    timeout_seconds=_read_timeout_seconds(),
                ),
            )
        except Exception as e:
            raise _map_vendor_exception(e) from e

    def create_notebook(
        self,
        *,
        title: str,
        auth_json_raw: str | None = None,
    ) -> Dict[str, Any]:
        self.health_check(auth_json_raw=auth_json_raw)
        notebook_title = str(title or "").strip() or "CCCC Space"
        auth_payload = validate_notebooklm_auth_json(auth_json_raw=auth_json_raw)
        try:
            return _run_with_vendor_auth(
                auth_payload,
                _create_notebook_async(
                    title=notebook_title,
                    auth_payload=auth_payload,
                    timeout_seconds=_read_timeout_seconds(),
                ),
            )
        except Exception as e:
            raise _map_vendor_exception(e) from e

    def list_notebooks(
        self,
        *,
        auth_json_raw: str | None = None,
    ) -> Dict[str, Any]:
        self.health_check(auth_json_raw=auth_json_raw)
        auth_payload = validate_notebooklm_auth_json(auth_json_raw=auth_json_raw)
        try:
            return _run_with_vendor_auth(
                auth_payload,
                _list_notebooks_async(
                    auth_payload=auth_payload,
                    timeout_seconds=_read_timeout_seconds(),
                ),
            )
        except Exception as e:
            raise _map_vendor_exception(e) from e

    def list_sources(
        self,
        *,
        remote_space_id: str,
        auth_json_raw: str | None = None,
    ) -> Dict[str, Any]:
        self.health_check(auth_json_raw=auth_json_raw)
        notebook_id = str(remote_space_id or "").strip()
        if not notebook_id:
            raise NotebookLMProviderError(
                code="space_provider_not_configured",
                message="missing remote_space_id (NotebookLM notebook id)",
                transient=False,
                degrade_provider=True,
            )
        auth_payload = validate_notebooklm_auth_json(auth_json_raw=auth_json_raw)
        try:
            return _run_with_vendor_auth(
                auth_payload,
                _list_sources_async(
                    notebook_id=notebook_id,
                    auth_payload=auth_payload,
                    timeout_seconds=_read_timeout_seconds(),
                ),
            )
        except Exception as e:
            raise _map_vendor_exception(e) from e

    def get_source_fulltext(
        self,
        *,
        remote_space_id: str,
        source_id: str,
        auth_json_raw: str | None = None,
    ) -> Dict[str, Any]:
        self.health_check(auth_json_raw=auth_json_raw)
        notebook_id = str(remote_space_id or "").strip()
        sid = str(source_id or "").strip()
        if not notebook_id:
            raise NotebookLMProviderError(
                code="space_provider_not_configured",
                message="missing remote_space_id (NotebookLM notebook id)",
                transient=False,
                degrade_provider=True,
            )
        if not sid:
            raise NotebookLMProviderError(
                code="space_job_invalid",
                message="source_id is required",
                transient=False,
                degrade_provider=False,
            )
        auth_payload = validate_notebooklm_auth_json(auth_json_raw=auth_json_raw)
        try:
            return _run_with_vendor_auth(
                auth_payload,
                _get_source_fulltext_async(
                    notebook_id=notebook_id,
                    source_id=sid,
                    auth_payload=auth_payload,
                    timeout_seconds=_read_timeout_seconds(),
                ),
            )
        except Exception as e:
            raise _map_vendor_exception(e) from e

    def add_file_source(
        self,
        *,
        remote_space_id: str,
        file_path: str,
        auth_json_raw: str | None = None,
    ) -> Dict[str, Any]:
        self.health_check(auth_json_raw=auth_json_raw)
        notebook_id = str(remote_space_id or "").strip()
        if not notebook_id:
            raise NotebookLMProviderError(
                code="space_provider_not_configured",
                message="missing remote_space_id (NotebookLM notebook id)",
                transient=False,
                degrade_provider=True,
            )
        target = str(file_path or "").strip()
        if not target:
            raise NotebookLMProviderError(
                code="space_job_invalid",
                message="file_path is required",
                transient=False,
                degrade_provider=False,
            )
        auth_payload = validate_notebooklm_auth_json(auth_json_raw=auth_json_raw)
        try:
            return _run_with_vendor_auth(
                auth_payload,
                _add_file_source_async(
                    notebook_id=notebook_id,
                    file_path=target,
                    auth_payload=auth_payload,
                    timeout_seconds=_read_timeout_seconds(),
                ),
            )
        except Exception as e:
            raise _map_vendor_exception(e) from e

    def delete_source(
        self,
        *,
        remote_space_id: str,
        source_id: str,
        auth_json_raw: str | None = None,
    ) -> Dict[str, Any]:
        self.health_check(auth_json_raw=auth_json_raw)
        notebook_id = str(remote_space_id or "").strip()
        sid = str(source_id or "").strip()
        if not notebook_id:
            raise NotebookLMProviderError(
                code="space_provider_not_configured",
                message="missing remote_space_id (NotebookLM notebook id)",
                transient=False,
                degrade_provider=True,
            )
        if not sid:
            raise NotebookLMProviderError(
                code="space_job_invalid",
                message="source_id is required",
                transient=False,
                degrade_provider=False,
            )
        auth_payload = validate_notebooklm_auth_json(auth_json_raw=auth_json_raw)
        try:
            return _run_with_vendor_auth(
                auth_payload,
                _delete_source_async(
                    notebook_id=notebook_id,
                    source_id=sid,
                    auth_payload=auth_payload,
                    timeout_seconds=_read_timeout_seconds(),
                ),
            )
        except Exception as e:
            raise _map_vendor_exception(e) from e

    def rename_source(
        self,
        *,
        remote_space_id: str,
        source_id: str,
        new_title: str,
        auth_json_raw: str | None = None,
    ) -> Dict[str, Any]:
        self.health_check(auth_json_raw=auth_json_raw)
        notebook_id = str(remote_space_id or "").strip()
        sid = str(source_id or "").strip()
        title = str(new_title or "").strip()
        if not notebook_id:
            raise NotebookLMProviderError(
                code="space_provider_not_configured",
                message="missing remote_space_id (NotebookLM notebook id)",
                transient=False,
                degrade_provider=True,
            )
        if not sid:
            raise NotebookLMProviderError(
                code="space_job_invalid",
                message="source_id is required",
                transient=False,
                degrade_provider=False,
            )
        if not title:
            raise NotebookLMProviderError(
                code="space_job_invalid",
                message="new_title is required",
                transient=False,
                degrade_provider=False,
            )
        auth_payload = validate_notebooklm_auth_json(auth_json_raw=auth_json_raw)
        try:
            return _run_with_vendor_auth(
                auth_payload,
                _rename_source_async(
                    notebook_id=notebook_id,
                    source_id=sid,
                    new_title=title,
                    auth_payload=auth_payload,
                    timeout_seconds=_read_timeout_seconds(),
                ),
            )
        except Exception as e:
            raise _map_vendor_exception(e) from e

    def refresh_source(
        self,
        *,
        remote_space_id: str,
        source_id: str,
        auth_json_raw: str | None = None,
    ) -> Dict[str, Any]:
        self.health_check(auth_json_raw=auth_json_raw)
        notebook_id = str(remote_space_id or "").strip()
        sid = str(source_id or "").strip()
        if not notebook_id:
            raise NotebookLMProviderError(
                code="space_provider_not_configured",
                message="missing remote_space_id (NotebookLM notebook id)",
                transient=False,
                degrade_provider=True,
            )
        if not sid:
            raise NotebookLMProviderError(
                code="space_job_invalid",
                message="source_id is required",
                transient=False,
                degrade_provider=False,
            )
        auth_payload = validate_notebooklm_auth_json(auth_json_raw=auth_json_raw)
        try:
            return _run_with_vendor_auth(
                auth_payload,
                _refresh_source_async(
                    notebook_id=notebook_id,
                    source_id=sid,
                    auth_payload=auth_payload,
                    timeout_seconds=_read_timeout_seconds(),
                ),
            )
        except Exception as e:
            raise _map_vendor_exception(e) from e

    def list_artifacts(
        self,
        *,
        remote_space_id: str,
        kind: str = "",
        auth_json_raw: str | None = None,
    ) -> Dict[str, Any]:
        self.health_check(auth_json_raw=auth_json_raw)
        notebook_id = str(remote_space_id or "").strip()
        if not notebook_id:
            raise NotebookLMProviderError(
                code="space_provider_not_configured",
                message="missing remote_space_id (NotebookLM notebook id)",
                transient=False,
                degrade_provider=True,
            )
        auth_payload = validate_notebooklm_auth_json(auth_json_raw=auth_json_raw)
        try:
            return _run_with_vendor_auth(
                auth_payload,
                _list_artifacts_async(
                    notebook_id=notebook_id,
                    kind=str(kind or "").strip().lower(),
                    auth_payload=auth_payload,
                    timeout_seconds=_read_timeout_seconds(),
                ),
            )
        except Exception as e:
            raise _map_vendor_exception(e) from e

    def generate_artifact(
        self,
        *,
        remote_space_id: str,
        kind: str,
        options: Dict[str, Any],
        auth_json_raw: str | None = None,
    ) -> Dict[str, Any]:
        self.health_check(auth_json_raw=auth_json_raw)
        notebook_id = str(remote_space_id or "").strip()
        if not notebook_id:
            raise NotebookLMProviderError(
                code="space_provider_not_configured",
                message="missing remote_space_id (NotebookLM notebook id)",
                transient=False,
                degrade_provider=True,
            )
        artifact_kind = str(kind or "").strip().lower()
        if not artifact_kind:
            raise NotebookLMProviderError(
                code="space_job_invalid",
                message="kind is required",
                transient=False,
                degrade_provider=False,
            )
        auth_payload = validate_notebooklm_auth_json(auth_json_raw=auth_json_raw)
        try:
            return _run_with_vendor_auth(
                auth_payload,
                _generate_artifact_async(
                    notebook_id=notebook_id,
                    kind=artifact_kind,
                    options=dict(options or {}),
                    auth_payload=auth_payload,
                    timeout_seconds=_read_timeout_seconds(),
                ),
            )
        except Exception as e:
            raise _map_vendor_exception(e) from e

    def wait_artifact(
        self,
        *,
        remote_space_id: str,
        task_id: str,
        timeout_seconds: float = 600.0,
        initial_interval: float = 2.0,
        max_interval: float = 10.0,
        auth_json_raw: str | None = None,
    ) -> Dict[str, Any]:
        self.health_check(auth_json_raw=auth_json_raw)
        notebook_id = str(remote_space_id or "").strip()
        tid = str(task_id or "").strip()
        if not notebook_id:
            raise NotebookLMProviderError(
                code="space_provider_not_configured",
                message="missing remote_space_id (NotebookLM notebook id)",
                transient=False,
                degrade_provider=True,
            )
        if not tid:
            raise NotebookLMProviderError(
                code="space_job_invalid",
                message="task_id is required",
                transient=False,
                degrade_provider=False,
            )
        auth_payload = validate_notebooklm_auth_json(auth_json_raw=auth_json_raw)
        try:
            return _run_with_vendor_auth(
                auth_payload,
                _wait_artifact_async(
                    notebook_id=notebook_id,
                    task_id=tid,
                    timeout_seconds=float(timeout_seconds or 600.0),
                    initial_interval=float(initial_interval or 2.0),
                    max_interval=float(max_interval or 10.0),
                    auth_payload=auth_payload,
                    timeout_seconds_for_client=_read_timeout_seconds(),
                ),
            )
        except Exception as e:
            raise _map_vendor_exception(e) from e

    def download_artifact(
        self,
        *,
        remote_space_id: str,
        kind: str,
        output_path: str,
        artifact_id: str = "",
        output_format: str = "",
        auth_json_raw: str | None = None,
    ) -> Dict[str, Any]:
        self.health_check(auth_json_raw=auth_json_raw)
        notebook_id = str(remote_space_id or "").strip()
        artifact_kind = str(kind or "").strip().lower()
        target = str(output_path or "").strip()
        aid = str(artifact_id or "").strip()
        fmt = str(output_format or "").strip().lower()
        if not notebook_id:
            raise NotebookLMProviderError(
                code="space_provider_not_configured",
                message="missing remote_space_id (NotebookLM notebook id)",
                transient=False,
                degrade_provider=True,
            )
        if not artifact_kind:
            raise NotebookLMProviderError(
                code="space_job_invalid",
                message="kind is required",
                transient=False,
                degrade_provider=False,
            )
        if not target:
            raise NotebookLMProviderError(
                code="space_job_invalid",
                message="output_path is required",
                transient=False,
                degrade_provider=False,
            )
        auth_payload = validate_notebooklm_auth_json(auth_json_raw=auth_json_raw)
        try:
            return _run_with_vendor_auth(
                auth_payload,
                _download_artifact_async(
                    notebook_id=notebook_id,
                    kind=artifact_kind,
                    output_path=target,
                    artifact_id=aid,
                    output_format=fmt,
                    auth_payload=auth_payload,
                    timeout_seconds=_read_timeout_seconds(),
                ),
            )
        except Exception as e:
            raise _map_vendor_exception(e) from e


def _read_timeout_seconds() -> float:
    raw = str(os.environ.get("CCCC_NOTEBOOKLM_TIMEOUT") or "").strip()
    if not raw:
        return 30.0
    try:
        timeout = float(raw)
    except Exception:
        return 30.0
    if timeout <= 0:
        return 30.0
    return min(timeout, 300.0)


_VENDOR_AUTH_ENV_LOCK = threading.Lock()
_VENDOR_AUTH_ENV_ACTIVE = 0
_VENDOR_AUTH_ENV_PREV_PRESENT = False
_VENDOR_AUTH_ENV_PREV_VALUE = ""


@contextmanager
def _vendor_auth_env(auth_payload: Dict[str, Any]):
    global _VENDOR_AUTH_ENV_ACTIVE, _VENDOR_AUTH_ENV_PREV_PRESENT, _VENDOR_AUTH_ENV_PREV_VALUE
    auth_json = json.dumps(auth_payload, ensure_ascii=False)
    with _VENDOR_AUTH_ENV_LOCK:
        if _VENDOR_AUTH_ENV_ACTIVE == 0:
            _VENDOR_AUTH_ENV_PREV_PRESENT = "NOTEBOOKLM_AUTH_JSON" in os.environ
            _VENDOR_AUTH_ENV_PREV_VALUE = str(os.environ.get("NOTEBOOKLM_AUTH_JSON") or "")
        _VENDOR_AUTH_ENV_ACTIVE += 1
        os.environ["NOTEBOOKLM_AUTH_JSON"] = auth_json
    try:
        yield
    finally:
        with _VENDOR_AUTH_ENV_LOCK:
            _VENDOR_AUTH_ENV_ACTIVE = max(0, _VENDOR_AUTH_ENV_ACTIVE - 1)
            if _VENDOR_AUTH_ENV_ACTIVE == 0:
                if _VENDOR_AUTH_ENV_PREV_PRESENT:
                    os.environ["NOTEBOOKLM_AUTH_JSON"] = _VENDOR_AUTH_ENV_PREV_VALUE
                else:
                    os.environ.pop("NOTEBOOKLM_AUTH_JSON", None)


def _run_coroutine_sync(coro):
    try:
        asyncio.get_running_loop()
    except RuntimeError:
        return asyncio.run(coro)

    result_holder: Dict[str, Any] = {}
    error_holder: Dict[str, BaseException] = {}

    def _runner() -> None:
        try:
            result_holder["value"] = asyncio.run(coro)
        except BaseException as e:  # pragma: no cover - exercised through caller paths
            error_holder["error"] = e

    thread = threading.Thread(target=_runner, daemon=True)
    thread.start()
    thread.join()
    if "error" in error_holder:
        raise error_holder["error"]
    return result_holder.get("value")


def _run_with_vendor_auth(auth_payload: Dict[str, Any], coro: Any) -> Any:
    with _vendor_auth_env(auth_payload):
        return _run_coroutine_sync(coro)


def _normalize_ingest_title(*, kind: str, payload: Mapping[str, Any]) -> str:
    title = str(payload.get("title") or "").strip()
    if title:
        return title[:120]
    stamp = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M:%SZ")
    return f"CCCC {kind} {stamp}"


_RESOURCE_DRIVE_MIME: Dict[str, str] = {
    "google_docs": "application/vnd.google-apps.document",
    "google_slides": "application/vnd.google-apps.presentation",
    "google_spreadsheet": "application/vnd.google-apps.spreadsheet",
}


def _looks_like_youtube_url(url: str) -> bool:
    text = str(url or "").strip()
    if not text:
        return False
    try:
        host = str(urlparse(text).hostname or "").strip().lower()
    except Exception:
        return False
    if not host:
        return False
    return host == "youtu.be" or host == "youtube.com" or host.endswith(".youtube.com")


def _normalize_resource_source_type(payload: Mapping[str, Any]) -> str:
    raw = str(payload.get("source_type") or payload.get("type") or "").strip().lower()
    alias = {
        "file": "file",
        "local_file": "file",
        "path": "file",
        "url": "web_page",
        "web_page": "web_page",
        "youtube": "youtube",
        "text": "pasted_text",
        "pasted_text": "pasted_text",
        "google_doc": "google_docs",
        "google_docs": "google_docs",
        "drive_doc": "google_docs",
        "google_slide": "google_slides",
        "google_slides": "google_slides",
        "drive_slide": "google_slides",
        "google_sheet": "google_spreadsheet",
        "google_sheets": "google_spreadsheet",
        "google_spreadsheet": "google_spreadsheet",
        "drive_sheet": "google_spreadsheet",
    }
    normalized = alias.get(raw, "")
    if normalized:
        return normalized

    file_path = str(payload.get("file_path") or payload.get("path") or "").strip()
    if file_path:
        return "file"

    url = str(payload.get("url") or "").strip()
    if url:
        return "youtube" if _looks_like_youtube_url(url) else "web_page"

    file_id = str(payload.get("file_id") or "").strip()
    if file_id:
        mime_hint = str(payload.get("mime_type") or "").strip().lower()
        if "presentation" in mime_hint:
            return "google_slides"
        if "spreadsheet" in mime_hint or "sheets" in mime_hint:
            return "google_spreadsheet"
        return "google_docs"

    if str(payload.get("content") or "").strip() or str(payload.get("text") or "").strip():
        return "pasted_text"
    return "pasted_text"


def _payload_to_text(payload: Mapping[str, Any]) -> str:
    if isinstance(payload.get("content"), str) and str(payload.get("content") or "").strip():
        return str(payload.get("content"))
    if isinstance(payload.get("text"), str) and str(payload.get("text") or "").strip():
        return str(payload.get("text"))
    return json.dumps(dict(payload), ensure_ascii=False, indent=2, sort_keys=True)


def _extract_query_source_ids(options: Mapping[str, Any]) -> list[str] | None:
    raw = options.get("source_ids")
    if not isinstance(raw, list):
        return None
    source_ids: list[str] = []
    for item in raw:
        sid = str(item or "").strip()
        if sid:
            source_ids.append(sid)
    return source_ids or None


async def _build_client(*, auth_payload: Dict[str, Any], timeout_seconds: float):
    from ._vendor.notebooklm.auth import AuthTokens, extract_cookies_from_storage, fetch_tokens
    from ._vendor.notebooklm.client import NotebookLMClient

    cookies = extract_cookies_from_storage(auth_payload)
    csrf_token, session_id = await fetch_tokens(cookies)
    auth = AuthTokens(
        cookies=cookies,
        csrf_token=csrf_token,
        session_id=session_id,
    )
    return NotebookLMClient(auth, timeout=timeout_seconds)


async def _ingest_async(
    *,
    notebook_id: str,
    kind: str,
    payload: Dict[str, Any],
    auth_payload: Dict[str, Any],
    timeout_seconds: float,
) -> Dict[str, Any]:
    client = await _build_client(auth_payload=auth_payload, timeout_seconds=timeout_seconds)
    title = _normalize_ingest_title(kind=kind, payload=payload)
    url = str(payload.get("url") or "").strip()
    source_type = ""
    async with client:
        if kind == "resource_ingest":
            source_type = _normalize_resource_source_type(payload)
            if source_type == "file":
                file_path = str(payload.get("file_path") or payload.get("path") or payload.get("url") or "").strip()
                if not file_path:
                    raise NotebookLMProviderError(
                        code="space_job_invalid",
                        message="file_path is required for file",
                        transient=False,
                        degrade_provider=False,
                    )
                source = await client.sources.add_file(notebook_id, file_path, wait=False)
                source_mode = source_type
            elif source_type in {"web_page", "youtube"}:
                if not url:
                    raise NotebookLMProviderError(
                        code="space_job_invalid",
                        message=f"url is required for {source_type}",
                        transient=False,
                        degrade_provider=False,
                    )
                source = await client.sources.add_url(notebook_id, url, wait=False)
                source_mode = source_type
            elif source_type == "pasted_text":
                text = _payload_to_text(payload).strip()
                if not text:
                    raise NotebookLMProviderError(
                        code="space_job_invalid",
                        message="content is required for pasted_text",
                        transient=False,
                        degrade_provider=False,
                    )
                source = await client.sources.add_text(
                    notebook_id,
                    title=title,
                    content=text,
                    wait=False,
                )
                source_mode = source_type
            elif source_type in _RESOURCE_DRIVE_MIME:
                file_id = str(payload.get("file_id") or "").strip()
                if not file_id:
                    raise NotebookLMProviderError(
                        code="space_job_invalid",
                        message=f"file_id is required for {source_type}",
                        transient=False,
                        degrade_provider=False,
                    )
                mime_type = str(payload.get("mime_type") or "").strip() or _RESOURCE_DRIVE_MIME[source_type]
                source = await client.sources.add_drive(
                    notebook_id,
                    file_id=file_id,
                    title=title,
                    mime_type=mime_type,
                    wait=False,
                )
                source_mode = source_type
            else:
                raise NotebookLMProviderError(
                    code="space_job_invalid",
                    message=f"unsupported resource_ingest source_type: {source_type}",
                    transient=False,
                    degrade_provider=False,
                )
        else:
            text = _payload_to_text(payload)
            source = await client.sources.add_text(
                notebook_id,
                title=title,
                content=text,
                wait=False,
            )
            source_mode = "text"
    return {
        "provider": "notebooklm",
        "remote_space_id": notebook_id,
        "accepted": True,
        "kind": kind,
        "source_mode": source_mode,
        "source_type": source_type or source_mode,
        "source_id": str(getattr(source, "id", "") or ""),
        "title": str(getattr(source, "title", "") or title),
    }


def _reference_to_dict(ref: Any) -> Dict[str, Any]:
    data: Dict[str, Any] = {
        "source_id": str(getattr(ref, "source_id", "") or ""),
    }
    citation_number = getattr(ref, "citation_number", None)
    if citation_number is not None:
        try:
            data["citation_number"] = int(citation_number)
        except Exception:
            pass
    cited_text = str(getattr(ref, "cited_text", "") or "").strip()
    if cited_text:
        data["cited_text"] = cited_text
    return data


async def _query_async(
    *,
    notebook_id: str,
    query: str,
    options: Dict[str, Any],
    auth_payload: Dict[str, Any],
    timeout_seconds: float,
) -> Dict[str, Any]:
    client = await _build_client(auth_payload=auth_payload, timeout_seconds=timeout_seconds)
    source_ids = _extract_query_source_ids(options)
    async with client:
        result = await client.chat.ask(
            notebook_id,
            question=query,
            source_ids=source_ids,
        )
    refs_raw = list(getattr(result, "references", []) or [])
    references = [_reference_to_dict(ref) for ref in refs_raw]
    return {
        "answer": str(getattr(result, "answer", "") or ""),
        "references": references,
        "remote_space_id": notebook_id,
        "provider": "notebooklm",
    }


async def _create_notebook_async(
    *,
    title: str,
    auth_payload: Dict[str, Any],
    timeout_seconds: float,
) -> Dict[str, Any]:
    client = await _build_client(auth_payload=auth_payload, timeout_seconds=timeout_seconds)
    async with client:
        notebook = await client.notebooks.create(title)
    notebook_id = str(getattr(notebook, "id", "") or "").strip()
    if not notebook_id:
        raise NotebookLMProviderError(
            code="space_provider_upstream_error",
            message="notebook create returned empty notebook id",
            transient=False,
            degrade_provider=False,
        )
    return {
        "provider": "notebooklm",
        "remote_space_id": notebook_id,
        "title": str(getattr(notebook, "title", "") or title),
        "created": True,
    }


async def _list_notebooks_async(
    *,
    auth_payload: Dict[str, Any],
    timeout_seconds: float,
) -> Dict[str, Any]:
    client = await _build_client(auth_payload=auth_payload, timeout_seconds=timeout_seconds)
    async with client:
        notebooks = await client.notebooks.list()
    spaces: list[Dict[str, Any]] = []
    for nb in list(notebooks or []):
        remote_space_id = str(getattr(nb, "id", "") or "").strip()
        if not remote_space_id:
            continue
        title = str(getattr(nb, "title", "") or "").strip() or remote_space_id
        created_at = getattr(nb, "created_at", None)
        created_at_s = ""
        if created_at is not None:
            try:
                created_at_s = created_at.isoformat()
            except Exception:
                created_at_s = str(created_at)
        spaces.append(
            {
                "remote_space_id": remote_space_id,
                "title": title,
                "created_at": created_at_s,
                "is_owner": bool(getattr(nb, "is_owner", True)),
            }
        )
    return {
        "provider": "notebooklm",
        "spaces": spaces,
    }


def _source_to_dict(source: Any) -> Dict[str, Any]:
    return {
        "source_id": str(getattr(source, "id", "") or ""),
        "title": str(getattr(source, "title", "") or ""),
        "url": str(getattr(source, "url", "") or ""),
        "status": int(getattr(source, "status", 0) or 0),
        "kind": _normalize_enum_token(getattr(source, "kind", "")),
    }


def _artifact_to_dict(artifact: Any) -> Dict[str, Any]:
    created_at = getattr(artifact, "created_at", None)
    created_at_s = ""
    if created_at is not None:
        try:
            created_at_s = created_at.isoformat()
        except Exception:
            created_at_s = str(created_at)
    return {
        "artifact_id": str(getattr(artifact, "id", "") or ""),
        "title": str(getattr(artifact, "title", "") or ""),
        "kind": _normalize_enum_token(getattr(artifact, "kind", "")),
        "status": str(getattr(artifact, "status_str", "") or ""),
        "created_at": created_at_s,
        "url": str(getattr(artifact, "url", "") or ""),
    }


def _as_string_list(raw: Any) -> list[str] | None:
    if not isinstance(raw, list):
        return None
    out: list[str] = []
    for item in raw:
        value = str(item or "").strip()
        if value:
            out.append(value)
    return out or None


def _normalize_enum_token(raw: Any) -> str:
    if raw is None:
        return ""
    value = getattr(raw, "value", raw)
    text = str(value or "").strip()
    if not text:
        text = str(raw or "").strip()
    text = text.lower()
    if "." in text:
        text = text.split(".")[-1].strip()
    text = text.replace("-", "_")
    alias = {
        "studyguide": "study_guide",
        "study": "study_guide",
        "datatable": "data_table",
        "table": "data_table",
        "slidedeck": "slide_deck",
        "slides": "slide_deck",
        "slide": "slide_deck",
        "deck": "slide_deck",
        "mindmap": "mind_map",
        "overview": "report",
        "summary": "report",
        "briefing": "report",
    }
    return alias.get(text, text)


def _enum_or_none(enum_cls: Any, raw: Any, *, field: str) -> Any:
    text = _normalize_enum_token(raw)
    if not text:
        return None
    for member in list(enum_cls):
        name = _normalize_enum_token(getattr(member, "name", ""))
        value = _normalize_enum_token(getattr(member, "value", ""))
        if text == name or text == value:
            return member
    raise NotebookLMProviderError(
        code="space_job_invalid",
        message=f"invalid {field}: {raw}",
        transient=False,
        degrade_provider=False,
    )


async def _list_artifacts_async(
    *,
    notebook_id: str,
    kind: str,
    auth_payload: Dict[str, Any],
    timeout_seconds: float,
) -> Dict[str, Any]:
    from ._vendor.notebooklm.types import ArtifactType

    client = await _build_client(auth_payload=auth_payload, timeout_seconds=timeout_seconds)
    artifact_type = _enum_or_none(ArtifactType, kind, field="kind") if kind else None
    async with client:
        artifacts = await client.artifacts.list(notebook_id, artifact_type=artifact_type)
    rows = [_artifact_to_dict(item) for item in list(artifacts or [])]
    rows = sorted(rows, key=lambda row: str(row.get("created_at") or ""), reverse=True)
    return {
        "provider": "notebooklm",
        "remote_space_id": notebook_id,
        "kind": kind,
        "artifacts": rows,
    }


async def _generate_artifact_async(
    *,
    notebook_id: str,
    kind: str,
    options: Dict[str, Any],
    auth_payload: Dict[str, Any],
    timeout_seconds: float,
) -> Dict[str, Any]:
    from ._vendor.notebooklm.rpc import (
        AudioFormat,
        AudioLength,
        InfographicDetail,
        InfographicOrientation,
        QuizDifficulty,
        QuizQuantity,
        ReportFormat,
        SlideDeckFormat,
        SlideDeckLength,
        VideoFormat,
        VideoStyle,
    )

    client = await _build_client(auth_payload=auth_payload, timeout_seconds=timeout_seconds)
    source_ids = _as_string_list(options.get("source_ids"))
    language = str(options.get("language") or options.get("lang") or "").strip()
    if not language:
        language = str(os.environ.get("CCCC_SPACE_ARTIFACT_LANGUAGE") or "").strip() or "en"
    instructions = str(options.get("instructions") or "").strip() or None

    async with client:
        if kind == "audio":
            status = await client.artifacts.generate_audio(
                notebook_id,
                source_ids=source_ids,
                language=language,
                instructions=instructions,
                audio_format=_enum_or_none(AudioFormat, options.get("audio_format"), field="audio_format"),
                audio_length=_enum_or_none(AudioLength, options.get("audio_length"), field="audio_length"),
            )
        elif kind == "video":
            status = await client.artifacts.generate_video(
                notebook_id,
                source_ids=source_ids,
                language=language,
                instructions=instructions,
                video_format=_enum_or_none(VideoFormat, options.get("video_format"), field="video_format"),
                video_style=_enum_or_none(VideoStyle, options.get("video_style"), field="video_style"),
            )
        elif kind == "report":
            status = await client.artifacts.generate_report(
                notebook_id,
                report_format=_enum_or_none(ReportFormat, options.get("report_format"), field="report_format")
                or ReportFormat.BRIEFING_DOC,
                source_ids=source_ids,
                language=language,
                custom_prompt=str(options.get("custom_prompt") or "").strip() or None,
            )
        elif kind == "study_guide":
            status = await client.artifacts.generate_study_guide(
                notebook_id,
                source_ids=source_ids,
                language=language,
            )
        elif kind == "quiz":
            status = await client.artifacts.generate_quiz(
                notebook_id,
                source_ids=source_ids,
                instructions=instructions,
                quantity=_enum_or_none(QuizQuantity, options.get("quantity"), field="quantity"),
                difficulty=_enum_or_none(QuizDifficulty, options.get("difficulty"), field="difficulty"),
            )
        elif kind == "flashcards":
            status = await client.artifacts.generate_flashcards(
                notebook_id,
                source_ids=source_ids,
                instructions=instructions,
                quantity=_enum_or_none(QuizQuantity, options.get("quantity"), field="quantity"),
                difficulty=_enum_or_none(QuizDifficulty, options.get("difficulty"), field="difficulty"),
            )
        elif kind == "infographic":
            status = await client.artifacts.generate_infographic(
                notebook_id,
                source_ids=source_ids,
                language=language,
                instructions=instructions,
                orientation=_enum_or_none(
                    InfographicOrientation,
                    options.get("orientation"),
                    field="orientation",
                ),
                detail_level=_enum_or_none(
                    InfographicDetail,
                    options.get("detail_level"),
                    field="detail_level",
                ),
            )
        elif kind == "slide_deck":
            status = await client.artifacts.generate_slide_deck(
                notebook_id,
                source_ids=source_ids,
                language=language,
                instructions=instructions,
                slide_format=_enum_or_none(SlideDeckFormat, options.get("slide_format"), field="slide_format"),
                slide_length=_enum_or_none(SlideDeckLength, options.get("slide_length"), field="slide_length"),
            )
        elif kind == "data_table":
            status = await client.artifacts.generate_data_table(
                notebook_id,
                source_ids=source_ids,
                language=language,
                instructions=instructions,
            )
        elif kind == "mind_map":
            out = await client.artifacts.generate_mind_map(
                notebook_id,
                source_ids=source_ids,
            )
            note_id = str((out or {}).get("note_id") or "").strip()
            return {
                "provider": "notebooklm",
                "remote_space_id": notebook_id,
                "kind": kind,
                "task_id": note_id,
                "status": "completed" if note_id else "failed",
                "metadata": {"note_id": note_id},
            }
        else:
            raise NotebookLMProviderError(
                code="space_job_invalid",
                message=f"unsupported artifact kind: {kind}",
                transient=False,
                degrade_provider=False,
            )

    return {
        "provider": "notebooklm",
        "remote_space_id": notebook_id,
        "kind": kind,
        "task_id": str(getattr(status, "task_id", "") or ""),
        "status": str(getattr(status, "status", "") or ""),
        "url": str(getattr(status, "url", "") or ""),
        "error": str(getattr(status, "error", "") or ""),
        "error_code": str(getattr(status, "error_code", "") or ""),
        "metadata": dict(getattr(status, "metadata", {}) or {}),
    }


async def _wait_artifact_async(
    *,
    notebook_id: str,
    task_id: str,
    timeout_seconds: float,
    initial_interval: float,
    max_interval: float,
    auth_payload: Dict[str, Any],
    timeout_seconds_for_client: float,
) -> Dict[str, Any]:
    client = await _build_client(auth_payload=auth_payload, timeout_seconds=timeout_seconds_for_client)
    async with client:
        status = await client.artifacts.wait_for_completion(
            notebook_id,
            task_id,
            timeout=float(timeout_seconds),
            initial_interval=float(initial_interval),
            max_interval=float(max_interval),
        )
    return {
        "provider": "notebooklm",
        "remote_space_id": notebook_id,
        "task_id": str(getattr(status, "task_id", "") or task_id),
        "status": str(getattr(status, "status", "") or ""),
        "url": str(getattr(status, "url", "") or ""),
        "error": str(getattr(status, "error", "") or ""),
        "error_code": str(getattr(status, "error_code", "") or ""),
        "metadata": dict(getattr(status, "metadata", {}) or {}),
    }


async def _download_artifact_async(
    *,
    notebook_id: str,
    kind: str,
    output_path: str,
    artifact_id: str,
    output_format: str,
    auth_payload: Dict[str, Any],
    timeout_seconds: float,
) -> Dict[str, Any]:
    client = await _build_client(auth_payload=auth_payload, timeout_seconds=timeout_seconds)
    aid = artifact_id or None
    target = str(Path(output_path).expanduser().resolve())
    fmt = output_format or "markdown"
    async with client:
        if kind == "audio":
            saved_path = await client.artifacts.download_audio(notebook_id, target, artifact_id=aid)
        elif kind == "video":
            saved_path = await client.artifacts.download_video(notebook_id, target, artifact_id=aid)
        elif kind == "infographic":
            saved_path = await client.artifacts.download_infographic(notebook_id, target, artifact_id=aid)
        elif kind == "slide_deck":
            saved_path = await client.artifacts.download_slide_deck(notebook_id, target, artifact_id=aid)
        elif kind == "report" or kind == "study_guide":
            saved_path = await client.artifacts.download_report(notebook_id, target, artifact_id=aid)
        elif kind == "mind_map":
            saved_path = await client.artifacts.download_mind_map(notebook_id, target, artifact_id=aid)
        elif kind == "data_table":
            saved_path = await client.artifacts.download_data_table(notebook_id, target, artifact_id=aid)
        elif kind == "quiz":
            saved_path = await client.artifacts.download_quiz(
                notebook_id,
                target,
                artifact_id=aid,
                output_format=fmt,
            )
        elif kind == "flashcards":
            saved_path = await client.artifacts.download_flashcards(
                notebook_id,
                target,
                artifact_id=aid,
                output_format=fmt,
            )
        else:
            raise NotebookLMProviderError(
                code="space_job_invalid",
                message=f"unsupported artifact kind: {kind}",
                transient=False,
                degrade_provider=False,
            )
    return {
        "provider": "notebooklm",
        "remote_space_id": notebook_id,
        "kind": kind,
        "artifact_id": artifact_id,
        "output_path": str(saved_path or target),
        "downloaded": True,
    }


async def _list_sources_async(
    *,
    notebook_id: str,
    auth_payload: Dict[str, Any],
    timeout_seconds: float,
) -> Dict[str, Any]:
    client = await _build_client(auth_payload=auth_payload, timeout_seconds=timeout_seconds)
    async with client:
        sources = await client.sources.list(notebook_id)
    return {
        "provider": "notebooklm",
        "remote_space_id": notebook_id,
        "sources": [_source_to_dict(source) for source in list(sources or [])],
    }


async def _get_source_fulltext_async(
    *,
    notebook_id: str,
    source_id: str,
    auth_payload: Dict[str, Any],
    timeout_seconds: float,
) -> Dict[str, Any]:
    client = await _build_client(auth_payload=auth_payload, timeout_seconds=timeout_seconds)
    async with client:
        full = await client.sources.get_fulltext(notebook_id, source_id)
    return {
        "provider": "notebooklm",
        "remote_space_id": notebook_id,
        "source_id": str(getattr(full, "source_id", "") or source_id),
        "title": str(getattr(full, "title", "") or ""),
        "kind": _normalize_enum_token(getattr(full, "kind", "")),
        "url": str(getattr(full, "url", "") or ""),
        "content": str(getattr(full, "content", "") or ""),
        "char_count": int(getattr(full, "char_count", 0) or 0),
    }


async def _add_file_source_async(
    *,
    notebook_id: str,
    file_path: str,
    auth_payload: Dict[str, Any],
    timeout_seconds: float,
) -> Dict[str, Any]:
    target = Path(file_path).expanduser().resolve()
    client = await _build_client(auth_payload=auth_payload, timeout_seconds=timeout_seconds)
    async with client:
        source = await client.sources.add_file(notebook_id, str(target), wait=False)
    out = _source_to_dict(source)
    out["provider"] = "notebooklm"
    out["remote_space_id"] = notebook_id
    out["file_path"] = str(target)
    out["accepted"] = True
    return out


async def _delete_source_async(
    *,
    notebook_id: str,
    source_id: str,
    auth_payload: Dict[str, Any],
    timeout_seconds: float,
) -> Dict[str, Any]:
    client = await _build_client(auth_payload=auth_payload, timeout_seconds=timeout_seconds)
    async with client:
        ok = await client.sources.delete(notebook_id, source_id)
    return {
        "provider": "notebooklm",
        "remote_space_id": notebook_id,
        "source_id": source_id,
        "deleted": bool(ok),
    }


async def _rename_source_async(
    *,
    notebook_id: str,
    source_id: str,
    new_title: str,
    auth_payload: Dict[str, Any],
    timeout_seconds: float,
) -> Dict[str, Any]:
    client = await _build_client(auth_payload=auth_payload, timeout_seconds=timeout_seconds)
    async with client:
        source = await client.sources.rename(notebook_id, source_id, new_title)
    out = _source_to_dict(source)
    out["provider"] = "notebooklm"
    out["remote_space_id"] = notebook_id
    out["renamed"] = True
    out["title"] = str(out.get("title") or new_title)
    return out


async def _refresh_source_async(
    *,
    notebook_id: str,
    source_id: str,
    auth_payload: Dict[str, Any],
    timeout_seconds: float,
) -> Dict[str, Any]:
    client = await _build_client(auth_payload=auth_payload, timeout_seconds=timeout_seconds)
    async with client:
        ok = await client.sources.refresh(notebook_id, source_id)
    return {
        "provider": "notebooklm",
        "remote_space_id": notebook_id,
        "source_id": source_id,
        "refreshed": bool(ok),
    }


def _map_vendor_exception(exc: Exception) -> NotebookLMProviderError:
    if isinstance(exc, NotebookLMProviderError):
        return exc
    try:
        from ._vendor.notebooklm.exceptions import (
            AuthError,
            ClientError,
            ConfigurationError,
            NetworkError,
            RPCTimeoutError,
            RPCError,
            RateLimitError,
            ServerError,
        )
    except Exception:
        return NotebookLMProviderError(
            code="space_provider_upstream_error",
            message=str(exc) or "NotebookLM upstream error",
            transient=True,
            degrade_provider=False,
        )

    if isinstance(exc, FileNotFoundError):
        return NotebookLMProviderError(
            code="space_provider_not_configured",
            message=str(exc) or "NotebookLM credentials not configured",
            transient=False,
            degrade_provider=True,
        )
    if isinstance(exc, (ConfigurationError, AuthError)):
        return NotebookLMProviderError(
            code="space_provider_auth_invalid",
            message=str(exc) or "NotebookLM auth invalid",
            transient=False,
            degrade_provider=True,
        )
    if isinstance(exc, ValueError):
        text = str(exc).lower()
        if "authentication expired" in text or "re-authenticate" in text:
            return NotebookLMProviderError(
                code="space_provider_auth_invalid",
                message=str(exc) or "NotebookLM auth invalid",
                transient=False,
                degrade_provider=True,
            )
        if "page structure may have changed" in text:
            return NotebookLMProviderError(
                code="space_provider_compat_mismatch",
                message=str(exc) or "NotebookLM API compatibility mismatch",
                transient=False,
                degrade_provider=True,
            )
        return NotebookLMProviderError(
            code="space_provider_upstream_error",
            message=str(exc) or "NotebookLM upstream error",
            transient=False,
            degrade_provider=False,
        )
    if isinstance(exc, RateLimitError):
        return NotebookLMProviderError(
            code="space_provider_rate_limited",
            message=str(exc) or "NotebookLM rate limited",
            transient=True,
            degrade_provider=False,
        )
    if isinstance(exc, RPCTimeoutError):
        return NotebookLMProviderError(
            code="space_provider_timeout",
            message=str(exc) or "NotebookLM request timed out",
            transient=True,
            degrade_provider=False,
        )
    if isinstance(exc, NetworkError):
        return NotebookLMProviderError(
            code="space_provider_upstream_error",
            message=str(exc) or "NotebookLM network error",
            transient=True,
            degrade_provider=False,
        )
    if isinstance(exc, ServerError):
        return NotebookLMProviderError(
            code="space_provider_upstream_error",
            message=str(exc) or "NotebookLM server error",
            transient=True,
            degrade_provider=False,
        )
    if isinstance(exc, ClientError):
        return NotebookLMProviderError(
            code="space_provider_upstream_error",
            message=str(exc) or "NotebookLM client error",
            transient=False,
            degrade_provider=False,
        )
    if isinstance(exc, RPCError):
        return NotebookLMProviderError(
            code="space_provider_upstream_error",
            message=str(exc) or "NotebookLM RPC error",
            transient=True,
            degrade_provider=False,
        )
    return NotebookLMProviderError(
        code="space_provider_upstream_error",
        message=str(exc) or "NotebookLM upstream error",
        transient=True,
        degrade_provider=False,
    )


_ADAPTER = NotebookLMAdapter()


def get_notebooklm_adapter() -> NotebookLMAdapter:
    return _ADAPTER
