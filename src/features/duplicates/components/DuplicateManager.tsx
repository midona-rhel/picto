import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Loader, Text, Kbd } from '@mantine/core';
import { EmptyState } from '../../../shared/components/EmptyState';
import { TextButton } from '../../../shared/components/TextButton';
import { notifySuccess, notifyError, notifyInfo, notifyWarning } from '../../../shared/lib/notify';
import {
  IconArrowLeft,
  IconArrowRight,
  IconCopy,
  IconRefresh,
  IconWand,
  IconX,
  IconCheck,
} from '@tabler/icons-react';
import { api } from '#desktop/api';
import { mediaFileUrl, mediaThumbnailUrl } from '../../../shared/lib/mediaUrl';
import { isImagePreloaded, queueImageDecode } from '../../../shared/lib/useImagePreloader';
import type { DuplicatePairDto, DuplicatePairsResponse, ResolveDuplicateAction } from '../../../shared/types/api';
import { useDomainStore } from '../../../state/domainStore';
import { registerUndoAction } from '../../../shared/controllers/undoRedoController';
import { useGlobalKeydown } from '../../../shared/hooks/useGlobalKeydown';
import { useImageZoom } from '../../viewer/hooks/useImageZoom';
import { useNavigatorRenderer } from '../../viewer/hooks/useNavigatorRenderer';
import { useNavigatorDrag } from '../../viewer/hooks/useNavigatorDrag';
import styles from './DuplicateManager.module.css';

const PERIODIC_SCAN_INTERVAL_MS = 5 * 60 * 1000; // 5 minutes

interface PairFileInfo {
  hash: string;
  name: string;
  size: number;
  mime: string;
  width: number;
  height: number;
  rating: number | null;
  tags: string[];
  sourceUrls: string[];
  imageUrl: string;
  thumbUrl: string;
}


function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function getSimilarityColor(pct: number): string {
  if (pct >= 99) return 'var(--color-negative, red)';
  if (pct >= 95) return 'var(--color-warning, orange)';
  return 'var(--color-text-secondary)';
}

