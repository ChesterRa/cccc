from __future__ import annotations

import io
import os
import tarfile
import tempfile
import unittest
from pathlib import Path

from cccc.kernel.cloudflared import (
    CloudflaredError,
    PINNED_VERSION,
    artifact_for,
    extract_binary,
    inspect,
    install_from_bytes,
    sha256_bytes,
)


class TestCloudflaredPin(unittest.TestCase):
    def setUp(self) -> None:
        self._old = os.environ.get("CCCC_HOME")
        self._tmp = tempfile.TemporaryDirectory()
        os.environ["CCCC_HOME"] = self._tmp.name
        self.home = Path(self._tmp.name)

    def tearDown(self) -> None:
        self._tmp.cleanup()
        if self._old is None:
            os.environ.pop("CCCC_HOME", None)
        else:
            os.environ["CCCC_HOME"] = self._old

    def test_rejects_sha256_mismatch(self) -> None:
        with self.assertRaises(CloudflaredError) as ctx:
            install_from_bytes(
                b"not-cloudflared", home=self.home, system="linux", machine="x86_64"
            )
        self.assertEqual(ctx.exception.code, "membership_subprocess")
        self.assertIn("sha256 mismatch", ctx.exception.message)

    def test_installs_verified_payload_under_home(self) -> None:
        payload = b"#!/bin/sh\necho cloudflared-fixture\n"
        state = install_from_bytes(
            payload,
            home=self.home,
            system="linux",
            machine="x86_64",
            expected_sha256=sha256_bytes(payload),
        )
        self.assertTrue(state["installed"])
        self.assertEqual(state["version"], PINNED_VERSION)
        installed = Path(state["path"])
        self.assertTrue(str(installed).startswith(str(self.home)))
        self.assertEqual(installed.read_bytes(), payload)
        self.assertTrue(os.access(installed, os.X_OK))

    def test_refuses_replace_without_explicit_upgrade(self) -> None:
        first = b"first-binary"
        install_from_bytes(
            first,
            home=self.home,
            system="linux",
            machine="x86_64",
            expected_sha256=sha256_bytes(first),
        )
        second = b"second-binary"
        with self.assertRaises(CloudflaredError) as ctx:
            install_from_bytes(
                second,
                home=self.home,
                system="linux",
                machine="x86_64",
                expected_sha256=sha256_bytes(second),
            )
        self.assertIn("cccc reach install", ctx.exception.message)
        self.assertEqual(
            (self.home / "libexec" / "cloudflared" / "cloudflared").read_bytes(), first
        )

    def test_upgrade_replaces_after_hash_check(self) -> None:
        first = b"first-binary"
        install_from_bytes(
            first,
            home=self.home,
            system="linux",
            machine="x86_64",
            expected_sha256=sha256_bytes(first),
        )
        second = b"second-binary"
        install_from_bytes(
            second,
            home=self.home,
            upgrade=True,
            system="linux",
            machine="x86_64",
            expected_sha256=sha256_bytes(second),
        )
        self.assertEqual(
            (self.home / "libexec" / "cloudflared" / "cloudflared").read_bytes(), second
        )

    def test_extracts_cloudflared_from_tgz(self) -> None:
        raw = b"darwin-binary"
        buf = io.BytesIO()
        with tarfile.open(fileobj=buf, mode="w:gz") as archive:
            info = tarfile.TarInfo(name="dist/cloudflared")
            info.size = len(raw)
            archive.addfile(info, io.BytesIO(raw))
        payload = buf.getvalue()
        self.assertEqual(extract_binary("cloudflared-darwin-arm64.tgz", payload), raw)
        state = install_from_bytes(
            payload,
            home=self.home,
            system="darwin",
            machine="arm64",
            expected_sha256=sha256_bytes(payload),
            expected_binary_sha256=sha256_bytes(raw),
        )
        self.assertEqual(Path(state["path"]).read_bytes(), raw)

    def test_darwin_accepts_rust_and_python_arm_architecture_names(self) -> None:
        self.assertEqual(
            artifact_for("darwin", "aarch64"), artifact_for("darwin", "arm64")
        )

    def test_inspect_missing_binary(self) -> None:
        state = inspect(self.home)
        self.assertTrue(state["supported"])
        self.assertFalse(state["installed"])
        self.assertFalse(state["matches_pin"])


if __name__ == "__main__":
    unittest.main()
