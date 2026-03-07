import React from 'react';
import ReactDOM from 'react-dom/client';
import { MantineProvider, createTheme, rem } from '@mantine/core';
import { api, getCurrentWindow } from '#desktop/api';
import { IconX } from '@tabler/icons-react';
import { LibraryPanel } from '#features/settings/components';
import { useThemeSync } from '../shared/hooks/useThemeSync';
import '@mantine/core/styles.css';
import '../shared/styles/globals.css';

const theme = createTheme({
  colors: {
    dark: [
      '#C1C2C5',
      '#A6A7AB',
      '#909296',
      '#5C5D63',
      '#3A3A40',
      '#2E2E34',
      '#27272D',
      '#1E1E22',
      '#19191D',
      '#141417',
    ],
  },
  fontSizes: {
    xs: rem(10),
    sm: rem(12),
    md: rem(13),
    lg: rem(14),
    xl: rem(16),
  },
  spacing: {
    xs: rem(6),
    sm: rem(8),
    md: rem(12),
    lg: rem(16),
    xl: rem(24),
  },
  radius: {
    xs: rem(2),
    sm: rem(4),
    md: rem(6),
    lg: rem(8),
    xl: rem(12),
  },
  defaultRadius: 'sm',
  components: {
    Text: { defaultProps: { size: 'sm' } },
    Button: { defaultProps: { size: 'xs' } },
    ActionIcon: { defaultProps: { size: 'sm', variant: 'subtle' } },
    TextInput: {
      defaultProps: { size: 'sm' },
      styles: { input: { minHeight: rem(28), height: rem(28) } },
    },
    Input: {
      defaultProps: { size: 'sm' },
      styles: { input: { minHeight: rem(28), height: rem(28) } },
    },
    Select: {
      defaultProps: { size: 'sm', withCheckIcon: false },
      styles: {
        input: {
          background: 'var(--color-white-10)',
          border: '1px solid var(--color-border-secondary)',
          color: 'var(--color-text-primary)',
        },
        dropdown: {
          background: 'linear-gradient(rgba(255,255,255,0.1), rgba(255,255,255,0.1)), linear-gradient(var(--context-menu-bg), var(--context-menu-bg))',
          border: '1px solid var(--color-border-secondary)',
          backdropFilter: 'none',
        },
        option: {
          color: 'var(--color-text-primary)',
        },
      },
    },
    Badge: { defaultProps: { size: 'sm' } },
  },
});

// Sync color scheme + theme from settings before React hydrates
(api.settings.get() as Promise<{ colorScheme?: string; theme?: string }>)
  .then((settings) => {
    const t = settings?.theme ?? (settings?.colorScheme === 'light' ? 'light' : 'dark');
    const scheme = t === 'auto' ? 'dark' : (t === 'light' || t === 'lightgray') ? 'light' : 'dark';
    document.documentElement.setAttribute('data-mantine-color-scheme', scheme);
    document.documentElement.dataset.theme = t === 'auto' ? '' : t;
  })
  .catch(() => {});

function LibraryManagerApp() {
  useThemeSync();

  const handleClose = () => {
    getCurrentWindow().close().catch(() => {});
  };

  return (
    <div style={{
      height: '100%',
      display: 'flex',
      flexDirection: 'column',
      overflow: 'hidden',
      userSelect: 'none',
      backgroundColor: 'var(--inspector-background)',
    }}>
      <div style={{
        minHeight: 48,
        padding: '0 12px 0 16px',
        display: 'flex',
        alignItems: 'center',
        borderBottom: '1px solid var(--color-border-secondary)',
        fontSize: 14,
        fontWeight: 600,
        color: 'var(--color-text-primary)',
        // @ts-expect-error Electron-specific CSS property
        WebkitAppRegion: 'drag',
      }}>
        <span style={{ flex: 1 }}>Library Manager</span>
        <button
          onClick={handleClose}
          style={{
            // @ts-expect-error Electron-specific CSS property
            WebkitAppRegion: 'no-drag',
            marginLeft: 'auto',
            width: 24,
            height: 24,
            borderRadius: 4,
            border: 'none',
            background: 'transparent',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            cursor: 'pointer',
            color: 'var(--color-text-secondary)',
            padding: 0,
          }}
        >
          <IconX size={14} />
        </button>
      </div>
      <div style={{ flex: 1, padding: 16, overflowY: 'auto', overflowX: 'hidden' }}>
        <LibraryPanel />
      </div>
    </div>
  );
}

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <MantineProvider theme={theme} defaultColorScheme="dark" cssVariablesSelector=":root:root">
      <LibraryManagerApp />
    </MantineProvider>
  </React.StrictMode>
);
