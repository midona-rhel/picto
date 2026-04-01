/**
 * AiTaggerPanel — floating glass panel for AI tag prediction.
 * TODO: Wire to ai_tag_predict / ai_tag_apply backend commands.
 */

import { useState } from 'react';
import { useAtomValue, useSetAtom } from 'jotai';
import { OverlayShell } from '../../shared/ui/OverlayShell';
import { aiTaggerOpenAtom } from '../../state/portals';
import shellStyles from '../../shared/ui/OverlayShell/OverlayShell.module.css';
import btnStyles from '../../shared/styles/actionButton.module.css';

export function AiTaggerPanel() {
  const open = useAtomValue(aiTaggerOpenAtom);
  const setOpen = useSetAtom(aiTaggerOpenAtom);
  const [pinned, setPinned] = useState(false);

  if (!open) return null;

  return (
    <OverlayShell
      open={open}
      onClose={() => setOpen(false)}
      pinned={pinned}
      onPinnedChange={setPinned}
      header={<span style={{ fontSize: 13, color: 'var(--color-text-secondary)' }}>AI Tagger</span>}
      footer={
        <>
          <span className={shellStyles.kbdHint}>
            <span className={shellStyles.kbd}>Esc</span> close
          </span>
          <div className={btnStyles.btnGroup}>
            <button className={btnStyles.btn} onClick={() => setOpen(false)} type="button">Cancel</button>
            <button className={`${btnStyles.btn} ${btnStyles.btnPrimary}`} type="button" disabled>Apply</button>
          </div>
        </>
      }
    >
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', flex: 1, color: 'var(--color-text-tertiary)', fontSize: 13 }}>
        AI Tagger — rebuild pending
      </div>
    </OverlayShell>
  );
}
