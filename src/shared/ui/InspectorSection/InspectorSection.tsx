import { useState, type ReactNode } from 'react';
import { IconChevronRight } from '@tabler/icons-react';
import styles from './InspectorSection.module.css';

interface Props {
  title: string;
  count?: number;
  collapsed?: boolean;
  onToggle?: () => void;
  children: ReactNode;
  onContextMenu?: (event: React.MouseEvent<HTMLDivElement>) => void;
}

export function InspectorSection({ title, count, collapsed: controlledCollapsed, onToggle, children, onContextMenu }: Props) {
  const [internalCollapsed, setInternalCollapsed] = useState(false);
  const collapsed = controlledCollapsed ?? internalCollapsed;
  const toggle = onToggle ?? (() => setInternalCollapsed((c) => !c));

  return (
    <div className={styles.section} onContextMenu={onContextMenu}>
      <div className={styles.header} onClick={toggle}>
        <span className={styles.title}>{title}</span>
        {count != null && <span className={styles.count}> ({count})</span>}
        <span className={`${styles.chevron} ${collapsed ? '' : styles.chevronExpanded}`}>
          <IconChevronRight size={14} />
        </span>
      </div>
      {!collapsed && <div className={styles.content}>{children}</div>}
    </div>
  );
}
