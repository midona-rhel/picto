import { useEffect, useMemo, useState } from 'react';
import { IconFolder, IconFolderQuestion, IconFolderStar, IconPhoto, IconInbox, IconTag, IconTrash, IconCopy } from '@tabler/icons-react';
import { useNavigationStore } from '../state-legacy/navigationStore';
import { useAtomValue } from 'jotai';
import { folderNodesAtom } from '../state/sidebar';
import { useDomainStore } from '../state-legacy/domainStore';
import { SHORTCUT_DEFS, formatKeysDisplay, getShortcut, matchesShortcutDef, parseShortcutKeys } from '../shared/lib/shortcuts';
import type { CommandAction } from '#features/app/components';

export function useCommandPalette() {
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [paletteMode, setPaletteMode] = useState<'all' | 'navigation'>('all');
  const { canGoBack, canGoForward, goBack, goForward, navigateToFolder, navigateToSmartFolder, navigateTo } = useNavigationStore();
  const folderNodes = useAtomValue(folderNodesAtom);
  const { smartFolders } = useDomainStore();

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const cmdPalette = getShortcut('nav.commandPalette');
      if (cmdPalette && matchesShortcutDef(e, cmdPalette)) {
        e.preventDefault();
        setPaletteMode('all');
        setPaletteOpen(true);
        return;
      }
      const goToFolder = getShortcut('nav.goToFolder');
      if (goToFolder && matchesShortcutDef(e, goToFolder)) {
        e.preventDefault();
        setPaletteMode('navigation');
        setPaletteOpen(true);
        return;
      }
      const back = getShortcut('nav.back');
      if (back && matchesShortcutDef(e, back)) {
        e.preventDefault();
        if (canGoBack) goBack();
        return;
      }
      const forward = getShortcut('nav.forward');
      if (forward && matchesShortcutDef(e, forward)) {
        e.preventDefault();
        if (canGoForward) goForward();
        return;
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [canGoBack, canGoForward, goBack, goForward]);

  const paletteActions = useMemo((): CommandAction[] => {
    const actions: CommandAction[] = [];

    // System navigation targets
    const navTargets: { id: string; label: string; icon: React.ReactNode; go: () => void }[] = [
      { id: 'go.allActive', label: 'All Active', icon: <IconPhoto size={16} />, go: () => navigateTo('images', null, null, null) },
      { id: 'go.inbox', label: 'Inbox', icon: <IconInbox size={16} />, go: () => navigateTo('images', null, null, 'inbox') },
      { id: 'go.uncategorized', label: 'Uncategorized', icon: <IconFolderQuestion size={16} />, go: () => navigateTo('images', null, null, 'uncategorized') },
      { id: 'go.untagged', label: 'Untagged', icon: <IconTag size={16} />, go: () => navigateTo('images', null, null, 'untagged') },
      { id: 'go.trash', label: 'Trash', icon: <IconTrash size={16} />, go: () => navigateTo('images', null, null, 'trash') },
      { id: 'go.duplicates', label: 'Duplicates', icon: <IconCopy size={16} />, go: () => navigateTo('duplicates') },
    ];
    for (const t of navTargets) {
      actions.push({ id: t.id, label: t.label, group: 'Navigation', icon: t.icon, execute: t.go });
    }

    // Dynamic folders
    for (const node of folderNodes) {
      if (node.kind === 'folder') {
        const folderId = parseInt(node.id.replace('folder:', ''), 10);
        if (isNaN(folderId)) continue;
        actions.push({
          id: `go.folder.${folderId}`,
          label: node.name,
          group: 'Navigation',
          icon: <IconFolder size={16} />,
          execute: () => navigateToFolder({ folder_id: folderId, name: node.name }),
        });
      }
    }

    // Dynamic smart folders
    for (const sf of smartFolders) {
      actions.push({
        id: `go.sf.${sf.id}`,
        label: sf.name,
        group: 'Navigation',
        icon: <IconFolderStar size={16} />,
        execute: () => navigateToSmartFolder({ id: sf.id, name: sf.name, predicate: sf.predicate ?? { groups: [] } }),
      });
    }

    // Shortcut-based actions (skip nav ones we already added, and skip palette itself)
    const skipIds = new Set(['nav.commandPalette', 'nav.goToFolder', 'nav.allActive', 'nav.inbox', 'nav.untagged', 'nav.trash']);
    for (const def of SHORTCUT_DEFS) {
      if (skipIds.has(def.id)) continue;
      actions.push({
        id: `shortcut.${def.id}`,
        label: def.label,
        description: def.description,
        group: def.group,
        shortcut: formatKeysDisplay(def.keys),
        execute: () => {
          const parsed = parseShortcutKeys(def.keys);
          if (parsed) {
            window.dispatchEvent(new KeyboardEvent('keydown', {
              key: parsed.key,
              code: parsed.code,
              metaKey: parsed.meta,
              ctrlKey: parsed.ctrl,
              altKey: parsed.alt,
              shiftKey: parsed.shift,
              bubbles: true,
            }));
          }
        },
      });
    }

    return actions;
  }, [folderNodes, smartFolders, navigateTo, navigateToFolder, navigateToSmartFolder]);

  const closePalette = () => setPaletteOpen(false);

  return { paletteOpen, closePalette, paletteMode, paletteActions };
}
