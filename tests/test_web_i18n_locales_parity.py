import json
import re
import unittest
from pathlib import Path
from typing import Any


def _leaf_key_paths(value: Any, prefix: str = "") -> set[str]:
    if isinstance(value, dict):
        paths: set[str] = set()
        for key, child in value.items():
            child_prefix = f"{prefix}.{key}" if prefix else key
            paths.update(_leaf_key_paths(child, child_prefix))
        return paths
    return {prefix}


def _leaf_string_values(value: Any, prefix: str = "") -> dict[str, str]:
    if isinstance(value, dict):
        out: dict[str, str] = {}
        for key, child in value.items():
            child_prefix = f"{prefix}.{key}" if prefix else key
            out.update(_leaf_string_values(child, child_prefix))
        return out
    if isinstance(value, str):
        return {prefix: value}
    return {}


_PLACEHOLDER_RE = re.compile(r"\{\{\s*[^}]+\s*\}\}|</?\d+>")


class TestWebI18nLocalesParity(unittest.TestCase):
    def test_locales_keep_same_namespaces_and_key_paths(self) -> None:
        repo_root = Path(__file__).resolve().parents[1]
        locales_root = repo_root / "web" / "src" / "i18n" / "locales"

        locales = ("en", "zh", "ja")
        baseline_files = sorted(p.name for p in (locales_root / "en").glob("*.json"))
        self.assertGreater(len(baseline_files), 0, msg="Expected at least one English locale namespace file")

        for locale in locales:
            locale_files = sorted(p.name for p in (locales_root / locale).glob("*.json"))
            self.assertEqual(
                locale_files,
                baseline_files,
                msg=f"Locale '{locale}' namespace files differ from English baseline",
            )

        for namespace_file in baseline_files:
            baseline_data = json.loads((locales_root / "en" / namespace_file).read_text(encoding="utf-8"))
            baseline_paths = _leaf_key_paths(baseline_data)

            for locale in locales[1:]:
                locale_data = json.loads((locales_root / locale / namespace_file).read_text(encoding="utf-8"))
                locale_paths = _leaf_key_paths(locale_data)
                missing_paths = sorted(baseline_paths - locale_paths)
                extra_paths = sorted(locale_paths - baseline_paths)

                self.assertEqual(
                    missing_paths,
                    [],
                    msg=f"Locale '{locale}' missing keys in {namespace_file}: {missing_paths}",
                )
                self.assertEqual(
                    extra_paths,
                    [],
                    msg=f"Locale '{locale}' has extra keys in {namespace_file}: {extra_paths}",
                )

    def test_locales_keep_same_placeholder_tokens(self) -> None:
        repo_root = Path(__file__).resolve().parents[1]
        locales_root = repo_root / "web" / "src" / "i18n" / "locales"
        locales = ("en", "zh", "ja")
        namespace_files = sorted(p.name for p in (locales_root / "en").glob("*.json"))

        for namespace_file in namespace_files:
            baseline_data = json.loads((locales_root / "en" / namespace_file).read_text(encoding="utf-8"))
            baseline_values = _leaf_string_values(baseline_data)

            for locale in locales[1:]:
                locale_data = json.loads((locales_root / locale / namespace_file).read_text(encoding="utf-8"))
                locale_values = _leaf_string_values(locale_data)
                for key_path, baseline_text in baseline_values.items():
                    baseline_tokens = sorted(_PLACEHOLDER_RE.findall(baseline_text))
                    locale_tokens = sorted(_PLACEHOLDER_RE.findall(locale_values.get(key_path, "")))
                    self.assertEqual(
                        locale_tokens,
                        baseline_tokens,
                        msg=(
                            f"Locale '{locale}' placeholder mismatch in {namespace_file}:{key_path}. "
                            f"Expected {baseline_tokens}, got {locale_tokens}"
                        ),
                    )


if __name__ == "__main__":
    unittest.main()
