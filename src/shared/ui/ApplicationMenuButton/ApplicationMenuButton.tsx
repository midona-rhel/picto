import type { MouseEvent } from 'react';
import { KbdTooltip } from '../KbdTooltip';
import { ContextMenu, useContextMenu, type MenuEntry } from '../ContextMenu';
import { formatKeysDisplay } from '../../lib/shortcuts';
import styles from './ApplicationMenuButton.module.css';
import { t } from '../../../i18n';

interface ApplicationMenuNode {
  id: string;
  label: string;
  type: string;
  enabled: boolean;
  checked: boolean;
  accelerator: string | null;
  submenu: ApplicationMenuNode[] | null;
}

function toMenuEntries(nodes: ApplicationMenuNode[], depth = 0): MenuEntry[] {
  return nodes.flatMap((node): MenuEntry[] => {
    if (node.type === 'separator') return [{ separator: true }];
    if (node.submenu) {
      const children = toMenuEntries(node.submenu, depth + 1);
      // ContextMenu intentionally supports one submenu level. The only deeper
      // application-menu branch is Recent Libraries, so expand it in place.
      if (depth > 0) return [{ separator: true }, ...children];
      return [{ submenu: true, label: node.label, children }];
    }
    return [{
      label: node.label,
      shortcut: node.accelerator
        ? formatKeysDisplay(node.accelerator.replace(/CmdOrCtrl|CommandOrControl/g, 'Mod'))
        : undefined,
      disabled: !node.enabled,
      checked: node.type === 'checkbox' || node.type === 'radio' ? node.checked : undefined,
      action: () => { void (window as any).picto?.api?.executeApplicationMenuItem?.(node.id); },
    }];
  });
}

export function usesInWindowApplicationMenu(
  platform = navigator.platform,
  debug = import.meta.env.DEV,
) {
  return debug || !/^mac/i.test(platform);
}

/**
 * Opens the one native application menu built in the Electron main process.
 * Production macOS keeps that menu in the system menu bar. Development also
 * exposes this trigger so the Windows/Linux menu can be verified on macOS.
 */
export function ApplicationMenuButton({
  platform = navigator.platform,
  debug = import.meta.env.DEV,
}: {
  platform?: string;
  debug?: boolean;
} = {}) {
  const menu = useContextMenu();
  if (!usesInWindowApplicationMenu(platform, debug)) return null;

  const openMenu = async (event: MouseEvent<HTMLButtonElement>) => {
    const rect = event.currentTarget.getBoundingClientRect();
    const model = await (window as any).picto?.api?.getApplicationMenu?.() as ApplicationMenuNode[] | undefined;
    if (!model?.length) return;
    menu.openAt({ x: rect.left, y: rect.bottom + 4 }, toMenuEntries(model), { showSearch: false });
  };

  return (
    <>
      <KbdTooltip label={t("Application menu")} position="bottom">
        <button
          type="button"
          className={styles.button}
          aria-label={t("Application menu")}
          aria-haspopup="menu"
          aria-expanded={menu.state ? true : undefined}
          onClick={(event) => { void openMenu(event); }}
        >
          <svg className={styles.icon} viewBox="0 0 16 15" aria-hidden="true">
            <path d="M1 3.5h14M1 7.5h14M1 11.5h14" />
          </svg>
        </button>
      </KbdTooltip>
      {menu.state ? (
        <ContextMenu
          entries={menu.state.entries}
          position={menu.state.position}
          onClose={menu.close}
          showSearch={false}
          width={220}
        />
      ) : null}
    </>
  );
}
