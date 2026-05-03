from __future__ import annotations

import hashlib
import importlib.resources
import json
import os
import re
import shlex
import shutil
import ssl
import stat
import subprocess
import tarfile
import time
import urllib.error
import urllib.request
from datetime import datetime, timezone
from email.message import Message
from pathlib import Path, PurePosixPath
from typing import Any, Callable, Dict, Iterable, List

from ...paths import ensure_home
from ...util.file_lock import acquire_lockfile, release_lockfile
from ...util.fs import atomic_write_json, read_json
from ...util.time import utc_now_iso


VOICE_MODEL_KIND_ASR = "asr"
VOICE_MODEL_KIND_DIARIZATION = "diarization"
VOICE_MODEL_STATUS_NOT_INSTALLED = "not_installed"
VOICE_MODEL_STATUS_DOWNLOADING = "downloading"
VOICE_MODEL_STATUS_READY = "ready"
VOICE_MODEL_STATUS_FAILED = "failed"
_INSTALL_STATE_FILENAME = "install-state.json"
_INSTALL_LOCK_FILENAME = ".install.lock"
_READ_CHUNK_BYTES = 1024 * 1024
_URL_OPEN_TIMEOUT_SECONDS = 60
_DOWNLOADING_STALE_SECONDS = 300
_DEFAULT_MANIFEST_RESOURCE = "voice-models.default.json"
_LOCAL_MANIFEST_REL_PATH = ("config", "voice-models.json")
_SHA256_HEX_LENGTH = 64
_VOICE_MODEL_ID_RE = re.compile(r"^[a-z0-9][a-z0-9._-]{0,63}$")
_CA_CERT_CANDIDATES = (
    "/etc/ssl/cert.pem",
    "/opt/homebrew/etc/ca-certificates/cert.pem",
    "/opt/homebrew/etc/openssl@3/cert.pem",
)


class VoiceModelError(Exception):
    def __init__(self, code: str, message: str, *, details: Dict[str, Any] | None = None) -> None:
        super().__init__(message)
        self.code = code
        self.message = message
        self.details = details or {}


def _voice_models_root() -> Path:
    return ensure_home() / "cache" / "voice-models"


def _directory_size_bytes(path: Path) -> int:
    total = 0
    if not path.exists():
        return total
    for item in path.rglob("*"):
        try:
            if item.is_file():
                total += int(item.stat().st_size)
        except OSError:
            continue
    return total


def _clear_voice_model_dir(model_id: str) -> None:
    root = voice_model_dir(model_id)
    if not root.exists():
        return
    for child in root.iterdir():
        if child.name == _INSTALL_LOCK_FILENAME:
            continue
        if child.is_dir():
            shutil.rmtree(child, ignore_errors=True)
        else:
            try:
                child.unlink()
            except FileNotFoundError:
                pass


def _local_manifest_path() -> Path:
    return ensure_home().joinpath(*_LOCAL_MANIFEST_REL_PATH)


def _normalize_model_id(value: Any) -> str:
    model_id = str(value or "").strip()
    if not model_id or not _VOICE_MODEL_ID_RE.match(model_id):
        raise VoiceModelError(
            "voice_model_manifest_invalid",
            "model_id must be a simple slug",
            details={"model_id": model_id},
        )
    return model_id


def _normalize_rel_model_path(value: Any, *, field: str) -> str:
    raw = str(value or "").strip()
    if not raw:
        raise VoiceModelError(
            "voice_model_manifest_invalid",
            f"{field} must be a non-empty relative path",
        )
    if any(ord(ch) < 32 for ch in raw):
        raise VoiceModelError(
            "voice_model_manifest_invalid",
            f"{field} contains control characters",
            details={"path": raw},
        )
    if "\\" in raw:
        raise VoiceModelError(
            "voice_model_manifest_invalid",
            f"{field} must use POSIX-style relative paths",
            details={"path": raw},
        )
    path = PurePosixPath(raw)
    if (
        path.as_posix() == "."
        or path.is_absolute()
        or any(part in {"", ".", ".."} or ":" in part for part in path.parts)
    ):
        raise VoiceModelError(
            "voice_model_manifest_invalid",
            f"{field} must stay inside the voice model directory",
            details={"path": raw},
        )
    return path.as_posix()


def _load_builtin_manifest() -> Any:
    try:
        return json.loads(
            importlib.resources.files("cccc.resources")
            .joinpath(_DEFAULT_MANIFEST_RESOURCE)
            .read_text(encoding="utf-8")
        )
    except FileNotFoundError:
        return {}
    except Exception as exc:
        raise VoiceModelError(
            "voice_model_manifest_invalid",
            f"built-in voice model manifest is invalid: {_DEFAULT_MANIFEST_RESOURCE}",
        ) from exc


def _merge_manifest_payloads(*payloads: Any) -> Dict[str, Any]:
    merged: Dict[str, Any] = {"voice_secretary_asr_models": []}
    seen: set[str] = set()
    for payload in payloads:
        catalog = _catalog_from_manifest(payload)
        for model_id, entry in catalog.items():
            if model_id in seen:
                merged["voice_secretary_asr_models"] = [
                    item for item in merged["voice_secretary_asr_models"] if str(item.get("model_id") or "") != model_id
                ]
            seen.add(model_id)
            merged["voice_secretary_asr_models"].append(entry)
    return merged


