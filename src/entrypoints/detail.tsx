import React from 'react';
import ReactDOM from 'react-dom/client';
import { MantineProvider } from '@mantine/core';
import { DetailWindow } from '#features/viewer/components/DetailWindow';
import { useThemeSync, useDerivedColorScheme } from '../shared/hooks/useThemeSync';
import '@mantine/core/styles.css';
import '../shared/styles/globals.css';

const hash = new URLSearchParams(window.location.search).get('hash');

function DetailApp() {
  useThemeSync();
  const colorScheme = useDerivedColorScheme();
  return (
    <MantineProvider forceColorScheme={colorScheme} cssVariablesSelector=":root:root">
      {hash ? <DetailWindow hash={hash} /> : <div style={{ color: '#888', padding: 24 }}>No image hash provided</div>}
    </MantineProvider>
  );
}

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <DetailApp />
  </React.StrictMode>
);
