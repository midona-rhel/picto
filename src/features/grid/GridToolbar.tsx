/**
 * Grid toolbar — renders in the titlebar right section.
 *
 * Layout matches legacy ImageGridControls normal mode:
 *   [title(abs)] [- slider +] [view btn][filter btn][search input] [perf] [loading]
 */

import { useAtomValue, useSetAtom } from 'jotai';
import { useRef, useEffect, useCallback } from 'react';
import { KbdTooltip } from '../../shared/ui/KbdTooltip';
import { IconSearch } from '@tabler/icons-react';
import {
  gridTransitionPhaseAtom,
  gridTargetSizeAtom,
  gridSearchTextAtom,
  gridFiltersAtom,
  gridFilterToolbarOpenAtom,
} from '../../state/grid';
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
import { countActiveGridFilters } from './GridFilterMenu';
import {
  ToolbarActualSizeIcon,
  ToolbarChevronIcon,
  ToolbarFilterIcon,
  ToolbarFitIcon,
  ToolbarLayoutIcon,
  ToolbarHistoryIcon,
} from '../../shared/ui/icons/toolbar-icons';
import { GroupEditIcon } from '../../shared/ui/icons/group-icons';
import styles from './GridToolbar.module.css';
import { useShortcutScope } from '../../shared/hooks/useShortcutScope';

const ZOOM_MIN = 150;
const ZOOM_MAX = 900;
const ZOOM_STEP = 50;

// ── Zoom controls ───────────────────────────────────────────────

