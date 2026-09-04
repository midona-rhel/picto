import { addDiagnostic } from '../../diagnostics/diagnosticsStore';

const MEDIA_ERRORS: Record<number, string> = {
  1: 'MEDIA_ERR_ABORTED', 2: 'MEDIA_ERR_NETWORK',
  3: 'MEDIA_ERR_DECODE', 4: 'MEDIA_ERR_SRC_NOT_SUPPORTED',
};

export function reportMediaPlaybackFailure(media: HTMLMediaElement, error?: unknown) {
  const reason = error instanceof Error || error instanceof DOMException ? `${error.name}: ${error.message}` : String(error ?? '');
  reportPlaybackFailure(media.currentSrc || media.src, {
    code: MEDIA_ERRORS[media.error?.code ?? 0] ?? 'PLAY_REJECTED',
    message: media.error?.message || reason,
    currentTime: media.currentTime, duration: media.duration,
    readyState: media.readyState, networkState: media.networkState,
    paused: media.paused,
    videoWidth: media instanceof HTMLVideoElement ? media.videoWidth : undefined,
    videoHeight: media instanceof HTMLVideoElement ? media.videoHeight : undefined,
  });
}

export function reportPlaybackFailure(source: string, details: Record<string, unknown>) {
  const entry = {
    level: 'ERROR', source: 'renderer', target: 'media.playback',
    timestamp: new Date().toISOString(),
    message: JSON.stringify({ source, ...details }),
  } as const;
  addDiagnostic(entry);
  // Standalone detail windows have their own renderer store. Relay to the
  // main window's diagnostics as well; broadcasts exclude the sender.
  void (window as any).picto?.events.emit('picto:log', entry).catch(() => {});
}

export function playMedia(media: HTMLMediaElement) {
  return media.play().catch((error: unknown) => {
    // Switching/closing media aborts pending play normally. A MediaError is
    // reported by the element's error listener, not twice by its play promise.
    if ((error instanceof Error || error instanceof DOMException) && error.name === 'AbortError') return;
    if (!media.error) reportMediaPlaybackFailure(media, error);
  });
}
