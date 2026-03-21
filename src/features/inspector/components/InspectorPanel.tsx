import { useState, useRef, useCallback, useEffect } from 'react';
import { Loader } from '@mantine/core';
import {
  IconPhoto,
  IconPlus,
  IconFolder,
  IconPin,
  IconPinFilled,
  IconSparkles,
  IconPencil,
  IconCheck,
  IconX,
} from '@tabler/icons-react';
import { TagSelectService } from '../../tags/components/tagSelectService';
import { FolderPickerService } from '../../../shared/services/folderPickerService';
import { WindowControls } from '../../layout/components/WindowControls';
import { KbdTooltip } from '../../../shared/components/KbdTooltip';
import { useNavigationStore } from '../../../state/navigationStore';
import { useFilterStore } from '../../../state/filterStore';
import { formatFileSize, formatDuration, formatDateTime, getFileExtension } from '../../../shared/lib/formatters';
import type { MediaItem } from '../../grid/shared';
import { GlassImagePreview } from '../../../shared/components/GlassImagePreview';
import { NamespaceTagChip } from '../../../shared/components/NamespaceTagChip';
import { StarRating } from '../../../shared/components/StarRating';
import { InspectorSection } from '../../../shared/components/InspectorSection';
import { PropertyRow } from '../../../shared/components/PropertyRow';

import { ColorPalette } from '../../../shared/components/ColorPalette';
import { EmptyState } from '../../../shared/components/EmptyState';
import type {
  EntityAllMetadata,
  ResolvedTagInfo,
  SelectionQuerySpec,
  SelectionSummary,
} from '../../grid/metadataPrefetch';
import type { CollectionSummary } from '../../../shared/types/api';
import type { FolderMembership } from '../hooks/useInspectorData';
import { AiTaggerService } from '../../../shared/services/aiTaggerService';
import { api } from '#desktop/api';

import styles from './InspectorPanel.module.css';

const isMac = navigator.platform.includes('Mac');

/** View-first editable text field — single row pill, hover popover for content. */
function EditableTextField({ value, onChange, placeholder, readOnly, multiline }: {
  value: string; onChange: (v: string) => void; placeholder: string; readOnly?: boolean; multiline?: boolean;
}) {
  const [editing, setEditing] = useState(false);
  const [hovering, setHovering] = useState(false);
  const inputRef = useRef<HTMLInputElement | HTMLTextAreaElement>(null);
  const hoverTimer = useRef<ReturnType<typeof setTimeout>>();

  useEffect(() => {
    if (editing && inputRef.current) inputRef.current.focus();
  }, [editing]);

  const handleDone = () => setEditing(false);

  // Only multiline fields get a hover popover (notes)
  const showPopover = multiline && hovering && !editing && !!value;

  const handleMouseEnter = () => {
    if (!multiline || editing) return;
    clearTimeout(hoverTimer.current);
    if (value) setHovering(true);
  };

  const handleMouseLeave = () => {
    hoverTimer.current = setTimeout(() => setHovering(false), 400);
  };

  return (
    <div
      className={editing ? styles.editableFieldExpanded : styles.editableField}
      onMouseEnter={handleMouseEnter}
      onMouseLeave={handleMouseLeave}
    >
      {editing && !readOnly ? (
        <>
          <div style={{ flex: 1, minWidth: 0 }}>
            {multiline ? (
              <textarea
                ref={inputRef as React.RefObject<HTMLTextAreaElement>}
                className={styles.ghostInput}
                value={value}
                onChange={(e) => onChange(e.target.value)}
                onKeyDown={(e) => { if (e.key === 'Escape') handleDone(); }}
                placeholder={placeholder}
                rows={2}
              />
            ) : (
              <input
                ref={inputRef as React.RefObject<HTMLInputElement>}
                className={styles.ghostInput}
                value={value}
                onChange={(e) => onChange(e.target.value)}
                onKeyDown={(e) => { if (e.key === 'Escape') handleDone(); if (e.key === 'Enter') handleDone(); }}
                placeholder={placeholder}
              />
            )}
          </div>
          <button className={styles.editToggleBtn} onClick={handleDone} tabIndex={-1} style={{ alignSelf: 'flex-start', marginTop: 2 }}>
            <IconCheck size={13} />
          </button>
        </>
      ) : (
        <>
          <div className={styles.editableFieldContent}>
            {value || <span className={styles.editableFieldPlaceholder}>{placeholder}</span>}
          </div>
          {!readOnly && (
            <button className={styles.editToggleBtn} onClick={() => setEditing(true)} tabIndex={-1}>
              <IconPencil size={13} />
            </button>
          )}
        </>
      )}
      {showPopover && (
        <div className={styles.fieldHoverPopover}>
          {value}
        </div>
      )}
    </div>
  );
}

