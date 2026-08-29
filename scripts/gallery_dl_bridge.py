#!/usr/bin/env python3
import argparse
import json
import logging
import sys
import time
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
                self.log.warning(
                    "Rule34 categorized tags require authentication; "
                    "continuing with gallery-dl's normal tags"
                )
                return root
            self.log.warning(
                "Rule34 categorized tag lookup failed (%s); "
                "continuing with gallery-dl's normal tags",
                message,
            )
            return root
        if not isinstance(data, list):
            self.log.warning(
                "Rule34 categorized tag lookup returned invalid output; "
                "continuing with gallery-dl's normal tags"
            )
            return root

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
    """Expose whole deviations and a durable source cursor.

    The gallery extractor already emits every supported target in a deviation.
    Re-queuing each result through the direct-deviation extractor repeats the
    page, deviation, and metadata requests and merely rediscovers the same
    archived file.
    """
    from gallery_dl.extractor import deviantart

    gallery = deviantart.DeviantartGalleryExtractor
    if getattr(gallery, "_picto_expand_deviations", False):
        return

    original_gallery_deviations = gallery.deviations

    def gallery_deviations_as_children(self):
        initial_skip = max(0, int(self.config("picto-post-skip") or 0))
        skip = initial_skip
        emitted = 0
        exhausted = False
        try:
            for item in original_gallery_deviations(self):
                if skip:
                    skip -= 1
                    continue
                if isinstance(item, tuple):
                    yield item
                    emitted += 1
                    continue

                deviation_id = item.get("deviationid", "?")
                if item.get("is_deleted") or item.get("tier_access") == "locked":
                    deferred = dict(item)
                    deferred.setdefault("category", "deviantart")
                    deferred["_picto_access"] = "deferred"
                    _emit("post_traversed", metadata=deferred)
                    self.log.warning(
                        "Skipping post %s (currently inaccessible)",
                        deviation_id,
                    )
                    emitted += 1
                    continue

                yield item
                emitted += 1
            exhausted = True
        finally:
            _emit(
                "source_cursor",
                cursor="" if exhausted else f"range:{initial_skip + emitted + 1}",
                item_count=emitted,
            )

    gallery.deviations = gallery_deviations_as_children
    gallery._picto_expand_deviations = True


def _install_early_post_window(extractor) -> None:
    """Skip source post IDs before extractors request their detail pages."""
    if getattr(extractor, "_picto_source_posts", False):
        return
    original_items = extractor.items

    def items_in_source_batches(self):
        source_posts = self.posts
        skip = max(0, int(self.config("picto-post-skip") or 0))
        start = skip
        emitted = 0
        exhausted = False

        def bounded_posts():
            nonlocal skip, emitted, exhausted
            for post in source_posts():
                if skip:
                    skip -= 1
                    continue
                yield post
                emitted += 1
            exhausted = True

        self.posts = bounded_posts
        try:
            yield from original_items(self)
        finally:
            del self.posts
            cursor = "" if exhausted else f"range:{start + emitted + 1}"
            _emit("source_cursor", cursor=cursor, item_count=emitted)

    extractor.items = items_in_source_batches
    extractor._picto_source_posts = True


def _install_detail_page_post_adapters(site_id: str) -> None:
    """Install early source windows for sites that resolve one page per post."""
    if site_id == "furaffinity":
        from gallery_dl.extractor import furaffinity

        _install_early_post_window(furaffinity.FuraffinityExtractor)
    elif site_id == "hentaifoundry":
        from gallery_dl.extractor import hentaifoundry

        _install_early_post_window(hentaifoundry.HentaifoundryExtractor)
    elif site_id == "newgrounds":
        from gallery_dl.extractor import newgrounds

        _install_early_post_window(newgrounds.NewgroundsExtractor)


