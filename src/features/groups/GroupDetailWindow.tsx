import { useCallback, useEffect, useRef, useState } from 'react';
import { IconPin, IconPinFilled } from '@tabler/icons-react';
import { viewerController } from '../../controllers/viewerController';
import { windowController } from '../../controllers/windowController';
import { KbdTooltip } from '../../shared/ui/KbdTooltip';
import { ToolbarCloseIcon } from '../../shared/ui/icons/toolbar-icons';
import { ModalLayer } from '../modals/ModalLayer';
import { TagSelectPanel } from '../tags/TagSelectPanel';
import { FolderPickerPanel } from '../folders/FolderPickerPanel';
import { AiTaggerPanel } from '../ai-tagger/AiTaggerPanel';
import { LibraryCoverDialogHost } from '../library/LibraryCoverDialogHost';
import { GroupSurface } from './GroupSurface';
import detailStyles from '../viewer/DetailWindow.module.css';

const TOOLBAR_HIDE_DELAY = 1000;

export function GroupDetailWindow({ groupId }: { groupId: number }) {
  const [title, setTitle] = useState('Group');
  const [ready, setReady] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [alwaysOnTop, setAlwaysOnTop] = useState(false);
  const [toolbarHidden, setToolbarHidden] = useState(true);
  const toolbarTimerRef = useRef<ReturnType<typeof setTimeout>>();

  useEffect(() => {
    let cancelled = false;
    void viewerController.prefetchItemDetails(groupId)
      .then((details) => {
        if (cancelled) return;
        if (details.root.kind !== 'collection') throw new Error('The selected item is not a group.');
        setTitle(details.root.name || 'Group');
        setReady(true);
      })
      .catch((reason) => {
        if (!cancelled) setError(reason instanceof Error ? reason.message : String(reason));
      });
    return () => { cancelled = true; };
  }, [groupId]);

  const toggleAlwaysOnTop = useCallback(() => {
    const next = !alwaysOnTop;
    setAlwaysOnTop(next);
    void windowController.setCurrentWindowAlwaysOnTop(next).catch(() => setAlwaysOnTop(!next));
  }, [alwaysOnTop]);

  const resetToolbarTimer = useCallback(() => {
    setToolbarHidden(false);
    clearTimeout(toolbarTimerRef.current);
    toolbarTimerRef.current = setTimeout(() => setToolbarHidden(true), TOOLBAR_HIDE_DELAY);
  }, []);

  useEffect(() => {
    const onBlur = () => {
      clearTimeout(toolbarTimerRef.current);
      setToolbarHidden(true);
    };
    document.addEventListener('mousemove', resetToolbarTimer);
    window.addEventListener('blur', onBlur);
    window.addEventListener('focus', resetToolbarTimer);
    resetToolbarTimer();
    return () => {
      document.removeEventListener('mousemove', resetToolbarTimer);
      window.removeEventListener('blur', onBlur);
      window.removeEventListener('focus', resetToolbarTimer);
      clearTimeout(toolbarTimerRef.current);
    };
  }, [resetToolbarTimer]);

  return (
    <div className={detailStyles.root}>
      <div
        className={`${detailStyles.toolbar} ${toolbarHidden ? detailStyles.toolbarHidden : ''}`}
        data-window-drag-region=""
      >
        <div className={detailStyles.toolbarLeft}>
          <span className={detailStyles.titleName}>{title}</span>
        </div>
        <div className={detailStyles.toolbarRight}>
          <KbdTooltip label={alwaysOnTop ? 'Unpin' : 'Always on top'} shortcutId="view.alwaysOnTop">
            <button
              className={`${detailStyles.icBtn} ${alwaysOnTop ? detailStyles.icBtnActive : ''}`}
              onClick={toggleAlwaysOnTop}
            >
              {alwaysOnTop ? <IconPinFilled size={16} /> : <IconPin size={16} />}
            </button>
          </KbdTooltip>
          <KbdTooltip label="Close" shortcutId="view.closeDetail">
            <button className={detailStyles.icBtn} onClick={() => void windowController.closeCurrentWindow()}>
              <ToolbarCloseIcon />
            </button>
          </KbdTooltip>
        </div>
      </div>

      {error ? (
        <div style={{ display: 'grid', height: '100%', placeItems: 'center', color: 'var(--color-danger)' }}>
          {error}
        </div>
      ) : ready ? (
        <GroupSurface
          groupId={groupId}
          presentation="detail"
          rootCurrentIndex={0}
          rootTotal={1}
          onNavigateRoot={() => {}}
          onClose={() => void windowController.closeCurrentWindow()}
        />
      ) : null}

      <ModalLayer />
      <TagSelectPanel />
      <FolderPickerPanel />
      <AiTaggerPanel />
      <LibraryCoverDialogHost />
    </div>
  );
}