function isValidUrl(text: string): boolean {
  try { new URL(text); return true; } catch { return false; }
}

function extractDomain(url: string): string {
  try { return new URL(url).hostname.replace(/^www\./, ''); } catch { return url; }
}

/** View-first URL list — single row pill shows domain summary, hover shows full URLs. */
function EditableUrlList({ urls, onChange, readOnly, fieldId, activePopover, onPopover }: {
  urls: string[]; onChange: (urls: string[]) => void; readOnly?: boolean;
  fieldId: string; activePopover: string | null; onPopover: (id: string | null) => void;
}) {
  const [editing, setEditing] = useState(false);
  const hoverTimer = useRef<ReturnType<typeof setTimeout>>();
  const showPopover = activePopover === fieldId && !editing && urls.length > 0;

  const handleUrlChange = (index: number, value: string) => {
    const next = [...urls];
    next[index] = value;
    onChange(next);
  };

  const handleRemove = (index: number) => {
    onChange(urls.filter((_, i) => i !== index));
  };

  const handleAdd = () => {
    onChange([...urls, '']);
  };

  const handleDone = () => {
    onChange(urls.filter((u) => u.trim()));
    setEditing(false);
  };

  const handleMouseEnter = () => {
    if (editing) return;
    clearTimeout(hoverTimer.current);
    if (urls.length > 0) onPopover(fieldId);
  };

  const handleMouseLeave = () => {
    hoverTimer.current = setTimeout(() => onPopover(null), 400);
  };

  if (editing && !readOnly) {
    return (
      <div className={styles.editableFieldColumn}>
        <div style={{ display: 'flex', alignItems: 'center' }}>
          <span className={styles.editableFieldPlaceholder} style={{ flex: 1, fontSize: 'var(--font-size-xs)' }}>Source URLs</span>
          <button className={styles.editToggleBtn} onClick={handleDone} tabIndex={-1}>
            <IconCheck size={13} />
          </button>
        </div>
        {urls.map((url, idx) => (
          <div key={idx} className={styles.urlEditRow}>
            <input
              className={styles.ghostInput}
              value={url}
              onChange={(e) => handleUrlChange(idx, e.target.value)}
              onKeyDown={(e) => { if (e.key === 'Escape') handleDone(); }}
              placeholder="https://..."
              autoFocus={idx === urls.length - 1 && !url}
            />
            <button className={styles.urlRemoveBtn} onClick={() => handleRemove(idx)} tabIndex={-1}>
              <IconX size={11} />
            </button>
          </div>
        ))}
        <button className={styles.inspectorAddUrlBtn} onClick={handleAdd}>
          <IconPlus size={11} />
          <span>Add URL</span>
        </button>
      </div>
    );
  }

  const domainSummary = urls.length > 0
    ? urls.map(extractDomain).join(', ')
    : '';

  return (
    <div
      className={styles.editableField}
      onMouseEnter={handleMouseEnter}
      onMouseLeave={handleMouseLeave}
    >
      <div className={styles.editableFieldContent}>
        {domainSummary || <span className={styles.editableFieldPlaceholder}>Source</span>}
      </div>
      {!readOnly && (
        <button className={styles.editToggleBtn} onClick={() => {
          onPopover(null);
          if (urls.length === 0) onChange(['']);
          setEditing(true);
        }} tabIndex={-1}>
          <IconPencil size={13} />
        </button>
      )}
      {showPopover && (
        <div className={styles.fieldHoverPopover}>
          {urls.map((url, idx) => {
            const valid = isValidUrl(url);
            return valid ? (
              <span
                key={idx}
                className={styles.hoverPopoverLink}
                onClick={() => api.os.openExternalUrl(url)}
              >
                {url}
              </span>
            ) : (
              <div key={idx} style={{ padding: '2px 0', color: 'var(--color-text-tertiary)' }}>{url}</div>
            );
          })}
        </div>
      )}
    </div>
  );
}

