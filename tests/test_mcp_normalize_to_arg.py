"""Tests for _normalize_to_arg in MCP server — handles JSON-encoded array strings."""

import unittest

from cccc.ports.mcp.server import _normalize_to_arg


class NormalizeToArgTest(unittest.TestCase):
    def test_plain_string(self) -> None:
        self.assertEqual(_normalize_to_arg("user"), ["user"])

    def test_list_of_strings(self) -> None:
        self.assertEqual(_normalize_to_arg(["user", "peer1"]), ["user", "peer1"])

    def test_json_encoded_array(self) -> None:
        """Agent passes to='[\"user\"]' instead of to=[\"user\"]."""
        self.assertEqual(_normalize_to_arg('["user"]'), ["user"])

    def test_json_encoded_array_multiple(self) -> None:
        self.assertEqual(_normalize_to_arg('["@all", "peer1"]'), ["@all", "peer1"])

    def test_none_returns_none(self) -> None:
        self.assertIsNone(_normalize_to_arg(None))

    def test_empty_string_returns_none(self) -> None:
        self.assertIsNone(_normalize_to_arg(""))

    def test_empty_list_returns_none(self) -> None:
        self.assertIsNone(_normalize_to_arg([]))

    def test_at_mention_string(self) -> None:
        self.assertEqual(_normalize_to_arg("@all"), ["@all"])

    def test_json_encoded_with_spaces(self) -> None:
        self.assertEqual(_normalize_to_arg(' ["user"] '), ["user"])

    def test_malformed_json_treated_as_plain_string(self) -> None:
        self.assertEqual(_normalize_to_arg("[not json"), ["[not json"])


if __name__ == "__main__":
    unittest.main()
