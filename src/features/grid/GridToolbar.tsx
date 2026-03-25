/**
 * Grid toolbar — renders in the titlebar right section.
 *
 * Layout matches legacy ImageGridControls normal mode:
 *   [count] [spacer] [-][slider][+] [view modes] [sort select][sort dir] [search] [loading]
 */

import { useAtomValue, useSetAtom } from 'jotai';
import { useRef, useState, useEffect, useCallback } from 'react';
import {
  IconLayoutGrid, IconLayoutList, IconLayoutRows,
  IconSortAscending, IconSortDescending,
  IconMinus, IconPlus,
  IconSearch,
} from '@tabler/icons-react';
import {
  gridTotalCountAtom, gridLoadingAtom,
  gridSortFieldAtom, gridSortDirectionAtom,
  gridViewModeAtom, gridTargetSizeAtom,
  gridScopeLabelAtom,
  type SortField, type GridViewMode,
} from '../../state/grid';
import { gridPerfAtom } from '../../state/gridPerf';
import { gridController } from '../../controllers/gridController';
import styles from './GridToolbar.module.css';

const SORT_OPTIONS: { value: SortField; label: string }[] = [
  { value: 'date_added', label: 'Date Added' },
  { value: 'date_created', label: 'Date Created' },
  { value: 'date_modified', label: 'Date Modified' },
  { value: 'name', label: 'Name' },
  { value: 'rating', label: 'Rating' },
  { value: 'size_bytes', label: 'Size' },
];

const VIEW_MODES: { value: GridViewMode; Icon: typeof IconLayoutGrid; label: string }[] = [
  { value: 'waterfall', Icon: IconLayoutRows, label: 'Waterfall' },
  { value: 'grid', Icon: IconLayoutGrid, label: 'Grid' },
  { value: 'justified', Icon: IconLayoutList, label: 'Justified' },
];

const ZOOM_MIN = 100;
const ZOOM_MAX = 900;
const ZOOM_STEP = 50;

