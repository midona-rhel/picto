#!/usr/bin/env python3
"""Run OF-Scraper in an isolated Picto profile and emit normalized NDJSON."""

from __future__ import annotations

import argparse
import html
import json
import multiprocessing
import os
import re
import shutil
import sqlite3
import sys
import threading
import traceback
import types
from collections import defaultdict
from datetime import datetime, timezone
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import urlencode


_EMIT_LOCK = threading.Lock()


def emit(event: str, **values: object) -> None:
    with _EMIT_LOCK:
        print(json.dumps({"event": event, **values}, ensure_ascii=False), flush=True)


class _TextExtractor(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.parts: list[str] = []

    def handle_data(self, data: str) -> None:
        self.parts.append(data)


def plain_text(value: object) -> str | None:
    parser = _TextExtractor()
    parser.feed(html.unescape(str(value or "")))
    text = re.sub(r"\s+", " ", " ".join(parser.parts)).strip()
    return text or None


def post_title(
    creator: object, text: object, created_at: object, post_id: object
) -> str:
    description = plain_text(text)
    if description:
        if len(description) <= 120:
            return description
        shortened = description[:120].rsplit(" ", 1)[0].strip()
        return f"{shortened or description[:120].strip()}..."
    creator_name = str(creator or "OnlyFans").strip() or "OnlyFans"
    date = str(created_at or "").strip()[:10]
    if date:
        return f"{creator_name} - {date}"
    return f"{creator_name} - post {post_id}"


def auth_from_request(request: dict[str, object]) -> dict[str, object]:
    cookies = dict(request["cookies"])
    headers = {str(key).lower(): value for key, value in dict(request["headers"]).items()}
    return {
        "sess": cookies["sess"],
        "auth_id": cookies["auth_id"],
        "auth_uid": cookies.get("auth_uid", cookies["auth_id"]),
        "user_agent": headers["user-agent"],
        "x-bc": headers["x-bc"],
    }


def write_runtime(request: dict[str, object]) -> Path:
    state = Path(str(request["state_dir"])).resolve()
    output = Path(str(request["output_dir"])).resolve()
    runtime = output.parent / "ofscraper-runtime"
    profile = runtime / "picto_profile"
    state_data = state / ".data"
    profile.mkdir(parents=True, exist_ok=True)
    state_data.mkdir(parents=True, exist_ok=True)
    output.mkdir(parents=True, exist_ok=True)
    config_path = runtime / "config.json"
    config = {
        "main_profile": "picto_profile",
        "metadata": str(state_data / "{model_id}"),
        "file_options": {
            "save_location": str(output),
            "dir_format": "{model_username}/{post_id}",
            "file_format": "{media_id}.{ext}",
        },
        "performance_options": {"download_sems": 1},
        # Owned media can be served as DRM DASH streams: decrypt through the
        # remote CDRM helper (the proven interactive setup) and give
        # OF-Scraper the bundled ffmpeg for segment merging.
        "cdm_options": {
            "private-key": None,
            "client-id": None,
            "key-mode-default": "cdrm",
        },
        "binary_options": {"ffmpeg": shutil.which("ffmpeg") or ""},
        "advanced_options": {
            "skip_unavailable_content": True,
            "incremental_downloads": True,
        },
    }
    config_path.write_text(json.dumps(config, indent=2), encoding="utf-8")
    return config_path


def configure_pacing() -> None:
    # Picto runs one subscription query at a time. Reducing every OF-Scraper
    # semaphore to one and every session interval to one second therefore gives
    # one serial request stream for onlyfans.com, including media requests.
    values = {
        "OFSC_DOWNLOAD_SEM_DEFAULT": "1",
        "OFSC_REQ_SEMAPHORE_MULTI": "1",
        "OFSC_SUBSCRIPTION_SEMS": "1",
        "OFSC_MAX_SEMS_BATCH_DOWNLOAD": "1",
        "OFSC_MAX_SEMS_SINGLE_THREAD_DOWNLOAD": "1",
        "OFSC_SESSION_MANAGER_SYNC_SEM_DEFAULT": "1",
        "OFSC_SESSION_MANAGER_SEM_DEFAULT": "1",
        "OFSC_API_REQ_SEM_MAX": "1",
        "OFSC_API_MAX_AREAS": "1",
        "OFSC_SESSION_MIN_SLEEP": "1",
        "OFSC_DOWNLOAD_SESSION_MIN_SLEEP": "1",
        "OFSC_METADATA_SESSION_MIN_SLEEP": "1",
        "OFSC_SUBSCRIPTION_SESSION_MIN_SLEEP": "1",
    }
    for key, value in values.items():
        os.environ[key] = value


def monitor_parent(parent_pid: int) -> None:
    """Terminate the sidecar if Electron exits without running Rust destructors."""
    import psutil

    try:
        parent = psutil.Process(parent_pid)
        started_at = parent.create_time()
    except psutil.Error:
        os._exit(0)
    while True:
        threading.Event().wait(1)
        try:
            current = psutil.Process(parent_pid)
            if not current.is_running() or current.create_time() != started_at:
                os._exit(0)
        except psutil.Error:
            os._exit(0)


def getter_with_default(getter):
    """Adapt the pinned OF-Scraper getter to its own two-argument call sites."""
    def wrapped(key: str, default: object = None) -> object:
        value = getter(key)
        return default if value is None else value

    return wrapped


def install_ofscraper_compatibility() -> None:
    # The pinned revision defines getattr(key), but three of its modules call
    # getattr(key, default). Patch the shared module once at the adapter boundary.
    import ofscraper.utils.of_env.of_env as of_env

    if not getattr(of_env.getattr, "_picto_accepts_default", False):
        wrapped = getter_with_default(of_env.getattr)
        wrapped._picto_accepts_default = True
        of_env.getattr = wrapped


def process_auth_reader(auth: dict[str, object]):
    # OF-Scraper intentionally owns header, cookie, and request signing logic.
    # Supplying its normal auth dictionary in process avoids a shared temporary
    # auth file that can be removed by a concurrently terminating prior attempt.
    captured = dict(auth)
    return lambda: dict(captured)


def process_auth_module(auth: dict[str, object]) -> types.ModuleType:
    module = types.ModuleType("ofscraper.utils.auth.file")
    module.read_auth = process_auth_reader(auth)
    return module


def install_ofscraper_auth(auth: dict[str, object]) -> None:
    """Keep Picto's captured session available for the entire sidecar run."""
    import ofscraper.utils.auth as auth_package

    # OF-Scraper's request layer only needs read_auth. Importing its interactive
    # auth file also imports prompts, settings, config, and console in a cycle.
    # Picto already owns login, so expose the captured session at that boundary.
    auth_file = process_auth_module(auth)
    sys.modules[auth_file.__name__] = auth_file
    auth_package.file = auth_file



def disable_ofscraper_update_checks() -> None:
    # The CLI performs unrelated GitHub/PyPI update checks without timeouts.
    # Picto ships a pinned sidecar, so those checks can only delay a run.
    import ofscraper.utils.logs.logs as logs

    logs.print_start_message = lambda: None
    logs.print_latest_version = lambda: None


def disable_ofscraper_startup_cdm_probe() -> None:
    # OF-Scraper probes its optional remote DRM helper before every run, even
    # when the selected posts contain no DRM video. Actual video decryption
    # still uses the configured CDM helper when a download requires it.
    import ofscraper.utils.system.network as network

    network.check_cdm = lambda: True


def use_in_process_download_locks() -> None:
    # This pinned OF-Scraper revision creates two aioprocessing locks that are
    # never consumed. In a frozen executable their multiprocessing bootstrap
    # terminates the download phase before any media is handled.
    import asyncio
    import ofscraper.commands.scraper.actions.utils.globals as action_globals

    action_globals.aioprocessing.AioLock = asyncio.Lock


def preserve_ofscraper_runtime_logger() -> None:
    # Creating OF-Scraper's named download logger stops the process-wide queue
    # listener, which also hides the exception that aborts the download action.
    # The shared logger is already configured for this run and is sufficient.
    import logging
    import ofscraper.commands.scraper.actions.utils.globals as action_globals

    original = action_globals.logger.get_shared_logger

    def get_shared_logger(name=None):
        return logging.getLogger("shared") if name else original()

    action_globals.logger.get_shared_logger = get_shared_logger


def bound_ofscraper_sync_connect_timeout() -> None:
    from ofscraper.managers.sessionmanager.sessionmanager import sessionManager

    original = sessionManager.requests

    def requests(manager, *args, **kwargs):
        if kwargs.get("connect_timeout") is None:
            kwargs["connect_timeout"] = manager._connect_timeout
        return original(manager, *args, **kwargs)

    sessionManager.requests = requests


def progress_snapshot(request: dict[str, object]) -> tuple[int, int, int]:
    post_count = 0
    downloaded_count = 0
    state = Path(str(request["state_dir"]))
    for database in creator_databases(state):
        try:
            connection = sqlite3.connect(f"file:{database}?mode=ro", uri=True, timeout=1)
            post_count += int(connection.execute("SELECT COUNT(*) FROM posts").fetchone()[0])
            downloaded_count += int(
                connection.execute(
                    "SELECT COUNT(*) FROM medias WHERE downloaded = 1"
                ).fetchone()[0]
            )
            connection.close()
        except sqlite3.Error:
            continue
    output = Path(str(request["output_dir"]))
    file_count = sum(1 for path in output.rglob("*") if path.is_file()) if output.exists() else 0
    return post_count, downloaded_count, file_count


def monitor_progress(
    request: dict[str, object],
    stop: threading.Event,
    emitted_posts: set[str],
    emitted_media: set[str],
    completed_posts: set[str],
    source_posts: list[dict[str, object]],
) -> None:
    previous = progress_snapshot(request)
    emit(
        "progress",
        posts=previous[0],
        downloaded=previous[1],
        files=previous[2],
    )
    while not stop.wait(2):
        emit_available_posts(
            request, emitted_posts, emitted_media, completed_posts, source_posts
        )
        current = progress_snapshot(request)
        if current != previous:
            emit(
                "progress",
                posts=current[0],
                downloaded=current[1],
                files=current[2],
            )
            previous = current


def _timestamp(value: object) -> float:
    if value is None:
        return 0.0
    if isinstance(value, (int, float)):
        return float(value)
    if hasattr(value, "float_timestamp"):
        return float(value.float_timestamp)
    if hasattr(value, "timestamp"):
        return float(value.timestamp())
    text = str(value).strip()
    if not text:
        return 0.0
    try:
        return float(text)
    except ValueError:
        parsed = datetime.fromisoformat(text.replace("Z", "+00:00"))
        if parsed.tzinfo is None:
            parsed = parsed.replace(tzinfo=timezone.utc)
        return parsed.timestamp()


SOURCE_GROUPS = ("purchased", "messages", "feed")
SOURCE_GROUP_RANK = {group: rank for rank, group in enumerate(SOURCE_GROUPS)}


def decode_cursor_details(
    value: object,
) -> tuple[str | None, str | None, str | None]:
    text = str(value or "").strip()
    if not text:
        return None, None, None
    if text.startswith("{"):
        try:
            cursor = json.loads(text)
            source_group = cursor.get("source_group")
            if source_group is not None and source_group not in SOURCE_GROUP_RANK:
                raise ValueError("unknown source group")
            return (
                str(cursor["created_at"]),
                str(cursor["post_id"]),
                source_group,
            )
        except (KeyError, TypeError, ValueError, json.JSONDecodeError):
            raise ValueError("Invalid OnlyFans pagination cursor") from None
    return text, None, None


def decode_cursor(value: object) -> tuple[str | None, str | None]:
    created_at, post_id, _ = decode_cursor_details(value)
    return created_at, post_id


def encode_cursor(
    created_at: object, post_id: object, source_group: object = None
) -> str:
    cursor = {"created_at": str(created_at), "post_id": str(post_id)}
    if source_group in SOURCE_GROUP_RANK:
        cursor["source_group"] = str(source_group)
    return json.dumps(cursor, separators=(",", ":"))


def recent_posts_url(model_id: object, area: str, before: object = None) -> str:
    suffix = {
        "Timeline": "posts",
        "Archived": "posts/archived",
        "Pinned": "posts",
        "Streams": "posts/streams",
    }[area]
    params: dict[str, object] = {
        "limit": 100,
        "order": "publish_date_desc",
        "skip_users": "all",
        "skip_users_dups": 1,
        "format": "infinite",
    }
    if area == "Timeline":
        params["pinned"] = 0
    elif area == "Pinned":
        params["pinned"] = 1
        params["counters"] = 0
        params.pop("order", None)
    before_timestamp, _, before_group = decode_cursor_details(before)
    if before_timestamp and before_group in (None, "feed") and area != "Pinned":
        params["beforePublishTime"] = _timestamp(before_timestamp)
    return f"https://onlyfans.com/api2/v2/users/{model_id}/{suffix}?{urlencode(params)}"


def recent_messages_url(model_id: object, before: object = None) -> str:
    params: dict[str, object] = {
        "limit": 100,
        "order": "desc",
        "skip_users": "all",
        "skip_users_dups": 1,
    }
    _, before_post_id, before_group = decode_cursor_details(before)
    if before_group == "messages" and before_post_id:
        params["id"] = before_post_id
    return f"https://onlyfans.com/api2/v2/chats/{model_id}/messages?{urlencode(params)}"


def post_bounded_media(media: list[object], limit: int) -> list[object]:
    """Keep every file from the newest `limit` source posts."""
    if limit <= 0:
        return media
    newest_by_post: dict[str, float] = {}
    for item in media:
        post_id = str(getattr(item, "post_id", ""))
        post = getattr(item, "post", None)
        date = getattr(item, "postdate", None) or getattr(post, "date", None)
        newest_by_post[post_id] = max(newest_by_post.get(post_id, 0.0), _timestamp(date))
    selected = {
        post_id
        for post_id, _ in sorted(
            newest_by_post.items(), key=lambda entry: (entry[1], entry[0]), reverse=True
        )[:limit]
    }
    return [item for item in media if str(getattr(item, "post_id", "")) in selected]


def source_post_group(post: object) -> str:
    try:
        value = post["source_group"]
    except (KeyError, TypeError):
        try:
            value = post.get("_picto_source_group")
        except AttributeError:
            value = None
    return str(value) if value in SOURCE_GROUP_RANK else "feed"


def source_post_id(post: object) -> str:
    try:
        value = post["post_id"]
    except (KeyError, TypeError):
        try:
            value = post.get("id")
        except AttributeError:
            value = None
    return str(value or "")


def source_post_created_at(post: object) -> object:
    for key in ("created_at", "postedAtPrecise", "postedAt", "createdAt"):
        try:
            value = post[key]
        except (KeyError, TypeError):
            try:
                value = post.get(key)
            except AttributeError:
                value = None
        if value is not None:
            return value
    return None


def ordered_source_posts(posts: list[object], limit: int) -> list[object]:
    """Order purchased content, creator DMs, then creator feed."""
    preferred: dict[str, object] = {}
    for post in posts:
        post_id = source_post_id(post)
        if not post_id:
            continue
        existing = preferred.get(post_id)
        if existing is None or SOURCE_GROUP_RANK[source_post_group(post)] < SOURCE_GROUP_RANK[source_post_group(existing)]:
            preferred[post_id] = post
    ordered = sorted(
        preferred.values(),
        key=lambda post: (_timestamp(source_post_created_at(post)), source_post_id(post)),
        reverse=True,
    )
    ordered.sort(key=lambda post: SOURCE_GROUP_RANK[source_post_group(post)])
    return ordered[:limit] if limit > 0 else ordered


def recent_post_ids(posts: list[dict[str, object]], limit: int) -> set[str]:
    return {source_post_id(post) for post in ordered_source_posts(posts, limit)}


def mark_source_group(
    posts: list[dict[str, object]], source_group: str
) -> list[dict[str, object]]:
    for post in posts:
        post["_picto_source_group"] = source_group
    return posts


def source_media_order(posts: list[dict[str, object]]) -> dict[tuple[str, str], int]:
    """Return the creator-authored order from each OnlyFans post payload."""
    order: dict[tuple[str, str], int] = {}
    for post in list(posts):
        post_id = str(post.get("id") or "")
        if not post_id:
            continue
        for position, media in enumerate(post.get("media") or [], start=1):
            if isinstance(media, dict) and media.get("id") is not None:
                order[(post_id, str(media["id"]))] = position
    return order


def source_group_by_post(posts: list[dict[str, object]]) -> dict[str, str]:
    groups: dict[str, str] = {}
    for post in posts:
        post_id = source_post_id(post)
        group = source_post_group(post)
        if post_id and (
            post_id not in groups
            or SOURCE_GROUP_RANK[group] < SOURCE_GROUP_RANK[groups[post_id]]
        ):
            groups[post_id] = group
    return groups


def table_exists(connection: sqlite3.Connection, table: str) -> bool:
    return connection.execute(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?)",
        [table],
    ).fetchone()[0] == 1


