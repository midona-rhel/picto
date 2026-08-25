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
    expect(tokens).toContain(":root[data-platform='mac']");
    expect(tokens).toContain(":root[data-platform='windows']");
    expect(tokens).toContain(":root[data-platform='linux']");
    expect(overlay).toContain('.checkBoxChecked {\n  background: var(--color-text-primary);');
    expect(tags).toContain('.tagRowSelected {\n  background: var(--color-surface-active);');
    expect(tags).not.toContain('var(--color-selection-bg)');
    expect(tags).not.toContain('--tag-text-dark');
    expect(tagPanel).not.toContain('tagGroupTextColor');
    expect(tagPanel).toContain('fillOpacity={showChecked && !onApplyTagFilter ? 0.58 : 0.28}');
    expect(folders).toContain('.rowSelected {\n  background: var(--color-surface-active);');
  });
});
