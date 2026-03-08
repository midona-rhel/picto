import { useEffect, useMemo, useRef, useState } from 'react';
import LayoutWorker from '../layoutWorker?worker';
import {
  bucketIndexEntriesToMap,
  buildBucketIndex,
  computeLayout,
  type LayoutResult,
} from '../layoutMath';
import type { GridViewMode } from '../runtime';
import type { MasonryImageItem } from '../shared';
import type { LayoutWorkerResponse } from '../layoutWorker';

const WORKER_LAYOUT_THRESHOLD = 180;

interface UseWaterfallLayoutWorkerArgs {
  images: MasonryImageItem[];
  layoutWidth: number;
  targetSize: number;
  gap: number;
  viewMode: GridViewMode;
  textHeight: number;
  paddingX: number;
}

interface SettledSnapshot {
  signature: string;
  images: MasonryImageItem[];
  layout: LayoutResult;
  bucketIndex: Map<number, number[]> | null;
}

interface UseWaterfallLayoutWorkerResult {
  renderImages: MasonryImageItem[];
  layout: LayoutResult;
  bucketIndex: Map<number, number[]> | null;
  pending: boolean;
  usedWorker: boolean;
}

function buildSignature(
  images: MasonryImageItem[],
  layoutWidth: number,
  targetSize: number,
  gap: number,
  textHeight: number,
  paddingX: number,
): string {
  const firstHash = images[0]?.hash ?? '';
  const lastHash = images[images.length - 1]?.hash ?? '';
  return [
    images.length,
    firstHash,
    lastHash,
    layoutWidth,
    targetSize,
    gap,
    textHeight,
    paddingX,
  ].join(':');
}

export function useWaterfallLayoutWorker({
  images,
  layoutWidth,
  targetSize,
  gap,
  viewMode,
  textHeight,
  paddingX,
}: UseWaterfallLayoutWorkerArgs): UseWaterfallLayoutWorkerResult {
  const shouldUseWorker = viewMode === 'waterfall' && images.length >= WORKER_LAYOUT_THRESHOLD && layoutWidth > 0;
  const signature = useMemo(
    () => buildSignature(images, layoutWidth, targetSize, gap, textHeight, paddingX),
    [images, layoutWidth, targetSize, gap, textHeight, paddingX],
  );
  const aspectRatios = useMemo(() => images.map((image) => image.aspectRatio), [images]);

  const workerRef = useRef<Worker | null>(null);
  const workerDisabledRef = useRef(false);
  const latestRequestIdRef = useRef(0);
  const latestRequestSignatureRef = useRef('');
  const latestImagesRef = useRef<MasonryImageItem[]>(images);
  const [settledSnapshot, setSettledSnapshot] = useState<SettledSnapshot | null>(null);
  const [pendingSignature, setPendingSignature] = useState<string | null>(null);

  const syncLayout = useMemo(() => {
    if (shouldUseWorker && settledSnapshot) {
      return null;
    }
    return computeLayout(images, layoutWidth, targetSize, gap, viewMode, textHeight, paddingX);
  }, [shouldUseWorker, settledSnapshot, images, layoutWidth, targetSize, gap, viewMode, textHeight, paddingX]);

  useEffect(() => {
    if (!shouldUseWorker || workerDisabledRef.current) {
      setPendingSignature(null);
      return;
    }

    let worker = workerRef.current;
    if (!worker) {
      try {
        worker = new LayoutWorker();
        workerRef.current = worker;
      } catch {
        workerDisabledRef.current = true;
        setPendingSignature(null);
        return;
      }
    }

    const requestId = latestRequestIdRef.current + 1;
    latestRequestIdRef.current = requestId;
    latestRequestSignatureRef.current = signature;
    latestImagesRef.current = images;
    setPendingSignature(signature);

    worker.onmessage = (event: MessageEvent<LayoutWorkerResponse>) => {
      const { requestId: responseId, layout, bucketEntries } = event.data;
      if (responseId !== latestRequestIdRef.current) return;
      setSettledSnapshot({
        signature: latestRequestSignatureRef.current,
        images: latestImagesRef.current,
        layout,
        bucketIndex: bucketIndexEntriesToMap(bucketEntries),
      });
      setPendingSignature(null);
    };

    worker.onerror = () => {
      workerDisabledRef.current = true;
      setPendingSignature(null);
    };

    worker.postMessage({
      requestId,
      aspectRatios,
      containerWidth: layoutWidth,
      targetSize,
      gap,
      viewMode,
      textHeight,
      paddingX,
    });
  }, [shouldUseWorker, signature, images, aspectRatios, layoutWidth, targetSize, gap, viewMode, textHeight, paddingX]);

  useEffect(() => {
    return () => {
      workerRef.current?.terminate();
      workerRef.current = null;
    };
  }, []);

  if (!shouldUseWorker || workerDisabledRef.current) {
    const layout = syncLayout ?? computeLayout(images, layoutWidth, targetSize, gap, viewMode, textHeight, paddingX);
    return {
      renderImages: images,
      layout,
      bucketIndex: viewMode === 'waterfall' ? buildBucketIndex(layout.positions) : null,
      pending: false,
      usedWorker: false,
    };
  }

  if (settledSnapshot?.signature === signature) {
    return {
      renderImages: settledSnapshot.images,
      layout: settledSnapshot.layout,
      bucketIndex: settledSnapshot.bucketIndex,
      pending: pendingSignature === signature,
      usedWorker: true,
    };
  }

  if (settledSnapshot) {
    return {
      renderImages: settledSnapshot.images,
      layout: settledSnapshot.layout,
      bucketIndex: settledSnapshot.bucketIndex,
      pending: true,
      usedWorker: true,
    };
  }

  const initialLayout = syncLayout ?? computeLayout(images, layoutWidth, targetSize, gap, viewMode, textHeight, paddingX);
  return {
    renderImages: images,
    layout: initialLayout,
    bucketIndex: buildBucketIndex(initialLayout.positions),
    pending: true,
    usedWorker: true,
  };
}
