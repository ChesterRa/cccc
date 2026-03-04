"""Install/uninstall mechanics, package command building, MCP roundtrip, and tool invocation."""

from __future__ import annotations

import hashlib
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
from datetime import datetime, timezone
from urllib.parse import urlparse
from typing import Any, Dict, List, Optional, Tuple
from urllib.request import Request, urlopen

from ....util.time import parse_utc_iso, utc_now_iso

from ._common import (
    _ARG_TEMPLATE_RE,
    _ENV_FORWARD_TEMPLATE_RE,
    _QUAL_BLOCKED,
    _env_int,
)
from ._runtime import _runtime_artifacts, _set_runtime_capability_artifact


def _pkg():
    """Get parent package module for mock-compatible function lookups."""
    return sys.modules[__name__.rsplit(".", 1)[0]]


def _install_spec_ready(rec: Dict[str, Any]) -> bool:
    install_mode = str(rec.get("install_mode") or "").strip()
    spec = rec.get("install_spec") if isinstance(rec.get("install_spec"), dict) else {}
    if install_mode == "remote_only":
        return bool(str(spec.get("url") or "").strip())
    if install_mode == "package":
        return bool(str(spec.get("identifier") or "").strip())
    if install_mode == "command":
        commands, _ = _command_stdio_command_candidates(rec)
        return bool(commands)
    return False


def _needs_registry_hydration(capability_id: str, rec: Dict[str, Any]) -> bool:
    cap_id = str(capability_id or "").strip()
    if not cap_id.startswith("mcp:"):
        return False
    if str(rec.get("kind") or "").strip().lower() != "mcp_toolpack":
        return False
    return not _install_spec_ready(rec)


def _merge_registry_install_into_record(rec: Dict[str, Any], fetched: Dict[str, Any]) -> Dict[str, Any]:
    merged = dict(rec)
    for key in (
        "install_mode",
        "install_spec",
        "source_uri",
        "source_record_id",
        "source_record_version",
        "updated_at_source",
        "last_synced_at",
        "health_status",
    ):
        if key in fetched:
            merged[key] = fetched.get(key)
    if not str(merged.get("name") or "").strip():
        merged["name"] = str(fetched.get("name") or "")
    if not str(merged.get("description_short") or "").strip():
        merged["description_short"] = str(fetched.get("description_short") or "")
    if not isinstance(merged.get("tags"), list) or not merged.get("tags"):
        merged["tags"] = list(fetched.get("tags") or [])
    qualification = str(merged.get("qualification_status") or "").strip().lower()
    merged["enable_supported"] = bool(_install_spec_ready(merged) and qualification != _QUAL_BLOCKED)
    return merged


def _github_headers() -> Dict[str, str]:
    headers = {
        "User-Agent": "cccc-capability-sync/1.0",
        "Accept": "application/vnd.github+json",
    }
    token = str(os.environ.get("CCCC_CAPABILITY_GITHUB_TOKEN") or "").strip()
    if token:
        headers["Authorization"] = f"Bearer {token}"
    return headers


def _catalog_staleness_seconds(last_synced_at: str) -> Optional[int]:
    dt = parse_utc_iso(str(last_synced_at or ""))
    if dt is None:
        return None
    now = datetime.now(timezone.utc)
    return max(0, int((now - dt).total_seconds()))


def _sanitize_tool_token(raw: str) -> str:
    token = re.sub(r"[^a-zA-Z0-9_]+", "_", str(raw or "").strip().lower()).strip("_")
    return token or "tool"


def _sanitize_skill_id_token(raw: str, *, default: str) -> str:
    token = re.sub(r"[^a-z0-9]+", "-", str(raw or "").strip().lower()).strip("-")
    return token or default


def _build_synthetic_tool_name(capability_id: str, real_tool_name: str, *, used: set[str]) -> str:
    cap_hash = hashlib.sha1(str(capability_id or "").encode("utf-8")).hexdigest()[:8]
    base = f"cccc_ext_{cap_hash}_{_sanitize_tool_token(real_tool_name)}"
    name = base
    i = 2
    while name in used:
        name = f"{base}_{i}"
        i += 1
    used.add(name)
    return name


def _normalize_mcp_input_schema(schema: Any) -> Dict[str, Any]:
    if isinstance(schema, dict) and str(schema.get("type") or "").strip():
        return dict(schema)
    return {"type": "object", "properties": {}, "required": []}


def _normalize_discovered_tools(capability_id: str, tools: Any) -> List[Dict[str, Any]]:
    if not isinstance(tools, list):
        return []
    out: List[Dict[str, Any]] = []
    used: set[str] = set()
    for item in tools:
        if not isinstance(item, dict):
            continue
        real_name = str(item.get("name") or "").strip()
        if not real_name:
            continue
        out.append(
            {
                "name": _build_synthetic_tool_name(capability_id, real_name, used=used),
                "real_tool_name": real_name,
                "description": str(item.get("description") or "").strip(),
                "inputSchema": _normalize_mcp_input_schema(item.get("inputSchema")),
            }
        )
    return out


def _normalize_registry_argument_entries(raw: Any) -> List[Dict[str, str]]:
    out: List[Dict[str, str]] = []
    if not isinstance(raw, list):
        return out
    for item in raw:
        if not isinstance(item, dict):
            continue
        arg_type = str(item.get("type") or "positional").strip().lower()
        if arg_type not in {"positional", "named"}:
            arg_type = "positional"
        value = str(item.get("value") or "").strip()
        name = str(item.get("name") or "").strip()
        if arg_type == "named":
            if not name:
                continue
            out.append({"type": "named", "name": name, "value": value})
            continue
        if not value:
            continue
        out.append({"type": "positional", "value": value})
    return out


def _normalize_registry_env_names(raw: Any, *, required_only: bool) -> List[str]:
    out: List[str] = []
    if not isinstance(raw, list):
        return out
    for item in raw:
        if not isinstance(item, dict):
            continue
        if required_only and (not bool(item.get("isRequired"))):
            continue
        name = str(item.get("name") or "").strip()
        if not name or name in out:
            continue
        out.append(name)
    return out


