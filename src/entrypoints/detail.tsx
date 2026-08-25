import React from 'react';
import ReactDOM from 'react-dom/client';
import { MantineProvider } from '@mantine/core';
import { DetailWindow } from '../features/viewer/DetailWindow';
import '@mantine/core/styles.css';
import '../shared/styles/tokens.css';
import '../app/globals.css';
import { startThemeRuntime } from '../runtime/themeRuntime';

startThemeRuntime();

const hash = new URLSearchParams(window.location.search).get('hash');

function DetailApp() {
  return (
    <MantineProvider defaultColorScheme="dark" cssVariablesSelector=":root:root">
      {hash ? (
        <DetailWindow hash={hash} />
      ) : (
        <div style={{ color: 'var(--color-text-secondary)', padding: 24 }}>No image hash provided</div>
      )}
    </MantineProvider>
  );
}

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <DetailApp />
  </React.StrictMode>,
);
