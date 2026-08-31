import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type RefObject,
  type SyntheticEvent,
  type TransitionEvent,
} from 'react';
import { atom, useAtomValue, useSetAtom } from 'jotai';
import {
  IconArrowsJoin,
  IconCheck,
  IconCopy,
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
import type { CanonicalEntityDetails, MediaRecord } from '../../shared/types/canonical';
import { mediaFileUrl, mediaThumbnailUrl } from '../../shared/lib/mediaUrl';
import { showErrorNotification, showInfoNotification, showWarningNotification } from '../../shared/lib/notifications';
import { useShortcutScope } from '../../shared/hooks/useShortcutScope';
import { libraryInvalidation } from '../../runtime/libraryInvalidation';
import btnStyles from '../../shared/styles/actionButton.module.css';
import { KbdTooltip } from '../../shared/ui/KbdTooltip';
import { getShortcut, matchesShortcutDef } from '../../shared/lib/shortcuts';
import { EmptyState, EmptyStateAction } from '../../shared/ui/EmptyState';
import { ProgressBar } from '../../shared/ui/ProgressBar';
import { PropertyRow } from '../../shared/ui/PropertyRow/PropertyRow';
import {
  TitlebarControlButton,
  TitlebarControlGroup,
  TitlebarControls,
  TitlebarCounter,
} from '../../shared/ui/TitlebarControls';
import { useLinkedComparisonZoom } from './useLinkedComparisonZoom';
import { filesController } from '../../controllers/filesController';
import { windowController } from '../../controllers/windowController';
import { ContextMenu, useContextMenu } from '../../shared/ui/ContextMenu/ContextMenu';
import { buildEntityOpenContextEntries } from '../grid/gridContextMenu';
import {
  ToolbarActualSizeIcon,
  ToolbarChevronIcon,
  ToolbarDifferenceIcon,
  ToolbarFitIcon,
} from '../../shared/ui/icons/toolbar-icons';
import styles from './DuplicatesScreen.module.css';
import { t } from '../../i18n';

const LOADING_MESSAGE_DELAY_MS = 200;

interface DuplicateToolbarModel {
  index: number;
  total: number;
  canPrevious: boolean;
  canNext: boolean;
  disabled: boolean;
  isFit: boolean;
  isActual: boolean;
  previous: () => void;
  next: () => void;
  fit: () => void;
  actual: () => void;
}

const duplicateToolbarAtom = atom<DuplicateToolbarModel | null>(null);

export function DuplicatesToolbar() {
  const model = useAtomValue(duplicateToolbarAtom);
  if (!model) return null;

  return (
    <TitlebarControls
      label={t("Duplicate review controls")}
      center={<TitlebarCounter current={model.index + 1} total={model.total} />}
      right={(
        <>
          <KbdTooltip label={t("Zoom to fit")} shortcutId="view.fitWindow">
            <TitlebarControlButton active={model.isFit} onClick={model.fit} aria-label={t("Zoom to fit")} aria-pressed={model.isFit}>
              <ToolbarFitIcon />
            </TitlebarControlButton>
          </KbdTooltip>
          <KbdTooltip label={t("Actual pixels")} shortcutId="view.actualSize">
            <TitlebarControlButton active={model.isActual} onClick={model.actual} aria-label={t("Actual pixels")} aria-pressed={model.isActual}>
              <ToolbarActualSizeIcon />
            </TitlebarControlButton>
          </KbdTooltip>
          <TitlebarControlGroup>
            <KbdTooltip label={t("Previous pair")} shortcutId="dup.prevPair">
              <TitlebarControlButton onClick={model.previous} disabled={!model.canPrevious || model.disabled} aria-label={t("Previous pair")}>
                <ToolbarChevronIcon direction="left" />
              </TitlebarControlButton>
            </KbdTooltip>
            <KbdTooltip label={t("Next pair")} shortcutId="dup.nextPair">
              <TitlebarControlButton onClick={model.next} disabled={!model.canNext || model.disabled} aria-label={t("Next pair")}>
                <ToolbarChevronIcon direction="right" />
              </TitlebarControlButton>
            </KbdTooltip>
          </TitlebarControlGroup>
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
  item: CanonicalEntityDetails | null;
  media: MediaRecord | null;
}

interface PairDetails {
  pairKey: string;
  left: SideState;
  right: SideState;
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
  if (file.pixel_width == null || file.pixel_height == null) return t('Unknown');
  return `${file.pixel_width} x ${file.pixel_height}`;
}

function similarityLabel(pair: DuplicatePair): string {
  const similarity = Math.max(0, 100 - pair.distance / 100);
  if (pair.distance === 0) return '100% similar';
  return t('{value0}% similar', { value0: similarity.toFixed(pair.distance < 10 ? 2 : 1) });
}

function mediaForFile(details: CanonicalEntityDetails, fileHash: string): MediaRecord | null {
  return details.media.find((media) => media.facts.content_hash === fileHash) ?? details.media[0] ?? null;
}

function sideIdentity(side: CandidateSide): number | null {
  return side.occurrences[0]?.root_item_id ?? null;
}

async function loadPairDetails(pair: DuplicatePair): Promise<PairDetails> {
  const loadSide = async (side: CandidateSide): Promise<SideState> => {
    const itemId = sideIdentity(side);
    if (itemId == null) return { item: null, media: null };
    const item = await getDuplicateItemDetails(itemId);
    return { item, media: mediaForFile(item, side.file.file_hash) };
  };
  const [left, right] = await Promise.all([loadSide(pair.left), loadSide(pair.right)]);
  return {
    pairKey: duplicatePairKey(pair),
    left,
    right,
  };
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
  pairKey: string;
  pairThumbnailsReady: boolean;
  onThumbnailReady: (side: ComparisonSide, pairKey: string) => void;
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
  pairKey,
  pairThumbnailsReady,
  onThumbnailReady,
}: MediaCardProps) {
  const label = side === 'left' ? 'Left candidate' : 'Right candidate';
  const contextMenu = useContextMenu();
  const thumbnailUrl = mediaThumbnailUrl(file.file_hash);
  const fullResolutionUrl = mediaFileUrl(file.file_hash, file.mime_type);
  const media = details.media;
  return (
    <article
      className={`${styles.mediaCard} ${smartMergeSurvivor ? styles.mergePreview : ''}`}
      data-smart-merge-survivor={smartMergeSurvivor || undefined}
      onContextMenu={(event) => contextMenu.open(event, buildEntityOpenContextEntries({
        hash: file.file_hash,
        onOpenDefault: (hash) => { void filesController.openDefaultAppForHash(hash); },
        onOpenNewWindow: () => { void windowController.openDetailWindow({
          hash: file.file_hash,
          width: file.pixel_width,
          height: file.pixel_height,
        }); },
      }), { showSearch: false })}
    >
      <header className={styles.cardHeader}>
        <span className={styles.sideLabel}>{label}</span>
        <KbdTooltip label={t("Keep {value0}", { value0: side })} shortcutId={side === 'left' ? 'dup.keepLeft' : 'dup.keepRight'}>
          <button className={btnStyles.btn} onClick={onKeep} disabled={disabled} aria-label={t("Keep {value0}", { value0: side })}>
            <IconCheck size={15} /> <span className={styles.keepLabel}>{t("Keep ")}{side}</span>
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
        {loading && !details.item && <div className={styles.previewState}>{t("Loading metadata...")}</div>}
        {!loading && !details.item && <div className={styles.previewState}>{t("Media details unavailable")}</div>}
        {!differenceActive && (
          <div className={styles.previewLayers} style={zoom.frameStyle(side)} data-testid={`${side}-preview-layers`}>
            <ProgressiveDuplicateImage
              assetKey={file.file_hash}
              thumbnailUrl={thumbnailUrl}
              fullResolutionUrl={fullResolutionUrl}
              alt={media?.media_name ?? file.file_hash}
              pairKey={pairKey}
              pairThumbnailsReady={pairThumbnailsReady}
              onThumbnailReady={() => onThumbnailReady(side, pairKey)}
            />
          </div>
        )}
        {differenceActive && differenceFiles && (
          <DifferenceComposite side={side} files={differenceFiles} zoom={zoom} />
        )}
      </div>
      <div className={styles.metadata}>
        <div className={styles.mediaName} title={media?.media_name ?? file.file_hash}>
          {media?.media_name ?? file.file_hash}
        </div>
        <PropertyRow label={t("Resolution")} value={dimensions(file)} />
        <PropertyRow label={t("Size")} value={formatBytes(file.size_bytes)} />
        <PropertyRow label={t("Format")} value={file.mime_type} />
        <PropertyRow label={t("Rating")} value={details.item?.rating ?? 'Unrated'} />
        <PropertyRow label={t("Tags")} value={String(details.item?.tag_ids.length ?? 0)} />
        <PropertyRow
          label={t("Created")}
          value={details.item?.root.captured_at_ms ? new Date(details.item.root.captured_at_ms).toLocaleDateString() : 'Unknown'}
        />
        <PropertyRow label={t("Added")} value={details.item ? new Date(details.item.root.imported_at_ms).toLocaleDateString() : 'Unknown'} />
        {details.item?.root.kind === 'collection' && (
          <PropertyRow label={t("Group")} value={details.item.root.name || `Group ${details.item.root.root_id}`} />
        )}
        <PropertyRow label={t("Occurrences")} value={String(occurrenceCount)} />
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

interface DuplicateImageLayers {
  assetKey: string;
  thumbnailUrl: string;
  fullResolutionUrl: string;
  previousUrl: string | null;
  thumbnailReady: boolean;
  fullReady: boolean;
  fullSettled: boolean;
}

interface ProgressiveDuplicateImageProps {
  assetKey: string;
  thumbnailUrl: string;
  fullResolutionUrl: string;
  alt: string;
  pairKey: string;
  pairThumbnailsReady: boolean;
  onThumbnailReady: () => void;
}

function ProgressiveDuplicateImage({
  assetKey,
  thumbnailUrl,
  fullResolutionUrl,
  alt,
  pairKey,
  pairThumbnailsReady,
  onThumbnailReady,
}: ProgressiveDuplicateImageProps) {
  const [layers, setLayers] = useState<DuplicateImageLayers>(() => ({
    assetKey,
    thumbnailUrl,
    fullResolutionUrl,
    previousUrl: null,
    thumbnailReady: false,
    fullReady: false,
    fullSettled: false,
  }));

  useLayoutEffect(() => {
    setLayers((current) => {
      if (current.assetKey === assetKey) return current;
      return {
        assetKey,
        thumbnailUrl,
        fullResolutionUrl,
        previousUrl: current.fullSettled ? current.fullResolutionUrl : current.thumbnailUrl,
        thumbnailReady: false,
        fullReady: false,
        fullSettled: false,
      };
    });
  }, [assetKey, fullResolutionUrl, thumbnailUrl]);

  const markThumbnailReady = (event: SyntheticEvent<HTMLImageElement>) => {
    if (event.currentTarget.dataset.assetKey !== assetKey) return;
    setLayers((current) => (
      current.assetKey === assetKey ? { ...current, thumbnailReady: true } : current
    ));
    onThumbnailReady();
  };
  const markFullReady = (event: SyntheticEvent<HTMLImageElement>) => {
    if (event.currentTarget.dataset.assetKey !== assetKey) return;
    setLayers((current) => (
      current.assetKey === assetKey ? { ...current, fullReady: true } : current
    ));
  };
  const settleFullImage = (event: TransitionEvent<HTMLImageElement>) => {
    if (event.currentTarget.dataset.assetKey !== assetKey) return;
    setLayers((current) => (
      current.assetKey === assetKey ? { ...current, fullSettled: true } : current
    ));
  };

  return (
    <>
      {layers.previousUrl && !pairThumbnailsReady && (
        <img
          key={`previous:${layers.previousUrl}`}
          className={styles.previousImage}
          src={layers.previousUrl}
          alt=""
          draggable={false}
        />
      )}
      {!layers.fullSettled && (
        <img
          key={`thumbnail:${layers.assetKey}`}
          data-asset-key={layers.assetKey}
          className={`${styles.thumbnailImage} ${pairThumbnailsReady ? styles.thumbnailImageReady : ''}`}
          src={layers.thumbnailUrl}
          onLoad={markThumbnailReady}
          alt={alt}
          draggable={false}
        />
      )}
      {pairThumbnailsReady && (
        <img
          key={`full:${pairKey}:${layers.assetKey}`}
          data-asset-key={layers.assetKey}
          data-resolution="full"
          className={`${styles.fullImage} ${layers.fullReady ? styles.fullImageVisible : ''}`}
          src={layers.fullResolutionUrl}
          onLoad={markFullReady}
          onTransitionEnd={settleFullImage}
          alt=""
          decoding="async"
          loading="eager"
          draggable={false}
        />
      )}
    </>
  );
}

export function DuplicatesScreen() {
  const setDuplicateToolbar = useSetAtom(duplicateToolbarAtom);
  const [pairs, setPairs] = useState<DuplicatePair[]>([]);
  const [activePairKey, setActivePairKey] = useState('');
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
  const preparedDetailsRef = useRef<PairDetails | null>(null);
  const scanningRef = useRef(false);
  const resolutionInFlightRef = useRef(false);
  const leftPreviewRef = useRef<HTMLDivElement>(null);
  const rightPreviewRef = useRef<HTMLDivElement>(null);

  const index = activePairKey
    ? pairs.findIndex((candidate) => duplicatePairKey(candidate) === activePairKey)
    : -1;
  const currentPair = index >= 0 ? pairs[index] : null;
  const pairKey = currentPair ? activePairKey : '';
  const pairsRef = useRef(pairs);
  pairsRef.current = pairs;
  const currentPairRef = useRef(currentPair);
  currentPairRef.current = currentPair;
  const activePairKeyRef = useRef(activePairKey);
  activePairKeyRef.current = activePairKey;
  const [thumbnailGate, setThumbnailGate] = useState({ pairKey: '', left: false, right: false });
  const [pendingPairKey, setPendingPairKey] = useState<string | null>(null);
  const [pendingThumbnailGate, setPendingThumbnailGate] = useState({ left: false, right: false });
  const [pendingDetails, setPendingDetails] = useState<PairDetails | null>(null);
  const pendingPair = pendingPairKey == null
    ? null
    : pairs.find((candidate) => duplicatePairKey(candidate) === pendingPairKey) ?? null;
  const markThumbnailReady = useCallback((side: ComparisonSide, readyPairKey: string) => {
    if (readyPairKey !== activePairKeyRef.current) return;
    setThumbnailGate((current) => {
      const gate = current.pairKey === readyPairKey
        ? current
        : { pairKey: readyPairKey, left: false, right: false };
      if (gate[side]) return gate;
      return { ...gate, [side]: true };
    });
  }, []);
  const pairThumbnailsReady = thumbnailGate.pairKey === pairKey
    && thumbnailGate.left
    && thumbnailGate.right;
  const navigating = pendingPairKey != null;
  const markPendingThumbnailReady = useCallback((side: ComparisonSide) => {
    setPendingThumbnailGate((current) => (
      current[side] ? current : { ...current, [side]: true }
    ));
  }, []);
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
    pairKey,
  });
  const differenceFiles = currentPair ? { left: currentPair.left.file, right: currentPair.right.file } : null;
  const differenceActive = pairThumbnailsReady
    && !metadataLoading
    && (differenceHovered || differenceFocused);
  const mergeWinner = currentPair ? smartMergeWinner(currentPair.decision) : null;
  const mergePreviewActive = !metadataLoading
    && !resolving
    && mergeWinner != null
    && (smartMergeHovered || smartMergeFocused);
  const showLoadingMessage = useDelayedFlag(loading);
  const showScanProgress = useDelayedFlag(scanning);

  const reportFailure = useCallback((cause: unknown, title = t('Duplicate review failed')) => {
    const message = cause instanceof Error ? cause.message : String(cause);
    setError(message);
    showErrorNotification({ title, message });
  }, []);

  useEffect(() => {
    if (!pendingPair || !pendingPairKey) {
      setPendingDetails(null);
      return;
    }
    let cancelled = false;
    void loadPairDetails(pendingPair)
      .then((details) => {
        if (!cancelled) setPendingDetails(details);
      })
      .catch((cause) => {
        if (cancelled) return;
        setPendingPairKey(null);
        setPendingThumbnailGate({ left: false, right: false });
        setPendingDetails(null);
        reportFailure(cause, t('Unable to load duplicate media'));
      });
    return () => {
      cancelled = true;
    };
  }, [pendingPairKey, reportFailure]);

  useLayoutEffect(() => {
    if (!pendingPairKey || !pendingThumbnailGate.left || !pendingThumbnailGate.right) return;
    const readyPair = pairs.find((candidate) => duplicatePairKey(candidate) === pendingPairKey);
    if (!readyPair) return;
    const readyPairKey = duplicatePairKey(readyPair);
    if (pendingDetails?.pairKey !== readyPairKey) return;

    requestIdRef.current += 1;
    preparedDetailsRef.current = pendingDetails;
    setLeft(pendingDetails.left);
    setRight(pendingDetails.right);
    setMetadataLoading(false);
    setThumbnailGate({ pairKey: readyPairKey, left: true, right: true });
    setActivePairKey(readyPairKey);
    setPendingPairKey(null);
    setPendingThumbnailGate({ left: false, right: false });
    setPendingDetails(null);
  }, [pairs, pendingDetails, pendingPairKey, pendingThumbnailGate.left, pendingThumbnailGate.right]);

  const loadPairs = useCallback(async ({
    showLoading = true,
    resetProgress = true,
  }: LoadPairsOptions = {}) => {
    if (showLoading) setLoading(true);
    setPendingPairKey(null);
    setPendingThumbnailGate({ left: false, right: false });
    setPendingDetails(null);
    setError(null);
    try {
      const page = await getDuplicatePairs();
      const activePairKey = activePairKeyRef.current;
      const nextPairKey = selectStablePairKey(pairsRef.current, activePairKey, page.items);
      setPairs(page.items);
      setActivePairKey(nextPairKey);
      setTotal(page.total);
      if (resetProgress) setInitialTotal(page.total);
    } catch (cause) {
      reportFailure(cause, t('Unable to load duplicate review'));
    } finally {
      if (showLoading) setLoading(false);
    }
  }, [reportFailure]);

  useEffect(() => {
    void loadPairs();
  }, [loadPairs]);

  useEffect(() => libraryInvalidation.register('duplicates', () => {
    // resolveDuplicatePair emits this invalidation itself. finishResolution
    // already fetches and prepares the replacement pair atomically, whereas a
    // second refresh would expose its hashes before its display names arrive.
    if (resolutionInFlightRef.current) return;
    void loadPairs({ showLoading: false, resetProgress: true });
  }), [loadPairs]);

  useEffect(() => {
    const pair = currentPairRef.current;
    if (!pair) {
      requestIdRef.current += 1;
      setLeft({ item: null, media: null });
      setRight({ item: null, media: null });
      return;
    }
    const prepared = preparedDetailsRef.current;
    if (prepared?.pairKey === pairKey) {
      requestIdRef.current += 1;
      preparedDetailsRef.current = null;
      setLeft(prepared.left);
      setRight(prepared.right);
      setMetadataLoading(false);
      return;
    }
    const requestId = ++requestIdRef.current;
    setMetadataLoading(true);
    setLeft({ item: null, media: null });
    setRight({ item: null, media: null });
    loadPairDetails(pair)
      .then((details) => {
        if (requestId !== requestIdRef.current) return;
        setLeft(details.left);
        setRight(details.right);
      })
      .catch((cause) => {
        if (requestId !== requestIdRef.current) return;
        reportFailure(cause, t('Unable to load duplicate media'));
      })
      .finally(() => {
        if (requestId === requestIdRef.current) setMetadataLoading(false);
      });
  // Pair detail identity is the two file IDs. A queue refresh commonly returns
  // a new object for the same pair; reloading on object identity would clear
  // its visible names and expose the hash fallback for a frame.
  }, [pairKey, reportFailure]);

  const finishResolution = useCallback(async (pair: DuplicatePair, action: DuplicateAction) => {
    resolutionInFlightRef.current = true;
    setResolving(true);
    setError(null);
    try {
      const result = await resolveDuplicatePair(action, pair);
      if (result.status === 'quality_ambiguous') {
        showWarningNotification({
          title: t("Smart merge needs a choice"),
          message: 'No clear quality winner. Choose left or right, or keep both.',
        });
        return;
      }
      const page = await getDuplicatePairs();
      const nextPairKey = selectStablePairKey(pairsRef.current, duplicatePairKey(pair), page.items);
      const nextPair = page.items.find((candidate) => duplicatePairKey(candidate) === nextPairKey) ?? null;
      const prepared = nextPair ? await loadPairDetails(nextPair) : null;
      preparedDetailsRef.current = prepared;
      setPendingPairKey(null);
      setPendingThumbnailGate({ left: false, right: false });
      setPendingDetails(null);
      setPairs(page.items);
      setTotal(page.total);
      setActivePairKey(nextPairKey);
      if (prepared) {
        setLeft(prepared.left);
        setRight(prepared.right);
      }
    } catch (cause) {
      reportFailure(cause, t('Unable to resolve duplicate pair'));
    } finally {
      resolutionInFlightRef.current = false;
      setResolving(false);
    }
  }, [reportFailure]);

  const resolveCurrent = useCallback((action: DuplicateAction) => {
    if (!currentPair || resolving || navigating) return;
    void finishResolution(currentPair, action);
  }, [currentPair, finishResolution, navigating, resolving]);

  const scan = useCallback(async () => {
    if (scanningRef.current) return;
    scanningRef.current = true;
    setScanning(true);
    setError(null);
    try {
      const summary = await scanDuplicates();
      showInfoNotification({
        title: t("Duplicate scan complete"),
        message: summary.candidate_count > 0
          ? `Found ${summary.candidate_count} new review pairs`
          : 'Scan complete - no new review pairs',
      });
      await loadPairs({ showLoading: false, resetProgress: true });
    } catch (cause) {
      reportFailure(cause, t('Unable to scan for duplicates'));
    } finally {
      scanningRef.current = false;
      setScanning(false);
    }
  }, [loadPairs, reportFailure]);

  const requestIndex = useCallback((nextIndex: number) => {
    const boundedIndex = Math.max(0, Math.min(Math.max(0, pairs.length - 1), nextIndex));
    const requestedPair = pairs[boundedIndex];
    const requestedPairKey = requestedPair ? duplicatePairKey(requestedPair) : null;
    if (!requestedPairKey || requestedPairKey === activePairKey || pendingPairKey != null) return;
    setPendingThumbnailGate({ left: false, right: false });
    setPendingDetails(null);
    setPendingPairKey(requestedPairKey);
  }, [activePairKey, pairs, pendingPairKey]);
  const goPrevious = useCallback(() => requestIndex(index - 1), [index, requestIndex]);
  const goNext = useCallback(() => requestIndex(index + 1), [index, requestIndex]);

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
      disabled: resolving,
      isFit: zoom.isFit,
      isActual: zoom.isActual,
      previous: goPrevious,
      next: goNext,
      fit: zoom.fit,
      actual: zoom.actual,
    });
  }, [
    currentPair,
    goNext,
    goPrevious,
    index,
    loading,
    pairs.length,
    resolving,
    setDuplicateToolbar,
    total,
    zoom.fit,
    zoom.actual,
    zoom.isActual,
    zoom.isFit,
  ]);

  useShortcutScope((event) => {
      if (matchesShortcutDef(event, getShortcut('dup.prevPair')!)) { goPrevious(); return true; }
      if (matchesShortcutDef(event, getShortcut('dup.nextPair')!)) { goNext(); return true; }
      if (matchesShortcutDef(event, getShortcut('dup.fitToWindow')!)) { zoom.fit(); return true; }
      if (matchesShortcutDef(event, getShortcut('view.actualSize')!)) { zoom.actual(); return true; }
      const shortcuts: Array<[string, DuplicateAction]> = [
        ['dup.keepLeft', 'keep_left'],
        ['dup.keepRight', 'keep_right'],
        ['dup.keepBoth', 'keep_both'],
        ['dup.smartMerge', 'smart_merge'],
        ['dup.notDuplicate', 'not_duplicate'],
      ];
      for (const [shortcutId, action] of shortcuts) {
        if (!matchesShortcutDef(event, getShortcut(shortcutId)!)) continue;
        resolveCurrent(action);
        return true;
      }
  }, { priority: 30 });

  if (loading) {
    return (
      <div className={styles.centerState} aria-busy="true">
        {showLoadingMessage ? t("Loading duplicate review queue...") : null}
      </div>
    );
  }

  if (!currentPair) {
    const title = error
      ? t('Unable to load duplicate review')
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
            <IconRefresh size={14} stroke={1.5} /> {error ? t("Retry") : t("Scan library")}
          </EmptyStateAction>
        )}
        progress={showScanProgress ? <ProgressBar indeterminate height={2} /> : null}
      />
    );
  }

  return (
    <section className={styles.root} aria-label={t("Duplicate review")}>
      {pendingPair && (
        <div className={styles.pairPreload} aria-hidden="true">
          {(['left', 'right'] as const).map((side) => (
            <img
              key={`${pendingPairKey}:${side}`}
              data-testid={`pending-${side}-thumbnail`}
              src={mediaThumbnailUrl(pendingPair[side].file.file_hash)}
              alt=""
              decoding="async"
              loading="eager"
              onLoad={(event) => {
                const image = event.currentTarget;
                void (image.decode?.() ?? Promise.resolve())
                  .catch(() => undefined)
                  .then(() => markPendingThumbnailReady(side));
              }}
            />
          ))}
        </div>
      )}
      {showScanProgress && (
        <div className={styles.scanProgress} role="status" aria-label={t("Scanning duplicate pairs")}><ProgressBar indeterminate height={2} /></div>
      )}

      <div className={styles.comparison}>
        <MediaCard side="left" file={currentPair.left.file} occurrenceCount={currentPair.left.occurrences.length} previewRef={leftPreviewRef} zoom={zoom} differenceActive={differenceActive} differenceFiles={differenceFiles} smartMergeSurvivor={mergePreviewActive && mergeWinner === 'left'} details={left} loading={metadataLoading} disabled={resolving} onKeep={() => resolveCurrent('keep_left')} pairKey={pairKey} pairThumbnailsReady={pairThumbnailsReady} onThumbnailReady={markThumbnailReady} />
        <MediaCard side="right" file={currentPair.right.file} occurrenceCount={currentPair.right.occurrences.length} previewRef={rightPreviewRef} zoom={zoom} differenceActive={differenceActive} differenceFiles={differenceFiles} smartMergeSurvivor={mergePreviewActive && mergeWinner === 'right'} details={right} loading={metadataLoading} disabled={resolving} onKeep={() => resolveCurrent('keep_right')} pairKey={pairKey} pairThumbnailsReady={pairThumbnailsReady} onThumbnailReady={markThumbnailReady} />
      </div>

      <footer className={styles.footer}>
        <div className={styles.footerActions}>
          <KbdTooltip label={t("These are different media")} shortcutId="dup.notDuplicate">
            <button className={btnStyles.btn} onClick={() => resolveCurrent('not_duplicate')} disabled={resolving}><IconX size={15} /> {t("Not duplicates")}</button>
          </KbdTooltip>
          <KbdTooltip label={t("Keep both files")} shortcutId="dup.keepBoth">
            <button className={btnStyles.btn} onClick={() => resolveCurrent('keep_both')} disabled={resolving}><IconCopy size={15} /> {t("Keep both")}</button>
          </KbdTooltip>
          <KbdTooltip label={t("Keep the stronger file and preserve item metadata")} shortcutId="dup.smartMerge">
            <button
              className={`${btnStyles.btn} ${btnStyles.btnPrimary}`}
              onClick={() => {
                setSmartMergeHovered(false);
                setSmartMergeFocused(false);
                resolveCurrent('smart_merge');
              }}
              onMouseEnter={() => setSmartMergeHovered(true)}
              onMouseLeave={() => setSmartMergeHovered(false)}
              onFocus={() => setSmartMergeFocused(true)}
              onBlur={() => setSmartMergeFocused(false)}
              disabled={resolving}
            >
              <IconArrowsJoin size={16} /> {t("Smart merge")}</button>
          </KbdTooltip>
          <KbdTooltip label={t("Show differences while held")}>
            <button
              className={`${btnStyles.btn} ${styles.differenceButton}`}
              onMouseEnter={() => setDifferenceHovered(true)}
              onMouseLeave={() => setDifferenceHovered(false)}
              onFocus={() => setDifferenceFocused(true)}
              onBlur={() => setDifferenceFocused(false)}
              disabled={resolving || !differenceFiles}
              aria-label={t("Show Difference")}
              aria-pressed={differenceActive}
            >
              <ToolbarDifferenceIcon /> {t("Show Difference")}</button>
          </KbdTooltip>
          <KbdTooltip label={t("Perceptual similarity is not pixel equality")}><span className={styles.similarity}>
            {similarityLabel(currentPair)}
          </span></KbdTooltip>
        </div>
      </footer>
    </section>
  );
}

function duplicatePairKey(pair: DuplicatePair): string {
  const left = Math.min(pair.file_id_a, pair.file_id_b);
  const right = Math.max(pair.file_id_a, pair.file_id_b);
  return `${left}:${right}`;
}

function selectStablePairKey(
  previousPairs: DuplicatePair[],
  activePairKey: string,
  nextPairs: DuplicatePair[],
): string {
  if (nextPairs.length === 0) return '';
  const nextKeys = new Set(nextPairs.map(duplicatePairKey));
  if (activePairKey && nextKeys.has(activePairKey)) return activePairKey;

  const previousIndex = previousPairs.findIndex(
    (candidate) => duplicatePairKey(candidate) === activePairKey,
  );
  if (previousIndex >= 0) {
    for (let index = previousIndex + 1; index < previousPairs.length; index += 1) {
      const key = duplicatePairKey(previousPairs[index]);
      if (nextKeys.has(key)) return key;
    }
    for (let index = previousIndex - 1; index >= 0; index -= 1) {
      const key = duplicatePairKey(previousPairs[index]);
      if (nextKeys.has(key)) return key;
    }
  }

  return duplicatePairKey(nextPairs[0]);
}