def _extract_required_env_from_runtime_arguments(raw: Any) -> List[str]:
    out: List[str] = []
    if not isinstance(raw, list):
        return out
    for item in raw:
        if not isinstance(item, dict):
            continue
        if str(item.get("type") or "").strip().lower() != "named":
            continue
        if str(item.get("name") or "").strip() not in {"-e", "--env"}:
            continue
        value = str(item.get("value") or "").strip()
        match = _ENV_FORWARD_TEMPLATE_RE.fullmatch(value)
        if not match:
            continue
        env_name = str(match.group(1) or "").strip()
        if not env_name:
            continue
        variables = item.get("variables") if isinstance(item.get("variables"), dict) else {}
        # Only treat as required when variable metadata explicitly marks it required.
        is_required = False
        for var_cfg in variables.values():
            if isinstance(var_cfg, dict) and bool(var_cfg.get("isRequired")):
                is_required = True
                break
        if is_required and env_name not in out:
            out.append(env_name)
    return out


def _literal_registry_argument_tokens(entries: List[Dict[str, str]]) -> Tuple[Optional[List[str]], str]:
    tokens: List[str] = []
    for item in entries:
        if not isinstance(item, dict):
            continue
        arg_type = str(item.get("type") or "positional").strip().lower()
        if arg_type == "named":
            name = str(item.get("name") or "").strip()
            value = str(item.get("value") or "").strip()
            if not name:
                continue
            tokens.append(name)
            if value:
                if _ARG_TEMPLATE_RE.search(value):
                    return None, "unsupported_argument_template"
                tokens.append(value)
            continue
        value = str(item.get("value") or "").strip()
        if not value:
            continue
        if _ARG_TEMPLATE_RE.search(value):
            return None, "unsupported_argument_template"
        tokens.append(value)
    return tokens, ""


def _oci_runtime_argument_tokens(entries: List[Dict[str, str]]) -> Tuple[Optional[List[str]], List[str], str]:
    tokens: List[str] = []
    forwarded_envs: List[str] = []
    for item in entries:
        if not isinstance(item, dict):
            continue
        arg_type = str(item.get("type") or "positional").strip().lower()
        if arg_type == "named":
            name = str(item.get("name") or "").strip()
            value = str(item.get("value") or "").strip()
            if not name:
                continue
            if value:
                match = _ENV_FORWARD_TEMPLATE_RE.fullmatch(value)
                if name in {"-e", "--env"} and match:
                    env_name = str(match.group(1) or "").strip()
                    if not env_name:
                        continue
                    tokens.extend([name, env_name])
                    if env_name not in forwarded_envs:
                        forwarded_envs.append(env_name)
                    continue
                if _ARG_TEMPLATE_RE.search(value):
                    return None, [], "unsupported_runtime_argument_template"
                tokens.extend([name, value])
            else:
                tokens.append(name)
            continue
        value = str(item.get("value") or "").strip()
        if not value:
            continue
        if _ARG_TEMPLATE_RE.search(value):
            return None, [], "unsupported_runtime_argument_template"
        tokens.append(value)
    return tokens, forwarded_envs, ""


def _required_environment_names(rec: Dict[str, Any]) -> List[str]:
    install_spec = rec.get("install_spec") if isinstance(rec.get("install_spec"), dict) else {}
    raw = install_spec.get("required_env")
    out: List[str] = []
    if isinstance(raw, list):
        for item in raw:
            name = str(item or "").strip()
            if name and name not in out:
                out.append(name)
    return out


def _missing_required_environment_names(rec: Dict[str, Any]) -> List[str]:
    missing: List[str] = []
    for name in _required_environment_names(rec):
        value = str(os.environ.get(name) or "").strip()
        if value:
            continue
        missing.append(name)
    return missing


def _normalize_registry_type_token(raw: str) -> str:
    token = str(raw or "").strip().lower()
    if token in {"", "npm", "node", "nodejs", "npmjs", "javascript", "js"}:
        return "npm"
    if token in {"pypi", "python", "python3", "py", "pip", "pip3", "pipx", "uvx", "uv", "poetry"}:
        return "pypi"
    if token in {"oci", "docker", "container", "container_image", "container-image", "podman", "ghcr", "ghcr.io"}:
        return "oci"
    return token


def _effective_registry_type(install_spec: Dict[str, Any]) -> str:
    spec = install_spec if isinstance(install_spec, dict) else {}
    declared = _normalize_registry_type_token(str(spec.get("registry_type") or ""))
    if declared in {"npm", "pypi", "oci"}:
        return declared
    runtime_hint = _normalize_registry_type_token(str(spec.get("runtime_hint") or ""))
    if runtime_hint in {"npm", "pypi", "oci"}:
        return runtime_hint
    identifier = str(spec.get("identifier") or "").strip().lower()
    if identifier.startswith(("docker://", "oci://")):
        return "oci"
    if identifier.startswith(("ghcr.io/", "docker.io/", "quay.io/")):
        return "oci"
    if identifier.endswith(".whl"):
        return "pypi"
    return declared or "npm"


def _normalize_command_token_list(raw: Any) -> List[str]:
    if isinstance(raw, list):
        return [str(x).strip() for x in raw if str(x).strip()]
    if isinstance(raw, str):
        text = str(raw).strip()
        if not text:
            return []
        try:
            return [str(x).strip() for x in shlex.split(text) if str(x).strip()]
        except Exception:
            return [token for token in text.split() if token]
    return []


def _collect_command_candidates(
    install_spec: Dict[str, Any],
    *,
    include_primary: bool,
    include_fallback: bool,
) -> List[List[str]]:
    out: List[List[str]] = []

    def _append(raw: Any) -> None:
        cmd = _normalize_command_token_list(raw)
        if cmd and cmd not in out:
            out.append(cmd)

    if include_primary:
        primary_list = install_spec.get("command_candidates")
        if isinstance(primary_list, list):
            for row in primary_list:
                _append(row)
        _append(install_spec.get("command"))
    if include_fallback:
        fallback_list = install_spec.get("fallback_command_candidates")
        if isinstance(fallback_list, list):
            for row in fallback_list:
                _append(row)
        _append(install_spec.get("fallback_command"))
    return out


def _command_stdio_command_candidates(rec: Dict[str, Any]) -> Tuple[List[List[str]], str]:
    install_spec = rec.get("install_spec") if isinstance(rec.get("install_spec"), dict) else {}
    commands = _collect_command_candidates(
        install_spec,
        include_primary=True,
        include_fallback=False,
    )
    if commands:
        return commands, ""
    return [], "missing_command_candidate"