def install_recent_post_window(request: dict[str, object]) -> dict[str, object]:
    """Replace OF-Scraper's full-history ascending sweep with one recent page."""
    from ofscraper.data.api import archive, messages, paid, pinned, streams, timeline
    import ofscraper.filters.media.filters as media_filters

    state: dict[str, object] = {"has_more": False, "errors": [], "posts": []}
    before = request.get("before")

    async def fetch(c, model_id: object, area: str) -> list[dict[str, object]]:
        url = recent_posts_url(model_id, area, before)
        try:
            async with c.requests_async(url=url) as response:
                if not 200 <= response.status < 300:
                    raise RuntimeError(f"OnlyFans {area} request failed with HTTP {response.status}")
                payload = await response.json_()
                if area != "Pinned":
                    state["has_more"] = bool(
                        state["has_more"] or payload.get("hasMore")
                    )
                posts = list(payload.get("list") or [])
                _, _, before_group = decode_cursor_details(before)
                if before and before_group in (None, "feed") and area == "Pinned":
                    cutoff = _timestamp(decode_cursor(before)[0])
                    posts = [
                        post
                        for post in posts
                        if _timestamp(
                            post.get("postedAtPrecise")
                            or post.get("postedAt")
                            or post.get("createdAt")
                        )
                        < cutoff
                    ]
                mark_source_group(posts, "feed")
                state["posts"].extend(posts)
                return posts
        except Exception as error:
            state["errors"].append(f"{area}: {error}")
            raise

    async def timeline_posts(model_id, username, c=None, post_id=None):
        return await fetch(c, model_id, "Timeline")

    async def archived_posts(model_id, username, c=None, post_id=None):
        return await fetch(c, model_id, "Archived")

    async def streamed_posts(model_id, username, c=None, post_id=None):
        return await fetch(c, model_id, "Streams")

    async def pinned_posts(model_id, c=None, post_id=None):
        return await fetch(c, model_id, "Pinned")

    original_paid_posts = paid.get_paid_posts

    async def purchased_posts(username, model_id, c=None):
        posts = list(await original_paid_posts(username, model_id, c=c) or [])
        before_timestamp, _, before_group = decode_cursor_details(before)
        if before_group in ("messages", "feed"):
            posts = []
        elif before_timestamp:
            cutoff = _timestamp(before_timestamp)
            posts = [
                post
                for post in posts
                if _timestamp(
                    post.get("postedAtPrecise")
                    or post.get("postedAt")
                    or post.get("createdAt")
                )
                < cutoff
            ]
        posts.sort(
            key=lambda post: (
                _timestamp(
                    post.get("postedAtPrecise")
                    or post.get("postedAt")
                    or post.get("createdAt")
                ),
                str(post.get("id") or ""),
            ),
            reverse=True,
        )
        limit = int(request["post_limit"])
        if len(posts) > limit:
            state["has_more"] = True
            posts = posts[:limit]
        mark_source_group(posts, "purchased")
        state["posts"].extend(posts)
        return posts

    async def creator_messages(model_id, username, c=None, post_id=None):
        before_timestamp, _, before_group = decode_cursor_details(before)
        if before_group == "feed":
            return []
        url = recent_messages_url(model_id, before)
        async with c.requests_async(url=url) as response:
            if not 200 <= response.status < 300:
                raise RuntimeError(
                    f"OnlyFans Messages request failed with HTTP {response.status}"
                )
            payload = await response.json_()
        posts = list(payload.get("list") or [])
        state["has_more"] = bool(state["has_more"] or payload.get("hasMore"))
        if before_timestamp and before_group == "messages":
            cutoff = _timestamp(before_timestamp)
            posts = [
                post
                for post in posts
                if _timestamp(
                    post.get("postedAtPrecise")
                    or post.get("postedAt")
                    or post.get("createdAt")
                )
                < cutoff
            ]
        mark_source_group(posts, "messages")
        state["posts"].extend(posts)
        return posts

    def limit_media_by_post(media):
        return filter_selected_download_media(request, state["posts"], media)

    timeline.get_timeline_posts = timeline_posts
    archive.get_archived_posts = archived_posts
    pinned.get_pinned_posts = pinned_posts
    streams.get_streams_posts = streamed_posts
    paid.get_paid_posts = purchased_posts
    messages.get_messages = creator_messages
    media_filters.ele_count_filter = limit_media_by_post
    return state


