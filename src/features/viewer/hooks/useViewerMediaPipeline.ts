/**
 * useViewerMediaPipeline — flicker-free two-layer image loading.
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

import { useState, useEffect, useCallback, type RefObject, type SyntheticEvent } from 'react';
import { mediaThumbnailUrl, mediaFileUrl } from '../../../shared/lib/mediaUrl';

export interface MediaPipelineInput {
  hash: string | null;
  thumbnailHash: string | null;
  mime: string;
  isVideo: boolean;
  imgRef: RefObject<HTMLImageElement | null>;
  neighborHashes?: string[];
}

export interface MediaPipelineOutput {
  /** The hash currently being DISPLAYED (may lag behind requested hash). */
  displayedHash: string | null;
  thumbUrl: string;
  fullUrl: string;
  thumbLoaded: boolean;
  fullVisible: boolean;
  handleThumbLoad: (e: SyntheticEvent<HTMLImageElement>) => void;
  handleFullLoad: (e: SyntheticEvent<HTMLImageElement>) => void;
}

export function useViewerMediaPipeline({
  hash,
  thumbnailHash,
  mime,
  isVideo,
  imgRef,
  neighborHashes = [],
}: MediaPipelineInput): MediaPipelineOutput {
  // What's currently shown to the user (lags behind `hash` until new thumb is ready)
  const [displayedHash, setDisplayedHash] = useState(hash);
  const [thumbUrl, setThumbUrl] = useState('');
  const [fullUrl, setFullUrl] = useState('');
  const [thumbLoaded, setThumbLoaded] = useState(false);
  const [fullVisible, setFullVisible] = useState(false);

  // Preload the next thumbnail in the background. Only swap when ready.
  useEffect(() => {
    if (!hash) { setDisplayedHash(null); setThumbUrl(''); setFullUrl(''); return; }
    if (hash === displayedHash) return; // Already showing this hash

    if (isVideo) {
      // Videos don't have thumbnails to preload — swap immediately
      setDisplayedHash(hash);
      setThumbUrl('');
      setFullUrl('');
      setThumbLoaded(true);
      setFullVisible(false);
      return;
    }

    const newThumbUrl = mediaThumbnailUrl(thumbnailHash ?? hash);
    let cancelled = false;

    const img = new Image();
    img.onload = () => {
      if (cancelled) return;
      // New thumbnail is decoded — commit the swap
      setDisplayedHash(hash);
      setThumbUrl(newThumbUrl);
      setThumbLoaded(true);
      setFullVisible(false);
      setFullUrl('');
      // Reset full-res image opacity
      const fullImg = imgRef.current;
      if (fullImg) { fullImg.style.transition = 'none'; fullImg.style.opacity = '0'; }
    };
    img.onerror = () => {
      if (cancelled) return;
      // Still swap even on error — show broken state rather than stuck on old image
      setDisplayedHash(hash);
      setThumbUrl(newThumbUrl);
      setThumbLoaded(false);
      setFullVisible(false);
      setFullUrl('');
    };
    img.src = newThumbUrl;

    return () => { cancelled = true; };
  }, [hash, thumbnailHash, isVideo]); // eslint-disable-line react-hooks/exhaustive-deps

  // First mount: show immediately (no previous image to hold)
  useEffect(() => {
    if (hash && !displayedHash) {
      const url = mediaThumbnailUrl(thumbnailHash ?? hash);
      setDisplayedHash(hash);
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
    const timer = setTimeout(() => setFullUrl(mediaFileUrl(fileHash, mime)), 100);
    return () => clearTimeout(timer);
  }, [displayedHash, hash, thumbnailHash, mime, isVideo]);

  // Prefetch neighbor thumbnails
  useEffect(() => {
    for (const h of neighborHashes) { const img = new Image(); img.src = mediaThumbnailUrl(h); }
  }, [neighborHashes]);

  const handleThumbLoad = useCallback((_e: SyntheticEvent<HTMLImageElement>) => {
    setThumbLoaded(true);
  }, []);

  const handleFullLoad = useCallback((e: SyntheticEvent<HTMLImageElement>) => {
    const img = e.currentTarget;
    const reveal = () => { img.style.transition = 'opacity 130ms ease-out'; img.style.opacity = '1'; setFullVisible(true); };
    if (typeof img.decode === 'function') { img.decode().then(reveal).catch(reveal); }
    else { reveal(); }
  }, []);

  return { displayedHash, thumbUrl, fullUrl, thumbLoaded, fullVisible, handleThumbLoad, handleFullLoad };
}
