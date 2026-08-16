/**
 * Grid view menu — layout, sort, and display options panel.
 * Uses shared ToggleSwitch and CmSelect components.
 */

import { useAtomValue, useSetAtom } from 'jotai';
import {
  IconBorderAll, IconLayoutBoard,
  IconSortAscending, IconSortDescending,
  IconAdjustments,
} from '@tabler/icons-react';
import {
  gridViewModeAtom, gridSortFieldAtom, gridSortDirectionAtom,
  gridShowNameAtom, gridShowExtensionAtom, gridShowResolutionAtom,
  gridShowExtensionLabelAtom, gridFitThumbnailsAtom,
  gridSoftTransitionActionAtom, gridScopeAtom,
  type SortField, type SortDirection, type GridViewMode,
} from '../../state/grid';
import { sidebarCollapsedAtom } from '../../state/navigation';
import { inspectorCollapsedAtom } from '../../state/navigation';
import { gridShowSubfoldersAtom } from '../../state/grid';
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
  const setSoftAction = useSetAtom(gridSoftTransitionActionAtom);
  const scope = useAtomValue(gridScopeAtom);
  const hasFixedOrder = scope.kind === 'folder'
    || (scope.kind === 'system' && scope.key === 'recent_viewed');

  const setSort = (f: SortField, d: SortDirection) => { void gridController.setSort(f, d); };

  return (
    <div className={s.panel}>
      <div className={s.headerRow}>
        <span className={s.headerLabel}>Layout</span>
        <CmSelect
          value={viewMode}
          options={LAYOUT_OPTIONS}
          onChange={(v) => {
            setSoftAction(() => () => setViewMode(v as GridViewMode));
            gridController.saveViewPref({ view_mode: v });
          }}
          width={130}
        />
      </div>

      {!hasFixedOrder && (
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
      )}
    </div>
  );
}

/** Display toggle panel — shown inside the "Display" submenu. */
function DisplayPanel() {
  const viewMode = useAtomValue(gridViewModeAtom);
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
  const inspectorCollapsed = useAtomValue(inspectorCollapsedAtom);
  const setInspectorCollapsed = useSetAtom(inspectorCollapsedAtom);
  const showSubfolders = useAtomValue(gridShowSubfoldersAtom);
  const setShowSubfolders = useSetAtom(gridShowSubfoldersAtom);

  const toggle = (label: string, on: boolean, flip: () => void, disabled = false) => (
    <div className={`${s.toggleRow} ${disabled ? s.toggleRowDisabled : ''}`} onClick={disabled ? undefined : flip}>
      <div className={s.toggleLabel}>{label}</div>
      <div className={s.toggleValue}><ToggleSwitch on={on} onChange={disabled ? () => {} : flip} /></div>
    </div>
  );

  return (
    <div className={s.panel}>
      {toggle('Show Name', showName, () => { setShowName(!showName); gridController.saveViewPref({ show_name: !showName }); })}
      {toggle('Show Resolution', showRes, () => { setShowRes(!showRes); gridController.saveViewPref({ show_resolution: !showRes }); })}
      {toggle('Show Extension', showExt, () => { setShowExt(!showExt); gridController.saveViewPref({ show_extension: !showExt }); })}
      {toggle('Show Label', showExtLabel, () => { setShowExtLabel(!showExtLabel); gridController.saveViewPref({ show_label: !showExtLabel }); })}
      {toggle('Fit Thumbnails', fitThumbs, () => { setFitThumbs(!fitThumbs); gridController.saveViewPref({ thumbnail_fit: !fitThumbs ? 'cover' : 'contain' }); }, viewMode !== 'grid')}
      {toggle('Show Subfolders', showSubfolders, () => setShowSubfolders(!showSubfolders))}

      <div className={s.sep} />

      {toggle('Show Sidebar', !sidebarCollapsed, () => setSidebarCollapsed(!sidebarCollapsed))}
      {toggle('Show Inspector', !inspectorCollapsed, () => setInspectorCollapsed(!inspectorCollapsed))}
    </div>
  );
}

/** Build entries for the toolbar view menu button (full panel: layout + sort + display toggles). */
export function buildViewMenuEntries(): MenuEntry[] {
  return [
    { custom: true, key: 'view-layout-sort', render: () => <ViewPanel /> },
    { separator: true },
    { custom: true, key: 'display-toggles', render: () => <DisplayPanel /> },
  ];
}

/** Build entries for context menu: layout/sort inline, separator, display as submenu. */
export function buildContextMenuViewEntries(): MenuEntry[] {
  return [
    { custom: true, key: 'view-layout-sort', render: () => <ViewPanel /> },
    { separator: true },
    { submenu: true, label: 'Display', icon: <IconAdjustments size={15} style={{ transform: 'rotate(90deg)' }} />, children: [
      { custom: true, key: 'display-toggles', render: () => <DisplayPanel /> },
    ] },
  ];
}
