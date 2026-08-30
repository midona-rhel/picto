import { getDefaultStore } from 'jotai';
import { chooseAndImportFiles, chooseAndImportFolder, filesController } from '../controllers/filesController';
import { listen } from '../platform/ipc';
import { gridScopeAtom } from '../state/grid';
import { exportModalAtom } from '../state/modals';
import { updateModalAtom } from '../state/modals';
import { selectionCountAtom, selectionTargetAtom } from '../state/selection';
import { showErrorNotification, showInfoNotification, showSuccessNotification } from '../shared/lib/notifications';

type ApplicationMenuEvent =
  | 'menu:import-files'
  | 'menu:import-folder'
  | 'menu:export-basic'
  | 'menu:export-advanced'
  | 'menu:show-updates';

const store = getDefaultStore();

function selectedExport() {
  return {
    count: store.get(selectionCountAtom),
    target: store.get(selectionTargetAtom),
  };
}

async function exportOriginals(): Promise<void> {
  const { count, target } = selectedExport();
  if (!target || count === 0) {
    showInfoNotification({ title: 'Select files to export', message: '' });
    return;
  }
  const result = await (window as any).picto.dialog.open({
    properties: ['openDirectory'],
    multiple: false,
    title: 'Choose export folder',
  });
  if (!result) return;
  const outputDir = typeof result === 'string' ? result : result[0];
  if (!outputDir) return;
  await filesController.exportMedia(target, { output_dir: outputDir, format: 'original' });
  showSuccessNotification({
    title: `Exported ${count} file${count === 1 ? '' : 's'}`,
    message: '',
  });
}

async function runMenuAction(name: ApplicationMenuEvent): Promise<void> {
  if (name === 'menu:show-updates') {
    store.set(updateModalAtom, { open: true });
    return;
  }
  if (name === 'menu:import-files') {
    await chooseAndImportFiles(store.get(gridScopeAtom));
    return;
  }
  if (name === 'menu:import-folder') {
    await chooseAndImportFolder(store.get(gridScopeAtom));
    return;
  }
  if (name === 'menu:export-basic') {
    await exportOriginals();
    return;
  }
  const { count, target } = selectedExport();
  if (!target || count === 0) {
    showInfoNotification({ title: 'Select files to export', message: '' });
    return;
  }
  store.set(exportModalAtom, { open: true, fileCount: count, target });
}

export function startApplicationMenuRuntime(): () => void {
  let disposed = false;
  const disposers: Array<() => void> = [];
  const names: ApplicationMenuEvent[] = [
    'menu:import-files',
    'menu:import-folder',
    'menu:export-basic',
    'menu:export-advanced',
    'menu:show-updates',
  ];
  for (const name of names) {
    void listen(name, () => {
      void runMenuAction(name).catch((error) => {
        showErrorNotification({
          title: 'Menu action failed',
          message: error instanceof Error ? error.message : String(error),
        });
      });
    }).then((dispose) => {
      if (disposed) dispose();
      else disposers.push(dispose);
    }).catch((error) => {
      console.error(`Failed to subscribe to ${name}`, error);
    });
  }
  return () => {
    disposed = true;
    for (const dispose of disposers) dispose();
  };
}
