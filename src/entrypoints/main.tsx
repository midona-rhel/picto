import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { Provider, getDefaultStore } from 'jotai';
import { MantineProvider } from '@mantine/core';
import { Notifications } from '@mantine/notifications';
import '@mantine/notifications/styles.css';
import '@mantine/core/styles.css';
import { AppShell } from '../app/AppShell';
import '../app/globals.css';

// One store for the entire app. Controllers and runtime settle mutate
// this same instance via getDefaultStore(), so Provider must use it too.
const store = getDefaultStore();

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <MantineProvider defaultColorScheme="dark">
      <Provider store={store}>
        <Notifications position="top-right" />
        <AppShell />
      </Provider>
    </MantineProvider>
  </StrictMode>,
);
