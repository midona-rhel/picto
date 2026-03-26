/**
 * Grid toolbar — renders in the titlebar right section.
 *
 * Layout matches legacy ImageGridControls normal mode:
 *   [title(abs)] [- slider +] [view btn][filter btn][search input] [perf] [loading]
 */

import { useAtomValue, useSetAtom } from 'jotai';
import { useRef, useState, useEffect, useCallback } from 'react';
import {
  IconMinus, IconPlus, IconSearch,
  IconAdjustments, IconFilter,
} from '@tabler/icons-react';
import {
  gridLoadingAtom,
  gridTargetSizeAtom,
  gridScopeLabelAtom,
  gridSearchTextAtom,
} from '../../state/grid';
import { gridController } from '../../controllers/gridController';
import { gridPerfAtom } from '../../state/gridPerf';
import styles from './GridToolbar.module.css';

const ZOOM_MIN = 100;
const ZOOM_MAX = 900;
const ZOOM_STEP = 50;

// ── Title with fade transition on scope change ──────────────────

const TITLE_FADE_MS = 250;

function ScopeTitle() {
  const label = useAtomValue(gridScopeLabelAtom);
  const [displayed, setDisplayed] = useState(label);
  const [fading, setFading] = useState(false);
  const pendingRef = useRef(label);

  useEffect(() => {
    if (label === displayed) return;
    pendingRef.current = label;
    setFading(true);
    const timer = window.setTimeout(() => {
      setDisplayed(pendingRef.current);
      setFading(false);
    }, TITLE_FADE_MS);
    return () => clearTimeout(timer);
  }, [label, displayed]);

  if (!displayed) return null;
  return (
    <span className={`${styles.title} ${fading ? styles.titleFadeOut : styles.titleFadeIn}`}>
      {displayed}
    </span>
  );
}

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

export function GridToolbar() {
  const loading = useAtomValue(gridLoadingAtom);

  const isDevHost = typeof window !== 'undefined'
    && (window.location.hostname === '127.0.0.1' || window.location.hostname === 'localhost');

  return (
    <div className={styles.toolbar}>
      <ScopeTitle />

      <div className={styles.centerGroup}>
        <ZoomControls />
      </div>

      <div className={styles.rightSection}>
        <button className={styles.icBtn} title="View">
          <IconAdjustments size={14} style={{ transform: 'rotate(90deg)' }} />
        </button>

        <button className={styles.icBtn} title="Filter">
          <IconFilter size={14} />
        </button>

        <SearchInput />
      </div>

      {isDevHost && <PerfChip />}
      {loading && <span className={styles.loadingDot} />}
    </div>
  );
}
