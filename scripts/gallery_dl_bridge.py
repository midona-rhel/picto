#!/usr/bin/env python3
import argparse
import collections
import json
import logging
import os
import sys
import traceback
from datetime import datetime, timezone
from typing import Any

_TAGS_LOG = logging.getLogger("picto_bridge.tags")


def _ensure_gallery_dl(module_dir: str | None) -> None:
    if module_dir:
        sys.path.insert(0, module_dir)
    try:
        import gallery_dl  # noqa: F401
    except ImportError as exc:
        raise SystemExit(f"bridge_import_error: {exc}") from exc


def _emit(event_type: str, **payload: Any) -> None:
    record = {"event": event_type}
    record.update(payload)
    sys.stdout.write(json.dumps(record, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def _trim(value: Any) -> str | None:
    if value is None:
        return None
    if isinstance(value, (int, float)):
        value = str(value)
    if not isinstance(value, str):
        return None
    value = value.strip()
    return value or None


def _push_unique(urls: list[str], value: Any) -> None:
    value = _trim(value)
    if value and value not in urls:
        urls.append(value)


def _canonical_site_id(category: str | None) -> str | None:
    if not category:
        return None
    mapping = {
        "pixivuser": "pixiv",
        "twitter": "twitter",
        "x": "twitter",
        "e926": "e621",
        "gelbooru_v02": "gelbooru",
    }
    return mapping.get(category, category)


def _normalize_created_at(raw: Any) -> str | None:
    if isinstance(raw, datetime):
        if raw.tzinfo is None:
            raw = raw.replace(tzinfo=timezone.utc)
        return raw.astimezone(timezone.utc).isoformat()
    raw = _trim(raw)
    if not raw:
        return None
    try:
        return datetime.fromisoformat(raw.replace("Z", "+00:00")).astimezone(
            timezone.utc
        ).isoformat()
    except Exception:
        pass
    for fmt in ("%Y-%m-%d %H:%M:%S%z", "%Y-%m-%d %H:%M:%S", "%Y-%m-%d"):
        try:
            parsed = datetime.strptime(raw, fmt)
            if parsed.tzinfo is None:
                parsed = parsed.replace(tzinfo=timezone.utc)
            return parsed.astimezone(timezone.utc).isoformat()
        except Exception:
            continue
    return raw


def _json_safe(value: Any) -> Any:
    if value is None or isinstance(value, (str, int, float, bool)):
        return value
    if isinstance(value, datetime):
        if value.tzinfo is None:
            value = value.replace(tzinfo=timezone.utc)
        return value.astimezone(timezone.utc).isoformat()
    if isinstance(value, dict):
        return {str(key): _json_safe(val) for key, val in value.items()}
    if isinstance(value, (list, tuple, set)):
        return [_json_safe(item) for item in value]
    return str(value)


def _collect_tags(category: str | None, meta: dict[str, Any]) -> list[list[str]]:
    tags: list[list[str]] = []

    def add(namespace: str, tag: Any) -> None:
        tag = _trim(tag)
        if tag:
            tags.append([namespace, tag])

    def add_many(namespace: str, value: Any) -> None:
        if isinstance(value, str):
            for tag in value.split():
                add(namespace, tag)
        elif isinstance(value, list):
            for entry in value:
                if isinstance(entry, str):
                    add(namespace, entry)
                elif isinstance(entry, dict):
                    add(namespace, entry.get("name"))

    if category in {
        "danbooru",
        "gelbooru",
        "rule34",
        "3dbooru",
        "sankaku",
        "idolcomplex",
        "safebooru",
        "yandere",
        "konachan",
    }:
        fields = (
            ("tags_artist", "creator"),
            ("tags_character", "character"),
            ("tags_copyright", "series"),
            ("tags_general", ""),
            ("tags_meta", "meta"),
            ("tags_metadata", "meta"),
            ("tag_string_artist", "creator"),
            ("tag_string_character", "character"),
            ("tag_string_copyright", "series"),
            ("tag_string_general", ""),
            ("tag_string_meta", "meta"),
            ("tag_string_metadata", "meta"),
        )
        for field, namespace in fields:
            add_many(namespace, meta.get(field))
        if not tags:
            add_many("", meta.get("tag_string"))
            add_many("", meta.get("tags"))
        return tags

    if category == "e621":
        namespace_map = {
            "artist": "creator",
            "character": "character",
            "copyright": "series",
            "general": "",
            "meta": "meta",
            "species": "species",
            "lore": "lore",
        }
        # gallery-dl >= 1.32 flattens the categorized tags object into a
        # plain list and exposes categories as tags_<category> keys.
        for key, namespace in namespace_map.items():
            add_many(namespace, meta.get(f"tags_{key}"))
        if tags:
            return tags
        # Legacy shape (< 1.32): tags is a dict keyed by category.
        raw_tags = meta.get("tags")
        if isinstance(raw_tags, dict):
            for key, value in raw_tags.items():
                add_many(namespace_map.get(key, key), value)
        return tags

    if category == "pixiv":
        add_many("", meta.get("tags"))
        user = meta.get("user")
        if isinstance(user, dict):
            add("creator", user.get("name") or user.get("id"))
        return tags

    add_many("", meta.get("tags"))
    for key in ("artist", "username", "user", "uploader", "blog_name", "user_name", "author", "creator"):
        value = meta.get(key)
        if isinstance(value, dict):
            value = value.get("username") or value.get("name") or value.get("nick")
        added_creator = False
        if isinstance(value, str) and value.strip():
            add("creator", value)
            added_creator = True
        if added_creator:
            break
    return tags


def _tag_summary(tags: list[list[str]]) -> dict[str, Any]:
    summary = {
        "normalized_tag_count": 0,
        "creator_tag_count": 0,
        "character_tag_count": 0,
        "series_tag_count": 0,
        "general_tag_count": 0,
        "meta_tag_count": 0,
        "other_namespaced_tag_count": 0,
    }
    for namespace, _subtag in tags:
        summary["normalized_tag_count"] += 1
        if namespace == "creator":
            summary["creator_tag_count"] += 1
        elif namespace == "character":
            summary["character_tag_count"] += 1
        elif namespace == "series":
            summary["series_tag_count"] += 1
        elif namespace == "meta":
            summary["meta_tag_count"] += 1
        elif namespace:
            summary["other_namespaced_tag_count"] += 1
        else:
            summary["general_tag_count"] += 1
    return summary


def _log_booru_tag_diagnostics(
    stage: str, category: str | None, meta: dict[str, Any], tags: list[list[str]]
) -> None:
    if category not in {
        "danbooru",
        "gelbooru",
        "rule34",
        "3dbooru",
        "sankaku",
        "idolcomplex",
        "safebooru",
        "yandere",
        "konachan",
    }:
        return
    summary = _tag_summary(tags)
    _TAGS_LOG.debug(
        (
            "stage=%s post_id=%s category=%s has_tags_artist=%s "
            "has_tags_character=%s has_tags_copyright=%s has_tags_general=%s "
            "normalized_tag_count=%s creator_tag_count=%s character_tag_count=%s "
            "series_tag_count=%s general_tag_count=%s meta_tag_count=%s "
            "other_namespaced_tag_count=%s tag_preview=%s"
        ),
        stage,
        _post_id(meta) or "?",
        category or "?",
        "tags_artist" in meta,
        "tags_character" in meta,
        "tags_copyright" in meta,
        "tags_general" in meta,
        summary["normalized_tag_count"],
        summary["creator_tag_count"],
        summary["character_tag_count"],
        summary["series_tag_count"],
        summary["general_tag_count"],
        summary["meta_tag_count"],
        summary["other_namespaced_tag_count"],
        tags[:3],
    )


def _canonical_post_url(category: str | None, meta: dict[str, Any]) -> str | None:
    if category == "gelbooru":
        post_id = _trim(meta.get("id"))
        if post_id:
            return (
                "https://gelbooru.com/index.php?page=post&s=view&id="
                f"{post_id}"
            )
    if category == "twitter":
        author = meta.get("author")
        if isinstance(author, dict):
            handle = _trim(author.get("nick")) or _trim(author.get("name"))
            tweet_id = _trim(meta.get("tweet_id"))
            if handle and tweet_id:
                return f"https://x.com/{handle}/status/{tweet_id}"
    for key in ("url", "post_url", "post", "source"):
        value = _trim(meta.get(key))
        if value and not value.startswith("http"):
            continue
        if value:
            return value
    return None


def _media_url(category: str | None, meta: dict[str, Any]) -> str | None:
    if category == "e621":
        file_meta = meta.get("file")
        if isinstance(file_meta, dict):
            return _trim(file_meta.get("url"))
    return _trim(meta.get("file_url")) or _trim(meta.get("media_url"))


def _ordered_source_urls(category: str | None, meta: dict[str, Any]) -> list[str]:
    urls: list[str] = []
    canonical = _canonical_post_url(category, meta)
    media = _media_url(category, meta)
    _push_unique(urls, canonical)
    if category == "e621":
        raw_sources = meta.get("sources")
        if isinstance(raw_sources, list):
            for entry in raw_sources:
                _push_unique(urls, entry)
    else:
        _push_unique(urls, meta.get("source"))
    _push_unique(urls, media)
    return urls


def _page_num(meta: dict[str, Any]) -> int | None:
    value = meta.get("num")
    return int(value) if isinstance(value, int) else None


def _page_count(meta: dict[str, Any]) -> int | None:
    value = meta.get("count", meta.get("page_count"))
    return int(value) if isinstance(value, int) else None


def _post_id(meta: dict[str, Any]) -> str | None:
    for key in ("tweet_id", "id", "index"):
        value = _trim(meta.get(key))
        if value:
            return value
    return None


def _title(meta: dict[str, Any]) -> str | None:
    commentary = meta.get("artist_commentary")
    if isinstance(commentary, dict):
        value = _trim(commentary.get("original_title"))
        if value:
            return value
    for key in ("title", "subject"):
        value = _trim(meta.get(key))
        if value:
            return value
    return None


def _description(meta: dict[str, Any]) -> str | None:
    commentary = meta.get("artist_commentary")
    if isinstance(commentary, dict):
        value = _trim(commentary.get("original_description"))
        if value:
            return value
    for key in ("description", "caption", "body", "content", "substring"):
        value = _trim(meta.get(key))
        if value:
            return value
    return None


def _rating(meta: dict[str, Any]) -> str | None:
    value = meta.get("rating")
    if isinstance(value, (int, float)):
        return str(value)
    return _trim(value)


def _creator_identifier(category: str | None, meta: dict[str, Any]) -> str | None:
    if category == "pixiv":
        user = meta.get("user")
        if isinstance(user, dict):
            return _trim(user.get("name")) or _trim(user.get("id"))
    if category == "twitter":
        author = meta.get("author")
        if isinstance(author, dict):
            return _trim(author.get("name")) or _trim(author.get("nick"))
    for key in ("artist", "username", "user", "uploader", "blog_name"):
        value = _trim(meta.get(key))
        if value:
            return value
    return None


def _normalized_metadata(
    url: str, meta: dict[str, Any], stage: str | None = None
) -> dict[str, Any]:
    category = _canonical_site_id(_trim(meta.get("category")))
    tags = _collect_tags(category, meta)
    if stage:
        _log_booru_tag_diagnostics(stage, category, meta, tags)
    creator = _creator_identifier(category, meta)
    creator_namespace = (
        "creator"
        if category in {
            "danbooru",
            "gelbooru",
            "rule34",
            "3dbooru",
            "sankaku",
            "idolcomplex",
            "safebooru",
            "yandere",
            "konachan",
        }
        else "creator"
    )
    if creator and [creator_namespace, creator] not in tags:
        tags.append([creator_namespace, creator])
    canonical_post_url = _canonical_post_url(category, meta)
    media_url = _media_url(category, meta)
    source_urls = _ordered_source_urls(category, meta)
    source_url = canonical_post_url or (source_urls[0] if source_urls else None)
    post_id = _post_id(meta)
    page_num = _page_num(meta)
    page_count = _page_count(meta)
    item_key = ":".join(
        [
            value
            for value in (
                category or "unknown",
                post_id or canonical_post_url or media_url or url,
                str(page_num) if page_num is not None else "0",
            )
            if value
        ]
    )
    return {
        "tags": tags,
        "description": _description(meta),
        "source_url": source_url,
        "source_urls": source_urls,
        "media_url": media_url,
        "rating": _rating(meta),
        "title": _title(meta),
        "post_id": post_id,
        "created_at": _normalize_created_at(
            meta.get("date")
            or meta.get("created_at")
            or meta.get("create_date")
            or meta.get("published_at")
            or meta.get("published")
            or meta.get("upload_date")
        ),
        "category": category,
        "page_num": page_num,
        "page_count": page_count,
        "canonical_post_url": canonical_post_url,
        "item_key": item_key,
        "raw_metadata": _json_safe(meta),
    }


class _StderrHandler(logging.Handler):
    def emit(self, record: logging.LogRecord) -> None:
        try:
            msg = self.format(record)
        except Exception:
            msg = record.getMessage()
        sys.stderr.write(msg + "\n")
        sys.stderr.flush()


class PictoDownloadJob:
    def __init__(self, url: str):
        from gallery_dl import job

        class _BridgeJob(job.DownloadJob):
            pass

        self._job = _BridgeJob(url)
        self._job.out = _NullOutput()
        self._job.initialize = self._wrap_initialize(self._job.initialize)

    def _wrap_initialize(self, original):
        def wrapped(*args, **kwargs):
            original(*args, **kwargs)
            if not isinstance(self._job.hooks, collections.defaultdict):
                self._job.hooks = collections.defaultdict(list)
            self._job.register_hooks(
                {
                    "prepare": self._safe_hook(self._on_prepare),
                    "after": self._safe_hook(self._on_after),
                    "skip": self._safe_hook(self._on_skip),
                    "error": self._safe_hook(self._on_error),
                }
            )

        return wrapped

    def _safe_hook(self, fn):
        def wrapped(pathfmt):
            try:
                return fn(pathfmt)
            except Exception:
                traceback.print_exc(file=sys.stderr)
                raise

        return wrapped

    def _on_prepare(self, pathfmt):
        meta = _normalized_metadata(
            pathfmt.kwdict.get("url") or "", dict(pathfmt.kwdict), "item_discovered"
        )
        _emit("item_discovered", metadata=meta)

    def _on_after(self, pathfmt):
        meta = _normalized_metadata(
            pathfmt.kwdict.get("url") or "", dict(pathfmt.kwdict), "item_downloaded"
        )
        _emit(
            "item_downloaded",
            file_path=pathfmt.path,
            metadata=meta,
        )

    def _on_skip(self, pathfmt):
        meta = _normalized_metadata(pathfmt.kwdict.get("url") or "", dict(pathfmt.kwdict))
        _emit("item_skipped_archive", metadata=meta)

    def _on_error(self, pathfmt):
        meta = _normalized_metadata(pathfmt.kwdict.get("url") or "", dict(pathfmt.kwdict))
        _emit(
            "item_failed_final",
            metadata=meta,
            file_path=pathfmt.path,
            temp_path=getattr(pathfmt, "temppath", None),
        )

    def run(self) -> int:
        return self._job.run()


class _NullOutput:
    def start(self, *_args, **_kwargs):
        return None

    def success(self, *_args, **_kwargs):
        return None

    def skip(self, *_args, **_kwargs):
        return None

    def progress(self, *_args, **_kwargs):
        return None


def _configure_logging() -> None:
    logging.root.handlers.clear()
    logging.root.setLevel(logging.DEBUG)
    handler = _StderrHandler()
    handler.setFormatter(logging.Formatter("[%(name)s][%(levelname)s] %(message)s"))
    logging.root.addHandler(handler)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--request", required=True)
    args = parser.parse_args()

    with open(args.request, encoding="utf-8") as handle:
        request = json.load(handle)

    _ensure_gallery_dl(request.get("gallery_dl_module_dir"))

    from gallery_dl import config

    _configure_logging()
    config.clear()
    if config_path := request.get("config_path"):
        config.load([config_path], strict=True)
    if request.get("post_range"):
        config.set((), "post-range", request["post_range"])
    if request.get("abort_threshold") is not None:
        config.set((), "skip", f"abort:{request['abort_threshold']}")
    if request.get("archive_path"):
        config.set((), "archive", request["archive_path"])
    if request.get("archive_prefix"):
        config.set((), "archive-prefix", request["archive_prefix"])
    config.set(("output",), "mode", "null")
    config.set(("downloader",), "progress", None)

    _emit(
        "run_started",
        subscription_id=request.get("subscription_id"),
        query_id=request.get("query_id"),
        url=request.get("url"),
    )
    try:
        bridge_job = PictoDownloadJob(request["url"])
        status = bridge_job.run()
    except Exception:
        traceback.print_exc(file=sys.stderr)
        raise
    _emit("run_finished", exit_code=status)
    return int(status or 0)


if __name__ == "__main__":
    raise SystemExit(main())