def _install_tumblr_post_adapter() -> None:
    """Limit whole Tumblr posts and expose the durable source cursor."""
    from gallery_dl.extractor import tumblr

    api = tumblr.TumblrAPI
    if getattr(api, "_picto_post_cursor", False):
        return
    original_posts = api.posts

    def posts_with_limit_and_cursor(self, blog, params):
        start_offset = max(0, int(self.extractor.config("offset") or 0))
        emitted = 0
        try:
            for post in original_posts(self, blog, params):
                yield post
                emitted += 1
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


def _install_fanbox_pagination_compatibility() -> None:
    """Accept FANBOX's current pagination envelope in gallery-dl.

    FANBOX added named fields inside several response bodies after gallery-dl
    1.32.9 shipped. Keep gallery-dl in charge of extraction and only unwrap
    the response values its extractor already expects.
    """
    from gallery_dl import version
    from gallery_dl.extractor import fanbox

    # This is only needed by the development 1.32.9 wheel. The pinned
    # Codeberg source already consumes FANBOX's named response fields.
    if version.__version__ != "1.32.9":
        return

    extractor = fanbox.FanboxCreatorExtractor
    if getattr(extractor, "_picto_pagination_envelope", False):
        return

    original_request_json = extractor.request_json

    def request_json_with_pagination_envelope(self, url, *args, **kwargs):
        response = original_request_json(self, url, *args, **kwargs)
        body = response.get("body") if isinstance(response, dict) else None
        if not isinstance(body, dict):
            return response
        envelopes = (
            ("/post.paginateCreator", "pageUrls"),
            ("/post.listCreator", "posts"),
            ("/post.info", "post"),
            ("/plan.listCreator", "plans"),
        )
        for endpoint, field in envelopes:
            if endpoint in url and field in body:
                response = dict(response)
                response["body"] = body[field]
                break
        return response

    extractor.request_json = request_json_with_pagination_envelope
    extractor._picto_pagination_envelope = True


def _install_fanbox_transport() -> None:
    """Use gallery-dl's proposed browser-TLS transport for FANBOX only.

    FANBOX accepts the captured browser session but rejects Python requests'
    TLS fingerprint. This is the transport from gallery-dl Codeberg PR #192;
    extraction, pagination, retries, and download ownership remain gallery-dl's.
    """
    from gallery_dl import exception, util
    from gallery_dl.extractor import fanbox
    from gallery_dl.extractor.common import Extractor
    from gallery_dl_curl_cffi import CurlCffiSessionWrapper

    extractor = fanbox.FanboxExtractor
    if getattr(extractor, "_picto_curl_cffi_transport", False):
        return
    # Stop patching once gallery-dl ships a FANBOX-specific session itself.
    if extractor._init_session is not Extractor._init_session:
        return

    def init_session(self):
        try:
            browser = self.config("browser") or self.browser
            if browser and isinstance(browser, str):
                browser = browser.lower().partition(":")[0]
            if browser not in ("firefox", "chrome"):
                browser = "firefox"

            proxy = self.config("proxy")
            if proxy:
                proxy = util.build_proxy_map(proxy, self.log)

            self.session = CurlCffiSessionWrapper(
                impersonate=browser,
                proxy=proxy,
                trust_env=bool(self.config("proxy-env", True)),
                requests_hosts=("downloads.fanbox.cc",),
            )
        except ImportError as exc:
            raise exception.StopExtraction(
                "curl_cffi is required for FANBOX subscriptions"
            ) from exc

        headers = self.session.headers
        if referer := self.config("referer", self.referer):
            headers["Referer"] = referer if isinstance(referer, str) else self.root + "/"
        if custom_headers := self.config("headers"):
            if isinstance(custom_headers, dict):
                headers.update(custom_headers)

    extractor._init_session = init_session
    extractor._picto_curl_cffi_transport = True