def run_ofscraper(
    request: dict[str, object], config_path: Path, auth: dict[str, object]
) -> tuple[dict[str, object], set[str], set[str], set[str]]:
    arguments = [
        "ofscraper",
        "--config", str(config_path),
        "--profile", "picto",
        "--action", "download",
        "--username", str(request["creator"]),
        "--download-area", "Purchased,Messages,Timeline,Archived,Pinned,Streams",
        "--max-post-count", str(request["post_limit"]),
        "--downloadsem", "1",
        "--force-all",
        "--no-live",
    ]
    before, _, before_group = decode_cursor_details(request.get("before"))
    if before and before_group in (None, "messages"):
        arguments.extend(["--before", before])
    sys.argv = arguments
    install_ofscraper_compatibility()
    install_ofscraper_auth(auth)
    import ofscraper.main.open.load as load
    from ofscraper.managers.manager import start_manager

    disable_ofscraper_update_checks()
    disable_ofscraper_startup_cdm_probe()
    use_in_process_download_locks()
    preserve_ofscraper_runtime_logger()
    bound_ofscraper_sync_connect_timeout()
    # OF-Scraper's public main catches every exception and returns success. Run
    # the same startup stages directly so Picto can report the real failure.
    load.systemSet()
    load.settings_loader()
    load.setdate()
    load.readConfig()
    load.setLogger()
    load.make_folder()
    window = install_recent_post_window(request)
    emitted_posts: set[str] = set()
    emitted_media: set[str] = set()
    completed_posts: set[str] = set()
    progress_stop = threading.Event()
    progress_thread = threading.Thread(
        target=monitor_progress,
        args=(
            request,
            progress_stop,
            emitted_posts,
            emitted_media,
            completed_posts,
            window["posts"],
        ),
        name="picto-onlyfans-progress",
        daemon=True,
    )
    progress_thread.start()
    try:
        start_manager()
    finally:
        progress_stop.set()
        progress_thread.join(timeout=3)
    errors = list(window["errors"])
    if errors:
        raise RuntimeError("; ".join(errors))
    emit_available_posts(
        request, emitted_posts, emitted_media, completed_posts, window["posts"]
    )
    return window, emitted_posts, emitted_media, completed_posts


