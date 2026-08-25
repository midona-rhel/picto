import { render, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { describe, expect, it, vi } from 'vitest';
import { OverlayShell } from './OverlayShell';

vi.mock('../KbdTooltip', () => ({ KbdTooltip: ({ children }: { children: ReactNode }) => children }));

describe('OverlayShell placement', () => {
  it('uses left/top coordinates for a below-anchor workflow', async () => {
    render(
      <OverlayShell
        open
        onClose={vi.fn()}
        width={340}
        height={480}
        anchorPosition={{ x: 120, y: 52 }}
        anchorPlacement="below"
      >
        Panel
      </OverlayShell>,
    );

    await waitFor(() => expect(document.querySelector('[data-overlay-shell]')).not.toBeNull());
    const panel = document.querySelector('[data-overlay-shell]') as HTMLElement;
    expect(panel.style.left).toBe('120px');
    expect(panel.style.top).toBe('52px');
    expect(panel.style.right).toBe('');
  });

  it('aligns an above-anchor workflow to the trigger left edge', async () => {
    render(
      <OverlayShell
        open
        onClose={vi.fn()}
        width={340}
        height={200}
        anchorPosition={{ x: 120, y: 400 }}
        anchorPlacement="above"
      >
        Panel
      </OverlayShell>,
    );

    await waitFor(() => expect(document.querySelector('[data-overlay-shell]')).not.toBeNull());
    const panel = document.querySelector('[data-overlay-shell]') as HTMLElement;
    expect(panel.style.left).toBe('120px');
    expect(panel.style.top).toBe('196px');
    expect(panel.style.right).toBe('');
  });
});