export const PANEL_MIN_WIDTH = 200;
export const PANEL_MAX_WIDTH = 600;
export const PANEL_DEFAULT_WIDTH = 250;

// Hydrus-style namespace sort order: most specific first
const NAMESPACE_ORDER: Record<string, number> = {
  creator: 0,
  studio: 1,
  series: 2,
  character: 3,
  person: 4,
  species: 5,
  meta: 6,
  system: 7,
  '': 8,
};

const SECTION_STORAGE_KEY = 'picto.inspector.sections';

interface SectionCollapseState {
  tags: boolean;
  folders: boolean;
  properties: boolean;
}

function loadSectionState(): SectionCollapseState {
  try {
    const stored = localStorage.getItem(SECTION_STORAGE_KEY);
    if (stored) return JSON.parse(stored);
  } catch { /* ignore */ }
  return { tags: false, folders: false, properties: false };
}

function saveSectionState(state: SectionCollapseState) {
  try {
    localStorage.setItem(SECTION_STORAGE_KEY, JSON.stringify(state));
  } catch { /* ignore */ }
}

function sortTags(tags: ResolvedTagInfo[]): ResolvedTagInfo[] {
  return [...tags].sort((a, b) => {
    const orderA = NAMESPACE_ORDER[a.namespace.toLowerCase()] ?? 7;
    const orderB = NAMESPACE_ORDER[b.namespace.toLowerCase()] ?? 7;
    if (orderA !== orderB) return orderA - orderB;
    return a.subtag.localeCompare(b.subtag);
  });
}

interface InspectorPanelProps {
  selectedImages: MediaItem[];
  selectionSummarySpec?: SelectionQuerySpec | null;
  imageName: string;
  onImageNameChange: (name: string) => void;
  width: number;
  onWidthChange: (width: number) => void;
  onResizeDragChange?: (dragging: boolean) => void;
  titlebarHeight?: number;
  onTitlebarMouseDown?: (e: React.MouseEvent<HTMLDivElement>) => void;
  isPinned?: boolean;
  onTogglePin?: () => void;

  // Data props (from useInspectorData hook)
  fileTags: ResolvedTagInfo[];
  fileMetadata: EntityAllMetadata | null;
  collectionSummary: CollectionSummary | null;
  selectionSummary: SelectionSummary | null;
  fileFolders: FolderMembership[];
  sourceUrls: string[];
  notes: string;

  // Mutation callbacks (from useInspectorData hook)
  onAddTags: (tags: string[]) => Promise<void>;
  onRemoveTags: (tags: string[]) => Promise<void>;
  onUpdateRating: (rating: number) => Promise<void>;
  onUpdateSourceUrls: (urls: string[]) => Promise<void>;
  onUpdateNotes: (text: string) => void;
  onAddToFolders: (folderIds: number[]) => Promise<void>;
  onRemoveFromFolder: (folderId: number) => Promise<void>;
  onReanalyzeColors: () => Promise<void>;
  onExport: () => void;
  refreshMetadata: () => void;
}