def _package_fallback_command_candidates(rec: Dict[str, Any]) -> Tuple[List[List[str]], str]:
    install_spec = rec.get("install_spec") if isinstance(rec.get("install_spec"), dict) else {}
    commands = _collect_command_candidates(
        install_spec,
        include_primary=True,
        include_fallback=True,
    )
    if commands:
        return commands, ""
    return [], "missing_command_candidate"


def _tool_name_aliases(raw: str) -> List[str]:
    name = str(raw or "").strip()
    if not name:
        return []
    out: List[str] = []
    for token in (name, name.replace("-", "_"), name.replace("_", "-")):
        if token and token not in out:
            out.append(token)
    return out


def _is_unknown_tool_error_message(msg: str) -> bool:
    text = str(msg or "").strip().lower()
    return "unknown tool" in text or "tool not found" in text


def _npx_package_command(rec: Dict[str, Any]) -> Tuple[Optional[List[str]], str]:
    install_spec = rec.get("install_spec") if isinstance(rec.get("install_spec"), dict) else {}
    identifier = str(install_spec.get("identifier") or "").strip()
    if not identifier:
        return None, "missing_package_identifier"
    version = str(install_spec.get("version") or "").strip()
    pkg = identifier
    if version and "@" in identifier[1:]:
        pkg = identifier
    elif version:
        pkg = f"{identifier}@{version}"
    package_args = _normalize_registry_argument_entries(install_spec.get("package_arguments"))
    package_tokens, package_reason = _literal_registry_argument_tokens(package_args)
    if package_tokens is None:
        return None, package_reason or "unsupported_package_arguments"
    runtime_hint_raw = str(install_spec.get("runtime_hint") or "").strip().lower()
    if runtime_hint_raw and runtime_hint_raw not in {"auto", "npx", "npm", "node", "nodejs"}:
        runtime_hint = _normalize_registry_type_token(runtime_hint_raw)
        if runtime_hint != "npm":
            return None, "unsupported_runtime_hint"
    return ["npx", "-y", pkg, *(package_tokens or [])], ""


def _pypi_package_commands(rec: Dict[str, Any]) -> Tuple[List[List[str]], str]:
    install_spec = rec.get("install_spec") if isinstance(rec.get("install_spec"), dict) else {}
    identifier = str(install_spec.get("identifier") or "").strip()
    if not identifier:
        return [], "missing_package_identifier"
    version = str(install_spec.get("version") or "").strip()
    base_spec = identifier
    if version and "@" in identifier[1:]:
        base_spec = identifier
    elif version:
        base_spec = f"{identifier}@{version}"

    runtime_entries = _normalize_registry_argument_entries(install_spec.get("runtime_arguments"))
    package_entries = _normalize_registry_argument_entries(install_spec.get("package_arguments"))
    runtime_tokens, runtime_reason = _literal_registry_argument_tokens(runtime_entries)
    if runtime_tokens is None:
        return [], runtime_reason or "unsupported_runtime_arguments"
    package_tokens, package_reason = _literal_registry_argument_tokens(package_entries)
    if package_tokens is None:
        return [], package_reason or "unsupported_package_arguments"

    runtime_hint = str(install_spec.get("runtime_hint") or "").strip().lower()
    if runtime_hint and runtime_hint not in {"uvx", "uv", "pipx", "python", "python3", "py", "pip", "pip3", "auto"}:
        return [], "unsupported_runtime_hint"

    # Prefer uvx for modern pypi MCP servers; fall back to pipx where needed.
    runners: List[str] = []
    if runtime_hint in {"", "uvx", "uv", "python", "python3", "py", "pip", "pip3", "auto"}:
        runners.append("uvx")
    if runtime_hint in {"", "pipx", "python", "python3", "py", "auto"}:
        runners.append("pipx")

    commands: List[List[str]] = []
    for runner in runners:
        if runner == "uvx":
            cmd = ["uvx", *((runtime_tokens or [base_spec]))]
            cmd.extend(package_tokens or [])
            commands.append(cmd)
            continue
        # pipx does not understand runtimeArguments shape; use --spec + entrypoint fallback.
        cmd = ["pipx", "run", "--spec", base_spec, identifier]
        cmd.extend(package_tokens or [])
        commands.append(cmd)
    return commands, ""


def _oci_package_commands(rec: Dict[str, Any]) -> Tuple[List[List[str]], str]:
    install_spec = rec.get("install_spec") if isinstance(rec.get("install_spec"), dict) else {}
    image = str(install_spec.get("identifier") or "").strip()
    if not image:
        return [], "missing_package_identifier"

    runtime_entries = _normalize_registry_argument_entries(install_spec.get("runtime_arguments"))
    package_entries = _normalize_registry_argument_entries(install_spec.get("package_arguments"))
    runtime_tokens, forwarded_envs, runtime_reason = _oci_runtime_argument_tokens(runtime_entries)
    if runtime_tokens is None:
        return [], runtime_reason or "unsupported_runtime_arguments"
    package_tokens, package_reason = _literal_registry_argument_tokens(package_entries)
    if package_tokens is None:
        return [], package_reason or "unsupported_package_arguments"

    runtime_hint = str(install_spec.get("runtime_hint") or "").strip().lower()
    if runtime_hint and runtime_hint not in {"docker", "podman", "container", "container-image", "container_image", "oci", "auto"}:
        return [], "unsupported_runtime_hint"
    engines: List[str] = []
    if runtime_hint in {"podman"}:
        engines.append("podman")
    elif runtime_hint in {"docker"}:
        engines.append("docker")
    else:
        engines.extend(["docker", "podman"])

    required_env = _required_environment_names(rec)
    env_names = install_spec.get("env_names") if isinstance(install_spec.get("env_names"), list) else []
    all_env_forward: List[str] = []
    for token in [*forwarded_envs, *required_env, *[str(x).strip() for x in env_names if str(x).strip()]]:
        if token and token not in all_env_forward:
            all_env_forward.append(token)

    commands: List[List[str]] = []
    for engine in engines:
        cmd = [engine, "run", "-i", "--rm"]
        for env_name in all_env_forward:
            cmd.extend(["-e", env_name])
        cmd.extend(runtime_tokens or [])
        cmd.append(image)
        cmd.extend(package_tokens or [])
        commands.append(cmd)
    return commands, ""


