import { useCallback, useEffect, useRef, useState, type RefObject } from 'react';
import { atom, useAtomValue, useSetAtom } from 'jotai';
import {
  IconArrowLeft,
  IconArrowRight,
  IconArrowsJoin,
  IconAspectRatio,
  IconCheck,
  IconCopy,
  IconLayersDifference,
  IconRefresh,
  IconX,
} from '@tabler/icons-react';
import {
  getDuplicateItemDetails,
  getDuplicatePairs,
  resolveDuplicatePair,
  scanDuplicates,
  type DuplicateAction,
  type DuplicatePair,
} from '../../platform/duplicateApi';
import type { CandidateSide } from '../../shared/types/generated/application/CandidateSide';
import type { FileQuality } from '../../shared/types/generated/application/FileQuality';
import type { ItemDetails } from '../../shared/types/generated/application/ItemDetails';
import type { MediaDetails } from '../../shared/types/generated/application/MediaDetails';
import { mediaFileUrl, mediaThumbnailUrl } from '../../shared/lib/mediaUrl';
import { showErrorNotification, showInfoNotification, showWarningNotification } from '../../shared/lib/notifications';
import { useMediaImagePipeline } from '../../shared/hooks/useMediaImagePipeline';
import { useShortcutScope } from '../../shared/hooks/useShortcutScope';
import { libraryInvalidation } from '../../runtime/libraryInvalidation';
import btnStyles from '../../shared/styles/actionButton.module.css';
import { KbdTooltip } from '../../shared/ui/KbdTooltip';
import { EmptyState, EmptyStateAction } from '../../shared/ui/EmptyState';
import { ProgressBar } from '../../shared/ui/ProgressBar';
import { PropertyRow } from '../../shared/ui/PropertyRow/PropertyRow';
import {
  TitlebarControlButton,
  TitlebarControls,
  TitlebarCounter,
  TitlebarZoomSlider,
} from '../../shared/ui/TitlebarControls';
import { useLinkedComparisonZoom } from './useLinkedComparisonZoom';
import { filesController } from '../../controllers/filesController';
import { windowController } from '../../controllers/windowController';
import { ContextMenu, useContextMenu } from '../../shared/ui/ContextMenu/ContextMenu';
import { buildEntityOpenContextEntries } from '../grid/gridContextMenu';
import styles from './DuplicatesScreen.module.css';

const LOADING_MESSAGE_DELAY_MS = 200;

interface DuplicateToolbarModel {
  index: number;
  total: number;
  canPrevious: boolean;
  canNext: boolean;
  disabled: boolean;
  zoomPercent: number;
  isFit: boolean;
  differenceAvailable: boolean;
  differenceActive: boolean;
  previous: () => void;
  next: () => void;
  zoomOut: () => void;
  zoomIn: () => void;
  setZoomPercent: (value: number) => void;
  fit: () => void;
  setDifferenceHovered: (active: boolean) => void;
  setDifferenceFocused: (active: boolean) => void;
}

const duplicateToolbarAtom = atom<DuplicateToolbarModel | null>(null);

