/**
 * Inspector panel — shows info about what's currently in focus.
 *
 * - Nothing selected → current scope (folder / system view)
 * - One item selected → that item's full details
 * - Multiple items selected → shared tags/folders from backend
 */

import { useEffect, useState } from 'react';
import { useAtomValue, getDefaultStore } from 'jotai';
import { IconAlertCircle, IconFolder, IconPlus } from '@tabler/icons-react';
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
import type { SelectionSummary } from '../../shared/types/canonical';
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
              <img src={`media://localhost/thumb/${(item!.thumbnail_hash ?? item!.entity_hash)}.jpg`} alt="" className={styles.previewImage} draggable={false} />
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
    if (loading && !entityData) return <Shell><div className={styles.empty}><span className={styles.emptyText}>Loading...</span></div></Shell>;
    if (error && !entityData) return <Shell><div className={styles.empty}><IconAlertCircle size={20} stroke={1.5} className={styles.errorIcon} /><span className={styles.errorText}>{error}</span></div></Shell>;
    if (!entityData) return null;

    const d = entityData;
    const isCollection = d.entity_kind === 'collection';
    const tags = [...(d.tags ?? [])].sort((a, b) => tagKey(a.namespace, a.subtag).localeCompare(tagKey(b.namespace, b.subtag)));
    const folders = (d.folders ?? []).map((f) => {
      const n = sidebarNodes.find((s) => s.id === `folder:${f.folder_id}`);
      return { id: f.folder_id, name: n?.name ?? f.name, color: n?.color ?? null };
    });
    const palette = (d.dominant_colors ?? []).map((c) => c.hex).filter((h): h is string => !!h && h.length > 0);

    return (
      <Shell footer={<InspectorActionBar count={1} />}>
        <Preview hashes={[d.thumbnail_hash]} type="single" />
        <ColorPalette colors={palette.length > 0 ? palette : d.dominant_color_hex ? [d.dominant_color_hex] : []} />

        <div className={styles.fieldStack}>
          <InspectorField value={d.name ?? ''} placeholder="Name" onCommit={(v) => { void entityMutations.setEntityName(d.entity_hash, v); }} />
          <InspectorField value={typeof d.notes === 'string' ? d.notes : ''} placeholder="Notes" onCommit={(v) => { void entityMutations.setEntityNotes(d.entity_hash, v); }} />
          <InspectorSourceField urls={d.source_urls ?? []} onChange={(urls) => { void entityMutations.setEntitySourceUrls(d.entity_hash, urls); }} />
        </div>

        <TagsSection
          tags={tags.map((t) => ({ ns: t.namespace, sub: t.subtag, raw: t.namespace && t.namespace !== 'default' ? `${t.namespace}:${t.subtag}` : t.subtag }))}
          onRemove={(raw) => { void entityMutations.removeEntityTags(d.entity_hash, [raw]); }}
        />

        <FoldersSection
          folders={folders}
          onRemove={(fid) => { void entityMutations.removeEntityFromFolder(d.entity_hash, fid); }}
          onNavigate={(fid) => { const nodeId = `folder:${fid}`; store.set(activeNodeIdAtom, nodeId); pushHistory(nodeId); }}
        />

        <InspectorSection title="Properties">
          <div className={styles.propsStack}>
            <StarRating value={d.rating ?? 0} onChange={(r) => { void entityMutations.setEntityRating(d.entity_hash, r); }} />
            {!isCollection && d.pixel_width && d.pixel_height && <PropertyRow label="Dimensions" value={`${d.pixel_width} × ${d.pixel_height}`} mono />}
            {!isCollection && <PropertyRow label="Size" value={fmtSize(d.size_bytes)} mono />}
            {!isCollection && <PropertyRow label="Type" value={fmtExt(d.mime_type)} title={d.mime_type} />}
            {!isCollection && d.duration_ms != null && d.duration_ms > 0 && <PropertyRow label="Duration" value={fmtDuration(d.duration_ms)} mono />}
            {isCollection && <PropertyRow label="Items" value={d.member_count?.toLocaleString() ?? '0'} mono />}
            {isCollection && d.total_size_bytes != null && <PropertyRow label="Total size" value={fmtSize(d.total_size_bytes)} mono />}
            <PropertyRow label="Date added" value={fmtDate(d.date_added)} mono />
            <PropertyRow label="Date created" value={fmtDate(d.date_created)} mono />
            <PropertyRow label="Date modified" value={fmtDate(d.date_modified)} mono />
          </div>
        </InspectorSection>
      </Shell>
    );
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

    return (
      <Shell footer={<InspectorActionBar count={count} />}>
        <Preview hashes={previewHashes} type="stacked" />
        <ColorPalette colors={[]} />

        <div className={styles.fieldStack}>
          <InspectorField value={`${count.toLocaleString()} items selected`} placeholder="Name" readOnly />
          <InspectorField value="" placeholder="Notes" onCommit={selTarget ? (v) => { void entityMutations.setTargetNotes(selTarget, v); } : undefined} />
        </div>

        <TagsSection
          tags={tags}
          onRemove={selTarget ? (raw) => { void entityMutations.removeTargetTags(selTarget, [raw]); } : undefined}
        />

        <FoldersSection
          folders={folders}
          onRemove={selTarget ? (fid) => { void entityMutations.updateTargetFolderMembership(selTarget, fid, 'remove'); } : undefined}
          onNavigate={(fid) => { const nodeId = `folder:${fid}`; store.set(activeNodeIdAtom, nodeId); pushHistory(nodeId); }}
        />

        <InspectorSection title="Properties">
          <div className={styles.propsStack}>
            <StarRating
              value={summary?.stats?.rating_stats?.shared ?? 0}
              onChange={selTarget ? (r) => { void entityMutations.setTargetRating(selTarget, r); } : undefined}
            />
            <PropertyRow label="Total size" value={summary?.stats?.total_size_bytes != null ? fmtSize(summary.stats.total_size_bytes) : '—'} mono />
          </div>
        </InspectorSection>
      </Shell>
    );
  }

  // ── Scope (nothing selected — show current view) ──────────────
  if (!scopeVM) return null;
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

  return (
    <Shell>
      <Preview hashes={scopeVM.previewItems.map((i) => i.thumbnail_hash)} type="collage" />
      <ColorPalette colors={[]} />

      <div className={styles.fieldStack}>
        <InspectorField value={node.name} placeholder="Name" readOnly={isSystem} onCommit={canEdit ? (v) => { void saveName(v); } : undefined} />
        <InspectorField
          value={(isSystem ? scopeVM.description : scopeVM.folder?.notes ?? scopeVM.smartFolder?.notes) ?? ''}
          placeholder="Notes" readOnly={isSystem}
          onCommit={canEdit ? (v) => { void saveNotes(v); } : undefined}
        />
      </div>

      <InspectorSection title="Properties">
        <div className={styles.propsStack}>
          <PropertyRow label="Items" value={scopeVM.totalCount.toLocaleString()} mono />
          {scopeSize != null && <PropertyRow label="Size" value={scopeSize} mono />}
          {scopeVM.searchText && <PropertyRow label="Search" value={scopeVM.searchText} />}
          {node.kind === 'folder' && <PropertyRow label="Auto tags" value={scopeVM.folder!.autoTags.length > 0 ? 'Yes' : 'No'} />}
          {node.kind === 'folder' && <PropertyRow label="Watch" value={scopeVM.folder!.watchEnabled ? 'Yes' : 'No'} />}
          {node.kind === 'smart_folder' && scopeVM.smartFolder?.sortField && (
            <PropertyRow label="Sort" value={`${scopeVM.smartFolder.sortField.replace(/_/g, ' ')}${scopeVM.smartFolder.sortOrder ? ` ${scopeVM.smartFolder.sortOrder.toUpperCase()}` : ''}`} />
          )}
        </div>
      </InspectorSection>
    </Shell>
  );
}