export function DuplicateManager() {
  const [pairs, setPairs] = useState<DuplicatePairDto[]>([]);
  const [totalPairs, setTotalPairs] = useState(0);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [hasMore, setHasMore] = useState(false);
  const [scanning, setScanning] = useState(false);
  const [currentIndex, setCurrentIndex] = useState(0);
  const [processing, setProcessing] = useState(false);
  const [leftFile, setLeftFile] = useState<PairFileInfo | null>(null);
  const [rightFile, setRightFile] = useState<PairFileInfo | null>(null);
  const [resolvedCount, setResolvedCount] = useState(0);
  const [leftDecoded, setLeftDecoded] = useState(false);
  const [rightDecoded, setRightDecoded] = useState(false);
  const initialTotalRef = useRef(0);
  const autoScanAttemptedRef = useRef(false);
  const scanningRef = useRef(false);
  const processingRef = useRef(false);

  scanningRef.current = scanning;
  processingRef.current = processing;

  const currentPair = pairs[currentIndex] ?? null;

  // Zoom infrastructure — left pane is the zoom container so that focal point,
  // containerSize, and navigator rect are all computed against a single pane.
  const leftPaneImageRef = useRef<HTMLDivElement>(null);
  const rightPaneImageRef = useRef<HTMLDivElement>(null);
  const leftFrameRef = useRef<HTMLDivElement>(null);
  const rightFrameRef = useRef<HTMLDivElement>(null);
  const navigatorRef = useRef<HTMLDivElement>(null);
  const navViewportRef = useRef<HTMLDivElement>(null);

  // Use the larger image's dimensions as reference so both frames render at the
  // same visual size — the smaller/lower-quality duplicate will show pixelation.
  const imageSize = useMemo(() => {
    if (!leftFile && !rightFile) return null;
    const lw = leftFile?.width ?? 0;
    const lh = leftFile?.height ?? 0;
    const rw = rightFile?.width ?? 0;
    const rh = rightFile?.height ?? 0;
    const leftArea = lw * lh;
    const rightArea = rw * rh;
    return leftArea >= rightArea
      ? { width: lw, height: lh }
      : { width: rw, height: rh };
  }, [leftFile?.width, leftFile?.height, rightFile?.width, rightFile?.height]);
  const imageSizeRef = useRef(imageSize);
  imageSizeRef.current = imageSize;

  // Stable reference — prevents useImageZoom callback cascade on every render
  const transformTargets = useMemo(() => [leftFrameRef, rightFrameRef], []);

  const {
    state: zoomState,
    setState: setZoomState,
    isDragging,
    navigatorRect,
    panToNormalized,
    onLiveFrameRef,
    containerSize,
    handlers: zoomHandlers,
  } = useImageZoom(leftPaneImageRef, imageSize, { transformTargets });

  // Right pane also supports wheel zoom — translate focal point relative to that pane
  useEffect(() => {
    const rightPane = rightPaneImageRef.current;
    if (!rightPane) return;
    const handleWheel = (e: WheelEvent) => {
      e.preventDefault();
      const rect = rightPane.getBoundingClientRect();
      const focalX = e.clientX - rect.left - rect.width / 2;
      const focalY = e.clientY - rect.top - rect.height / 2;
      const sensitivity = 0.004;
      const multiplier = Math.exp(-e.deltaY * sensitivity);
      setZoomState(prev => {
        const newScale = Math.min(8, Math.max(0.05, prev.scale * multiplier));
        const ratio = newScale / prev.scale;
        return { scale: newScale, tx: focalX - ratio * (focalX - prev.tx), ty: focalY - ratio * (focalY - prev.ty) };
      }, true);
    };
    rightPane.addEventListener('wheel', handleWheel, { passive: false });
    return () => rightPane.removeEventListener('wheel', handleWheel);
  }, [setZoomState]);

  // Fit-to-pane: read pane dimensions directly from DOM (not state — avoids async race)
  const fitToPane = useCallback(() => {
    const pane = leftPaneImageRef.current;
    if (!pane || !imageSize) return false;
    const pw = pane.clientWidth;
    const ph = pane.clientHeight;
    if (pw === 0 || ph === 0) return false;
    const scale = Math.min(pw / imageSize.width, ph / imageSize.height, 1);
    setZoomState({ scale, tx: 0, ty: 0 });
    return true;
  }, [imageSize, setZoomState]);

  const dummyImgRef = useRef<HTMLImageElement>(null);
  useNavigatorRenderer(
    dummyImgRef, navigatorRef, navViewportRef, imageSizeRef,
    zoomState, navigatorRect, 120, undefined, onLiveFrameRef, containerSize,
  );

  const handleNavMouseDown = useNavigatorDrag(navigatorRef, imageSizeRef, panToNormalized);

  // Reset zoom when pair changes. We track a composite key of pair hash + image
  // dimensions so that navigating to a new pair always re-fits, even if the effect
  // fires before leftFile has updated (stale imageSize from previous pair).
  const fitToPaneRef = useRef(fitToPane);
  fitToPaneRef.current = fitToPane;
  const lastFitKeyRef = useRef<string | null>(null);
  useEffect(() => {
    if (!imageSize || !currentPair) return;
    const key = `${currentPair.hash_a}:${imageSize.width}x${imageSize.height}`;
    if (lastFitKeyRef.current === key) return;
    if (fitToPaneRef.current()) {
      lastFitKeyRef.current = key;
    }
  }, [currentPair?.hash_a, imageSize, containerSize]);

  /** Push the live duplicate count to the sidebar immediately (bypasses compiler lag). */
  const refreshDuplicateCount = useCallback(async () => {
    try {
      const { count } = await api.duplicates.getCount();
      useDomainStore.getState().setDuplicatesCount(count);
    } catch {
      // Non-critical — event bridge will eventually sync
    }
  }, []);

  const loadPairs = useCallback(async () => {
    try {
      setLoading(true);
      const result: DuplicatePairsResponse = await api.duplicates.getPairs(null, 200, 'detected');
      setPairs(result.items);
      setTotalPairs(result.total);
      setNextCursor(result.next_cursor);
      setHasMore(result.has_more);
      setCurrentIndex(0);
      initialTotalRef.current = result.total;
      setResolvedCount(0);
    } catch (err) {
      console.error('Failed to load duplicate pairs:', err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadPairs();
  }, [loadPairs]);

  const loadMorePairs = useCallback(async () => {
    if (!hasMore || !nextCursor || loadingMore) return;
    try {
      setLoadingMore(true);
      const result: DuplicatePairsResponse = await api.duplicates.getPairs(nextCursor, 200, 'detected');
      setPairs((prev) => [...prev, ...result.items]);
      setNextCursor(result.next_cursor);
      setHasMore(result.has_more);
      setTotalPairs(result.total);
    } catch (err) {
      console.error('Failed to load more duplicate pairs:', err);
    } finally {
      setLoadingMore(false);
    }
  }, [hasMore, nextCursor, loadingMore]);

  useEffect(() => {
    if (!currentPair) {
      setLeftFile(null);
      setRightFile(null);
      return;
    }

    const loadFileInfo = async () => {
      try {
        const batch = await api.grid.getEntitiesMetadataBatch([
          currentPair.hash_a,
          currentPair.hash_b,
        ]);

        const buildInfo = (hash: string): PairFileInfo => {
          const meta = batch.items[hash];
          const mime = meta?.entity.mime ?? 'image/jpeg';
          return {
            hash,
            name: meta?.entity.name ?? `${hash.slice(0, 12)}...`,
            size: meta?.entity.size ?? 0,
            mime,
            width: meta?.entity.width ?? 0,
            height: meta?.entity.height ?? 0,
            rating: meta?.entity.rating ?? null,
            tags: meta?.tags.map((t) => t.display_tag) ?? [],
            sourceUrls: meta?.entity.source_urls ?? [],
            imageUrl: mediaFileUrl(hash, mime),
            thumbUrl: mediaThumbnailUrl(hash),
          };
        };

        setLeftFile(buildInfo(currentPair.hash_a));
        setRightFile(buildInfo(currentPair.hash_b));
      } catch (err) {
        console.error('Failed to load file metadata:', err);
      }
    };

    loadFileInfo();
  }, [currentPair?.hash_a, currentPair?.hash_b]);

  useEffect(() => {
    const leftUrl = leftFile?.imageUrl ?? '';
    const rightUrl = rightFile?.imageUrl ?? '';
    setLeftDecoded(leftUrl ? isImagePreloaded(leftUrl) : false);
    setRightDecoded(rightUrl ? isImagePreloaded(rightUrl) : false);
    const cancels: (() => void)[] = [];
    if (leftUrl && !isImagePreloaded(leftUrl)) {
      cancels.push(queueImageDecode(leftUrl, () => setLeftDecoded(true), 'high'));
    }
    if (rightUrl && !isImagePreloaded(rightUrl)) {
      cancels.push(queueImageDecode(rightUrl, () => setRightDecoded(true), 'high'));
    }
    return () => cancels.forEach((c) => c());
  }, [leftFile?.imageUrl, rightFile?.imageUrl]);

  useEffect(() => {
    if (loading || loadingMore || !hasMore) return;
    if (pairs.length === 0 || currentIndex >= pairs.length - 5) {
      void loadMorePairs();
    }
  }, [loading, loadingMore, hasMore, pairs.length, currentIndex, loadMorePairs]);

  const goToNext = useCallback(() => {
    if (currentIndex < pairs.length - 1) {
      setCurrentIndex((i) => i + 1);
    }
  }, [currentIndex, pairs.length]);

  const goToPrev = useCallback(() => {
    if (currentIndex > 0) {
      setCurrentIndex((i) => i - 1);
    }
  }, [currentIndex]);

  const handleAction = useCallback(
    async (action: ResolveDuplicateAction) => {
      if (!currentPair || processing) return;
      try {
        setProcessing(true);
        const pairSnapshot = currentPair;
        await api.duplicates.resolvePair(action, currentPair.hash_a, currentPair.hash_b);

        if (action === 'keep_left' || action === 'keep_right' || action === 'not_duplicate' || action === 'keep_both') {
          const loserHash = action === 'keep_left'
            ? pairSnapshot.hash_b
            : action === 'keep_right'
              ? pairSnapshot.hash_a
              : null;
          registerUndoAction({
            label: `Resolve duplicate (${action})`,
            undo: async () => {
              if (loserHash) {
                await api.files.setStatus(loserHash, 'active');
              }
              // Re-scan to re-detect/open the pair state.
              await api.duplicates.scan();
              await loadPairs();
            },
            redo: async () => {
              await api.duplicates.resolvePair(action, pairSnapshot.hash_a, pairSnapshot.hash_b);
              await loadPairs();
            },
          });
        }

        setPairs((prev) => {
          const updated = [...prev];
          updated.splice(currentIndex, 1);
          const nextLen = updated.length;
          setCurrentIndex((idx) => Math.min(idx, Math.max(0, nextLen - 1)));
          return updated;
        });
        setResolvedCount((c) => c + 1);

        const labels: Record<string, string> = {
          smart_merge: 'Smart merged',
          keep_left: 'Kept left',
          keep_right: 'Kept right',
          not_duplicate: 'Marked as not duplicate',
          keep_both: 'Kept both',
        };
        notifySuccess(labels[action] ?? 'Resolved', 'Done');

        await refreshDuplicateCount();
      } catch (err) {
        notifyError(err);
      } finally {
        setProcessing(false);
      }
    },
    [currentPair, processing, currentIndex, loadPairs, refreshDuplicateCount],
  );

  const handleDuplicateHotkeys = useCallback((e: KeyboardEvent) => {
    const target = e.target as HTMLElement;
    if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable) return;

    switch (e.key) {
      case 'ArrowLeft':
        e.preventDefault();
        goToPrev();
        break;
      case 'ArrowRight':
        e.preventDefault();
        goToNext();
        break;
      case 's':
      case 'S':
        e.preventDefault();
        handleAction('smart_merge');
        break;
      case 'l':
      case 'L':
        e.preventDefault();
        handleAction('keep_left');
        break;
      case 'r':
      case 'R':
        e.preventDefault();
        handleAction('keep_right');
        break;
      case 'n':
      case 'N':
        e.preventDefault();
        handleAction('not_duplicate');
        break;
      case 'f':
      case 'F':
      case '0':
        e.preventDefault();
        fitToPane();
        break;
    }
  }, [goToPrev, goToNext, handleAction, fitToPane]);
  useGlobalKeydown(handleDuplicateHotkeys);

  const scanForDuplicates = useCallback(async () => {
    try {
      setScanning(true);
      const result = await api.duplicates.scan();
      if (result.reviewable_detected_new > 0) {
        notifyInfo(
          `Found ${result.reviewable_detected_new} new duplicate pair(s) (${result.reviewable_detected_total} in review queue)`,
          'Scan Complete',
        );
      } else if (result.reviewable_detected_total > 0) {
        notifyInfo(
          `${result.reviewable_detected_total} duplicate pair(s) in review queue`,
          'Scan Complete',
        );
      } else if (result.candidates_found > 0) {
        notifyInfo(
          'No reviewable pairs found (exact matches may have auto-merged)',
          'Scan Complete',
        );
      } else {
        notifySuccess('No duplicates found', 'Scan Complete');
      }
      await loadPairs();
      void refreshDuplicateCount();
    } catch (err) {
      notifyError('Failed to scan');
      console.error(err);
    } finally {
      setScanning(false);
    }
  }, [loadPairs, refreshDuplicateCount]);

  useEffect(() => {
    if (loading || scanning) return;
    if (pairs.length > 0) return;
    if (autoScanAttemptedRef.current) return;
    autoScanAttemptedRef.current = true;
    void scanForDuplicates();
  }, [loading, scanning, pairs.length, scanForDuplicates]);

  useEffect(() => {
    const timer = setInterval(async () => {
      if (scanningRef.current || processingRef.current) return;
      try {
        const result = await api.duplicates.scan();
        void refreshDuplicateCount();
        if (result.reviewable_detected_total > 0) {
          const fresh = await api.duplicates.getPairs(null, 200, 'detected');
          setPairs(fresh.items);
          setTotalPairs(fresh.total);
          setNextCursor(fresh.next_cursor);
          setHasMore(fresh.has_more);
        } else {
          setPairs([]);
          setTotalPairs(0);
          setNextCursor(null);
          setHasMore(false);
          setCurrentIndex(0);
        }
      } catch {
        // Silent failure for periodic scan
      }
    }, PERIODIC_SCAN_INTERVAL_MS);

    return () => clearInterval(timer);
  }, [refreshDuplicateCount]);

  const showCompare = !loading && pairs.length > 0;
  const showLoading = loading || (!loading && pairs.length === 0 && loadingMore);
  const showEmpty = !loading && pairs.length === 0 && !loadingMore;

  const totalForProgress = initialTotalRef.current || totalPairs || pairs.length;
  const progressPercent = totalForProgress > 0 ? (resolvedCount / totalForProgress) * 100 : 0;

  return (
    <div className={styles.root}>
      {/* Loading / empty overlays */}
      {showLoading && (
        <div className={styles.centeredState}>
          <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 12 }}>
            <Loader size="lg" />
            <Text c="dimmed">Loading duplicate pairs...</Text>
          </div>
        </div>
      )}
      {showEmpty && (
        <div className={styles.centeredState}>
          <EmptyState
            icon={IconCopy}
            title={resolvedCount > 0 ? 'All Resolved' : 'No Duplicates Found'}
            description={
              resolvedCount > 0
                ? `All ${resolvedCount} duplicate pair(s) have been resolved`
                : 'Scan your library to detect duplicate images using perceptual hashing'
            }
            action={
              <TextButton onClick={scanForDuplicates} disabled={scanning}>
                <IconRefresh size={14} />
                {scanning ? 'Scanning...' : 'Scan for Duplicates'}
              </TextButton>
            }
          />
        </div>
      )}

      {/* Always render the layout so zoom refs mount and effects register */}
      <div className={styles.topBar} style={showCompare ? undefined : { visibility: 'hidden' }}>
        <div className={styles.topBarLeft}>
          <Text fw={600} size="sm">
            Duplicate Review
          </Text>
          <Text size="xs" c="dimmed">
            Pair {currentIndex + 1} of {pairs.length}{hasMore ? '+' : ''} (total {totalPairs})
          </Text>
          {currentPair && (
            <Text
              size="xs"
              className={styles.similarity}
              style={{ color: getSimilarityColor(currentPair.similarity_pct) }}
            >
              {currentPair.similarity_pct}% similar
            </Text>
          )}
        </div>
        <div className={styles.topBarRight}>
          <TextButton compact onClick={scanForDuplicates} disabled={scanning}>
            <IconRefresh size={14} />
            Re-scan
          </TextButton>
          {hasMore && (
            <TextButton compact onClick={() => void loadMorePairs()} disabled={loadingMore}>
              {loadingMore ? 'Loading…' : 'Load More'}
            </TextButton>
          )}
          <Text size="xs" c="dimmed">
            {resolvedCount} resolved
          </Text>
        </div>
      </div>

      <div className={styles.progressBar} style={showCompare ? undefined : { visibility: 'hidden' }}>
        <div className={styles.progressFill} style={{ width: `${progressPercent}%` }} />
      </div>

      <div className={styles.compareArea} style={showCompare ? undefined : { visibility: 'hidden' }}>
        <div className={styles.pane}>
          {/* paneImage always rendered so leftPaneImageRef is always in the DOM for useImageZoom */}
          <div ref={leftPaneImageRef} className={`${styles.paneImage}${isDragging ? ` ${styles.dragging}` : ''}`} onMouseDown={zoomHandlers.onMouseDown}>
            {leftFile && imageSize && (
              <div
                ref={leftFrameRef}
                className={styles.paneImageFrame}
                style={{ width: imageSize.width, height: imageSize.height }}
              >
                <img src={leftFile.thumbUrl} alt={leftFile.name} className={styles.paneImg} />
                {leftDecoded && (
                  <img src={leftFile.imageUrl} alt={leftFile.name} className={`${styles.paneImg} ${styles.paneFull}`} />
                )}
              </div>
            )}
            {/* Navigator inside left pane, positioned relative to image area */}
            <div
              ref={navigatorRef}
              className={styles.navigator}
              onMouseDown={handleNavMouseDown}
              style={{ display: 'none' }}
            >
              {leftFile && <img src={leftFile.thumbUrl} alt="" />}
              <div ref={navViewportRef} className={styles.navigatorViewport} />
            </div>
          </div>
          {leftFile && (
            <div className={styles.paneMeta}>
              <div className={styles.metaRow}>
                <span className={styles.metaLabel}>Name</span>
                <span className={styles.metaValue}>{leftFile.name}</span>
              </div>
              <div className={styles.metaRow}>
                <span className={styles.metaLabel}>Size</span>
                <span className={styles.metaValue}>
                  {leftFile.width}x{leftFile.height} &middot; {formatSize(leftFile.size)}
                </span>
              </div>
              <div className={styles.metaRow}>
                <span className={styles.metaLabel}>Format</span>
                <span className={styles.metaValue}>{leftFile.mime}</span>
              </div>
              <div className={styles.metaRow}>
                <span className={styles.metaLabel}>Tags</span>
                <span className={styles.metaValue}>{leftFile.tags.length}</span>
              </div>
            </div>
          )}
        </div>

        <div className={styles.actionColumn}>
          <button
            className={styles.actionBtnPrimary}
            onClick={() => handleAction('smart_merge')}
            disabled={processing}
          >
            <IconWand size={14} /> Smart Merge
            <span className={styles.actionKbd}>S</span>
          </button>
          <button
            className={styles.actionBtn}
            onClick={() => handleAction('keep_left')}
            disabled={processing}
          >
            <IconArrowLeft size={14} /> Keep Left
            <span className={styles.actionKbd}>L</span>
          </button>
          <button
            className={styles.actionBtn}
            onClick={() => handleAction('keep_right')}
            disabled={processing}
          >
            Keep Right <IconArrowRight size={14} />
            <span className={styles.actionKbd}>R</span>
          </button>
          <button
            className={styles.actionBtn}
            onClick={() => handleAction('not_duplicate')}
            disabled={processing}
          >
            <IconX size={14} /> Not Duplicate
            <span className={styles.actionKbd}>N</span>
          </button>
          <button
            className={styles.actionBtn}
            onClick={() => handleAction('keep_both')}
            disabled={processing}
          >
            <IconCheck size={14} /> Keep Both
          </button>
        </div>

        <div className={styles.pane}>
          {/* paneImage always rendered for right pane wheel handler */}
          <div ref={rightPaneImageRef} className={`${styles.paneImage}${isDragging ? ` ${styles.dragging}` : ''}`} onMouseDown={zoomHandlers.onMouseDown}>
            {rightFile && imageSize && (
              <div
                ref={rightFrameRef}
                className={styles.paneImageFrame}
                style={{ width: imageSize.width, height: imageSize.height }}
              >
                <img src={rightFile.thumbUrl} alt={rightFile.name} className={styles.paneImg} />
                {rightDecoded && (
                  <img src={rightFile.imageUrl} alt={rightFile.name} className={`${styles.paneImg} ${styles.paneFull}`} />
                )}
              </div>
            )}
          </div>
          {rightFile && (
            <div className={styles.paneMeta}>
              <div className={styles.metaRow}>
                <span className={styles.metaLabel}>Name</span>
                <span className={styles.metaValue}>{rightFile.name}</span>
              </div>
              <div className={styles.metaRow}>
                <span className={styles.metaLabel}>Size</span>
                <span className={styles.metaValue}>
                  {rightFile.width}x{rightFile.height} &middot; {formatSize(rightFile.size)}
                </span>
              </div>
              <div className={styles.metaRow}>
                <span className={styles.metaLabel}>Format</span>
                <span className={styles.metaValue}>{rightFile.mime}</span>
              </div>
              <div className={styles.metaRow}>
                <span className={styles.metaLabel}>Tags</span>
                <span className={styles.metaValue}>{rightFile.tags.length}</span>
              </div>
            </div>
          )}
        </div>
      </div>

      <div className={styles.bottomBar} style={showCompare ? undefined : { visibility: 'hidden' }}>
        <TextButton onClick={goToPrev} disabled={currentIndex === 0 || processing}>
          <IconArrowLeft size={14} /> Prev
        </TextButton>
        <span className={styles.kbdHint}>
          <Kbd size="xs">S</Kbd> merge
        </span>
        <span className={styles.kbdHint}>
          <Kbd size="xs">L</Kbd> left
        </span>
        <span className={styles.kbdHint}>
          <Kbd size="xs">R</Kbd> right
        </span>
        <span className={styles.kbdHint}>
          <Kbd size="xs">N</Kbd> not dup
        </span>
        <span className={styles.kbdHint}>
          <Kbd size="xs">&larr;</Kbd>
          <Kbd size="xs">&rarr;</Kbd> navigate
        </span>
        <TextButton onClick={goToNext} disabled={currentIndex >= pairs.length - 1 || processing}>
          Next <IconArrowRight size={14} />
        </TextButton>
      </div>
    </div>
  );
}
