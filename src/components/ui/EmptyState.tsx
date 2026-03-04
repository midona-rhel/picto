import type { ReactNode, ComponentType } from 'react';
import { Text } from '@mantine/core';

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
    <div style={{
      display: 'flex',
      flexDirection: 'column',
      alignItems: 'center',
      gap: compact ? 8 : 12,
      padding: compact ? '12px 0' : '24px 0',
    }}>
      {Icon ? <Icon size={compact ? 24 : 36} stroke={1.5} color="var(--color-text-tertiary)" /> : iconNode}
      {title && <Text fw={500}>{title}</Text>}
      {description && <Text size="sm" c="dimmed" style={{ textAlign: 'center' }}>{description}</Text>}
      {action}
    </div>
  );
}
