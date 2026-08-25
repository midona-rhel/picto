import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { AuthSessionState } from '../../../shared/types/subscriptions';
import type { AuthSiteSnapshot } from '../../../shared/types/subscriptionsWorkspace';
import { AuthSiteDetail } from './AuthSiteDetail';

const idleSession: AuthSessionState = {
  site_category: null,
  status: 'idle',
  title: null,
  current_url: null,
  message: null,
};

const entry = {
  site: {
    id: 'fanbox',
    name: 'pixivFANBOX',
    domain: 'fanbox.cc',
    credential_owner_site_id: 'fanbox',
    credential_types: ['cookies'],
    supports_query: true,
    supports_account: false,
    auth_required_for_full_access: true,
    auth_strictly_required: false,
    example_query: '',
  },
  subscriptions: [],
  queryCount: 2,
  credential: null,
  health: null,
  issues: [],
} satisfies AuthSiteSnapshot;

describe('AuthSiteDetail', () => {
  it('keeps one stable account structure and starts the external login flow', () => {
    const onStartLogin = vi.fn(async () => undefined);
    render(
      <AuthSiteDetail
        entry={entry}
        session={idleSession}
        busy={false}
        message={null}
        onStartLogin={onStartLogin}
        onSaveManualOnlyFans={vi.fn(async () => undefined)}
        onCancelLogin={vi.fn(async () => undefined)}
        onRemoveCredential={vi.fn(async () => undefined)}
      />,
    );

    expect(screen.getByRole('main')).toHaveTextContent('Account');
    expect(screen.getByRole('main')).toHaveTextContent('Credential');
    expect(screen.getByRole('main')).toHaveTextContent('Subscriptions');
    expect(screen.getByRole('main')).toHaveTextContent('Last checked');
    expect(screen.queryByText('No login in progress')).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Log In' }));
    expect(onStartLogin).toHaveBeenCalledOnce();
  });

  it('shows remove without replacing the fixed detail rows', () => {
    const onRemoveCredential = vi.fn(async () => undefined);
    render(
      <AuthSiteDetail
        entry={{ ...entry, credential: { site_category: 'fanbox', credential_type: 'cookies', display_name: null, created_at: '2026-08-24' } }}
        session={idleSession}
        busy={false}
        message={null}
        onStartLogin={vi.fn(async () => undefined)}
        onSaveManualOnlyFans={vi.fn(async () => undefined)}
        onCancelLogin={vi.fn(async () => undefined)}
        onRemoveCredential={onRemoveCredential}
      />,
    );

    expect(screen.getByText('cookies')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Remove' }));
    expect(onRemoveCredential).toHaveBeenCalledOnce();
    expect(screen.getByText('Last checked')).toBeInTheDocument();
  });

  it('offers the complete manual fallback only for OnlyFans', async () => {
    const onSaveManualOnlyFans = vi.fn(async () => undefined);
    render(
      <AuthSiteDetail
        entry={{ ...entry, site: { ...entry.site, id: 'onlyfans', name: 'OnlyFans', domain: 'onlyfans.com', credential_owner_site_id: 'onlyfans' } }}
        session={idleSession}
        busy={false}
        message={null}
        onStartLogin={vi.fn(async () => undefined)}
        onSaveManualOnlyFans={onSaveManualOnlyFans}
        onCancelLogin={vi.fn(async () => undefined)}
        onRemoveCredential={vi.fn(async () => undefined)}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Enter session manually…' }));
    fireEvent.change(screen.getByLabelText('Cookie'), { target: { value: 'sess=session; auth_id=42' } });
    fireEvent.change(screen.getByLabelText('User-Agent'), { target: { value: 'Chrome' } });
    fireEvent.change(screen.getByLabelText('X-BC'), { target: { value: 'signature' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save Session' }));

    await waitFor(() => {
      expect(onSaveManualOnlyFans).toHaveBeenCalledWith({
        cookie: 'sess=session; auth_id=42',
        user_agent: 'Chrome',
        x_bc: 'signature',
      });
    });
  });
});
