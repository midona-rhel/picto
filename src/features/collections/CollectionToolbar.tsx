import { useAtomValue } from 'jotai';
import { IconCheck, IconEdit } from '@tabler/icons-react';
import { collectionChromeAtom } from '../../state/collections';
import { KbdTooltip } from '../../shared/ui/KbdTooltip';
import {
  TitlebarControlButton,
  TitlebarControls,
} from '../../shared/ui/TitlebarControls';
import { ToolbarHistoryIcon } from '../../shared/ui/icons/toolbar-icons';
import styles from './CollectionToolbar.module.css';

export function CollectionToolbar() {
  const chrome = useAtomValue(collectionChromeAtom);
  if (!chrome) return null;

  const editing = chrome.mode === 'editor';
  return (
    <TitlebarControls
      label="Collection controls"
      left={(
        <>
          <KbdTooltip label="Back to grid" shortcut="Escape">
            <TitlebarControlButton onClick={chrome.close} aria-label="Back to grid">
              <ToolbarHistoryIcon direction="back" />
            </TitlebarControlButton>
          </KbdTooltip>
          <span className={styles.breadcrumb}>
            <button type="button" className={styles.parent} onClick={chrome.close}>
              {chrome.parentLabel}
            </button>
            <span className={styles.separator}>/</span>
            <span className={styles.current}>{chrome.label}</span>
          </span>
        </>
      )}
      right={(
        <KbdTooltip label={editing ? 'Finish editing' : 'Edit collection'}>
          <TitlebarControlButton
            active={editing}
            onClick={editing ? chrome.finishEditing : chrome.edit}
            aria-label={editing ? 'Finish editing collection' : 'Edit collection'}
          >
            {editing
              ? <IconCheck size={16} stroke={1.5} />
              : <IconEdit size={16} stroke={1.5} />}
          </TitlebarControlButton>
        </KbdTooltip>
      )}
    />
  );
}