def creator_databases(state: Path) -> list[Path]:
    return sorted((state / ".data").glob("*/user_data.db"))


def selected_posts(
    connection: sqlite3.Connection,
    request: dict[str, object],
    source_posts: list[dict[str, object]] | None = None,
) -> list[dict[str, object]]:
    candidates: list[dict[str, object]] = []
    has_media = table_exists(connection, "medias")
    paid_expression = (
        "CASE WHEN EXISTS (SELECT 1 FROM medias m "
        "WHERE m.post_id = p.post_id AND m.model_id = p.model_id "
        "AND LOWER(m.api_type) = 'paid') THEN 'purchased' ELSE 'feed' END"
        if has_media
        else "'feed'"
    )
    if table_exists(connection, "posts"):
        candidates.extend(
            dict(row)
            for row in connection.execute(
                f"""
                SELECT p.post_id, p.text, p.created_at,
                       {paid_expression} AS source_group
                FROM posts p
                WHERE p.is_deleted = 0 OR p.is_deleted IS NULL
                """
            )
        )
    if table_exists(connection, "messages"):
        candidates.extend(
            dict(row)
            for row in connection.execute(
                """
                SELECT post_id, text, created_at, 'messages' AS source_group
                FROM messages
                WHERE is_deleted = 0 OR is_deleted IS NULL
                """
            )
        )

    groups = source_group_by_post(source_posts or [])
    if source_posts is not None:
        fetched_post_ids = {source_post_id(post) for post in source_posts}
        candidates = [
            post for post in candidates if source_post_id(post) in fetched_post_ids
        ]
    for post in candidates:
        post_id = source_post_id(post)
        if post_id in groups:
            post["source_group"] = groups[post_id]

    before, before_post_id, before_group = decode_cursor_details(request.get("before"))
    if before:
        cutoff = _timestamp(before)

        def follows_cursor(post: dict[str, object]) -> bool:
            post_time = _timestamp(post["created_at"])
            if before_group is None:
                return post_time < cutoff or (
                    post_time == cutoff and source_post_id(post) < str(before_post_id or "")
                )
            post_rank = SOURCE_GROUP_RANK[source_post_group(post)]
            cursor_rank = SOURCE_GROUP_RANK[before_group]
            return post_rank > cursor_rank or (
                post_rank == cursor_rank
                and (
                    post_time < cutoff
                    or (
                        post_time == cutoff
                        and source_post_id(post) < str(before_post_id or "")
                    )
                )
            )

        candidates = [post for post in candidates if follows_cursor(post)]

    return [
        dict(post)
        for post in ordered_source_posts(candidates, int(request["post_limit"]))
    ]