def _install_patreon_attachment_adapter() -> None:
    """Let the downloader resolve Patreon attachment redirects on demand.

    gallery-dl normally resolves every attachment URL before checking its
    archive. That turns a scan of already handled posts into one network
    request per attachment. The downloader already follows redirects, so
    yielding the original URL preserves downloads while archive hits remain
    local.
    """
    from gallery_dl import text
    from gallery_dl.extractor import patreon

    extractor = patreon.PatreonExtractor
    if getattr(extractor, "_picto_subscription_adapter", False):
        return

    original_finalize = extractor.finalize

    def pagination_in_source_batches(self, url):
        headers = {"Content-Type": "application/vnd.api+json"}
        skip = max(0, int(self.config("picto-post-skip") or 0))
        self._picto_source_exhausted = False

        while url:
            self._update_cursor(url)
            url = text.ensure_http_scheme(url)
            page = self.request_json(url, headers=headers)
            included = self._transform(page.get("included") or ())
            for post in page.get("data") or ():
                if skip:
                    skip -= 1
                    continue
                yield self._process(post, included)

            url = (page.get("links") or {}).get("next")

        self._picto_source_exhausted = True
        self._update_cursor("")

    def attachments_without_preflight(self, post):
        for attachment in post.get("attachments") or ():
            if url := attachment.get("url"):
                yield "attachment", attachment, url, attachment["name"]

        for attachment in post.get("attachments_media") or ():
            if url := attachment.get("download_url"):
                yield "attachment", attachment, url, attachment["file_name"]

    def filename_without_preflight(self, url):
        return text.filename_from_url(url)

    def finalize_with_cursor(self, status):
        if getattr(self, "_picto_source_exhausted", False):
            cursor = ""
        else:
            # A successful-post gate can stop in the middle of an API page.
            # Reopening that page is safe because gallery-dl's archive skips
            # files already accepted by Picto.
            cursor = self._cursor or "patreon:first-page"
        _emit("source_cursor", cursor=cursor)
        return original_finalize(self, status)

    extractor._attachments = attachments_without_preflight
    extractor._filename = filename_without_preflight
    extractor._pagination = pagination_in_source_batches
    extractor.finalize = finalize_with_cursor
    extractor._picto_lazy_attachments = True
    extractor._picto_subscription_adapter = True


def _install_subscribestar_post_adapter() -> None:
    """Apply Picto's range to SubscribeStar posts, never attachment files."""
    from gallery_dl import text
    from gallery_dl.extractor import subscribestar

    extractor = subscribestar.SubscribestarExtractor
    if getattr(extractor, "_picto_source_posts", False):
        return

    def pagination_in_source_batches(self, url, params=None):
        needle_next_page = 'data-role="infinite_scroll-next_page" href="'
        page = self.request(url, params=params).text
        skip = max(0, int(self.config("picto-post-skip") or 0))
        emitted = 0
        start = skip
        exhausted = False
        try:
            while True:
                posts = page.split('<div class="post ')[1:]
                if not posts:
                    exhausted = True
                    return
                for post in posts:
                    if skip:
                        skip -= 1
                        continue
                    yield post
                    emitted += 1

                next_url = text.extr(posts[-1], needle_next_page, '"')
                if not next_url:
                    exhausted = True
                    return
                page = self.request_json(
                    self.root + text.unescape(next_url)
                )["html"]
        finally:
            cursor = "" if exhausted else f"range:{start + emitted + 1}"
            _emit("source_cursor", cursor=cursor, item_count=emitted)

    extractor._pagination = pagination_in_source_batches
    extractor._picto_source_posts = True


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


def _event_metadata(pathfmt) -> dict[str, Any]:
    """Return source metadata without gallery-dl's private transport payloads.

    Some extractors attach complete HTML documents to ``_page``. Sending that
    document on every hook can turn one downloaded item into several megabytes
    of NDJSON and delay ingestion behind data Picto never reads.
    """
    metadata = dict(pathfmt.kwdict)
    metadata.pop("_page", None)
    if isinstance(parent := metadata.get("_parent"), dict):
        parent = dict(parent)
        parent.pop("_page", None)
        metadata["_parent"] = parent
    return _json_safe(metadata)


