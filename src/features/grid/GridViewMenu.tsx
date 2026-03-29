/**
 * Grid view menu — layout, sort, and display options panel.
 * Uses shared ToggleSwitch and CmSelect components.
 */

import { useAtomValue, useSetAtom } from 'jotai';
import {
  IconBorderAll, IconLayoutBoard,
  IconSortAscending, IconSortDescending,
} from '@tabler/icons-react';
import {
  gridViewModeAtom, gridSortFieldAtom, gridSortDirectionAtom,
  gridShowNameAtom, gridShowExtensionAtom, gridShowResolutionAtom,
  gridShowExtensionLabelAtom, gridFitThumbnailsAtom,
  gridSoftTransitionActionAtom,
  type SortField, type SortDirection, type GridViewMode,
} from '../../state/grid';
import { sidebarCollapsedAtom } from '../../state/navigation';
import { gridController } from '../../controllers/gridController';
import { ToggleSwitch } from '../../shared/ui/ToggleSwitch/ToggleSwitch';
import { CmSelect } from '../../shared/ui/CmSelect/CmSelect';
import type { CmSelectOption } from '../../shared/ui/CmSelect/CmSelect';
import type { MenuEntry } from '../../shared/ui/ContextMenu/ContextMenu';
import s from './GridViewMenu.module.css';

// ── Options ──────────────────────────────────────────────────────

function LayoutIcon({ mode }: { mode: string }) {
  if (mode === 'grid') return <IconBorderAll size={14} />;
  if (mode === 'justified') return <IconLayoutBoard size={14} style={{ transform: 'rotate(-90deg)' }} />;
  return <IconLayoutBoard size={14} />;
}

const LAYOUT_OPTIONS: CmSelectOption[] = [
  { value: 'waterfall', label: 'Waterfall', icon: <LayoutIcon mode="waterfall" /> },
  { value: 'grid', label: 'Grid', icon: <LayoutIcon mode="grid" /> },
  { value: 'justified', label: 'Justified', icon: <LayoutIcon mode="justified" /> },
];

const SORT_OPTIONS: CmSelectOption[] = [
  { value: 'date_added', label: 'Date Added' },
  { value: 'date_created', label: 'Date Created' },
  { value: 'date_modified', label: 'Date Modified' },
  { value: 'name', label: 'Name' },
  { value: 'size_bytes', label: 'File Size' },
  { value: 'rating', label: 'Rating' },
  { value: 'duration', label: 'Duration' },
];

// ── Panel ────────────────────────────────────────────────────────

function ViewPanel() {
  const viewMode = useAtomValue(gridViewModeAtom);
  const setViewMode = useSetAtom(gridViewModeAtom);
  const sortField = useAtomValue(gridSortFieldAtom);
  const sortDir = useAtomValue(gridSortDirectionAtom);
  const showName = useAtomValue(gridShowNameAtom);
  const setShowName = useSetAtom(gridShowNameAtom);
  const showRes = useAtomValue(gridShowResolutionAtom);
  const setShowRes = useSetAtom(gridShowResolutionAtom);
  const showExt = useAtomValue(gridShowExtensionAtom);
  const setShowExt = useSetAtom(gridShowExtensionAtom);
  const showExtLabel = useAtomValue(gridShowExtensionLabelAtom);
  const setShowExtLabel = useSetAtom(gridShowExtensionLabelAtom);
  const fitThumbs = useAtomValue(gridFitThumbnailsAtom);
  const setFitThumbs = useSetAtom(gridFitThumbnailsAtom);
  const sidebarCollapsed = useAtomValue(sidebarCollapsedAtom);
  const setSidebarCollapsed = useSetAtom(sidebarCollapsedAtom);
  const setSoftAction = useSetAtom(gridSoftTransitionActionAtom);

  const setSort = (f: SortField, d: SortDirection) => { void gridController.setSort(f, d); };

  const toggle = (label: string, on: boolean, flip: () => void, disabled = false) => (
    <div className={`${s.toggleRow} ${disabled ? s.toggleRowDisabled : ''}`} onClick={disabled ? undefined : flip}>
      <div className={s.toggleLabel}>{label}</div>
      <div className={s.toggleValue}><ToggleSwitch on={on} onChange={disabled ? () => {} : flip} /></div>
    </div>
  );

  return (
    <div className={s.panel}>
      <div className={s.headerRow}>
        <span className={s.headerLabel}>Layout</span>
        <CmSelect
          value={viewMode}
          options={LAYOUT_OPTIONS}
          onChange={(v) => setSoftAction(() => () => setViewMode(v as GridViewMode))}
          width={130}
        />
      </div>

      <div className={s.headerRow}>
        <span className={s.headerLabel}>Sort by</span>
        <div className={s.sortControls}>
          <CmSelect
            value={sortField}
            options={SORT_OPTIONS}
            onChange={(v) => setSort(v as SortField, sortDir)}
            width={130}
          />
          <div className={s.dirPill}>
            <button className={`${s.dirBtn} ${sortDir === 'asc' ? s.dirBtnActive : ''}`} onClick={() => setSort(sortField, 'asc')} type="button">
              <IconSortAscending size={14} />
            </button>
            <button className={`${s.dirBtn} ${sortDir === 'desc' ? s.dirBtnActive : ''}`} onClick={() => setSort(sortField, 'desc')} type="button">
              <IconSortDescending size={14} />
            </button>
          </div>
        </div>
      </div>

      <div className={s.sep} />

      {toggle('Show Name', showName, () => setShowName(!showName))}
      {toggle('Show resolution', showRes, () => setShowRes(!showRes))}
      {toggle('Show extension', showExt, () => setShowExt(!showExt))}
      {toggle('Show label', showExtLabel, () => setShowExtLabel(!showExtLabel))}
      {toggle('Fit thumbnails', fitThumbs, () => setFitThumbs(!fitThumbs), viewMode !== 'grid')}
      {toggle('Show subfolders', false, () => { /* TODO */ })}

      <div className={s.sep} />

      {toggle('Show Sidebar', !sidebarCollapsed, () => setSidebarCollapsed(!sidebarCollapsed))}
      {toggle('Show Inspector', true, () => { /* TODO */ })}
    </div>
  );
}

export function buildViewMenuEntries(): MenuEntry[] {
  return [{ custom: true, key: 'view-panel', render: () => <ViewPanel /> }];
}
