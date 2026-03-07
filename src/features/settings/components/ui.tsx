import type { ReactNode } from 'react';
import { Text } from '@mantine/core';
import styles from './Settings.module.css';

/* --------------------------------------------------------------------------
   SettingsBlock — section card (panelBlock + blockTitle + blockContent)
   -------------------------------------------------------------------------- */
interface SettingsBlockProps {
  title?: string;
  description?: string;
  dimmed?: boolean;
  children: ReactNode;
  /** Remove padding and clip overflow (for embedded search + tables) */
  flush?: boolean;
  /** Override border color (for error states) */
  borderColor?: string;
}

export function SettingsBlock({ title, description, dimmed, children, flush, borderColor }: SettingsBlockProps) {
  return (
    <div className={`${styles.panelBlock} ${dimmed ? styles.blockDimmed : ''}`}>
      {title && <div className={styles.blockTitle}>{title}</div>}
      <div
        className={`${styles.blockContent} ${flush ? styles.blockContentFlush : ''}`}
        style={borderColor ? { borderColor } : undefined}
      >
        {description && <Text size="xs" c="dimmed" mb={12}>{description}</Text>}
        {children}
      </div>
    </div>
  );
}

/* --------------------------------------------------------------------------
   SettingsRow — horizontal label (left) + right-aligned control
   -------------------------------------------------------------------------- */
interface SettingsRowProps {
  label: string;
  children: ReactNode;
  /** Render a separator line before this row */
  separator?: boolean;
  /** Make label lighter weight (for sub-items like Active/Inbox/Trash) */
  light?: boolean;
}

export function SettingsRow({ label, children, separator, light }: SettingsRowProps) {
  return (
    <>
      {separator && <div className={styles.blockSeparator} />}
      <div className={styles.labelItem}>
        <label className={light ? styles.settingsLabelLight : undefined}>{label}</label>
        <div className={styles.right}>{children}</div>
      </div>
    </>
  );
}

/* --------------------------------------------------------------------------
   SettingsButtonRow — right-aligned button group
   -------------------------------------------------------------------------- */
interface SettingsButtonRowProps {
  children: ReactNode;
}

export function SettingsButtonRow({ children }: SettingsButtonRowProps) {
  return (
    <div className={styles.settingsButtonRow}>
      {children}
    </div>
  );
}

/* --------------------------------------------------------------------------
   SettingsInputGroup — horizontal flex row for input + button combos
   -------------------------------------------------------------------------- */
interface SettingsInputGroupProps {
  children: ReactNode;
  mb?: number;
}

export function SettingsInputGroup({ children, mb }: SettingsInputGroupProps) {
  return (
    <div className={styles.settingsInputGroup} style={mb ? { marginBottom: mb } : undefined}>
      {children}
    </div>
  );
}