def _item_url(pathfmt) -> Any:
    return _json_safe(pathfmt.kwdict.get("url") or pathfmt.kwdict.get("_url"))


def _post_identity(metadata: dict[str, Any]) -> str:
    category = str(metadata.get("category") or "")
    parent = metadata.get("_parent")
    parent = parent if isinstance(parent, dict) else {}
    fields = {
        "artstation": ("hash_id", "project_hash_id", "project_id"),
        "deviantart": ("deviationid",),
        "ehentai": ("gid", "gallery_id"),
        "exhentai": ("gid", "gallery_id"),
        "hentaifoundry": ("index", "id"),
        "newgrounds": ("index", "id"),
        "twitter": ("tweet_id", "id"),
        "subscribestar": ("post_id", "id"),
    }.get(category, ("id", "post_id"))
    for source in (metadata, parent):
        for field in fields:
            value = source.get(field)
            if value is not None and str(value).strip():
                return f"{category}:{field}:{value}"
    for field in ("post_url", "canonical_url", "url"):
        value = metadata.get(field)
        if value is not None and str(value).strip():
            return f"{category}:{field}:{value}"
    return f"{category}:metadata:{json.dumps(metadata, sort_keys=True, default=str)}"


class _AcceptedPostGate:
    def __init__(self, limit: int | None, accepted_extensions: list[str] | None):
        self.limit = max(0, int(limit or 0))
        self.accepted_extensions = {
            str(extension).lower().lstrip(".")
            for extension in (accepted_extensions or ())
        }
        self.traversed: set[str] = set()
        self.reserved: set[str] = set()
        self.accepted: set[str] = set()
        self.current: str | None = None
        self.current_has_media = False

    def begin(self, metadata: dict[str, Any]) -> tuple[bool, str]:
        identity = _post_identity(metadata)
        if identity in self.traversed:
            return False, identity
        if self.limit and len(self.accepted) >= self.limit:
            from gallery_dl import exception

            self.current = None
            raise exception.StopExtraction()
        self.traversed.add(identity)
        self.current = identity
        self.current_has_media = False
        return True, identity

    def prepare(self, path: str) -> None:
        path = str(path)
        extension = path.rsplit(".", 1)[-1].lower() if "." in path else ""
        if self.accepted_extensions and extension not in self.accepted_extensions:
            return
        if self.current is not None:
            self.reserved.add(self.current)

    def downloaded(self, path: str, metadata: dict[str, Any]) -> None:
        path = str(path)
        extension = path.rsplit(".", 1)[-1].lower() if "." in path else ""
        if self.accepted_extensions and extension not in self.accepted_extensions:
            return
        if self.current is not None:
            self.current_has_media = True

    def has_current(self) -> bool:
        return self.current is not None

    def complete(self) -> None:
        if self.current is not None and self.current_has_media:
            self.accepted.add(self.current)
        elif self.current is not None:
            self.reserved.discard(self.current)
        self.current = None
        self.current_has_media = False


_recent_download_errors: list[str] = []


class _StderrHandler(logging.Handler):
    def emit(self, record: logging.LogRecord) -> None:
        try:
            msg = self.format(record)
        except Exception:
            msg = record.getMessage()
        if record.levelno >= logging.WARNING:
            _recent_download_errors.append(record.getMessage())
            del _recent_download_errors[:-3]
        _emit(
            "gallery_log",
            logger=record.name,
            level=record.levelname.lower(),
            message=record.getMessage(),
        )
        sys.stderr.write(msg + "\n")
        sys.stderr.flush()


class _HookRegistry(dict):
    def __missing__(self, key):
        callbacks = []
        self[key] = callbacks
        return callbacks


_DOMAIN_INTERVAL_SECONDS = 1.0
_domain_slot_lock = __import__("threading").Lock()
_next_request_monotonic: dict[str, float] = {}