def selected_download_post_ids(
    request: dict[str, object], source_posts: list[dict[str, object]]
) -> set[str]:
    """Select the same persisted post window that Picto later validates."""
    selected: set[str] = set()
    for database in creator_databases(Path(str(request["state_dir"]))):
        try:
            connection = sqlite3.connect(
                f"file:{database}?mode=ro", uri=True, timeout=1
            )
            connection.row_factory = sqlite3.Row
            selected.update(
                source_post_id(post)
                for post in selected_posts(connection, request, source_posts)
            )
            connection.close()
        except sqlite3.Error:
            continue
    return selected


def filter_selected_download_media(
    request: dict[str, object],
    source_posts: list[dict[str, object]],
    media: list[object],
) -> list[object]:
    selected = selected_download_post_ids(request, source_posts)
    return [
        item
        for item in media
        if str(getattr(item, "post_id", "")) in selected
    ]


def media_rows_for_post(
    connection: sqlite3.Connection,
    post_id: str,
    source_posts: list[dict[str, object]] | None = None,
) -> list[sqlite3.Row]:
    has_messages = table_exists(connection, "messages")
    message_join = (
        "LEFT JOIN messages msg ON msg.post_id = m.post_id AND msg.model_id = m.model_id"
        if has_messages
        else ""
    )
    post_text = (
        "CASE WHEN LOWER(m.api_type) IN ('message', 'messages') THEN msg.text ELSE p.text END"
        if has_messages
        else "p.text"
    )
    post_created_at = (
        "CASE WHEN LOWER(m.api_type) IN ('message', 'messages') "
        "THEN msg.created_at ELSE p.created_at END"
        if has_messages
        else "p.created_at"
    )
    is_deleted = (
        "CASE WHEN LOWER(m.api_type) IN ('message', 'messages') "
        "THEN msg.is_deleted ELSE p.is_deleted END"
        if has_messages
        else "p.is_deleted"
    )
    rows = connection.execute(
        f"""
        SELECT m.id AS database_id, m.media_id, m.post_id, m.link, m.directory, m.filename,
               m.media_type, m.api_type, m.downloaded, m.unlocked,
               m.created_at AS media_created_at, m.posted_at,
               {post_text} AS post_text, {post_created_at} AS post_created_at,
               p.archived, p.pinned, p.stream, {is_deleted} AS is_deleted
        FROM medias m
        LEFT JOIN posts p ON p.post_id = m.post_id AND p.model_id = m.model_id
        {message_join}
        WHERE m.post_id = ? AND ({is_deleted} = 0 OR {is_deleted} IS NULL)
        ORDER BY m.id
        """,
        [post_id],
    ).fetchall()
    order = source_media_order(source_posts or [])
    return sorted(
        rows,
        key=lambda row: (
            order.get((str(row["post_id"]), str(row["media_id"])), 2**63 - 1),
            int(row["database_id"]),
        ),
    )


