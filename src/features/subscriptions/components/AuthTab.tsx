import { IconBolt, IconShieldLock } from '@tabler/icons-react';
import { CmSelect } from '../../../shared/ui/CmSelect/CmSelect';
import type {
  CredentialDomain,
  CredentialHealth,
  CredentialType,
  SubscriptionSiteInfo,
} from '../../../shared/types/subscriptions';
import {
  isBooruApiKeyCategory,
  isFuraffinityCategory,
  isPixivCategory,
  isTwitterCategory,
} from '../subscriptionUtils';
import { ActionButton } from './ActionButton';
import styles from '../SubscriptionsScreen.module.css';

export type AuthFormState = {
  siteCategory: string;
  credentialType: CredentialType;
  displayName: string;
  booruApiRaw: string;
  username: string;
  password: string;
  oauthToken: string;
  cookiesRaw: string;
};

function authStatusText(credential: CredentialDomain | null, health: CredentialHealth | null, site: SubscriptionSiteInfo | null): string {
  if (!credential) {
    return site?.auth_required_for_full_access ? 'Required for full access' : 'Optional';
  }
  if (!health) return 'Saved';
  switch (health.health_status) {
    case 'unauthorized':
    case 'expired':
      return 'Expired';
    case 'error':
      return 'Error';
    case 'healthy':
      return 'Healthy';
    default:
      return 'Saved';
  }
}

export function AuthTab({
  site,
  credential,
  health,
  authBusy,
  authMessage,
  form,
  onFormChange,
  onSave,
  onRemove,
  onPixivLogin,
}: {
  site: SubscriptionSiteInfo | null;
  credential: CredentialDomain | null;
  health: CredentialHealth | null;
  authBusy: boolean;
  authMessage: string | null;
  form: AuthFormState;
  onFormChange: (patch: Partial<AuthFormState>) => void;
  onSave: () => Promise<void>;
  onRemove: () => Promise<void>;
  onPixivLogin: () => Promise<void>;
}) {
  const siteId = site?.id ?? form.siteCategory;

  return (
    <div className={styles.section}>
      <div className={styles.authCard}>
        <div className={styles.authHeader}>
          <div className={styles.titleWrap}>
            <div className={styles.sectionTitle}>
              <IconShieldLock size={16} /> {site?.name ?? form.siteCategory}
            </div>
            <div className={styles.muted}>{authStatusText(credential, health, site)}</div>
          </div>
          {credential && (
            <ActionButton variant="danger" compact disabled={authBusy} onClick={() => { void onRemove(); }}>
              Remove Credential
            </ActionButton>
          )}
        </div>

        <div className={styles.queryStats}>
          <span className={styles.smallBadge}>Method {credential?.credential_type ?? form.credentialType}</span>
          {health?.health_status && <span className={styles.smallBadge}>Health {health.health_status}</span>}
        </div>
        {health?.last_error && (
          <div className={styles.errorBanner}>{health.last_error}</div>
        )}
        {authMessage && <div className={styles.helperCard}>{authMessage}</div>}

        {isPixivCategory(siteId) ? (
          <div className={styles.section}>
            <div className={styles.helperCard}>
              Pixiv uses the guided login flow. A popup window will handle authorization and save the refresh token automatically.
            </div>
            <div className={styles.inlineActions}>
              <ActionButton variant="primary" compact disabled={authBusy} onClick={() => { void onPixivLogin(); }}>
                <IconBolt size={14} />
                {authBusy ? 'Waiting for login…' : 'Log in with Pixiv'}
              </ActionButton>
            </div>
          </div>
        ) : (
          <div className={styles.section}>
            <label className={styles.label}>
              Display Name
              <input
                className={styles.field}
                value={form.displayName}
                onChange={(e) => onFormChange({ displayName: e.target.value })}
              />
            </label>

            {isBooruApiKeyCategory(siteId) ? (
              <label className={styles.label}>
                API Credential String
                <input
                  className={styles.field}
                  value={form.booruApiRaw}
                  placeholder="&api_key=YOUR_API_KEY&user_id=YOUR_USER_ID"
                  onChange={(e) => onFormChange({ booruApiRaw: e.target.value })}
                />
              </label>
            ) : isTwitterCategory(siteId) ? (
              <div className={styles.gridTwo}>
                <label className={styles.label}>
                  auth_token
                  <input className={styles.field} value={form.username} onChange={(e) => onFormChange({ username: e.target.value })} />
                </label>
                <label className={styles.label}>
                  ct0
                  <input className={styles.field} value={form.password} onChange={(e) => onFormChange({ password: e.target.value })} />
                </label>
              </div>
            ) : isFuraffinityCategory(siteId) ? (
              <label className={styles.label}>
                Cookies
                <textarea
                  className={styles.textarea}
                  value={form.cookiesRaw}
                  placeholder="a=VALUE&#10;b=VALUE"
                  onChange={(e) => onFormChange({ cookiesRaw: e.target.value })}
                />
              </label>
            ) : (
              <>
                <label className={styles.label}>
                  Credential Type
                  <CmSelect
                    value={form.credentialType}
                    options={[
                      { value: 'username_password', label: 'Username + Password' },
                      { value: 'oauth_token', label: 'OAuth Token' },
                      { value: 'cookies', label: 'Cookies' },
                      { value: 'api_key', label: 'API Key' },
                    ]}
                    onChange={(val) => onFormChange({ credentialType: val as CredentialType })}
                  />
                </label>
                {(form.credentialType === 'username_password' || form.credentialType === 'api_key') && (
                  <div className={styles.gridTwo}>
                    <label className={styles.label}>
                      {form.credentialType === 'api_key' ? 'Label' : 'Username'}
                      <input className={styles.field} value={form.username} onChange={(e) => onFormChange({ username: e.target.value })} />
                    </label>
                    <label className={styles.label}>
                      {form.credentialType === 'api_key' ? 'API Key' : 'Password'}
                      <input className={styles.field} type="password" value={form.password} onChange={(e) => onFormChange({ password: e.target.value })} />
                    </label>
                  </div>
                )}
                {form.credentialType === 'oauth_token' && (
                  <label className={styles.label}>
                    OAuth / Refresh Token
                    <input className={styles.field} value={form.oauthToken} onChange={(e) => onFormChange({ oauthToken: e.target.value })} />
                  </label>
                )}
                {form.credentialType === 'cookies' && (
                  <label className={styles.label}>
                    Cookies
                    <textarea className={styles.textarea} value={form.cookiesRaw} onChange={(e) => onFormChange({ cookiesRaw: e.target.value })} />
                  </label>
                )}
              </>
            )}

            <div className={styles.inlineActions}>
              <ActionButton variant="primary" compact disabled={authBusy} onClick={() => { void onSave(); }}>
                Save Credential
              </ActionButton>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
