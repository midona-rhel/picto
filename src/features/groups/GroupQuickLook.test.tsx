import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { GroupQuickLookContent } from './GroupQuickLook';

vi.mock('./GroupSurface', () => ({
  GroupSurface: (props: { groupId: number; presentation: string }) => (
    <div data-testid="group-surface" data-id={props.groupId} data-presentation={props.presentation} />
  ),
}));

describe('GroupQuickLookContent', () => {
  it('renders the inset group reader without owning the persistent overlay', () => {
    render(
      <GroupQuickLookContent
        groupId={7}
        currentIndex={2}
        totalCount={10}
        onNavigate={vi.fn()}
        onClose={vi.fn()}
      />,
    );

    expect(screen.getByTestId('group-surface').closest('[data-group-quick-look]')).toBeInTheDocument();
    expect(document.body.querySelector('[data-quick-look-overlay]')).toBeNull();
    expect(screen.getByTestId('group-surface')).toHaveAttribute('data-id', '7');
    expect(screen.getByTestId('group-surface')).toHaveAttribute('data-presentation', 'quicklook');
  });
});
