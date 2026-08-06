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
                if not isinstance(self.hooks, collections.defaultdict):
                    self.hooks = collections.defaultdict(list)
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
