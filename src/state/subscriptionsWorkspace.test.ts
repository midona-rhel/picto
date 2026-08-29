import { describe, expect, it } from 'vitest';
import {
  beginSubscriptionDetailRefresh,
  EMPTY_SUBSCRIPTION_DETAIL_STATE,
  type SubscriptionDetailState,
} from './subscriptionsWorkspace';

describe('subscription detail revalidation', () => {
  it('keeps the painted state unchanged for the selected subscription', () => {
    const current: SubscriptionDetailState = {
      ...EMPTY_SUBSCRIPTION_DETAIL_STATE,
      subscriptionId: '7',
      loading: false,
      issueTotalCount: 2,
    };

    expect(beginSubscriptionDetailRefresh(current, '7')).toBe(current);
  });

  it('shows loading when navigating to another subscription', () => {
    const current = { ...EMPTY_SUBSCRIPTION_DETAIL_STATE, subscriptionId: '7' };

    expect(beginSubscriptionDetailRefresh(current, '8')).toMatchObject({
      subscriptionId: '8',
      loading: true,
    });
  });
});
