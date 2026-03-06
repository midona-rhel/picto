import { useCallback, useEffect, useMemo, useState } from 'react';
import type { MouseEvent as ReactMouseEvent } from 'react';
import { useMantineColorScheme } from '@mantine/core';
import { useHotkeys } from '@mantine/hooks';
import { getCurrentWindow, setTheme as setAppTheme, api, listen } from '#desktop/api';

import { registerCacheCleanup } from '../utils/cacheCleanup';
import { useNavigationStore, type ViewType } from '../stores/navigationStore';
import { initSettingsStore, themeToColorScheme, useSettingsStore } from '../stores/settingsStore';
import { SidebarController } from '../controllers/sidebarController';
import { performRedo, performUndo } from '../controllers/undoRedoController';
import { setupEventBridge, teardownEventBridge } from '../stores/eventBridge';
import { useTaskRuntimeStore } from '../stores/taskRuntimeStore';
import { runBestEffort } from '../lib/asyncOps';

export interface AppBootstrap {
  appWindow: ReturnType<typeof getCurrentWindow>;
  handleTitlebarMouseDown: (event: ReactMouseEvent<HTMLDivElement>) => void;
  displayedTitle: string;
  handleScopeTransitionMidpoint: () => void;
}

export function useAppBootstrap(): AppBootstrap {
  const { titlebarTitle, currentView } = useNavigationStore();
  const { settings, loaded: settingsLoaded } = useSettingsStore();
  const { colorScheme, setColorScheme } = useMantineColorScheme();
  const appWindow = useMemo(() => getCurrentWindow(), []);
  const isSystemDark = colorScheme === 'dark';

  // Displayed title lags behind titlebarTitle — only updates when grid fade-out completes.
  const [displayedTitle, setDisplayedTitle] = useState(titlebarTitle);
  const handleScopeTransitionMidpoint = useCallback(() => {
    setDisplayedTitle(useNavigationStore.getState().titlebarTitle);
  }, []);
  useEffect(() => {
    if (currentView !== 'images') setDisplayedTitle(titlebarTitle);
  }, [titlebarTitle, currentView]);

  useEffect(() => {
    void initSettingsStore();
  }, []);

  useEffect(() => {
    if (!settingsLoaded) return;
    const theme = settings.theme ?? (settings.colorScheme === 'light' ? 'light' : 'dark');
    const scheme = themeToColorScheme(theme);
    if (scheme !== colorScheme) setColorScheme(scheme);
    document.documentElement.dataset.theme = theme === 'auto' ? '' : theme;
  }, [settingsLoaded, settings.theme]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    registerCacheCleanup();
    runBestEffort('startup.enableModernWindowStyle', api.os.enableModernWindowStyle(4.0));
    runBestEffort('startup.windowShow', appWindow.show());
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

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

  useEffect(() => {
    void SidebarController.fetchInitialTree();
    setupEventBridge();
    void useTaskRuntimeStore.getState().ensureInitialized();
    return () => {
      teardownEventBridge();
      useTaskRuntimeStore.getState().teardown();
    };
  }, []);

  useEffect(() => {
    const unlisten = listen('menu:open-settings', () => {
      runBestEffort('menu.openSettingsWindow', api.os.openSettingsWindow());
    });
    return () => { runBestEffort('menu.unlistenOpenSettings', unlisten.then((fn) => fn())); };
  }, []);

  useEffect(() => {
    const unlisten = listen<string>('menu:navigate', (event) => {
      const view = event.payload as ViewType | undefined;
      if (view) useNavigationStore.getState().navigateTo(view);
    });
    return () => { runBestEffort('menu.unlistenNavigate', unlisten.then((fn) => fn())); };
  }, []);

  useEffect(() => {
    const unlistenUndo = listen('menu:undo', () => {
      void performUndo();
    });
    const unlistenRedo = listen('menu:redo', () => {
      void performRedo();
    });
    return () => {
      runBestEffort('menu.unlistenUndo', unlistenUndo.then((fn) => fn()));
      runBestEffort('menu.unlistenRedo', unlistenRedo.then((fn) => fn()));
    };
  }, []);

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

  useEffect(() => {
    const isEditableTarget = (target: EventTarget | null): boolean => {
      if (!(target instanceof HTMLElement)) return false;
      if (target.isContentEditable) return true;
      const tag = target.tagName;
      return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT';
    };

    const isMac = /Mac|iPhone|iPad/.test(navigator.platform);
    const onKeyDown = (event: KeyboardEvent) => {
      if (isEditableTarget(event.target)) return;
      if (event.altKey) return;

      const key = event.key.toLowerCase();
      const modPressed = isMac ? event.metaKey : event.ctrlKey;
      if (!modPressed) return;

      const isUndo = key === 'z' && !event.shiftKey;
      const isRedo = (key === 'z' && event.shiftKey) || (!isMac && key === 'y');
      if (!isUndo && !isRedo) return;

      event.preventDefault();
      if (isUndo) {
        void performUndo();
      } else {
        void performRedo();
      }
    };

    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, []);

  return { appWindow, handleTitlebarMouseDown, displayedTitle, handleScopeTransitionMidpoint };
}