def resolve_output_path(row: sqlite3.Row, output_root: Path) -> Path:
    path = Path(row["directory"] or "") / str(row["filename"] or "")
    if not path.is_absolute():
        path = output_root / path
    if not path.is_file() and row["filename"]:
        path = next(output_root.rglob(str(row["filename"])), path)
    return path


def emit_post_events(
    request: dict[str, object],
    post: sqlite3.Row,
    entries: list[tuple[int, int, sqlite3.Row, Path]],
    accessible: bool,
    emitted_posts: set[str],
    emitted_media: set[str],
) -> None:
    post_id = str(post["post_id"])
    creator = str(request["creator"])
    post_url = f"https://onlyfans.com/{post_id}/{creator}"
    title = post_title(creator, post["text"], post["created_at"], post_id)
    if post_id not in emitted_posts:
        emit(
            "post_traversed",
            post_id=post_id,
            post_url=post_url,
            creator=creator,
            title=title,
            description=plain_text(post["text"]),
            created_at=post["created_at"],
            accessible=accessible,
        )
        emitted_posts.add(post_id)
    for position, page_count, row, path in entries:
        media_key = f"{post_id}:{row['media_id']}"
        if media_key in emitted_media:
            continue
        created_at = row["post_created_at"] or row["posted_at"] or row["media_created_at"]
        emit(
            "item",
            file_path=str(path),
            post_id=post_id,
            item_key=str(row["media_id"]),
            position=position,
            page_count=page_count,
            creator=creator,
            title=title,
            description=plain_text(row["post_text"]),
            created_at=created_at,
            post_url=post_url,
            media_url=row["link"],
            raw_metadata={
                "creator": creator,
                "username": creator,
                "post_id": post_id,
                "media_id": row["media_id"],
                "media_type": row["media_type"],
                "api_type": row["api_type"],
            },
        )
        emitted_media.add(media_key)


