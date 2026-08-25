/**
 * KbdTooltip — glass tooltip with label + keyboard shortcut badges.
 * Uses Mantine Tooltip with custom glass styles, matching legacy v0.5.0-alpha exactly.
 */

import { Tooltip } from '@mantine/core';
import { formatKeysAsArray, getShortcut } from '../../lib/shortcuts';
import styles from './KbdTooltip.module.css';
import type { ReactNode } from 'react';

interface KbdTooltipProps {
  label: string;
  /** Resolve the current binding from the registry, including user overrides. */
  shortcutId?: string;
  /** Use only for local controls that are intentionally absent from the registry. */
  shortcut?: string;
  children: ReactNode;
  position?: 'top' | 'bottom' | 'left' | 'right';
}

const tooltipStyles = {
  tooltip: {
    background: 'var(--glass-bg)',
    backdropFilter: 'blur(16px)',
    WebkitBackdropFilter: 'blur(16px)',
    border: '1px solid var(--color-border-secondary)',
    borderTop: 'var(--glass-border-top)',
    boxShadow: '0 2px 8px rgba(0, 0, 0, 0.3)',
    borderRadius: 4,
    padding: '0 6px',
    height: 24,
    display: 'flex',
    alignItems: 'center',
  },
};

export function KbdTooltip({ label, shortcutId, shortcut, children, position = 'bottom' }: KbdTooltipProps) {
  const binding = shortcutId ? getShortcut(shortcutId)?.keys : shortcut;
  const keys = binding ? formatKeysAsArray(binding) : [];

  const tooltipLabel = (
    <span className={styles.content}>
      <span>{label}</span>
      {keys.map((k, i) => (
        <kbd key={i} className={styles.kbd}>{k}</kbd>
      ))}
    </span>
  );

  return (
    <Tooltip
      label={tooltipLabel}
      position={position}
      offset={6}
      openDelay={400}
      closeDelay={0}
      withArrow={false}
      zIndex={10001}
      styles={tooltipStyles}
      transitionProps={{ transition: 'pop', duration: 150 }}
    >
      {children as any}
    </Tooltip>
  );
}
