import { useCallback, useEffect, useRef, useState } from 'react';
import {
  IconArrowLeft,
  IconArrowRight,
  IconArrowsJoin,
  IconAspectRatio,
  IconCheck,
  IconCopy,
  IconLayersDifference,
  IconMinus,
  IconPlus,
  IconRefresh,
  IconX,
} from '@tabler/icons-react';
import { getEntityDetails } from '../../platform/entityApi';
import {
  getDuplicatePairs,
  resolveDuplicatePair,
  scanDuplicates,
  type DuplicateAction,
  type DuplicatePair,
} from '../../platform/duplicateApi';
import type { CanonicalEntityDetails } from '../../shared/types/canonical';
import { mediaFileUrl, mediaThumbnailUrl } from '../../shared/lib/mediaUrl';
import { showErrorNotification, showInfoNotification, showWarningNotification } from '../../shared/lib/notifications';
import { useMediaImagePipeline } from '../../shared/hooks/useMediaImagePipeline';
import btnStyles from '../../shared/styles/actionButton.module.css';
import iconStyles from '../../shared/styles/iconButton.module.css';
import { KbdTooltip } from '../../shared/ui/KbdTooltip';
import { EmptyState, EmptyStateAction } from '../../shared/ui/EmptyState';
import { ProgressBar } from '../../shared/ui/ProgressBar';
import { PropertyRow } from '../../shared/ui/PropertyRow/PropertyRow';
import { useLinkedComparisonZoom } from './useLinkedComparisonZoom';
import styles from './DuplicatesScreen.module.css';

const PAGE_SIZE = 100;
const LOADING_MESSAGE_DELAY_MS = 200;

interface LoadPairsOptions {
  showLoading?: boolean;
  resetProgress?: boolean;
}

