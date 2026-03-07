import type { ReactNode } from 'react';
import type { TablerIcon } from '@tabler/icons-react';
import { Loader, Text } from '@mantine/core';
import styles from './StateBlock.module.css';

type StateVariant = 'loading' | 'empty' | 'error';

type IconComponent = TablerIcon;

interface StateBlockProps {
  variant: StateVariant;
  title?: string;
  description?: string;
  icon?: IconComponent;
  iconNode?: ReactNode;
  action?: ReactNode;
  compact?: boolean;
  minHeight?: number | string;
  className?: string;
}

function renderIcon(
  variant: StateVariant,
  compact: boolean,
  Icon?: IconComponent,
  iconNode?: ReactNode,
): ReactNode {
  if (iconNode) return iconNode;
  if (Icon) {
    const color = variant === 'error' ? 'var(--color-danger, #ff6b6b)' : 'var(--color-text-tertiary)';
    return <Icon size={compact ? 24 : 36} stroke={1.5} color={color} />;
  }
  if (variant === 'loading') return <Loader size={compact ? 'xs' : 'sm'} />;
  return null;
}

export function StateBlock({
  variant,
  title,
  description,
  icon,
  iconNode,
  action,
  compact = false,
  minHeight,
  className,
}: StateBlockProps) {
  const rootClassName = [
    styles.root,
    compact ? styles.compact : '',
    className ?? '',
  ].filter(Boolean).join(' ');

  return (
    <div className={rootClassName} style={minHeight != null ? { minHeight } : undefined}>
      {renderIcon(variant, compact, icon, iconNode)}
      {title ? <Text className={styles.title}>{title}</Text> : null}
      {description ? (
        <Text className={[styles.description, variant === 'error' ? styles.descriptionError : ''].filter(Boolean).join(' ')}>
          {description}
        </Text>
      ) : null}
      {action ?? null}
    </div>
  );
}
