import { useAtomValue } from 'jotai';
import { IconEdit } from '@tabler/icons-react';
import { collectionChromeAtom } from '../../state/collections';
import { KbdTooltip } from '../../shared/ui/KbdTooltip';
import {
  TitlebarControlButton,
  TitlebarControlGroup,
  TitlebarControls,
  TitlebarCounter,
} from '../../shared/ui/TitlebarControls';
import { ToolbarChevronIcon, ToolbarHistoryIcon } from '../../shared/ui/icons/toolbar-icons';

export function CollectionToolbar() {
  const chrome = useAtomValue(collectionChromeAtom);
  if (!chrome) return null;

  const editing = chrome.mode === 'editor';
  const canPrevious = chrome.currentIndex > 0;
  const canNext = chrome.currentIndex < chrome.total - 1;
  return (
    <TitlebarControls
      label="Collection controls"
      left={(
        <>
          <KbdTooltip label={editing ? 'Back to collection' : 'Back to grid'} shortcut="Escape">
            <TitlebarControlButton
              onClick={editing ? chrome.showReader : chrome.close}
              aria-label={editing ? 'Back to collection' : 'Back to grid'}
            >
              <ToolbarHistoryIcon direction="back" />
            </TitlebarControlButton>
          </KbdTooltip>
          {!editing ? <TitlebarCounter current={chrome.currentIndex + 1} total={chrome.total} /> : null}
        </>
      )}
      right={!editing ? (
        <>
          <KbdTooltip label="Edit collection">
            <TitlebarControlButton
              onClick={chrome.edit}
              aria-label="Edit collection"
            >
              <IconEdit size={16} stroke={1.5} />
            </TitlebarControlButton>
          </KbdTooltip>
          <TitlebarControlGroup>
            <KbdTooltip label="Previous" shortcut="ArrowLeft">
              <TitlebarControlButton
                disabled={!canPrevious}
                onClick={canPrevious ? () => chrome.navigate(-1) : undefined}
                aria-label="Previous"
              >
                <ToolbarChevronIcon direction="left" />
              </TitlebarControlButton>
            </KbdTooltip>
            <KbdTooltip label="Next" shortcut="ArrowRight">
              <TitlebarControlButton
                disabled={!canNext}
                onClick={canNext ? () => chrome.navigate(1) : undefined}
                aria-label="Next"
              >
                <ToolbarChevronIcon direction="right" />
              </TitlebarControlButton>
            </KbdTooltip>
          </TitlebarControlGroup>
        </>
      ) : null}
    />
  );
}
