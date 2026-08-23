import { useAtomValue } from 'jotai';
import { IconEdit } from '@tabler/icons-react';
import { collectionChromeAtom } from '../../state/collections';
import { KbdTooltip } from '../../shared/ui/KbdTooltip';
import {
  TitlebarControlButton,
  TitlebarControls,
} from '../../shared/ui/TitlebarControls';
import { ToolbarHistoryIcon } from '../../shared/ui/icons/toolbar-icons';

export function CollectionToolbar() {
  const chrome = useAtomValue(collectionChromeAtom);
  if (!chrome) return null;

  const editing = chrome.mode === 'editor';
  return (
    <TitlebarControls
      label="Collection controls"
      left={(
        <KbdTooltip label={editing ? 'Back to collection' : 'Back to grid'} shortcut="Escape">
          <TitlebarControlButton
            onClick={editing ? chrome.showReader : chrome.close}
            aria-label={editing ? 'Back to collection' : 'Back to grid'}
          >
            <ToolbarHistoryIcon direction="back" />
          </TitlebarControlButton>
        </KbdTooltip>
      )}
      right={!editing ? (
        <KbdTooltip label="Edit collection">
          <TitlebarControlButton
            onClick={chrome.edit}
            aria-label="Edit collection"
          >
            <IconEdit size={16} stroke={1.5} />
          </TitlebarControlButton>
        </KbdTooltip>
      ) : null}
    />
  );
}
