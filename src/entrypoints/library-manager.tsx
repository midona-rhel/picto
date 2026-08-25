import React from 'react';
import ReactDOM from 'react-dom/client';
import '../app/globals.css';
import { LibraryManager } from '../features/library/LibraryManager';
import { startThemeRuntime } from '../runtime/themeRuntime';

startThemeRuntime();

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <LibraryManager />
  </React.StrictMode>,
);
