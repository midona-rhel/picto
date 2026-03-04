import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  ActionIcon,
  Modal,
  Select,
  Stack,
  Text,
  TextInput,
  Textarea,
} from '@mantine/core';
import { IconCheck, IconKey, IconPlus, IconTrash, IconX } from '@tabler/icons-react';
import { api, getCurrentWindow } from '#desktop/api';
import { notifyError, notifySuccess } from '../../lib/notify';
import { CreateFlowModal, FlowsWorking, type FlowResultEntry } from '../FlowsWorking';
import { SubscriptionController } from '../../controllers/subscriptionController';
import type {
  CredentialDomain,
  CredentialHealth,
  CredentialType,
  SubscriptionSiteInfo,
} from '../../types/api';
import { TextButton } from '../ui/TextButton';
import styles from './SubscriptionsWindow.module.css';

type CredentialFormState = {
  siteCategory: string;
  credentialType: CredentialType;
  displayName: string;
  rule34Raw: string;
  username: string;
  password: string;
  oauthToken: string;
  cookiesRaw: string;
};

const CREDENTIAL_TYPE_OPTIONS = [
  { value: 'username_password', label: 'Username + Password / API Key' },
  { value: 'oauth_token', label: 'OAuth / Refresh Token' },
  { value: 'cookies', label: 'Cookies (key=value per line)' },
  { value: 'api_key', label: 'API Key' },
];

function isRule34Category(siteCategory: string): boolean {
  const normalized = siteCategory.trim().toLowerCase();
  return normalized === 'rule34' || normalized === 'rule34xxx' || normalized === 'rule34.xxx';
}

function parseRule34Credential(raw: string): { userId: string; apiKey: string } | null {
  const input = raw.trim();
  if (!input) return null;

  let query = input;
  const qIndex = input.indexOf('?');
  if (qIndex >= 0 && qIndex < input.length - 1) {
    query = input.slice(qIndex + 1);
  }
  if (query.startsWith('&') || query.startsWith('?')) {
    query = query.slice(1);
  }

  const params = new URLSearchParams(query);
  const apiKey = (params.get('api_key') ?? params.get('api-key') ?? '').trim();
  const userId = (params.get('user_id') ?? params.get('user-id') ?? '').trim();
  if (!apiKey || !userId) return null;
  return { userId, apiKey };
}

function parseCookies(raw: string): Record<string, string> {
  const out: Record<string, string> = {};
  const lines = raw.split('\n').map((s) => s.trim()).filter(Boolean);
  for (const line of lines) {
    const idx = line.indexOf('=');
    if (idx <= 0) continue;
    const key = line.slice(0, idx).trim();
    const value = line.slice(idx + 1).trim();
    if (!key || !value) continue;
    out[key] = value;
  }
  return out;
}

function validateCredentialForm(form: CredentialFormState): string | null {
  if (!form.siteCategory) return 'No site selected for credential.';
  const isRule34 = isRule34Category(form.siteCategory);
  if (isRule34 && form.credentialType !== 'api_key') {
    return 'rule34.xxx requires API key credentials (user-id + api-key).';
  }
  if (form.credentialType === 'username_password') {
    if (!form.username.trim() || !form.password.trim()) {
      return 'Username and password are required.';
    }
  }
  if (form.credentialType === 'api_key') {
    if (isRule34) {
      if (!parseRule34Credential(form.rule34Raw)) {
        return 'Paste rule34 credentials containing both api_key and user_id.';
      }
    } else if (!form.password.trim()) {
      return 'API key value is required.';
    }
  }
  if (form.credentialType === 'oauth_token') {
    if (!form.oauthToken.trim()) {
      return 'OAuth/refresh token is required.';
    }
  }
  if (form.credentialType === 'cookies') {
    const parsed = parseCookies(form.cookiesRaw);
    if (Object.keys(parsed).length === 0) {
      return 'At least one cookie entry (key=value) is required.';
    }
  }
  return null;
}

