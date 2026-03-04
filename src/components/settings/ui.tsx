import type { ReactNode, CSSProperties } from 'react';
import { Text } from '@mantine/core';
import styles from '../Settings.module.css';

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
  const blockStyle: CSSProperties | undefined = dimmed
    ? { opacity: 0.5, pointerEvents: 'none' }
    : undefined;

  const contentStyle: CSSProperties | undefined =
    flush || borderColor
      ? {
          ...(flush ? { padding: 0, overflow: 'hidden' } : undefined),
          ...(borderColor ? { borderColor } : undefined),
        }
      : undefined;

  return (
    <div className={styles.panelBlock} style={blockStyle}>
      {title && <div className={styles.blockTitle}>{title}</div>}
      <div className={styles.blockContent} style={contentStyle}>
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
        <label style={light ? { fontWeight: 'var(--font-weight-regular)' as any } : undefined}>{label}</label>
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
    <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 6, marginTop: 8 }}>
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
    <div style={{ display: 'flex', gap: 6, alignItems: 'center', marginBottom: mb }}>
      {children}
    </div>
  );
}