def _manifest_source(source: str = "") -> Any:
    raw = str(source or "").strip()
    if not raw:
        builtin = _load_builtin_manifest()
        local_path = _local_manifest_path()
        if not local_path.exists():
            return builtin
        local = read_json(local_path)
        if not isinstance(local, dict):
            raise VoiceModelError(
                "voice_model_manifest_invalid",
                "local voice model manifest must be a JSON object",
                details={"path": str(local_path)},
            )
        return _merge_manifest_payloads(builtin, local)
    if raw.startswith("{") or raw.startswith("["):
        try:
            return json.loads(raw)
        except Exception as exc:
            raise VoiceModelError(
                "voice_model_manifest_invalid",
                "voice model manifest JSON is invalid",
            ) from exc
    path = Path(raw)
    if not path.exists():
        raise VoiceModelError(
            "voice_model_manifest_missing",
            f"voice model manifest not found: {path}",
            details={"path": str(path)},
        )
    payload = read_json(path)
    if not isinstance(payload, dict):
        raise VoiceModelError(
            "voice_model_manifest_invalid",
            "voice model manifest must be a JSON object",
            details={"path": str(path)},
        )
    return payload


def _normalize_artifact(item: Any) -> Dict[str, Any]:
    if not isinstance(item, dict):
        raise VoiceModelError("voice_model_manifest_invalid", "artifact entry must be an object")
    rel_path = _normalize_rel_model_path(item.get("path"), field="artifact path")
    url = str(item.get("url") or "").strip()
    sha256 = str(item.get("sha256") or "").strip().lower()
    if not rel_path or not url or not sha256:
        raise VoiceModelError(
            "voice_model_manifest_invalid",
            "artifact entries require path, url, and sha256",
        )
    entry: Dict[str, Any] = {
        "path": rel_path,
        "url": url,
        "sha256": sha256,
        "executable": bool(item.get("executable")),
    }
    archive = str(item.get("archive") or "").strip().lower()
    if archive:
        if archive not in {"tar", "tar.gz", "tgz", "tar.bz2", "tbz2"}:
            raise VoiceModelError(
                "voice_model_manifest_invalid",
                f"unsupported artifact archive type: {archive}",
            )
        entry["archive"] = archive
    if item.get("size_bytes") is not None:
        try:
            entry["size_bytes"] = int(item.get("size_bytes"))
        except Exception as exc:
            raise VoiceModelError(
                "voice_model_manifest_invalid",
                f"artifact size_bytes must be an integer: {rel_path}",
            ) from exc
    return entry


