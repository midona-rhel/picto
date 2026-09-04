import React from 'react';
import ReactDOM from 'react-dom/client';
import { PictoThemeProvider } from '../runtime/PictoThemeProvider';
import '@mantine/core/styles.css';
import '../app/globals.css';
import { SubscriptionsScreen } from '../features/subscriptions/SubscriptionsScreen';
import { startThemeRuntime } from '../runtime/themeRuntime';
import {
  refreshSubscriptionsWorkspace,
  startSubscriptionsSettle,
} from '../runtime/subscriptionsSettle';
import { libraryInvalidation } from '../runtime/libraryInvalidation';
import { startLocalizedRenderer } from '../i18n';

startThemeRuntime();
libraryInvalidation.start();
const stopSubscriptionsRuntime = startSubscriptionsSettle();
void refreshSubscriptionsWorkspace();
window.addEventListener('beforeunload', () => {
  stopSubscriptionsRuntime();
  libraryInvalidation.stop();
}, { once: true });

const root = ReactDOM.createRoot(document.getElementById('root')!);
startLocalizedRenderer(() => {
  root.render(
    <React.StrictMode>
      <PictoThemeProvider><SubscriptionsScreen standalone /></PictoThemeProvider>
    </React.StrictMode>,
  );
});
