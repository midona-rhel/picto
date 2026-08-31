import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

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
      const source = readFileSync(resolve(process.cwd(), relativePath), 'utf8');
      for (const fixedColor of DARK_ONLY_NEUTRALS) {
        expect(source, `${relativePath} contains ${fixedColor}`).not.toMatch(fixedColor);
      }
    }
  });

  it('derives both rails and text from the active theme family', () => {
    const tokens = readFileSync(resolve(process.cwd(), 'src/shared/styles/tokens.css'), 'utf8');
    expect(tokens).toContain('--color-rail-surface: color-mix(in srgb, var(--theme-contrast) 3%, var(--theme-background));');
    expect(tokens).toContain('--color-text-primary: var(--theme-text);');
    expect(tokens).toContain(':root[data-mantine-color-scheme="light"]');
    expect(tokens).toContain('--theme-text: #2c2f32;');
  });

  it('lets Font Viewer auto mode inherit the application theme', () => {
    const source = readFileSync(resolve(process.cwd(), 'src/features/viewer/document/FontViewer.module.css'), 'utf8');
    expect(source).toContain('background: var(--color-bg-app);');
    expect(source).toContain('--font-page-bg: var(--color-bg-app);');
    expect(source).toContain('--font-page-fg: var(--color-text-primary);');
  });

  it('keeps tag and folder selection typography and selection neutral', () => {
    const tokens = readFileSync(resolve(process.cwd(), 'src/shared/styles/tokens.css'), 'utf8');
    const overlay = readFileSync(resolve(process.cwd(), 'src/shared/ui/OverlayShell/OverlayShell.module.css'), 'utf8');
    const tags = readFileSync(resolve(process.cwd(), 'src/features/tags/TagSelectPanel.module.css'), 'utf8');
    const tagPanel = readFileSync(resolve(process.cwd(), 'src/features/tags/TagSelectPanel.tsx'), 'utf8');
    const folders = readFileSync(resolve(process.cwd(), 'src/shared/ui/FolderTree/FolderTree.module.css'), 'utf8');

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
    expect(tags).not.toContain('opacity: 0.95;');
    expect(tags).not.toContain('--tag-text-dark');
    expect(tagPanel).not.toContain('tagGroupTextColor');
    expect(tagPanel).toContain('fillOpacity={showChecked && !onApplyTagFilter ? 0.58 : 0.28}');
    expect(folders).toContain('.checkSelected {\n  background: var(--color-primary);');
  });

  it('uses the canonical typography roles for application chrome', () => {
    const tokens = readFileSync(resolve(process.cwd(), 'src/shared/styles/tokens.css'), 'utf8');
    const modal = readFileSync(resolve(process.cwd(), 'src/shared/ui/GlassModal/GlassModal.module.css'), 'utf8');
    const sidebarRows = readFileSync(resolve(process.cwd(), 'src/shared/ui/SidebarRow/SidebarRow.module.css'), 'utf8');
    const sidebar = readFileSync(resolve(process.cwd(), 'src/features/sidebar/Sidebar.module.css'), 'utf8');
    const librarySwitcher = readFileSync(resolve(process.cwd(), 'src/features/library/LibrarySwitcherButton.module.css'), 'utf8');
    const appShell = readFileSync(resolve(process.cwd(), 'src/app/AppShell.module.css'), 'utf8');
    const tooltip = readFileSync(resolve(process.cwd(), 'src/shared/ui/KbdTooltip/KbdTooltip.module.css'), 'utf8');

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
    const tokens = readFileSync(resolve(process.cwd(), 'src/shared/styles/tokens.css'), 'utf8');
    const actions = readFileSync(resolve(process.cwd(), 'src/shared/styles/actionButton.module.css'), 'utf8');
    const icons = readFileSync(resolve(process.cwd(), 'src/shared/styles/iconButton.module.css'), 'utf8');
    const subscriptions = readFileSync(resolve(process.cwd(), 'src/features/subscriptions/SubscriptionsScreen.module.css'), 'utf8');
    const libraries = readFileSync(resolve(process.cwd(), 'src/features/library/LibraryManager.module.css'), 'utf8');

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
    const select = readFileSync(resolve(process.cwd(), 'src/shared/ui/CmSelect/CmSelect.module.css'), 'utf8');
    const tokens = readFileSync(resolve(process.cwd(), 'src/shared/styles/tokens.css'), 'utf8');
    const inspectorSection = readFileSync(resolve(process.cwd(), 'src/shared/ui/InspectorSection/InspectorSection.module.css'), 'utf8');
    const propertyRow = readFileSync(resolve(process.cwd(), 'src/shared/ui/PropertyRow/PropertyRow.module.css'), 'utf8');
    const toolbar = readFileSync(resolve(process.cwd(), 'src/features/grid/GridToolbar.module.css'), 'utf8');
    const appShell = readFileSync(resolve(process.cwd(), 'src/app/AppShell.module.css'), 'utf8');
    const subscriptions = readFileSync(resolve(process.cwd(), 'src/features/subscriptions/SubscriptionsScreen.module.css'), 'utf8');
    const duplicates = readFileSync(resolve(process.cwd(), 'src/features/duplicates/DuplicatesScreen.module.css'), 'utf8');
    const tagManager = readFileSync(resolve(process.cwd(), 'src/features/tags/TagManagerScreen.module.css'), 'utf8');

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
    const appShell = readFileSync(resolve(process.cwd(), 'src/app/AppShell.module.css'), 'utf8');
    const settings = readFileSync(resolve(process.cwd(), 'src/features/settings/Settings.module.css'), 'utf8');

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
