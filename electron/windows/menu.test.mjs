import { expect, test, vi } from 'vitest';
import { createMenuManager } from './menu.mjs';

function buildMenuTemplate(platform = 'linux', overrides = {}) {
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
    platform,
    ...overrides,
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

test('the burger menu exposes every cross-platform macOS application-menu section', () => {
  const macLabels = buildMenuTemplate('darwin').map((item) => item.label);
  const burgerLabels = buildMenuTemplate('win32').map((item) => item.label);

  expect(burgerLabels).toEqual(macLabels.filter((label) => label !== 'Picto'));
});

test('the log window is available in release menus and routes to the main window', () => {
  const sendToMainWindow = vi.fn();
  const template = buildMenuTemplate('win32', { sendToMainWindow });
  const view = template.find((item) => item.label === 'View');
  const logWindow = view.submenu.find((item) => item.label === 'Log Window');

  expect(logWindow.accelerator).toBe('CmdOrCtrl+Shift+L');
  logWindow.click();
  expect(sendToMainWindow).toHaveBeenCalledWith('menu:toggle-diagnostics');
});

test('library and file commands invoke live application workflows', () => {
  const openLibraryManager = vi.fn();
  const sendToMainWindow = vi.fn();
  const template = buildMenuTemplate('win32', { openLibraryManager, sendToMainWindow });
  const byLabel = new Map(template.map((item) => [item.label, item]));
  const library = new Map(byLabel.get('Library').submenu.filter((item) => item.label).map((item) => [item.label, item]));
  const file = new Map(byLabel.get('File').submenu.filter((item) => item.label).map((item) => [item.label, item]));

  library.get('New Library…').click();
  file.get('Import Files…').click();
  file.get('Import Folder…').click();
  file.get('Export Originals…').click();
  file.get('Export As…').click();

  expect(openLibraryManager).toHaveBeenCalledOnce();
  expect(sendToMainWindow.mock.calls).toEqual([
    ['menu:import-files'],
    ['menu:import-folder'],
    ['menu:export-basic'],
    ['menu:export-advanced'],
  ]);
});
