import { useState, useCallback, useRef, useLayoutEffect } from 'react';
import { isImagePreloaded } from './useImagePreloader';

/**
 * Manages image loaded/ready state across viewer components.
 * Tracks the decoded URL — empty string means "not ready, show thumbnail".
 * The preloader calls markImageReady(url) after decode() resolves,
 * guaranteeing the bitmap is cached before the DOM <img> renders it.
 */
export function useImageLoadState(
  currentHash: string | null,
  currentUrl?: string | null,
  _fitToWindow?: unknown,
  _zoomCache?: unknown,
  onNavigate?: () => void,
): {
  decodedSrc: string;
  imageLoaded: boolean;
  markImageReady: (url: string) => void;
} {
  const [loaded, setLoaded] = useState<{ hash: string | null; src: string }>({
    hash: null,
    src: '',
  });
  const imageReadyRef = useRef(false);
  const navStartedAtRef = useRef(0);
  const pendingTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Reset on image change. If already decoded, show full image immediately to
  // avoid thumbnail flash during rapid navigation.
  useLayoutEffect(() => {
    if (pendingTimerRef.current) {
      clearTimeout(pendingTimerRef.current);
      pendingTimerRef.current = null;
    }
    navStartedAtRef.current = performance.now();
    if (currentHash && currentUrl && isImagePreloaded(currentUrl)) {
      imageReadyRef.current = true;
      setLoaded({ hash: currentHash, src: currentUrl });
    } else {
      imageReadyRef.current = false;
      setLoaded({ hash: null, src: '' });
    }
    onNavigate?.();
  }, [currentHash, currentUrl]); // eslint-disable-line react-hooks/exhaustive-deps

  const markImageReady = useCallback((url: string) => {
    if (imageReadyRef.current || !currentHash) return;
    const minThumbMs = 90;
    const elapsed = performance.now() - navStartedAtRef.current;
    const commit = () => {
      if (imageReadyRef.current) return;
      imageReadyRef.current = true;
      setLoaded({ hash: currentHash, src: url });
    };
    if (elapsed >= minThumbMs) {
      commit();
      return;
    }
    pendingTimerRef.current = setTimeout(() => {
      pendingTimerRef.current = null;
      commit();
    }, minThumbMs - elapsed);
  }, [currentHash]); // eslint-disable-line react-hooks/exhaustive-deps

  const isCurrentLoaded = loaded.hash === currentHash && loaded.src !== '';
  return {
    decodedSrc: isCurrentLoaded ? loaded.src : '',
    imageLoaded: isCurrentLoaded,
    markImageReady,
  };
}
