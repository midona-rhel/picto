import { act, render, screen } from '@testing-library/react';
import { useMantineColorScheme } from '@mantine/core';
import { expect, it, vi } from 'vitest';
import { PictoThemeProvider } from './PictoThemeProvider';
import { applyTheme } from './themeRuntime';

function Scheme() {
  return <span>{useMantineColorScheme().colorScheme}</span>;
}

it('ignores stale Mantine storage and follows library theme changes without replacing document attributes', () => {
  localStorage.setItem('mantine-color-scheme-value', 'dark');
  applyTheme('light', false, 'windows');
  const readStorage = vi.spyOn(localStorage, 'getItem');
  render(<PictoThemeProvider><Scheme /></PictoThemeProvider>);
  expect(screen.getByText('light')).toBeInTheDocument();
  expect(document.documentElement.dataset.mantineColorScheme).toBe('light');
  act(() => { applyTheme('purple', false, 'windows'); });
  expect(screen.getByText('dark')).toBeInTheDocument();
  expect(document.documentElement.dataset.theme).toBe('purple');
  expect(document.documentElement.dataset.mantineColorScheme).toBe('dark');
  expect(readStorage).not.toHaveBeenCalled();
  readStorage.mockRestore();
});