def _normalize_model_entry(item: Any) -> Dict[str, Any]:
    if not isinstance(item, dict):
        raise VoiceModelError("voice_model_manifest_invalid", "model entry must be an object")
    model_id = _normalize_model_id(item.get("model_id"))
    title = str(item.get("title") or model_id).strip()
    kind = str(item.get("kind") or VOICE_MODEL_KIND_ASR).strip() or VOICE_MODEL_KIND_ASR
    command_template = item.get("command_template")
    streaming = item.get("streaming")
    diarization = item.get("diarization")
    if not model_id:
        raise VoiceModelError("voice_model_manifest_invalid", "model_id is required")
    if kind not in {VOICE_MODEL_KIND_ASR, VOICE_MODEL_KIND_DIARIZATION}:
        raise VoiceModelError("voice_model_manifest_invalid", f"unsupported voice model kind: {kind}")
    if (
        (not isinstance(command_template, (str, list)) or not command_template)
        and not isinstance(streaming, dict)
        and not isinstance(diarization, dict)
    ):
        raise VoiceModelError(
            "voice_model_manifest_invalid",
            f"model {model_id} requires command_template, streaming config, or diarization config",
        )
    artifacts = item.get("artifacts")
    if not isinstance(artifacts, list) or not artifacts:
        raise VoiceModelError(
            "voice_model_manifest_invalid",
            f"model {model_id} requires at least one artifact",
        )
    description = str(item.get("description") or "").strip()
    normalized = {
        "model_id": model_id,
        "kind": kind,
        "title": title or model_id,
        "description": description,
        "command_template": command_template,
        "artifacts": [_normalize_artifact(entry) for entry in artifacts],
    }
    if isinstance(streaming, dict):
        engine = str(streaming.get("engine") or "").strip()
        tokens = _normalize_rel_model_path(streaming.get("tokens"), field=f"model {model_id} streaming.tokens")
        if not engine or not tokens:
            raise VoiceModelError(
                "voice_model_manifest_invalid",
                f"model {model_id} streaming config requires engine and tokens",
            )
        streaming_config: Dict[str, Any] = {
            "engine": engine,
            "tokens": tokens,
            "sample_rate": int(streaming.get("sample_rate") or 16000),
            "num_threads": int(streaming.get("num_threads") or 2),
            "provider": str(streaming.get("provider") or "cpu").strip() or "cpu",
        }
        for key in ("model", "encoder", "decoder", "joiner", "bpe_vocab"):
            value = str(streaming.get(key) or "").strip()
            if value:
                streaming_config[key] = _normalize_rel_model_path(value, field=f"model {model_id} streaming.{key}")
        if engine == "zipformer2_ctc" and not streaming_config.get("model"):
            raise VoiceModelError(
                "voice_model_manifest_invalid",
                f"model {model_id} zipformer2_ctc streaming config requires model",
            )
        if engine == "transducer" and (
            not streaming_config.get("encoder") or not streaming_config.get("decoder") or not streaming_config.get("joiner")
        ):
            raise VoiceModelError(
                "voice_model_manifest_invalid",
                f"model {model_id} transducer streaming config requires encoder, decoder and joiner",
            )
        if engine == "paraformer" and (not streaming_config.get("encoder") or not streaming_config.get("decoder")):
            raise VoiceModelError(
                "voice_model_manifest_invalid",
                f"model {model_id} paraformer streaming config requires encoder and decoder",
            )
        normalized["streaming"] = streaming_config
    if isinstance(diarization, dict):
        segmentation_model = _normalize_rel_model_path(
            diarization.get("segmentation_model"),
            field=f"model {model_id} diarization.segmentation_model",
        )
        embedding_model = _normalize_rel_model_path(
            diarization.get("embedding_model"),
            field=f"model {model_id} diarization.embedding_model",
        )
        if not segmentation_model or not embedding_model:
            raise VoiceModelError(
                "voice_model_manifest_invalid",
                f"model {model_id} diarization config requires segmentation_model and embedding_model",
            )
        normalized["diarization"] = {
            "engine": str(diarization.get("engine") or "offline_speaker_diarization").strip() or "offline_speaker_diarization",
            "segmentation_model": segmentation_model,
            "embedding_model": embedding_model,
            "sample_rate": int(diarization.get("sample_rate") or 16000),
            "num_threads": int(diarization.get("num_threads") or 2),
            "provider": str(diarization.get("provider") or "cpu").strip() or "cpu",
            "num_speakers": int(diarization.get("num_speakers") or -1),
            "cluster_threshold": float(diarization.get("cluster_threshold") or 0.5),
            "min_duration_on": float(diarization.get("min_duration_on") or 0.3),
            "min_duration_off": float(diarization.get("min_duration_off") or 0.5),
        }
    required_files = item.get("required_files")
    if isinstance(required_files, list):
        normalized["required_files"] = [
            _normalize_rel_model_path(path, field=f"model {model_id} required_files")
            for path in required_files
            if str(path or "").strip()
        ]
    normalized["manifest_sha256"] = hashlib.sha256(
        json.dumps(normalized, ensure_ascii=False, sort_keys=True).encode("utf-8")
    ).hexdigest()
    return normalized


def _catalog_from_manifest(payload: Any) -> Dict[str, Dict[str, Any]]:
    if payload in ({}, None):
        return {}
    items: Iterable[Any]
    if isinstance(payload, dict):
        raw_items = payload.get("voice_secretary_asr_models")
        items = raw_items if isinstance(raw_items, list) else []
    elif isinstance(payload, list):
        items = payload
    else:
        raise VoiceModelError("voice_model_manifest_invalid", "voice model manifest must be an object or list")
    catalog: Dict[str, Dict[str, Any]] = {}
    for raw in items:
        entry = _normalize_model_entry(raw)
        catalog[str(entry["model_id"])] = entry
    return catalog


def load_voice_model_catalog(source: str = "") -> Dict[str, Dict[str, Any]]:
    return _catalog_from_manifest(_manifest_source(source))


def voice_model_dir(model_id: str) -> Path:
    return _voice_models_root() / _normalize_model_id(model_id)


def _install_state_path(model_id: str) -> Path:
    return voice_model_dir(model_id) / _INSTALL_STATE_FILENAME


def _install_lock_path(model_id: str) -> Path:
    return voice_model_dir(model_id) / _INSTALL_LOCK_FILENAME


def _read_install_state(model_id: str) -> Dict[str, Any]:
    state = read_json(_install_state_path(model_id))
    return dict(state) if isinstance(state, dict) else {}


def _write_install_state(model_id: str, payload: Dict[str, Any]) -> None:
    install_dir = voice_model_dir(model_id)
    install_dir.mkdir(parents=True, exist_ok=True)
    atomic_write_json(_install_state_path(model_id), payload, indent=2)


def _updated_at_age_seconds(state: Dict[str, Any]) -> float | None:
    raw = str(state.get("updated_at") or "").strip()
    if not raw:
        return None
    try:
        updated_at = datetime.fromisoformat(raw.replace("Z", "+00:00"))
    except ValueError:
        return None
    if updated_at.tzinfo is None:
        updated_at = updated_at.replace(tzinfo=timezone.utc)
    return max(0.0, (datetime.now(timezone.utc) - updated_at).total_seconds())


