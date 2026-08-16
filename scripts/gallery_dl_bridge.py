#!/usr/bin/env python3
import argparse
import json
import logging
import sys
import traceback
from datetime import datetime, timezone
from typing import Any


def _ensure_gallery_dl(module_dir: str | None) -> None:
    if module_dir:
        sys.path.insert(0, module_dir)
    try:
        import gallery_dl  # noqa: F401
    except ImportError as exc:
        raise SystemExit(f"bridge_import_error: {exc}") from exc


def _install_rule34_tag_info_adapter() -> None:
    """Add categorized API tags to gallery-dl's Rule34 page results.

    gallery-dl normally obtains tag categories by scraping every post page.
    Rule34's authenticated API exposes the same categories for an entire page,
    so Picto enriches the extractor result once per page and avoids HTML rate
    limits without replacing gallery-dl's pagination, archive, or downloads.
    """
    from gallery_dl.extractor import gelbooru_v02

    extractor = gelbooru_v02.GelbooruV02Extractor
    if getattr(extractor, "_picto_rule34_tag_info", False):
        return
    original_api_request = extractor._api_request

    def api_request_with_tag_info(self, params):
        root = original_api_request(self, params)
        if self.category != "rule34":
            return root

        url = self.root_api + "/index.php?page=dapi&s=post&q=index"
        json_params = dict(params)
        json_params["json"] = "1"
        json_params["fields"] = "tag_info"
        data = self.request_json(url, params=json_params)
        if isinstance(data, str):
            message = data
        elif isinstance(data, dict) and data.get("success") is False:
            message = data.get("message") or "Unknown API error"
        else:
            message = None
        if message:
            if str(message).lower().startswith("missing authentication"):
                raise self.exc.AuthRequired(
                    "'api-key' & 'user-id'", "the API", message
                )
            raise self.exc.AbortExtraction(f"'{message}'")
        if not isinstance(data, list):
            raise self.exc.AbortExtraction(
                "Rule34 API returned an invalid tag_info response"
            )

        tag_info_by_id = {
            str(post.get("id")): post.get("tag_info") or ()
            for post in data
        }
        type_fields = {
            "artist": "tags_artist",
            "character": "tags_character",
            "copyright": "tags_copyright",
            "metadata": "tags_metadata",
            "tag": "tags_general",
        }
        for post in root:
            categorized = {}
            for tag in tag_info_by_id.get(post.attrib.get("id"), ()):
                field = type_fields.get(tag.get("type"))
                name = tag.get("tag")
                if field and name:
                    categorized.setdefault(field, []).append(name)
            for field, values in categorized.items():
                post.attrib[field] = " ".join(values)
        return root

    extractor._api_request = api_request_with_tag_info
    extractor._picto_rule34_tag_info = True


def _install_sankaku_cursor_adapter() -> None:
    """Expose Sankaku's keyset cursor while gallery-dl keeps ownership."""
    from gallery_dl.extractor import sankaku

    api = sankaku.SankakuAPI
    if getattr(api, "_picto_source_cursor", False):
        return

    original_init = api.__init__
    original_call = api._call

    def init_with_page_size(self, extractor):
        original_init(self, extractor)
        page_size = extractor.config("picto-page-size")
        if page_size:
            extractor.per_page = max(1, min(100, int(page_size)))

    def call_with_cursor(self, endpoint, params=None):
        if endpoint == "/v2/posts/keyset":
            params = dict(params or {})
            if "next" not in params:
                cursor = self.extractor.config("picto-next")
                if cursor:
                    params["next"] = cursor

        data = original_call(self, endpoint, params)
        if endpoint == "/v2/posts/keyset" and isinstance(data, dict):
            items = data.get("data") or ()
            cursor = (data.get("meta") or {}).get("next")
            _emit("source_cursor", cursor=cursor, item_count=len(items))
        return data

    api.__init__ = init_with_page_size
    api._call = call_with_cursor
    api._picto_source_cursor = True


def _install_deviantart_deviation_adapter() -> None:
    """Expand profile results through gallery-dl's deviation extractor.

    DeviantArt's profile API returns only the primary file for a deviation.
    gallery-dl's direct-deviation extractor already expands additional media,
    so queue each discovered profile result into that existing extractor.
    """
    from gallery_dl.extractor import deviantart

    gallery = deviantart.DeviantartGalleryExtractor
    deviation = deviantart.DeviantartDeviationExtractor
    if getattr(gallery, "_picto_expand_deviations", False):
        return

    original_gallery_deviations = gallery.deviations
    def gallery_deviations_as_children(self):
        mature = self.config("mature", "true")
        public_only = mature is False or str(mature).lower() in (
            "0",
            "false",
            "no",
            "off",
        )
        for item in original_gallery_deviations(self):
            if isinstance(item, tuple):
                yield item
                continue
            if item.get("is_deleted") or item.get("tier_access") == "locked":
                continue
            # DeviantArt includes mature placeholders even with
            # mature_content=false. Their CDN token returns 403 anonymously,
            # so they are outside the public-only source contract.
            if public_only and item.get("is_mature"):
                self.log.info(
                    "Skipping mature deviation %s in public-only mode",
                    item.get("deviationid", "?"),
                )
                continue
            url = item.get("url")
            if not url:
                deviation_id = item.get("deviationid")
                if not deviation_id:
                    continue
                url = f"https://www.deviantart.com/view/{deviation_id}/"
            item["_extractor"] = deviation
            yield url, item

    gallery.deviations = gallery_deviations_as_children
    gallery._picto_expand_deviations = True


