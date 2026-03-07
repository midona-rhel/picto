import React from "react";
import ReactDOM from "react-dom/client";
import { MantineProvider, createTheme, rem } from "@mantine/core";
import { Notifications } from "@mantine/notifications";
import App from "../app/App";
import { AppErrorBoundary } from '#features/layout/components';
import "@mantine/core/styles.css";
import "@mantine/notifications/styles.css";
import "../shared/styles/globals.css";

// Compact theme matching the sidebar aesthetic — small text, tight spacing, subtle radii.
// The left sidebar (NavSection) is the sizing reference:
//   Section labels: xs (10px), body text: sm (12px), primary: md (13px), icons: 14–18px.
// Component defaults are set here so individual controls don't need size= overrides.
const theme = createTheme({
  // Warm-tinted dark palette matching the context menu (rgba(30, 30, 34))
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
  headings: {
    sizes: {
      h1: { fontSize: rem(18), lineHeight: '1.3' },
      h2: { fontSize: rem(16), lineHeight: '1.3' },
      h3: { fontSize: rem(14), lineHeight: '1.3' },
      h4: { fontSize: rem(13), lineHeight: '1.3' },
    },
  },
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
    Badge: { defaultProps: { size: 'sm' } },
    Slider: { defaultProps: { size: 'xs' } },
    Tabs: { defaultProps: { variant: 'default' } },
  },
});

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <MantineProvider theme={theme} defaultColorScheme="dark" cssVariablesSelector=":root:root">
      <Notifications />
      <AppErrorBoundary>
        <App />
      </AppErrorBoundary>
    </MantineProvider>
  </React.StrictMode>
);
