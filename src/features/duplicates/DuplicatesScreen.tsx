import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  IconArrowLeft,
  IconArrowRight,
  IconArrowsJoin,
  IconCheck,
  IconCopy,
  IconRefresh,
  IconX,
} from '@tabler/icons-react';
import { getEntityDetails } from '../../platform/entityApi';
import {
  getDuplicatePairs,
  resolveDuplicatePair,
  scanDuplicates,
  type DuplicateAction,
  type DuplicateCollectionConflict,
  type DuplicatePair,
} from '../../platform/duplicateApi';
import type { CanonicalEntityDetails } from '../../shared/types/canonical';
import { mediaFileUrl, mediaThumbnailUrl } from '../../shared/lib/mediaUrl';
import { ProgressBar } from '../../shared/ui/ProgressBar';
import { PropertyRow } from '../../shared/ui/PropertyRow/PropertyRow';
import styles from './DuplicatesScreen.module.css';

const PAGE_SIZE = 100;

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function dimensions(details: CanonicalEntityDetails): string {
  if (details.pixel_width == null || details.pixel_height == null) return 'Unknown';
  return `${details.pixel_width} x ${details.pixel_height}`;
}

interface ConflictChoice {
  pair: DuplicatePair;
  action: DuplicateAction;
  conflict: DuplicateCollectionConflict;
}

interface MediaCardProps {
  side: 'left' | 'right';
  details: CanonicalEntityDetails | null;
  loading: boolean;
  onKeep: () => void;
  disabled: boolean;
}

