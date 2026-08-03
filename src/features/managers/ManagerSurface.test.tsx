import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { isManagerNodeId, ManagerSurface } from './ManagerSurface';

vi.mock('../duplicates/DuplicatesScreen', () => ({
  DuplicatesScreen: () => <div>Duplicate manager</div>,
}));
vi.mock('../subscriptions/SubscriptionsScreen', () => ({
  SubscriptionsScreen: () => <div>Subscription manager</div>,
}));

describe('ManagerSurface', () => {
  it('keeps manager nodes out of grid dispatch and routes them explicitly', () => {
    expect(isManagerNodeId('system:duplicates')).toBe(true);
    expect(isManagerNodeId('system:subscriptions')).toBe(true);
    expect(isManagerNodeId('system:tag_manager')).toBe(true);
    expect(isManagerNodeId('system:active')).toBe(false);

    const view = render(<ManagerSurface nodeId="system:duplicates" />);
    expect(screen.getByText('Duplicate manager')).toBeInTheDocument();

    view.rerender(<ManagerSurface nodeId="system:subscriptions" />);
    expect(screen.getByText('Subscription manager')).toBeInTheDocument();
  });
});
