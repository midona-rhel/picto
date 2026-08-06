import unittest
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))

import gallery_dl_bridge


class GelbooruNormalizationTests(unittest.TestCase):
    def test_gelbooru_prefers_post_url_and_metadata_date(self) -> None:
        meta = {
            "category": "gelbooru_v02",
            "id": 13753751,
            "file_url": "https://img2.gelbooru.com/images/b8/8c/example.jpg",
            "source": "https://x.com/eguardx_/status/1646952886658560002",
            "date": "2026-03-30T03:30:30+00:00",
            "created_at": "Sun Mar 29 22:30:30 -0500 2026",
            "tags_character": "princess_peach",
            "tags_general": "dress",
            "tags_metadata": "highres",
        }

        normalized = gallery_dl_bridge._normalized_metadata(  # noqa: SLF001
            "https://gelbooru.com/index.php?page=post&s=list&tags=princess_peach+dress",
            meta,
        )

        self.assertEqual(
            normalized["canonical_post_url"],
            "https://gelbooru.com/index.php?page=post&s=view&id=13753751",
        )
        self.assertEqual(
            normalized["source_urls"],
            [
                "https://gelbooru.com/index.php?page=post&s=view&id=13753751",
                "https://x.com/eguardx_/status/1646952886658560002",
                "https://img2.gelbooru.com/images/b8/8c/example.jpg",
            ],
        )
        self.assertEqual(normalized["source_url"], normalized["canonical_post_url"])
        self.assertEqual(normalized["created_at"], "2026-03-30T03:30:30+00:00")
        self.assertIn(["meta", "highres"], normalized["tags"])
        self.assertIn(["character", "princess_peach"], normalized["tags"])
        self.assertIn(["", "dress"], normalized["tags"])

    def test_gelbooru_uses_booru_tag_namespaces(self) -> None:
        meta = {
            "category": "gelbooru_v02",
            "id": 1,
            "tags_artist": "foo_artist",
            "tags_character": "princess_peach",
            "tags_copyright": "mario_(series)",
            "tags_general": "dress",
            "tags_metadata": "highres",
            "artist": "foo_artist",
        }

        normalized = gallery_dl_bridge._normalized_metadata(  # noqa: SLF001
            "https://gelbooru.com/index.php?page=post&s=view&id=1",
            meta,
        )

        # Booru categories map to canonical picto namespaces.
        self.assertIn(["creator", "foo_artist"], normalized["tags"])
        self.assertIn(["character", "princess_peach"], normalized["tags"])
        self.assertIn(["series", "mario_(series)"], normalized["tags"])
        self.assertIn(["meta", "highres"], normalized["tags"])
        self.assertNotIn(["artist", "foo_artist"], normalized["tags"])
        self.assertNotIn(["copyright", "mario_(series)"], normalized["tags"])


class PixivNormalizationTests(unittest.TestCase):
    def test_user_and_search_imports_prefer_the_artwork_page(self) -> None:
        meta = {
            "category": "pixiv",
            "id": 114223105,
            "url": "https://i.pximg.net/img-original/example_p0.png",
            "tags": ["original"],
            "user": {"id": 1234, "name": "Artist"},
        }

        for query_url in (
            "https://www.pixiv.net/en/users/1234/artworks",
            "https://www.pixiv.net/en/tags/original/artworks?s_mode=s_tag",
        ):
            with self.subTest(query_url=query_url):
                normalized = gallery_dl_bridge._normalized_metadata(  # noqa: SLF001
                    query_url,
                    meta,
                )
                post_url = "https://www.pixiv.net/en/artworks/114223105"
                self.assertEqual(normalized["canonical_post_url"], post_url)
                self.assertEqual(normalized["source_url"], post_url)
                self.assertEqual(
                    normalized["source_urls"],
                    [post_url, "https://i.pximg.net/img-original/example_p0.png"],
                )


if __name__ == "__main__":
    unittest.main()
