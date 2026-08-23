import { useCallback, useEffect, useMemo, useRef, useState, type MutableRefObject, type RefObject } from 'react';
import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';
import { cancelDrag, endDrag, getDragState, isDragActive, moveDrag, setDropTarget, setInternalDragOrigin, startDrag, startNativeDrag } from '../dragState';
import { createNativeDragImageUrl } from '../dragGhostSpec';
import type { GridLayoutModel } from './gridLayoutModel';
import { planFolderReorder } from './folderReorder';
import { computeReorderTarget, hitTestTile } from './hitTesting';
import { CANVAS_SCROLL_IDLE_DELAY_MS, classifyCanvasScrollPhase } from './scrollState';
import type { ThumbnailPipeline } from './thumbnailPipeline';
import type { DirtyLanes, ReorderDrop } from './useCanvasRenderer';
import { zoomButtonRect } from './useCanvasRenderer';

const HOVER_PREVIEW_DELAY_MS = 200;
const HOVER_HIDE_DELAY_MS = 90;

function useLatest<T>(value: T) {
  const ref = useRef(value);
  ref.current = value;
  return ref;
}

export function useCanvasInteractionRefs() {
  const hoveredTileRef = useRef<number | null>(null);
  const isScrollingRef = useRef(false);
  const reorderDropRef = useRef<ReorderDrop | null>(null);
  const overlayBlankRef = useRef(true);
  return useMemo(() => ({ hoveredTileRef, isScrollingRef, reorderDropRef, overlayBlankRef }), []);
}

interface InteractionOptions {
  containerRef: RefObject<HTMLDivElement>;
  headerRef: RefObject<HTMLDivElement>;
  pipelineRef: RefObject<ThumbnailPipeline | null>;
  interactionRefs: ReturnType<typeof useCanvasInteractionRefs>;
  layout: GridLayoutModel;
  items: CanonicalEntityGridItem[];
  textHeight: number;
  headerHeight: number;
  interactive: boolean;
  selectedEntityHashes: Set<string>;
  selectedFolderNodeIds: Set<string>;
  dragSourceScope: { kind: string; id?: number | null; key?: string | null } | null;
  lastScrollTopRef: MutableRefObject<number>;
  markDirty: (lanes: DirtyLanes) => void;
  onTileClick?: (index: number, item: CanonicalEntityGridItem, event?: React.MouseEvent) => void;
  onTileDoubleClick?: (index: number, item: CanonicalEntityGridItem) => void;
  onEmptyClick?: () => void;
  onTileContextMenu?: (index: number, item: CanonicalEntityGridItem, position: { x: number; y: number }) => void;
  onEmptyContextMenu?: (position: { x: number; y: number }) => void;
  onSelectionChange?: (hashes: Set<string>) => void;
  onMarqueeSelectionChange?: (selection: { entityHashes: Set<string>; folderNodeIds: Set<string> }) => void;
  collectHeaderMarqueeHits?: (rect: { left: number; top: number; width: number; height: number }) => Set<string>;
  onLoadMore?: () => void;
  onScrollTopChange?: (scrollTop: number) => void;
}

function layoutPoint(clientX: number, clientY: number, container: HTMLDivElement, headerHeight: number) {
  const rect = container.getBoundingClientRect();
  const zoomX = rect.width / (container.offsetWidth || 1);
  const zoomY = rect.height / (container.offsetHeight || 1);
  return {
    x: (clientX - rect.left) / zoomX,
    y: (clientY - rect.top) / zoomY + container.scrollTop - headerHeight,
  };
}

