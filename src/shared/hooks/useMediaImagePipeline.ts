/**
 * useMediaImagePipeline — flicker-free two-layer image loading.
 *
 * Key behavior: the CURRENT image stays visible until the NEXT image's
 * thumbnail is fully decoded. No blank frames during navigation.
 *
 * Flow:
 * 1. Hash changes → start preloading new thumbnail in background
 * 2. Keep showing old image until new thumbnail is ready
 * 3. Once ready → swap displayed content atomically
 * 4. After 100ms delay, start loading full-res, fade in over thumbnail
 * 5. Prefetch neighbor thumbnails
 */

import { useState, useEffect, useCallback, type SyntheticEvent } from 'react';
import { mediaThumbnailUrl, mediaFileUrl } from '../lib/mediaUrl';

export interface MediaPipelineInput {
  hash: string | null;
  thumbnailHash: string | null;
  /** An already-prepared preview source supplied by an outer viewer handoff. */
  thumbnailUrlOverride?: string;
  mime: string;
  isVideo: boolean;
  neighborHashes?: string[];
  /** Delay before requesting full resolution; viewers may opt into immediate promotion. */
  fullResolutionDelayMs?: number;
  /** Keep the previous frame until the original can replace a missing thumbnail. */
  fallbackToFullResolution?: boolean;
}

export interface MediaPipelineOutput {
  /** The hash currently being DISPLAYED (may lag behind requested hash). */
  displayedHash: string | null;
  thumbUrl: string;
  fullUrl: string;
  thumbLoaded: boolean;
  /** The first thumbnail request has either decoded or failed, so an opaque viewer can paint safely. */
  thumbSettled: boolean;
  fullVisible: boolean;
  handleThumbLoad: (e: SyntheticEvent<HTMLImageElement>) => void;
  handleFullLoad: (e: SyntheticEvent<HTMLImageElement>) => void;
}

