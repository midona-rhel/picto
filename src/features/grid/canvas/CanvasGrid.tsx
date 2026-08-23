import { useCallback, useEffect, useLayoutEffect, useMemo, useRef } from 'react';
import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';
import type { GridViewMode, LayoutResult } from '../layout/types';
import { HoverPreviewPortal } from './HoverPreviewPortal';
import { estimateGridScrollHeight, GridLayoutRuntime } from './gridLayoutModel';
import { DragGhost } from '../DragGhost';
import { resolveGridScrollAnchor } from './gridScrollAnchor';
import { useCanvasViewport } from './useCanvasViewport';
import { useCanvasRenderer } from './useCanvasRenderer';
import { useCanvasInteractionRefs, useCanvasInteractions } from './useCanvasInteractions';
import { GridRenameOverlay } from './GridRenameOverlay';
import { GRID_GAP } from '../gridAppearance';
import styles from './CanvasGrid.module.css';

const TEXT_NAME_ROW_H = 20;
const EMPTY_HASH_SET = new Set<string>();

function useLatest<T>(value: T) {
  const ref = useRef(value);
  ref.current = value;
  return ref;
}

export interface CanvasGridProps {
  items: CanonicalEntityGridItem[];
  viewMode: GridViewMode;
  targetSize: number;
  showName: boolean;
  showExtension: boolean;
  showExtensionLabel?: boolean;
  showResolution?: boolean;
  fitThumbnails?: boolean;
  totalCount?: number | null;
  onTileClick?: (index: number, item: CanonicalEntityGridItem, event?: React.MouseEvent) => void;
  onTileDoubleClick?: (index: number, item: CanonicalEntityGridItem) => void;
  onEmptyClick?: () => void;
  onTileContextMenu?: (index: number, item: CanonicalEntityGridItem, position: { x: number; y: number }) => void;
  onEmptyContextMenu?: (position: { x: number; y: number }) => void;
  onLoadMore?: () => void;
  onFirstPaint?: () => void;
  onScrollTopChange?: (scrollTop: number) => void;
  interactive?: boolean;
  suppressTileReveal?: boolean;
  initialScrollTop?: number | null;
  selectedEntityHashes?: Set<string>;
  selectedFolderNodeIds?: Set<string>;
  onSelectionChange?: (hashes: Set<string>) => void;
  onMarqueeSelectionChange?: (selection: { entityHashes: Set<string>; folderNodeIds: Set<string> }) => void;
  collectHeaderMarqueeHits?: (rect: { left: number; top: number; width: number; height: number }) => Set<string>;
  headerContent?: React.ReactNode;
  dragSourceScope?: { kind: string; id?: number | null; key?: string | null } | null;
  onContainerRef?: (el: HTMLDivElement | null) => void;
  onLayoutChange?: (layout: LayoutResult) => void;
  renamingIndex?: number | null;
  onRenameCommit?: (index: number, newName: string) => void;
  onRenameCancel?: () => void;
}