function MediaCard({ side, details, loading, onKeep, disabled }: MediaCardProps) {
  const label = side === 'left' ? 'Left candidate' : 'Right candidate';
  const fullUrl = details ? mediaFileUrl(details.thumbnail_hash, details.mime_type) : '';
  return (
    <article className={styles.mediaCard}>
      <header className={styles.cardHeader}>
        <span className={styles.sideLabel}>{label}</span>
        <button className={styles.keepButton} onClick={onKeep} disabled={disabled || !details}>
          <IconCheck size={15} /> Keep {side}
        </button>
      </header>
      <div className={styles.preview}>
        {loading && <div className={styles.previewState}>Loading metadata...</div>}
        {!loading && !details && <div className={styles.previewState}>Media unavailable</div>}
        {details && (
          <img
            src={fullUrl}
            onError={(event) => {
              event.currentTarget.onerror = null;
              event.currentTarget.src = mediaThumbnailUrl(details.thumbnail_hash);
            }}
            alt={details.name ?? label}
            draggable={false}
          />
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
  const [notice, setNotice] = useState<string | null>(null);
  const [left, setLeft] = useState<CanonicalEntityDetails | null>(null);
  const [right, setRight] = useState<CanonicalEntityDetails | null>(null);
  const [metadataLoading, setMetadataLoading] = useState(false);
  const [conflict, setConflict] = useState<ConflictChoice | null>(null);
  const requestIdRef = useRef(0);

  const currentPair = pairs[index] ?? null;
  const resolvedCount = Math.max(0, initialTotal - total);

  const loadPairs = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const page = await getDuplicatePairs({ limit: PAGE_SIZE });
      setPairs(page.items);
      setTotal(page.total);
      setInitialTotal(page.total);
      setNextCursor(page.next_cursor);
      setHasMore(page.has_more);
      setIndex(0);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadPairs();
  }, [loadPairs]);

  useEffect(() => {
    if (!currentPair) {
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
        setError(cause instanceof Error ? cause.message : String(cause));
      })
      .finally(() => {
        if (requestId === requestIdRef.current) setMetadataLoading(false);
      });
  }, [currentPair]);

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
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setLoadingMore(false);
    }
  }, [hasMore, loadingMore, nextCursor]);

  useEffect(() => {
    if (hasMore && index >= pairs.length - 4) void loadMore();
  }, [hasMore, index, loadMore, pairs.length]);

  const removeCurrentPair = useCallback(() => {
    setPairs((current) => {
      const next = current.filter((_, pairIndex) => pairIndex !== index);
      setIndex((currentIndex) => Math.min(currentIndex, Math.max(0, next.length - 1)));
      return next;
    });
    setTotal((current) => Math.max(0, current - 1));
  }, [index]);

  const finishResolution = useCallback(async (
    pair: DuplicatePair,
    action: DuplicateAction,
    preferredCollectionId?: number,
  ) => {
    setResolving(true);
    setError(null);
    try {
      const result = await resolveDuplicatePair(
        action,
        pair.hash_a,
        pair.hash_b,
        preferredCollectionId,
      );
      if (result.status === 'conflict' && result.conflict) {
        setConflict({ pair, action, conflict: result.conflict });
        return;
      }
      setConflict(null);
      removeCurrentPair();
      setNotice(action === 'not_duplicate' ? 'Marked as different media.' : 'Duplicate decision saved.');
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setResolving(false);
    }
  }, [removeCurrentPair]);

  const scan = useCallback(async () => {
    setScanning(true);
    setError(null);
    setNotice(null);
    try {
      const summary = await scanDuplicates();
      setNotice(
        summary.reviewable_detected_new > 0
          ? `Found ${summary.reviewable_detected_new} new review pair${summary.reviewable_detected_new === 1 ? '' : 's'}.`
          : `Scan complete. ${summary.reviewable_detected_total} pair${summary.reviewable_detected_total === 1 ? '' : 's'} need review.`,
      );
      await loadPairs();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setScanning(false);
    }
  }, [loadPairs]);

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
      if (!currentPair || resolving || conflict) return;
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
  }, [conflict, currentPair, finishResolution, goNext, goPrevious, resolving]);

  const progressTotal = initialTotal || total;
  const similarity = useMemo(() => currentPair ? `${currentPair.similarity_pct.toFixed(0)}% similar` : '', [currentPair]);

  if (loading) {
    return <div className={styles.centerState}>Loading duplicate review queue...</div>;
  }

  if (!currentPair) {
    return (
      <section className={styles.emptyState}>
        <div className={styles.emptyIcon}><IconCopy size={30} /></div>
        <h1>{resolvedCount > 0 ? 'Review complete' : 'No duplicate pairs'}</h1>
        <p>{resolvedCount > 0 ? `${resolvedCount} decisions saved.` : 'Scan the library to compare perceptually similar images.'}</p>
        {error && <div className={styles.errorBanner}>{error}</div>}
        {notice && <div className={styles.noticeBanner}>{notice}</div>}
        <button className={styles.primaryButton} onClick={scan} disabled={scanning}>
          <IconRefresh size={16} /> {scanning ? 'Scanning...' : 'Scan library'}
        </button>
      </section>
    );
  }

  return (
    <section className={styles.root} aria-label="Duplicate review">
      <header className={styles.toolbar}>
        <div>
          <div className={styles.eyebrow}>Duplicate review</div>
          <h1>{similarity}</h1>
        </div>
        <div className={styles.toolbarMeta}>
          <span>{index + 1} of {total}</span>
          <button className={styles.secondaryButton} onClick={scan} disabled={scanning || resolving}>
            <IconRefresh size={15} /> {scanning ? 'Scanning...' : 'Re-scan'}
          </button>
        </div>
      </header>

      {progressTotal > 0 && <ProgressBar done={resolvedCount} total={progressTotal} height={2} />}
      {error && <div className={styles.errorBanner}>{error}</div>}
      {notice && <div className={styles.noticeBanner}>{notice}</div>}

      <div className={styles.comparison}>
        <MediaCard side="left" details={left} loading={metadataLoading} disabled={resolving} onKeep={() => void finishResolution(currentPair, 'keep_left')} />
        <div className={styles.divider}><span>VS</span></div>
        <MediaCard side="right" details={right} loading={metadataLoading} disabled={resolving} onKeep={() => void finishResolution(currentPair, 'keep_right')} />
      </div>

      <footer className={styles.actions}>
        <button className={styles.iconButton} onClick={goPrevious} disabled={index === 0 || resolving} aria-label="Previous pair">
          <IconArrowLeft size={18} />
        </button>
        <button className={styles.secondaryButton} onClick={() => void finishResolution(currentPair, 'not_duplicate')} disabled={resolving}>
          <IconX size={15} /> Not duplicates <kbd>N</kbd>
        </button>
        <button className={styles.secondaryButton} onClick={() => void finishResolution(currentPair, 'keep_both')} disabled={resolving}>
          <IconCopy size={15} /> Keep both
        </button>
        <button className={styles.primaryButton} onClick={() => void finishResolution(currentPair, 'smart_merge')} disabled={resolving}>
          <IconArrowsJoin size={16} /> Smart merge <kbd>S</kbd>
        </button>
        <button className={styles.iconButton} onClick={goNext} disabled={index >= pairs.length - 1 || resolving} aria-label="Next pair">
          <IconArrowRight size={18} />
        </button>
      </footer>

      {conflict && (
        <div className={styles.conflictBackdrop} role="dialog" aria-modal="true" aria-labelledby="duplicate-conflict-title">
          <div className={styles.conflictCard}>
            <div className={styles.eyebrow}>Collection ownership conflict</div>
            <h2 id="duplicate-conflict-title">Choose the surviving collection</h2>
            <p>Both files belong to different collections. The surviving media entity can remain in only one.</p>
            <div className={styles.conflictActions}>
              {conflict.conflict.winner_collection_id != null && (
                <button className={styles.primaryButton} onClick={() => void finishResolution(conflict.pair, conflict.action, conflict.conflict.winner_collection_id!)} disabled={resolving}>
                  Collection {conflict.conflict.winner_collection_id}
                </button>
              )}
              {conflict.conflict.loser_collection_id != null && (
                <button className={styles.primaryButton} onClick={() => void finishResolution(conflict.pair, conflict.action, conflict.conflict.loser_collection_id!)} disabled={resolving}>
                  Collection {conflict.conflict.loser_collection_id}
                </button>
              )}
              <button className={styles.secondaryButton} onClick={() => setConflict(null)} disabled={resolving}>Cancel</button>
            </div>
          </div>
        </div>
      )}
    </section>
  );
}