export function useMediaImagePipeline({
  hash,
  thumbnailHash,
  thumbnailUrlOverride,
  mime,
  isVideo,
  neighborHashes = [],
  fullResolutionDelayMs = 100,
  fallbackToFullResolution = false,
}: MediaPipelineInput): MediaPipelineOutput {
  // What's currently shown to the user (lags behind `hash` until new thumb is ready)
  // Start empty so the first render takes the same thumbnail-first path as navigation.
  const [displayedHash, setDisplayedHash] = useState<string | null>(null);
  const [displayedThumbnailHash, setDisplayedThumbnailHash] = useState<string | null>(null);
  const [thumbUrl, setThumbUrl] = useState('');
  const [fullUrl, setFullUrl] = useState('');
  const [thumbLoaded, setThumbLoaded] = useState(false);
  const [thumbSettled, setThumbSettled] = useState(false);
  const [fullVisible, setFullVisible] = useState(false);

  // Preload the next thumbnail in the background. Only swap when ready.
  useEffect(() => {
    if (!hash) {
      setDisplayedHash(null);
      setDisplayedThumbnailHash(null);
      setThumbUrl('');
      setFullUrl('');
      setThumbSettled(false);
      return;
    }

    const requestedThumbnailHash = thumbnailHash ?? hash;
    if (hash === displayedHash && requestedThumbnailHash === displayedThumbnailHash) return;

    if (isVideo) {
      // Videos don't have thumbnails to preload — swap immediately
      setDisplayedHash(hash);
      setDisplayedThumbnailHash(requestedThumbnailHash);
      setThumbUrl('');
      setFullUrl('');
      setThumbLoaded(true);
      setThumbSettled(true);
      setFullVisible(false);
      return;
    }

    const newThumbUrl = thumbnailUrlOverride ?? mediaThumbnailUrl(requestedThumbnailHash);
    let cancelled = false;

    const img = new Image();
    const commitThumbnail = (url = newThumbUrl) => {
      if (cancelled) return;
      setDisplayedHash(hash);
      setDisplayedThumbnailHash(requestedThumbnailHash);
      setThumbUrl(url);
      setThumbLoaded(true);
      setThumbSettled(true);
      setFullVisible(false);
      setFullUrl('');
    };
    img.onload = () => {
      if (typeof img.decode === 'function') {
        img.decode().then(() => commitThumbnail()).catch(() => commitThumbnail());
      }
      else commitThumbnail();
    };
    img.onerror = () => {
      if (cancelled) return;
      if (fallbackToFullResolution) {
        const fallbackUrl = mediaFileUrl(requestedThumbnailHash, mime);
        const original = new Image();
        original.onload = () => {
          if (typeof original.decode === 'function') {
            original.decode().then(() => commitThumbnail(fallbackUrl)).catch(() => commitThumbnail(fallbackUrl));
          } else {
            commitThumbnail(fallbackUrl);
          }
        };
        original.onerror = () => {
          if (cancelled) return;
          setDisplayedHash(hash);
          setDisplayedThumbnailHash(requestedThumbnailHash);
          setThumbUrl(newThumbUrl);
          setThumbLoaded(false);
          setThumbSettled(true);
          setFullVisible(false);
          setFullUrl('');
        };
        original.src = fallbackUrl;
        return;
      }
      // Still swap even on error — show broken state rather than stuck on old image
      setDisplayedHash(hash);
      setDisplayedThumbnailHash(requestedThumbnailHash);
      setThumbUrl(newThumbUrl);
      setThumbLoaded(false);
      setThumbSettled(true);
      setFullVisible(false);
      setFullUrl('');
    };
    img.src = newThumbUrl;

    return () => { cancelled = true; };
  }, [hash, thumbnailHash, thumbnailUrlOverride, isVideo, fallbackToFullResolution, mime]); // eslint-disable-line react-hooks/exhaustive-deps

  // First mount: show immediately (no previous image to hold)
  useEffect(() => {
    if (hash && !displayedHash) {
      const url = thumbnailUrlOverride ?? mediaThumbnailUrl(thumbnailHash ?? hash);
      setDisplayedHash(hash);
      setDisplayedThumbnailHash(thumbnailHash ?? hash);
      setThumbUrl(url);
      setThumbLoaded(false);
    }
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Full-res URL — delayed 100ms after displayed hash commits
  useEffect(() => {
    const fileHash = thumbnailHash ?? displayedHash;
    if (!fileHash || !displayedHash || isVideo) { setFullUrl(''); return; }
    // Only load full-res for the currently displayed hash
    if (displayedHash !== hash) return; // Still transitioning
    const nextFullUrl = mediaFileUrl(fileHash, mime);
    if (fullResolutionDelayMs <= 0) {
      setFullUrl(nextFullUrl);
      return;
    }
    const timer = setTimeout(() => setFullUrl(nextFullUrl), fullResolutionDelayMs);
    return () => clearTimeout(timer);
  }, [displayedHash, fullResolutionDelayMs, hash, thumbnailHash, mime, isVideo]);

  // Prefetch neighbor thumbnails
  useEffect(() => {
    for (const h of neighborHashes) { const img = new Image(); img.src = mediaThumbnailUrl(h); }
  }, [neighborHashes]);

  const handleThumbLoad = useCallback((e: SyntheticEvent<HTMLImageElement>) => {
    const img = e.currentTarget;
    const reveal = () => {
      setThumbLoaded(true);
      setThumbSettled(true);
    };
    if (typeof img.decode === 'function') img.decode().then(reveal).catch(reveal);
    else reveal();
  }, []);

  const handleFullLoad = useCallback((e: SyntheticEvent<HTMLImageElement>) => {
    const img = e.currentTarget;
    const reveal = () => {
      img.style.display = '';
      setFullVisible(true);
    };
    if (typeof img.decode === 'function') { img.decode().then(reveal).catch(reveal); }
    else { reveal(); }
  }, []);

  return { displayedHash, thumbUrl, fullUrl, thumbLoaded, thumbSettled, fullVisible, handleThumbLoad, handleFullLoad };
}
