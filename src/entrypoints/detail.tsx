import React from 'react';
import ReactDOM from 'react-dom/client';
import { PictoThemeProvider } from '../runtime/PictoThemeProvider';
import { DetailWindow } from '../features/viewer/DetailWindow';
import { GroupDetailWindow } from '../features/groups/GroupDetailWindow';
import '@mantine/core/styles.css';
import '../shared/styles/tokens.css';
import '../app/globals.css';
import { startThemeRuntime } from '../runtime/themeRuntime';
import { startLocalizedRenderer, t } from '../i18n';

startThemeRuntime();

const hash = new URLSearchParams(window.location.search).get('hash');
const itemIdParam = new URLSearchParams(window.location.search).get('item_id');
const itemId = itemIdParam == null ? null : Number(itemIdParam);

function DetailApp() {
  return (
    <PictoThemeProvider cssVariablesSelector=":root:root">
      {Number.isSafeInteger(itemId) && itemId! > 0 ? (
        <GroupDetailWindow groupId={itemId!} />
      ) : hash ? (
        <DetailWindow hash={hash} />
      ) : (
        <div style={{ color: 'var(--color-text-secondary)', padding: 24 }}>{t("No detail item provided")}</div>
      )}
    </PictoThemeProvider>
  );
}

const root = ReactDOM.createRoot(document.getElementById('root')!);
startLocalizedRenderer(() => {
  root.render(
    <React.StrictMode>
      <DetailApp />
    </React.StrictMode>,
  );
});
