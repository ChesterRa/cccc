import unittest

from cccc.ports.im.adapters.feishu.attachments import parse_download_attachment


class TestFeishuAttachmentDownloadParsing(unittest.TestCase):
    def test_parses_image_attachment_key(self) -> None:
        parsed = parse_download_attachment({"kind": "image", "image_key": "img_key"})

        self.assertEqual(parsed.kind, "image")
        self.assertEqual(parsed.key, "img_key")

    def test_parses_file_attachment_key(self) -> None:
        parsed = parse_download_attachment({"kind": "file", "file_key": "file_key"})

        self.assertEqual(parsed.kind, "file")
        self.assertEqual(parsed.key, "file_key")

    def test_rejects_image_without_key(self) -> None:
        with self.assertRaisesRegex(ValueError, "Missing image_key"):
            parse_download_attachment({"kind": "image"})

    def test_rejects_file_without_key(self) -> None:
        with self.assertRaisesRegex(ValueError, "Missing file_key"):
            parse_download_attachment({"kind": "file"})

    def test_rejects_unknown_kind(self) -> None:
        with self.assertRaisesRegex(ValueError, "Unknown attachment kind: video"):
            parse_download_attachment({"kind": "video"})


if __name__ == "__main__":
    unittest.main()
