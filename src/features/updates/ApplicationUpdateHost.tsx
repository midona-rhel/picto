import { useEffect } from 'react';
import { useAtomValue, useSetAtom } from 'jotai';
import { listen } from '../../platform/ipc';
import { startUpdateRuntime } from '../../runtime/updateRuntime';
import { updateModalAtom } from '../../state/modals';
import { UpdateModal } from '../modals/UpdateModal';

export function ApplicationUpdateHost() {
  const modal = useAtomValue(updateModalAtom);
  const setModal = useSetAtom(updateModalAtom);

  useEffect(() => {
    const stopRuntime = startUpdateRuntime();
    let disposed = false;
    let stopMenu: (() => void) | undefined;
    void listen('menu:show-updates', () => setModal({ open: true })).then((stop) => {
      if (disposed) stop();
      else stopMenu = stop;
    });
    return () => {
      disposed = true;
      stopMenu?.();
      stopRuntime();
    };
  }, [setModal]);

  return <UpdateModal open={modal.open} onClose={() => setModal({ open: false })} />;
}
