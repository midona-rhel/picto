import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { GUIDED_TOUR_STEPS } from './tutorialSteps';

const runtime = vi.hoisted(() => ({
  startTutorialSession: vi.fn().mockResolvedValue(undefined),
  finishTutorialSession: vi.fn().mockResolvedValue(undefined),
  executeTutorialActions: vi.fn().mockResolvedValue(undefined),
  waitForTutorialCondition: vi.fn().mockResolvedValue(undefined),
}));
vi.mock('./tutorialRuntime', () => runtime);

import { HelpOverlay } from './HelpOverlay';

function renderHelp() {
  return render(
    <div>
      <header data-help-region="sidebar" />
      <aside data-help-id="sidebar"><button data-help-id="sidebar-library-switcher">Library</button></aside>
      <main data-help-id="workspace" />
      <aside data-help-id="inspector" />
      <HelpOverlay />
    </div>,
  );
}

describe('guided tour', () => {
  beforeEach(() => { vi.clearAllMocks(); });

  it('uses one declarative sequence covering every approved chapter', () => {
    expect(new Set(GUIDED_TOUR_STEPS.map((entry) => entry.id)).size).toBe(GUIDED_TOUR_STEPS.length);
    expect(new Set(GUIDED_TOUR_STEPS.map((entry) => entry.chapter))).toEqual(new Set([
      'sidebar', 'all-media', 'inspector', 'inbox', 'folders', 'tags',
      'collections', 'subscriptions', 'duplicates', 'trash',
    ]));
  });

  it('starts the ephemeral library before showing the real-interface tour', async () => {
    renderHelp();
    fireEvent.click(screen.getByRole('button', { name: 'Help and tutorial' }));
    fireEvent.click(screen.getByRole('button', { name: 'Start guided tour' }));
    await waitFor(() => expect(runtime.startTutorialSession).toHaveBeenCalledOnce());
    expect(await screen.findByRole('dialog')).toHaveTextContent('Browse your library');
    expect(runtime.executeTutorialActions).toHaveBeenCalledWith(GUIDED_TOUR_STEPS[0].enter);
  });

  it('opens the launcher with question mark outside editable controls', () => {
    renderHelp();
    fireEvent.keyDown(window, { key: '?' });
    expect(screen.getByText('Explore Picto')).toBeInTheDocument();
  });

  it('only exposes coachmark navigation and restores the original session on exit', async () => {
    renderHelp();
    fireEvent.click(screen.getByRole('button', { name: 'Help and tutorial' }));
    fireEvent.click(screen.getByRole('button', { name: 'Start guided tour' }));
    await screen.findByRole('dialog');
    expect(screen.getByRole('button', { name: /Previous/ })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Skip' })).toBeEnabled();
    fireEvent.click(screen.getByRole('button', { name: 'Skip' }));
    await waitFor(() => expect(runtime.finishTutorialSession).toHaveBeenCalledOnce());
    expect(screen.getByRole('button', { name: 'Help and tutorial' })).toBeInTheDocument();
  });

  it('prepares each real view before advancing', async () => {
    renderHelp();
    fireEvent.click(screen.getByRole('button', { name: 'Help and tutorial' }));
    fireEvent.click(screen.getByRole('button', { name: 'Start guided tour' }));
    await screen.findByRole('dialog');
    fireEvent.click(screen.getByRole('button', { name: /Next/ }));
    await waitFor(() => expect(runtime.executeTutorialActions).toHaveBeenCalledWith(GUIDED_TOUR_STEPS[1].enter));
    expect(screen.getByRole('dialog')).toHaveTextContent('Your tutorial library');
  });
});
