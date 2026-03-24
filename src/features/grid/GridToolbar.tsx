/**
 * Grid toolbar — renders in the titlebar right section.
 * Contains: item count, sort, view mode, zoom slider.
 */

import { useAtomValue, useSetAtom } from 'jotai';
import {
  IconLayoutGrid, IconLayoutList, IconLayoutRows,
  IconSortAscending, IconSortDescending,
} from '@tabler/icons-react';
import {
  gridTotalCountAtom, gridLoadingAtom,
  gridSortFieldAtom, gridSortDirectionAtom,
  gridViewModeAtom, gridTargetSizeAtom,
  type SortField, type GridViewMode,
} from '../../state/grid';
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

export function GridToolbar() {
  const totalCount = useAtomValue(gridTotalCountAtom);
  const loading = useAtomValue(gridLoadingAtom);
  const sortField = useAtomValue(gridSortFieldAtom);
  const sortDirection = useAtomValue(gridSortDirectionAtom);
  const viewMode = useAtomValue(gridViewModeAtom);
  const targetSize = useAtomValue(gridTargetSizeAtom);
  const setViewMode = useSetAtom(gridViewModeAtom);
  const setTargetSize = useSetAtom(gridTargetSizeAtom);

  const SortIcon = sortDirection === 'desc' ? IconSortDescending : IconSortAscending;

  return (
    <div className={styles.toolbar}>
      {/* Item count */}
      <span className={styles.count}>
        {loading && totalCount == null ? '…' : totalCount != null ? totalCount.toLocaleString() : '0'}
      </span>

      <div className={styles.spacer} />

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

      {/* Zoom slider */}
      <div className={styles.zoomGroup}>
        <input
          type="range"
          min={100}
          max={900}
          step={10}
          value={targetSize}
          onChange={(e) => setTargetSize(Number(e.target.value))}
          className={styles.zoomSlider}
        />
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

      {loading && <span className={styles.loadingDot} />}
    </div>
  );
}
