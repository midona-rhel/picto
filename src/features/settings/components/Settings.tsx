import { useState, useMemo } from 'react';
import type { FC, ReactNode } from 'react';
import {
  IconSettings2,
  IconCommand,
  IconDownload,
  IconCloud,
  IconCopy,
  IconAlertTriangle,
  IconCode,
  IconX,
} from '@tabler/icons-react';
import { getCurrentWindow } from '#desktop/api';

import { GeneralPanel } from './GeneralPanel';
import { ShortcutsPanel } from './ShortcutsPanel';
import { DownloadServicesPanel } from './DownloadServicesPanel';
import { PtrPanel } from './PtrPanel';
import { DuplicatesPanel } from './DuplicatesPanel';
import { DangerZonePanel } from './DangerZonePanel';
import { DeveloperPanel } from './DeveloperPanel';
import styles from './Settings.module.css';

interface NavItem {
  id: string;
  label: string;
  icon: FC<{ size?: number | string }>;
  panel: () => ReactNode;
  keywords?: string;
}

type SidebarEntry = NavItem | { type: 'separator' };

const SIDEBAR_PANELS: SidebarEntry[] = [
  { id: 'general', label: 'General', icon: IconSettings2, panel: () => <GeneralPanel />, keywords: 'theme appearance color language zoom' },
  { id: 'shortcuts', label: 'Shortcuts', icon: IconCommand, panel: () => <ShortcutsPanel />, keywords: 'keyboard shortcut keybind hotkey' },
  { type: 'separator' },
  { id: 'downloads', label: 'Downloads', icon: IconDownload, panel: () => <DownloadServicesPanel />, keywords: 'download service gallery-dl rate limit batch' },
  { id: 'ptr', label: 'PTR', icon: IconCloud, panel: () => <PtrPanel />, keywords: 'ptr public tag repository hydrus sync tags' },
  { id: 'duplicates', label: 'Duplicates', icon: IconCopy, panel: () => <DuplicatesPanel />, keywords: 'duplicate merge phash similarity threshold' },
  { type: 'separator' },
  { id: 'developer', label: 'Developer', icon: IconCode, panel: () => <DeveloperPanel />, keywords: 'developer perf slo diagnostics debug' },
  { id: 'danger', label: 'Danger Zone', icon: IconAlertTriangle, panel: () => <DangerZonePanel />, keywords: 'danger reset delete' },
];

function isSeparator(entry: SidebarEntry): entry is { type: 'separator' } {
  return 'type' in entry && entry.type === 'separator';
}

const ALL_ITEMS = SIDEBAR_PANELS.filter((e): e is NavItem => !isSeparator(e));

export function Settings() {
  const [selected, setSelected] = useState('general');
  const [keyword, setKeyword] = useState('');
  const activeItem = ALL_ITEMS.find((i) => i.id === selected) ?? ALL_ITEMS[0];

  const filteredPanels = useMemo(() => {
    if (!keyword.trim()) return SIDEBAR_PANELS;
    const lower = keyword.toLowerCase();
    return SIDEBAR_PANELS.filter((entry) => {
      if (isSeparator(entry)) return false;
      return (
        entry.label.toLowerCase().includes(lower) ||
        (entry.keywords && entry.keywords.toLowerCase().includes(lower))
      );
    });
  }, [keyword]);

  const handleClose = () => {
    getCurrentWindow().close().catch(() => {});
  };

  return (
    <div className={styles.root}>
      <div className={styles.sidebar}>
        <div className={styles.sidebarTitle}>Settings</div>
        <input
          className={styles.sidebarSearch}
          type="search"
          placeholder="Search"
          value={keyword}
          onChange={(e) => setKeyword(e.target.value)}
        />
        <div className={styles.sidebarItems}>
          {filteredPanels.map((entry, i) => {
            if (isSeparator(entry)) return <div key={`sep-${i}`} className={styles.separator} />;
            const Icon = entry.icon;
            const isActive = entry.id === selected;
            return (
              <button
                key={entry.id}
                className={isActive ? styles.navItemActive : styles.navItem}
                onClick={() => setSelected(entry.id)}
              >
                <Icon size={20} />
                {entry.label}
              </button>
            );
          })}
        </div>
      </div>

      <div className={styles.content}>
        <div className={styles.contentHeader}>
          <span className={styles.contentHeaderTitle}>{activeItem.label}</span>
          <button className={styles.closeButton} onClick={handleClose}>
            <IconX size={14} />
          </button>
        </div>
        <div className={styles.contentBody}>
          {activeItem.panel()}
        </div>
      </div>
    </div>
  );
}