def _package_stdio_command_candidates(rec: Dict[str, Any]) -> Tuple[List[List[str]], str]:
    install_spec = rec.get("install_spec") if isinstance(rec.get("install_spec"), dict) else {}
    registry_type = _effective_registry_type(install_spec)
    if registry_type == "npm":
        command, reason = _npx_package_command(rec)
        if not command:
            return [], reason
        commands: List[List[str]] = [command]
        identifier = str(install_spec.get("identifier") or "").strip()
        version = str(install_spec.get("version") or "").strip()
        if identifier and version:
            fallback_spec = dict(install_spec)
            fallback_spec["version"] = ""
            fallback_rec = dict(rec)
            fallback_rec["install_spec"] = fallback_spec
            fallback_command, fallback_reason = _npx_package_command(fallback_rec)
            if fallback_command and fallback_command not in commands:
                commands.append(fallback_command)
            elif (not fallback_command) and fallback_reason:
                return commands, reason
        return commands, reason
    if registry_type == "pypi":
        return _pypi_package_commands(rec)
    if registry_type == "oci":
        return _oci_package_commands(rec)
    return [], f"unsupported_registry_type:{registry_type}"


def _choose_available_command(commands: List[List[str]]) -> List[List[str]]:
    if not isinstance(commands, list):
        return []
    if len(commands) <= 1:
        return commands
    available: List[List[str]] = []
    unavailable: List[List[str]] = []
    for cmd in commands:
        if not isinstance(cmd, list) or not cmd:
            continue
        exe = str(cmd[0] or "").strip()
        if exe and shutil.which(exe):
            available.append(cmd)
        else:
            unavailable.append(cmd)
    return [*available, *unavailable]


def _installer_label_for_command(rec: Dict[str, Any], command: List[str]) -> str:
    install_spec = rec.get("install_spec") if isinstance(rec.get("install_spec"), dict) else {}
    install_mode = str(rec.get("install_mode") or "").strip().lower()
    registry_type = _effective_registry_type(install_spec)
    exe = str((command or [None])[0] or "").strip().lower()
    if install_mode == "command":
        if exe in {"npx", "uvx", "pipx", "docker", "podman"}:
            return f"command_{exe}"
        return "command_stdio"
    if registry_type == "npm":
        return "npm_npx"
    if registry_type == "pypi":
        if exe == "pipx":
            return "pypi_pipx"
        return "pypi_uvx"
    if registry_type == "oci":
        if exe == "podman":
            return "oci_podman"
        return "oci_docker"
    return f"{registry_type}_stdio"


def _stdio_mcp_roundtrip(
    command: List[str],
    requests: List[Dict[str, Any]],
    *,
    timeout_s: float,
    env_override: Optional[Dict[str, str]] = None,
) -> List[Dict[str, Any]]:
    env: Optional[Dict[str, str]] = None
    if isinstance(env_override, dict):
        merged = dict(os.environ)
        for key, value in env_override.items():
            k = str(key or "").strip()
            if not k:
                continue
            merged[k] = str(value or "")
        env = merged
    proc = subprocess.Popen(
        command,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        env=env,
    )
    input_blob = "\n".join(json.dumps(req, ensure_ascii=False) for req in requests) + "\n"
    try:
        out, err = proc.communicate(input=input_blob, timeout=max(2.0, float(timeout_s)))
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.communicate()
        raise TimeoutError("stdio mcp request timed out")
    if proc.returncode not in {0, None} and not out.strip():
        raise RuntimeError(f"stdio mcp exited with code {proc.returncode}: {err.strip()}")
    responses: List[Dict[str, Any]] = []
    for line in str(out or "").splitlines():
        raw = line.strip()
        if not raw:
            continue
        try:
            item = json.loads(raw)
        except Exception:
            continue
        if isinstance(item, dict):
            responses.append(item)
    return responses


def _extract_jsonrpc_result(
    responses: List[Dict[str, Any]],
    *,
    req_id: int,
    operation: str,
) -> Dict[str, Any]:
    for item in responses:
        if int(item.get("id") or -1) != int(req_id):
            continue
        err = item.get("error")
        if isinstance(err, dict):
            raise RuntimeError(f"{operation} failed: {str(err.get('message') or 'unknown error')}")
        result = item.get("result")
        return result if isinstance(result, dict) else {}
    raise RuntimeError(f"{operation} failed: missing response")


def _http_jsonrpc_request(
    url: str,
    payload: Dict[str, Any],
    *,
    timeout_s: float,
    session_id: str = "",
) -> Tuple[Dict[str, Any], str]:
    body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
    req = Request(url, data=body, method="POST")
    req.add_header("Content-Type", "application/json")
    req.add_header("Accept", "application/json")
    if session_id:
        req.add_header("Mcp-Session-Id", session_id)
    with urlopen(req, timeout=max(2.0, float(timeout_s))) as resp:
        payload_text = resp.read().decode("utf-8", errors="replace")
        out_session = str(resp.headers.get("Mcp-Session-Id") or session_id or "").strip()
    data = json.loads(payload_text) if payload_text.strip() else {}
    if not isinstance(data, dict):
        raise ValueError("remote mcp response is not JSON object")
    return data, out_session


def _remote_mcp_call(url: str, method: str, params: Dict[str, Any], *, timeout_s: float) -> Dict[str, Any]:
    init_req = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "cccc-capability-runtime", "version": "1.0"},
        },
    }
    init_resp, session_id = _http_jsonrpc_request(url, init_req, timeout_s=timeout_s)
    if isinstance(init_resp.get("error"), dict):
        raise RuntimeError(str((init_resp.get("error") or {}).get("message") or "remote initialize failed"))
    call_req = {
        "jsonrpc": "2.0",
        "id": 2,
        "method": str(method or ""),
        "params": params if isinstance(params, dict) else {},
    }
    call_resp, _ = _http_jsonrpc_request(url, call_req, timeout_s=timeout_s, session_id=session_id)
    if isinstance(call_resp.get("error"), dict):
        raise RuntimeError(str((call_resp.get("error") or {}).get("message") or f"remote {method} failed"))
    result = call_resp.get("result")
    return result if isinstance(result, dict) else {}