def _host_state_name(host: str) -> str:
    return "".join(c if c.isalnum() or c in ".-" else "_" for c in host) + ".slot"


def _load_host_slot(state_dir: str, host: str, now: float) -> float | None:
    import os

    try:
        with open(
            os.path.join(state_dir, _host_state_name(host)), encoding="utf-8"
        ) as handle:
            stored = float(handle.read().strip())
    except (OSError, ValueError):
        return None
    if stored > now + 60.0:
        # A monotonic value from a previous boot; unrelated to this clock.
        return None
    return stored


def _store_host_slot(state_dir: str, host: str, slot: float) -> None:
    import os

    try:
        os.makedirs(state_dir, exist_ok=True)
        with open(
            os.path.join(state_dir, _host_state_name(host)), "w", encoding="utf-8"
        ) as handle:
            handle.write(f"{slot:.3f}")
    except OSError:
        pass


def _pace_domain_request(host: str, now: float, sleep=None, state_dir=None) -> float:
    """Reserve the next send slot for the host, then sleep the remainder, so
    consecutive requests to one host are at least one second apart whichever
    gallery-dl stream — extractor, page fetch, or media download — issued
    them, including concurrent callers.

    gallery-dl paces its extractor requests (`sleep-request`) and its media
    downloads (`sleep`) with two independent clocks, so their interleavings
    can put two requests to the same host inside one second. This limiter at
    the real HTTP boundary is the policy's guarantee; the gallery-dl options
    stay on as defense in depth and never add delay beyond the remainder.

    A bridge process runs one source window; `state_dir` carries each host's
    reserved slot across consecutive processes so a fresh process cannot
    request inside the previous process's interval.
    """
    import time as _time

    sleep = sleep or _time.sleep
    with _domain_slot_lock:
        if state_dir and host not in _next_request_monotonic:
            stored = _load_host_slot(state_dir, host, now)
            if stored is not None:
                _next_request_monotonic[host] = stored
        slot = max(now, _next_request_monotonic.get(host, now))
        _next_request_monotonic[host] = slot + _DOMAIN_INTERVAL_SECONDS
        if state_dir:
            _store_host_slot(state_dir, host, slot + _DOMAIN_INTERVAL_SECONDS)
    if slot > now:
        sleep(slot - now)
    return slot


def _install_request_pacing_and_trace(
    trace_path: str | None, state_dir: str | None
) -> None:
    """Enforce the per-domain interval and optionally record every request's
    host and timestamp as certification evidence."""
    import os
    import time
    import urllib.parse

    import requests

    original = requests.Session.request

    def paced(self, method, url, *args, **kwargs):
        host = urllib.parse.urlsplit(str(url)).hostname or ""
        sent_at = _pace_domain_request(host, time.monotonic(), state_dir=state_dir)
        if trace_path:
            entry = {
                "ts_ms": int(time.time() * 1000),
                "monotonic_ms": int(sent_at * 1000),
                "host": host,
                "method": str(method).upper(),
                "pid": os.getpid(),
            }
            with open(trace_path, "a", encoding="utf-8") as handle:
                handle.write(json.dumps(entry) + "\n")
        return original(self, method, url, *args, **kwargs)

    requests.Session.request = paced


def _configure_source_window(config, request: dict[str, Any]) -> None:
    """Install the durable Rust cursor and batch size into extractor config."""
    if request.get("post_limit") is not None:
        config.set((), "picto-post-limit", request["post_limit"])
        config.set((), "picto-page-size", request["post_limit"])
    range_start = max(1, int(request.get("range_start") or 1))
    if source_cursor := request.get("source_cursor"):
        config.set((), "picto-post-skip", 0)
        if source_cursor != "patreon:first-page":
            config.set((), "picto-source-cursor", source_cursor)
            config.set((), "picto-next", source_cursor)
        if request.get("site_id") == "tumblr":
            config.set((), "offset", source_cursor)
    else:
        config.set((), "picto-post-skip", range_start - 1)
        if request.get("site_id") == "tumblr":
            config.set((), "offset", range_start - 1)


