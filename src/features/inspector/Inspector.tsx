/**
 * Inspector panel — persistent right-hand context surface for grid view.
 *
 * Entity mode renders a committed displayed entity snapshot.
 * Scope mode renders a committed displayed grid snapshot.
 */

import { useEffect, useMemo, useState, type CSSProperties } from 'react';
import { useAtomValue } from 'jotai';
import {
  IconAlertCircle,
  IconFolder,
} from '@tabler/icons-react';
import * as api from '../../platform/api';
import { PropertyRow } from '../../shared/ui/PropertyRow/PropertyRow';
import { InspectorSection } from '../../shared/ui/InspectorSection/InspectorSection';
import { StarRating } from '../../shared/ui/StarRating/StarRating';
import {
  displayedInspectorEntityDataAtom,
  displayedInspectorTargetAtom,
  inspectorErrorAtom,
  inspectorLoadingAtom,
  scopeInspectorViewModelAtom,
} from '../../state/inspector';
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

function extractDomain(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, '');
  } catch {
    return url;
  }
}

const NS_ORDER: Record<string, number> = {
  creator: 0, studio: 1, series: 2, character: 3, person: 4,
  species: 5, meta: 6, system: 7, '': 8, default: 8,
};

const NS_COLORS: Record<string, [number, number, number]> = {
  creator: [170, 0, 0],
  studio: [128, 0, 0],
  character: [0, 170, 0],
  person: [0, 128, 0],
  series: [170, 0, 170],
  species: [0, 130, 170],
  meta: [160, 160, 160],
  system: [153, 101, 21],
  '': [114, 160, 193],
  default: [114, 160, 193],
};

function tagChipStyle(ns: string): CSSProperties {
  const [r, g, b] = NS_COLORS[ns.toLowerCase()] ?? NS_COLORS.default;
  return {
    background: `rgba(${r}, ${g}, ${b}, 0.12)`,
    border: `1px solid rgba(${r}, ${g}, ${b}, 0.25)`,
    color: 'rgba(255, 255, 255, 0.85)',
  };
}

function tagSortKey(ns: string, sub: string): string {
  return `${(NS_ORDER[ns.toLowerCase()] ?? 7).toString().padStart(2, '0')}:${sub.toLowerCase()}`;
}

function formatLabel(value: string): string {
  return value.replace(/_/g, ' ').replace(/\b\w/g, (match) => match.toUpperCase());
}

function formatSmartRuleValue(rule: Record<string, unknown>): string {
  if (Array.isArray(rule.values) && rule.values.length > 0) {
    return rule.values.filter((value): value is string => typeof value === 'string').join(', ');
  }
  if (rule.value != null && typeof rule.value !== 'object') return String(rule.value);
  if (rule.value2 != null && typeof rule.value2 !== 'object') return String(rule.value2);
  return '';
}

function summarizePredicate(predicate: unknown): string[] {
  if (!predicate || typeof predicate !== 'object') return [];
  const groups = (predicate as { groups?: unknown }).groups;
  if (!Array.isArray(groups)) return [];

  const summaries: string[] = [];
  for (const group of groups) {
    if (!group || typeof group !== 'object') continue;
    const rules = (group as { rules?: unknown }).rules;
    if (!Array.isArray(rules)) continue;
    for (const rule of rules) {
      if (!rule || typeof rule !== 'object') continue;
      const typedRule = rule as Record<string, unknown>;
      const field = typeof typedRule.field === 'string' ? typedRule.field : 'rule';
      const op = typeof typedRule.op === 'string' ? typedRule.op : '';
      const value = formatSmartRuleValue(typedRule);
      summaries.push([formatLabel(field), op, value].filter(Boolean).join(' '));
      if (summaries.length >= 8) return summaries;
    }
  }
  return summaries;
}

