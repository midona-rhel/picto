import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { Provider, getDefaultStore } from 'jotai';
import { MantineProvider } from '@mantine/core';
import '@mantine/core/styles.css';
import { AppShell } from '../app/AppShell';
import { LibraryGate } from '../features/library/LibraryGate';
import { NotificationHost } from '../shared/ui/NotificationHost/NotificationHost';
import '../app/globals.css';
import { startThemeRuntime } from '../runtime/themeRuntime';

startThemeRuntime();

// One store for the entire app. Controllers and runtime settle mutate
// this same instance via getDefaultStore(), so Provider must use it too.
const store = getDefaultStore();

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <MantineProvider defaultColorScheme="dark">
      <Provider store={store}>
        <NotificationHost />
        <LibraryGate>
          <AppShell />
        </LibraryGate>
      </Provider>
    </MantineProvider>
  </StrictMode>,
);
