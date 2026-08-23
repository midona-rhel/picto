import { useAtomValue } from 'jotai';
import { useRef, useState, useEffect, useCallback } from 'react';
import { KbdTooltip } from '../../shared/ui/KbdTooltip';
import {
  IconSearch,
} from '@tabler/icons-react';
import { gridTargetSizeAtom, gridSearchTextAtom, gridFiltersAtom } from '../../state/grid';
import { gridController } from '../../controllers/gridController';
import { viewerDisplayStateAtom, viewerDisplayControlsAtom } from '../../state/viewer';
import { ContextMenu, useContextMenu } from '../../shared/ui/ContextMenu';
import {
  TitlebarControlButton,
  TitlebarControlGroup,
  TitlebarControls,
  TitlebarCounter,
  TitlebarRangeSlider,
  TitlebarZoomSlider,
} from '../../shared/ui/TitlebarControls';
import { buildViewMenuEntries } from './GridViewMenu';
import { buildFilterMenuEntries, countActiveGridFilters } from './GridFilterMenu';
import {
  ToolbarActualSizeIcon,
  ToolbarChevronIcon,
  ToolbarFilterIcon,
  ToolbarFitIcon,
  ToolbarLayoutIcon,
  ToolbarHistoryIcon,
} from '../../shared/ui/icons/toolbar-icons';
import styles from './GridToolbar.module.css';

const ZOOM_MIN = 150;
const ZOOM_MAX = 900;
const ZOOM_STEP = 50;

function ZoomControls() {
  const targetSize = useAtomValue(gridTargetSizeAtom);

  const setAndSave = useCallback((v: number) => {
    gridController.dispatch({ type: 'view', patch: { targetSize: v } });
  }, []);

  const zoomIn = useCallback(() => {
    setAndSave(Math.min(ZOOM_MAX, targetSize + ZOOM_STEP));
  }, [targetSize, setAndSave]);

  const zoomOut = useCallback(() => {
    setAndSave(Math.max(ZOOM_MIN, targetSize - ZOOM_STEP));
  }, [targetSize, setAndSave]);

  return (
    <div className={styles.sliderSection}>
      <TitlebarZoomSlider
        min={ZOOM_MIN}
        max={ZOOM_MAX}
        step={10}
        value={targetSize}
        onChange={setAndSave}
        onZoomOut={zoomOut}
        onZoomIn={zoomIn}
      />
    </div>
  );
}

function SearchInput() {
  const searchText = useAtomValue(gridSearchTextAtom);
  const ref = useRef<HTMLInputElement>(null);

  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      if ((e.metaKey || e.ctrlKey) && e.key === 'f') {
        e.preventDefault();
        ref.current?.focus();
      }
    }
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, []);

  return (
    <div className={styles.searchWrap}>
      <IconSearch size={13} className={styles.searchIcon} />
      <input
        ref={ref}
        type="text"
        className={styles.searchInput}
        placeholder="Search files, notes, sources..."
        value={searchText}
        onChange={(e) => gridController.dispatch({ type: 'search', text: e.target.value })}
      />
    </div>
  );
}

// Logarithmic zoom slider: 0→5%, 50→100%, 100→800%
function zoomToSlider(zoomPct: number): number {
  if (zoomPct <= 100) return 50 * Math.log(zoomPct / 5) / Math.log(100 / 5);
  return 50 + 50 * Math.log(zoomPct / 100) / Math.log(800 / 100);
}
function sliderToZoom(pos: number): number {
  if (pos <= 50) return 5 * Math.pow(100 / 5, pos / 50);
  return 100 * Math.pow(800 / 100, (pos - 50) / 50);
}