def _supported_external_install_record(rec: Dict[str, Any]) -> Tuple[bool, str]:
    install_mode = str(rec.get("install_mode") or "").strip()
    spec = rec.get("install_spec") if isinstance(rec.get("install_spec"), dict) else {}
    if install_mode == "remote_only":
        transport = str(spec.get("transport") or "").strip().lower()
        if transport in {"", "streamable-http", "http", "sse"}:
            if str(spec.get("url") or "").strip():
                return True, ""
            return False, "missing_remote_url"
        return False, f"unsupported_remote_transport:{transport or 'unknown'}"
    if install_mode == "package":
        commands, reason = _package_stdio_command_candidates(rec)
        if commands:
            return True, ""
        fallback_commands, _ = _package_fallback_command_candidates(rec)
        if fallback_commands:
            return True, ""
        if str(reason or "").startswith("unsupported_registry_type:"):
            return False, "unsupported_registry_type"
        return False, (reason or "unsupported_runtime_hint")
    if install_mode == "command":
        commands, reason = _command_stdio_command_candidates(rec)
        if not commands:
            return False, (reason or "missing_command_candidate")
        return True, ""
    return False, f"unsupported_install_mode:{install_mode or 'unknown'}"


def _record_enable_supported(rec: Dict[str, Any], *, capability_id: str = "") -> bool:
    raw = rec.get("enable_supported")
    if isinstance(raw, bool):
        return raw
    cap_id = str(capability_id or rec.get("capability_id") or "").strip()
    kind = str(rec.get("kind") or "").strip().lower()
    qualification = str(rec.get("qualification_status") or "").strip().lower()
    if qualification == _QUAL_BLOCKED:
        return False
    if cap_id.startswith("pack:"):
        return True
    if kind == "skill":
        return qualification != _QUAL_BLOCKED
    if _needs_registry_hydration(cap_id, rec):
        return True
    supported, _ = _supported_external_install_record(rec)
    return bool(supported)


def _external_artifact_cache_key(rec: Dict[str, Any], *, capability_id: str) -> str:
    install_mode = str(rec.get("install_mode") or "").strip().lower()
    spec = rec.get("install_spec") if isinstance(rec.get("install_spec"), dict) else {}
    if install_mode == "remote_only":
        url = str(spec.get("url") or "").strip()
        if url:
            return f"remote_only::{url}"
    if install_mode == "package":
        registry_type = _effective_registry_type(spec)
        identifier = str(spec.get("identifier") or "").strip()
        version = str(spec.get("version") or "").strip()
        runtime_hint = str(spec.get("runtime_hint") or "").strip().lower()
        runtime_args = spec.get("runtime_arguments") if isinstance(spec.get("runtime_arguments"), list) else []
        package_args = spec.get("package_arguments") if isinstance(spec.get("package_arguments"), list) else []
        args_digest = ""
        if runtime_args or package_args:
            try:
                args_payload = json.dumps(
                    {"runtime": runtime_args, "package": package_args},
                    ensure_ascii=False,
                    sort_keys=True,
                )
                args_digest = hashlib.sha1(args_payload.encode("utf-8")).hexdigest()[:8]
            except Exception:
                args_digest = ""
        if identifier:
            return f"package::{registry_type}::{identifier}::{version}::{runtime_hint}::{args_digest}"
    if install_mode == "command":
        commands = _collect_command_candidates(
            spec,
            include_primary=True,
            include_fallback=True,
        )
        env_map = spec.get("env") if isinstance(spec.get("env"), dict) else {}
        env_names = sorted(str(k).strip() for k in env_map.keys() if str(k).strip())
        payload = {
            "commands": commands,
            "required_env": _required_environment_names(rec),
            "env_names": env_names,
        }
        try:
            digest = hashlib.sha1(json.dumps(payload, ensure_ascii=False, sort_keys=True).encode("utf-8")).hexdigest()[:16]
        except Exception:
            digest = hashlib.sha1(str(payload).encode("utf-8")).hexdigest()[:16]
        return f"command::{digest}"
    return f"capability::{str(capability_id or '').strip()}"


def _external_artifact_id(rec: Dict[str, Any], *, capability_id: str) -> str:
    key = _external_artifact_cache_key(rec, capability_id=capability_id)
    return f"art_{hashlib.sha1(key.encode('utf-8')).hexdigest()[:16]}"


def _is_package_probe_degradable_error(err: Exception) -> bool:
    if isinstance(err, TimeoutError):
        return True
    text = str(err or "").strip().lower()
    if not text:
        return False
    return any(
        token in text
        for token in (
            "stdio mcp request timed out",
            "tools/list returned no tools",
            "tools/list failed: missing response",
            "missing response",
        )
    )


def _classify_external_install_error(err: Exception) -> Dict[str, Any]:
    message = str(err or "").strip()
    text = message.lower()
    out: Dict[str, Any] = {
        "code": "install_failed",
        "message": message,
        "retryable": False,
    }
    if not message:
        return out

    if message.startswith("missing_required_env:"):
        raw_names = [x.strip() for x in message.split(":", 1)[1].split(",")]
        names = [x for x in raw_names if x]
        out["code"] = "missing_required_env"
        out["required_env"] = names
        return out
    if message.startswith("unsupported_registry_type:"):
        out["code"] = "unsupported_registry_type"
        out["registry_type"] = message.split(":", 1)[1].strip()
        return out
    if message == "unsupported_registry_type":
        out["code"] = "unsupported_registry_type"
        return out
    if message.startswith("invalid_remote_url"):
        out["code"] = "invalid_remote_url"
        return out
    if message.startswith("missing_command_candidate"):
        out["code"] = "missing_command_candidate"
        return out
    if message.startswith("unsupported_install_mode:"):
        out["code"] = "unsupported_install_mode"
        out["install_mode"] = message.split(":", 1)[1].strip()
        return out
    if message.startswith("unsupported_runtime_hint"):
        out["code"] = "unsupported_runtime_hint"
        return out
    if message.startswith("missing_package_identifier"):
        out["code"] = "missing_package_identifier"
        return out
    if message.startswith("missing_remote_url"):
        out["code"] = "missing_remote_url"
        return out
    if "probe_failed" in text:
        out["code"] = "probe_failed"
        out["retryable"] = True
        return out
    if isinstance(err, TimeoutError) or ("timed out" in text) or ("timeout" in text):
        out["code"] = "probe_timeout"
        out["retryable"] = True
        return out
    if isinstance(err, FileNotFoundError):
        out["code"] = "runtime_binary_missing"
        return out
    if "permission denied while trying to connect to the docker daemon socket" in text:
        out["code"] = "runtime_permission_denied"
        return out
    if "permission denied" in text and ("docker.sock" in text or "podman" in text):
        out["code"] = "runtime_permission_denied"
        return out
    if "cannot find module" in text or "module_not_found" in text:
        out["code"] = "runtime_dependency_missing"
        return out
    if "stdio mcp exited with code" in text:
        out["code"] = "runtime_start_failed"
        return out
    if any(token in text for token in ("name or service not known", "temporary failure in name resolution", "nodename nor servname provided")):
        out["code"] = "network_dns_failure"
        out["retryable"] = True
        return out
    if any(token in text for token in ("connection refused", "connection reset", "network is unreachable")):
        out["code"] = "network_unreachable"
        out["retryable"] = True
        return out
    if "http error 401" in text or "unauthorized" in text:
        out["code"] = "upstream_unauthorized"
        return out
    if "http error 403" in text or "forbidden" in text:
        out["code"] = "upstream_forbidden"
        return out
    return out


