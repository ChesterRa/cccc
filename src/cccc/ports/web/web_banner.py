from __future__ import annotations

import socket
import sys
from typing import TextIO


_WILDCARD_HOSTS = {"0.0.0.0", "::", "[::]"}


def display_local_host(host: str) -> str:
    value = str(host or "").strip()
    if value in _WILDCARD_HOSTS:
        return "localhost"
    return value or "localhost"


def http_host_literal(host: str) -> str:
    value = display_local_host(host)
    if value != "localhost" and ":" in value and not (
        value.startswith("[") and value.endswith("]")
    ):
        return f"[{value}]"
    return value


def detect_lan_ipv4() -> str:
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
            sock.settimeout(0.1)
            sock.connect(("8.8.8.8", 80))
            return str(sock.getsockname()[0] or "").strip()
    except Exception:
        return ""


def urls(host: str, port: int, lan_ip: str = "") -> tuple[str, str | None]:
    value = str(host or "").strip()
    local_url = f"http://{http_host_literal(value)}:{int(port)}"
    network_url = None
    if value in _WILDCARD_HOSTS:
        address = str(lan_ip or "").strip()
        if address and address != "127.0.0.1":
            network_url = f"http://{address}:{int(port)}"
    return local_url, network_url


def print_web_banner(
    host: str,
    port: int,
    *,
    implementation: str,
    stream: TextIO | None = None,
) -> None:
    output = stream if stream is not None else sys.stderr
    lan_ip = detect_lan_ipv4() if str(host or "").strip() in _WILDCARD_HOSTS else ""
    local_url, network_url = urls(host, port, lan_ip)
    print(f"[cccc] Implementation: {implementation}", file=output)
    print("[cccc] Starting web server...", file=output)
    print(f"[cccc]   Local:   {local_url}", file=output)
    if network_url is not None:
        print(f"[cccc]   Network: {network_url}", file=output)