class PictoDownloadJob:
    def __init__(
        self,
        url: str,
        post_limit: int | None = None,
        accepted_extensions: list[str] | None = None,
        post_terminal_mode: str | None = None,
    ):
        from gallery_dl import job

        bridge = self
        self._post_gate = _AcceptedPostGate(post_limit, accepted_extensions)
        self._post_terminal_mode = post_terminal_mode

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
                        "post": bridge._safe_hook(bridge._on_post),
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
            except Exception as error:
                from gallery_dl import exception

                if isinstance(error, exception.ControlException):
                    raise
                traceback.print_exc(file=sys.stderr)
                raise

        return wrapped

    def _on_prepare(self, pathfmt):
        _recent_download_errors.clear()
        self._post_gate.prepare(pathfmt.path)
        _emit(
            "item_discovered",
            item_url=_item_url(pathfmt),
            metadata=_event_metadata(pathfmt),
        )

    def _on_post(self, pathfmt):
        metadata = _event_metadata(pathfmt)
        identity = _post_identity(metadata)
        if self._post_gate.has_current() and self._post_gate.current != identity:
            self._acknowledge_current_post()
        first_visit, _ = self._post_gate.begin(metadata)
        if first_visit:
            _emit("post_traversed", metadata=metadata)

    def _acknowledge_current_post(self):
        if not self._post_gate.has_current():
            return
        _emit("post_complete")
        # Rust acknowledges only after every file from this post has reached
        # canonical library state.
        if not sys.stdin.readline():
            from gallery_dl import exception

            raise exception.StopExtraction()
        self._post_gate.complete()

    def _on_after(self, pathfmt):
        metadata = _event_metadata(pathfmt)
        self._post_gate.downloaded(pathfmt.path, metadata)
        _emit(
            "item_downloaded",
            file_path=pathfmt.path,
            item_url=_item_url(pathfmt),
            metadata=metadata,
        )
        if self._is_terminal_item(metadata):
            self._acknowledge_current_post()

    def _on_skip(self, pathfmt):
        metadata = _event_metadata(pathfmt)
        _emit(
            "item_skipped_archive",
            item_url=_item_url(pathfmt),
            metadata=metadata,
        )
        if self._is_terminal_item(metadata):
            self._acknowledge_current_post()

    def _on_error(self, pathfmt):
        metadata = _event_metadata(pathfmt)
        messages = list(dict.fromkeys(_recent_download_errors))
        _emit(
            "item_failed_final",
            item_url=_item_url(pathfmt),
            metadata=metadata,
            file_path=pathfmt.path,
            temp_path=getattr(pathfmt, "temppath", None),
            error_message="; ".join(messages) or None,
        )
        if self._is_terminal_item(metadata):
            self._acknowledge_current_post()

    def _is_terminal_item(self, metadata: dict[str, Any]) -> bool:
        mode = self._post_terminal_mode
        if mode == "single":
            return True
        if mode not in ("count-one", "count-zero"):
            return False
        parent = metadata.get("_parent")
        sources = (metadata, parent if isinstance(parent, dict) else {})
        for source in sources:
            count = source.get("count") or source.get("page_count")
            num = source.get("num")
            try:
                count = int(count)
                num = int(num)
            except (TypeError, ValueError):
                continue
            if count < 1:
                continue
            return num >= count if mode == "count-one" else num + 1 >= count
        return False

    def run(self) -> int:
        status = self._job.run()
        self._acknowledge_current_post()
        return status


