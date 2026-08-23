import { expect, test } from 'vitest';
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

  expect(labels).toContain('Library');
  expect(labels).toContain('File');
  expect(labels).toContain('Organize');

  const byLabel = new Map(template.map((item) => [item.label, item]));
  expect(byLabel.get('Library').submenu.some((item) => item.label === 'Subscriptions…')).toBe(true);
  expect(byLabel.get('File').submenu.some((item) => item.label === 'Import Files…')).toBe(true);
  expect(byLabel.get('Organize').submenu.some((item) => item.label === 'Tags')).toBe(true);
  expect(byLabel.get('View').submenu.some((item) => item.label === 'Tags')).toBe(false);
});
