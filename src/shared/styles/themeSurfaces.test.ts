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
});