def emit_available_posts(
    request: dict[str, object],
    emitted_posts: set[str],
    emitted_media: set[str],
    completed_posts: set[str],
    source_posts: list[dict[str, object]] | None = None,
) -> None:
    """Emit downloaded files and post boundaries newest-first during a run."""
    output_root = Path(str(request["output_dir"])).resolve()
    for database in creator_databases(Path(str(request["state_dir"]))):
        try:
            connection = sqlite3.connect(
                f"file:{database}?mode=ro", uri=True, timeout=1
            )
            connection.row_factory = sqlite3.Row
            posts = selected_posts(connection, request, source_posts)
            for post in posts:
                post_id = str(post["post_id"])
                if post_id in completed_posts:
                    continue
                rows = media_rows_for_post(connection, post_id, source_posts)
                if not rows:
                    # A missing media row is ambiguous while OF-Scraper writes.
                    # Do not let an older post overtake it.
                    break
                eligible = [
                    row
                    for row in rows
                    if bool(row["unlocked"]) or bool(row["downloaded"])
                ]
                page_count = len(eligible)
                entries: list[tuple[int, int, sqlite3.Row, Path]] = []
                missing_file = False
                for position, row in enumerate(eligible, start=1):
                    if not bool(row["downloaded"]):
                        continue
                    media_key = f"{post_id}:{row['media_id']}"
                    if media_key in emitted_media:
                        continue
                    path = resolve_output_path(row, output_root)
                    if not path.is_file():
                        missing_file = True
                        break
                    entries.append((position, page_count, row, path))
                accessible = not any(
                    not bool(row["downloaded"]) and not bool(row["unlocked"])
                    for row in rows
                )
                emit_post_events(
                    request,
                    post,
                    entries,
                    accessible,
                    emitted_posts,
                    emitted_media,
                )
                if missing_file:
                    break
                if any(not bool(row["downloaded"]) for row in eligible):
                    # Files from the active post can stream, but older posts
                    # cannot overtake its completion boundary.
                    break
                emit("post_complete", post_id=post_id)
                completed_posts.add(post_id)
            connection.close()
        except sqlite3.Error:
            continue


