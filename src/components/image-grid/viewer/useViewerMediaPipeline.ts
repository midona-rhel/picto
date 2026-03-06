import { useState, useEffect, useLayoutEffect, useRef, useCallback, type MutableRefObject, type RefObject, type SyntheticEvent } from 'react';
import { api } from '#desktop/api';
import { mediaFileUrl, mediaThumbnailUrl } from '../../../lib/mediaUrl';
import { useImagePreloader, queueImageDecode, isImagePreloaded } from '../useImagePreloader';
import { useImageLoadState } from '../useImageLoadState';
import { useZoomCache } from '../useZoomCache';
import type { ImageSize, ZoomState } from '../useImageZoom';
import type { MasonryImageItem } from '../shared';
import { logBestEffortError } from '../../../lib/asyncOps';
import { buildNeighborDecodePlan } from './preloadPlan';

export interface ViewerMediaPipelineInput {
  currentImage: MasonryImageItem | null;
  images: Array<Pick<MasonryImageItem, 'hash' | 'mime'>>;
  currentIndex: number;
  imageUrl: string;
  isVideo: boolean;
  imageSize: ImageSize | null;
  zoomState: ZoomState;
  setZoomState: (s: ZoomState) => void;
  calcFitScale: (size: ImageSize) => number;
  containerRef: RefObject<HTMLElement | null>;
  imgRef: RefObject<HTMLImageElement | null>;
  zoomCache: Map<string, ZoomState>;
  skipFullQualityDecode?: boolean;
  skipNeighborPrefetch?: boolean;
  onHashChangeStart?: () => void;
  onImageReady?: () => void;
  ensureThumbLogContext: string;
  preloadThumbLogContext: string;
}

export interface ViewerMediaPipelineOutput {
  decodedSrc: string;
  imageLoaded: boolean;
  thumbLoaded: boolean;
  fullImageVisible: boolean;
  displayThumbUrl: string;
  allowFullQuality: boolean;
  backdropSrcRef: MutableRefObject<string>;
  lastLoadedSrcRef: MutableRefObject<string>;
  setThumbLoaded: (loaded: boolean) => void;
  setFullImageVisible: (visible: boolean) => void;
  handleThumbLoad: (event: SyntheticEvent<HTMLImageElement>) => void;
  handleFullLoad: (event: SyntheticEvent<HTMLImageElement>) => void;
}

export function useViewerMediaPipeline({
  currentImage,
  images,
  currentIndex,
  imageUrl,
  isVideo,
  imageSize,
  zoomState,
  setZoomState,
  calcFitScale,
  containerRef,
  imgRef,
  zoomCache,
  skipFullQualityDecode = false,
  skipNeighborPrefetch = false,
  onHashChangeStart,
  onImageReady,
  ensureThumbLogContext,
  preloadThumbLogContext,
}: ViewerMediaPipelineInput): ViewerMediaPipelineOutput {
  const { decodedSrc, imageLoaded, markImageReady } = useImageLoadState(
    currentImage?.hash ?? null,
    imageUrl || null,
    undefined,
    zoomCache,
    onHashChangeStart,
  );
  const [fullImageVisible, setFullImageVisible] = useState(false);
  const [thumbLoaded, setThumbLoaded] = useState(false);
  const [displayThumbUrl, setDisplayThumbUrl] = useState('');
  const [allowFullQuality, setAllowFullQuality] = useState(false);
  const backdropSrcRef = useRef('');
  const lastLoadedSrcRef = useRef('');

  useLayoutEffect(() => {
    if (lastLoadedSrcRef.current) {
      backdropSrcRef.current = lastLoadedSrcRef.current;
    }
    setFullImageVisible(false);
    setThumbLoaded(isVideo);
    setAllowFullQuality(false);
    const img = imgRef.current;
    if (img) {
      img.style.transition = 'none';
      img.style.opacity = '0';
    }
  }, [currentImage?.hash]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    const timer = setTimeout(() => setAllowFullQuality(true), 100);
    return () => clearTimeout(timer);
  }, [currentImage?.hash]);

  useEffect(() => {
    if (!currentImage) return;
    const targetThumb = mediaThumbnailUrl(currentImage.hash);
    let cancelled = false;
    let cancelDecode: (() => void) | null = null;
    setDisplayThumbUrl('');
    void (async () => {
      try {
        await api.file.ensureThumbnail(currentImage.hash);
      } catch (error) {
        logBestEffortError(ensureThumbLogContext, error);
      }
      if (cancelled) return;
      const ensuredThumb = `${targetThumb}?ready=${Date.now()}`;
      cancelDecode = queueImageDecode(
        ensuredThumb,
        () => {
          if (cancelled) return;
          setDisplayThumbUrl(ensuredThumb);
        },
        'high',
      );
    })();
    return () => {
      cancelled = true;
      if (cancelDecode) cancelDecode();
    };
  }, [currentImage?.hash, ensureThumbLogContext]);

  useEffect(() => {
    if (!onImageReady || !imageLoaded) return;
    onImageReady();
  }, [imageLoaded, onImageReady]);

  useZoomCache(
    currentImage?.hash ?? null,
    imageSize,
    zoomState,
    setZoomState,
    calcFitScale,
    zoomCache,
    imageLoaded,
    containerRef,
  );

  useImagePreloader(
    allowFullQuality && !skipFullQualityDecode ? (imageUrl || null) : null,
    isVideo,
    markImageReady,
  );

  useEffect(() => {
    if (skipNeighborPrefetch || images.length === 0) return;
    const cleanups: Array<() => void> = [];
    const plan = buildNeighborDecodePlan(images, currentIndex);
    for (const thumbTask of plan.thumbs) {
      const thumbUrl = mediaThumbnailUrl(thumbTask.hash);
      if (isImagePreloaded(thumbUrl)) continue;
      void api.file.ensureThumbnail(thumbTask.hash)
        .catch((error) => {
          logBestEffortError(preloadThumbLogContext, error);
        })
        .finally(() => {
          queueImageDecode(`${thumbUrl}?ready=${Date.now()}`, () => {}, thumbTask.priority);
        });
    }
    for (const fullTask of plan.fulls) {
      const fullUrl = mediaFileUrl(fullTask.hash, fullTask.mime);
      if (isImagePreloaded(fullUrl)) continue;
      cleanups.push(queueImageDecode(fullUrl, () => {}, fullTask.priority));
    }
    return () => {
      for (const cleanup of cleanups) cleanup();
    };
  }, [skipNeighborPrefetch, images, currentIndex, preloadThumbLogContext]);

  const handleThumbLoad = useCallback((event: SyntheticEvent<HTMLImageElement>) => {
    lastLoadedSrcRef.current = event.currentTarget.src;
    setThumbLoaded(true);
  }, []);

  const handleFullLoad = useCallback((event: SyntheticEvent<HTMLImageElement>) => {
    const img = event.currentTarget;
    lastLoadedSrcRef.current = img.src;
    const revealFull = () => setFullImageVisible(true);
    if (typeof img.decode === 'function') {
      img.decode().then(revealFull).catch(revealFull);
    } else {
      revealFull();
    }
  }, []);

  return {
    decodedSrc,
    imageLoaded,
    thumbLoaded,
    fullImageVisible,
    displayThumbUrl,
    allowFullQuality,
    backdropSrcRef,
    lastLoadedSrcRef,
    setThumbLoaded,
    setFullImageVisible,
    handleThumbLoad,
    handleFullLoad,
  };
}
