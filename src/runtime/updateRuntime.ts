import { getDefaultStore } from 'jotai';
import { getUpdateState, onUpdateState, type UpdateState } from '../platform/updateApi';
import { updateModalAtom } from '../state/modals';
import { showInfoNotification, showSuccessNotification } from '../shared/lib/notifications';
import { t } from '../i18n';

const store = getDefaultStore();

export function startUpdateRuntime(): () => void {
  let dispose: (() => void) | undefined;
  let announced = '';
  const announce = (state: UpdateState) => {
    if (!state.version) return;
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
  void getUpdateState().then(announce).catch(() => {});
  void onUpdateState(announce).then((value) => { dispose = value; });
  return () => dispose?.();
}
