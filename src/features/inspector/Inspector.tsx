/**
 * Inspector panel — shows info about what's currently in focus.
 *
 * - Nothing selected → current scope (folder / system view)
 * - One item selected → that item's full details
 * - Multiple items selected → shared tags/folders from backend
 */

import { forwardRef, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { flushSync } from 'react-dom';
import { useAtomValue, getDefaultStore } from 'jotai';
import { IconAlertCircle, IconFolder } from '@tabler/icons-react';
import { ContextMenu, useContextMenu } from '../../shared/ui/ContextMenu';
import { KbdTooltip } from '../../shared/ui/KbdTooltip';
import { ColorPalette } from '../../shared/ui/ColorPalette';
import * as entityMutations from '../../controllers/entityMutations';
import { foldersController } from '../../controllers/foldersController';
import { smartFoldersController } from '../../controllers/smartFoldersController';
import { PropertyRow } from '../../shared/ui/PropertyRow/PropertyRow';
import { InspectorSection } from '../../shared/ui/InspectorSection/InspectorSection';
import { StarRating } from '../../shared/ui/StarRating/StarRating';
import { InspectorField, InspectorFieldGroup, InspectorSourceField } from '../../shared/ui/InspectorField/InspectorField';
import { TagChip } from '../../shared/ui/TagChip/TagChip';
import type { ItemTarget } from '../../shared/types/generated/application/ItemTarget';
import type { ItemDetails } from '../../shared/types/generated/application/ItemDetails';
import type { SelectionSummary } from '../../shared/types/generated/application/SelectionSummary';
import {
  displayedInspectorItemDetailsAtom,
  displayedInspectorTargetAtom,
  inspectorErrorAtom,
  inspectorLoadingAtom,
  inspectorPinnedAtom,
  scopeInspectorViewModelAtom,
} from '../../state/inspector';
import {
  selectionCountAtom,
  selectionFingerprintAtom,
  selectionTargetAtom,
} from '../../state/selection';
import { sidebarNodesAtom } from '../../state/sidebar';
import { tagSelectPortalAtom, folderPickerPortalAtom, aiTaggerPortalAtom } from '../../state/portals';
import { getShortcut, formatKeysDisplay } from '../../shared/lib/shortcuts';
import { confirmModalAtom, exportModalAtom } from '../../state/modals';
import { navigateToNode, navigateWithGridFilters } from '../../state/navigationHistory';
import { activeNodeIdAtom } from '../../state/navigation';
import { InspectorAddIcon, InspectorExportIcon } from '../../shared/ui/icons/toolbar-icons';
import { createEmptyItemFilters } from '../../shared/lib/itemFilters';
import { showTagItems } from '../../controllers/gridNavigationController';
import { libraryInvalidation } from '../../runtime/libraryInvalidation';
import { filesController } from '../../controllers/filesController';
import { windowController } from '../../controllers/windowController';
import { buildEntityOpenContextEntries, buildLibraryCoverContextEntry } from '../grid/gridContextMenu';
import { gridFiltersAtom, gridFilterToolbarOpenAtom } from '../../state/grid';
import styles from './Inspector.module.css';
import { ThumbnailImage } from '../../shared/ui/ThumbnailImage/ThumbnailImage';
import { buildCommonTagContextEntries } from '../tags/tagContextMenu';
import { setTagStarred, useTagPreferences } from '../tags/tagPreferences';
import { tagsController } from '../../controllers/tagsController';
import type { CanonicalNamespaceSummary } from '../../shared/types/canonical';
import { IconAutoTag } from '../../shared/ui/icons/sidebar-menu-icons';
import { openCurrentLibraryCoverPicker } from '../library/libraryAppearance';
import { showErrorNotification } from '../../shared/lib/notifications';
import { formatLabelForMime } from '../grid/canvas/primitives';

const store = getDefaultStore();

function reportRatingFailure(reason: unknown): void {
  showErrorNotification({
    title: 'Could not change rating',
    message: reason instanceof Error ? reason.message : String(reason),
  });
}

// ── Formatters ──────────────────────────────────────────────────

function fmtSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1048576) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1073741824) return `${(bytes / 1048576).toFixed(1)} MB`;
  return `${(bytes / 1073741824).toFixed(2)} GB`;
}

