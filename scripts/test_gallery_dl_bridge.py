import importlib.util
import io
import json
import pathlib
import sys
import unittest
from unittest.mock import Mock


BRIDGE_PATH = pathlib.Path(__file__).with_name("gallery_dl_bridge.py")
SPEC = importlib.util.spec_from_file_location("picto_gallery_dl_bridge", BRIDGE_PATH)
BRIDGE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(BRIDGE)


class AcceptedPostGateTests(unittest.TestCase):
    def test_post_completion_emits_boundary_then_consumes_acknowledgement(self):
        stdin, stdout = sys.stdin, sys.stdout
        gate = Mock()
        gate.has_current.return_value = True
        try:
            sys.stdin = io.StringIO("continue\n")
            sys.stdout = io.StringIO()
            BRIDGE.PictoDownloadJob._on_post_complete(
                type("Job", (), {"_post_gate": gate})(),
                None,
            )
            event = json.loads(sys.stdout.getvalue())
        finally:
            sys.stdin, sys.stdout = stdin, stdout

        self.assertEqual(event, {"event": "post_complete"})
        gate.complete.assert_called_once_with()

    def test_multi_file_post_identity_ignores_file_specific_urls(self):
        first = {
            "category": "webtoons",
            "title_no": 123,
            "episode_no": 7,
            "url": "https://cdn.example/001.jpg",
        }
        second = {**first, "url": "https://cdn.example/002.jpg"}

        self.assertEqual(BRIDGE._post_identity(first), BRIDGE._post_identity(second))

    def test_only_supported_downloads_consume_the_post_limit(self):
        from gallery_dl import exception

        gate = BRIDGE._AcceptedPostGate(2, ["jpg", "png"])

        self.assertTrue(gate.begin({"category": "fanbox", "id": 1})[0])
        gate.downloaded("unsupported.txt", {"category": "fanbox", "id": 1})
        gate.complete()

        self.assertTrue(gate.begin({"category": "fanbox", "id": 2})[0])
        gate.downloaded("first.jpg", {"category": "fanbox", "id": 2})
        self.assertFalse(gate.begin({"category": "fanbox", "id": 2})[0])
        gate.downloaded("second.png", {"category": "fanbox", "id": 2})
        gate.complete()

        self.assertTrue(gate.begin({"category": "fanbox", "id": 3})[0])
        gate.downloaded("third.png", {"category": "fanbox", "id": 3})
        gate.complete()

        with self.assertRaises(exception.StopExtraction):
            gate.begin({"category": "fanbox", "id": 4})

    def test_patreon_attachment_identity_does_not_bypass_post_limit(self):
        from gallery_dl import exception

        gate = BRIDGE._AcceptedPostGate(1, ["png"])
        gate.begin({"category": "patreon", "id": 42})
        gate.downloaded(
            "attachment.png",
            {"category": "patreon", "id": 9001, "post_id": 42},
        )
        gate.complete()

        with self.assertRaises(exception.StopExtraction):
            gate.begin({"category": "patreon", "id": 43})


if __name__ == "__main__":
    unittest.main()
