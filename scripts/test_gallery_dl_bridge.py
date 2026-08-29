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
        self.job._post_terminal_mode = None

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

    def test_single_media_posts_settle_before_the_extractor_advances(self):
        self.job._post_terminal_mode = "single"
        first = PathFormat("post-1", "/tmp/one.jpg")
        next_post = PathFormat("post-2", "/tmp/two.jpg")

        self.job._on_post(first)
        self.job._on_after(first)
        self.job._on_post(next_post)

        self.assertEqual(
            self.events,
            [
                "post_traversed",
                "item_downloaded",
                "post_complete",
                "post_traversed",
            ],
        )

    def test_counted_posts_settle_only_after_the_last_media_item(self):
        self.job._post_terminal_mode = "count-one"
        first = PathFormat("post-1", "/tmp/one.jpg")
        first.kwdict.update({"num": 1, "count": 2})
        last = PathFormat("post-1", "/tmp/two.jpg")
        last.kwdict.update({"num": 2, "count": 2})

        self.job._on_post(first)
        self.job._on_after(first)
        self.assertNotIn("post_complete", self.events)
        self.job._on_post(last)
        self.job._on_after(last)

        self.assertEqual(self.events[-1], "post_complete")

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


class DomainRequestPacingTests(unittest.TestCase):
    def setUp(self):
        bridge._next_request_monotonic.clear()
        self.sleeps = []

    def sleep(self, seconds):
        self.sleeps.append(seconds)

    def test_consecutive_requests_to_one_host_wait_only_the_remainder(self):
        bridge._pace_domain_request("e621.net", 100.0, sleep=self.sleep)
        bridge._pace_domain_request("e621.net", 100.4, sleep=self.sleep)

        self.assertEqual(len(self.sleeps), 1)
        self.assertAlmostEqual(self.sleeps[0], 0.6, places=6)

    def test_a_request_after_the_interval_is_not_delayed(self):
        bridge._pace_domain_request("e621.net", 100.0, sleep=self.sleep)
        bridge._pace_domain_request("e621.net", 101.25, sleep=self.sleep)

        self.assertEqual(self.sleeps, [])

    def test_hosts_are_paced_independently(self):
        bridge._pace_domain_request("e621.net", 100.0, sleep=self.sleep)
        bridge._pace_domain_request("static1.e621.net", 100.1, sleep=self.sleep)

        self.assertEqual(self.sleeps, [])

    def test_the_sleep_advances_the_recorded_send_time(self):
        bridge._pace_domain_request("e621.net", 100.0, sleep=self.sleep)
        sent_at = bridge._pace_domain_request("e621.net", 100.4, sleep=self.sleep)

        self.assertAlmostEqual(sent_at, 101.0, places=6)
        # A third request right after the delayed second one still waits a
        # full interval measured from the actual send time.
        bridge._pace_domain_request("e621.net", 101.0, sleep=self.sleep)
        self.assertAlmostEqual(self.sleeps[-1], 1.0, places=6)

    def test_slots_persist_across_processes_through_the_state_directory(self):
        import tempfile

        with tempfile.TemporaryDirectory() as state_dir:
            bridge._pace_domain_request(
                "yande.re", 100.0, sleep=self.sleep, state_dir=state_dir
            )
            # A fresh process has an empty in-memory map but must honor the
            # previous process's reservation from the state directory.
            bridge._next_request_monotonic.clear()
            sent_at = bridge._pace_domain_request(
                "yande.re", 100.3, sleep=self.sleep, state_dir=state_dir
            )

        self.assertAlmostEqual(sent_at, 101.0, places=6)
        self.assertAlmostEqual(self.sleeps[-1], 0.7, places=6)

    def test_a_stored_slot_from_a_previous_boot_is_ignored(self):
        import tempfile

        with tempfile.TemporaryDirectory() as state_dir:
            bridge._store_host_slot(state_dir, "yande.re", 999999.0)
            sent_at = bridge._pace_domain_request(
                "yande.re", 100.0, sleep=self.sleep, state_dir=state_dir
            )

        self.assertAlmostEqual(sent_at, 100.0, places=6)
        self.assertEqual(self.sleeps, [])

    def test_concurrent_callers_reserve_distinct_send_slots(self):
        # Two callers arriving before either sends must not share a base:
        # each reserves its own slot a full interval apart.
        first = bridge._pace_domain_request("yande.re", 100.0, sleep=self.sleep)
        second = bridge._pace_domain_request("yande.re", 100.001, sleep=self.sleep)
        third = bridge._pace_domain_request("yande.re", 100.002, sleep=self.sleep)

        self.assertAlmostEqual(first, 100.0, places=6)
        self.assertAlmostEqual(second, 101.0, places=6)
        self.assertAlmostEqual(third, 102.0, places=6)

    def test_provider_intervals_match_gallery_dl_and_others_are_randomized(self):
        self.assertEqual(bridge._request_interval_for_site("ehentai"), (3.0, 6.0))
        self.assertEqual(bridge._request_interval_for_site("e621"), (1.0, 1.5))
        self.assertEqual(bridge._request_interval_for_site("furaffinity"), (1.0, 1.0))
        self.assertEqual(bridge._request_interval_for_site("twitter"), (0.5, 2.0))

    def test_random_interval_reserves_the_sampled_delay(self):
        first = bridge._pace_domain_request(
            "example.com",
            100.0,
            sleep=self.sleep,
            interval=(0.5, 2.0),
            uniform=lambda _minimum, _maximum: 1.25,
        )
        second = bridge._pace_domain_request(
            "example.com",
            100.5,
            sleep=self.sleep,
            interval=(0.5, 2.0),
            uniform=lambda _minimum, _maximum: 1.25,
        )

        self.assertAlmostEqual(first, 100.0, places=6)
        self.assertAlmostEqual(second, 101.25, places=6)
        self.assertAlmostEqual(self.sleeps[-1], 0.75, places=6)


if __name__ == "__main__":
    unittest.main()
