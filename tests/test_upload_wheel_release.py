import hashlib
import io
import json
from pathlib import Path
from subprocess import CompletedProcess

from scripts import upload_wheel_release


def test_existing_hashes_reads_registry_digests(monkeypatch) -> None:
    payload = {
        "releases": {
            "0.4.35rc1": [
                {
                    "filename": "cccc_pair-0.4.36-py3-none-manylinux_2_28_x86_64.whl",
                    "digests": {"sha256": "ABC123"},
                }
            ]
        }
    }
    observed = 0

    def urlopen(_request, *, timeout: int):
        nonlocal observed
        observed += 1
        assert timeout == 30
        return io.BytesIO(json.dumps(payload).encode())

    monkeypatch.setattr(upload_wheel_release.urllib.request, "urlopen", urlopen)

    assert upload_wheel_release.existing_hashes("testpypi") == {
        "cccc_pair-0.4.36-py3-none-manylinux_2_28_x86_64.whl": "abc123"
    }
    assert observed == 1


def test_existing_release_set_is_an_idempotent_success(monkeypatch, tmp_path: Path) -> None:
    wheel = tmp_path / "cccc_pair-0.4.36-py3-none-manylinux_2_28_x86_64.whl"
    wheel.write_bytes(b"same build")
    monkeypatch.setattr(
        upload_wheel_release,
        "existing_hashes",
        lambda _repository: {wheel.name: hashlib.sha256(wheel.read_bytes()).hexdigest()},
    )
    monkeypatch.setattr(
        upload_wheel_release.subprocess,
        "run",
        lambda *_args, **_kwargs: (_ for _ in ()).throw(AssertionError("upload must be skipped")),
    )

    assert upload_wheel_release.upload_missing("testpypi", [wheel]) == 0


def test_only_missing_distributions_are_uploaded(monkeypatch, tmp_path: Path) -> None:
    existing = tmp_path / "cccc_pair-0.4.36-py3-none-manylinux_2_28_x86_64.whl"
    missing = tmp_path / "cccc_pair-0.4.36-py3-none-win_amd64.whl"
    existing.write_bytes(b"existing build")
    missing.write_bytes(b"missing build")
    observed: list[list[str]] = []
    snapshots = 0

    def existing_hashes(_repository: str) -> dict[str, str]:
        nonlocal snapshots
        snapshots += 1
        return {existing.name: hashlib.sha256(existing.read_bytes()).hexdigest()}

    monkeypatch.setattr(
        upload_wheel_release,
        "existing_hashes",
        existing_hashes,
    )

    def run(command: list[str], **_kwargs) -> CompletedProcess:
        observed.append(command)
        return CompletedProcess(command, 0)

    monkeypatch.setattr(upload_wheel_release.subprocess, "run", run)

    assert upload_wheel_release.upload_missing("testpypi", [existing, missing]) == 0
    assert snapshots == 1
    assert len(observed) == 1
    assert "--skip-existing" not in observed[0]
    assert str(existing) not in observed[0]
    assert str(missing) in observed[0]


def test_same_filename_with_different_hash_fails_without_upload(monkeypatch, tmp_path: Path) -> None:
    wheel = tmp_path / "cccc_pair-0.4.36-py3-none-manylinux_2_28_x86_64.whl"
    wheel.write_bytes(b"rebuilt artifact")
    monkeypatch.setattr(
        upload_wheel_release,
        "existing_hashes",
        lambda _repository: {wheel.name: hashlib.sha256(b"published artifact").hexdigest()},
    )
    monkeypatch.setattr(
        upload_wheel_release.subprocess,
        "run",
        lambda *_args, **_kwargs: (_ for _ in ()).throw(AssertionError("upload must be blocked")),
    )

    assert upload_wheel_release.upload_missing("testpypi", [wheel]) == 1