function ZoomControls() {
  const targetSize = useAtomValue(gridTargetSizeAtom);

  const setAndSave = useCallback((v: number) => {
    gridController.updateView({ targetSize: v });
    gridController.saveViewPref({ target_size: v });
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

// ── Search input ────────────────────────────────────────────────

function SearchInput() {
  const searchText = useAtomValue(gridSearchTextAtom);
  const ref = useRef<HTMLInputElement>(null);

  useShortcutScope((e) => {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'f') {
      e.preventDefault();
      ref.current?.focus();
    }
  }, { priority: 30 });

  return (
    <div className={styles.searchWrap}>
      <IconSearch size={13} aria-hidden="true" />
      <input
        ref={ref}
        type="text"
        className={styles.searchInput}
        placeholder="Search"
        value={searchText}
        onChange={(e) => gridController.setSearchText(e.target.value)}
      />
    </div>
  );
}

// ── Toolbar root ────────────────────────────────────────────────

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
  const zoomMenu = useContextMenu();

  useEffect(() => {
    if (!controls?.zoom) return;
    return controls.zoom.subscribeZoomScale((scale) => {
      const percent = Math.round(scale * 100);
      if (zoomLabelRef.current) zoomLabelRef.current.textContent = `${percent}%`;
      if (sliderRef.current && !sliderDraggingRef.current) {
        sliderRef.current.value = String(zoomToSlider(scale * 100));
      }
    });
  }, [controls]);

  if (!state || !controls) return null;

  const canPrev = !!controls.navigate && state.currentIndex > 0;
  const canNext = !!controls.navigate && state.currentIndex < state.total - 1;
  const zoom = controls.zoom;
  const openZoomMenu = (event: React.MouseEvent) => {
    if (!zoom) return;
    const current = Number.parseInt(zoomLabelRef.current?.textContent ?? '100', 10);
    const levels = [5, 10, 25, 50, 100, 125, 150, 200, 300, 400, 800];
    zoomMenu.open(event, [
      ...levels.map((percent) => ({
        label: `${percent}%`,
        checked: current === percent,
        action: () => zoom.setZoomScale(percent / 100),
      })),
      { separator: true },
      { label: 'Actual size', shortcut: 'Mod+0', action: zoom.fitActual },
      { label: 'Fit to window', shortcut: '`', action: zoom.fitToWindow },
    ], { showSearch: false });
  };

  return (
    <>
    <TitlebarControls
      left={(
        <>
        <KbdTooltip label={controls.backLabel ?? 'Back to grid'} shortcut="Escape">
          <TitlebarControlButton onClick={controls.close} aria-label={controls.backLabel ?? 'Back to grid'}>
            <ToolbarHistoryIcon direction="back" />
          </TitlebarControlButton>
        </KbdTooltip>
        {controls.navigate ? <TitlebarCounter current={state.currentIndex + 1} total={state.total} /> : null}
        </>
      )}
      center={zoom ? (
        <div className={styles.sliderSection}>
          <span
            ref={zoomLabelRef}
            className={styles.zoomLabel}
            role="button"
            tabIndex={0}
            onClick={openZoomMenu}
            onContextMenu={(event) => {
              event.preventDefault();
              event.stopPropagation();
              const current = Number.parseInt(zoomLabelRef.current?.textContent ?? '100', 10);
              if (current === 100) zoom.fitToWindow();
              else zoom.fitActual();
            }}
          >{state.zoomPercent ?? 100}%</span>
          <TitlebarRangeSlider
            ref={sliderRef}
            aria-label="Zoom"
            min={0}
            max={100}
            step={0.5}
            defaultValue={zoomToSlider(state.zoomPercent ?? 100)}
            onValueChange={(value) => {
              sliderDraggingRef.current = true;
              zoom.setZoomScale(sliderToZoom(value) / 100);
            }}
            onMouseUp={() => { sliderDraggingRef.current = false; }}
            onTouchEnd={() => { sliderDraggingRef.current = false; }}
            onPointerCancel={() => { sliderDraggingRef.current = false; }}
            onKeyUp={() => { sliderDraggingRef.current = false; }}
            onBlur={() => { sliderDraggingRef.current = false; }}
          />
        </div>
      ) : null}
      right={(
        <>
        {zoom ? (
          <>
            <KbdTooltip label="Fit to window" shortcut="`">
              <TitlebarControlButton onClick={zoom.fitToWindow} aria-label="Fit to window">
                <ToolbarFitIcon />
              </TitlebarControlButton>
            </KbdTooltip>
            <KbdTooltip label="Actual size" shortcut="Mod+0">
              <TitlebarControlButton onClick={zoom.fitActual} aria-label="Actual size">
                <ToolbarActualSizeIcon />
              </TitlebarControlButton>
            </KbdTooltip>
          </>
        ) : null}
        {controls.edit ? (
          <KbdTooltip label="Edit group">
            <TitlebarControlButton onClick={controls.edit} aria-label="Edit group">
              <GroupEditIcon size={16} />
            </TitlebarControlButton>
          </KbdTooltip>
        ) : null}
        {controls.navigate ? <TitlebarControlGroup>
          <KbdTooltip label="Previous" shortcut="ArrowLeft">
            <TitlebarControlButton
              disabled={!canPrev}
              onClick={canPrev ? () => controls.navigate?.(-1) : undefined}
              aria-label="Previous"
            >
              <ToolbarChevronIcon direction="left" />
            </TitlebarControlButton>
          </KbdTooltip>
          <KbdTooltip label="Next" shortcut="ArrowRight">
            <TitlebarControlButton
              disabled={!canNext}
              onClick={canNext ? () => controls.navigate?.(1) : undefined}
              aria-label="Next"
            >
              <ToolbarChevronIcon direction="right" />
            </TitlebarControlButton>
          </KbdTooltip>
        </TitlebarControlGroup> : null}
        </>
      )}
    />
    {zoomMenu.state && (
      <ContextMenu
        entries={zoomMenu.state.entries}
        position={zoomMenu.state.position}
        showSearch={zoomMenu.state.showSearch}
        onClose={zoomMenu.close}
      />
    )}
    </>
  );
}

export function GridToolbar() {
  const transitionPhase = useAtomValue(gridTransitionPhaseAtom);
  const viewMenu = useContextMenu();
  const filters = useAtomValue(gridFiltersAtom);
  const filterToolbarOpen = useAtomValue(gridFilterToolbarOpenAtom);
  const setFilterToolbarOpen = useSetAtom(gridFilterToolbarOpenAtom);
  const activeFilterCount = countActiveGridFilters(filters);
  const viewBtnRef = useRef<HTMLButtonElement>(null);

  const openViewMenu = useCallback(() => {
    const rect = viewBtnRef.current?.getBoundingClientRect();
    if (!rect) return;
    viewMenu.openAt({ x: rect.left, y: rect.bottom + 4 }, buildViewMenuEntries());
  }, [viewMenu]);

  return (
    <div
      className={styles.toolbar}
      data-transition-phase={transitionPhase}
    >
      <div className={styles.centerGroup}>
        <ZoomControls />
      </div>

      <div className={styles.rightSection}>
        <KbdTooltip label="View options">
          <TitlebarControlButton ref={viewBtnRef} active={viewMenu.state != null} onClick={openViewMenu} aria-label="View options">
            <ToolbarLayoutIcon />
          </TitlebarControlButton>
        </KbdTooltip>

        <KbdTooltip label="Filter library">
          <TitlebarControlButton active={filterToolbarOpen || activeFilterCount > 0} onClick={() => setFilterToolbarOpen((value) => !value)} aria-label="Filter library">
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
          showSearch={viewMenu.state.showSearch}
          onClose={viewMenu.close}
        />
      )}
    </div>
  );
}
