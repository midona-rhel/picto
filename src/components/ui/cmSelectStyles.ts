/**
 * Shared Mantine Select style overrides for pickers.
 *
 * Single source of truth — all Select components import from here.
 */

export const cmSelectInput: React.CSSProperties = {
  background: 'var(--color-white-10)',
  border: '1px solid var(--color-border-secondary)',
  color: 'var(--color-text-primary)',
  borderRadius: 6,
  height: 26,
  minHeight: 26,
  lineHeight: '24px',
  paddingTop: 0,
  paddingBottom: 0,
  fontSize: 'var(--mantine-font-size-sm)',
};

export const cmSelectDropdown: React.CSSProperties = {
  background: 'linear-gradient(rgba(255,255,255,0.1), rgba(255,255,255,0.1)), linear-gradient(var(--context-menu-bg), var(--context-menu-bg))',
  border: '1px solid var(--color-border-secondary)',
  borderRadius: 6,
  backdropFilter: 'none',
  WebkitBackdropFilter: 'none',
};

export const cmSelectOption: React.CSSProperties = {
  fontSize: 'var(--mantine-font-size-sm)',
  color: 'var(--color-text-primary)',
};

export const cmComboboxProps = {
  withinPortal: true,
  zIndex: 10100,
  offset: 4,
} as const;
