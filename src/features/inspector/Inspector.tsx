/**
 * Inspector panel — shows info about what's currently in focus.
 *
 * - Nothing selected → current scope (folder / system view)
 * - One item selected → that item's full details
 * - Multiple items selected → shared tags/folders from backend
 */

import { forwardRef, useEffect, useState } from 'react';
import { useAtomValue, getDefaultStore } from 'jotai';
import { IconAlertCircle, IconFolder, IconSparkles } from '@tabler/icons-react';
import { ContextMenu, useContextMenu } from '../../shared/ui/ContextMenu';
import { KbdTooltip } from '../../shared/ui/KbdTooltip';
import { ColorPalette } from '../../shared/ui/ColorPalette';
import * as entityMutations from '../../controllers/entityMutations';
import { foldersController } from '../../controllers/foldersController';
import { smartFoldersController } from '../../controllers/smartFoldersController';
import { PropertyRow } from '../../shared/ui/PropertyRow/PropertyRow';
import { InspectorSection } from '../../shared/ui/InspectorSection/InspectorSection';
import { StarRating } from '../../shared/ui/StarRating/StarRating';
import { InspectorField, InspectorSourceField } from '../../shared/ui/InspectorField/InspectorField';
import { TagChip } from '../../shared/ui/TagChip/TagChip';
import type { EntityTarget, SelectionSummary } from '../../shared/types/canonical';
import {
  displayedInspectorEntityDataAtom,
  displayedInspectorTargetAtom,
  inspectorErrorAtom,
  inspectorLoadingAtom,
  scopeInspectorViewModelAtom,
} from '../../state/inspector';
import { gridItemsAtom } from '../../state/grid';
import {
  selectionCountAtom,
  selectionFingerprintAtom,
  selectionTargetAtom,
  selectedEntityHashesAtom,
} from '../../state/selection';
import { sidebarNodesAtom } from '../../state/sidebar';
import { tagSelectPortalAtom, folderPickerPortalAtom, aiTaggerPortalAtom } from '../../state/portals';
import { getShortcut, formatKeysDisplay } from '../../shared/lib/shortcuts';
import { activeNodeIdAtom } from '../../state/navigation';
import { pushHistory } from '../../state/navigationHistory';
import { InspectorAddIcon } from '../../shared/ui/icons/toolbar-icons';
import styles from './Inspector.module.css';

const store = getDefaultStore();

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

const NS_ORDER: Record<string, number> = { creator: 0, studio: 1, series: 2, character: 3, person: 4, species: 5, meta: 6, system: 7, '': 8, default: 8 };
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
  target: EntityTarget | null | undefined,
  summary: SelectionSummary | null,
): boolean {
  if (target?.kind !== 'entity_hashes' || !summary || summary.pending) return false;
  const mimeCounts = summary.stats.mime_counts;
  if (!mimeCounts) return false;
  const imageCount = Object.entries(mimeCounts)
    .filter(([mime]) => mime.startsWith('image/'))
    .reduce((count, [, value]) => count + value, 0);
  return imageCount === summary.selected_count;
}

// ── Portal opener ───────────────────────────────────────────────

function openPortal(e: React.MouseEvent, atom: typeof tagSelectPortalAtom) {
  const btn = e.currentTarget.getBoundingClientRect();
  const panel = e.currentTarget.closest('[class*="inspector"]') as HTMLElement | null;
  const x = panel ? panel.getBoundingClientRect().left : btn.left;
  store.set(atom, { open: true, anchor: { x, y: btn.top } });
}

// ── Preview components ──────────────────────────────────────────

