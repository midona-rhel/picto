import importlib.util
import io
import pathlib
import sys
import unittest


MODULE_PATH = pathlib.Path(__file__).with_name("gallery_dl_bridge.py")
SPEC = importlib.util.spec_from_file_location("picto_gallery_dl_bridge", MODULE_PATH)
bridge = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(bridge)


class PathFormat:
    def __init__(self, post_id: str, path: str):
        self.kwdict = {
            "category": "subscribestar",
            "post_id": post_id,
            "url": f"https://example.invalid/{post_id}/{pathlib.Path(path).name}",
        }
        self.path = path


class GalleryBridgePostBoundaryTests(unittest.TestCase):
    def setUp(self):
        self.events = []
        self.original_emit = bridge._emit
        self.original_stdin = sys.stdin
        bridge._emit = lambda event_type, **payload: self.events.append(event_type)
        sys.stdin = io.StringIO("ack\nack\n")
        self.job = bridge.PictoDownloadJob.__new__(bridge.PictoDownloadJob)
        self.job._post_gate = bridge._AcceptedPostGate(2, ["jpg"])

    def tearDown(self):
        bridge._emit = self.original_emit
        sys.stdin = self.original_stdin

    def test_all_files_finish_before_the_post_is_acknowledged(self):
        first = PathFormat("post-1", "/tmp/one.jpg")
        second = PathFormat("post-1", "/tmp/two.jpg")
        next_post = PathFormat("post-2", "/tmp/three.jpg")

        self.job._on_post(first)
        self.job._on_after(first)
        self.job._on_post(second)
        self.job._on_after(second)
        self.assertEqual(
            self.events,
            ["post_traversed", "item_downloaded", "item_downloaded"],
        )

        self.job._on_post(next_post)
        self.assertEqual(
            self.events,
            [
                "post_traversed",
                "item_downloaded",
                "item_downloaded",
                "post_complete",
                "post_traversed",
            ],
        )

    def test_the_final_post_is_acknowledged_when_the_extractor_ends(self):
        item = PathFormat("post-1", "/tmp/one.jpg")
        self.job._on_post(item)
        self.job._on_after(item)
        self.job._job = type("FinishedJob", (), {"run": lambda _self: 0})()

        self.assertEqual(self.job.run(), 0)
        self.assertEqual(
            self.events,
            ["post_traversed", "item_downloaded", "post_complete"],
        )

    def test_limit_stops_before_announcing_the_next_post(self):
        self.job._post_gate = bridge._AcceptedPostGate(1, ["jpg"])
        first = PathFormat("post-1", "/tmp/directory-without-extension")
        first.path = "/tmp/one.jpg"
        next_post = PathFormat("post-2", "/tmp/directory-without-extension")

        self.job._on_post(first)
        self.job._on_prepare(first)
        self.job._on_after(first)
        self.job._acknowledge_current_post()

        with self.assertRaises(Exception):
            self.job._on_post(next_post)
        self.assertEqual(
            self.events,
            [
                "post_traversed",
                "item_discovered",
                "item_downloaded",
                "post_complete",
            ],
        )


class GalleryBridgeDownloadProgressTests(unittest.TestCase):
    def setUp(self):
        self.events = []
        self.original_emit = bridge._emit
        self.original_monotonic = bridge.time.monotonic
        bridge._emit = lambda event_type, **payload: self.events.append(
            (event_type, payload)
        )

    def tearDown(self):
        bridge._emit = self.original_emit
        bridge.time.monotonic = self.original_monotonic

    def test_download_progress_is_throttled_to_the_watchdog_heartbeat_rate(self):
        output = bridge._NullOutput()
        moments = iter((1.0, 1.1, 1.5))
        bridge.time.monotonic = lambda: next(moments)

        output.progress(1_000, 100, 100)
        output.progress(1_000, 200, 100)
        output.progress(1_000, 600, 100)

        self.assertEqual(
            self.events,
            [
                (
                    "download_progress",
                    {
                        "bytes_total": 1_000,
                        "bytes_downloaded": 100,
                        "bytes_per_second": 100,
                    },
                ),
                (
                    "download_progress",
                    {
                        "bytes_total": 1_000,
                        "bytes_downloaded": 600,
                        "bytes_per_second": 100,
                    },
                ),
            ],
        )


if __name__ == "__main__":
    unittest.main()