class _NullOutput:
    def __init__(self):
        self._last_progress_at = 0.0

    def start(self, *_args, **_kwargs):
        self._last_progress_at = 0.0
        return None

    def success(self, *_args, **_kwargs):
        return None

    def skip(self, *_args, **_kwargs):
        return None

    def progress(self, bytes_total, bytes_downloaded, bytes_per_second):
        now = time.monotonic()
        if now - self._last_progress_at < 0.5:
            return None
        self._last_progress_at = now
        _emit(
            "download_progress",
            bytes_total=bytes_total,
            bytes_downloaded=bytes_downloaded,
            bytes_per_second=bytes_per_second,
        )
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
        _install_fanbox_transport()
        _install_fanbox_pagination_compatibility()
        _install_patreon_attachment_adapter()
        _install_subscribestar_post_adapter()
        import gallery_dl
        import yt_dlp
        from gallery_dl.extractor import deviantart
        from gallery_dl.extractor import fanbox
        from gallery_dl.extractor import gelbooru_v02
        from gallery_dl.extractor import patreon
        from gallery_dl.extractor import tumblr
        from gallery_dl.extractor import subscribestar

        _emit(
            "bridge_self_test",
            gallery_dl_version=gallery_dl.__version__,
            gallery_dl_imported=True,
            yt_dlp_version=yt_dlp.version.__version__,
            yt_dlp_imported=True,
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
            fanbox_transport_initialized=bool(
                getattr(fanbox.FanboxExtractor, "_picto_curl_cffi_transport", False)
                or fanbox.FanboxExtractor._init_session.__module__.endswith("curl_cffi_shim")
            ),
            patreon_adapter_initialized=bool(getattr(
                patreon.PatreonExtractor,
                "_picto_subscription_adapter",
                False,
            )),
            subscribestar_adapter_initialized=bool(getattr(
                subscribestar.SubscribestarExtractor,
                "_picto_source_posts",
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
    _install_detail_page_post_adapters(request.get("site_id"))
    if request.get("site_id") == "tumblr":
        _install_tumblr_post_adapter()
    if request.get("site_id") == "fanbox":
        _install_fanbox_transport()
        _install_fanbox_pagination_compatibility()
    if request.get("site_id") == "patreon":
        _install_patreon_attachment_adapter()
    if request.get("site_id") == "subscribestar":
        _install_subscribestar_post_adapter()

    from gallery_dl import config

    _configure_logging()
    config.clear()
    if config_path := request.get("config_path"):
        config.load([config_path], strict=True)
    source_batched_sites = {
        "deviantart",
        "furaffinity",
        "hentaifoundry",
        "newgrounds",
        "patreon",
        "subscribestar",
        "tumblr",
    }
    if request.get("post_range") and request.get("site_id") not in source_batched_sites:
        config.set((), "post-range", request["post_range"])
    if request.get("child_range"):
        config.set((), "child-range", request["child_range"])
    _configure_source_window(config, request)
    if request.get("abort_threshold") is not None:
        config.set((), "skip", f"abort:{request['abort_threshold']}")
    if request.get("archive_path"):
        config.set((), "archive", request["archive_path"])
    if request.get("archive_prefix"):
        config.set((), "archive-prefix", request["archive_prefix"])
    config.set(("output",), "mode", "null")
    # Keep an internal byte heartbeat while a large file is transferring.
    # Picto's Rust watchdog consumes this event; it is not renderer progress.
    config.set(("downloader",), "progress", 0.5)
    _install_request_pacing_and_trace(
        request.get("request_trace_path"), request.get("pacing_state_dir")
    )

    _emit(
        "run_started",
        subscription_id=request.get("subscription_id"),
        query_id=request.get("query_id"),
        url=request.get("url"),
    )
    try:
        bridge_job = PictoDownloadJob(
            request["url"],
            request.get("post_limit"),
            request.get("accepted_extensions"),
            request.get("post_terminal_mode"),
        )
        status = bridge_job.run()
    except Exception:
        traceback.print_exc(file=sys.stderr)
        raise
    _emit("run_finished", exit_code=status)
    return int(status or 0)


if __name__ == "__main__":
    raise SystemExit(main())
