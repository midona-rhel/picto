import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import { ViewPrefsController, type ViewPrefsPatch } from '../controllers/viewPrefsController';
import type { AppSettings } from '../stores/settingsStore';
import type { GridViewMode } from '#features/grid/types';

export type ThumbnailFitMode = 'contain' | 'cover';

export interface DisplayOptions {
  showTileName: boolean;
  showResolution: boolean;
  showExtension: boolean;
  showExtensionLabel: boolean;
  thumbnailFitMode: ThumbnailFitMode;
}

type UseScopedGridPreferencesParams = {
  currentView: string;
  activeFolderId: number | null;
  activeSmartFolderId: string | null;
  activeStatusFilter: string | null;
  settingsLoaded: boolean;
  defaultGridViewMode: GridViewMode;
  defaultGridTargetSize: number;
  defaultSortField: AppSettings['gridSortField'];
  defaultSortOrder: AppSettings['gridSortOrder'];
  defaultDisplayOptions: DisplayOptions;
};

type UseScopedGridPreferencesResult = {
  gridScopeKey: string | null;
  gridViewMode: GridViewMode;
  gridTargetSize: number;
  gridSortField: AppSettings['gridSortField'];
  gridSortOrder: AppSettings['gridSortOrder'];
  displayOptions: DisplayOptions;
  handleGridViewModeChange: (mode: GridViewMode) => void;
  handleGridTargetSizeChange: (size: number) => void;
  handleGridSortFieldChange: (field: AppSettings['gridSortField']) => void;
  handleGridSortOrderChange: (order: AppSettings['gridSortOrder']) => void;
  handleDisplayOptionChange: <K extends keyof DisplayOptions>(key: K, value: DisplayOptions[K]) => void;
};

const VALID_VIEW_MODES = new Set<string>(['grid', 'waterfall', 'justified']);

