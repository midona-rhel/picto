import React from 'react';
import ReactDOM from 'react-dom/client';
import { PictoThemeProvider } from '../runtime/PictoThemeProvider';
import '@mantine/core/styles.css';
import '../app/globals.css';
import { LibraryManager } from '../features/library/LibraryManager';
import { startThemeRuntime } from '../runtime/themeRuntime';
import { startLocalizedRenderer } from '../i18n';

startThemeRuntime();

const root = ReactDOM.createRoot(document.getElementById('root')!);
startLocalizedRenderer(() => {
  root.render(
    <React.StrictMode>
      <PictoThemeProvider cssVariablesSelector=":root:root">
        <LibraryManager />
      </PictoThemeProvider>
    </React.StrictMode>,
  );
});
