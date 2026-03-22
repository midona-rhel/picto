import { useEffect } from 'react';
import { listen } from '#desktop/api';
import { useDomainStore } from '../state/domainStore';
import { useImportActionStore } from '../state/importActionStore';
import { useStateChangeStore } from '../runtime/stateChanges/stateChangeStore';
import { useLibraryStore } from '../state/libraryStore';
import { useExportActionStore } from '../state/exportActionStore';
import { useTaskStore } from '../state/taskStore';
import { importController } from '../controllers/importController';
import { exportController } from '../controllers/exportController';
import { useNavigationStore, type ViewType } from '../state/navigationStore';
import { startAllStateChangeAppliers, stopAllStateChangeAppliers } from '../runtime/refresherOrchestrator';
import { useSubscriptionProgressStore } from '../state/taskStore';
import { performUndo, performRedo } from '../shared/controllers/undoRedoController';
import { useUpdaterStore } from '../state/updaterStore';
import { useLogStore } from '../state/logStore';
import { runBestEffort } from '../shared/lib/asyncOps';
import type { ResourceKey } from '../shared/types/backendState';
import type { ManualImportProgressEvent, MediaExportProgressEvent } from '../shared/types/api/events';
import { windowController } from '../controllers/windowController';

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
    void useDomainStore.getState().fetchSidebarTree();
    void useStateChangeStore.getState().ensureInitialized();
    useSubscriptionProgressStore.getState().start();
    startAllStateChangeAppliers();

    // Library lifecycle listeners (previously in eventBridge)
    const libraryListeners = Promise.all([
      listen('library-switching', () => {
        useLibraryStore.getState().setSwitching(true);
        useTaskStore.getState().setLibrarySwitching(true);
      }),
      listen('library-switched', () => {
        useTaskStore.getState().setLibrarySwitching(false);
        useStateChangeStore.getState().queueRefreshTargets([
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
      stopAllStateChangeAppliers();
      useSubscriptionProgressStore.getState().stop();
      useStateChangeStore.getState().teardown();
      runBestEffort('cleanup.libraryListeners', libraryListeners.then((fns) => { for (const fn of fns) fn(); }));
    };
  }, []);

  useEffect(() => {
    const unlisten = listen('menu:open-settings', () => {
      runBestEffort('menu.openSettingsWindow', windowController.openSettings());
    });
    return () => { runBestEffort('menu.unlistenOpenSettings', unlisten.then((fn) => fn())); };
  }, []);

  useEffect(() => {
    const unlisten = listen('menu:import-files', () => {
      if (useNavigationStore.getState().currentView !== 'images') {
        useNavigationStore.getState().navigateTo('images');
      }
      useImportActionStore.getState().requestImportFilesDialog();
    });
    return () => { runBestEffort('menu.unlistenImportFiles', unlisten.then((fn) => fn())); };
  }, []);

  useEffect(() => {
    const unlisten = listen('menu:import-folder', () => {
      if (useNavigationStore.getState().currentView !== 'images') {
        useNavigationStore.getState().navigateTo('images');
      }
      useImportActionStore.getState().requestImportFolderDialog();
    });
    return () => { runBestEffort('menu.unlistenImportFolder', unlisten.then((fn) => fn())); };
  }, []);

  useEffect(() => {
    const unlisten = listen('menu:export-basic', () => {
      if (useNavigationStore.getState().currentView !== 'images') {
        useNavigationStore.getState().navigateTo('images');
      }
      useExportActionStore.getState().requestBasicExport();
    });
    return () => { runBestEffort('menu.unlistenExportBasic', unlisten.then((fn) => fn())); };
  }, []);

  useEffect(() => {
    const unlisten = listen('menu:export-advanced', () => {
      if (useNavigationStore.getState().currentView !== 'images') {
        useNavigationStore.getState().navigateTo('images');
      }
      useExportActionStore.getState().requestAdvancedExport();
    });
    return () => { runBestEffort('menu.unlistenExportAdvanced', unlisten.then((fn) => fn())); };
  }, []);

  useEffect(() => {
    const unlisten = listen<ManualImportProgressEvent>('manual-import-progress', (event) => {
      const p = event.payload;
      importController.updateProgress({
        done: p.done,
        total: p.total,
        statusText: p.current_file,
        imported: p.imported,
        skipped: p.skipped,
        errors: p.errors,
      });
      if (p.done >= p.total && p.total > 0) {
        importController.finish();
      }
    });
    return () => { runBestEffort('manualImportProgress.unlisten', unlisten.then((fn) => fn())); };
  }, []);

  useEffect(() => {
    const unlisten = listen<MediaExportProgressEvent>('media-export-progress', (event) => {
      const p = event.payload;
      exportController.updateProgress({
        done: p.done,
        total: p.total,
        statusText: p.current_file,
        exported: p.exported,
        skipped: p.skipped,
        errors: p.errors,
      });
      if (p.done >= p.total && p.total > 0) {
        exportController.finish();
      }
    });
    return () => { runBestEffort('mediaExportProgress.unlisten', unlisten.then((fn) => fn())); };
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

  // Backend log forwarding
  useEffect(() => {
    const unlisten = listen<{ level: string; target: string; message: string; timestamp: string }>('log', (event) => {
      const { level, target, message, timestamp } = event.payload;
      useLogStore.getState().addEntry({
        level: level as 'TRACE' | 'DEBUG' | 'INFO' | 'WARN' | 'ERROR',
        target,
        message,
        timestamp,
      });
    });
    return () => { runBestEffort('log.unlisten', unlisten.then((fn) => fn())); };
  }, []);

  // Auto-updater status listener
  useEffect(() => {
    const unlisten = window.picto?.updater?.onStatus((event) => {
      useUpdaterStore.getState().handleStatusEvent(event);
    });
    return () => { unlisten?.(); };
  }, []);
}
