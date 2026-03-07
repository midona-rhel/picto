import React from 'react';
import ReactDOM from 'react-dom/client';
import { MantineProvider, createTheme, rem } from '@mantine/core';
import { Notifications } from '@mantine/notifications';
import { api } from '#desktop/api';
import { SubscriptionsWindow } from '#features/subscriptions/components';
import { useThemeSync } from '../shared/hooks/useThemeSync';
import '@mantine/core/styles.css';
import '@mantine/notifications/styles.css';
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
    Textarea: { defaultProps: { size: 'sm' } },
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
  },
});

(api.settings.get() as Promise<{ colorScheme?: string; theme?: string }>)
  .then((settings) => {
    const selectedTheme = settings?.theme ?? (settings?.colorScheme === 'light' ? 'light' : 'dark');
    const scheme = selectedTheme === 'auto'
      ? 'dark'
      : (selectedTheme === 'light' || selectedTheme === 'lightgray') ? 'light' : 'dark';
    document.documentElement.setAttribute('data-mantine-color-scheme', scheme);
    document.documentElement.dataset.theme = selectedTheme === 'auto' ? '' : selectedTheme;
  })
  .catch(() => {});

function SubscriptionsApp() {
  useThemeSync();
  return <SubscriptionsWindow />;
}

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <MantineProvider theme={theme} defaultColorScheme="dark" cssVariablesSelector=":root:root">
      <Notifications />
      <SubscriptionsApp />
    </MantineProvider>
  </React.StrictMode>,
);
