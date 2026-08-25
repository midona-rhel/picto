import React from 'react';
import ReactDOM from 'react-dom/client';
import '../app/globals.css';
import { Settings } from '../features/settings/Settings';
import { startThemeRuntime } from '../runtime/themeRuntime';

startThemeRuntime();

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <Settings />
  </React.StrictMode>,
);
