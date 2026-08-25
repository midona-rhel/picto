import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import type { MenuCustom } from '../../shared/ui/ContextMenu/ContextMenu';
import { buildViewMenuEntries } from './GridViewMenu';

describe('GridViewMenu', () => {
  it('keeps transient grayscale out of persistent display preferences', () => {
    const display = buildViewMenuEntries().find(
      (entry): entry is MenuCustom => 'custom' in entry && entry.key === 'display-toggles',
    );
    expect(display).toBeDefined();
    render(display!.render());

    expect(screen.queryByText('Grayscale Preview')).not.toBeInTheDocument();
  });
});
