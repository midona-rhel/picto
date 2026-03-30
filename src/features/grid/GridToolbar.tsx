/**
 * Grid toolbar — renders in the titlebar right section.
 *
 * Layout matches legacy ImageGridControls normal mode:
 *   [title(abs)] [- slider +] [view btn][filter btn][search input] [perf] [loading]
 */

import { useAtomValue, useSetAtom } from 'jotai';
import { useRef, useState, useEffect, useCallback } from 'react';
import { KbdTooltip } from '../../shared/ui/KbdTooltip';
import {
  IconMinus, IconPlus, IconSearch,
  IconAdjustments, IconFilter,
  IconArrowLeft, IconChevronLeft, IconChevronRight,
  IconArrowsMaximize, IconMaximize,
} from '@tabler/icons-react';
import {
  gridTargetSizeAtom,
  gridSearchTextAtom,
} from '../../state/grid';
import { gridController } from '../../controllers/gridController';
import { gridPerfAtom } from '../../state/gridPerf';
import { viewerDisplayStateAtom, viewerDisplayControlsAtom } from '../../state/viewer';
import { ContextMenu, useContextMenu } from '../../shared/ui/ContextMenu';
import { buildViewMenuEntries } from './GridViewMenu';
import styles from './GridToolbar.module.css';

const ZOOM_MIN = 150;
const ZOOM_MAX = 900;
const ZOOM_STEP = 50;

// ── Zoom controls ───────────────────────────────────────────────

function ZoomControls() {
  const targetSize = useAtomValue(gridTargetSizeAtom);
  const setTargetSize = useSetAtom(gridTargetSizeAtom);

  const zoomIn = useCallback(() => {
    setTargetSize(Math.min(ZOOM_MAX, targetSize + ZOOM_STEP));
  }, [targetSize, setTargetSize]);

  const zoomOut = useCallback(() => {
    setTargetSize(Math.max(ZOOM_MIN, targetSize - ZOOM_STEP));
  }, [targetSize, setTargetSize]);

  return (
    <div className={styles.sliderSection}>
      <button
        className={styles.icBtn}
        onClick={zoomOut}
        disabled={targetSize <= ZOOM_MIN}
        title="Zoom out (-)"
      >
        <IconMinus size={16} />
      </button>
      <input
        type="range"
        min={ZOOM_MIN}
        max={ZOOM_MAX}
        step={10}
        value={targetSize}
        onChange={(e) => setTargetSize(Number(e.target.value))}
        className={styles.zoomSlider}
      />
      <button
        className={styles.icBtn}
        onClick={zoomIn}
        disabled={targetSize >= ZOOM_MAX}
        title="Zoom in (+)"
      >
        <IconPlus size={16} />
      </button>
    </div>
  );
}

// ── Search input ────────────────────────────────────────────────

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
        onChange={(e) => gridController.setSearchText(e.target.value)}
      />
    </div>
  );
}

// ── Perf chip (dev only) ────────────────────────────────────────

function PerfChip() {
  const perf = useAtomValue(gridPerfAtom);
  if (!perf) return null;
  return (
    <div
      className={`${styles.perfChip} ${perf.missedFrames > 0 ? styles.perfChipWarn : ''}`}
      title={`fps ${perf.fps} | missed ${perf.missedFrames} | draw ${perf.drawOverBudgetFrames} | gap ${perf.avgFrameGapMs.toFixed(1)}ms avg / ${perf.maxFrameGapMs.toFixed(1)}ms max | ${perf.inferredCause}`}
    >
      {perf.fps}fps · {perf.inferredCause}
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
  const [sliderPos, setSliderPos] = useState(50);
  const sliderDraggingRef = useRef(false);

  // Sync slider from viewer state when not dragging
  useEffect(() => {
    if (state && !sliderDraggingRef.current) {
      setSliderPos(zoomToSlider(state.zoomPercent));
    }
  }, [state?.zoomPercent]);

  if (!state || !controls) return null;

  const canPrev = state.currentIndex > 0;
  const canNext = state.currentIndex < state.total - 1;

  return (
    <div className={styles.toolbar}>
      <div className={styles.leftSection}>
        <KbdTooltip label="Back to grid" shortcut="Escape">
          <button className={styles.icBtn} onClick={controls.close}>
            <IconArrowLeft size={16} />
          </button>
        </KbdTooltip>
        <span className={styles.counter}>
          {state.currentIndex + 1} / {state.total}
        </span>
      </div>

      <div className={styles.centerGroup}>
        <div className={styles.sliderSection}>
          <span className={styles.zoomLabel}>{state.zoomPercent}%</span>
          <input
            type="range"
            className={styles.zoomSlider}
            min={0}
            max={100}
            step={0.5}
            value={sliderPos}
            onChange={(e) => {
              sliderDraggingRef.current = true;
              const v = Number(e.target.value);
              setSliderPos(v);
              controls.setZoomScale(sliderToZoom(v) / 100);
            }}
            onMouseUp={() => { sliderDraggingRef.current = false; }}
            onTouchEnd={() => { sliderDraggingRef.current = false; }}
          />
        </div>
      </div>

      <div className={styles.rightSection}>
        <KbdTooltip label="Fit to window" shortcut="`">
          <button className={styles.icBtn} onClick={controls.fitToWindow}>
            <IconArrowsMaximize size={14} />
          </button>
        </KbdTooltip>
        <KbdTooltip label="Actual size" shortcut="Mod+0">
          <button className={styles.icBtn} onClick={controls.fitActual}>
            <IconMaximize size={14} />
          </button>
        </KbdTooltip>
        <div className={styles.navGroup}>
          <KbdTooltip label="Previous" shortcut="ArrowLeft">
            <button
              className={`${styles.icBtn} ${!canPrev ? styles.icBtnDisabled : ''}`}
              onClick={canPrev ? () => controls.navigate(-1) : undefined}
            >
              <IconChevronLeft size={16} />
            </button>
          </KbdTooltip>
          <KbdTooltip label="Next" shortcut="ArrowRight">
            <button
              className={`${styles.icBtn} ${!canNext ? styles.icBtnDisabled : ''}`}
              onClick={canNext ? () => controls.navigate(1) : undefined}
            >
              <IconChevronRight size={16} />
            </button>
          </KbdTooltip>
        </div>
      </div>
    </div>
  );
}

export function GridToolbar() {
  const isDevHost = typeof window !== 'undefined'
    && (window.location.hostname === '127.0.0.1' || window.location.hostname === 'localhost');
  const viewMenu = useContextMenu();
  const viewBtnRef = useRef<HTMLButtonElement>(null);

  const openViewMenu = useCallback(() => {
    const rect = viewBtnRef.current?.getBoundingClientRect();
    if (!rect) return;
    viewMenu.openAt({ x: rect.left, y: rect.bottom + 4 }, buildViewMenuEntries());
  }, [viewMenu]);

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
        <button
          ref={viewBtnRef}
          className={`${styles.icBtn} ${viewMenu.state ? styles.icBtnActive : ''}`}
          onClick={openViewMenu}
          title="View"
        >
          <IconAdjustments size={14} style={{ transform: 'rotate(90deg)' }} />
        </button>

        <button className={styles.icBtn} title="Filter">
          <IconFilter size={14} />
        </button>

        <SearchInput />
      </div>

      {isDevHost && <PerfChip />}

      {viewMenu.state && (
        <ContextMenu
          entries={viewMenu.state.entries}
          position={viewMenu.state.position}
          onClose={viewMenu.close}
          searchable={false}
          width={270}
        />
      )}
    </div>
  );
}