def _diagnostics_from_install_error(err: Exception) -> List[Dict[str, Any]]:
    info = _classify_external_install_error(err)
    code = str(info.get("code") or "install_failed")
    message = str(info.get("message") or str(err or "")).strip()
    diag: Dict[str, Any] = {
        "code": code,
        "message": message,
        "retryable": bool(info.get("retryable")),
    }
    required_env = info.get("required_env")
    if isinstance(required_env, list) and required_env:
        diag["required_env"] = [str(x).strip() for x in required_env if str(x).strip()]
    action_hints: List[str] = []
    if code == "missing_required_env":
        action_hints.append("set_required_env_then_retry")
        action_hints.append("request_user_secrets_if_unavailable")
    elif code == "runtime_binary_missing":
        action_hints.append("install_or_expose_runtime_binary_then_retry")
        action_hints.append("fallback_to_install_mode_command_if_available")
    elif code == "runtime_permission_denied":
        action_hints.append("grant_runtime_permission_then_retry")
    elif code in {"runtime_dependency_missing", "runtime_start_failed"}:
        action_hints.append("retry_with_safe_runtime_flags_or_different_version")
        action_hints.append("fallback_to_install_mode_command_if_available")
    elif code in {"probe_timeout", "network_dns_failure", "network_unreachable"}:
        action_hints.append("retry_or_fix_network_then_retry")
    elif code in {"unsupported_registry_type", "unsupported_runtime_hint", "missing_package_identifier"}:
        action_hints.append("fallback_to_install_mode_command_if_available")
    elif code == "missing_command_candidate":
        action_hints.append("provide_command_or_command_candidates")
    elif code == "invalid_remote_url":
        action_hints.append("provide_valid_remote_http_url")
    if action_hints:
        diag["action_hints"] = action_hints
    return [diag]


def _command_base_env_override(rec: Dict[str, Any]) -> Dict[str, str]:
    install_spec = rec.get("install_spec") if isinstance(rec.get("install_spec"), dict) else {}
    raw = install_spec.get("env") if isinstance(install_spec.get("env"), dict) else {}
    out: Dict[str, str] = {}
    for key, value in raw.items():
        name = str(key or "").strip()
        if not name:
            continue
        out[name] = str(value or "")
    return out


def _merge_env_maps(base: Dict[str, str], overlay: Dict[str, str]) -> Dict[str, str]:
    merged = dict(base)
    merged.update(overlay)
    return merged


def _missing_command_binaries(commands: List[List[str]]) -> List[str]:
    missing: List[str] = []
    for cmd in commands:
        if not isinstance(cmd, list) or not cmd:
            continue
        exe = str(cmd[0] or "").strip()
        if not exe:
            continue
        if shutil.which(exe):
            continue
        if exe not in missing:
            missing.append(exe)
    return missing


def _preflight_external_install(rec: Dict[str, Any], *, capability_id: str) -> Dict[str, Any]:
    cid = str(capability_id or "").strip()
    install_mode = str(rec.get("install_mode") or "").strip().lower()
    supported, reason = _supported_external_install_record(rec)
    if not supported:
        code = reason or "unsupported_external_installer"
        diagnostics = _diagnostics_from_install_error(ValueError(code))
        return {
            "ok": False,
            "code": code,
            "message": code,
            "capability_id": cid,
            "diagnostics": diagnostics,
        }

    missing_env = _missing_required_environment_names(rec)
    if missing_env:
        msg = "missing_required_env:" + ",".join(sorted(set(missing_env)))
        diagnostics = _diagnostics_from_install_error(ValueError(msg))
        return {
            "ok": False,
            "code": "missing_required_env",
            "message": msg,
            "capability_id": cid,
            "required_env": sorted(set(missing_env)),
            "diagnostics": diagnostics,
        }

    if install_mode == "remote_only":
        spec = rec.get("install_spec") if isinstance(rec.get("install_spec"), dict) else {}
        url = str(spec.get("url") or "").strip()
        parsed = urlparse(url)
        if not parsed.scheme or parsed.scheme not in {"http", "https"} or not parsed.netloc:
            msg = "invalid_remote_url"
            diagnostics = _diagnostics_from_install_error(ValueError(msg))
            return {
                "ok": False,
                "code": "invalid_remote_url",
                "message": msg,
                "capability_id": cid,
                "diagnostics": diagnostics,
            }
        return {"ok": True, "code": "", "message": "", "capability_id": cid, "diagnostics": []}

    if install_mode == "package":
        # Include package-derived commands and explicit fallback command candidates.
        # Preflight should pass when any viable command path is available.
        commands, _ = _package_fallback_command_candidates(rec)
        if not commands:
            commands, _ = _package_stdio_command_candidates(rec)
    elif install_mode == "command":
        commands, _ = _command_stdio_command_candidates(rec)
    else:
        commands = []

    commands = _choose_available_command(commands)
    if commands:
        has_available = any(
            bool(shutil.which(str(cmd[0] or "").strip()))
            for cmd in commands
            if isinstance(cmd, list) and cmd
        )
        if not has_available:
            missing_bins = _missing_command_binaries(commands)
            msg = "runtime_binary_missing:" + ",".join(missing_bins)
            diagnostics = _diagnostics_from_install_error(FileNotFoundError(msg))
            out: Dict[str, Any] = {
                "ok": False,
                "code": "runtime_binary_missing",
                "message": msg,
                "capability_id": cid,
                "diagnostics": diagnostics,
            }
            if missing_bins:
                out["missing_binaries"] = missing_bins
            return out

    return {"ok": True, "code": "", "message": "", "capability_id": cid, "diagnostics": []}


