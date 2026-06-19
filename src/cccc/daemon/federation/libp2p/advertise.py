"""Resolve libp2p multiaddrs safe to advertise to remote CCCC peers."""

from __future__ import annotations

import ipaddress
import os
import socket
from pathlib import Path
from typing import Any, Optional
from urllib.parse import urlparse

from ....kernel.settings import get_remote_access_settings, resolve_remote_access_web_binding
from .sidecar import parse_direct_multiaddr


def local_advertised_multiaddrs(*, home: Optional[Path] = None) -> tuple[str, ...]:
    """Return running sidecar addrs adjusted for cross-machine announcement.

    The sidecar may listen on loopback by default. When the operator has
    configured a concrete advertised host, or when Web remote access already has
    a concrete IPv4 binding, publish that host instead of leaking 127.0.0.1 to
    the peer. We do not guess from 0.0.0.0 or hostnames.
    """
    try:
        from .supervisor import read_sidecar_status

        status = read_sidecar_status(home=home)
    except Exception:
        return ()
    if str(status.get("status") or "").strip() != "running":
        return ()
    return rewrite_advertised_multiaddrs(status.get("multiaddrs"), advertise_host=_advertise_host())


def default_listen_multiaddr() -> str:
    configured = str(os.environ.get("CCCC_LIBP2P_LISTEN_MULTIADDR") or "").strip()
    if configured:
        return configured
    if default_advertise_host():
        return "/ip4/0.0.0.0/tcp/0"
    return "/ip4/127.0.0.1/tcp/0"


def default_advertise_host() -> str:
    return _explicit_advertise_host() or _derived_advertise_host()


def rewrite_advertised_multiaddrs(raw_addrs: Any, *, advertise_host: str = "") -> tuple[str, ...]:
    addrs = [str(addr or "").strip() for addr in (raw_addrs or []) if str(addr or "").strip()] if isinstance(raw_addrs, list) else []
    if not addrs:
        return ()
    host = _concrete_advertise_ipv4(advertise_host)
    out: list[str] = []
    seen: set[str] = set()
    for addr in addrs:
        rewritten = _rewrite_one(addr, host)
        if not rewritten or rewritten in seen:
            continue
        seen.add(rewritten)
        out.append(rewritten)
    return tuple(out)


def _rewrite_one(addr: str, advertise_host: str) -> str:
    try:
        parsed = parse_direct_multiaddr(addr)
    except Exception:
        return ""
    current_ip = ipaddress.IPv4Address(parsed.host)
    host = advertise_host
    if not host:
        if current_ip.is_loopback:
            return ""
        host = parsed.host
    return f"/ip4/{host}/tcp/{parsed.port}/p2p/{parsed.peer_id}"


def _advertise_host() -> str:
    return default_advertise_host()


def _explicit_advertise_host() -> str:
    return _concrete_advertise_ipv4(os.environ.get("CCCC_LIBP2P_ADVERTISE_HOST"))


def _derived_advertise_host() -> str:
    if not _remote_access_enabled():
        return ""
    binding = resolve_remote_access_web_binding()
    public_host = _host_from_public_url(str(binding.get("web_public_url") or ""))
    public_ip = _concrete_advertise_ipv4(public_host)
    if public_ip:
        return public_ip
    host_ip = _concrete_advertise_ipv4(str(binding.get("web_host") or ""))
    if host_ip:
        return host_ip
    return _auto_detect_advertise_host()


def _remote_access_enabled() -> bool:
    settings = get_remote_access_settings()
    return bool(settings.get("enabled")) and str(settings.get("provider") or "") != "off"


def _auto_detect_advertise_host() -> str:
    for candidate in (_outbound_route_ipv4(), *_hostname_ipv4s()):
        host = _concrete_advertise_ipv4(candidate)
        if host:
            return host
    return ""


def _outbound_route_ipv4() -> str:
    sock: Optional[socket.socket] = None
    try:
        sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        sock.connect(("8.8.8.8", 80))
        return str(sock.getsockname()[0])
    except Exception:
        return ""
    finally:
        if sock is not None:
            try:
                sock.close()
            except Exception:
                pass


def _hostname_ipv4s() -> tuple[str, ...]:
    try:
        infos = socket.getaddrinfo(socket.gethostname(), None, socket.AF_INET, socket.SOCK_DGRAM)
    except Exception:
        return ()
    out: list[str] = []
    seen: set[str] = set()
    for info in infos:
        try:
            host = str(info[4][0])
        except Exception:
            continue
        if host in seen:
            continue
        seen.add(host)
        out.append(host)
    return tuple(out)


def _host_from_public_url(value: str) -> str:
    raw = str(value or "").strip()
    if not raw:
        return ""
    parsed = urlparse(raw if "://" in raw else f"http://{raw}")
    return str(parsed.hostname or "").strip()


def _concrete_advertise_ipv4(value: object) -> str:
    raw = str(value or "").strip()
    if not raw:
        return ""
    try:
        ip = ipaddress.IPv4Address(raw)
    except Exception:
        return ""
    if ip.is_loopback or ip.is_link_local or ip.is_unspecified or ip.is_multicast or ip.is_reserved:
        return ""
    return str(ip)
