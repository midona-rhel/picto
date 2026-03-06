import { useCallback, useEffect, useRef, useState } from 'react';
import { Loader, Text, Kbd } from '@mantine/core';
import { EmptyState } from './ui/EmptyState';
import { TextButton } from './ui/TextButton';
import { notifySuccess, notifyError, notifyInfo, notifyWarning } from '../lib/notify';
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
import { mediaFileUrl, mediaThumbnailUrl } from '../lib/mediaUrl';
import { isImagePreloaded, queueImageDecode } from './image-grid/useImagePreloader';
import type { DuplicatePairDto, DuplicatePairsResponse, ResolveDuplicateAction } from '../types/api';
import { useDomainStore } from '../stores/domainStore';
import { registerUndoAction } from '../controllers/undoRedoController';
import { useGlobalKeydown } from '../hooks/useGlobalKeydown';
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
        const batch = await api.grid.getFilesMetadataBatch([
          currentPair.hash_a,
          currentPair.hash_b,
        ]);

        const buildInfo = (hash: string): PairFileInfo => {
          const meta = batch.items[hash];
          const mime = meta?.file.mime ?? 'image/jpeg';
          return {
            hash,
            name: meta?.file.name ?? `${hash.slice(0, 12)}...`,
            size: meta?.file.size ?? 0,
            mime,
            width: meta?.file.width ?? 0,
            height: meta?.file.height ?? 0,
            rating: meta?.file.rating ?? null,
            tags: meta?.tags.map((t) => t.display_tag) ?? [],
            sourceUrls: meta?.file.source_urls ?? [],
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
                await api.file.setStatus(loserHash, 'active');
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
        } else {
          notifyWarning('Smart merge changes metadata and file state; undo is not supported yet.', 'Not Undoable');
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

        void refreshDuplicateCount();
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
    }
  }, [goToPrev, goToNext, handleAction]);
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

  if (loading) {
    return (
      <div className={styles.centeredState}>
        <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 12 }}>
          <Loader size="lg" />
          <Text c="dimmed">Loading duplicate pairs...</Text>
        </div>
      </div>
    );
  }

  if (pairs.length === 0) {
    if (loadingMore) {
      return (
        <div className={styles.centeredState}>
          <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 12 }}>
            <Loader size="lg" />
            <Text c="dimmed">Loading duplicate pairs...</Text>
          </div>
        </div>
      );
    }

    return (
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
    );
  }

  const totalForProgress = initialTotalRef.current || totalPairs || pairs.length;
  const progressPercent = totalForProgress > 0 ? (resolvedCount / totalForProgress) * 100 : 0;

  return (
    <div className={styles.root}>
      <div className={styles.topBar}>
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

      <div className={styles.progressBar}>
        <div className={styles.progressFill} style={{ width: `${progressPercent}%` }} />
      </div>

      <div className={styles.compareArea}>
        <div className={styles.pane}>
          {leftFile && (
            <>
              <div className={styles.paneImage}>
                <img src={leftFile.thumbUrl} alt={leftFile.name} className={styles.paneThumb} />
                {leftDecoded && (
                  <img src={leftFile.imageUrl} alt={leftFile.name} className={styles.paneFull} />
                )}
              </div>
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
            </>
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
          {rightFile && (
            <>
              <div className={styles.paneImage}>
                <img src={rightFile.thumbUrl} alt={rightFile.name} className={styles.paneThumb} />
                {rightDecoded && (
                  <img src={rightFile.imageUrl} alt={rightFile.name} className={styles.paneFull} />
                )}
              </div>
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
            </>
          )}
        </div>
      </div>

      <div className={styles.bottomBar}>
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
