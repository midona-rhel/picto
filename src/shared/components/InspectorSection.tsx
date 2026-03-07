import type { ReactNode } from 'react';
import { IconChevronRight } from '@tabler/icons-react';
import styles from './InspectorSection.module.css';

interface InspectorSectionProps {
  title: string;
  count?: number;
  collapsed: boolean;
  onToggle: () => void;
  children: ReactNode;
}

export function InspectorSection({ title, count, collapsed, onToggle, children }: InspectorSectionProps) {
  return (
    <div className={styles.infoSection}>
      <div className={styles.sectionHeader} onClick={onToggle}>
        <span className={styles.sectionTitle}>
          {title}
          {count != null && <span className={styles.sectionCount}> ({count})</span>}
        </span>
        <IconChevronRight
          size={14}
          className={`${styles.sectionChevron} ${!collapsed ? styles.sectionChevronExpanded : ''}`}
        />
      </div>
      {!collapsed && (
        <div className={styles.sectionContent}>
          {children}
        </div>
      )}
    </div>
  );
}
