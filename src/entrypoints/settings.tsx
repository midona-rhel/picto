import React from 'react';
import ReactDOM from 'react-dom/client';
import { MantineProvider } from '@mantine/core';
import '@mantine/core/styles.css';
import '../app/globals.css';
import { Settings } from '../features/settings/Settings';
import { startThemeRuntime } from '../runtime/themeRuntime';
import { startLocalizedRenderer } from '../i18n';

startThemeRuntime();

const root = ReactDOM.createRoot(document.getElementById('root')!);
startLocalizedRenderer(() => {
  root.render(
    <React.StrictMode>
      <MantineProvider defaultColorScheme="dark" cssVariablesSelector=":root:root">
        <Settings />
      </MantineProvider>
    </React.StrictMode>,
  );
});
