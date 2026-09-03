import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const source = (relativePath: string) => readFileSync(resolve(process.cwd(), relativePath), 'utf8').replace(/\r\n/g, '\n');

const CORE_THEME_SURFACES = [
  'src/app/AppShell.module.css',
  'src/features/sidebar/Sidebar.module.css',
  'src/features/inspector/Inspector.module.css',
  'src/features/settings/Settings.module.css',
  'src/features/auth/AuthWorkspace.module.css',
  'src/features/subscriptions/SubscriptionsScreen.module.css',
  'src/features/tags/TagManagerScreen.module.css',
  'src/features/tags/TagSelectPanel.module.css',
  'src/features/viewer/document/DocumentViewerShell.module.css',
] as const;

const DARK_ONLY_NEUTRALS = [
  /rgb\(31,\s*32,\s*35\)/i,
  /rgb\(248,\s*249,\s*251\)/i,
  /#f8f9fb\b/i,
  /rgba\(248,\s*249,\s*251,/i,
] as const;

describe('theme surface ownership', () => {
  it('keeps theme colors in tokens instead of core surface modules', () => {
    for (const relativePath of CORE_THEME_SURFACES) {
      const contents = source(relativePath);
      for (const fixedColor of DARK_ONLY_NEUTRALS) {
        expect(contents, `${relativePath} contains ${fixedColor}`).not.toMatch(fixedColor);
      }
    }
  });

  it('derives both rails and text from the active theme family', () => {
    const tokens = source('src/shared/styles/tokens.css');
    expect(tokens).toContain('--color-rail-surface: color-mix(in srgb, var(--theme-contrast) 3%, var(--theme-background));');
    expect(tokens).toContain('--color-text-primary: var(--theme-text);');
    expect(tokens).toContain(':root[data-mantine-color-scheme="light"]');
    expect(tokens).toContain('--theme-text: #2c2f32;');
  });

  it('lets Font Viewer auto mode inherit the application theme', () => {
    const contents = source('src/features/viewer/document/FontViewer.module.css');
    expect(contents).toContain('background: var(--color-bg-app);');
    expect(contents).toContain('--font-page-bg: var(--color-bg-app);');
    expect(contents).toContain('--font-page-fg: var(--color-text-primary);');
  });

  it('keeps tag and folder selection typography and selection neutral', () => {
    const tokens = source('src/shared/styles/tokens.css');
    const overlay = source('src/shared/ui/OverlayShell/OverlayShell.module.css');
    const tags = source('src/features/tags/TagSelectPanel.module.css');
    const tagPanel = source('src/features/tags/TagSelectPanel.tsx');
    const folders = source('src/shared/ui/FolderTree/FolderTree.module.css');

    expect(overlay).toContain('font-family: var(--font-family-ui);');
    expect(tokens).toContain("--font-family-ui: 'SF Pro Text', -apple-system, BlinkMacSystemFont, 'Geist'");
    expect(tokens).toContain(":root[data-platform='mac']");
    expect(tokens).toContain(":root[data-platform='windows'],");
    expect(tokens).toContain("--font-family-ui: 'Geist', system-ui");
    expect(tokens).toContain('--mantine-font-family: var(--font-family-ui);');
    expect(overlay).toContain('.checkBoxChecked {\n  background: var(--color-text-primary);');
    expect(tags).toContain('.tagRowSelected {\n  background: var(--color-selection-bg);');
    expect(tags).toContain('font: var(--font-weight-regular) var(--font-size-md)/24px var(--font-family-ui);');
    expect(tags).toContain('font: var(--font-weight-regular) var(--font-size-md)/25px var(--font-family-ui);');
    expect(overlay).toContain('.header {\n  height: 40px;');
    expect(overlay).toMatch(/\.header \{[\s\S]*?color: var\(--color-text-primary\);[\s\S]*?font-size: var\(--font-size-md\);/);
    expect(tags).toContain('font-family: var(--font-family-ui);\n  color: var(--color-text-tertiary);');
    expect(tags).toContain('opacity: 0.7;');
    expect(tags).not.toContain('opacity: 0.95;');
    expect(tags).not.toContain('--tag-text-dark');
    expect(tagPanel).not.toContain('tagGroupTextColor');
    expect(tags).toContain('color: color-mix(in srgb, var(--tag-color, var(--color-text-primary)) 12%, var(--color-text-primary));');
    expect(tags).toContain(':global(:root[data-mantine-color-scheme="light"]) .tagName');
    expect(tagPanel).toContain("fill={showChecked ? 'currentColor' : 'none'}");
    expect(tagPanel).toContain('fillOpacity={showChecked ? 1 : 0}');
    expect(tags).toContain('.tagBookmark {\n  width: 14px;\n  height: 14px;');
    expect(folders).toContain('.checkSelected {\n  background: var(--color-primary);');
    expect(folders).toMatch(/\.expandBtn \{[\s\S]*?width: 26px;[\s\S]*?height: 26px;/);
    expect(folders).toContain('.expandBtn:hover,\n.expandBtn:focus-visible {\n  color: var(--color-text-primary);\n  background: var(--color-surface-active);');
  });

  it('aligns enabled portal and filter toolbar icon actions', () => {
    const overlay = source('src/shared/ui/OverlayShell/OverlayShell.module.css');
    const filterLogic = source('src/shared/ui/FilterLogicTabs/FilterLogicTabs.module.css');
    const gridFilters = source('src/features/grid/GridFilterMenu.module.css');
    expect(overlay).toMatch(/\.pinBtn \{[\s\S]*?color: var\(--color-text-primary\);/);
    expect(overlay).toMatch(/\.viewTab \{[\s\S]*?color: var\(--color-text-primary\);/);
    expect(filterLogic).toMatch(/\.button \{[\s\S]*?color: var\(--color-text-primary\);/);
    expect(gridFilters).toMatch(/\.filterRight \{[\s\S]*?height: 32px;[\s\S]*?padding-top: 6px;/);
    expect(gridFilters).toMatch(/\.filterAction \{[\s\S]*?width: 24px;[\s\S]*?height: 24px;/);
    expect(gridFilters).toMatch(/\.addButton \{[\s\S]*?width: 24px;/);
  });

  it('uses the canonical typography roles for application chrome', () => {
    const tokens = source('src/shared/styles/tokens.css');
    const modal = source('src/shared/ui/GlassModal/GlassModal.module.css');
    const sidebarRows = source('src/shared/ui/SidebarRow/SidebarRow.module.css');
    const sidebar = source('src/features/sidebar/Sidebar.module.css');
    const librarySwitcher = source('src/features/library/LibrarySwitcherButton.module.css');
    const appShell = source('src/app/AppShell.module.css');
    const tooltip = source('src/shared/ui/KbdTooltip/KbdTooltip.module.css');

    expect(tokens).toContain('--font-size-caption: 11px;');
    expect(tokens).toContain('--font-size-sm: 12px;');
    expect(tokens).toContain('--font-size-md: 13px;');
    expect(tokens).toContain('--font-size-lg: 14px;');
    expect(tokens).toContain('--font-weight-bold: 600;');
    expect(tokens).toContain('--color-text-secondary: color-mix(in srgb, var(--theme-text) 70%, transparent);');
    expect(tokens).toContain('--color-text-tertiary: color-mix(in srgb, var(--theme-text) 60%, transparent);');
    expect(modal).toContain('font-size: var(--font-size-md);');
    expect(modal).toContain('.title {\n  flex: 1;\n  font-size: var(--font-size-lg);\n  font-weight: var(--font-weight-bold);');
    expect(modal).toContain('.fieldLabel {\n  font-size: var(--font-size-md);');
    expect(sidebarRows).toContain('font-size: var(--font-size-md);');
    expect(appShell).toContain("--sidebar-font-family: 'Picto Inspector Roboto', var(--font-family-ui);");
    expect(appShell).toContain("--sidebar-mono-font-family: 'Fira Mono', var(--font-family-mono);");
    expect(sidebar).toContain('font-family: var(--sidebar-font-family, var(--font-family-ui));');
    expect(sidebar).toContain('var(--sidebar-font-family, var(--font-family-ui))');
    expect(librarySwitcher).toContain('font-family: var(--sidebar-font-family, var(--font-family-ui));');
    expect(sidebarRows).toContain('font-family: var(--sidebar-mono-font-family, var(--font-family-mono));');
    expect(sidebarRows).toContain('.active .count,\n.selected .count {\n  color: var(--sidebar-text-primary, var(--color-text-primary));');
    expect(sidebarRows).not.toContain('box-shadow: inset 0 0 0 1px var(--color-border-focus);');
    expect(tooltip).toContain('font-size: var(--font-size-md);');
    expect(tooltip).toContain('font-size: var(--font-size-xs);');
  });

  it('uses the canonical action and icon button geometry', () => {
    const tokens = source('src/shared/styles/tokens.css');
    const actions = source('src/shared/styles/actionButton.module.css');
    const icons = source('src/shared/styles/iconButton.module.css');
    const subscriptions = source('src/features/subscriptions/SubscriptionsScreen.module.css');
    const libraries = source('src/features/library/LibraryManager.module.css');

    expect(tokens).toContain('--action-button-height: 30px;');
    expect(tokens).toContain('--icon-button-size: 24px;');
    expect(actions).toContain('font-size: var(--font-size-md);');
    expect(actions).toContain('font-weight: var(--font-weight-medium);');
    expect(actions).toContain('padding: 0 20px;');
    expect(icons).toContain('max-width: 18px;');
    expect(subscriptions).toContain("composes: btn btnPrimary from '../../shared/styles/actionButton.module.css';");
    expect(libraries).toContain("composes: btn btnPrimary from '../../shared/styles/actionButton.module.css';");
  });

  it('keeps feature surfaces on the shared Picto role hierarchy', () => {
    const select = source('src/shared/ui/CmSelect/CmSelect.module.css');
    const tokens = source('src/shared/styles/tokens.css');
    const inspectorSection = source('src/shared/ui/InspectorSection/InspectorSection.module.css');
    const propertyRow = source('src/shared/ui/PropertyRow/PropertyRow.module.css');
    const toolbar = source('src/features/grid/GridToolbar.module.css');
    const appShell = source('src/app/AppShell.module.css');
    const subscriptions = source('src/features/subscriptions/SubscriptionsScreen.module.css');
    const duplicates = source('src/features/duplicates/DuplicatesScreen.module.css');
    const tagManager = source('src/features/tags/TagManagerScreen.module.css');

    expect(select.match(/font-size: var\(--font-size-md\);/g)?.length).toBeGreaterThanOrEqual(2);
    expect(inspectorSection).toContain('font-size: 12px;');
    expect(inspectorSection).toContain('font-weight: var(--font-weight-bold);');
    expect(inspectorSection).toContain('color: var(--inspector-text-tertiary, var(--color-text-tertiary));');
    expect(inspectorSection).toContain('.chevronExpanded {\n  opacity: 0;');
    expect(inspectorSection).toContain('.header:hover .chevron:not(.chevronExpanded) {\n  opacity: 0.55;');
    expect(propertyRow).toContain('font-size: 11px;');
    expect(propertyRow).toContain('grid-template-columns: 88px minmax(0, 1fr);');
    expect(propertyRow).toContain('column-gap: 12px;');
    expect(propertyRow).toContain('text-align: right;');
    expect(propertyRow.match(/color: var\(--inspector-text-primary, var\(--color-text-primary\)\);/g)).toHaveLength(2);
    expect(propertyRow).toContain('font-family: var(--inspector-mono-font-family, var(--font-family-mono));');
    expect(propertyRow).toContain('opacity: 0.8;');
    expect(tokens).toContain("font-family: 'Fira Mono';");
    expect(tokens).toContain("url('../assets/fonts/FiraMono-Regular.woff2')");
    expect(tokens).toContain("font-family: 'Picto Inspector Roboto';");
    expect(tokens).toContain("url('../assets/fonts/Roboto-Variable.woff2')");
    expect(toolbar).toContain('font-size: var(--font-size-md);');
    expect(appShell).toContain('font-weight: var(--font-weight-regular);');
    expect(appShell).toContain("--inspector-font-family: 'Picto Inspector Roboto', var(--font-family-ui);");
    expect(appShell).toContain("--inspector-mono-font-family: 'Fira Mono', var(--font-family-mono);");
    expect(subscriptions).toContain('font-size: var(--font-size-caption);');
    expect(subscriptions).toContain('font-size: var(--font-size-md);');
    expect(duplicates).toContain('font-size: var(--font-size-md);');
    expect(tagManager).toContain('font: var(--font-size-md)/26px var(--font-family-ui);');
  });

  it('gives the settings window the same directional macOS rim as the main window', () => {
    const appShell = source('src/app/AppShell.module.css');
    const settings = source('src/features/settings/Settings.module.css');

    for (const declaration of [
      'border-top: var(--glass-border-top);',
      'border-right: var(--glass-border-side);',
      'border-bottom: var(--glass-border-bottom);',
      'border-left: var(--glass-border-side);',
      'border-radius: 10px;',
    ]) {
      expect(appShell).toContain(declaration);
      expect(settings).toContain(declaration);
    }
    expect(settings).toContain(":global(html[data-platform='mac']) .root::after");
  });
});
