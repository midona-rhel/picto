import { getDefaultStore } from 'jotai';
import { openPictoPackImport } from '../controllers/filesController';
import { claimAssociatedPictoPack } from '../platform/associatedFilesApi';
import { showErrorNotification } from '../shared/lib/notifications';
import { pictoPackModalAtom } from '../state/modals';
import { listen } from '../platform/ipc';
import { t } from '../i18n';

const store = getDefaultStore();

export function startAssociatedFilesRuntime(): () => void {
  let disposed = false;
  let draining = false;
  const disposers: Array<() => void> = [];

  const drain = async () => {
    if (disposed || draining || store.get(pictoPackModalAtom).open) return;
    draining = true;
    let claimed = false;
    try {
      const path = await claimAssociatedPictoPack();
      if (!path || disposed) return;
      claimed = true;
      await openPictoPackImport(path);
    } catch (reason) {
      claimed = true;
      showErrorNotification({
        title: t("Could not import Picto Pack"),
        message: reason instanceof Error ? reason.message : String(reason),
      });
    } finally {
      draining = false;
      if (claimed && !disposed && !store.get(pictoPackModalAtom).open) queueMicrotask(() => { void drain(); });
    }
  };

  disposers.push(store.sub(pictoPackModalAtom, () => { void drain(); }));
  void listen('picto:associated-file-queued', () => { void drain(); }).then((dispose) => {
    if (disposed) dispose();
    else disposers.push(dispose);
  });
  void drain();

  return () => {
    disposed = true;
    for (const dispose of disposers) dispose();
  };
}