def _install_via_stdio_commands(
    rec: Dict[str, Any],
    *,
    capability_id: str,
    commands: List[List[str]],
    install_mode_label: str,
    base_env_override: Optional[Dict[str, str]] = None,
    fallback_from_mode: str = "",
    fallback_reason: str = "",
) -> Dict[str, Any]:
    missing_env = _missing_required_environment_names(rec)
    if missing_env:
        raise ValueError("missing_required_env:" + ",".join(sorted(set(missing_env))))
    requests = [
        {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "cccc-capability-runtime", "version": "1.0"},
            },
        },
        {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}},
    ]
    commands = _pkg()._choose_available_command(commands)
    probe_timeout_s = float(max(5, min(_env_int("CCCC_CAPABILITY_PACKAGE_PROBE_TIMEOUT_SECONDS", 30), 120)))
    base_env = dict(base_env_override or {})
    last_error: Optional[Exception] = None
    preferred_error: Optional[Exception] = None
    chosen_command: List[str] = []
    chosen_env: Dict[str, str] = {}
    tools: List[Dict[str, Any]] = []
    for command in commands:
        exe = str((command or [""])[0] or "").strip().lower()
        attempt_envs: List[Dict[str, str]] = [dict(base_env)]
        if exe == "npx":
            safe_env = _merge_env_maps(
                base_env,
                {
                    "PUPPETEER_SKIP_DOWNLOAD": "1",
                    "PUPPETEER_SKIP_CHROMIUM_DOWNLOAD": "1",
                    "PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD": "1",
                },
            )
            if safe_env != attempt_envs[0]:
                attempt_envs.append(safe_env)
        for env_try in attempt_envs:
            try:
                if env_try:
                    responses = _pkg()._stdio_mcp_roundtrip(
                        command,
                        requests,
                        timeout_s=probe_timeout_s,
                        env_override=env_try,
                    )
                else:
                    responses = _pkg()._stdio_mcp_roundtrip(
                        command,
                        requests,
                        timeout_s=probe_timeout_s,
                    )
                tools_result = _extract_jsonrpc_result(responses, req_id=2, operation="tools/list")
                tools = _normalize_discovered_tools(capability_id, tools_result.get("tools"))
                if not tools:
                    raise RuntimeError("stdio tools/list returned no tools")
                chosen_command = list(command)
                chosen_env = dict(env_try)
                break
            except FileNotFoundError as e:
                last_error = e
                if preferred_error is None:
                    preferred_error = e
                continue
            except Exception as e:
                last_error = e
                preferred_error = e
                continue
        if chosen_command:
            break
    if not chosen_command:
        effective_error = preferred_error if isinstance(preferred_error, Exception) else last_error
        if isinstance(effective_error, Exception) and commands and _is_package_probe_degradable_error(effective_error):
            fallback_command = list(commands[0])
            classified = _classify_external_install_error(effective_error)
            result: Dict[str, Any] = {
                "state": "installed_degraded",
                "installer": _installer_label_for_command(
                    {"install_mode": install_mode_label, "install_spec": rec.get("install_spec")},
                    fallback_command,
                ),
                "install_mode": install_mode_label,
                "invoker": {
                    "type": "command_stdio" if install_mode_label == "command" else "package_stdio",
                    "command": fallback_command,
                },
                "tools": [],
                "last_error": str(effective_error),
                "last_error_code": str(classified.get("code") or "probe_timeout"),
                "retryable": bool(classified.get("retryable")),
                "updated_at": utc_now_iso(),
            }
            if base_env:
                result["invoker"]["env"] = dict(base_env)
            if fallback_from_mode:
                result["fallback_from"] = fallback_from_mode
            if fallback_reason:
                result["fallback_reason"] = fallback_reason
            return result
        if isinstance(effective_error, Exception):
            raise effective_error
        raise RuntimeError("package install failed: no runnable command candidate")
    invoker: Dict[str, Any] = {
        "type": "command_stdio" if install_mode_label == "command" else "package_stdio",
        "command": chosen_command,
    }
    if chosen_env:
        invoker["env"] = chosen_env
    result = {
        "state": "installed",
        "installer": _installer_label_for_command(
            {"install_mode": install_mode_label, "install_spec": rec.get("install_spec")},
            chosen_command,
        ),
        "install_mode": install_mode_label,
        "invoker": invoker,
        "tools": tools,
        "last_error": "",
        "updated_at": utc_now_iso(),
    }
    if fallback_from_mode:
        result["fallback_from"] = fallback_from_mode
    if fallback_reason:
        result["fallback_reason"] = fallback_reason
    return result


def _artifact_entry_from_install(
    install: Dict[str, Any],
    *,
    artifact_id: str,
    install_key: str,
    capability_id: str,
) -> Dict[str, Any]:
    return {
        "artifact_id": str(artifact_id or "").strip(),
        "install_key": str(install_key or "").strip(),
        "state": str(install.get("state") or "").strip() or "unknown",
        "installer": str(install.get("installer") or "").strip(),
        "install_mode": str(install.get("install_mode") or "").strip(),
        "invoker": dict(install.get("invoker")) if isinstance(install.get("invoker"), dict) else {},
        "tools": list(install.get("tools") or []) if isinstance(install.get("tools"), list) else [],
        "last_error": str(install.get("last_error") or "").strip(),
        "last_error_code": str(install.get("last_error_code") or "").strip(),
        "updated_at": str(install.get("updated_at") or "").strip() or utc_now_iso(),
        "capability_ids": [str(capability_id or "").strip()] if str(capability_id or "").strip() else [],
    }


