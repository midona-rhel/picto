import type { ReactNode, ComponentType } from 'react';
import { StateBlock } from './state';

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type IconComponent = ComponentType<any>;

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