function ScopePreview({
  items,
}: {
  items: Array<{ thumbnail_hash: string; entity_hash: string }>;
}) {
  // Take first 4 unique thumbnails for the collage
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
              const item = thumbs[i % thumbs.length]; // repeat if < 4 items
              return (
                <div key={i} className={styles.collageCell}>
                  <img
                    src={`media://localhost/thumb/${item.thumbnail_hash}.jpg`}
                    alt=""
                    draggable={false}
                  />
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

function EditableNotesField({
  value,
  disabled = false,
  placeholder = 'Notes',
  onCommit,
}: {
  value: string | null;
  disabled?: boolean;
  placeholder?: string;
  onCommit?: (nextValue: string) => Promise<void>;
}) {
  const [draft, setDraft] = useState(value ?? '');

  useEffect(() => {
    setDraft(value ?? '');
  }, [value]);

  const handleBlur = async () => {
    if (disabled || !onCommit) return;
    const nextValue = draft.trim();
    const currentValue = (value ?? '').trim();
    if (nextValue === currentValue) return;
    try {
      await onCommit(nextValue);
    } catch {
      setDraft(value ?? '');
    }
  };

  if (disabled) {
    return (
      <div className={`${styles.readOnlyField} ${styles.notesFieldReadOnly}`}>
        {value || <span className={styles.placeholder}>{placeholder}</span>}
      </div>
    );
  }

  return (
    <textarea
      className={`${styles.readOnlyField} ${styles.notesField}`}
      value={draft}
      placeholder={placeholder}
      rows={3}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={() => {
        void handleBlur();
      }}
    />
  );
}

function EntityInspector() {
  const data = useAtomValue(displayedInspectorEntityDataAtom);
  const loading = useAtomValue(inspectorLoadingAtom);
  const error = useAtomValue(inspectorErrorAtom);

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
        <img
          src={`media://localhost/thumb/${data.entity_hash}.jpg`}
          alt={data.name ?? ''}
          className={styles.previewImage}
          draggable={false}
        />
      </div>

      <div className={styles.fieldStack}>
        <div className={styles.readOnlyField}>
          {data.name || <span className={styles.placeholder}>Name</span>}
        </div>
        <div className={`${styles.readOnlyField} ${styles.notesFieldReadOnly}`}>
          {data.notes || <span className={styles.placeholder}>Notes</span>}
        </div>
        {sourceUrls.length > 0 ? (
          <div className={styles.readOnlyField}>{sourceUrls.map(extractDomain).join(', ')}</div>
        ) : (
          <div className={styles.readOnlyField}>
            <span className={styles.placeholder}>Source</span>
          </div>
        )}
      </div>

      {sortedTags.length > 0 && (
        <InspectorSection title="Tags" count={sortedTags.length}>
          <div className={styles.tagsWrap}>
            {sortedTags.map((tag) => (
              <span
                key={`${tag.namespace}:${tag.subtag}`}
                className={styles.tag}
                style={tagChipStyle(tag.namespace)}
              >
                {tag.namespace !== 'default' && tag.namespace !== '' && (
                  <span className={styles.tagNamespace}>{tag.namespace}:</span>
                )}
                {tag.subtag}
              </span>
            ))}
          </div>
        </InspectorSection>
      )}

      {folders.length > 0 && (
        <InspectorSection title="Folders">
          <div className={styles.foldersWrap}>
            {folders.map((folder) => (
              <span key={folder.folder_id} className={styles.folderChip}>
                <IconFolder size={10} stroke={1.5} />
                {folder.name}
              </span>
            ))}
          </div>
        </InspectorSection>
      )}

      <InspectorSection title="Properties">
        <div className={styles.propsStack}>
          <StarRating value={data.rating ?? 0} />

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
  const ruleSummaries = useMemo(
    () => summarizePredicate(vm?.smartFolder?.predicate),
    [vm?.smartFolder?.predicate],
  );
  if (!vm) return null;

  const notesValue =
    vm.node.kind === 'system'
      ? vm.description
      : vm.folder?.notes ?? vm.smartFolder?.notes ?? null;

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

  return (
    <div className={styles.contentStack}>
      <div className={styles.preview}>
        <ScopePreview items={vm.previewItems} />
      </div>

      <div className={styles.fieldStack}>
        <div className={styles.readOnlyField}>
          {vm.node.name || <span className={styles.placeholder}>Name</span>}
        </div>
        <EditableNotesField
          value={notesValue}
          disabled={vm.node.kind === 'system'}
          onCommit={saveNotes}
        />
      </div>

        <InspectorSection title="Properties">
        <div className={styles.propsStack}>
          <PropertyRow label="Kind" value={formatLabel(vm.node.kind)} />
          <PropertyRow
            label="Size"
            value={vm.totalSizeBytes != null ? formatFileSize(vm.totalSizeBytes) : null}
            mono
          />
          {vm.parentName && <PropertyRow label="Parent" value={vm.parentName} />}
          {vm.searchText && <PropertyRow label="Search" value={vm.searchText} />}
          <PropertyRow
            label="Auto tags"
            value={vm.folder ? String(vm.folder.autoTags.length) : null}
            mono
          />
          <PropertyRow
            label="Watch enabled"
            value={vm.folder ? (vm.folder.watchEnabled ? 'Yes' : 'No') : null}
          />
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
          <PropertyRow label="Items" value={vm.totalCount.toLocaleString()} mono />
        </div>
      </InspectorSection>

      {ruleSummaries.length > 0 && (
        <InspectorSection title="Rules" count={ruleSummaries.length}>
          <div className={styles.propsStack}>
            {ruleSummaries.map((summary, index) => (
              <PropertyRow key={`${index}:${summary}`} label={`Rule ${index + 1}`} value={summary} />
            ))}
          </div>
        </InspectorSection>
      )}
    </div>
  );
}

export function Inspector() {
  const target = useAtomValue(displayedInspectorTargetAtom);

  if (target.kind === 'none') return null;

  return (
    <div className={styles.panel}>
      <div className={styles.scrollContent}>
        {target.kind === 'entity' ? <EntityInspector /> : <ScopeInspector />}
      </div>
    </div>
  );
}