def _install_tumblr_post_adapter() -> None:
    """Limit whole Tumblr posts and expose the durable source cursor."""
    from gallery_dl.extractor import tumblr

    api = tumblr.TumblrAPI
    if getattr(api, "_picto_post_cursor", False):
        return
    original_posts = api.posts

    def posts_with_limit_and_cursor(self, blog, params):
        limit = self.extractor.config("picto-post-limit")
        limit = max(0, int(limit)) if limit else None
        start_offset = max(0, int(self.extractor.config("offset") or 0))
        emitted = 0
        try:
            for post in original_posts(self, blog, params):
                if limit is not None and emitted >= limit:
                    break
                emitted += 1
                yield post
        finally:
            # Tumblr's API offset counts every source post, including posts
            # without supported image media. Rust must resume from this raw
            # offset rather than from the smaller imported-media count.
            _emit(
                "source_cursor",
                cursor=str(start_offset + emitted),
                item_count=emitted,
            )

    api.posts = posts_with_limit_and_cursor
    api._picto_post_cursor = True


def _emit(event_type: str, **payload: Any) -> None:
    record = {"event": event_type}
    record.update(payload)
    sys.stdout.write(json.dumps(record, ensure_ascii=False) + "\n")
    sys.stdout.flush()


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


def _item_url(pathfmt) -> Any:
    return _json_safe(pathfmt.kwdict.get("url") or pathfmt.kwdict.get("_url"))


class _StderrHandler(logging.Handler):
    def emit(self, record: logging.LogRecord) -> None:
        try:
            msg = self.format(record)
        except Exception:
            msg = record.getMessage()
        sys.stderr.write(msg + "\n")
        sys.stderr.flush()


class _HookRegistry(dict):
    def __missing__(self, key):
        callbacks = []
        self[key] = callbacks
        return callbacks


class PictoDownloadJob:
    def __init__(self, url: str):
        from gallery_dl import job

        bridge = self

        # Hooks must be registered at CLASS level: dispatching extractors
        # (twitter user → timeline, furaffinity gallery/scraps, ...) spawn
        # CHILD jobs via `self.__class__(extr, self)`. Instance-level hook
        # registration on the parent leaves children silent — their downloads
        # succeed on disk but no item events reach Picto, so nothing ingests.
        class _BridgeJob(job.DownloadJob):
            def initialize(self, *args, **kwargs):
                super().initialize(*args, **kwargs)
                self.out = _NullOutput()
                self.hooks = _HookRegistry(self.hooks)
                self.register_hooks(
                    {
                        "prepare": bridge._safe_hook(bridge._on_prepare),
                        "after": bridge._safe_hook(bridge._on_after),
                        "skip": bridge._safe_hook(bridge._on_skip),
                        "error": bridge._safe_hook(bridge._on_error),
                    }
                )

        self._job = _BridgeJob(url)
        self._job.out = _NullOutput()

    def _safe_hook(self, fn):
        def wrapped(pathfmt):
            try:
                return fn(pathfmt)
            except Exception:
                traceback.print_exc(file=sys.stderr)
                raise

        return wrapped

    def _on_prepare(self, pathfmt):
        _emit(
            "item_discovered",
            item_url=_item_url(pathfmt),
            metadata=_json_safe(dict(pathfmt.kwdict)),
        )

    def _on_after(self, pathfmt):
        _emit(
            "item_downloaded",
            file_path=pathfmt.path,
            item_url=_item_url(pathfmt),
            metadata=_json_safe(dict(pathfmt.kwdict)),
        )

    def _on_skip(self, pathfmt):
        _emit(
            "item_skipped_archive",
            item_url=_item_url(pathfmt),
            metadata=_json_safe(dict(pathfmt.kwdict)),
        )

    def _on_error(self, pathfmt):
        _emit(
            "item_failed_final",
            item_url=_item_url(pathfmt),
            metadata=_json_safe(dict(pathfmt.kwdict)),
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
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--request")
    mode.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        _ensure_gallery_dl(None)
        _install_rule34_tag_info_adapter()
        _install_deviantart_deviation_adapter()
        _install_tumblr_post_adapter()
        import gallery_dl
        from gallery_dl.extractor import deviantart
        from gallery_dl.extractor import gelbooru_v02
        from gallery_dl.extractor import tumblr

        _emit(
            "bridge_self_test",
            gallery_dl_version=gallery_dl.__version__,
            gallery_dl_imported=True,
            rule34_adapter_initialized=bool(getattr(
                gelbooru_v02.GelbooruV02Extractor,
                "_picto_rule34_tag_info",
                False,
            )),
            deviantart_adapter_initialized=bool(getattr(
                deviantart.DeviantartGalleryExtractor,
                "_picto_expand_deviations",
                False,
            )),
            tumblr_adapter_initialized=bool(getattr(
                tumblr.TumblrAPI,
                "_picto_post_cursor",
                False,
            )),
        )
        return 0

    with open(args.request, encoding="utf-8") as handle:
        request = json.load(handle)

    _ensure_gallery_dl(request.get("gallery_dl_module_dir"))
    if request.get("site_id") == "rule34":
        _install_rule34_tag_info_adapter()
    if request.get("site_id") in ("idolcomplex", "sankaku"):
        _install_sankaku_cursor_adapter()
    if request.get("site_id") == "deviantart":
        _install_deviantart_deviation_adapter()
    if request.get("site_id") == "tumblr":
        _install_tumblr_post_adapter()

    from gallery_dl import config

    _configure_logging()
    config.clear()
    if config_path := request.get("config_path"):
        config.load([config_path], strict=True)
    if request.get("post_range"):
        config.set((), "post-range", request["post_range"])
    if request.get("child_range"):
        config.set((), "child-range", request["child_range"])
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
