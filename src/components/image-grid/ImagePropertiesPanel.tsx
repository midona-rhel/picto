import { useState, useRef, useCallback, useEffect } from 'react';
import { Loader } from '@mantine/core';
import {
  IconPhoto,
  IconPlus,
  IconFolder,
  IconAlignLeft,
  IconPin,
  IconPinFilled,
} from '@tabler/icons-react';
import { TagSelectService } from '../tags/tagSelectService';
import { FolderPickerService } from '../../shared/services/folderPickerService';
import { WindowControls } from '../layout/WindowControls';
import { KbdTooltip } from '../../shared/components/KbdTooltip';
import { useNavigationStore } from '../../state/navigationStore';
import { useFilterStore } from '../../state/filterStore';
import { formatFileSize, formatDuration, getFileExtension } from '../../shared/lib/formatters';
import { MasonryImageItem } from './shared';
import { GlassImagePreview } from './GlassImagePreview';
import { NamespaceTagChip } from '../../shared/components/NamespaceTagChip';
import { StarRating } from '../../shared/components/StarRating';
import { InspectorSection } from '../../shared/components/InspectorSection';
import { PropertyRow } from '../../shared/components/PropertyRow';
import { UrlListEditor } from '../../shared/components/UrlListEditor';
import { ColorPalette } from '../../shared/components/ColorPalette';
import { EmptyState } from '../../shared/components/EmptyState';
import type {
  EntityAllMetadata,
  ResolvedTagInfo,
  SelectionQuerySpec,
  SelectionSummary,
} from './metadataPrefetch';
import type { CollectionSummary } from '../../shared/types/api';
import type { FolderMembership } from '../../hooks/useInspectorData';
import styles from './ImagePropertiesPanel.module.css';

const isMac = navigator.platform.includes('Mac');

