import { useEffect } from 'react';
import { api, listen } from '#desktop/api';
import { SidebarController } from '../shared/controllers/sidebarController';
import { useRuntimeSyncStore } from '../state/runtimeSyncStore';
import { useLibraryStore } from '../state/libraryStore';
import { useNavigationStore, type ViewType } from '../state/navigationStore';
import { startAllRefreshers, stopAllRefreshers } from '../runtime/refresherOrchestrator';
import { performUndo, performRedo } from '../shared/controllers/undoRedoController';
import { runBestEffort } from '../shared/lib/asyncOps';
import type { ResourceKey } from '../shared/types/generated/runtime-contract';

/**
 * Consolidates all native event listeners and runtime init/teardown
 * that were previously scattered across useAppBootstrap.
 *
 * Owns:
 * - Sidebar initial fetch + runtime sync init + refresher lifecycle
 * - Library switching/switched listeners
 * - Menu event listeners (open-settings, navigate, undo, redo)
 */
export function useNativeEventListeners(): void {
  useEffect(() => {
    void SidebarController.fetchInitialTree();
    void useRuntimeSyncStore.getState().ensureInitialized();
    startAllRefreshers();

    // Library lifecycle listeners (previously in eventBridge)
    const libraryListeners = Promise.all([
      listen('library-switching', () => {
        useLibraryStore.getState().setSwitching(true);
      }),
      listen('library-switched', () => {
        useRuntimeSyncStore.getState().markResourcesStale([
          'sidebar/tree' as ResourceKey,
          'sidebar/counts' as ResourceKey,
          'grid/system:all' as ResourceKey,
          'selection/current' as ResourceKey,
        ]);
        useLibraryStore.getState().setSwitching(false);
        useLibraryStore.getState().loadConfig();
      }),
    ]);
    return () => {
      stopAllRefreshers();
      useRuntimeSyncStore.getState().teardown();
      runBestEffort('cleanup.libraryListeners', libraryListeners.then((fns) => { for (const fn of fns) fn(); }));
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
}