function fmtDate(iso: string | null | undefined): string | null {
  if (!iso) return null;
  try { return new Date(iso).toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' }); }
  catch { return iso; }
}

function fmtDuration(ms: number): string {
  const s = Math.floor(ms / 1000);
  const m = Math.floor(s / 60);
  const sec = s % 60;
  if (m >= 60) return `${Math.floor(m / 60)}:${String(m % 60).padStart(2, '0')}:${String(sec).padStart(2, '0')}`;
  return `${m}:${String(sec).padStart(2, '0')}`;
}

const EXT: Record<string, string> = {
  jpeg: 'JPG', png: 'PNG', gif: 'GIF', webp: 'WEBP', 'svg+xml': 'SVG',
  bmp: 'BMP', tiff: 'TIFF', avif: 'AVIF', heic: 'HEIC', heif: 'HEIF',
  mp4: 'MP4', webm: 'WEBM', quicktime: 'MOV', 'x-matroska': 'MKV',
  pdf: 'PDF', 'epub+zip': 'EPUB',
};
function fmtExt(mime: string) { const s = mime.split('/')[1] ?? ''; return EXT[s] ?? s.replace(/^x-/, '').toUpperCase(); }

const NS_ORDER: Record<string, number> = { creator: 0, studio: 1, series: 2, character: 3, person: 4, species: 5, system: 6, '': 7, default: 7 };
function tagKey(ns: string, sub: string) { return `${(NS_ORDER[ns.toLowerCase()] ?? 7).toString().padStart(2, '0')}:${sub.toLowerCase()}`; }

function hexToRgb(hex: string | null | undefined): [number, number, number] {
  if (!hex) return [134, 142, 150];
  const h = hex.replace('#', '');
  return [parseInt(h.substring(0, 2), 16), parseInt(h.substring(2, 4), 16), parseInt(h.substring(4, 6), 16)];
}

function parseTag(t: string) {
  const i = t.indexOf(':');
  return i > 0 ? { ns: t.slice(0, i), sub: t.slice(i + 1), raw: t } : { ns: '', sub: t, raw: t };
}

function selectionSupportsAiTagging(
  target: ItemTarget | null | undefined,
  summary: SelectionSummary | null,
): boolean {
  if (!target || !summary) return false;
  const mimeCounts = summary.stats.mime_counts;
  if (!mimeCounts) return false;
  const imageCount = Object.entries(mimeCounts)
    .filter(([mime]) => mime.startsWith('image/'))
    .reduce((count, [, value]) => count + value, 0);
  const mediaCount = Object.values(mimeCounts).reduce((count, value) => count + value, 0);
  return mediaCount > 0 && imageCount === mediaCount;
}

// ── Portal opener ───────────────────────────────────────────────

function openPortal(e: React.MouseEvent, atom: typeof tagSelectPortalAtom) {
  const btn = e.currentTarget.getBoundingClientRect();
  const panel = e.currentTarget.closest('[class*="inspector"]') as HTMLElement | null;
  const x = panel ? panel.getBoundingClientRect().left : btn.left;
  store.set(atom, { open: true, anchor: { x, y: btn.top } });
}

function sameStrings(left: string[] | null | undefined, right: string[]): boolean {
  return Boolean(left && left.length === right.length && left.every((value, index) => value === right[index]));
}

function confirmSelectionOverwrite(
  field: 'notes' | 'sources',
  selectedCount: number,
  onConfirm: () => void,
) {
  const label = field === 'notes' ? 'notes' : 'source URLs';
  store.set(confirmModalAtom, {
    open: true,
    title: `Overwrite ${field}?`,
    message: `This will replace existing ${label} on all ${selectedCount.toLocaleString()} selected items. Are you sure?`,
    confirmLabel: `Overwrite ${field === 'notes' ? 'Notes' : 'Sources'}`,
    danger: true,
    onConfirm,
  });
}

// ── Preview components ──────────────────────────────────────────

function Preview({
  hashes,
  backgrounds = [],
  type,
  formatLabel,
  fontHashes = new Set<string>(),
}: {
  hashes: string[];
  backgrounds?: readonly (string | null)[];
  type: 'single' | 'collage' | 'stacked';
  formatLabel?: string;
  fontHashes?: ReadonlySet<string>;
}) {
  const contextMenu = useContextMenu();
  if (type === 'single' && hashes[0]) {
    const hash = hashes[0];
    return (
      <div
        className={styles.preview}
        onContextMenu={(event) => contextMenu.open(event, buildEntityOpenContextEntries({
          hash,
          onOpenDefault: (value) => { void filesController.openDefaultAppForHash(value); },
          onRevealInFolder: (value) => { void filesController.revealHashInFolder(value); },
          onOpenNewWindow: () => { void windowController.openDetailWindow({ hash }); },
        }).concat(buildLibraryCoverContextEntry(hash, () => {
          void openCurrentLibraryCoverPicker({
            media_item_id: -1,
            file_hash: hash,
            name: null,
            pixel_width: null,
            pixel_height: null,
            mime_type: null,
          }).catch((reason) => showErrorNotification({
            title: 'Could not set library cover',
            message: reason instanceof Error ? reason.message : String(reason),
          }));
        })))}
      >
        <div className={styles.previewFrame} style={{ background: backgrounds[0] ?? undefined }}>
          <ThumbnailImage
            src={`media://localhost/thumb/${hash}.jpg`}
            alt=""
            className={styles.previewImage}
            draggable={false}
            fallback={fontHashes.has(hash) ? 'font' : 'broken'}
          />
          <div className={styles.previewGlass} />
          {formatLabel && <span className={styles.previewTypeLabel} data-inspector-format-label>{formatLabel}</span>}
        </div>
        {contextMenu.state && <ContextMenu entries={contextMenu.state.entries} position={contextMenu.state.position} onClose={contextMenu.close} />}
      </div>
    );
  }

  if (type === 'collage') {
    return (
      <div className={styles.preview}>
      <div className={styles.thumbnail}>
        <div className={styles.pic3} />
        <div className={styles.pic2} />
        <div className={styles.pic1}>
          {hashes.length > 0 ? (
            <div className={styles.collage}>
              {[0, 1, 2, 3].map((i) => (
                <div key={i} className={styles.collageCell} style={{ background: backgrounds[i] ?? undefined }}>
                  {hashes[i] && <ThumbnailImage
                    src={`media://localhost/thumb/${hashes[i]}.jpg`}
                    alt=""
                    draggable={false}
                    fallback={fontHashes.has(hashes[i]) ? 'font' : 'broken'}
                  />}
                </div>
              ))}
            </div>
          ) : (
            <div className={styles.folderPlaceholder}><IconFolder size={32} stroke={1} /></div>
          )}
        </div>
      </div>
      </div>
    );
  }

  return <StackedPreview hashes={hashes} backgrounds={backgrounds} fontHashes={fontHashes} />;
}

function StackedPreview({
  hashes,
  backgrounds,
  fontHashes,
}: {
  hashes: string[];
  backgrounds: readonly (string | null)[];
  fontHashes: ReadonlySet<string>;
}) {
  const previews = hashes.slice(-5);
  const previewOffset = hashes.length - previews.length;
  type StackPose = 'base' | 'left' | 'right';
  type StackEntry = {
    hash: string;
    background: string | null;
    font: boolean;
    pose: StackPose;
    z: number;
    entering: boolean;
    exiting: boolean;
  };
  const poses = useRef(new Map<string, StackPose>());
  const nextPose = useRef(0);
  const nextZ = useRef(0);
  const makeEntry = (hash: string, index: number, entering: boolean): StackEntry => {
    let pose = poses.current.get(hash);
    if (!pose) {
      const sequence = nextPose.current++;
      pose = sequence === 0 ? 'base' : sequence % 2 === 0 ? 'left' : 'right';
      poses.current.set(hash, pose);
    }
    return {
      hash,
      background: backgrounds[previewOffset + index] ?? null,
      font: fontHashes.has(hash),
      pose,
      z: nextZ.current++,
      entering,
      exiting: false,
    };
  };
  const [displayed, setDisplayed] = useState<StackEntry[]>(() => (
    previews.map((hash, index) => makeEntry(hash, index, false))
  ));
  const previewKey = previews.join('\u0000');
  const metadataKey = previews.map((hash, index) => (
    `${backgrounds[previewOffset + index] ?? ''}:${fontHashes.has(hash) ? 1 : 0}`
  )).join('\u0000');

  useLayoutEffect(() => {
    setDisplayed((current) => {
      const byHash = new Map(current.map((entry) => [entry.hash, entry]));
      const next = previews.map((hash, index) => {
        const existing = byHash.get(hash);
        if (!existing) return makeEntry(hash, index, true);
        return {
          ...existing,
          background: backgrounds[previewOffset + index] ?? null,
          font: fontHashes.has(hash),
          entering: false,
          exiting: false,
        };
      });
      const availableExitSlots = Math.max(0, 5 - next.length);
      const exiting = current
        .filter((entry) => !previews.includes(entry.hash) && !entry.exiting)
        .slice(-availableExitSlots)
        .map((entry) => ({ ...entry, entering: false, exiting: true }));
      return [...next, ...exiting];
    });
  }, [previewKey, metadataKey]);

  useEffect(() => {
    if (!displayed.some((entry) => entry.exiting)) return;
    const timeout = window.setTimeout(() => {
      setDisplayed((current) => current.filter((entry) => !entry.exiting));
    }, 220);
    return () => window.clearTimeout(timeout);
  }, [displayed]);

  if (displayed.length === 0) return null;
  const topHash = previews[previews.length - 1];
  return (
    <div className={styles.preview}>
      <div className={styles.stackContainer}>
        {displayed.map((entry) => {
          const top = entry.hash === topHash;
          const motionClass = entry.pose === 'base'
            ? styles.stackItemBase
            : entry.pose === 'left' ? styles.stackItemLeft : styles.stackItemRight;
          const enterClass = entry.entering && entry.pose !== 'base'
            ? entry.pose === 'left' ? styles.stackItemEnterLeft : styles.stackItemEnterRight
            : '';
          return (
            <div
              key={entry.hash}
              className={`${styles.stackItem} ${motionClass} ${enterClass} ${entry.exiting ? styles.stackItemExit : ''}`}
              data-inspector-preview-hash={entry.hash}
              data-inspector-stack-position={entry.exiting ? 'exiting' : top ? 'top' : 'behind'}
              style={{
              zIndex: entry.z, filter: top ? undefined : 'brightness(0.7)',
              }}
            >
              <div className={styles.previewFrame} style={{ background: entry.background ?? undefined }}>
                <ThumbnailImage
                  src={`media://localhost/thumb/${entry.hash}.jpg`}
                  alt=""
                  className={styles.previewImage}
                  draggable={false}
                  fallback={entry.font ? 'font' : 'broken'}
                />
                <div className={styles.previewGlass} />
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

// ── Multi-select summary hook ───────────────────────────────────

function useSelectionSummary() {
  const target = useAtomValue(selectionTargetAtom);
  const selectionCount = useAtomValue(selectionCountAtom);
  const selectionFingerprint = useAtomValue(selectionFingerprintAtom);
  const pinned = useAtomValue(inspectorPinnedAtom);
  const [result, setResult] = useState<{ fingerprint: string; summary: SelectionSummary } | null>(null);
  const [loadingFingerprint, setLoadingFingerprint] = useState<string | null>(null);
  const [failedFingerprint, setFailedFingerprint] = useState<string | null>(null);
  const [refreshRevision, setRefreshRevision] = useState(0);
  // Keep the last committed summary while a changed selection is resolving. The
  // displayed inspector still describes that committed selection during this time.
  const summary = result?.summary ?? null;
  const needsSummary = Boolean(!pinned && target && selectionCount >= 2);

  useEffect(() => libraryInvalidation.register('library', (payload) => {
    setRefreshRevision(payload.revision);
  }), []);

  useEffect(() => {
    if (pinned || !target || selectionCount < 2) {
      setResult(null);
      setLoadingFingerprint(null);
      setFailedFingerprint(null);
      return;
    }
    let stale = false;
    const refreshingDisplayedSummary = result?.fingerprint === selectionFingerprint;
    if (!refreshingDisplayedSummary) setLoadingFingerprint(null);
    setFailedFingerprint(null);
    const timer = window.setTimeout(() => {
      if (stale || refreshingDisplayedSummary || store.get(inspectorPinnedAtom)) return;
      flushSync(() => {
        setResult(null);
        setLoadingFingerprint(selectionFingerprint);
      });
      store.set(displayedInspectorTargetAtom, {
        kind: 'multi',
        count: selectionCount,
        selectionMode: target.kind === 'query' ? 'query_results' : 'explicit',
      });
    }, 250);
    void entityMutations.getTargetSelectionSummary(target).then((s) => {
      if (!stale && !store.get(inspectorPinnedAtom)) {
        window.clearTimeout(timer);
        flushSync(() => {
          setResult({ fingerprint: selectionFingerprint, summary: s });
          setLoadingFingerprint(null);
        });
        store.set(displayedInspectorTargetAtom, {
          kind: 'multi',
          count: s.selected_count,
          selectionMode: target.kind === 'query' ? 'query_results' : 'explicit',
        });
      }
    }).catch(() => {
      if (!stale && !store.get(inspectorPinnedAtom)) {
        window.clearTimeout(timer);
        flushSync(() => {
          setResult(null);
          setLoadingFingerprint(null);
          setFailedFingerprint(selectionFingerprint);
        });
        store.set(displayedInspectorTargetAtom, {
          kind: 'multi',
          count: selectionCount,
          selectionMode: target.kind === 'query' ? 'query_results' : 'explicit',
        });
      }
    });
    return () => {
      stale = true;
      window.clearTimeout(timer);
    };
  }, [pinned, refreshRevision, selectionCount, selectionFingerprint, target]);

  return {
    target,
    summary,
    pending: needsSummary && summary == null,
    showLoading: loadingFingerprint === selectionFingerprint,
    failed: failedFingerprint === selectionFingerprint,
  };
}

function itemDetailsDisplay(details: ItemDetails) {
  const primary = details.media[0] ?? null;
  const totalSize = details.media.reduce((total, media) => total + media.size_bytes, 0);
  const mimeTypes = [...new Set(details.media.map((media) => media.mime_type))];
  const ratings = details.media.map((media) => media.rating);
  const sharedRating = ratings.length > 0 && ratings.every((rating) => rating === ratings[0])
    ? ratings[0]
    : null;
  return { primary, totalSize, mimeTypes, sharedRating };
}

type CorePropertyLabel =
  | 'Items'
  | 'Dimensions'
  | 'Size'
  | 'Type'
  | 'Duration'
  | 'Date added'
  | 'Date created'
  | 'Date modified';

type CoreProperty = {
  label: CorePropertyLabel;
  value: string;
  mono: boolean;
  title?: string;
  loading?: boolean;
  showLoading?: boolean;
};

const CORE_PROPERTIES: Array<Pick<CoreProperty, 'label' | 'mono'>> = [
  { label: 'Items', mono: true },
  { label: 'Dimensions', mono: true },
  { label: 'Size', mono: true },
  { label: 'Type', mono: false },
  { label: 'Duration', mono: true },
  { label: 'Date added', mono: true },
  { label: 'Date created', mono: true },
  { label: 'Date modified', mono: true },
];

type CorePropertyValues = Partial<Record<CorePropertyLabel, Pick<CoreProperty, 'value' | 'title' | 'loading' | 'showLoading'>>>;

function normalizedCoreProperties(values: CorePropertyValues): CoreProperty[] {
  return CORE_PROPERTIES.flatMap(({ label, mono }) => {
    const property = values[label];
    if (!property || (!property.loading && (property.value === '' || property.value === '—'))) return [];
    return [{ label, mono, ...property }];
  });
}

type TextFieldModel = {
  value: string;
  readOnly?: boolean;
  onCommit?: (value: string) => void;
};

type SourceFieldModel = {
  urls: string[];
  onChange?: (urls: string[]) => void;
  unavailable?: boolean;
};

type InspectorSkeletonProps = {
  preview: React.ReactNode;
  palette: string[];
  name?: TextFieldModel;
  notes?: TextFieldModel;
  source?: SourceFieldModel;
  selectionCount?: number;
  showSource?: boolean;
  rating?: { value: number; onChange?: (rating: number) => void };
  coreProperties: CoreProperty[];
  tags: Array<{ ns: string; sub: string; raw: string }>;
  showTags?: boolean;
  onRemoveTag?: (raw: string) => void;
  folders: Array<{ id: number; name: string; color: string | null }>;
  showFolders?: boolean;
  onRemoveFolder?: (folderId: number) => void;
  onNavigateFolder?: (folderId: number) => void;
  extras?: Array<{ label: string; value: string }>;
  propertyAction?: React.ReactNode;
  action?: React.ReactNode;
  status?: { kind: 'loading' | 'error'; message: string };
  summaryPending?: boolean;
  showSummaryLoading?: boolean;
};

/** The inspector's invariant top stack; content state changes data, not row order. */
export function InspectorSkeleton({
  preview,
  palette,
  name,
  notes,
  source,
  selectionCount,
  showSource = true,
  rating,
  coreProperties,
  tags,
  showTags = true,
  onRemoveTag,
  folders,
  showFolders = true,
  onRemoveFolder,
  onNavigateFolder,
  extras = [],
  propertyAction,
  action,
  status,
  summaryPending = false,
  showSummaryLoading = false,
}: InspectorSkeletonProps) {
  return (
    <Shell>
      {preview}
      <ColorPalette
        colors={palette}
        onFilter={(hex) => {
          store.set(gridFilterToolbarOpenAtom, true);
          navigateWithGridFilters(store.get(activeNodeIdAtom), {
            ...store.get(gridFiltersAtom),
            color_hex: hex,
          });
        }}
      />

      {selectionCount != null && (
        <div className={styles.selectionCount} data-inspector-selection-count="">
          {selectionCount.toLocaleString()} items selected
        </div>
      )}
      {(name || notes || (showSource && source)) && (
        <InspectorFieldGroup>
        <div className={styles.fieldStack} data-inspector-identity="">
          {name && <div data-inspector-anchor="name">
            <InspectorField value={name.value} placeholder="Name" readOnly={name.readOnly} onCommit={name.onCommit} />
          </div>}
          {notes && <div data-inspector-anchor="notes">
            <InspectorField value={notes.value} placeholder="Notes" readOnly={notes.readOnly} onCommit={notes.onCommit} />
          </div>}
          {showSource && source && (
            <div data-inspector-anchor="source">
              <InspectorSourceField urls={source.urls} onChange={source.onChange} unavailable={source.unavailable} />
            </div>
          )}
        </div>
        </InspectorFieldGroup>
      )}

      {showTags && (
        <div data-inspector-section="tags">
          <TagsSection tags={tags} onRemove={onRemoveTag} editable={Boolean(onRemoveTag)} pending={summaryPending} showLoading={showSummaryLoading} />
        </div>
      )}
      {showFolders && (
        <div data-inspector-section="folders">
          <FoldersSection folders={folders} onRemove={onRemoveFolder} onNavigate={onNavigateFolder} editable={Boolean(onRemoveFolder)} pending={summaryPending} showLoading={showSummaryLoading} />
        </div>
      )}
      <div data-inspector-section="properties">
        <InspectorSection title="Properties">
          <div className={styles.propsStack} data-inspector-core-properties="">
            {rating && (summaryPending
              ? <div className={styles.pendingRating}>{showSummaryLoading && <SummarySpinner label="Loading shared rating" />}</div>
              : <StarRating value={rating.value} onChange={rating.onChange} onError={reportRatingFailure} />)}
            {coreProperties.filter((property) => property.loading || (property.value !== '' && property.value !== '—')).map((property) => (
              <div key={property.label} data-inspector-core-property={property.label}>
                <PropertyRow {...property} />
              </div>
            ))}
          </div>
          {propertyAction}
        </InspectorSection>
      </div>

      {extras.length > 0 && (
        <div data-inspector-section="details">
          <InspectorSection title="Details">
            <div className={styles.propsStack}>
              {extras.map((property) => <PropertyRow key={property.label} {...property} />)}
            </div>
          </InspectorSection>
        </div>
      )}

      {action && <div className={styles.flowAction} data-inspector-section="actions">{action}</div>}

      {status && (
        <div className={styles.status} role="status">
          {status.kind === 'error' && <IconAlertCircle size={16} stroke={1.5} className={styles.errorIcon} />}
          <span className={status.kind === 'error' ? styles.errorText : styles.emptyInline}>{status.message}</span>
        </div>
      )}
    </Shell>
  );
}

function UnavailableInspectorSkeleton({ status, showSource = true }: { status?: InspectorSkeletonProps['status']; showSource?: boolean }) {
  return (
    <InspectorSkeleton
      preview={<div className={styles.preview} aria-hidden="true" />}
      palette={[]}
      name={{ value: '—', readOnly: true }}
      notes={{ value: '—', readOnly: true }}
      source={{ urls: [], unavailable: true }}
      showSource={showSource}
      coreProperties={normalizedCoreProperties({})}
      tags={[]}
      showTags={false}
      folders={[]}
      showFolders={false}
      status={status}
    />
  );
}

function navigateToFolder(folderId: number) {
  navigateToNode(`folder:${folderId}`);
}

function InspectorExportAction({ target, count }: { target: ItemTarget; count: number }) {
  return (
    <div className={styles.flowAction} data-inspector-property-action="export">
      <InspectorActionButton
        variant="flow"
        action="export"
        onClick={() => store.set(exportModalAtom, { open: true, fileCount: count, target })}
      >
        <InspectorExportIcon />
        <span>Export</span>
      </InspectorActionButton>
    </div>
  );
}

// ── Main component ──────────────────────────────────────────────

export function Inspector() {
  const inspectorTarget = useAtomValue(displayedInspectorTargetAtom);
  const loading = useAtomValue(inspectorLoadingAtom);
  const error = useAtomValue(inspectorErrorAtom);
  const entityData = useAtomValue(displayedInspectorItemDetailsAtom);
  const scopeVM = useAtomValue(scopeInspectorViewModelAtom);
  const sidebarNodes = useAtomValue(sidebarNodesAtom);
  const {
    target: selTarget,
    summary,
    pending: summaryPending,
    showLoading: showSummaryLoading,
    failed: summaryFailed,
  } = useSelectionSummary();

  if (inspectorTarget.kind === 'none') return null;

  // ── Single entity ─────────────────────────────────────────────
  if (inspectorTarget.kind === 'item') {
    if (!entityData) return <UnavailableInspectorSkeleton status={error ? { kind: 'error', message: error } : loading ? { kind: 'loading', message: 'Loading...' } : undefined} />;

    const d = entityData;
    const { primary, totalSize, mimeTypes, sharedRating } = itemDetailsDisplay(d);
    const tags = [...(d.aggregate_tags ?? [])]
      .map(parseTag)
      .sort((a, b) => tagKey(a.ns, a.sub).localeCompare(tagKey(b.ns, b.sub)));
    const folders = (d.folder_ids ?? []).map((folderId) => {
      const n = sidebarNodes.find((s) => s.id === `folder:${folderId}`);
      return { id: folderId, name: n?.name ?? `Folder ${folderId}`, color: n?.color ?? null };
    });
    const palette = primary?.dominant_colors ?? [];
    return <InspectorSkeleton
      preview={<Preview
        hashes={primary ? [primary.file_hash] : []}
        backgrounds={primary ? [primary.dominant_color_hex] : []}
        type="single"
        formatLabel={primary ? formatLabelForMime(primary.mime_type) : undefined}
        fontHashes={primary?.mime_type.startsWith('font/') ? new Set([primary.file_hash]) : undefined}
      />}
      palette={palette}
      name={{ value: d.label ?? primary?.name ?? '', onCommit: (value) => { void entityMutations.setItemName(d.item_id, value); } }}
      notes={{ value: primary?.notes ?? '', onCommit: (value) => { void entityMutations.setItemNotes(d.item_id, value); } }}
      source={{ urls: primary?.source_urls ?? [], onChange: (urls) => { void entityMutations.setItemSourceUrls(d.item_id, urls); } }}
      rating={{ value: sharedRating ?? 0, onChange: (rating) => entityMutations.setItemRating(d.item_id, rating) }}
      coreProperties={normalizedCoreProperties({
        Items: { value: d.media.length.toLocaleString() },
        Dimensions: { value: primary?.pixel_width && primary.pixel_height ? `${primary.pixel_width} × ${primary.pixel_height}` : '—' },
        Size: { value: fmtSize(totalSize) },
        Type: { value: mimeTypes.length === 1 ? fmtExt(mimeTypes[0]) : 'Mixed', title: mimeTypes.join(', ') },
        Duration: { value: primary?.duration_ms != null && primary.duration_ms > 0 ? fmtDuration(primary.duration_ms) : '—' },
        'Date added': { value: fmtDate(primary?.imported_at) ?? '—' },
        'Date created': { value: fmtDate(primary?.captured_at) ?? '—' },
        'Date modified': { value: '—' },
      })}
      tags={tags}
      onRemoveTag={(raw) => { void entityMutations.removeItemTags(d.item_id, [raw]); }}
      folders={folders}
      onRemoveFolder={(folderId) => { void entityMutations.removeItemFromFolder(d.item_id, folderId); }}
      onNavigateFolder={navigateToFolder}
      propertyAction={d.media.length > 0 ? <InspectorExportAction target={{ kind: 'explicit', item_ids: [d.item_id] }} count={d.media.length} /> : undefined}
      action={<InspectorAutoTagAction count={d.media.length} enabled={d.media.length > 0 && d.media.every((media) => media.mime_type.startsWith('image/'))} />}
    />;
  }

  // ── Multi-select ──────────────────────────────────────────────
  if (inspectorTarget.kind === 'multi') {
    // Use backend count when available — it's in sync with the tags/folders data
    const count = summary?.selected_count ?? inspectorTarget.count;
    const tags = (summary?.shared_tags ?? []).map((t) => parseTag(t.tag));
    const folders = (summary?.shared_folders ?? []).map((f) => {
      const n = sidebarNodes.find((s) => s.id === `folder:${f.folder_id}`);
      return { id: f.folder_id, name: n?.name ?? f.name, color: n?.color ?? null };
    });
    const previewHashes = summary?.sample_hashes ?? [];
    const previewMimeTypes = Object.keys(summary?.stats?.mime_counts ?? {});
    const fontPreviewHashes = previewMimeTypes.length > 0 && previewMimeTypes.every((mime) => mime.startsWith('font/'))
      ? new Set(previewHashes)
      : undefined;
    const commitNotes = selTarget ? (notes: string) => {
      const apply = () => { void entityMutations.setTargetNotes(selTarget, notes); };
      if ((summary?.notes_present_count ?? 0) > 0 && notes !== (summary?.shared_notes ?? '')) {
        confirmSelectionOverwrite('notes', count, apply);
      } else {
        apply();
      }
    } : undefined;
    const commitSources = selTarget ? (urls: string[]) => {
      if (sameStrings(summary?.shared_source_urls, urls)) return;
      const apply = () => { void entityMutations.setTargetSourceUrls(selTarget, urls); };
      if ((summary?.source_urls_present_count ?? 0) > 0) {
        confirmSelectionOverwrite('sources', count, apply);
      } else {
        apply();
      }
    } : undefined;

    return <InspectorSkeleton
      preview={<Preview hashes={previewHashes} type="stacked" fontHashes={fontPreviewHashes} />}
      palette={[]}
      selectionCount={count}
      notes={{ value: summary?.shared_notes ?? '', onCommit: commitNotes, readOnly: summaryPending }}
      source={{ urls: summary?.shared_source_urls ?? [], onChange: commitSources, unavailable: summaryPending }}
      rating={{ value: summary?.stats?.rating_stats?.shared ?? 0, onChange: selTarget ? (rating) => entityMutations.setTargetRating(selTarget, rating) : undefined }}
      coreProperties={normalizedCoreProperties({
        Size: summaryPending
          ? { value: '', loading: true, showLoading: showSummaryLoading }
          : { value: summary?.stats?.total_size_bytes != null ? fmtSize(summary.stats.total_size_bytes) : '—' },
      })}
      tags={tags}
      onRemoveTag={selTarget ? (raw) => { void entityMutations.removeTargetTags(selTarget, [raw]); } : undefined}
      folders={folders}
      onRemoveFolder={selTarget ? (folderId) => { void entityMutations.updateTargetFolderMembership(selTarget, folderId, 'remove'); } : undefined}
      onNavigateFolder={navigateToFolder}
      propertyAction={selTarget && count > 0 ? <InspectorExportAction target={selTarget} count={count} /> : undefined}
      action={<InspectorAutoTagAction count={count} enabled={selectionSupportsAiTagging(selTarget, summary)} />}
      summaryPending={summaryPending}
      showSummaryLoading={showSummaryLoading}
      status={summaryFailed ? { kind: 'error', message: 'Could not load selection details.' } : undefined}
    />;
  }

  // ── Scope (nothing selected — show current view) ──────────────
  if (!scopeVM) return <UnavailableInspectorSkeleton showSource={false} status={error ? { kind: 'error', message: error } : loading ? { kind: 'loading', message: 'Loading...' } : undefined} />;
  const node = scopeVM.node;
  const isSystem = node.kind === 'system';
  const canEdit = !isSystem;

  const saveName = async (v: string) => {
    if (scopeVM.folder?.folderId != null) { await foldersController.rename(scopeVM.folder.folderId, v); return; }
    if (scopeVM.smartFolder?.smartFolderId != null) {
      await smartFoldersController.update(scopeVM.smartFolder.smartFolderId, buildSmartFolderPayload(scopeVM, { name: v }));
    }
  };
  const saveNotes = async (v: string) => {
    if (scopeVM.folder?.folderId != null) { await foldersController.applyNotes(scopeVM.folder.folderId, v || null); return; }
    if (scopeVM.smartFolder?.smartFolderId != null) {
      await smartFoldersController.update(scopeVM.smartFolder.smartFolderId, buildSmartFolderPayload(scopeVM, { notes: v || null }));
    }
  };

  const scopeSize = scopeVM.totalSizeBytes != null ? fmtSize(scopeVM.totalSizeBytes) : null;
  const exportScope = node.kind === 'folder' && scopeVM.folder?.folderId != null
    ? { kind: 'folder' as const, folder_id: scopeVM.folder.folderId }
    : node.kind === 'smart_folder' && scopeVM.smartFolder?.smartFolderId != null
      ? { kind: 'smart_folder' as const, smart_folder_id: scopeVM.smartFolder.smartFolderId }
      : null;

  const extras = [
    scopeVM.searchText ? { label: 'Search', value: scopeVM.searchText } : null,
    node.kind === 'folder' ? { label: 'Auto tags', value: scopeVM.folder!.autoTags.length > 0 ? 'Yes' : 'No' } : null,
    node.kind === 'folder' ? { label: 'Watch', value: scopeVM.folder!.watchEnabled ? 'Yes' : 'No' } : null,
  ].filter((property): property is { label: string; value: string } => property !== null);

  return <InspectorSkeleton
    preview={<Preview
      hashes={scopeVM.previewItems.map((item) => item.display_file_hash)}
      backgrounds={scopeVM.previewItems.map((item) => item.dominant_color_hex)}
      type="collage"
      fontHashes={new Set(scopeVM.previewItems
        .filter((item) => item.display_mime_type?.startsWith('font/'))
        .map((item) => item.display_file_hash))}
    />}
    palette={[]}
    name={{ value: node.name, readOnly: isSystem, onCommit: canEdit ? (value) => { void saveName(value); } : undefined }}
    notes={isSystem ? undefined : { value: scopeVM.folder?.notes ?? scopeVM.smartFolder?.notes ?? '', onCommit: canEdit ? (value) => { void saveNotes(value); } : undefined }}
    source={{ urls: [], unavailable: true }}
    showSource={false}
    coreProperties={normalizedCoreProperties({
      Items: { value: scopeVM.totalCount.toLocaleString() },
      Size: { value: scopeSize ?? '—' },
    })}
    tags={[]}
    showTags={false}
    folders={[]}
    showFolders={false}
    extras={extras}
    propertyAction={exportScope && scopeVM.totalCount > 0 ? <InspectorExportAction
      count={scopeVM.totalCount}
      target={{
        kind: 'query',
        query: {
          scope: exportScope,
          filters: createEmptyItemFilters(),
          sort: { field: 'imported_at', direction: 'descending', random_seed: null },
        },
        excluded_item_ids: [],
      }}
    /> : undefined}
  />;
}

// ── Shared sub-components ───────────────────────────────────────

function Shell({ children }: { children: React.ReactNode }) {
  return (
    <div className={styles.panel} data-inspector-panel="">
      <div className={styles.scrollContent} data-inspector-scroll-content="">
        <div className={styles.contentStack}>{children}</div>
      </div>
    </div>
  );
}

/** The in-flow action lives after inspector properties, not in chrome. */
function InspectorAutoTagAction({ count, enabled }: { count: number; enabled: boolean }) {
  const autoTagDef = getShortcut('organize.autoTag');
  return (
    <KbdTooltip
      label={enabled ? 'Suggest tags with AI' : 'AI tagging requires an explicit image selection'}
      shortcut={autoTagDef ? formatKeysDisplay(autoTagDef.keys) : undefined}
    >
      <InspectorActionButton
        variant="flow"
        action="auto-tag"
        onClick={(e) => openPortal(e, aiTaggerPortalAtom)}
        disabled={!enabled}
      >
        <IconAutoTag size={14} />
        <span>{count > 1 ? `Auto Tag ${count.toLocaleString()} Images` : 'Auto Tag'}</span>
      </InspectorActionButton>
    </KbdTooltip>
  );
}

const InspectorActionButton = forwardRef<HTMLButtonElement, {
  action: string;
  children: React.ReactNode;
  className?: string;
  disabled?: boolean;
  onClick: (event: React.MouseEvent<HTMLButtonElement>) => void;
  variant?: 'default' | 'primary' | 'flow' | 'empty-section';
}>(function InspectorActionButton({
  action,
  children,
  className,
  disabled,
  onClick,
  variant = 'default',
}, ref) {
  const variantClass = variant === 'primary'
    ? styles.actionBtnPrimary
    : variant === 'flow'
      ? styles.flowActionBtn
      : variant === 'empty-section'
        ? styles.emptySectionActionBtn
      : styles.actionBtn;
  const buttonClass = [variantClass, className]
    .filter(Boolean)
    .join(' ');
  return <button ref={ref} className={buttonClass} data-inspector-action={action} data-inspector-button-primitive="action" data-inspector-button-variant={variant} data-inspector-empty-action={variant === 'empty-section' ? action : undefined} disabled={disabled} onClick={onClick} type="button">{children}</button>;
});

function SummarySpinner({ label }: { label: string }) {
  return <span className={styles.summarySpinner} data-inspector-summary-loading="" aria-label={label} />;
}

function TagsSection({ tags, onRemove, editable = true, pending = false, showLoading = false }: {
  tags: Array<{ ns: string; sub: string; raw: string }>;
  onRemove?: (raw: string) => void;
  editable?: boolean;
  pending?: boolean;
  showLoading?: boolean;
}) {
  const chipMenu = useContextMenu();
  const tagPreferences = useTagPreferences();
  const [namespaces, setNamespaces] = useState<CanonicalNamespaceSummary[]>([]);
  const hasTags = tags.length > 0;
  useEffect(() => {
    void tagsController.getNamespaceSummary().then(setNamespaces).catch(() => {});
  }, []);
  return (
    <InspectorSection
      title="Tags"
      onContextMenu={editable ? (event) => {
        event.preventDefault();
        event.stopPropagation();
        openPortal(event, tagSelectPortalAtom);
      } : undefined}
    >
      {pending ? <div className={styles.pendingSection}>{showLoading && <SummarySpinner label="Loading shared tags" />}</div> : hasTags || editable ? <div className={styles.tagsWrap}>
        {hasTags && tags.map((t) => (
          <TagChip
            key={t.raw} namespace={t.ns} subtag={t.sub}
            onRemove={onRemove ? () => onRemove(t.raw) : undefined}
            onContextMenu={(e) => {
              e.preventDefault();
              e.stopPropagation();
              chipMenu.openAt(
                { x: e.clientX, y: e.clientY },
                buildCommonTagContextEntries({
                  tag: { namespace: t.ns, subtag: t.sub },
                  namespaces,
                  starred: tagPreferences.starredTags.includes(t.raw),
                  onFilter: showTagItems,
                  onStarChange: (name, starred) => { void setTagStarred(name, starred); },
                  onRemove: onRemove ? () => onRemove(t.raw) : undefined,
                }),
              );
            }}
          />
        ))}
        {editable && !hasTags && <KbdTooltip label="Add Tags" shortcutId="organize.addTag">
          <InspectorActionButton action="add-tags" variant="empty-section" onClick={(e) => openPortal(e, tagSelectPortalAtom)}>
            <InspectorAddIcon />
            <span>Add Tags</span>
          </InspectorActionButton>
        </KbdTooltip>}
        {editable && hasTags && <KbdTooltip label="Add Tags" shortcutId="organize.addTag">
          <button
            aria-label="Add Tags"
            className={styles.tagAddBtn}
            data-inspector-action="add-tags"
            onClick={(e) => openPortal(e, tagSelectPortalAtom)}
            type="button"
          >
            <InspectorAddIcon />
          </button>
        </KbdTooltip>}
      </div> : null}
      {chipMenu.state && <ContextMenu entries={chipMenu.state.entries} position={chipMenu.state.position} onClose={chipMenu.close} />}
    </InspectorSection>
  );
}

function FoldersSection({ folders, onRemove, onNavigate, editable = true, pending = false, showLoading = false }: {
  folders: Array<{ id: number; name: string; color: string | null }>;
  onRemove?: (fid: number) => void;
  onNavigate?: (folderId: number) => void;
  editable?: boolean;
  pending?: boolean;
  showLoading?: boolean;
}) {
  const chipMenu = useContextMenu();
  const hasFolders = folders.length > 0;
  return (
    <InspectorSection
      title="Folders"
      onContextMenu={editable ? (event) => {
        event.preventDefault();
        event.stopPropagation();
        openPortal(event, folderPickerPortalAtom);
      } : undefined}
    >
      {pending ? <div className={styles.pendingSection}>{showLoading && <SummarySpinner label="Loading shared folders" />}</div> : hasFolders || editable ? <div className={styles.foldersWrap}>
        {hasFolders && folders.map((f) => (
          <TagChip
            key={f.id} namespace="" subtag={f.name} colorRgb={hexToRgb(f.color)}
            onRemove={onRemove ? () => onRemove(f.id) : undefined}
            onContextMenu={(e) => {
              e.preventDefault();
              chipMenu.openAt({ x: e.clientX, y: e.clientY }, [
                ...(onNavigate ? [{ label: 'Open Folder', action: () => onNavigate(f.id) }] : []),
                ...(onRemove ? [{ label: 'Remove', action: () => onRemove(f.id) }] : []),
              ]);
            }}
          />
        ))}
        {editable && !hasFolders && <KbdTooltip label="Add to Folder" shortcutId="organize.addFolder">
          <InspectorActionButton action="add-folder" variant="empty-section" onClick={(e) => openPortal(e, folderPickerPortalAtom)}>
            <InspectorAddIcon />
            <span>Add to Folder</span>
          </InspectorActionButton>
        </KbdTooltip>}
        {editable && hasFolders && <KbdTooltip label="Add to Folder" shortcutId="organize.addFolder">
          <button aria-label="Add to Folder" className={styles.tagAddBtn} data-inspector-action="add-folder" onClick={(e) => openPortal(e, folderPickerPortalAtom)} type="button">
            <InspectorAddIcon />
          </button>
        </KbdTooltip>}
      </div> : null}
      {chipMenu.state && <ContextMenu entries={chipMenu.state.entries} position={chipMenu.state.position} onClose={chipMenu.close} />}
    </InspectorSection>
  );
}

// ── Smart folder update helper ──────────────────────────────────

function buildSmartFolderPayload(
  scopeVM: NonNullable<ReturnType<typeof scopeInspectorViewModelAtom['read']>>,
  overrides: { name?: string; notes?: string | null },
) {
  const sf = scopeVM.smartFolder!;
  return {
    smart_folder_id: sf.smartFolderId!, name: overrides.name ?? scopeVM.node.name,
    parent_id: sf.parentId ?? null, icon: scopeVM.node.icon ?? null,
    color: scopeVM.node.color ?? null, notes: overrides.notes !== undefined ? overrides.notes : sf.notes ?? null,
    predicate_json: JSON.stringify(sf.predicate ?? { groups: [] }),
    display_order: scopeVM.node.sort_order ?? null, created_at: null, updated_at: null,
  };
}