// ── Shared sub-components ───────────────────────────────────────

function Shell({ children, footer }: { children: React.ReactNode; footer?: React.ReactNode }) {
  return (
    <div className={styles.panel} data-inspector-panel="">
      <div className={styles.scrollContent}>
        <div className={styles.contentStack}>{children}</div>
      </div>
      {footer}
    </div>
  );
}

/** Pinned action bar at the bottom of the inspector: Auto Tag + Export
 * (export is a reserved slot, not available yet). */
function InspectorActionBar({ count }: { count: number }) {
  const autoTagDef = getShortcut('organize.autoTag');
  return (
    <div className={styles.actionBar}>
      <KbdTooltip
        label="Suggest tags with AI"
        shortcut={autoTagDef ? formatKeysDisplay(autoTagDef.keys) : undefined}
      >
        <button
          className={styles.actionBtnPrimary}
          onClick={(e) => openPortal(e, aiTaggerPortalAtom)}
          type="button"
        >
          {count > 1 ? `Auto Tag ${count.toLocaleString()} Images` : 'Auto Tag'}
        </button>
      </KbdTooltip>
      <button className={styles.actionBtn} type="button" disabled>
        Export
      </button>
    </div>
  );
}

function TagsSection({ tags, onRemove, onNavigate }: {
  tags: Array<{ ns: string; sub: string; raw: string }>;
  onRemove?: (raw: string) => void;
  onNavigate?: (tag: string) => void;
}) {
  const chipMenu = useContextMenu();
  return (
    <InspectorSection title="Tags" count={tags.length}>
      <div className={styles.tagsWrap}>
        {tags.map((t) => (
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
        <KbdTooltip label="Add Tags" shortcut="T">
          <button className={styles.tagAddBtn} onClick={(e) => openPortal(e, tagSelectPortalAtom)} type="button">
            <IconPlus size={14} stroke={1.5} />
          </button>
        </KbdTooltip>
      </div>
      {chipMenu.state && <ContextMenu entries={chipMenu.state.entries} position={chipMenu.state.position} onClose={chipMenu.close} searchable={false} />}
    </InspectorSection>
  );
}

function FoldersSection({ folders, onRemove, onNavigate }: {
  folders: Array<{ id: number; name: string; color: string | null }>;
  onRemove?: (fid: number) => void;
  onNavigate?: (folderId: number) => void;
}) {
  const chipMenu = useContextMenu();
  return (
    <InspectorSection title="Folders" count={folders.length}>
      <div className={styles.foldersWrap}>
        {folders.map((f) => (
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
        <KbdTooltip label="Add to Folder" shortcut="F">
          <button className={styles.tagAddBtn} onClick={(e) => openPortal(e, folderPickerPortalAtom)} type="button">
            <IconPlus size={14} stroke={1.5} />
          </button>
        </KbdTooltip>
      </div>
      {chipMenu.state && <ContextMenu entries={chipMenu.state.entries} position={chipMenu.state.position} onClose={chipMenu.close} searchable={false} />}
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
