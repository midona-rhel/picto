import React from 'react';
import ReactDOM from 'react-dom/client';
import '../app/globals.css';
import { SubscriptionsScreen } from '../features/subscriptions/SubscriptionsScreen';
import { startThemeRuntime } from '../runtime/themeRuntime';
import {
  refreshSubscriptionsWorkspace,
  startSubscriptionsSettle,
} from '../runtime/subscriptionsSettle';
import { libraryInvalidation } from '../runtime/libraryInvalidation';

startThemeRuntime();
libraryInvalidation.start();
const stopSubscriptionsRuntime = startSubscriptionsSettle();
void refreshSubscriptionsWorkspace();
window.addEventListener('beforeunload', () => {
  stopSubscriptionsRuntime();
  libraryInvalidation.stop();
}, { once: true });

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <SubscriptionsScreen standalone />
  </React.StrictMode>,
);
