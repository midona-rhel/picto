import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { Provider, getDefaultStore } from 'jotai';
import { PictoThemeProvider } from '../runtime/PictoThemeProvider';
import '@mantine/core/styles.css';
import { AppShell } from '../app/AppShell';
import { LibraryGate } from '../features/library/LibraryGate';
import { NotificationHost } from '../shared/ui/NotificationHost/NotificationHost';
import { ApplicationUpdateHost } from '../features/updates/ApplicationUpdateHost';
import '../app/globals.css';
import { startThemeRuntime } from '../runtime/themeRuntime';
import { startLocalizedRenderer } from '../i18n';

startThemeRuntime();

// One store for the entire app. Controllers and runtime settle mutate
// this same instance via getDefaultStore(), so Provider must use it too.
const store = getDefaultStore();

const root = createRoot(document.getElementById('root')!);
startLocalizedRenderer(() => {
  root.render(
    <StrictMode>
      <PictoThemeProvider>
        <Provider store={store}>
          <NotificationHost />
          <ApplicationUpdateHost />
          <LibraryGate>
            <AppShell />
          </LibraryGate>
        </Provider>
      </PictoThemeProvider>
    </StrictMode>,
  );
});
