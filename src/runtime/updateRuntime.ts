import { getDefaultStore } from 'jotai';
import { getUpdateState, onUpdateState, type UpdateState } from '../platform/updateApi';
import { updateModalAtom } from '../state/modals';
import { showInfoNotification, showSuccessNotification } from '../shared/lib/notifications';

const store = getDefaultStore();

export function startUpdateRuntime(): () => void {
  let dispose: (() => void) | undefined;
  let announced = '';
  const announce = (state: UpdateState) => {
    if (!state.version) return;
    const key = `${state.status}:${state.version}`;
    if (announced === key) return;
    if (state.status === 'available' && state.platform === 'darwin') {
      announced = key;
      showInfoNotification({ title: `Picto ${state.version} is available`, message: 'Open the release notes to download it.', duration: 10_000, action: { label: 'View', onClick: () => store.set(updateModalAtom, { open: true }) } });
    } else if (state.status === 'downloaded') {
      announced = key;
      showSuccessNotification({ title: `Picto ${state.version} is ready`, message: 'Restart to finish updating.', duration: 10_000, action: { label: 'View', onClick: () => store.set(updateModalAtom, { open: true }) } });
    }
  };
  void getUpdateState().then(announce).catch(() => {});
  void onUpdateState(announce).then((value) => { dispose = value; });
  return () => dispose?.();
}