def _voice_runtime_id_for_model(model_id: str) -> str:
    if str(model_id or "").strip().startswith("sherpa_onnx_"):
        from .voice_runtime_deps import VOICE_RUNTIME_ID_SHERPA_ONNX_STREAMING

        return VOICE_RUNTIME_ID_SHERPA_ONNX_STREAMING
    return ""


def _render_command_template(command_template: Any, *, model_id: str, model_dir: Path) -> str:
    _ = model_id
    mapping = {
        "python": shlex.quote(os.sys.executable),
        "model_dir": shlex.quote(str(model_dir)),
    }
    if isinstance(command_template, list):
        rendered: List[str] = []
        for raw in command_template:
            text = str(raw or "")
            for key, value in mapping.items():
                text = text.replace("{" + key + "}", value)
            rendered.append(text)
        return " ".join(rendered)
    text = str(command_template or "")
    for key, value in mapping.items():
        text = text.replace("{" + key + "}", value)
    return text


def _https_context() -> ssl.SSLContext:
    env_cafile = str(os.environ.get("SSL_CERT_FILE") or "").strip()
    if env_cafile and Path(env_cafile).exists():
        try:
            return ssl.create_default_context(cafile=env_cafile)
        except OSError:
            pass
    for candidate in _CA_CERT_CANDIDATES:
        if Path(candidate).exists():
            try:
                return ssl.create_default_context(cafile=candidate)
            except OSError:
                pass
    return ssl.create_default_context()


def _urlopen(target: Any, *, timeout: int = _URL_OPEN_TIMEOUT_SECONDS):
    raw_url = str(getattr(target, "full_url", "") or target or "")
    if raw_url.startswith("https://"):
        return urllib.request.urlopen(target, timeout=timeout, context=_https_context())
    return urllib.request.urlopen(target, timeout=timeout)


def _download_with_curl(url: str, tmp_path: Path, *, progress: Callable[[int], None] | None = None) -> int | None:
    curl = shutil.which("curl")
    if not curl or not (url.startswith("http://") or url.startswith("https://")):
        return None
    env = os.environ.copy()
    for key in ("SSL_CERT_FILE", "CURL_CA_BUNDLE", "REQUESTS_CA_BUNDLE"):
        env.pop(key, None)
    command = [
        curl,
        "--fail",
        "--location",
        "--silent",
        "--show-error",
        "--connect-timeout",
        "30",
        "--output",
        str(tmp_path),
        url,
    ]
    process = subprocess.Popen(command, env=env, stderr=subprocess.PIPE, text=True)
    last_progress = -1
    while process.poll() is None:
        try:
            size = tmp_path.stat().st_size
        except FileNotFoundError:
            size = 0
        if progress is not None and size != last_progress:
            progress(size)
            last_progress = size
        time.sleep(0.5)
    stderr = process.stderr.read() if process.stderr is not None else ""
    if process.returncode != 0:
        raise VoiceModelError(
            "voice_model_download_failed",
            "voice model artifact download failed",
            details={"url": url, "error": stderr.strip(), "returncode": process.returncode},
        )
    size = tmp_path.stat().st_size
    if progress is not None:
        progress(size)
    return int(size)


def _header_sha256(headers: Message) -> str:
    candidates = [
        headers.get("x-linked-etag"),
        headers.get("etag"),
        headers.get("x-amz-meta-sha256"),
        headers.get("x-checksum-sha256"),
    ]
    for raw in candidates:
        value = str(raw or "").strip().strip('"').lower()
        if len(value) == _SHA256_HEX_LENGTH and all(ch in "0123456789abcdef" for ch in value):
            return value
    return ""


def _validate_remote_artifact_metadata(artifact: Dict[str, Any]) -> None:
    url = str(artifact.get("url") or "").strip()
    if not (url.startswith("http://") or url.startswith("https://")):
        return
    request = urllib.request.Request(url, method="HEAD")
    try:
        with _urlopen(request, timeout=20) as resp:
            headers = resp.headers
    except (urllib.error.HTTPError, urllib.error.URLError, TimeoutError, OSError):
        return
    remote_sha256 = _header_sha256(headers)
    expected_sha256 = str(artifact["sha256"]).strip().lower()
    if remote_sha256 and remote_sha256 != expected_sha256:
        raise VoiceModelError(
            "voice_model_manifest_hash_invalid",
            f"remote artifact hash does not match manifest before download: {artifact['path']}",
            details={
                "path": str(artifact["path"]),
                "url": url,
                "expected_sha256": expected_sha256,
                "remote_sha256": remote_sha256,
                "hint": "For HuggingFace Xet-backed files, use the file content hash from ETag/X-Linked-ETag, not X-Xet-Hash.",
            },
        )
    expected_size = artifact.get("size_bytes")
    linked_size = str(headers.get("x-linked-size") or headers.get("content-length") or "").strip()
    if expected_size is not None and linked_size.isdigit() and int(linked_size) != int(expected_size):
        raise VoiceModelError(
            "voice_model_manifest_size_invalid",
            f"remote artifact size does not match manifest before download: {artifact['path']}",
            details={
                "path": str(artifact["path"]),
                "url": url,
                "expected_size_bytes": int(expected_size),
                "remote_size_bytes": int(linked_size),
            },
        )