export function CanvasGrid({
  items,
  viewMode,
  targetSize,
  showName,
  showExtension,
  showExtensionLabel = false,
  showResolution = false,
  fitThumbnails = false,
  totalCount = null,
  onTileClick,
  onTileDoubleClick,
  onEmptyClick,
  onTileContextMenu,
  onEmptyContextMenu,
  onSelectionChange,
  onLoadMore,
  onFirstPaint,
  onScrollTopChange,
  interactive = true,
  suppressTileReveal = false,
  initialScrollTop = null,
  selectedEntityHashes = EMPTY_HASH_SET,
  selectedFolderNodeIds = EMPTY_HASH_SET,
  headerContent,
  dragSourceScope = null,
  onContainerRef,
  onLayoutChange,
  renamingIndex = null,
  onRenameCommit,
  onRenameCancel,
  onMarqueeSelectionChange,
  collectHeaderMarqueeHits,
}: CanvasGridProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const containerCallbackRef = useCallback((el: HTMLDivElement | null) => {
    (containerRef as React.MutableRefObject<HTMLDivElement | null>).current = el;
    onContainerRef?.(el);
  }, [onContainerRef]);
  const headerRef = useRef<HTMLDivElement>(null);
  const baseCanvasRef = useRef<HTMLCanvasElement>(null);
  const overlayCanvasRef = useRef<HTMLCanvasElement>(null);
  const viewportLayerRef = useRef<HTMLDivElement>(null);
  const lastScrollTopRef = useRef(0);
  const prevLayoutRef = useRef<ReturnType<GridLayoutRuntime['update']> | null>(null);
  const prevItemsRef = useRef(items);
  const interactionRefs = useCanvasInteractionRefs();
  const firstPaintRef = useRef(false);
  const resizeRedrawRef = useRef<() => void>(() => {});
  const viewportRefs = useMemo(() => ({
    container: containerRef,
    viewportLayer: viewportLayerRef,
    baseCanvas: baseCanvasRef,
    overlayCanvas: overlayCanvasRef,
    header: headerRef,
    redraw: resizeRedrawRef,
  }), []);
  const { layoutWidth, headerHeight } = useCanvasViewport(viewportRefs, headerContent);

  const textHeight = (showName ? TEXT_NAME_ROW_H : 0) + (showResolution ? TEXT_NAME_ROW_H : 0);

  const layoutRuntimeRef = useRef(new GridLayoutRuntime());
  const layoutModel = useMemo(() => layoutRuntimeRef.current.update(items, {
    width: layoutWidth.width,
    targetSize,
    gap: GRID_GAP,
    viewMode,
    textHeight,
    scrollbarWidth: layoutWidth.scrollbarWidth,
  }), [items, layoutWidth, targetSize, viewMode, textHeight]);
  const estimatedScrollHeight = useMemo(
    () => estimateGridScrollHeight(layoutModel.totalHeight, items.length, totalCount),
    [items.length, layoutModel.totalHeight, totalCount],
  );

  const onLayoutChangeRef = useLatest(onLayoutChange);
  useEffect(() => { onLayoutChangeRef.current?.(layoutModel); }, [layoutModel]);
  const selectedHashesRef = useLatest(selectedEntityHashes);

  useLayoutEffect(() => {
    const prev = prevLayoutRef.current;
    prevLayoutRef.current = layoutModel;
    const prevItems = prevItemsRef.current;
    prevItemsRef.current = items;

    if (!prev || prev === layoutModel) return;
    if (prev.positions.length === 0 || layoutModel.positions.length === 0) return;

    const container = containerRef.current;
    if (!container) return;
    const vh = container.clientHeight;
    if (vh === 0) return;

    const next = resolveGridScrollAnchor({
      previousPositions: prev.positions,
      nextPositions: layoutModel.positions,
      previousItems: prevItems,
      nextItems: items,
      selectedHashes: selectedHashesRef.current,
      scrollTop: lastScrollTopRef.current,
      viewportHeight: vh,
    });
    if (next == null) return;

    container.scrollTop = next;
    lastScrollTopRef.current = next;
  }, [layoutModel, items]);

  const prevItemsLengthForScrollRef = useRef(0);
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const prevLen = prevItemsLengthForScrollRef.current;
    prevItemsLengthForScrollRef.current = items.length;

    if (initialScrollTop != null && initialScrollTop > 0) {
      firstPaintRef.current = false;
      container.scrollTop = initialScrollTop;
      lastScrollTopRef.current = initialScrollTop;
    } else if (prevLen === 0 || items.length === 0) {
      firstPaintRef.current = false;
      container.scrollTop = 0;
      lastScrollTopRef.current = 0;
    }
  }, [items]); // eslint-disable-line react-hooks/exhaustive-deps -- initialScrollTop read once per items change

  const { markDirty, pipelineRef } = useCanvasRenderer({
    containerRef,
    baseCanvasRef,
    overlayCanvasRef,
    layout: layoutModel,
    sourceItems: items,
    viewMode,
    textHeight,
    headerHeight,
    showName,
    showExtension,
    showExtensionLabel,
    showResolution,
    fitThumbnails,
    suppressTileReveal,
    selectedHashes: selectedEntityHashes,
    ...interactionRefs,
    firstPaintRef,
    onFirstPaint,
  });
  resizeRedrawRef.current = () => markDirty('both');

  const { events, hoverPreview, marqueeVisual, dragGhost } = useCanvasInteractions({
    containerRef,
    headerRef,
    pipelineRef,
    interactionRefs,
    layout: layoutModel,
    items,
    textHeight,
    headerHeight,
    interactive,
    selectedEntityHashes,
    selectedFolderNodeIds,
    dragSourceScope,
    lastScrollTopRef,
    markDirty,
    onTileClick,
    onTileDoubleClick,
    onEmptyClick,
    onTileContextMenu,
    onEmptyContextMenu,
    onSelectionChange,
    onMarqueeSelectionChange,
    collectHeaderMarqueeHits,
    onLoadMore,
    onScrollTopChange,
  });

  return (
    <div className={styles.root}>
      <div
        ref={containerCallbackRef}
        className={`${styles.container} ${interactive ? '' : styles.containerFrozen}`}
        {...events}
      >
        {headerContent && <div ref={headerRef}>{headerContent}</div>}
        {marqueeVisual && (
          <div className={styles.marquee} style={{
            left: marqueeVisual.left,
            top: marqueeVisual.top,
            width: marqueeVisual.width,
            height: marqueeVisual.height,
          }} />
        )}
        {renamingIndex != null && (() => {
          const pos = layoutModel.positions[renamingIndex];
          const item = items[renamingIndex];
          if (!pos || !item) return null;
          return (
            <GridRenameOverlay
              key={`rename-${renamingIndex}`}
              index={renamingIndex}
              item={item}
              position={pos}
              textHeight={textHeight}
              headerHeight={headerHeight}
              onCommit={onRenameCommit}
              onCancel={onRenameCancel}
            />
          );
        })()}
        <div
          className={styles.canvasWrap}
          data-grid-layout
          style={{ height: `${estimatedScrollHeight}px` }}
        >
          <div
            ref={viewportLayerRef}
            className={styles.viewportLayer}
          >
            <canvas
              ref={baseCanvasRef}
              className={styles.baseCanvas}
            />
            <canvas
              ref={overlayCanvasRef}
              className={styles.overlayCanvas}
            />
          </div>
        </div>
      </div>
      {hoverPreview && <HoverPreviewPortal hash={hoverPreview.hash} mime={hoverPreview.mime} />}
      {dragGhost && (
        <DragGhost
          x={dragGhost.x}
          y={dragGhost.y}
          thumbnailHashes={dragGhost.thumbnailHashes}
          count={dragGhost.count}
        />
      )}
    </div>
  );
}
