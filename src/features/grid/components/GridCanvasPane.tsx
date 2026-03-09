import type { MutableRefObject, PointerEventHandler, RefObject } from 'react';
import { CanvasGrid } from '../CanvasGrid';
import { SubfolderGrid } from '../SubfolderGrid';
import { GridInlineRenameOverlay } from './GridInlineRenameOverlay';
import type { MasonryImageItem, MediaItem } from '../shared';
import type { LayoutItem } from '../layoutMath';
import type { GridEmptyContext, GridViewMode } from '../runtime';

export function GridCanvasPane(props: {
  scrollRef: RefObject<HTMLDivElement | null>;
  handleContextMenu: React.MouseEventHandler<HTMLDivElement>;
  handleBoxPointerDown: PointerEventHandler<HTMLDivElement>;
  gridFreezeActive: boolean;
  grayscalePreview: boolean;
  displayFolderId: number | null;
  showSubfolders: boolean;
  hasVisibleSubfolders: boolean;
  displayTargetSize: number;
  totalImageCount: number;
  onOpenFolder: (id: number, name: string) => void;
  selectedSubfolderId: number | null;
  onSelectedSubfolderChange: (id: number | null) => void;
  images: MasonryImageItem[];
  selectedHashes: Set<string>;
  searchTags?: string[];
  gap: number;
  viewMode: GridViewMode;
  onImageClick: (image: MasonryImageItem, event: React.MouseEvent) => void;
  onImport: () => void;
  onImportFolder?: () => void;
  onContainerWidthChange: (width: number) => void;
  showEmptyState: boolean;
  emptyContext: GridEmptyContext;
  popHash: string | null;
  onPopComplete: () => void;
  marqueeActive: boolean;
  showTileName: boolean;
  showResolution: boolean;
  showExtension: boolean;
  showExtensionLabel: boolean;
  thumbnailFitMode: 'cover' | 'contain';
  marqueeRectRef: RefObject<{ left: number; top: number; width: number; height: number } | null>;
  marqueeHitHashesRef: RefObject<Set<string> | null>;
  scheduleRedrawRef: MutableRefObject<(() => void) | null>;
  onLayoutChange: (positions: LayoutItem[]) => void;
  reorderMode: boolean;
  onReorder?: (movedHashes: string[], targetIndex: number) => void;
  onLoadMore?: () => void;
  totalCount: number;
  renamingHash: string | null;
  renameInputRef: RefObject<HTMLInputElement>;
  renameValue: string;
  setRenameValue: (value: string) => void;
  commitRename: () => void;
  cancelRename: () => void;
  positions: LayoutItem[];
  renameImages: MediaItem[];
}) {
  const {
    scrollRef,
    handleContextMenu,
    handleBoxPointerDown,
    gridFreezeActive,
    grayscalePreview,
    displayFolderId,
    showSubfolders,
    hasVisibleSubfolders,
    displayTargetSize,
    totalImageCount,
    onOpenFolder,
    selectedSubfolderId,
    onSelectedSubfolderChange,
    images,
    selectedHashes,
    searchTags,
    gap,
    viewMode,
    onImageClick,
    onImport,
    onImportFolder,
    onContainerWidthChange,
    showEmptyState,
    emptyContext,
    popHash,
    onPopComplete,
    marqueeActive,
    showTileName,
    showResolution,
    showExtension,
    showExtensionLabel,
    thumbnailFitMode,
    marqueeRectRef,
    marqueeHitHashesRef,
    scheduleRedrawRef,
    onLayoutChange,
    reorderMode,
    onReorder,
    onLoadMore,
    totalCount,
    renamingHash,
    renameInputRef,
    renameValue,
    setRenameValue,
    commitRename,
    cancelRename,
    positions,
    renameImages,
  } = props;

  return (
      <div
      ref={scrollRef as RefObject<HTMLDivElement>}
      data-grid-container
      onContextMenu={handleContextMenu}
      onPointerDown={handleBoxPointerDown}
      style={{
        flex: 1,
        overflowY: 'auto',
        scrollbarGutter: 'stable both-edges',
        overflowX: 'hidden',
        userSelect: 'none',
        WebkitUserSelect: 'none',
        position: 'relative',
        pointerEvents: gridFreezeActive ? 'none' : 'auto',
        filter: grayscalePreview ? 'grayscale(1)' : undefined,
      } as React.CSSProperties}
    >
      <div style={{ height: 8 }} />
      <div style={{ position: 'relative' }}>
        {displayFolderId != null && showSubfolders && (
          <SubfolderGrid
            folderId={displayFolderId}
            targetSize={displayTargetSize}
            totalImageCount={totalImageCount}
            onOpenFolder={onOpenFolder}
            selectedSubfolderId={selectedSubfolderId}
            paused={gridFreezeActive}
            onSelectedSubfolderChange={onSelectedSubfolderChange}
          />
        )}
        <CanvasGrid
          images={images}
          targetSize={displayTargetSize}
          gap={gap}
          viewMode={viewMode}
          selectedHashes={selectedHashes}
          searchTags={searchTags}
          onImageClick={onImageClick}
          onImport={onImport}
          onImportFolder={onImportFolder}
          onContainerWidthChange={onContainerWidthChange}
          showEmptyState={showEmptyState && !hasVisibleSubfolders}
          emptyContext={emptyContext}
          scrollContainerRef={scrollRef}
          popHash={popHash}
          onPopComplete={onPopComplete}
          frozen={gridFreezeActive}
          marqueeActive={marqueeActive}
          showTileName={showTileName}
          showResolution={showResolution}
          showExtension={showExtension}
          showExtensionLabel={showExtensionLabel}
          thumbnailFitMode={thumbnailFitMode}
          marqueeRectRef={marqueeRectRef}
          marqueeHitHashesRef={marqueeHitHashesRef}
          scheduleRedrawRef={scheduleRedrawRef}
          onLayoutChange={onLayoutChange}
          reorderMode={reorderMode}
          onReorder={onReorder}
          onLoadMore={onLoadMore}
          totalCount={totalCount}
          renamingHash={renamingHash}
        />
        {renamingHash && (
          <GridInlineRenameOverlay
            renamingHash={renamingHash}
            positions={positions}
            images={renameImages}
            showTileName={showTileName}
            showResolution={showResolution}
            scrollRoot={scrollRef.current}
            renameInputRef={renameInputRef}
            renameValue={renameValue}
            setRenameValue={setRenameValue}
            commitRename={commitRename}
            cancelRename={cancelRename}
          />
        )}
      </div>
    </div>
  );
}
