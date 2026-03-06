import React from 'react';
import ReactDOM from 'react-dom/client';
import { MantineProvider } from '@mantine/core';
import { api } from '#desktop/api';
import { DetailWindow } from '#features/viewer/components';
import '@mantine/core/styles.css';
import './styles/globals.css';

// Sync color scheme from app settings so CSS custom properties resolve correctly.
// Default is dark (matches :root in globals.css); light override kicks in via attribute.
(api.settings.get() as Promise<{ colorScheme?: string }>)
  .then((settings) => {
    const scheme = settings?.colorScheme === 'light' ? 'light' : 'dark';
    document.documentElement.setAttribute('data-mantine-color-scheme', scheme);
  })
  .catch(() => {});

const hash = new URLSearchParams(window.location.search).get('hash');

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <MantineProvider defaultColorScheme="dark" cssVariablesSelector=":root:root">
      {hash ? <DetailWindow hash={hash} /> : <div style={{ color: '#888', padding: 24 }}>No image hash provided</div>}
    </MantineProvider>
  </React.StrictMode>
);
