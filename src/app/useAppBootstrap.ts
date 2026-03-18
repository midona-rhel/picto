import { useCallback, useEffect, useMemo, useState } from 'react';
import type { MouseEvent as ReactMouseEvent } from 'react';
import { useMantineColorScheme } from '@mantine/core';
import { useHotkeys } from '@mantine/hooks';
import { getCurrentWindow, setTheme as setAppTheme } from '#desktop/api';

import { deriveNavigationTitle, useNavigationStore } from '../state/navigationStore';
import { useDomainStore } from '../state/domainStore';
import { useSettingsStore } from '../state/settingsStore';
import { performRedo, performUndo } from '../shared/controllers/undoRedoController';
import { runBestEffort } from '../shared/lib/asyncOps';
import { useGlobalKeydown } from '../shared/hooks/useGlobalKeydown';
import { useThemeSync } from '../shared/hooks/useThemeSync';
import { useNativeEventListeners } from './useNativeEventListeners';

export interface AppBootstrap {
  appWindow: ReturnType<typeof getCurrentWindow>;
  handleTitlebarMouseDown: (event: ReactMouseEvent<HTMLDivElement>) => void;
  displayedTitle: string;
  handleScopeTransitionMidpoint: () => void;
}

export function useAppBootstrap(): AppBootstrap {
  const activeCollectionId = useNavigationStore((state) => state.activeCollectionId);
  const activeFolderId = useNavigationStore((state) => state.activeFolderId);
  const activeSmartFolderId = useNavigationStore((state) => state.activeSmartFolderId);
  const activeStatusFilter = useNavigationStore((state) => state.activeStatusFilter);
  const currentView = useNavigationStore((state) => state.currentView);
  const filterTags = useNavigationStore((state) => state.filterTags);
  const smartFolders = useDomainStore((state) => state.smartFolders);
  const folderNodes = useDomainStore((state) => state.folderNodes);
  const collectionTitles = useNavigationStore((state) => state.collectionTitles);
  const activeFolderLabel = useMemo(
    () => (activeFolderId != null ? folderNodes.find((node) => node.id === `folder:${activeFolderId}`)?.name ?? null : null),
    [activeFolderId, folderNodes],
  );
  const activeSmartFolder = useMemo(() => {
    if (!activeSmartFolderId) return null;
    const active = smartFolders.find((folder) => folder.id === activeSmartFolderId);
    if (!active) return null;
    return {
      id: active.id,
      name: active.name,
      parent_id: active.parent_id != null ? Number(active.parent_id) : null,
      icon: active.icon ?? null,
      color: active.color ?? null,
      predicate: active.predicate ?? active.localPredicate ?? { groups: [] },
      sort_field: active.sort_field ?? null,
      sort_order: active.sort_order ?? null,
    };
  }, [activeSmartFolderId, smartFolders]);
  const activeCollectionLabel = activeCollectionId != null
    ? collectionTitles[activeCollectionId] ?? `Collection ${activeCollectionId}`
    : null;
  const titlebarTitle = useMemo(() => deriveNavigationTitle({
    activeCollectionId,
    activeCollectionLabel,
    activeFolderLabel,
    activeSmartFolderLabel: activeSmartFolder?.name ?? null,
    activeStatusFilter,
    currentView,
    filterTags,
  }), [activeCollectionId, activeCollectionLabel, activeFolderId, activeFolderLabel, activeSmartFolder?.name, activeStatusFilter, currentView, filterTags]);
  const { colorScheme } = useMantineColorScheme();
  const appWindow = useMemo(() => getCurrentWindow(), []);
  const isSystemDark = colorScheme === 'dark';

  // ── Theme sync (settings init + Mantine color scheme + DOM attribute) ──
  useThemeSync();

  // ── Native event listeners (library lifecycle, menu events, runtime init) ──
  useNativeEventListeners();

  const [displayedTitle, setDisplayedTitle] = useState(titlebarTitle);
  const handleScopeTransitionMidpoint = useCallback(() => {
    const state = useNavigationStore.getState();
    const domainState = useDomainStore.getState();
    const activeSmartFolder = domainState.smartFolders.find((folder) => folder.id === state.activeSmartFolderId);
    setDisplayedTitle(deriveNavigationTitle({
      activeCollectionId: state.activeCollectionId,
      activeCollectionLabel: state.activeCollectionId != null
        ? state.collectionTitles[state.activeCollectionId] ?? `Collection ${state.activeCollectionId}`
        : null,
      activeFolderLabel: state.activeFolderId != null
        ? domainState.folderNodes.find((node) => node.id === `folder:${state.activeFolderId}`)?.name ?? null
        : null,
      activeSmartFolderLabel: activeSmartFolder?.name ?? null,
      activeStatusFilter: state.activeStatusFilter,
      currentView: state.currentView,
      filterTags: state.filterTags,
    }));
  }, []);
  useEffect(() => {
    setDisplayedTitle(titlebarTitle);
  }, [titlebarTitle]);

  // ── One-time startup: show window ──
  useEffect(() => {
    runBestEffort('startup.windowShow', appWindow.show());
  }, [appWindow]);

  // ── Native theme sync (Electron window + app-level) ──
  useEffect(() => {
    const theme = isSystemDark ? 'dark' : 'light';
    void Promise.all([
      setAppTheme(theme),
      appWindow.setTheme(theme),
    ]).catch((err) => console.debug('Failed to apply native theme:', err));
  }, [appWindow, isSystemDark]);

  const handleTitlebarMouseDown = useCallback((event: ReactMouseEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    const target = event.target as HTMLElement | null;
    if (!target) return;
    // Exclude interactive elements and explicit no-drag regions
    if (target.closest('button, a, input, select, textarea, .no-drag-region')) return;
    event.preventDefault();
    if (event.detail === 2) {
      void appWindow.toggleMaximize();
      return;
    }
    void appWindow.startDragging();
  }, [appWindow]);

  useHotkeys([
    ['mod+alt+1', () => {
      const s = useSettingsStore.getState();
      s.updateSetting('showSidebar', !s.settings.showSidebar);
    }],
    ['mod+alt+2', () => {
      const s = useSettingsStore.getState();
      s.updateSetting('showInspector', !s.settings.showInspector);
    }],
    ['tab', () => {
      const s = useSettingsStore.getState();
      const bothVisible = s.settings.showSidebar && s.settings.showInspector;
      s.updateSetting('showSidebar', !bothVisible);
      s.updateSetting('showInspector', !bothVisible);
    }],
    ['mod+alt+4', () => {
      const s = useSettingsStore.getState();
      s.updateSetting('showTileName', !s.settings.showTileName);
    }],
    ['mod+alt+5', () => {
      const s = useSettingsStore.getState();
      const allMeta = s.settings.showResolution && s.settings.showExtension;
      s.updateSetting('showResolution', !allMeta);
      s.updateSetting('showExtension', !allMeta);
    }],
  ]);

  const handleUndoRedoKeydown = useCallback((event: KeyboardEvent) => {
    const isEditableTarget = (target: EventTarget | null): boolean => {
      if (!(target instanceof HTMLElement)) return false;
      if (target.isContentEditable) return true;
      const tag = target.tagName;
      return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT';
    };

    if (isEditableTarget(event.target)) return;
    if (event.altKey) return;

    const isMacPlatform = /Mac|iPhone|iPad/.test(navigator.platform);
    const key = event.key.toLowerCase();
    const modPressed = isMacPlatform ? event.metaKey : event.ctrlKey;
    if (!modPressed) return;

    const isUndo = key === 'z' && !event.shiftKey;
    const isRedo = (key === 'z' && event.shiftKey) || (!isMacPlatform && key === 'y');
    if (!isUndo && !isRedo) return;

    event.preventDefault();
    if (isUndo) {
      void performUndo();
    } else {
      void performRedo();
    }
  }, []);
  useGlobalKeydown(handleUndoRedoKeydown);

  return { appWindow, handleTitlebarMouseDown, displayedTitle, handleScopeTransitionMidpoint };
}