def _download_artifact(
    artifact: Dict[str, Any],
    *,
    output_path: Path,
    progress: Callable[[int], None] | None = None,
) -> Dict[str, Any]:
    _validate_remote_artifact_metadata(artifact)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    tmp_path = output_path.with_suffix(output_path.suffix + ".part")
    hasher = hashlib.sha256()
    bytes_written = 0
    try:
        curl_bytes = _download_with_curl(str(artifact["url"]), tmp_path, progress=progress)
        if curl_bytes is None:
            with _urlopen(str(artifact["url"])) as resp, tmp_path.open("wb") as handle:
                while True:
                    chunk = resp.read(_READ_CHUNK_BYTES)
                    if not chunk:
                        break
                    hasher.update(chunk)
                    handle.write(chunk)
                    bytes_written += len(chunk)
                    if progress is not None:
                        progress(bytes_written)
        else:
            bytes_written = curl_bytes
            with tmp_path.open("rb") as handle:
                while True:
                    chunk = handle.read(_READ_CHUNK_BYTES)
                    if not chunk:
                        break
                    hasher.update(chunk)
    except (urllib.error.HTTPError, urllib.error.URLError, TimeoutError, OSError) as exc:
        try:
            tmp_path.unlink()
        except Exception:
            pass
        raise VoiceModelError(
            "voice_model_download_failed",
            f"voice model artifact download failed: {artifact['path']}",
            details={
                "path": str(artifact["path"]),
                "url": str(artifact["url"]),
                "error": str(exc),
            },
        ) from exc
    digest = hasher.hexdigest()
    if digest != str(artifact["sha256"]):
        try:
            tmp_path.unlink()
        except Exception:
            pass
        raise VoiceModelError(
            "voice_model_hash_mismatch",
            f"downloaded artifact hash mismatch: {artifact['path']}",
            details={"expected_sha256": artifact["sha256"], "actual_sha256": digest},
        )
    expected_size = artifact.get("size_bytes")
    if expected_size is not None and int(expected_size) != int(bytes_written):
        try:
            tmp_path.unlink()
        except Exception:
            pass
        raise VoiceModelError(
            "voice_model_size_mismatch",
            f"downloaded artifact size mismatch: {artifact['path']}",
            details={"expected_size_bytes": int(expected_size), "actual_size_bytes": int(bytes_written)},
        )
    os.replace(tmp_path, output_path)
    if bool(artifact.get("executable")):
        current_mode = output_path.stat().st_mode
        output_path.chmod(current_mode | stat.S_IXUSR)
    return {
        "path": str(artifact["path"]),
        "sha256": digest,
        "size_bytes": int(bytes_written),
    }


def _safe_extract_tar(archive_path: Path, output_dir: Path) -> None:
    output_root = output_dir.resolve()
    with tarfile.open(archive_path) as archive:
        members = archive.getmembers()
        for member in members:
            if not (member.isfile() or member.isdir()):
                raise VoiceModelError(
                    "voice_model_archive_invalid",
                    f"voice model archive contains an unsupported entry type: {member.name}",
                )
            member_name = _normalize_rel_model_path(member.name, field="archive member path")
            target = (output_root / member_name).resolve()
            try:
                target.relative_to(output_root)
            except ValueError as exc:
                raise VoiceModelError(
                    "voice_model_archive_invalid",
                    f"voice model archive contains an unsafe path: {member.name}",
                ) from exc
        archive.extractall(output_root, members=members)


def _install_progress_state(
    *,
    model_id: str,
    entry: Dict[str, Any],
    status: str,
    downloaded_bytes: int,
    total_bytes: int,
    current_artifact_path: str = "",
    artifact_index: int = 0,
    artifact_count: int = 0,
    error: Dict[str, Any] | None = None,
) -> Dict[str, Any]:
    progress_percent = 0.0
    if total_bytes > 0:
        progress_percent = min(100.0, max(0.0, (downloaded_bytes / total_bytes) * 100.0))
    return {
        "model_id": model_id,
        "status": status,
        "updated_at": utc_now_iso(),
        "error": error or {},
        "manifest_sha256": entry.get("manifest_sha256"),
        "downloaded_bytes": int(max(0, downloaded_bytes)),
        "total_size_bytes": int(max(0, total_bytes)),
        "progress_percent": round(progress_percent, 1),
        "current_artifact_path": current_artifact_path,
        "artifact_index": int(max(0, artifact_index)),
        "artifact_count": int(max(0, artifact_count)),
    }


def list_voice_models(source: str = "") -> List[Dict[str, Any]]:
    try:
        catalog = load_voice_model_catalog(source)
    except VoiceModelError as exc:
        return [
            {
                "model_id": "",
                "status": VOICE_MODEL_STATUS_FAILED,
                "available": False,
                "error": {"code": exc.code, "message": exc.message, "details": exc.details},
            }
        ]
    return [get_voice_model_status(model_id, source=source) for model_id in sorted(catalog.keys())]