export function DuplicatesToolbar() {
  const model = useAtomValue(duplicateToolbarAtom);
  if (!model) return null;

  return (
    <TitlebarControls
      label="Duplicate review controls"
      center={(
        <TitlebarZoomSlider
          min={10}
          max={800}
          value={model.zoomPercent}
          onChange={model.setZoomPercent}
          onZoomOut={model.zoomOut}
          onZoomIn={model.zoomIn}
        />
      )}
      right={(
        <>
          <KbdTooltip label="Previous pair" shortcut="ArrowLeft">
            <TitlebarControlButton onClick={model.previous} disabled={!model.canPrevious || model.disabled} aria-label="Previous pair">
              <IconArrowLeft size={17} />
            </TitlebarControlButton>
          </KbdTooltip>
          <TitlebarCounter current={model.index + 1} total={model.total} />
          <KbdTooltip label="Next pair" shortcut="ArrowRight">
            <TitlebarControlButton onClick={model.next} disabled={!model.canNext || model.disabled} aria-label="Next pair">
              <IconArrowRight size={17} />
            </TitlebarControlButton>
          </KbdTooltip>
          <KbdTooltip label="Fit both images">
            <TitlebarControlButton active={model.isFit} onClick={model.fit} aria-label="Fit both images" aria-pressed={model.isFit}>
              <IconAspectRatio size={16} />
            </TitlebarControlButton>
          </KbdTooltip>
          <KbdTooltip label="Hold to highlight differences">
            <TitlebarControlButton
              active={model.differenceActive}
              onMouseEnter={() => model.setDifferenceHovered(true)}
              onMouseLeave={() => model.setDifferenceHovered(false)}
              onFocus={() => model.setDifferenceFocused(true)}
              onBlur={() => model.setDifferenceFocused(false)}
              disabled={model.disabled || !model.differenceAvailable}
              aria-label="Highlight differences"
              aria-pressed={model.differenceActive}
            >
              <IconLayersDifference size={16} />
            </TitlebarControlButton>
          </KbdTooltip>
        </>
      )}
    />
  );
}

interface LoadPairsOptions {
  showLoading?: boolean;
  resetProgress?: boolean;
}

interface SideState {
  item: ItemDetails | null;
  media: MediaDetails | null;
}

type ComparisonSide = 'left' | 'right';

