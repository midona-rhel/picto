import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

describe('thumbnail worker cancellation ownership', () => {
  let worker: { postMessage: ReturnType<typeof vi.fn>; onmessage: (event: unknown) => void };
  beforeEach(async () => {
    vi.resetModules();
    worker = { postMessage: vi.fn(), onmessage: () => {} };
    vi.stubGlobal('self', worker);
    await import('./thumbnailDecodeWorker');
  });
  afterEach(() => { vi.unstubAllGlobals(); });
  const entry = { fileHash: 'same-thumbnail', url: 'media://localhost/thumb/fixture.jpg', quality: 'thumbnail' };
  const plan = (worker: { onmessage: (event: unknown) => void }, entries: unknown[]) => worker.onmessage({ data: { type: 'plan', entries } });

  it('admits only six active requests even when many nearby thumbnails are planned', () => {
    const pending = deferred<Response>();
    const fetch = vi.fn().mockReturnValue(pending.promise);
    vi.stubGlobal('fetch', fetch);
    plan(worker, Array.from({ length: 100 }, (_, index) => ({ ...entry, fileHash: `tile-${index}` })));
    expect(fetch).toHaveBeenCalledTimes(6);
    plan(worker, []);
    expect(fetch.mock.calls.every(([, options]) => options.signal.aborted)).toBe(true);
  });

  it('an old aborted fetch cannot remove or duplicate the replacement request', async () => {
    const old = deferred<Response>();
    const replacement = deferred<Response>();
    const fetch = vi.fn().mockReturnValueOnce(old.promise).mockReturnValue(replacement.promise);
    vi.stubGlobal('fetch', fetch);
    const bitmap = { close: vi.fn() };
    vi.stubGlobal('createImageBitmap', vi.fn().mockResolvedValue(bitmap));
    plan(worker, [entry]);
    plan(worker, []);
    plan(worker, [entry]);
    old.reject(Object.assign(new Error('aborted'), { name: 'AbortError' }));
    await vi.waitFor(() => expect(fetch).toHaveBeenCalledTimes(2));
    await Promise.resolve();
    expect(fetch).toHaveBeenCalledTimes(2);
    replacement.resolve(new Response('fixture'));
    await vi.waitFor(() => expect(worker.postMessage).toHaveBeenCalledWith(
      expect.objectContaining({ type: 'bitmap', bitmap }), [bitmap],
    ));
    expect(fetch).toHaveBeenCalledTimes(2);
    expect(worker.postMessage.mock.calls.filter(([message]) => message.type === 'error')).toEqual([]);
  });

  it('a stale decode failure after cancellation does not poison the current thumbnail', async () => {
    const oldDecode = deferred<ImageBitmap>();
    const bitmap = { close: vi.fn() };
    const decode = vi.fn().mockReturnValueOnce(oldDecode.promise).mockResolvedValue(bitmap);
    vi.stubGlobal('fetch', vi.fn().mockImplementation(async () => new Response('fixture')));
    vi.stubGlobal('createImageBitmap', decode);
    plan(worker, [entry]);
    await vi.waitFor(() => expect(decode).toHaveBeenCalledOnce());
    plan(worker, []);
    plan(worker, [entry]);
    oldDecode.reject(new Error('decode failed after abort'));
    await vi.waitFor(() => expect(worker.postMessage).toHaveBeenCalledWith(
      expect.objectContaining({ type: 'bitmap', bitmap }), [bitmap],
    ));
    expect(worker.postMessage.mock.calls.filter(([message]) => message.type === 'error')).toEqual([]);
  });
});
