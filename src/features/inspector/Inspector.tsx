/**
 * Inspector panel — persistent right-hand context surface for grid view.
 *
 * Entity mode renders a committed displayed entity snapshot.
 * Scope mode renders a committed displayed grid snapshot.
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import { useAtomValue } from 'jotai';
import {
  IconAlertCircle,
  IconFolder,
  IconPlus,
} from '@tabler/icons-react';
import { ColorPalette } from '../../shared/ui/ColorPalette';
import * as api from '../../platform/api';
import * as entityMutations from '../../controllers/entityMutations';
import { PropertyRow } from '../../shared/ui/PropertyRow/PropertyRow';
import { InspectorSection } from '../../shared/ui/InspectorSection/InspectorSection';
import { StarRating } from '../../shared/ui/StarRating/StarRating';
import { InspectorField, InspectorSourceField } from '../../shared/ui/InspectorField/InspectorField';
import { TagChip } from '../../shared/ui/TagChip/TagChip';
import {
  displayedInspectorEntityDataAtom,
  displayedInspectorTargetAtom,
  inspectorErrorAtom,
  inspectorLoadingAtom,
  scopeInspectorViewModelAtom,
} from '../../state/inspector';
import { gridItemsAtom } from '../../state/grid';
import { selectionTargetAtom, selectedEntityHashesAtom } from '../../state/selection';
import { sidebarNodesAtom } from '../../state/sidebar';
import { promptForFolderId } from '../../shared/lib/selectFolderPrompt';
import styles from './Inspector.module.css';

function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

function formatDateTime(iso: string | null | undefined): string | null {
  if (!iso) return null;
  try {
    return new Date(iso).toLocaleDateString(undefined, {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
    });
  } catch {
    return iso;
  }
}

function formatDuration(ms: number): string {
  const s = Math.floor(ms / 1000);
  const m = Math.floor(s / 60);
  const sec = s % 60;
  if (m >= 60) {
    const h = Math.floor(m / 60);
    return `${h}:${String(m % 60).padStart(2, '0')}:${String(sec).padStart(2, '0')}`;
  }
  return `${m}:${String(sec).padStart(2, '0')}`;
}

const MIME_EXT_MAP: Record<string, string> = {
  jpeg: 'JPG', png: 'PNG', gif: 'GIF', webp: 'WEBP', 'svg+xml': 'SVG',
  bmp: 'BMP', tiff: 'TIFF', avif: 'AVIF', heic: 'HEIC', heif: 'HEIF',
  mp4: 'MP4', webm: 'WEBM', quicktime: 'MOV', 'x-matroska': 'MKV',
  'x-msvideo': 'AVI', 'x-flv': 'FLV',
  pdf: 'PDF', 'epub+zip': 'EPUB',
};

function getFileExt(mime: string): string {
  const sub = mime.split('/')[1] ?? '';
  return MIME_EXT_MAP[sub] ?? sub.replace(/^x-/, '').toUpperCase();
}

const NS_ORDER: Record<string, number> = {
  creator: 0, studio: 1, series: 2, character: 3, person: 4,
  species: 5, meta: 6, system: 7, '': 8, default: 8,
};


function tagSortKey(ns: string, sub: string): string {
  return `${(NS_ORDER[ns.toLowerCase()] ?? 7).toString().padStart(2, '0')}:${sub.toLowerCase()}`;
}

function formatLabel(value: string): string {
  return value.replace(/_/g, ' ').replace(/\b\w/g, (match) => match.toUpperCase());
}

function hexToRgb(hex: string | null | undefined): [number, number, number] {
  if (!hex) return [134, 142, 150]; // default gray
  const h = hex.replace('#', '');
  return [
    parseInt(h.substring(0, 2), 16),
    parseInt(h.substring(2, 4), 16),
    parseInt(h.substring(4, 6), 16),
  ];
}


function ScopePreview({ items }: { items: Array<{ thumbnail_hash: string; entity_hash: string }> }) {
  const thumbs = items.slice(0, 4);
  const hasImages = thumbs.length > 0;

  return (
    <div className={styles.thumbnail}>
      <div className={styles.pic3} />
      <div className={styles.pic2} />
      <div className={styles.pic1}>
        {hasImages ? (
          <div className={styles.collage}>
            {[0, 1, 2, 3].map((i) => {
              const item = thumbs[i % thumbs.length];
              return (
                <div key={i} className={styles.collageCell}>
                  <img src={`media://localhost/thumb/${item.thumbnail_hash}.jpg`} alt="" draggable={false} />
                </div>
              );
            })}
          </div>
        ) : (
          <div className={styles.folderPlaceholder}>
            <IconFolder size={32} stroke={1} />
          </div>
        )}
      </div>
    </div>
  );
}


function TagInput({ onAdd }: { onAdd: (tag: string) => void }) {
  const [value, setValue] = useState('');
  const [open, setOpen] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  const commit = useCallback(() => {
    const trimmed = value.trim();
    if (trimmed) {
      onAdd(trimmed);
      setValue('');
    }
    setOpen(false);
  }, [value, onAdd]);

  if (!open) {
    return (
      <button
        className={styles.tagAddBtn}
        onClick={() => { setOpen(true); setTimeout(() => inputRef.current?.focus(), 0); }}
        type="button"
        title="Add tag"
      >
        <IconPlus size={14} stroke={1.5} />
      </button>
    );
  }

  return (
    <input
      ref={inputRef}
      className={styles.tagInput}
      value={value}
      placeholder="tag"
      onChange={(e) => setValue(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === 'Enter') commit();
        if (e.key === 'Escape') { setValue(''); setOpen(false); }
      }}
      onBlur={commit}
    />
  );
}

function EntityInspector() {
  const data = useAtomValue(displayedInspectorEntityDataAtom);
  const loading = useAtomValue(inspectorLoadingAtom);
  const error = useAtomValue(inspectorErrorAtom);
  const sidebarNodes = useAtomValue(sidebarNodesAtom);

  if (loading && !data) {
    return (
      <div className={styles.empty}>
        <span className={styles.emptyText}>Loading…</span>
      </div>
    );
  }

  if (error && !data) {
    return (
      <div className={styles.empty}>
        <IconAlertCircle size={20} stroke={1.5} className={styles.errorIcon} />
        <span className={styles.errorText}>{error}</span>
      </div>
    );
  }

  if (!data) return null;

  const isCollection = data.entity_kind === 'collection';
  const ext = getFileExt(data.mime_type);
  const dims =
    data.pixel_width && data.pixel_height
      ? `${data.pixel_width} × ${data.pixel_height}`
      : null;
  const sortedTags = [...(data.tags ?? [])].sort((a, b) =>
    tagSortKey(a.namespace, a.subtag).localeCompare(tagSortKey(b.namespace, b.subtag)),
  );
  const sourceUrls = data.source_urls ?? [];
  const folders = data.folders ?? [];

  return (
    <div className={styles.contentStack}>
      <div className={styles.preview}>
        <div className={styles.previewFrame}>
          <img
            src={`media://localhost/thumb/${data.thumbnail_hash}.jpg`}
            alt={data.name ?? ''}
            className={styles.previewImage}
            draggable={false}
          />
          <div className={styles.previewGlass} />
        </div>
      </div>

      <ColorPalette colors={data.dominant_color_hex ? [data.dominant_color_hex] : []} />

      <div className={styles.fieldStack}>
        <InspectorField
          value={data.name ?? ''}
          placeholder="Name"
          onCommit={(name) => { void entityMutations.setEntityName(data.entity_hash, name); }}
        />
        <InspectorField
          value={typeof data.notes === 'string' ? data.notes : ''}
          placeholder="Notes"

          onCommit={(text) => { void entityMutations.setEntityNotes(data.entity_hash, text); }}
        />
        <InspectorSourceField
          urls={sourceUrls}
          onChange={(urls) => { void entityMutations.setEntitySourceUrls(data.entity_hash, urls); }}
        />
      </div>

      <InspectorSection title="Tags" count={sortedTags.length}>
        <div className={styles.tagsWrap}>
          {sortedTags.map((tag) => {
            const rawTag = tag.namespace && tag.namespace !== 'default' ? `${tag.namespace}:${tag.subtag}` : tag.subtag;
            return (
              <TagChip
                key={rawTag}
                namespace={tag.namespace}
                subtag={tag.subtag}
                onRemove={() => { void entityMutations.removeEntityTags(data.entity_hash, [rawTag]); }}
              />
            );
          })}
          <TagInput onAdd={(tag) => { void entityMutations.addEntityTags(data.entity_hash, [tag]); }} />
        </div>
      </InspectorSection>

      <InspectorSection title="Folders" count={folders.length}>
        <div className={styles.foldersWrap}>
          {folders.map((folder) => {
            const node = sidebarNodes.find((n) => n.id === `folder:${folder.folder_id}`);
            return (
              <TagChip
                key={folder.folder_id}
                namespace=""
                subtag={node?.name ?? folder.name}
                colorRgb={hexToRgb(node?.color)}
                onRemove={() => { void entityMutations.removeEntityFromFolder(data.entity_hash, folder.folder_id); }}
              />
            );
          })}
          <button
            className={styles.tagAddBtn}
            onClick={() => {
              const folderId = promptForFolderId(sidebarNodes);
              if (folderId != null) {
                void entityMutations.updateTargetFolderMembership(
                  { kind: 'entity_hashes', entity_hashes: [data.entity_hash] },
                  folderId,
                  'add',
                );
              }
            }}
            type="button"
            title="Add to folder"
          >
            <IconPlus size={14} stroke={1.5} />
          </button>
        </div>
      </InspectorSection>

      <InspectorSection title="Properties">
        <div className={styles.propsStack}>
          <StarRating
            value={data.rating ?? 0}
            onChange={(rating) => { void entityMutations.setEntityRating(data.entity_hash, rating); }}
          />

          {!isCollection && (
            <>
              <PropertyRow label="Dimensions" value={dims} mono />
              <PropertyRow label="Size" value={formatFileSize(data.size_bytes)} mono />
              <PropertyRow label="Type" value={ext} title={data.mime_type} />
              {data.duration_ms != null && data.duration_ms > 0 && (
                <PropertyRow label="Duration" value={formatDuration(data.duration_ms)} mono />
              )}
            </>
          )}

          {isCollection && (
            <>
              <PropertyRow label="Items" value={data.member_count?.toLocaleString() ?? '0'} mono />
              <PropertyRow
                label="Total size"
                value={data.total_size_bytes != null ? formatFileSize(data.total_size_bytes) : '—'}
                mono
              />
            </>
          )}

          <PropertyRow label="Date added" value={formatDateTime(data.date_added)} mono />
          <PropertyRow label="Date created" value={formatDateTime(data.date_created)} mono />
          <PropertyRow label="Date modified" value={formatDateTime(data.date_modified)} mono />
        </div>
      </InspectorSection>
    </div>
  );
}

function ScopeInspector() {
  const vm = useAtomValue(scopeInspectorViewModelAtom);
  if (!vm) return null;

  const isSystem = vm.node.kind === 'system';
  const isFolder = vm.node.kind === 'folder';
  const isSmartFolder = vm.node.kind === 'smart_folder';
  const canEditFields = !isSystem;

  const notesValue =
    isSystem
      ? vm.description
      : vm.folder?.notes ?? vm.smartFolder?.notes ?? null;

  const saveName = async (nextValue: string) => {
    if (vm.folder?.folderId != null) {
      await api.renameFolder(vm.folder.folderId, nextValue);
      return;
    }
    if (vm.smartFolder?.smartFolderId != null) {
      await api.updateSmartFolder({
        id: String(vm.smartFolder.smartFolderId),
        folder: {
          smart_folder_id: vm.smartFolder.smartFolderId,
          name: nextValue,
          parent_id: vm.smartFolder.parentId ?? null,
          icon: vm.node.icon ?? null,
          color: vm.node.color ?? null,
          notes: vm.smartFolder.notes ?? null,
          predicate_json: JSON.stringify(vm.smartFolder.predicate ?? { groups: [] }),
          sort_field: vm.smartFolder.sortField ?? null,
          sort_order: vm.smartFolder.sortOrder ?? null,
          display_order: vm.node.sort_order ?? null,
          created_at: null,
          updated_at: null,
        },
      });
    }
  };

  const saveNotes = async (nextValue: string) => {
    if (vm.folder?.folderId != null) {
      await api.updateFolder(vm.folder.folderId, { notes: nextValue || null });
      return;
    }
    if (vm.smartFolder?.smartFolderId != null) {
      await api.updateSmartFolder({
        id: String(vm.smartFolder.smartFolderId),
        folder: {
          smart_folder_id: vm.smartFolder.smartFolderId,
          name: vm.node.name,
          parent_id: vm.smartFolder.parentId ?? null,
          icon: vm.node.icon ?? null,
          color: vm.node.color ?? null,
          notes: nextValue || null,
          predicate_json: JSON.stringify(vm.smartFolder.predicate ?? { groups: [] }),
          sort_field: vm.smartFolder.sortField ?? null,
          sort_order: vm.smartFolder.sortOrder ?? null,
          display_order: vm.node.sort_order ?? null,
          created_at: null,
          updated_at: null,
        },
      });
    }
  };

  const sizeDisplay = vm.totalSizeBytes != null && vm.totalSizeBytes > 0
    ? formatFileSize(vm.totalSizeBytes)
    : vm.totalSizeBytes === 0 ? '0' : null;

  return (
    <div className={styles.contentStack}>
      <div className={styles.preview}>
        <ScopePreview items={vm.previewItems} />
      </div>

      <ColorPalette colors={[]} />

      <div className={styles.fieldStack}>
        <InspectorField
          value={vm.node.name}
          placeholder="Name"
          readOnly={isSystem}
          onCommit={canEditFields ? (name) => { void saveName(name); } : undefined}
        />
        <InspectorField
          value={notesValue ?? ''}
          placeholder="Notes"

          readOnly={isSystem}
          onCommit={canEditFields ? (text) => { void saveNotes(text); } : undefined}
        />
      </div>

      <InspectorSection title="Properties">
        <div className={styles.propsStack}>
          <PropertyRow label="Items" value={vm.totalCount.toLocaleString()} mono />
          <PropertyRow label="Size" value={sizeDisplay} mono />
          {vm.searchText && <PropertyRow label="Search" value={vm.searchText} />}
          {isFolder && (
            <>
              <PropertyRow label="Auto tags" value={vm.folder!.autoTags.length > 0 ? 'Yes' : 'No'} />
              <PropertyRow label="Watch" value={vm.folder!.watchEnabled ? 'Yes' : 'No'} />
            </>
          )}
          {isSmartFolder && (
            <PropertyRow
              label="Sort"
              value={
                vm.smartFolder?.sortField
                  ? `${formatLabel(vm.smartFolder.sortField)}${
                      vm.smartFolder.sortOrder ? ` ${vm.smartFolder.sortOrder.toUpperCase()}` : ''
                    }`
                  : null
              }
            />
          )}
        </div>
      </InspectorSection>
    </div>
  );
}

function StackedPreview({ hashes }: { hashes: string[] }) {
  const items = useAtomValue(gridItemsAtom);
  const previewItems = hashes
    .slice(0, 3)
    .map((h) => items.find((it) => it.entity_hash === h))
    .filter(Boolean) as typeof items;

  if (previewItems.length === 0) return null;

  const rotations = [-4, 2, 0];
  const offsets = [{ x: -8, y: 4 }, { x: 6, y: -3 }, { x: 0, y: 0 }];
  const startIdx = 3 - previewItems.length;

  return (
    <div className={styles.preview}>
      <div className={styles.stackContainer}>
        {previewItems.map((item, i) => {
          const idx = startIdx + i;
          const isTop = i === previewItems.length - 1;
          const thumbHash = item.thumbnail_hash ?? item.entity_hash;
          return (
            <div
              key={item.entity_hash}
              className={styles.stackItem}
              style={{
                transform: `rotate(${rotations[idx]}deg) translate(${offsets[idx].x}px, ${offsets[idx].y}px)`,
                zIndex: i,
                filter: isTop ? undefined : 'brightness(0.7)',
              }}
            >
              <div className={styles.previewFrame}>
                <img
                  src={`media://localhost/thumb/${thumbHash}.jpg`}
                  alt=""
                  className={styles.previewImage}
                  draggable={false}
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

function MultiSelectInspector({
  count,
  selectionMode,
}: {
  count: number;
  selectionMode: 'explicit' | 'query_results';
}) {
  const target = useAtomValue(selectionTargetAtom);
  const sidebarNodes = useAtomValue(sidebarNodesAtom);
  const selectedHashes = useAtomValue(selectedEntityHashesAtom);
  const [summary, setSummary] = useState<import('../../shared/types/canonical').SelectionSummary | null>(null);

  // Fetch selection summary whenever target changes
  useEffect(() => {
    if (!target) { setSummary(null); return; }
    let stale = false;
    void entityMutations.getTargetSelectionSummary(target).then((s) => {
      if (!stale) setSummary(s);
    }).catch(() => {});
    return () => { stale = true; };
  }, [target]);

  if (!target) return null;

  const isVirtualAll = selectionMode === 'query_results';
  const sharedRating = summary?.stats?.rating_stats?.shared ?? null;
  const totalSize = summary?.stats?.total_size_bytes;
  const sharedTags = summary?.shared_tags ?? [];
  const topTags = summary?.top_tags ?? [];
  const sharedFolders = summary?.shared_folders ?? [];
  const previewHashes = summary?.sample_hashes ?? [...selectedHashes].slice(0, 3);

  return (
    <div className={styles.contentStack}>
      <StackedPreview hashes={previewHashes} />

      <ColorPalette colors={[]} />

      <div className={styles.fieldStack}>
        <InspectorField
          value={isVirtualAll
            ? `All ${count.toLocaleString()} items selected`
            : `${count.toLocaleString()} items selected`}
          placeholder="Name"
          readOnly
        />
        <InspectorField
          value=""
          placeholder="Notes"
          onCommit={(text) => { void entityMutations.setTargetNotes(target, text); }}
        />
      </div>

      <InspectorSection title="Tags" count={sharedTags.length}>
        <div className={styles.tagsWrap}>
          {sharedTags.map((t) => (
            <TagChip
              key={t.tag}
              namespace={t.tag.includes(':') ? t.tag.split(':')[0] : ''}
              subtag={t.tag.includes(':') ? t.tag.split(':').slice(1).join(':') : t.tag}
              onRemove={() => { void entityMutations.removeTargetTags(target, [t.tag]); }}
            />
          ))}
          {topTags.filter((t) => !sharedTags.some((s) => s.tag === t.tag)).slice(0, 10).map((t) => (
            <TagChip
              key={`top:${t.tag}`}
              namespace={t.tag.includes(':') ? t.tag.split(':')[0] : ''}
              subtag={t.tag.includes(':') ? t.tag.split(':').slice(1).join(':') : t.tag}
            />
          ))}
          <TagInput onAdd={(tag) => { void entityMutations.addTargetTags(target, [tag]); }} />
        </div>
      </InspectorSection>

      <InspectorSection title="Folders" count={sharedFolders.length}>
        <div className={styles.foldersWrap}>
          {sharedFolders.map((folder) => {
            const node = sidebarNodes.find((n) => n.id === `folder:${folder.folder_id}`);
            return (
              <TagChip
                key={folder.folder_id}
                namespace=""
                subtag={node?.name ?? folder.name}
                colorRgb={hexToRgb(node?.color)}
                onRemove={() => { void entityMutations.updateTargetFolderMembership(target, folder.folder_id, 'remove'); }}
              />
            );
          })}
          <button
            className={styles.tagAddBtn}
            onClick={() => {
              const folderId = promptForFolderId(sidebarNodes);
              if (folderId != null) { void entityMutations.updateTargetFolderMembership(target, folderId, 'add'); }
            }}
            type="button"
            title="Add to folder"
          >
            <IconPlus size={14} stroke={1.5} />
          </button>
        </div>
      </InspectorSection>

      <InspectorSection title="Properties">
        <div className={styles.propsStack}>
          <StarRating
            value={sharedRating ?? 0}
            onChange={(rating) => { void entityMutations.setTargetRating(target, rating); }}
          />
          <PropertyRow label="Total size" value={totalSize != null ? formatFileSize(totalSize) : '—'} mono />
        </div>
      </InspectorSection>
    </div>
  );
}

export function Inspector() {
  const target = useAtomValue(displayedInspectorTargetAtom);

  if (target.kind === 'none') return null;

  return (
    <div className={styles.panel}>
      <div className={styles.scrollContent}>
        {target.kind === 'entity' ? (
          <EntityInspector />
        ) : target.kind === 'multi' ? (
          <MultiSelectInspector count={target.count} selectionMode={target.selectionMode} />
        ) : (
          <ScopeInspector />
        )}
      </div>
    </div>
  );
}
