"""Pinned cloudflared helper: verify hash, install under CCCC_HOME, upgrade explicitly."""

from __future__ import annotations

import hashlib
import io
import os
import platform
import stat
import tarfile
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any, Callable, Dict, Optional

from ..paths import ensure_home
from ..util.fs import atomic_write_json
from ..util.time import utc_now_iso

PINNED_VERSION = "2026.8.2"
MAX_DOWNLOAD_BYTES = 80 * 1024 * 1024
_GITHUB_BASE = (
    f"https://github.com/cloudflare/cloudflared/releases/download/{PINNED_VERSION}"
)

# Official release checksums for 2026.8.2. Windows is same-contract, allowed to lag.
_ARTIFACTS: Dict[tuple[str, str], tuple[str, str, str]] = {
    ("linux", "x86_64"): (
        "cloudflared-linux-amd64",
        "fcfb02b575a52ca1af2e3267af4e1517bcdeb30ac48c834c69abaed3c0576ad2",
        "fcfb02b575a52ca1af2e3267af4e1517bcdeb30ac48c834c69abaed3c0576ad2",
    ),
    ("linux", "amd64"): (
        "cloudflared-linux-amd64",
        "fcfb02b575a52ca1af2e3267af4e1517bcdeb30ac48c834c69abaed3c0576ad2",
        "fcfb02b575a52ca1af2e3267af4e1517bcdeb30ac48c834c69abaed3c0576ad2",
    ),
    ("linux", "aarch64"): (
        "cloudflared-linux-arm64",
        "7747d94570fb390cf47dcb4f9555c193c6355cda9793f0d878d9049e5d6a7790",
        "7747d94570fb390cf47dcb4f9555c193c6355cda9793f0d878d9049e5d6a7790",
    ),
    ("linux", "arm64"): (
        "cloudflared-linux-arm64",
        "7747d94570fb390cf47dcb4f9555c193c6355cda9793f0d878d9049e5d6a7790",
        "7747d94570fb390cf47dcb4f9555c193c6355cda9793f0d878d9049e5d6a7790",
    ),
    ("darwin", "x86_64"): (
        "cloudflared-darwin-amd64.tgz",
        "f1727723c586500e2092368ae21871b3df7ddfd2cb097f22d81bee4a9c458bb4",
        "b0f770e1e0b281399a57219b840fd8eef1cc25387a404124248157ea2073727a",
    ),
    ("darwin", "amd64"): (
        "cloudflared-darwin-amd64.tgz",
        "f1727723c586500e2092368ae21871b3df7ddfd2cb097f22d81bee4a9c458bb4",
        "b0f770e1e0b281399a57219b840fd8eef1cc25387a404124248157ea2073727a",
    ),
    ("darwin", "arm64"): (
        "cloudflared-darwin-arm64.tgz",
        "9042c2c5d8b2de78e60f313d5fb31b6c5c1cebde787a3caf1f2c9588084ac442",
        "b61054d3d6326ea558cb49826eebf5676e0d0a36d51b546975096ca3e0e3c89d",
    ),
    ("darwin", "aarch64"): (
        "cloudflared-darwin-arm64.tgz",
        "9042c2c5d8b2de78e60f313d5fb31b6c5c1cebde787a3caf1f2c9588084ac442",
        "b61054d3d6326ea558cb49826eebf5676e0d0a36d51b546975096ca3e0e3c89d",
    ),
}


class CloudflaredError(Exception):
    def __init__(self, code: str, message: str) -> None:
        super().__init__(message)
        self.code = code
        self.message = message


def install_dir(home: Optional[Path] = None) -> Path:
    base = Path(home) if home is not None else ensure_home()
    return base / "libexec" / "cloudflared"


def binary_path(home: Optional[Path] = None) -> Path:
    return install_dir(home) / "cloudflared"


def meta_path(home: Optional[Path] = None) -> Path:
    return install_dir(home) / "install.json"


def current_platform() -> tuple[str, str]:
    system = platform.system().strip().lower()
    machine = platform.machine().strip().lower()
    return system, machine


def artifact_for(system: str, machine: str) -> tuple[str, str, str]:
    spec = _ARTIFACTS.get((system, machine))
    if spec is None:
        raise CloudflaredError(
            "membership_subprocess",
            f"cloudflared is not provided for {system}/{machine} in this release",
        )
    return spec


def download_url(artifact: str) -> str:
    base = (
        str(os.environ.get("CCCC_CLOUDFLARED_BASE_URL") or _GITHUB_BASE)
        .strip()
        .rstrip("/")
    )
    return f"{base}/{artifact}"


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while True:
            chunk = handle.read(1024 * 1024)
            if not chunk:
                break
            digest.update(chunk)
    return digest.hexdigest()


