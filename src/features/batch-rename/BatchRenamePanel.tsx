/**
 * BatchRenamePanel — floating glass panel for batch rename operations.
 * TODO: Wire to backend rename command, add template/regex modes, live preview.
 */

import { useState } from 'react';
import { useAtomValue, useSetAtom } from 'jotai';
import { OverlayShell } from '../../shared/ui/OverlayShell';
import { batchRenameOpenAtom } from '../../state/portals';
import shellStyles from '../../shared/ui/OverlayShell/OverlayShell.module.css';

export function BatchRenamePanel() {
  const open = useAtomValue(batchRenameOpenAtom);
  const setOpen = useSetAtom(batchRenameOpenAtom);
  const [pinned, setPinned] = useState(false);

  if (!open) return null;

  return (
    <OverlayShell
      open={open}
      onClose={() => setOpen(false)}
      width={480}
      pinned={pinned}
      onPinnedChange={setPinned}
      header={<span style={{ fontSize: 13, color: 'var(--color-text-secondary)' }}>Batch Rename</span>}
      footer={
        <>
          <span className={shellStyles.kbdHint}>
            <span className={shellStyles.kbd}>Esc</span> close
          </span>
          <div className={shellStyles.footerBtnGroup}>
            <button className={shellStyles.footerBtn} onClick={() => setOpen(false)} type="button">Cancel</button>
            <button className={`${shellStyles.footerBtn} ${shellStyles.footerBtnPrimary}`} type="button" disabled>Rename</button>
          </div>
        </>
      }
    >
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', flex: 1, color: 'var(--color-text-tertiary)', fontSize: 13 }}>
        Batch Rename — rebuild pending
      </div>
    </OverlayShell>
  );
}