export function useScopedGridPreferences({
  currentView,
  activeFolderId,
  activeSmartFolderId,
  activeStatusFilter,
  settingsLoaded,
  defaultGridViewMode,
  defaultGridTargetSize,
  defaultSortField,
  defaultSortOrder,
  defaultDisplayOptions,
}: UseScopedGridPreferencesParams): UseScopedGridPreferencesResult {
  const [scopedGridViewMode, setScopedGridViewMode] = useState<GridViewMode | null>(null);
  const [scopedGridTargetSize, setScopedGridTargetSize] = useState<number | null>(null);
  const [scopedGridSortField, setScopedGridSortField] = useState<AppSettings['gridSortField'] | null>(null);
  const [scopedGridSortOrder, setScopedGridSortOrder] = useState<AppSettings['gridSortOrder'] | null>(null);
  const [scopedDisplay, setScopedDisplay] = useState<DisplayOptions | null>(null);
  const [scopeFallback, setScopeFallback] = useState<{
    viewMode: GridViewMode;
    targetSize: number;
    sortField: AppSettings['gridSortField'];
    sortOrder: AppSettings['gridSortOrder'];
    display: DisplayOptions;
  }>({
    viewMode: defaultGridViewMode,
    targetSize: defaultGridTargetSize,
    sortField: defaultSortField,
    sortOrder: defaultSortOrder,
    display: defaultDisplayOptions,
  });

  const viewPrefsLoadSeq = useRef(0);
  const saveViewPrefsTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingViewPrefsPatch = useRef<ViewPrefsPatch>({});
  const prevScopeKeyRef = useRef<string | null>(null);

  const gridScopeKey = useMemo(() => {
    if (currentView !== 'images') return null;
    if (activeFolderId) return `folder:${activeFolderId}`;
    if (activeSmartFolderId) return `smart:${activeSmartFolderId}`;
    if (activeStatusFilter === 'inbox') return 'system:inbox';
    if (activeStatusFilter === 'uncategorized') return 'system:uncategorized';
    if (activeStatusFilter === 'trash') return 'system:trash';
    return 'system:all';
  }, [currentView, activeFolderId, activeSmartFolderId, activeStatusFilter]);

  const effectiveGridViewMode = scopedGridViewMode ?? scopeFallback.viewMode;
  const effectiveGridTargetSize = scopedGridTargetSize ?? scopeFallback.targetSize;
  const effectiveGridSortField = scopedGridSortField ?? scopeFallback.sortField;
  const effectiveGridSortOrder = scopedGridSortOrder ?? scopeFallback.sortOrder;
  const effectiveDisplay = scopedDisplay ?? scopeFallback.display;
  const effectiveGridViewModeRef = useRef(effectiveGridViewMode);
  effectiveGridViewModeRef.current = effectiveGridViewMode;
  const effectiveGridTargetSizeRef = useRef(effectiveGridTargetSize);
  effectiveGridTargetSizeRef.current = effectiveGridTargetSize;
  const effectiveGridSortFieldRef = useRef(effectiveGridSortField);
  effectiveGridSortFieldRef.current = effectiveGridSortField;
  const effectiveGridSortOrderRef = useRef(effectiveGridSortOrder);
  effectiveGridSortOrderRef.current = effectiveGridSortOrder;
  const effectiveDisplayRef = useRef(effectiveDisplay);
  effectiveDisplayRef.current = effectiveDisplay;

  const persistViewPrefs = useCallback((patch: ViewPrefsPatch) => {
    if (!gridScopeKey) return;
    pendingViewPrefsPatch.current = { ...pendingViewPrefsPatch.current, ...patch };
    if (saveViewPrefsTimer.current) clearTimeout(saveViewPrefsTimer.current);
    saveViewPrefsTimer.current = setTimeout(() => {
      const mergedPatch = pendingViewPrefsPatch.current;
      pendingViewPrefsPatch.current = {};
      void ViewPrefsController.set(gridScopeKey, mergedPatch).catch((e) => {
        console.error('Failed to persist view prefs:', e);
      });
    }, 180);
  }, [gridScopeKey]);

  useEffect(() => {
    if (!settingsLoaded || !gridScopeKey) return;
    const scopeChanged = prevScopeKeyRef.current !== gridScopeKey;
    if (scopeChanged) {
      // Latch geometry/order at scope entry so async settings/view-pref hydration
      // cannot briefly flash a different target size (waterfall single-tile flicker).
      setScopeFallback({
        viewMode: effectiveGridViewModeRef.current,
        targetSize: effectiveGridTargetSizeRef.current,
        sortField: effectiveGridSortFieldRef.current,
        sortOrder: effectiveGridSortOrderRef.current,
        display: effectiveDisplayRef.current,
      });
      setScopedGridViewMode(null);
      setScopedGridTargetSize(null);
      setScopedGridSortField(null);
      setScopedGridSortOrder(null);
      setScopedDisplay(null);
      prevScopeKeyRef.current = gridScopeKey;
    }

    const seq = ++viewPrefsLoadSeq.current;
    pendingViewPrefsPatch.current = {};
    if (saveViewPrefsTimer.current) {
      clearTimeout(saveViewPrefsTimer.current);
      saveViewPrefsTimer.current = null;
    }

    void ViewPrefsController.get(gridScopeKey)
      .then((pref) => {
        if (viewPrefsLoadSeq.current !== seq) return;
        const nextViewMode =
          VALID_VIEW_MODES.has(pref?.view_mode ?? '')
            ? (pref!.view_mode as GridViewMode)
            : defaultGridViewMode;
        const nextTargetSize =
          typeof pref?.target_size === 'number' ? pref.target_size : defaultGridTargetSize;
        const nextSortField =
          (pref?.sort_field as AppSettings['gridSortField'] | null | undefined) ?? defaultSortField;
        const nextSortOrder =
          (pref?.sort_order as AppSettings['gridSortOrder'] | null | undefined) ?? defaultSortOrder;

        const nextDisplay: DisplayOptions = {
          showTileName: pref?.show_name ?? defaultDisplayOptions.showTileName,
          showResolution: pref?.show_resolution ?? defaultDisplayOptions.showResolution,
          showExtension: pref?.show_extension ?? defaultDisplayOptions.showExtension,
          showExtensionLabel: pref?.show_label ?? defaultDisplayOptions.showExtensionLabel,
          thumbnailFitMode: (pref?.thumbnail_fit === 'contain' || pref?.thumbnail_fit === 'cover')
            ? pref.thumbnail_fit
            : defaultDisplayOptions.thumbnailFitMode,
        };

        setScopedGridViewMode(nextViewMode);
        setScopedGridTargetSize(nextTargetSize);
        setScopedGridSortField(nextSortField);
        setScopedGridSortOrder(nextSortOrder);
        setScopedDisplay(nextDisplay);
        setScopeFallback({
          viewMode: nextViewMode,
          targetSize: nextTargetSize,
          sortField: nextSortField,
          sortOrder: nextSortOrder,
          display: nextDisplay,
        });
      })
      .catch((e) => console.error('Failed to load view prefs:', e));
  }, [
    settingsLoaded,
    gridScopeKey,
    defaultGridViewMode,
    defaultGridTargetSize,
    defaultSortField,
    defaultSortOrder,
    defaultDisplayOptions,
  ]);

  useEffect(() => {
    return () => {
      if (saveViewPrefsTimer.current) clearTimeout(saveViewPrefsTimer.current);
      pendingViewPrefsPatch.current = {};
    };
  }, []);

  const handleGridViewModeChange = useCallback((mode: GridViewMode) => {
    setScopedGridViewMode(mode);
    persistViewPrefs({ view_mode: mode });
  }, [persistViewPrefs]);

  const handleGridTargetSizeChange = useCallback((size: number) => {
    setScopedGridTargetSize(size);
    persistViewPrefs({ target_size: size });
  }, [persistViewPrefs]);

  const handleGridSortFieldChange = useCallback((field: AppSettings['gridSortField']) => {
    setScopedGridSortField(field);
    persistViewPrefs({ sort_field: field });
  }, [persistViewPrefs]);

  const handleGridSortOrderChange = useCallback((order: AppSettings['gridSortOrder']) => {
    setScopedGridSortOrder(order);
    persistViewPrefs({ sort_order: order });
  }, [persistViewPrefs]);

  const DISPLAY_KEY_TO_PATCH: Record<string, string> = {
    showTileName: 'show_name',
    showResolution: 'show_resolution',
    showExtension: 'show_extension',
    showExtensionLabel: 'show_label',
    thumbnailFitMode: 'thumbnail_fit',
  };

  const handleDisplayOptionChange = useCallback(<K extends keyof DisplayOptions>(key: K, value: DisplayOptions[K]) => {
    setScopedDisplay((prev) => {
      const base = prev ?? defaultDisplayOptions;
      return { ...base, [key]: value };
    });
    const patchKey = DISPLAY_KEY_TO_PATCH[key];
    if (patchKey) {
      persistViewPrefs({ [patchKey]: value } as ViewPrefsPatch);
    }
  }, [persistViewPrefs, defaultDisplayOptions]);

  return {
    gridScopeKey,
    gridViewMode: effectiveGridViewMode,
    gridTargetSize: effectiveGridTargetSize,
    gridSortField: effectiveGridSortField,
    gridSortOrder: effectiveGridSortOrder,
    displayOptions: effectiveDisplay,
    handleGridViewModeChange,
    handleGridTargetSizeChange,
    handleGridSortFieldChange,
    handleGridSortOrderChange,
    handleDisplayOptionChange,
  };
}