/** Owns hit testing, hover, marquee, drag/reorder, and scroll interaction. */
export function useCanvasInteractions(options: InteractionOptions) {
  const {
    containerRef, headerRef, pipelineRef, interactionRefs, layout, items,
    textHeight, headerHeight, interactive, selectedEntityHashes,
    selectedFolderNodeIds, dragSourceScope, lastScrollTopRef, markDirty,
    onTileClick, onTileDoubleClick, onEmptyClick, onTileContextMenu,
    onEmptyContextMenu, onSelectionChange, onMarqueeSelectionChange,
    collectHeaderMarqueeHits, onLoadMore, onScrollTopChange,
  } = options;
  const { hoveredTileRef, isScrollingRef, reorderDropRef, overlayBlankRef } = interactionRefs;
  const [hoverPreview, setHoverPreview] = useState<{ hash: string; mime: string } | null>(null);
  const [marqueeVisual, setMarqueeVisual] = useState<{ left: number; top: number; width: number; height: number } | null>(null);
  const [dragGhost, setDragGhost] = useState<{ x: number; y: number; count: number; thumbnailHashes: string[] } | null>(null);
  const latest = {
    items: useLatest(items), layout: useLatest(layout), textHeight: useLatest(textHeight),
    headerHeight: useLatest(headerHeight), markDirty: useLatest(markDirty),
    selectionChanged: useLatest(onSelectionChange), loadMore: useLatest(onLoadMore),
    scrollChanged: useLatest(onScrollTopChange), selected: useLatest(selectedEntityHashes),
  };
  const lastScrollTimeRef = useRef(0);
  const idleTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const hoverTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const hoverHideTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const dragJustEndedRef = useRef(false);
  const tileDragRef = useRef<{ tileIdx: number; startClientX: number; startClientY: number } | null>(null);
  const marqueeRef = useRef({ startX: 0, startY: 0, active: false, lastClientX: 0, lastClientY: 0 });
  const marqueeRectRef = useRef<{ left: number; top: number; width: number; height: number } | null>(null);
  const marqueeBaseEntities = useRef(new Set<string>());
  const marqueeBaseFolders = useRef(new Set<string>());
  const candidateBuffer = useRef<number[]>([]);
  const autoScrollRef = useRef<number | null>(null);
  const autoScrollSpeedRef = useRef(0);

  const isInHeader = useCallback((target: EventTarget) => headerRef.current?.contains(target as Node) ?? false, [headerRef]);
  const isHeaderControl = useCallback((target: EventTarget) => {
    const element = target as HTMLElement;
    return isInHeader(target) && (!!element.closest('[data-grid-header-interactive]') || !!element.closest('button'));
  }, [isInHeader]);
  const hit = useCallback((clientX: number, clientY: number) => {
    const container = containerRef.current;
    if (!container) return null;
    const point = layoutPoint(clientX, clientY, container, headerHeight);
    const index = hitTestTile(layout.positions, point.x, point.y, textHeight, 0, layout.positions.length);
    return { ...point, index };
  }, [containerRef, headerHeight, layout.positions, textHeight]);

  useEffect(() => {
    const onMove = (event: MouseEvent) => {
      if (!isDragActive()) return;
      if (event.clientX <= 0 || event.clientY <= 0 || event.clientX >= window.innerWidth || event.clientY >= window.innerHeight) {
        const state = getDragState();
        reorderDropRef.current = null;
        setDragGhost(null);
        const icon = createNativeDragImageUrl(
          state.hashes.slice(0, 3), state.hashes.length,
          (hash) => pipelineRef.current?.get(hash)?.thumb ?? null,
        );
        setInternalDragOrigin(true);
        startNativeDrag(state.hashes, icon);
        dragJustEndedRef.current = true;
        latest.markDirty.current('overlay');
        return;
      }
      moveDrag(event.clientX, event.clientY);
      setDragGhost((current) => current ? { ...current, x: event.clientX, y: event.clientY } : null);
      if (getDragState().sourceScope?.kind !== 'folder') return;
      const container = containerRef.current;
      if (!container) return;
      const model = latest.layout.current;
      const point = layoutPoint(event.clientX, event.clientY, container, latest.headerHeight.current);
      const excluded = new Set<number>();
      for (const hash of getDragState().hashes) {
        const index = model.hashToIndex.get(hash);
        if (index != null) excluded.add(index);
      }
      const target = computeReorderTarget(model.positions, point.x, point.y, latest.textHeight.current, excluded);
      reorderDropRef.current = target ? { dropIndex: target.index, dropSide: target.side } : null;
      latest.markDirty.current('overlay');
    };
    const onUp = (event: MouseEvent) => {
      if (isDragActive() && !(event.clientX <= 0 || event.clientY <= 0 || event.clientX >= window.innerWidth || event.clientY >= window.innerHeight)) {
        const reorder = reorderDropRef.current;
        if (reorder && !getDragState().dropTarget) {
          const moves = planFolderReorder(
            latest.items.current.map((item) => item.entity_hash),
            new Set(getDragState().hashes), reorder.dropIndex, reorder.dropSide,
          );
          if (moves.length) setDropTarget({ kind: 'reorder', moves });
        }
        reorderDropRef.current = null;
        const hashes = new Set(getDragState().hashes);
        endDrag();
        dragJustEndedRef.current = true;
        latest.selectionChanged.current?.(hashes);
        latest.markDirty.current('overlay');
      }
      setDragGhost(null);
    };
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
    return () => {
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
      reorderDropRef.current = null;
      if (isDragActive()) cancelDrag();
    };
  }, []);

  const handleScroll = useCallback(() => {
    const container = containerRef.current;
    if (!container) return;
    const now = performance.now();
    const scrollTop = container.scrollTop;
    const delta = scrollTop - lastScrollTopRef.current;
    const elapsed = now - lastScrollTimeRef.current;
    lastScrollTopRef.current = scrollTop;
    lastScrollTimeRef.current = now;
    latest.scrollChanged.current?.(scrollTop);
    if (!interactive) {
      markDirty('both');
      return;
    }
    const phase = classifyCanvasScrollPhase(elapsed > 0 ? Math.abs(delta) / elapsed * 1000 : 0);
    isScrollingRef.current = phase !== 'idle';
    hoveredTileRef.current = null;
    if (hoverTimerRef.current) clearTimeout(hoverTimerRef.current);
    if (hoverHideTimerRef.current) clearTimeout(hoverHideTimerRef.current);
    hoverTimerRef.current = hoverHideTimerRef.current = null;
    setHoverPreview(null);
    const overlay = selectedEntityHashes.size > 0 || reorderDropRef.current != null || !overlayBlankRef.current;
    markDirty(overlay ? 'both' : 'base');
    if (idleTimerRef.current) clearTimeout(idleTimerRef.current);
    idleTimerRef.current = setTimeout(() => { isScrollingRef.current = false; markDirty('base'); }, CANVAS_SCROLL_IDLE_DELAY_MS);
    const loadedEnd = headerHeight + layout.totalHeight;
    if (loadedEnd - scrollTop - container.clientHeight < container.clientHeight * 3) onLoadMore?.();
  }, [containerRef, headerHeight, interactive, isScrollingRef, lastScrollTopRef, layout.totalHeight, markDirty, onLoadMore, overlayBlankRef, reorderDropRef, selectedEntityHashes.size]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container || !onLoadMore) return;
    if (headerHeight + layout.totalHeight - container.scrollTop - container.clientHeight < container.clientHeight * 3) onLoadMore();
  }, [containerRef, headerHeight, layout.totalHeight, onLoadMore]);

  const handleClick = useCallback((event: React.MouseEvent) => {
    if (dragJustEndedRef.current) { dragJustEndedRef.current = false; return; }
    if (isHeaderControl(event.target)) return;
    const result = hit(event.clientX, event.clientY);
    if (result?.index != null && items[result.index]) onTileClick?.(result.index, items[result.index], event);
    else onEmptyClick?.();
  }, [hit, isHeaderControl, items, onEmptyClick, onTileClick]);
  const handleDoubleClick = useCallback((event: React.MouseEvent) => {
    if (isHeaderControl(event.target)) return;
    const result = hit(event.clientX, event.clientY);
    if (result?.index != null && items[result.index]) onTileDoubleClick?.(result.index, items[result.index]);
  }, [hit, isHeaderControl, items, onTileDoubleClick]);
  const handleContextMenu = useCallback((event: React.MouseEvent) => {
    if (isInHeader(event.target)) return;
    event.preventDefault();
    const result = hit(event.clientX, event.clientY);
    const position = { x: event.clientX, y: event.clientY };
    if (result?.index != null && items[result.index]) onTileContextMenu?.(result.index, items[result.index], position);
    else onEmptyContextMenu?.(position);
  }, [hit, isInHeader, items, onEmptyContextMenu, onTileContextMenu]);

  const clearHoverTimers = useCallback(() => {
    if (hoverTimerRef.current) clearTimeout(hoverTimerRef.current);
    if (hoverHideTimerRef.current) clearTimeout(hoverHideTimerRef.current);
    hoverTimerRef.current = hoverHideTimerRef.current = null;
  }, []);
  const handleMouseMove = useCallback((event: React.MouseEvent) => {
    if (marqueeRef.current.active) return;
    const result = hit(event.clientX, event.clientY);
    const index = result?.index ?? null;
    if (index !== hoveredTileRef.current) { hoveredTileRef.current = index; markDirty('overlay'); }
    const position = index == null ? null : layout.positions[index];
    const item = index == null ? null : items[index];
    const onZoom = result && position
      ? (() => { const rect = zoomButtonRect(position, position.h - textHeight); return result.x >= rect.x && result.x < rect.x + rect.width && result.y >= rect.y && result.y < rect.y + rect.height; })()
      : false;
    if (onZoom && item && !item.mime_type.startsWith('video/')) {
      if (hoverHideTimerRef.current) clearTimeout(hoverHideTimerRef.current);
      hoverHideTimerRef.current = null;
      if (!hoverTimerRef.current) hoverTimerRef.current = setTimeout(() => {
        hoverTimerRef.current = null;
        setHoverPreview({ hash: item.entity_hash, mime: item.mime_type });
      }, HOVER_PREVIEW_DELAY_MS);
    } else {
      if (hoverTimerRef.current) clearTimeout(hoverTimerRef.current);
      hoverTimerRef.current = null;
      if (hoverPreview && !hoverHideTimerRef.current) hoverHideTimerRef.current = setTimeout(() => {
        hoverHideTimerRef.current = null;
        setHoverPreview(null);
      }, HOVER_HIDE_DELAY_MS);
    }
  }, [hit, hoverPreview, hoveredTileRef, items, layout.positions, markDirty, textHeight]);
  const handleMouseLeave = useCallback(() => {
    if (isDragActive()) return;
    if (hoveredTileRef.current != null) { hoveredTileRef.current = null; markDirty('overlay'); }
    clearHoverTimers();
    if (hoverPreview) hoverHideTimerRef.current = setTimeout(() => setHoverPreview(null), HOVER_HIDE_DELAY_MS);
  }, [clearHoverTimers, hoverPreview, hoveredTileRef, markDirty]);

  const collectMarqueeHits = useCallback((left: number, top: number, width: number, height: number) => {
    const entityHashes = new Set(marqueeBaseEntities.current);
    const folderNodeIds = new Set(marqueeBaseFolders.current);
    const candidates = candidateBuffer.current;
    candidates.length = 0;
    layout.spatialIndex.queryYRange(top, top + height, candidates);
    for (const index of candidates) {
      const position = layout.positions[index];
      const item = items[index];
      if (position && item && position.x + position.w > left && position.x < left + width
          && position.y + position.h - textHeight > top && position.y < top + height) entityHashes.add(item.entity_hash);
    }
    for (const id of collectHeaderMarqueeHits?.({ left, top, width, height }) ?? []) folderNodeIds.add(id);
    return { entityHashes, folderNodeIds };
  }, [collectHeaderMarqueeHits, items, layout, textHeight]);

  const updateMarquee = useCallback((left: number, top: number, width: number, height: number) => {
    if (width < 5 && height < 5) return;
    marqueeRectRef.current = { left, top, width, height };
    setMarqueeVisual({ left, top: top + headerHeight, width, height });
    onMarqueeSelectionChange?.(collectMarqueeHits(left, top, width, height));
  }, [collectMarqueeHits, headerHeight, onMarqueeSelectionChange]);
  const handlePointerDown = useCallback((event: React.PointerEvent) => {
    if (event.button !== 0 || isHeaderControl(event.target)) return;
    const container = containerRef.current;
    const result = hit(event.clientX, event.clientY);
    if (!container || !result) return;
    if (result.index != null) {
      tileDragRef.current = { tileIdx: result.index, startClientX: event.clientX, startClientY: event.clientY };
      return;
    }
    tileDragRef.current = null;
    const rect = container.getBoundingClientRect();
    const zoomY = rect.height / (container.offsetHeight || 1);
    marqueeRef.current = { startX: result.x, startY: result.y, active: true, lastClientX: result.x, lastClientY: (event.clientY - rect.top) / zoomY };
    const additive = event.shiftKey || event.metaKey || event.ctrlKey;
    marqueeBaseEntities.current = additive ? new Set(selectedEntityHashes) : new Set();
    marqueeBaseFolders.current = additive ? new Set(selectedFolderNodeIds) : new Set();
    marqueeRectRef.current = null;
    autoScrollSpeedRef.current = 0;
    container.setPointerCapture(event.pointerId);
    if (autoScrollRef.current == null) {
      const tick = () => {
        if (!marqueeRef.current.active) { autoScrollRef.current = null; return; }
        const current = containerRef.current;
        if (current && autoScrollSpeedRef.current !== 0) {
          current.scrollTop += autoScrollSpeedRef.current;
          const state = marqueeRef.current;
          updateMarquee(
            Math.min(state.startX, state.lastClientX),
            Math.min(state.startY, state.lastClientY + current.scrollTop - headerHeight),
            Math.abs(state.lastClientX - state.startX),
            Math.abs(state.lastClientY + current.scrollTop - headerHeight - state.startY),
          );
          markDirty('both');
        }
        autoScrollRef.current = requestAnimationFrame(tick);
      };
      autoScrollRef.current = requestAnimationFrame(tick);
    }
  }, [containerRef, headerHeight, hit, isHeaderControl, markDirty, selectedEntityHashes, selectedFolderNodeIds, updateMarquee]);
  const handlePointerMove = useCallback((event: React.PointerEvent) => {
    if (tileDragRef.current && !isDragActive()) {
      const pending = tileDragRef.current;
      if (Math.abs(event.clientX - pending.startClientX) > 5 || Math.abs(event.clientY - pending.startClientY) > 5) {
        const item = items[pending.tileIdx];
        if (item) {
          const hashes = selectedEntityHashes.has(item.entity_hash) ? [...selectedEntityHashes] : [item.entity_hash];
          const thumbnailHashes = hashes.slice(0, 3);
          startDrag(hashes, event.clientX, event.clientY, dragSourceScope);
          setDragGhost({ x: event.clientX, y: event.clientY, count: hashes.length, thumbnailHashes });
          reorderDropRef.current = null;
          tileDragRef.current = null;
        }
      }
      return;
    }
    if (isDragActive() || !marqueeRef.current.active) return;
    const container = containerRef.current;
    if (!container) return;
    const rect = container.getBoundingClientRect();
    const zoomX = rect.width / (container.offsetWidth || 1);
    const zoomY = rect.height / (container.offsetHeight || 1);
    const clientY = (event.clientY - rect.top) / zoomY;
    const x = (event.clientX - rect.left) / zoomX;
    const y = clientY + container.scrollTop - headerHeight;
    marqueeRef.current.lastClientX = x;
    marqueeRef.current.lastClientY = clientY;
    autoScrollSpeedRef.current = clientY < 50 ? -12 * (1 - clientY / 50)
      : clientY > container.clientHeight - 50 ? 12 * (1 - (container.clientHeight - clientY) / 50) : 0;
    updateMarquee(
      Math.min(marqueeRef.current.startX, x), Math.min(marqueeRef.current.startY, y),
      Math.abs(x - marqueeRef.current.startX), Math.abs(y - marqueeRef.current.startY),
    );
    markDirty('overlay');
  }, [containerRef, dragSourceScope, headerHeight, items, layout.hashToIndex, markDirty, reorderDropRef, selectedEntityHashes, updateMarquee]);
  const handlePointerUp = useCallback((event: React.PointerEvent) => {
    tileDragRef.current = null;
    if (isDragActive() || !marqueeRef.current.active) return;
    const hadMarquee = marqueeRectRef.current != null;
    marqueeRef.current.active = false;
    marqueeRectRef.current = null;
    setMarqueeVisual(null);
    autoScrollSpeedRef.current = 0;
    if (autoScrollRef.current != null) cancelAnimationFrame(autoScrollRef.current);
    autoScrollRef.current = null;
    containerRef.current?.releasePointerCapture(event.pointerId);
    if (hadMarquee) dragJustEndedRef.current = true;
    markDirty('overlay');
  }, [containerRef, markDirty]);

  useEffect(() => () => {
    if (idleTimerRef.current) clearTimeout(idleTimerRef.current);
    clearHoverTimers();
    if (autoScrollRef.current != null) cancelAnimationFrame(autoScrollRef.current);
  }, [clearHoverTimers]);

  return {
    events: {
      onScroll: handleScroll, onClick: handleClick, onDoubleClick: handleDoubleClick,
      onContextMenu: handleContextMenu, onMouseMove: handleMouseMove,
      onMouseLeave: handleMouseLeave, onPointerDown: handlePointerDown,
      onPointerMove: handlePointerMove, onPointerUp: handlePointerUp,
    },
    hoverPreview,
    marqueeVisual,
    dragGhost,
  };
}
