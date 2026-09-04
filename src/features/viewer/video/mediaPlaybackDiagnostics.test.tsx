import { fireEvent, render } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { clearDiagnostics, getDiagnosticsSnapshot } from '../../diagnostics/diagnosticsStore';
import { useVideoPlayer } from './useVideoPlayer';
import { playMedia } from './mediaPlaybackDiagnostics';

beforeEach(clearDiagnostics);
afterEach(() => { vi.restoreAllMocks(); delete (window as any).picto; });

function Player() {
  const { videoRef } = useVideoPlayer();
  return <video ref={videoRef} src="media://localhost/file/failing.webm" />;
}

it('logs decoder failures with the file and playback state', () => {
  const emit = vi.fn().mockResolvedValue(undefined);
  (window as any).picto = { events: { emit } };
  const { container } = render(<Player />);
  const media = container.querySelector('video')!;
  Object.defineProperty(media, 'error', { value: { code: 3, message: 'PIPELINE_ERROR_DECODE' } });
  fireEvent.error(media);
  const entry = getDiagnosticsSnapshot().find(entry => entry.target === 'media.playback')!;
  expect(entry.level).toBe('ERROR');
  expect(emit).toHaveBeenCalledWith('picto:log', expect.objectContaining({ target: 'media.playback', source: 'renderer' }));
  expect(JSON.parse(entry.message)).toMatchObject({
    source: 'media://localhost/file/failing.webm', code: 'MEDIA_ERR_DECODE',
    message: 'PIPELINE_ERROR_DECODE', currentTime: 0, readyState: 0,
  });
});

it('reports rejected play but ignores normal navigation cancellation', async () => {
  const media = document.createElement('video');
  vi.spyOn(media, 'play').mockRejectedValueOnce(new DOMException('navigation', 'AbortError'))
    .mockRejectedValueOnce(new DOMException('unsupported source', 'NotSupportedError'));
  await playMedia(media);
  expect(getDiagnosticsSnapshot()).toHaveLength(0);
  await playMedia(media);
  expect(getDiagnosticsSnapshot()).toHaveLength(1);
  expect(getDiagnosticsSnapshot()[0].message).toContain('unsupported source');
});