def get_voice_model_status(model_id: str, *, source: str = "") -> Dict[str, Any]:
    model_id = str(model_id or "").strip()
    try:
        catalog = load_voice_model_catalog(source)
    except VoiceModelError as exc:
        return {
            "model_id": model_id,
            "status": VOICE_MODEL_STATUS_FAILED,
            "available": False,
            "installed": False,
            "error": {"code": exc.code, "message": exc.message, "details": exc.details},
        }
    entry = catalog.get(model_id)
    if entry is None:
        return {
            "model_id": model_id,
            "status": "unknown",
            "available": False,
        }
    state = _read_install_state(model_id)
    status = str(state.get("status") or VOICE_MODEL_STATUS_NOT_INSTALLED).strip() or VOICE_MODEL_STATUS_NOT_INSTALLED
    if status == VOICE_MODEL_STATUS_DOWNLOADING:
        age_seconds = _updated_at_age_seconds(state)
        if age_seconds is not None and age_seconds > _DOWNLOADING_STALE_SECONDS:
            status = VOICE_MODEL_STATUS_FAILED
            state = {
                **state,
                "status": status,
                "error": {
                    "code": "voice_model_install_stale",
                    "message": "voice model install did not report progress and appears to have stopped",
                    "details": {"age_seconds": int(age_seconds)},
                },
            }
    state_manifest_sha256 = str(state.get("manifest_sha256") or "").strip()
    manifest_sha256 = str(entry.get("manifest_sha256") or "").strip()
    if state_manifest_sha256 and manifest_sha256 and state_manifest_sha256 != manifest_sha256:
        status = VOICE_MODEL_STATUS_NOT_INSTALLED
        state = {
            "status": status,
            "error": {},
            "manifest_sha256": manifest_sha256,
        }
    install_dir = voice_model_dir(model_id)
    artifacts_ready = True
    required_paths = [str(path) for path in (entry.get("required_files") or [])]
    if not required_paths and isinstance(entry.get("streaming"), dict):
        streaming = entry["streaming"]
        required_paths = [str(streaming.get(key) or "") for key in ("tokens", "model", "encoder", "decoder", "joiner", "bpe_vocab")]
        required_paths = [path for path in required_paths if path]
    if not required_paths and isinstance(entry.get("diarization"), dict):
        diarization = entry["diarization"]
        required_paths = [str(diarization.get(key) or "") for key in ("segmentation_model", "embedding_model")]
        required_paths = [path for path in required_paths if path]
    for artifact in entry.get("artifacts") or []:
        if not (install_dir / str(artifact["path"])).exists():
            artifacts_ready = False
            break
    for rel_path in required_paths:
        if not (install_dir / rel_path).exists():
            artifacts_ready = False
            break
    if status == VOICE_MODEL_STATUS_READY and not artifacts_ready:
        status = VOICE_MODEL_STATUS_NOT_INSTALLED
    return {
        "model_id": model_id,
        "kind": entry.get("kind"),
        "title": entry.get("title"),
        "description": entry.get("description"),
        "runtime_id": _voice_runtime_id_for_model(model_id),
        "status": status,
        "available": True,
        "installed": status == VOICE_MODEL_STATUS_READY,
        "install_dir": str(install_dir),
        "installed_at": str(state.get("installed_at") or ""),
        "updated_at": str(state.get("updated_at") or ""),
        "error": state.get("error") if isinstance(state.get("error"), dict) else {},
        "manifest_sha256": str(entry.get("manifest_sha256") or ""),
        "command_ready": bool(status == VOICE_MODEL_STATUS_READY and entry.get("command_template")),
        "streaming_ready": bool(status == VOICE_MODEL_STATUS_READY and entry.get("streaming")),
        "diarization_ready": bool(status == VOICE_MODEL_STATUS_READY and entry.get("diarization")),
        "streaming": entry.get("streaming") if isinstance(entry.get("streaming"), dict) else {},
        "diarization": entry.get("diarization") if isinstance(entry.get("diarization"), dict) else {},
        "downloaded_bytes": int(state.get("downloaded_bytes") or 0),
        "total_size_bytes": int(state.get("total_size_bytes") or 0),
        "progress_percent": float(state.get("progress_percent") or 0.0),
        "current_artifact_path": str(state.get("current_artifact_path") or ""),
        "artifact_index": int(state.get("artifact_index") or 0),
        "artifact_count": int(state.get("artifact_count") or 0),
        "disk_usage_bytes": _directory_size_bytes(install_dir),
        "artifacts": [
            {
                "path": str(artifact["path"]),
                "size_bytes": artifact.get("size_bytes"),
            }
            for artifact in (entry.get("artifacts") or [])
        ],
    }