function smartMergeWinner(decision: DuplicatePair['decision']): ComparisonSide | null {
  if (decision === 'LeftBetter' || decision === 'AutoTieLeft') return 'left';
  if (decision === 'RightBetter' || decision === 'AutoTieRight') return 'right';
  return null;
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

function dimensions(file: FileQuality): string {
  if (file.pixel_width == null || file.pixel_height == null) return 'Unknown';
  return `${file.pixel_width} x ${file.pixel_height}`;
}

function mediaForFile(details: ItemDetails, fileHash: string): MediaDetails | null {
  return details.media.find((media) => media.file_hash === fileHash) ?? details.media[0] ?? null;
}

function sideIdentity(side: CandidateSide): number | null {
  return side.occurrences[0]?.root_item_id ?? null;
}

interface MediaCardProps {
  side: ComparisonSide;
  file: FileQuality;
  occurrenceCount: number;
  details: SideState;
  previewRef: RefObject<HTMLDivElement>;
  zoom: ReturnType<typeof useLinkedComparisonZoom>;
  differenceActive: boolean;
  differenceFiles: { left: FileQuality; right: FileQuality } | null;
  smartMergeSurvivor: boolean;
  loading: boolean;
  onKeep: () => void;
  disabled: boolean;
}

function DifferenceComposite({
  side,
  files,
  zoom,
}: {
  side: 'left' | 'right';
  files: { left: FileQuality; right: FileQuality };
  zoom: ReturnType<typeof useLinkedComparisonZoom>;
}) {
  const imageUrl = (file: FileQuality) => mediaFileUrl(file.file_hash, file.mime_type);
  const fallback = (file: FileQuality) => mediaThumbnailUrl(file.file_hash);
  return (
    <div
      className={`${styles.previewLayers} ${styles.differenceComposite}`}
      style={zoom.frameStyle(side)}
      data-testid={`${side}-difference-composite`}
    >
      <img
        src={imageUrl(files.left)}
        onError={(event) => { event.currentTarget.onerror = null; event.currentTarget.src = fallback(files.left); }}
        alt=""
        draggable={false}
      />
      <img
        className={styles.differenceLayer}
        src={imageUrl(files.right)}
        onError={(event) => { event.currentTarget.onerror = null; event.currentTarget.src = fallback(files.right); }}
        alt=""
        draggable={false}
      />
    </div>
  );
}

function MediaCard({
  side,
  file,
  occurrenceCount,
  details,
  previewRef,
  zoom,
  differenceActive,
  differenceFiles,
  smartMergeSurvivor,
  loading,
  onKeep,
  disabled,
}: MediaCardProps) {
  const label = side === 'left' ? 'Left candidate' : 'Right candidate';
  const contextMenu = useContextMenu();
  const fullImgRef = useRef<HTMLImageElement>(null);
  const pipeline = useMediaImagePipeline({
    hash: file.file_hash,
    thumbnailHash: file.file_hash,
    mime: file.mime_type,
    isVideo: false,
  });
  const thumbnailUrl = mediaThumbnailUrl(file.file_hash);
  const media = details.media;
  return (
    <article
      className={`${styles.mediaCard} ${smartMergeSurvivor ? styles.mergePreview : ''}`}
      data-smart-merge-survivor={smartMergeSurvivor || undefined}
      onContextMenu={(event) => contextMenu.open(event, buildEntityOpenContextEntries({
        hash: file.file_hash,
        onOpenDefault: (hash) => { void filesController.openDefaultAppForHash(hash); },
        onOpenNewWindow: (hash) => { void windowController.openDetailWindow({
          hash,
          width: file.pixel_width,
          height: file.pixel_height,
        }); },
      }), { showSearch: false })}
    >
      <header className={styles.cardHeader}>
        <span className={styles.sideLabel}>{label}</span>
        <KbdTooltip label={`Keep ${side}`} shortcut={side === 'left' ? 'L' : 'R'}>
          <button className={btnStyles.btn} onClick={onKeep} disabled={disabled} aria-label={`Keep ${side}`}>
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
        {loading && !details.item && <div className={styles.previewState}>Loading metadata...</div>}
        {!loading && !details.item && <div className={styles.previewState}>Media details unavailable</div>}
        {!differenceActive && (
          <div className={styles.previewLayers} style={zoom.frameStyle(side)} data-testid={`${side}-preview-layers`}>
            <img
              className={styles.thumbnailImage}
              src={pipeline.thumbUrl || thumbnailUrl}
              onLoad={pipeline.handleThumbLoad}
              alt={media?.name ?? file.file_hash}
              draggable={false}
            />
            {pipeline.fullUrl && (
              <img
                ref={fullImgRef}
                key={pipeline.fullUrl}
                className={`${styles.fullImage} ${pipeline.fullVisible ? styles.fullImageVisible : ''}`}
                src={pipeline.fullUrl}
                onLoad={pipeline.handleFullLoad}
                alt=""
                decoding="async"
                draggable={false}
              />
            )}
          </div>
        )}
        {differenceActive && differenceFiles && (
          <DifferenceComposite side={side} files={differenceFiles} zoom={zoom} />
        )}
      </div>
      <div className={styles.metadata}>
        <div className={styles.mediaName} title={media?.name ?? file.file_hash}>
          {media?.name ?? file.file_hash}
        </div>
        <PropertyRow label="Resolution" value={dimensions(file)} />
        <PropertyRow label="Size" value={formatBytes(file.size_bytes)} />
        <PropertyRow label="Format" value={file.mime_type} />
        <PropertyRow label="Rating" value={media?.rating == null ? 'Unrated' : String(media.rating)} />
        <PropertyRow label="Tags" value={String(media?.tags.length ?? 0)} />
        <PropertyRow
          label="Created"
          value={media?.captured_at ? new Date(media.captured_at).toLocaleDateString() : 'Unknown'}
        />
        <PropertyRow label="Added" value={media ? new Date(media.imported_at).toLocaleDateString() : 'Unknown'} />
        {details.item?.kind === 'collection' && (
          <PropertyRow label="Group" value={details.item.label ?? `Group ${details.item.item_id}`} />
        )}
        <PropertyRow label="Occurrences" value={String(occurrenceCount)} />
      </div>
      {contextMenu.state && (
        <ContextMenu
          entries={contextMenu.state.entries}
          position={contextMenu.state.position}
          onClose={contextMenu.close}
          showSearch={contextMenu.state.showSearch}
        />
      )}
    </article>
  );
}

export function DuplicatesScreen() {
  const setDuplicateToolbar = useSetAtom(duplicateToolbarAtom);
  const [pairs, setPairs] = useState<DuplicatePair[]>([]);
  const [index, setIndex] = useState(0);
  const [total, setTotal] = useState(0);
  const [initialTotal, setInitialTotal] = useState(0);
  const [loading, setLoading] = useState(true);
  const [scanning, setScanning] = useState(false);
  const [resolving, setResolving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [left, setLeft] = useState<SideState>({ item: null, media: null });
  const [right, setRight] = useState<SideState>({ item: null, media: null });
  const [metadataLoading, setMetadataLoading] = useState(false);
  const [differenceHovered, setDifferenceHovered] = useState(false);
  const [differenceFocused, setDifferenceFocused] = useState(false);
  const [smartMergeHovered, setSmartMergeHovered] = useState(false);
  const [smartMergeFocused, setSmartMergeFocused] = useState(false);
  const requestIdRef = useRef(0);
  const scanningRef = useRef(false);
  const leftPreviewRef = useRef<HTMLDivElement>(null);
  const rightPreviewRef = useRef<HTMLDivElement>(null);

  const currentPair = pairs[index] ?? null;
  const resolvedCount = Math.max(0, initialTotal - total);
  const zoom = useLinkedComparisonZoom({
    leftContainerRef: leftPreviewRef,
    rightContainerRef: rightPreviewRef,
    leftImageSize: currentPair?.left.file.pixel_width && currentPair.left.file.pixel_height
      ? { width: currentPair.left.file.pixel_width, height: currentPair.left.file.pixel_height }
      : null,
    rightImageSize: currentPair?.right.file.pixel_width && currentPair.right.file.pixel_height
      ? { width: currentPair.right.file.pixel_width, height: currentPair.right.file.pixel_height }
      : null,
    pairKey: currentPair ? `${currentPair.file_id_a}:${currentPair.file_id_b}` : '',
  });
  const differenceFiles = currentPair ? { left: currentPair.left.file, right: currentPair.right.file } : null;
  const differenceActive = !metadataLoading && (differenceHovered || differenceFocused);
  const mergeWinner = currentPair ? smartMergeWinner(currentPair.decision) : null;
  const mergePreviewActive = !metadataLoading
    && !resolving
    && mergeWinner != null
    && (smartMergeHovered || smartMergeFocused);
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
      const page = await getDuplicatePairs();
      setPairs(page.items);
      setTotal(page.total);
      if (resetProgress) setInitialTotal(page.total);
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

  useEffect(() => libraryInvalidation.register('duplicates', () => {
    void loadPairs({ showLoading: false, resetProgress: true });
  }), [loadPairs]);

  useEffect(() => {
    if (!currentPair) {
      requestIdRef.current += 1;
      setLeft({ item: null, media: null });
      setRight({ item: null, media: null });
      return;
    }
    const requestId = ++requestIdRef.current;
    setMetadataLoading(true);
    setLeft({ item: null, media: null });
    setRight({ item: null, media: null });
    const loadSide = async (side: CandidateSide): Promise<SideState> => {
      const itemId = sideIdentity(side);
      if (itemId == null) return { item: null, media: null };
      const item = await getDuplicateItemDetails(itemId);
      return { item, media: mediaForFile(item, side.file.file_hash) };
    };
    Promise.all([loadSide(currentPair.left), loadSide(currentPair.right)])
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

  const finishResolution = useCallback(async (pair: DuplicatePair, action: DuplicateAction) => {
    setResolving(true);
    setError(null);
    try {
      const result = await resolveDuplicatePair(action, pair);
      if (result.status === 'quality_ambiguous') {
        showWarningNotification({
          title: 'Smart merge needs a choice',
          message: 'No clear quality winner. Choose left or right, or keep both.',
        });
        return;
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
        message: summary.candidate_count > 0
          ? `Found ${summary.candidate_count} new review pairs`
          : 'Scan complete - no new review pairs',
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

  useEffect(() => () => setDuplicateToolbar(null), [setDuplicateToolbar]);

  useEffect(() => {
    if (loading || !currentPair) {
      setDuplicateToolbar(null);
      return;
    }
    setDuplicateToolbar({
      index,
      total,
      canPrevious: index > 0,
      canNext: index < pairs.length - 1,
      disabled: resolving || metadataLoading,
      zoomPercent: zoom.zoomPercent,
      isFit: zoom.isFit,
      differenceAvailable: differenceFiles != null,
      differenceActive,
      previous: goPrevious,
      next: goNext,
      zoomOut: zoom.zoomOut,
      zoomIn: zoom.zoomIn,
      setZoomPercent: zoom.setZoomPercent,
      fit: zoom.fit,
      setDifferenceHovered,
      setDifferenceFocused,
    });
  }, [
    currentPair,
    differenceActive,
    differenceFiles,
    goNext,
    goPrevious,
    index,
    loading,
    metadataLoading,
    pairs.length,
    resolving,
    setDuplicateToolbar,
    total,
    zoom.fit,
    zoom.isFit,
    zoom.setZoomPercent,
    zoom.zoomIn,
    zoom.zoomOut,
    zoom.zoomPercent,
  ]);

  useShortcutScope((event) => {
      if (event.key === 'ArrowLeft') { goPrevious(); return true; }
      if (event.key === 'ArrowRight') { goNext(); return true; }
      if (!currentPair || resolving || metadataLoading) return;
      const shortcuts: Partial<Record<string, DuplicateAction>> = {
        l: 'keep_left',
        r: 'keep_right',
        s: 'smart_merge',
        n: 'not_duplicate',
      };
      const action = shortcuts[event.key.toLowerCase()];
      if (action) {
        void finishResolution(currentPair, action);
        return true;
      }
  }, { priority: 30 });

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
      {showScanProgress && (
        <div className={styles.scanProgress} role="status" aria-label="Scanning duplicate pairs"><ProgressBar indeterminate height={2} /></div>
      )}

      <div className={styles.comparison}>
        <MediaCard side="left" file={currentPair.left.file} occurrenceCount={currentPair.left.occurrences.length} previewRef={leftPreviewRef} zoom={zoom} differenceActive={differenceActive} differenceFiles={differenceFiles} smartMergeSurvivor={mergePreviewActive && mergeWinner === 'left'} details={left} loading={metadataLoading} disabled={resolving || metadataLoading} onKeep={() => void finishResolution(currentPair, 'keep_left')} />
        <MediaCard side="right" file={currentPair.right.file} occurrenceCount={currentPair.right.occurrences.length} previewRef={rightPreviewRef} zoom={zoom} differenceActive={differenceActive} differenceFiles={differenceFiles} smartMergeSurvivor={mergePreviewActive && mergeWinner === 'right'} details={right} loading={metadataLoading} disabled={resolving || metadataLoading} onKeep={() => void finishResolution(currentPair, 'keep_right')} />
      </div>

      <footer className={styles.footer}>
        <div className={styles.footerActions}>
          <KbdTooltip label="These are different media" shortcut="N">
            <button className={btnStyles.btn} onClick={() => void finishResolution(currentPair, 'not_duplicate')} disabled={resolving || metadataLoading}><IconX size={15} /> Not duplicates</button>
          </KbdTooltip>
          <button className={btnStyles.btn} onClick={() => void finishResolution(currentPair, 'keep_both')} disabled={resolving || metadataLoading}><IconCopy size={15} /> Keep both</button>
          <KbdTooltip label="Keep the stronger file and preserve item metadata" shortcut="S">
            <button
              className={`${btnStyles.btn} ${btnStyles.btnPrimary}`}
              onClick={() => {
                setSmartMergeHovered(false);
                setSmartMergeFocused(false);
                void finishResolution(currentPair, 'smart_merge');
              }}
              onMouseEnter={() => setSmartMergeHovered(true)}
              onMouseLeave={() => setSmartMergeHovered(false)}
              onFocus={() => setSmartMergeFocused(true)}
              onBlur={() => setSmartMergeFocused(false)}
              disabled={resolving || metadataLoading}
            >
              <IconArrowsJoin size={16} /> Smart merge
            </button>
          </KbdTooltip>
          <span className={styles.similarity}>{currentPair.similarity_pct.toFixed(0)}% match</span>
        </div>
      </footer>
    </section>
  );
}
