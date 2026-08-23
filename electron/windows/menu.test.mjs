import test from 'node:test';
import assert from 'node:assert/strict';
import { createMenuManager } from './menu.mjs';

function buildMenuTemplate() {
  let template = null;
  const manager = createMenuManager({
    app: { name: 'Picto' },
    Menu: {
      buildFromTemplate: (next) => {
        template = next;
        return next;
      },
      setApplicationMenu: () => {},
    },
    dialog: {},
    isDev: false,
    getCachedConfig: () => ({ pinnedLibraries: [], libraryHistory: [] }),
    saveGlobalConfig: async () => {},
    getCurrentLibraryRoot: () => null,
    libraryDisplayName: (path) => path,
    switchLibrary: async () => {},
    openSettingsWindow: () => {},
    openSubscriptionsWindow: () => {},
    openLibraryManager: () => {},
    sendToFocusedWindow: () => {},
    sendToMainWindow: () => {},
  });
  manager.buildAppMenu();
  return template;
}

test('groups existing Picto actions into library, file, and organize menus', () => {
  const template = buildMenuTemplate();
  const labels = template.map((item) => item.label);

  assert.ok(labels.includes('Library'));
  assert.ok(labels.includes('File'));
  assert.ok(labels.includes('Organize'));

  const byLabel = new Map(template.map((item) => [item.label, item]));
  assert.ok(byLabel.get('Library').submenu.some((item) => item.label === 'Subscriptions…'));
  assert.ok(byLabel.get('File').submenu.some((item) => item.label === 'Import Files…'));
  assert.ok(byLabel.get('Organize').submenu.some((item) => item.label === 'Tags'));
  assert.ok(!byLabel.get('View').submenu.some((item) => item.label === 'Tags'));
});
