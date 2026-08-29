#!/usr/bin/env python3

import contextlib
import io
import json
import sqlite3
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace

import onlyfans_bridge


class OnlyFansBridgeTests(unittest.TestCase):
    def test_onlyfans_title_uses_post_text_and_has_a_stable_fallback(self) -> None:
        self.assertEqual(
            onlyfans_bridge.post_title("alice", "<strong>Hello</strong> world", None, "1"),
            "Hello world",
        )
        self.assertEqual(
            onlyfans_bridge.post_title("alice", None, "2026-08-25T12:00:00Z", "1"),
            "alice - 2026-08-25",
        )

    def test_runtime_uses_ofscraper_native_pacing_and_concurrency(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            root = Path(raw_root)
            config = onlyfans_bridge.write_runtime(
                {
                    "state_dir": str(root / "state"),
                    "output_dir": str(root / "downloads"),
                }
            )

            configured = json.loads(config.read_text())
            self.assertNotIn("performance_options", configured)

        arguments = onlyfans_bridge.ofscraper_arguments(
            {"creator": "alice", "post_limit": 10, "before": None}, config
        )
        self.assertNotIn("--downloadsem", arguments)

    def test_recent_post_url_is_newest_first_and_uses_before_cursor(self) -> None:
        url = onlyfans_bridge.recent_posts_url(
            42, "Timeline", "2026-08-24T12:00:00Z"
        )

        self.assertIn("/users/42/posts?", url)
        self.assertIn("order=publish_date_desc", url)
        self.assertIn("beforePublishTime=1787572800.0", url)

    def test_pinned_url_is_bounded_without_advancing_history_cursor(self) -> None:
        url = onlyfans_bridge.recent_posts_url(
            42, "Pinned", "2026-08-24T12:00:00Z"
        )

        self.assertIn("/users/42/posts?", url)
        self.assertIn("pinned=1", url)
        self.assertNotIn("beforePublishTime", url)

    def test_post_limit_keeps_every_media_file_from_selected_posts(self) -> None:
        media = [
            SimpleNamespace(post_id="new", postdate="2026-08-24T12:00:00Z"),
            SimpleNamespace(post_id="new", postdate="2026-08-24T12:00:00Z"),
            SimpleNamespace(post_id="old", postdate="2026-08-23T12:00:00Z"),
        ]

        selected = onlyfans_bridge.post_bounded_media(media, 1)

        self.assertEqual([item.post_id for item in selected], ["new", "new"])

    def test_raw_post_limit_counts_locked_posts_before_media_expansion(self) -> None:
        selected = onlyfans_bridge.recent_post_ids(
            [
                {"id": "locked", "postedAtPrecise": 30},
                {"id": "visible", "postedAtPrecise": 20},
                {"id": "older", "postedAtPrecise": 10},
            ],
            2,
        )

        self.assertEqual(selected, {"locked", "visible"})

    def test_onlyfans_source_order_is_purchased_then_messages_then_feed(self) -> None:
        ordered = onlyfans_bridge.ordered_source_posts(
            [
                {"id": "feed", "createdAt": 300, "_picto_source_group": "feed"},
                {
                    "id": "message",
                    "createdAt": 200,
                    "_picto_source_group": "messages",
                },
                {
                    "id": "purchase",
                    "createdAt": 100,
                    "_picto_source_group": "purchased",
                },
            ],
            100,
        )

        self.assertEqual(
            [onlyfans_bridge.source_post_id(post) for post in ordered],
            ["purchase", "message", "feed"],
        )

    def test_source_media_order_uses_the_post_payload_not_database_order(self) -> None:
        order = onlyfans_bridge.source_media_order(
            [
                {
                    "id": "post-1",
                    "media": [{"id": "second"}, {"id": "first"}],
                }
            ]
        )

        self.assertEqual(order[("post-1", "second")], 1)
        self.assertEqual(order[("post-1", "first")], 2)

    def test_ofscraper_getter_compatibility_supplies_only_missing_defaults(self) -> None:
        values = {"present": "configured", "zero": 0}
        getter = onlyfans_bridge.getter_with_default(values.get)

        self.assertEqual(getter("present", "fallback"), "configured")
        self.assertEqual(getter("zero", 10), 0)
        self.assertEqual(getter("missing", "fallback"), "fallback")

    def test_output_preserves_post_order_and_defers_locked_media(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            root = Path(raw_root)
            state = root / "state"
            data = state / ".data" / "creator-1"
            output = root / "downloads"
            data.mkdir(parents=True)
            output.mkdir()
            database = sqlite3.connect(data / "user_data.db")
            database.executescript(
                """
                CREATE TABLE posts (
                    post_id TEXT, text TEXT, created_at TEXT, model_id TEXT,
                    is_deleted INTEGER, archived INTEGER, pinned INTEGER,
                    stream INTEGER
                );
                CREATE TABLE medias (
                    id INTEGER, media_id TEXT, post_id TEXT, link TEXT,
                    directory TEXT, filename TEXT, media_type TEXT, api_type TEXT,
                    downloaded INTEGER, unlocked INTEGER, created_at TEXT,
                    posted_at TEXT, model_id TEXT
                );
                """
            )
            database.executemany(
                "INSERT INTO posts VALUES (?, ?, ?, 'creator-1', 0, 0, 0, 0)",
                [
                    ("new", "<strong>Hello &amp; welcome</strong>", "2026-08-24T12:00:00Z"),
                    ("locked", "Later", "2026-08-23T12:00:00Z"),
                ],
            )
            files = []
            for position in (1, 2):
                path = output / f"media-{position}.jpg"
                path.write_bytes(b"image")
                files.append(path)
                database.execute(
                    "INSERT INTO medias VALUES (?, ?, 'new', ?, ?, ?, 'photo', 'Timeline', 1, 1, ?, ?, 'creator-1')",
                    (
                        position,
                        f"media-{position}",
                        f"https://cdn/{position}",
                        str(output),
                        path.name,
                        "2026-08-24T12:00:00Z",
                        "2026-08-24T12:00:00Z",
                    ),
                )
            database.execute(
                "INSERT INTO medias VALUES (3, 'locked-media', 'locked', NULL, '', '', 'photo', 'Timeline', 0, 0, ?, ?, 'creator-1')",
                ("2026-08-23T12:00:00Z", "2026-08-23T12:00:00Z"),
            )
            database.commit()
            database.close()

            request = {
                "state_dir": str(state),
                "output_dir": str(output),
                "creator": "alice",
                "post_limit": 100,
            }
            stream = io.StringIO()
            with contextlib.redirect_stdout(stream):
                oldest, count = onlyfans_bridge.output_items(
                    request,
                    source_posts=[
                        {
                            "id": "new",
                            "media": [{"id": "media-2"}, {"id": "media-1"}],
                        },
                        {"id": "locked", "media": [{"id": "locked-media"}]},
                    ],
                )
            events = [json.loads(line) for line in stream.getvalue().splitlines()]

            self.assertEqual(
                json.loads(oldest),
                {
                    "created_at": "2026-08-23T12:00:00Z",
                    "post_id": "locked",
                    "source_group": "feed",
                },
            )
            self.assertEqual(count, 2)
            self.assertEqual([event["position"] for event in events if event["event"] == "item"], [1, 2])
            self.assertEqual(
                [event["item_key"] for event in events if event["event"] == "item"],
                ["media-2", "media-1"],
            )
            self.assertEqual(
                [event["description"] for event in events if event["event"] == "item"],
                ["Hello & welcome", "Hello & welcome"],
            )
            self.assertEqual(
                [event["title"] for event in events if event["event"] == "item"],
                ["Hello & welcome", "Hello & welcome"],
            )
            traversed = {
                event["post_id"]: event["accessible"]
                for event in events
                if event["event"] == "post_traversed"
            }
            self.assertEqual(traversed, {"new": True, "locked": False})

    def test_auth_remains_process_local(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            root = Path(raw_root)
            state = root / "state"
            output = root / "run" / "downloads"
            config = onlyfans_bridge.write_runtime(
                {
                    "state_dir": str(state),
                    "output_dir": str(output),
                    "cookies": {"sess": "secret", "auth_id": "42"},
                    "headers": {"user-agent": "Picto", "x-bc": "signature"},
                }
            )

            self.assertFalse((config.parent / "picto_profile" / "auth.json").exists())
            self.assertFalse((state / "picto_profile" / "auth.json").exists())
            configured = json.loads(config.read_text())
            self.assertTrue(configured["metadata"].startswith(str(state.resolve())))

            auth = onlyfans_bridge.auth_from_request(
                {
                    "cookies": {"sess": "secret", "auth_id": "42"},
                    "headers": {"user-agent": "Picto", "x-bc": "signature"},
                }
            )
            self.assertEqual(auth["sess"], "secret")
            self.assertEqual(auth["auth_uid"], "42")

    def test_ofscraper_auth_reader_uses_process_local_session(self) -> None:
        reader = onlyfans_bridge.process_auth_reader(
            {
                "sess": "captured",
                "auth_id": "42",
                "auth_uid": "42",
                "user_agent": "Picto",
                "x-bc": "signature",
            }
        )

        first = reader()
        first["sess"] = "modified"
        self.assertEqual(reader()["sess"], "captured")

    def test_ofscraper_auth_module_exposes_only_process_local_reader(self) -> None:
        module = onlyfans_bridge.process_auth_module({"sess": "captured"})

        self.assertEqual(module.__name__, "ofscraper.utils.auth.file")
        self.assertEqual(module.read_auth(), {"sess": "captured"})
        self.assertFalse(hasattr(module, "edit_auth"))

    def test_composite_cursor_does_not_skip_posts_with_the_same_timestamp(self) -> None:
        connection = sqlite3.connect(":memory:")
        connection.row_factory = sqlite3.Row
        connection.execute(
            "CREATE TABLE posts (post_id TEXT, text TEXT, created_at TEXT, is_deleted INTEGER)"
        )
        connection.executemany(
            "INSERT INTO posts VALUES (?, '', '2026-08-24T12:00:00Z', 0)",
            [("3",), ("2",), ("1",)],
        )
        request = {
            "before": onlyfans_bridge.encode_cursor("2026-08-24T12:00:00Z", "3"),
            "post_limit": 10,
        }

        posts = onlyfans_bridge.selected_posts(connection, request)

        self.assertEqual([post["post_id"] for post in posts], ["2", "1"])

    def test_selected_posts_include_messages_and_preserve_source_priority(self) -> None:
        connection = sqlite3.connect(":memory:")
        connection.row_factory = sqlite3.Row
        connection.executescript(
            """
            CREATE TABLE posts (
                post_id TEXT, text TEXT, created_at TEXT, model_id TEXT,
                is_deleted INTEGER
            );
            CREATE TABLE messages (
                post_id TEXT, text TEXT, created_at TEXT, model_id TEXT,
                is_deleted INTEGER
            );
            CREATE TABLE medias (
                post_id TEXT, model_id TEXT, api_type TEXT
            );
            INSERT INTO posts VALUES
                ('purchase', '', '2026-08-01T00:00:00Z', 'creator', 0),
                ('feed', '', '2026-08-03T00:00:00Z', 'creator', 0);
            INSERT INTO messages VALUES
                ('message', '', '2026-08-02T00:00:00Z', 'creator', 0);
            INSERT INTO medias VALUES ('purchase', 'creator', 'Paid');
            """
        )

        posts = onlyfans_bridge.selected_posts(
            connection, {"post_limit": 100}
        )

        self.assertEqual(
            [post["post_id"] for post in posts],
            ["purchase", "message", "feed"],
        )

    def test_grouped_cursor_finishes_messages_before_starting_feed(self) -> None:
        connection = sqlite3.connect(":memory:")
        connection.row_factory = sqlite3.Row
        connection.executescript(
            """
            CREATE TABLE posts (
                post_id TEXT, text TEXT, created_at TEXT, is_deleted INTEGER
            );
            CREATE TABLE messages (
                post_id TEXT, text TEXT, created_at TEXT, is_deleted INTEGER
            );
            INSERT INTO posts VALUES ('feed', '', '2026-08-24T00:00:00Z', 0);
            INSERT INTO messages VALUES
                ('new-message', '', '2026-08-23T00:00:00Z', 0),
                ('old-message', '', '2026-08-22T00:00:00Z', 0);
            """
        )
        request = {
            "before": onlyfans_bridge.encode_cursor(
                "2026-08-23T00:00:00Z", "new-message", "messages"
            ),
            "post_limit": 100,
        }

        posts = onlyfans_bridge.selected_posts(connection, request)

        self.assertEqual(
            [post["post_id"] for post in posts], ["old-message", "feed"]
        )

    def test_accessible_media_without_a_download_blocks_cursor_advancement(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            root = Path(raw_root)
            state = root / "state"
            data = state / ".data" / "creator-1"
            output = root / "downloads"
            data.mkdir(parents=True)
            output.mkdir()
            database = sqlite3.connect(data / "user_data.db")
            database.executescript(
                """
                CREATE TABLE posts (
                    post_id TEXT, text TEXT, created_at TEXT, model_id TEXT,
                    is_deleted INTEGER, archived INTEGER, pinned INTEGER,
                    stream INTEGER
                );
                CREATE TABLE medias (
                    id INTEGER, media_id TEXT, post_id TEXT, link TEXT,
                    directory TEXT, filename TEXT, media_type TEXT, api_type TEXT,
                    downloaded INTEGER, unlocked INTEGER, created_at TEXT,
                    posted_at TEXT, model_id TEXT
                );
                INSERT INTO posts VALUES (
                    'post-1', '', '2026-08-24T12:00:00Z', 'creator-1', 0, 0, 0, 0
                );
                INSERT INTO medias VALUES (
                    1, 'media-1', 'post-1', 'https://cdn/1', '', '', 'photo',
                    'Timeline', 0, 1, '2026-08-24T12:00:00Z',
                    '2026-08-24T12:00:00Z', 'creator-1'
                );
                """
            )
            database.close()

            with self.assertRaisesRegex(RuntimeError, "did not download 1 accessible"):
                onlyfans_bridge.output_items(
                    {
                        "state_dir": str(state),
                        "output_dir": str(output),
                        "creator": "alice",
                        "post_limit": 100,
                    }
                )

    def test_drm_protected_media_defers_the_post_instead_of_failing(self):
        with tempfile.TemporaryDirectory() as raw_root:
            root = Path(raw_root)
            state = root / "state"
            data = state / ".data" / "creator-1"
            output = root / "downloads"
            data.mkdir(parents=True)
            output.mkdir()
            database = sqlite3.connect(data / "user_data.db")
            database.executescript(
                """
                CREATE TABLE posts (
                    post_id TEXT, text TEXT, created_at TEXT, model_id TEXT,
                    is_deleted INTEGER, archived INTEGER, pinned INTEGER,
                    stream INTEGER
                );
                CREATE TABLE medias (
                    id INTEGER, media_id TEXT, post_id TEXT, link TEXT,
                    directory TEXT, filename TEXT, media_type TEXT, api_type TEXT,
                    downloaded INTEGER, unlocked INTEGER, created_at TEXT,
                    posted_at TEXT, model_id TEXT
                );
                INSERT INTO posts VALUES (
                    'post-1', '', '2026-08-24T12:00:00Z', 'creator-1', 0, 0, 0, 0
                );
                INSERT INTO medias VALUES (
                    1, 'media-1', 'post-1',
                    'https://cdn/stream/dash/manifest.mpd', '', '', 'video',
                    'Timeline', 0, 1, '2026-08-24T12:00:00Z',
                    '2026-08-24T12:00:00Z', 'creator-1'
                );
                """
            )
            database.close()

            events = []
            original = onlyfans_bridge.emit
            onlyfans_bridge.emit = lambda event, **values: events.append(
                (event, values)
            )
            try:
                onlyfans_bridge.output_items(
                    {
                        "state_dir": str(state),
                        "output_dir": str(output),
                        "creator": "alice",
                        "post_limit": 100,
                    }
                )
            finally:
                onlyfans_bridge.emit = original

            traversed = [v for e, v in events if e == "post_traversed"]
            self.assertEqual(len(traversed), 1)
            self.assertFalse(traversed[0]["accessible"])

    def test_current_source_window_excludes_stale_database_posts(self) -> None:
        connection = sqlite3.connect(":memory:")
        connection.row_factory = sqlite3.Row
        connection.executescript(
            """
            CREATE TABLE posts (
                post_id TEXT, text TEXT, created_at TEXT, is_deleted INTEGER
            );
            INSERT INTO posts VALUES
                ('stale', '', '2026-08-25T00:00:00Z', 0),
                ('fetched', '', '2026-08-24T00:00:00Z', 0);
            """
        )

        posts = onlyfans_bridge.selected_posts(
            connection,
            {"post_limit": 1},
            [{"id": "fetched", "createdAt": "2026-08-24T00:00:00Z"}],
        )

        self.assertEqual([post["post_id"] for post in posts], ["fetched"])

    def test_download_filter_uses_the_persisted_post_without_advancing(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            root = Path(raw_root)
            data = root / ".data" / "creator"
            data.mkdir(parents=True)
            connection = sqlite3.connect(data / "user_data.db")
            connection.executescript(
                """
                CREATE TABLE posts (
                    post_id TEXT, text TEXT, created_at TEXT, model_id TEXT,
                    is_deleted INTEGER
                );
                CREATE TABLE medias (
                    post_id TEXT, model_id TEXT, api_type TEXT
                );
                INSERT INTO posts VALUES
                    ('persisted', '', '2026-08-28T19:00:16Z', 'creator', 0),
                    ('later', '', '2026-08-27T19:00:16Z', 'creator', 0);
                INSERT INTO medias VALUES
                    ('persisted', 'creator', 'Timeline'),
                    ('later', 'creator', 'Timeline');
                """
            )
            connection.close()
            source_posts = [
                {
                    "id": "raw-only",
                    "createdAt": "2026-08-29T19:00:16Z",
                    "_picto_source_group": "feed",
                },
                {
                    "id": "persisted",
                    "createdAt": "2026-08-28T19:00:16Z",
                    "_picto_source_group": "feed",
                },
                {
                    "id": "later",
                    "createdAt": "2026-08-27T19:00:16Z",
                    "_picto_source_group": "feed",
                },
            ]
            request = {"state_dir": str(root), "post_limit": 1}
            media = [
                *[SimpleNamespace(post_id="persisted") for _ in range(22)],
                SimpleNamespace(post_id="later"),
            ]

            self.assertEqual(
                onlyfans_bridge.recent_post_ids(source_posts, 1), {"raw-only"}
            )
            filtered = onlyfans_bridge.filter_selected_download_media(
                request, source_posts, media
            )
            self.assertEqual(len(filtered), 22)
            self.assertEqual({item.post_id for item in filtered}, {"persisted"})

    def test_message_cursor_requests_the_next_message_page(self) -> None:
        cursor = onlyfans_bridge.encode_cursor(
            "2026-08-24T00:00:00Z", "message-42", "messages"
        )
        self.assertIn(
            "id=message-42",
            onlyfans_bridge.recent_messages_url("creator", cursor),
        )
        purchased_cursor = onlyfans_bridge.encode_cursor(
            "2026-08-24T00:00:00Z", "purchase-1", "purchased"
        )
        self.assertNotIn(
            "id=", onlyfans_bridge.recent_messages_url("creator", purchased_cursor)
        )

    def test_completed_posts_stream_newest_first_before_the_run_finishes(self) -> None:
        with tempfile.TemporaryDirectory() as raw_root:
            root = Path(raw_root)
            state = root / "state"
            data = state / ".data" / "creator-1"
            output = root / "downloads"
            data.mkdir(parents=True)
            output.mkdir()
            new_file = output / "new.jpg"
            new_file.write_bytes(b"new")
            old_first_file = output / "old-first.jpg"
            old_first_file.write_bytes(b"old-first")
            database_path = data / "user_data.db"
            database = sqlite3.connect(database_path)
            database.executescript(
                """
                CREATE TABLE posts (
                    post_id TEXT, text TEXT, created_at TEXT, model_id TEXT,
                    is_deleted INTEGER, archived INTEGER, pinned INTEGER,
                    stream INTEGER
                );
                CREATE TABLE medias (
                    id INTEGER, media_id TEXT, post_id TEXT, link TEXT,
                    directory TEXT, filename TEXT, media_type TEXT, api_type TEXT,
                    downloaded INTEGER, unlocked INTEGER, created_at TEXT,
                    posted_at TEXT, model_id TEXT
                );
                INSERT INTO posts VALUES
                    ('new', 'Newest', '2026-08-24T12:00:00Z', 'creator-1', 0, 0, 0, 0),
                    ('old', 'Older', '2026-08-23T12:00:00Z', 'creator-1', 0, 0, 0, 0);
                """
            )
            database.execute(
                "INSERT INTO medias VALUES (1, 'new-media', 'new', NULL, ?, ?, 'photo', 'Timeline', 1, 1, '', '', 'creator-1')",
                (str(output), new_file.name),
            )
            database.execute(
                "INSERT INTO medias VALUES (2, 'old-media-1', 'old', NULL, ?, ?, 'photo', 'Timeline', 1, 1, '', '', 'creator-1')",
                (str(output), old_first_file.name),
            )
            database.execute(
                "INSERT INTO medias VALUES (3, 'old-media-2', 'old', NULL, ?, 'old-second.jpg', 'photo', 'Timeline', 0, 1, '', '', 'creator-1')",
                (str(output),),
            )
            database.commit()
            database.close()
            request = {
                "state_dir": str(state),
                "output_dir": str(output),
                "creator": "alice",
                "post_limit": 100,
            }
            emitted_posts: set[str] = set()
            emitted_media: set[str] = set()
            completed_posts: set[str] = set()

            first = io.StringIO()
            with contextlib.redirect_stdout(first):
                onlyfans_bridge.emit_available_posts(
                    request,
                    emitted_posts,
                    emitted_media,
                    completed_posts,
                    [
                        {
                            "id": "old",
                            "media": [{"id": "old-media-2"}, {"id": "old-media-1"}],
                        },
                        {"id": "new", "media": [{"id": "new-media"}]},
                    ],
                )
            first_events = [json.loads(line) for line in first.getvalue().splitlines()]
            self.assertEqual(
                [event["post_id"] for event in first_events if event["event"] == "post_traversed"],
                ["new", "old"],
            )
            self.assertEqual(
                [event["item_key"] for event in first_events if event["event"] == "item"],
                ["new-media", "old-media-1"],
            )
            self.assertEqual(
                [event["post_id"] for event in first_events if event["event"] == "post_complete"],
                ["new"],
            )

            old_file = output / "old-second.jpg"
            old_file.write_bytes(b"old")
            database = sqlite3.connect(database_path)
            database.execute(
                "UPDATE medias SET downloaded = 1 WHERE media_id = 'old-media-2'"
            )
            database.commit()
            database.close()
            second = io.StringIO()
            with contextlib.redirect_stdout(second):
                onlyfans_bridge.emit_available_posts(
                    request,
                    emitted_posts,
                    emitted_media,
                    completed_posts,
                    [
                        {
                            "id": "old",
                            "media": [{"id": "old-media-2"}, {"id": "old-media-1"}],
                        },
                        {"id": "new", "media": [{"id": "new-media"}]},
                    ],
                )
            second_events = [json.loads(line) for line in second.getvalue().splitlines()]
            self.assertEqual(
                [event["post_id"] for event in second_events if event["event"] == "post_traversed"],
                [],
            )
            self.assertEqual(
                [event["item_key"] for event in second_events if event["event"] == "item"],
                ["old-media-2"],
            )
            self.assertEqual(
                [event["post_id"] for event in second_events if event["event"] == "post_complete"],
                ["old"],
            )


if __name__ == "__main__":
    unittest.main()