export function InspectorPanel({
  selectedImages,
  selectionSummarySpec,
  imageName,
  onImageNameChange,
  width: panelWidth,
  onWidthChange,
  onResizeDragChange,
  titlebarHeight,
  onTitlebarMouseDown,
  isPinned,
  onTogglePin,
  fileTags,
  fileMetadata,
  collectionSummary,
  selectionSummary,
  fileFolders,
  sourceUrls,
  notes,
  onAddTags,
  onRemoveTags,
  onUpdateRating,
  onUpdateSourceUrls,
  onUpdateNotes,
  onAddToFolders,
  onRemoveFromFolder,
  onReanalyzeColors,
  onExport,
  refreshMetadata,
}: InspectorPanelProps) {
  const [sectionState, setSectionState] = useState<SectionCollapseState>(loadSectionState);
  const [activePopover, setActivePopover] = useState<string | null>(null);
  const autoTagBtnRef = useRef<HTMLButtonElement>(null);
  const addTagBtnRef = useRef<HTMLButtonElement>(null);
  const addFolderBtnRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const isDragging = useRef(false);
  const startX = useRef(0);
  const startWidth = useRef(0);
  const lastDragWidth = useRef(panelWidth);

  const navigateToFolder = useNavigationStore((s) => s.navigateToFolder);
  const navigateToFilterTags = useNavigationStore((s) => s.navigateToFilterTags);
  const setColorFilter = useFilterStore((s) => s.setColorFilter);
  const setFilterBarOpen = useFilterStore((s) => s.setFilterBarOpen);

  const rawSelectedImage = selectedImages.length === 1 ? selectedImages[0] : null;
  const rawSelectedCollection = rawSelectedImage?.is_collection ? collectionSummary : null;

  // Detect whether data has loaded for the current selection to avoid flicker.
  // When switching between file ↔ collection, selectedImage changes immediately
  // but fileMetadata/collectionSummary lag behind the async fetch.
  const dataReady = rawSelectedImage
    ? rawSelectedImage.is_collection
      ? !!rawSelectedCollection
      : !!fileMetadata
    : true; // no selection = always "ready" (empty state)

  // Keep showing previous selection until new data arrives
  const prevSnapshotRef = useRef<{
    selectedImage: typeof rawSelectedImage;
    selectedCollection: typeof rawSelectedCollection;
  }>({ selectedImage: null, selectedCollection: null });

  if (dataReady) {
    prevSnapshotRef.current = {
      selectedImage: rawSelectedImage,
      selectedCollection: rawSelectedCollection,
    };
  }

  const selectedImage = dataReady ? rawSelectedImage : prevSnapshotRef.current.selectedImage;
  const selectedCollection = dataReady ? rawSelectedCollection : prevSnapshotRef.current.selectedCollection;

  const isVirtualSelectionSummary = !!selectionSummarySpec;

  const toggleSection = useCallback((key: keyof SectionCollapseState) => {
    setSectionState((prev) => {
      const next = { ...prev, [key]: !prev[key] };
      saveSectionState(next);
      return next;
    });
  }, []);

  const onDragStart = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    isDragging.current = true;
    startX.current = e.clientX;
    startWidth.current = panelWidth;
    lastDragWidth.current = panelWidth;
    panelRef.current?.classList.add(styles.panelDragging);

    let idleTimer: ReturnType<typeof setTimeout> | null = null;
    let froze = false;
    const onMove = (ev: MouseEvent) => {
      if (!isDragging.current) return;
      const delta = startX.current - ev.clientX;
      const next = Math.round(Math.min(PANEL_MAX_WIDTH, Math.max(PANEL_MIN_WIDTH, startWidth.current + delta)));
      lastDragWidth.current = next;
      if (panelRef.current) {
        panelRef.current.style.width = next + 'px';
      }
      document.documentElement.style.setProperty('--inspector-width', next + 'px');
      // Freeze once on first move, unfreeze after 200ms idle
      if (!froze) { froze = true; onResizeDragChange?.(true); }
      if (idleTimer) clearTimeout(idleTimer);
      idleTimer = setTimeout(() => { froze = false; onResizeDragChange?.(false); }, 200);
    };
    const onUp = () => {
      if (!isDragging.current) return;
      isDragging.current = false;
      if (idleTimer) clearTimeout(idleTimer);
      if (froze) { froze = false; onResizeDragChange?.(false); }
      panelRef.current?.classList.remove(styles.panelDragging);
      document.removeEventListener('mousemove', onMove);
      document.removeEventListener('mouseup', onUp);
      window.removeEventListener('mouseup', onUp);
      window.removeEventListener('blur', onUp);
      onWidthChange(lastDragWidth.current);
    };
    document.addEventListener('mousemove', onMove);
    document.addEventListener('mouseup', onUp);
    window.addEventListener('mouseup', onUp);
    window.addEventListener('blur', onUp);
  }, [panelWidth, onWidthChange, onResizeDragChange]);

  const handleRemoveTag = useCallback((tag: ResolvedTagInfo) => {
    onRemoveTags([tag.raw_tag]).catch((err) => console.error('Failed to remove tag:', err));
  }, [onRemoveTags]);

  const handleOpenTagPicker = useCallback(() => {
    if (!addTagBtnRef.current) return;
    TagSelectService.open({
      anchorEl: addTagBtnRef.current,
      selectedTags: fileTags.map((t) => t.raw_tag),
      onToggle: (tag, added) => {
        if (added) {
          onAddTags([tag]).catch((err) => console.error('Failed to add tag:', err));
        } else {
          onRemoveTags([tag]).catch((err) => console.error('Failed to remove tag:', err));
        }
      },
      onClose: () => {},
    });
  }, [fileTags, onAddTags, onRemoveTags]);

  const handleOpenFolderPicker = useCallback(() => {
    if (!addFolderBtnRef.current) return;
    FolderPickerService.open({
      anchorEl: addFolderBtnRef.current,
      selectedFolderIds: fileFolders.map((f) => f.folder_id),
      onToggle: (folderId, _folderName, added) => {
        if (added) {
          onAddToFolders([folderId]).catch((err) => console.error('Failed to add to folder:', err));
        } else {
          onRemoveFromFolder(folderId).catch((err) => console.error('Failed to remove from folder:', err));
        }
      },
    });
  }, [fileFolders, onAddToFolders]);

  const handleAutoTag = useCallback(async () => {
    if (!autoTagBtnRef.current) return;

    let hashes: string[];

    if (selectionSummarySpec) {
      // Virtual Select All: resolve all hashes from the backend
      hashes = await api.selection.resolveHashes(selectionSummarySpec);
    } else if (selectedImages.length === 1 && selectedImages[0]?.is_collection && selectedImages[0].entity_id != null) {
      // Single collection: tag all member files
      hashes = await api.collections.listMemberHashes(selectedImages[0].entity_id);
    } else {
      hashes = selectedImages.map((i) => i.hash);
    }

    if (hashes.length === 0) return;

    AiTaggerService.open({
      anchorEl: autoTagBtnRef.current,
      hashes,
      onApply: async () => {
        refreshMetadata();
      },
    });
  }, [selectedImages, selectionSummarySpec, refreshMetadata]);

  // Keyboard shortcuts: T = open tag picker, F = open folder picker
  const handleOpenTagPickerRef = useRef(handleOpenTagPicker);
  handleOpenTagPickerRef.current = handleOpenTagPicker;
  const handleOpenFolderPickerRef = useRef(handleOpenFolderPicker);
  handleOpenFolderPickerRef.current = handleOpenFolderPicker;
  const selectedImagesRef = useRef(selectedImages);
  selectedImagesRef.current = selectedImages;

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
      if (e.metaKey || e.ctrlKey || e.altKey || e.shiftKey) return;
      if (selectedImagesRef.current.length === 0) return;

      if (e.key === 't' || e.key === 'T') {
        e.preventDefault();
        handleOpenTagPickerRef.current();
      } else if (e.key === 'f' || e.key === 'F') {
        e.preventDefault();
        handleOpenFolderPickerRef.current();
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, []);

  const handleRatingClick = useCallback((star: number) => {
    onUpdateRating(star).catch((err) => console.error('Failed to update rating:', err));
  }, [onUpdateRating]);

  const handleUrlChange = useCallback((next: string[]) => {
    onUpdateSourceUrls(next).catch((err) => console.error('Failed to set source URLs:', err));
  }, [onUpdateSourceUrls]);

  const handleRemoveFolderMembership = useCallback((folderId: number) => {
    onRemoveFromFolder(folderId).catch((err) => console.error('Failed to remove from folder:', err));
  }, [onRemoveFromFolder]);

  const handleFindSimilarColor = useCallback((hex: string) => {
    setColorFilter(hex.toUpperCase());
    setFilterBarOpen(true);
  }, [setColorFilter, setFilterBarOpen]);

  // Multi-selection values from backend SelectionSummary (used for both multi-file and virtual)
  const displayedTotalSize = selectionSummary?.stats?.total_size_bytes ?? null;
  const sharedRating = selectionSummary?.stats?.rating_stats?.shared ?? null;
  const displayedRating = typeof sharedRating === 'number' ? sharedRating : 0;

  const renderTags = () => (
    <InspectorSection
      title="Tags"
      count={fileTags.length}
      collapsed={sectionState.tags}
      onToggle={() => toggleSection('tags')}
    >
      <div className={styles.tagsWrap}>
        {sortTags(fileTags).map((tag) => (
          <NamespaceTagChip
            key={tag.raw_tag}
            tag={tag.display_tag}
            namespace={tag.namespace}
            onLabelClick={() => navigateToFilterTags([tag.display_tag])}
            onRemove={() => handleRemoveTag(tag)}
          />
        ))}
        <KbdTooltip label="Add Tags" shortcut="T">
          <button
            ref={addTagBtnRef}
            className={styles.addButton}
            onClick={handleOpenTagPicker}
          >
            <IconPlus size={14} />
          </button>
        </KbdTooltip>
      </div>
    </InspectorSection>
  );

  const renderFolders = (editable: boolean) => (
    <InspectorSection
      title="Folders"
      collapsed={sectionState.folders}
      onToggle={() => toggleSection('folders')}
    >
      <div className={styles.foldersWrap}>
        {fileFolders.map((folder) => (
          <NamespaceTagChip
            key={folder.folder_id}
            tag={folder.folder_name}
            icon={<IconFolder size={14} />}
            colorRgb={[134, 142, 150]}
            onLabelClick={() => navigateToFolder({ folder_id: folder.folder_id, name: folder.folder_name })}
            onRemove={editable ? () => handleRemoveFolderMembership(folder.folder_id) : undefined}
          />
        ))}
        {editable && (
          <KbdTooltip label="Add to Folders" shortcut="F">
            <button
              ref={addFolderBtnRef}
              className={styles.addButton}
              onClick={handleOpenFolderPicker}
            >
              <IconPlus size={14} />
            </button>
          </KbdTooltip>
        )}
      </div>
    </InspectorSection>
  );

  const renderProperties = () => {
    const isCollection = !!selectedImage?.is_collection;
    const rating = isCollection
      ? (selectedCollection?.rating ?? 0)
      : (fileMetadata?.entity.rating ?? selectedImage?.rating ?? 0);

    // created_at = content origin date (for collections: oldest member's content date)
    const createdAt = isCollection ? selectedCollection?.date_created : fileMetadata?.entity.date_created;
    const updatedAt = isCollection ? selectedCollection?.date_modified : fileMetadata?.entity.date_modified;

    return (
      <InspectorSection
        title="Properties"
        collapsed={sectionState.properties}
        onToggle={() => toggleSection('properties')}
      >
        <div className={styles.propsStack}>
          <StarRating value={rating} onChange={handleRatingClick} />

          {/* File-specific: dimensions, size, type, duration */}
          {selectedImage && !isCollection && (
            <>
              <PropertyRow label="Dimensions" mono value={`${selectedImage.width ?? '?'} × ${selectedImage.height ?? '?'}`} />
              <PropertyRow label="Size" mono value={formatFileSize(selectedImage.size)} />
              <PropertyRow label="Type" title={selectedImage.mime || undefined} value={getFileExtension(selectedImage.name, selectedImage.mime)} />
              {selectedImage.duration_ms != null && selectedImage.duration_ms > 0 && (
                <PropertyRow label="Duration" mono value={formatDuration(selectedImage.duration_ms)} />
              )}
            </>
          )}

          {/* Collection-specific: items, total size, types */}
          {isCollection && selectedCollection && (() => {
            const itemCount = selectedCollection.image_count ?? selectedImage?.collection_item_count ?? 0;
            const totalSize = selectedCollection.total_size_bytes;
            const mimeSummary = selectedCollection.mime_breakdown?.length
              ? selectedCollection.mime_breakdown
                .slice(0, 3)
                .map((m) => `${getFileExtension(`x.${m.mime.split('/')[1] ?? 'bin'}`, m.mime)} (${m.count})`)
                .join(', ')
              : '...';
            return (
              <>
                <PropertyRow label="Items" mono value={itemCount.toLocaleString()} />
                <PropertyRow label="Total size" mono value={typeof totalSize === 'number' ? formatFileSize(totalSize) : '...'} />
                <PropertyRow label="Types" value={mimeSummary} />
              </>
            );
          })()}

          {/* Shared: dates */}
          {selectedImage && !isCollection && (
            <PropertyRow label="Date added" mono value={formatDateTime(selectedImage.date_added)} />
          )}
          {isCollection && updatedAt && (
            <PropertyRow label="Date added" mono value={formatDateTime(updatedAt)} />
          )}
          {createdAt && (
            <PropertyRow label="Date created" mono value={formatDateTime(createdAt)} />
          )}
          {updatedAt && (
            <PropertyRow label="Date modified" mono value={formatDateTime(updatedAt)} />
          )}
        </div>
      </InspectorSection>
    );
  };

  return (
    <div ref={panelRef} className={styles.panel} style={{ width: panelWidth }}>
      <div className={styles.resizeHandle} onMouseDown={onDragStart} />

      {titlebarHeight != null && titlebarHeight > 0 && (
        <div
          className={styles.titlebarSpacer}
          style={{ height: titlebarHeight }}
          onMouseDown={onTitlebarMouseDown}
        >
          {onTogglePin && (
            <KbdTooltip label={isPinned ? 'Unpin' : 'Pin'}>
              <button
                className={`${styles.pinBtn} ${isPinned ? styles.pinBtnActive : ''}`}
                onClick={onTogglePin}
                aria-label={isPinned ? 'Unpin Inspector' : 'Pin Inspector'}
              >
                {isPinned ? <IconPinFilled size={14} /> : <IconPin size={14} />}
              </button>
            </KbdTooltip>
          )}
          {!isMac && <WindowControls />}
        </div>
      )}

      <div className={styles.scrollContent}>
        <div className={styles.contentStack}>
          {isVirtualSelectionSummary ? (
            <>
              {selectedImages.length > 0 ? (
                <GlassImagePreview images={selectedImages} />
              ) : (
                <div className={styles.loadingCenter}>
                  <Loader size="sm" />
                </div>
              )}

              <div className={styles.selectionTitle}>
                {selectionSummary ? `${selectionSummary.selected_count.toLocaleString()} items selected` : 'Loading selection summary...'}
              </div>

              <div className={styles.fieldStack}>
                <EditableTextField value={notes} onChange={onUpdateNotes} placeholder="Notes" multiline  />
                <EditableUrlList urls={sourceUrls} onChange={handleUrlChange} fieldId="vs-urls" activePopover={activePopover} onPopover={setActivePopover} />
              </div>

              {renderTags()}
              {renderFolders(true)}

              <InspectorSection
                title="Properties"
                collapsed={sectionState.properties}
                onToggle={() => toggleSection('properties')}
              >
                <div className={styles.propsStack}>
                  <StarRating value={displayedRating} onChange={handleRatingClick} />
                  <PropertyRow label="Total size" mono value={displayedTotalSize != null ? formatFileSize(displayedTotalSize) : '...'} />
                </div>
              </InspectorSection>

              <KbdTooltip label="Auto-Tag" shortcut="Mod+Shift+A">
                <button ref={autoTagBtnRef} className={styles.exportButton} onClick={handleAutoTag}>
                  <IconSparkles size={12} style={{ marginRight: 4 }} />Auto-Tag
                </button>
              </KbdTooltip>
              <KbdTooltip label="Export" shortcut="Mod+Shift+E">
                <button className={styles.exportButton} onClick={onExport}>Export</button>
              </KbdTooltip>
            </>
          ) : selectedImages.length === 0 ? (
            <div style={{ height: 400, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
              <EmptyState
                icon={IconPhoto}
                description="Select an item to view properties"
              />
            </div>
          ) : selectedImage ? (
            /* Single image view */
            <>
              <GlassImagePreview images={[selectedImage]} />

              {/* Always reserve space for color palette */}
              <ColorPalette
                colors={(!selectedImage.is_collection ? fileMetadata?.entity.dominant_colors : null) ?? []}
                onFindSimilarColor={!selectedImage.is_collection ? handleFindSimilarColor : undefined}
                onReanalyzeColors={!selectedImage.is_collection ? onReanalyzeColors : undefined}
              />

              <div className={styles.fieldStack}>
                <EditableTextField value={imageName} onChange={onImageNameChange} placeholder="Name" />
                <EditableTextField value={notes} onChange={onUpdateNotes} placeholder="Notes" readOnly={selectedImage.is_collection} multiline />
                <EditableUrlList urls={sourceUrls} onChange={handleUrlChange} readOnly={selectedImage.is_collection} fieldId="urls" activePopover={activePopover} onPopover={setActivePopover} />
              </div>

              {renderTags()}
              {renderFolders(!selectedImage.is_collection)}
              {renderProperties()}

              <KbdTooltip label="Auto-Tag" shortcut="Mod+Shift+A">
                <button ref={autoTagBtnRef} className={styles.exportButton} onClick={handleAutoTag}>
                  <IconSparkles size={12} style={{ marginRight: 4 }} />Auto-Tag
                </button>
              </KbdTooltip>
              <KbdTooltip label="Export" shortcut="Mod+Shift+E">
                <button className={styles.exportButton} onClick={onExport}>Export</button>
              </KbdTooltip>
            </>
          ) : (
            /* Multi-selection view */
            <>
              <GlassImagePreview images={selectedImages} />

              <div className={styles.selectionTitle}>
                {selectedImages.length.toLocaleString()} items selected
              </div>

              <div className={styles.fieldStack}>
                <EditableTextField value={notes} onChange={onUpdateNotes} placeholder="Notes" multiline />
                <EditableUrlList urls={sourceUrls} onChange={handleUrlChange} fieldId="ms-urls" activePopover={activePopover} onPopover={setActivePopover} />
              </div>

              {renderTags()}
              {renderFolders(true)}

              <InspectorSection
                title="Properties"
                collapsed={sectionState.properties}
                onToggle={() => toggleSection('properties')}
              >
                <div className={styles.propsStack}>
                  <StarRating value={displayedRating} onChange={handleRatingClick} />
                  <PropertyRow label="Total size" mono value={displayedTotalSize != null ? formatFileSize(displayedTotalSize) : '—'} />
                </div>
              </InspectorSection>

              <KbdTooltip label="Auto-Tag" shortcut="Mod+Shift+A">
                <button ref={autoTagBtnRef} className={styles.exportButton} onClick={handleAutoTag}>
                  <IconSparkles size={12} style={{ marginRight: 4 }} />Auto-Tag
                </button>
              </KbdTooltip>
              <KbdTooltip label="Export" shortcut="Mod+Shift+E">
                <button className={styles.exportButton} onClick={onExport}>Export</button>
              </KbdTooltip>
            </>
          )}
        </div>
      </div>

    </div>
  );
}