function useDelayedFlag(active: boolean, delayMs = LOADING_MESSAGE_DELAY_MS): boolean {
  const [visible, setVisible] = useState(false);
  useEffect(() => {
    if (!active) {
      setVisible(false);
      return;
    }
    const timeout = window.setTimeout(() => setVisible(true), delayMs);
    return () => window.clearTimeout(timeout);
  }, [active, delayMs]);
  return visible;
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function dimensions(details: CanonicalEntityDetails): string {
  if (details.pixel_width == null || details.pixel_height == null) return 'Unknown';
  return `${details.pixel_width} x ${details.pixel_height}`;
}

interface MediaCardProps {
  side: 'left' | 'right';
  previewRef: React.RefObject<HTMLDivElement>;
  zoom: ReturnType<typeof useLinkedComparisonZoom>;
  differenceActive: boolean;
  differenceImages: { left: CanonicalEntityDetails; right: CanonicalEntityDetails } | null;
  details: CanonicalEntityDetails | null;
  loading: boolean;
  onKeep: () => void;
  disabled: boolean;
}

function DifferenceComposite({
  side,
  images,
  zoom,
}: {
  side: 'left' | 'right';
  images: { left: CanonicalEntityDetails; right: CanonicalEntityDetails };
  zoom: ReturnType<typeof useLinkedComparisonZoom>;
}) {
  const aspectRatio = images.left.pixel_width && images.left.pixel_height
    ? images.left.pixel_width / images.left.pixel_height
    : 1;
  const imageUrl = (details: CanonicalEntityDetails) => mediaFileUrl(details.entity_hash, details.mime_type);
  const fallback = (details: CanonicalEntityDetails) => mediaThumbnailUrl(details.entity_hash);
  return (
    <div
      className={`${styles.previewLayers} ${styles.differenceComposite}`}
      style={zoom.differenceFrameStyle(side, aspectRatio)}
      data-testid={`${side}-difference-composite`}
    >
      <img
        src={imageUrl(images.left)}
        onError={(event) => { event.currentTarget.onerror = null; event.currentTarget.src = fallback(images.left); }}
        alt=""
        draggable={false}
      />
      <img
        className={styles.differenceLayer}
        src={imageUrl(images.right)}
        onError={(event) => { event.currentTarget.onerror = null; event.currentTarget.src = fallback(images.right); }}
        alt=""
        draggable={false}
      />
    </div>
  );
}

function MediaCard({ side, previewRef, zoom, differenceActive, differenceImages, details, loading, onKeep, disabled }: MediaCardProps) {
  const label = side === 'left' ? 'Left candidate' : 'Right candidate';
  const fullImgRef = useRef<HTMLImageElement>(null);
  const pipeline = useMediaImagePipeline({
    hash: details?.entity_hash ?? null,
    thumbnailHash: details?.entity_hash ?? null,
    mime: details?.mime_type ?? '',
    isVideo: false,
    imgRef: fullImgRef,
  });
  const thumbnailUrl = details ? mediaThumbnailUrl(details.entity_hash) : '';
  return (
    <article className={styles.mediaCard}>
      <header className={styles.cardHeader}>
        <span className={styles.sideLabel}>{label}</span>
        <KbdTooltip label={`Keep ${side}`} shortcut={side === 'left' ? 'L' : 'R'}>
          <button className={btnStyles.btn} onClick={onKeep} disabled={disabled || !details} aria-label={`Keep ${side}`}>
            <IconCheck size={15} /> <span className={styles.keepLabel}>Keep {side}</span>
          </button>
        </KbdTooltip>
      </header>
      <div
        ref={previewRef}
        data-testid={`${side}-preview`}
        className={`${styles.preview} ${zoom.draggingSide === side ? styles.previewDragging : ''}`}
        onPointerDown={(event) => zoom.handlers.onPointerDown(side, event)}
        onPointerMove={(event) => zoom.handlers.onPointerMove(side, event)}
        onPointerUp={zoom.handlers.onPointerUp}
        onPointerCancel={zoom.handlers.onPointerUp}
        onDoubleClick={zoom.fit}
      >
        {loading && !details && <div className={styles.previewState}>Loading metadata...</div>}
        {!loading && !details && <div className={styles.previewState}>Media unavailable</div>}
        {details && !differenceActive && (
          <div className={styles.previewLayers} style={zoom.frameStyle(side)} data-testid={`${side}-preview-layers`}>
            <img
              className={styles.thumbnailImage}
              src={pipeline.thumbUrl || thumbnailUrl}
              onLoad={pipeline.handleThumbLoad}
              alt={details.name ?? label}
              draggable={false}
            />
            {pipeline.fullUrl && (
              <img
                ref={fullImgRef}
                className={styles.fullImage}
                src={pipeline.fullUrl}
                onLoad={pipeline.handleFullLoad}
                onError={(event) => { event.currentTarget.style.display = 'none'; }}
                alt=""
                decoding="async"
                draggable={false}
              />
            )}
          </div>
        )}
        {differenceActive && differenceImages && (
          <DifferenceComposite side={side} images={differenceImages} zoom={zoom} />
        )}
      </div>
      {details && (
        <div className={styles.metadata}>
          <div className={styles.mediaName} title={details.name ?? details.entity_hash}>
            {details.name ?? details.entity_hash}
          </div>
          <PropertyRow label="Resolution" value={dimensions(details)} />
          <PropertyRow label="Size" value={formatBytes(details.size_bytes)} />
          <PropertyRow label="Format" value={details.mime_type} />
          <PropertyRow label="Rating" value={details.rating == null ? 'Unrated' : String(details.rating)} />
          <PropertyRow label="Tags" value={String(details.tags.length)} />
          <PropertyRow label="Added" value={new Date(details.date_added).toLocaleDateString()} />
        </div>
      )}
    </article>
  );
}

export function DuplicatesScreen() {
  const [pairs, setPairs] = useState<DuplicatePair[]>([]);
  const [index, setIndex] = useState(0);
  const [total, setTotal] = useState(0);
  const [initialTotal, setInitialTotal] = useState(0);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [hasMore, setHasMore] = useState(false);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [scanning, setScanning] = useState(false);
  const [resolving, setResolving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [left, setLeft] = useState<CanonicalEntityDetails | null>(null);
  const [right, setRight] = useState<CanonicalEntityDetails | null>(null);
  const [metadataLoading, setMetadataLoading] = useState(false);
  const [differenceHovered, setDifferenceHovered] = useState(false);
  const [differenceFocused, setDifferenceFocused] = useState(false);
  const requestIdRef = useRef(0);
  const scanningRef = useRef(false);
  const leftPreviewRef = useRef<HTMLDivElement>(null);
  const rightPreviewRef = useRef<HTMLDivElement>(null);

  const currentPair = pairs[index] ?? null;
  const resolvedCount = Math.max(0, initialTotal - total);
  const zoom = useLinkedComparisonZoom({
    leftContainerRef: leftPreviewRef,
    rightContainerRef: rightPreviewRef,
    leftImageSize: left?.pixel_width && left.pixel_height
      ? { width: left.pixel_width, height: left.pixel_height }
      : null,
    rightImageSize: right?.pixel_width && right.pixel_height
      ? { width: right.pixel_width, height: right.pixel_height }
      : null,
    pairKey: currentPair ? `${currentPair.hash_a}:${currentPair.hash_b}` : '',
  });
  const differenceImages = left && right ? { left, right } : null;
  const differenceActive = !metadataLoading && (differenceHovered || differenceFocused);
  const showLoadingMessage = useDelayedFlag(loading);
  const showScanProgress = useDelayedFlag(scanning);

  const reportFailure = useCallback((cause: unknown, title = 'Duplicate review failed') => {
    const message = cause instanceof Error ? cause.message : String(cause);
    setError(message);
    showErrorNotification({ title, message });
  }, []);

  const loadPairs = useCallback(async ({
    showLoading = true,
    resetProgress = true,
  }: LoadPairsOptions = {}) => {
    if (showLoading) setLoading(true);
    setError(null);
    try {
      const page = await getDuplicatePairs({ limit: PAGE_SIZE });
      setPairs(page.items);
      setTotal(page.total);
      if (resetProgress) setInitialTotal(page.total);
      setNextCursor(page.next_cursor);
      setHasMore(page.has_more);
      setIndex((current) => resetProgress ? 0 : Math.min(current, Math.max(0, page.items.length - 1)));
    } catch (cause) {
      reportFailure(cause, 'Unable to load duplicate review');
    } finally {
      if (showLoading) setLoading(false);
    }
  }, [reportFailure]);

  useEffect(() => {
    void loadPairs();
  }, [loadPairs]);

  useEffect(() => {
    if (!currentPair) {
      requestIdRef.current += 1;
      setLeft(null);
      setRight(null);
      return;
    }
    const requestId = ++requestIdRef.current;
    setMetadataLoading(true);
    setLeft(null);
    setRight(null);
    Promise.all([
      getEntityDetails(currentPair.hash_a),
      getEntityDetails(currentPair.hash_b),
    ])
      .then(([nextLeft, nextRight]) => {
        if (requestId !== requestIdRef.current) return;
        setLeft(nextLeft);
        setRight(nextRight);
      })
      .catch((cause) => {
        if (requestId !== requestIdRef.current) return;
        reportFailure(cause, 'Unable to load duplicate media');
      })
      .finally(() => {
        if (requestId === requestIdRef.current) setMetadataLoading(false);
      });
  }, [currentPair, reportFailure]);

  const loadMore = useCallback(async () => {
    if (!hasMore || !nextCursor || loadingMore) return;
    setLoadingMore(true);
    try {
      const page = await getDuplicatePairs({ cursor: nextCursor, limit: PAGE_SIZE });
      setPairs((current) => {
        const known = new Set(current.map((pair) => `${pair.hash_a}:${pair.hash_b}`));
        return current.concat(page.items.filter((pair) => !known.has(`${pair.hash_a}:${pair.hash_b}`)));
      });
      setNextCursor(page.next_cursor);
      setHasMore(page.has_more);
      setTotal(page.total);
    } catch (cause) {
      reportFailure(cause, 'Unable to load more duplicate pairs');
    } finally {
      setLoadingMore(false);
    }
  }, [hasMore, loadingMore, nextCursor, reportFailure]);

  useEffect(() => {
    if (hasMore && index >= pairs.length - 4) void loadMore();
  }, [hasMore, index, loadMore, pairs.length]);

  const finishResolution = useCallback(async (
    pair: DuplicatePair,
    action: DuplicateAction,
  ) => {
    setResolving(true);
    setError(null);
    try {
      const result = await resolveDuplicatePair(
        action,
        pair.hash_a,
        pair.hash_b,
      );
      if (result.status === 'quality_ambiguous') {
        showWarningNotification({
          title: 'Smart merge needs a choice',
          message: 'No clear quality winner. Choose left or right, or keep both.',
        });
        return;
      }
      if (result.blob_cleanup_pending) {
        showWarningNotification({
          title: 'Duplicate cleanup pending',
          message: result.cleanup_error?.trim() || 'Blob cleanup will retry automatically.',
        });
      }
      await loadPairs({ showLoading: false, resetProgress: false });
    } catch (cause) {
      reportFailure(cause, 'Unable to resolve duplicate pair');
    } finally {
      setResolving(false);
    }
  }, [loadPairs, reportFailure]);

  const scan = useCallback(async () => {
    if (scanningRef.current) return;
    scanningRef.current = true;
    setScanning(true);
    setError(null);
    try {
      const summary = await scanDuplicates();
      showInfoNotification({
        title: 'Duplicate scan complete',
        message: summary.reviewable_detected_new > 0
          ? `Found ${summary.reviewable_detected_new} new review pairs`
          : 'Scan complete — no new review pairs',
      });
      await loadPairs({ showLoading: false, resetProgress: true });
    } catch (cause) {
      reportFailure(cause, 'Unable to scan for duplicates');
    } finally {
      scanningRef.current = false;
      setScanning(false);
    }
  }, [loadPairs, reportFailure]);

  const goPrevious = useCallback(() => setIndex((current) => Math.max(0, current - 1)), []);
  const goNext = useCallback(() => {
    setIndex((current) => Math.min(Math.max(0, pairs.length - 1), current + 1));
  }, [pairs.length]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (target?.matches('input, textarea, select, [contenteditable="true"]')) return;
      if (event.key === 'ArrowLeft') goPrevious();
      if (event.key === 'ArrowRight') goNext();
      if (!currentPair || resolving || metadataLoading) return;
      const shortcuts: Partial<Record<string, DuplicateAction>> = {
        l: 'keep_left',
        r: 'keep_right',
        s: 'smart_merge',
        n: 'not_duplicate',
      };
      const action = shortcuts[event.key.toLowerCase()];
      if (action) void finishResolution(currentPair, action);
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [currentPair, finishResolution, goNext, goPrevious, metadataLoading, resolving]);

  if (loading) {
    return (
      <div className={styles.centerState} aria-busy="true">
        {showLoadingMessage ? 'Loading duplicate review queue...' : null}
      </div>
    );
  }

  if (!currentPair) {
    const title = error
      ? 'Unable to load duplicate review'
      : resolvedCount > 0 ? 'Review complete' : 'No duplicate pairs';
    const description = error
      ? error
      : resolvedCount > 0
        ? `${resolvedCount} decisions saved.`
        : 'Scan the library to find similar images.';
    return (
      <EmptyState
        icon={<IconCopy size={28} stroke={1.2} style={{ color: 'var(--color-bg-app)' }} />}
        title={title}
        description={description}
        actions={(
          <EmptyStateAction onClick={scan} disabled={scanning || resolving}>
            <IconRefresh size={14} stroke={1.5} /> {error ? 'Retry' : 'Scan library'}
          </EmptyStateAction>
        )}
        progress={showScanProgress ? <ProgressBar indeterminate height={2} /> : null}
      />
    );
  }

  return (
    <section className={styles.root} aria-label="Duplicate review">
      <header className={styles.comparisonHeader}>
        <div className={styles.headerNav}>
          <KbdTooltip label="Previous pair" shortcut="ArrowLeft">
            <button className={`${iconStyles.iconBtn} ${index === 0 || resolving ? iconStyles.iconBtnDisabled : ''}`} onClick={goPrevious} disabled={index === 0 || resolving} aria-label="Previous pair">
              <IconArrowLeft size={17} />
            </button>
          </KbdTooltip>
          <span className={styles.position}>{index + 1} / {total}</span>
          <KbdTooltip label="Next pair" shortcut="ArrowRight">
            <button className={`${iconStyles.iconBtn} ${index >= pairs.length - 1 || resolving ? iconStyles.iconBtnDisabled : ''}`} onClick={goNext} disabled={index >= pairs.length - 1 || resolving} aria-label="Next pair">
              <IconArrowRight size={17} />
            </button>
          </KbdTooltip>
        </div>
        <div className={styles.zoomControls}>
          <KbdTooltip label="Zoom out">
            <button className={iconStyles.iconBtn} onClick={zoom.zoomOut} aria-label="Zoom out">
              <IconMinus size={16} />
            </button>
          </KbdTooltip>
          <span className={styles.zoomPercent}>{zoom.zoomPercent}%</span>
          <KbdTooltip label="Zoom in">
            <button className={iconStyles.iconBtn} onClick={zoom.zoomIn} aria-label="Zoom in">
              <IconPlus size={16} />
            </button>
          </KbdTooltip>
          <KbdTooltip label="Fit both images">
            <button className={`${iconStyles.iconBtn} ${zoom.isFit ? iconStyles.iconBtnFilled : ''}`} onClick={zoom.fit} aria-label="Fit both images" aria-pressed={zoom.isFit}>
              <IconAspectRatio size={16} />
            </button>
          </KbdTooltip>
          <KbdTooltip label="Hold to highlight differences">
            <button
              className={`${iconStyles.iconBtn} ${differenceActive ? iconStyles.iconBtnActive : ''}`}
              onMouseEnter={() => setDifferenceHovered(true)}
              onMouseLeave={() => setDifferenceHovered(false)}
              onFocus={() => setDifferenceFocused(true)}
              onBlur={() => setDifferenceFocused(false)}
              disabled={metadataLoading || !differenceImages}
              aria-label="Highlight differences"
              aria-pressed={differenceActive}
            >
              <IconLayersDifference size={16} />
            </button>
          </KbdTooltip>
        </div>
        <KbdTooltip label="Re-scan library">
          <button className={`${iconStyles.iconBtn} ${scanning || resolving ? iconStyles.iconBtnDisabled : ''}`} onClick={scan} disabled={scanning || resolving} aria-label="Re-scan library">
            <IconRefresh size={17} />
          </button>
        </KbdTooltip>
      </header>

      {showScanProgress && (
        <div className={styles.scanProgress} role="status" aria-label="Scanning duplicate pairs">
          <ProgressBar indeterminate height={2} />
        </div>
      )}

      <div className={styles.comparison}>
        <MediaCard side="left" previewRef={leftPreviewRef} zoom={zoom} differenceActive={differenceActive} differenceImages={differenceImages} details={left} loading={metadataLoading} disabled={resolving || metadataLoading} onKeep={() => void finishResolution(currentPair, 'keep_left')} />
        <MediaCard side="right" previewRef={rightPreviewRef} zoom={zoom} differenceActive={differenceActive} differenceImages={differenceImages} details={right} loading={metadataLoading} disabled={resolving || metadataLoading} onKeep={() => void finishResolution(currentPair, 'keep_right')} />
      </div>

      <footer className={styles.footer}>
        <div className={styles.footerActions}>
          <KbdTooltip label="These are different media" shortcut="N">
            <button className={btnStyles.btn} onClick={() => void finishResolution(currentPair, 'not_duplicate')} disabled={resolving || metadataLoading}>
              <IconX size={15} /> Not duplicates
            </button>
          </KbdTooltip>
          <button className={btnStyles.btn} onClick={() => void finishResolution(currentPair, 'keep_both')} disabled={resolving || metadataLoading}>
            <IconCopy size={15} /> Keep both
          </button>
          <KbdTooltip label="Keep the stronger file and merge metadata" shortcut="S">
            <button className={`${btnStyles.btn} ${btnStyles.btnPrimary}`} onClick={() => void finishResolution(currentPair, 'smart_merge')} disabled={resolving || metadataLoading}>
              <IconArrowsJoin size={16} /> Smart merge
            </button>
          </KbdTooltip>
          <span className={styles.similarity}>{currentPair.similarity_pct.toFixed(0)}% match</span>
        </div>
      </footer>

    </section>
  );
}