function Preview({ hashes, type }: { hashes: string[]; type: 'single' | 'collage' | 'stacked' }) {
  if (type === 'single' && hashes[0]) {
    return (
      <div className={styles.preview}>
        <div className={styles.previewFrame}>
          <img src={`media://localhost/thumb/${hashes[0]}.jpg`} alt="" className={styles.previewImage} draggable={false} />
          <div className={styles.previewGlass} />
        </div>
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
                <div key={i} className={styles.collageCell}>
                  {hashes[i] && <img src={`media://localhost/thumb/${hashes[i]}.jpg`} alt="" draggable={false} />}
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

  // Stacked
  const items = useAtomValue(gridItemsAtom);
  const previews = hashes.slice(0, 3).map((h) => items.find((it) => it.entity_hash === h)).filter(Boolean);
  if (previews.length === 0) return null;
  const rots = [-4, 2, 0];
  const offs = [{ x: -8, y: 4 }, { x: 6, y: -3 }, { x: 0, y: 0 }];
  const start = 3 - previews.length;
  return (
    <div className={styles.preview}>
      <div className={styles.stackContainer}>
        {previews.map((item, i) => (
          <div key={item!.entity_hash} className={styles.stackItem} style={{
            transform: `rotate(${rots[start + i]}deg) translate(${offs[start + i].x}px, ${offs[start + i].y}px)`,
            zIndex: i, filter: i === previews.length - 1 ? undefined : 'brightness(0.7)',
          }}>
            <div className={styles.previewFrame}>
              <img src={`media://localhost/thumb/${item!.entity_hash}.jpg`} alt="" className={styles.previewImage} draggable={false} />
              <div className={styles.previewGlass} />
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

// ── Multi-select summary hook ───────────────────────────────────

function useSelectionSummary() {
  const target = useAtomValue(selectionTargetAtom);
  const selectedHashes = useAtomValue(selectedEntityHashesAtom);
  const selectionCount = useAtomValue(selectionCountAtom);
  const selectionFingerprint = useAtomValue(selectionFingerprintAtom);
  const [summary, setSummary] = useState<SelectionSummary | null>(null);
  const [ready, setReady] = useState(false);

  useEffect(() => {
    if (!target || selectionCount < 2) {
      setSummary(null);
      setReady(true); // nothing to wait for
      return;
    }
    let stale = false;
    // Don't clear summary — keep old data visible until new arrives
    setReady(false);
    void entityMutations.getTargetSelectionSummary(target).then((s) => {
      if (!stale) { setSummary(s); setReady(true); }
    }).catch(() => { if (!stale) setReady(true); });
    return () => { stale = true; };
  }, [selectionCount, selectionFingerprint, target]);

  return { target, selectedHashes, summary, ready };
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

type CorePropertyValues = Partial<Record<CorePropertyLabel, Pick<CoreProperty, 'value' | 'title'>>>;

function normalizedCoreProperties(values: CorePropertyValues): CoreProperty[] {
  return CORE_PROPERTIES.flatMap(({ label, mono }) => {
    const property = values[label];
    if (!property || property.value === '' || property.value === '—') return [];
    return [{ label, mono, value: property.value, title: property.title }];
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
  name: TextFieldModel;
  notes: TextFieldModel;
  source: SourceFieldModel;
  showSource?: boolean;
  rating: { value: number; onChange?: (rating: number) => void };
  coreProperties: CoreProperty[];
  tags: Array<{ ns: string; sub: string; raw: string }>;
  showTags?: boolean;
  onRemoveTag?: (raw: string) => void;
  folders: Array<{ id: number; name: string; color: string | null }>;
  showFolders?: boolean;
  onRemoveFolder?: (folderId: number) => void;
  onNavigateFolder?: (folderId: number) => void;
  extras?: Array<{ label: string; value: string }>;
  action?: React.ReactNode;
  status?: { kind: 'loading' | 'error'; message: string };
};

/** The inspector's invariant top stack; content state changes data, not row order. */
export function InspectorSkeleton({
  preview,
  palette,
  name,
  notes,
  source,
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
  action,
  status,
}: InspectorSkeletonProps) {
  return (
    <Shell>
      {preview}
      <ColorPalette colors={palette} />

      <div className={styles.fieldStack} data-inspector-identity="">
        <div data-inspector-anchor="name">
          <InspectorField value={name.value} placeholder="Name" readOnly={name.readOnly} onCommit={name.onCommit} />
        </div>
        <div data-inspector-anchor="notes">
          <InspectorField value={notes.value} placeholder="Notes" readOnly={notes.readOnly} onCommit={notes.onCommit} />
        </div>
        {showSource && (
          <div data-inspector-anchor="source">
            <InspectorSourceField urls={source.urls} onChange={source.onChange} unavailable={source.unavailable} />
          </div>
        )}
      </div>

      {showTags && (
        <div data-inspector-section="tags">
          <TagsSection tags={tags} onRemove={onRemoveTag} editable={Boolean(onRemoveTag)} />
        </div>
      )}
      {showFolders && (
        <div data-inspector-section="folders">
          <FoldersSection folders={folders} onRemove={onRemoveFolder} onNavigate={onNavigateFolder} editable={Boolean(onRemoveFolder)} />
        </div>
      )}
      <div data-inspector-section="properties">
        <InspectorSection title="Properties">
          <div className={styles.propsStack} data-inspector-core-properties="">
            <StarRating value={rating.value} onChange={rating.onChange} />
            {coreProperties.filter((property) => property.value !== '' && property.value !== '—').map((property) => (
              <div key={property.label} data-inspector-core-property={property.label}>
                <PropertyRow {...property} />
              </div>
            ))}
          </div>
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
      rating={{ value: 0 }}
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
  const nodeId = `folder:${folderId}`;
  store.set(activeNodeIdAtom, nodeId);
  pushHistory(nodeId);
}

// ── Main component ──────────────────────────────────────────────

export function Inspector() {
  const inspectorTarget = useAtomValue(displayedInspectorTargetAtom);
  const loading = useAtomValue(inspectorLoadingAtom);
  const error = useAtomValue(inspectorErrorAtom);
  const entityData = useAtomValue(displayedInspectorEntityDataAtom);
  const scopeVM = useAtomValue(scopeInspectorViewModelAtom);
  const sidebarNodes = useAtomValue(sidebarNodesAtom);
  const { target: selTarget, selectedHashes, summary } = useSelectionSummary();

  if (inspectorTarget.kind === 'none') return null;

  // ── Single entity ─────────────────────────────────────────────
  if (inspectorTarget.kind === 'entity') {
    if (!entityData) return <UnavailableInspectorSkeleton status={error ? { kind: 'error', message: error } : loading ? { kind: 'loading', message: 'Loading...' } : undefined} />;

    const d = entityData;
    const tags = [...(d.tags ?? [])].sort((a, b) => tagKey(a.namespace, a.subtag).localeCompare(tagKey(b.namespace, b.subtag)));
    const folders = (d.folders ?? []).map((f) => {
      const n = sidebarNodes.find((s) => s.id === `folder:${f.folder_id}`);
      return { id: f.folder_id, name: n?.name ?? f.name, color: n?.color ?? null };
    });
    const palette = (d.dominant_colors ?? []).map((c) => c.hex).filter((h): h is string => !!h && h.length > 0);

    return <InspectorSkeleton
      preview={<Preview hashes={[d.entity_hash]} type="single" />}
      palette={palette.length > 0 ? palette : d.dominant_color_hex ? [d.dominant_color_hex] : []}
      name={{ value: d.name ?? '', onCommit: (value) => { void entityMutations.setEntityName(d.entity_hash, value); } }}
      notes={{ value: d.notes ?? '', onCommit: (value) => { void entityMutations.setEntityNotes(d.entity_hash, value); } }}
      source={{ urls: d.source_urls ?? [], onChange: (urls) => { void entityMutations.setEntitySourceUrls(d.entity_hash, urls); } }}
      rating={{ value: d.rating ?? 0, onChange: (rating) => { void entityMutations.setEntityRating(d.entity_hash, rating); } }}
      coreProperties={normalizedCoreProperties({
        Items: { value: '1' },
        Dimensions: { value: d.pixel_width && d.pixel_height ? `${d.pixel_width} × ${d.pixel_height}` : '—' },
        Size: { value: fmtSize(d.size_bytes) },
        Type: { value: fmtExt(d.mime_type), title: d.mime_type },
        Duration: { value: d.duration_ms != null && d.duration_ms > 0 ? fmtDuration(d.duration_ms) : '—' },
        'Date added': { value: fmtDate(d.date_added) ?? '—' },
        'Date created': { value: fmtDate(d.date_created) ?? '—' },
        'Date modified': { value: fmtDate(d.date_modified) ?? '—' },
      })}
      tags={tags.map((t) => ({ ns: t.namespace, sub: t.subtag, raw: t.namespace && t.namespace !== 'default' ? `${t.namespace}:${t.subtag}` : t.subtag }))}
      onRemoveTag={(raw) => { void entityMutations.removeEntityTags(d.entity_hash, [raw]); }}
      folders={folders}
      onRemoveFolder={(folderId) => { void entityMutations.removeEntityFromFolder(d.entity_hash, folderId); }}
      onNavigateFolder={navigateToFolder}
      action={<InspectorAutoTagAction count={1} enabled={d.mime_type.startsWith('image/')} />}
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
    const previewHashes = summary?.sample_hashes ?? [...selectedHashes].slice(0, 3);

    return <InspectorSkeleton
      preview={<Preview hashes={previewHashes} type="stacked" />}
      palette={[]}
      name={{ value: `${count.toLocaleString()} items selected`, readOnly: true }}
      notes={{ value: '', onCommit: selTarget ? (value) => { void entityMutations.setTargetNotes(selTarget, value); } : undefined }}
      source={{ urls: [], unavailable: true }}
      rating={{ value: summary?.stats?.rating_stats?.shared ?? 0, onChange: selTarget ? (rating) => { void entityMutations.setTargetRating(selTarget, rating); } : undefined }}
      coreProperties={normalizedCoreProperties({
        Items: { value: count.toLocaleString() },
        Size: { value: summary?.stats?.total_size_bytes != null ? fmtSize(summary.stats.total_size_bytes) : '—' },
      })}
      tags={tags}
      onRemoveTag={selTarget ? (raw) => { void entityMutations.removeTargetTags(selTarget, [raw]); } : undefined}
      folders={folders}
      onRemoveFolder={selTarget ? (folderId) => { void entityMutations.updateTargetFolderMembership(selTarget, folderId, 'remove'); } : undefined}
      onNavigateFolder={navigateToFolder}
      action={<InspectorAutoTagAction count={count} enabled={selectionSupportsAiTagging(selTarget, summary)} />}
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

  const extras = [
    scopeVM.searchText ? { label: 'Search', value: scopeVM.searchText } : null,
    node.kind === 'folder' ? { label: 'Auto tags', value: scopeVM.folder!.autoTags.length > 0 ? 'Yes' : 'No' } : null,
    node.kind === 'folder' ? { label: 'Watch', value: scopeVM.folder!.watchEnabled ? 'Yes' : 'No' } : null,
    node.kind === 'smart_folder' && scopeVM.smartFolder?.sortField
      ? { label: 'Sort', value: `${scopeVM.smartFolder.sortField.replace(/_/g, ' ')}${scopeVM.smartFolder.sortOrder ? ` ${scopeVM.smartFolder.sortOrder.toUpperCase()}` : ''}` }
      : null,
  ].filter((property): property is { label: string; value: string } => property !== null);

  return <InspectorSkeleton
    preview={<Preview hashes={scopeVM.previewItems.map((item) => item.entity_hash)} type="collage" />}
    palette={[]}
    name={{ value: node.name, readOnly: isSystem, onCommit: canEdit ? (value) => { void saveName(value); } : undefined }}
    notes={{ value: (isSystem ? scopeVM.description : scopeVM.folder?.notes ?? scopeVM.smartFolder?.notes) ?? '', readOnly: isSystem, onCommit: canEdit ? (value) => { void saveNotes(value); } : undefined }}
    source={{ urls: [], unavailable: true }}
    showSource={false}
    rating={{ value: 0 }}
    coreProperties={normalizedCoreProperties({
      Items: { value: scopeVM.totalCount.toLocaleString() },
      Size: { value: scopeSize ?? '—' },
    })}
    tags={[]}
    showTags={false}
    folders={[]}
    showFolders={false}
    extras={extras}
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

/** reference application's analogous export action lives after inspector properties, not in chrome. */
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
        <IconSparkles size={14} stroke={1.5} />
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

function TagsSection({ tags, onRemove, onNavigate, editable = true }: {
  tags: Array<{ ns: string; sub: string; raw: string }>;
  onRemove?: (raw: string) => void;
  onNavigate?: (tag: string) => void;
  editable?: boolean;
}) {
  const chipMenu = useContextMenu();
  const hasTags = tags.length > 0;
  return (
    <InspectorSection title="Tags" count={tags.length}>
      {hasTags || editable ? <div className={styles.tagsWrap}>
        {hasTags && tags.map((t) => (
          <TagChip
            key={t.raw} namespace={t.ns} subtag={t.sub}
            onRemove={onRemove ? () => onRemove(t.raw) : undefined}
            onContextMenu={(e) => {
              e.preventDefault();
              chipMenu.openAt({ x: e.clientX, y: e.clientY }, [
                ...(onNavigate ? [{ label: 'Show Items', action: () => onNavigate(t.raw) }] : []),
                { label: 'Copy', action: () => { (window as any).picto?.clipboard?.writeText(t.raw) ?? navigator.clipboard.writeText(t.raw); } },
                ...(onRemove ? [{ label: 'Remove', action: () => onRemove(t.raw), danger: true }] : []),
              ]);
            }}
          />
        ))}
        {editable && !hasTags && <KbdTooltip label="Add Tags" shortcut="T">
          <InspectorActionButton action="add-tags" variant="empty-section" onClick={(e) => openPortal(e, tagSelectPortalAtom)}>
            <InspectorAddIcon />
            <span>Add Tags</span>
          </InspectorActionButton>
        </KbdTooltip>}
        {editable && hasTags && <KbdTooltip label="Add Tags" shortcut="T">
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

function FoldersSection({ folders, onRemove, onNavigate, editable = true }: {
  folders: Array<{ id: number; name: string; color: string | null }>;
  onRemove?: (fid: number) => void;
  onNavigate?: (folderId: number) => void;
  editable?: boolean;
}) {
  const chipMenu = useContextMenu();
  const hasFolders = folders.length > 0;
  return (
    <InspectorSection title="Folders" count={folders.length}>
      {hasFolders || editable ? <div className={styles.foldersWrap}>
        {hasFolders && folders.map((f) => (
          <TagChip
            key={f.id} namespace="" subtag={f.name} colorRgb={hexToRgb(f.color)}
            onRemove={onRemove ? () => onRemove(f.id) : undefined}
            onContextMenu={(e) => {
              e.preventDefault();
              chipMenu.openAt({ x: e.clientX, y: e.clientY }, [
                ...(onNavigate ? [{ label: 'Open Folder', action: () => onNavigate(f.id) }] : []),
                ...(onRemove ? [{ label: 'Remove', action: () => onRemove(f.id), danger: true }] : []),
              ]);
            }}
          />
        ))}
        {editable && !hasFolders && <KbdTooltip label="Add to Folder" shortcut="F">
          <InspectorActionButton action="add-folder" variant="empty-section" onClick={(e) => openPortal(e, folderPickerPortalAtom)}>
            <InspectorAddIcon />
            <span>Add to Folder</span>
          </InspectorActionButton>
        </KbdTooltip>}
        {editable && hasFolders && <KbdTooltip label="Add to Folder" shortcut="F">
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
    sort_field: sf.sortField ?? null, sort_order: sf.sortOrder ?? null,
    display_order: scopeVM.node.sort_order ?? null, created_at: null, updated_at: null,
  };
}
