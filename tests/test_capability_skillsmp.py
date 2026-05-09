from __future__ import annotations

import os
import tempfile
import unittest


class TestCapabilitySkillsMP(unittest.TestCase):
    def _with_home(self):
        old_home = os.environ.get("CCCC_HOME")
        td_ctx = tempfile.TemporaryDirectory(ignore_cleanup_errors=True)
        td = td_ctx.__enter__()
        os.environ["CCCC_HOME"] = td

        def cleanup() -> None:
            td_ctx.__exit__(None, None, None)
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

        return td, cleanup

    def _call(self, op: str, args: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        return handle_request(DaemonRequest.model_validate({"op": op, "args": args}))

    def test_parse_skillsmp_proxy_search_markdown_avoids_generic_skill_md_name(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        markdown = (
            '[finishing-branch.md 2.1k from "openakita/openakita-skills" '
            "Finish development branches with verification. 2026-03-20]"
            "(https://skillsmp.com/skills/openakita-openakita-skills-superpowers-finishing-branch-skill-md)\n"
        )
        rows = ops._parse_skillsmp_proxy_search_markdown(markdown, limit=10)
        self.assertTrue(rows)
        first = rows[0] if isinstance(rows[0], dict) else {}
        self.assertEqual(str(first.get("name") or ""), "finishing-branch")

    def test_parse_skillsmp_proxy_search_markdown_dedupes_http_https_slug(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        markdown = (
            "[brainstorming.md](http://skillsmp.com/skills/openakita-openakita-skills-superpowers-brainstorming-skill-md)\n"
            "[brainstorming.md](https://skillsmp.com/skills/openakita-openakita-skills-superpowers-brainstorming-skill-md)\n"
        )
        rows = ops._parse_skillsmp_proxy_search_markdown(markdown, limit=10)

        self.assertEqual(len(rows), 1)
        first = rows[0] if isinstance(rows[0], dict) else {}
        self.assertEqual(
            first.get("source_uri"),
            "https://skillsmp.com/skills/openakita-openakita-skills-superpowers-brainstorming-skill-md",
        )

    def test_skillsmp_record_display_name_repairs_catalog_skill_md(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        name = ops._skillsmp_record_display_name(
            {
                "name": "skill-md",
                "source_uri": "https://skillsmp.com/skills/mukul975-anthropic-cybersecurity-skills-skills-collecting-open-source-intelligence-skill-md",
            }
        )
        self.assertEqual(name, "collecting-open-source-intelligence")

    def test_capability_overview_repairs_persisted_skillsmp_skill_md_name(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            cap_id = "skill:skillsmp:mukul975-anthropic-cybersecurity-skills-skills-collecting-open-source-intelligence-skill-md-0c74ff93"
            path, catalog_doc = ops._load_catalog_doc()
            records = catalog_doc.get("records") if isinstance(catalog_doc.get("records"), dict) else {}
            records[cap_id] = {
                "capability_id": cap_id,
                "kind": "skill",
                "name": "skill-md",
                "description_short": "Collects and synthesizes open-source intelligence.",
                "source_id": "skillsmp_remote",
                "source_uri": "https://skillsmp.com/skills/mukul975-anthropic-cybersecurity-skills-skills-collecting-open-source-intelligence-skill-md",
                "qualification_status": "qualified",
                "enable_supported": True,
            }
            catalog_doc["records"] = records
            ops._save_catalog_doc(path, catalog_doc)

            overview_resp, _ = self._call(
                "capability_overview",
                {
                    "query": "open-source intelligence",
                    "limit": 20,
                    "include_indexed": True,
                },
            )
            self.assertTrue(overview_resp.ok, getattr(overview_resp, "error", None))
            result = overview_resp.result if isinstance(overview_resp.result, dict) else {}
            items = result.get("items") if isinstance(result.get("items"), list) else []
            row = next((item for item in items if isinstance(item, dict) and item.get("capability_id") == cap_id), {})
            self.assertEqual(str(row.get("name") or ""), "collecting-open-source-intelligence")
        finally:
            cleanup()

    def test_capability_overview_dedupes_persisted_skillsmp_http_https_records(self) -> None:
        from cccc.daemon.ops import capability_ops as ops

        _, cleanup = self._with_home()
        try:
            slug = "openakita-openakita-skills-superpowers-brainstorming-skill-md"
            path, catalog_doc = ops._load_catalog_doc()
            records = catalog_doc.get("records") if isinstance(catalog_doc.get("records"), dict) else {}
            for scheme, suffix in (("http", "70948a47"), ("https", "265ea9a3")):
                cap_id = f"skill:skillsmp:{slug}-{suffix}"
                records[cap_id] = {
                    "capability_id": cap_id,
                    "kind": "skill",
                    "name": "brainstorming",
                    "description_short": "Brainstorming skill.",
                    "source_id": "skillsmp_remote",
                    "source_uri": f"{scheme}://skillsmp.com/skills/{slug}",
                    "qualification_status": "qualified",
                    "enable_supported": True,
                }
            catalog_doc["records"] = records
            ops._save_catalog_doc(path, catalog_doc)

            overview_resp, _ = self._call(
                "capability_overview",
                {
                    "query": slug,
                    "limit": 20,
                    "include_indexed": True,
                },
            )

            self.assertTrue(overview_resp.ok, getattr(overview_resp, "error", None))
            result = overview_resp.result if isinstance(overview_resp.result, dict) else {}
            items = result.get("items") if isinstance(result.get("items"), list) else []
            matches = [
                item for item in items
                if isinstance(item, dict)
                and str(item.get("source_id") or "") == "skillsmp_remote"
                and slug in str(item.get("source_uri") or item.get("capability_id") or "")
            ]
            self.assertEqual(len(matches), 1)
            self.assertEqual(matches[0].get("capability_id"), f"skill:skillsmp:{slug}-265ea9a3")
            self.assertEqual(matches[0].get("source_uri"), f"https://skillsmp.com/skills/{slug}")
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
