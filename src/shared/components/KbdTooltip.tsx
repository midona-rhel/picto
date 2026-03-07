import { Tooltip } from '@mantine/core';
import { formatKeysAsArray } from '../../shared/lib/shortcuts';
import st from './KbdTooltip.module.css';
import type { ReactNode } from 'react';

interface KbdTooltipProps {
  /** Action label text shown in the tooltip */
  label: string;
  /** Shortcut key string, e.g. "Mod+ArrowLeft" or "Escape" */
  shortcut?: string;
  children: ReactNode;
  /** Tooltip position (default: bottom) */
  position?: 'top' | 'bottom' | 'left' | 'right';
}

const tooltipStyles = {
  tooltip: {
    background: 'var(--context-menu-bg)',
    backdropFilter: 'blur(16px)',
    WebkitBackdropFilter: 'blur(16px)',
    border: '1px solid var(--context-menu-border)',
    borderTop: '1px solid var(--color-white-20)',
    boxShadow: 'var(--box-border-shadow)',
    borderRadius: 4,
    padding: '0 6px',
    height: 24,
    display: 'flex',
    alignItems: 'center',
  },
};

export function KbdTooltip({ label, shortcut, children, position = 'bottom' }: KbdTooltipProps) {
  const keys = shortcut ? formatKeysAsArray(shortcut) : [];

  const tooltipLabel = (
    <span className={st.tooltipContent}>
      <span>{label}</span>
      {keys.map((k, i) => (
        <kbd key={i} className={st.kbd}>{k}</kbd>
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
      {/* Mantine Tooltip requires a single element child for ref forwarding; ReactNode is broader */}
      {/* eslint-disable-next-line @typescript-eslint/no-explicit-any */}
      {children as any}
    </Tooltip>
  );
}
