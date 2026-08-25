# -*- coding: utf-8 -*-

# Vendored from gallery-dl Codeberg PR #192 at commit
# a4038d260071dc1b91b821c41ba6d9440c0f706c.
#
# This program is free software; you can redistribute it and/or modify
# it under the terms of the GNU General Public License version 2 as
# published by the Free Software Foundation.

"""curl_cffi.requests.Session wrapped with a requests.Session
-compatible API for browser TLS fingerprint impersonation."""

import threading
import weakref as _weakref
from copy import copy
from concurrent.futures.thread import (
    ThreadPoolExecutor,
    _threads_queues,
    _worker,
)
from http.client import responses as HTTP_STATUS_PHRASES
from urllib.parse import urlsplit
import requests
import requests.exceptions

try:
    import curl_cffi.requests
    import curl_cffi.requests.exceptions as _cexc
except ImportError:
    curl_cffi = None
    _cexc = None


def _translate_exception(exc):
    if _cexc is None:
        return exc
    if isinstance(exc, _cexc.ConnectionError):
        return requests.exceptions.ConnectionError(exc)
    if isinstance(exc, _cexc.Timeout):
        return requests.exceptions.Timeout(exc)
    if isinstance(exc, _cexc.ContentDecodingError):
        return requests.exceptions.ContentDecodingError(exc)
    if isinstance(exc, _cexc.ChunkedEncodingError):
        return requests.exceptions.ChunkedEncodingError(exc)
    if isinstance(exc, _cexc.RequestException):
        return requests.exceptions.RequestException(exc)
    return exc


def _wrap_request(func, *args, **kwargs):
    try:
        return func(*args, **kwargs)
    except Exception as exc:
        exc = _translate_exception(exc)
        if isinstance(exc, requests.exceptions.RequestException):
            raise exc
        raise


class _DaemonThreadPoolExecutor(ThreadPoolExecutor):
    # Workers block inside libcurl's perform loop and do not
    # respond to shutdown sentinels. Spawning them as daemon
    # threads excludes them from threading._shutdown's join.

    def _adjust_thread_count(self):
        if self._idle_semaphore.acquire(timeout=0):
            return

        def weakref_cb(_, q=self._work_queue):
            q.put(None)

        num_threads = len(self._threads)
        if num_threads < self._max_workers:
            thread_name = "%s_%d" % (
                self._thread_name_prefix or self, num_threads)
            # _worker signature changed in Python 3.14
            if hasattr(self, "_create_worker_context"):
                args = (
                    _weakref.ref(self, weakref_cb),
                    self._create_worker_context(),
                    self._work_queue,
                )
            else:
                args = (
                    _weakref.ref(self, weakref_cb),
                    self._work_queue,
                    self._initializer,
                    self._initargs,
                )
            t = threading.Thread(
                name=thread_name, target=_worker, args=args,
                daemon=True,
            )
            t.start()
            self._threads.add(t)
            _threads_queues[t] = self._work_queue


def _replace_executor_with_daemon(session):
    # curl_cffi.requests.Session.executor is a read-only property
    # backed by self._executor; assigning _executor directly
    # installs our daemon variant before streaming creates one.
    try:
        if not hasattr(session, "_executor"):
            return
        existing = getattr(session, "_executor", None)
        session._executor = _DaemonThreadPoolExecutor()
        if existing is not None:
            existing.shutdown(wait=False)
    except Exception:
        pass


def _detach_session_threads(session):
    # concurrent.futures.thread._python_exit (registered via
    # threading._register_atexit) iterates _threads_queues and
    # calls t.join() on each worker; libcurl workers are blocked
    # in C and never see the sentinel, so pop them instead.
    try:
        executor = getattr(session, "_executor", None)
        if executor is None:
            return
        for t in list(getattr(executor, "_threads", ())):
            _threads_queues.pop(t, None)
    except Exception:
        pass


class _RawProxy():
    __slots__ = ("_response",)

    def __init__(self, response):
        self._response = response

    @property
    def chunked(self):
        te = self._response.headers.get(
            "transfer-encoding", "")
        return "chunked" in te.lower()