def output_items(
    request: dict[str, object],
    emitted_posts: set[str] | None = None,
    emitted_media: set[str] | None = None,
    completed_posts: set[str] | None = None,
    source_posts: list[dict[str, object]] | None = None,
) -> tuple[str | None, int]:
    emitted_posts = emitted_posts if emitted_posts is not None else set()
    emitted_media = emitted_media if emitted_media is not None else set()
    completed_posts = completed_posts if completed_posts is not None else set()
    rows: list[sqlite3.Row] = []
    posts: dict[str, sqlite3.Row] = {}
    deferred: dict[str, str] = {}
    missing_accessible: list[str] = []
    drm_blocked: set[str] = set()
    databases = creator_databases(Path(str(request["state_dir"])))
    if not databases:
        raise RuntimeError("OF-Scraper produced no creator database; verify the OnlyFans login and creator name")
    for database in databases:
        connection = sqlite3.connect(database)
        connection.row_factory = sqlite3.Row
        selected = selected_posts(connection, request, source_posts)
        for post in selected:
            posts[str(post["post_id"])] = post
            for row in media_rows_for_post(
                connection, str(post["post_id"]), source_posts
            ):
                if bool(row["downloaded"]):
                    rows.append(row)
                elif bool(row["unlocked"]):
                    link = str(row["link"] or "")
                    if ".mpd" in link or "/dash/" in link:
                        # DRM-protected media cannot download without a
                        # configured CDM helper. The post defers instead of
                        # failing the whole run.
                        drm_blocked.add(str(row["post_id"]))
                    else:
                        missing_accessible.append(
                            f"{row['post_id']}:{row['media_id']} ({link[:120]})"
                        )
                else:
                    deferred[str(row["post_id"])] = (
                        "OnlyFans post is not currently accessible"
                    )
        connection.close()

    downloaded_posts = {str(row["post_id"]) for row in rows}
    for post_id in drm_blocked:
        if post_id not in downloaded_posts:
            deferred[post_id] = (
                "OnlyFans media is DRM-protected and requires a configured CDM helper"
            )

    if missing_accessible:
        sample = ", ".join(missing_accessible[:3])
        raise RuntimeError(
            f"OF-Scraper did not download {len(missing_accessible)} accessible media item(s): {sample}"
        )

    output_root = Path(str(request["output_dir"])).resolve()
    existing = []
    missing_downloads: list[str] = []
    for row in rows:
        media_key = f"{row['post_id']}:{row['media_id']}"
        if media_key in emitted_media:
            continue
        path = Path(row["directory"] or "") / str(row["filename"] or "")
        if not path.is_absolute():
            path = output_root / path
        if not path.is_file() and row["filename"]:
            path = next(output_root.rglob(str(row["filename"])), path)
        if path.is_file():
            existing.append((row, path))
        else:
            missing_downloads.append(f"{row['post_id']}:{row['media_id']}")
    if missing_downloads:
        sample = ", ".join(missing_downloads[:3])
        raise RuntimeError(
            f"OF-Scraper marked {len(missing_downloads)} media item(s) downloaded but no file exists: {sample}"
        )
    grouped: dict[str, list[tuple[sqlite3.Row, Path]]] = defaultdict(list)
    for row, path in existing:
        grouped[str(row["post_id"])].append((row, path))
    ordered_posts = ordered_source_posts(
        list(posts.values()), int(request["post_limit"])
    )
    next_cursor = (
        encode_cursor(
            ordered_posts[-1]["created_at"],
            ordered_posts[-1]["post_id"],
            source_post_group(ordered_posts[-1]),
        )
        if ordered_posts
        else None
    )
    for post in ordered_posts:
        post_id = str(post["post_id"])
        order = source_media_order(source_posts or [])
        post_entries = sorted(
            grouped.get(post_id, []),
            key=lambda entry: (
                order.get((post_id, str(entry[0]["media_id"])), 2**63 - 1),
                int(entry[0]["database_id"]),
            ),
        )
        page_count = len(post_entries)
        emit_post_events(
            request,
            post,
            [
                (position, page_count, row, path)
                for position, (row, path) in enumerate(post_entries, start=1)
            ],
            post_id not in deferred,
            emitted_posts,
            emitted_media,
        )
        if post_id not in completed_posts:
            emit("post_complete", post_id=post_id)
            completed_posts.add(post_id)
    return next_cursor, len(posts)


def main() -> int:
    multiprocessing.freeze_support()
    parser = argparse.ArgumentParser()
    parser.add_argument("--request", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        configure_pacing()
        sys.argv = ["ofscraper", "--no-live"]
        install_ofscraper_compatibility()
        install_ofscraper_auth(
            {
                "sess": "self-test",
                "auth_id": "0",
                "auth_uid": "0",
                "user_agent": "Picto self-test",
                "x-bc": "self-test",
            }
        )
        import ofscraper.main.open.load as _load
        from ofscraper.managers.manager import start_manager as _start_manager

        emit(
            "onlyfans_self_test",
            ofscraper_imported=callable(_load.systemSet) and callable(_start_manager),
        )
        return 0
    if args.request is None:
        parser.error("--request is required")
    request = json.loads(args.request.read_text(encoding="utf-8"))
    parent_pid = int(request.get("parent_pid") or 0)
    if parent_pid > 0:
        threading.Thread(
            target=monitor_parent,
            args=(parent_pid,),
            name="picto-onlyfans-parent",
            daemon=True,
        ).start()
    try:
        configure_pacing()
        config = write_runtime(request)
        source_window, emitted_posts, emitted_media, completed_posts = run_ofscraper(
            request, config, auth_from_request(request)
        )
        next_cursor, post_count = output_items(
            request,
            emitted_posts,
            emitted_media,
            completed_posts,
            source_window["posts"],
        )
        history_complete = (
            bool(request.get("history_complete"))
            or (post_count < int(request["post_limit"]) and not source_window["has_more"])
        )
        emit("summary", next_before=next_cursor, history_complete=history_complete)
        return 0
    except Exception as error:
        message = str(error) or error.__class__.__name__
        lower = message.lower()
        kind = "runtime"
        if "auth" in lower or "401" in lower or "403" in lower:
            kind = "authentication"
        elif "429" in lower or "rate limit" in lower:
            kind = "rate_limited"
        elif "network" in lower or "connection" in lower or "resolve" in lower:
            kind = "network"
        emit("error", kind=kind, message=message)
        print(traceback.format_exc(), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