export function ViewerToolbar() {
  const state = useAtomValue(viewerDisplayStateAtom);
  const controls = useAtomValue(viewerDisplayControlsAtom);
  const sliderDraggingRef = useRef(false);
  const sliderRef = useRef<HTMLInputElement>(null);
  const zoomLabelRef = useRef<HTMLSpanElement>(null);

  useEffect(() => {
    if (!controls) return;
    return controls.subscribeZoomScale((scale) => {
      const percent = Math.round(scale * 100);
      if (zoomLabelRef.current) zoomLabelRef.current.textContent = `${percent}%`;
      if (sliderRef.current && !sliderDraggingRef.current) {
        sliderRef.current.value = String(zoomToSlider(scale * 100));
      }
    });
  }, [controls]);

  if (!state || !controls) return null;

  const canPrev = state.currentIndex > 0;
  const canNext = state.currentIndex < state.total - 1;

  return (
    <TitlebarControls
      left={(
        <>
        <KbdTooltip label="Back to grid" shortcut="Escape">
          <TitlebarControlButton onClick={controls.close}>
            <ToolbarHistoryIcon direction="back" />
          </TitlebarControlButton>
        </KbdTooltip>
        <TitlebarCounter current={state.currentIndex + 1} total={state.total} />
        </>
      )}
      center={(
        <div className={styles.sliderSection}>
          <span ref={zoomLabelRef} className={styles.zoomLabel}>{state.zoomPercent}%</span>
          <TitlebarRangeSlider
            ref={sliderRef}
            aria-label="Zoom"
            min={0}
            max={100}
            step={0.5}
            defaultValue={zoomToSlider(state.zoomPercent)}
            onValueChange={(value) => {
              sliderDraggingRef.current = true;
              controls.setZoomScale(sliderToZoom(value) / 100);
            }}
            onMouseUp={() => { sliderDraggingRef.current = false; }}
            onTouchEnd={() => { sliderDraggingRef.current = false; }}
            onPointerCancel={() => { sliderDraggingRef.current = false; }}
            onKeyUp={() => { sliderDraggingRef.current = false; }}
            onBlur={() => { sliderDraggingRef.current = false; }}
          />
        </div>
      )}
      right={(
        <>
        <KbdTooltip label="Fit to window" shortcut="`">
          <TitlebarControlButton onClick={controls.fitToWindow}>
            <ToolbarFitIcon />
          </TitlebarControlButton>
        </KbdTooltip>
        <KbdTooltip label="Actual size" shortcut="Mod+0">
          <TitlebarControlButton onClick={controls.fitActual}>
            <ToolbarActualSizeIcon />
          </TitlebarControlButton>
        </KbdTooltip>
        <TitlebarControlGroup>
          <KbdTooltip label="Previous" shortcut="ArrowLeft">
            <TitlebarControlButton
              disabled={!canPrev}
              onClick={canPrev ? () => controls.navigate(-1) : undefined}
            >
              <ToolbarChevronIcon direction="left" />
            </TitlebarControlButton>
          </KbdTooltip>
          <KbdTooltip label="Next" shortcut="ArrowRight">
            <TitlebarControlButton
              disabled={!canNext}
              onClick={canNext ? () => controls.navigate(1) : undefined}
            >
              <ToolbarChevronIcon direction="right" />
            </TitlebarControlButton>
          </KbdTooltip>
        </TitlebarControlGroup>
        </>
      )}
    />
  );
}

export function GridToolbar() {
  const viewMenu = useContextMenu();
  const filterMenu = useContextMenu();
  const filters = useAtomValue(gridFiltersAtom);
  const activeFilterCount = countActiveGridFilters(filters);
  const viewBtnRef = useRef<HTMLButtonElement>(null);
  const filterBtnRef = useRef<HTMLButtonElement>(null);

  const openViewMenu = useCallback(() => {
    const rect = viewBtnRef.current?.getBoundingClientRect();
    if (!rect) return;
    viewMenu.openAt({ x: rect.left, y: rect.bottom + 4 }, buildViewMenuEntries());
  }, [viewMenu]);

  const openFilterMenu = useCallback(() => {
    const rect = filterBtnRef.current?.getBoundingClientRect();
    if (!rect) return;
    filterMenu.openAt({ x: rect.left, y: rect.bottom + 4 }, buildFilterMenuEntries(filterMenu.close));
  }, [filterMenu]);

  const toolbarRef = useRef<HTMLDivElement>(null);
  const [toolbarWidth, setToolbarWidth] = useState(9999);
  useEffect(() => {
    const el = toolbarRef.current;
    if (!el) return;
    const ro = new ResizeObserver((entries) => {
      for (const entry of entries) setToolbarWidth(entry.contentRect.width);
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const showZoom = toolbarWidth > 300;

  return (
    <div ref={toolbarRef} className={styles.toolbar}>
      <div className={styles.centerGroup} style={showZoom ? undefined : { visibility: 'hidden', pointerEvents: 'none' }}>
        <ZoomControls />
      </div>

      <div className={styles.rightSection}>
        <KbdTooltip label="View options">
          <TitlebarControlButton ref={viewBtnRef} active={viewMenu.state != null} onClick={openViewMenu} aria-label="View options">
            <ToolbarLayoutIcon />
          </TitlebarControlButton>
        </KbdTooltip>

        <KbdTooltip label="Filter library">
          <TitlebarControlButton ref={filterBtnRef} active={filterMenu.state != null || activeFilterCount > 0} onClick={openFilterMenu} aria-label="Filter library">
            <ToolbarFilterIcon />
            {activeFilterCount > 0 ? <span className={styles.filterBadge}>{activeFilterCount}</span> : null}
          </TitlebarControlButton>
        </KbdTooltip>

        <SearchInput />
      </div>

      {viewMenu.state && (
        <ContextMenu
          entries={viewMenu.state.entries}
          position={viewMenu.state.position}
          onClose={viewMenu.close}
        />
      )}
      {filterMenu.state && (
        <ContextMenu entries={filterMenu.state.entries} position={filterMenu.state.position} onClose={filterMenu.close} />
      )}
    </div>
  );
}
