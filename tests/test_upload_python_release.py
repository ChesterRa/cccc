from pathlib import Path
from subprocess import CompletedProcess

from scripts import upload_python_release


def test_existing_release_set_is_an_idempotent_success(monkeypatch, tmp_path: Path) -> None:
    wheel = tmp_path / "cccc_pair-0.4.35rc1-py3-none-any.whl"
    wheel.touch()
    monkeypatch.setattr(upload_python_release, "existing_filenames", lambda _repository: {wheel.name})
    monkeypatch.setattr(
        upload_python_release.subprocess,
        "run",
        lambda *_args, **_kwargs: (_ for _ in ()).throw(AssertionError("upload must be skipped")),
    )

    assert upload_python_release.upload_missing("testpypi", [wheel]) == 0


def test_only_missing_distributions_are_uploaded(monkeypatch, tmp_path: Path) -> None:
    existing = tmp_path / "cccc_pair-0.4.35rc1-py3-none-any.whl"
    missing = tmp_path / "cccc_pair-0.4.35rc1-cp314-win_amd64.whl"
    existing.touch()
    missing.touch()
    observed: list[list[str]] = []
    monkeypatch.setattr(
        upload_python_release,
        "existing_filenames",
        lambda _repository: {existing.name},
    )

    def run(command: list[str], **_kwargs) -> CompletedProcess:
        observed.append(command)
        return CompletedProcess(command, 0)

    monkeypatch.setattr(upload_python_release.subprocess, "run", run)

    assert upload_python_release.upload_missing("testpypi", [existing, missing]) == 0
    assert len(observed) == 1
    assert "--skip-existing" in observed[0]
    assert str(existing) not in observed[0]
    assert str(missing) in observed[0]