export function SubscriptionsWindow() {
  const [createModalOpen, setCreateModalOpen] = useState(false);
  const [flowRefreshToken, setFlowRefreshToken] = useState(0);
  const [flowLastResults, setFlowLastResults] = useState<Record<string, FlowResultEntry>>({});
  const [sites, setSites] = useState<SubscriptionSiteInfo[]>([]);
  const [credentials, setCredentials] = useState<CredentialDomain[]>([]);
  const [credentialHealth, setCredentialHealth] = useState<CredentialHealth[]>([]);
  const [credentialModalOpen, setCredentialModalOpen] = useState(false);
  const [savingCredential, setSavingCredential] = useState(false);
  const [credentialForm, setCredentialForm] = useState<CredentialFormState>({
    siteCategory: '',
    credentialType: 'username_password',
    displayName: '',
    rule34Raw: '',
    username: '',
    password: '',
    oauthToken: '',
    cookiesRaw: '',
  });

  const loadCredentialData = useCallback(async () => {
    try {
      const [siteCatalog, storedCreds, health] = await Promise.all([
        SubscriptionController.getSiteCatalog(),
        SubscriptionController.listCredentials(),
        SubscriptionController.listCredentialHealth(),
      ]);
      setSites(siteCatalog);
      setCredentials(storedCreds);
      setCredentialHealth(health);
    } catch (error) {
      notifyError(`Failed to load site/credential data: ${String(error)}`);
    }
  }, []);

  useEffect(() => {
    void loadCredentialData();
  }, [loadCredentialData]);

  const credentialMap = useMemo(() => {
    const map = new Map<string, CredentialDomain>();
    for (const row of credentials) map.set(row.site_category, row);
    return map;
  }, [credentials]);

  const healthMap = useMemo(() => {
    const map = new Map<string, CredentialHealth>();
    for (const row of credentialHealth) map.set(row.site_category, row);
    return map;
  }, [credentialHealth]);

  const authSites = useMemo(
    () => sites.filter((site) => site.auth_supported),
    [sites],
  );
  const isRule34Credential = useMemo(
    () => isRule34Category(credentialForm.siteCategory),
    [credentialForm.siteCategory],
  );
  const credentialTypeOptions = useMemo(
    () => (isRule34Credential
      ? [{ value: 'api_key', label: 'API Key (rule34 user-id + api-key)' }]
      : CREDENTIAL_TYPE_OPTIONS),
    [isRule34Credential],
  );

  const openCredentialModal = (site: SubscriptionSiteInfo) => {
    const existing = credentialMap.get(site.id);
    const defaultType: CredentialType = isRule34Category(site.id)
      ? 'api_key'
      : (existing?.credential_type ?? 'username_password');
    setCredentialForm({
      siteCategory: site.id,
      credentialType: defaultType,
      displayName: existing?.display_name ?? site.name,
      rule34Raw: '',
      username: '',
      password: '',
      oauthToken: '',
      cookiesRaw: '',
    });
    setCredentialModalOpen(true);
  };

  const saveCredential = async () => {
    if (!credentialForm.siteCategory) return;
    try {
      const validationError = validateCredentialForm(credentialForm);
      if (validationError) {
        notifyError(validationError, 'Invalid Credential');
        return;
      }
      setSavingCredential(true);
      const cookies = credentialForm.credentialType === 'cookies'
        ? parseCookies(credentialForm.cookiesRaw)
        : undefined;
      const parsedRule34 = isRule34Credential
        ? parseRule34Credential(credentialForm.rule34Raw)
        : null;
      await SubscriptionController.setCredential({
        siteCategory: credentialForm.siteCategory,
        credentialType: credentialForm.credentialType,
        displayName: credentialForm.displayName || null,
        username: isRule34Credential
          ? (parsedRule34?.userId ?? null)
          : (credentialForm.username || null),
        password: isRule34Credential
          ? (parsedRule34?.apiKey ?? null)
          : (credentialForm.password || null),
        oauthToken: credentialForm.oauthToken || null,
        cookies: cookies && Object.keys(cookies).length > 0 ? cookies : null,
      });
      notifySuccess('Credential saved to secure storage');
      setCredentialModalOpen(false);
      await loadCredentialData();
    } catch (error) {
      notifyError(`Failed to save credential: ${String(error)}`);
    } finally {
      setSavingCredential(false);
    }
  };

  const deleteCredential = async (siteCategory: string) => {
    try {
      await SubscriptionController.deleteCredential(siteCategory);
      notifySuccess('Credential removed');
      await loadCredentialData();
    } catch (error) {
      notifyError(`Failed to delete credential: ${String(error)}`);
    }
  };

  const closeWindow = () => {
    getCurrentWindow().close().catch(() => {});
  };
  const openDownloadSettings = () => {
    api.os.openSettingsWindow().catch(() => {});
  };

  return (
    <div className={styles.root}>
      <div className={styles.header}>
        <div className={styles.headerTitle}>Subscriptions</div>
        <div className={styles.headerActions}>
          <TextButton compact className={styles.noDrag} onClick={openDownloadSettings}>
            Download Settings
          </TextButton>
          <ActionIcon className={styles.closeButton} variant="subtle" color="gray" onClick={closeWindow}>
            <IconX size={14} />
          </ActionIcon>
        </div>
      </div>

      <div className={styles.body}>
        <section className={`${styles.section} ${styles.credentialsSection}`}>
          <div className={styles.sectionTitle}>Site Credentials</div>
          <div className={styles.sectionHelp}>
            Configure optional site login credentials in OS secure storage. Some sites require auth for full access.
          </div>
          <div className={`${styles.siteGrid} ${styles.cardsContainer}`}>
            {authSites.map((site) => {
              const cred = credentialMap.get(site.id);
              const health = healthMap.get(site.id);
              const missingAuth = site.auth_supported && site.auth_required_for_full_access && !cred;
              const healthStatus = site.auth_supported
                ? (missingAuth ? 'missing' : (health?.health_status ?? 'unknown'))
                : 'unknown';
              const healthLabel = healthStatus.replace('_', ' ');
              return (
                <div key={site.id} className={styles.siteRow}>
                  <div>
                    <div className={styles.siteName}>{site.name}</div>
                    <div className={styles.siteDomain}>{site.domain}</div>
                  </div>
                  <div className={styles.capabilityList}>
                    <span className={site.supports_query ? styles.capabilityOn : styles.capabilityOff}>
                      {site.supports_query ? <IconCheck size={11} /> : <IconX size={11} />}
                      Query
                    </span>
                    <span className={site.supports_account ? styles.capabilityOn : styles.capabilityOff}>
                      {site.supports_account ? <IconCheck size={11} /> : <IconX size={11} />}
                      Account
                    </span>
                    <span className={site.auth_supported ? styles.capabilityOn : styles.capabilityOff}>
                      {site.auth_supported ? <IconCheck size={11} /> : <IconX size={11} />}
                      Auth
                    </span>
                  </div>
                  <Text size="xs" c={cred ? 'green' : 'dimmed'}>
                    {site.auth_supported
                      ? (cred ? `Saved (${cred.credential_type})` : 'No credential')
                      : 'Auth not supported'}
                  </Text>
                  <Text
                    size="xs"
                    className={
                      healthStatus === 'valid'
                        ? styles.healthGood
                        : healthStatus === 'unauthorized' || healthStatus === 'expired' || healthStatus === 'missing'
                          ? styles.healthWarn
                          : healthStatus === 'error'
                            ? styles.healthBad
                            : styles.healthUnknown
                    }
                  >
                    {site.auth_supported ? healthLabel : 'n/a'}
                  </Text>
                  <div style={{ display: 'inline-flex', gap: 6 }}>
                    {site.auth_supported && (
                      <ActionIcon variant="subtle" color="gray" onClick={() => openCredentialModal(site)}>
                        <IconKey size={14} />
                      </ActionIcon>
                    )}
                    {site.auth_supported && cred && (
                      <ActionIcon variant="subtle" color="red" onClick={() => deleteCredential(site.id)}>
                        <IconTrash size={14} />
                      </ActionIcon>
                    )}
                  </div>
                  {site.auth_supported && health?.last_error && (
                    <Text size="xs" c="dimmed" style={{ gridColumn: '1 / -1' }}>
                      Last auth error: {health.last_error}
                    </Text>
                  )}
                </div>
              );
            })}
            {authSites.length === 0 && (
              <Text size="xs" c="dimmed">
                No credential-capable sites are currently enabled.
              </Text>
            )}
          </div>
        </section>

        <section className={`${styles.section} ${styles.subscriptionsSection}`}>
          <div className={styles.sectionHeaderRow}>
            <div className={styles.sectionTitle}>Subscriptions</div>
            <TextButton compact onClick={() => setCreateModalOpen(true)}>
              <IconPlus size={12} />
              New
            </TextButton>
          </div>
          <div className={`${styles.flowsWrap} ${styles.cardsContainer}`}>
            <FlowsWorking
              flowId={null}
              lastResults={flowLastResults}
              onLastResultsChange={setFlowLastResults}
              showHeader={false}
              layoutMode="list"
              refreshToken={flowRefreshToken}
            />
          </div>
        </section>
      </div>

      <CreateFlowModal
        opened={createModalOpen}
        onClose={() => setCreateModalOpen(false)}
        onCreated={() => setFlowRefreshToken((v) => v + 1)}
      />

      <Modal
        opened={credentialModalOpen}
        onClose={() => setCredentialModalOpen(false)}
        title="Configure Site Credential"
        size="md"
      >
        <Stack gap="sm">
          {!isRule34Credential && (
            <Select
              label="Credential Type"
              value={credentialForm.credentialType}
              data={credentialTypeOptions}
              onChange={(value) => {
                if (!value) return;
                setCredentialForm((prev) => ({ ...prev, credentialType: value as CredentialType }));
              }}
              allowDeselect={false}
            />
          )}
          {isRule34Credential && (
            <div className={styles.rule34Hint}>
              <Text size="xs" c="dimmed">
                rule34.xxx requires both <code>user-id</code> and <code>api-key</code>.
              </Text>
              <Text size="xs" c="dimmed">
                Get both values from: <code>https://api.rule34.xxx</code>
              </Text>
              <Text size="xs" c="dimmed">
                gallery-dl/API format: <code>&amp;api_key=&lt;YOUR_API_KEY&gt;&amp;user_id=&lt;YOUR_USER_ID&gt;</code>
              </Text>
            </div>
          )}
          <TextInput
            label="Display Name"
            value={credentialForm.displayName}
            onChange={(e) => setCredentialForm((prev) => ({ ...prev, displayName: e.currentTarget.value }))}
          />
          {isRule34Credential && (
            <TextInput
              label="Rule34 API Credential String"
              placeholder="&api_key=<YOUR_API_KEY>&user_id=<YOUR_USER_ID>"
              value={credentialForm.rule34Raw}
              onChange={(e) => setCredentialForm((prev) => ({ ...prev, rule34Raw: e.currentTarget.value }))}
            />
          )}
          {!isRule34Credential && (credentialForm.credentialType === 'username_password' || credentialForm.credentialType === 'api_key') && (
            <>
              <TextInput
                label={credentialForm.credentialType === 'api_key'
                  ? 'API Key Label (optional username field)'
                  : 'Username / Email'}
                value={credentialForm.username}
                onChange={(e) => setCredentialForm((prev) => ({ ...prev, username: e.currentTarget.value }))}
              />
              <TextInput
                label={credentialForm.credentialType === 'api_key'
                  ? 'API Key / Password'
                  : 'Password / API Key'}
                type="password"
                value={credentialForm.password}
                onChange={(e) => setCredentialForm((prev) => ({ ...prev, password: e.currentTarget.value }))}
              />
            </>
          )}
          {credentialForm.credentialType === 'oauth_token' && (
            <TextInput
              label="Refresh/OAuth Token"
              value={credentialForm.oauthToken}
              onChange={(e) => setCredentialForm((prev) => ({ ...prev, oauthToken: e.currentTarget.value }))}
            />
          )}
          {credentialForm.credentialType === 'cookies' && (
            <Textarea
              label="Cookies (one key=value per line)"
              minRows={5}
              value={credentialForm.cookiesRaw}
              onChange={(e) => setCredentialForm((prev) => ({ ...prev, cookiesRaw: e.currentTarget.value }))}
            />
          )}
          <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
            <TextButton compact onClick={() => setCredentialModalOpen(false)} disabled={savingCredential}>
              Cancel
            </TextButton>
            <TextButton compact onClick={saveCredential} disabled={savingCredential}>
              Save
            </TextButton>
          </div>
        </Stack>
      </Modal>
    </div>
  );
}