def install_voice_model(model_id: str, *, source: str = "") -> Dict[str, Any]:
    model_id = str(model_id or "").strip()
    catalog = load_voice_model_catalog(source)
    entry = catalog.get(model_id)
    if entry is None:
        raise VoiceModelError(
            "voice_model_not_found",
            f"voice model not found: {model_id}",
            details={"model_id": model_id},
        )
    install_dir = voice_model_dir(model_id)
    install_dir.mkdir(parents=True, exist_ok=True)
    lock_handle = acquire_lockfile(_install_lock_path(model_id))
    try:
        artifacts = entry.get("artifacts") or []
        total_expected_bytes = sum(int(artifact.get("size_bytes") or 0) for artifact in artifacts)
        artifact_count = len(artifacts)
        _write_install_state(
            model_id,
            _install_progress_state(
                model_id=model_id,
                entry=entry,
                status=VOICE_MODEL_STATUS_DOWNLOADING,
                downloaded_bytes=0,
                total_bytes=total_expected_bytes,
                artifact_count=artifact_count,
            ),
        )
        downloaded: List[Dict[str, Any]] = []
        total_bytes = 0
        for index, artifact in enumerate(artifacts, start=1):
            artifact_path = str(artifact["path"])
            last_progress_write = 0.0

            def on_progress(artifact_bytes_written: int, *, path: str = artifact_path, artifact_index: int = index) -> None:
                nonlocal last_progress_write
                now = time.monotonic()
                if now - last_progress_write < 0.5 and artifact_bytes_written != int(artifact.get("size_bytes") or -1):
                    return
                last_progress_write = now
                _write_install_state(
                    model_id,
                    _install_progress_state(
                        model_id=model_id,
                        entry=entry,
                        status=VOICE_MODEL_STATUS_DOWNLOADING,
                        downloaded_bytes=total_bytes + int(artifact_bytes_written),
                        total_bytes=total_expected_bytes,
                        current_artifact_path=path,
                        artifact_index=artifact_index,
                        artifact_count=artifact_count,
                    ),
                )

            result = _download_artifact(
                artifact,
                output_path=install_dir / artifact_path,
                progress=on_progress,
            )
            if artifact.get("archive"):
                _safe_extract_tar(install_dir / artifact_path, install_dir)
            downloaded.append(result)
            total_bytes += int(result.get("size_bytes") or 0)
            _write_install_state(
                model_id,
                _install_progress_state(
                    model_id=model_id,
                    entry=entry,
                    status=VOICE_MODEL_STATUS_DOWNLOADING,
                    downloaded_bytes=total_bytes,
                    total_bytes=total_expected_bytes,
                    current_artifact_path=artifact_path,
                    artifact_index=index,
                    artifact_count=artifact_count,
                ),
            )
        payload = {
            "model_id": model_id,
            "status": VOICE_MODEL_STATUS_READY,
            "updated_at": utc_now_iso(),
            "installed_at": utc_now_iso(),
            "error": {},
            "manifest_sha256": entry.get("manifest_sha256"),
            "artifacts": downloaded,
            "total_size_bytes": int(total_bytes),
            "downloaded_bytes": int(total_bytes),
            "progress_percent": 100.0,
            "current_artifact_path": "",
            "artifact_index": artifact_count,
            "artifact_count": artifact_count,
            "command": _render_command_template(entry.get("command_template"), model_id=model_id, model_dir=install_dir),
        }
        _write_install_state(model_id, payload)
        return get_voice_model_status(model_id, source=source)
    except VoiceModelError as exc:
        _write_install_state(
            model_id,
            {
                **_install_progress_state(
                    model_id=model_id,
                    entry=entry,
                    status=VOICE_MODEL_STATUS_FAILED,
                    downloaded_bytes=int(_read_install_state(model_id).get("downloaded_bytes") or 0),
                    total_bytes=int(_read_install_state(model_id).get("total_size_bytes") or 0),
                    error={"code": exc.code, "message": exc.message, "details": exc.details},
                ),
            }
        )
        raise
    except Exception as exc:
        current = _read_install_state(model_id)
        _write_install_state(
            model_id,
            _install_progress_state(
                model_id=model_id,
                entry=entry,
                status=VOICE_MODEL_STATUS_FAILED,
                downloaded_bytes=int(current.get("downloaded_bytes") or 0),
                total_bytes=int(current.get("total_size_bytes") or total_expected_bytes),
                error={
                    "code": "voice_model_install_failed",
                    "message": str(exc),
                    "details": {"error_type": type(exc).__name__},
                },
            ),
        )
        raise
    finally:
        release_lockfile(lock_handle)


def begin_voice_model_install(model_id: str, *, source: str = "") -> Dict[str, Any]:
    model_id = str(model_id or "").strip()
    catalog = load_voice_model_catalog(source)
    entry = catalog.get(model_id)
    if entry is None:
        raise VoiceModelError(
            "voice_model_not_found",
            f"voice model not found: {model_id}",
            details={"model_id": model_id},
        )
    artifacts = entry.get("artifacts") or []
    _write_install_state(
        model_id,
        _install_progress_state(
            model_id=model_id,
            entry=entry,
            status=VOICE_MODEL_STATUS_DOWNLOADING,
            downloaded_bytes=0,
            total_bytes=sum(int(artifact.get("size_bytes") or 0) for artifact in artifacts),
            artifact_count=len(artifacts),
        ),
    )
    return get_voice_model_status(model_id, source=source)