class _TranslationIterator():
    """Wraps a response body iterator, translating curl_cffi exceptions
    to the requests exception hierarchy on __next__()."""

    def __init__(self, iterator):
        self._iterator = iterator

    def __iter__(self):
        return self

    def __next__(self):
        try:
            return next(self._iterator)
        except StopIteration:
            raise
        except Exception as exc:
            raise _translate_exception(exc)


class CurlCffiResponseWrapper():

    def __init__(self, response):
        self._response = response
        self.raw = _RawProxy(response)

    def __getattr__(self, name):
        return getattr(self._response, name)

    def iter_content(self, chunk_size=1, decode_unicode=False):
        return _TranslationIterator(
            self._response.iter_content(chunk_size, decode_unicode))

    @property
    def reason(self):
        r = self._response.reason
        if r:
            return r
        return HTTP_STATUS_PHRASES.get(
            self._response.status_code, "")


class CookieJarWrapper():
    # Delegates iteration to the underlying http.cookiejar.CookieJar
    # so callers see Cookie objects with .name/.domain/.expires
    # rather than the bare name strings curl_cffi.Cookies yields.

    def __init__(self, cffi_cookies):
        self._cookies = cffi_cookies

    @property
    def jar(self):
        return self._cookies.jar

    def __iter__(self):
        return iter(self._cookies.jar)

    def __bool__(self):
        return bool(list(self._cookies.jar))

    def __len__(self):
        return len(list(self._cookies.jar))

    def set_cookie(self, cookie):
        self._cookies.jar.set_cookie(cookie)

    def set(self, name, value, domain="", path="/"):
        self._cookies.set(
            name, value, domain=domain, path=path)


class CurlCffiSessionWrapper():

    def __init__(self, impersonate="firefox", proxy=None,
                 trust_env=True, session=None, requests_hosts=()):
        if curl_cffi is None:
            raise ImportError(
                "curl_cffi is required but not installed. "
                "Install it with: pip install curl_cffi"
            )
        if session is not None:
            self._session = session
        else:
            self._session = curl_cffi.requests.Session(
                impersonate=impersonate,
            )
            if proxy:
                self._session.proxies = proxy
            self._session.trust_env = trust_env

        # Prevent interpreter shutdown from hanging on the
        # libcurl-blocked worker threads: daemon executor makes
        # _thread_shutdown skip them, atexit callback pops them
        # from _threads_queues before _python_exit iterates.
        _replace_executor_with_daemon(self._session)
        threading._register_atexit(
            _detach_session_threads, self._session)

        self.trust_env = trust_env
        self.headers = self._session.headers
        self.cookies = CookieJarWrapper(self._session.cookies)
        self._requests_hosts = frozenset(requests_hosts)
        self._requests_session = None
        if self._requests_hosts:
            fallback = self._requests_session = requests.Session()
            fallback.trust_env = trust_env
            if proxy:
                fallback.proxies = proxy

    def _sync_requests_session(self):
        session = self._requests_session
        session.headers.clear()
        session.headers.update(dict(self.headers))
        session.cookies.clear()
        for cookie in self.cookies.jar:
            session.cookies.set_cookie(copy(cookie))
        return session

    def request(self, method, url, **kwargs):
        # requests silently drops None-valued headers; curl_cffi
        # rejects them
        if "headers" in kwargs and kwargs["headers"]:
            kwargs["headers"] = {
                k: v for k, v in kwargs["headers"].items()
                if v is not None
            }
        host = urlsplit(url).hostname
        if host in self._requests_hosts:
            session = self._sync_requests_session()
            headers = dict(kwargs.get("headers") or ())
            headers.setdefault("Connection", "close")
            kwargs["headers"] = headers
            return _wrap_request(session.request, method, url, **kwargs)

        response = _wrap_request(
            self._session.request, method, url, **kwargs)
        return CurlCffiResponseWrapper(response)

    def mount(self, prefix, adapter):
        """No-op — curl_cffi does not use adapters."""

    def close(self):
        if self._requests_session is not None:
            self._requests_session.close()
        self._session.close()
