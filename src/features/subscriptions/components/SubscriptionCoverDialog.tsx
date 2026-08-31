import { useCallback, useEffect, useRef, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import type { SubscriptionCoverCandidate } from '../../../shared/types/generated/application/SubscriptionCoverCandidate';
import type { SubscriptionCoverCandidateCursor } from '../../../shared/types/generated/application/SubscriptionCoverCandidateCursor';
import { mediaThumbnailUrl } from '../../../shared/lib/mediaUrl';
import { showErrorNotification } from '../../../shared/lib/notifications';
import { GlassModal, modalStyles } from '../../../shared/ui/GlassModal/GlassModal';
import { ThumbnailImage } from '../../../shared/ui/ThumbnailImage/ThumbnailImage';
import { ActionButton } from './ActionButton';
import {
  SubscriptionCoverImage,
  subscriptionCoverGeometry,
  type SubscriptionCoverDimensions,
} from './SubscriptionCoverImage';
import styles from './SubscriptionCoverDialog.module.css';
import { t } from '../../../i18n';

type Target = { id: string; name: string };
type Crop = { focusX: number; focusY: number; zoomPercent: number };

export type MediaCoverCandidate = SubscriptionCoverCandidate & {
  mime_type?: string | null;
};

export interface MediaCoverCandidatePage<TCursor> {
  candidates: MediaCoverCandidate[];
  next_cursor: TCursor | null;
}

export interface MediaCoverDialogProps<TCursor> {
  target: Target | null;
  busy: boolean;
  initialCandidate?: MediaCoverCandidate | null;
  instructions?: string;
  emptyText?: string;
  onLoad: (targetId: string, cursor?: TCursor | null) => Promise<MediaCoverCandidatePage<TCursor>>;
  onSave: (targetId: string, candidate: MediaCoverCandidate, crop: Crop) => Promise<boolean>;
  onClose: () => void;
}

const DEFAULT_CROP: Crop = { focusX: 500, focusY: 500, zoomPercent: 100 };

export function MediaCoverDialog<TCursor>({
  target,
  busy,
  initialCandidate = null,
  instructions = 'Select one of the images downloaded that has been added to the library.',
  emptyText = 'No images have been added yet.',
  onLoad,
  onSave,
  onClose,
}: MediaCoverDialogProps<TCursor>) {
  const [candidates, setCandidates] = useState<MediaCoverCandidate[]>([]);
  const [selected, setSelected] = useState<MediaCoverCandidate | null>(null);
  const [crop, setCrop] = useState<Crop>(DEFAULT_CROP);
  const [loading, setLoading] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [nextCursor, setNextCursor] = useState<TCursor | null>(null);
  const [gridWidth, setGridWidth] = useState(560);
  const [cropDimensions, setCropDimensions] = useState<SubscriptionCoverDimensions>({ width: 1, height: 1 });
  const dragRef = useRef<{ x: number; y: number; focusX: number; focusY: number } | null>(null);
  const candidateGridRef = useRef<HTMLDivElement>(null);
  const targetId = target?.id ?? null;
  const activeTargetId = useRef(targetId);
  activeTargetId.current = targetId;

  useEffect(() => {
    if (!targetId) return;
    let cancelled = false;
    setSelected(initialCandidate);
    setCrop(DEFAULT_CROP);
    setCropDimensions({ width: 1, height: 1 });
    setCandidates([]);
    setNextCursor(null);
    setLoading(true);
    void onLoad(targetId, null)
      .then((page) => {
        if (!cancelled) {
          setCandidates(page.candidates);
          setNextCursor(page.next_cursor);
        }
      })
      .catch((error) => {
        if (cancelled) return;
        setCandidates([]);
        showErrorNotification({
          title: t("Unable to load subscription media"),
          message: error instanceof Error ? error.message : String(error),
        });
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => { cancelled = true; };
    // Candidate eligibility changes through explicit library invalidation. A
    // parent refresh must not reset an in-progress crop just because it
    // recreated the callback.
  }, [targetId]); // eslint-disable-line react-hooks/exhaustive-deps

  const loadMore = useCallback(() => {
    if (!targetId || !nextCursor || loadingMore) return;
    setLoadingMore(true);
    void onLoad(targetId, nextCursor)
      .then((page) => {
        if (activeTargetId.current !== targetId) return;
        setCandidates((current) => [...current, ...page.candidates]);
        setNextCursor(page.next_cursor);
      })
      .catch((error) => showErrorNotification({
        title: t("Unable to load more subscription media"),
        message: error instanceof Error ? error.message : String(error),
      }))
      .finally(() => setLoadingMore(false));
  }, [loadingMore, nextCursor, onLoad, targetId]);

  useEffect(() => {
    const element = candidateGridRef.current;
    if (!element || typeof ResizeObserver === 'undefined') return undefined;
    const observer = new ResizeObserver(([entry]) => {
      setGridWidth(entry?.contentRect.width || element.clientWidth || 560);
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, [selected, loading]);

  const columnCount = Math.max(1, Math.floor((gridWidth + 8) / 112));
  const rowHeight = (gridWidth - (columnCount - 1) * 8) / columnCount + 8;
  const candidateRowCount = Math.ceil(candidates.length / columnCount);
  const candidateVirtualizer = useVirtualizer({
    count: candidateRowCount,
    getScrollElement: () => candidateGridRef.current,
    estimateSize: () => rowHeight,
    overscan: 3,
    initialRect: { width: gridWidth, height: 560 },
    observeElementRect: (instance, callback) => {
      const element = instance.scrollElement;
      if (!element) return undefined;
      const publish = () => {
        const rect = element.getBoundingClientRect();
        callback({ width: rect.width || gridWidth, height: rect.height || 560 });
      };
      publish();
      const Observer = instance.targetWindow?.ResizeObserver;
      if (!Observer) return undefined;
      const observer = new Observer(publish);
      observer.observe(element);
      return () => observer.disconnect();
    },
  });
  const virtualRows = candidateVirtualizer.getVirtualItems();
  const lastVirtualRow = virtualRows[virtualRows.length - 1]?.index ?? -1;

  useEffect(() => {
    candidateVirtualizer.measure();
  }, [candidateVirtualizer, rowHeight]);

  useEffect(() => {
    if (!nextCursor || loadingMore || lastVirtualRow < candidateRowCount - 3) return;
    loadMore();
  }, [candidateRowCount, lastVirtualRow, loadMore, loadingMore, nextCursor]);

  useEffect(() => {
    if (loading || candidates.length > 0 || !nextCursor || loadingMore) return;
    loadMore();
  }, [candidates.length, loadMore, loading, loadingMore, nextCursor]);

  const commit = async () => {
    if (!target || !selected) return;
    if (await onSave(target.id, selected, crop)) onClose();
  };

  const setZoom = (zoomPercent: number) => {
    setCrop((current) => ({
      ...current,
      zoomPercent: Math.max(100, Math.min(300, Math.round(zoomPercent / 5) * 5)),
    }));
  };

  return (
    <GlassModal
      open={target != null}
      onClose={onClose}
      title={t("Set cover photo")}
      size={selected ? 'md' : 'lg'}
      footer={selected ? (
        <div className={styles.footerActions}>
          <ActionButton variant="ghost" onClick={() => setSelected(null)} disabled={busy}>{t("Back")}</ActionButton>
          <ActionButton variant="primary" onClick={() => void commit()} disabled={busy}>{t("Confirm")}</ActionButton>
        </div>
      ) : undefined}
    >
      {selected ? (
        <div className={styles.cropStep}>
          <div
            className={styles.cropViewport}
            aria-label={t("Cover crop preview")}
            onWheel={(event) => {
              event.preventDefault();
              setCrop((current) => ({
                ...current,
                zoomPercent: Math.max(
                  100,
                  Math.min(300, current.zoomPercent - Math.sign(event.deltaY) * 5),
                ),
              }));
            }}
            onPointerDown={(event) => {
              event.currentTarget.setPointerCapture(event.pointerId);
              dragRef.current = {
                x: event.clientX,
                y: event.clientY,
                focusX: crop.focusX,
                focusY: crop.focusY,
              };
            }}
            onPointerMove={(event) => {
              const drag = dragRef.current;
              if (!drag || !selected) return;
              const rect = event.currentTarget.getBoundingClientRect();
              const currentGeometry = subscriptionCoverGeometry(cropDimensions, crop);
              const horizontalTravel = Math.max(0, currentGeometry.widthRatio - 1) * rect.width;
              const verticalTravel = Math.max(0, currentGeometry.heightRatio - 1) * rect.height;
              setCrop((current) => ({
                ...current,
                focusX: horizontalTravel === 0
                  ? current.focusX
                  : Math.max(0, Math.min(1000, Math.round(
                    drag.focusX - ((event.clientX - drag.x) / horizontalTravel) * 1000,
                  ))),
                focusY: verticalTravel === 0
                  ? current.focusY
                  : Math.max(0, Math.min(1000, Math.round(
                    drag.focusY - ((event.clientY - drag.y) / verticalTravel) * 1000,
                  ))),
              }));
            }}
            onPointerUp={() => { dragRef.current = null; }}
            onPointerCancel={() => { dragRef.current = null; }}
          >
            <SubscriptionCoverImage
              fileHash={selected.file_hash}
              preferThumbnail={selected.mime_type != null && !selected.mime_type.startsWith('image/')}
              progressive={selected.mime_type == null || selected.mime_type.startsWith('image/')}
              crop={crop}
              fallbackDimensions={{
                width: selected.pixel_width ?? 1,
                height: selected.pixel_height ?? 1,
              }}
              className={styles.cropImage}
              onDimensionsChange={setCropDimensions}
            />
          </div>
          <label className={styles.zoomRow}>
            <span>{t("Zoom")}</span>
            <input
              className={modalStyles.rangeInput}
              type="range"
              min={100}
              max={300}
              step={5}
              value={crop.zoomPercent}
              aria-label={t("Cover zoom")}
              onChange={(event) => setZoom(Number(event.currentTarget.value))}
            />
          </label>
        </div>
      ) : (
        <div className={styles.selectStep}>
          <p className={styles.instructions}>{instructions}</p>
          {loading ? (
            <p className={styles.empty}>{t("Loading…")}</p>
          ) : candidates.length === 0 ? (
            <p className={styles.empty}>{emptyText}</p>
          ) : (
            <div className={styles.candidateGrid} ref={candidateGridRef}>
              <div
                className={styles.candidateGridInner}
                style={{ height: candidateVirtualizer.getTotalSize() }}
              >
                {virtualRows.map((virtualRow) => (
                  <div
                    className={styles.candidateRow}
                    key={virtualRow.key}
                    style={{
                      gridTemplateColumns: `repeat(${columnCount}, minmax(0, 1fr))`,
                      height: rowHeight - 8,
                      transform: `translateY(${virtualRow.start}px)`,
                    }}
                  >
                    {Array.from({ length: columnCount }, (_, columnIndex) => {
                      const candidate = candidates[virtualRow.index * columnCount + columnIndex];
                      if (!candidate) return null;
                      return (
                        <button
                          type="button"
                          className={styles.candidate}
                          key={candidate.media_item_id}
                          onClick={() => {
                            setCrop(DEFAULT_CROP);
                            setCropDimensions({
                              width: candidate.pixel_width ?? 1,
                              height: candidate.pixel_height ?? 1,
                            });
                            setSelected(candidate);
                          }}
                          title={candidate.name ?? 'Subscription media'}
                        >
                          <ThumbnailImage
                            src={mediaThumbnailUrl(candidate.file_hash)}
                            alt={candidate.name ?? ''}
                            loading="lazy"
                            draggable={false}
                          />
                        </button>
                      );
                    })}
                  </div>
                ))}
              </div>
              {loadingMore ? <span className={styles.loadingMore}>{t("Loading more…")}</span> : null}
            </div>
          )}
        </div>
      )}
    </GlassModal>
  );
}

export function SubscriptionCoverDialog(
  props: MediaCoverDialogProps<SubscriptionCoverCandidateCursor>,
) {
  return <MediaCoverDialog {...props} />;
}
