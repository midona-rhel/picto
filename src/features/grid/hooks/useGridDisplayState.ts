import { useMemo, useRef } from 'react';
import { useSettingsStore } from '../../../state/settingsStore';
import { useScopedDisplay } from '../../../shared/contexts/ScopedDisplayContext';
import { useDomainStore } from '../../../state/domainStore';

export function useGridDisplayState(args: {
  displayFolderId: number | null;
}) {
  const { displayFolderId } = args;
  const { settings: globalSettings, updateSetting } = useSettingsStore();
  const scopedCtx = useScopedDisplay();
  const scopedOpts = scopedCtx?.displayOptions;
  const folderNodes = useDomainStore((s) => s.folderNodes);

  const displaySettings = useMemo(() => ({
    ...globalSettings,
    ...(scopedOpts ? {
      showTileName: scopedOpts.showTileName,
      showResolution: scopedOpts.showResolution,
      showExtension: scopedOpts.showExtension,
      showExtensionLabel: scopedOpts.showExtensionLabel,
      thumbnailFitMode: scopedOpts.thumbnailFitMode,
    } : {}),
  }), [globalSettings, scopedOpts]);

  const displaySettingsRef = useRef(displaySettings);
  displaySettingsRef.current = displaySettings;

  const hasVisibleSubfolders = useMemo(() => {
    if (!displayFolderId || !displaySettings.showSubfolders) return false;
    const parentNodeId = `folder:${displayFolderId}`;
    return folderNodes.some((n) => n.parent_id === parentNodeId);
  }, [displayFolderId, displaySettings.showSubfolders, folderNodes]);

  return {
    displaySettings,
    displaySettingsRef,
    hasVisibleSubfolders,
    updateSetting,
  };
}
