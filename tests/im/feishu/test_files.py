import unittest
from pathlib import Path

from cccc.ports.im.adapters.feishu.files import prepare_upload_file


class TestFeishuFilePreparation(unittest.TestCase):
    def test_sanitizes_filename_and_detects_regular_file_mime_type(self) -> None:
        prepared = prepare_upload_file(Path("fallback.txt"), "reports\\2026/final.md")

        self.assertEqual(prepared.filename, "reports_2026_final.md")
        self.assertEqual(prepared.mime_type, "text/markdown")
        self.assertFalse(prepared.is_image)

    def test_uses_path_name_when_filename_is_empty(self) -> None:
        prepared = prepare_upload_file(Path("photo.png"), "")

        self.assertEqual(prepared.filename, "photo.png")
        self.assertEqual(prepared.mime_type, "image/png")
        self.assertTrue(prepared.is_image)

    def test_falls_back_to_generic_file_and_octet_stream(self) -> None:
        prepared = prepare_upload_file(Path(""), "")

        self.assertEqual(prepared.filename, "file")
        self.assertEqual(prepared.mime_type, "application/octet-stream")
        self.assertFalse(prepared.is_image)


if __name__ == "__main__":
    unittest.main()
