from __future__ import annotations

import os
import stat


def test_web_bootstrap_token_is_host_local_and_one_time(tmp_path) -> None:
    from cccc.kernel.access_tokens import create_access_token
    from cccc.kernel.web_bootstrap import (
        consume_web_bootstrap_token,
        ensure_web_bootstrap_token,
    )

    path = ensure_web_bootstrap_token(tmp_path)
    assert path is not None
    secret = path.read_text(encoding="utf-8").strip()
    assert secret.startswith("boot_")
    if os.name != "nt":
        assert stat.S_IMODE(path.stat().st_mode) == 0o600
    assert not consume_web_bootstrap_token("wrong", tmp_path)
    assert consume_web_bootstrap_token(secret, tmp_path)
    assert not path.exists()

    create_access_token("admin", is_admin=True, home=tmp_path)
    assert ensure_web_bootstrap_token(tmp_path) is None
    assert not path.exists()
