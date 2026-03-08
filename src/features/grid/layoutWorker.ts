/// <reference lib="webworker" />

import { buildBucketIndexEntries, computeLayoutFromAspectRatios, type BucketIndexEntry, type LayoutResult } from './layoutMath';
import type { GridViewMode } from './runtime';

export interface LayoutWorkerRequest {
  requestId: number;
  aspectRatios: number[];
  containerWidth: number;
  targetSize: number;
  gap: number;
  viewMode: GridViewMode;
  textHeight: number;
  paddingX: number;
}

export interface LayoutWorkerResponse {
  requestId: number;
  layout: LayoutResult;
  bucketEntries: BucketIndexEntry[];
}

const worker = self as DedicatedWorkerGlobalScope;

worker.onmessage = (event: MessageEvent<LayoutWorkerRequest>) => {
  const {
    requestId,
    aspectRatios,
    containerWidth,
    targetSize,
    gap,
    viewMode,
    textHeight,
    paddingX,
  } = event.data;

  const layout = computeLayoutFromAspectRatios(
    aspectRatios,
    containerWidth,
    targetSize,
    gap,
    viewMode,
    textHeight,
    paddingX,
  );
  const bucketEntries = viewMode === 'waterfall'
    ? buildBucketIndexEntries(layout.positions)
    : [];

  worker.postMessage({
    requestId,
    layout,
    bucketEntries,
  } satisfies LayoutWorkerResponse);
};

export {};