def remove_voice_model(model_id: str, *, source: str = "") -> Dict[str, Any]:
    model_id = str(model_id or "").strip()
    catalog = load_voice_model_catalog(source)
    entry = catalog.get(model_id)
    if entry is None:
        raise VoiceModelError(
            "voice_model_not_found",
            f"voice model not found: {model_id}",
            details={"model_id": model_id},
        )
    status = get_voice_model_status(model_id, source=source)
    if str(status.get("status") or "") == VOICE_MODEL_STATUS_DOWNLOADING:
        raise VoiceModelError(
            "voice_model_busy",
            f"voice model is currently downloading: {model_id}",
            details={"model_id": model_id},
        )
    lock_handle = acquire_lockfile(_install_lock_path(model_id))
    try:
        _clear_voice_model_dir(model_id)
        _write_install_state(
            model_id,
            {
                "model_id": model_id,
                "status": VOICE_MODEL_STATUS_NOT_INSTALLED,
                "updated_at": utc_now_iso(),
                "installed_at": "",
                "error": {},
                "manifest_sha256": entry.get("manifest_sha256"),
                "artifacts": [],
                "total_size_bytes": sum(int(artifact.get("size_bytes") or 0) for artifact in (entry.get("artifacts") or [])),
                "downloaded_bytes": 0,
                "progress_percent": 0.0,
                "current_artifact_path": "",
                "artifact_index": 0,
                "artifact_count": len(entry.get("artifacts") or []),
                "command": "",
            },
        )
        return get_voice_model_status(model_id, source=source)
    finally:
        release_lockfile(lock_handle)


def resolve_installed_voice_model_command(model_id: str) -> str:
    model_id = str(model_id or "").strip()
    if not model_id:
        return ""
    status = get_voice_model_status(model_id)
    if str(status.get("status") or "") != VOICE_MODEL_STATUS_READY:
        return ""
    state = _read_install_state(model_id)
    command = str(state.get("command") or "").strip()
    if command:
        catalog = load_voice_model_catalog()
        entry = catalog.get(model_id)
        if entry is not None:
            return _render_command_template(entry.get("command_template"), model_id=model_id, model_dir=voice_model_dir(model_id))
    return command


def resolve_installed_voice_model_streaming_config(model_id: str, *, source: str = "") -> Dict[str, Any]:
    model_id = str(model_id or "").strip()
    if not model_id:
        return {}
    status = get_voice_model_status(model_id, source=source)
    if str(status.get("status") or "") != VOICE_MODEL_STATUS_READY:
        return {}
    catalog = load_voice_model_catalog(source)
    entry = catalog.get(model_id) or {}
    streaming = entry.get("streaming") if isinstance(entry.get("streaming"), dict) else {}
    if not streaming:
        return {}
    model_dir = voice_model_dir(model_id)
    resolved: Dict[str, Any] = {
        "model_id": model_id,
        "model_dir": str(model_dir),
        "engine": str(streaming.get("engine") or ""),
        "sample_rate": int(streaming.get("sample_rate") or 16000),
        "num_threads": int(streaming.get("num_threads") or 2),
        "provider": str(streaming.get("provider") or "cpu").strip() or "cpu",
    }
    for key in ("tokens", "model", "encoder", "decoder", "joiner", "bpe_vocab"):
        value = str(streaming.get(key) or "").strip()
        if value:
            resolved[key] = str(model_dir / value)
    return resolved


def resolve_installed_voice_model_diarization_config(model_id: str, *, source: str = "") -> Dict[str, Any]:
    model_id = str(model_id or "").strip()
    if not model_id:
        return {}
    status = get_voice_model_status(model_id, source=source)
    if str(status.get("status") or "") != VOICE_MODEL_STATUS_READY:
        return {}
    catalog = load_voice_model_catalog(source)
    entry = catalog.get(model_id) or {}
    diarization = entry.get("diarization") if isinstance(entry.get("diarization"), dict) else {}
    if not diarization:
        return {}
    model_dir = voice_model_dir(model_id)
    resolved: Dict[str, Any] = {
        "model_id": model_id,
        "model_dir": str(model_dir),
        "engine": str(diarization.get("engine") or "offline_speaker_diarization"),
        "sample_rate": int(diarization.get("sample_rate") or 16000),
        "num_threads": int(diarization.get("num_threads") or 2),
        "provider": str(diarization.get("provider") or "cpu").strip() or "cpu",
        "num_speakers": int(diarization.get("num_speakers") or -1),
        "cluster_threshold": float(diarization.get("cluster_threshold") or 0.5),
        "min_duration_on": float(diarization.get("min_duration_on") or 0.3),
        "min_duration_off": float(diarization.get("min_duration_off") or 0.5),
    }
    for key in ("segmentation_model", "embedding_model"):
        value = str(diarization.get(key) or "").strip()
        if value:
            resolved[key] = str(model_dir / value)
    return resolved