/** Compact notes field — icon | separator | single-line display, hover shows editable textarea below */
function NotesField({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const [showEditor, setShowEditor] = useState(false);
  const displayRef = useRef<HTMLDivElement>(null);
  const editorTimerRef = useRef<ReturnType<typeof setTimeout>>();
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const openEditor = () => {
    clearTimeout(editorTimerRef.current);
    setShowEditor(true);
  };

  const scheduleClose = () => {
    editorTimerRef.current = setTimeout(() => setShowEditor(false), 200);
  };

  const handleRowMouseEnter = () => {
    clearTimeout(editorTimerRef.current);
    if (value) setShowEditor(true);
  };

  const handleRowMouseLeave = () => {
    scheduleClose();
  };

  const handleEditorMouseEnter = () => {
    clearTimeout(editorTimerRef.current);
  };

  const handleEditorMouseLeave = () => {
    // Only close if textarea is not focused
    if (document.activeElement !== textareaRef.current) {
      scheduleClose();
    }
  };

  const handleBlur = () => {
    scheduleClose();
  };

  return (
    <div className={styles.fieldRow} style={{ position: 'relative' }}>
      <KbdTooltip label="Notes">
        <button
          className={styles.fieldRowIcon}
          onClick={openEditor}
          tabIndex={-1}
        >
          <IconAlignLeft size={14} />
        </button>
      </KbdTooltip>
      <div className={styles.fieldRowSep} />
      <div
        ref={displayRef}
        className={styles.fieldRowContent}
        onClick={!value ? openEditor : undefined}
        onMouseEnter={handleRowMouseEnter}
        onMouseLeave={handleRowMouseLeave}
      >
        {value || <span className={styles.fieldRowPlaceholder}>Notes</span>}
      </div>
      {showEditor && (
        <div
          className={styles.fieldRowPopover}
          onMouseEnter={handleEditorMouseEnter}
          onMouseLeave={handleEditorMouseLeave}
        >
          <textarea
            ref={textareaRef}
            className={styles.fieldRowPopoverTextarea}
            value={value}
            onChange={(e) => onChange(e.target.value)}
            onFocus={() => clearTimeout(editorTimerRef.current)}
            onBlur={handleBlur}
            onKeyDown={(e) => { if (e.key === 'Escape') { e.preventDefault(); setShowEditor(false); } }}
            placeholder="Notes"
          />
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

interface ImagePropertiesPanelProps {
  selectedImages: MasonryImageItem[];
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
}

export function ImagePropertiesPanel({
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
}: ImagePropertiesPanelProps) {
  const [sectionState, setSectionState] = useState<SectionCollapseState>(loadSectionState);
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

  const selectedImage = selectedImages.length === 1 ? selectedImages[0] : null;
  const selectedCollection = selectedImage?.is_collection ? collectionSummary : null;
  const isMulti = selectedImages.length > 1;
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
    onResizeDragChange?.(true);
    startX.current = e.clientX;
    startWidth.current = panelWidth;
    lastDragWidth.current = panelWidth;
    panelRef.current?.classList.add(styles.panelDragging);

    const onMove = (ev: MouseEvent) => {
      if (!isDragging.current) return;
      const delta = startX.current - ev.clientX;
      const next = Math.round(Math.min(PANEL_MAX_WIDTH, Math.max(PANEL_MIN_WIDTH, startWidth.current + delta)));
      lastDragWidth.current = next;
      if (panelRef.current) {
        panelRef.current.style.width = next + 'px';
      }
      document.documentElement.style.setProperty('--inspector-width', next + 'px');
    };
    const onUp = () => {
      if (!isDragging.current) return;
      isDragging.current = false;
      onResizeDragChange?.(false);
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
        if (!added) return;
        onAddToFolders([folderId]).catch((err) => console.error('Failed to add to folder:', err));
      },
    });
  }, [fileFolders, onAddToFolders]);

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

  // Compute multi-selection aggregated values
  const commonRating = isMulti
    ? (selectedImages.every(i => (i.rating ?? 0) === (selectedImages[0].rating ?? 0))
      ? (selectedImages[0].rating ?? 0)
      : 0)
    : 0;
  const totalSize = isMulti ? selectedImages.reduce((sum, i) => sum + i.size, 0) : 0;

  const displayedTotalSize = isVirtualSelectionSummary
    ? (selectionSummary?.stats?.total_size_bytes ?? null)
    : totalSize;
  const virtualSharedRating = selectionSummary?.stats?.rating_stats?.shared ?? null;
  const displayedRating = isVirtualSelectionSummary
    ? (typeof virtualSharedRating === 'number' ? virtualSharedRating : 0)
    : commonRating;

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
            onRemove={editable && selectedImage ? () => handleRemoveFolderMembership(folder.folder_id) : undefined}
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

  const renderProperties = (rating: number) => (
    <InspectorSection
      title="Properties"
      collapsed={sectionState.properties}
      onToggle={() => toggleSection('properties')}
    >
      <div className={styles.propsStack}>
        <StarRating value={rating} onChange={handleRatingClick} />
        {selectedImage && (
          <>
            <PropertyRow label="Dimensions" mono value={`${selectedImage.width ?? '?'} × ${selectedImage.height ?? '?'}`} />
            <PropertyRow label="Size" mono value={formatFileSize(selectedImage.size)} />
            <PropertyRow label="Type" title={selectedImage.mime || undefined} value={getFileExtension(selectedImage.name, selectedImage.mime)} />
            {selectedImage.duration_ms != null && selectedImage.duration_ms > 0 && (
              <PropertyRow label="Duration" mono value={formatDuration(selectedImage.duration_ms)} />
            )}
            <PropertyRow label="Date added" mono value={new Date(selectedImage.imported_at).toLocaleDateString()} />
          </>
        )}
      </div>
    </InspectorSection>
  );

  const renderCollectionProperties = () => {
    const itemCount = selectedCollection?.image_count ?? selectedImage?.collection_item_count ?? 0;
    const totalSize = selectedCollection?.total_size_bytes;
    const mimeSummary = selectedCollection?.mime_breakdown?.length
      ? selectedCollection.mime_breakdown
        .slice(0, 3)
        .map((m) => `${getFileExtension(`x.${m.mime.split('/')[1] ?? 'bin'}`, m.mime)} (${m.count})`)
        .join(', ')
      : '...';

    return (
      <InspectorSection
        title="Properties"
        collapsed={sectionState.properties}
        onToggle={() => toggleSection('properties')}
      >
        <div className={styles.propsStack}>
          <StarRating value={selectedCollection?.rating ?? 0} onChange={handleRatingClick} />
          <PropertyRow label="Items" mono value={itemCount.toLocaleString()} />
          <PropertyRow label="Total size" mono value={typeof totalSize === 'number' ? formatFileSize(totalSize) : '...'} />
          <PropertyRow label="Types" value={mimeSummary} />
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
                <NotesField value={notes} onChange={onUpdateNotes} />
                <UrlListEditor urls={sourceUrls} onChange={handleUrlChange} />
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

              <button className={styles.exportButton}>Export</button>
            </>
          ) : selectedImages.length === 0 ? (
            <div style={{ height: 400, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
              <EmptyState
                icon={IconPhoto}
                description="Select an image to view properties"
              />
            </div>
          ) : selectedImage ? (
            /* Single image view */
            <>
              <GlassImagePreview images={[selectedImage]} />

              {/* Reserve space to prevent layout shift */}
              {!selectedImage.is_collection && (
                <ColorPalette
                  colors={fileMetadata?.file.dominant_colors ?? []}
                  onFindSimilarColor={handleFindSimilarColor}
                  onReanalyzeColors={onReanalyzeColors}
                />
              )}

              <div className={styles.fieldStack}>
                <input
                  className={styles.fieldInput}
                  value={imageName}
                  onChange={(e) => onImageNameChange(e.target.value)}
                  placeholder="Name"
                />
                <NotesField value={notes} onChange={onUpdateNotes} />
                <UrlListEditor urls={sourceUrls} onChange={handleUrlChange} />
              </div>

              {renderTags()}
              {renderFolders(!selectedImage.is_collection)}
              {selectedImage.is_collection
                ? renderCollectionProperties()
                : renderProperties(fileMetadata?.file.rating ?? selectedImage.rating ?? 0)}

              <button className={styles.exportButton}>Export</button>
            </>
          ) : (
            /* Multi-selection view */
            <>
              <GlassImagePreview images={selectedImages} />

              <div className={styles.selectionTitle}>
                {selectedImages.length.toLocaleString()} items selected
              </div>

              <div className={styles.fieldStack}>
                <NotesField value={notes} onChange={onUpdateNotes} />
                <UrlListEditor urls={sourceUrls} onChange={handleUrlChange} />
              </div>

              {renderTags()}
              {renderFolders(true)}

              <InspectorSection
                title="Properties"
                collapsed={sectionState.properties}
                onToggle={() => toggleSection('properties')}
              >
                <div className={styles.propsStack}>
                  <StarRating value={commonRating} onChange={handleRatingClick} />
                  <PropertyRow label="Total size" mono value={formatFileSize(totalSize)} />
                </div>
              </InspectorSection>

              <button className={styles.exportButton}>Export</button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
