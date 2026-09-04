import { getDefaultStore } from 'jotai';
import { getUpdateState, onUpdateState, type UpdateState } from '../platform/updateApi';
import { updateModalAtom } from '../state/modals';
import { showInfoNotification, showSuccessNotification } from '../shared/lib/notifications';
import { t } from '../i18n';

const store = getDefaultStore();

export function startUpdateRuntime(): () => void {
  let dispose: (() => void) | undefined;
  let disposed = false;
  let eventsReceived = 0;
  let announced = '';
  const announce = (state: UpdateState) => {
    if (disposed || !state.version) return;
    const key = `${state.status}:${state.version}`;
    if (announced === key) return;
    if (state.status === 'installed') {
      announced = key;
      store.set(updateModalAtom, { open: true });
    } else if (state.status === 'available') {
      announced = key;
      showInfoNotification({
        title: t("Picto {value0} is available", { value0: state.version }),
        message: state.platform === 'darwin'
          ? 'Open the release notes to download it.'
          : t("Updates download in the background and install after Picto closes."),
        duration: 10_000,
        action: { label: t("View"), onClick: () => store.set(updateModalAtom, { open: true }) },
      });
    } else if (state.status === 'downloaded') {
      announced = key;
      showSuccessNotification({ title: t("Picto {value0} is ready", { value0: state.version }), message: 'Restart to finish updating.', duration: 10_000, action: { label: t("View"), onClick: () => store.set(updateModalAtom, { open: true }) } });
    }
  };
  void onUpdateState((state) => { eventsReceived += 1; announce(state); }).then((value) => {
    if (disposed) value(); else dispose = value;
  }).catch((error) => console.error('[updates] Could not subscribe to update state', error));
  void getUpdateState().then((state) => { if (eventsReceived === 0) announce(state); }).catch(() => {});
  return () => { disposed = true; dispose?.(); };
}
