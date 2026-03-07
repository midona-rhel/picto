import type { ReactNode } from 'react';
import type { TablerIcon } from '@tabler/icons-react';
import { StateBlock } from './state';

type IconComponent = TablerIcon;

interface EmptyStateProps {
  /** Tabler icon component — auto-sized and colored */
  icon?: IconComponent;
  /** Pre-rendered icon element (when you already have JSX) */
  iconNode?: ReactNode;
  title?: string;
  description?: string;
  action?: ReactNode;
  /** Compact mode — less padding, smaller icon (for inline lists) */
  compact?: boolean;
}

export function EmptyState({ icon: Icon, iconNode, title, description, action, compact }: EmptyStateProps) {
  return (
    <StateBlock
      variant="empty"
      icon={Icon}
      iconNode={iconNode}
      title={title}
      description={description}
      action={action}
      compact={compact}
    />
  );
}