export function GridToolbar() {
  const totalCount = useAtomValue(gridTotalCountAtom);
  const loading = useAtomValue(gridLoadingAtom);
  const sortField = useAtomValue(gridSortFieldAtom);
  const sortDirection = useAtomValue(gridSortDirectionAtom);
  const viewMode = useAtomValue(gridViewModeAtom);
  const targetSize = useAtomValue(gridTargetSizeAtom);
  const perf = useAtomValue(gridPerfAtom);
  const setViewMode = useSetAtom(gridViewModeAtom);
  const setTargetSize = useSetAtom(gridTargetSizeAtom);
  const scopeLabel = useAtomValue(gridScopeLabelAtom);

  const [searchValue, setSearchValue] = useState('');
  const searchRef = useRef<HTMLInputElement>(null);

  const SortIcon = sortDirection === 'desc' ? IconSortDescending : IconSortAscending;
  const isDevHost = typeof window !== 'undefined'
    && (window.location.hostname === '127.0.0.1' || window.location.hostname === 'localhost');

  const zoomIn = useCallback(() => {
    setTargetSize(Math.min(ZOOM_MAX, targetSize + ZOOM_STEP));
  }, [targetSize, setTargetSize]);

  const zoomOut = useCallback(() => {
    setTargetSize(Math.max(ZOOM_MIN, targetSize - ZOOM_STEP));
  }, [targetSize, setTargetSize]);

  // Cmd+F focuses search
  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      if ((e.metaKey || e.ctrlKey) && e.key === 'f') {
        e.preventDefault();
        searchRef.current?.focus();
      }
    }
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, []);

  return (
    <div className={styles.toolbar}>
      {/* Scope label + count */}
      {scopeLabel && <span className={styles.scopeLabel}>{scopeLabel}</span>}
      <span className={styles.count}>
        {loading && totalCount == null ? '…' : totalCount != null ? totalCount.toLocaleString() : '0'}
      </span>

      <div className={styles.spacer} />

      {/* Zoom: minus, slider, plus */}
      <div className={styles.zoomGroup}>
        <button
          className={styles.zoomBtn}
          onClick={zoomOut}
          disabled={targetSize <= ZOOM_MIN}
          title="Zoom out (-)"
        >
          <IconMinus size={14} stroke={1.5} />
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
          className={styles.zoomBtn}
          onClick={zoomIn}
          disabled={targetSize >= ZOOM_MAX}
          title="Zoom in (+)"
        >
          <IconPlus size={14} stroke={1.5} />
        </button>
      </div>

      {/* View mode buttons */}
      <div className={styles.viewModes}>
        {VIEW_MODES.map(({ value, Icon, label }) => (
          <button
            key={value}
            className={`${styles.viewModeBtn} ${viewMode === value ? styles.viewModeActive : ''}`}
            onClick={() => setViewMode(value)}
            title={label}
          >
            <Icon size={15} stroke={1.5} />
          </button>
        ))}
      </div>

      {/* Sort */}
      <div className={styles.sortGroup}>
        <select
          className={styles.sortSelect}
          value={sortField}
          onChange={(e) => gridController.setSort(e.target.value as SortField, sortDirection)}
        >
          {SORT_OPTIONS.map((opt) => (
            <option key={opt.value} value={opt.value}>{opt.label}</option>
          ))}
        </select>
        <button
          className={styles.sortDirBtn}
          onClick={() => gridController.setSort(sortField, sortDirection === 'desc' ? 'asc' : 'desc')}
          title={sortDirection === 'desc' ? 'Descending' : 'Ascending'}
        >
          <SortIcon size={14} stroke={1.5} />
        </button>
      </div>

      {/* Search */}
      <div className={styles.searchGroup}>
        <IconSearch size={13} stroke={1.5} className={styles.searchIcon} />
        <input
          ref={searchRef}
          type="text"
          className={styles.searchInput}
          placeholder="Search..."
          value={searchValue}
          onChange={(e) => setSearchValue(e.target.value)}
        />
      </div>

      {isDevHost && perf && (
        <div
          className={`${styles.perfChip} ${perf.missedFrames > 0 ? styles.perfChipWarn : ''}`}
          title={`fps ${perf.fps} | missed ${perf.missedFrames} | near ${perf.nearThresholdFrames} | pauses ${perf.pauseFrames} | draw ${perf.drawOverBudgetFrames} | gap ${perf.avgFrameGapMs.toFixed(2)}ms avg / ${perf.maxFrameGapMs.toFixed(2)}ms max | missed max ${perf.maxMissedFrameGapMs.toFixed(2)}ms | pause max ${perf.maxPauseGapMs.toFixed(2)}ms | total p99 ${perf.totalP99Ms.toFixed(2)}ms | culprit ${perf.slowestPhase} ${perf.slowestPhaseP99Ms.toFixed(2)}ms | queue ${perf.queueDepth} | loads ${perf.activeLoads} | cache ${perf.cacheMb.toFixed(1)}MB | visible tiles ${perf.visibleTileCount} | visible thumbs ${perf.visibleUniqueThumbCount} | ready ${perf.visibleUniqueThumbReady} | loading ${perf.visibleUniqueThumbLoading} | queued ${perf.visibleUniqueThumbQueued} | missing ${perf.visibleUniqueThumbMissing} | scroll ${perf.scrollActive ? 'active' : 'idle'} | scroll frames ${perf.scrollFrames} | velocity ${perf.avgScrollVelocityPxPerMs.toFixed(2)}px/ms avg / ${perf.maxScrollVelocityPxPerMs.toFixed(2)}px/ms max | raf idle ${perf.rafFramesWhileIdle} | raf scroll ${perf.rafFramesWhileScrolling} | mode ${perf.scrollTranslationMode}`}
        >
          {perf.fps}fps · missed {perf.missedFrames} · {perf.inferredCause}
        </div>
      )}

      {loading && <span className={styles.loadingDot} />}
    </div>
  );
}
