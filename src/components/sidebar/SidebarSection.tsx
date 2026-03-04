
import { type ReactNode, useState } from 'react';
import { IconChevronRight, IconPlus } from '@tabler/icons-react';
import { KbdTooltip } from '../ui/KbdTooltip';
import styles from './Sidebar.module.css';

interface SidebarSectionProps {
  title: string;
  children: ReactNode;
  defaultCollapsed?: boolean;
  onAdd?: () => void;
}

export function SidebarSection({ title, children, defaultCollapsed = false, onAdd }: SidebarSectionProps) {
  const [collapsed, setCollapsed] = useState(defaultCollapsed);

  const arrowCls = `${styles.sectionArrow} ${collapsed ? '' : styles.sectionArrowExpanded}`;

  return (
    <div>
      <div className={styles.sectionHeader} onClick={() => setCollapsed((c) => !c)}>
        <div className={styles.sectionTitleRow}>
          <span className={styles.sectionTitle}>{title}</span>
          <span className={arrowCls}>
            <IconChevronRight size={10} />
          </span>
        </div>
        {onAdd && (
          <KbdTooltip label={`New ${title.replace(/s$/, '')}`}>
            <button
              className={styles.sectionAddBtn}
              onClick={(e) => { e.stopPropagation(); onAdd(); }}
            >
              <IconPlus size={12} />
            </button>
          </KbdTooltip>
        )}
      </div>
      {!collapsed && <div className={styles.sectionBody}>{children}</div>}
    </div>
  );
}
