import { useState, useEffect, useRef, useCallback } from 'react';
import { useDebouncedCallback } from '../../hooks/useDebouncedCallback';
import { Slider } from '@mantine/core';
import {
  IconChevronLeft,
  IconChevronRight,
  IconSearch,
  IconMinus,
  IconPlus,
  IconArrowLeft,
  IconArrowsMaximize,
  IconMaximize,
  IconFilter,
  IconFilterFilled,
  IconAdjustments,
} from '@tabler/icons-react';
import { DisplayOptionsPanel } from './DisplayOptionsPanel';
import { SortByRow } from './SortByRow';
import { LayoutRow } from './LayoutRow';
import { KbdTooltip } from '../ui/KbdTooltip';
import { ContextMenu, useContextMenu, type ContextMenuEntry } from '../ui/ContextMenu';
import { useFilterStore, useActiveFilterCount } from '../../stores/filterStore';
import type { DetailViewState, DetailViewControls } from './DetailView';
import type { GridViewMode } from './runtime';
import { useGlobalKeydown } from '../../hooks/useGlobalKeydown';
import st from './ImageGridControls.module.css';

const MIN_SIZE = 100;
const MAX_SIZE = 900;

interface ImageGridControlsProps {
  title: string;
  onBack?: () => void;
  onForward?: () => void;
  canGoBack?: boolean;
  canGoForward?: boolean;
  showSizeControls?: boolean;
  showSearch?: boolean;
  targetSize: number;
  onTargetSizeChange: (size: number) => void;
  containerWidth: number;
  // Sort props
  sortField?: string;
  sortOrder?: string;
  onSortFieldChange?: (field: string) => void;
  onSortOrderChange?: (order: string) => void;
  /** When set, sort dropdown shows one-time folder sort actions instead of persistent sort modes. */
  folderId?: number | null;
  // Folder sort action callbacks (one-time rearrangement of position_rank)
  onSortFolderAction?: (sortBy: string, direction: string) => void;
  onReverseFolderAction?: () => void;
  onReverseSelectedAction?: () => void;
  // View mode
  viewMode?: GridViewMode;
  onViewModeChange?: (mode: GridViewMode) => void;
  // Search text
  searchText?: string;
  onSearchTextChange?: (text: string) => void;
  // Detail mode props
  detailViewState?: DetailViewState | null;
  detailViewControls?: DetailViewControls | null;
}

