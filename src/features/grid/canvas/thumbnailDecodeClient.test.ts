import { describe, expect, it, vi } from 'vitest';
import { ThumbnailDecodeClient } from './thumbnailDecodeClient';

function fakeWorker() {
  return {
    onmessage: null as ((event: MessageEvent) => void) | null,
    onerror: null as ((event: Event) => void) | null,
    postMessage: vi.fn(),
    terminate: vi.fn(),
  };
}

describe('ThumbnailDecodeClient', () => {
  it('keeps plans and callbacks isolated per canvas owner', () => {
    const firstWorker = fakeWorker();
    const secondWorker = fakeWorker();
    const firstBitmap = vi.fn();
    const secondBitmap = vi.fn();
    const first = new ThumbnailDecodeClient(firstBitmap, vi.fn(), () => firstWorker as unknown as Worker);
    const second = new ThumbnailDecodeClient(secondBitmap, vi.fn(), () => secondWorker as unknown as Worker);

    first.sendPlan([{ fileHash: 'first', url: '/first', quality: 'full' }]);
    second.sendPlan([{ fileHash: 'second', url: '/second', quality: 'thumbnail' }]);
    firstWorker.onmessage?.({
      data: { type: 'bitmap', fileHash: 'first', quality: 'full', bitmap: {} },
    } as MessageEvent);

    expect(firstWorker.postMessage).toHaveBeenCalledWith({
      type: 'plan', entries: [{ fileHash: 'first', url: '/first', quality: 'full' }],
    });
    expect(secondWorker.postMessage).toHaveBeenCalledWith({
      type: 'plan', entries: [{ fileHash: 'second', url: '/second', quality: 'thumbnail' }],
    });
    expect(firstBitmap).toHaveBeenCalledWith('first', {}, 'full');
    expect(secondBitmap).not.toHaveBeenCalled();

    first.invalidate('first');
    expect(firstWorker.postMessage).toHaveBeenCalledWith({ type: 'invalidate', fileHash: 'first' });

    first.terminate();
    expect(firstWorker.terminate).toHaveBeenCalledOnce();
    expect(secondWorker.terminate).not.toHaveBeenCalled();
  });

  it('forwards actionable decode failure details', () => {
    const worker = fakeWorker();
    const onError = vi.fn();
    const client = new ThumbnailDecodeClient(vi.fn(), onError, () => worker as unknown as Worker);
    client.sendPlan([]);
    const failure = {
      url: 'media://localhost/file/deadbeef.png',
      stage: 'decode',
      message: 'The source image could not be decoded',
      attempt: 2,
      terminal: true,
    } as const;

    worker.onmessage?.({
      data: { type: 'error', fileHash: 'deadbeef', quality: 'full', failure },
    } as MessageEvent);

    expect(onError).toHaveBeenCalledWith('deadbeef', 'full', failure);
  });
});
