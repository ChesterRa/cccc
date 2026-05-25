from cccc.daemon.codex_config_ops import inject_codex_openai_base_url_config


def test_inject_codex_openai_base_url_config_from_env() -> None:
    cmd = inject_codex_openai_base_url_config(
        ["codex", "--search"],
        {"OPENAI_BASE_URL": "https://proxy.example/v1"},
    )

    assert cmd == [
        "codex",
        "-c",
        'openai_base_url="https://proxy.example/v1"',
        "--search",
    ]


def test_inject_codex_openai_base_url_config_preserves_explicit_provider_config() -> None:
    cmd = inject_codex_openai_base_url_config(
        ["codex", "-c", 'model_providers.proxy.base_url="https://explicit.example/v1"', "--search"],
        {"OPENAI_BASE_URL": "https://env.example/v1"},
    )

    assert cmd == ["codex", "-c", 'model_providers.proxy.base_url="https://explicit.example/v1"', "--search"]


def test_inject_codex_openai_base_url_config_ignores_non_codex_command() -> None:
    cmd = inject_codex_openai_base_url_config(
        ["custom-codex-wrapper", "--search"],
        {"OPENAI_BASE_URL": "https://proxy.example/v1"},
    )

    assert cmd == ["custom-codex-wrapper", "--search"]