export function ImageGridControls({
  title,
  onBack,
  onForward,
  canGoBack = false,
  canGoForward = false,
  showSizeControls = true,
  showSearch = true,
  targetSize,
  onTargetSizeChange,
  containerWidth,
  sortField = 'imported_at',
  sortOrder = 'desc',
  onSortFieldChange,
  onSortOrderChange,
  folderId,
  onSortFolderAction,
  onReverseFolderAction,
  onReverseSelectedAction,
  viewMode = 'waterfall',
  onViewModeChange,
  searchText = '',
  onSearchTextChange,
  detailViewState,
  detailViewControls,
}: ImageGridControlsProps) {
  const isDetailMode = !!detailViewState && !!detailViewControls;

  const gap = 8;
  const columnCount = containerWidth > 0
    ? Math.max(1, Math.round((containerWidth + gap) / (targetSize + gap)))
    : 1;

  const [localSize, setLocalSize] = useState(targetSize);
  const isDragging = useRef(false);
  const dragDebounce = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => {
    if (!isDragging.current) {
      setLocalSize(targetSize);
    }
  }, [targetSize]);

  // Zoom slider for detail mode — logarithmic scale: 0→5%, 50→100%, 100→800%
  const zoomToSlider = (zoomPct: number): number => {
    if (zoomPct <= 100) {
      return 50 * Math.log(zoomPct / 5) / Math.log(100 / 5);
    }
    return 50 + 50 * Math.log(zoomPct / 100) / Math.log(800 / 100);
  };
  const sliderToZoom = (pos: number): number => {
    if (pos <= 50) {
      return 5 * Math.pow(100 / 5, pos / 50);
    }
    return 100 * Math.pow(800 / 100, (pos - 50) / 50);
  };

  const [localSliderPos, setLocalSliderPos] = useState(50);
  useEffect(() => {
    if (detailViewState) setLocalSliderPos(zoomToSlider(detailViewState.zoomPercent));
  }, [detailViewState?.zoomPercent]);

  const handleMinus = () => {
    if (targetSize <= MIN_SIZE) return;
    const newCols = columnCount + 1;
    const computed = Math.floor((containerWidth - (newCols - 1) * gap) / newCols);
    const newSize = Math.max(MIN_SIZE, computed);
    onTargetSizeChange(newSize);
    setLocalSize(newSize);
  };

  const handlePlus = () => {
    if (targetSize >= MAX_SIZE) return;
    if (columnCount <= 1) {
      onTargetSizeChange(MAX_SIZE);
      setLocalSize(MAX_SIZE);
      return;
    }
    const newCols = columnCount - 1;
    const computed = Math.floor((containerWidth - (newCols - 1) * gap) / newCols);
    const newSize = Math.min(MAX_SIZE, computed);
    onTargetSizeChange(newSize);
    setLocalSize(newSize);
  };

  // --- View context menu (layout + sort + display) ---
  const viewMenu = useContextMenu();
  const viewBtnRef = useRef<HTMLButtonElement>(null);

  const handleViewClick = useCallback(() => {
    if (!viewBtnRef.current) return;
    const rect = viewBtnRef.current.getBoundingClientRect();
    const items: ContextMenuEntry[] = [
      // Layout mode — same LayoutRow as right-click menu
      {
        type: 'custom',
        key: 'layout',
        render: () => (
          <LayoutRow viewMode={viewMode} onChange={(m) => onViewModeChange?.(m)} />
        ),
      },
    ];

    // Sort row — only for non-folder views (folders use one-time sort via context menu)
    if (!folderId) {
      items.push({
        type: 'custom',
        key: 'sortby',
        render: () => (
          <SortByRow
            field={sortField}
            order={sortOrder}
            onFieldChange={(f) => onSortFieldChange?.(f)}
            onOrderChange={(o) => onSortOrderChange?.(o)}
          />
        ),
      });
    }

    // Display options — flat below sort
    items.push({ type: 'separator' });
    items.push({
      type: 'custom',
      key: 'display-panel',
      render: () => <DisplayOptionsPanel />,
    });

    viewMenu.openAt({ x: rect.left, y: rect.bottom + 4 }, items);
  }, [viewMenu, viewMode, onViewModeChange, sortField, sortOrder, onSortFieldChange, onSortOrderChange, folderId, onSortFolderAction, onReverseFolderAction, onReverseSelectedAction]);

  // --- Filter button ---
  const toggleFilterBar = useFilterStore((s) => s.toggleFilterBar);
  const filterBarOpen = useFilterStore((s) => s.filterBarOpen);
  const activeFilterCount = useActiveFilterCount();
  const filterActive = filterBarOpen || activeFilterCount > 0;

  // --- Search text debounce ---
  const [localSearchText, setLocalSearchText] = useState(searchText);
  useEffect(() => { setLocalSearchText(searchText); }, [searchText]);
  const debouncedSearchChange = useDebouncedCallback((v: string) => onSearchTextChange?.(v), 300);

  const handleSearchInput = useCallback((value: string) => {
    setLocalSearchText(value);
    debouncedSearchChange(value);
  }, [debouncedSearchChange]);

  // ── Keyboard shortcuts for zoom/size (mount-once, refs for stale-closure safety) ──
  const detailStateRef = useRef(detailViewState);
  detailStateRef.current = detailViewState;
  const detailControlsRef = useRef(detailViewControls);
  detailControlsRef.current = detailViewControls;
  const handlePlusRef = useRef(handlePlus);
  handlePlusRef.current = handlePlus;
  const handleMinusRef = useRef(handleMinus);
  handleMinusRef.current = handleMinus;
  const showSizeRef = useRef(showSizeControls);
  showSizeRef.current = showSizeControls;

  const handleGlobalZoomHotkeys = useCallback((e: KeyboardEvent) => {
    if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
    const ds = detailStateRef.current;
    const dc = detailControlsRef.current;
    const inDetail = !!ds && !!dc;

    switch (e.key) {
      case '`':
        if (inDetail) { e.preventDefault(); dc!.fitToWindow(); }
        break;
      case '0':
        if (inDetail && (e.metaKey || e.ctrlKey)) { e.preventDefault(); dc!.fitActual(); }
        break;
      case '=':
      case '+':
        if (inDetail) { e.preventDefault(); dc!.setZoomScale(ds!.zoomScale * 1.25); }
        else if (showSizeRef.current) { e.preventDefault(); handlePlusRef.current(); }
        break;
      case '-':
        if (!e.metaKey && !e.ctrlKey) {
          if (inDetail) { e.preventDefault(); dc!.setZoomScale(ds!.zoomScale / 1.25); }
          else if (showSizeRef.current) { e.preventDefault(); handleMinusRef.current(); }
        }
        break;
    }
  }, []);
  useGlobalKeydown(handleGlobalZoomHotkeys);

  // Measure toolbar width for responsive collapse
  const toolbarRef = useRef<HTMLDivElement>(null);
  const [toolbarWidth, setToolbarWidth] = useState(0);
  useEffect(() => {
    const el = toolbarRef.current;
    if (!el) return;
    const ro = new ResizeObserver(([entry]) => {
      setToolbarWidth(entry.contentRect.width);
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // Collapse: hide title at <550px, hide zoom panel at <450px
  const showTitle = toolbarWidth === 0 || toolbarWidth >= 550;
  const showZoomPanel = showSizeControls && (toolbarWidth === 0 || toolbarWidth >= 450);


  // ========================================================================
  // DETAIL MODE TOOLBAR
  // ========================================================================
  if (isDetailMode) {
    const ds = detailViewState!;
    const dc = detailViewControls!;
    const canPrev = ds.currentIndex > 0;
    const canNext = ds.currentIndex < ds.total - 1;
    return (
      <div className={st.toolbar} ref={toolbarRef}>
        {/* Left: back arrow + counter */}
        <div className={st.leftSection}>
          <KbdTooltip label="Back to grid" shortcut="Escape">
            <button className={st.icBtn} onClick={dc.close}>
              <IconArrowLeft size={16} />
            </button>
          </KbdTooltip>
          <span className={st.counter}>
            {ds.currentIndex + 1} / {ds.total}
          </span>
        </div>

        {/* Center: zoom slider + percentage */}
        <div className={`${st.centerGroup}`}>
          <div className={st.sliderSection}>
            <span className={st.zoomRatio}>{`${ds.zoomPercent}%`}</span>
            <Slider
              value={localSliderPos}
              onChange={(v) => {
                setLocalSliderPos(v);
                dc.setZoomScale(sliderToZoom(v) / 100);
              }}
              min={0}
              max={100}
              step={0.5}
              w={80}
              label={null}
            />
          </div>
        </div>

        {/* Right: fit/actual + gallery navigation */}
        <div className={`${st.rightSection}`}>
          <KbdTooltip label="Fit to window" shortcut="`">
            <button
              className={st.icBtn}
              onClick={dc.fitToWindow}
            >
              <IconArrowsMaximize size={14} />
            </button>
          </KbdTooltip>
          <KbdTooltip label="Actual size" shortcut="Mod+0">
            <button
              className={st.icBtn}
              onClick={dc.fitActual}
            >
              <IconMaximize size={14} />
            </button>
          </KbdTooltip>
          <div className={st.icBtnGroup}>
            <KbdTooltip label="Previous" shortcut="ArrowLeft">
              <button
                className={`${st.icBtn} ${!canPrev ? st.icBtnDisabled : ''}`}
                onClick={canPrev ? () => dc.navigate(-1) : undefined}
              >
                <IconChevronLeft size={16} />
              </button>
            </KbdTooltip>
            <KbdTooltip label="Next" shortcut="ArrowRight">
              <button
                className={`${st.icBtn} ${!canNext ? st.icBtnDisabled : ''}`}
                onClick={canNext ? () => dc.navigate(1) : undefined}
              >
                <IconChevronRight size={16} />
              </button>
            </KbdTooltip>
          </div>
        </div>
      </div>
    );
  }

  // ========================================================================
  // NORMAL MODE TOOLBAR
  // ========================================================================
  return (
    <div className={st.toolbar} ref={toolbarRef}>
      {/* Left: back/forward + title */}
      <div className={st.leftSection}>
        <div className={st.icBtnGroup}>
          <KbdTooltip label="Back" shortcut="Mod+ArrowLeft">
            <button
              className={`${st.icBtn} ${!canGoBack ? st.icBtnDisabled : ''}`}
              onClick={canGoBack ? onBack : undefined}
            >
              <IconChevronLeft size={16} />
            </button>
          </KbdTooltip>
          <KbdTooltip label="Forward" shortcut="Mod+ArrowRight">
            <button
              className={`${st.icBtn} ${!canGoForward ? st.icBtnDisabled : ''}`}
              onClick={canGoForward ? onForward : undefined}
            >
              <IconChevronRight size={16} />
            </button>
          </KbdTooltip>
        </div>
        {showTitle && title && <span className={st.title}>{title}</span>}
      </div>

      {/* Center: size controls — centered between left and right */}
      {showZoomPanel ? (
        <div className={`${st.centerGroup}`}>
          <div className={st.sliderSection}>
            <KbdTooltip label="Zoom out" shortcut="-">
              <button className={st.icBtn} onClick={handleMinus}>
                <IconMinus size={16} />
              </button>
            </KbdTooltip>
            <Slider
              value={localSize}
              onChange={(v) => {
                isDragging.current = true;
                setLocalSize(v);
                if (dragDebounce.current) clearTimeout(dragDebounce.current);
                dragDebounce.current = setTimeout(() => { onTargetSizeChange(v); }, 80);
              }}
              onChangeEnd={(v) => {
                if (dragDebounce.current) clearTimeout(dragDebounce.current);
                isDragging.current = false;
                setLocalSize(v);
                onTargetSizeChange(v);
              }}
              min={MIN_SIZE}
              max={MAX_SIZE}
              step={1}
              w={80}
              label={null}
            />
            <KbdTooltip label="Zoom in" shortcut="+">
              <button className={st.icBtn} onClick={handlePlus}>
                <IconPlus size={16} />
              </button>
            </KbdTooltip>
          </div>
        </div>
      ) : (
        <div className={st.spacer} />
      )}

      {/* Right: view + filter + search */}
      {showSearch && (
        <div className={`${st.rightSection}`}>
          <KbdTooltip label="View">
            <button
              ref={viewBtnRef}
              className={st.icBtn}
              onClick={handleViewClick}
            >
              <IconAdjustments size={14} />
            </button>
          </KbdTooltip>

          <KbdTooltip label="Filter">
            <button
              className={st.icBtn}
              onClick={toggleFilterBar}
            >
              {filterActive ? <IconFilterFilled size={14} /> : <IconFilter size={14} />}
              {activeFilterCount > 0 && <span className={st.filterBadge} />}
            </button>
          </KbdTooltip>

          <div className={st.searchInputWrap}>
            <IconSearch size={13} className={st.searchInputIcon} />
            <input
              className={st.searchInput}
              type="text"
              placeholder="Search..."
              value={localSearchText}
              onChange={(e) => handleSearchInput(e.target.value)}
            />
          </div>
        </div>
      )}

      {/* View context menu portal */}
      {viewMenu.state && (
        <ContextMenu
          items={viewMenu.state.items}
          position={viewMenu.state.position}
          onClose={viewMenu.close}
          searchable={false}
        />
      )}
    </div>
  );
}
