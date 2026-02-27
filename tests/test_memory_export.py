"""Tests for MemoryExporter — memory.md read-only export (Step 3)."""

import json
import os
import tempfile
import unittest

from cccc.kernel.memory import MemoryStore
from cccc.kernel.memory_export import export_markdown, export_manifest


class ExportTestBase(unittest.TestCase):
    def setUp(self):
        self._td = tempfile.TemporaryDirectory()
        self.db_path = os.path.join(self._td.name, "memory.db")
        self.group_id = "g_test_export"
        self.store = MemoryStore(self.db_path, group_id=self.group_id)
        self.output_dir = os.path.join(self._td.name, "output")
        os.makedirs(self.output_dir)

    def tearDown(self):
        self.store.close()
        self._td.cleanup()


class TestExportMarkdown(ExportTestBase):
    """export_markdown() produces valid Markdown output."""

    def test_empty_export(self):
        """Empty store produces minimal markdown."""
        md = export_markdown(self.store)
        self.assertIn("# Memory Export", md)
        self.assertIn("Total: 0", md)

    def test_export_groups_by_kind(self):
        """Memories are grouped by kind in sections."""
        self.store.store("A decision was made", kind="decision", status="solid")
        self.store.store("An observation", kind="observation", status="solid")
        md = export_markdown(self.store)
        self.assertIn("## decision", md)
        self.assertIn("## observation", md)
        self.assertIn("A decision was made", md)
        self.assertIn("An observation", md)

    def test_export_only_solid(self):
        """Only solid memories are exported by default."""
        self.store.store("solid mem", status="solid")
        self.store.store("draft mem", status="draft")
        md = export_markdown(self.store)
        self.assertIn("solid mem", md)
        self.assertNotIn("draft mem", md)

    def test_export_includes_metadata(self):
        """Each memory entry includes metadata."""
        self.store.store(
            "test content",
            kind="fact",
            status="solid",
            actor_id="peer-impl",
            confidence="high",
        )
        md = export_markdown(self.store)
        self.assertIn("test content", md)
        self.assertIn("peer-impl", md)
        self.assertIn("high", md)

    def test_export_include_draft(self):
        """export_markdown(include_draft=True) includes drafts."""
        self.store.store("draft mem", status="draft")
        md = export_markdown(self.store, include_draft=True)
        self.assertIn("draft mem", md)

    def test_export_idempotent(self):
        """Calling export twice produces identical output."""
        self.store.store("stable content", status="solid")
        md1 = export_markdown(self.store)
        md2 = export_markdown(self.store)
        self.assertEqual(md1, md2)


class TestExportManifest(ExportTestBase):
    """export_manifest() produces JSON with SHA-256 hash."""

    def test_manifest_structure(self):
        """Manifest has required fields."""
        self.store.store("test", status="solid")
        md = export_markdown(self.store)
        manifest = export_manifest(md, group_id=self.group_id)
        self.assertIn("group_id", manifest)
        self.assertIn("sha256", manifest)
        self.assertIn("memory_count", manifest)
        self.assertIn("exported_at", manifest)
        self.assertEqual(manifest["group_id"], self.group_id)

    def test_manifest_hash_changes_with_content(self):
        """Hash changes when content changes."""
        self.store.store("content A", status="solid")
        md1 = export_markdown(self.store)
        m1 = export_manifest(md1, group_id=self.group_id)

        self.store.store("content B", status="solid")
        md2 = export_markdown(self.store)
        m2 = export_manifest(md2, group_id=self.group_id)

        self.assertNotEqual(m1["sha256"], m2["sha256"])

    def test_manifest_hash_stable(self):
        """Same content produces same hash."""
        self.store.store("stable", status="solid")
        md = export_markdown(self.store)
        m1 = export_manifest(md, group_id=self.group_id)
        m2 = export_manifest(md, group_id=self.group_id)
        self.assertEqual(m1["sha256"], m2["sha256"])

    def test_manifest_count_matches(self):
        """memory_count matches actual exported memories."""
        self.store.store("mem1", status="solid")
        self.store.store("mem2", status="solid")
        self.store.store("mem3", status="draft")  # not exported
        md = export_markdown(self.store)
        manifest = export_manifest(md, group_id=self.group_id, memory_count=2)
        self.assertEqual(manifest["memory_count"], 2)


class TestExportToFile(ExportTestBase):
    """File-based export integration."""

    def test_write_markdown_file(self):
        """Can write markdown to a file path."""
        self.store.store("file test", status="solid")
        md = export_markdown(self.store)
        md_path = os.path.join(self.output_dir, "memory.md")
        with open(md_path, "w", encoding="utf-8") as f:
            f.write(md)
        self.assertTrue(os.path.exists(md_path))
        with open(md_path, encoding="utf-8") as f:
            content = f.read()
        self.assertIn("file test", content)

    def test_write_manifest_file(self):
        """Can write manifest to a JSON file."""
        self.store.store("manifest test", status="solid")
        md = export_markdown(self.store)
        manifest = export_manifest(md, group_id=self.group_id)
        manifest_path = os.path.join(self.output_dir, "manifest.json")
        with open(manifest_path, "w", encoding="utf-8") as f:
            json.dump(manifest, f)
        self.assertTrue(os.path.exists(manifest_path))
        with open(manifest_path, encoding="utf-8") as f:
            loaded = json.load(f)
        self.assertEqual(loaded["group_id"], self.group_id)


if __name__ == "__main__":
    unittest.main()