def _upsert_runtime_artifact_for_capability(
    runtime_doc: Dict[str, Any],
    *,
    artifact_id: str,
    capability_id: str,
    artifact_entry: Dict[str, Any],
) -> None:
    aid = str(artifact_id or "").strip()
    cid = str(capability_id or "").strip()
    if not aid or not cid:
        return
    artifacts = _runtime_artifacts(runtime_doc)
    row = dict(artifact_entry) if isinstance(artifact_entry, dict) else {}
    row["artifact_id"] = aid
    caps_raw = row.get("capability_ids")
    caps = [str(x).strip() for x in caps_raw if str(x).strip()] if isinstance(caps_raw, list) else []
    if cid not in caps:
        caps.append(cid)
    row["capability_ids"] = caps
    artifacts[aid] = row
    runtime_doc["artifacts"] = artifacts
    _set_runtime_capability_artifact(runtime_doc, capability_id=cid, artifact_id=aid)

def _install_external_capability(rec: Dict[str, Any], *, capability_id: str) -> Dict[str, Any]:
    install_mode = str(rec.get("install_mode") or "").strip()
    if install_mode == "remote_only":
        spec = rec.get("install_spec") if isinstance(rec.get("install_spec"), dict) else {}
        url = str(spec.get("url") or "").strip()
        if not url:
            raise ValueError("missing_remote_url")
        tools_result = _remote_mcp_call(url, "tools/list", {}, timeout_s=12.0)
        tools = _normalize_discovered_tools(capability_id, tools_result.get("tools"))
        if not tools:
            raise RuntimeError("remote tools/list returned no tools")
        return {
            "state": "installed",
            "installer": "remote_http",
            "install_mode": "remote_only",
            "invoker": {"type": "remote_http", "url": url},
            "tools": tools,
            "last_error": "",
            "updated_at": utc_now_iso(),
        }

    if install_mode == "package":
        commands, reason = _package_stdio_command_candidates(rec)
        if commands:
            try:
                return _install_via_stdio_commands(
                    rec,
                    capability_id=capability_id,
                    commands=commands,
                    install_mode_label="package",
                )
            except Exception as e:
                fallback_commands, fallback_reason = _package_fallback_command_candidates(rec)
                if fallback_commands:
                    return _install_via_stdio_commands(
                        rec,
                        capability_id=capability_id,
                        commands=fallback_commands,
                        install_mode_label="command",
                        base_env_override=_command_base_env_override(rec),
                        fallback_from_mode="package",
                        fallback_reason=str(e)[:280],
                    )
                raise
        fallback_commands, fallback_reason = _package_fallback_command_candidates(rec)
        if fallback_commands:
            return _install_via_stdio_commands(
                rec,
                capability_id=capability_id,
                commands=fallback_commands,
                install_mode_label="command",
                base_env_override=_command_base_env_override(rec),
                fallback_from_mode="package",
                fallback_reason=reason or fallback_reason,
            )
        raise ValueError(reason or "unsupported_runtime_hint")

    if install_mode == "command":
        commands, reason = _command_stdio_command_candidates(rec)
        if not commands:
            raise ValueError(reason or "missing_command_candidate")
        return _install_via_stdio_commands(
            rec,
            capability_id=capability_id,
            commands=commands,
            install_mode_label="command",
            base_env_override=_command_base_env_override(rec),
        )

    raise ValueError(f"unsupported_install_mode:{install_mode or 'unknown'}")


def _invoke_installed_external_tool(
    install: Dict[str, Any],
    *,
    real_tool_name: str,
    arguments: Dict[str, Any],
) -> Dict[str, Any]:
    invoker = install.get("invoker") if isinstance(install.get("invoker"), dict) else {}
    invoker_type = str(invoker.get("type") or "").strip()
    if invoker_type == "remote_http":
        url = str(invoker.get("url") or "").strip()
        if not url:
            raise ValueError("missing_remote_url")
        return _remote_mcp_call(
            url,
            "tools/call",
            {"name": real_tool_name, "arguments": arguments if isinstance(arguments, dict) else {}},
            timeout_s=30.0,
        )
    if invoker_type in {"npm_stdio", "package_stdio", "command_stdio"}:
        command = invoker.get("command")
        cmd = [str(x) for x in command] if isinstance(command, list) else []
        if not cmd:
            raise ValueError("missing_package_command")
        env_override = invoker.get("env") if isinstance(invoker.get("env"), dict) else {}
        requests = [
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {"name": "cccc-capability-runtime", "version": "1.0"},
                },
            },
            {
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {
                    "name": real_tool_name,
                    "arguments": arguments if isinstance(arguments, dict) else {},
                },
            },
        ]
        call_timeout_s = float(max(5, min(_env_int("CCCC_CAPABILITY_PACKAGE_CALL_TIMEOUT_SECONDS", 45), 180)))
        if env_override:
            responses = _pkg()._stdio_mcp_roundtrip(
                cmd,
                requests,
                timeout_s=call_timeout_s,
                env_override=env_override,
            )
        else:
            responses = _pkg()._stdio_mcp_roundtrip(cmd, requests, timeout_s=call_timeout_s)
        return _extract_jsonrpc_result(responses, req_id=2, operation="tools/call")
    raise ValueError(f"unsupported_invoker:{invoker_type or 'unknown'}")


def _invoke_installed_external_tool_with_aliases(
    install: Dict[str, Any],
    *,
    requested_tool_name: str,
    arguments: Dict[str, Any],
) -> Tuple[Dict[str, Any], str]:
    names = _tool_name_aliases(requested_tool_name)
    if not names:
        token = str(requested_tool_name or "").strip()
        if not token:
            raise ValueError("missing_tool_name")
        names = [token]
    last_unknown_error: Optional[Exception] = None
    for name in names:
        try:
            return (
                _pkg()._invoke_installed_external_tool(
                    install,
                    real_tool_name=name,
                    arguments=arguments,
                ),
                name,
            )
        except Exception as e:
            if _is_unknown_tool_error_message(str(e)):
                last_unknown_error = e
                continue
            raise
    if isinstance(last_unknown_error, Exception):
        raise last_unknown_error
    raise RuntimeError(f"unknown tool: {requested_tool_name}")
