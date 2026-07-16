from __future__ import annotations

import errno
import socket
import sys


def _probe_connect_hosts(host: str) -> list[str]:
    raw = str(host or "").strip()
    normalized = raw.lower()
    if normalized in {"", "0.0.0.0"}:
        return ["127.0.0.1"]
    if normalized in {"::", "[::]"}:
        return ["::1"]
    if normalized == "localhost":
        return ["127.0.0.1", "::1"]
    return [raw]


def _listener_detected(*, host: str, port: int) -> bool:
    seen: set[tuple[int, int, int, tuple[object, ...]]] = set()
    for probe_host in _probe_connect_hosts(host):
        try:
            infos = socket.getaddrinfo(str(probe_host), int(port), type=socket.SOCK_STREAM)
        except OSError:
            continue
        for family, socktype, proto, _canonname, sockaddr in infos:
            addr_key = (int(family), int(socktype), int(proto), tuple(sockaddr))
            if addr_key in seen:
                continue
            seen.add(addr_key)
            sock = socket.socket(family, socktype, proto)
            try:
                try:
                    sock.settimeout(0.2)
                except OSError:
                    pass
                if int(sock.connect_ex(sockaddr)) == 0:
                    return True
            except OSError:
                continue
            finally:
                try:
                    sock.close()
                except Exception:
                    pass
    return False


def _switch_port_examples(port: int = 9000) -> str:
    cli_cmd = f"`cccc --port {int(port)}`"
    if sys.platform.startswith("win"):
        env_cmd = f"`$env:CCCC_WEB_PORT={int(port)}; cccc`"
    else:
        env_cmd = f"`CCCC_WEB_PORT={int(port)} cccc`"
    return f"Example: {cli_cmd} or {env_cmd}."


def _format_bind_target(host: str, port: int) -> str:
    raw_host = str(host or "").strip() or "0.0.0.0"
    if ":" in raw_host and not (raw_host.startswith("[") and raw_host.endswith("]")):
        raw_host = f"[{raw_host}]"
    return f"{raw_host}:{int(port)}"


def _is_windows_access_denied(exc: OSError) -> bool:
    if not sys.platform.startswith("win"):
        return False
    if getattr(exc, "winerror", None) == 10013:
        return True
    return int(getattr(exc, "errno", 0) or 0) in {errno.EACCES, errno.EPERM}


def _is_addr_in_use(exc: OSError) -> bool:
    if int(getattr(exc, "errno", 0) or 0) == errno.EADDRINUSE:
        return True
    return int(getattr(exc, "winerror", 0) or 0) == 10048


def describe_bind_error(*, host: str, port: int, exc: OSError) -> str:
    target = _format_bind_target(host, port)
    base = f"Web port {int(port)} is unavailable on {target}."
    if _is_addr_in_use(exc):
        if sys.platform.startswith("win") and not _listener_detected(host=host, port=port):
            return (
                f"{base} Windows reported the port as unavailable, but no TCP listener was detected. "
                "On Windows this commonly means the port falls inside an excluded TCP port range "
                "reserved by Hyper-V, WSL, WinNAT, or HNS, even when no process is listening. "
                "Check with `netsh interface ipv4 show excludedportrange protocol=tcp`, then restart "
                f"CCCC with a different port. {_switch_port_examples()}"
            )
        return (
            f"{base} Another process is already using that port. "
            "Stop the existing process, or restart CCCC on a different port. "
            f"{_switch_port_examples()}"
        )
    if _is_windows_access_denied(exc):
        return (
            f"{base} Windows denied the bind. On Windows this commonly means the port falls "
            "inside an excluded TCP port range reserved by Hyper-V, WSL, WinNAT, or HNS, "
            "even when no process is listening. Check with "
            "`netsh interface ipv4 show excludedportrange protocol=tcp`, then restart CCCC "
            f"with a different port. {_switch_port_examples()}"
        )
    detail = str(exc).strip()
    if detail:
        return f"{base} Bind failed: {detail}"
    return f"{base} Bind failed."


def ensure_tcp_port_bindable(*, host: str, port: int) -> None:
    host_s = str(host or "").strip() or "0.0.0.0"
    port_n = int(port)
    flags = socket.AI_PASSIVE if host_s in {"0.0.0.0", "::", ""} else 0
    infos = socket.getaddrinfo(host_s, port_n, type=socket.SOCK_STREAM, flags=flags)
    last_error: OSError | None = None
    seen: set[tuple[int, int, int, tuple[object, ...]]] = set()
    for family, socktype, proto, _canonname, sockaddr in infos:
        addr_key = (int(family), int(socktype), int(proto), tuple(sockaddr))
        if addr_key in seen:
            continue
        seen.add(addr_key)
        sock = socket.socket(family, socktype, proto)
        try:
            if not sys.platform.startswith("win"):
                try:
                    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
                except OSError:
                    pass
            if sys.platform.startswith("win") and hasattr(socket, "SO_EXCLUSIVEADDRUSE"):
                try:
                    sock.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
                except OSError:
                    pass
            else:
                # Allow binding to a port in TIME_WAIT state (e.g. after Ctrl+C).
                # The actual web server (uvicorn) sets SO_REUSEADDR by default,
                # so the preflight check should match that behavior to avoid
                # false-positive "port unavailable" errors.
                sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            sock.bind(sockaddr)
            return
        except OSError as exc:
            last_error = exc
        finally:
            try:
                sock.close()
            except Exception:
                pass
    if last_error is not None:
        raise RuntimeError(describe_bind_error(host=host_s, port=port_n, exc=last_error)) from last_error
    raise RuntimeError(f"Web port {port_n} is unavailable on {_format_bind_target(host_s, port_n)}.")