def extract_binary(artifact: str, payload: bytes) -> bytes:
    if not artifact.endswith(".tgz"):
        return payload
    with tarfile.open(fileobj=io.BytesIO(payload), mode="r:gz") as archive:
        for member in archive.getmembers():
            name = Path(member.name).name
            if member.isfile() and name == "cloudflared":
                extracted = archive.extractfile(member)
                if extracted is None:
                    break
                return extracted.read()
    raise CloudflaredError(
        "membership_subprocess",
        "cloudflared archive did not contain a cloudflared binary",
    )


def inspect(home: Optional[Path] = None) -> Dict[str, Any]:
    path = binary_path(home)
    meta_file = meta_path(home)
    meta: Dict[str, Any] = {}
    if meta_file.is_file():
        try:
            import json

            raw = json.loads(meta_file.read_text(encoding="utf-8"))
            if isinstance(raw, dict):
                meta = raw
        except (OSError, ValueError):
            meta = {}
    installed = path.is_file()
    digest = sha256_file(path) if installed else None
    version = str(meta.get("version") or "").strip() or None
    expected = None
    try:
        _, _, expected = artifact_for(*current_platform())
    except CloudflaredError:
        expected = None
    matches_pin = bool(
        installed
        and version == PINNED_VERSION
        and digest
        and expected
        and digest == expected
    )
    return {
        "supported": expected is not None,
        "installed": installed,
        "path": str(path) if installed else None,
        "version": version,
        "sha256": digest,
        "pinned_version": PINNED_VERSION,
        "matches_pin": matches_pin,
    }


def install_from_bytes(
    payload: bytes,
    *,
    home: Optional[Path] = None,
    upgrade: bool = False,
    system: Optional[str] = None,
    machine: Optional[str] = None,
    expected_sha256: Optional[str] = None,
    expected_binary_sha256: Optional[str] = None,
) -> Dict[str, Any]:
    sys_name, mach = current_platform()
    if system:
        sys_name = system
    if machine:
        mach = machine
    artifact, pinned_artifact, pinned_binary = artifact_for(sys_name, mach)
    expected_artifact = (expected_sha256 or pinned_artifact).strip().lower()
    artifact_digest = sha256_bytes(payload)
    if artifact_digest != expected_artifact:
        raise CloudflaredError(
            "membership_subprocess",
            f"cloudflared artifact sha256 mismatch (got {artifact_digest}, expected {expected_artifact})",
        )
    state = inspect(home)
    if state["installed"] and not state["matches_pin"] and not upgrade:
        raise CloudflaredError(
            "membership_subprocess",
            "installed cloudflared is not the pinned release; run `cccc reach install` to upgrade",
        )
    binary = extract_binary(artifact, payload)
    binary_override = expected_binary_sha256
    if (
        binary_override is None
        and expected_sha256 is not None
        and not artifact.endswith(".tgz")
    ):
        binary_override = expected_artifact
    expected_binary = (binary_override or pinned_binary).strip().lower()
    binary_digest = sha256_bytes(binary)
    if binary_digest != expected_binary:
        raise CloudflaredError(
            "membership_subprocess",
            f"cloudflared binary sha256 mismatch (got {binary_digest}, expected {expected_binary})",
        )
    dest_dir = install_dir(home)
    dest_dir.mkdir(parents=True, exist_ok=True)
    dest = binary_path(home)
    tmp = dest.with_name(dest.name + ".tmp")
    tmp.write_bytes(binary)
    tmp.chmod(stat.S_IRUSR | stat.S_IWUSR | stat.S_IXUSR)
    tmp.replace(dest)
    atomic_write_json(
        meta_path(home),
        {
            "version": PINNED_VERSION,
            "artifact": artifact,
            "artifact_sha256": expected_artifact,
            "sha256": expected_binary,
            "installed_at": utc_now_iso(),
        },
    )
    return inspect(home)


Downloader = Callable[[str], bytes]


def _default_download(url: str) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": "cccc-cloudflared"})
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            data = response.read(MAX_DOWNLOAD_BYTES + 1)
    except urllib.error.URLError as exc:
        raise CloudflaredError(
            "membership_network", f"failed to download cloudflared: {exc}"
        ) from exc
    if len(data) > MAX_DOWNLOAD_BYTES:
        raise CloudflaredError(
            "membership_subprocess", "cloudflared download exceeded size limit"
        )
    return data


def ensure(
    *,
    home: Optional[Path] = None,
    upgrade: bool = False,
    download: Optional[Downloader] = None,
    system: Optional[str] = None,
    machine: Optional[str] = None,
) -> Dict[str, Any]:
    state = inspect(home)
    if state["matches_pin"]:
        return state
    if state["installed"] and not upgrade:
        raise CloudflaredError(
            "membership_subprocess",
            "installed cloudflared is not the pinned release; run `cccc reach install` to upgrade",
        )
    sys_name, mach = current_platform()
    artifact, _artifact_sha256, _binary_sha256 = artifact_for(
        system or sys_name, machine or mach
    )
    fetcher = download or _default_download
    payload = fetcher(download_url(artifact))
    return install_from_bytes(
        payload,
        home=home,
        upgrade=True,
        system=system or sys_name,
        machine=machine or mach,
    )
